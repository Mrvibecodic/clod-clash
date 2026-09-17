use crate::config::Config;
use crate::core::{
    CoreManager, handle,
    manager::{ExitStop, RunningMode},
    sysopt,
};
use crate::module::lightweight;
use crate::process::AsyncHandler;
use crate::utils;
use crate::utils::window_manager::WindowManager;
use clash_verge_logging::{Type, logging};
use tokio::time::{Duration, timeout};

pub async fn open_or_close_dashboard() {
    if lightweight::is_in_lightweight_mode() {
        let _ = lightweight::exit_lightweight_mode().await;
        return;
    }

    let result = WindowManager::toggle_main_window().await;
    logging!(info, Type::Window, "Window toggle result: {result:?}");
}

/// clod:exit-pace — у выхода два темпа.
///
/// Обычный выход человек видит: он нажал «Выход» или закрыл окно и подождёт
/// секунду-другую, пока мы честно снимем системный прокси. На выключении
/// компьютера и выходе из сеанса нас скоро убьют, поэтому там бюджеты
/// укорочены — задача не «сделать всё», а «успеть снять прокси».
///
/// Полутора секунд на снятие не хватало ровно в том случае, ради которого всё
/// и делается: медленный диск, много сетевых подключений, выключение
/// компьютера. Прокси оставался в системе, и до следующего запуска клиента
/// интернета не было.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExitPace {
    Interactive,
    SessionEnding,
}

impl From<clash_verge_signal::Shutdown> for ExitPace {
    fn from(shutdown: clash_verge_signal::Shutdown) -> Self {
        match shutdown {
            clash_verge_signal::Shutdown::Interactive => Self::Interactive,
            clash_verge_signal::Shutdown::SessionEnding => Self::SessionEnding,
        }
    }
}

impl ExitPace {
    /// Снятие системного прокси — самое важное, что делается на выходе.
    const fn sysproxy_budget(self) -> Duration {
        match self {
            Self::Interactive => Duration::from_secs(8),
            Self::SessionEnding => Duration::from_secs(3),
        }
    }

    /// Ни один бюджет завершения сеанса не короче прежнего безусловного: на
    /// выходе из сеанса ядро тоже надо успеть остановить, иначе оно останется
    /// жить с занятыми портами и поднятым туннелем. Задачи уборки идут
    /// параллельно на обоих путях выхода, поэтому длинный бюджет соседа
    /// снятию прокси не мешает.
    const fn tun_off_budget(self) -> Duration {
        Duration::from_secs(3)
    }

    const fn core_stop_budget(self) -> Duration {
        Duration::from_secs(5)
    }

    const fn lock_wait_budget(self) -> Duration {
        Duration::from_secs(5)
    }

    /// Сохранение настроек. Раньше шло ДО уборки и без предела вовсе: на
    /// медленном диске прокси даже не начинали снимать, пока три файла не лягут.
    const fn save_budget(self) -> Duration {
        match self {
            Self::Interactive => Duration::from_secs(10),
            Self::SessionEnding => Duration::from_secs(5),
        }
    }

    /// Ветка ядра на пути с отменой выхода: снятие туннеля, ожидание замка
    /// жизненного цикла, остановка ядра и опрос службы о живости.
    const fn core_branch_with_cancel(self) -> Duration {
        self.core_branch_without_cancel()
            .saturating_add(self.lock_wait_budget())
            .saturating_add(crate::constants::timing::SERVICE_STATUS_WAIT)
    }

    /// Ветка ядра на пути без отмены: ожидание замка входит в срок остановки.
    const fn core_branch_without_cancel(self) -> Duration {
        self.tun_off_budget().saturating_add(self.core_stop_budget())
    }

    /// clod:dns-exit — в обычном темпе потолок шага DNS равен самому долгому
    /// соседу ТОГО ЖЕ пути уборки: путей два, и ветка ядра у них разной длины.
    const fn dns_budget(self, core_branch: Duration) -> Duration {
        match self {
            Self::Interactive => longest(core_branch, longest(self.save_budget(), self.sysproxy_budget())),
            Self::SessionEnding => Duration::from_secs(3),
        }
    }
}

