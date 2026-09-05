use crate::{
    config::{Config, IClashTemp, IProfiles, IVerge},
    constants::files::DNS_CONFIG,
    core::backup,
    process::AsyncHandler,
    utils::{
        dirs::{self, PathBufExec as _, app_home_dir, local_backup_dir, verge_path},
        help,
    },
};
use anyhow::{Result, anyhow};
use chrono::Utc;
use clash_verge_logging::{Type, logging};
use reqwest_dav::list_cmd::ListFile;
use serde::Serialize;
use smartstring::alias::String;
use std::path::PathBuf;
use tokio::fs;

#[derive(Debug, Serialize)]
pub struct LocalBackupFile {
    pub filename: String,
    pub path: String,
    pub last_modified: String,
    pub content_length: u64,
}

fn top_level_backup_files() -> [&'static str; 4] {
    [dirs::CLASH_CONFIG, dirs::VERGE_CONFIG, dirs::PROFILE_YAML, DNS_CONFIG]
}

fn restorable_backup_entry(name: &str) -> Option<PathBuf> {
    if name.is_empty() || name.ends_with('/') {
        return None;
    }
    let mut parts = name.split('/');
    let first = parts.next()?;
    match parts.next() {
        None => top_level_backup_files()
            .contains(&first)
            .then(|| PathBuf::from(first)),
        Some(second) => {
            if first != "profiles" || parts.next().is_some() || !is_plain_file_name(second) {
                return None;
            }
            Some(PathBuf::from("profiles").join(second))
        }
    }
}

async fn extract_backup(archive: PathBuf, target: PathBuf) -> Result<()> {
    AsyncHandler::spawn_blocking(move || -> Result<()> {
        let file = std::fs::File::open(&archive)?;
        let mut zip = zip::ZipArchive::new(file)?;
        for index in 0..zip.len() {
            let mut entry = zip.by_index(index)?;
            let raw_name = entry.name().to_owned();
            let Some(relative) = restorable_backup_entry(&raw_name) else {
                logging!(
                    warn,
                    Type::Backup,
                    "backup entry is not part of a backup and was not unpacked: {raw_name}"
                );
                continue;
            };
            let destination = target.join(&relative);
            if let Some(parent) = destination.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut written = std::fs::File::create(&destination)?;
            std::io::copy(&mut entry, &mut written)?;
        }
        Ok(())
    })
    .await?
}

fn machine_local_of(verge: &IVerge) -> IVerge {
    IVerge {
        webdav_url: verge.webdav_url.clone(),
        webdav_username: verge.webdav_username.clone(),
        webdav_password: verge.webdav_password.clone(),
        hwid: verge.hwid.clone(),
        tun_setup_declined: verge.tun_setup_declined.clone(),
        ..IVerge::default()
    }
}

fn keep_machine_local(restored: &mut IVerge, local: IVerge) {
    restored.webdav_url = local.webdav_url;
    restored.webdav_username = local.webdav_username;
    restored.webdav_password = local.webdav_password;
    restored.hwid = local.hwid;
    restored.tun_setup_declined = local.tun_setup_declined;
}

async fn machine_local_config() -> IVerge {
    let verge = Config::verge().await;
    let verge = verge.latest_arc();
    machine_local_of(&verge)
}

async fn finalize_restored_verge_config(local: IVerge) -> Result<()> {
    let mut restored = help::read_yaml::<IVerge>(&verge_path()?).await?;
    keep_machine_local(&mut restored, local);
    restored.save_file().await?;

    let restored_clash = IClashTemp::new().await;
    let clash_draft = Config::clash().await;
    clash_draft.edit_draft(|d| {
        *d = restored_clash.clone();
    });
    clash_draft.apply();

    let restored_profiles = IProfiles::new().await;
    let profiles_draft = Config::profiles().await;
    profiles_draft.edit_draft(|d| {
        *d = restored_profiles.clone();
    });
    profiles_draft.apply();

    let verge_draft = Config::verge().await;
    verge_draft.edit_draft(|d| {
        *d = restored.clone();
    });
    verge_draft.apply();

    if let Err(err) = super::patch_verge(&restored, true).await {
        logging!(error, Type::Backup, "Failed to apply restored verge config: {err:#?}");
    }
    Ok(())
}

