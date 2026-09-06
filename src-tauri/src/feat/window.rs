use crate::config::Config;
use crate::core::{CoreManager, handle, sysopt};
use crate::module::lightweight;
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
    /// параллельно, поэтому длинный бюджет соседа снятию прокси не мешает.
    const fn tun_off_budget(self) -> Duration {
        Duration::from_secs(3)
    }

    const fn core_stop_budget(self) -> Duration {
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
    quit_at(ExitPace::Interactive).await;
}

/// Выход по сигналу операционной системы.
pub async fn quit_by_signal(shutdown: clash_verge_signal::Shutdown) {
    quit_at(shutdown.into()).await;
}

pub async fn quit_at(pace: ExitPace) {
    logging!(debug, Type::System, "запуск процесса выхода ({pace:?})");
    handle::Handle::global().set_is_exiting();

    utils::server::shutdown_embedded_server();

    logging!(info, Type::System, "начало асинхронной очистки ресурсов");
    let cleanup = clean_async_at(pace).await;

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

pub struct CleanupOutcome {
    pub all_success: bool,
    pub sysproxy_cleared: bool,
}

pub async fn clean_async() -> bool {
    clean_async_at(ExitPace::Interactive).await.all_success
}

pub async fn clean_async_at(pace: ExitPace) -> CleanupOutcome {
    logging!(info, Type::System, "начало асинхронной очистки...");

    // Сохранение настроек идёт наравне с уборкой, а не перед ней: файлы, которые
    // мы пишем, к остановке ядра и к системному прокси отношения не имеют.
    let save_task = tokio::task::spawn(async move {
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
    });

    let proxy_task = tokio::task::spawn(async move {
        if !sysopt::Sysopt::global().we_applied_system_proxy() {
            logging!(info, Type::Window, "системный прокси нами не ставился, сброс пропущен");
            return true;
        }

        logging!(info, Type::Window, "сброс системного прокси...");
        match timeout(pace.sysproxy_budget(), sysopt::Sysopt::global().reset_sysproxy()).await {
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
    });

    let core_task = tokio::task::spawn(async move {
        logging!(info, Type::System, "disable tun");
        // Черновик, а не committed: сохранение настроек идёт теперь наравне с
        // уборкой, и committed может ещё не знать про только что включённый TUN.
        // Черновик — это то, что человек выбрал последним.
        let tun_enabled = Config::verge().await.latest_arc().enable_tun_mode.unwrap_or(false);
        if tun_enabled {
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

    let dns_task = tokio::task::spawn(async move {
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
        true
    });

    let (save_result, proxy_result, core_result, dns_result) = tokio::join!(save_task, proxy_task, core_task, dns_task);

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
