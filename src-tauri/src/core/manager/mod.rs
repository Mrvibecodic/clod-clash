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
        Arc, LazyLock,
        atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering},
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

static MODE_SERVICE: LazyLock<Arc<RunningMode>> = LazyLock::new(|| Arc::new(RunningMode::Service));
static MODE_SIDECAR: LazyLock<Arc<RunningMode>> = LazyLock::new(|| Arc::new(RunningMode::Sidecar));
static MODE_NOT_RUNNING: LazyLock<Arc<RunningMode>> = LazyLock::new(|| Arc::new(RunningMode::NotRunning));

/// Каким способом ядро поднимается: намерение, а не состояние процесса.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Backend {
    Sidecar = 0,
    Service = 1,
}

impl Backend {
    const fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::Service,
            _ => Self::Sidecar,
        }
    }
}

/// Что с процессом ядра на самом деле.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Liveness {
    /// Ядра нет: оно не запускалось либо остановка прошла.
    Down = 0,
    /// Ядро поднято.
    Up = 1,
    /// Мы прямо сейчас просим это ядро умереть: смерть плановая.
    Stopping = 2,
    /// Остановка не удалась: процесс ядра жив, и это тот же самый процесс.
    StopFailed = 3,
}

impl Liveness {
    const fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::Up,
            2 => Self::Stopping,
            3 => Self::StopFailed,
            _ => Self::Down,
        }
    }
}

/// Как два раздельных факта читаются тем, кто спрашивает «что с ядром».
const fn running_mode_of(liveness: Liveness, backend: Backend) -> RunningMode {
    match (liveness, backend) {
        (Liveness::Down, _) => RunningMode::NotRunning,
        (_, Backend::Service) => RunningMode::Service,
        (_, Backend::Sidecar) => RunningMode::Sidecar,
    }
}

/// Смерть, которую заказала остановка: воскрешать такое ядро нельзя.
const fn a_death_we_asked_for(liveness: Liveness) -> bool {
    matches!(liveness, Liveness::Stopping | Liveness::StopFailed)
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
    backend: AtomicU8,
    liveness: AtomicU8,
    child_sidecar: ArcSwapOption<CommandChild>,
    sidecar_pid: AtomicU32,
}

