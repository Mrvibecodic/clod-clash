use parking_lot::RwLock;
use tauri::{AppHandle, Runtime, State, command};
use tauri_plugin_clipboard_manager::{ClipboardExt as _, Error};

use crate::Platform;

#[command]
pub fn export_diagnostic_info<R: Runtime>(
    app_handle: AppHandle<R>,
    state: State<'_, RwLock<Platform>>,
) -> Result<(), Error> {
    let info = state.inner().read().to_string();
    let clipboard = app_handle.clipboard();
    clipboard.write_text(info)
}
