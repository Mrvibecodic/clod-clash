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
    auto_url: std::string::String,
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
            auto_url: auto.url,
        })
    }

    fn describe(&self) -> std::string::String {
        format!(
            "sysproxy enable={} {}:{}, autoproxy enable={} url={}",
            self.sys_enable, self.host, self.port, self.auto_enable, self.auto_url
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

fn bare_host(host: &str) -> &str {
    host.strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .unwrap_or(host)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SystemProxyOwnership {
    ours: bool,
    someone_else_is_switched_on: bool,
}

fn how_the_system_proxy_stands_with(
    observed: &ObservedProxy,
    ours: &[(&str, u16, &str)],
    trust_auto_readback: bool,
) -> SystemProxyOwnership {
    let manual_is_ours = observed.sys_enable
        && ours
            .iter()
            .any(|(host, port, _)| bare_host(&observed.host) == bare_host(host) && observed.port == *port);
    let pac_is_ours = ours
        .iter()
        .any(|(_, _, pac_url)| !pac_url.is_empty() && observed.auto_url == *pac_url);

    SystemProxyOwnership {
        ours: manual_is_ours || pac_is_ours,
        someone_else_is_switched_on: (observed.sys_enable && !manual_is_ours)
            || (trust_auto_readback && observed.auto_enable && !pac_is_ours),
    }
}

fn how_the_system_proxy_stands(observed: &ObservedProxy, ours: &[(&str, u16, &str)]) -> SystemProxyOwnership {
    how_the_system_proxy_stands_with(observed, ours, AUTO_SWITCH_READBACK_IS_TRUSTWORTHY)
}

const fn nothing_of_ours_can_be_in_the_system(ever_applied: bool, wants_proxy: bool) -> bool {
    !ever_applied && !wants_proxy
}

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
        !self.sys_enable || (bare_host(&observed.host) == bare_host(&self.host) && observed.port == self.port)
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

fn refused_by_the_system(wanted: &WantedProxy, after: Option<&ObservedProxy>) -> Option<anyhow::Error> {
    match after {
        Some(state) if !wanted.accepted_by(state) => Some(the_system_refused(wanted, Some(state))),
        _ => None,
    }
}

fn the_system_refused(wanted: &WantedProxy, after: Option<&ObservedProxy>) -> anyhow::Error {
    anyhow::anyhow!(
        "the system did not accept the proxy settings: wanted {}, the system reports {}",
        wanted.describe(),
        after.map_or_else(ObservedProxy::unreadable, ObservedProxy::describe)
    )
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

    async fn system_proxy_ownership(&self) -> Option<SystemProxyOwnership> {
        let verge = Config::verge().await.latest_arc();
        let host = verge.proxy_host.as_deref().unwrap_or("127.0.0.1").to_owned();
        drop(verge);
        let port = Config::effective_mixed_port().await;
        let pac_url = format!("http://{host}:{}/commands/pac", IVerge::get_singleton_port());

        let observed = tokio::task::spawn_blocking(ObservedProxy::read).await.ok()??;

        let previous = self.applied_target.read().as_ref().map(|target| {
            (
                target.sys.host.to_string(),
                target.sys.port,
                target.auto.url.to_string(),
            )
        });
        let mut ours = vec![(host.as_str(), port, pac_url.as_str())];
        if let Some((host, port, pac_url)) = previous.as_ref() {
            ours.push((host.as_str(), *port, pac_url.as_str()));
        }

        Some(how_the_system_proxy_stands(&observed, &ours))
    }

    async fn wants_system_proxy(&self) -> bool {
        Config::verge()
            .await
            .latest_arc()
            .enable_system_proxy
            .unwrap_or_default()
    }

    pub async fn reset_sysproxy_if_ours(&self) -> Result<()> {
        if nothing_of_ours_can_be_in_the_system(
            self.ever_applied.load(Ordering::SeqCst),
            self.wants_system_proxy().await,
        ) {
            logging!(
                info,
                Type::Core,
                "системный прокси этим приложением не ставился — снимать нечего"
            );
            return Ok(());
        }

        if self
            .system_proxy_ownership()
            .await
            .is_some_and(|ownership| !ownership.ours || ownership.someone_else_is_switched_on)
        {
            logging!(
                info,
                Type::Core,
                "системный прокси в системе поставлен не нами — не трогаем"
            );
            return Ok(());
        }

        self.ever_applied.store(true, Ordering::SeqCst);
        self.reset_sysproxy().await
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

    pub async fn update_sysproxy(&self) -> Result<()> {
        if handle::Handle::global().is_exiting() {
            return Ok(());
        }
        let _lock = self.update_lock.lock().await;

        let verge = Config::verge().await.latest_arc();
        let port = Config::effective_mixed_port().await;
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
        if !wants_to_enable {
            let was_ever_applied = self.ever_applied.load(Ordering::SeqCst);
            if self.system_proxy_ownership().await.is_some_and(|ownership| {
                ownership.someone_else_is_switched_on || (!ownership.ours && !was_ever_applied)
            }) {
                logging!(
                    info,
                    Type::Core,
                    "в системе стоят чужие настройки прокси — выключать их не будем"
                );
                self.last_write_failed.store(false, Ordering::SeqCst);
                self.aim_guard(guard_type);
                return Ok(());
            }
        }
        self.ever_applied.store(true, Ordering::SeqCst);

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

            if failure.is_none() {
                failure = refused_by_the_system(&probe, after.as_ref());
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
                self.last_write_failed.store(true, Ordering::SeqCst);
                self.aim_guard(GuardType::None);
                Err(error)
            }
            None => {
                let owns_the_state = target.sys.enable || target.auto.enable;
                self.ever_applied.store(owns_the_state, Ordering::SeqCst);
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

    async fn reset_sysproxy(&self) -> Result<()> {
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

        let port = Config::effective_mixed_port().await;
        let host = Config::verge()
            .await
            .latest_arc()
            .proxy_host
            .as_deref()
            .unwrap_or("127.0.0.1")
            .to_owned();
        let pac_port = IVerge::get_singleton_port();

        let bypass = get_bypass().await;

        let (sys, auto) = {
            let (sys, auto) = &mut *self.inner_proxy.write();
            if sys.host.is_empty() {
                sys.host = host.as_str().into();
                sys.port = port;
                sys.bypass = bypass.as_str().into();
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
        BYPASS_SEPARATOR, DEFAULT_BYPASS, ObservedProxy, ProxyApplyStep, WantedProxy, format_bypass,
        how_the_system_proxy_stands, how_the_system_proxy_stands_with, nothing_of_ours_can_be_in_the_system,
        proxy_apply_steps, refused_by_the_system,
    };

    fn observed(sys_enable: bool, host: &str, port: u16, auto_enable: bool) -> ObservedProxy {
        ObservedProxy {
            sys_enable,
            host: host.to_owned(),
            port,
            auto_enable,
            auto_url: std::string::String::new(),
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
    fn a_wanted_mode_goes_on_before_the_other_one_goes_off() {
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

    fn seen(sys_enable: bool, host: &str, port: u16, auto_enable: bool, auto_url: &str) -> ObservedProxy {
        ObservedProxy {
            sys_enable,
            host: host.to_owned(),
            port,
            auto_enable,
            auto_url: auto_url.to_owned(),
        }
    }

    const OUR_PAC: &str = "http://127.0.0.1:33331/commands/pac";

    fn ownership_of(state: &ObservedProxy) -> super::SystemProxyOwnership {
        how_the_system_proxy_stands(state, &[("127.0.0.1", 7897, OUR_PAC)])
    }

    #[test]
    fn a_bracketed_ipv6_host_still_recognises_itself() {
        let state = seen(true, "::1", 7897, false, "");

        assert!(how_the_system_proxy_stands(&state, &[("[::1]", 7897, OUR_PAC)]).ours);
        assert!(how_the_system_proxy_stands(&state, &[("::1", 7897, OUR_PAC)]).ours);
        assert!(!how_the_system_proxy_stands(&state, &[("::2", 7897, OUR_PAC)]).ours);
    }

    #[test]
    fn a_proxy_on_someone_elses_address_is_not_ours() {
        let ownership = ownership_of(&seen(true, "10.0.0.1", 3128, false, ""));

        assert!(!ownership.ours);
        assert!(ownership.someone_else_is_switched_on);
    }

    #[test]
    fn our_own_pac_url_proves_the_proxy_is_ours_even_when_the_switch_reads_off() {
        let switched_on = ownership_of(&seen(false, "", 0, true, OUR_PAC));
        let switch_reads_off = ownership_of(&seen(false, "", 0, false, OUR_PAC));

        assert!(switched_on.ours);
        assert!(!switched_on.someone_else_is_switched_on);
        assert!(switch_reads_off.ours);
        assert!(!switch_reads_off.someone_else_is_switched_on);
    }

    #[test]
    fn a_foreign_manual_proxy_beside_our_stale_pac_url_stays_switched_on() {
        let state = seen(true, "10.0.0.1", 3128, true, OUR_PAC);
        let ours = [("127.0.0.1", 7897, OUR_PAC)];

        for trust_auto_readback in [true, false] {
            let ownership = how_the_system_proxy_stands_with(&state, &ours, trust_auto_readback);
            assert!(ownership.ours);
            assert!(ownership.someone_else_is_switched_on, "{trust_auto_readback}");
        }
    }

    #[test]
    fn a_foreign_autoconfig_url_blocks_us_only_where_its_switch_can_be_read() {
        let state = seen(true, "127.0.0.1", 7897, true, "http://proxy.corp/wpad.dat");
        let ours = [("127.0.0.1", 7897, OUR_PAC)];

        assert!(how_the_system_proxy_stands_with(&state, &ours, true).someone_else_is_switched_on);
        assert!(!how_the_system_proxy_stands_with(&state, &ours, false).someone_else_is_switched_on);
    }

    #[test]
    fn a_proxy_that_is_ours_end_to_end_leaves_nothing_foreign_switched_on() {
        let ownership = ownership_of(&seen(true, "127.0.0.1", 7897, false, ""));

        assert!(ownership.ours);
        assert!(!ownership.someone_else_is_switched_on);
    }

    #[test]
    fn a_proxy_left_over_from_the_previous_target_is_still_ours() {
        let state = seen(true, "127.0.0.1", 7890, false, "");
        let only_now = ownership_of(&state);
        let with_the_previous_target =
            how_the_system_proxy_stands(&state, &[("127.0.0.1", 7897, OUR_PAC), ("127.0.0.1", 7890, OUR_PAC)]);

        assert!(!only_now.ours);
        assert!(only_now.someone_else_is_switched_on);
        assert!(with_the_previous_target.ours);
        assert!(!with_the_previous_target.someone_else_is_switched_on);
    }

    #[test]
    fn a_foreign_pac_url_is_not_ours() {
        let state = seen(false, "", 0, true, "http://proxy.corp/wpad.dat");
        let ours = [("127.0.0.1", 7897, OUR_PAC)];

        assert!(!ownership_of(&state).ours);
        assert!(how_the_system_proxy_stands_with(&state, &ours, true).someone_else_is_switched_on);
    }

    #[test]
    fn an_empty_pac_url_matches_nothing() {
        let state = seen(false, "", 0, true, "");

        assert!(!how_the_system_proxy_stands(&state, &[("127.0.0.1", 7897, "")]).ours);
    }

    #[test]
    fn an_untouched_system_holds_nothing_of_anyones() {
        let ownership = ownership_of(&seen(false, "", 0, false, ""));

        assert!(!ownership.ours);
        assert!(!ownership.someone_else_is_switched_on);
    }

    #[test]
    fn a_readback_that_disagrees_with_the_target_is_a_failed_write() {
        let want = wanted(true, "127.0.0.1", 7897, false);

        assert!(refused_by_the_system(&want, Some(&observed(true, "127.0.0.1", 7890, false))).is_some());
        assert!(refused_by_the_system(&want, Some(&observed(false, "", 0, false))).is_some());
        assert!(refused_by_the_system(&want, Some(&observed(true, "127.0.0.1", 7897, false))).is_none());
        assert!(refused_by_the_system(&want, None).is_none());
    }

    #[test]
    fn the_exit_reads_the_system_only_when_something_of_ours_could_be_there() {
        assert!(nothing_of_ours_can_be_in_the_system(false, false));
        assert!(!nothing_of_ours_can_be_in_the_system(true, false));
        assert!(!nothing_of_ours_can_be_in_the_system(false, true));
        assert!(!nothing_of_ours_can_be_in_the_system(true, true));
    }

    #[test]
    fn a_bracketed_ipv6_host_is_not_read_as_a_refused_write() {
        let want = wanted(true, "[::1]", 7897, false);

        assert!(want.accepted_by(&observed(true, "::1", 7897, false)));
        assert!(!want.accepted_by(&observed(true, "::2", 7897, false)));
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
