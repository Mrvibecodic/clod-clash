use super::CmdResult;
use crate::{cmd::StringifyErr as _, feat};
use feat::LocalBackupFile;
use smartstring::alias::String;

#[tauri::command]
pub async fn create_local_backup() -> CmdResult<()> {
    feat::create_local_backup().await.stringify_err()
}

#[tauri::command]
pub async fn list_local_backup() -> CmdResult<Vec<LocalBackupFile>> {
    feat::list_local_backup().await.stringify_err()
}

#[tauri::command]
pub async fn delete_local_backup(filename: String) -> CmdResult<()> {
    feat::delete_local_backup(filename).await.stringify_err()
}

#[tauri::command]
pub async fn restore_local_backup(filename: String) -> CmdResult<()> {
    feat::restore_local_backup(filename).await.stringify_err()
}

#[tauri::command]
pub async fn import_local_backup(source: String) -> CmdResult<String> {
    feat::import_local_backup(source).await.stringify_err()
}

#[tauri::command]
pub async fn export_local_backup(app: tauri::AppHandle, filename: String) -> CmdResult<bool> {
    use tauri_plugin_dialog::DialogExt as _;

    let dialog = app.dialog().file().set_file_name(filename.as_str());
    let Some(destination) = tokio::task::spawn_blocking(move || dialog.blocking_save_file())
        .await
        .stringify_err()?
        .and_then(|path| path.into_path().ok())
    else {
        return Ok(false);
    };
    feat::export_local_backup(filename, destination).await.stringify_err()?;
    Ok(true)
}
