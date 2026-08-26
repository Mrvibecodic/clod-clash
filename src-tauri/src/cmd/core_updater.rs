use super::CmdResult;
use crate::core::core_updater;

#[tauri::command]
pub async fn get_core_updater_status() -> CmdResult<core_updater::CoreUpdaterStatus> {
    Ok(core_updater::status().await)
}

#[tauri::command]
pub async fn check_core_update() -> CmdResult<core_updater::CoreUpdateCheck> {
    core_updater::check_core_update()
        .await
        .map_err(|err| super::public_error_text(&format!("{err:#}")))
}

#[tauri::command]
pub async fn download_and_apply_core() -> CmdResult<core_updater::CoreUpdateCheck> {
    core_updater::download_and_apply_core()
        .await
        .map_err(|err| super::public_error_text(&format!("{err:#}")))
}

#[tauri::command]
pub async fn revert_core() -> CmdResult {
    core_updater::revert_core()
        .await
        .map_err(|err| super::public_error_text(&format!("{err:#}")))
}

#[tauri::command]
pub async fn repin_core_binaries() -> CmdResult {
    core_updater::repin_core_binaries().await;
    Ok(())
}

#[tauri::command]
pub async fn disable_managed_core() -> CmdResult {
    core_updater::disable_managed_core()
        .await
        .map_err(|err| super::public_error_text(&format!("{err:#}")))
}
