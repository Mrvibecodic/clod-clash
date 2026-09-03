use crate::{
    config::{Config, IVerge},
    core::handle,
    process::AsyncHandler,
    singleton,
};
use anyhow::Result;
use clash_verge_logging::{Type, logging};
use parking_lot::RwLock;
use scopeguard::defer;
use smartstring::alias::String;
use std::{
    fmt::Write as _,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use sysproxy::{Autoproxy, GuardMonitor, GuardType, Sysproxy};
use tokio::sync::Mutex as TokioMutex;

const PROXY_OBSERVE_TICK: Duration = Duration::from_secs(5);
const RESET_LOCK_BUDGET: Duration = Duration::from_millis(400);

static OBSERVER_RUNNING: AtomicBool = AtomicBool::new(false);

#[derive(Clone, PartialEq, Eq)]
struct ObservedProxy {
    sys_enable: bool,
    host: std::string::String,
    port: u16,
    auto_enable: bool,
}

impl ObservedProxy {
    fn read() -> Option<Self> {
        let sys = Sysproxy::get_system_proxy().ok()?;
        let auto = Autoproxy::get_auto_proxy().ok()?;
        Some(Self {
            sys_enable: sys.enable,
            host: sys.host.to_string(),
            port: sys.port,
            auto_enable: auto.enable,
        })
    }

    fn describe(&self) -> std::string::String {
        format!(
            "sysproxy enable={} {}:{}, autoproxy enable={}",
            self.sys_enable, self.host, self.port, self.auto_enable
        )
    }

    fn unreadable() -> std::string::String {
        std::string::String::from("unreadable")
    }
}

#[derive(Clone)]
struct WantedProxy {
    sys_enable: bool,
    auto_enable: bool,
    host: std::string::String,
    port: u16,
}

#[derive(PartialEq, Eq)]
struct AppliedTarget {
    sys: Sysproxy,
    auto: Autoproxy,
    pac_body: Option<std::string::String>,
}

struct AppliedProxy {
    before: Option<ObservedProxy>,
    after: Option<ObservedProxy>,
    steps: std::string::String,
    failure: Option<anyhow::Error>,
    skipped: bool,
}

const AUTO_SWITCH_READBACK_IS_TRUSTWORTHY: bool = !cfg!(target_os = "windows");

impl WantedProxy {
    const fn switches_match_with(&self, observed: &ObservedProxy, trust_auto_readback: bool) -> bool {
        if observed.sys_enable != self.sys_enable {
            return false;
        }
        if !trust_auto_readback && !self.auto_enable {
            return true;
        }
        observed.auto_enable == self.auto_enable
    }

    const fn switches_match(&self, observed: &ObservedProxy) -> bool {
        self.switches_match_with(observed, AUTO_SWITCH_READBACK_IS_TRUSTWORTHY)
    }

    fn accepted_by(&self, observed: &ObservedProxy) -> bool {
        if !self.switches_match(observed) {
            return false;
        }
        !self.sys_enable || (observed.host == self.host && observed.port == self.port)
    }

    fn describe(&self) -> std::string::String {
        format!(
            "sysproxy enable={} {}:{}, autoproxy enable={}",
            self.sys_enable, self.host, self.port, self.auto_enable
        )
    }
}

fn apply_once(
    sys: &Sysproxy,
    auto: &Autoproxy,
    apply_steps: &[ProxyApplyStep],
    log: &mut std::string::String,
) -> Option<anyhow::Error> {
    let mut failure = None;

    for step in apply_steps.iter().copied() {
        let started = Instant::now();
        let outcome = match step {
            ProxyApplyStep::Autoproxy => with_system_call_retry(|| auto.set_auto_proxy()),
            ProxyApplyStep::Sysproxy => with_system_call_retry(|| sys.set_system_proxy()),
        };
        if !log.is_empty() {
            log.push_str(", ");
        }
        let _ = write!(log, "{step:?} {}ms", started.elapsed().as_millis());
        if let Err(e) = outcome {
            log.push_str(" FAILED");
            if failure.is_none() {
                failure = Some(anyhow::Error::new(e));
            }
        }
    }

    failure
}

fn report_applied(
    wanted: &WantedProxy,
    before: Option<&ObservedProxy>,
    after: Option<&ObservedProxy>,
    steps: &str,
    verbose: bool,
) {
    if let Some(after) = after
        && !wanted.accepted_by(after)
    {
        logging!(
            warn,
            Type::Core,
            "the system did not accept the proxy settings: wanted {}, the system reports {}",
            wanted.describe(),
            after.describe()
        );
    }

    if !verbose {
        return;
    }

    logging!(
        info,
        Type::Core,
        "proxy settings written: wanted {}, before {}, steps [{steps}], after {}",
        wanted.describe(),
        before.map_or_else(ObservedProxy::unreadable, ObservedProxy::describe),
        after.map_or_else(ObservedProxy::unreadable, ObservedProxy::describe)
    );
}

pub async fn verbose_diagnostics() -> bool {
    Config::verge().await.latest_arc().verbose_diagnostics()
}

pub fn spawn_proxy_observer() {
    AsyncHandler::spawn(|| async {
        if !verbose_diagnostics().await || OBSERVER_RUNNING.swap(true, Ordering::AcqRel) {
            return;
        }
        defer! {
            OBSERVER_RUNNING.store(false, Ordering::SeqCst);
        }

        let mut last: Option<ObservedProxy> = None;

        loop {
            tokio::time::sleep(PROXY_OBSERVE_TICK).await;
            if handle::Handle::global().is_exiting() || !verbose_diagnostics().await {
                return;
            }

            let Ok(Some(now)) = tokio::task::spawn_blocking(ObservedProxy::read).await else {
                continue;
            };
            if last.as_ref() == Some(&now) {
                continue;
            }

            let ours = Sysopt::global().applying.load(Ordering::SeqCst);
            logging!(
                info,
                Type::Core,
                "observed proxy settings: {} (we were {}writing at that moment)",
                now.describe(),
                if ours { "" } else { "not " }
            );
            last = Some(now);
        }
    });
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProxyApplyStep {
    Sysproxy,
    Autoproxy,
}

#[cfg(not(target_os = "macos"))]
const STEPS_SYSPROXY_ONLY: &[ProxyApplyStep] = &[ProxyApplyStep::Sysproxy];
#[cfg(not(target_os = "macos"))]
const STEPS_AUTOPROXY_ONLY: &[ProxyApplyStep] = &[ProxyApplyStep::Autoproxy];
#[cfg(target_os = "macos")]
const STEPS_AUTOPROXY_THEN_SYSPROXY: &[ProxyApplyStep] = &[ProxyApplyStep::Autoproxy, ProxyApplyStep::Sysproxy];
const STEPS_SYSPROXY_THEN_AUTOPROXY: &[ProxyApplyStep] = &[ProxyApplyStep::Sysproxy, ProxyApplyStep::Autoproxy];

#[cfg(not(target_os = "macos"))]
const fn proxy_apply_steps(sys_enabled: bool, auto_enabled: bool) -> &'static [ProxyApplyStep] {
    if sys_enabled {
        STEPS_SYSPROXY_ONLY
    } else if auto_enabled {
        STEPS_AUTOPROXY_ONLY
    } else {
        STEPS_SYSPROXY_THEN_AUTOPROXY
    }
}

#[cfg(target_os = "macos")]
const fn proxy_apply_steps(sys_enabled: bool, _auto_enabled: bool) -> &'static [ProxyApplyStep] {
    if sys_enabled {
        STEPS_SYSPROXY_THEN_AUTOPROXY
    } else {
        STEPS_AUTOPROXY_THEN_SYSPROXY
    }
}

