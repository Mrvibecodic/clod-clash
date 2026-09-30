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

/// Ядро обновляет себя само (`/upgrade` через службу); итог — по версии ядра,
/// которое ответит после перезапуска.
#[tauri::command]
pub async fn upgrade_core_itself() -> CmdResult<core_updater::SelfUpgrade> {
    core_updater::upgrade_through_core()
        .await
        .map_err(|err| super::public_error_text(&format!("{err:#}")))
}
