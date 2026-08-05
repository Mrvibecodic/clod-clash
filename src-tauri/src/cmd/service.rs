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

// clod:tun-ready — состояние TUN как его видит бэкенд. Интерфейсу нужен не
// флаг из конфига (желание), а факт: работает ли туннель прямо сейчас и можно
// ли его вообще поднять на этой машине.
#[derive(serde::Serialize)]
pub struct TunState {
    /// Пользователь хочет TUN (сохранённая настройка).
    pub desired: bool,
    /// TUN реально подан ядру: желание есть и режим не подавлен.
    pub active: bool,
    /// Прав хватает: приложение привилегировано или служба отвечает.
    pub capable: bool,
    /// Автоматическую настройку службы на этой версии уже пробовали (отклонили,
    /// провалили или довели до конца).
    pub setup_declined: bool,
}

#[tauri::command]
pub async fn get_tun_state() -> CmdResult<TunState> {
    let desired = crate::feat::tun::desired().await;
    Ok(TunState {
        desired,
        active: desired && !crate::feat::tun::is_suppressed(),
        capable: crate::feat::tun::is_capable().await,
        setup_declined: crate::feat::tun::setup_declined_for_this_version().await,
    })
}

// clod:tun-ready — «сделай так, чтобы TUN работал»: если прав уже хватает,
// ничего не происходит; иначе ставится служба (один запрос прав). Отдельно от
// install_service, потому что вызывается из тумблера TUN, а не из ремонта.
#[tauri::command]
pub async fn ensure_tun_ready() -> CmdResult<bool> {
    use crate::feat::tun::SetupOutcome;
    match crate::feat::tun::ensure_ready(true).await {
        SetupOutcome::AlreadyReady | SetupOutcome::Installed => Ok(true),
        SetupOutcome::Declined | SetupOutcome::Failed => Ok(false),
        // clod:tun-deadline — «ещё идёт» — это не `false`. `false` в интерфейсе
        // означает «TUN на этой машине недоступен» (тумблер обратно, красная
        // плашка), а здесь установка жива и системный диалог всё ещё ждёт
        // ответа. Сигнатуру не трогаем: ошибку все три вызывающих места уже
        // умеют показывать, и, в отличие от `false`, она ничего не отменяет.
        SetupOutcome::Pending => Err(
            "The system authorisation dialog is still open. TUN will turn on once the background service is installed."
                .into(),
        ),
    }
}
