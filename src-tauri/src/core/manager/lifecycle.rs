use super::{CoreManager, RunningMode};
use crate::cmd::StringifyErr as _;
use crate::config::{Config, IVerge};
use crate::core::handle::Handle;
use crate::core::manager::CLASH_LOGGER;
use crate::core::service::{SERVICE_MANAGER, ServiceStatus};
use anyhow::Result;
use clash_verge_logging::{Type, logging};
use scopeguard::defer;
use smartstring::alias::String;
use tauri_plugin_clash_verge_sysinfo;
use tauri_plugin_clash_verge_sysinfo::is_current_app_handle_admin;

const fn should_wait_for_service(tun_enabled: bool, service_ready: bool, is_admin: bool) -> bool {
    tun_enabled && !service_ready && !is_admin
}

/// Результат передачи sidecar→service
enum HandoffOutcome {
    /// Служба ещё не готова
    NotReady,
    /// Передача завершена или не требуется
    Done,
    /// Передача не удалась, выполнен откат
    Failed,
}

impl CoreManager {
    pub async fn start_core(&self) -> Result<()> {
        let _life = self.lifecycle_lock.lock().await;
        self.start_core_inner().await
    }

    /// Вызывающий должен уже удерживать `lifecycle_lock`.
    async fn start_core_inner(&self) -> Result<()> {
        // При завершении работы новое ядро больше не запускается.
        if Handle::global().is_exiting() {
            return Ok(());
        }

        // Идемпотентность при уже работающем ядре; для рестарта использовать restart_core.
        if !matches!(*self.get_running_mode(), RunningMode::NotRunning) {
            logging!(
                info,
                Type::Core,
                "start_core called while a core is running; treated as no-op"
            );
            return Ok(());
        }

        self.prepare_startup().await;
        defer! {
            self.after_core_process();
        }

        // Во время ожидания службы может начаться завершение работы; откатываем
        // состояние, если фактического запуска не произошло.
        if Handle::global().is_exiting() {
            self.set_running_mode(RunningMode::NotRunning);
            return Ok(());
        }

        let mut result = match *self.get_running_mode() {
            RunningMode::Service => self.start_core_by_service().await,
            RunningMode::NotRunning | RunningMode::Sidecar => self.start_core_by_sidecar().await,
        };

        // clod:tun-ready — служба может отказать (сломана, старой версии,
        // пользователь отклонил переустановку). Раньше это означало «ядро не
        // запущено вообще», то есть отсутствие интернета вместо потери TUN.
        // Падаем в sidecar и продолжаем ждать службу в фоне.
        if let Err(e) = &result
            && matches!(*self.get_running_mode(), RunningMode::Service)
        {
            logging!(
                warn,
                Type::Core,
                "service start failed ({}); falling back to sidecar",
                e
            );
            result = self.start_core_by_sidecar().await;
        }

        // При ошибке запуска откатываем mode, чтобы разрешить повторную попытку.
        if result.is_err() {
            self.set_running_mode(RunningMode::NotRunning);
            return result;
        }

        // clod:tun-ready — проверяем факт, а не заявку: если ядро не смогло
        // поднять устройство, честно гасим TUN и говорим об этом.
        if crate::feat::tun::desired().await && !crate::feat::tun::is_suppressed() {
            crate::feat::tun::spawn_start_verification();
        }

        // После отката к sidecar в фоне ждём готовности службы для передачи
        if matches!(*self.get_running_mode(), RunningMode::Sidecar) {
            self.spawn_service_handoff_watcher().await;
        }

        result
    }

    pub async fn stop_core(&self) -> Result<()> {
        let _life = self.lifecycle_lock.lock().await;
        self.stop_core_inner().await
    }

    /// Вызывающий должен уже удерживать `lifecycle_lock`.
    async fn stop_core_inner(&self) -> Result<()> {
        CLASH_LOGGER.clear_logs().await;
        defer! {
            self.after_core_process();
        }

        match *self.get_running_mode() {
            RunningMode::Service => self.stop_core_by_service().await,
            RunningMode::Sidecar => {
                self.stop_core_by_sidecar();
                Ok(())
            }
            RunningMode::NotRunning => Ok(()),
        }
    }

    pub async fn restart_core(&self) -> Result<()> {
        // Блокировка удерживается на весь stop+start, чтобы избежать вклинивания
        // других операций жизненного цикла.
        let _life = self.lifecycle_lock.lock().await;
        logging!(info, Type::Core, "Restarting core");
        self.stop_core_inner().await?;
        self.start_core_inner().await
    }

