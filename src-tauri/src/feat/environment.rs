use crate::{
    config::Config,
    constants::timing,
    core::{CoreManager, handle, sysopt::verbose_diagnostics},
    process::AsyncHandler,
};
use clash_verge_logging::{Type, logging};
use parking_lot::Mutex;
use std::{
    collections::BTreeSet,
    fmt::Write as _,
    net::{Ipv4Addr, Ipv6Addr},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tauri_plugin_mihomo::Mihomo;

static WATCHDOG_GENERATION: AtomicU64 = AtomicU64::new(0);
static WAKE_REARM_PENDING: AtomicBool = AtomicBool::new(false);
static LISTING_FAILED: AtomicBool = AtomicBool::new(false);
static CLOCK_FAILED: AtomicBool = AtomicBool::new(false);
static WAKE_REARM_SINCE: Mutex<Option<Instant>> = Mutex::new(None);

const SLEEP_SLACK: Duration = Duration::from_secs(20);

const FINGERPRINT_ENTRIES_SHOWN: usize = 8;

const CORE_TUNNEL_BASE: &str = "meta";

const WAITED_WORTH_SPELLING_OUT: Duration = Duration::from_secs(1);

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod sleep_clock {
    use std::time::Duration;

    #[cfg(target_os = "linux")]
    const CLOCK_COUNTING_SLEEP: libc::clockid_t = libc::CLOCK_BOOTTIME;
    #[cfg(target_os = "macos")]
    const CLOCK_COUNTING_SLEEP: libc::clockid_t = libc::CLOCK_MONOTONIC;

    pub fn reading() -> Option<Duration> {
        let mut moment = libc::timespec { tv_sec: 0, tv_nsec: 0 };
        let asked = unsafe { libc::clock_gettime(CLOCK_COUNTING_SLEEP, &raw mut moment) };
        if asked != 0 || moment.tv_sec < 0 || moment.tv_nsec < 0 {
            return None;
        }
        Some(Duration::new(moment.tv_sec as u64, moment.tv_nsec as u32))
    }

    pub const fn asleep(instant_delta: Duration, before: Duration, after: Duration) -> Duration {
        after.saturating_sub(before).saturating_sub(instant_delta)
    }
}

#[cfg(target_os = "windows")]
mod sleep_clock {
    use std::time::Duration;
    use windows_sys::Win32::System::WindowsProgramming::QueryUnbiasedInterruptTime;

    pub fn reading() -> Option<Duration> {
        let mut awake_in_100ns: u64 = 0;
        if unsafe { QueryUnbiasedInterruptTime(&raw mut awake_in_100ns) } == 0 {
            return None;
        }
        Some(Duration::from_nanos(awake_in_100ns.saturating_mul(100)))
    }

    pub const fn asleep(instant_delta: Duration, before: Duration, after: Duration) -> Duration {
        instant_delta.saturating_sub(after.saturating_sub(before))
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
mod sleep_clock {
    use std::time::Duration;

    pub const fn reading() -> Option<Duration> {
        None
    }

    pub const fn asleep(_instant_delta: Duration, _before: Duration, _after: Duration) -> Duration {
        Duration::ZERO
    }
}

fn sleep_gap(instant_delta: Duration, before: Option<Duration>, after: Option<Duration>) -> Option<Duration> {
    let (Some(before), Some(after)) = (before, after) else {
        if !CLOCK_FAILED.swap(true, Ordering::AcqRel) {
            logging!(
                warn,
                Type::Core,
                "[clod] the system did not tell how long it stayed awake; sleep goes unnoticed on this machine"
            );
        }
        return None;
    };
    CLOCK_FAILED.store(false, Ordering::Release);
    Some(sleep_clock::asleep(instant_delta, before, after))
}

fn slept_through(gap: Option<Duration>) -> bool {
    gap.is_some_and(|gap| gap > SLEEP_SLACK)
}

fn spelled_out(span: Duration) -> std::string::String {
    let seconds = span.as_secs();
    if seconds < 60 {
        return format!("{seconds}s");
    }
    if seconds < 3600 {
        return format!("{}m {}s", seconds / 60, seconds % 60);
    }
    format!("{}h {}m", seconds / 3600, (seconds % 3600) / 60)
}

fn looks_like_the_core_default_tunnel(name: &str) -> bool {
    name.strip_prefix(CORE_TUNNEL_BASE)
        .is_some_and(|index| index.chars().all(|c| c.is_ascii_digit()))
}

fn named_with_an_index(name: &str, base: &str) -> bool {
    name.strip_prefix(base)
        .is_some_and(|index| !index.is_empty() && index.chars().all(|c| c.is_ascii_digit()))
}

/// Мост, имя которому выдала сама песочница: `br-` и двенадцать шестнадцатеричных
/// цифр от идентификатора сети.
///
/// clod:net-virtual — `docker compose up` создаёт такой мост под каждый проект, а
/// `docker compose down` его уносит. Домашний мост `br-lan` и внешний коммутатор
/// `br0` под это правило не попадают: там после `br-` либо ничего, либо не
/// двенадцать шестнадцатеричных цифр.
fn looks_like_a_generated_bridge(name: &str) -> bool {
    name.strip_prefix("br-")
        .is_some_and(|id| id.len() == 12 && id.chars().all(|c| c.is_ascii_hexdigit()))
}

/// Виртуальные коммутаторы и мосты локальных песочниц.
///
/// clod:net-virtual — Docker, Podman, LXD/Incus, WSL, Hyper-V, VirtualBox,
/// VMware и Parallels поднимают и гасят свои адаптеры по команде пользователя;
/// к пути машины наружу это отношения не имеет. Без этого `wsl --shutdown`,
/// остановка Docker или `docker compose down` рвали все живые соединения ровно
/// так же, как мигание Teredo.
fn is_a_local_sandbox_adapter(name: &str) -> bool {
    let name = name.to_lowercase();
    // Только имена, которые эти песочницы дают сами. Голый `veth` сюда не
    // годится, и просто `br-` тоже: под него попал бы домашний мост `br-lan`, а
    // на таких машинах это и есть единственный путь наружу. Короткие `cni` и
    // `vnic` берутся только с числовым индексом, `vEthernet (nat)` — целиком,
    // чтобы не задеть внешний коммутатор Hyper-V.
    [
        "docker",
        "podman",
        "virbr",
        "vboxnet",
        "vmnet",
        "vmware",
        "wsl",
        "lxdbr",
        "incusbr",
        "default switch",
        "vethernet (nat)",
    ]
    .iter()
    .any(|known| name.contains(known))
        || named_with_an_index(&name, "cni")
        || named_with_an_index(&name, "vnic")
        || looks_like_a_generated_bridge(&name)
}

fn is_our_tunnel(name: &str) -> bool {
    let name = name.to_lowercase();
    name.contains("mihomo") || name.starts_with("utun") || looks_like_the_core_default_tunnel(&name)
}

const fn v4_carries_traffic(ip: Ipv4Addr) -> bool {
    !ip.is_loopback() && !ip.is_link_local() && !ip.is_unspecified()
}

/// Переходные псевдотуннели IPv6: Teredo, 6to4, ISATAP.
///
/// clod:net-teredo — Windows поднимает и опускает их сама, по своему расписанию,
/// по нескольку раз в час, и путь машины к сети при этом не меняется. Трафик мы
/// через них не пускаем, так что в отпечатке сети им делать нечего: иначе
/// очередная спячка Teredo читается как «пропал адрес, к которому привязаны
/// соединения», и все живые соединения рвутся на ровном месте.
///
/// Определяются по адресу, а не по имени интерфейса: имя на Windows
/// локализуется, префиксы — нет.
const fn v6_is_transition_tunnel(ip: Ipv6Addr) -> bool {
    let segments = ip.segments();
    // Teredo — 2001:0::/32, 6to4 — 2002::/16.
    if (segments[0] == 0x2001 && segments[1] == 0) || segments[0] == 0x2002 {
        return true;
    }
    // ISATAP — идентификатор интерфейса ::0:5efe:a.b.c.d или ::200:5efe:a.b.c.d.
    matches!(segments[4], 0 | 0x0200) && segments[5] == 0x5efe
}

const fn v6_carries_traffic(ip: Ipv6Addr) -> bool {
    !ip.is_loopback() && !ip.is_unspecified() && (ip.segments()[0] & 0xffc0) != 0xfe80 && !v6_is_transition_tunnel(ip)
}

fn v6_prefix(ip: std::net::Ipv6Addr) -> std::string::String {
    let segments = ip.segments();
    format!(
        "{:x}:{:x}:{:x}:{:x}::/64",
        segments[0], segments[1], segments[2], segments[3]
    )
}

/// Единственное место, где решается принадлежность интерфейса пути наружу.
///
/// clod:net-virtual — решение принимается один раз, при построении переписи.
/// Повторять его на разнице двух переписей бессмысленно: отсеянное в перепись
/// не попадает, и второй фильтр всегда пропускал бы всё подряд, создавая
/// видимость защиты.
fn carries_a_path_of_its_own(name: &str) -> bool {
    !is_our_tunnel(name) && !is_a_local_sandbox_adapter(name)
}

/// Отпечаток сети — адреса, по которым трафик действительно может уйти.
///
/// clod:net-teredo — раньше в набор попадал каждый адрес каждого чужого
/// интерфейса, включая link-local и переходные туннели, а проверки
/// `*_carries_traffic` применялись только к отдельному флагу. Из-за этого
/// исчезновение адреса, которым никто не пользовался, считалось потерей пути.
/// Теперь набор и флаг говорят об одном и том же: пуст — сети нет.
fn fingerprint_of(interfaces: Vec<network_interface::NetworkInterface>) -> BTreeSet<std::string::String> {
    let mut entries = BTreeSet::new();

    for interface in interfaces {
        let network_interface::NetworkInterface { name, addr, .. } = interface;
        if !carries_a_path_of_its_own(&name) {
            continue;
        }
        for address in addr {
            match address {
                network_interface::Addr::V4(v4) if v4_carries_traffic(v4.ip) => {
                    entries.insert(format!("{name}:{}", v4.ip));
                }
                network_interface::Addr::V6(v6) if v6_carries_traffic(v6.ip) => {
                    entries.insert(format!("{name}:{}", v6_prefix(v6.ip)));
                }
                _ => (),
            }
        }
    }

    entries
}

fn network_fingerprint() -> Option<BTreeSet<std::string::String>> {
    Some(fingerprint_of(crate::cmd::network::get_network_interfaces_info().ok()?))
}

fn listing_just_failed() -> bool {
    !LISTING_FAILED.swap(true, Ordering::AcqRel)
}

async fn first_fingerprint() -> Option<BTreeSet<std::string::String>> {
    let view = AsyncHandler::spawn_blocking(network_fingerprint).await.ok().flatten();
    if view.is_none() && listing_just_failed() {
        logging!(
            warn,
            Type::Core,
            "[clod] the network interfaces could not be listed at startup; the first listing that succeeds counts as a change, but never as a lost path"
        );
    }
    view
}

fn listed<'a>(entries: impl Iterator<Item = &'a std::string::String>) -> std::string::String {
    let mut shown = 0_usize;
    let mut extra = 0_usize;
    let mut out = std::string::String::new();

    for entry in entries {
        if shown < FINGERPRINT_ENTRIES_SHOWN {
            if shown > 0 {
                out.push_str(", ");
            }
            out.push_str(entry);
            shown += 1;
        } else {
            extra += 1;
        }
    }

    if shown == 0 {
        return std::string::String::from("nothing");
    }
    if extra > 0 {
        let _ = write!(out, " and {extra} more");
    }
    out
}

fn interface_of(entry: &str) -> &str {
    entry.split_once(':').map_or(entry, |(name, _)| name)
}

/// В переписи только адреса интерфейсов, несущих путь наружу, поэтому исчезнувший
/// адрес — это исчезнувший путь, без дополнительных условий.
fn path_was_lost(before: &BTreeSet<std::string::String>, after: &BTreeSet<std::string::String>) -> bool {
    !before.is_subset(after)
}

/// clod:net-listing — «перечислить не удалось» и «адресов нет» — разные вещи: до
/// первой удачной переписи сравнивать не с чем, и потерей пути это не станет
/// никогда. Но первая удачная перепись после неудачного старта — это смена
/// окружения: прокси надо пере-навести, пустые наборы правил — дозалить.
fn changes_from(before: Option<&BTreeSet<std::string::String>>, view: &BTreeSet<std::string::String>) -> (bool, bool) {
    match before {
        Some(before) => (before != view, path_was_lost(before, view)),
        None => (!view.is_empty(), false),
    }
}

fn interfaces_of<'a>(entries: impl Iterator<Item = &'a std::string::String>) -> Vec<std::string::String> {
    let mut names: Vec<std::string::String> = entries.map(|entry| interface_of(entry).to_owned()).collect();
    names.dedup();
    names
}

fn report_fingerprint_change(
    before: &BTreeSet<std::string::String>,
    after: &BTreeSet<std::string::String>,
    verbose: bool,
) {
    if verbose {
        logging!(
            info,
            Type::Core,
            "[clod] network fingerprint: {} entries before, {} after; appeared {}; gone {}",
            before.len(),
            after.len(),
            listed(after.difference(before)),
            listed(before.difference(after))
        );
        return;
    }

    logging!(
        info,
        Type::Core,
        "[clod] network fingerprint: {} entries before, {} after; appeared on {}; gone from {}",
        before.len(),
        after.len(),
        listed(interfaces_of(after.difference(before)).iter()),
        listed(interfaces_of(before.difference(after)).iter())
    );
}

const CONNECTIONS_CALL_TIMEOUT: Duration = Duration::from_secs(3);

async fn close_live_connections(verbose: bool) {
    let core = detached_core_client().await;
    let live = if verbose {
        tokio::time::timeout(CONNECTIONS_CALL_TIMEOUT, core.get_connections())
            .await
            .ok()
            .and_then(Result::ok)
            .and_then(|response| response.connections)
            .map_or(0, |connections| connections.len())
    } else {
        0
    };

    let outcome = tokio::time::timeout(CONNECTIONS_CALL_TIMEOUT, core.close_all_connections()).await;

    match outcome {
        Ok(Ok(())) if verbose => logging!(
            info,
            Type::Core,
            "[clod] closed {live} live connections after the environment changed"
        ),
        Ok(Ok(())) => (),
        Ok(Err(e)) => logging!(
            debug,
            Type::Core,
            "[clod] could not close connections after the environment changed: {e}"
        ),
        Err(_) => logging!(
            debug,
            Type::Core,
            "[clod] the core did not answer closing connections after the environment changed"
        ),
    }
}

async fn reconcile(
    reason: &str,
    slept: bool,
    path_was_lost: bool,
    tun_is_being_rearmed: bool,
    network_carries_traffic: bool,
) {
    logging!(info, Type::Core, "[clod] environment changed ({reason}), reconciling");

    let verge = Config::verge().await.latest_arc();
    let wants_sysproxy = verge.enable_system_proxy.unwrap_or(false);
    let verbose = verge.verbose_diagnostics();
    let may_close_connections = verge.auto_close_connection();
    drop(verge);
    if wants_sysproxy {
        CoreManager::global().point_system_proxy_at_the_confirmed_port().await;
    }

    if !may_close_connections {
        if verbose {
            logging!(
                info,
                Type::Core,
                "[clod] live connections left alone: closing them on network events is turned off"
            );
        }
    } else if slept || path_was_lost {
        logging!(
            info,
            Type::Core,
            "[clod] closing live connections: {}",
            match (slept, path_was_lost) {
                (true, true) => "the machine was asleep and an address is gone",
                (true, false) => "the machine was asleep",
                _ => "an address the connections were bound to is gone",
            }
        );
        close_live_connections(verbose).await;
    } else if verbose {
        logging!(
            info,
            Type::Core,
            "[clod] live connections left alone: no address went away, the old path still stands"
        );
    }

    if !slept && !tun_is_being_rearmed {
        crate::feat::tun::recheck_after_network_change().await;
    }

    if network_carries_traffic {
        AsyncHandler::spawn(|| async { refill_empty_rule_sets().await });
    }
}

static RULE_SETS_REFILLING: AtomicBool = AtomicBool::new(false);
static RULE_SETS_REFILL_ASKED_AGAIN: AtomicBool = AtomicBool::new(false);
const RULE_SET_LIST_TIMEOUT: Duration = Duration::from_secs(5);
const RULE_SET_FETCH_TIMEOUT: Duration = Duration::from_secs(25);

pub(crate) async fn detached_core_client() -> Mihomo {
    let mihomo = handle::Handle::mihomo().await;
    Mihomo {
        protocol: mihomo.protocol.clone(),
        external_host: mihomo.external_host.clone(),
        external_port: mihomo.external_port,
        secret: mihomo.secret.clone(),
        socket_path: mihomo.socket_path.clone(),
        connection_manager: Arc::clone(&mihomo.connection_manager),
    }
}

async fn refill_empty_rule_sets() {
    RULE_SETS_REFILL_ASKED_AGAIN.store(true, Ordering::SeqCst);
    loop {
        if RULE_SETS_REFILLING.swap(true, Ordering::SeqCst) {
            return;
        }
        {
            scopeguard::defer! {
                RULE_SETS_REFILLING.store(false, Ordering::SeqCst);
            }
            while RULE_SETS_REFILL_ASKED_AGAIN.swap(false, Ordering::SeqCst) {
                if handle::Handle::global().is_exiting() {
                    return;
                }
                refill_empty_rule_sets_once().await;
            }
        }
        if !RULE_SETS_REFILL_ASKED_AGAIN.load(Ordering::SeqCst) {
            return;
        }
    }
}

async fn ask_for_each_rule_set<F, Fut>(empty: Vec<String>, each: Duration, ask: F) -> usize
where
    F: Fn(String) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    let mut asked = 0_usize;
    for name in empty {
        if handle::Handle::global().is_exiting() {
            break;
        }
        if tokio::time::timeout(each, ask(name.clone())).await.is_err() {
            logging!(
                info,
                Type::Core,
                "[clod] rule set {name}: no answer from the core within {}s; leaving it to the core's own retry",
                each.as_secs()
            );
        }
        asked += 1;
    }
    asked
}