pub struct Sysopt {
    update_lock: TokioMutex<()>,
    reset_sysproxy: AtomicBool,
    applying: AtomicBool,
    inner_proxy: Arc<RwLock<(Sysproxy, Autoproxy)>>,
    applied_target: Arc<RwLock<Option<AppliedTarget>>>,
    last_write_failed: AtomicBool,
    ever_applied: AtomicBool,
    guard: Arc<RwLock<GuardMonitor>>,
}

impl Default for Sysopt {
    fn default() -> Self {
        Self {
            update_lock: TokioMutex::new(()),
            reset_sysproxy: AtomicBool::new(false),
            applying: AtomicBool::new(false),
            inner_proxy: Arc::new(RwLock::new((Sysproxy::default(), Autoproxy::default()))),
            applied_target: Arc::new(RwLock::new(None)),
            last_write_failed: AtomicBool::new(false),
            ever_applied: AtomicBool::new(false),
            guard: Arc::new(RwLock::new(GuardMonitor::new(GuardType::None, Duration::from_secs(30)))),
        }
    }
}

#[cfg(target_os = "windows")]
static DEFAULT_BYPASS: &str = "localhost;127.*;192.168.*;10.*;172.16.*;172.17.*;172.18.*;172.19.*;172.20.*;172.21.*;172.22.*;172.23.*;172.24.*;172.25.*;172.26.*;172.27.*;172.28.*;172.29.*;172.30.*;172.31.*;<local>";
#[cfg(target_os = "windows")]
static BYPASS_SEPARATOR: &str = ";";
#[cfg(target_os = "linux")]
static DEFAULT_BYPASS: &str = "localhost,127.0.0.1,192.168.0.0/16,10.0.0.0/8,172.16.0.0/12,::1";
#[cfg(any(target_os = "linux", target_os = "macos"))]
static BYPASS_SEPARATOR: &str = ",";
#[cfg(target_os = "macos")]
static DEFAULT_BYPASS: &str =
    "127.0.0.1,192.168.0.0/16,10.0.0.0/8,172.16.0.0/12,localhost,*.local,*.crashlytics.com,<local>";

