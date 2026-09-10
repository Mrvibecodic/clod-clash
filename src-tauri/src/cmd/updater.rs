use super::CmdResult;
use crate::core::updater;
use tauri::{Manager as _, Webview};

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