pub async fn create_backup_and_upload_webdav() -> Result<()> {
    let (file_name, temp_file_path) = backup::create_backup().await.map_err(|err| {
        logging!(error, Type::Backup, "Failed to create backup: {err:#?}");
        err
    })?;

    if let Err(err) = backup::WebDavClient::global()
        .upload(temp_file_path.clone(), file_name)
        .await
    {
        logging!(error, Type::Backup, "Failed to upload to WebDAV: {err:#?}");
        backup::WebDavClient::global().reset();
        return Err(err);
    }

    if let Err(err) = temp_file_path.remove_if_exists().await {
        logging!(warn, Type::Backup, "Failed to remove temp file: {err:#?}");
    }

    Ok(())
}

pub async fn list_wevdav_backup() -> Result<Vec<ListFile>> {
    backup::WebDavClient::global().list().await.map_err(|err| {
        logging!(error, Type::Backup, "Failed to list WebDAV backup files: {err:#?}");
        err
    })
}

pub async fn delete_webdav_backup(filename: String) -> Result<()> {
    backup::WebDavClient::global().delete(filename).await.map_err(|err| {
        logging!(error, Type::Backup, "Failed to delete WebDAV backup file: {err:#?}");
        err
    })
}

pub async fn restore_webdav_backup(filename: String) -> Result<()> {
    let local = machine_local_config().await;

    let backup_storage_path = app_home_dir()
        .map_err(|e| anyhow::anyhow!("Failed to get app home dir: {e}"))?
        .join(filename.as_str());
    backup::WebDavClient::global()
        .download(filename, backup_storage_path.clone())
        .await
        .map_err(|err| {
            logging!(error, Type::Backup, "Failed to download WebDAV backup file: {err:#?}");
            err
        })?;

    let res = match extract_backup(backup_storage_path.clone(), app_home_dir()?).await {
        Ok(()) => finalize_restored_verge_config(local).await,
        Err(err) => Err(err),
    };
    let _ = backup_storage_path.remove_if_exists().await;
    res
}

pub async fn create_local_backup() -> Result<()> {
    create_local_backup_with_namer(|name| name.to_string().into())
        .await
        .map(|_| ())
}

pub async fn create_local_backup_with_namer<F>(namer: F) -> Result<String>
where
    F: FnOnce(&str) -> String,
{
    let (file_name, temp_file_path) = backup::create_backup().await.map_err(|err| {
        logging!(error, Type::Backup, "Failed to create local backup: {err:#?}");
        err
    })?;

    let backup_dir = local_backup_dir()?;
    let final_name = namer(file_name.as_str());
    let target_path = backup_dir.join(final_name.as_str());

    if let Err(err) = move_file(temp_file_path.clone(), target_path.clone()).await {
        logging!(error, Type::Backup, "Failed to move local backup file: {err:#?}");
        if let Err(clean_err) = temp_file_path.remove_if_exists().await {
            logging!(
                warn,
                Type::Backup,
                "Failed to remove temp backup file after move error: {clean_err:#?}"
            );
        }
        return Err(err);
    }

    Ok(final_name)
}

pub async fn import_local_backup(source: String) -> Result<String> {
    let source_path = PathBuf::from(source.as_str());
    if !source_path.exists() {
        return Err(anyhow!("Backup file not found: {source}"));
    }
    if !source_path.is_file() {
        return Err(anyhow!("Backup path is not a file: {source}"));
    }

    let ext = source_path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .unwrap_or_default();
    if ext != "zip" {
        return Err(anyhow!("Only .zip backup files are supported"));
    }

    let file_name = source_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow!("Invalid backup file name"))?;

    let backup_dir = local_backup_dir()?;
    let target_path = backup_dir.join(file_name);

    if target_path == source_path {
        return Ok(file_name.to_string().into());
    }

    if let Some(parent) = target_path.parent() {
        fs::create_dir_all(parent).await?;
    }

    if target_path.exists() {
        return Err(anyhow!("Backup file already exists: {file_name}"));
    }

    fs::copy(&source_path, &target_path)
        .await
        .map_err(|err| anyhow!("Failed to import backup file: {err:#?}"))?;

    Ok(file_name.to_string().into())
}

