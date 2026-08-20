use super::{CmdResult, StringifyErr as _};
use crate::core::service::{self, SERVICE_MANAGER, ServiceStatus};

async fn execute_service_operation_sync(status: ServiceStatus, op_type: &str) -> CmdResult {
    SERVICE_MANAGER
        .handle_service_status(status)
        .await
        .map_err(|e| format!("{op_type} Service failed: {e}").into())
}

#[tauri::command]
pub async fn install_service() -> CmdResult {
    execute_service_operation_sync(ServiceStatus::InstallRequired, "Install").await
}

#[tauri::command]
pub async fn uninstall_service() -> CmdResult {
    execute_service_operation_sync(ServiceStatus::UninstallRequired, "Uninstall").await
}

#[tauri::command]
pub async fn reinstall_service() -> CmdResult {
    execute_service_operation_sync(ServiceStatus::ReinstallRequired, "Reinstall").await
}

#[tauri::command]
pub async fn repair_service() -> CmdResult {
    execute_service_operation_sync(ServiceStatus::ForceReinstallRequired, "Repair").await
}

#[tauri::command]
pub async fn is_service_available() -> CmdResult<bool> {
    service::is_service_available().await.stringify_err()?;
    Ok(true)
}

#[derive(serde::Serialize)]
pub struct TunState {
    pub desired: bool,
    pub active: bool,
    pub capable: bool,
    pub setup_declined: bool,
    pub needs_repair: bool,
    pub runtime_stack: Option<String>,
}

#[tauri::command]
pub async fn get_tun_state() -> CmdResult<TunState> {
    let desired = crate::feat::tun::desired().await;
    Ok(TunState {
        desired,
        active: crate::feat::tun::is_active_with(desired),
        capable: crate::feat::tun::is_capable().await,
        setup_declined: crate::feat::tun::setup_declined_for_this_version().await,
        needs_repair: crate::feat::tun::needs_repair().await,
        runtime_stack: crate::feat::tun::runtime_stack().await,
    })
}

#[tauri::command]
pub async fn get_core_firewall_ok() -> CmdResult<Option<bool>> {
    Ok(firewall_platform::probe().await)
}

#[tauri::command]
pub async fn fix_core_firewall() -> CmdResult<Option<bool>> {
    firewall_platform::repair().await
}

#[cfg(target_os = "windows")]
mod firewall_platform {
    use crate::cmd::{CmdResult, StringifyErr as _};

    pub async fn probe() -> Option<bool> {
        crate::core::firewall::inbound_allowed().await
    }

    pub async fn repair() -> CmdResult<Option<bool>> {
        crate::core::firewall::allow_inbound().await.stringify_err()?;
        Ok(crate::core::firewall::inbound_allowed().await)
    }
}

#[cfg(not(target_os = "windows"))]
mod firewall_platform {
    use super::CmdResult;

    #[allow(clippy::unused_async)]
    pub async fn probe() -> Option<bool> {
        None
    }

    #[allow(clippy::unused_async, clippy::unnecessary_wraps)]
    pub async fn repair() -> CmdResult<Option<bool>> {
        Ok(None)
    }
}

#[tauri::command]
pub async fn ensure_tun_ready() -> CmdResult<bool> {
    use crate::feat::tun::SetupOutcome;
    match crate::feat::tun::ensure_ready(true).await {
        SetupOutcome::AlreadyReady | SetupOutcome::Installed => Ok(true),
        SetupOutcome::Declined | SetupOutcome::Failed => Ok(false),
        SetupOutcome::Pending => Err(
            "The system authorisation dialog is still open. TUN will turn on once the background service is installed."
                .into(),
        ),
    }
}
