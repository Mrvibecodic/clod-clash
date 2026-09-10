mod config;
mod lifecycle;
mod state;

pub use lifecycle::ExitStop;

use anyhow::Result;
use arc_swap::{ArcSwap, ArcSwapOption};
use clash_verge_logger::AsyncLogger;
use once_cell::sync::Lazy;
use std::{
    fmt,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
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
    handoff_watcher_generation: AtomicU64,
    starting: AtomicBool,
    restart_pending: AtomicBool,
    // Сколько плановых пауз ядра идёт прямо сейчас: обычный перезапуск,
    // замена сборки ядра, удаление службы, передача ядра службе. Счётчик, а не
    // флаг — паузы вкладываются друг в друга.
    planned_pauses: AtomicU32,
}

/// Плановая пауза ядра: пока она жива, «ядра нет» не показывается.
///
/// Признак «идёт перезапуск» ставился только после падения; штатный
/// перезапуск, замена ядра и удаление службы (остановка → удаление на
/// секунды → запуск) его не ставили, и на это время главная показывала
/// красный баннер «ядро не запущено» с кнопкой «Запустить», а трей — обычный
/// значок. Отпускается при выходе из области — в том числе по ошибке.
pub struct PlannedPause<'a> {
    manager: &'a CoreManager,
}

impl Drop for PlannedPause<'_> {
    fn drop(&mut self) {
        self.manager.planned_pauses.fetch_sub(1, Ordering::AcqRel);
    }
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
            handoff_watcher_generation: AtomicU64::new(0),
            starting: AtomicBool::new(false),
            restart_pending: AtomicBool::new(false),
            planned_pauses: AtomicU32::new(0),
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

    pub fn is_starting(&self) -> bool {
        self.starting.load(Ordering::Acquire)
            || self.restart_pending.load(Ordering::Acquire)
            || self.planned_pauses.load(Ordering::Acquire) > 0
            || !crate::utils::resolve::is_resolve_done()
    }

    pub fn planned_pause(&self) -> PlannedPause<'_> {
        self.planned_pauses.fetch_add(1, Ordering::AcqRel);
        PlannedPause { manager: self }
    }

    pub(super) fn set_restart_pending(&self, pending: bool) {
        self.restart_pending.store(pending, Ordering::Release);
    }

    pub(super) fn claim_sidecar_exit(&self, pid: u32) -> bool {
        self.state
            .load()
            .sidecar_pid
            .compare_exchange(pid, 0, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    pub fn is_down(&self) -> bool {
        matches!(*self.get_running_mode(), RunningMode::NotRunning) && !self.is_starting()
    }

    pub(super) fn mark_starting(&self) {
        self.starting.store(true, Ordering::Release);
    }

    pub(super) fn clear_starting(&self) {
        self.starting.store(false, Ordering::Release);
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
