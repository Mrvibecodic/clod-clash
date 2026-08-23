use crate::{
    config::Config,
    core::{CoreManager, handle, tray},
    feat::clean_async,
    process::AsyncHandler,
    utils,
};
use clash_verge_logging::{Type, logging};
use serde_yaml_ng::{Mapping, Value};
use smartstring::alias::String;

pub async fn restart_clash_core() {
    match CoreManager::global().restart_core().await {
        Ok(_) => {
            handle::Handle::refresh_clash();
            if let Err(err) = crate::config::profiles::activate_selected_nodes() {
                logging!(
                    warn,
                    Type::Core,
                    "Warning: restore selection after core restart failed: {err}"
                );
            }
            handle::Handle::notice_message("set_config::ok", "ok");
        }
        Err(err) => {
            handle::Handle::notice_message("set_config::error", format!("{err}"));
            logging!(error, Type::Core, "{err}");
        }
    }
}

pub async fn restart_app() {
    logging!(debug, Type::System, "Запуск процесса перезапуска приложения");
    handle::Handle::global().set_is_exiting();

    utils::server::shutdown_embedded_server();
    Config::apply_all_and_save_file().await;

    logging!(info, Type::System, "Начало асинхронной очистки ресурсов");
    let cleanup_result = clean_async().await;

    logging!(
        info,
        Type::System,
        "Очистка ресурсов завершена, код выхода: {}",
        if cleanup_result { 0 } else { 1 }
    );

    let app_handle = handle::Handle::app_handle();
    app_handle.restart();
}

fn after_change_clash_mode() {
    AsyncHandler::spawn(move || async {
        let mihomo = handle::Handle::mihomo().await;
        match mihomo.get_connections().await {
            Ok(connections) => {
                if let Some(connections_array) = connections.connections {
                    for connection in connections_array {
                        let _ = mihomo.close_connection(&connection.id).await;
                    }
                    drop(mihomo);
                }
            }
            Err(err) => {
                logging!(error, Type::Core, "Failed to get connections: {err}");
            }
        }
    });
}

async fn mode_locked_by_panel() -> bool {
    let profiles = Config::profiles().await.latest_arc();
    profiles
        .get_current()
        .and_then(|uid| profiles.get_item(uid).ok())
        .and_then(|item| item.lock_mode)
        .unwrap_or(false)
}

pub async fn change_clash_mode(mode: String) -> Result<(), String> {
    if mode_locked_by_panel().await {
        logging!(
            info,
            Type::Core,
            "mode change refused: locked by the panel (clod-lock-mode)"
        );
        return Err(clash_verge_i18n::t!("common.modeLocked").into_owned().into());
    }
    let mut mapping = Mapping::new();
    mapping.insert(Value::from("mode"), Value::from(mode.as_str()));
    let json_value = serde_json::json!({
        "mode": mode
    });
    logging!(debug, Type::Core, "change clash mode to {mode}");
    if let Err(err) = handle::Handle::mihomo().await.patch_base_config(&json_value).await {
        logging!(error, Type::Core, "{err}");
        return Err(err.to_string().into());
    }

    let clash = Config::clash().await;
    clash.edit_draft(|d| d.patch_config(&mapping));
    clash.apply();

    let runtime = Config::runtime().await;
    runtime.edit_draft(|d| d.patch_config(&mapping));
    runtime.apply();
    if let Err(err) = Config::generate_file(crate::config::ConfigType::Run).await {
        logging!(
            warn,
            Type::Core,
            "Warning: failed to refresh runtime config file after mode change: {err}"
        );
    }

    let clash_data = clash.data_arc();
    if clash_data.save_config().await.is_ok() {
        handle::Handle::refresh_clash();
        tray::Tray::global().update_menu_and_icon().await;
    }

    let is_auto_close_connection = Config::verge().await.data_arc().auto_close_connection();
    if is_auto_close_connection {
        after_change_clash_mode();
    }

    Ok(())
}
