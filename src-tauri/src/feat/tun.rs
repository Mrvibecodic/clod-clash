use std::{
    sync::atomic::{AtomicBool, AtomicU32, Ordering},
    time::{Duration, Instant},
};

use clash_verge_logging::{Type, logging};
use parking_lot::Mutex;
use tauri_plugin_clash_verge_sysinfo::is_current_app_handle_admin;

use crate::{
    config::Config,
    constants::timing,
    core::{
        handle::Handle,
        service::{
            ElevationPending, SERVICE_MANAGER, ServiceBusy, ServiceRegistration, ServiceStatus, elevation_in_flight,
            is_service_available, service_registration, start_registered_service,
        },
    },
    process::AsyncHandler,
};

static SUPPRESSED: AtomicBool = AtomicBool::new(false);
static START_FAILED: AtomicBool = AtomicBool::new(false);
static SETUP_RUNNING: AtomicBool = AtomicBool::new(false);
static WATCHDOG_RUNNING: AtomicBool = AtomicBool::new(false);
static START_ATTEMPTS: AtomicU32 = AtomicU32::new(0);
static WATCH_ANCHOR: Mutex<Option<String>> = Mutex::new(None);

const TUN_FAILURE_MARKERS: &[&str] = &["start tun listening error", "configure tun interface"];

const TUN_START_ATTEMPTS: u32 = 3;
const TUN_RETRY_DELAY: Duration = Duration::from_secs(5);
const TUN_PATCH_TIMEOUT: Duration = Duration::from_secs(3);

pub fn is_suppressed() -> bool {
    SUPPRESSED.load(Ordering::Acquire)
}

pub fn suppress(reason: &str) {
    if !SUPPRESSED.swap(true, Ordering::AcqRel) {
        logging!(warn, Type::Core, "TUN suppressed for this session: {}", reason);
    }
}

pub fn clear_suppression() {
    SUPPRESSED.store(false, Ordering::Release);
    START_FAILED.store(false, Ordering::Release);
    START_ATTEMPTS.store(0, Ordering::Release);
}

pub async fn desired() -> bool {
    Config::verge().await.latest_arc().enable_tun_mode.unwrap_or(false)
}

const fn is_claimed(desired: bool, suppressed: bool) -> bool {
    desired && !suppressed
}

async fn claimed() -> bool {
    is_claimed(desired().await, is_suppressed())
}

pub fn is_active_with(desired: bool) -> bool {
    is_claimed(desired, is_suppressed())
}

pub fn is_app_elevated() -> bool {
    is_current_app_handle_admin(Handle::app_handle())
}

pub async fn is_capable() -> bool {
    if is_app_elevated() {
        return true;
    }
    if is_service_available().await.is_err() {
        return false;
    }
    !clash_verge_service_ipc::is_reinstall_service_needed().await
}

pub async fn needs_repair() -> bool {
    service_needs_repair().await
}

async fn service_needs_repair() -> bool {
    is_service_available().await.is_ok() && clash_verge_service_ipc::is_reinstall_service_needed().await
}

pub fn line_reports_tun_failure(line: &str) -> bool {
    let lowered = line.to_ascii_lowercase();
    TUN_FAILURE_MARKERS.iter().any(|marker| lowered.contains(marker))
}

pub fn report_start_failure(detail: &str) {
    if START_FAILED.swap(true, Ordering::AcqRel) {
        return;
    }

    let attempt = START_ATTEMPTS.fetch_add(1, Ordering::AcqRel) + 1;
    if should_retry(attempt) {
        logging!(
            warn,
            Type::Core,
            "TUN failed to start ({} of {}): {}",
            attempt,
            TUN_START_ATTEMPTS,
            detail
        );
        schedule_retry();
        return;
    }

    give_up_on_tun(detail);
}

const fn should_retry(attempt: u32) -> bool {
    attempt < TUN_START_ATTEMPTS
}