async fn move_file(from: PathBuf, to: PathBuf) -> Result<()> {
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent).await?;
    }

    match fs::rename(&from, &to).await {
        Ok(_) => Ok(()),
        Err(rename_err) => {
            logging!(
                warn,
                Type::Backup,
                "Failed to rename backup file directly, fallback to copy/remove: {rename_err:#?}"
            );
            fs::copy(&from, &to)
                .await
                .map_err(|err| anyhow!("Failed to copy backup file: {err:#?}"))?;
            fs::remove_file(&from)
                .await
                .map_err(|err| anyhow!("Failed to remove temp backup file: {err:#?}"))?;
            Ok(())
        }
    }
}

pub async fn list_local_backup() -> Result<Vec<LocalBackupFile>> {
    let backup_dir = local_backup_dir()?;
    if !backup_dir.exists() {
        return Ok(vec![]);
    }

    let mut backups = Vec::new();
    let mut dir = fs::read_dir(&backup_dir).await?;
    while let Some(entry) = dir.next_entry().await? {
        let path = entry.path();
        let metadata = entry.metadata().await?;
        if !metadata.is_file() {
            continue;
        }

        let file_name = match path.file_name().and_then(|name| name.to_str()) {
            Some(name) => name,
            None => continue,
        };
        let last_modified = metadata
            .modified()
            .map(|time| chrono::DateTime::<Utc>::from(time).to_rfc3339())
            .unwrap_or_default();
        backups.push(LocalBackupFile {
            filename: file_name.into(),
            path: path.to_string_lossy().into(),
            last_modified: last_modified.into(),
            content_length: metadata.len(),
        });
    }

    backups.sort_by(|a, b| b.filename.cmp(&a.filename));
    Ok(backups)
}

fn is_plain_file_name(filename: &str) -> bool {
    !(filename.is_empty()
        || filename == "."
        || filename == ".."
        || filename.contains('/')
        || filename.contains('\\')
        || filename.contains('\0')
        || (cfg!(windows) && filename.contains(':')))
}

fn backup_file_path(filename: &str) -> Result<PathBuf> {
    if !is_plain_file_name(filename) {
        return Err(anyhow!("Invalid backup file name: {filename}"));
    }

    Ok(local_backup_dir()?.join(filename))
}

pub async fn delete_local_backup(filename: String) -> Result<()> {
    let target_path = backup_file_path(filename.as_str())?;
    if !target_path.exists() {
        logging!(warn, Type::Backup, "Local backup file not found: {}", filename);
        return Ok(());
    }
    target_path.remove_if_exists().await?;
    Ok(())
}

pub async fn restore_local_backup(filename: String) -> Result<()> {
    let target_path = backup_file_path(filename.as_str())?;
    if !target_path.exists() {
        return Err(anyhow!("Backup file not found: {}", filename));
    }

    let local = machine_local_config().await;

    extract_backup(target_path, app_home_dir()?).await?;
    finalize_restored_verge_config(local).await?;
    Ok(())
}

