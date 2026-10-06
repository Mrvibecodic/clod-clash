use super::CmdResult;
use crate::core::tray::Tray;
use crate::process::AsyncHandler;
use clash_verge_logging::{Type, logging};
use std::sync::atomic::{AtomicBool, Ordering};

static TRAY_SYNC_RUNNING: AtomicBool = AtomicBool::new(false);
static TRAY_SYNC_PENDING: AtomicBool = AtomicBool::new(false);

/// Пересобрать меню трея после выбора узла в окне. Частые выборы подряд
/// сливаются в одну пересборку.
pub fn sync_tray_proxy_selection() {
    if TRAY_SYNC_RUNNING
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        AsyncHandler::spawn(move || async move {
            run_tray_sync_loop().await;
        });
    } else {
        TRAY_SYNC_PENDING.store(true, Ordering::Release);
    }
}

async fn run_tray_sync_loop() {
    loop {
        match Tray::global().update_menu().await {
            Ok(_) => {
                logging!(info, Type::Cmd, "Tray proxy selection synced successfully");
            }
            Err(e) => {
                logging!(error, Type::Cmd, "Failed to sync tray proxy selection: {e}");
            }
        }

        if !TRAY_SYNC_PENDING.swap(false, Ordering::AcqRel) {
            TRAY_SYNC_RUNNING.store(false, Ordering::Release);

            if TRAY_SYNC_PENDING.swap(false, Ordering::AcqRel)
                && TRAY_SYNC_RUNNING
                    .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok()
            {
                continue;
            }

            break;
        }
    }
}

#[tauri::command]
pub async fn close_connections_via(previous_proxy: String) -> CmdResult<usize> {
    Ok(crate::feat::close_connections_via(&previous_proxy).await)
}

/// clod:freeze — пометки «режется» / «не отвечает» текущей подписки в текущей
/// сети: имя узла → `frozen` | `dead`. Пусто — ничего не помечено или ядро
/// без отпечатков.
#[tauri::command]
pub async fn get_freeze_marks() -> CmdResult<std::collections::BTreeMap<String, &'static str>> {
    Ok(crate::module::freeze_check::marks())
}