impl Default for State {
    fn default() -> Self {
        Self {
            backend: AtomicU8::new(Backend::Sidecar as u8),
            liveness: AtomicU8::new(Liveness::Down as u8),
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
        match running_mode_of(self.liveness(), self.backend()) {
            RunningMode::Service => Arc::clone(&MODE_SERVICE),
            RunningMode::Sidecar => Arc::clone(&MODE_SIDECAR),
            RunningMode::NotRunning => Arc::clone(&MODE_NOT_RUNNING),
        }
    }

    pub(super) fn backend(&self) -> Backend {
        Backend::from_u8(self.state.load().backend.load(Ordering::Acquire))
    }

    fn liveness(&self) -> Liveness {
        Liveness::from_u8(self.state.load().liveness.load(Ordering::Acquire))
    }

    /// Остановка не удалась, и ядро прежнего запуска всё ещё живо.
    pub fn stop_failed(&self) -> bool {
        matches!(self.liveness(), Liveness::StopFailed)
    }

    pub(super) fn a_death_we_asked_for(&self) -> bool {
        a_death_we_asked_for(self.liveness())
    }

    /// Ядро прежнего запуска живо, а власти над ним у приложения больше нет:
    /// новый процесс встал бы вторым ядром на тот же порт.
    pub(super) fn refuse_to_double_the_core(&self) -> Result<()> {
        if self.stop_failed() {
            anyhow::bail!("ядро прежнего запуска не остановилось — второй процесс не поднимаем");
        }
        Ok(())
    }

    /// Намерение: каким способом пойдёт следующий запуск.
    pub(super) fn aim_at(&self, backend: Backend) {
        self.state.load().backend.store(backend as u8, Ordering::Release);
    }

    pub(super) fn note_core_is_up(&self, backend: Backend) {
        let state = self.state.load();
        state.backend.store(backend as u8, Ordering::Release);
        state.liveness.store(Liveness::Up as u8, Ordering::Release);
    }

    pub(super) fn note_stopping(&self) {
        self.state
            .load()
            .liveness
            .store(Liveness::Stopping as u8, Ordering::Release);
    }

    pub(super) fn note_core_is_down(&self) {
        self.state
            .load()
            .liveness
            .store(Liveness::Down as u8, Ordering::Release);
    }

    /// Отказ убийства применяется только к живому ядру: если оно тем временем
    /// умерло само, «не остановилось» было бы враньём.
    pub(super) fn note_stop_failed(&self) {
        let state = self.state.load();
        for was in [Liveness::Stopping, Liveness::Up] {
            if state
                .liveness
                .compare_exchange(
                    was as u8,
                    Liveness::StopFailed as u8,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
                .is_ok()
            {
                return;
            }
        }
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

#[cfg(test)]
mod tests {
    use super::{Backend, CoreManager, Liveness, RunningMode, a_death_we_asked_for, running_mode_of};

    #[test]
    fn only_a_core_confirmed_gone_reads_as_no_core() {
        for backend in [Backend::Sidecar, Backend::Service] {
            assert_eq!(running_mode_of(Liveness::Down, backend), RunningMode::NotRunning);
            for liveness in [Liveness::Up, Liveness::Stopping, Liveness::StopFailed] {
                assert_ne!(
                    running_mode_of(liveness, backend),
                    RunningMode::NotRunning,
                    "{liveness:?}/{backend:?}: живое ядро не имеет права выглядеть как «ядра нет» — \
                     следующий запуск поднял бы второй процесс поверх него"
                );
            }
        }
    }

    #[test]
    fn a_living_core_is_read_as_the_backend_that_runs_it() {
        for liveness in [Liveness::Up, Liveness::Stopping, Liveness::StopFailed] {
            assert_eq!(running_mode_of(liveness, Backend::Service), RunningMode::Service);
            assert_eq!(running_mode_of(liveness, Backend::Sidecar), RunningMode::Sidecar);
        }
    }

    #[test]
    fn a_death_the_stop_asked_for_is_never_a_crash() {
        assert!(a_death_we_asked_for(Liveness::Stopping));
        assert!(a_death_we_asked_for(Liveness::StopFailed));
        assert!(!a_death_we_asked_for(Liveness::Up));
        assert!(!a_death_we_asked_for(Liveness::Down));
    }

    #[test]
    fn both_cells_survive_the_round_trip_through_the_atomic() {
        for backend in [Backend::Sidecar, Backend::Service] {
            assert_eq!(Backend::from_u8(backend as u8), backend);
        }
        for liveness in [Liveness::Down, Liveness::Up, Liveness::Stopping, Liveness::StopFailed] {
            assert_eq!(Liveness::from_u8(liveness as u8), liveness);
        }
    }

    #[test]
    fn aiming_at_a_backend_does_not_claim_a_core_is_running() {
        let manager = CoreManager::default();
        manager.aim_at(Backend::Service);
        assert_eq!(manager.backend(), Backend::Service);
        assert_eq!(*manager.get_running_mode(), RunningMode::NotRunning);
    }

    #[test]
    fn a_failed_stop_keeps_the_core_visible_and_refuses_a_second_one() {
        let manager = CoreManager::default();
        manager.note_core_is_up(Backend::Sidecar);
        manager.note_stopping();
        manager.note_stop_failed();

        assert!(manager.stop_failed());
        assert_eq!(*manager.get_running_mode(), RunningMode::Sidecar);
        assert!(manager.refuse_to_double_the_core().is_err());
    }

    #[test]
    fn a_core_that_died_on_its_own_is_not_relabelled_as_a_failed_stop() {
        let manager = CoreManager::default();
        manager.note_core_is_up(Backend::Sidecar);
        manager.note_stopping();
        manager.note_core_is_down();
        manager.note_stop_failed();

        assert!(!manager.stop_failed());
        assert_eq!(*manager.get_running_mode(), RunningMode::NotRunning);
        assert!(manager.refuse_to_double_the_core().is_ok());
    }

    #[test]
    fn a_confirmed_stop_opens_the_way_for_the_next_start() {
        let manager = CoreManager::default();
        manager.note_core_is_up(Backend::Service);
        manager.note_stopping();
        manager.note_core_is_down();

        assert_eq!(*manager.get_running_mode(), RunningMode::NotRunning);
        assert!(manager.refuse_to_double_the_core().is_ok());
    }
}