fn give_up_on_tun(detail: &str) {
    suppress("core failed to start the TUN device");
    logging!(error, Type::Core, "TUN failed to start: {}", detail);
    Handle::notice_message("tun::start_failed", detail.to_owned());

    let detail = detail.to_owned();
    AsyncHandler::spawn(move || async move {
        if let Err(e) = crate::core::CoreManager::global().update_config_checked().await {
            logging!(
                warn,
                Type::Core,
                "failed to drop TUN from the running config after {}: {}",
                detail,
                e
            );
        }
        Handle::refresh_verge();
        let _ = crate::core::tray::Tray::global().update_menu().await;
    });
}

fn schedule_retry() {
    AsyncHandler::spawn(|| async {
        tokio::time::sleep(TUN_RETRY_DELAY).await;
        if !claimed().await {
            return;
        }
        START_FAILED.store(false, Ordering::Release);
        recreate_tun_device().await;
    });
}

async fn recreate_tun_device() {
    let anchor = log_anchor().await;

    for (step, enable) in [("off", false), ("on", true)] {
        let patch = serde_json::json!({ "tun": { "enable": enable } });
        let mihomo = Handle::mihomo().await;
        match tokio::time::timeout(TUN_PATCH_TIMEOUT, mihomo.patch_base_config(&patch)).await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                logging!(warn, Type::Core, "could not switch the TUN device {}: {}", step, e);
                return;
            }
            Err(_) => {
                logging!(
                    warn,
                    Type::Core,
                    "the core did not answer switching the TUN device {}",
                    step
                );
                return;
            }
        }
    }

    logging!(info, Type::Core, "the TUN device was re-created");
    spawn_start_verification(anchor);
}

pub async fn rearm_after_wake() {
    if !claimed().await {
        return;
    }
    START_ATTEMPTS.store(0, Ordering::Release);
    START_FAILED.store(false, Ordering::Release);
    recreate_tun_device().await;
}

pub async fn recheck_after_network_change() {
    if !claimed().await {
        return;
    }
    START_ATTEMPTS.store(0, Ordering::Release);
    START_FAILED.store(false, Ordering::Release);
    spawn_start_verification(log_anchor().await);
}

pub async fn log_anchor() -> Option<String> {
    crate::core::CoreManager::global()
        .get_clash_logs()
        .await
        .ok()
        .and_then(|logs| logs.last().map(ToString::to_string))
}

pub fn spawn_start_verification(anchor: Option<String>) {
    *WATCH_ANCHOR.lock() = anchor;
    AsyncHandler::spawn(|| async {
        tokio::time::sleep(timing::TUN_VERIFY_DELAY).await;
        if !claimed().await {
            return;
        }
        if matches!(verify_round().await, Round::Watching) {
            spawn_watchdog();
        }
    });
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Round {
    Watching,
    Done,
}

async fn verify_round() -> Round {
    let anchor = WATCH_ANCHOR.lock().clone();
    let Ok(logs) = crate::core::CoreManager::global().get_clash_logs().await else {
        return Round::Watching;
    };
    match verdict(&logs, anchor.as_deref()) {
        Verdict::Failed(line) => {
            report_start_failure(line);
            Round::Done
        }
        Verdict::Clean(next) => {
            *WATCH_ANCHOR.lock() = next;
            Round::Watching
        }
    }
}

fn spawn_watchdog() {
    if WATCHDOG_RUNNING.swap(true, Ordering::AcqRel) {
        return;
    }
    AsyncHandler::spawn(|| async {
        scopeguard::defer! {
            WATCHDOG_RUNNING.store(false, Ordering::Release);
        }
        loop {
            tokio::time::sleep(timing::TUN_WATCH_INTERVAL).await;
            if !claimed().await || matches!(verify_round().await, Round::Done) {
                return;
            }
        }
    });
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Verdict<'a> {
    Failed(&'a str),
    Clean(Option<String>),
}

fn verdict<'a, S: AsRef<str>>(logs: &'a [S], anchor: Option<&str>) -> Verdict<'a> {
    if let Some(line) = fresh_failure(logs, anchor) {
        return Verdict::Failed(line.as_ref());
    }
    Verdict::Clean(
        logs.last()
            .map(|line| line.as_ref().to_owned())
            .or_else(|| anchor.map(ToOwned::to_owned)),
    )
}