async fn refill_empty_rule_sets_once() {
    let core = detached_core_client().await;
    let listed = tokio::time::timeout(RULE_SET_LIST_TIMEOUT, core.get_rule_providers()).await;
    let Ok(Ok(listed)) = listed else {
        return;
    };
    let mut empty: Vec<String> = listed
        .providers
        .values()
        .filter(|provider| {
            provider.rule_count == 0 && matches!(provider.vehicle_type, tauri_plugin_mihomo::models::VehicleType::HTTP)
        })
        .map(|provider| provider.name.clone())
        .collect();
    if empty.is_empty() {
        return;
    }
    empty.sort();
    logging!(
        info,
        Type::Core,
        "[clod] {} rule set(s) are still empty after the network came back; asking the core to fetch them: {}",
        empty.len(),
        empty.join(", ")
    );
    let core = &core;
    ask_for_each_rule_set(empty, RULE_SET_FETCH_TIMEOUT, move |name| async move {
        if let Err(e) = core.update_rule_provider(&name).await {
            logging!(info, Type::Core, "[clod] rule set {name} is not fetched yet: {e}");
        }
    })
    .await;
}

fn hold_the_tun_rearm(tun_is_wanted: bool) {
    *WAKE_REARM_SINCE.lock() = Some(Instant::now());
    if WAKE_REARM_PENDING.swap(true, Ordering::AcqRel) || !tun_is_wanted {
        return;
    }
    logging!(
        info,
        Type::Core,
        "[clod] the TUN device waits for a routable address before it is re-created"
    );
}

