use std::{
    sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
    time::{Duration, Instant},
};

use clash_verge_logging::{Type, logging};
use parking_lot::Mutex;
use tauri_plugin_clash_verge_sysinfo::is_current_app_handle_admin;
use tauri_plugin_mihomo::models::TunConfig;

use crate::{
    config::Config,
    constants::timing,
    core::{
        handle::Handle,
        notification::EXIT_REFUSAL_STATUS,
        service::{
            ElevationPending, SERVICE_MANAGER, ServiceBusy, ServiceProbe, ServiceRegistration, ServiceStatus,
            elevation_in_flight, probe_service, service_registration, start_registered_service,
        },
        validate::ValidationOutcome,
    },
    process::AsyncHandler,
    utils::network::{NetworkManager, ProxyType},
};

static SUPPRESSED: AtomicBool = AtomicBool::new(false);
static START_FAILED: AtomicBool = AtomicBool::new(false);
static SETUP_RUNNING: AtomicBool = AtomicBool::new(false);
static WATCHDOG_GENERATION: AtomicU64 = AtomicU64::new(0);
static VERIFY_GENERATION: AtomicU64 = AtomicU64::new(0);
static START_ATTEMPTS: AtomicU32 = AtomicU32::new(0);
static RETRY_PENDING: AtomicBool = AtomicBool::new(false);
static WATCH_ANCHOR: Mutex<Option<String>> = Mutex::new(None);
static LAST_FAILURE: Mutex<Option<&'static str>> = Mutex::new(None);
static TRAFFIC_PROBE_RUNNING: AtomicBool = AtomicBool::new(false);
static NO_TRAFFIC_NOTICED: AtomicBool = AtomicBool::new(false);
static TRAFFIC_PROBE_EPOCH: AtomicU64 = AtomicU64::new(0);
static LAST_REARM_AT: Mutex<Option<Instant>> = Mutex::new(None);
static RECREATE_RUNNING: AtomicBool = AtomicBool::new(false);
static BRING_BACK_RUNNING: AtomicBool = AtomicBool::new(false);
static REARM_BACKOFF: AtomicU32 = AtomicU32::new(0);
static LAST_NOTICED: Mutex<Option<&'static str>> = Mutex::new(None);
static CAPABILITY: Mutex<Option<(Instant, bool, bool)>> = Mutex::new(None);
const READBACK_DOWN_STRIKES: u32 = 2;
const TRAFFIC_RECHECK_ROUNDS: u32 = 10;
const TRAFFIC_RECHECK_MAX_ROUNDS: u32 = 120;
const REARM_BACKOFF_STEPS: u32 = 4;
const TUN_FAILURE_MARKERS: &[&str] = &["start tun listening error", "configure tun interface"];

const FAILURE_START: &str = "startFailed";
const FAILURE_ADAPTER_BUSY: &str = "adapterBusy";
const FAILURE_NO_RIGHTS: &str = "noRights";
const FAILURE_NO_TRAFFIC: &str = "noTraffic";
const FAILURE_SETUP: &str = "setupFailed";
const FAILURE_RIGHTS_DECLINED: &str = "rightsDeclined";
const FAILURE_SERVICE_SILENT: &str = "serviceSilent";

const TUN_START_ATTEMPTS: u32 = 3;
const TUN_RETRY_DELAY: Duration = Duration::from_secs(5);
const TUN_READ_TIMEOUT: Duration = Duration::from_secs(3);
const TUN_PATCH_TIMEOUT: Duration = Duration::from_secs(10);
const TUN_TAKEDOWN_TIMEOUT: Duration = Duration::from_secs(5);
const TUN_REARM_COOLDOWN: Duration = Duration::from_secs(300);
const TUN_BRING_BACK_ATTEMPTS: u32 = 3;
const TUN_BRING_BACK_RETRY_DELAY: Duration = Duration::from_secs(2);
const TRAFFIC_PROBE_URL: &str = "https://cp.cloudflare.com/generate_204";
const TRAFFIC_PROBE_TIMEOUT_SECS: u64 = 8;
const TRAFFIC_PROBE_DELAY: Duration = Duration::from_secs(5);
const TRAFFIC_PROBE_RETRY_DELAY: Duration = Duration::from_secs(3);
const CAPABILITY_FRESH_FOR: Duration = Duration::from_secs(60);

pub fn is_suppressed() -> bool {
    SUPPRESSED.load(Ordering::Acquire)
}

async fn wait_before_the_next_bring_back() -> bool {
    tokio::time::sleep(TUN_BRING_BACK_RETRY_DELAY).await;
    desired().await && !is_suppressed()
}

async fn bring_tun_back(reason: &str) {
    if BRING_BACK_RUNNING.swap(true, Ordering::AcqRel) {
        logging!(info, Type::Core, "TUN is already being brought back; skipping this one");
        return;
    }
    scopeguard::defer! {
        BRING_BACK_RUNNING.store(false, Ordering::Release);
    }

    logging!(info, Type::Core, "bringing TUN back: {}", reason);
    let mut last;
    let mut refused = 0_u32;
    loop {
        let anchor = log_anchor().await;
        match crate::core::CoreManager::global().update_config_forced().await {
            Ok(Ok(_)) => {
                spawn_start_verification(anchor);
                Handle::refresh_verge();
                let _ = crate::core::tray::Tray::global().update_menu().await;
                return;
            }
            Ok(Err(outcome @ ValidationOutcome::Skipped { .. })) => {
                logging!(info, Type::Core, "not bringing TUN back right now: {}", outcome);
                return;
            }
            Ok(Err(outcome)) => last = outcome.to_string(),
            Err(e) => last = format!("{e}"),
        }
        refused += 1;
        if refused >= TUN_BRING_BACK_ATTEMPTS {
            break;
        }
        logging!(
            info,
            Type::Core,
            "could not bring TUN back yet ({} of {}): {}",
            refused,
            TUN_BRING_BACK_ATTEMPTS,
            last
        );
        if !wait_before_the_next_bring_back().await {
            return;
        }
    }
    logging!(warn, Type::Core, "could not bring TUN back: {}", last);
    hold_tun_down(
        "the running config would not take the TUN device",
        FAILURE_START,
        "tun::start_failed",
    );
    drop_tun_from_the_running_config("a failed attempt to bring TUN back");
}

/// clod:tun-before-service — служба появилась (поставлена из запроса или
/// кнопкой, ожила остановленная), подавление снято, а принятый конфиг был
/// собран без TUN, пока службы не было. Ядро под службой стартует с принятого
/// файла — то есть без туннеля, хотя тумблер говорит «включён». Пересобрать,
/// если TUN хочется и в принятом его нет; иначе трафик шёл бы мимо туннеля до
/// первой случайной пересборки.
/// То же отдельной задачей: из путей передачи ядра службе, чьи будущие иначе
/// замыкались бы сами на себя (перезапуск ядра снова заводит наблюдателя
/// передачи). Передача случается и посреди `patch_verge` (тумблер TUN, кнопка
/// починки службы): сначала дождаться этой правки, иначе пересборка пошла бы
/// из ещё не зафиксированных настроек и заняла бы дверь у самого `patch_verge`.
pub fn spawn_bringing_tun_back_if_the_config_lacks_it(reason: &'static str) {
    AsyncHandler::spawn(move || {
        Box::pin(async move {
            drop(crate::feat::patch_verge_lock().lock().await);
            bring_tun_back_if_the_config_lacks_it(reason).await;
        }) as std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>
    });
}

