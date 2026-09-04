#[cfg(any(windows, target_os = "macos"))]
use crate::utils::dirs;
use crate::{
    config::Config,
    constants::timing,
    core::{
        handle::Handle, owner_identity::current_owner_credentials, runtime_bundle::collect_runtime_bundle, tray::Tray,
    },
};
use anyhow::{Context as _, Result, bail};
use backon::{ConstantBuilder, Retryable as _};
use clash_verge_logging::{Type, logging};
use clash_verge_service_ipc::{
    OwnerSessionProof, ServiceErrorCode, ServiceStatusSnapshot, StageRuntimeOutcome, StartClashRequest, WriterConfig,
};
use compact_str::CompactString;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use scopeguard::defer;
use std::{
    borrow::Cow,
    env::current_exe,
    future::Future,
    path::{Path, PathBuf},
    process::Command as StdCommand,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};
use tokio::sync::Notify;

static ACTIVE_SERVICE_SESSION: Lazy<Mutex<Option<ActiveServiceSession>>> = Lazy::new(|| Mutex::new(None));

#[derive(Clone)]
struct ActiveServiceSession {
    proof: OwnerSessionProof,
    supports_runtime_staging: bool,
}

fn generate_service_session_token() -> Result<String> {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes).context("failed to generate service owner session")?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn active_service_session() -> Result<OwnerSessionProof> {
    ACTIVE_SERVICE_SESSION
        .lock()
        .as_ref()
        .map(|session| session.proof.clone())
        .context("service owner session is not active")
}

pub(crate) fn active_service_supports_runtime_staging() -> bool {
    ACTIVE_SERVICE_SESSION
        .lock()
        .as_ref()
        .is_some_and(|session| session.supports_runtime_staging)
}

pub(crate) fn clear_active_service_session() {
    ACTIVE_SERVICE_SESSION.lock().take();
}

pub(crate) fn has_active_service_session() -> bool {
    ACTIVE_SERVICE_SESSION.lock().is_some()
}

async fn probe_runtime_staging_support() -> bool {
    match clash_verge_service_ipc::get_version().await {
        Ok(response) if response.code == 0 => response
            .data
            .as_ref()
            .is_some_and(clash_verge_service_ipc::ProtocolInfo::supports_runtime_staging),
        Ok(response) => {
            logging!(
                warn,
                Type::Service,
                "service protocol query returned {}: {}; config changes take the restart path",
                response.code,
                response.message
            );
            false
        }
        Err(error) => {
            logging!(
                warn,
                Type::Service,
                "failed to query the service protocol: {error:#}; config changes take the restart path"
            );
            false
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServiceBusy;

impl std::fmt::Display for ServiceBusy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("service operation already running")
    }
}

impl std::error::Error for ServiceBusy {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ElevationPending;

impl std::fmt::Display for ElevationPending {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the system authorisation dialog has not been answered yet")
    }
}

impl std::error::Error for ElevationPending {}

static ELEVATION_IN_FLIGHT: AtomicBool = AtomicBool::new(false);
static BUNDLE_REJECTION: Lazy<Mutex<Option<crate::core::runtime_bundle::UnusableBundle>>> =
    Lazy::new(|| Mutex::new(None));

pub(crate) fn bundle_rejection() -> Option<String> {
    BUNDLE_REJECTION
        .lock()
        .as_ref()
        .map(|rejection| rejection.message.clone())
}

pub(crate) fn bundle_rejection_for(config: &serde_yaml_ng::Mapping) -> Option<String> {
    let remembered = BUNDLE_REJECTION.lock().clone()?;
    let providers = crate::core::runtime_bundle::provider_fingerprint(config);
    (providers == remembered.providers).then_some(remembered.message)
}

pub(crate) async fn bundle_rejection_for_the_running_config() -> Option<String> {
    BUNDLE_REJECTION.lock().as_ref()?;
    let runtime = Config::runtime().await;
    let running = runtime.data_arc();
    bundle_rejection_for(running.config.as_ref()?)
}

fn forget_bundle_rejection() {
    BUNDLE_REJECTION.lock().take();
}

fn remember_bundle_rejection(rejection: &crate::core::runtime_bundle::UnusableBundle) {
    *BUNDLE_REJECTION.lock() = Some(rejection.clone());
}

pub fn elevation_in_flight() -> bool {
    ELEVATION_IN_FLIGHT.load(Ordering::Acquire)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceStatus {
    Ready,
    NeedsReinstall,
    InstallRequired,
    UninstallRequired,
    ReinstallRequired,
    ForceReinstallRequired,
    Unavailable(String),
}

pub struct ServiceManager {
    status: Mutex<ServiceStatus>,
    operation_running: AtomicBool,
    operation_started: Mutex<Option<Instant>>,
    operation_done: Notify,
}

#[cfg(not(target_os = "macos"))]
fn service_core_path(clash_core: &str, bin_ext: &str) -> Result<PathBuf> {
    Ok(current_exe()?.with_file_name(format!("{clash_core}{bin_ext}")))
}

pub async fn bundled_core_path() -> Result<PathBuf> {
    let verge_config = Config::verge().await;
    let clash_core = verge_config.latest_arc().get_valid_clash_core();
    drop(verge_config);
    let bin_ext = if cfg!(windows) { ".exe" } else { "" };
    service_core_path(&clash_core, bin_ext)
}

#[cfg(target_os = "macos")]
fn service_core_path(clash_core: &str, bin_ext: &str) -> Result<PathBuf> {
    let binary_name = format!("{clash_core}{bin_ext}");
    let exe_path = current_exe()?;
    let candidate = exe_path.with_file_name(&binary_name);

    if !is_macos_app_translocated(&exe_path) {
        return Ok(candidate);
    }

    if let Some(stable_path) = stable_macos_core_path_for_translocated_app(&exe_path, &binary_name) {
        logging!(
            warn,
            Type::Service,
            "macOS App Translocation detected for core path {:?}; using stable installed path {:?}",
            candidate,
            stable_path
        );
        return Ok(stable_path);
    }

    notify_translocated_core_path();
    bail!(
        "macOS App Translocation detected; refusing to start service with temporary core path {:?}",
        candidate
    )
}

#[cfg(target_os = "macos")]
fn notify_translocated_core_path() {
    crate::process::AsyncHandler::spawn(|| async {
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        crate::core::handle::Handle::notice_message(
            "set_config::error",
            clash_verge_i18n::t!("service.translocatedCorePath").to_string(),
        );
    });
}

#[cfg(target_os = "macos")]
fn is_macos_app_translocated(path: &Path) -> bool {
    path.components()
        .any(|component| component.as_os_str() == "AppTranslocation")
}

#[cfg(target_os = "macos")]
fn stable_macos_core_path_for_translocated_app(exe_path: &Path, binary_name: &str) -> Option<PathBuf> {
    let bundle_name = macos_app_bundle_name(exe_path)?;
    macos_core_path_in_install_roots(
        &bundle_name,
        binary_name,
        [Path::new("/Applications"), Path::new("/Applications/Utilities")],
    )
}

#[cfg(target_os = "macos")]
fn macos_app_bundle_name(path: &Path) -> Option<std::ffi::OsString> {
    path.ancestors().find_map(|ancestor| {
        let is_app_bundle = ancestor
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("app"));

        if is_app_bundle {
            ancestor.file_name().map(std::ffi::OsString::from)
        } else {
            None
        }
    })
}

#[cfg(target_os = "macos")]
fn macos_core_path_in_install_roots<'a>(
    bundle_name: &std::ffi::OsStr,
    binary_name: &str,
    install_roots: impl IntoIterator<Item = &'a Path>,
) -> Option<PathBuf> {
    install_roots.into_iter().find_map(|root| {
        let core_path = root
            .join(Path::new(bundle_name))
            .join("Contents")
            .join("MacOS")
            .join(binary_name);

        core_path.is_file().then_some(core_path)
    })
}

