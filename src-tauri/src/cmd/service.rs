use super::{CmdResult, StringifyErr as _};
use crate::core::service::{self, SERVICE_MANAGER, ServiceStatus};
use crate::core::{CoreManager, manager::RunningMode};
use clash_verge_logging::{Type, logging};

async fn execute_service_operation_sync(status: ServiceStatus, op_type: &str) -> CmdResult {
    SERVICE_MANAGER
        .handle_service_status(status)
        .await
        .map_err(|e| format!("{op_type} Service failed: {e}").into())
}

/// Как прошло удаление службы — окно говорит о каждом шаге словами.
#[derive(Debug, Default, serde::Serialize)]
pub struct ServiceUninstall {
    /// Ядро под службой не остановилось — службу не трогали.
    stop_error: Option<String>,
    uninstall_error: Option<String>,
    /// Ядро, остановленное ради удаления, поднималось своим процессом.
    restarted: bool,
    restart_error: Option<String>,
}

/// Ядро, работавшее под службой, останавливается перед удалением и после
/// поднимается своим процессом — под плановой паузой, так что остановку никто
/// не принимает за падение. Ядро своим процессом не трогаем вовсе.
#[tauri::command]
pub async fn uninstall_service() -> CmdResult<ServiceUninstall> {
    crate::feat::refuse_while_exiting().stringify_err()?;
    let manager = CoreManager::global();
    let ran_under_service = matches!(*manager.get_running_mode(), RunningMode::Service);
    let _pause = ran_under_service.then(|| manager.planned_pause());
    Ok(run_uninstall(
        ran_under_service,
        async || manager.stop_core().await,
        async || execute_service_operation_sync(ServiceStatus::UninstallRequired, "Uninstall").await,
        // Перезапуском, а не стартом: пока спрашивали права, доставка конфига
        // могла поднять ядро снова под службой, и удалённая служба унесла его с
        // собой — старт счёл бы его работающим.
        async || {
            manager.restart_core().await?;
            crate::core::handle::Handle::refresh_clash();
            Ok(())
        },
    )
    .await)
}

/// Шаги удаления службы: остановка ядра (только под службой), удаление,
/// подъём ядра своим процессом (только под службой).
async fn run_uninstall(
    under_service: bool,
    stop: impl AsyncFnOnce() -> anyhow::Result<()>,
    uninstall: impl AsyncFnOnce() -> CmdResult,
    restart: impl AsyncFnOnce() -> anyhow::Result<()>,
) -> ServiceUninstall {
    let mut outcome = ServiceUninstall::default();
    if under_service && let Err(e) = stop().await {
        // Отказ остановки не говорит, легло ядро или живо. Служба остаётся:
        // удалённая, она оставила бы живое ядро без хозяина, а запуск поверх
        // поднял бы второй процесс.
        logging!(
            warn,
            Type::Service,
            "перед удалением службы ядро не остановилось, служба остаётся: {e:#}"
        );
        outcome.stop_error = Some(super::public_error_text(&e).to_string());
        return outcome;
    }
    outcome.uninstall_error = uninstall().await.err().map(|e| e.to_string());
    // Ядро остановили мы — поднимаем обратно, удалилась служба или нет.
    if under_service {
        outcome.restarted = true;
        if let Err(e) = restart().await {
            logging!(
                error,
                Type::Service,
                "после удаления службы ядро не поднялось своим процессом: {e:#}"
            );
            outcome.restart_error = Some(super::public_error_text(&e).to_string());
        }
    }
    outcome
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
    pub failure: Option<&'static str>,
}

#[tauri::command]
pub async fn get_tun_state() -> CmdResult<TunState> {
    let desired = crate::feat::tun::desired().await;
    let (capable, needs_repair) = crate::feat::tun::capability_and_repair().await;
    Ok(TunState {
        desired,
        active: crate::feat::tun::is_active_with(desired),
        capable,
        setup_declined: crate::feat::tun::setup_declined_for_this_version().await,
        needs_repair,
        runtime_stack: crate::feat::tun::runtime_stack().await,
        failure: crate::feat::tun::last_failure(),
    })
}

#[tauri::command]
pub async fn get_core_firewall_ok() -> CmdResult<Option<bool>> {
    Ok(firewall_platform::probe().await)
}

