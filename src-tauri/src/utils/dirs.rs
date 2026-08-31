use crate::core::{CoreManager, handle, manager::RunningMode};
use anyhow::Result;
use async_trait::async_trait;
use clash_verge_logging::{Type, logging};
use once_cell::sync::OnceCell;
#[cfg(unix)]
use std::iter;
#[cfg(unix)]
use std::path::Path;
use std::{fs, path::PathBuf};
use tauri::Manager as _;

#[cfg(not(feature = "verge-dev"))]
pub static APP_ID: &str = "io.clodclash.app";
#[cfg(not(feature = "verge-dev"))]
pub static BACKUP_DIR: &str = "clod-clash-backup";

#[cfg(feature = "verge-dev")]
pub static APP_ID: &str = "io.clodclash.app.dev";
#[cfg(feature = "verge-dev")]
pub static BACKUP_DIR: &str = "clod-clash-backup-dev";

pub static PORTABLE_FLAG: OnceCell<bool> = OnceCell::new();

pub static CLASH_CONFIG: &str = "config.yaml";
pub static VERGE_CONFIG: &str = "verge.yaml";
pub static PROFILE_YAML: &str = "profiles.yaml";

/// init portable flag
pub fn init_portable_flag() -> Result<()> {
    use tauri::utils::platform::current_exe;

    let app_exe = current_exe()?;
    if let Some(dir) = app_exe.parent() {
        let dir = PathBuf::from(dir).join(".config/PORTABLE");

        if dir.exists() {
            PORTABLE_FLAG.get_or_init(|| true);
        }
    }
    PORTABLE_FLAG.get_or_init(|| false);
    Ok(())
}

/// get the verge app home dir
pub fn app_home_dir() -> Result<PathBuf> {
    use tauri::utils::platform::current_exe;

    let flag = PORTABLE_FLAG.get().unwrap_or(&false);
    if *flag {
        let app_exe = current_exe()?;
        let app_exe = dunce::canonicalize(app_exe)?;
        let app_dir = app_exe
            .parent()
            .ok_or_else(|| anyhow::anyhow!("failed to get the portable app dir"))?;
        return Ok(PathBuf::from(app_dir).join(".config").join(APP_ID));
    }

    // Избегаем падения, если Handle ещё не инициализирован
    let app_handle = handle::Handle::app_handle();

    match app_handle.path().data_dir() {
        Ok(dir) => Ok(dir.join(APP_ID)),
        Err(e) => {
            logging!(error, Type::File, "Failed to get the app home directory: {e}");
            Err(anyhow::anyhow!("Failed to get the app homedirectory"))
        }
    }
}

/// get the resources dir
pub fn app_resources_dir() -> Result<PathBuf> {
    // Избегаем падения, если Handle ещё не инициализирован
    let app_handle = handle::Handle::app_handle();

    match app_handle.path().resource_dir() {
        Ok(dir) => Ok(dir.join("resources")),
        Err(e) => {
            logging!(error, Type::File, "Failed to get the resource directory: {e}");
            Err(anyhow::anyhow!("Failed to get the resource directory"))
        }
    }
}

/// profiles dir
pub fn app_profiles_dir() -> Result<PathBuf> {
    Ok(app_home_dir()?.join("profiles"))
}

/// icons dir
pub fn app_icons_dir() -> Result<PathBuf> {
    Ok(app_home_dir()?.join("icons"))
}

/// logs dir
pub fn app_logs_dir() -> Result<PathBuf> {
    Ok(app_home_dir()?.join("logs"))
}

/// service logs dir
#[cfg(target_os = "macos")]
pub fn service_logs_root_dir() -> Result<PathBuf> {
    Ok(app_home_dir()?.join("service-logs"))
}

/// service logs dir
#[cfg(not(target_os = "macos"))]
pub fn service_logs_root_dir() -> Result<PathBuf> {
    app_logs_dir()
}

// latest verge log
pub fn app_latest_log() -> Result<PathBuf> {
    Ok(app_logs_dir()?.join("latest.log"))
}

/// local backups dir
pub fn local_backup_dir() -> Result<PathBuf> {
    let dir = app_home_dir()?.join(BACKUP_DIR);
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub fn clash_path() -> Result<PathBuf> {
    Ok(app_home_dir()?.join(CLASH_CONFIG))
}

pub fn verge_path() -> Result<PathBuf> {
    Ok(app_home_dir()?.join(VERGE_CONFIG))
}

pub fn profiles_path() -> Result<PathBuf> {
    Ok(app_home_dir()?.join(PROFILE_YAML))
}

#[cfg(target_os = "macos")]
pub fn service_path() -> Result<PathBuf> {
    let res_dir = app_resources_dir()?;
    Ok(res_dir.join("clash-verge-service"))
}

#[cfg(windows)]
pub fn service_path() -> Result<PathBuf> {
    let res_dir = app_resources_dir()?;
    Ok(res_dir.join("clash-verge-service.exe"))
}

pub fn sidecar_log_dir() -> Result<PathBuf> {
    let log_dir = app_logs_dir()?.join("sidecar");
    let _ = std::fs::create_dir_all(&log_dir);

    Ok(log_dir)
}

pub fn service_log_dir() -> Result<PathBuf> {
    let log_dir = service_logs_root_dir()?.join("service");
    let _ = std::fs::create_dir_all(&log_dir);

    Ok(log_dir)
}

pub fn clash_latest_log() -> Result<PathBuf> {
    match *CoreManager::global().get_running_mode() {
        RunningMode::Service => Ok(service_log_dir()?.join("service_latest.log")),
        RunningMode::Sidecar | RunningMode::NotRunning => Ok(sidecar_log_dir()?.join("sidecar_latest.log")),
    }
}

pub fn path_to_str(path: &PathBuf) -> Result<&str> {
    let path_str = path
        .as_os_str()
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("failed to get path from {:?}", path))?;
    Ok(path_str)
}

