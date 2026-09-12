use super::CmdResult;
use crate::{cmd::StringifyErr as _, config::IVerge, core, feat};
use reqwest_dav::list_cmd::ListFile;
use smartstring::alias::String;

/// Сохранить конфиг WebDAV
#[tauri::command]
pub async fn save_webdav_config(url: String, username: String, password: String) -> CmdResult<()> {
    let patch = IVerge {
        webdav_url: Some(url),
        webdav_username: Some(username),
        webdav_password: Some(password),
        ..IVerge::default()
    };
    feat::commit_verge_edit(|verge| verge.patch_config(&patch))
        .await
        .stringify_err()?;
    core::backup::WebDavClient::global().reset();
    Ok(())
}

/// Создать резервную копию WebDAV и загрузить её
#[tauri::command]
pub async fn create_webdav_backup() -> CmdResult<()> {
    feat::create_backup_and_upload_webdav().await.stringify_err()
}

/// Список файлов резервных копий на WebDAV
#[tauri::command]
pub async fn list_webdav_backup() -> CmdResult<Vec<ListFile>> {
    feat::list_wevdav_backup().await.stringify_err()
}

/// Удалить файл резервной копии на WebDAV
#[tauri::command]
pub async fn delete_webdav_backup(filename: String) -> CmdResult<()> {
    feat::delete_webdav_backup(filename).await.stringify_err()
}

/// Восстановить файл резервной копии из WebDAV
#[tauri::command]
pub async fn restore_webdav_backup(filename: String) -> CmdResult<()> {
    Box::pin(feat::restore_webdav_backup(filename)).await.stringify_err()
}
