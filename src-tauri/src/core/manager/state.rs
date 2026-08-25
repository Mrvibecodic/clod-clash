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
use compact_str::CompactString;
use log::Level;
use scopeguard::defer;
use std::{
    sync::atomic::{AtomicBool, AtomicU32, Ordering},
    time::Duration,
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
const MAX_CRASH_RESTARTS: u32 = 3;
const CORE_RESTART_DELAY: Duration = Duration::from_secs(1);
const CORE_STABLE_AFTER: Duration = Duration::from_secs(60);

static SERVICE_WATCHDOG_RUNNING: AtomicBool = AtomicBool::new(false);

fn exit_is_a_crash(current: &RunningMode, expected: &RunningMode, app_exiting: bool) -> bool {
    !app_exiting && current == expected
}

fn handle_core_exit(message: &str, expected: &RunningMode) {
    let manager = CoreManager::global();
    if !exit_is_a_crash(
        &manager.get_running_mode(),
        expected,
        handle::Handle::global().is_exiting(),
    ) {
        return;
    }

    logging!(warn, Type::Core, "core exited unexpectedly: {}", message);
    manager.set_running_mode(RunningMode::NotRunning);

    let attempt = CRASH_RESTARTS.fetch_add(1, Ordering::AcqRel) + 1;
    if attempt > MAX_CRASH_RESTARTS {
        logging!(
            error,
            Type::Core,
            "core crashed {} times in a row; not restarting automatically",
            attempt
        );
        handle::Handle::notice_message("core::crashed", message.to_owned());
        return;
    }

    AsyncHandler::spawn(move || async move {
        tokio::time::sleep(CORE_RESTART_DELAY).await;
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

pub(super) fn spawn_service_health_watchdog() {
    if SERVICE_WATCHDOG_RUNNING.swap(true, Ordering::AcqRel) {
        return;
    }
    AsyncHandler::spawn(|| async {
        defer! {
            SERVICE_WATCHDOG_RUNNING.store(false, Ordering::Release);
        }
        let mut misses: u32 = 0;
        loop {
            tokio::time::sleep(timing::CORE_HEALTH_INTERVAL).await;

            let manager = CoreManager::global();
            if handle::Handle::global().is_exiting() || !matches!(*manager.get_running_mode(), RunningMode::Service) {
                return;
            }
            if manager.is_config_update_in_progress() {
                misses = 0;
                continue;
            }
            if core_answers().await {
                misses = 0;
                continue;
            }

            misses += 1;
            logging!(
                warn,
                Type::Core,
                "the core did not answer under the service ({}/{})",
                misses,
                timing::CORE_HEALTH_MISSES
            );
            if misses < timing::CORE_HEALTH_MISSES {
                continue;
            }

            handle_core_exit("the core stopped answering under the service", &RunningMode::Service);
            return;
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
        let previous_mask = unsafe { tauri_plugin_clash_verge_sysinfo::libc::umask(0o007) };
        let (mut rx, child) = command
            .args([
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
            ])
            .spawn()?;
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

        #[cfg(unix)]
        unsafe {
            tauri_plugin_clash_verge_sysinfo::libc::umask(previous_mask)
        };

        let pid = child.pid();
        logging!(trace, Type::Core, "Sidecar started with PID: {}", pid);

        self.set_running_child_sidecar(child);
        self.set_running_mode(RunningMode::Sidecar);

        AsyncHandler::spawn(|| async move {
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
                        handle_core_exit(&message, &RunningMode::Sidecar);
                        break;
                    }
                    _ => continue,
                };
                let message = CompactString::from(&*String::from_utf8_lossy(&line));
                Logger::global().writer_sidecar_log(level, &message);
                if crate::feat::tun::line_reports_tun_failure(&message) {
                    crate::feat::tun::report_start_failure(&message);
                }
                CLASH_LOGGER.append_log(message).await;
            }
        });

        Ok(())
    }

    pub(super) fn stop_core_by_sidecar(&self) {
        logging!(info, Type::Core, "Stopping sidecar");
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
