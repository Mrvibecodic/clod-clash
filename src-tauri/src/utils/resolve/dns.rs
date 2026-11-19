#![cfg(target_os = "macos")]

use clash_verge_logging::{Type, logging};
use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

const STATE_FILE: &str = "original_dns.txt";
const SCRIPT_TIMEOUT: Duration = Duration::from_secs(10);
const LOCK_HANDOVER: Duration = Duration::from_secs(2);
const OVERRIDE_HOLD: Duration = Duration::from_secs(SCRIPT_TIMEOUT.as_secs() + LOCK_HANDOVER.as_secs());

/// Меньше этого обратному скрипту давать незачем: bash с `networksetup` за
/// такой срок могут не успеть, скрипт был бы убит на полпути, а в журнал ушло
/// бы «не уложился» вместо «не запускали».
const SCRIPT_NEEDS_AT_LEAST: Duration = Duration::from_secs(1);

const fn no_longer_than(limit: Duration, ceiling: Duration) -> Duration {
    if limit.as_nanos() <= ceiling.as_nanos() {
        limit
    } else {
        ceiling
    }
}

/// clod:dns-exit — потолок шага на выходе даёт вызывающий: он считает его из
/// соседей своего пути уборки, чтобы не DNS решал, сколько длится выход.
/// Потолок НЕ делится на доли: у двух ожиданий разные источники истины.
/// Ожидание замка равно `OVERRIDE_HOLD` — столько идущая подмена может его
/// держать; потолок короче этого срока зависшую подмену не пережидает, и
/// возврат DNS тогда достаётся следующему запуску. Обратному скрипту достаётся `SCRIPT_TIMEOUT` — его
/// собственный таймаут, тот же, что и вне выхода. Разделить потолок значит
/// отнять у скрипта время в единственном случае, который бывает на деле, —
/// когда замок свободен. Поэтому потолок работает сроком на весь шаг, и
/// уступает ему тот, кто в этом шаге оказался последним.
const fn exit_lock_wait(ceiling: Duration) -> Duration {
    no_longer_than(OVERRIDE_HOLD, ceiling)
}

/// Сколько достаётся обратному скрипту; `None` — остатка не хватит и на запуск.
const fn exit_script_time(ceiling: Duration, waited: Duration) -> Option<Duration> {
    let left = no_longer_than(SCRIPT_TIMEOUT, ceiling.saturating_sub(waited));
    if left.as_nanos() < SCRIPT_NEEDS_AT_LEAST.as_nanos() {
        None
    } else {
        Some(left)
    }
}

pub const OVERRIDE_SERVER: &str = "94.140.14.140";

/// Адрес подмены прежних версий: оставленный ими след возвращается так же, как свой.
const EARLIER_OVERRIDE_SERVER: &str = "114.114.114.114";

static OVERRIDE_LOCK: Mutex<()> = Mutex::const_new(());

/// Номер решения о подмене и номер уже применённого.
///
/// clod:dns-order — решения выполняются отдельными задачами, и планировщик
/// вправе поменять их местами: без номеров устаревшее «включить» перебивало бы
/// свежее «выключить» и оставляло подмену при снятом туннеле.
static TICKETS: AtomicU64 = AtomicU64::new(0);
static APPLIED_TICKET: AtomicU64 = AtomicU64::new(0);

/// Подтверждено ли, что подмена реально применена, — и для какой сети.
///
/// clod:dns-half — файла состояния для этого мало: скрипт записывает прежние
/// адреса ДО того, как подменит их, и убитый по таймауту оставляет след при
/// нетронутом системном DNS. Без этого признака приложение считало бы дело
/// сделанным и больше никогда подмену не поставило.
///
/// Подмена стоит на той сетевой службе, что была первичной при постановке.
/// Первичной стала другая (Wi-Fi → кабель) — у неё свои адреса DNS, и запросы
/// имён идут на них мимо туннеля; на той ли службе стоит подмена, знает только
/// скрипт. Поэтому подтверждение привязано к номеру сети: скрипт подтверждает
/// ту сеть, которую застал перед запуском, и начатый до смены сети новую не
/// подтвердит, сколько бы он ни шёл.
static OVERRIDE_CONFIRMED: Confirmation = Confirmation::new();

struct Confirmation {
    network: AtomicU64,
    confirmed_for: AtomicU64,
}

impl Confirmation {
    const fn new() -> Self {
        Self {
            network: AtomicU64::new(1),
            confirmed_for: AtomicU64::new(0),
        }
    }

