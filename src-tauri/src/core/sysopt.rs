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

struct AppliedProxy {
    before: Option<ObservedProxy>,
    after: Option<ObservedProxy>,
    steps: std::string::String,
    failure: Option<anyhow::Error>,
}

impl WantedProxy {
    const fn switches_match(&self, observed: &ObservedProxy) -> bool {
        observed.sys_enable == self.sys_enable && observed.auto_enable == self.auto_enable
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
    apply_steps: [ProxyApplyStep; 2],
    log: &mut std::string::String,
) -> Option<anyhow::Error> {
    let mut failure = None;

    for step in apply_steps {
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

const fn proxy_apply_steps(sys_enabled: bool, auto_enabled: bool) -> [ProxyApplyStep; 2] {
    if sys_enabled && !auto_enabled {
        [ProxyApplyStep::Autoproxy, ProxyApplyStep::Sysproxy]
    } else {
        [ProxyApplyStep::Sysproxy, ProxyApplyStep::Autoproxy]
    }
}

pub struct Sysopt {
    update_lock: TokioMutex<()>,
    reset_sysproxy: AtomicBool,
    applying: AtomicBool,
    inner_proxy: Arc<RwLock<(Sysproxy, Autoproxy)>>,
    guard: Arc<RwLock<GuardMonitor>>,
}

impl Default for Sysopt {
    fn default() -> Self {
        Self {
            update_lock: TokioMutex::new(()),
            reset_sysproxy: AtomicBool::new(false),
            applying: AtomicBool::new(false),
            inner_proxy: Arc::new(RwLock::new((Sysproxy::default(), Autoproxy::default()))),
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
                .set_interval(Duration::from_secs(verge.proxy_guard_duration.unwrap_or(30)));
        }
        logging!(info, Type::Core, "Starting system proxy guard...");
        {
            let guard = self.access_guard();
            guard.write().start();
        }
    }

    pub async fn wait_idle(&self) {
        let _ = self.update_lock.lock().await;
    }

    pub fn we_applied_system_proxy(&self) -> bool {
        let (sys, auto) = &*self.inner_proxy.read();
        sys.enable || auto.enable
    }

    pub async fn update_sysproxy(&self) -> Result<()> {
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

        self.access_guard().write().set_guard_type(guard_type);

        let apply_steps = proxy_apply_steps(sys.enable, auto.enable);
        let wanted = WantedProxy {
            sys_enable: sys.enable,
            auto_enable: auto.enable,
            host: sys.host.to_string(),
            port: sys.port,
        };
        let verbose = verge.verbose_diagnostics();

        self.applying.store(true, Ordering::SeqCst);
        defer! {
            self.applying.store(false, Ordering::SeqCst);
        }

        let probe = wanted.clone();
        let applied = tokio::task::spawn_blocking(move || {
            let before = verbose.then(ObservedProxy::read).flatten();
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
            }
        })
        .await?;

        report_applied(
            &wanted,
            applied.before.as_ref(),
            applied.after.as_ref(),
            &applied.steps,
            verbose,
        );

        applied.failure.map_or(Ok(()), Err)
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

        self.access_guard().write().set_guard_type(GuardType::None);

        let (sys, auto) = {
            let (sys, auto) = &mut *self.inner_proxy.write();
            sys.enable = false;
            auto.enable = false;
            (sys.clone(), auto.clone())
        };

        tokio::task::spawn_blocking(move || -> Result<()> {
            with_system_call_retry(|| sys.set_system_proxy())?;
            with_system_call_retry(|| auto.set_auto_proxy())?;
            Ok(())
        })
        .await??;

        Ok(())
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
        assert!(!want.accepted_by(&observed(true, "127.0.0.1", 7897, true)));
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

    #[test]
    fn pure_sysproxy_mode_clears_pac_before_enabling_global_proxy() {
        assert_eq!(
            proxy_apply_steps(true, false),
            [ProxyApplyStep::Autoproxy, ProxyApplyStep::Sysproxy]
        );
    }

    #[test]
    fn pac_mode_clears_global_proxy_before_enabling_pac() {
        assert_eq!(
            proxy_apply_steps(false, true),
            [ProxyApplyStep::Sysproxy, ProxyApplyStep::Autoproxy]
        );
    }

    #[test]
    fn disabled_mode_clears_global_proxy_before_pac() {
        assert_eq!(
            proxy_apply_steps(false, false),
            [ProxyApplyStep::Sysproxy, ProxyApplyStep::Autoproxy]
        );
    }
}
