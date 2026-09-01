mod config;
mod lifecycle;
mod state;

use anyhow::Result;
use arc_swap::{ArcSwap, ArcSwapOption};
use clash_verge_logger::AsyncLogger;
use once_cell::sync::Lazy;
use std::{
    fmt,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    time::Instant,
};
use tauri_plugin_shell::process::CommandChild;

use crate::singleton;
#[cfg(target_os = "windows")]
use std::os::windows::io::OwnedHandle;

pub(crate) static CLASH_LOGGER: Lazy<Arc<AsyncLogger>> = Lazy::new(|| Arc::new(AsyncLogger::new()));

#[derive(Debug, serde::Serialize, PartialEq, Eq)]
pub enum RunningMode {
    Service,
    Sidecar,
    NotRunning,
}

impl fmt::Display for RunningMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Service => write!(f, "Service"),
            Self::Sidecar => write!(f, "Sidecar"),
            Self::NotRunning => write!(f, "NotRunning"),
        }
    }
}

#[derive(Debug)]
pub struct CoreManager {
    state: ArcSwap<State>,
    last_update: ArcSwapOption<Instant>,
    #[cfg(target_os = "windows")]
    job_handle: ArcSwapOption<OwnedHandle>,
    config_update_in_progress: AtomicBool,
    // Сериализует start/stop/restart и передачу sidecar→service.
    // Порядок блокировок фиксирован: config_update_in_progress → lifecycle_lock.
    lifecycle_lock: tokio::sync::Mutex<()>,
    // Флаг синглтона watcher передачи sidecar→service.
    handoff_watcher_running: AtomicBool,
}

#[derive(Debug)]
struct State {
    running_mode: ArcSwap<RunningMode>,
    child_sidecar: ArcSwapOption<CommandChild>,
    sidecar_pid: AtomicU32,
}

impl Default for State {
    fn default() -> Self {
        Self {
            running_mode: ArcSwap::new(Arc::new(RunningMode::NotRunning)),
            child_sidecar: ArcSwapOption::new(None),
            sidecar_pid: AtomicU32::new(0),
        }
    }
}

impl Default for CoreManager {
    fn default() -> Self {
        Self {
            state: ArcSwap::new(Arc::new(State::default())),
            last_update: ArcSwapOption::new(None),
            #[cfg(target_os = "windows")]
            job_handle: ArcSwapOption::new(None),
            config_update_in_progress: AtomicBool::new(false),
            lifecycle_lock: tokio::sync::Mutex::new(()),
            handoff_watcher_running: AtomicBool::new(false),
        }
    }
}

impl CoreManager {
    fn new() -> Self {
        Self::default()
    }

    pub fn get_running_mode(&self) -> Arc<RunningMode> {
        Arc::clone(&self.state.load().running_mode.load())
    }

    pub fn sidecar_pid(&self) -> Option<u32> {
        match self.state.load().sidecar_pid.load(Ordering::Acquire) {
            0 => None,
            pid => Some(pid),
        }
    }

    pub(super) fn set_sidecar_pid(&self, pid: u32) {
        self.state.load().sidecar_pid.store(pid, Ordering::Release);
    }

    pub(super) fn clear_sidecar_pid(&self) {
        self.state.load().sidecar_pid.store(0, Ordering::Release);
    }

    pub fn take_child_sidecar(&self) -> Option<CommandChild> {
        self.state
            .load()
            .child_sidecar
            .swap(None)
            .and_then(|arc| Arc::try_unwrap(arc).ok())
    }

    pub fn get_last_update(&self) -> Option<Arc<Instant>> {
        self.last_update.load_full()
    }

    pub fn set_running_mode(&self, mode: RunningMode) {
        let state = self.state.load();
        state.running_mode.store(Arc::new(mode));
    }

    pub fn set_running_child_sidecar(&self, child: CommandChild) {
        let state = self.state.load();
        state.child_sidecar.store(Some(Arc::new(child)));
    }

    pub fn set_last_update(&self, time: Instant) {
        self.last_update.store(Some(Arc::new(time)));
    }

    /// Replaces the Windows Job Object handle owned by the core manager
    ///
    /// Passing `None` drops the current handle, which closes the Job Object
    /// and terminates its assigned processes due to `KILL_ON_JOB_CLOSE`.
    ///
    /// This method is currently only used on Windows.
    #[cfg(target_os = "windows")]
    fn set_job_handle(&self, handle: Option<OwnedHandle>) {
        self.job_handle.store(handle.map(Arc::new));
    }

    fn try_start_config_update(&self) -> bool {
        !self.config_update_in_progress.swap(true, Ordering::AcqRel)
    }

    fn finish_config_update(&self) {
        self.config_update_in_progress.store(false, Ordering::Release);
    }

    /// clod:core-health — ядру сейчас законно не до ответов: идёт применение
    /// конфига. Сторож такой круг пропускает, иначе обычная перезагрузка
    /// конфига под службой засчиталась бы ему как смерть ядра.
    pub(super) fn is_config_update_in_progress(&self) -> bool {
        self.config_update_in_progress.load(Ordering::Acquire)
    }

    pub async fn init(&self) -> Result<()> {
        self.start_core().await?;
        Ok(())
    }
}

singleton!(CoreManager, CORE_MANAGER);