const fn longest(one: Duration, other: Duration) -> Duration {
    if one.as_nanos() >= other.as_nanos() { one } else { other }
}

pub async fn quit() {
    quit_at(ExitPace::Interactive, true).await;
}

/// Выход по сигналу операционной системы.
pub async fn quit_by_signal(shutdown: clash_verge_signal::Shutdown) {
    quit_at(shutdown.into(), false).await;
}

pub fn refuse_while_exiting() -> anyhow::Result<()> {
    if handle::Handle::global().is_exiting() {
        let refusal = clash_verge_i18n::t!("common.exitInProgress");
        anyhow::bail!("{refusal}");
    }
    Ok(())
}

pub async fn quit_at(pace: ExitPace, cancel_if_core_stays: bool) {
    if !handle::Handle::global().begin_exiting(cancel_if_core_stays) {
        logging!(info, Type::System, "выход уже идёт, повторный запрос пропущен");
        handle::Handle::notice_message(crate::core::notification::EXIT_REFUSAL_STATUS, "");
        return;
    }
    logging!(debug, Type::System, "запуск процесса выхода ({pace:?})");

    logging!(info, Type::System, "начало асинхронной очистки ресурсов");
    let cleanup = if cancel_if_core_stays {
        match clean_core_first(pace).await {
            Ok(cleanup) => cleanup,
            Err(reason) => {
                cancel_the_exit(reason);
                return;
            }
        }
    } else {
        utils::server::shutdown_embedded_server();
        clean_async_at(pace).await
    };

    if cleanup
        .sysproxy
        .is_some_and(|outcome| !the_take_down_went_as_asked(outcome))
    {
        // Последствие переживает выход: в системе остался НАШ прокси,
        // указывающий на порт, которого через секунду не станет. Чужой прокси,
        // до которого мы намеренно не дотронулись, переживает выход сам по
        // себе и ни о чём человека не извещает.
        logging!(
            error,
            Type::Window,
            "системный прокси остался в системе ({:?})",
            cleanup.sysproxy
        );
        // Окно умирает через миллисекунды, а очередь отложенных уведомлений —
        // память этого же процесса. Единственное, что переживает выход, —
        // уведомление операционной системы.
        utils::notification::notify_event(utils::notification::NotificationEvent::SysproxyLeftBehind).await;
        // Плагин отдаёт показ отдельной задаче и возвращается сразу; выход
        // через миллисекунду убил бы её раньше, чем демон уведомлений получит
        // сообщение. Полсекунды — только на этом, редком, пути.
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    logging!(
        info,
        Type::System,
        "очистка ресурсов завершена, код выхода: {}",
        if cleanup.all_success { 0 } else { 1 }
    );

    let app_handle = handle::Handle::app_handle();
    app_handle.exit(if cleanup.all_success { 0 } else { 1 });
}

fn cancel_the_exit(reason: String) {
    logging!(
        error,
        Type::System,
        "выход отменён: ядро не остановилось ({reason}); приложение остаётся работать"
    );
    handle::Handle::global().clear_is_exiting();
    AsyncHandler::spawn(move || async move {
        utils::notification::notify_event(utils::notification::NotificationEvent::QuitCancelled).await;
        handle::Handle::notice_message("app_quit::core_still_running", reason);
        if !lightweight::exit_lightweight_mode().await {
            WindowManager::show_main_window().await;
        }
        if let Err(error) = CoreManager::global().resume_after_a_cancelled_exit().await {
            logging!(
                error,
                Type::Core,
                "после отменённого выхода ядро не поднялось заново: {error:#}"
            );
        }
        // Уборка шла параллельно с остановкой ядра и успела снять то, что
        // приложению теперь снова нужно. Каждый шаг — пустой, если снимать
        // было нечего, а вернуть системный прокси может только живое ядро:
        // без него человеку об этом говорят вслух.
        crate::feat::tun::bring_back_after_a_cancelled_exit().await;
        if Config::verge().await.latest_arc().enable_system_proxy.unwrap_or(false) {
            if matches!(*CoreManager::global().get_running_mode(), RunningMode::NotRunning) {
                logging!(
                    error,
                    Type::Window,
                    "выход отменён, но ядра нет: системный прокси снят и вернётся, когда ядро поднимется"
                );
                handle::Handle::notice_message("sysproxy::core_not_running", "");
            }
            CoreManager::global().point_system_proxy_at_the_confirmed_port().await;
        }
        #[cfg(target_os = "macos")]
        crate::utils::resolve::dns::apply_remembered_desire();
        // Сторожа ядра и окружения выходят по флагу выхода и сами не
        // возвращаются: запуск ядра их не заводит, если ядро числится живым.
        CoreManager::global().watch_the_core_again().await;
        crate::feat::environment::spawn_environment_watchdog();
        #[cfg(target_os = "linux")]
        crate::core::tray::Tray::catch_up_after_a_cancelled_exit();
        handle::Handle::refresh_clash();
        handle::Handle::refresh_verge();
        tokio::time::sleep(Duration::from_secs(1)).await;
        crate::core::traffic_estimate::resume();
        #[cfg(target_os = "macos")]
        {
            let enable_tray_speed = Config::verge().await.latest_arc().enable_tray_speed.unwrap_or(false);
            crate::core::tray::Tray::global().update_speed_task(enable_tray_speed);
        }
    });
}

/// Перед аварийным перезапуском при зависшем окне настройки сохраняются
/// всегда. Ядро под службой перезапуск переживает, прокси и туннель остаются
/// рабочими, и снимать их значило бы пустить трафик напрямую. Во всех прочих
/// случаях прокси после нас укажет в никуда: он снимается, а своё ядро
/// останавливается — возвращённое после отменённого выхода уже не умирает
/// вместе с нами.
#[cfg(target_os = "windows")]
pub async fn tidy_up_for_a_forced_restart() -> bool {
    let pace = ExitPace::SessionEnding;
    let save = spawn_save_task(pace);
    if matches!(*CoreManager::global().get_running_mode(), RunningMode::Service) {
        return save.await.unwrap_or_default();
    }
    let stop = timeout(pace.core_stop_budget(), CoreManager::global().stop_core());
    let (saved, proxy, stopped) = tokio::join!(save, spawn_proxy_task(pace), stop);
    saved.unwrap_or_default()
        && the_take_down_went_as_asked(proxy.unwrap_or_default())
        && stopped.is_ok_and(|stopped| stopped.is_ok())
}

pub struct CleanupOutcome {
    pub all_success: bool,
    pub sysproxy: Option<ProxyAtExit>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ProxyAtExit {
    Cleared,
    LeftToItsOwner,
    OursIsStillInTheSystem,
    #[default]
    Refused,
}

const fn the_take_down_went_as_asked(outcome: ProxyAtExit) -> bool {
    matches!(outcome, ProxyAtExit::Cleared | ProxyAtExit::LeftToItsOwner)
}

pub async fn clean_async() -> bool {
    clean_async_at(ExitPace::Interactive).await.all_success
}

async fn turn_the_tun_off(pace: ExitPace) {
    logging!(info, Type::System, "disable tun");
    // Черновик, а не committed: сохранение настроек идёт теперь наравне с
    // уборкой, и committed может ещё не знать про только что включённый TUN.
    // Черновик — это то, что человек выбрал последним.
    let tun_enabled = Config::verge().await.latest_arc().enable_tun_mode.unwrap_or(false);
    if !tun_enabled {
        return;
    }
    let disable_tun = serde_json::json!({ "tun": { "enable": false } });

    logging!(info, Type::System, "send disable tun request to mihomo");
    match timeout(pace.tun_off_budget(), async {
        crate::feat::environment::detached_core_client()
            .await
            .patch_base_config(&disable_tun)
            .await
    })
    .await
    {
        Ok(Ok(_)) => {
            logging!(info, Type::Window, "режим TUN отключён");
        }
        Ok(Err(e)) => {
            logging!(warn, Type::Window, "Warning: не удалось отключить режим TUN: {e}");
        }
        Err(_) => {
            logging!(
                warn,
                Type::Window,
                "Warning: таймаут отключения режима TUN (возможно, система выключается), продолжаем выход"
            );
        }
    }
}

async fn clean_core_first(pace: ExitPace) -> Result<CleanupOutcome, String> {
    // clod:exit-order — уборка, которой ядро не нужно (настройки, системный
    // прокси, DNS), стартует сразу и идёт параллельно с остановкой ядра, как в
    // альфе 3: прокси снимается на первых секундах, а не после того, как ядро
    // отработает все свои бюджеты. Иначе зависшее ядро держало окно
    // неотвечающим до двадцати секунд, человек снимал приложение через
    // диспетчер — и прокси оставался в системе на мёртвом порту.
    //
    // Отмена выхода при живом, но неостанавливаемом ядре — редкий исход, и
    // всё, что уборка к тому моменту сняла, возвращает `cancel_the_exit`.
    // Уборку дожидаемся всегда, в том числе перед отменой: иначе её снятие
    // легло бы поверх восстановления.
    let rest = tokio::task::spawn(clean_the_rest(pace, pace.dns_budget(pace.core_branch_with_cancel())));
    let stop_budget = pace.core_stop_budget();
    let core = async {
        turn_the_tun_off(pace).await;
        logging!(info, Type::System, "stop core");
        CoreManager::global()
            .stop_core_for_exit(pace.lock_wait_budget(), stop_budget)
            .await
    };
    let (rest, stop) = tokio::join!(rest, core);
    let mut cleanup = match rest {
        Ok(cleanup) => cleanup,
        Err(error) => {
            logging!(error, Type::Window, "задача уборки не вернула результат: {error}");
            CleanupOutcome {
                all_success: false,
                sysproxy: None,
            }
        }
    };

    let core_stopped = match stop {
        ExitStop::Stopped => {
            logging!(info, Type::Window, "ядро остановлено");
            true
        }
        ExitStop::LockBusy => {
            logging!(
                warn,
                Type::Window,
                "Warning: ядро занято другой операцией и не освободилось за {} с, продолжаем выход",
                pace.lock_wait_budget().as_secs()
            );
            false
        }
        ExitStop::Failed {
            reason,
            core_alive: true,
        } => {
            logging!(warn, Type::Window, "Warning: не удалось остановить ядро: {reason}");
            return Err(reason);
        }
        ExitStop::Failed {
            reason,
            core_alive: false,
        } => {
            logging!(
                warn,
                Type::Window,
                "Warning: остановка ядра вернула ошибку ({reason}), но ядра уже нет — продолжаем выход"
            );
            false
        }
    };

    // Только после решения об отмене: встроенный сервер поднимается один раз
    // за процесс, и погасить его при отменённом выходе значило бы остаться
    // без PAC и без передачи одиночного экземпляра.
    utils::server::shutdown_embedded_server();
    cleanup.all_success = cleanup.all_success && core_stopped;
    Ok(cleanup)
}

fn spawn_save_task(pace: ExitPace) -> tokio::task::JoinHandle<bool> {
    // Сохранение настроек идёт наравне с уборкой, а не перед ней: файлы, которые
    // мы пишем, к остановке ядра и к системному прокси отношения не имеют.
    tokio::task::spawn(async move {
        match timeout(pace.save_budget(), Config::apply_all_and_save_file()).await {
            Ok(()) => true,
            Err(_) => {
                logging!(
                    warn,
                    Type::System,
                    "Warning: таймаут сохранения настроек при выходе, продолжаем выход"
                );
                false
            }
        }
    })
}

fn spawn_proxy_task(pace: ExitPace) -> tokio::task::JoinHandle<ProxyAtExit> {
    tokio::task::spawn(async move {
        logging!(info, Type::Window, "сброс системного прокси...");
        match timeout(
            pace.sysproxy_budget(),
            sysopt::Sysopt::global().reset_sysproxy_if_ours(),
        )
        .await
        {
            Ok(Ok(sysopt::SysproxyTakeDown::Cleared)) => {
                logging!(info, Type::Window, "системный прокси сброшен");
                ProxyAtExit::Cleared
            }
            Ok(Ok(sysopt::SysproxyTakeDown::LeftToItsOwner)) => {
                logging!(
                    warn,
                    Type::Window,
                    "Warning: системный прокси не снимали — в системе стоят настройки, поставленные не нами"
                );
                ProxyAtExit::LeftToItsOwner
            }
            Ok(Ok(sysopt::SysproxyTakeDown::OursIsStillInTheSystem)) => {
                logging!(
                    error,
                    Type::Window,
                    "системный прокси снять не удалось — наши настройки остаются в системе"
                );
                ProxyAtExit::OursIsStillInTheSystem
            }
            Ok(Err(e)) => {
                logging!(warn, Type::Window, "Warning: не удалось сбросить системный прокси: {e}");
                ProxyAtExit::Refused
            }
            Err(_) => {
                logging!(
                    warn,
                    Type::Window,
                    "Warning: таймаут сброса системного прокси, продолжаем выход"
                );
                ProxyAtExit::Refused
            }
        }
    })
}

fn spawn_dns_task(budget: Duration) -> tokio::task::JoinHandle<bool> {
    tokio::task::spawn(async move {
        #[cfg(target_os = "macos")]
        {
            let restored = crate::utils::resolve::dns::restore_public_dns_before_exit(budget).await;
            if restored {
                logging!(info, Type::Window, "настройки DNS восстановлены");
            } else {
                logging!(warn, Type::Window, "Warning: не удалось восстановить настройки DNS");
            }
            restored
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = budget;
            true
        }
    })
}

async fn clean_the_rest(pace: ExitPace, dns_budget: Duration) -> CleanupOutcome {
    let (save_result, proxy_result, dns_result) = tokio::join!(
        spawn_save_task(pace),
        spawn_proxy_task(pace),
        spawn_dns_task(dns_budget)
    );
    let save_success = save_result.unwrap_or_default();
    let proxy_outcome = proxy_result.unwrap_or_default();
    let dns_success = dns_result.unwrap_or_default();
    logging!(
        info,
        Type::System,
        "асинхронное завершение выполнено — настройки: {}, прокси: {:?}, DNS: {}",
        save_success,
        proxy_outcome,
        dns_success
    );
    CleanupOutcome {
        all_success: save_success && the_take_down_went_as_asked(proxy_outcome) && dns_success,
        sysproxy: Some(proxy_outcome),
    }
}

pub async fn clean_async_at(pace: ExitPace) -> CleanupOutcome {
    logging!(info, Type::System, "начало асинхронной очистки...");

    let core_task = tokio::task::spawn(async move {
        turn_the_tun_off(pace).await;

        let stop_timeout = pace.core_stop_budget();

        logging!(info, Type::System, "stop core");
        match timeout(stop_timeout, CoreManager::global().stop_core()).await {
            Ok(Ok(())) => {
                logging!(info, Type::Window, "ядро остановлено");
                true
            }
            Ok(Err(e)) => {
                logging!(warn, Type::Window, "Warning: не удалось остановить ядро: {e}");
                false
            }
            Err(_) => {
                logging!(
                    warn,
                    Type::Window,
                    "Warning: таймаут остановки ядра (возможно, система выключается), продолжаем выход"
                );
                false
            }
        }
    });

    let (save_result, proxy_result, core_result, dns_result) = tokio::join!(
        spawn_save_task(pace),
        spawn_proxy_task(pace),
        core_task,
        spawn_dns_task(pace.dns_budget(pace.core_branch_without_cancel()))
    );

    let save_success = save_result.unwrap_or_default();
    let proxy_outcome = proxy_result.unwrap_or_default();
    let core_success = core_result.unwrap_or_default();
    let dns_success = dns_result.unwrap_or_default();

    let all_success = save_success && the_take_down_went_as_asked(proxy_outcome) && core_success && dns_success;

    logging!(
        info,
        Type::System,
        "асинхронное завершение выполнено — настройки: {}, прокси: {:?}, ядро: {}, DNS: {}, итог: {}",
        save_success,
        proxy_outcome,
        core_success,
        dns_success,
        all_success
    );

    CleanupOutcome {
        all_success,
        sysproxy: Some(proxy_outcome),
    }
}

#[cfg(target_os = "macos")]
pub async fn hide() {
    use crate::module::lightweight::add_light_weight_timer;

    let enable_auto_light_weight_mode = Config::verge()
        .await
        .data_arc()
        .enable_auto_light_weight_mode
        .unwrap_or(false);

    if enable_auto_light_weight_mode {
        add_light_weight_timer().await;
    }

    if let Some(window) = WindowManager::get_main_window()
        && window.is_visible().unwrap_or(false)
    {
        let _ = window.hide();
        crate::core::notification::frontend_stopped_listening();
    }
    handle::Handle::global().set_activation_policy_accessory();
}

#[cfg(test)]
mod tests {
    use super::{ExitPace, ProxyAtExit, the_take_down_went_as_asked};

    #[test]
    fn only_our_own_proxy_left_behind_or_a_refusal_warns_the_person_and_fails_the_exit() {
        assert!(the_take_down_went_as_asked(ProxyAtExit::Cleared));
        assert!(the_take_down_went_as_asked(ProxyAtExit::LeftToItsOwner));
        assert!(!the_take_down_went_as_asked(ProxyAtExit::OursIsStillInTheSystem));
        assert!(!the_take_down_went_as_asked(ProxyAtExit::Refused));
    }

    #[test]
    fn a_task_that_never_answered_counts_as_a_refusal() {
        assert_eq!(ProxyAtExit::default(), ProxyAtExit::Refused);
    }

    #[test]
    fn session_ending_never_waits_longer_than_an_interactive_quit() {
        let quick = ExitPace::SessionEnding;
        let calm = ExitPace::Interactive;
        assert!(quick.sysproxy_budget() <= calm.sysproxy_budget());
        assert!(quick.tun_off_budget() <= calm.tun_off_budget());
        assert!(quick.core_stop_budget() <= calm.core_stop_budget());
        assert!(quick.save_budget() <= calm.save_budget());
        for core_branch in [calm.core_branch_with_cancel(), calm.core_branch_without_cancel()] {
            assert!(quick.dns_budget(core_branch) <= calm.dns_budget(core_branch));
        }
    }

    #[test]
    fn the_dns_step_never_outlasts_the_neighbours_of_its_own_path() {
        for pace in [ExitPace::Interactive, ExitPace::SessionEnding] {
            for core_branch in [pace.core_branch_with_cancel(), pace.core_branch_without_cancel()] {
                let neighbours = core_branch.max(pace.save_budget()).max(pace.sysproxy_budget());
                assert!(pace.dns_budget(core_branch) <= neighbours);
            }
        }
        assert!(ExitPace::Interactive.core_branch_without_cancel() < ExitPace::Interactive.core_branch_with_cancel());
    }

    #[test]
    fn no_step_got_less_room_than_it_had_before() {
        // Прежние безусловные бюджеты: снятие прокси 1.5 с, выключение TUN 3 с,
        // остановка ядра 5 с, сохранение настроек — без предела вовсе.
        for pace in [ExitPace::Interactive, ExitPace::SessionEnding] {
            assert!(pace.sysproxy_budget() >= std::time::Duration::from_millis(1500));
            assert!(pace.tun_off_budget() >= std::time::Duration::from_secs(3));
            assert!(pace.core_stop_budget() >= std::time::Duration::from_secs(5));
        }
    }

    #[test]
    fn the_signal_kind_decides_the_pace() {
        assert_eq!(
            ExitPace::from(clash_verge_signal::Shutdown::Interactive),
            ExitPace::Interactive
        );
        assert_eq!(
            ExitPace::from(clash_verge_signal::Shutdown::SessionEnding),
            ExitPace::SessionEnding
        );
    }
}