pub fn get_encryption_key() -> Result<Vec<u8>> {
    let app_dir = app_home_dir()?;
    let key_path = app_dir.join(".encryption_key");

    if key_path.exists() {
        // Read existing key
        fs::read(&key_path).map_err(|e| anyhow::anyhow!("Failed to read encryption key: {}", e))
    } else {
        // Generate and save new key
        let mut key = vec![0u8; 32];
        getrandom::fill(&mut key)?;

        // Ensure directory exists
        if let Some(parent) = key_path.parent() {
            fs::create_dir_all(parent).map_err(|e| anyhow::anyhow!("Failed to create key directory: {}", e))?;
        }
        // Save key
        fs::write(&key_path, &key).map_err(|e| anyhow::anyhow!("Failed to save encryption key: {}", e))?;
        Ok(key)
    }
}

#[cfg(unix)]
pub fn ensure_mihomo_safe_dir() -> Option<PathBuf> {
    iter::once("/tmp")
        .map(PathBuf::from)
        .find(|path| path.exists())
        .or_else(|| {
            std::env::var_os("HOME").and_then(|home| {
                let home_config = PathBuf::from(home).join(".config");
                if home_config.exists() || fs::create_dir_all(&home_config).is_ok() {
                    Some(home_config)
                } else {
                    logging!(error, Type::File, "Failed to create safe directory: {home_config:?}");
                    None
                }
            })
        })
}

#[cfg(unix)]
fn owner_ipc_suffix() -> String {
    crate::core::owner_identity::current_owner_identity()
        .map(|identity| clash_verge_service_ipc::owner_key(&identity))
        .unwrap_or_else(|_| unsafe { tauri_plugin_clash_verge_sysinfo::libc::geteuid() }.to_string())
}

#[cfg(unix)]
fn ensure_private_dir(dir: &Path) -> Result<()> {
    use std::os::unix::fs::{DirBuilderExt as _, MetadataExt as _, PermissionsExt as _};

    if let Err(error) = fs::DirBuilder::new().mode(0o700).create(dir)
        && error.kind() != std::io::ErrorKind::AlreadyExists
    {
        return Err(error.into());
    }

    let meta = fs::symlink_metadata(dir)?;
    if !meta.is_dir() {
        anyhow::bail!("{dir:?} занят не каталогом");
    }
    if meta.uid() != unsafe { tauri_plugin_clash_verge_sysinfo::libc::geteuid() } {
        anyhow::bail!("{dir:?} принадлежит другому пользователю");
    }
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(unix)]
const IPC_SOCKET_PATH_LIMIT: usize = 100;

#[cfg(unix)]
fn socket_in_private_dir(dir: &Path) -> Result<PathBuf> {
    let socket = dir.join("verge-mihomo.sock");
    if socket.as_os_str().len() > IPC_SOCKET_PATH_LIMIT {
        anyhow::bail!("путь сокета {socket:?} длиннее допустимого");
    }
    ensure_private_dir(dir)?;
    Ok(socket)
}

#[cfg(unix)]
pub fn sidecar_ipc_path() -> Result<PathBuf> {
    let flavor = if cfg!(feature = "verge-dev") { "dev" } else { "release" };
    let dir_name = format!("verge-{flavor}-{}", owner_ipc_suffix());
    let mut last_error = None;

    if let Some(dir) = ensure_mihomo_safe_dir().map(|base_dir| base_dir.join(&dir_name)) {
        match socket_in_private_dir(&dir) {
            Ok(socket) => return Ok(socket),
            Err(error) => last_error = Some(error),
        }
    }

    if let Some(error) = last_error.as_ref() {
        logging!(
            warn,
            Type::File,
            "каталог для сокета ядра недоступен ({}), берём запасной",
            error
        );
    }

    let unique_name = format!("{dir_name}-{}", std::process::id());
    if let Some(dir) = ensure_mihomo_safe_dir().map(|base_dir| base_dir.join(&unique_name)) {
        match socket_in_private_dir(&dir) {
            Ok(socket) => return Ok(socket),
            Err(error) => last_error = Some(error),
        }
    }

    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("Failed to determine ipc path")))
}

#[cfg(target_os = "windows")]
pub fn sidecar_ipc_path() -> Result<PathBuf> {
    let identity = crate::core::owner_identity::current_owner_identity()?;
    let flavor = if cfg!(feature = "verge-dev") { "dev" } else { "release" };
    Ok(PathBuf::from(format!(
        r"\\.\pipe\verge-mihomo-sidecar-{flavor}-{}",
        clash_verge_service_ipc::owner_key(&identity)
    )))
}

/// clod:svc-2.6 — где слушает API ядро, запущенное СЛУЖБОЙ.
///
/// Служба назначает этот путь сама и передаёт его ядру CLI-флагом, перекрывая
/// `external-controller-*` из нашего yaml; подключаться в service-режиме надо
/// именно сюда, а не к sidecar-пути выше — иначе API ядра недостижим.
pub fn service_ipc_path() -> Result<PathBuf> {
    Ok(PathBuf::from(clash_verge_service_ipc::mihomo_ipc_path(
        &crate::core::owner_identity::current_owner_identity()?,
    )))
}
#[async_trait]
pub trait PathBufExec {
    async fn remove_if_exists(&self) -> Result<()>;
}

#[async_trait]
impl PathBufExec for PathBuf {
    async fn remove_if_exists(&self) -> Result<()> {
        if self.exists() {
            tokio::fs::remove_file(self).await?;
            logging!(info, Type::File, "Removed file: {:?}", self);
        }
        Ok(())
    }
}
