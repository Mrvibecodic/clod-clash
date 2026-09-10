use crate::config::Config;
use crate::core::{CoreManager, handle, manager::ExitStop, sysopt};
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

    #[cfg(target_os = "macos")]
    const fn dns_budget(self) -> Duration {
        match self {
            Self::Interactive => crate::utils::resolve::dns::RESTORE_BUDGET,
            Self::SessionEnding => Duration::from_secs(3),
        }
    }
}

pub async fn quit() {
    quit_at(ExitPace::Interactive, true).await;
}

/// Выход по сигналу операционной системы.
pub async fn quit_by_signal(shutdown: clash_verge_signal::Shutdown) {
    quit_at(shutdown.into(), false).await;
}

pub async fn quit_at(pace: ExitPace, cancel_if_core_stays: bool) {
    if !handle::Handle::global().begin_exiting() {
        logging!(info, Type::System, "выход уже идёт, повторный запрос пропущен");
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

    if !cleanup.sysproxy_cleared {
        // Последствие переживает выход: в системе остался прокси, указывающий на
        // порт, которого через секунду не станет. Обычные уведомления на выходе
        // глушатся, поэтому идём мимо этой заглушки.
        logging!(
            error,
            Type::Window,
            "системный прокси остался в системе: снять его при выходе не удалось"
        );
        handle::Handle::notice_message_while_exiting("app_quit::sysproxy_reset_failed", "");
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
        if let Err(error) = CoreManager::global().start_core().await {
            logging!(
                error,
                Type::Core,
                "после отменённого выхода ядро не поднялось заново: {error:#}"
            );
        }
        // Уборка шла параллельно с остановкой ядра и успела снять то, что
        // приложению теперь снова нужно. Каждый шаг — пустой, если снимать
        // было нечего.
        crate::feat::tun::bring_back_after_a_cancelled_exit().await;
        if Config::verge().await.latest_arc().enable_system_proxy.unwrap_or(false) {
            match sysopt::Sysopt::global().update_sysproxy().await {
                // Сброс прокси на выходе остановил и его сторож; запись сама
                // его не поднимает — только `refresh_guard`, как везде в коде.
                Ok(()) => sysopt::Sysopt::global().refresh_guard().await,
                Err(error) => {
                    logging!(
                        warn,
                        Type::Core,
                        "после отменённого выхода системный прокси не вернулся: {error}"
                    );
                    handle::Handle::notice_message("sysproxy::write_failed", error.to_string());
                }
            }
        }
        #[cfg(target_os = "macos")]
        crate::utils::resolve::dns::apply_remembered_desire();
        // Сторожа ядра и окружения выходят по флагу выхода и сами не
        // возвращаются: запуск ядра их не заводит, если ядро числится живым.
        CoreManager::global().watch_the_core_again().await;
        crate::feat::environment::spawn_environment_watchdog();
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

pub struct CleanupOutcome {
    pub all_success: bool,
    pub sysproxy_cleared: bool,
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
        handle::Handle::mihomo().await.patch_base_config(&disable_tun).await
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
    let rest = tokio::task::spawn(clean_the_rest(pace));
    let stop_budget = pace.core_stop_budget();
    let core = async {
        turn_the_tun_off(pace).await;
        logging!(info, Type::System, "stop core");
        CoreManager::global()
            .stop_core_for_exit(pace.lock_wait_budget(), stop_budget)
            .await
    };
    let (rest, stop) = tokio::join!(rest, core);
    let mut cleanup = rest.unwrap_or(CleanupOutcome {
        all_success: false,
        sysproxy_cleared: false,
    });

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

fn spawn_proxy_task(pace: ExitPace) -> tokio::task::JoinHandle<bool> {
    tokio::task::spawn(async move {
        logging!(info, Type::Window, "сброс системного прокси...");
        match timeout(
            pace.sysproxy_budget(),
            sysopt::Sysopt::global().reset_sysproxy_if_ours(),
        )
        .await
        {
            Ok(Ok(_)) => {
                logging!(info, Type::Window, "системный прокси сброшен");
                true
            }
            Ok(Err(e)) => {
                logging!(warn, Type::Window, "Warning: не удалось сбросить системный прокси: {e}");
                false
            }
            Err(_) => {
                logging!(
                    warn,
                    Type::Window,
                    "Warning: таймаут сброса системного прокси, продолжаем выход"
                );
                false
            }
        }
    })
}

fn spawn_dns_task(pace: ExitPace) -> tokio::task::JoinHandle<bool> {
    tokio::task::spawn(async move {
        #[cfg(target_os = "macos")]
        match timeout(pace.dns_budget(), crate::utils::resolve::dns::restore_public_dns()).await {
            Ok(restored) => {
                if restored {
                    logging!(info, Type::Window, "настройки DNS восстановлены");
                } else {
                    logging!(warn, Type::Window, "Warning: не удалось восстановить настройки DNS");
                }
                restored
            }
            Err(_) => {
                logging!(warn, Type::Window, "Warning: таймаут восстановления настроек DNS");
                false
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = pace;
            true
        }
    })
}

async fn clean_the_rest(pace: ExitPace) -> CleanupOutcome {
    let (save_result, proxy_result, dns_result) =
        tokio::join!(spawn_save_task(pace), spawn_proxy_task(pace), spawn_dns_task(pace));
    let save_success = save_result.unwrap_or_default();
    let proxy_success = proxy_result.unwrap_or_default();
    let dns_success = dns_result.unwrap_or_default();
    logging!(
        info,
        Type::System,
        "асинхронное завершение выполнено — настройки: {}, прокси: {}, DNS: {}",
        save_success,
        proxy_success,
        dns_success
    );
    CleanupOutcome {
        all_success: save_success && proxy_success && dns_success,
        sysproxy_cleared: proxy_success,
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
        spawn_dns_task(pace)
    );

    let save_success = save_result.unwrap_or_default();
    let proxy_success = proxy_result.unwrap_or_default();
    let core_success = core_result.unwrap_or_default();
    let dns_success = dns_result.unwrap_or_default();

    let all_success = save_success && proxy_success && core_success && dns_success;

    logging!(
        info,
        Type::System,
        "асинхронное завершение выполнено — настройки: {}, прокси: {}, ядро: {}, DNS: {}, итог: {}",
        save_success,
        proxy_success,
        core_success,
        dns_success,
        all_success
    );

    CleanupOutcome {
        all_success,
        sysproxy_cleared: proxy_success,
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
    use super::ExitPace;

    #[test]
    fn session_ending_never_waits_longer_than_an_interactive_quit() {
        let quick = ExitPace::SessionEnding;
        let calm = ExitPace::Interactive;
        assert!(quick.sysproxy_budget() <= calm.sysproxy_budget());
        assert!(quick.tun_off_budget() <= calm.tun_off_budget());
        assert!(quick.core_stop_budget() <= calm.core_stop_budget());
        assert!(quick.save_budget() <= calm.save_budget());
        #[cfg(target_os = "macos")]
        assert!(quick.dns_budget() <= calm.dns_budget());
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
