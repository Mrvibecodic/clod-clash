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

pub const OVERRIDE_SERVER: &str = "114.114.114.114";

static OVERRIDE_LOCK: Mutex<()> = Mutex::const_new(());

/// Номер решения о подмене и номер уже применённого.
///
/// clod:dns-order — решения выполняются отдельными задачами, и планировщик
/// вправе поменять их местами: без номеров устаревшее «включить» перебивало бы
/// свежее «выключить» и оставляло подмену при снятом туннеле.
static TICKETS: AtomicU64 = AtomicU64::new(0);
static APPLIED_TICKET: AtomicU64 = AtomicU64::new(0);

/// Подтверждено ли, что подмена реально применена.
///
/// clod:dns-half — файла состояния для этого мало: скрипт записывает прежние
/// адреса ДО того, как подменит их, и убитый по таймауту оставляет след при
/// нетронутом системном DNS. Без этого признака приложение считало бы дело
/// сделанным и больше никогда подмену не поставило.
static OVERRIDE_CONFIRMED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Чего от подмены хотят по последнему собранному конфигу.
static DESIRE: parking_lot::Mutex<Option<Desire>> = parking_lot::Mutex::new(None);

#[derive(Clone, Copy)]
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
    *DESIRE.lock() = Some(Desire {
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
    *DESIRE.lock() = None;
}

/// Применить запомненное — после того, как ядро приняло конфиг.
pub fn apply_remembered_desire() {
    if crate::core::handle::Handle::global().is_exiting() {
        return;
    }
    // Номер выдаётся под тем же замком, что и чтение заявки: иначе вытесненный
    // между этими шагами поток унёс бы свежий номер со старым желанием.
    let taken = {
        let desire = DESIRE.lock();
        (*desire).map(|desire| (desire, take_the_newest_ticket()))
    };
    let Some((desire, ticket)) = taken else {
        return;
    };
    crate::process::AsyncHandler::spawn(move || async move {
        sync_override(ticket, desire.want_base, desire.shaped_fake_ip).await;
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
        if !overridden || !OVERRIDE_CONFIRMED.load(Ordering::SeqCst) {
            set_public_dns_locked(OVERRIDE_SERVER.to_owned()).await;
        }
    } else if overridden {
        restore_public_dns_locked(SCRIPT_TIMEOUT).await;
    }
}

pub async fn restore_public_dns() -> bool {
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
    let done = run_dns_script(
        "set_dns.sh",
        vec![dns_server, state_arg()],
        "set system dns",
        SCRIPT_TIMEOUT,
    )
    .await;
    OVERRIDE_CONFIRMED.store(done, Ordering::SeqCst);
    done
}

async fn restore_public_dns_locked(limit: Duration) -> bool {
    let done = run_dns_script(
        "unset_dns.sh",
        vec![state_arg(), OVERRIDE_SERVER.to_owned()],
        "unset system dns",
        limit,
    )
    .await;
    if done {
        OVERRIDE_CONFIRMED.store(false, Ordering::SeqCst);
    }
    done
}

#[cfg(test)]
mod tests {
    use super::{SCRIPT_NEEDS_AT_LEAST, SCRIPT_TIMEOUT, exit_lock_wait, exit_script_time};
    use crate::feat::ExitPace;
    use std::time::Duration;

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
