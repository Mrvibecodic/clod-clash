//! clod:F5 — commands of the managed Mihomo core.

use super::CmdResult;
use crate::core::core_updater;

/// Managed-core state for the settings dialog.
#[tauri::command]
pub async fn get_core_updater_status() -> CmdResult<core_updater::CoreUpdaterStatus> {
    Ok(core_updater::status().await)
}

/// Ask the configured channel for its newest build.
#[tauri::command]
pub async fn check_core_update() -> CmdResult<core_updater::CoreUpdateCheck> {
    core_updater::check_core_update()
        .await
        .map_err(|err| format!("{err:#}").into())
}

/// Download, verify and switch to the newest build of the channel.
#[tauri::command]
pub async fn download_and_apply_core() -> CmdResult<core_updater::CoreUpdateCheck> {
    core_updater::download_and_apply_core()
        .await
        .map_err(|err| format!("{err:#}").into())
}

/// Go back to the previously active managed version.
#[tauri::command]
pub async fn revert_core() -> CmdResult {
    core_updater::revert_core()
        .await
        .map_err(|err| format!("{err:#}").into())
}

/// Back to the bundled sidecar (keeps the downloaded versions on disk).
#[tauri::command]
pub async fn disable_managed_core() -> CmdResult {
    core_updater::disable_managed_core()
        .await
        .map_err(|err| format!("{err:#}").into())
}
