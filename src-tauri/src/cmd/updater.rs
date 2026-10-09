use super::CmdResult;
use crate::core::{SilentUpdater, updater};
use tauri::{Manager as _, Webview, ipc::Channel};

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUpdateMetadata {
    rid: tauri::ResourceId,
    current_version: String,
    version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    body: Option<String>,
    raw_json: serde_json::Value,
}

#[tauri::command]
pub async fn check_app_update(webview: Webview) -> CmdResult<Option<AppUpdateMetadata>> {
    let Some(update) = updater::check_update_with_fallback(webview.app_handle())
        .await
        .map_err(|err| super::public_error_text(&format!("{err:#}")))?
    else {
        return Ok(None);
    };
    let current_version = update.current_version.clone();
    let version = update.version.clone();
    let body = update.body.clone();
    let raw_json = update.raw_json.clone();
    let rid = webview.resources_table().add(update);
    Ok(Some(AppUpdateMetadata {
        rid,
        current_version,
        version,
        body,
        raw_json,
    }))
}

/// Поставить обновление, найденное `check_app_update`, и перезапуститься.
/// `false` — отменили до запуска установщика. Перезапуск здесь, а не в окне:
/// окно могло закрыться посреди загрузки (облегчённый режим), а поставленное
/// без перезапуска оставило бы приложение на старой копии. На Windows
/// установщик завершает процесс сам.
#[tauri::command]
pub async fn install_app_update(
    webview: Webview,
    rid: tauri::ResourceId,
    on_event: Channel<updater::DownloadEvent>,
) -> CmdResult<bool> {
    let update = webview
        .resources_table()
        .get::<tauri_plugin_updater::Update>(rid)
        .map_err(|err| super::public_error_text(&format!("{err:#}")))?;
    let installed = SilentUpdater::global()
        .install_manually(webview.app_handle(), (*update).clone(), |event| {
            let _ = on_event.send(event);
        })
        .await
        .map_err(|err| super::public_error_text(&format!("{err:#}")))?;
    if installed {
        crate::feat::restart_app().await;
    }
    Ok(installed)
}

#[tauri::command]
pub fn cancel_app_update() {
    SilentUpdater::global().cancel_manual_install();
}