    fn the_network_changed(&self) {
        self.network.fetch_add(1, Ordering::SeqCst);
    }

    fn network(&self) -> u64 {
        self.network.load(Ordering::SeqCst)
    }

    fn set(&self, network_before_the_script: u64, done: bool) {
        self.confirmed_for
            .store(if done { network_before_the_script } else { 0 }, Ordering::SeqCst);
    }

    fn withdraw(&self) {
        self.confirmed_for.store(0, Ordering::SeqCst);
    }

    fn stands(&self) -> bool {
        self.confirmed_for.load(Ordering::SeqCst) == self.network()
    }
}

/// Чего от подмены хотят. `built` — заявка конфига в принятом слоте: её пишут
/// и перед перезапуском ядра, раньше, чем оно конфиг приняло, а при отказе
/// возвращают прежнюю. `applied` — заявка, с которой ядро работает: `built` на
/// тот миг, когда ядро конфиг подтвердило. Смена сети повторяет только её.
/// Новый процесс ядра её не наследует: до его ответа неизвестно, с чем он встал.
static DESIRE: parking_lot::Mutex<Desires> = parking_lot::Mutex::new(Desires {
    built: None,
    applied: None,
});

struct Desires {
    built: Option<Desire>,
    applied: Option<Desire>,
}

