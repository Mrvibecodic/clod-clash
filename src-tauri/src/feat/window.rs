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

pub async fn quit() {
    logging!(debug, Type::System, "запуск процесса выхода");
    handle::Handle::global().set_is_exiting();

    utils::server::shutdown_embedded_server();
    Config::apply_all_and_save_file().await;

    logging!(info, Type::System, "начало асинхронной очистки ресурсов");
    let cleanup_result = clean_async().await;

    logging!(
        info,
        Type::System,
        "очистка ресурсов завершена, код выхода: {}",
        if cleanup_result { 0 } else { 1 }
    );

    let app_handle = handle::Handle::app_handle();
    app_handle.exit(if cleanup_result { 0 } else { 1 });
}

pub async fn clean_async() -> bool {
    logging!(info, Type::System, "начало асинхронной очистки...");

    let proxy_task = tokio::task::spawn(async {
        let sys_proxy_enabled = Config::verge().await.data_arc().enable_system_proxy.unwrap_or(false);
        if !sys_proxy_enabled && !sysopt::Sysopt::global().we_applied_system_proxy() {
            logging!(
                info,
                Type::Window,
                "системный прокси не включён и нами не ставился, сброс пропущен"
            );
            return true;
        }

        logging!(info, Type::Window, "сброс системного прокси...");
        match timeout(Duration::from_millis(1500), sysopt::Sysopt::global().reset_sysproxy()).await {
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

    let core_task = tokio::task::spawn(async {
        logging!(info, Type::System, "disable tun");
        let tun_enabled = Config::verge().await.data_arc().enable_tun_mode.unwrap_or(false);
        if tun_enabled {
            let disable_tun = serde_json::json!({ "tun": { "enable": false } });

            logging!(info, Type::System, "send disable tun request to mihomo");
            match timeout(
                Duration::from_millis(3000),
                handle::Handle::mihomo().await.patch_base_config(&disable_tun),
            )
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

        let stop_timeout = Duration::from_secs(5);

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

    let dns_task = tokio::task::spawn(async {
        #[cfg(target_os = "macos")]
        match timeout(
            Duration::from_millis(1000),
            crate::utils::resolve::dns::restore_public_dns(),
        )
        .await
        {
            Ok(_) => {
                logging!(info, Type::Window, "настройки DNS восстановлены");
                true
            }
            Err(_) => {
                logging!(warn, Type::Window, "Warning: таймаут восстановления настроек DNS");
                false
            }
        }
        #[cfg(not(target_os = "macos"))]
        true
    });

    let (proxy_result, core_result, dns_result) = tokio::join!(proxy_task, core_task, dns_task);

    let proxy_success = proxy_result.unwrap_or_default();
    let core_success = core_result.unwrap_or_default();
    let dns_success = dns_result.unwrap_or_default();

    let all_success = proxy_success && core_success && dns_success;

    logging!(
        info,
        Type::System,
        "асинхронное завершение выполнено — прокси: {}, ядро: {}, DNS: {}, итог: {}",
        proxy_success,
        core_success,
        dns_success,
        all_success
    );

    all_success
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
