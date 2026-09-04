use super::{CoreManager, RunningMode};
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

static SERVICE_WATCHDOG_GENERATION: AtomicU64 = AtomicU64::new(0);

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

fn handle_core_exit(message: &str, expected: &RunningMode, terminated_pid: Option<u32>) {
    let manager = CoreManager::global();
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

    logging!(warn, Type::Core, "core exited unexpectedly: {}", message);
    manager.clear_sidecar_pid();
    manager.set_running_mode(RunningMode::NotRunning);
    manager.after_core_process();

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
            return;
        }
        logging!(
            info,
            Type::Core,
            "restarting the core after a crash (attempt {})",
            attempt
        );
        if let Err(e) = manager.start_core().await {
            logging!(error, Type::Core, "failed to restart the core after a crash: {}", e);
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
    tokio::time::timeout(timing::CORE_HEALTH_INTERVAL, async {
        handle::Handle::mihomo().await.get_version().await.is_ok()
    })
    .await
    .unwrap_or(false)
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
        if self.silent >= timing::CORE_HEALTH_MISSES {
            HealthStep::CoreLost("the core stopped answering under the service")
        } else {
            HealthStep::Continue
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
    if wants_sysproxy && let Err(e) = crate::core::sysopt::Sysopt::global().update_sysproxy().await {
        logging!(
            warn,
            Type::Core,
            "failed to reapply the system proxy after a restart: {}",
            e
        );
    }
    crate::feat::tun::enforce_undesired_off().await;
    notice_the_restart(reason);
    true
}

pub(super) fn spawn_service_health_watchdog() {
    let generation = SERVICE_WATCHDOG_GENERATION.fetch_add(1, Ordering::AcqRel) + 1;
    AsyncHandler::spawn(move || async move {
        let mut watch = HealthWatch::default();
        let mut skipped: u32 = 0;
        loop {
            let manager = CoreManager::global();
            if handle::Handle::global().is_exiting()
                || SERVICE_WATCHDOG_GENERATION.load(Ordering::Acquire) != generation
                || !matches!(*manager.get_running_mode(), RunningMode::Service)
            {
                return;
            }
            if manager.is_config_update_in_progress() {
                skipped += 1;
                if skipped <= timing::CORE_HEALTH_MAX_SKIPS {
                    watch = HealthWatch {
                        restart_count: watch.restart_count,
                        ..HealthWatch::default()
                    };
                    tokio::time::sleep(timing::CORE_HEALTH_INTERVAL).await;
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

            let mut step = watch.observe(sample_the_service().await);
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
            if SERVICE_WATCHDOG_GENERATION.load(Ordering::Acquire) != generation {
                return;
            }
            match step {
                HealthStep::Continue | HealthStep::ProbeTheCore => {}
                HealthStep::RestartedByService { restarts, reason } => {
                    logging!(
                        warn,
                        Type::Core,
                        "the service restarted the core by itself ({} time(s) since we looked): {}",
                        restarts,
                        reason
                    );
                    let _ = after_core_came_back(&reason).await;
                }
                HealthStep::CoreLost(why) => {
                    handle_core_exit(why, &RunningMode::Service, None);
                    return;
                }
            }
            tokio::time::sleep(timing::CORE_HEALTH_INTERVAL).await;
        }
    });
}

impl CoreManager {
    pub async fn get_clash_logs(&self) -> Result<Vec<CompactString>> {
        match *self.get_running_mode() {
            RunningMode::Service => service::get_clash_logs_by_service().await,
            RunningMode::Sidecar => Ok(CLASH_LOGGER.get_logs().await),
            RunningMode::NotRunning => Ok(Vec::new()),
        }
    }

    pub(super) async fn start_core_by_sidecar(&self) -> Result<()> {
        logging!(info, Type::Core, "Starting core in sidecar mode");

        let sidecar_ipc = dirs::sidecar_ipc_path()?;
        handle::Handle::app_handle()
            .mihomo()
            .write()
            .await
            .update_socket_path(dirs::path_to_str(&sidecar_ipc)?.to_owned())?;

        let config_file = Config::generate_file(crate::config::ConfigType::Run).await?;
        let app_handle = handle::Handle::app_handle();
        let clash_core = Config::verge().await.latest_arc().get_valid_clash_core();
        let config_dir = dirs::app_home_dir()?;

        let managed_binary = crate::core::core_updater::managed_core_binary().await;
        let command = match &managed_binary {
            Some(path) => {
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
        self.set_running_mode(RunningMode::Sidecar);

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

    pub(super) fn stop_core_by_sidecar(&self) {
        logging!(info, Type::Core, "Stopping sidecar");
        self.clear_sidecar_pid();
        defer! {
            self.set_running_mode(RunningMode::NotRunning);
        }
        if let Some(child) = self.take_child_sidecar() {
            let pid = child.pid();

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

            let result = child.kill();
            logging!(
                trace,
                Type::Core,
                "Sidecar stopped (PID: {:?}, Result: {:?})",
                pid,
                result
            );
        }
    }

    pub(super) async fn start_core_by_service(&self) -> Result<()> {
        logging!(info, Type::Core, "Starting core in service mode");

        let service_ipc = dirs::service_ipc_path()?;
        handle::Handle::app_handle()
            .mihomo()
            .write()
            .await
            .update_socket_path(dirs::path_to_str(&service_ipc)?.to_owned())?;

        let config_file = Config::generate_file(crate::config::ConfigType::Run).await?;

        #[cfg(target_os = "windows")]
        {
            let mut last_err = None;
            for attempt in 0..timing::SERVICE_START_RETRIES {
                match service::run_core_by_service(&config_file).await {
                    Ok(()) => {
                        self.set_running_mode(RunningMode::Service);
                        spawn_service_health_watchdog();
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
            self.set_running_mode(RunningMode::Service);
            spawn_service_health_watchdog();
            Ok(())
        }
    }

    pub(super) async fn stop_core_by_service(&self) -> Result<()> {
        logging!(info, Type::Core, "Stopping service");
        SERVICE_WATCHDOG_GENERATION.fetch_add(1, Ordering::AcqRel);
        self.clear_sidecar_pid();
        defer! {
            self.set_running_mode(RunningMode::NotRunning);
        }
        service::stop_core_by_service().await?;
        Ok(())
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
        assert!(matches!(watch.core_probed(false), HealthStep::CoreLost(_)));
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
        assert!(matches!(watch.core_probed(false), HealthStep::CoreLost(_)));
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
