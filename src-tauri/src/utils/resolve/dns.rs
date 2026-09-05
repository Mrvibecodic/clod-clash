#![cfg(target_os = "macos")]

use clash_verge_logging::{Type, logging};
use std::time::Duration;
use tokio::sync::Mutex;

const STATE_FILE: &str = "original_dns.txt";
const SCRIPT_TIMEOUT: Duration = Duration::from_secs(10);

static OVERRIDE_LOCK: Mutex<()> = Mutex::const_new(());

fn state_path() -> Option<std::path::PathBuf> {
    crate::utils::dirs::app_home_dir().ok().map(|dir| dir.join(STATE_FILE))
}

pub fn has_pending_restore() -> bool {
    state_path().is_some_and(|path| path.exists())
}

pub async fn restore_public_dns_if_pending() {
    let _serialized = OVERRIDE_LOCK.lock().await;
    if !has_pending_restore() {
        return;
    }
    logging!(
        warn,
        Type::Config,
        "system DNS was left overridden by a previous run; restoring"
    );
    restore_public_dns_locked().await;
}

pub async fn sync_override(wanted: bool, dns_server: String) {
    let _serialized = OVERRIDE_LOCK.lock().await;
    let overridden = has_pending_restore();
    if wanted && !overridden {
        set_public_dns_locked(dns_server).await;
    } else if !wanted && overridden {
        restore_public_dns_locked().await;
    }
}

pub async fn restore_public_dns() -> bool {
    let _serialized = OVERRIDE_LOCK.lock().await;
    if !has_pending_restore() {
        return true;
    }
    restore_public_dns_locked().await
}

async fn set_public_dns_locked(dns_server: String) -> bool {
    use crate::{core::handle, utils::dirs};
    use tauri_plugin_shell::ShellExt as _;
    let app_handle = handle::Handle::app_handle();

    logging!(info, Type::Config, "try to set system dns");
    let resource_dir = match dirs::app_resources_dir() {
        Ok(dir) => dir,
        Err(e) => {
            logging!(error, Type::Config, "Failed to get resource directory: {}", e);
            return false;
        }
    };
    let script = resource_dir.join("set_dns.sh");
    if !script.exists() {
        logging!(error, Type::Config, "set_dns.sh not found");
        return false;
    }
    let script = script.to_string_lossy().into_owned();
    let state = state_path()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ran = app_handle
        .shell()
        .command("bash")
        .args([script, dns_server, state])
        .current_dir(resource_dir)
        .status();
    match tokio::time::timeout(SCRIPT_TIMEOUT, ran).await {
        Err(_) => {
            logging!(error, Type::Config, "set system dns timed out");
            false
        }
        Ok(outcome) => match outcome {
            Ok(status) => {
                if status.success() {
                    logging!(info, Type::Config, "set system dns successfully");
                    true
                } else {
                    let code = status.code().unwrap_or(-1);
                    logging!(error, Type::Config, "set system dns failed: {code}");
                    false
                }
            }
            Err(err) => {
                logging!(error, Type::Config, "set system dns failed: {err}");
                false
            }
        },
    }
}

async fn restore_public_dns_locked() -> bool {
    use crate::{core::handle, utils::dirs};
    use tauri_plugin_shell::ShellExt as _;
    let app_handle = handle::Handle::app_handle();

    logging!(info, Type::Config, "try to unset system dns");
    let resource_dir = match dirs::app_resources_dir() {
        Ok(dir) => dir,
        Err(e) => {
            logging!(error, Type::Config, "Failed to get resource directory: {}", e);
            return false;
        }
    };
    let script = resource_dir.join("unset_dns.sh");
    if !script.exists() {
        logging!(error, Type::Config, "unset_dns.sh not found");
        return false;
    }
    let script = script.to_string_lossy().into_owned();
    let state = state_path()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ran = app_handle
        .shell()
        .command("bash")
        .args([script, state])
        .current_dir(resource_dir)
        .status();
    match tokio::time::timeout(SCRIPT_TIMEOUT, ran).await {
        Err(_) => {
            logging!(error, Type::Config, "unset system dns timed out");
            false
        }
        Ok(outcome) => match outcome {
            Ok(status) => {
                if status.success() {
                    logging!(info, Type::Config, "unset system dns successfully");
                    true
                } else {
                    let code = status.code().unwrap_or(-1);
                    logging!(error, Type::Config, "unset system dns failed: {code}");
                    false
                }
            }
            Err(err) => {
                logging!(error, Type::Config, "unset system dns failed: {err}");
                false
            }
        },
    }
}