pub async fn export_local_backup(filename: String, dest_path: PathBuf) -> Result<()> {
    let source_path = backup_file_path(filename.as_str())?;
    if !source_path.exists() {
        return Err(anyhow!("Backup file not found: {}", filename));
    }

    if let Some(parent) = dest_path.parent() {
        fs::create_dir_all(parent).await?;
    }

    fs::copy(&source_path, &dest_path)
        .await
        .map(|_| ())
        .map_err(|err| anyhow!("Failed to export backup file: {err:#?}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{is_plain_file_name, keep_machine_local, machine_local_of, restorable_backup_entry};
    use crate::config::IVerge;
    use std::path::PathBuf;

    #[test]
    fn names_that_leave_the_backup_directory_are_rejected() {
        for name in [
            "",
            ".",
            "..",
            "../../verge.yaml",
            "..\\..\\verge.yaml",
            "sub/dir.zip",
            "/etc/passwd",
            "C:\\Windows\\System32\\drivers\\etc\\hosts",
        ] {
            assert!(!is_plain_file_name(name), "traversal accepted: {name:?}");
        }
    }

    #[test]
    fn names_the_app_actually_creates_are_accepted() {
        for name in [
            "linux-backup-2026-08-23_12-30-00.zip",
            "windows-backup-2026-08-23_12-30-00.zip",
            "macos-backup-2026-08-23_12-30-00-auto-scheduled.zip",
            "macos-backup-2026-08-23_12-30-00-auto-merge.zip",
            "macos-backup-2026-08-23_12-30-00-auto-script.zip",
        ] {
            assert!(is_plain_file_name(name), "legitimate backup name rejected: {name}");
        }
    }

    #[test]
    fn an_imported_file_keeps_its_own_name() {
        assert!(is_plain_file_name("моя копия (2).zip"));
        assert!(is_plain_file_name("backup 2026.08.23.zip"));
    }

    fn this_machine() -> IVerge {
        IVerge {
            webdav_url: Some("https://my-nas.local/dav".into()),
            webdav_username: Some("me".into()),
            webdav_password: Some("mine".into()),
            hwid: Some("this-machine".into()),
            tun_setup_declined: Some("0.1.10".into()),
            language: Some("ru".into()),
            ..IVerge::default()
        }
    }

    fn the_other_machine() -> IVerge {
        IVerge {
            webdav_url: Some("https://their-nas.local/dav".into()),
            webdav_username: Some("them".into()),
            webdav_password: Some("theirs".into()),
            hwid: Some("their-machine".into()),
            tun_setup_declined: Some("0.1.4".into()),
            language: Some("en".into()),
            enable_tun_mode: Some(true),
            ..IVerge::default()
        }
    }

    #[test]
    fn only_the_machine_bound_fields_are_taken_from_this_machine() {
        let local = machine_local_of(&this_machine());
        assert_eq!(local.hwid.as_deref(), Some("this-machine"));
        assert_eq!(local.webdav_username.as_deref(), Some("me"));
        assert_eq!(local.tun_setup_declined.as_deref(), Some("0.1.10"));
        assert_eq!(local.language, None);
        assert_eq!(local.enable_tun_mode, None);
    }

    #[test]
    fn a_backup_from_another_machine_does_not_bring_its_fingerprint() {
        let local = machine_local_of(&this_machine());
        let mut restored = the_other_machine();
        keep_machine_local(&mut restored, local);

        assert_eq!(restored.hwid.as_deref(), Some("this-machine"));
        assert_eq!(restored.webdav_url.as_deref(), Some("https://my-nas.local/dav"));
        assert_eq!(restored.webdav_username.as_deref(), Some("me"));
        assert_eq!(restored.webdav_password.as_deref(), Some("mine"));
        assert_eq!(restored.tun_setup_declined.as_deref(), Some("0.1.10"));
    }

    #[test]
    fn everything_else_still_comes_from_the_backup() {
        let local = machine_local_of(&this_machine());
        let mut restored = the_other_machine();
        keep_machine_local(&mut restored, local);

        assert_eq!(restored.language.as_deref(), Some("en"));
        assert_eq!(restored.enable_tun_mode, Some(true));
    }

    #[test]
    fn a_machine_without_a_fingerprint_yet_does_not_inherit_one() {
        let local = machine_local_of(&IVerge::default());
        let mut restored = the_other_machine();
        keep_machine_local(&mut restored, local);

        assert_eq!(restored.hwid, None);
        assert_eq!(restored.webdav_url, None);
    }

    #[test]
    fn everything_the_app_puts_into_a_backup_is_unpacked() {
        for name in [
            "config.yaml",
            "verge.yaml",
            "profiles.yaml",
            "dns_config.yaml",
            "profiles/Rmc1x0.yaml",
            "profiles/моя подписка.yaml",
        ] {
            assert!(
                restorable_backup_entry(name).is_some(),
                "a legitimate backup entry was refused: {name}"
            );
        }
        assert_eq!(
            restorable_backup_entry("profiles/Rmc1x0.yaml"),
            Some(PathBuf::from("profiles").join("Rmc1x0.yaml"))
        );
    }

    #[test]
    fn service_files_that_have_no_business_in_a_backup_are_left_alone() {
        for name in [
            ".core-digests.json",
            ".geo-assets.json",
            "encryption.key",
            "mihomo.exe",
            "logs/latest.log",
            "profiles/nested/evil.yaml",
            "profiles/",
            "",
        ] {
            assert!(
                restorable_backup_entry(name).is_none(),
                "an entry outside the backup was accepted: {name:?}"
            );
        }
    }

    #[test]
    fn entries_that_try_to_leave_the_folder_are_refused() {
        for name in [
            "../verge.yaml",
            "../../etc/passwd",
            "profiles/../../verge.yaml",
            "profiles/..\\..\\verge.yaml",
            "/etc/passwd",
            "C:\\Windows\\System32\\drivers\\etc\\hosts",
        ] {
            assert!(
                restorable_backup_entry(name).is_none(),
                "a traversal entry was accepted: {name:?}"
            );
        }
    }
}
