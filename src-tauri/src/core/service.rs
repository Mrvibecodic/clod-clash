use crate::{
    config::{Config, IClashTemp},
    core::{handle::Handle, logger::Logger, tray::Tray},
    utils::dirs,
};
use anyhow::{Context as _, Result, bail};
use backon::{ConstantBuilder, Retryable as _};
use clash_verge_logging::{Type, logging};
use clash_verge_service_ipc::CoreConfig;
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
    time::Duration,
};
use tokio::sync::Notify;

/// Операция со службой уже идёт — это НЕ провал настройки.
///
/// clod: разница принципиальная. Ожидание службы и хэндофф зовут `refresh()` в
/// фоне, и наткнуться на занятый менеджер легко; если считать это провалом,
/// автонастройка запишет «на этой версии уже пробовали» и выключит себя, ни
/// разу не показав пользователю запрос прав.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServiceBusy;

impl std::fmt::Display for ServiceBusy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("service operation already running")
    }
}

impl std::error::Error for ServiceBusy {}

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
    operation_done: Notify,
}

#[cfg(not(target_os = "macos"))]
fn service_core_path(clash_core: &str, bin_ext: &str) -> Result<PathBuf> {
    Ok(current_exe()?.with_file_name(format!("{clash_core}{bin_ext}")))
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

    // Даём пользователю уведомление с действием, затем bail и не запускаем службу —
    // не даём стартовать ядру с временного пути.
    notify_translocated_core_path();
    bail!(
        "macOS App Translocation detected; refusing to start service with temporary core path {:?}",
        candidate
    )
}

/// Отправляет пользователю уведомление о translocation. Отправка **с задержкой**: во
/// время запуска app автоматически пытается запустить core, и в этот момент слушатель
/// `verge://notice-message` на фронтенде (регистрируется только после монтирования React
/// layout) может быть ещё не готов, а у backend emit нет очереди повторной отправки —
/// немедленная отправка потеряется. Отправка после монтирования фронтенда покрывает и
/// случай "автозапуск core при старте не удался", и ручной запуск (небольшая задержка
/// уведомления об ошибке приемлема). Переиспользуем фронтенд-обработчик `set_config::error`
/// для показа этого сообщения.
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

/// Перед удалением службы от root очищаем остатки core и IPC-сокет.
#[cfg(target_os = "macos")]
fn macos_force_stop_core_shell() -> String {
    use crate::config::IVerge;

    // Очищаем только ядра службы, принадлежащие root.
    let mut parts: Vec<String> = IVerge::VALID_CLASH_CORES
        .iter()
        .map(|core| format!("/usr/bin/pkill -U root -x {core} 2>/dev/null || true"))
        .collect();

    if let Ok(ipc) = dirs::ipc_path()
        && let Ok(ipc_str) = dirs::path_to_str(&ipc)
    {
        // Экранируем одинарные кавычки, чтобы не сломать shell-параметр.
        let escaped = ipc_str.replace('\'', r"'\''");
        parts.push(format!("/bin/rm -f '{escaped}' 2>/dev/null || true"));
    }

    parts.join("; ")
}

