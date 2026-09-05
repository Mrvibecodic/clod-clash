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

async fn run_dns_script(script_name: &str, args: Vec<String>, what: &str) -> bool {
    use crate::{core::handle, utils::dirs};
    use tauri_plugin_shell::{ShellExt as _, process::CommandEvent};

    logging!(info, Type::Config, "try to {what}");
    let resource_dir = match dirs::app_resources_dir() {
        Ok(dir) => dir,
        Err(e) => {
            logging!(error, Type::Config, "Failed to get resource directory: {}", e);
            return false;
        }
    };
    let script = resource_dir.join(script_name);
    if !script.exists() {
        logging!(error, Type::Config, "{script_name} not found");
        return false;
    }

    let mut command_args = Vec::with_capacity(args.len() + 1);
    command_args.push(script.to_string_lossy().into_owned());
    command_args.extend(args);

    let spawned = handle::Handle::app_handle()
        .shell()
        .command("bash")
        .args(command_args)
        .current_dir(resource_dir)
        .spawn();
    let (mut events, child) = match spawned {
        Ok(spawned) => spawned,
        Err(err) => {
            logging!(error, Type::Config, "{what} failed: {err}");
            return false;
        }
    };

    let terminated = async {
        while let Some(event) = events.recv().await {
            if let CommandEvent::Terminated(payload) = event {
                return payload.code;
            }
        }
        None
    };

    match tokio::time::timeout(SCRIPT_TIMEOUT, terminated).await {
        Err(_) => {
            logging!(error, Type::Config, "{what} timed out");
            if let Err(err) = child.kill() {
                logging!(error, Type::Config, "{what} could not be stopped: {err}");
            }
            false
        }
        Ok(Some(0)) => {
            logging!(info, Type::Config, "{what} successfully");
            true
        }
        Ok(code) => {
            logging!(error, Type::Config, "{what} failed: {}", code.unwrap_or(-1));
            false
        }
    }
}

fn state_arg() -> String {
    state_path()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default()
}

async fn set_public_dns_locked(dns_server: String) -> bool {
    run_dns_script("set_dns.sh", vec![dns_server, state_arg()], "set system dns").await
}

async fn restore_public_dns_locked() -> bool {
    run_dns_script("unset_dns.sh", vec![state_arg()], "unset system dns").await
}