fn format_bypass(use_default: bool, custom_bypass: &str) -> String {
    if custom_bypass.is_empty() {
        DEFAULT_BYPASS.into()
    } else if use_default {
        format!("{DEFAULT_BYPASS}{BYPASS_SEPARATOR}{custom_bypass}").into()
    } else {
        custom_bypass.into()
    }
}

async fn get_bypass() -> String {
    let verge = Config::verge().await.latest_arc();
    let use_default = verge.use_default_bypass.unwrap_or(true);
    let custom_bypass = verge.system_proxy_bypass.as_deref().unwrap_or("");

    format_bypass(use_default, custom_bypass)
}

singleton!(Sysopt, SYSOPT);

impl Sysopt {
    fn new() -> Self {
        Self::default()
    }

    fn access_guard(&self) -> Arc<RwLock<GuardMonitor>> {
        Arc::clone(&self.guard)
    }

    pub async fn refresh_guard(&self) {
        logging!(info, Type::Core, "Refreshing system proxy guard...");
        let verge = Config::verge().await.latest_arc();
        if !verge.enable_system_proxy.unwrap_or_default() {
            logging!(info, Type::Core, "System proxy is disabled.");
            self.access_guard().write().stop();
            return;
        }
        if !verge.enable_proxy_guard.unwrap_or_default() {
            logging!(info, Type::Core, "System proxy guard is disabled.");
            self.access_guard().write().stop();
            return;
        }
        logging!(
            info,
            Type::Core,
            "Updating system proxy with duration: {} seconds",
            verge.proxy_guard_duration.unwrap_or(30)
        );
        {
            let guard = self.access_guard();
            guard
                .write()
                .set_interval(Duration::from_secs(verge.proxy_guard_duration.unwrap_or(30).max(1)));
        }
        logging!(info, Type::Core, "Starting system proxy guard...");
        {
            let guard = self.access_guard();
            guard.write().start();
        }
    }

    pub fn write_failed(&self) -> bool {
        self.last_write_failed.load(Ordering::SeqCst)
    }

    pub fn stop_proxy_guard(&self) {
        let guard = self.access_guard();
        let mut monitor = guard.write();
        monitor.stop();
        monitor.set_guard_type(GuardType::None);
    }