fn fresh_failure<'a, S: AsRef<str>>(logs: &'a [S], anchor: Option<&str>) -> Option<&'a S> {
    let from = anchor
        .and_then(|anchor| logs.iter().rposition(|line| line.as_ref() == anchor))
        .map_or(0, |position| position + 1);
    logs[from..]
        .iter()
        .rev()
        .take(200)
        .find(|line| line_reports_tun_failure(line.as_ref()))
}

async fn record_setup_attempt() {
    let version = env!("CARGO_PKG_VERSION");
    let verge = Config::verge().await;
    verge.edit_draft(|d| {
        d.tun_setup_declined = Some(version.into());
    });
    verge.apply();
    let data = Config::verge().await.latest_arc();
    if let Err(e) = data.save_file().await {
        logging!(warn, Type::Core, "failed to persist the TUN setup attempt: {}", e);
    }
    Handle::refresh_verge();
}

pub async fn setup_declined_for_this_version() -> bool {
    Config::verge()
        .await
        .latest_arc()
        .tun_setup_declined
        .as_deref()
        .is_some_and(|declined_at| declined_at == env!("CARGO_PKG_VERSION"))
}

pub async fn clear_setup_declined() {
    if Config::verge().await.latest_arc().tun_setup_declined.is_none() {
        return;
    }
    let verge = Config::verge().await;
    verge.edit_draft(|d| {
        d.tun_setup_declined = None;
    });
    verge.apply();
    let data = Config::verge().await.latest_arc();
    if let Err(e) = data.save_file().await {
        logging!(warn, Type::Core, "failed to clear the TUN setup decline: {}", e);
    }
    Handle::refresh_verge();
}