#[cfg(target_os = "macos")]
const fn macos_cleanup_translocated_desired_state_shell() -> &'static str {
    "for f in '/var/root/.local/state/clash-verge-service/desired-state.json' '/var/lib/clash-verge-service/desired-state.json'; do if [ -f \"$f\" ] && /usr/bin/grep -q AppTranslocation \"$f\"; then backup=\"$f.apptranslocation.bak\"; if [ -e \"$backup\" ]; then backup=\"$f.apptranslocation.$(/bin/date +%s).bak\"; fi; /bin/mv \"$f\" \"$backup\"; fi; done"
}

#[cfg(target_os = "macos")]
fn macos_force_stop_core_shell() -> String {
    use crate::config::IVerge;

    let mut parts: Vec<String> = IVerge::VALID_CLASH_CORES
        .iter()
        .map(|core| format!("/usr/bin/pkill -U root -x {core} 2>/dev/null || true"))
        .collect();

    if let Ok(ipc) = dirs::sidecar_ipc_path()
        && let Ok(ipc_str) = dirs::path_to_str(&ipc)
    {
        let escaped = ipc_str.replace('\'', r"'\''");
        parts.push(format!("/bin/rm -f '{escaped}' 2>/dev/null || true"));
    }

    parts.join("; ")
}

#[cfg(target_os = "macos")]
fn escape_osascript_double_quoted_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

#[cfg(target_os = "windows")]
fn uninstall_service() -> Result<()> {
    logging!(info, Type::Service, "uninstall service");

    use deelevate::{PrivilegeLevel, Token};
    use runas::Command as RunasCommand;
    use std::os::windows::process::CommandExt as _;

    let binary_path = dirs::service_path()?;
    let uninstall_path = binary_path.with_file_name("clash-verge-service-uninstall.exe");

    if !uninstall_path.exists() {
        bail!(format!("uninstaller not found: {uninstall_path:?}"));
    }

    let token = Token::with_current_process()?;
    let level = token.privilege_level()?;
    let status = match level {
        PrivilegeLevel::NotPrivileged => RunasCommand::new(uninstall_path).show(false).status()?,
        _ => StdCommand::new(uninstall_path).creation_flags(0x08000000).status()?,
    };

    if !status.success() {
        bail!(
            "failed to uninstall service with status {}",
            status.code().unwrap_or(-1)
        );
    }

    Ok(())
}

#[cfg(target_os = "windows")]
fn install_service() -> Result<()> {
    use std::process::Output;
    logging!(info, Type::Service, "install service");

    use deelevate::{PrivilegeLevel, Token};
    use runas::Command as RunasCommand;
    use std::os::windows::process::CommandExt as _;

    let binary_path = dirs::service_path()?;
    let install_path = binary_path.with_file_name("clash-verge-service-install.exe");

    if !install_path.exists() {
        bail!(format!("installer not found: {install_path:?}"));
    }

    let token = Token::with_current_process()?;
    let level = token.privilege_level()?;
    let output = match level {
        PrivilegeLevel::NotPrivileged => {
            let status = RunasCommand::new(&install_path).show(false).status()?;
            Output {
                status,
                stdout: Vec::new(),
                stderr: Vec::new(),
            }
        }
        _ => StdCommand::new(&install_path).creation_flags(0x08000000).output()?,
    };

    if let Some((code, err)) = check_output_error(&output) {
        logging!(
            error,
            Type::Service,
            "failed to install service code: {}, details: {}",
            code,
            err
        );
        bail!("failed to install service code: {}, details: {}", code, err);
    }

    Ok(())
}

#[cfg(target_os = "linux")]
const LINUX_SERVICE_BINARY: &str = "/var/lib/clash-verge-service/bin/clash-verge-service";

#[cfg(target_os = "linux")]
const LINUX_SERVICE_BIN_DIR: &str = "/var/lib/clash-verge-service/bin";

#[cfg(target_os = "linux")]
const LINUX_SERVICE_FCONTEXT: &str = "/var/lib/clash-verge-service/bin(/.*)?";

#[cfg(target_os = "linux")]
fn selinux_is_enforcing() -> bool {
    std::fs::read_to_string("/sys/fs/selinux/enforce").is_ok_and(|value| value.trim() == "1")
}

#[cfg(target_os = "linux")]
fn selinux_install_prefix() -> String {
    selinux_install_prefix_for(selinux_is_enforcing())
}

#[cfg(target_os = "linux")]
fn selinux_install_prefix_for(enforcing: bool) -> String {
    if !enforcing {
        return String::new();
    }
    let dir = shell_single_quote(LINUX_SERVICE_BIN_DIR);
    let fcontext = shell_single_quote(LINUX_SERVICE_FCONTEXT);
    format!(
        "mkdir -p {dir} >/dev/null 2>&1 || true; \
if command -v semanage >/dev/null 2>&1; then \
semanage fcontext -a -t bin_t {fcontext} >/dev/null 2>&1 || true; fi; \
if command -v chcon >/dev/null 2>&1; then chcon -R -t bin_t {dir} >/dev/null 2>&1 || true; fi; \
systemctl reset-failed clash-verge-service.service >/dev/null 2>&1 || true; "
    )
}

