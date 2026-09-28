use super::CmdResult;
use crate::core::core_updater;

#[tauri::command]
pub async fn get_core_updater_status() -> CmdResult<core_updater::CoreUpdaterStatus> {
    Ok(core_updater::status().await)
}

/// Обновить выбранное встроенное ядро силами приложения (папка программы
/// доступна на запись): скачать, сверить, подменить с откатом.
#[tauri::command]
pub async fn update_bundled_core() -> CmdResult<core_updater::BundledCoreUpdate> {
    core_updater::update_bundled_core()
        .await
        .map_err(|err| super::public_error_text(&format!("{err:#}")))
}

/// Ядро подменило свой файл (`/upgrade` через службу) и сейчас перезапустится. Об
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
