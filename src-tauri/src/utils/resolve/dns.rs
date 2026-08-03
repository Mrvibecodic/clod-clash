//! clod: подмена системного DNS нужна только на macOS — там TUN не
//! перехватывает запросы, которые система шлёт мимо интерфейса.
#![cfg(target_os = "macos")]

use clash_verge_logging::{Type, logging};

/// clod: файл-«надгробие» с исходным DNS живёт рядом с конфигами, а не в
/// ресурсах приложения: ресурсы переживают не каждое обновление и могут быть
/// только для чтения, а восстановить DNS надо в том числе после того, как
/// приложение убили.
const STATE_FILE: &str = "original_dns.txt";

fn state_path() -> Option<std::path::PathBuf> {
    crate::utils::dirs::app_home_dir().ok().map(|dir| dir.join(STATE_FILE))
}

/// Подменяли ли мы системный DNS и ещё не вернули его.
pub fn has_pending_restore() -> bool {
    state_path().is_some_and(|path| path.exists())
}

/// clod: вызывается на старте. Если файл состояния остался с прошлого запуска,
/// значит приложение не успело вернуть DNS — упало, было убито или выключили
/// питание. Возвращаем сейчас, иначе весь резолв так и уйдёт на чужой сервер.
pub async fn restore_public_dns_if_pending() {
    if !has_pending_restore() {
        return;
    }
    logging!(
        warn,
        Type::Config,
        "system DNS was left overridden by a previous run; restoring"
    );
    restore_public_dns().await;
}

pub async fn set_public_dns(dns_server: String) {
    use crate::{core::handle, utils::dirs};
    use tauri_plugin_shell::ShellExt as _;
    let app_handle = handle::Handle::app_handle();

    logging!(info, Type::Config, "try to set system dns");
    let resource_dir = match dirs::app_resources_dir() {
        Ok(dir) => dir,
        Err(e) => {
            logging!(error, Type::Config, "Failed to get resource directory: {}", e);
            return;
        }
    };
    let script = resource_dir.join("set_dns.sh");
    if !script.exists() {
        logging!(error, Type::Config, "set_dns.sh not found");
        return;
    }
    let script = script.to_string_lossy().into_owned();
    let state = state_path()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default();
    match app_handle
        .shell()
        .command("bash")
        .args([script, dns_server, state])
        .current_dir(resource_dir)
        .status()
        .await
    {
        Ok(status) => {
            if status.success() {
                logging!(info, Type::Config, "set system dns successfully");
            } else {
                let code = status.code().unwrap_or(-1);
                logging!(error, Type::Config, "set system dns failed: {code}");
            }
        }
        Err(err) => {
            logging!(error, Type::Config, "set system dns failed: {err}");
        }
    }
}

pub async fn restore_public_dns() {
    use crate::{core::handle, utils::dirs};
    use tauri_plugin_shell::ShellExt as _;
    let app_handle = handle::Handle::app_handle();
    logging!(info, Type::Config, "try to unset system dns");
    let resource_dir = match dirs::app_resources_dir() {
        Ok(dir) => dir,
        Err(e) => {
            logging!(error, Type::Config, "Failed to get resource directory: {}", e);
            return;
        }
    };
    let script = resource_dir.join("unset_dns.sh");
    if !script.exists() {
        logging!(error, Type::Config, "unset_dns.sh not found");
        return;
    }
    let script = script.to_string_lossy().into_owned();
    let state = state_path()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default();
    match app_handle
        .shell()
        .command("bash")
        .args([script, state])
        .current_dir(resource_dir)
        .status()
        .await
    {
        Ok(status) => {
            if status.success() {
                logging!(info, Type::Config, "unset system dns successfully");
            } else {
                let code = status.code().unwrap_or(-1);
                logging!(error, Type::Config, "unset system dns failed: {code}");
            }
        }
        Err(err) => {
            logging!(error, Type::Config, "unset system dns failed: {err}");
        }
    }
}