#[cfg(target_os = "linux")]
fn selinux_recovery_tail() -> String {
    selinux_recovery_tail_for(selinux_is_enforcing())
}

#[cfg(target_os = "linux")]
fn selinux_recovery_tail_for(enforcing: bool) -> String {
    if !enforcing {
        return String::new();
    }
    let binary = shell_single_quote(LINUX_SERVICE_BINARY);
    let fcontext = shell_single_quote(LINUX_SERVICE_FCONTEXT);
    format!(
        "; rc=$?; if [ \"$rc\" -ne 0 ] && [ -f {binary} ] && command -v chcon >/dev/null 2>&1; then \
chcon -t bin_t {binary} >/dev/null 2>&1 || true; \
if command -v semanage >/dev/null 2>&1; then \
semanage fcontext -a -t bin_t {fcontext} >/dev/null 2>&1 || true; fi; \
systemctl reset-failed clash-verge-service.service >/dev/null 2>&1 || true; \
systemctl start clash-verge-service.service >/dev/null 2>&1 || true; \
if systemctl is-active --quiet clash-verge-service.service; then rc=0; fi; fi; exit $rc"
    )
}

#[cfg(target_os = "linux")]
fn selinux_hint() -> String {
    if selinux_is_enforcing() {
        format!(" {}", clash_verge_i18n::t!("service.selinuxBlocked"))
    } else {
        String::new()
    }
}

#[cfg(target_os = "linux")]
const fn pkexec_itself_failed(code: Option<i32>) -> bool {
    matches!(code, Some(126 | 127))
}

#[cfg(target_os = "linux")]
fn pkexec_failure_hint(code: Option<i32>) -> String {
    match code {
        Some(126) => "administrator rights were not granted: the polkit prompt was cancelled or declined".to_owned(),
        Some(127) => {
            "pkexec could not run the service installer: polkit is missing an agent for this session".to_owned()
        }
        other => format!("pkexec failed with status {}", exit_status_text(other)),
    }
}

#[cfg(target_os = "linux")]
fn exit_status_text(code: Option<i32>) -> String {
    code.map_or_else(
        || "unknown (terminated by a signal)".to_owned(),
        |code| code.to_string(),
    )
}

#[cfg(target_os = "linux")]
fn uninstall_service() -> Result<()> {
    logging!(info, Type::Service, "uninstall service");

    let uninstall_path = tauri::utils::platform::current_exe()?.with_file_name("clash-verge-service-uninstall");

    if !uninstall_path.exists() {
        bail!(format!("uninstaller not found: {uninstall_path:?}"));
    }

    let status = if linux_running_as_root() {
        StdCommand::new(&uninstall_path).status()?
    } else {
        let elevator = crate::utils::help::linux_elevator()?;
        let status = StdCommand::new(&elevator)
            .arg("--disable-internal-agent")
            .arg(&uninstall_path)
            .status()?;
        if pkexec_itself_failed(status.code()) {
            bail!("{}", pkexec_failure_hint(status.code()));
        }
        status
    };
    logging!(
        info,
        Type::Service,
        "uninstall status code:{}",
        exit_status_text(status.code())
    );

    if !status.success() {
        bail!(
            "failed to uninstall service with status {}",
            exit_status_text(status.code())
        );
    }

    Ok(())
}

#[cfg(target_os = "linux")]
fn install_service() -> Result<()> {
    logging!(info, Type::Service, "install service");

    let install_path = tauri::utils::platform::current_exe()?.with_file_name("clash-verge-service-install");

    if !install_path.exists() {
        bail!(format!("installer not found: {install_path:?}"));
    }

    let script = format!(
        "{}{}{}",
        selinux_install_prefix(),
        shell_single_quote(&install_path.to_string_lossy()),
        selinux_recovery_tail()
    );

    let output = if linux_running_as_root() {
        StdCommand::new("sh").args(["-c", &script]).output()?
    } else {
        let elevator = crate::utils::help::linux_elevator()?;
        let output = StdCommand::new(&elevator)
            .args(["--disable-internal-agent", "sh", "-c", &script])
            .output()?;
        if pkexec_itself_failed(output.status.code()) {
            bail!("{}", pkexec_failure_hint(output.status.code()));
        }
        output
    };

    if let Some((code, err)) = check_output_error(&output) {
        logging!(
            error,
            Type::Service,
            "failed to install service code: {}, details: {}",
            code,
            err
        );
        bail!(
            "failed to install service code: {}, details: {}{}",
            code,
            err,
            selinux_hint()
        );
    }

    Ok(())
}

#[cfg(target_os = "linux")]
fn linux_running_as_root() -> bool {
    use crate::core::handle;
    use tauri_plugin_clash_verge_sysinfo::is_current_app_handle_admin;
    let app_handle = handle::Handle::app_handle();
    is_current_app_handle_admin(app_handle)
}

