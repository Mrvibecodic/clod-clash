use crate::core::{CoreManager, manager::RunningMode, notification::NotificationSystem};

/// Получить текущий режим работы ядра
#[tauri::command]
pub async fn get_running_mode() -> Result<String, String> {
    let manager = CoreManager::global();
    let mode = manager.get_running_mode();
    if matches!(*mode, RunningMode::NotRunning) && !manager.is_down() {
        return Ok("Starting".to_owned());
    }
    Ok(mode.to_string())
}

#[tauri::command]
pub async fn take_pending_notices() -> Result<Vec<(String, String)>, String> {
    Ok(NotificationSystem::take_pending_notices())
}
