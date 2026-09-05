use super::{CoreManager, RunningMode};
use crate::cmd::StringifyErr as _;
use crate::config::{Config, IVerge};
use crate::constants::timing;
use crate::core::handle::Handle;
use crate::core::manager::CLASH_LOGGER;
use crate::core::service::{SERVICE_MANAGER, ServiceStatus};
use crate::process::AsyncHandler;
use anyhow::Result;
use clash_verge_logging::{Type, logging};
use scopeguard::defer;
use smartstring::alias::String;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use tauri_plugin_clash_verge_sysinfo;
use tauri_plugin_clash_verge_sysinfo::is_current_app_handle_admin;

static MIXED_PORT_CHECK_GENERATION: AtomicU64 = AtomicU64::new(0);
static PORT_BUSY_NOTICED: AtomicU32 = AtomicU32::new(0);

#[derive(Debug, PartialEq, Eq)]
enum PortReport {
    Serving,
    Other(u16),
    NotServing,
    Silent,
}

const fn port_report(reported: Option<u16>, expected: u16) -> PortReport {
    match reported {
        Some(port) if port == expected => PortReport::Serving,
        Some(0) => PortReport::NotServing,
        Some(port) => PortReport::Other(port),
        None => PortReport::Silent,
    }
}

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

        let attempted_service = matches!(*self.get_running_mode(), RunningMode::Service);
        let mut result = self.start_and_confirm(attempted_service).await;

        // clod:tun-ready — служба может отказать (сломана, старой версии,
        // пользователь отклонил переустановку). Раньше это означало «ядро не
        // запущено вообще», то есть отсутствие интернета вместо потери TUN.
        // Падаем в sidecar и продолжаем ждать службу в фоне.
        if let Err(e) = &result
            && attempted_service
        {
            logging!(
                warn,
                Type::Core,
                "service start failed ({}); falling back to sidecar",
                e
            );
            let rejected_bundle = {
                let runtime = Config::runtime().await;
                let latest = runtime.latest_arc();
                latest
                    .config
                    .as_ref()
                    .and_then(crate::core::service::bundle_rejection_for)
            };
            result = self.start_and_confirm(false).await;
            if result.is_ok()
                && let Some(message) = rejected_bundle
            {
                Handle::notice_message("service::bundle_rejected", message);
            }
        }

        // При ошибке запуска откатываем mode, чтобы разрешить повторную попытку.
        if let Err(error) = &result {
            self.set_running_mode(RunningMode::NotRunning);
            Handle::notice_message("core::not_ready", error.to_string());
            return result;
        }

        if Handle::global().is_exiting() {
            return result;
        }

        // clod: the core has just been started from the draft build —
        // `generate_file` writes `latest` — so the draft is now what mihomo
        // actually runs and belongs in the committed slot. Without this commit
        // the committed slot stayed empty for the whole cold start (nothing on
        // the boot path calls `apply`), and everything answering "what is
        // applied" — server descriptions, the sentinel report — reported
        // nothing until the first `update_config_*` cycle.
        //
        // Strictly after a successful start: committing a build the core
        // refused would defeat the point of the draft/committed split, see the
        // note on `IRuntime::sentinel_report`.
        Config::runtime().await.apply();

        // clod:tun-ready — проверяем факт, а не заявку: если ядро не смогло
        // поднять устройство, честно гасим TUN и говорим об этом.
        if crate::feat::tun::desired().await && !crate::feat::tun::is_suppressed() {
            crate::feat::tun::spawn_start_verification(crate::feat::tun::log_anchor().await);
        } else {
            AsyncHandler::spawn(|| async { crate::feat::tun::enforce_undesired_off().await });
        }

        crate::feat::environment::spawn_environment_watchdog();

        // После отката к sidecar в фоне ждём готовности службы для передачи
        if matches!(*self.get_running_mode(), RunningMode::Sidecar) {
            self.spawn_service_handoff_watcher().await;
        }

        result
    }

    async fn start_and_confirm(&self, use_service: bool) -> Result<()> {
        if use_service {
            self.start_core_by_service().await?;
        } else {
            self.start_core_by_sidecar().await?;
        }

        let Err(error) = self.confirm_core_ready().await else {
            Self::spawn_mixed_port_check();
            return Ok(());
        };

        logging!(
            error,
            Type::Core,
            "ядро запущено, но не отвечает по управляющему каналу: {}",
            error
        );
        if use_service {
            let _ = self.stop_core_by_service().await;
        } else {
            self.stop_core_by_sidecar();
        }
        Err(error)
    }

    async fn confirm_core_ready(&self) -> Result<()> {
        let mut last: Option<std::string::String> = None;

        for _ in 0..timing::CORE_READY_ATTEMPTS {
            if Handle::global().is_exiting() {
                return Ok(());
            }
            if matches!(*self.get_running_mode(), RunningMode::NotRunning) {
                anyhow::bail!("ядро завершилось, не ответив");
            }

            let probe = {
                let mihomo = Handle::mihomo().await;
                tokio::time::timeout(timing::CORE_READY_PROBE_TIMEOUT, mihomo.get_version()).await
            };
            match probe {
                Ok(Ok(_)) => return Ok(()),
                Ok(Err(error)) => last = Some(error.to_string()),
                Err(_) => last = Some("ядро не ответило за отведённое время".to_owned()),
            }

            tokio::time::sleep(timing::CORE_READY_INTERVAL).await;
        }

        anyhow::bail!("{}", last.unwrap_or_else(|| "причина неизвестна".to_owned()))
    }

    fn spawn_mixed_port_check() {
        let generation = MIXED_PORT_CHECK_GENERATION.fetch_add(1, Ordering::AcqRel) + 1;
        AsyncHandler::spawn(move || async move {
            let manager = Self::global();
            let expected = {
                let verge = Config::verge().await.latest_arc();
                match verge.verge_mixed_port {
                    Some(port) => port,
                    None => Config::clash().await.latest_arc().get_mixed_port(),
                }
            };

            let mut answered = false;
            for _ in 0..timing::MIXED_PORT_CHECK_ATTEMPTS {
                if Handle::global().is_exiting()
                    || MIXED_PORT_CHECK_GENERATION.load(Ordering::Acquire) != generation
                    || matches!(*manager.get_running_mode(), RunningMode::NotRunning)
                {
                    return;
                }

                let reported = {
                    let mihomo = Handle::mihomo().await;
                    match tokio::time::timeout(timing::CORE_READY_PROBE_TIMEOUT, mihomo.get_base_config()).await {
                        Ok(Ok(config)) => Some(config.mixed_port),
                        _ => None,
                    }
                };
                match port_report(reported, expected) {
                    PortReport::Serving => {
                        PORT_BUSY_NOTICED.store(0, Ordering::Release);
                        return;
                    }
                    PortReport::Other(port) => {
                        logging!(
                            warn,
                            Type::Core,
                            "ядро слушает порт {} вместо запрошенного {}",
                            port,
                            expected
                        );
                        return;
                    }
                    PortReport::NotServing => answered = true,
                    PortReport::Silent => {}
                }

                tokio::time::sleep(timing::MIXED_PORT_CHECK_INTERVAL).await;
            }

            if !answered {
                logging!(
                    warn,
                    Type::Core,
                    "ядро не ответило, слушает ли оно порт {} — проверку пропускаем",
                    expected
                );
                return;
            }

            if !crate::cmd::network::is_port_in_use(expected).await {
                logging!(
                    warn,
                    Type::Core,
                    "ядро не слушает порт {}, хотя порт свободен",
                    expected
                );
                return;
            }

            let mode = manager.get_running_mode();
            let own_pid = if matches!(*mode, RunningMode::Sidecar) {
                manager.sidecar_pid()
            } else {
                None
            };
            if crate::core::orphan::another_core_of_ours_is_running(own_pid, matches!(*mode, RunningMode::Service))
                .await
            {
                logging!(
                    warn,
                    Type::Core,
                    "порт {} занят другим нашим же ядром — оставляем как есть",
                    expected
                );
                return;
            }

            logging!(
                error,
                Type::Core,
                "порт {} занят посторонним приложением: ядро его не слушает, трафик через системный прокси не пойдёт",
                expected
            );
            if !Config::verge().await.latest_arc().enable_system_proxy.unwrap_or(false) {
                return;
            }
            if PORT_BUSY_NOTICED.swap(u32::from(expected), Ordering::AcqRel) != u32::from(expected) {
                Handle::notice_message("core::port_busy", expected.to_string());
            }
        });
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

        // clod:tun-ready — новая сборка ядра заслуживает честной попытки.
        // Подавление ставится на сессию (ядро не смогло поднять устройство) и
        // в конфиг не пишется; пережив подмену бинаря, оно означало бы «TUN не
        // работает, потому что не работал у ПРОШЛОГО ядра» — а обновление ядра
        // как раз и берут ради таких починок. Проверку факта после старта
        // делает `start_core_inner`.
        crate::feat::tun::clear_suppression();

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

    pub(super) fn after_core_process(&self) {
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

        let attempts = (|| async {
            if Handle::global().is_exiting() {
                return Ok(());
            }
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
        .retry(backoff);

        // clod: число попыток ограничивало ТОЛЬКО паузы между ними, а сама
        // попытка ходит в службу по IPC и может ждать сколько угодно — при этом
        // весь цикл стоит на пути запуска приложения. Общий потолок — вдвое от
        // отведённого на ожидание: дальше поднимаемся как sidecar, а служба,
        // когда очнётся, подхватится хэндоффом.
        let ceiling = timing::SERVICE_WAIT_MAX * 2;
        if tokio::time::timeout(ceiling, attempts).await.is_err() {
            logging!(
                warn,
                Type::Core,
                "служба не ответила за {:?} — продолжаем запуск без неё",
                ceiling
            );
        }
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
        if service::bundle_rejection_for_the_running_config().await.is_some() {
            logging!(
                info,
                Type::Core,
                "the current configuration cannot run under the service; staying in sidecar mode until it changes"
            );
            return HandoffOutcome::Failed;
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

        match self.start_and_confirm(true).await {
            Ok(()) => {
                logging!(info, Type::Core, "handoff to service mode succeeded");
                // clod: под службой поднимается НОВЫЙ процесс ядра — с первым
                // узлом каждой группы. На Windows с TUN это основной путь
                // запуска, и без возврата выбор пользователя терялся при каждом
                // старте приложения.
                if let Err(e) = crate::config::profiles::activate_selected_nodes() {
                    logging!(
                        warn,
                        Type::Core,
                        "Warning: restore selection after the handoff failed: {e}"
                    );
                }
                HandoffOutcome::Done
            }
            Err(e) => {
                logging!(
                    error,
                    Type::Core,
                    "handoff to service failed: {}; restarting sidecar",
                    e
                );
                self.roll_back_to_sidecar().await;
                HandoffOutcome::Failed
            }
        }
    }

    async fn roll_back_to_sidecar(&self) {
        if let Err(error) = self.start_and_confirm(false).await {
            logging!(
                error,
                Type::Core,
                "failed to restart sidecar after handoff failure: {}",
                error
            );
            Handle::notice_message("core::handoff_failed", error.to_string());
            if let Err(last) = self.start_core_by_sidecar().await {
                logging!(
                    error,
                    Type::Core,
                    "sidecar did not come back after the handoff at all: {}",
                    last
                );
                return;
            }
        }

        if let Err(error) = crate::config::profiles::activate_selected_nodes() {
            logging!(
                warn,
                Type::Core,
                "Warning: restore selection after the handoff rollback failed: {error}"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{PortReport, port_report, should_wait_for_service};

    #[test]
    fn a_silent_core_is_not_a_busy_port() {
        assert_eq!(port_report(None, 7897), PortReport::Silent);
        assert_eq!(port_report(Some(0), 7897), PortReport::NotServing);
        assert_eq!(port_report(Some(7897), 7897), PortReport::Serving);
        assert_eq!(port_report(Some(7890), 7897), PortReport::Other(7890));
    }

    #[test]
    fn service_wait_is_only_required_for_non_admin_tun() {
        assert!(should_wait_for_service(true, false, false));
        assert!(!should_wait_for_service(true, false, true));
        assert!(!should_wait_for_service(true, true, false));
        assert!(!should_wait_for_service(false, false, false));
    }
}