impl Desires {
    /// Ядро приняло собранный конфиг.
    const fn apply_the_built(&mut self) -> Option<Desire> {
        if self.built.is_some() {
            self.applied = self.built;
        }
        self.built
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Desire {
    want_base: bool,
    shaped_fake_ip: bool,
}

/// Запомнить, чего требует собранный конфиг. Ничего не применяет.
///
/// clod:dns-applied — подмена ставится только после того, как конфиг ПРИНЯТ:
/// раньше она делалась прямо при сборке, и отвергнутый ядром конфиг оставлял
/// весь резолв машины на чужом сервере без туннеля.
pub fn remember_desire(want_base: bool, shaped_fake_ip: bool) {
    DESIRE.lock().built = Some(Desire {
        want_base,
        shaped_fake_ip,
    });
}

/// Забыть заявку — конфиг, который её принёс, не поехал.
///
/// clod:dns-applied — иначе желание от отвергнутого или заменённого запасным
/// конфига дожило бы до ближайшего удачного старта ядра и применилось бы к
/// совсем другому конфигу.
pub fn forget_desire() {
    DESIRE.lock().built = None;
}

/// Применить запомненное — после того, как ядро приняло конфиг.
pub fn apply_remembered_desire() {
    if crate::core::handle::Handle::global().is_exiting() {
        return;
    }
    // Номер выдаётся под тем же замком, что и чтение заявки: иначе вытесненный
    // между этими шагами поток унёс бы свежий номер со старым желанием.
    let taken = {
        let mut desire = DESIRE.lock();
        desire
            .apply_the_built()
            .map(|desire| (desire, take_the_newest_ticket()))
    };
    let Some((desire, ticket)) = taken else {
        return;
    };
    crate::process::AsyncHandler::spawn(move || async move {
        sync_override(ticket, desire.want_base, desire.shaped_fake_ip).await;
    });
}

/// Поднят новый процесс ядра: что он принял, скажет `apply_remembered_desire`.
pub fn a_new_core_is_starting() {
    DESIRE.lock().applied = None;
}

/// Сеть сменилась: подмена остаётся без подтверждения, и при живом ядре
/// скрипт сверяет, на той ли она службе. Без живого ядра сверка достаётся его
/// подъёму.
pub fn the_network_changed(the_core_runs: bool) {
    OVERRIDE_CONFIRMED.the_network_changed();
    if !the_core_runs || crate::core::handle::Handle::global().is_exiting() {
        return;
    }
    // Заявка и номер — под одним замком: снятая следом (`the_tunnel_is_gone`,
    // выключатель) получит номер старше и перебьёт эту.
    let taken = {
        let desire = DESIRE.lock();
        desire.applied.map(|desire| (desire, take_the_newest_ticket()))
    };
    let Some((desire, ticket)) = taken else {
        return;
    };
    crate::process::AsyncHandler::spawn(move || async move {
        sync_override(ticket, desire.want_base, desire.shaped_fake_ip).await;
    });
}

/// Туннеля больше нет: ядро сдалось или не поднялось. Подмена без него
/// уводит все имена машины мимо туннеля — снимается; ставит её снова, как и
/// при старте, следующий удачный подъём ядра.
pub fn the_tunnel_is_gone() {
    DESIRE.lock().applied = None;
    let ticket = take_the_newest_ticket();
    crate::process::AsyncHandler::spawn(move || async move {
        sync_override(ticket, false, false).await;
    });
}

fn state_path() -> Option<std::path::PathBuf> {
    crate::utils::dirs::app_home_dir().ok().map(|dir| dir.join(STATE_FILE))
}

pub fn has_pending_restore() -> bool {
    state_path().is_some_and(|path| path.exists())
}

/// Номер, который отменяет всё уже поставленное в очередь.
///
/// clod:dns-order — снятие подмены должно перебивать заявку, ждущую замок:
/// иначе устаревшая задача возвращала бы подмену уже после того, как
/// пользователь её выключил или приложение вышло.
fn take_the_newest_ticket() -> u64 {
    TICKETS.fetch_add(1, Ordering::SeqCst) + 1
}

pub async fn restore_public_dns_if_pending() {
    let ticket = take_the_newest_ticket();
    let _serialized = OVERRIDE_LOCK.lock().await;
    APPLIED_TICKET.fetch_max(ticket, Ordering::SeqCst);
    if !has_pending_restore() {
        return;
    }
    logging!(
        warn,
        Type::Config,
        "system DNS was left overridden by a previous run; restoring"
    );
    restore_public_dns_locked(SCRIPT_TIMEOUT).await;
}

async fn sync_override(ticket: u64, want_base: bool, shaped_fake_ip: bool) {
    let _serialized = OVERRIDE_LOCK.lock().await;
    if APPLIED_TICKET.load(Ordering::SeqCst) > ticket || crate::core::handle::Handle::global().is_exiting() {
        return;
    }
    APPLIED_TICKET.store(ticket, Ordering::SeqCst);
    let overridden = has_pending_restore();
    let wanted = want_base && (shaped_fake_ip || overridden);
    if wanted {
        if !overridden || !OVERRIDE_CONFIRMED.stands() {
            set_public_dns_locked(OVERRIDE_SERVER.to_owned()).await;
        }
    } else if overridden {
        restore_public_dns_locked(SCRIPT_TIMEOUT).await;
    }
}

pub async fn restore_public_dns() -> bool {
    // Подмену выключил человек: применённой заявки больше нет, и смене сети
    // повторять нечего.
    DESIRE.lock().applied = None;
    let ticket = take_the_newest_ticket();
    let _serialized = OVERRIDE_LOCK.lock().await;
    APPLIED_TICKET.fetch_max(ticket, Ordering::SeqCst);
    if !has_pending_restore() {
        return true;
    }
    restore_public_dns_locked(SCRIPT_TIMEOUT).await
}

pub async fn restore_public_dns_before_exit(budget: Duration) -> bool {
    let started = Instant::now();
    let ticket = take_the_newest_ticket();
    let lock_wait = exit_lock_wait(budget);
    let Ok(_serialized) = tokio::time::timeout(lock_wait, OVERRIDE_LOCK.lock()).await else {
        logging!(
            warn,
            Type::Config,
            "unset system dns: the override still running did not finish within the {}s it is waited for",
            lock_wait.as_secs()
        );
        return false;
    };
    APPLIED_TICKET.fetch_max(ticket, Ordering::SeqCst);
    if !has_pending_restore() {
        return true;
    }
    let Some(script_time) = exit_script_time(budget, started.elapsed()) else {
        logging!(
            warn,
            Type::Config,
            "unset system dns: not started — the override that held the lock left less than {}s of the {}s exit ceiling",
            SCRIPT_NEEDS_AT_LEAST.as_secs(),
            budget.as_secs()
        );
        return false;
    };
    restore_public_dns_locked(script_time).await
}

async fn run_dns_script(script_name: &str, args: Vec<String>, what: &str, limit: Duration) -> bool {
    use crate::utils::dirs;
    use tokio::io::AsyncReadExt as _;

    logging!(info, Type::Config, "try to {what}");
    let resource_dir = match dirs::app_resources_dir() {
        Ok(dir) => dir,
        Err(e) => {
            logging!(error, Type::Config, "Failed to get resource directory: {}", e);
            return false;
        }
    };
    let script = resource_dir.join(script_name);
    if !script.exists() {
        logging!(error, Type::Config, "{script_name} not found");
        return false;
    }

    // Своя группа процессов: по таймауту снимается вся — и bash, и зависший в
    // нём networksetup. Иначе networksetup доделывал бы подмену или возврат
    // уже после того, как замок отпущен и решение принято.
    let mut child = match tokio::process::Command::new("bash")
        .arg(&script)
        .args(args)
        .current_dir(&resource_dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .process_group(0)
        .spawn()
    {
        Ok(child) => child,
        Err(err) => {
            logging!(error, Type::Config, "{what} failed: {err}");
            return false;
        }
    };
    // Номер группы — сразу: после выхода bash `child.id()` его уже не отдаст,
    // а держать трубу может оставшийся в группе процесс.
    let group = child.id().and_then(|pid| libc::pid_t::try_from(pid).ok());

    // Трубы читаются вместе с ожиданием и под тем же сроком: болтливый скрипт
    // не встанет на полной трубе, а чужой держатель трубы не продлит замок.
    let (mut out, mut err) = (child.stdout.take(), child.stderr.take());
    let (mut said, mut said_err) = (Vec::new(), Vec::new());
    let finished = tokio::time::timeout(limit, async {
        let read_out = async {
            if let Some(out) = out.as_mut() {
                let _ = out.read_to_end(&mut said).await;
            }
        };
        let read_err = async {
            if let Some(err) = err.as_mut() {
                let _ = err.read_to_end(&mut said_err).await;
            }
        };
        tokio::join!(child.wait(), read_out, read_err).0
    })
    .await;

    // Что скрипт сказал о причине: отказ networksetup, исчезнувшая служба,
    // неудачная запись файла — и на каком шаге он встал, если не уложился.
    said.extend(said_err);
    for line in std::string::String::from_utf8_lossy(&said).lines() {
        logging!(warn, Type::Config, "{what}: {line}");
    }

    match finished {
        Err(_) => {
            logging!(error, Type::Config, "{what} timed out");
            if let Some(group) = group
                // SAFETY: сигнал группе, которую процесс возглавляет сам (`process_group(0)`).
                && unsafe { libc::killpg(group, libc::SIGKILL) } != 0
            {
                logging!(
                    error,
                    Type::Config,
                    "{what} could not be stopped: {}",
                    std::io::Error::last_os_error()
                );
            }
            let _ = child.wait().await;
            false
        }
        Ok(Ok(status)) if status.success() => {
            logging!(info, Type::Config, "{what} successfully");
            true
        }
        Ok(Ok(status)) => {
            logging!(error, Type::Config, "{what} failed: {status}");
            false
        }
        Ok(Err(err)) => {
            logging!(error, Type::Config, "{what} failed: {err}");
            false
        }
    }
}

fn state_arg() -> String {
    state_path()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default()
}

async fn set_public_dns_locked(dns_server: String) -> bool {
    // Файл состояния не трогаем даже при отказе: в нём записаны прежние адреса,
    // и потерять их хуже, чем повторить попытку. Скрипт применяет подмену
    // повторно без вреда, а `OVERRIDE_CONFIRMED` не даст счесть дело сделанным.
    let network = OVERRIDE_CONFIRMED.network();
    let done = run_dns_script(
        "set_dns.sh",
        vec![dns_server, state_arg(), EARLIER_OVERRIDE_SERVER.to_owned()],
        "set system dns",
        SCRIPT_TIMEOUT,
    )
    .await;
    OVERRIDE_CONFIRMED.set(network, done);
    done
}

async fn restore_public_dns_locked(limit: Duration) -> bool {
    let done = run_dns_script(
        "unset_dns.sh",
        vec![
            state_arg(),
            OVERRIDE_SERVER.to_owned(),
            EARLIER_OVERRIDE_SERVER.to_owned(),
        ],
        "unset system dns",
        limit,
    )
    .await;
    if done {
        OVERRIDE_CONFIRMED.withdraw();
    }
    done
}

#[cfg(test)]
mod tests {
    use super::{
        Confirmation, Desire, Desires, SCRIPT_NEEDS_AT_LEAST, SCRIPT_TIMEOUT, exit_lock_wait, exit_script_time,
    };
    use crate::feat::ExitPace;
    use std::time::Duration;

    #[test]
    fn an_override_is_confirmed_only_for_the_network_its_script_started_in() {
        let confirmation = Confirmation::new();
        assert!(!confirmation.stands(), "nothing was set yet");

        let seen = confirmation.network();
        confirmation.set(seen, true);
        assert!(confirmation.stands());

        confirmation.the_network_changed();
        assert!(!confirmation.stands(), "the service may be another one now");

        // Скрипт начат до смены сети, закончил после: новую сеть не подтверждает.
        let before_the_change = confirmation.network();
        confirmation.the_network_changed();
        confirmation.set(before_the_change, true);
        assert!(!confirmation.stands());

        let seen = confirmation.network();
        confirmation.set(seen, false);
        assert!(!confirmation.stands(), "a script that failed confirms nothing");
        confirmation.set(seen, true);
        assert!(confirmation.stands());
        confirmation.withdraw();
        assert!(!confirmation.stands());
    }

    #[test]
    fn a_built_desire_counts_as_applied_only_once_the_core_took_it() {
        let wanted = Desire {
            want_base: true,
            shaped_fake_ip: true,
        };
        let unwanted = Desire {
            want_base: false,
            shaped_fake_ip: false,
        };
        let mut desires = Desires {
            built: None,
            applied: None,
        };
        assert_eq!(desires.apply_the_built(), None);
        assert_eq!(desires.applied, None);

        desires.built = Some(wanted);
        assert_eq!(desires.applied, None, "built, not yet taken by the core");
        assert_eq!(desires.apply_the_built(), Some(wanted));
        assert_eq!(desires.applied, Some(wanted));

        // Следующий конфиг собран, ядро его ещё не приняло — или отвергло.
        desires.built = Some(unwanted);
        assert_eq!(desires.applied, Some(wanted));
        desires.built = None;
        assert_eq!(desires.apply_the_built(), None);
        assert_eq!(desires.applied, Some(wanted), "the core still runs the config it took");
    }

    /// Смена сети снимает подтверждение при любом исходе; повторяет она только
    /// применённую заявку и только при живом ядре. Туннель ушёл или подмену
    /// выключили — применённой заявки нет, и снята она раньше выдачи номера.
    #[test]
    fn a_changed_network_repeats_only_what_was_applied_and_only_to_a_core_that_runs() {
        let own = crate::utils::source_scan::production_code(include_str!("dns.rs"));
        let body = |signature: &str| crate::utils::source_scan::fn_body(own, signature).unwrap_or_default();

        let changed = body("pub fn the_network_changed(");
        let doubted = changed
            .find("OVERRIDE_CONFIRMED.the_network_changed()")
            .unwrap_or(usize::MAX);
        let asked = changed.find("if !the_core_runs || ").unwrap_or(usize::MAX);
        let numbered = changed
            .find("desire.applied.map(|desire| (desire, take_the_newest_ticket()))")
            .unwrap_or(usize::MAX);
        let applied = changed.find("sync_override(ticket").unwrap_or_default();
        assert!(doubted < asked && asked < numbered && numbered < applied, "{changed}");
        assert!(!changed.contains(".built"), "{changed}");

        for gone in ["pub fn the_tunnel_is_gone(", "pub async fn restore_public_dns("] {
            let gone = body(gone);
            let forgotten = gone.find("DESIRE.lock().applied = None;").unwrap_or(usize::MAX);
            let numbered = gone.find("take_the_newest_ticket()").unwrap_or_default();
            assert!(forgotten < numbered, "{gone}");
        }
        assert!(
            body("pub fn a_new_core_is_starting(").contains("DESIRE.lock().applied = None;"),
            "{own}"
        );
        let manager = crate::utils::source_scan::production_code(include_str!("../../core/manager/mod.rs"));
        let spawned = crate::utils::source_scan::fn_body(manager, "fn note_core_is_up(").unwrap_or_default();
        let forgotten = spawned.find("dns::a_new_core_is_starting()").unwrap_or(usize::MAX);
        let alive = spawned.find("Liveness::Up as u8").unwrap_or_default();
        assert!(forgotten < alive, "{spawned}");

        let set = body("async fn set_public_dns_locked(");
        let seen = set
            .find("let network = OVERRIDE_CONFIRMED.network()")
            .unwrap_or(usize::MAX);
        let ran = set.find("run_dns_script(").unwrap_or(usize::MAX);
        let confirmed = set.find("OVERRIDE_CONFIRMED.set(network, done)").unwrap_or_default();
        assert!(seen < ran && ran < confirmed, "{set}");

        let watchdog = crate::utils::source_scan::without_test_modules(include_str!("../../feat/environment.rs"));
        let reconcile = crate::utils::source_scan::fn_body(watchdog, "async fn reconcile(").unwrap_or_default();
        let asked = reconcile
            .split_once("dns::the_network_changed(")
            .and_then(|(_, rest)| rest.split_once(");"))
            .map_or("", |(asked, _)| asked);
        assert!(
            asked.contains("network_carries_traffic") && asked.contains("RunningMode::NotRunning"),
            "{reconcile}"
        );
    }

    /// Скрипт подмены: без маршрута по умолчанию службу не ищет, а подмену,
    /// уже стоящую на первичной службе, не снимает и не ставит заново.
    #[test]
    fn the_script_moves_the_override_only_when_it_is_not_where_the_network_is() {
        let script = include_str!("../../../../scripts/set_dns.sh");
        let at = |piece: &str| script.find(piece).unwrap_or(usize::MAX);
        let routeless = at(r#"[ -z "$nic" ] && echo "no default route" && exit 1"#);
        let resolved = at("hardware_port=$(networksetup -listnetworkserviceorder");
        let in_place = at(r#"[ "$(head -n 1 "$state_file")" == "$hardware_port" ]"#);
        let stands =
            at(r#"networksetup -getdnsservers "$hardware_port" 2>/dev/null | tr -d '[:space:]')" == "$1" ]; then"#);
        let returned = at(r#"unset_dns.sh" "$state_file""#);
        assert!(
            routeless < resolved && resolved < in_place && in_place < stands && stands < returned,
            "{script}"
        );
        assert!(returned < usize::MAX, "{script}");
    }

    /// Потолки обычного выхода — с отменой и без неё.
    const fn calm() -> [Duration; 2] {
        ExitPace::Interactive.dns_ceilings()
    }

    /// Все потолки, которые даёт выход, плюс заведомо тесные.
    fn every_ceiling() -> Vec<Duration> {
        let mut all = calm().to_vec();
        all.extend(ExitPace::SessionEnding.dns_ceilings());
        all.extend([Duration::from_secs(2), Duration::from_secs(1)]);
        all
    }

    /// Докуда идущая подмена может додержать замок: `run_dns_script` снимает
    /// зависшего ребёнка только по своему таймеру.
    const A_HUNG_OVERRIDE_HOLDS_THE_LOCK_FOR: Duration = SCRIPT_TIMEOUT;

    #[test]
    fn the_reverse_script_keeps_its_own_timeout_while_the_ceiling_allows_it() {
        for ceiling in calm() {
            assert_eq!(exit_script_time(ceiling, Duration::ZERO), Some(SCRIPT_TIMEOUT));
        }
        for ceiling in ExitPace::SessionEnding.dns_ceilings() {
            assert!(ceiling < SCRIPT_TIMEOUT);
            assert_eq!(exit_script_time(ceiling, Duration::ZERO), Some(ceiling));
        }
    }

    #[test]
    fn the_wait_outlasts_an_override_that_holds_the_lock_to_its_last_second() {
        let longest = calm().into_iter().max().unwrap_or_default();
        assert!(exit_lock_wait(longest) > A_HUNG_OVERRIDE_HOLDS_THE_LOCK_FOR);
    }

    #[test]
    fn the_two_limits_are_not_a_split_of_the_ceiling() {
        let longest = calm().into_iter().max().unwrap_or_default();
        assert!(exit_lock_wait(longest) + SCRIPT_TIMEOUT > longest);
    }

    #[test]
    fn the_step_never_outlives_the_ceiling_it_is_given() {
        for ceiling in every_ceiling() {
            let waited = exit_lock_wait(ceiling);
            assert!(waited <= ceiling);
            assert!(waited + exit_script_time(ceiling, waited).unwrap_or_default() <= ceiling);
        }
    }

    #[test]
    fn a_ceiling_of_a_second_or_two_starves_neither_the_wait_nor_the_script() {
        for ceiling in [Duration::from_secs(1), Duration::from_secs(2)] {
            assert!(!exit_lock_wait(ceiling).is_zero());
            assert_eq!(exit_script_time(ceiling, Duration::ZERO), Some(ceiling));
        }
    }

    #[test]
    fn a_remainder_too_short_to_reach_the_work_does_not_start_the_script() {
        for ceiling in calm() {
            let almost_all = ceiling - Duration::from_millis(50);
            assert_eq!(exit_script_time(ceiling, almost_all), None);
            assert_eq!(exit_script_time(ceiling, ceiling), None);
            assert_eq!(
                exit_script_time(ceiling, ceiling - SCRIPT_NEEDS_AT_LEAST),
                Some(SCRIPT_NEEDS_AT_LEAST)
            );
        }
    }
}
