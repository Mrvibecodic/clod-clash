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

/// Ядро подменило свой файл (`/upgrade`) и сейчас перезапустится. Об
/// обновлении пользователь уже знает — «ядро упало» на этот перезапуск не пишем.
/// Только на Windows: на macOS/Linux ядро делает exec, процесс тот же, и
/// перезапуска, о котором мог бы сказать сторож, нет.
#[tauri::command]
pub async fn core_replaced_itself() -> CmdResult {
    #[cfg(windows)]
    crate::core::CoreManager::global().a_restart_the_user_knows_of();
    core_updater::repin_core_binaries().await;
    Ok(())
}

#[tauri::command]
pub async fn disable_managed_core() -> CmdResult {
    core_updater::disable_managed_core()
        .await
        .map_err(|err| super::public_error_text(&format!("{err:#}")))
}
