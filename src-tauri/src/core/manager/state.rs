use super::{Backend, CoreManager, RunningMode};
use crate::{
    AsyncHandler,
    config::{Config, IClashTemp},
    constants::timing,
    core::{handle, logger::Logger, manager::CLASH_LOGGER, service},
    logging,
    utils::dirs,
};
use anyhow::Result;
use clash_verge_logging::Type;
use clash_verge_service_ipc::ServiceLifecycleState;
use compact_str::CompactString;
use log::Level;
#[cfg(unix)]
use scopeguard::defer;
use std::{
    sync::{
        Mutex,
        atomic::{AtomicU32, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tauri_plugin_mihomo::MihomoExt as _;
use tauri_plugin_shell::ShellExt as _;

#[cfg(target_os = "windows")]
use {
    std::os::windows::io::{AsRawHandle as _, FromRawHandle as _, OwnedHandle},
    windows_sys::Win32::{
        Foundation::HANDLE,
        System::{
            JobObjects::{
                AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation, SetInformationJobObject,
            },
            Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_SET_QUOTA, PROCESS_TERMINATE},
        },
    },
};

static CRASH_RESTARTS: AtomicU32 = AtomicU32::new(0);
static LAST_CRASH_AT: Mutex<Option<Instant>> = Mutex::new(None);
const MAX_CRASH_RESTARTS: u32 = 3;
const CORE_RESTART_DELAY: Duration = Duration::from_secs(1);
const CORE_RESTART_DELAY_CAP: Duration = Duration::from_secs(4);
const CORE_STABLE_AFTER: Duration = Duration::from_secs(60);

static CORE_WATCHDOG_GENERATION: AtomicU64 = AtomicU64::new(0);

fn point_core_client_at(socket_path: &str) {
    if let Err(err) = handle::Handle::app_handle().mihomo().update_socket_path(socket_path) {
        logging!(
            error,
            Type::Core,
            "клиент ядра не переведён на сокет {socket_path}: {err}"
        );
    }
}

/// Мёртво ли ядро после попытки остановки: таблица процессов решает, исход
/// команды убийства — только запасной ответ на случай, когда таблица не
/// читается.
const fn the_core_is_gone(kill_reported_ok: bool, look: crate::core::orphan::Look) -> bool {
    match look {
        crate::core::orphan::Look::Gone => true,
        crate::core::orphan::Look::Alive => false,
        crate::core::orphan::Look::Unknown => kill_reported_ok,
    }
}

fn exit_is_a_crash(current: &RunningMode, expected: &RunningMode, app_exiting: bool) -> bool {
    !app_exiting && current == expected
}

fn terminated_process_is_the_running_one(
    expected: &RunningMode,
    running_pid: Option<u32>,
    terminated_pid: Option<u32>,
) -> bool {
    match (expected, terminated_pid) {
        (RunningMode::Sidecar, Some(pid)) => running_pid == Some(pid),
        (RunningMode::Sidecar, None) => false,
        _ => true,
    }
}

const fn restart_delay(attempt: u32) -> Duration {
    match attempt {
        0 | 1 => CORE_RESTART_DELAY,
        2 => Duration::from_secs(2),
        _ => CORE_RESTART_DELAY_CAP,
    }
}

fn crash_attempt_number(previous_crash: Option<Instant>, now: Instant) -> u32 {
    let stale = previous_crash.is_none_or(|last| now.duration_since(last) >= CORE_STABLE_AFTER);
    if stale {
        CRASH_RESTARTS.store(0, Ordering::Release);
    }
    CRASH_RESTARTS.fetch_add(1, Ordering::AcqRel) + 1
}

pub(super) fn handle_core_exit(message: &str, expected: &RunningMode, terminated_pid: Option<u32>) {
    let manager = CoreManager::global();
    let asked_for = manager.a_death_we_asked_for();
    if !exit_is_a_crash(
        &manager.get_running_mode(),
        expected,
        handle::Handle::global().is_exiting(),
    ) {
        return;
    }
    if !terminated_process_is_the_running_one(expected, manager.sidecar_pid(), terminated_pid) {
        logging!(
            info,
            Type::Core,
            "ignoring the exit of core pid {:?}: it is not the process we run now ({:?})",
            terminated_pid,
            manager.sidecar_pid()
        );
        return;
    }
    match terminated_pid {
        Some(pid) if !manager.claim_sidecar_exit(pid) => {
            logging!(
                info,
                Type::Core,
                "the exit of core pid {} is already being handled",
                pid
            );
            return;
        }
        Some(_) => {}
        None => manager.clear_sidecar_pid(),
    }

    // Поздняя смерть ядра, которое остановка не смогла убить: это не
    // падение (воскрешать нельзя — человек просил остановить), но и не
    // молчание: остановка давно отчиталась отказом, о смерти теперь никто,
    // кроме этого места, трею и главной не скажет.
    let a_stop_that_failed_just_finished = manager.stop_failed();
    manager.note_core_is_down();
    if asked_for {
        logging!(info, Type::Core, "core exited as asked: {}", message);
        if a_stop_that_failed_just_finished {
            manager.after_core_process();
        }
        return;
    }

    logging!(warn, Type::Core, "core exited unexpectedly: {}", message);

    let now = Instant::now();
    let attempt = {
        let mut last = match LAST_CRASH_AT.lock() {
            Ok(last) => last,
            Err(poisoned) => poisoned.into_inner(),
        };
        let attempt = crash_attempt_number(*last, now);
        *last = Some(now);
        attempt
    };
    manager.set_restart_pending(attempt <= MAX_CRASH_RESTARTS);
    manager.after_core_process();
    if attempt > MAX_CRASH_RESTARTS {
        logging!(
            error,
            Type::Core,
            "core crashed {} times in a row; not restarting automatically",
            attempt
        );
        handle::Handle::notice_message("core::crashed", message.to_owned());
        AsyncHandler::spawn(|| async {
            if !Config::verge().await.latest_arc().enable_system_proxy.unwrap_or(false) {
                return;
            }
            crate::core::sysopt::Sysopt::global().stop_proxy_guard();
            handle::Handle::notice_message("sysproxy::core_gave_up", "");
        });
        return;
    }

    let message = message.to_owned();
    AsyncHandler::spawn(move || async move {
        tokio::time::sleep(restart_delay(attempt)).await;
        let manager = CoreManager::global();
        if handle::Handle::global().is_exiting() || !matches!(*manager.get_running_mode(), RunningMode::NotRunning) {
            manager.set_restart_pending(false);
            return;
        }
        logging!(
            info,
            Type::Core,
            "restarting the core after a crash (attempt {})",
            attempt
        );
        let restarted = manager.start_core().await;
        manager.set_restart_pending(false);
        if let Err(e) = restarted {
            logging!(error, Type::Core, "failed to restart the core after a crash: {}", e);
            manager.after_core_process();
            return;
        }
        if !after_core_came_back(&message).await {
            return;
        }
        tokio::time::sleep(CORE_STABLE_AFTER).await;
        if !matches!(*CoreManager::global().get_running_mode(), RunningMode::NotRunning) {
            CRASH_RESTARTS.store(0, Ordering::Release);
        }
    });
}

async fn core_answers() -> bool {
    let core = crate::feat::environment::detached_core_client();
    tokio::time::timeout(timing::CORE_HEALTH_INTERVAL, core.get_version())
        .await
        .is_ok_and(|answered| answered.is_ok())
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ServiceSample {
    Unreadable,
    Status {
        is_active: bool,
        desired_running: bool,
        state: ServiceLifecycleState,
        core_pid: Option<u32>,
        restart_count: u32,
        last_exit_reason: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum HealthStep {
    Continue,
    ProbeTheCore,
    RestartedByService { restarts: u32, reason: String },
    CoreLost(&'static str),
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct HealthWatch {
    unreadable: u32,
    silent: u32,
    missing_core: u32,
    restart_count: Option<u32>,
}

const fn service_is_settling(state: ServiceLifecycleState) -> bool {
    matches!(
        state,
        ServiceLifecycleState::Starting | ServiceLifecycleState::RecoveringCore
    )
}

impl HealthWatch {
    fn observe(&mut self, sample: ServiceSample) -> HealthStep {
        match sample {
            ServiceSample::Unreadable => {
                self.unreadable = self.unreadable.saturating_add(1);
                HealthStep::ProbeTheCore
            }
            ServiceSample::Status {
                is_active,
                desired_running,
                state,
                core_pid,
                restart_count,
                last_exit_reason,
            } => {
                self.unreadable = 0;
                if !is_active {
                    return HealthStep::CoreLost("the service is no longer running the core for us");
                }
                let baseline = *self.restart_count.get_or_insert(restart_count);
                if restart_count < baseline {
                    self.restart_count = Some(restart_count);
                } else if restart_count > baseline
                    && core_pid.is_some()
                    && matches!(state, ServiceLifecycleState::Running)
                {
                    self.restart_count = Some(restart_count);
                    self.missing_core = 0;
                    self.silent = 0;
                    return HealthStep::RestartedByService {
                        restarts: restart_count - baseline,
                        reason: last_exit_reason.unwrap_or_default(),
                    };
                }
                if matches!(state, ServiceLifecycleState::Fatal) {
                    return HealthStep::CoreLost("the service gave up on the core");
                }
                if !desired_running && core_pid.is_none() {
                    return HealthStep::CoreLost("the service was told to stop the core");
                }
                if service_is_settling(state) {
                    self.missing_core = 0;
                    self.silent = 0;
                    return HealthStep::Continue;
                }
                if core_pid.is_none() {
                    self.missing_core = self.missing_core.saturating_add(1);
                    if self.missing_core >= timing::CORE_HEALTH_MISSES {
                        return HealthStep::CoreLost("the core is gone and the service is not bringing it back");
                    }
                    return HealthStep::Continue;
                }
                self.missing_core = 0;
                HealthStep::ProbeTheCore
            }
        }
    }

    const fn core_probed(&mut self, answers: bool) -> HealthStep {
        if answers {
            self.silent = 0;
            self.unreadable = 0;
            return HealthStep::Continue;
        }
        self.silent = self.silent.saturating_add(1);
        if self.silent < timing::CORE_HEALTH_MISSES {
            HealthStep::Continue
        } else if self.unreadable >= timing::CORE_HEALTH_MISSES {
            HealthStep::CoreLost("neither the service nor the core answers")
        } else {
            HealthStep::CoreLost("the core stopped answering under the service")
        }
    }
}

async fn sample_the_service() -> ServiceSample {
    if !service::is_service_ipc_path_exists() {
        logging!(warn, Type::Core, "the service socket is gone");
        return ServiceSample::Unreadable;
    }
    let status = tokio::time::timeout(timing::SERVICE_STATUS_WAIT, service::service_status())
        .await
        .unwrap_or_else(|_| Err(anyhow::anyhow!("no answer within {:?}", timing::SERVICE_STATUS_WAIT)));
    match status {
        Ok(status) => ServiceSample::Status {
            is_active: status.is_active,
            desired_running: status.desired_core_should_be_running,
            state: status.service_state,
            core_pid: status.core_pid,
            restart_count: status.restart_count,
            last_exit_reason: status.last_core_exit_reason,
        },
        Err(e) => {
            logging!(warn, Type::Core, "the service did not report its status: {e:#}");
            ServiceSample::Unreadable
        }
    }
}

static LAST_RESTART_NOTICE: Mutex<Option<Instant>> = Mutex::new(None);

fn restart_notice_is_due(previous: Option<Instant>, now: Instant) -> bool {
    previous.is_none_or(|last| now.duration_since(last) >= CORE_STABLE_AFTER)
}

fn notice_the_restart(reason: &str) {
    let now = Instant::now();
    let due = {
        let mut last = match LAST_RESTART_NOTICE.lock() {
            Ok(last) => last,
            Err(poisoned) => poisoned.into_inner(),
        };
        let due = restart_notice_is_due(*last, now);
        if due {
            *last = Some(now);
        }
        due
    };
    if due {
        handle::Handle::notice_message("core::restarted", reason.to_owned());
    }
}

async fn after_core_came_back(reason: &str) -> bool {
    handle::Handle::refresh_clash();
    if let Err(e) = crate::config::profiles::activate_selected_nodes() {
        logging!(
            warn,
            Type::Core,
            "Warning: restore selection after a crash restart failed: {e}"
        );
    }
    if let Err(e) = crate::core::tray::Tray::global().update_menu().await {
        logging!(warn, Type::Core, "failed to refresh the tray after a restart: {}", e);
    }
    if handle::Handle::global().is_exiting()
        || matches!(*CoreManager::global().get_running_mode(), RunningMode::NotRunning)
    {
        return false;
    }
    let wants_sysproxy = Config::verge().await.latest_arc().enable_system_proxy.unwrap_or(false);
    if wants_sysproxy {
        CoreManager::global().point_system_proxy_at_the_confirmed_port().await;
    }
    crate::feat::tun::enforce_undesired_off().await;
    notice_the_restart(reason);
    true
}

enum CoreWatch {
    Service(HealthWatch),
    Sidecar { pid: u32, silent: u32 },
}

impl CoreWatch {
    fn still_ours(&self, manager: &CoreManager) -> bool {
        match self {
            Self::Service(_) => matches!(*manager.get_running_mode(), RunningMode::Service),
            Self::Sidecar { pid, .. } => manager.sidecar_pid() == Some(*pid),
        }
    }

    fn yield_to_config_update(&mut self) {
        match self {
            Self::Service(watch) => {
                *watch = HealthWatch {
                    restart_count: watch.restart_count,
                    ..HealthWatch::default()
                };
            }
            Self::Sidecar { silent, .. } => *silent = 0,
        }
    }

    async fn look(&mut self, manager: &CoreManager, generation: u64) -> bool {
        match self {
            Self::Service(watch) => look_at_the_service(manager, watch, generation).await,
            Self::Sidecar { pid, silent } => look_at_the_sidecar(manager, *pid, silent, generation).await,
        }
    }
}

async fn look_at_the_service(manager: &CoreManager, watch: &mut HealthWatch, generation: u64) -> bool {
    let sample = sample_the_service().await;
    if let ServiceSample::Status { core_pid, .. } = &sample {
        manager.remember_service_core_pid(*core_pid);
    }
    let mut step = watch.observe(sample);
    if matches!(step, HealthStep::ProbeTheCore) {
        let answers = core_answers().await;
        if !answers {
            logging!(
                warn,
                Type::Core,
                "the core did not answer under the service ({}/{})",
                watch.silent + 1,
                timing::CORE_HEALTH_MISSES
            );
        }
        step = watch.core_probed(answers);
    }
    if CORE_WATCHDOG_GENERATION.load(Ordering::Acquire) != generation {
        return false;
    }
    match step {
        HealthStep::Continue | HealthStep::ProbeTheCore => true,
        HealthStep::RestartedByService { restarts, reason } => {
            logging!(
                warn,
                Type::Core,
                "the service restarted the core by itself ({} time(s) since we looked): {}",
                restarts,
                reason
            );
            let _ = after_core_came_back(&reason).await;
            true
        }
        HealthStep::CoreLost(why) => {
            handle_core_exit(why, &RunningMode::Service, None);
            false
        }
    }
}

async fn look_at_the_sidecar(manager: &CoreManager, pid: u32, silent: &mut u32, generation: u64) -> bool {
    if core_answers().await {
        *silent = 0;
        return true;
    }
    *silent += 1;
    logging!(
        warn,
        Type::Core,
        "the core process {} did not answer ({}/{})",
        pid,
        *silent,
        timing::CORE_HEALTH_MISSES
    );
    if *silent < timing::CORE_HEALTH_MISSES {
        return true;
    }
    if CORE_WATCHDOG_GENERATION.load(Ordering::Acquire) != generation {
        return false;
    }
    manager.recover_hung_sidecar(pid).await;
    false
}

fn spawn_core_health_watchdog(mut watch: CoreWatch) {
    let generation = CORE_WATCHDOG_GENERATION.fetch_add(1, Ordering::AcqRel) + 1;
    AsyncHandler::spawn(move || async move {
        let mut look_now = matches!(watch, CoreWatch::Service(_));
        let mut skipped: u32 = 0;
        loop {
            if !std::mem::take(&mut look_now) {
                tokio::time::sleep(timing::CORE_HEALTH_INTERVAL).await;
            }
            let manager = CoreManager::global();
            if handle::Handle::global().is_exiting()
                || CORE_WATCHDOG_GENERATION.load(Ordering::Acquire) != generation
                || !watch.still_ours(manager)
            {
                return;
            }
            if manager.is_config_update_in_progress() {
                skipped += 1;
                if skipped <= timing::CORE_HEALTH_MAX_SKIPS {
                    watch.yield_to_config_update();
                    continue;
                }
                if skipped == timing::CORE_HEALTH_MAX_SKIPS + 1 {
                    logging!(
                        warn,
                        Type::Core,
                        "применение конфига идёт {} кругов подряд — сторож больше не уступает",
                        skipped
                    );
                }
            } else {
                skipped = 0;
            }

            if !watch.look(manager, generation).await {
                return;
            }
        }
    });
}

impl CoreManager {
    /// Снова поставить сторож здоровья над уже работающим ядром.
    ///
    /// Сторожа выходят по флагу выхода, а заводятся только запуском ядра —
    /// который ничего не делает, если ядро числится живым. После отменённого
    /// выхода приложение оставалось без сторожа зависшего ядра — то есть без
    /// того, что эту ситуацию и чинит. Новый сторож поднимает поколение,
    /// уцелевший старый уходит сам.
    pub async fn watch_the_core_again(&self) {
        let _life = self.lifecycle_lock.lock().await;
        self.watch_the_core_again_locked();
    }

    /// Вызывающий должен уже удерживать `lifecycle_lock`.
    pub(super) fn watch_the_core_again_locked(&self) {
        match *self.get_running_mode() {
            RunningMode::Service => spawn_core_health_watchdog(CoreWatch::Service(HealthWatch::default())),
            RunningMode::Sidecar => {
                if let Some(pid) = self.sidecar_pid() {
                    spawn_core_health_watchdog(CoreWatch::Sidecar { pid, silent: 0 });
                }
            }
            RunningMode::NotRunning => {}
        }
    }

    async fn recover_hung_sidecar(&self, pid: u32) {
        let _life = self.lifecycle_lock.lock().await;
        if handle::Handle::global().is_exiting() || self.sidecar_pid() != Some(pid) {
            return;
        }
        logging!(
            warn,
            Type::Core,
            "the core process {} is alive but stopped answering; killing it before the restart",
            pid
        );
        // Намерение здесь — «ядро должно работать», поэтому `Stopping` не
        // ставится: доказанная смерть уходит в обычный путь падения с
        // перезапуском.
        if !self.kill_the_sidecar_and_prove_it(Some(pid)).await {
            self.note_stop_failed();
            logging!(
                error,
                Type::Core,
                "зависшее ядро {} убить не удалось — второй процесс поверх него не поднимаем",
                pid
            );
            return;
        }
        handle_core_exit("the core stopped answering", &RunningMode::Sidecar, Some(pid));
    }

    pub async fn get_clash_log_snapshot(&self) -> Result<String> {
        service::get_clash_log_snapshot_by_service().await
    }

    pub async fn get_clash_logs(&self) -> Result<Vec<CompactString>> {
        match *self.get_running_mode() {
            RunningMode::Service => service::get_clash_logs_by_service().await,
            RunningMode::Sidecar => Ok(CLASH_LOGGER.get_logs().await),
            RunningMode::NotRunning => Ok(Vec::new()),
        }
    }

    pub(super) async fn start_core_by_sidecar(&self) -> Result<()> {
        self.refuse_to_double_the_core()?;
        logging!(info, Type::Core, "Starting core in sidecar mode");

        let sidecar_ipc = dirs::sidecar_ipc_path()?;
        point_core_client_at(dirs::path_to_str(&sidecar_ipc)?);

        let config_file = Config::generate_file(crate::config::ConfigType::Run).await?;
        let app_handle = handle::Handle::app_handle();
        let clash_core = Config::verge().await.latest_arc().get_valid_clash_core();
        let config_dir = dirs::app_home_dir()?;
        #[cfg(unix)]
        discard_unwritable_core_cache(&config_dir);

        let managed = crate::core::core_updater::managed_core().await;
        let command = match &managed {
            Some((_, path)) => {
                logging!(info, Type::Core, "using managed core: {}", path.display());
                app_handle.shell().command(path)
            }
            None => app_handle.shell().sidecar(clash_core.as_str())?,
        };

        #[cfg(unix)]
        let previous_mask = unsafe { tauri_plugin_clash_verge_sysinfo::libc::umask(0o077) };
        #[cfg(unix)]
        defer! {
            unsafe { tauri_plugin_clash_verge_sysinfo::libc::umask(previous_mask) };
        }
        let command = command.args([
            "-d",
            dirs::path_to_str(&config_dir)?,
            "-f",
            dirs::path_to_str(&config_file)?,
            if cfg!(windows) {
                "-ext-ctl-pipe"
            } else {
                "-ext-ctl-unix"
            },
            &IClashTemp::guard_external_controller_ipc(),
        ]);
        #[cfg(windows)]
        let command = command.env(
            "LISTEN_NAMEDPIPE_SDDL",
            crate::core::owner_identity::current_user_pipe_sddl()?,
        );
        let (mut rx, child) = command.spawn()?;
        #[cfg(target_os = "windows")]
        {
            let job = match create_and_assign_sidecar_job(child.pid()) {
                Ok(job) => job,
                Err(job_error) => {
                    let pid = child.pid();

                    let error = match child.kill() {
                        Ok(()) => job_error,
                        Err(kill_error) => anyhow::anyhow!(
                            "failed to configure Job Object for sidecar PID {pid}: \
                            {job_error:#}; failed to terminate child: {kill_error:#}"
                        ),
                    };

                    logging!(error, Type::Core, "Failed to start sidecar: {error:#}");
                    return Err(error);
                }
            };
            self.set_job_handle(Some(job));
        }

        let pid = child.pid();
        logging!(trace, Type::Core, "Sidecar started with PID: {}", pid);

        self.set_running_child_sidecar(child);
        self.set_sidecar_pid(pid);
        crate::core::core_updater::note_started_core(managed.map(|(version, _)| version));
        self.note_core_is_up(Backend::Sidecar);
        spawn_core_health_watchdog(CoreWatch::Sidecar { pid, silent: 0 });

        AsyncHandler::spawn(move || async move {
            while let Some(event) = rx.recv().await {
                let (level, line) = match event {
                    tauri_plugin_shell::process::CommandEvent::Stdout(line) => (Level::Info, line),
                    tauri_plugin_shell::process::CommandEvent::Stderr(line) => (Level::Warn, line),
                    tauri_plugin_shell::process::CommandEvent::Terminated(term) => {
                        let message = if let Some(code) = term.code {
                            CompactString::from(format!("Process terminated with code: {}", code))
                        } else if let Some(signal) = term.signal {
                            CompactString::from(format!("Process terminated by signal: {}", signal))
                        } else {
                            CompactString::from("Process terminated")
                        };
                        Logger::global().writer_sidecar_log(Level::Info, &message);
                        handle_core_exit(&message, &RunningMode::Sidecar, Some(pid));
                        break;
                    }
                    _ => continue,
                };
                let message = CompactString::from(&*String::from_utf8_lossy(&line));
                Logger::global().writer_sidecar_log(level, &message);
                if Self::global().sidecar_pid() == Some(pid) && crate::feat::tun::line_reports_tun_failure(&message) {
                    crate::feat::tun::report_start_failure(&message);
                }
                CLASH_LOGGER.append_log(message).await;
            }
        });

        Ok(())
    }

    /// Остановить ядро, запущенное самим приложением.
    ///
    /// Раньше «остановлено» здесь говорилось всегда: отказ `kill` уходил в
    /// журнал уровня trace, а если ссылку на процесс держал кто-то ещё,
    /// убийства не было вовсе — и это тоже считалось успехом. Выход верил и
    /// закрывал окно, оставляя ядро с занятым портом. Теперь отказ — это
    /// отказ: без ссылки на процесс убиваем по номеру, и только доказанно
    /// убитый или исчезнувший процесс считается остановленным.
    pub(super) async fn stop_core_by_sidecar(&self) -> Result<()> {
        logging!(info, Type::Core, "Stopping sidecar");
        CORE_WATCHDOG_GENERATION.fetch_add(1, Ordering::AcqRel);
        let pid = self.sidecar_pid();
        self.note_stopping();
        if self.kill_the_sidecar_and_prove_it(pid).await {
            self.clear_sidecar_pid();
            self.note_core_is_down();
            return Ok(());
        }
        self.note_stop_failed();
        anyhow::bail!(
            "процесс ядра {} не остановлен: он всё ещё в таблице процессов",
            pid.unwrap_or(0)
        )
    }

    /// Убить процесс ядра и ДОКАЗАТЬ его смерть. `true` — процесса больше нет.
    ///
    /// clod:stop-proof — исход самой команды убийства ни на что не влияет:
    /// `CommandChild::kill` отчитывается за системный вызов, `kill_process` —
    /// за отправленный сигнал, а на Windows процесс к тому же добивает
    /// закрытие Job-объекта, и `kill()` по уже умирающему процессу вправе
    /// вернуть ошибку. Единственный источник правды — таблица процессов;
    /// команда лишь пишет свой исход в журнал, а решает `wait_until_gone`.
    /// Когда таблица не читается вовсе, верим исходу команды — хуже прежнего
    /// поведения это не делает.
    async fn kill_the_sidecar_and_prove_it(&self, pid: Option<u32>) -> bool {
        let mut pid = pid;
        let kill_reported_ok = match self.take_child_sidecar() {
            Some(child) => {
                // Номер мог быть уже стёрт обработкой смерти (падение во время
                // проверки готовности): доказательство берётся с самой ссылки,
                // иначе мёртвый процесс числился бы «не остановившимся».
                let pid = *pid.get_or_insert_with(|| child.pid());

                #[cfg(target_os = "windows")]
                {
                    self.set_job_handle(None);
                    logging!(
                        trace,
                        Type::Core,
                        "Closed job handle for sidecar process (PID: {})",
                        pid
                    );
                }

                match child.kill() {
                    Ok(()) => true,
                    Err(error) => {
                        logging!(warn, Type::Core, "kill() по процессу ядра {pid} отказал: {error}");
                        false
                    }
                }
            }
            None => match pid {
                Some(pid) => crate::core::orphan::kill_process(pid).await,
                None => return true,
            },
        };
        let Some(pid) = pid else {
            return kill_reported_ok;
        };
        let look = crate::core::orphan::wait_until_gone(pid, crate::core::orphan::DEATH_PROOF_BUDGET).await;
        let gone = the_core_is_gone(kill_reported_ok, look);
        logging!(
            trace,
            Type::Core,
            "sidecar {pid}: kill reported {kill_reported_ok}, process table says {look:?}, gone = {gone}"
        );
        gone
    }

    pub(super) async fn start_core_by_service(&self) -> Result<()> {
        self.refuse_to_double_the_core()?;
        logging!(info, Type::Core, "Starting core in service mode");

        let service_ipc = dirs::service_ipc_path()?;
        point_core_client_at(dirs::path_to_str(&service_ipc)?);

        let config_file = Config::generate_file(crate::config::ConfigType::Run).await?;

        #[cfg(target_os = "windows")]
        {
            let mut last_err = None;
            for attempt in 0..timing::SERVICE_START_RETRIES {
                match service::run_core_by_service(&config_file).await {
                    Ok(()) => {
                        self.note_core_is_up(Backend::Service);
                        spawn_core_health_watchdog(CoreWatch::Service(HealthWatch::default()));
                        return Ok(());
                    }
                    Err(e) => {
                        logging!(
                            warn,
                            Type::Core,
                            "service start attempt {}/{} failed: {}",
                            attempt + 1,
                            timing::SERVICE_START_RETRIES,
                            e
                        );
                        if crate::core::core_integrity::is_core_binary_changed(&e) {
                            return Err(e);
                        }
                        last_err = Some(e);
                        tokio::time::sleep(timing::SERVICE_START_RETRY_DELAY).await;
                    }
                }
            }
            Err(last_err.unwrap_or_else(|| anyhow::anyhow!("service start failed")))
        }

        #[cfg(not(target_os = "windows"))]
        {
            service::run_core_by_service(&config_file).await?;
            self.note_core_is_up(Backend::Service);
            spawn_core_health_watchdog(CoreWatch::Service(HealthWatch::default()));
            Ok(())
        }
    }

    /// clod:stop-proof — успех IPC здесь доказательство: служба убивает
    /// ядро через `tokio::process::Child::kill`, который ждёт смерти, и только
    /// потом отвечает (`clash-verge-service-ipc`, `CoreManager::stop_core`).
    /// Не доказан ОТКАЗ: ошибка IPC значит «служба не ответила», а не «ядро
    /// живо» — служба могла умереть вместе со своим ядром или раньше него.
    /// Тогда смерть ядра проверяется по номеру процесса, который служба
    /// сообщала сторожу, а без номера — по таблице процессов целиком.
    pub(super) async fn stop_core_by_service(&self) -> Result<()> {
        logging!(info, Type::Core, "Stopping service");
        CORE_WATCHDOG_GENERATION.fetch_add(1, Ordering::AcqRel);
        self.note_stopping();
        let stop = service::stop_core_by_service().await;
        let gone = match &stop {
            Ok(()) => true,
            Err(error) => {
                let gone = self.the_service_core_is_gone().await;
                logging!(
                    warn,
                    Type::Core,
                    "служба не остановила ядро ({error:#}); таблица процессов: ядра {}",
                    if gone { "нет" } else { "живо" }
                );
                gone
            }
        };
        if gone {
            self.remember_service_core_pid(None);
            self.clear_sidecar_pid();
            self.note_core_is_down();
            return Ok(());
        }
        self.note_stop_failed();
        stop
    }

    async fn the_service_core_is_gone(&self) -> bool {
        use crate::core::orphan::{
            DEATH_PROOF_BUDGET, Look, another_core_of_ours_is_running, pid_belongs_to_our_core, wait_until_gone,
        };

        match self.service_core_pid() {
            Some(pid) => match wait_until_gone(pid, DEATH_PROOF_BUDGET).await {
                Look::Gone => true,
                // Служба сообщала этот номер давно: он мог перейти к чужому
                // процессу, и тогда ядра под службой уже нет.
                Look::Alive => !pid_belongs_to_our_core(pid).await,
                // Номер известен, а таблица не читается: считать ядро мёртвым
                // по неведению нельзя — это и есть путь к двойному запуску.
                Look::Unknown => false,
            },
            None => !another_core_of_ours_is_running(None, false).await,
        }
    }
}

#[cfg(target_os = "windows")]
fn create_and_assign_sidecar_job(child_pid: u32) -> Result<OwnedHandle> {
    unsafe {
        let raw_job: HANDLE = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if raw_job.is_null() {
            return Err(last_win32_error("CreateJobObjectW failed"));
        }
        let job = OwnedHandle::from_raw_handle(raw_job);
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;

        let set_info_result = SetInformationJobObject(
            job.as_raw_handle() as HANDLE,
            JobObjectExtendedLimitInformation,
            &mut info as *mut _ as *mut _,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        );
        if set_info_result == 0 {
            return Err(last_win32_error("SetInformationJobObject failed"));
        }

        let raw_process_handle = OpenProcess(
            PROCESS_SET_QUOTA | PROCESS_TERMINATE | PROCESS_QUERY_INFORMATION,
            0,
            child_pid,
        );
        if raw_process_handle.is_null() {
            return Err(last_win32_error("OpenProcess failed"));
        }
        let process_handle = OwnedHandle::from_raw_handle(raw_process_handle);

        let assign_result = AssignProcessToJobObject(job.as_raw_handle(), process_handle.as_raw_handle());
        if assign_result == 0 {
            return Err(last_win32_error("AssignProcessToJobObject failed"));
        }

        Ok(job)
    }
}

#[cfg(target_os = "windows")]
fn last_win32_error(operation: &'static str) -> anyhow::Error {
    anyhow::Error::new(std::io::Error::last_os_error()).context(operation)
}

#[cfg(test)]
mod stop_proof_tests {
    use super::the_core_is_gone;
    use crate::core::orphan::Look;

    /// Таблица процессов старше исхода команды в обе стороны; только когда
    /// её не прочитать, слово остаётся за командой.
    #[test]
    fn the_process_table_outranks_the_kill_report() {
        for kill_reported_ok in [true, false] {
            assert!(the_core_is_gone(kill_reported_ok, Look::Gone));
            assert!(!the_core_is_gone(kill_reported_ok, Look::Alive));
            assert_eq!(the_core_is_gone(kill_reported_ok, Look::Unknown), kill_reported_ok);
        }
    }
}

#[cfg(test)]
mod exit_tests {
    use super::{RunningMode, exit_is_a_crash};

    #[test]
    fn only_an_exit_in_the_mode_we_watched_counts_as_a_crash() {
        assert!(exit_is_a_crash(&RunningMode::Sidecar, &RunningMode::Sidecar, false));
        assert!(exit_is_a_crash(&RunningMode::Service, &RunningMode::Service, false));
        assert!(!exit_is_a_crash(&RunningMode::NotRunning, &RunningMode::Sidecar, false));
        assert!(!exit_is_a_crash(&RunningMode::NotRunning, &RunningMode::Service, false));
        assert!(!exit_is_a_crash(&RunningMode::Service, &RunningMode::Sidecar, false));
        assert!(!exit_is_a_crash(&RunningMode::Sidecar, &RunningMode::Service, false));
        assert!(!exit_is_a_crash(&RunningMode::Sidecar, &RunningMode::Sidecar, true));
        assert!(!exit_is_a_crash(&RunningMode::Service, &RunningMode::Service, true));
    }
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::create_and_assign_sidecar_job;
    use anyhow::Result;
    use std::{
        process::{Child, Command, Stdio},
        thread::sleep,
        time::{Duration, Instant},
    };

    fn spawn_long_lived() -> Result<Child> {
        let child = Command::new("ping")
            .args(["-n", "999", "127.0.0.1"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        Ok(child)
    }

    fn wait_until_exited(child: &mut Child, timeout: Duration) -> Result<bool> {
        let deadline = Instant::now() + timeout;
        loop {
            if child.try_wait()?.is_some() {
                return Ok(true);
            }
            if Instant::now() >= deadline {
                return Ok(false);
            }
            sleep(Duration::from_millis(50));
        }
    }

    #[test]
    fn job_kills_child_on_handle_drop() -> Result<()> {
        let mut child = spawn_long_lived()?;

        let job = create_and_assign_sidecar_job(child.id())?;

        assert!(
            child.try_wait()?.is_none(),
            "child should still be running after being assigned to the job"
        );

        drop(job);

        assert!(
            wait_until_exited(&mut child, Duration::from_secs(5))?,
            "child should be terminated after the job handle is dropped"
        );

        Ok(())
    }

    #[test]
    fn returns_err_for_invalid_pid() {
        let result = create_and_assign_sidecar_job(0xFFFF_FFFC);
        assert!(result.is_err(), "expected Err for a non-existent PID");
    }
}

#[cfg(unix)]
fn discard_unwritable_core_cache(config_dir: &std::path::Path) {
    let cache = config_dir.join("cache.db");
    match std::fs::OpenOptions::new().append(true).open(&cache) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => match std::fs::remove_file(&cache) {
            Ok(()) => logging!(
                info,
                Type::Core,
                "негодный кэш ядра удалён перед запуском: {}",
                cache.display()
            ),
            Err(error) => logging!(
                warn,
                Type::Core,
                "негодный кэш ядра {} удалить не удалось: {error}",
                cache.display()
            ),
        },
        Err(error) => logging!(warn, Type::Core, "кэш ядра {} не проверен: {error}", cache.display()),
    }
}

#[cfg(all(test, unix))]
mod core_cache_tests {
    use super::discard_unwritable_core_cache;
    use std::os::unix::fs::PermissionsExt as _;

    fn scratch(name: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("clod-core-cache-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        assert!(std::fs::create_dir_all(&root).is_ok());
        root
    }

    #[test]
    fn a_cache_the_user_cannot_write_is_discarded() {
        if unsafe { tauri_plugin_clash_verge_sysinfo::libc::geteuid() } == 0 {
            return;
        }
        let root = scratch("unwritable");
        let cache = root.join("cache.db");
        assert!(std::fs::write(&cache, b"stale").is_ok());
        assert!(std::fs::set_permissions(&cache, std::fs::Permissions::from_mode(0o444)).is_ok());

        discard_unwritable_core_cache(&root);

        assert!(!cache.exists());
    }

    #[test]
    fn a_writable_cache_survives() {
        let root = scratch("writable");
        let cache = root.join("cache.db");
        assert!(std::fs::write(&cache, b"live").is_ok());

        discard_unwritable_core_cache(&root);

        assert_eq!(std::fs::read(&cache).unwrap_or_default(), b"live");
    }
}

#[cfg(test)]
mod crash_tests {
    use super::{
        CORE_RESTART_DELAY, CORE_RESTART_DELAY_CAP, RunningMode, restart_delay, terminated_process_is_the_running_one,
    };
    use std::time::Duration;

    #[test]
    fn a_late_exit_of_a_replaced_process_is_not_a_crash() {
        assert!(terminated_process_is_the_running_one(
            &RunningMode::Sidecar,
            Some(10),
            Some(10)
        ));
        assert!(!terminated_process_is_the_running_one(
            &RunningMode::Sidecar,
            Some(11),
            Some(10)
        ));
        assert!(!terminated_process_is_the_running_one(
            &RunningMode::Sidecar,
            None,
            Some(10)
        ));
        assert!(!terminated_process_is_the_running_one(
            &RunningMode::Sidecar,
            Some(10),
            None
        ));
        assert!(terminated_process_is_the_running_one(&RunningMode::Service, None, None));
    }

    #[test]
    fn restart_delay_grows_and_stops_growing() {
        assert_eq!(restart_delay(1), CORE_RESTART_DELAY);
        assert_eq!(restart_delay(2), Duration::from_secs(2));
        assert_eq!(restart_delay(3), CORE_RESTART_DELAY_CAP);
        assert_eq!(restart_delay(9), CORE_RESTART_DELAY_CAP);
    }
}

#[cfg(test)]
mod health_tests {
    use super::{HealthStep, HealthWatch, ServiceLifecycleState, ServiceSample, restart_notice_is_due};
    use crate::constants::timing;
    use std::time::{Duration, Instant};

    fn running(core_pid: Option<u32>, restart_count: u32) -> ServiceSample {
        ServiceSample::Status {
            is_active: true,
            desired_running: true,
            state: ServiceLifecycleState::Running,
            core_pid,
            restart_count,
            last_exit_reason: None,
        }
    }

    fn in_state(state: ServiceLifecycleState, restart_count: u32) -> ServiceSample {
        ServiceSample::Status {
            is_active: true,
            desired_running: true,
            state,
            core_pid: None,
            restart_count,
            last_exit_reason: Some(String::from("exit code 2")),
        }
    }

    #[test]
    fn a_healthy_core_is_still_asked_directly() {
        let mut watch = HealthWatch::default();
        assert_eq!(watch.observe(running(Some(7), 3)), HealthStep::ProbeTheCore);
        assert_eq!(watch.core_probed(true), HealthStep::Continue);
        assert_eq!(watch.observe(running(Some(7), 3)), HealthStep::ProbeTheCore);
    }

    #[test]
    fn a_hung_core_that_the_service_still_sees_is_lost_after_repeated_silence() {
        let mut watch = HealthWatch::default();
        for _ in 1..timing::CORE_HEALTH_MISSES {
            assert_eq!(watch.observe(running(Some(7), 0)), HealthStep::ProbeTheCore);
            assert_eq!(watch.core_probed(false), HealthStep::Continue);
        }
        assert_eq!(watch.observe(running(Some(7), 0)), HealthStep::ProbeTheCore);
        assert_eq!(
            watch.core_probed(false),
            HealthStep::CoreLost("the core stopped answering under the service")
        );
    }

    #[test]
    fn one_answer_clears_the_silence() {
        let mut watch = HealthWatch::default();
        assert_eq!(watch.observe(running(Some(7), 0)), HealthStep::ProbeTheCore);
        assert_eq!(watch.core_probed(false), HealthStep::Continue);
        assert_eq!(watch.observe(running(Some(7), 0)), HealthStep::ProbeTheCore);
        assert_eq!(watch.core_probed(true), HealthStep::Continue);
        assert_eq!(watch.observe(running(Some(7), 0)), HealthStep::ProbeTheCore);
        assert_eq!(watch.core_probed(false), HealthStep::Continue);
    }

    #[test]
    fn a_restart_done_by_the_service_is_reported_once_with_its_count_and_reason() {
        let mut watch = HealthWatch::default();
        assert_eq!(watch.observe(running(Some(7), 3)), HealthStep::ProbeTheCore);
        assert_eq!(
            watch.observe(in_state(ServiceLifecycleState::RecoveringCore, 3)),
            HealthStep::Continue
        );
        let ServiceSample::Status { last_exit_reason, .. } = in_state(ServiceLifecycleState::Running, 5) else {
            unreachable!()
        };
        let sample = ServiceSample::Status {
            is_active: true,
            desired_running: true,
            state: ServiceLifecycleState::Running,
            core_pid: Some(8),
            restart_count: 5,
            last_exit_reason,
        };
        assert_eq!(
            watch.observe(sample),
            HealthStep::RestartedByService {
                restarts: 2,
                reason: String::from("exit code 2"),
            }
        );
        assert_eq!(watch.observe(running(Some(8), 5)), HealthStep::ProbeTheCore);
    }

    #[test]
    fn the_first_sample_sets_the_baseline_instead_of_reporting_history() {
        let mut watch = HealthWatch::default();
        assert_eq!(watch.observe(running(Some(7), 42)), HealthStep::ProbeTheCore);
    }

    #[test]
    fn a_service_that_was_itself_restarted_starts_a_new_baseline() {
        let mut watch = HealthWatch::default();
        assert_eq!(watch.observe(running(Some(7), 42)), HealthStep::ProbeTheCore);
        assert_eq!(watch.observe(running(Some(9), 0)), HealthStep::ProbeTheCore);
        assert!(matches!(
            watch.observe(running(Some(10), 1)),
            HealthStep::RestartedByService { restarts: 1, .. }
        ));
    }

    #[test]
    fn a_service_that_gave_up_hands_the_core_back_to_us() {
        let mut watch = HealthWatch::default();
        assert_eq!(watch.observe(running(Some(7), 0)), HealthStep::ProbeTheCore);
        assert!(matches!(
            watch.observe(in_state(ServiceLifecycleState::Fatal, 0)),
            HealthStep::CoreLost(_)
        ));
    }

    #[test]
    fn a_service_still_recovering_is_left_to_it() {
        let mut watch = HealthWatch::default();
        for _ in 0..10 {
            assert_eq!(
                watch.observe(in_state(ServiceLifecycleState::RecoveringCore, 0)),
                HealthStep::Continue
            );
        }
    }

    #[test]
    fn a_missing_core_outside_recovery_counts_towards_loss() {
        let mut watch = HealthWatch::default();
        for _ in 1..timing::CORE_HEALTH_MISSES {
            assert_eq!(watch.observe(running(None, 0)), HealthStep::Continue);
        }
        assert!(matches!(watch.observe(running(None, 0)), HealthStep::CoreLost(_)));
    }

    #[test]
    fn a_silent_service_falls_back_to_asking_the_core() {
        let mut watch = HealthWatch::default();
        assert_eq!(watch.observe(ServiceSample::Unreadable), HealthStep::ProbeTheCore);
        assert_eq!(watch.core_probed(true), HealthStep::Continue);
        for _ in 1..timing::CORE_HEALTH_MISSES {
            assert_eq!(watch.observe(ServiceSample::Unreadable), HealthStep::ProbeTheCore);
            assert_eq!(watch.core_probed(false), HealthStep::Continue);
        }
        assert_eq!(watch.observe(ServiceSample::Unreadable), HealthStep::ProbeTheCore);
        assert_eq!(
            watch.core_probed(false),
            HealthStep::CoreLost("neither the service nor the core answers")
        );
    }

    #[test]
    fn a_displaced_owner_and_a_stop_requested_elsewhere_are_losses_for_us() {
        let mut watch = HealthWatch::default();
        let displaced = ServiceSample::Status {
            is_active: false,
            desired_running: true,
            state: ServiceLifecycleState::Running,
            core_pid: Some(5),
            restart_count: 0,
            last_exit_reason: None,
        };
        assert!(matches!(watch.observe(displaced), HealthStep::CoreLost(_)));
        let stopped = ServiceSample::Status {
            is_active: true,
            desired_running: false,
            state: ServiceLifecycleState::Running,
            core_pid: None,
            restart_count: 0,
            last_exit_reason: None,
        };
        assert!(matches!(watch.observe(stopped), HealthStep::CoreLost(_)));
    }

    #[test]
    fn an_unreadable_wish_to_stop_does_not_kill_a_core_that_is_still_there() {
        let mut watch = HealthWatch::default();
        let stopped_on_paper = ServiceSample::Status {
            is_active: true,
            desired_running: false,
            state: ServiceLifecycleState::Running,
            core_pid: Some(5),
            restart_count: 0,
            last_exit_reason: None,
        };
        assert_eq!(watch.observe(stopped_on_paper), HealthStep::ProbeTheCore);
    }

    #[test]
    fn the_restart_notice_is_not_repeated_within_the_stable_window() {
        let now = Instant::now();
        assert!(restart_notice_is_due(None, now));
        assert!(!restart_notice_is_due(Some(now), now + Duration::from_secs(5)));
        assert!(restart_notice_is_due(Some(now), now + super::CORE_STABLE_AFTER));
    }
}