    /// clod:core-updater — stop the core, run `swap` (pointer writes on disk),
    /// start again; when the new core fails to start, run `rollback` and start
    /// once more. The lifecycle lock is held across the whole sequence so no
    /// concurrent lifecycle operation can slip in between stop and start and
    /// resurrect the old binary mid-swap. Every error path attempts to leave
    /// a core running — an update must never cost the user their connection.
    pub async fn restart_core_swapped(
        &self,
        swap: impl FnOnce() -> Result<()> + Send,
        rollback: impl FnOnce() -> Result<()> + Send,
    ) -> Result<()> {
        let _life = self.lifecycle_lock.lock().await;
        self.stop_core_inner().await?;

        if let Err(swap_error) = swap() {
            // Nothing switched; bring the old core back before reporting.
            if let Err(start_error) = self.start_core_inner().await {
                return Err(swap_error.context(format!("and restarting the old core failed too: {start_error:#}")));
            }
            return Err(swap_error);
        }

        match self.start_core_inner().await {
            Ok(()) => Ok(()),
            Err(start_error) => {
                logging!(
                    error,
                    Type::Core,
                    "new core failed to start, rolling back: {start_error:#}"
                );
                let rollback_result = rollback();
                // Attempt a start even when the rollback write failed — a core
                // resolved through whatever pointers remain (or the sidecar
                // fallback) still beats no core at all.
                let restart_result = self.start_core_inner().await;
                match (rollback_result, restart_result) {
                    (Ok(()), Ok(())) => {
                        Err(start_error.context("the new core failed to start; the previous one is back"))
                    }
                    (Ok(()), Err(restart_error)) => Err(start_error.context(format!(
                        "and restarting the previous core failed too: {restart_error:#}"
                    ))),
                    (Err(rollback_error), Ok(())) => Err(start_error.context(format!(
                        "the rollback write failed ({rollback_error:#}) but a core is running again"
                    ))),
                    (Err(rollback_error), Err(restart_error)) => Err(start_error.context(format!(
                        "the rollback failed ({rollback_error:#}) and so did the restart: {restart_error:#}"
                    ))),
                }
            }
        }
    }

    pub async fn change_core(&self, clash_core: &String) -> Result<(), String> {
        if !IVerge::VALID_CLASH_CORES.contains(&clash_core.as_str()) {
            return Err(format!("Invalid clash core: {}", clash_core).into());
        }

        Config::verge().await.edit_draft(|d| {
            d.clash_core = Some(clash_core.to_owned());
        });
        Config::verge().await.apply();

        let verge_data = Config::verge().await.latest_arc();
        verge_data.save_file().await.map_err(|e| e.to_string())?;

        self.update_config_checked().await.stringify_err()?;
        Ok(())
    }

    async fn prepare_startup(&self) {
        self.wait_for_service_if_needed().await;
        self.set_running_mode(match SERVICE_MANAGER.current().await {
            ServiceStatus::Ready => RunningMode::Service,
            _ => RunningMode::Sidecar,
        });
    }

    fn after_core_process(&self) {
        let app_handle = Handle::app_handle();
        tauri_plugin_clash_verge_sysinfo::set_app_core_mode(app_handle, self.get_running_mode().to_string());
    }

    async fn wait_for_service_if_needed(&self) {
        use crate::{config::Config, constants::timing, core::service};
        use backon::{ConstantBuilder, Retryable as _};

        let tun_enabled = Config::verge().await.latest_arc().enable_tun_mode.unwrap_or(false);
        let service_ready = matches!(SERVICE_MANAGER.current().await, ServiceStatus::Ready);
        let is_admin = is_current_app_handle_admin(Handle::app_handle());

        if !should_wait_for_service(tun_enabled, service_ready, is_admin) {
            if tun_enabled && !service_ready && is_admin {
                logging!(
                    info,
                    Type::Core,
                    "service unavailable while app is elevated; starting sidecar immediately"
                );
            }
            return;
        }

        let max_times = timing::SERVICE_WAIT_MAX.as_millis() / timing::SERVICE_WAIT_INTERVAL.as_millis();
        let backoff = ConstantBuilder::default()
            .with_delay(timing::SERVICE_WAIT_INTERVAL)
            .with_max_times(max_times as usize);

        let _ = (|| async {
            if matches!(SERVICE_MANAGER.current().await, ServiceStatus::Ready) {
                return Ok(());
            }

            // If the service IPC path is not ready yet, treat it as transient and retry.
            // Running init/refresh too early can mark service state unavailable and break later config reloads.
            if !service::is_service_ipc_path_exists() {
                return Err(anyhow::anyhow!("Service IPC not ready"));
            }

            SERVICE_MANAGER.init().await?;
            let _ = SERVICE_MANAGER.refresh().await;

            if matches!(SERVICE_MANAGER.current().await, ServiceStatus::Ready) {
                Ok(())
            } else {
                Err(anyhow::anyhow!("Service not ready"))
            }
        })
        .retry(backoff)
        .await;
    }