    pub async fn wait_idle(&self) {
        let _ = self.update_lock.lock().await;
    }

    pub fn we_applied_system_proxy(&self) -> bool {
        self.ever_applied.load(Ordering::SeqCst)
    }

    pub async fn update_sysproxy(&self) -> Result<()> {
        if handle::Handle::global().is_exiting() {
            return Ok(());
        }
        let _lock = self.update_lock.lock().await;

        let verge = Config::verge().await.latest_arc();
        let port = match verge.verge_mixed_port {
            Some(port) => port,
            None => Config::clash().await.latest_arc().get_mixed_port(),
        };
        let pac_port = IVerge::get_singleton_port();
        let bypass = get_bypass().await;

        let (sys_enable, pac_enable, proxy_host, proxy_guard) = (
            verge.enable_system_proxy.unwrap_or_default(),
            verge.proxy_auto_config.unwrap_or_default(),
            verge.proxy_host.as_deref().unwrap_or("127.0.0.1"),
            verge.enable_proxy_guard.unwrap_or_default(),
        );

        let (sys, auto, guard_type) = {
            let (sys, auto) = &mut *self.inner_proxy.write();
            sys.host = proxy_host.into();
            sys.port = port;
            sys.bypass = bypass.into();
            auto.url = format!("http://{proxy_host}:{pac_port}/commands/pac");

            let guard_type = if !sys_enable {
                sys.enable = false;
                auto.enable = false;
                GuardType::None
            } else if pac_enable {
                sys.enable = false;
                auto.enable = true;
                if proxy_guard {
                    GuardType::Autoproxy(auto.clone())
                } else {
                    GuardType::None
                }
            } else {
                sys.enable = true;
                auto.enable = false;
                if proxy_guard {
                    GuardType::Sysproxy(sys.clone())
                } else {
                    GuardType::None
                }
            };

            (sys.clone(), auto.clone(), guard_type)
        };

        let apply_steps = proxy_apply_steps(sys.enable, auto.enable);
        let wanted = WantedProxy {
            sys_enable: sys.enable,
            auto_enable: auto.enable,
            host: sys.host.to_string(),
            port: sys.port,
        };
        let verbose = verge.verbose_diagnostics();

        let target = AppliedTarget {
            sys: sys.clone(),
            auto: auto.clone(),
            pac_body: auto
                .enable
                .then(|| verge.pac_file_content.as_deref().unwrap_or_default().to_owned()),
        };
        let readback_can_prove_the_mode = AUTO_SWITCH_READBACK_IS_TRUSTWORTHY || sys.enable;
        let target_is_unchanged = readback_can_prove_the_mode
            && self
                .applied_target
                .read()
                .as_ref()
                .is_some_and(|previous| *previous == target);

        let wants_to_enable = sys.enable || auto.enable;
        let was_ever_applied = self.ever_applied.load(Ordering::SeqCst);
        if wants_to_enable {
            self.ever_applied.store(true, Ordering::SeqCst);
        }

        self.applying.store(true, Ordering::SeqCst);
        defer! {
            self.applying.store(false, Ordering::SeqCst);
        }

        let probe = wanted.clone();
        let applied = tokio::task::spawn_blocking(move || {
            let before = (target_is_unchanged || verbose || wants_to_enable)
                .then(ObservedProxy::read)
                .flatten();

            if target_is_unchanged && before.as_ref().is_some_and(|state| probe.accepted_by(state)) {
                return AppliedProxy {
                    before: None,
                    after: None,
                    steps: std::string::String::new(),
                    failure: None,
                    skipped: true,
                };
            }

            let mut steps = std::string::String::new();
            let mut failure = apply_once(&sys, &auto, apply_steps, &mut steps);
            let mut after = ObservedProxy::read();

            if after.as_ref().is_some_and(|state| !probe.switches_match(state)) {
                steps.push_str("; the system disagreed, writing again: ");
                let retry = apply_once(&sys, &auto, apply_steps, &mut steps);
                failure = failure.or(retry);
                after = ObservedProxy::read();
            }

            AppliedProxy {
                before,
                after,
                steps,
                failure,
                skipped: false,
            }
        })
        .await?;

        if applied.skipped {
            if verbose {
                logging!(
                    info,
                    Type::Core,
                    "system proxy already matches the target, skipped writing"
                );
            }
            self.last_write_failed.store(false, Ordering::SeqCst);
            self.aim_guard(guard_type);
            return Ok(());
        }

        let nothing_changed = applied.before.is_some() && applied.before == applied.after;
        if nothing_changed && !was_ever_applied {
            self.ever_applied.store(false, Ordering::SeqCst);
        }

        report_applied(
            &wanted,
            applied.before.as_ref(),
            applied.after.as_ref(),
            &applied.steps,
            verbose,
        );

        match applied.failure {
            Some(error) => {
                *self.applied_target.write() = None;
                if !nothing_changed {
                    self.ever_applied.store(true, Ordering::SeqCst);
                }
                self.stop_proxy_guard();
                self.last_write_failed.store(true, Ordering::SeqCst);
                Err(error)
            }
            None => {
                self.ever_applied
                    .store(target.sys.enable || target.auto.enable, Ordering::SeqCst);
                *self.applied_target.write() = Some(target);
                self.last_write_failed.store(false, Ordering::SeqCst);
                self.aim_guard(guard_type);
                Ok(())
            }
        }
    }

