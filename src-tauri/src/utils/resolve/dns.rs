#![cfg(target_os = "macos")]

use clash_verge_logging::{Type, logging};
use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};
use tokio::sync::Mutex;

const STATE_FILE: &str = "original_dns.txt";
const SCRIPT_TIMEOUT: Duration = Duration::from_secs(10);

/// Потолок всего шага на выходе: сначала пережидается идущая подмена, потом своё
/// время получает обратный скрипт.
///
/// clod:dns-exit — ожидание замка должно быть заведомо ДЛИННЕЕ `SCRIPT_TIMEOUT`:
/// замок держится от начала подмены до её конца, то есть дольше, чем работает
/// сам скрипт, и при равных числах зависшую подмену выход не переждал бы никогда
/// — обратный скрипт не запускался бы вовсе именно в том случае, ради которого
/// ожидание и заведено. При этом сумма не выходит за потолок интерактивного
/// выхода, который и без DNS равен восемнадцати секундам (снятие туннеля,
/// ожидание замка жизненного цикла, остановка ядра и опрос службы).
pub const RESTORE_BUDGET: Duration = Duration::from_secs(18);

/// Сколько из потолка достаётся самому обратному скрипту. Типовая подмена
/// отрабатывает за секунду-две, десять секунд — это уже зависшая.
const RESTORE_SCRIPT_SHARE: Duration = Duration::from_secs(6);

/// Как потолок шага делится между ожиданием чужой подмены и обратным скриптом.
const fn exit_shares(ceiling: Duration) -> (Duration, Duration) {
    let ceiling = ceiling.as_secs();
    let share = RESTORE_SCRIPT_SHARE.as_secs();
    if ceiling <= share {
        let wait = ceiling / 3;
        return (Duration::from_secs(wait), Duration::from_secs(ceiling - wait));
    }
    (Duration::from_secs(ceiling - share), Duration::from_secs(share))
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
    let ticket = take_the_newest_ticket();
    let (lock_wait, script_time) = exit_shares(budget);
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
    match tokio::time::timeout(script_time, restore_public_dns_locked(script_time)).await {
        Ok(done) => done,
        Err(_) => {
            logging!(
                warn,
                Type::Config,
                "unset system dns did not finish within the {}s it is given after the lock",
                script_time.as_secs()
            );
            false
        }
    }
}

async fn run_dns_script(script_name: &str, args: Vec<String>, what: &str, limit: Duration) -> bool {
    use crate::{core::handle, utils::dirs};
    use tauri_plugin_shell::{ShellExt as _, process::CommandEvent};

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

    let mut command_args = Vec::with_capacity(args.len() + 1);
    command_args.push(script.to_string_lossy().into_owned());
    command_args.extend(args);

    let spawned = handle::Handle::app_handle()
        .shell()
        .command("bash")
        .args(command_args)
        .current_dir(resource_dir)
        .spawn();
    let (mut events, child) = match spawned {
        Ok(spawned) => spawned,
        Err(err) => {
            logging!(error, Type::Config, "{what} failed: {err}");
            return false;
        }
    };

    let terminated = async {
        while let Some(event) = events.recv().await {
            if let CommandEvent::Terminated(payload) = event {
                return payload.code;
            }
        }
        None
    };

    match tokio::time::timeout(limit, terminated).await {
        Err(_) => {
            logging!(error, Type::Config, "{what} timed out");
            if let Err(err) = child.kill() {
                logging!(error, Type::Config, "{what} could not be stopped: {err}");
            }
            false
        }
        Ok(Some(0)) => {
            logging!(info, Type::Config, "{what} successfully");
            true
        }
        Ok(code) => {
            logging!(error, Type::Config, "{what} failed: {}", code.unwrap_or(-1));
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
    let done = run_dns_script("unset_dns.sh", vec![state_arg()], "unset system dns", limit).await;
    if done {
        OVERRIDE_CONFIRMED.store(false, Ordering::SeqCst);
    }
    done
}

#[cfg(test)]
mod tests {
    use super::{RESTORE_BUDGET, RESTORE_SCRIPT_SHARE, SCRIPT_TIMEOUT, exit_shares};
    use std::time::Duration;

    /// Сколько подмена занимает, когда всё в порядке.
    const A_TYPICAL_OVERRIDE_RUN: Duration = Duration::from_secs(2);

    /// Потолок интерактивного выхода без DNS: снятие туннеля (3) + ожидание
    /// замка жизненного цикла (5) + остановка ядра (5) + опрос службы (5).
    const THE_EXIT_CEILING_WITHOUT_DNS: Duration = Duration::from_secs(18);

    #[test]
    fn the_wait_and_the_script_split_one_ceiling_that_outlasts_a_hung_override() {
        let (lock_wait, script_time) = exit_shares(RESTORE_BUDGET);

        assert_eq!(lock_wait + script_time, RESTORE_BUDGET);
        assert_eq!(script_time, RESTORE_SCRIPT_SHARE);
        // Замок держится дольше самого скрипта: `run_dns_script` ещё ищет ресурсы,
        // запускает `bash` и убивает зависшего ребёнка после таймера.
        assert!(lock_wait > SCRIPT_TIMEOUT);
        assert!(script_time >= A_TYPICAL_OVERRIDE_RUN * 2);
        assert!(script_time < SCRIPT_TIMEOUT);
        assert!(RESTORE_BUDGET <= THE_EXIT_CEILING_WITHOUT_DNS);
    }

    #[test]
    fn a_ceiling_shorter_than_the_script_share_still_leaves_both_some_time() {
        let hurried = Duration::from_secs(3);
        let (lock_wait, script_time) = exit_shares(hurried);

        assert_eq!(lock_wait + script_time, hurried);
        assert!(!lock_wait.is_zero());
        assert!(script_time > lock_wait);
    }
}