pub async fn bring_tun_back_if_the_config_lacks_it(reason: &str) {
    // По зафиксированным настройкам, не по черновику: идущая правка тумблера
    // сама пересоберёт конфиг, когда её зафиксируют.
    let wanted = Config::verge().await.data_arc().enable_tun_mode.unwrap_or(false);
    if !is_claimed(wanted, is_suppressed()) {
        return;
    }
    if accepted_has_tun().await {
        return;
    }
    bring_tun_back(reason).await;
}

/// Есть ли туннель в принятом конфиге — в том, на котором работает ядро.
async fn accepted_has_tun() -> bool {
    Config::runtime()
        .await
        .data_arc()
        .config
        .as_ref()
        .and_then(|config| config.get("tun"))
        .and_then(|tun| tun.get("enable"))
        .and_then(serde_yaml_ng::Value::as_bool)
        .unwrap_or(false)
}

/// Что сделать с туннелем у только что поднятого ядра.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NewCoreTun {
    /// Нужен и есть в конфиге — проверить по факту.
    Verify,
    /// Нужен, а конфиг собран без него (выкинут после провала в этом сеансе,
    /// подавление сняли «Перезапуском ядра» или заменой ядра; ядро уехало к
    /// службе) — вернуть в конфиг; возврат сам и проверит.
    BringBack,
    /// Не нужен — снять, если ядро его держит.
    TakeDown,
}

const fn new_core_tun(claimed: bool, accepted_has_tun: bool) -> NewCoreTun {
    match (claimed, accepted_has_tun) {
        (true, true) => NewCoreTun::Verify,
        (true, false) => NewCoreTun::BringBack,
        (false, _) => NewCoreTun::TakeDown,
    }
}

/// Поднялся новый процесс ядра — любым путём, в том числе перезапуском самой
/// службой. clod:tun-ready — туннель, который нужен и есть в его конфиге,
/// проверяем по факту: если ядро не смогло поднять устройство, честно гасим
/// TUN и говорим об этом. Нужный, но собранный без туннеля, возвращаем. Ненужный
/// — снимаем.
pub async fn follow_up_on_a_new_core() {
    match new_core_tun(claimed().await, accepted_has_tun().await) {
        NewCoreTun::Verify => spawn_start_verification(log_anchor().await),
        NewCoreTun::BringBack => {
            spawn_bringing_tun_back_if_the_config_lacks_it("the core came up without the TUN it should have");
        }
        NewCoreTun::TakeDown => {
            AsyncHandler::spawn(|| async { enforce_undesired_off().await });
        }
    }
}

pub fn suppress(reason: &str) {
    if !SUPPRESSED.swap(true, Ordering::AcqRel) {
        logging!(warn, Type::Core, "TUN suppressed for this session: {}", reason);
    }
}

fn forget_last_notice() {
    *LAST_NOTICED.lock() = None;
}

pub fn clear_suppression() {
    clear_failure();
    forget_last_notice();
    SUPPRESSED.store(false, Ordering::Release);
    START_FAILED.store(false, Ordering::Release);
    START_ATTEMPTS.store(0, Ordering::Release);
    NO_TRAFFIC_NOTICED.store(false, Ordering::Release);
}

pub async fn desired() -> bool {
    Config::verge().await.latest_arc().enable_tun_mode.unwrap_or(false)
}

const fn is_claimed(desired: bool, suppressed: bool) -> bool {
    desired && !suppressed
}

async fn claimed() -> bool {
    is_claimed(desired().await, is_suppressed())
}

pub fn is_active_with(desired: bool) -> bool {
    is_claimed(desired, is_suppressed())
}

pub fn is_app_elevated() -> bool {
    is_current_app_handle_admin(Handle::app_handle())
}

pub async fn is_capable() -> bool {
    is_app_elevated() || matches!(probe_service().await, ServiceProbe::Ready(_))
}

pub async fn capability_and_repair() -> (bool, bool) {
    if let Some((asked_at, capable, needs_repair)) = *CAPABILITY.lock()
        && asked_at.elapsed() < CAPABILITY_FRESH_FOR
    {
        return (capable, needs_repair);
    }
    let (capable, needs_repair) = capability_and_repair_now().await;
    *CAPABILITY.lock() = Some((Instant::now(), capable, needs_repair));
    (capable, needs_repair)
}

pub fn forget_capability() {
    *CAPABILITY.lock() = None;
}

async fn capability_and_repair_now() -> (bool, bool) {
    let elevated = is_app_elevated();
    capability_of(&probe_service().await, elevated)
}

/// (умеет ли TUN, нужна ли починка службы) по одному опросу службы.
const fn capability_of(probe: &ServiceProbe, elevated: bool) -> (bool, bool) {
    match probe {
        ServiceProbe::Ready(_) => (true, false),
        ServiceProbe::Outdated => (elevated, true),
        ServiceProbe::Silent(_) => (elevated, false),
    }
}

pub async fn service_needs_repair() -> bool {
    probe_service().await == ServiceProbe::Outdated
}

const TUN_ADAPTER_BUSY_MARKERS: &[&str] = &["already exists", "file exists", "resource busy", "in use", "wintun"];

pub fn line_reports_adapter_busy(line: &str) -> bool {
    let lowered = line.to_ascii_lowercase();
    TUN_ADAPTER_BUSY_MARKERS.iter().any(|marker| lowered.contains(marker))
}

const TUN_NO_RIGHTS_MARKERS: &[&str] = &["operation not permitted", "access is denied", "permission denied"];

const SETUP_RIGHTS_DECLINED_MARKERS: &[&str] = &[
    "prompt was dismissed",
    "cancelled or declined",
    "rights were not granted",
];

fn line_reports_no_rights(line: &str) -> bool {
    let lowered = line.to_ascii_lowercase();
    TUN_NO_RIGHTS_MARKERS.iter().any(|marker| lowered.contains(marker))
}

fn setup_rights_declined(detail: &str) -> bool {
    let lowered = detail.to_ascii_lowercase();
    SETUP_RIGHTS_DECLINED_MARKERS
        .iter()
        .any(|marker| lowered.contains(marker))
}

fn set_failure(tag: &'static str) {
    *LAST_FAILURE.lock() = Some(tag);
}

fn clear_failure() {
    *LAST_FAILURE.lock() = None;
}

fn clear_failure_tag(tag: &'static str) {
    let mut failure = LAST_FAILURE.lock();
    if *failure == Some(tag) {
        *failure = None;
    }
}

pub fn last_failure() -> Option<&'static str> {
    *LAST_FAILURE.lock()
}

pub fn line_reports_tun_failure(line: &str) -> bool {
    let lowered = line.to_ascii_lowercase();
    TUN_FAILURE_MARKERS.iter().any(|marker| lowered.contains(marker))
}

pub fn report_start_failure(detail: &str) {
    if START_FAILED.swap(true, Ordering::AcqRel) {
        return;
    }

    let attempt = START_ATTEMPTS.fetch_add(1, Ordering::AcqRel) + 1;
    if should_retry(attempt) {
        logging!(
            warn,
            Type::Core,
            "TUN failed to start ({} of {}): {}",
            attempt,
            TUN_START_ATTEMPTS,
            detail
        );
        schedule_retry();
        return;
    }

    give_up_on_tun(detail);
}

const fn should_retry(attempt: u32) -> bool {
    attempt < TUN_START_ATTEMPTS
}

fn announce_once(failure: &'static str, event: &str) {
    let said_before = {
        let mut last = LAST_NOTICED.lock();
        let repeat = *last == Some(failure);
        *last = Some(failure);
        repeat
    };
    if !said_before {
        Handle::notice_message(event, "");
    }
}

