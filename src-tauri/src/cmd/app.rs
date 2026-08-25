use super::CmdResult;
use crate::{cmd::StringifyErr as _, feat, utils::dirs};
use smartstring::alias::String;
use tauri::{AppHandle, Manager as _};

#[tauri::command]
pub async fn copy_support_bundle(app: AppHandle, lines: Option<usize>) -> CmdResult<usize> {
    use tauri_plugin_clipboard_manager::ClipboardExt as _;

    let bundle = crate::module::support_bundle::build(lines).await.stringify_err()?;
    let size = bundle.len();
    app.clipboard().write_text(bundle).stringify_err()?;
    Ok(size)
}

#[tauri::command]
pub async fn export_logs(path: std::string::String) -> CmdResult<usize> {
    crate::module::log_export::write_archive(std::path::PathBuf::from(path))
        .await
        .stringify_err()
}

#[tauri::command]
pub async fn open_app_dir() -> CmdResult<()> {
    let app_dir = dirs::app_home_dir().stringify_err()?;
    open::that(app_dir).stringify_err()
}

#[tauri::command]
pub async fn open_core_dir() -> CmdResult<()> {
    let core_dir = tauri::utils::platform::current_exe().stringify_err()?;
    let core_dir = core_dir.parent().ok_or("failed to get core dir")?;
    open::that(core_dir).stringify_err()
}

#[tauri::command]
pub async fn open_logs_dir() -> CmdResult<()> {
    let log_dir = dirs::app_logs_dir().stringify_err()?;
    open::that(log_dir).stringify_err()
}

const OPENABLE_URL_SCHEMES: &[&str] = &["https", "tg", "mailto"];

fn is_openable_url(url: &str) -> bool {
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return false;
    };
    if OPENABLE_URL_SCHEMES.contains(&parsed.scheme()) {
        return true;
    }
    parsed.scheme() == "http"
        && parsed.host_str().is_some_and(|host| {
            let host = host.trim_start_matches('[').trim_end_matches(']');
            host.eq_ignore_ascii_case("localhost") || host.parse::<std::net::IpAddr>().is_ok_and(|ip| ip.is_loopback())
        })
}

#[tauri::command]
pub fn open_web_url(url: String) -> CmdResult<()> {
    if !is_openable_url(url.as_str()) {
        return Err("url scheme is not allowed".into());
    }
    open::that(url.as_str()).stringify_err()
}

#[tauri::command]
pub fn get_connect_session_start() -> Option<i64> {
    feat::connect_session_start()
}

#[tauri::command]
pub fn open_devtools(app_handle: AppHandle) {
    if let Some(window) = app_handle.get_webview_window("main") {
        if !window.is_devtools_open() {
            window.open_devtools();
        } else {
            window.close_devtools();
        }
    }
}

#[tauri::command]
pub async fn exit_app() {
    feat::quit().await;
}

#[tauri::command]
pub async fn restart_app() -> CmdResult<()> {
    feat::restart_app().await;
    Ok(())
}
#[tauri::command]
pub fn get_app_dir() -> CmdResult<String> {
    let app_home_dir = dirs::app_home_dir().stringify_err()?.to_string_lossy().into();
    Ok(app_home_dir)
}
#[tauri::command]
pub async fn download_icon_cache(url: String, name: String) -> CmdResult<String> {
    feat::download_icon_cache(url, name).await
}

#[tauri::command]
pub async fn copy_icon_file(path: String, icon_info: feat::IconInfo) -> CmdResult<String> {
    feat::copy_icon_file(path, icon_info).await
}

#[tauri::command]
pub async fn save_window_size_for_mode(simple: bool) -> CmdResult<()> {
    if let Some(window) = crate::utils::window_manager::WindowManager::get_main_window() {
        crate::utils::resolve::window::save_window_size_for_mode(&window, simple).await;
    }
    Ok(())
}

#[tauri::command]
pub async fn apply_window_size_for_mode(simple: bool) -> CmdResult<()> {
    if let Some(window) = crate::utils::window_manager::WindowManager::get_main_window() {
        crate::utils::resolve::window::apply_window_size_for_mode(&window, simple).await;
    }
    Ok(())
}

#[tauri::command]
pub async fn fit_window_to_content(content_height: f64) -> CmdResult<f64> {
    let Some(window) = crate::utils::window_manager::WindowManager::get_main_window() else {
        return Ok(0.0);
    };
    Ok(crate::utils::resolve::window::fit_window_to_content(&window, content_height).await)
}

#[tauri::command]
pub fn get_traffic_estimate() -> crate::core::traffic_estimate::TrafficEstimate {
    crate::core::traffic_estimate::snapshot()
}

#[cfg(test)]
mod tests {
    use super::is_openable_url;

    #[test]
    fn only_web_and_messenger_links_are_opened() {
        for url in [
            "https://example.com/support",
            "http://127.0.0.1:9097/ui/",
            "http://localhost:9097/",
            "tg://resolve?domain=example",
            "mailto:support@example.com",
        ] {
            assert!(is_openable_url(url), "{url}");
        }
        for url in [
            "http://example.com",
            "http://192.168.1.1/",
            "file:///C:/Windows/System32/cmd.exe",
            "C:\\Windows\\System32\\cmd.exe",
            "ms-settings:network",
            "javascript:alert(1)",
            "",
        ] {
            assert!(!is_openable_url(url), "{url}");
        }
    }
}
