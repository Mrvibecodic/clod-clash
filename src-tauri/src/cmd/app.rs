use super::CmdResult;
use crate::core::autostart;
use crate::{cmd::StringifyErr as _, feat, utils::dirs};
use smartstring::alias::String;
use tauri::{AppHandle, Manager as _};

/// clod: отчёт для поддержки — собрать и положить в буфер обмена.
///
/// Возвращает только размер: интерфейсу сам текст не нужен — он уже в буфере, —
/// а отдавать наружу состояние подписки и хвосты логов лишний раз незачем.
#[tauri::command]
pub async fn copy_support_bundle(app: AppHandle, lines: Option<usize>) -> CmdResult<usize> {
    use tauri_plugin_clipboard_manager::ClipboardExt as _;

    let bundle = crate::module::support_bundle::build(lines).await.stringify_err()?;
    let size = bundle.len();
    app.clipboard().write_text(bundle).stringify_err()?;
    Ok(size)
}

/// 打开应用程序所在目录
#[tauri::command]
pub async fn open_app_dir() -> CmdResult<()> {
    let app_dir = dirs::app_home_dir().stringify_err()?;
    open::that(app_dir).stringify_err()
}

/// 打开核心所在目录
#[tauri::command]
pub async fn open_core_dir() -> CmdResult<()> {
    let core_dir = tauri::utils::platform::current_exe().stringify_err()?;
    let core_dir = core_dir.parent().ok_or("failed to get core dir")?;
    open::that(core_dir).stringify_err()
}

/// 打开日志目录
#[tauri::command]
pub async fn open_logs_dir() -> CmdResult<()> {
    let log_dir = dirs::app_logs_dir().stringify_err()?;
    open::that(log_dir).stringify_err()
}

/// 打开网页链接
#[tauri::command]
pub fn open_web_url(url: String) -> CmdResult<()> {
    open::that(url.as_str()).stringify_err()
}

/// clod:simple-mode — epoch ms of the moment the Connect targets came up,
/// or `null` when nothing is active. Source of truth for the session timer:
/// the frontend cannot track this itself across page switches and tray
/// toggles.
#[tauri::command]
pub fn get_connect_session_start() -> Option<i64> {
    feat::connect_session_start()
}

/// 打开/关闭开发者工具
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

/// 退出应用
#[tauri::command]
pub async fn exit_app() {
    feat::quit().await;
}

/// 重启应用
#[tauri::command]
pub async fn restart_app() -> CmdResult<()> {
    feat::restart_app().await;
    Ok(())
}

/// 获取便携版标识
#[tauri::command]
pub fn get_portable_flag() -> bool {
    *dirs::PORTABLE_FLAG.get().unwrap_or(&false)
}

/// 获取应用目录
#[tauri::command]
pub fn get_app_dir() -> CmdResult<String> {
    let app_home_dir = dirs::app_home_dir().stringify_err()?.to_string_lossy().into();
    Ok(app_home_dir)
}

/// 获取当前自启动状态
#[tauri::command]
pub fn get_auto_launch_status() -> CmdResult<bool> {
    autostart::get_launch_status().stringify_err()
}

/// 下载图标缓存
#[tauri::command]
pub async fn download_icon_cache(url: String, name: String) -> CmdResult<String> {
    feat::download_icon_cache(url, name).await
}

/// 复制图标文件
#[tauri::command]
pub async fn copy_icon_file(path: String, icon_info: feat::IconInfo) -> CmdResult<String> {
    feat::copy_icon_file(path, icon_info).await
}

// clod:mode-window begin
/// Remember the current window size for the mode the user is leaving (or
/// simply using — also called after a manual resize settles).
#[tauri::command]
pub async fn save_window_size_for_mode(simple: bool) -> CmdResult<()> {
    if let Some(window) = crate::utils::window_manager::WindowManager::get_main_window() {
        crate::utils::resolve::window::save_window_size_for_mode(&window, simple).await;
    }
    Ok(())
}

/// Resize the window for the mode the user is entering, keeping it on-screen.
#[tauri::command]
pub async fn apply_window_size_for_mode(simple: bool) -> CmdResult<()> {
    if let Some(window) = crate::utils::window_manager::WindowManager::get_main_window() {
        crate::utils::resolve::window::apply_window_size_for_mode(&window, simple).await;
    }
    Ok(())
}
// clod:mode-window end

// clod:traffic-estimate — сколько клиент насчитал сверх данных подписки.
// Само по себе это число ничего не решает: статусы «трафик закончился» и
// критические состояния по-прежнему считаются только по подписке.
#[tauri::command]
pub fn get_traffic_estimate() -> crate::core::traffic_estimate::TrafficEstimate {
    crate::core::traffic_estimate::snapshot()
}