async fn proven_alive_at_startup(user_initiated: bool) {
    if !user_initiated {
        clear_setup_declined().await;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupOutcome {
    AlreadyReady,
    Installed,
    Declined,
    Failed,
    Pending,
}

pub async fn ensure_ready(user_initiated: bool) -> SetupOutcome {
    if already_ready(user_initiated).await {
        return SetupOutcome::AlreadyReady;
    }

    if user_initiated {
        clear_setup_declined().await;
    } else if setup_declined_for_this_version().await {
        logging!(
            info,
            Type::Service,
            "service setup was already attempted on this version; not asking again"
        );
        return SetupOutcome::Declined;
    }

    if SETUP_RUNNING.swap(true, Ordering::AcqRel) {
        return SetupOutcome::Declined;
    }
    scopeguard::defer! {
        SETUP_RUNNING.store(false, Ordering::Release);
    }

    set_up_service().await
}

async fn already_ready(user_initiated: bool) -> bool {
    if is_capable().await {
        clear_suppression();
        proven_alive_at_startup(user_initiated).await;
        return true;
    }

    if nudge_registered_service().await || wait_until_capable(true).await {
        clear_suppression();
        proven_alive_at_startup(user_initiated).await;
        crate::core::CoreManager::global().handoff_to_service_if_needed().await;
        return true;
    }

    false
}

async fn nudge_registered_service() -> bool {
    if !matches!(service_registration(), ServiceRegistration::Stopped) {
        return false;
    }

    logging!(
        info,
        Type::Service,
        "the service is registered but stopped; starting it without asking for rights"
    );

    let started = tokio::task::spawn_blocking(start_registered_service)
        .await
        .unwrap_or(false);
    if !started {
        return false;
    }

    wait_until_capable(false).await
}

async fn set_up_service() -> SetupOutcome {
    let action = required_action().await;
    logging!(
        info,
        Type::Service,
        "preparing the background service for TUN: {:?}",
        action
    );
    let _ = SERVICE_MANAGER.current().await;
    Handle::notice_message("tun::setup_started", "");

    if let Err(e) = SERVICE_MANAGER.handle_service_status(action).await {
        if e.downcast_ref::<ServiceBusy>().is_some() {
            if elevation_in_flight() {
                logging!(
                    info,
                    Type::Service,
                    "an authorisation dialog is already open; not asking a second time"
                );
                return SetupOutcome::Pending;
            }
            logging!(info, Type::Service, "the service manager is busy; leaving it be");
            return SetupOutcome::Declined;
        }
        if e.downcast_ref::<ElevationPending>().is_some() {
            logging!(
                warn,
                Type::Service,
                "the authorisation dialog is still open; not waiting for it any longer"
            );
            record_setup_attempt().await;
            return SetupOutcome::Pending;
        }
        let detail = format!("{e}");
        logging!(warn, Type::Service, "background service setup failed: {}", detail);
        record_setup_attempt().await;
        Handle::notice_message("tun::setup_failed", detail);
        return SetupOutcome::Failed;
    }

    record_setup_attempt().await;

    if !wait_until_capable(false).await {
        logging!(
            warn,
            Type::Service,
            "the service setup reported success, but the service still does not answer"
        );
        Handle::notice_message("tun::setup_failed", "service is silent after setup".to_owned());
        return SetupOutcome::Failed;
    }

    clear_suppression();
    logging!(info, Type::Service, "background service is up");
    Handle::notice_message("tun::setup_done", "");
    Handle::refresh_verge();
    crate::core::CoreManager::global().handoff_to_service_if_needed().await;
    SetupOutcome::Installed
}

async fn required_action() -> ServiceStatus {
    action_for(service_registration(), service_needs_repair().await)
}

const fn action_for(registration: ServiceRegistration, needs_repair: bool) -> ServiceStatus {
    if needs_repair {
        return ServiceStatus::ReinstallRequired;
    }

    match registration {
        ServiceRegistration::Missing | ServiceRegistration::Stopped => ServiceStatus::InstallRequired,
        ServiceRegistration::Running => ServiceStatus::ForceReinstallRequired,
        ServiceRegistration::Unknown => ServiceStatus::InstallRequired,
    }
}

async fn wait_until_capable(trust_registration: bool) -> bool {
    let deadline = Instant::now() + timing::TUN_SERVICE_APPEAR_WAIT;
    loop {
        if is_capable().await {
            return true;
        }
        let registration = service_registration();
        let pointless = matches!(registration, ServiceRegistration::Missing)
            || (trust_registration
                && (matches!(registration, ServiceRegistration::Stopped) || service_needs_repair().await));
        if pointless || Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(timing::TUN_SERVICE_APPEAR_INTERVAL).await;
    }
}

pub async fn init_startup_setup() {
    if !desired().await {
        logging!(
            info,
            Type::Service,
            "TUN is off; leaving the background service alone at startup"
        );
        return;
    }

    let outcome = ensure_ready(false).await;
    logging!(info, Type::Service, "startup TUN readiness: {:?}", outcome);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_the_core_tun_failures() {
        assert!(line_reports_tun_failure(
            "Start TUN listening error: configure tun interface: Connect: operation not permitted"
        ));
        assert!(line_reports_tun_failure("configure tun interface: Access is denied."));
        assert!(!line_reports_tun_failure("[TCP] tun accept connection"));
        assert!(!line_reports_tun_failure("Start initial provider default"));
    }

    #[test]
    fn asks_for_the_smallest_service_action() {
        assert_eq!(
            action_for(ServiceRegistration::Unknown, false),
            ServiceStatus::InstallRequired
        );
        assert_eq!(
            action_for(ServiceRegistration::Missing, false),
            ServiceStatus::InstallRequired
        );
        assert_eq!(
            action_for(ServiceRegistration::Stopped, false),
            ServiceStatus::InstallRequired
        );
        assert_eq!(
            action_for(ServiceRegistration::Running, false),
            ServiceStatus::ForceReinstallRequired
        );
        for registration in [
            ServiceRegistration::Missing,
            ServiceRegistration::Stopped,
            ServiceRegistration::Running,
            ServiceRegistration::Unknown,
        ] {
            assert_eq!(action_for(registration, true), ServiceStatus::ReinstallRequired);
        }
    }

    #[test]
    fn old_complaints_do_not_count_against_a_new_attempt() {
        const FAILURE: &str = "Start TUN listening error: configure tun interface: Access is denied.";
        let logs = [FAILURE, "[TCP] tun accept connection"];
        assert!(fresh_failure(&logs, Some("[TCP] tun accept connection")).is_none());
        assert_eq!(
            fresh_failure(&logs, Some("Start initial provider default")).copied(),
            Some(FAILURE)
        );
        assert_eq!(fresh_failure(&logs, None).copied(), Some(FAILURE));
        assert_eq!(fresh_failure(&logs, Some("evicted line")).copied(), Some(FAILURE));
        let repeated = ["[TCP] tun accept connection", FAILURE, "[TCP] tun accept connection"];
        assert!(fresh_failure(&repeated, Some("[TCP] tun accept connection")).is_none());
    }

    #[test]
    fn the_watchdog_runs_exactly_while_tun_is_claimed() {
        assert!(is_claimed(true, false));
        assert!(!is_claimed(false, false));
        assert!(!is_claimed(true, true));
        assert!(!is_claimed(false, true));
    }

    #[test]
    fn each_round_moves_the_anchor_to_the_last_seen_line() {
        const FAILURE: &str = "Start TUN listening error: configure tun interface: Access is denied.";
        let logs = ["[TCP] tun accept connection", "Start initial provider default"];
        assert_eq!(
            verdict(&logs, Some("[TCP] tun accept connection")),
            Verdict::Clean(Some("Start initial provider default".to_owned()))
        );
        let empty: [&str; 0] = [];
        assert_eq!(
            verdict(&empty, Some("anchor")),
            Verdict::Clean(Some("anchor".to_owned()))
        );
        assert_eq!(verdict(&empty, None), Verdict::Clean(None));
        let broken = ["[TCP] tun accept connection", FAILURE];
        assert_eq!(
            verdict(&broken, Some("[TCP] tun accept connection")),
            Verdict::Failed(FAILURE)
        );
        assert_eq!(
            verdict(&broken, Some(FAILURE)),
            Verdict::Clean(Some(FAILURE.to_owned()))
        );
    }

    #[test]
    fn suppression_is_a_session_flag() {
        clear_suppression();
        assert!(!is_suppressed());
        suppress("test");
        assert!(is_suppressed());
        clear_suppression();
        assert!(!is_suppressed());
    }

    #[test]
    fn the_tunnel_is_retried_before_it_is_given_up_on() {
        const { assert!(TUN_START_ATTEMPTS > 1) };
        assert!(should_retry(1));
        assert!(should_retry(TUN_START_ATTEMPTS - 1));
        assert!(!should_retry(TUN_START_ATTEMPTS));
        assert!(!should_retry(TUN_START_ATTEMPTS + 1));
    }

    #[test]
    fn a_cleared_suppression_gives_the_tunnel_a_fresh_budget() {
        START_ATTEMPTS.store(TUN_START_ATTEMPTS, Ordering::Release);
        suppress("test");
        clear_suppression();
        assert_eq!(START_ATTEMPTS.load(Ordering::Acquire), 0);
        assert!(!is_suppressed());
    }
}