fn rearm_is_due(carries_traffic: bool) -> bool {
    carries_traffic && WAKE_REARM_PENDING.swap(false, Ordering::AcqRel)
}

fn worth_spelling_out(waited: Duration) -> Option<Duration> {
    (waited >= WAITED_WORTH_SPELLING_OUT).then_some(waited)
}

async fn rearm_the_tun_after_wake() {
    let waited = WAKE_REARM_SINCE
        .lock()
        .take()
        .map(|since| since.elapsed())
        .and_then(worth_spelling_out);
    if crate::feat::tun::desired().await {
        match waited {
            Some(waited) => logging!(
                info,
                Type::Core,
                "[clod] the network came back {} after the machine woke up; the TUN device gets a fresh budget",
                spelled_out(waited)
            ),
            None => logging!(
                info,
                Type::Core,
                "[clod] the machine woke up with the network already there; the TUN device gets a fresh budget"
            ),
        }
    } else {
        logging!(
            info,
            Type::Core,
            "[clod] the machine woke up with the network already there; the TUN device is switched off, nothing to re-create"
        );
    }
    AsyncHandler::spawn(|| async { crate::feat::tun::rearm_after_wake().await });
}

pub fn spawn_environment_watchdog() {
    if handle::Handle::global().is_exiting() {
        return;
    }
    let generation = WATCHDOG_GENERATION.fetch_add(1, Ordering::AcqRel) + 1;

    AsyncHandler::spawn(move || async move {
        let mut stopped_on_purpose = scopeguard::guard(false, move |on_purpose| {
            if !on_purpose
                && !handle::Handle::global().is_exiting()
                && WATCHDOG_GENERATION.load(Ordering::Acquire) == generation
            {
                logging!(
                    warn,
                    Type::Core,
                    "[clod] the environment watchdog stopped; it comes back with the next core start"
                );
            }
        });
        let mut last_tick = Instant::now();
        let mut last_awake = sleep_clock::reading();
        let mut last_network = first_fingerprint().await;
        let mut ticks: u32 = 0;

        loop {
            tokio::time::sleep(timing::ENVIRONMENT_TICK).await;
            ticks = ticks.wrapping_add(1);
            if handle::Handle::global().is_exiting() || WATCHDOG_GENERATION.load(Ordering::Acquire) != generation {
                *stopped_on_purpose = true;
                return;
            }

            let now_tick = Instant::now();
            let now_awake = sleep_clock::reading();
            let instant_delta = now_tick.duration_since(last_tick);
            let gap = sleep_gap(instant_delta, last_awake, now_awake);
            let slept = slept_through(gap);
            if slept && let Some(gap) = gap {
                logging!(
                    info,
                    Type::Core,
                    "[clod] the machine was asleep for {}",
                    spelled_out(gap)
                );
            }

            let view = AsyncHandler::spawn_blocking(network_fingerprint).await.ok().flatten();

            last_tick = now_tick;
            last_awake = now_awake;

            if slept || ticks.is_multiple_of(timing::TUN_UNWANTED_SWEEP_EVERY_TICKS) {
                crate::feat::tun::enforce_undesired_off().await;
            }

            if slept {
                hold_the_tun_rearm(crate::feat::tun::desired().await);
            }

            let Some(view) = view else {
                if listing_just_failed() {
                    logging!(
                        warn,
                        Type::Core,
                        "[clod] the network interfaces could not be listed; the previous fingerprint stands"
                    );
                }
                if slept {
                    reconcile("woke up", true, false, false, false).await;
                }
                continue;
            };
            LISTING_FAILED.store(false, Ordering::Release);

            let (network_changed, path_is_gone) = changes_from(last_network.as_ref(), &view);
            if network_changed && let Some(before) = last_network.as_ref() {
                report_fingerprint_change(before, &view, verbose_diagnostics().await);
            }
            let view_carries_traffic = !view.is_empty();
            last_network = Some(view);

            let rearm_is_now = rearm_is_due(view_carries_traffic);

            let reason = match (slept, network_changed) {
                (true, true) => "woke up, network differs",
                (true, false) => "woke up",
                (false, true) => "network changed",
                (false, false) => {
                    if rearm_is_now {
                        rearm_the_tun_after_wake().await;
                    }
                    continue;
                }
            };

            reconcile(reason, slept, path_is_gone, rearm_is_now, view_carries_traffic).await;
            if rearm_is_now {
                rearm_the_tun_after_wake().await;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{
        CORE_TUNNEL_BASE, FINGERPRINT_ENTRIES_SHOWN, SLEEP_SLACK, ask_for_each_rule_set, changes_from, fingerprint_of,
        interface_of, is_a_local_sandbox_adapter, is_our_tunnel, listed, looks_like_the_core_default_tunnel,
        path_was_lost, sleep_gap, slept_through, spelled_out, v4_carries_traffic, v6_carries_traffic,
        v6_is_transition_tunnel, worth_spelling_out,
    };
    use crate::constants::timing;
    use std::{
        net::{Ipv4Addr, Ipv6Addr},
        time::Duration,
    };

    #[test]
    fn an_empty_difference_is_spelled_out() {
        assert_eq!(listed(std::iter::empty()), "nothing");
    }

    #[test]
    fn a_long_difference_is_cut_and_counted() {
        let entries: Vec<std::string::String> = (0..FINGERPRINT_ENTRIES_SHOWN + 3)
            .map(|i| format!("if{i}:10.0.0.{i}"))
            .collect();
        let text = listed(entries.iter());

        assert!(text.starts_with("if0:10.0.0.0, if1:10.0.0.1"));
        assert!(text.ends_with("and 3 more"));
    }

    #[test]
    fn the_core_default_tunnel_is_the_base_name_with_an_index() {
        assert!(looks_like_the_core_default_tunnel(CORE_TUNNEL_BASE));
        assert!(looks_like_the_core_default_tunnel(&format!("{CORE_TUNNEL_BASE}0")));
        assert!(looks_like_the_core_default_tunnel(&format!("{CORE_TUNNEL_BASE}12")));
        assert!(!looks_like_the_core_default_tunnel(&format!("{CORE_TUNNEL_BASE}-work")));
        assert!(!looks_like_the_core_default_tunnel("tun0"));
        assert!(!looks_like_the_core_default_tunnel("wg0"));
        assert!(!looks_like_the_core_default_tunnel("eth0"));
    }

    #[test]
    fn the_tunnel_filter_does_not_depend_on_anything_that_can_change() {
        let ours = format!("{CORE_TUNNEL_BASE}0");

        assert!(is_our_tunnel("Mihomo"));
        assert!(is_our_tunnel("mihomo-tun"));
        assert!(is_our_tunnel("Meta"));
        assert!(is_our_tunnel(&ours));
        assert!(is_our_tunnel("utun4"));
        assert!(!is_our_tunnel("wg0"));
        assert!(!is_our_tunnel("eth0"));
        assert!(!is_our_tunnel("en0"));
        assert!(!is_our_tunnel("metavpn"));
    }

    #[test]
    fn an_entry_keeps_its_interface_name_even_when_the_address_has_colons() {
        assert_eq!(interface_of("eth0:192.168.1.5"), "eth0");
        assert_eq!(interface_of("eth0:2a02:1:2:3::/64"), "eth0");
        assert_eq!(interface_of("Ethernet 2:10.0.0.4"), "Ethernet 2");
        assert_eq!(interface_of("nothing"), "nothing");
    }

    fn fingerprint(entries: &[&str]) -> std::collections::BTreeSet<std::string::String> {
        entries.iter().map(|entry| (*entry).to_owned()).collect()
    }

    #[test]
    fn an_address_that_went_away_is_a_lost_path() {
        let before = fingerprint(&["eth0:10.0.0.2", "wlan0:192.168.1.7"]);
        let same = before.clone();
        let with_one_more = fingerprint(&["eth0:10.0.0.2", "wlan0:192.168.1.7", "wlan0:192.168.1.8"]);
        let without_wlan = fingerprint(&["eth0:10.0.0.2"]);
        let wlan_readdressed = fingerprint(&["eth0:10.0.0.2", "wlan0:192.168.1.9"]);
        let all_readdressed = fingerprint(&["eth0:10.0.0.9", "wlan0:192.168.1.9"]);
        let nothing = fingerprint(&[]);

        assert!(!path_was_lost(&before, &same));
        assert!(!path_was_lost(&before, &with_one_more));
        assert!(path_was_lost(&before, &without_wlan));
        assert!(path_was_lost(&before, &wlan_readdressed));
        assert!(path_was_lost(&before, &all_readdressed));
        assert!(path_was_lost(&before, &nothing));
        assert!(!path_was_lost(&nothing, &before));
    }

    #[test]
    fn a_foreign_tunnel_that_keeps_its_address_does_not_hide_a_lost_path() {
        let before = fingerprint(&["wg0:10.6.0.2", "tailscale0:100.64.1.5", "wlan0:192.168.1.7"]);
        let moved_to_a_hotspot = fingerprint(&["wg0:10.6.0.2", "tailscale0:100.64.1.5", "wlan0:172.20.10.3"]);
        let cable_pulled = fingerprint(&["wg0:10.6.0.2", "tailscale0:100.64.1.5"]);

        assert!(path_was_lost(&before, &moved_to_a_hotspot));
        assert!(path_was_lost(&before, &cable_pulled));
    }

    fn with_v4(name: &str, ip: [u8; 4]) -> network_interface::NetworkInterface {
        network_interface::NetworkInterface::new_afinet(name, Ipv4Addr::from(ip), None, None, 1, false)
    }

    /// Перепись строится из того, что отдаёт система, а не из набранного руками
    /// набора: иначе проверялось бы свойство одной функции, а не поведение.
    #[test]
    fn a_sandbox_going_down_never_reaches_the_census_and_so_is_not_a_lost_path() {
        let ours = format!("{CORE_TUNNEL_BASE}0");
        let sandboxes_up = vec![
            with_v4("eth0", [10, 0, 0, 2]),
            with_v4("docker0", [172, 17, 0, 1]),
            with_v4("br-3f2a1b9c8d7e", [172, 18, 0, 1]),
            with_v4("podman0", [10, 88, 0, 1]),
            with_v4("lxdbr0", [10, 55, 1, 1]),
            with_v4("incusbr0", [10, 56, 1, 1]),
            with_v4("cni0", [10, 244, 0, 1]),
            with_v4("vnic0", [10, 211, 55, 2]),
            with_v4("vnic1", [10, 37, 129, 2]),
            with_v4("vEthernet (nat)", [172, 26, 0, 1]),
            with_v4("vEthernet (Default Switch)", [172, 20, 0, 1]),
            with_v4("utun4", [198, 19, 0, 1]),
            with_v4(&ours, [198, 18, 0, 1]),
        ];
        let compose_down = vec![with_v4("eth0", [10, 0, 0, 2])];

        let before = fingerprint_of(sandboxes_up);
        let after = fingerprint_of(compose_down);

        assert_eq!(before, fingerprint(&["eth0:10.0.0.2"]));
        assert_eq!(before, after);
        assert!(!path_was_lost(&before, &after));
        assert_eq!(changes_from(Some(&before), &after), (false, false));
    }

    #[test]
    fn an_interface_the_census_keeps_is_a_lost_path_when_its_address_goes() {
        let docked = fingerprint_of(vec![
            with_v4("eth0", [10, 0, 0, 2]),
            with_v4("wlan0", [192, 168, 1, 7]),
            with_v4("br-lan", [192, 168, 2, 1]),
            with_v4("br0", [192, 168, 3, 1]),
            with_v4("vEthernet (External Switch)", [192, 168, 4, 1]),
        ]);
        let undocked = fingerprint_of(vec![
            with_v4("wlan0", [192, 168, 1, 7]),
            with_v4("br-lan", [192, 168, 2, 1]),
            with_v4("br0", [192, 168, 3, 1]),
            with_v4("vEthernet (External Switch)", [192, 168, 4, 1]),
        ]);

        assert!(docked.contains("br-lan:192.168.2.1"));
        assert!(docked.contains("br0:192.168.3.1"));
        assert!(docked.contains("vEthernet (External Switch):192.168.4.1"));
        assert!(path_was_lost(&docked, &undocked));
        assert_eq!(changes_from(Some(&docked), &undocked), (true, true));
    }

    #[test]
    fn a_rotated_v6_prefix_is_a_lost_path() {
        let before = fingerprint(&["eth0:10.0.0.2", "eth0:2a02:1:2:3::/64"]);
        let rotated = fingerprint(&["eth0:10.0.0.2", "eth0:2a02:1:2:4::/64"]);
        let v4_readdressed = fingerprint(&["eth0:10.0.0.9", "eth0:2a02:1:2:3::/64"]);
        let v6_only_before = fingerprint(&["eth0:2a02:1:2:3::/64"]);
        let v6_only_rotated = fingerprint(&["eth0:2a02:1:2:4::/64"]);

        assert!(path_was_lost(&before, &rotated));
        assert!(path_was_lost(&before, &v4_readdressed));
        assert!(path_was_lost(&v6_only_before, &v6_only_rotated));
        assert!(!path_was_lost(&v6_only_before, &before));
    }

    #[test]
    fn the_first_listing_after_a_failed_start_is_a_change_but_never_a_lost_path() {
        let addresses = fingerprint(&["eth0:10.0.0.2"]);
        let nothing = fingerprint(&[]);

        assert_eq!(changes_from(None, &addresses), (true, false));
        assert_eq!(changes_from(None, &nothing), (false, false));
        assert_eq!(changes_from(Some(&addresses), &addresses), (false, false));
        assert_eq!(changes_from(Some(&nothing), &addresses), (true, false));
        assert_eq!(changes_from(Some(&addresses), &nothing), (true, true));
    }

    #[tokio::test]
    async fn every_empty_rule_set_gets_its_own_timeout() {
        let empty: Vec<std::string::String> = (0..15).map(|i| format!("set{i}")).collect();
        let asked = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = std::sync::Arc::clone(&asked);
        let each = Duration::from_millis(20);
        let started = std::time::Instant::now();

        let attempted = ask_for_each_rule_set(empty, each, move |_name| {
            let counter = std::sync::Arc::clone(&counter);
            async move {
                counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                tokio::time::sleep(Duration::from_secs(30)).await;
            }
        })
        .await;

        assert_eq!(attempted, 15);
        assert_eq!(asked.load(std::sync::atomic::Ordering::SeqCst), 15);
        assert!(started.elapsed() < each * 15 * 3);
    }

    #[test]
    fn the_sleep_threshold_leaves_room_for_a_busy_machine() {
        assert!(SLEEP_SLACK > timing::ENVIRONMENT_TICK);
    }

    #[test]
    fn a_missing_clock_reading_is_never_a_sleep() {
        assert!(!slept_through(sleep_gap(
            Duration::from_secs(3600),
            None,
            Some(Duration::from_secs(1))
        )));
        assert!(!slept_through(sleep_gap(
            Duration::from_secs(3600),
            Some(Duration::from_secs(1)),
            None
        )));
    }

    #[test]
    fn a_span_is_spelled_out_for_a_person() {
        assert_eq!(spelled_out(Duration::ZERO), "0s");
        assert_eq!(spelled_out(Duration::from_secs(9)), "9s");
        assert_eq!(spelled_out(Duration::from_secs(59)), "59s");
        assert_eq!(spelled_out(Duration::from_secs(60)), "1m 0s");
        assert_eq!(spelled_out(Duration::from_secs(75)), "1m 15s");
        assert_eq!(spelled_out(Duration::from_secs(3599)), "59m 59s");
        assert_eq!(spelled_out(Duration::from_secs(3600)), "1h 0m");
        assert_eq!(spelled_out(Duration::from_secs(7325)), "2h 2m");
    }

    #[test]
    fn a_wait_shorter_than_a_second_is_not_worth_a_number() {
        assert_eq!(worth_spelling_out(Duration::ZERO), None);
        assert_eq!(worth_spelling_out(Duration::from_millis(999)), None);
        assert_eq!(worth_spelling_out(Duration::from_secs(1)), Some(Duration::from_secs(1)));
        assert_eq!(
            worth_spelling_out(Duration::from_secs(45)),
            Some(Duration::from_secs(45))
        );
    }

    #[test]
    fn the_measured_sleep_is_what_gets_printed() {
        let tick = timing::ENVIRONMENT_TICK;
        let hour = Duration::from_secs(3600);

        #[cfg(any(target_os = "linux", target_os = "macos"))]
        let gap = sleep_gap(tick, Some(Duration::ZERO), Some(hour));
        #[cfg(target_os = "windows")]
        let gap = sleep_gap(hour, Some(Duration::ZERO), Some(tick));
        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        let gap = sleep_gap(hour, Some(Duration::ZERO), Some(tick));

        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        assert_eq!(gap, Some(Duration::ZERO));
        #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
        assert_eq!(gap, Some(hour - tick));
        assert_eq!(sleep_gap(tick, None, Some(hour)), None);
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn sleep_is_what_the_awake_clock_missed() {
        let tick = timing::ENVIRONMENT_TICK;
        assert!(!slept_through(sleep_gap(tick, Some(Duration::ZERO), Some(tick))));
        assert!(!slept_through(sleep_gap(
            Duration::from_secs(45),
            Some(Duration::ZERO),
            Some(Duration::from_secs(45))
        )));
        assert!(slept_through(sleep_gap(
            tick,
            Some(Duration::ZERO),
            Some(Duration::from_secs(3600))
        )));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn sleep_is_what_the_awake_clock_missed() {
        let tick = timing::ENVIRONMENT_TICK;
        assert!(!slept_through(sleep_gap(tick, Some(Duration::ZERO), Some(tick))));
        assert!(!slept_through(sleep_gap(
            Duration::from_secs(45),
            Some(Duration::ZERO),
            Some(Duration::from_secs(45))
        )));
        assert!(slept_through(sleep_gap(
            Duration::from_secs(3600),
            Some(Duration::ZERO),
            Some(tick)
        )));
    }

    #[test]
    fn only_a_routable_address_counts_as_a_network() {
        assert!(v4_carries_traffic(Ipv4Addr::new(192, 168, 1, 10)));
        assert!(!v4_carries_traffic(Ipv4Addr::LOCALHOST));
        assert!(!v4_carries_traffic(Ipv4Addr::UNSPECIFIED));
        assert!(!v4_carries_traffic(Ipv4Addr::new(169, 254, 3, 7)));

        assert!(v6_carries_traffic(Ipv6Addr::new(0x2a02, 1, 2, 3, 4, 5, 6, 7)));
        assert!(!v6_carries_traffic(Ipv6Addr::LOCALHOST));
        assert!(!v6_carries_traffic(Ipv6Addr::UNSPECIFIED));
        assert!(!v6_carries_traffic(Ipv6Addr::new(0xfe80, 0, 0, 0, 1, 2, 3, 4)));
    }

    #[test]
    fn a_sandbox_adapter_is_not_a_path_of_its_own() {
        for name in [
            "docker0",
            "docker_gwbridge",
            "vEthernet (WSL (Hyper-V firewall))",
            "vEthernet (Default Switch)",
            "vboxnet0",
            "virbr0",
            "VMware Network Adapter VMnet8",
            "br-3f2a1b9c8d7e",
            "br-0123456789ab",
            "podman0",
            "cni-podman1",
            "lxdbr0",
            "incusbr0",
            "cni0",
            "vnic0",
            "vnic1",
            "vEthernet (nat)",
        ] {
            assert!(is_a_local_sandbox_adapter(name), "{name} должен считаться песочницей");
        }

        for name in [
            "eth0",
            "wlan0",
            "Ethernet 2",
            "Wi-Fi",
            "en0",
            "bridge0",
            "br-lan",
            "br0",
            "br-guest",
            "br-3f2a1b9c8d7",
            "br-3f2a1b9c8d7ef",
            "vEthernet (External Switch)",
            "cnifoo",
            "vnic",
        ] {
            assert!(!is_a_local_sandbox_adapter(name), "{name} несёт путь наружу");
        }
    }

    #[test]
    fn a_transition_tunnel_is_not_a_path_of_its_own() {
        let teredo = Ipv6Addr::new(0x2001, 0, 0x0a0a, 0x0a0a, 0, 0, 0, 1);
        let six_to_four = Ipv6Addr::new(0x2002, 0xc000, 0x0204, 0, 0, 0, 0, 1);
        let isatap = Ipv6Addr::new(0x2a02, 1, 2, 3, 0, 0x5efe, 0xc0a8, 0x0105);
        let isatap_public = Ipv6Addr::new(0x2a02, 1, 2, 3, 0x0200, 0x5efe, 0xc0a8, 0x0105);

        assert!(v6_is_transition_tunnel(teredo));
        assert!(v6_is_transition_tunnel(six_to_four));
        assert!(v6_is_transition_tunnel(isatap));
        assert!(v6_is_transition_tunnel(isatap_public));

        assert!(!v6_carries_traffic(teredo));
        assert!(!v6_carries_traffic(six_to_four));
        assert!(!v6_carries_traffic(isatap));
        assert!(!v6_carries_traffic(isatap_public));

        // Соседние адреса из тех же диапазонов остаются обычной сетью.
        assert!(!v6_is_transition_tunnel(Ipv6Addr::new(
            0x2001, 0x0db8, 0, 0, 0, 0, 0, 1
        )));
        assert!(!v6_is_transition_tunnel(Ipv6Addr::new(0x2003, 1, 2, 3, 4, 5, 6, 7)));
        assert!(v6_carries_traffic(Ipv6Addr::new(0x2001, 0x0db8, 0, 0, 0, 0, 0, 1)));
    }
}