    fn aim_guard(&self, guard_type: GuardType) {
        self.access_guard().write().set_guard_type(guard_type);
    }

    pub async fn reset_sysproxy(&self) -> Result<()> {
        if self
            .reset_sysproxy
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Ok(());
        }
        defer! {
            self.reset_sysproxy.store(false, Ordering::SeqCst);
        }

        let _lock = tokio::time::timeout(RESET_LOCK_BUDGET, self.update_lock.lock())
            .await
            .ok();

        {
            let guard = self.access_guard();
            let mut monitor = guard.write();
            monitor.stop();
            monitor.set_guard_type(GuardType::None);
        }

        let port = match Config::verge().await.latest_arc().verge_mixed_port {
            Some(port) => port,
            None => Config::clash().await.latest_arc().get_mixed_port(),
        };
        let host = Config::verge()
            .await
            .latest_arc()
            .proxy_host
            .as_deref()
            .unwrap_or("127.0.0.1")
            .to_owned();
        let pac_port = IVerge::get_singleton_port();

        let (sys, auto) = {
            let (sys, auto) = &mut *self.inner_proxy.write();
            if sys.host.is_empty() {
                sys.host = host.as_str().into();
                sys.port = port;
            }
            if auto.url.is_empty() {
                auto.url = format!("http://{host}:{pac_port}/commands/pac");
            }
            sys.enable = false;
            auto.enable = false;
            (sys.clone(), auto.clone())
        };
        *self.applied_target.write() = None;

        let apply_steps = proxy_apply_steps(false, false);
        let outcome = tokio::task::spawn_blocking(move || {
            let mut steps = std::string::String::new();
            apply_once(&sys, &auto, apply_steps, &mut steps)
        })
        .await?;

        if outcome.is_none() {
            self.ever_applied.store(false, Ordering::SeqCst);
        }
        outcome.map_or(Ok(()), Err)
    }
}

const SYSTEM_CALL_ATTEMPTS: u32 = 3;
const SYSTEM_CALL_RETRY_DELAY: Duration = Duration::from_millis(100);

#[cfg(target_os = "windows")]
const fn is_failed_system_call(error: &sysproxy::Error) -> bool {
    matches!(error, sysproxy::Error::SystemCall(_))
}

#[cfg(not(target_os = "windows"))]
const fn is_failed_system_call(_error: &sysproxy::Error) -> bool {
    false
}