#[tauri::command]
pub async fn fix_core_firewall() -> CmdResult<Option<bool>> {
    crate::feat::refuse_while_exiting().stringify_err()?;
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
        // Правило могло быть причиной «TUN поднят, но трафик не идёт»: без
        // новой пробы плашка об этом висела бы до следующего круга сторожа.
        crate::feat::tun::recheck_traffic();
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
    use crate::feat::tun::SetupAnswer;
    crate::feat::refuse_while_exiting().stringify_err()?;
    match crate::feat::tun::ensure_ready(true).await.answer() {
        SetupAnswer::Ready => Ok(true),
        SetupAnswer::NotReady => Ok(false),
        SetupAnswer::Refused(marker) => Err(marker.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::{ServiceUninstall, run_uninstall};
    use std::sync::Mutex;

    /// Прогнать удаление с шагами-заглушками; `fails` — какие шаги откажут.
    /// Возвращает исход и шаги в том порядке, в каком их вызвали.
    async fn uninstall_with(under_service: bool, fails: &[&str]) -> (ServiceUninstall, Vec<&'static str>) {
        let called = Mutex::new(Vec::new());
        let step = |name: &'static str| {
            if let Ok(mut called) = called.lock() {
                called.push(name);
            }
            !fails.contains(&name)
        };
        let outcome = run_uninstall(
            under_service,
            async || {
                if step("stop") {
                    Ok(())
                } else {
                    Err(anyhow::anyhow!("stop refused"))
                }
            },
            async || {
                if step("uninstall") {
                    Ok(())
                } else {
                    Err("uninstall refused".into())
                }
            },
            async || {
                if step("restart") {
                    Ok(())
                } else {
                    Err(anyhow::anyhow!("restart refused"))
                }
            },
        )
        .await;
        (outcome, called.into_inner().unwrap_or_default())
    }

    #[tokio::test]
    async fn a_core_that_did_not_stop_keeps_its_service() {
        let (outcome, called) = uninstall_with(true, &["stop"]).await;
        assert_eq!(called, ["stop"], "служба не удаляется, ядро не поднимается");
        assert_eq!(outcome.stop_error.as_deref(), Some("stop refused"));
        assert!(!outcome.restarted);
        assert_eq!(outcome.uninstall_error, None);
        assert_eq!(outcome.restart_error, None);
    }

    #[tokio::test]
    async fn the_stopped_core_comes_back_even_if_the_service_stayed() {
        let (outcome, called) = uninstall_with(true, &["uninstall"]).await;
        assert_eq!(called, ["stop", "uninstall", "restart"]);
        assert_eq!(outcome.stop_error, None);
        assert_eq!(outcome.uninstall_error.as_deref(), Some("uninstall refused"));
        assert!(outcome.restarted);
        assert_eq!(outcome.restart_error, None);
    }

    #[tokio::test]
    async fn a_core_that_did_not_come_back_is_reported() {
        let (outcome, called) = uninstall_with(true, &["restart"]).await;
        assert_eq!(called, ["stop", "uninstall", "restart"]);
        assert_eq!(outcome.uninstall_error, None);
        assert!(outcome.restarted);
        assert_eq!(outcome.restart_error.as_deref(), Some("restart refused"));
    }

    #[tokio::test]
    async fn a_core_of_its_own_process_is_not_touched() {
        for fails in [&[][..], &["uninstall"][..]] {
            let (outcome, called) = uninstall_with(false, fails).await;
            assert_eq!(called, ["uninstall"], "{fails:?}");
            assert!(!outcome.restarted, "{fails:?}");
            assert_eq!(outcome.stop_error, None);
            assert_eq!(outcome.restart_error, None);
            assert_eq!(outcome.uninstall_error.is_some(), !fails.is_empty());
        }
    }

    #[test]
    fn the_core_comes_back_by_a_restart() {
        // Пока спрашивали права, доставка могла поднять ядро под службой снова:
        // старт счёл бы его работающим. Без живого ядра — только по исходнику.
        let source = crate::utils::source_scan::production_code(include_str!("service.rs"));
        let body = crate::utils::source_scan::fn_body(source, "pub async fn uninstall_service").unwrap_or_default();
        assert!(!body.is_empty(), "тело uninstall_service не найдено — тест ослеп");
        assert!(body.contains("manager.restart_core()"), "{body}");
        assert!(!body.contains(".start_core()"), "{body}");
    }
}