fn hold_tun_down(reason: &str, failure: &'static str, event: &str) {
    suppress(reason);
    set_failure(failure);
    announce_once(failure, event);
}

fn give_up_on_tun(detail: &str) {
    logging!(error, Type::Core, "TUN failed to start: {}", detail);
    let (event, failure) = if line_reports_adapter_busy(detail) {
        ("tun::adapter_busy", FAILURE_ADAPTER_BUSY)
    } else if line_reports_no_rights(detail) {
        ("tun::no_rights", FAILURE_NO_RIGHTS)
    } else {
        ("tun::start_failed", FAILURE_START)
    };
    let service_refused_this_config =
        failure == FAILURE_NO_RIGHTS && crate::core::service::bundle_rejection().is_some();
    if service_refused_this_config {
        suppress("core failed to start the TUN device");
        set_failure(failure);
        AsyncHandler::spawn(move || async move {
            if crate::core::service::bundle_rejection_for_the_running_config()
                .await
                .is_none()
            {
                announce_once(failure, event);
            }
        });
    } else {
        hold_tun_down("core failed to start the TUN device", failure, event);
    }

    drop_tun_from_the_running_config(detail);
}

fn drop_tun_from_the_running_config(reason: &str) {
    let reason = reason.to_owned();
    AsyncHandler::spawn(move || async move {
        if let Err(e) = crate::core::CoreManager::global().update_config_checked().await {
            logging!(
                warn,
                Type::Core,
                "failed to drop TUN from the running config after {}: {}",
                reason,
                e
            );
        }
        Handle::refresh_verge();
        let _ = crate::core::tray::Tray::global().update_menu().await;
    });
}

fn schedule_retry() {
    if RETRY_PENDING.swap(true, Ordering::AcqRel) {
        return;
    }
    AsyncHandler::spawn(|| async {
        tokio::time::sleep(TUN_RETRY_DELAY).await;
        RETRY_PENDING.store(false, Ordering::Release);
        START_FAILED.store(false, Ordering::Release);
        if !claimed().await {
            return;
        }
        recreate_tun_device().await;
    });
}

fn core_is_running() -> bool {
    !matches!(
        *crate::core::CoreManager::global().get_running_mode(),
        crate::core::manager::RunningMode::NotRunning
    )
}

enum SwitchFailure {
    Refused(std::string::String),
    Silent,
}

impl SwitchFailure {
    fn detail(&self) -> std::string::String {
        match self {
            Self::Refused(e) => format!("the core refused the request: {e}"),
            Self::Silent => std::string::String::from("the core did not answer in time"),
        }
    }
}

async fn switch_tun_device(enable: bool) -> Result<(), SwitchFailure> {
    let patch = serde_json::json!({ "tun": { "enable": enable } });
    let budget = if enable {
        TUN_PATCH_TIMEOUT
    } else {
        TUN_TAKEDOWN_TIMEOUT
    };
    let core = crate::core::handle::Handle::mihomo();
    let outcome = tokio::time::timeout(budget, core.patch_base_config(&patch)).await;
    match outcome {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => Err(SwitchFailure::Refused(e.to_string())),
        Err(_) => Err(SwitchFailure::Silent),
    }
}

async fn recreate_tun_device() {
    if RECREATE_RUNNING.swap(true, Ordering::AcqRel) {
        logging!(
            info,
            Type::Core,
            "a TUN device re-creation is already running; skipping this one"
        );
        return;
    }
    scopeguard::defer! {
        RECREATE_RUNNING.store(false, Ordering::Release);
    }

    let anchor = log_anchor().await;

    for (step, enable) in [("off", false), ("on", true)] {
        let Err(failure) = switch_tun_device(enable).await else {
            continue;
        };
        let detail = failure.detail();
        logging!(warn, Type::Core, "could not switch the TUN device {}: {}", step, detail);
        if core_is_running() {
            report_start_failure(&detail);
        } else {
            logging!(
                info,
                Type::Core,
                "the core is not running; this does not count against the TUN start budget"
            );
        }
        return;
    }

    logging!(info, Type::Core, "the TUN device was re-created");
    spawn_start_verification(anchor);
}

/// Вернуть туннель, снятый уборкой отменённого выхода.
///
/// Условие то же, по которому уборка его гасила: тумблер включён и подъём
/// не подавлен. Проверка после подъёма — та же, что после старта ядра.
pub async fn bring_back_after_a_cancelled_exit() {
    if !claimed().await {
        return;
    }
    let anchor = log_anchor().await;
    match switch_tun_device(true).await {
        Ok(()) => {
            logging!(info, Type::Core, "TUN brought back after the cancelled exit");
            spawn_start_verification(anchor);
        }
        Err(failure) => logging!(
            warn,
            Type::Core,
            "could not bring TUN back after the cancelled exit: {}",
            failure.detail()
        ),
    }
}