fn with_system_call_retry(mut apply: impl FnMut() -> sysproxy::Result<()>) -> sysproxy::Result<()> {
    for _ in 1..SYSTEM_CALL_ATTEMPTS {
        match apply() {
            Err(error) if is_failed_system_call(&error) => {
                logging!(warn, Type::Core, "system proxy write failed, retrying: {error}");
                std::thread::sleep(SYSTEM_CALL_RETRY_DELAY);
            }
            other => return other,
        }
    }
    apply()
}

#[cfg(test)]
mod tests {
    use super::{
        BYPASS_SEPARATOR, DEFAULT_BYPASS, ObservedProxy, ProxyApplyStep, WantedProxy, format_bypass, proxy_apply_steps,
    };

    fn observed(sys_enable: bool, host: &str, port: u16, auto_enable: bool) -> ObservedProxy {
        ObservedProxy {
            sys_enable,
            host: host.to_owned(),
            port,
            auto_enable,
        }
    }

    fn wanted(sys_enable: bool, host: &str, port: u16, auto_enable: bool) -> WantedProxy {
        WantedProxy {
            sys_enable,
            auto_enable,
            host: host.to_owned(),
            port,
        }
    }

    #[test]
    fn the_written_proxy_counts_as_accepted_only_when_the_system_agrees() {
        let want = wanted(true, "127.0.0.1", 7897, false);

        assert!(want.accepted_by(&observed(true, "127.0.0.1", 7897, false)));
        assert!(!want.accepted_by(&observed(false, "127.0.0.1", 7897, false)));
        assert!(!want.accepted_by(&observed(true, "127.0.0.1", 7890, false)));
    }

    #[test]
    fn a_disabled_proxy_is_judged_by_the_switch_alone() {
        let want = wanted(false, "127.0.0.1", 7897, false);

        assert!(want.accepted_by(&observed(false, "", 0, false)));
        assert!(!want.accepted_by(&observed(true, "127.0.0.1", 7897, false)));
    }

    #[test]
    fn empty_custom_bypass_uses_defaults() {
        assert_eq!(format_bypass(false, ""), DEFAULT_BYPASS);
    }

    #[test]
    fn custom_bypass_can_replace_defaults() {
        assert_eq!(format_bypass(false, "example.com"), "example.com");
    }

    #[test]
    fn default_and_custom_bypass_use_platform_separator() {
        assert_eq!(
            format_bypass(true, "example.com"),
            format!("{DEFAULT_BYPASS}{BYPASS_SEPARATOR}example.com")
        );
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn a_shared_mode_switch_is_written_once_and_never_cleared_first() {
        assert_eq!(proxy_apply_steps(true, false), [ProxyApplyStep::Sysproxy]);
        assert_eq!(proxy_apply_steps(false, true), [ProxyApplyStep::Autoproxy]);
        assert_eq!(
            proxy_apply_steps(false, false),
            [ProxyApplyStep::Sysproxy, ProxyApplyStep::Autoproxy]
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn independent_switches_turn_the_wanted_mode_on_before_turning_the_other_off() {
        assert_eq!(
            proxy_apply_steps(true, false),
            [ProxyApplyStep::Sysproxy, ProxyApplyStep::Autoproxy]
        );
        assert_eq!(
            proxy_apply_steps(false, true),
            [ProxyApplyStep::Autoproxy, ProxyApplyStep::Sysproxy]
        );
        assert_eq!(
            proxy_apply_steps(false, false),
            [ProxyApplyStep::Autoproxy, ProxyApplyStep::Sysproxy]
        );
    }

    #[test]
    fn a_stale_pac_url_left_in_the_registry_is_not_read_as_a_failed_write() {
        let want = wanted(true, "127.0.0.1", 7897, false);
        let with_stale_pac = observed(true, "127.0.0.1", 7897, true);

        assert!(!want.switches_match_with(&with_stale_pac, true));
        assert!(want.switches_match_with(&with_stale_pac, false));
    }

    #[test]
    fn a_pac_that_was_asked_for_is_still_verified() {
        let want = wanted(false, "127.0.0.1", 7897, true);

        assert!(!want.switches_match_with(&observed(false, "", 0, false), false));
        assert!(want.switches_match_with(&observed(false, "", 0, true), false));
    }
}