#[cfg(target_os = "macos")]
fn escape_osascript_double_quoted_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(target_os = "macos")]
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
        _ => {
            // StdCommand returns Output directly
            StdCommand::new(&install_path).creation_flags(0x08000000).output()?
        }
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
fn uninstall_service() -> Result<()> {
    logging!(info, Type::Service, "uninstall service");

    let uninstall_path = tauri::utils::platform::current_exe()?.with_file_name("clash-verge-service-uninstall");

    if !uninstall_path.exists() {
        bail!(format!("uninstaller not found: {uninstall_path:?}"));
    }

    let elevator = crate::utils::help::linux_elevator();
    let status = if linux_running_as_root() {
        StdCommand::new(&uninstall_path).status()?
    } else {
        let result = StdCommand::new(&elevator).arg(&uninstall_path).status()?;

        // Если pkexec не сработал, откатываемся на sudo
        if !result.success() && elevator.contains("pkexec") {
            logging!(
                warn,
                Type::Service,
                "pkexec failed with code {}, falling back to sudo",
                result.code().unwrap_or(-1)
            );
            StdCommand::new("sudo").arg(&uninstall_path).status()?
        } else {
            result
        }
    };
    logging!(
        info,
        Type::Service,
        "uninstall status code:{}",
        status.code().unwrap_or(-1)
    );

    if !status.success() {
        bail!(
            "failed to uninstall service with status {}",
            status.code().unwrap_or(-1)
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

    let elevator = crate::utils::help::linux_elevator();
    let output = if linux_running_as_root() {
        StdCommand::new(&install_path).output()?
    } else {
        let result = StdCommand::new(&elevator).arg(&install_path).output()?;

        // Если pkexec не сработал, откатываемся на sudo
        if !result.status.success() && elevator.contains("pkexec") {
            logging!(
                warn,
                Type::Service,
                "pkexec failed with code {}, falling back to sudo",
                result.status.code().unwrap_or(-1)
            );
            StdCommand::new("sudo").arg(&install_path).output()?
        } else {
            result
        }
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

    // clash_verge_i18n::sync_locale(Config::verge().await.latest_arc().language.as_deref());

    let prompt = clash_verge_i18n::t!("service.adminUninstallPrompt");
    // Сначала очищаем остатки службы, затем запускаем деинсталлятор.
    let uninstall_quoted = shell_single_quote(&uninstall_shell);
    let shell = format!("{}; sudo {uninstall_quoted}", macos_force_stop_core_shell());
    let shell = escape_osascript_double_quoted_string(&shell);
    let command = format!(r#"do shell script "{shell}" with administrator privileges with prompt "{prompt}""#);

    // logging!(debug, Type::Service, "uninstall command: {}", command);

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

    // clash_verge_i18n::sync_locale(Config::verge().await.latest_arc().language.as_deref());

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
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stderr.is_empty() {
        return Some((code, stderr));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !stdout.is_empty() {
        return Some((code, stdout));
    }
    Some((code, Cow::Borrowed("Unknown error")))
}

/// clod:tun-ready — что о службе знает система, независимо от того, отвечает
/// ли служба по IPC.
///
/// Без этого «служба не отвечает» читалось как «службы нет», и единственным
/// ответом была установка с запросом прав — даже когда служба стоит и просто
/// ещё не подняла канал. Опрос выполняется БЕЗ повышения прав: обычному
/// пользователю разрешено читать состояние служб.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceRegistration {
    /// Служба не зарегистрирована — её надо ставить.
    Missing,
    /// Зарегистрирована, но не работает — её надо запустить.
    ///
    /// На macOS этого состояния не бывает: чтение системного домена launchd
    /// обычному пользователю может быть запрещено, и «остановлена» там было бы
    /// догадкой, за которую платят лишним запросом прав.
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    Stopped,
    /// Зарегистрирована и работает.
    Running,
    /// Спросить не удалось — решаем по IPC, как раньше.
    Unknown,
}

#[cfg(target_os = "windows")]
pub fn service_registration() -> ServiceRegistration {
    use std::os::windows::process::CommandExt as _;

    // ERROR_SERVICE_DOES_NOT_EXIST: `sc query` отдаёт его кодом возврата.
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
            // Подписи в выводе локализованы, а сами состояния — нет.
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
    // Судим по слову, а не по коду возврата: у стартующего юнита `is-active`
    // печатает `activating` и выходит с ненулевым кодом — а это ровно тот
    // случай, ради которого мы и ждём, вместо того чтобы просить прав.
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
        // Ненулевой код — это не только «не работает»: чтение системного домена
        // launchd обычному пользователю может быть просто не разрешено. Врать
        // «остановлена» нельзя — на этом слове мы просим права.
        _ => ServiceRegistration::Unknown,
    }
}

fn reinstall_service() -> Result<()> {
    logging!(info, Type::Service, "reinstall service");

    // Сначала удаляем службу
    if let Err(err) = uninstall_service() {
        logging!(warn, Type::Service, "failed to uninstall service: {}", err);
    }

    // Затем устанавливаем службу
    match install_service() {
        Ok(_) => Ok(()),
        Err(err) => {
            bail!(format!("failed to install service: {err}"))
        }
    }
}

/// Принудительная переустановка службы (кнопка исправления в UI)
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

/// Пытаемся запустить core через службу
pub(super) async fn start_with_existing_service(config_file: &PathBuf) -> Result<()> {
    logging!(info, Type::Service, "Попытка запуска ядра через существующую службу");

    let verge_config = Config::verge().await;
    let clash_core = verge_config.latest_arc().get_valid_clash_core();
    drop(verge_config);

    let bin_ext = if cfg!(windows) { ".exe" } else { "" };
    // clod:F5 — the managed core is deliberately NOT handed to the service:
    // the service runs elevated, and `{app_home}/cores` is writable by the
    // unprivileged user, so pointing the service there would turn "can write
    // app-data" into "code runs as SYSTEM/root". Service mode always runs the
    // admin-owned bundled core; the managed core is a sidecar-mode feature.
    if crate::core::core_updater::managed_core_binary().await.is_some() {
        logging!(
            warn,
            Type::Service,
            "managed core is enabled but service mode runs the bundled core (privilege boundary)"
        );
    }
    let bin_path = service_core_path(&clash_core, bin_ext)?;

    let payload = clash_verge_service_ipc::ClashConfig {
        core_config: CoreConfig {
            config_path: dirs::path_to_str(config_file)?.into(),
            core_path: dirs::path_to_str(&bin_path)?.into(),
            core_ipc_path: IClashTemp::guard_external_controller_ipc(),
            config_dir: dirs::path_to_str(&dirs::app_home_dir()?)?.into(),
        },
        log_config: Logger::global().service_writer_config()?,
    };

    let response = clash_verge_service_ipc::start_clash(&payload)
        .await
        .context("Не удалось подключиться к Clash Verge Service")?;

    if response.code > 0 {
        let err_msg = response.message;
        logging!(error, Type::Service, "Не удалось запустить ядро: {}", err_msg);
        bail!(err_msg);
    }

    logging!(info, Type::Service, "Служба успешно запустила ядро");
    Ok(())
}

// Запуск core через службу
pub(super) async fn run_core_by_service(config_file: &PathBuf) -> Result<()> {
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
    logging!(info, Type::Service, "Получение логов Clash в режиме службы");

    let response = clash_verge_service_ipc::get_clash_logs()
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

    logging!(info, Type::Service, "Логи Clash в режиме службы успешно получены");
    Ok(response.data.unwrap_or_default())
}

/// Остановка core через службу
pub(super) async fn stop_core_by_service() -> Result<()> {
    logging!(info, Type::Service, "Остановка ядра через службу (IPC)");

    let response = clash_verge_service_ipc::stop_clash()
        .await
        .context("Не удалось подключиться к Clash Verge Service")?;

    if response.code > 0 {
        let err_msg = response.message;
        logging!(error, Type::Service, "Не удалось остановить ядро: {}", err_msg);
        bail!(err_msg);
    }

    logging!(info, Type::Service, "Служба успешно остановила ядро");
    Ok(())
}

/// Проверяем, запущена ли служба
pub async fn is_service_available() -> Result<()> {
    if let Err(e) = Path::metadata(clash_verge_service_ipc::IPC_PATH.as_ref()) {
        let verge = Config::verge().await;
        let verge_last = verge.latest_arc();
        let is_enable = verge_last.enable_tun_mode.unwrap_or(false);
        if is_enable {
            logging!(warn, Type::Service, "Some issue with service IPC Path: {}", e);
        }
        return Err(e.into());
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
    Path::new(clash_verge_service_ipc::IPC_PATH).exists()
}

impl ServiceManager {
    pub const fn config() -> clash_verge_service_ipc::IpcConfig {
        clash_verge_service_ipc::IpcConfig {
            default_timeout: Duration::from_millis(150),
            retry_delay: Duration::from_millis(250),
            max_retries: 20,
        }
    }

    pub async fn init(&self) -> Result<()> {
        if let Err(e) = clash_verge_service_ipc::connect().await {
            self.set_status(ServiceStatus::Unavailable(
                "Ошибка подключения к службе: {e}".to_string(),
            ));
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
            notified.await;
        }
    }

    fn set_status(&self, status: ServiceStatus) {
        *self.status.lock() = status;
    }

    async fn run_operation(&self, operation: impl Future<Output = Result<()>>) -> Result<()> {
        {
            if self.operation_running.swap(true, Ordering::AcqRel) {
                return Err(ServiceBusy.into());
            }
            defer! {
                self.operation_running.store(false, Ordering::Release);
                self.operation_done.notify_waiters();
            }

            operation.await?;
        }

        Tray::global().update_menu().await
    }

    /// clod: только наблюдение, никаких привилегированных действий.
    ///
    /// Раньше `refresh()` при несовпадении версии службы уходил в переустановку
    /// — а зовут его старт ядра, ожидание службы и хэндофф. После обновления
    /// приложения (служба на диске старая) это давало запрос прав из ниоткуда,
    /// причём по разу на каждую попытку: `start_core_by_service` на Windows
    /// повторяет старт пять раз, плюс рестарты ядра и watcher — пользователь
    /// видел «Разрешить внести изменения?» полтора десятка раз подряд.
    /// Чинит службу теперь только явная команда из интерфейса.
    pub async fn refresh(&self) -> Result<()> {
        self.run_operation(async {
            if let Err(e) = is_service_available().await {
                let reason = format!("service IPC is not answering: {e}");
                self.set_status(ServiceStatus::Unavailable(reason.clone()));
                bail!(reason);
            }
            if clash_verge_service_ipc::is_reinstall_service_needed().await {
                self.set_status(ServiceStatus::NeedsReinstall);
                // Одно уведомление на сессию: чинить будет пользователь кнопкой,
                // а не мы молча с запросом прав.
                if !REINSTALL_NOTICED.swap(true, Ordering::AcqRel) {
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
                run_service_command(reinstall_service, "reinstall service")?;
                wait_for_service_ipc(self).await?;
            }
            ServiceStatus::ForceReinstallRequired => {
                logging!(
                    info,
                    Type::Service,
                    "Требуется принудительная переустановка службы, запуск процесса"
                );
                run_service_command(force_reinstall_service, "force reinstall service")?;
                wait_for_service_ipc(self).await?;
            }
            ServiceStatus::InstallRequired => {
                REINSTALL_NOTICED.store(false, Ordering::Release);
                logging!(
                    info,
                    Type::Service,
                    "Требуется установка службы, запуск процесса установки"
                );
                run_service_command(install_service, "install service")?;
                wait_for_service_ipc(self).await?;
                if clash_verge_service_ipc::is_reinstall_service_needed().await {
                    logging!(
                        info,
                        Type::Service,
                        "Версия службы не совпадает, запуск процесса переустановки"
                    );
                    self.set_status(ServiceStatus::NeedsReinstall);
                    run_service_command(reinstall_service, "reinstall service")?;
                    wait_for_service_ipc(self).await?;
                }
            }
            ServiceStatus::UninstallRequired => {
                logging!(
                    info,
                    Type::Service,
                    "Требуется удаление службы, запуск процесса удаления"
                );
                run_service_command(uninstall_service, "uninstall service")?;
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

fn run_service_command(operation: impl FnOnce() -> Result<()>, label: &'static str) -> Result<()> {
    tokio::task::block_in_place(operation).with_context(|| format!("{label} failed"))
}

/// clod: про устаревшую службу говорим один раз за сессию.
static REINSTALL_NOTICED: AtomicBool = AtomicBool::new(false);

pub static SERVICE_MANAGER: Lazy<ServiceManager> = Lazy::new(|| ServiceManager {
    status: Mutex::new(ServiceStatus::Unavailable("Need Checks".into())),
    operation_running: AtomicBool::new(false),
    operation_done: Notify::new(),
});

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