pub async fn rearm_after_wake() {
    if !desired().await {
        return;
    }
    let was_suppressed = is_suppressed();
    if was_suppressed && !a_new_network_could_help() && !rights_have_arrived() {
        logging!(
            info,
            Type::Core,
            "TUN stays down after the wake-up: its last failure ({}) is not something a new environment fixes",
            last_failure().unwrap_or("unknown")
        );
        return;
    }
    if was_suppressed {
        clear_suppression();
    }
    START_ATTEMPTS.store(0, Ordering::Release);
    START_FAILED.store(false, Ordering::Release);
    stamp_rearm();
    if was_suppressed {
        bring_tun_back("the machine woke up into a new environment").await;
        return;
    }
    // clod:Р30П-11 — рабочий туннель после сна не трогаем: пересоздание рвёт
    // его соединения и на время без маршрутов пускает трафик мимо. Пересоздаём
    // только то, что по факту не работает.
    match health_after_wake().await {
        Health::Works => logging!(
            info,
            Type::Core,
            "the TUN device works after the wake-up; left as it is"
        ),
        Health::Unknown(why) => logging!(
            info,
            Type::Core,
            "the TUN device could not be checked after the wake-up ({}); left as it is",
            why
        ),
        Health::Broken(why) => {
            if !claimed().await {
                return;
            }
            logging!(
                warn,
                Type::Core,
                "the TUN device does not work after the wake-up ({}); re-creating it",
                why
            );
            recreate_tun_device().await;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Health {
    Works,
    Broken(&'static str),
    Unknown(&'static str),
}

/// Состояние туннеля по фактам: что о нём говорит ядро, ведёт ли в него
/// система путь наружу и идёт ли через него трафик.
async fn health_after_wake() -> Health {
    let Some(tun) = read_tun_config().await else {
        return Health::Unknown("the core did not answer");
    };
    if !tun.enable {
        return Health::Broken("the core reports the device is not up");
    }
    if system_route(&tun) == Route::Bypass {
        return Health::Broken("the system route to the internet bypasses it");
    }
    match traffic_through_the_tunnel().await {
        Traffic::Flows => {
            traffic_seen_flowing();
            Health::Works
        }
        Traffic::Stuck => Health::Broken("it passes no traffic"),
        Traffic::Unknown => Health::Unknown("the proxy itself does not answer"),
    }
}

/// Куда проверяется маршрут. Пакет туда не уходит: UDP-сокет только
/// спрашивает систему, каким путём и с какого адреса она бы его отправила.
/// Порт — как у обычного трафика, не 53: DNS у туннеля на Linux идёт
/// отдельным правилом и показал бы туннель там, где остальное идёт мимо.
const ROUTE_CHECK_TARGET: std::net::Ipv4Addr = std::net::Ipv4Addr::new(1, 1, 1, 1);
const ROUTE_CHECK_PORT: u16 = 443;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Route {
    Tunnel,
    Bypass,
    Unknown,
}

fn system_route(tun: &TunConfig) -> Route {
    if !route_check_applies(tun) {
        return Route::Unknown;
    }
    source_towards(ROUTE_CHECK_TARGET).map_or(Route::Unknown, |source| {
        route_by_source(source, tun.inet4_address.as_deref().unwrap_or_default())
    })
}

/// Путь в туннель система выбирает с адресом самого туннеля — так на всех
/// трёх ОС. Любой другой адрес значит, что путь наружу идёт мимо него.
fn route_by_source(source: std::net::IpAddr, tunnel_addresses: &[String]) -> Route {
    let prefixes: Vec<_> = tunnel_addresses
        .iter()
        .filter_map(|text| crate::enhance::LanPrefix::parse(text))
        .collect();
    if prefixes.is_empty() || source.is_unspecified() || source.is_loopback() {
        return Route::Unknown;
    }
    if prefixes.iter().any(|prefix| prefix.contains(source)) {
        Route::Tunnel
    } else {
        Route::Bypass
    }
}

/// Проверка маршрута честна, только когда туннель забирает весь путь наружу
/// и наш процесс в нём. Сужения адресов, наборы исключений, правила по
/// пользователям, интерфейсам и портам, auto-redirect — ответа «по маршруту» нет.
fn route_check_applies(tun: &TunConfig) -> bool {
    let target = std::net::IpAddr::V4(ROUTE_CHECK_TARGET);
    let excluded = [&tun.route_exclude_address, &tun.inet4_route_exclude_address]
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|text| crate::enhance::LanPrefix::parse(text))
        .any(|prefix| prefix.contains(target));
    let none = |list: &Option<Vec<String>>| list.as_ref().is_none_or(Vec::is_empty);
    let no_uids = |list: &Option<Vec<u32>>| list.as_ref().is_none_or(Vec::is_empty);
    let no_ports = |list: &Option<Vec<u16>>| list.as_ref().is_none_or(Vec::is_empty);
    tun.auto_route
        && tun.auto_redirect != Some(true)
        && !excluded
        && none(&tun.route_address)
        && none(&tun.route_address_set)
        && none(&tun.inet4_route_address)
        && none(&tun.route_exclude_address_set)
        && none(&tun.include_interface)
        && none(&tun.exclude_interface)
        && no_uids(&tun.include_uid)
        && none(&tun.include_uid_range)
        && no_uids(&tun.exclude_uid)
        && none(&tun.exclude_uid_range)
        && no_ports(&tun.exclude_src_port)
        && none(&tun.exclude_src_port_range)
        && no_ports(&tun.exclude_dst_port)
        && none(&tun.exclude_dst_port_range)
}

fn source_towards(target: std::net::Ipv4Addr) -> Option<std::net::IpAddr> {
    let socket = std::net::UdpSocket::bind((std::net::Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    socket.connect((target, ROUTE_CHECK_PORT)).ok()?;
    socket.local_addr().ok().map(|local| local.ip())
}

async fn probe_traffic(proxy_type: ProxyType) -> bool {
    let client = match NetworkManager::new()
        .create_request(proxy_type, Some(TRAFFIC_PROBE_TIMEOUT_SECS), None, false)
        .await
    {
        Ok(client) => client,
        Err(_) => return false,
    };
    match client.get(TRAFFIC_PROBE_URL).send().await {
        Ok(response) => response.status().as_u16() == 204,
        Err(_) => false,
    }
}

/// Причину «трафик не идёт» могли только что убрать (починили брандмауэр).
/// Идущая проба могла начать запросы до починки — её вердикт «не идёт» уже не
/// публикуем; если трафика и правда нет, это скажет следующая проба.
#[cfg(windows)]
pub fn recheck_traffic() {
    TRAFFIC_PROBE_EPOCH.fetch_add(1, Ordering::AcqRel);
    spawn_traffic_probe();
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Traffic {
    Flows,
    Stuck,
    Unknown,
}

/// Идёт ли трафик машины через туннель. «Не знаю» — когда не отвечает сам
/// прокси (сеть или узел, а не туннель) или туннель перестали хотеть.
async fn traffic_through_the_tunnel() -> Traffic {
    if !probe_traffic(ProxyType::Localhost).await {
        return Traffic::Unknown;
    }
    if probe_traffic(ProxyType::None).await {
        return Traffic::Flows;
    }
    tokio::time::sleep(TRAFFIC_PROBE_RETRY_DELAY).await;
    if !claimed().await {
        return Traffic::Unknown;
    }
    if probe_traffic(ProxyType::None).await {
        return Traffic::Flows;
    }
    if !probe_traffic(ProxyType::Localhost).await {
        return Traffic::Unknown;
    }
    Traffic::Stuck
}

fn traffic_seen_flowing() {
    NO_TRAFFIC_NOTICED.store(false, Ordering::Release);
    clear_failure_tag(FAILURE_NO_TRAFFIC);
}

fn spawn_traffic_probe() {
    if TRAFFIC_PROBE_RUNNING.swap(true, Ordering::AcqRel) {
        return;
    }
    AsyncHandler::spawn(|| async {
        scopeguard::defer! {
            TRAFFIC_PROBE_RUNNING.store(false, Ordering::Release);
        }
        let epoch = TRAFFIC_PROBE_EPOCH.load(Ordering::Acquire);
        tokio::time::sleep(TRAFFIC_PROBE_DELAY).await;
        if !claimed().await {
            return;
        }
        match traffic_through_the_tunnel().await {
            Traffic::Unknown => return,
            Traffic::Flows => {
                traffic_seen_flowing();
                return;
            }
            Traffic::Stuck => {}
        }
        if TRAFFIC_PROBE_EPOCH.load(Ordering::Acquire) != epoch {
            logging!(
                info,
                Type::Core,
                "the traffic probe began before a fix and its verdict is stale; not reporting it"
            );
            return;
        }
        let stack = runtime_stack().await.unwrap_or_else(|| String::from("unknown"));
        logging!(
            warn,
            Type::Core,
            "the TUN device is up but passes no traffic (stack {})",
            stack
        );
        if !NO_TRAFFIC_NOTICED.swap(true, Ordering::AcqRel) {
            set_failure(FAILURE_NO_TRAFFIC);
            Handle::notice_message("tun::no_traffic", stack);
        }
    });
}

async fn read_tun_config() -> Option<TunConfig> {
    let core = crate::core::handle::Handle::mihomo();
    match tokio::time::timeout(TUN_READ_TIMEOUT, core.get_base_config()).await {
        Ok(Ok(config)) => Some(config.tun),
        _ => None,
    }
}

pub async fn runtime_stack() -> Option<String> {
    if !claimed().await {
        return None;
    }
    match read_tun_config().await {
        Some(tun) if tun.enable => Some(tun.stack.to_string()),
        _ => None,
    }
}

pub async fn enforce_undesired_off() {
    if claimed().await {
        return;
    }
    if matches!(
        *crate::core::CoreManager::global().get_running_mode(),
        crate::core::manager::RunningMode::NotRunning
    ) {
        return;
    }
    let Some(tun) = read_tun_config().await else {
        return;
    };
    if !tun.enable || claimed().await {
        return;
    }
    logging!(
        warn,
        Type::Core,
        "the core still holds the TUN device while it is not wanted; taking it down"
    );
    match switch_tun_device(false).await {
        Ok(()) => {
            if claimed().await {
                logging!(
                    warn,
                    Type::Core,
                    "TUN became wanted while it was being taken down; bringing it back"
                );
                AsyncHandler::spawn(|| async { recreate_tun_device().await });
            } else {
                logging!(info, Type::Core, "the unwanted TUN device was taken down");
            }
        }
        Err(failure) => logging!(
            warn,
            Type::Core,
            "could not take down the unwanted TUN device: {}",
            failure.detail()
        ),
    }
}

const FAILURES_A_NEW_NETWORK_CAN_FIX: &[&str] = &[FAILURE_START, FAILURE_ADAPTER_BUSY];

fn a_new_network_could_help() -> bool {
    last_failure().is_none_or(|failure| FAILURES_A_NEW_NETWORK_CAN_FIX.contains(&failure))
}

fn rights_have_arrived() -> bool {
    is_app_elevated()
        || matches!(
            *crate::core::CoreManager::global().get_running_mode(),
            crate::core::manager::RunningMode::Service
        )
}

fn rearm_cooldown() -> Duration {
    let steps = REARM_BACKOFF.load(Ordering::Acquire).min(REARM_BACKOFF_STEPS);
    TUN_REARM_COOLDOWN * (1 << steps)
}

fn stamp_rearm() {
    *LAST_REARM_AT.lock() = Some(Instant::now());
}

fn rearm_cooldown_expired() -> bool {
    let now = Instant::now();
    let cooldown = rearm_cooldown();
    let expired = {
        let mut last = LAST_REARM_AT.lock();
        if last.is_some_and(|at| now.duration_since(at) < cooldown) {
            false
        } else {
            *last = Some(now);
            true
        }
    };
    if expired {
        REARM_BACKOFF.fetch_add(1, Ordering::AcqRel);
    }
    expired
}

pub async fn recheck_after_network_change() {
    if !desired().await {
        return;
    }
    if is_suppressed() {
        if BRING_BACK_RUNNING.load(Ordering::Acquire) {
            return;
        }
        if !a_new_network_could_help() || !rearm_cooldown_expired() {
            return;
        }
        clear_suppression();
        AsyncHandler::spawn(|| async {
            bring_tun_back("the network changed and the suppressed device gets a fresh budget").await;
        });
        return;
    }
    if RETRY_PENDING.load(Ordering::Acquire) {
        logging!(
            info,
            Type::Core,
            "the network changed while the TUN device is still being retried; the budget stands"
        );
        return;
    }
    START_ATTEMPTS.store(0, Ordering::Release);
    START_FAILED.store(false, Ordering::Release);
    spawn_start_verification(log_anchor().await);
}

pub async fn log_anchor() -> Option<String> {
    crate::core::CoreManager::global()
        .get_clash_logs()
        .await
        .ok()
        .and_then(|logs| logs.last().map(ToString::to_string))
}

pub fn spawn_start_verification(anchor: Option<String>) {
    *WATCH_ANCHOR.lock() = anchor;
    let generation = VERIFY_GENERATION.fetch_add(1, Ordering::AcqRel) + 1;
    AsyncHandler::spawn(move || async move {
        tokio::time::sleep(timing::TUN_VERIFY_DELAY).await;
        if VERIFY_GENERATION.load(Ordering::Acquire) != generation || !claimed().await {
            return;
        }
        if !matches!(verify_round().await, Round::Done) {
            spawn_watchdog();
            spawn_traffic_probe();
        }
    });
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Round {
    Clean,
    Unknown,
    Done,
}

async fn device_reported_up() -> Option<bool> {
    read_tun_config().await.map(|tun| tun.enable)
}

async fn verify_round() -> Round {
    let anchor = WATCH_ANCHOR.lock().clone();
    let Ok(logs) = crate::core::CoreManager::global().get_clash_logs().await else {
        return Round::Unknown;
    };
    match verdict(&logs, anchor.as_deref()) {
        Verdict::Failed(line) => {
            report_start_failure(line);
            Round::Done
        }
        Verdict::Clean(next) => {
            *WATCH_ANCHOR.lock() = next;
            Round::Clean
        }
    }
}

fn spawn_watchdog() {
    let generation = WATCHDOG_GENERATION.fetch_add(1, Ordering::AcqRel) + 1;
    AsyncHandler::spawn(move || async move {
        let mut rounds: u32 = 0;
        let mut down_rounds: u32 = 0;
        let mut probe_gap = TRAFFIC_RECHECK_ROUNDS;
        let mut next_probe = TRAFFIC_RECHECK_ROUNDS;
        loop {
            tokio::time::sleep(timing::TUN_WATCH_INTERVAL).await;
            if WATCHDOG_GENERATION.load(Ordering::Acquire) != generation || !claimed().await {
                return;
            }
            match device_reported_up().await {
                Some(false) => {
                    down_rounds += 1;
                    if down_rounds >= READBACK_DOWN_STRIKES {
                        report_start_failure("the core reports the TUN device is not up");
                        return;
                    }
                }
                Some(true) => {
                    down_rounds = 0;
                    REARM_BACKOFF.store(0, Ordering::Release);
                }
                None => {}
            }
            match verify_round().await {
                Round::Done => return,
                Round::Clean => {
                    START_ATTEMPTS.store(0, Ordering::Release);
                    rounds = rounds.saturating_add(1);
                    if rounds >= next_probe {
                        spawn_traffic_probe();
                        probe_gap = probe_gap.saturating_mul(2).min(TRAFFIC_RECHECK_MAX_ROUNDS);
                        next_probe = rounds.saturating_add(probe_gap);
                    }
                }
                Round::Unknown => {}
            }
        }
    });
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Verdict<'a> {
    Failed(&'a str),
    Clean(Option<String>),
}

fn verdict<'a, S: AsRef<str>>(logs: &'a [S], anchor: Option<&str>) -> Verdict<'a> {
    if let Some(line) = fresh_failure(logs, anchor) {
        return Verdict::Failed(line.as_ref());
    }
    Verdict::Clean(
        logs.last()
            .map(|line| line.as_ref().to_owned())
            .or_else(|| anchor.map(ToOwned::to_owned)),
    )
}

fn fresh_failure<'a, S: AsRef<str>>(logs: &'a [S], anchor: Option<&str>) -> Option<&'a S> {
    let from = anchor
        .and_then(|anchor| logs.iter().rposition(|line| line.as_ref() == anchor))
        .map_or(0, |position| position + 1);
    logs[from..]
        .iter()
        .rev()
        .find(|line| line_reports_tun_failure(line.as_ref()))
}

async fn record_setup_attempt() {
    let version = env!("CARGO_PKG_VERSION");
    let _serialized = crate::feat::config::patch_verge_lock().lock().await;
    let verge = Config::verge().await;
    verge.edit_draft(|d| {
        d.tun_setup_declined = Some(version.into());
    });
    verge.apply();
    let data = Config::verge().await.latest_arc();
    if let Err(e) = data.save_file().await {
        logging!(warn, Type::Core, "failed to persist the TUN setup attempt: {}", e);
    }
    Handle::refresh_verge();
}

pub async fn setup_declined_for_this_version() -> bool {
    Config::verge()
        .await
        .latest_arc()
        .tun_setup_declined
        .as_deref()
        .is_some_and(|declined_at| declined_at == env!("CARGO_PKG_VERSION"))
}

pub async fn clear_setup_declined() {
    if Config::verge().await.latest_arc().tun_setup_declined.is_none() {
        return;
    }
    let _serialized = crate::feat::config::patch_verge_lock().lock().await;
    let verge = Config::verge().await;
    verge.edit_draft(|d| {
        d.tun_setup_declined = None;
    });
    verge.apply();
    let data = Config::verge().await.latest_arc();
    if let Err(e) = data.save_file().await {
        logging!(warn, Type::Core, "failed to clear the TUN setup decline: {}", e);
    }
    Handle::refresh_verge();
}

async fn proven_alive_at_startup(user_initiated: bool) {
    if !user_initiated {
        clear_setup_declined().await;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupOutcome {
    AlreadyReady,
    Installed,
    Declined,
    Busy,
    Failed,
    Pending,
    Exiting,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupAnswer {
    Ready,
    NotReady,
    Refused(&'static str),
}

impl SetupOutcome {
    pub const fn answer(self) -> SetupAnswer {
        match self {
            Self::AlreadyReady | Self::Installed => SetupAnswer::Ready,
            Self::Declined | Self::Failed => SetupAnswer::NotReady,
            Self::Busy => SetupAnswer::Refused("tun::setup_busy"),
            Self::Pending => SetupAnswer::Refused("tun::setup_pending"),
            Self::Exiting => SetupAnswer::Refused(EXIT_REFUSAL_STATUS),
        }
    }
}

pub async fn ensure_ready(user_initiated: bool) -> SetupOutcome {
    if Handle::global().is_exiting() {
        logging!(
            info,
            Type::Service,
            "подготовка службы для TUN пропущена: выход уже идёт"
        );
        return SetupOutcome::Exiting;
    }

    if user_initiated {
        forget_last_notice();
    }

    if already_ready(user_initiated).await {
        return SetupOutcome::AlreadyReady;
    }

    if user_initiated {
        clear_setup_declined().await;
    } else if setup_declined_for_this_version().await {
        logging!(
            info,
            Type::Service,
            "service setup was already attempted on this version; not asking again"
        );
        return SetupOutcome::Declined;
    }

    if SETUP_RUNNING.swap(true, Ordering::AcqRel) {
        return SetupOutcome::Busy;
    }
    scopeguard::defer! {
        SETUP_RUNNING.store(false, Ordering::Release);
    }

    set_up_service().await
}

async fn already_ready(user_initiated: bool) -> bool {
    if is_capable().await {
        clear_suppression();
        proven_alive_at_startup(user_initiated).await;
        // Ядро ещё своим процессом без прав — TUN в нём не поднять: сначала
        // передать ядро службе, возврат TUN пойдёт следом за передачей.
        let manager = crate::core::CoreManager::global();
        if matches!(*manager.get_running_mode(), crate::core::manager::RunningMode::Sidecar) && !is_app_elevated() {
            manager.handoff_to_service_if_needed().await;
        } else {
            bring_tun_back_if_the_config_lacks_it("the service turned out to be available").await;
        }
        return true;
    }

    if nudge_registered_service().await || wait_until_capable(true).await {
        clear_suppression();
        proven_alive_at_startup(user_initiated).await;
        crate::core::CoreManager::global().handoff_to_service_if_needed().await;
        return true;
    }

    false
}

async fn nudge_registered_service() -> bool {
    if !matches!(service_registration(), ServiceRegistration::Stopped) {
        return false;
    }

    logging!(
        info,
        Type::Service,
        "the service is registered but stopped; starting it without asking for rights"
    );

    let started = tokio::task::spawn_blocking(start_registered_service)
        .await
        .unwrap_or(false);
    if !started {
        return false;
    }

    wait_until_capable(false).await
}

async fn set_up_service() -> SetupOutcome {
    let action = required_action().await;
    logging!(
        info,
        Type::Service,
        "preparing the background service for TUN: {:?}",
        action
    );
    let _ = SERVICE_MANAGER.current().await;
    Handle::notice_message("tun::setup_started", "");

    if let Err(e) = SERVICE_MANAGER.handle_service_status(action).await {
        if e.downcast_ref::<ServiceBusy>().is_some() {
            if elevation_in_flight() {
                logging!(
                    info,
                    Type::Service,
                    "an authorisation dialog is already open; not asking a second time"
                );
                return SetupOutcome::Pending;
            }
            logging!(info, Type::Service, "the service manager is busy; leaving it be");
            return SetupOutcome::Busy;
        }
        if e.downcast_ref::<ElevationPending>().is_some() {
            logging!(
                warn,
                Type::Service,
                "the authorisation dialog is still open; not waiting for it any longer"
            );
            record_setup_attempt().await;
            return SetupOutcome::Pending;
        }
        let detail = format!("{e}");
        logging!(warn, Type::Service, "background service setup failed: {}", detail);
        record_setup_attempt().await;
        let (event, failure) = if setup_rights_declined(&detail) {
            ("tun::rights_declined", FAILURE_RIGHTS_DECLINED)
        } else {
            ("tun::setup_failed", FAILURE_SETUP)
        };
        set_failure(failure);
        Handle::notice_message(event, "");
        return SetupOutcome::Failed;
    }

    record_setup_attempt().await;

    if !wait_until_capable(false).await {
        logging!(
            warn,
            Type::Service,
            "the service setup reported success, but the service still does not answer"
        );
        set_failure(FAILURE_SERVICE_SILENT);
        Handle::notice_message("tun::service_silent", "");
        return SetupOutcome::Failed;
    }

    clear_suppression();
    clear_setup_declined().await;
    logging!(info, Type::Service, "background service is up");
    Handle::notice_message("tun::setup_done", "");
    Handle::refresh_verge();
    crate::core::CoreManager::global().handoff_to_service_if_needed().await;
    SetupOutcome::Installed
}

async fn required_action() -> ServiceStatus {
    action_for(service_registration(), service_needs_repair().await)
}

const fn action_for(registration: ServiceRegistration, needs_repair: bool) -> ServiceStatus {
    if needs_repair {
        return ServiceStatus::ReinstallRequired;
    }

    match registration {
        ServiceRegistration::Missing | ServiceRegistration::Stopped => ServiceStatus::InstallRequired,
        ServiceRegistration::Running => ServiceStatus::ForceReinstallRequired,
        ServiceRegistration::Unknown => ServiceStatus::InstallRequired,
    }
}

async fn wait_until_capable(trust_registration: bool) -> bool {
    if is_app_elevated() {
        return true;
    }
    let deadline = Instant::now() + timing::TUN_SERVICE_APPEAR_WAIT;
    loop {
        // Подвисшая служба держит опрос минутами — дольше срока не ждём. Опрос
        // только читает, обрывать его безопасно; последнему кругу после срока
        // остаётся один интервал.
        let left = deadline
            .saturating_duration_since(Instant::now())
            .max(timing::TUN_SERVICE_APPEAR_INTERVAL);
        let probe = tokio::time::timeout(left, probe_service())
            .await
            .unwrap_or_else(|_| ServiceProbe::Silent("no answer within the wait".into()));
        if matches!(probe, ServiceProbe::Ready(_)) {
            return true;
        }
        let registration = service_registration();
        let pointless = matches!(registration, ServiceRegistration::Missing)
            || (trust_registration
                && (matches!(registration, ServiceRegistration::Stopped) || probe == ServiceProbe::Outdated));
        if pointless || Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(timing::TUN_SERVICE_APPEAR_INTERVAL).await;
    }
}

/// clod:tun-before-service — TUN включён, прав администратора нет, а службы
/// нет вовсе: без неё туннель не поднимется, и ждать её (`SERVICE_WAIT_MAX`),
/// а потом трижды пробовать туннель — только держать человека без ядра
/// полминуты. Проверка дешёвая, без IPC: регистрация службы в системе. Флаг
/// сеансовый, как и при отказе ядра: установит службу — `set_up_service` сам
/// снимет подавление и передаст ядро службе. Остановленную службу сюда не
/// относим — она может подняться, её ждём как прежде.
pub async fn hold_down_without_a_service() {
    use crate::core::service::{ServiceRegistration, service_registration};

    if !desired().await || is_app_elevated() {
        return;
    }
    let registration = tokio::task::spawn_blocking(service_registration)
        .await
        .unwrap_or(ServiceRegistration::Unknown);
    if !matches!(registration, ServiceRegistration::Missing) {
        return;
    }
    logging!(
        warn,
        Type::Core,
        "TUN is on, the app is not elevated and the service is not installed: starting without TUN"
    );
    hold_tun_down("the service is not installed", FAILURE_NO_RIGHTS, "tun::no_rights");
}

pub async fn init_startup_setup() {
    if !desired().await {
        logging!(
            info,
            Type::Service,
            "TUN is off; leaving the background service alone at startup"
        );
        return;
    }

    let outcome = ensure_ready(false).await;
    logging!(info, Type::Service, "startup TUN readiness: {:?}", outcome);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_follows_one_probe_of_the_service() {
        use crate::core::service::ServiceReady;

        let ready = ServiceProbe::Ready(ServiceReady { staging: false });
        let silent = ServiceProbe::Silent("example".into());
        for elevated in [false, true] {
            assert_eq!(capability_of(&ready, elevated), (true, false));
            assert_eq!(capability_of(&ServiceProbe::Outdated, elevated), (elevated, true));
            assert_eq!(capability_of(&silent, elevated), (elevated, false));
        }
    }

    fn tunnel(edit: impl FnOnce(&mut TunConfig)) -> TunConfig {
        let mut tun = TunConfig {
            enable: true,
            auto_route: true,
            inet4_address: Some(vec!["198.18.0.1/30".into()]),
            ..TunConfig::default()
        };
        edit(&mut tun);
        tun
    }

    #[test]
    fn the_route_is_judged_by_the_source_the_system_picks() {
        let ours = vec![String::from("198.18.0.1/30")];
        let cases: [(&str, &[String], Route); 6] = [
            ("198.18.0.1", &ours, Route::Tunnel),
            ("198.18.0.2", &ours, Route::Tunnel),
            ("192.168.1.20", &ours, Route::Bypass),
            ("10.0.0.5", &ours, Route::Bypass),
            ("127.0.0.1", &ours, Route::Unknown),
            ("0.0.0.0", &ours, Route::Unknown),
        ];
        for (source, addresses, expected) in cases {
            let source: std::net::IpAddr = source
                .parse()
                .unwrap_or(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST));
            assert_eq!(route_by_source(source, addresses), expected, "{source}");
        }
        let lan: std::net::IpAddr = std::net::IpAddr::V4(std::net::Ipv4Addr::new(192, 168, 1, 20));
        assert_eq!(route_by_source(lan, &[]), Route::Unknown);
        assert_eq!(route_by_source(lan, &[String::from("junk")]), Route::Unknown);
    }

    #[test]
    fn the_route_is_checked_only_when_the_tunnel_takes_the_whole_way_out() {
        let some = |items: &[&str]| Some(items.iter().map(|item| String::from(*item)).collect::<Vec<_>>());
        let cases: Vec<(&str, TunConfig, bool)> = vec![
            ("plain", tunnel(|_| {}), true),
            ("empty lists", tunnel(|t| t.route_address = Some(vec![])), true),
            (
                "private exclusions",
                tunnel(|t| t.route_exclude_address = some(&["10.0.0.0/8", "192.168.0.0/16"])),
                true,
            ),
            ("no auto-route", tunnel(|t| t.auto_route = false), false),
            ("auto-redirect", tunnel(|t| t.auto_redirect = Some(true)), false),
            ("narrowed", tunnel(|t| t.route_address = some(&["8.8.8.0/24"])), false),
            (
                "narrowed, old key",
                tunnel(|t| t.inet4_route_address = some(&["8.8.8.0/24"])),
                false,
            ),
            (
                "narrowed by a set",
                tunnel(|t| t.route_address_set = some(&["geoip-us"])),
                false,
            ),
            (
                "target excluded",
                tunnel(|t| t.route_exclude_address = some(&["1.1.1.0/24"])),
                false,
            ),
            (
                "target excluded, old key",
                tunnel(|t| t.inet4_route_exclude_address = some(&["1.0.0.0/8"])),
                false,
            ),
            (
                "exclusion set",
                tunnel(|t| t.route_exclude_address_set = some(&["geoip-ru"])),
                false,
            ),
            ("interfaces", tunnel(|t| t.include_interface = some(&["eth0"])), false),
            ("uids", tunnel(|t| t.include_uid = Some(vec![1000])), false),
            (
                "uid ranges",
                tunnel(|t| t.exclude_uid_range = some(&["1000:2000"])),
                false,
            ),
            ("source ports", tunnel(|t| t.exclude_src_port = Some(vec![5000])), false),
            (
                "destination ports",
                tunnel(|t| t.exclude_dst_port = Some(vec![443])),
                false,
            ),
            (
                "destination port ranges",
                tunnel(|t| t.exclude_dst_port_range = some(&["400:500"])),
                false,
            ),
        ];
        for (name, tun, expected) in cases {
            assert_eq!(route_check_applies(&tun), expected, "{name}");
        }
    }

    #[test]
    fn tells_a_rights_failure_from_a_busy_adapter() {
        assert!(line_reports_no_rights(
            "Start TUN listening error: configure tun interface: Access is denied."
        ));
        assert!(line_reports_no_rights(
            "configure tun interface: Connect: operation not permitted"
        ));
        assert!(!line_reports_no_rights("configure tun interface: file exists"));
        assert!(line_reports_adapter_busy("create tun: wintun adapter already exists"));
    }

    #[test]
    fn tells_a_declined_prompt_from_a_broken_installer() {
        assert!(setup_rights_declined("the elevation prompt was dismissed"));
        assert!(setup_rights_declined(
            "administrator rights were not granted: the polkit prompt was cancelled or declined"
        ));
        assert!(!setup_rights_declined("uninstaller not found"));
    }

    #[test]
    fn recognises_the_core_tun_failures() {
        assert!(line_reports_tun_failure(
            "Start TUN listening error: configure tun interface: Connect: operation not permitted"
        ));
        assert!(line_reports_tun_failure("configure tun interface: Access is denied."));
        assert!(!line_reports_tun_failure("[TCP] tun accept connection"));
        assert!(!line_reports_tun_failure("Start initial provider default"));
    }

    #[test]
    fn tells_a_busy_adapter_apart_from_other_failures() {
        assert!(line_reports_adapter_busy(
            "Start TUN listening error: wintun: Cannot create a file when that file already exists."
        ));
        assert!(line_reports_adapter_busy(
            "configure tun interface: device or resource busy"
        ));
        assert!(!line_reports_adapter_busy("configure tun interface: Access is denied."));
    }

    #[test]
    fn asks_for_the_smallest_service_action() {
        assert_eq!(
            action_for(ServiceRegistration::Unknown, false),
            ServiceStatus::InstallRequired
        );
        assert_eq!(
            action_for(ServiceRegistration::Missing, false),
            ServiceStatus::InstallRequired
        );
        assert_eq!(
            action_for(ServiceRegistration::Stopped, false),
            ServiceStatus::InstallRequired
        );
        assert_eq!(
            action_for(ServiceRegistration::Running, false),
            ServiceStatus::ForceReinstallRequired
        );
        for registration in [
            ServiceRegistration::Missing,
            ServiceRegistration::Stopped,
            ServiceRegistration::Running,
            ServiceRegistration::Unknown,
        ] {
            assert_eq!(action_for(registration, true), ServiceStatus::ReinstallRequired);
        }
    }

    #[test]
    fn old_complaints_do_not_count_against_a_new_attempt() {
        const FAILURE: &str = "Start TUN listening error: configure tun interface: Access is denied.";
        let logs = [FAILURE, "[TCP] tun accept connection"];
        assert!(fresh_failure(&logs, Some("[TCP] tun accept connection")).is_none());
        assert_eq!(
            fresh_failure(&logs, Some("Start initial provider default")).copied(),
            Some(FAILURE)
        );
        assert_eq!(fresh_failure(&logs, None).copied(), Some(FAILURE));
        assert_eq!(fresh_failure(&logs, Some("evicted line")).copied(), Some(FAILURE));
        let repeated = ["[TCP] tun accept connection", FAILURE, "[TCP] tun accept connection"];
        assert!(fresh_failure(&repeated, Some("[TCP] tun accept connection")).is_none());
    }

    #[test]
    fn the_watchdog_runs_exactly_while_tun_is_claimed() {
        assert!(is_claimed(true, false));
        assert!(!is_claimed(false, false));
        assert!(!is_claimed(true, true));
        assert!(!is_claimed(false, true));
    }

    #[test]
    fn each_round_moves_the_anchor_to_the_last_seen_line() {
        const FAILURE: &str = "Start TUN listening error: configure tun interface: Access is denied.";
        let logs = ["[TCP] tun accept connection", "Start initial provider default"];
        assert_eq!(
            verdict(&logs, Some("[TCP] tun accept connection")),
            Verdict::Clean(Some("Start initial provider default".to_owned()))
        );
        let empty: [&str; 0] = [];
        assert_eq!(
            verdict(&empty, Some("anchor")),
            Verdict::Clean(Some("anchor".to_owned()))
        );
        assert_eq!(verdict(&empty, None), Verdict::Clean(None));
        let broken = ["[TCP] tun accept connection", FAILURE];
        assert_eq!(
            verdict(&broken, Some("[TCP] tun accept connection")),
            Verdict::Failed(FAILURE)
        );
        assert_eq!(
            verdict(&broken, Some(FAILURE)),
            Verdict::Clean(Some(FAILURE.to_owned()))
        );
    }

    #[test]
    fn suppression_and_the_attempt_budget_are_session_state() {
        clear_suppression();
        assert!(!is_suppressed());
        suppress("test");
        assert!(is_suppressed());
        START_ATTEMPTS.store(TUN_START_ATTEMPTS, Ordering::Release);
        clear_suppression();
        assert!(!is_suppressed());
        assert_eq!(START_ATTEMPTS.load(Ordering::Acquire), 0);
    }

    const EVERY_OUTCOME: [SetupOutcome; 7] = [
        SetupOutcome::AlreadyReady,
        SetupOutcome::Installed,
        SetupOutcome::Declined,
        SetupOutcome::Busy,
        SetupOutcome::Failed,
        SetupOutcome::Pending,
        SetupOutcome::Exiting,
    ];

    #[test]
    fn a_refused_preparation_never_passes_for_a_service_that_is_not_installed() {
        let refused = SetupOutcome::Exiting.answer();
        assert_eq!(refused, SetupAnswer::Refused(EXIT_REFUSAL_STATUS));
        assert_ne!(
            refused,
            SetupOutcome::Declined.answer(),
            "«идёт выход» и «человек отказал» отвечают одинаково — экран позовёт ставить службу"
        );
        assert_ne!(
            refused,
            SetupOutcome::Failed.answer(),
            "«идёт выход» неотличим от провалившейся установки"
        );
        assert_ne!(refused, SetupOutcome::AlreadyReady.answer());
    }

    #[test]
    fn every_outcome_of_the_preparation_carries_its_own_answer() {
        let mut markers = std::collections::BTreeSet::new();
        let (mut ready, mut not_ready) = (0_usize, 0_usize);
        for outcome in EVERY_OUTCOME {
            match outcome.answer() {
                SetupAnswer::Ready => ready += 1,
                SetupAnswer::NotReady => not_ready += 1,
                SetupAnswer::Refused(marker) => {
                    assert!(!marker.is_empty(), "{outcome:?}: отказ без причины");
                    assert!(markers.insert(marker), "{outcome:?}: причина отказа уже занята");
                }
            }
        }
        assert_eq!(ready, 2, "готовыми считаются ровно два исхода");
        assert_eq!(not_ready, 2, "«службы нет» значат ровно два исхода");
        assert_eq!(markers.len(), 3, "у каждого отказа своя причина");
    }

    #[test]
    fn the_tunnel_is_retried_before_it_is_given_up_on() {
        const { assert!(TUN_START_ATTEMPTS > 1) };
        assert!(should_retry(1));
        assert!(should_retry(TUN_START_ATTEMPTS - 1));
        assert!(!should_retry(TUN_START_ATTEMPTS));
        assert!(!should_retry(TUN_START_ATTEMPTS + 1));
    }

    #[test]
    fn what_this_session_brings_up_is_decided_before_anyone_reads_it() {
        let resolve = crate::utils::source_scan::production_code(include_str!("../utils/resolve/mod.rs"));
        let setup = crate::utils::source_scan::fn_body(resolve, "pub fn resolve_setup_async").unwrap_or_default();
        assert_eq!(setup.matches("init_launch_connect_state()").count(), 1, "{setup}");
        let decided = setup.find("init_launch_connect_state()").unwrap_or(usize::MAX);
        for reader in [
            "init_window()",
            "hold_down_without_a_service()",
            "init_verge_config()",
            "init_core_manager()",
        ] {
            let read = setup.find(reader).unwrap_or_default();
            assert!(decided < read, "{reader} читает флаги подключения до решения");
        }
        let tun = crate::utils::source_scan::production_code(include_str!("tun.rs"));
        let hold_down =
            crate::utils::source_scan::fn_body(tun, "pub async fn hold_down_without_a_service").unwrap_or_default();
        assert!(hold_down.contains("desired().await"), "{hold_down}");
        assert!(!tun.contains("launch_connect_state"), "второе знание того же решения");
    }

    #[test]
    fn a_new_core_without_the_tun_it_should_have_gets_it_back() {
        assert_eq!(new_core_tun(true, true), NewCoreTun::Verify);
        // Туннель выкинут из конфига после провала, подавление сняли
        // перезапуском или заменой ядра: включённым без проверки его не оставить.
        assert_eq!(new_core_tun(true, false), NewCoreTun::BringBack);
        assert_eq!(new_core_tun(false, true), NewCoreTun::TakeDown);
        assert_eq!(new_core_tun(false, false), NewCoreTun::TakeDown);
        let tun = crate::utils::source_scan::production_code(include_str!("tun.rs"));
        let follow_up =
            crate::utils::source_scan::fn_body(tun, "pub async fn follow_up_on_a_new_core").unwrap_or_default();
        assert!(
            follow_up.contains("new_core_tun(claimed().await, accepted_has_tun().await)")
                && follow_up.contains("spawn_bringing_tun_back_if_the_config_lacks_it("),
            "{follow_up}"
        );
    }
}