#[cfg(target_os = "macos")]
fn uninstall_service() -> Result<()> {
    logging!(info, Type::Service, "uninstall service");

    let binary_path = dirs::service_path()?;
    let uninstall_path = binary_path.with_file_name("clash-verge-service-uninstall");

    if !uninstall_path.exists() {
        bail!(format!("uninstaller not found: {uninstall_path:?}"));
    }

    let uninstall_shell: String = uninstall_path.to_string_lossy().into_owned();

    let prompt = clash_verge_i18n::t!("service.adminUninstallPrompt");
    let uninstall_quoted = shell_single_quote(&uninstall_shell);
    let shell = format!("{}; sudo {uninstall_quoted}", macos_force_stop_core_shell());
    let shell = escape_osascript_double_quoted_string(&shell);
    let command = format!(r#"do shell script "{shell}" with administrator privileges with prompt "{prompt}""#);

    let status = StdCommand::new("osascript").args(vec!["-e", &command]).status()?;

    if !status.success() {
        bail!(
            "failed to uninstall service with status {}",
            status.code().unwrap_or(-1)
        );
    }

    Ok(())
}

#[cfg(target_os = "macos")]
fn install_service() -> Result<()> {
    logging!(info, Type::Service, "install service");

    let binary_path = dirs::service_path()?;
    let install_path = binary_path.with_file_name("clash-verge-service-install");

    if !install_path.exists() {
        bail!(format!("installer not found: {install_path:?}"));
    }

    let install_shell: String = install_path.to_string_lossy().into_owned();

    let gid = tauri_plugin_clash_verge_sysinfo::current_gid();
    let prompt = clash_verge_i18n::t!("service.adminInstallPrompt");
    let install_quoted = shell_single_quote(&install_shell);
    let shell = format!(
        "{}; sudo CLASH_VERGE_SERVICE_GID={gid} {install_quoted}",
        macos_cleanup_translocated_desired_state_shell()
    );
    let shell = escape_osascript_double_quoted_string(&shell);
    let command = format!(r#"do shell script "{shell}" with administrator privileges with prompt "{prompt}""#);

    let output = StdCommand::new("osascript").args(vec!["-e", &command]).output()?;
    if let Some((code, err)) = check_output_error(&output) {
        logging!(
            error,
            Type::Service,
            "failed to install service code: {}, details: {}",
            code,
            err
        );
        bail!("failed to install service code: {}, details: {}", code, err);
    }

    Ok(())
}

fn check_output_error(output: &std::process::Output) -> Option<(i32, Cow<'_, str>)> {
    if output.status.success() {
        return None;
    }
    let code = output.status.code().unwrap_or(-1);
    Some((code, describe_failure(code, &output.stdout, &output.stderr)))
}

fn describe_failure<'a>(code: i32, stdout: &'a [u8], stderr: &'a [u8]) -> Cow<'a, str> {
    let stderr = String::from_utf8_lossy(stderr);
    if !stderr.trim().is_empty() {
        return stderr;
    }
    let stdout = String::from_utf8_lossy(stdout);
    if !stdout.trim().is_empty() {
        return stdout;
    }
    match code {
        1223 => Cow::Borrowed("the elevation prompt was dismissed"),
        740 => Cow::Borrowed("the installer needs administrator rights"),
        1060 => Cow::Borrowed("the service is not registered"),
        1056 => Cow::Borrowed("the service is already running"),
        5 => Cow::Borrowed("access denied"),
        _ => Cow::Owned(format!(
            "the installer exited with code {code} and printed nothing (it runs elevated, so its output is not ours to read)"
        )),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceRegistration {
    Missing,
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    Stopped,
    Running,
    Unknown,
}

#[cfg(target_os = "windows")]
pub fn service_registration() -> ServiceRegistration {
    use std::os::windows::process::CommandExt as _;

    const SERVICE_DOES_NOT_EXIST: i32 = 1060;
    const CREATE_NO_WINDOW: u32 = 0x08000000;

    let Ok(output) = StdCommand::new("sc.exe")
        .args(["query", "clash_verge_service"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
    else {
        return ServiceRegistration::Unknown;
    };

    match output.status.code() {
        Some(0) => {
            let state = String::from_utf8_lossy(&output.stdout).to_ascii_uppercase();
            if state.contains("RUNNING") || state.contains("START_PENDING") {
                ServiceRegistration::Running
            } else {
                ServiceRegistration::Stopped
            }
        }
        Some(SERVICE_DOES_NOT_EXIST) => ServiceRegistration::Missing,
        _ => ServiceRegistration::Unknown,
    }
}

#[cfg(target_os = "windows")]
pub fn start_registered_service() -> bool {
    use std::os::windows::process::CommandExt as _;

    const CREATE_NO_WINDOW: u32 = 0x08000000;
    const SERVICE_ALREADY_RUNNING: i32 = 1056;

    let Ok(output) = StdCommand::new("sc.exe")
        .args(["start", "clash_verge_service"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
    else {
        return false;
    };

    let code = output.status.code().unwrap_or(-1);
    if matches!(code, 0 | SERVICE_ALREADY_RUNNING) {
        return true;
    }

    logging!(
        info,
        Type::Service,
        "the registered service did not start without rights: {}",
        describe_failure(code, &output.stdout, &output.stderr)
    );
    false
}

#[cfg(not(target_os = "windows"))]
pub const fn start_registered_service() -> bool {
    false
}

#[cfg(target_os = "linux")]
pub fn service_registration() -> ServiceRegistration {
    if !Path::new("/etc/systemd/system/clash-verge-service.service").exists() {
        return ServiceRegistration::Missing;
    }
    let Ok(output) = StdCommand::new("systemctl")
        .args(["is-active", "clash-verge-service.service"])
        .output()
    else {
        return ServiceRegistration::Unknown;
    };
    match String::from_utf8_lossy(&output.stdout).trim() {
        "active" | "activating" | "reloading" | "deactivating" => ServiceRegistration::Running,
        "inactive" | "failed" => ServiceRegistration::Stopped,
        _ => ServiceRegistration::Unknown,
    }
}

#[cfg(target_os = "macos")]
pub fn service_registration() -> ServiceRegistration {
    const LABEL: &str = "io.github.clash-verge-rev.clash-verge-rev.service";

    if !Path::new("/Library/LaunchDaemons/io.github.clash-verge-rev.clash-verge-rev.service.plist").exists() {
        return ServiceRegistration::Missing;
    }
    match StdCommand::new("/bin/launchctl")
        .args(["print", &format!("system/{LABEL}")])
        .output()
    {
        Ok(output) if output.status.success() => ServiceRegistration::Running,
        _ => ServiceRegistration::Unknown,
    }
}

#[cfg(target_os = "windows")]
fn reinstall_service() -> Result<()> {
    logging!(info, Type::Service, "reinstall service");

    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use deelevate::{PrivilegeLevel, Token};
    use runas::Command as RunasCommand;
    use std::os::windows::process::CommandExt as _;

    let binary_path = dirs::service_path()?;
    let uninstall_path = binary_path.with_file_name("clash-verge-service-uninstall.exe");
    let install_path = binary_path.with_file_name("clash-verge-service-install.exe");

    if !install_path.exists() {
        bail!(format!("installer not found: {install_path:?}"));
    }

    let ps_quote = |path: &std::path::Path| format!("'{}'", path.display().to_string().replace('\'', "''"));
    let mut script = String::new();
    if uninstall_path.exists() {
        script.push_str(&format!("& {}; ", ps_quote(&uninstall_path)));
    }
    script.push_str(&format!("& {}; exit $LASTEXITCODE", ps_quote(&install_path)));
    let encoded: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let encoded = STANDARD.encode(encoded);

    let args = ["-NoProfile", "-NonInteractive", "-EncodedCommand", &encoded];
    let token = Token::with_current_process()?;
    let status = match token.privilege_level()? {
        PrivilegeLevel::NotPrivileged => RunasCommand::new("powershell.exe").args(&args).show(false).status()?,
        _ => StdCommand::new("powershell.exe")
            .args(args)
            .creation_flags(0x08000000)
            .status()?,
    };

    if !status.success() {
        bail!(
            "failed to reinstall service with status {}",
            status.code().unwrap_or(-1)
        );
    }

    Ok(())
}

#[cfg(target_os = "linux")]
fn reinstall_service() -> Result<()> {
    logging!(info, Type::Service, "reinstall service");

    let exe = tauri::utils::platform::current_exe()?;
    let uninstall_path = exe.with_file_name("clash-verge-service-uninstall");
    let install_path = exe.with_file_name("clash-verge-service-install");

    if !install_path.exists() {
        bail!(format!("installer not found: {install_path:?}"));
    }

    let mut script = String::new();
    if uninstall_path.exists() {
        script.push_str(&shell_single_quote(&uninstall_path.to_string_lossy()));
        script.push_str("; ");
    }
    script.push_str(&selinux_install_prefix());
    script.push_str(&shell_single_quote(&install_path.to_string_lossy()));
    script.push_str(&selinux_recovery_tail());

    let status = if linux_running_as_root() {
        StdCommand::new("sh").args(["-c", &script]).status()?
    } else {
        let elevator = crate::utils::help::linux_elevator()?;
        let status = StdCommand::new(&elevator)
            .args(["--disable-internal-agent", "sh", "-c", &script])
            .status()?;
        if pkexec_itself_failed(status.code()) {
            bail!("{}", pkexec_failure_hint(status.code()));
        }
        status
    };

    if !status.success() {
        bail!(
            "failed to reinstall service with status {}{}",
            exit_status_text(status.code()),
            selinux_hint()
        );
    }

    Ok(())
}

#[cfg(target_os = "macos")]
fn reinstall_service() -> Result<()> {
    logging!(info, Type::Service, "reinstall service");

    let binary_path = dirs::service_path()?;
    let uninstall_path = binary_path.with_file_name("clash-verge-service-uninstall");
    let install_path = binary_path.with_file_name("clash-verge-service-install");

    if !install_path.exists() {
        bail!(format!("installer not found: {install_path:?}"));
    }

    let gid = tauri_plugin_clash_verge_sysinfo::current_gid();
    let prompt = clash_verge_i18n::t!("service.adminInstallPrompt");

    let mut shell = macos_force_stop_core_shell();
    if uninstall_path.exists() {
        shell.push_str("; sudo ");
        shell.push_str(&shell_single_quote(&uninstall_path.to_string_lossy()));
    }
    shell.push_str("; ");
    shell.push_str(macos_cleanup_translocated_desired_state_shell());
    shell.push_str("; sudo CLASH_VERGE_SERVICE_GID=");
    shell.push_str(&gid.to_string());
    shell.push(' ');
    shell.push_str(&shell_single_quote(&install_path.to_string_lossy()));

    let shell = escape_osascript_double_quoted_string(&shell);
    let command = format!(r#"do shell script "{shell}" with administrator privileges with prompt "{prompt}""#);

    let output = StdCommand::new("osascript").args(vec!["-e", &command]).output()?;
    if let Some((code, err)) = check_output_error(&output) {
        logging!(
            error,
            Type::Service,
            "failed to reinstall service code: {}, details: {}",
            code,
            err
        );
        bail!("failed to reinstall service code: {}, details: {}", code, err);
    }

    Ok(())
}

fn force_reinstall_service() -> Result<()> {
    logging!(
        info,
        Type::Service,
        "Пользователь запросил принудительную переустановку службы"
    );
    reinstall_service().map_err(|err| {
        logging!(
            error,
            Type::Service,
            "Принудительная переустановка службы не удалась: {}",
            err
        );
        err
    })
}

async fn collect_service_runtime_bundle(config_file: &Path) -> Result<clash_verge_service_ipc::RuntimeBundle> {
    let verge_config = Config::verge().await;
    let clash_core = verge_config.latest_arc().get_valid_clash_core();
    drop(verge_config);

    let bin_ext = if cfg!(windows) { ".exe" } else { "" };
    if crate::core::core_updater::managed_core_binary().await.is_some() {
        logging!(
            warn,
            Type::Service,
            "managed core is enabled but service mode runs the bundled core (privilege boundary)"
        );
    }
    let bin_path = service_core_path(&clash_core, bin_ext)?;
    crate::core::core_integrity::ensure_elevated_binary_is_known(&bin_path).await?;
    match collect_runtime_bundle(config_file, &bin_path).await {
        Ok(bundle) => {
            forget_bundle_rejection();
            Ok(bundle)
        }
        Err(error) => {
            if let Some(unusable) = error.downcast_ref::<crate::core::runtime_bundle::UnusableBundle>() {
                logging!(
                    warn,
                    Type::Service,
                    "this configuration cannot be handed to the service: {unusable}"
                );
                remember_bundle_rejection(unusable);
            }
            Err(error)
        }
    }
}

pub(crate) enum StageRequest {
    Refused { code: u16, message: CompactString },
    Unbuildable(String),
    Answered(StageRuntimeOutcome),
}

impl StageRequest {
    pub(crate) const fn is_about_the_bundle(code: u16) -> bool {
        code == ServiceErrorCode::InvalidRuntimeAsset as u16 || code == ServiceErrorCode::InvalidInstallLocation as u16
    }
}

pub(crate) async fn stage_runtime_by_service(config_file: &Path) -> Result<StageRequest> {
    let session = active_service_session()?;
    let credentials = current_owner_credentials()?;
    let runtime = match collect_service_runtime_bundle(config_file).await {
        Ok(runtime) => runtime,
        Err(error) => {
            return match error.downcast_ref::<crate::core::runtime_bundle::UnusableBundle>() {
                Some(unusable) => Ok(StageRequest::Unbuildable(unusable.message.clone())),
                None => Err(error),
            };
        }
    };

    let response = clash_verge_service_ipc::stage_runtime(&credentials, &session, &runtime)
        .await
        .context("Не удалось подключиться к Clash Verge Service")?;
    if response.code > 0 {
        return Ok(StageRequest::Refused {
            code: response.code,
            message: response.message.into(),
        });
    }
    response
        .data
        .map(StageRequest::Answered)
        .context("служба не вернула результат подмены рантайма")
}

pub(super) async fn start_with_existing_service(config_file: &Path) -> Result<()> {
    logging!(info, Type::Service, "Попытка запуска ядра через существующую службу");
    clear_active_service_session();

    let credentials = current_owner_credentials()?;
    let runtime = collect_service_runtime_bundle(config_file).await?;
    let proposed_session_token = generate_service_session_token()?;
    let request = StartClashRequest {
        runtime,
        proposed_session_token: proposed_session_token.clone(),
        macos_proxy: None,
    };

    let response = clash_verge_service_ipc::start_clash(&credentials, &request)
        .await
        .context("Не удалось подключиться к Clash Verge Service")?;

    if response.code > 0 {
        let err_msg = response.message;
        logging!(error, Type::Service, "Не удалось запустить ядро: {}", err_msg);
        bail!(err_msg);
    }

    let result = response.data.context("служба не вернула сведения о сессии")?;
    let supports_runtime_staging = probe_runtime_staging_support().await;
    *ACTIVE_SERVICE_SESSION.lock() = Some(ActiveServiceSession {
        proof: OwnerSessionProof {
            generation: result.session.generation,
            token: proposed_session_token,
        },
        supports_runtime_staging,
    });

    logging!(info, Type::Service, "Служба успешно запустила ядро");
    Ok(())
}

pub(super) async fn run_core_by_service(config_file: &Path) -> Result<()> {
    logging!(info, Type::Service, "Попытка запуска ядра через службу");

    SERVICE_MANAGER.refresh().await?;

    logging!(
        info,
        Type::Service,
        "Служба уже запущена и версия совпадает, используем напрямую"
    );
    start_with_existing_service(config_file).await
}

pub(super) async fn get_clash_logs_by_service() -> Result<Vec<CompactString>> {
    logging!(debug, Type::Service, "Получение логов Clash в режиме службы");

    let credentials = current_owner_credentials()?;
    let response = clash_verge_service_ipc::get_clash_logs(&credentials)
        .await
        .context("Не удалось подключиться к Clash Verge Service")?;

    if response.code > 0 {
        let err_msg = response.message;
        logging!(
            error,
            Type::Service,
            "Не удалось получить логи Clash в режиме службы: {}",
            err_msg
        );
        bail!(err_msg);
    }

    logging!(debug, Type::Service, "Логи Clash в режиме службы успешно получены");
    Ok(response.data.unwrap_or_default())
}

pub(super) async fn service_status() -> Result<ServiceStatusSnapshot> {
    let credentials = current_owner_credentials()?;
    let response = clash_verge_service_ipc::get_status(&credentials)
        .await
        .context("Не удалось подключиться к Clash Verge Service")?;
    if response.code > 0 {
        bail!(response.message);
    }
    response.data.context("служба не вернула своё состояние")
}

pub(super) async fn stop_core_by_service() -> Result<()> {
    logging!(info, Type::Service, "Остановка ядра через службу (IPC)");

    let credentials = current_owner_credentials()?;
    let session = active_service_session()?;
    let response = clash_verge_service_ipc::stop_clash(&credentials, &session)
        .await
        .context("Не удалось подключиться к Clash Verge Service")?;

    if response.code > 0 {
        if response.code == ServiceErrorCode::NotActive as u16
            || response.code == ServiceErrorCode::StaleOwnerSession as u16
        {
            logging!(
                warn,
                Type::Service,
                "the running core is no longer ours ({}); nothing left to stop",
                response.message
            );
            clear_active_service_session();
            return Ok(());
        }
        let err_msg = response.message;
        logging!(error, Type::Service, "Не удалось остановить ядро: {}", err_msg);
        bail!(err_msg);
    }

    clear_active_service_session();
    logging!(info, Type::Service, "Служба успешно остановила ядро");
    Ok(())
}

pub(crate) async fn update_writer_by_service(writer: &WriterConfig) -> Result<()> {
    let credentials = current_owner_credentials()?;
    let session = active_service_session()?;
    let response = clash_verge_service_ipc::update_writer(&credentials, &session, writer)
        .await
        .context("Не удалось подключиться к Clash Verge Service")?;
    if response.code > 0 {
        bail!(response.message);
    }
    Ok(())
}

const WINDOWS_PIPE_BUSY: i32 = 231;

fn ipc_path_busy(error: &std::io::Error) -> bool {
    cfg!(windows) && error.raw_os_error() == Some(WINDOWS_PIPE_BUSY)
}

pub async fn is_service_available() -> Result<()> {
    match Path::metadata(clash_verge_service_ipc::IPC_PATH.as_ref()) {
        Ok(_) => {}
        Err(e) if ipc_path_busy(&e) => {}
        Err(e) => {
            let verge = Config::verge().await;
            let verge_last = verge.latest_arc();
            let is_enable = verge_last.enable_tun_mode.unwrap_or(false);
            if is_enable {
                logging!(warn, Type::Service, "Some issue with service IPC Path: {}", e);
            }
            return Err(e.into());
        }
    }
    clash_verge_service_ipc::connect().await?;
    Ok(())
}

async fn wait_for_service_ipc(manager: &ServiceManager) -> Result<()> {
    let config = ServiceManager::config();

    let backoff = ConstantBuilder::default()
        .with_delay(config.retry_delay)
        .with_max_times(config.max_retries);

    let result = (|| async {
        if !is_service_ipc_path_exists() {
            bail!("IPC path not ready");
        }
        clash_verge_service_ipc::connect().await.map(drop)
    })
    .retry(backoff)
    .await;

    if result.is_ok() {
        manager.set_status(ServiceStatus::Ready);
    } else {
        manager.set_status(ServiceStatus::Unavailable("Waiting for service to be available".into()));
    }

    result
}

pub fn is_service_ipc_path_exists() -> bool {
    match Path::metadata(clash_verge_service_ipc::IPC_PATH.as_ref()) {
        Ok(_) => true,
        Err(e) => ipc_path_busy(&e),
    }
}

impl ServiceManager {
    pub const fn config() -> clash_verge_service_ipc::IpcConfig {
        clash_verge_service_ipc::IpcConfig {
            default_timeout: Duration::from_millis(1000),
            retry_delay: Duration::from_millis(500),
            max_retries: 20,
        }
    }

    pub async fn init(&self) -> Result<()> {
        if let Err(e) = clash_verge_service_ipc::connect().await {
            self.set_status(ServiceStatus::Unavailable(format!("Ошибка подключения к службе: {e}")));
            return Err(e);
        }
        Ok(())
    }

    pub async fn current(&self) -> ServiceStatus {
        loop {
            let notified = self.operation_done.notified();
            if !self.operation_running.load(Ordering::Acquire) {
                let status = self.status.lock().clone();
                if !self.operation_running.load(Ordering::Acquire) {
                    return status;
                }
            }
            let started = *self.operation_started.lock();
            let waited = started.map_or(Duration::ZERO, |start| start.elapsed());
            let left = timing::SERVICE_STATUS_WAIT.saturating_sub(waited);
            if tokio::time::timeout(left, notified).await.is_err() {
                if !left.is_zero() {
                    logging!(
                        warn,
                        Type::Service,
                        "a service operation is still running after {:?}; reporting the last known status",
                        timing::SERVICE_STATUS_WAIT
                    );
                }
                return self.status.lock().clone();
            }
        }
    }

    fn set_status(&self, status: ServiceStatus) {
        *self.status.lock() = status;
        crate::feat::tun::forget_capability();
    }

    async fn run_operation(&self, operation: impl Future<Output = Result<()>>) -> Result<()> {
        {
            if self.operation_running.swap(true, Ordering::AcqRel) {
                return Err(ServiceBusy.into());
            }
            *self.operation_started.lock() = Some(Instant::now());
            defer! {
                *self.operation_started.lock() = None;
                self.operation_running.store(false, Ordering::Release);
                self.operation_done.notify_waiters();
            }

            operation.await?;
        }

        if let Err(e) = Tray::global().update_menu().await {
            logging!(warn, Type::Tray, "tray menu refresh failed after a service operation: {e}");
        }
        Ok(())
    }

    pub async fn refresh(&self) -> Result<()> {
        self.run_operation(async {
            if let Err(e) = is_service_available().await {
                let reason = format!("service IPC is not answering: {e}");
                self.set_status(ServiceStatus::Unavailable(reason.clone()));
                bail!(reason);
            }
            if clash_verge_service_ipc::is_reinstall_service_needed().await {
                self.set_status(ServiceStatus::NeedsReinstall);
                if crate::feat::tun::desired().await && !REINSTALL_NOTICED.swap(true, Ordering::AcqRel) {
                    logging!(
                        warn,
                        Type::Service,
                        "service version mismatch; repair is up to the user"
                    );
                    Handle::notice_message("service::needs_repair", "");
                }
                bail!("service version mismatch");
            }
            self.set_status(ServiceStatus::Ready);
            Ok(())
        })
        .await
    }

    pub async fn handle_service_status(&self, status: ServiceStatus) -> Result<()> {
        self.run_operation(self.apply_service_status(status)).await
    }

    async fn install_service_once(&self) -> Result<()> {
        if crate::feat::tun::needs_repair().await {
            logging!(
                info,
                Type::Service,
                "Зарегистрирована служба чужой версии, ставим переустановкой — один запрос прав"
            );
            self.set_status(ServiceStatus::NeedsReinstall);
            run_service_command(reinstall_service, "reinstall service").await?;
            return wait_for_service_ipc(self).await;
        }

        logging!(
            info,
            Type::Service,
            "Требуется установка службы, запуск процесса установки"
        );
        run_service_command(install_service, "install service").await?;
        wait_for_service_ipc(self).await?;

        if clash_verge_service_ipc::is_reinstall_service_needed().await {
            logging!(
                warn,
                Type::Service,
                "Служба встала, но версия не совпала; ремонт — за пользователем"
            );
            self.set_status(ServiceStatus::NeedsReinstall);
            if !REINSTALL_NOTICED.swap(true, Ordering::AcqRel) {
                Handle::notice_message("service::needs_repair", "");
            }
            bail!("service version mismatch after install");
        }

        Ok(())
    }

    async fn apply_service_status(&self, status: ServiceStatus) -> Result<()> {
        self.set_status(status.clone());
        match status {
            ServiceStatus::Ready => logging!(info, Type::Service, "Служба готова, запуск напрямую"),
            ServiceStatus::NeedsReinstall | ServiceStatus::ReinstallRequired => {
                REINSTALL_NOTICED.store(false, Ordering::Release);
                logging!(
                    info,
                    Type::Service,
                    "Требуется переустановка службы, запуск процесса переустановки"
                );
                run_service_command(reinstall_service, "reinstall service").await?;
                wait_for_service_ipc(self).await?;
            }
            ServiceStatus::ForceReinstallRequired => {
                logging!(
                    info,
                    Type::Service,
                    "Требуется принудительная переустановка службы, запуск процесса"
                );
                run_service_command(force_reinstall_service, "force reinstall service").await?;
                wait_for_service_ipc(self).await?;
            }
            ServiceStatus::InstallRequired => {
                REINSTALL_NOTICED.store(false, Ordering::Release);
                self.install_service_once().await?;
            }
            ServiceStatus::UninstallRequired => {
                logging!(
                    info,
                    Type::Service,
                    "Требуется удаление службы, запуск процесса удаления"
                );
                run_service_command(uninstall_service, "uninstall service").await?;
                self.set_status(ServiceStatus::Unavailable("Service Uninstalled".into()));
            }
            ServiceStatus::Unavailable(reason) => {
                logging!(
                    info,
                    Type::Service,
                    "Служба недоступна: {}, будет использован режим Sidecar",
                    reason
                );
                bail!("Служба недоступна: {}", reason);
            }
        }

        Ok(())
    }
}

async fn run_service_command(
    operation: impl FnOnce() -> Result<()> + Send + 'static,
    label: &'static str,
) -> Result<()> {
    if ELEVATION_IN_FLIGHT.swap(true, Ordering::AcqRel) {
        logging!(
            info,
            Type::Service,
            "{} skipped: an authorisation dialog is already open",
            label
        );
        return Err(ElevationPending.into());
    }
    let task = tokio::task::spawn_blocking(move || {
        defer! {
            ELEVATION_IN_FLIGHT.store(false, Ordering::Release);
        }
        operation()
    });

    match tokio::time::timeout(timing::SERVICE_ELEVATION_WAIT, task).await {
        Ok(Ok(result)) => result.with_context(|| format!("{label} failed")),
        Ok(Err(join_error)) => Err(anyhow::Error::new(join_error).context(format!("{label} failed"))),
        Err(_) => {
            logging!(
                warn,
                Type::Service,
                "{} is still waiting for the authorisation dialog after {:?}; releasing the service manager",
                label,
                timing::SERVICE_ELEVATION_WAIT
            );
            Err(ElevationPending.into())
        }
    }
}

static REINSTALL_NOTICED: AtomicBool = AtomicBool::new(false);

pub static SERVICE_MANAGER: Lazy<ServiceManager> = Lazy::new(|| ServiceManager {
    status: Mutex::new(ServiceStatus::Unavailable("Need Checks".into())),
    operation_running: AtomicBool::new(false),
    operation_started: Mutex::new(None),
    operation_done: Notify::new(),
});

#[cfg(test)]
mod ipc_probe_tests {
    use super::{WINDOWS_PIPE_BUSY, ipc_path_busy};

    #[test]
    fn a_busy_pipe_means_the_service_is_alive() {
        let busy = std::io::Error::from_raw_os_error(WINDOWS_PIPE_BUSY);
        assert_eq!(ipc_path_busy(&busy), cfg!(windows));
    }

    #[test]
    fn a_missing_path_is_never_read_as_busy() {
        let missing = std::io::Error::from(std::io::ErrorKind::NotFound);
        assert!(!ipc_path_busy(&missing));

        let denied = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
        assert!(!ipc_path_busy(&denied));
    }
}

#[cfg(test)]
mod failure_tests {
    use super::describe_failure;

    #[test]
    fn a_silent_installer_failure_is_explained_by_its_exit_code() {
        assert_eq!(describe_failure(1223, b"", b""), "the elevation prompt was dismissed");
        assert_eq!(
            describe_failure(740, b"", b""),
            "the installer needs administrator rights"
        );
        assert_eq!(describe_failure(1060, b"", b""), "the service is not registered");

        let unknown = describe_failure(42, b"", b"");
        assert!(unknown.contains("42"), "{unknown}");
        assert!(unknown.contains("elevated"), "{unknown}");

        assert_eq!(describe_failure(1223, b"", b"real stderr"), "real stderr");
        assert_eq!(describe_failure(1223, b"real stdout", b"   "), "real stdout");
    }
}

#[cfg(all(test, target_os = "linux"))]
mod selinux_tests {
    use super::{
        LINUX_SERVICE_BIN_DIR, LINUX_SERVICE_BINARY, pkexec_itself_failed, selinux_install_prefix_for,
        selinux_recovery_tail_for,
    };

    #[test]
    fn without_selinux_the_script_gets_nothing() {
        assert_eq!(selinux_recovery_tail_for(false), "");
        assert_eq!(selinux_install_prefix_for(false), "");
    }

    #[test]
    fn the_label_is_set_before_the_installer_runs() {
        let prefix = selinux_install_prefix_for(true);

        assert!(
            prefix.contains("chcon -R -t bin_t"),
            "каталог размечается заранее, вместе с уже лежащим файлом: {prefix}"
        );
        assert!(
            prefix.contains(LINUX_SERVICE_BIN_DIR),
            "правится каталог службы, а не что-то ещё: {prefix}"
        );
        assert!(
            prefix.contains("systemctl reset-failed"),
            "лимит частоты запусков снимается до установки: {prefix}"
        );
        assert!(
            !prefix.contains("exit"),
            "голова не имеет права оборвать установку: {prefix}"
        );
        assert!(
            prefix.trim_end().ends_with(';'),
            "за головой сразу идёт установщик: {prefix}"
        );
    }

    #[test]
    fn nothing_in_the_prefix_may_fail_the_installation() {
        let prefix = selinux_install_prefix_for(true);

        for command in prefix.split(';').map(str::trim).filter(|part| !part.is_empty()) {
            assert!(
                command.ends_with("|| true") || command.starts_with("if ") || command == "fi",
                "команда головы обязана прощать себе неудачу: {command}"
            );
        }
    }

    #[test]
    fn the_recovery_runs_only_after_a_failed_installer() {
        let tail = selinux_recovery_tail_for(true);

        assert!(tail.contains("rc=$?"), "код установщика запоминается: {tail}");
        assert!(
            tail.contains("[ \"$rc\" -ne 0 ]"),
            "починка не трогает удачную установку: {tail}"
        );
        assert!(tail.contains("chcon -t bin_t"), "метка меняется на исполняемую: {tail}");
        assert!(
            tail.contains(LINUX_SERVICE_BINARY),
            "правится именно бинарь службы: {tail}"
        );
        assert!(
            tail.contains("systemctl is-active --quiet clash-verge-service.service"),
            "успех подтверждается фактом, а не молчанием chcon: {tail}"
        );
        assert!(
            tail.trim_end().ends_with("exit $rc"),
            "наружу уходит код установщика, а не последней команды: {tail}"
        );
    }

    #[test]
    fn only_pkexec_own_failures_are_reported_as_such() {
        assert!(pkexec_itself_failed(Some(126)));
        assert!(pkexec_itself_failed(Some(127)));
        assert!(!pkexec_itself_failed(None));
        assert!(!pkexec_itself_failed(Some(1)));
        assert!(!pkexec_itself_failed(Some(2)));
        assert!(!pkexec_itself_failed(Some(0)));
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    use std::fs;

    fn test_dir(name: &str) -> std::io::Result<PathBuf> {
        let path = std::env::temp_dir().join(format!("clash-verge-service-path-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path)?;
        Ok(path)
    }

    #[test]
    fn detects_app_translocation_paths() {
        let path = Path::new(
            "/private/var/folders/example/T/AppTranslocation/123/d/Clash Verge.app/Contents/MacOS/Clash Verge",
        );

        assert!(is_macos_app_translocated(path));
    }

    #[test]
    fn extracts_app_bundle_name_from_executable_path() {
        let path = Path::new("/Applications/Clash Verge.app/Contents/MacOS/Clash Verge");

        assert_eq!(
            macos_app_bundle_name(path).as_deref(),
            Some(std::ffi::OsStr::new("Clash Verge.app"))
        );
    }

    #[test]
    fn resolves_existing_core_path_from_install_roots() -> std::io::Result<()> {
        let root = test_dir("resolve-existing-core-path")?;
        let core_dir = root.join("Clash Verge.app").join("Contents").join("MacOS");
        let core_path = core_dir.join("verge-mihomo");

        fs::create_dir_all(&core_dir)?;
        fs::write(&core_path, b"")?;

        let resolved = macos_core_path_in_install_roots(
            std::ffi::OsStr::new("Clash Verge.app"),
            "verge-mihomo",
            [root.as_path()],
        );

        assert_eq!(resolved, Some(core_path));

        fs::remove_dir_all(root)?;
        Ok(())
    }
}
