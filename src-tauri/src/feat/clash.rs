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

/// clod:Э11-10 — один запрос вместо сотен.
///
/// Раньше здесь забирался весь список соединений и каждое закрывалось отдельным
/// запросом: на нагруженной машине это сотни последовательных обращений к ядру,
/// притом что `DELETE /connections` закрывает всё разом. Делать это на бэкенде
/// по-прежнему нужно — режим меняют и трей, и горячие клавиши, мимо интерфейса.
fn close_connections_after_mode_change() {
    AsyncHandler::spawn(|| async {
        if let Err(err) = handle::Handle::mihomo().await.close_all_connections().await {
            logging!(warn, Type::Core, "Warning: не удалось разорвать соединения: {err}");
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

    if Config::verge().await.data_arc().auto_close_connection() {
        close_connections_after_mode_change();
    }

    Ok(())
}