    /// clod:tun-ready — служба появилась (например, мы её только что
    /// установили): переезжаем на неё сразу, не дожидаясь окна watcher-а.
    pub async fn handoff_to_service_if_needed(&self) {
        if !matches!(*self.get_running_mode(), RunningMode::Sidecar) {
            return;
        }
        if !crate::feat::tun::desired().await {
            return;
        }
        match self.try_handoff_sidecar_to_service().await {
            HandoffOutcome::Done => {}
            HandoffOutcome::NotReady => self.spawn_service_handoff_watcher().await,
            HandoffOutcome::Failed => {
                logging!(warn, Type::Core, "immediate handoff failed; staying in sidecar mode");
            }
        }
    }

    /// Ждёт готовности службы в течение окна времени, затем передаёт от sidecar к service
    async fn spawn_service_handoff_watcher(&self) {
        use crate::constants::timing;
        use crate::process::AsyncHandler;
        use std::sync::atomic::Ordering;
        use std::time::Instant;

        // Передача службе нужна только в режиме TUN
        let needs_service = Config::verge().await.latest_arc().enable_tun_mode.unwrap_or(false);
        if !needs_service {
            return;
        }

        // Синглтон, чтобы избежать параллельной передачи
        if self.handoff_watcher_running.swap(true, Ordering::AcqRel) {
            return;
        }

        logging!(
            info,
            Type::Core,
            "service not ready at startup; sidecar active, watching for handoff"
        );

        AsyncHandler::spawn(|| async move {
            let manager = Self::global();
            let started = Instant::now();
            loop {
                if started.elapsed() >= timing::SERVICE_HANDOFF_WINDOW {
                    logging!(
                        info,
                        Type::Core,
                        "service handoff window elapsed; staying in sidecar mode"
                    );
                    break;
                }
                tokio::time::sleep(timing::SERVICE_HANDOFF_INTERVAL).await;

                // Выходим, если режим уже изменился
                if !matches!(*manager.get_running_mode(), RunningMode::Sidecar) {
                    break;
                }
                match manager.try_handoff_sidecar_to_service().await {
                    // Передано или не требуется
                    HandoffOutcome::Done => break,
                    // Откат к sidecar выполнен, прекращаем попытки
                    HandoffOutcome::Failed => {
                        logging!(warn, Type::Core, "handoff attempt failed; staying in sidecar mode");
                        break;
                    }
                    HandoffOutcome::NotReady => {}
                }
            }
            manager.handoff_watcher_running.store(false, Ordering::Release);
        });
    }

    /// После готовности службы останавливает sidecar и перезапускает ядро через service
    async fn try_handoff_sidecar_to_service(&self) -> HandoffOutcome {
        use crate::core::service;

        // Принудительно обновляем состояние службы, чтобы кэшированное состояние
        // не блокировало передачу
        if !service::is_service_ipc_path_exists() {
            return HandoffOutcome::NotReady;
        }
        if SERVICE_MANAGER.init().await.is_err() {
            return HandoffOutcome::NotReady;
        }
        let _ = SERVICE_MANAGER.refresh().await;
        if !matches!(SERVICE_MANAGER.current().await, ServiceStatus::Ready) {
            return HandoffOutcome::NotReady;
        }

        // Сначала захватываем блокировку config; при неудаче уступаем идущему обновлению.
        if !self.try_start_config_update() {
            return HandoffOutcome::NotReady;
        }
        defer! {
            self.finish_config_update();
        }

        // Затем захватываем блокировку lifecycle; порядок блокировок фиксирован: config→lifecycle.
        let _life = self.lifecycle_lock.lock().await;

        // После захвата блокировки повторно проверяем режим работы и состояние TUN
        if !matches!(*self.get_running_mode(), RunningMode::Sidecar)
            || !Config::verge().await.latest_arc().enable_tun_mode.unwrap_or(false)
        {
            return HandoffOutcome::Done;
        }

        logging!(
            info,
            Type::Core,
            "service became ready; handing off from sidecar to service"
        );
        self.stop_core_by_sidecar();

        match self.start_core_by_service().await {
            Ok(()) => {
                logging!(info, Type::Core, "handoff to service mode succeeded");
                HandoffOutcome::Done
            }
            Err(e) => {
                logging!(
                    error,
                    Type::Core,
                    "handoff to service failed: {}; restarting sidecar",
                    e
                );
                if let Err(e2) = self.start_core_by_sidecar().await {
                    logging!(
                        error,
                        Type::Core,
                        "failed to restart sidecar after handoff failure: {}",
                        e2
                    );
                }
                HandoffOutcome::Failed
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::should_wait_for_service;

    #[test]
    fn service_wait_is_only_required_for_non_admin_tun() {
        assert!(should_wait_for_service(true, false, false));
        assert!(!should_wait_for_service(true, false, true));
        assert!(!should_wait_for_service(true, true, false));
        assert!(!should_wait_for_service(false, false, false));
    }
}
