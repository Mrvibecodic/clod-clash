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
pub async fn take_pending_notices() -> Result<Vec<(String, String, u32)>, String> {
    Ok(NotificationSystem::take_pending_notices())
}

/// Отказы панели в устройстве, которые ушли без страницы, — в том виде, в
/// каком они в силе сейчас: удалённая или вылечившаяся подписка не всплывёт.
#[tauri::command]
pub async fn take_unseen_device_refusals() -> Vec<serde_json::Value> {
    let mut refusals = Vec::new();
    for uid in NotificationSystem::take_unseen_device_refusals() {
        refusals.extend(crate::feat::device_refusal(&uid).await);
    }
    refusals
}

#[tauri::command]
pub fn stop_listening_notices() {
    crate::core::notification::frontend_stopped_listening();
}
