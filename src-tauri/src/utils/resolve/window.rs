use dark_light::{Mode as SystemTheme, detect as detect_system_theme};
use tauri::utils::config::Color;
use tauri::webview::PageLoadEvent;
use tauri::{Theme, WebviewWindow};

use crate::{config::Config, core::handle, utils::resolve::window_script::build_window_initial_script};
use clash_verge_logging::{Type, logging, logging_error};

const DARK_BACKGROUND_COLOR: Color = Color(46, 48, 61, 255);
const LIGHT_BACKGROUND_COLOR: Color = Color(245, 245, 245, 255);
const DARK_BACKGROUND_HEX: &str = "#2E303D";
const LIGHT_BACKGROUND_HEX: &str = "#F5F5F5";

const DEFAULT_WIDTH: f64 = 940.0;
const DEFAULT_HEIGHT: f64 = 700.0;

const MINIMAL_WIDTH: f64 = 520.0;
const MINIMAL_HEIGHT: f64 = 520.0;

#[cfg(target_os = "linux")]
const DEFAULT_DECORATIONS: bool = false;
#[cfg(not(target_os = "linux"))]
const DEFAULT_DECORATIONS: bool = true;

#[cfg(target_os = "windows")]
const WEBVIEW2_BROWSER_ARGS: &str = concat!(
    "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection",
    " --disable-renderer-backgrounding",
    " --disable-background-timer-throttling",
    " --disable-backgrounding-occluded-windows",
);

const fn restored_window_size_is_too_small(width: u32, height: u32) -> bool {
    width < MINIMAL_WIDTH as u32 || height < MINIMAL_HEIGHT as u32
}

fn restore_default_size_if_needed(window: &WebviewWindow) {
    let Ok(size) = window.inner_size() else {
        return;
    };
    let scale = window.scale_factor().unwrap_or(1.0);
    let logical: tauri::LogicalSize<f64> = size.to_logical(scale);

    if !restored_window_size_is_too_small(logical.width as u32, logical.height as u32) {
        return;
    }

    logging_error!(
        Type::Window,
        window.set_size(tauri::LogicalSize::new(DEFAULT_WIDTH, DEFAULT_HEIGHT))
    );
    logging_error!(Type::Window, window.center());
}

const MIN_VISIBLE_WIDTH: i32 = 100;
const MIN_VISIBLE_HEIGHT: i32 = 50;

fn restore_position_if_offscreen(window: &WebviewWindow) {
    let (Ok(pos), Ok(size), Ok(monitors)) = (
        window.outer_position(),
        window.outer_size(),
        window.available_monitors(),
    ) else {
        return;
    };
    if monitors.is_empty() {
        return;
    }

    let reachable = monitors.iter().any(|monitor| {
        let m_pos = monitor.position();
        let m_size = monitor.size();
        let overlap_x = (pos.x + size.width as i32).min(m_pos.x + m_size.width as i32) - pos.x.max(m_pos.x);
        let overlap_y = (pos.y + size.height as i32).min(m_pos.y + m_size.height as i32) - pos.y.max(m_pos.y);
        overlap_x >= MIN_VISIBLE_WIDTH && overlap_y >= MIN_VISIBLE_HEIGHT
    });

    if !reachable {
        logging!(warn, Type::Window, "restored window position is off-screen, centering");
        logging_error!(Type::Window, window.center());
    }
}

const SIMPLE_MODE_SIZE: (f64, f64) = (560.0, 720.0);
const ADVANCED_MODE_SIZE: (f64, f64) = (1100.0, 760.0);

const LEGACY_ADVANCED_SIZE: (u32, u32) = (DEFAULT_WIDTH as u32, DEFAULT_HEIGHT as u32);

async fn effective_simple_mode() -> bool {
    let verge = Config::verge().await.latest_arc();
    if let Some(choice) = verge.simple_mode {
        return choice;
    }
    let profiles = Config::profiles().await.latest_arc();
    profiles
        .current
        .as_ref()
        .and_then(|uid| profiles.get_item(uid).ok())
        .and_then(|item| item.simple_mode)
        .unwrap_or(true)
}

const fn stored_mode_size(verge: &crate::config::IVerge, simple: bool) -> Option<(u32, u32)> {
    if simple {
        verge.window_size_simple
    } else {
        drop_legacy_advanced_size(verge.window_size_advanced)
    }
}

const fn drop_legacy_advanced_size(stored: Option<(u32, u32)>) -> Option<(u32, u32)> {
    match stored {
        Some((w, h)) if w == LEGACY_ADVANCED_SIZE.0 && h == LEGACY_ADVANCED_SIZE.1 => None,
        other => other,
    }
}

const fn stored_mode_pos(verge: &crate::config::IVerge, simple: bool) -> Option<(i32, i32)> {
    if simple {
        verge.window_pos_simple
    } else {
        verge.window_pos_advanced
    }
}

const fn default_mode_size(simple: bool) -> (f64, f64) {
    if simple { SIMPLE_MODE_SIZE } else { ADVANCED_MODE_SIZE }
}

pub async fn save_window_size_for_mode(window: &WebviewWindow, simple: bool) {
    if window.is_maximized().unwrap_or(false) || window.is_fullscreen().unwrap_or(false) {
        return;
    }
    let Ok(size) = window.inner_size() else { return };
    let scale = window.scale_factor().unwrap_or(1.0);
    let logical: tauri::LogicalSize<f64> = size.to_logical(scale);
    if restored_window_size_is_too_small(logical.width as u32, logical.height as u32) {
        return;
    }
    let size = (!window_fit_content_enabled().await).then_some((logical.width as u32, logical.height as u32));
    let pos = window.outer_position().ok().map(|pos| (pos.x, pos.y));
    let patch = crate::config::IVerge {
        window_size_simple: if simple { size } else { None },
        window_size_advanced: if simple { None } else { size },
        window_pos_simple: if simple { pos } else { None },
        window_pos_advanced: if simple { None } else { pos },
        ..Default::default()
    };
    logging_error!(Type::Window, crate::feat::patch_verge(&patch, false).await);
}

pub async fn apply_window_size_for_mode(window: &WebviewWindow, simple: bool) {
    if window.is_maximized().unwrap_or(false) || window.is_fullscreen().unwrap_or(false) {
        return;
    }
    let (stored, stored_pos) = {
        let verge = Config::verge().await.latest_arc();
        (stored_mode_size(&verge, simple), stored_mode_pos(&verge, simple))
    };
    let (width, height) = stored
        .map(|(w, h)| (f64::from(w), f64::from(h)))
        .unwrap_or_else(|| default_mode_size(simple));
    logging_error!(
        Type::Window,
        window.set_size(tauri::LogicalSize::new(
            width.max(MINIMAL_WIDTH),
            height.max(MINIMAL_HEIGHT),
        ))
    );
    if let Some((x, y)) = stored_pos {
        logging_error!(Type::Window, window.set_position(tauri::PhysicalPosition::new(x, y)));
        restore_position_if_offscreen(window);
    } else {
        logging_error!(Type::Window, window.center());
    }
    keep_window_on_screen(window);
}

fn keep_window_on_screen(window: &WebviewWindow) {
    let monitor = match window.current_monitor() {
        Ok(Some(monitor)) => monitor,
        _ => match window.primary_monitor() {
            Ok(Some(monitor)) => monitor,
            _ => return,
        },
    };
    let area = monitor.work_area();
    let (Ok(pos), Ok(size)) = (window.outer_position(), window.outer_size()) else {
        return;
    };

    let fit_width = size.width.min(area.size.width);
    let fit_height = size.height.min(area.size.height);
    if fit_width != size.width || fit_height != size.height {
        logging_error!(
            Type::Window,
            window.set_size(tauri::PhysicalSize::new(fit_width, fit_height))
        );
    }

    let max_x = area.position.x + area.size.width as i32 - fit_width as i32;
    let max_y = area.position.y + area.size.height as i32 - fit_height as i32;
    let new_x = pos.x.clamp(area.position.x, max_x.max(area.position.x));
    let new_y = pos.y.clamp(area.position.y, max_y.max(area.position.y));
    if new_x != pos.x || new_y != pos.y {
        logging_error!(
            Type::Window,
            window.set_position(tauri::PhysicalPosition::new(new_x, new_y))
        );
    }
}

const FIT_BOTTOM_MARGIN: f64 = 8.0;

pub async fn window_fit_content_enabled() -> bool {
    Config::verge().await.latest_arc().window_fit_content.unwrap_or(true)
}

fn content_height_ceiling(window: &WebviewWindow) -> Option<f64> {
    let monitor = match window.current_monitor() {
        Ok(Some(monitor)) => monitor,
        _ => window.primary_monitor().ok().flatten()?,
    };
    let (Ok(outer), Ok(inner)) = (window.outer_size(), window.inner_size()) else {
        return None;
    };
    let scale = window.scale_factor().unwrap_or(1.0);
    let area_height = f64::from(monitor.work_area().size.height) / scale;
    let frame = f64::from(outer.height.saturating_sub(inner.height)) / scale;
    Some((area_height - frame - FIT_BOTTOM_MARGIN).max(MINIMAL_HEIGHT))
}

const fn clamp_fit_height(content: f64, ceiling: f64) -> f64 {
    if !content.is_finite() {
        return MINIMAL_HEIGHT;
    }
    content.clamp(MINIMAL_HEIGHT, ceiling.max(MINIMAL_HEIGHT))
}

pub async fn fit_window_to_content(window: &WebviewWindow, content_height: f64) -> f64 {
    let ceiling = content_height_ceiling(window).unwrap_or(MINIMAL_HEIGHT);
    if window.is_maximized().unwrap_or(false) || window.is_fullscreen().unwrap_or(false) {
        return ceiling;
    }
    if window.is_minimized().unwrap_or(false) || !window.is_visible().unwrap_or(true) {
        return ceiling;
    }
    if !window_fit_content_enabled().await {
        return ceiling;
    }
    let target = clamp_fit_height(content_height, ceiling);
    let Ok(inner) = window.inner_size() else { return ceiling };
    if inner.width == 0 || inner.height == 0 {
        return ceiling;
    }
    let scale = window.scale_factor().unwrap_or(1.0);
    let current: tauri::LogicalSize<f64> = inner.to_logical(scale);
    if (current.height - target).abs() < 1.0 {
        return ceiling;
    }
    logging_error!(
        Type::Window,
        window.set_size(tauri::LogicalSize::new(current.width, target))
    );
    keep_window_on_screen(window);
    ceiling
}

#[cfg(test)]
mod fit_tests {
    use super::{MINIMAL_HEIGHT, clamp_fit_height};

    #[test]
    fn content_height_wins_between_the_bounds() {
        assert!((clamp_fit_height(720.0, 900.0) - 720.0).abs() < f64::EPSILON);
    }

    #[test]
    fn ceiling_caps_tall_content() {
        assert!((clamp_fit_height(1200.0, 900.0) - 900.0).abs() < f64::EPSILON);
    }

    #[test]
    fn never_below_the_window_minimum() {
        assert!((clamp_fit_height(120.0, 900.0) - MINIMAL_HEIGHT).abs() < f64::EPSILON);
        assert!((clamp_fit_height(700.0, 300.0) - MINIMAL_HEIGHT).abs() < f64::EPSILON);
    }

    #[test]
    fn broken_measurement_falls_back_to_the_minimum() {
        assert!((clamp_fit_height(f64::NAN, 900.0) - MINIMAL_HEIGHT).abs() < f64::EPSILON);
    }
}

pub async fn build_new_window() -> Result<WebviewWindow, String> {
    let app_handle = handle::Handle::app_handle();

    let config = Config::verge().await;
    let latest = config.latest_arc();
    let start_page = latest.start_page.as_deref().unwrap_or("/");
    let initial_theme_mode = match latest.theme_mode.as_deref() {
        Some("dark") => "dark",
        Some("light") => "light",
        _ => "system",
    };

    let resolved_theme = match initial_theme_mode {
        "dark" => Some(Theme::Dark),
        "light" => Some(Theme::Light),
        _ => None,
    };

    let prefers_dark_background = match resolved_theme {
        Some(Theme::Dark) => true,
        Some(Theme::Light) => false,
        _ => !matches!(detect_system_theme().ok(), Some(SystemTheme::Light)),
    };

    let background_color = if prefers_dark_background {
        DARK_BACKGROUND_COLOR
    } else {
        LIGHT_BACKGROUND_COLOR
    };

    let initial_script = build_window_initial_script(initial_theme_mode, DARK_BACKGROUND_HEX, LIGHT_BACKGROUND_HEX);

    let mut builder = tauri::WebviewWindowBuilder::new(app_handle, "main", tauri::WebviewUrl::App(start_page.into()))
        .title(crate::constants::branding::APP_NAME)
        .center()
        .decorations(DEFAULT_DECORATIONS)
        .fullscreen(false)
        .inner_size(DEFAULT_WIDTH, DEFAULT_HEIGHT)
        .min_inner_size(MINIMAL_WIDTH, MINIMAL_HEIGHT)
        .visible(false)
        .initialization_script(&initial_script)
        .general_autofill_enabled(false)
        .on_page_load(move |window, payload| {
            if payload.event() != PageLoadEvent::Finished {
                return;
            }

            logging_error!(Type::Window, window.show());
            logging_error!(Type::Window, window.set_focus());
        });

    #[cfg(target_os = "windows")]
    {
        builder = builder.additional_browser_args(WEBVIEW2_BROWSER_ARGS);
    }

    if let Some(theme) = resolved_theme {
        builder = builder.theme(Some(theme));
    }

    builder = builder.background_color(background_color);

    match builder.build() {
        Ok(window) => {
            logging_error!(Type::Window, window.set_background_color(Some(background_color)));
            restore_default_size_if_needed(&window);
            apply_window_size_for_mode(&window, effective_simple_mode().await).await;
            restore_position_if_offscreen(&window);
            crate::utils::ui_watchdog::watch(&window);
            #[cfg(target_os = "macos")]
            take_webview_needs_reload();
            Ok(window)
        }
        Err(e) => Err(e.to_string()),
    }
}

#[cfg(target_os = "macos")]
static WEBVIEW_NEEDS_RELOAD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[cfg(target_os = "macos")]
pub fn take_webview_needs_reload() -> bool {
    WEBVIEW_NEEDS_RELOAD.swap(false, std::sync::atomic::Ordering::SeqCst)
}

#[cfg(target_os = "macos")]
pub fn on_web_content_process_terminated(webview: &tauri::Webview) {
    if handle::Handle::global().is_exiting() {
        return;
    }

    logging!(
        warn,
        Type::Window,
        "процесс рендеринга WebView завершён системой (label={}), начало восстановления",
        webview.label()
    );

    let window = webview.window();
    let is_user_visible = window.is_visible().unwrap_or(false) && !window.is_minimized().unwrap_or(false);

    let is_main_window = webview.label() == "main";
    let reload_now = is_user_visible || !is_main_window;

    if !reload_now {
        WEBVIEW_NEEDS_RELOAD.store(true, std::sync::atomic::Ordering::SeqCst);
        logging!(
            info,
            Type::Window,
            "окно не видно, страница перезагрузится при следующем открытии окна"
        );
    }

    let webview = webview.clone();
    crate::process::AsyncHandler::spawn(move || async move {
        if let Err(err) = handle::Handle::mihomo().await.clear_all_ws_connections().await {
            logging!(
                warn,
                Type::Window,
                "не удалось очистить подключения Mihomo WebSocket: {err}"
            );
        } else {
            logging!(info, Type::Window, "все подключения Mihomo WebSocket очищены");
        }
        if reload_now {
            logging_error!(Type::Window, webview.reload());
        }
    });
}

#[cfg(target_os = "macos")]
pub fn reload_main_window_if_needed() {
    if !take_webview_needs_reload() {
        return;
    }
    let Some(window) = crate::utils::window_manager::WindowManager::get_main_window() else {
        return;
    };
    logging!(
        info,
        Type::Window,
        "процесс рендеринга был завершён системой, страница перезагружена после фокуса окна"
    );
    if let Err(e) = window.reload() {
        logging!(warn, Type::Window, "не удалось перезагрузить страницу: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ADVANCED_MODE_SIZE, LEGACY_ADVANCED_SIZE, drop_legacy_advanced_size, restored_window_size_is_too_small,
    };

    #[test]
    fn legacy_advanced_size_is_replaced_by_the_current_default() {
        assert_eq!(drop_legacy_advanced_size(Some(LEGACY_ADVANCED_SIZE)), None);
    }

    #[test]
    fn advanced_size_chosen_by_the_user_survives() {
        assert_eq!(drop_legacy_advanced_size(Some((1280, 800))), Some((1280, 800)));
        assert_eq!(drop_legacy_advanced_size(Some((940, 701))), Some((940, 701)));
        assert_eq!(drop_legacy_advanced_size(None), None);
    }

    #[test]
    fn advanced_default_is_not_the_legacy_one() {
        assert_ne!(
            (ADVANCED_MODE_SIZE.0 as u32, ADVANCED_MODE_SIZE.1 as u32),
            LEGACY_ADVANCED_SIZE
        );
    }

    #[test]
    fn restored_window_size_rejects_zero_dimensions() {
        assert!(restored_window_size_is_too_small(0, 700));
        assert!(restored_window_size_is_too_small(940, 0));
    }

    #[test]
    fn restored_window_size_rejects_dimensions_below_minimum() {
        assert!(restored_window_size_is_too_small(519, 700));
        assert!(restored_window_size_is_too_small(940, 519));
    }

    #[test]
    fn restored_window_size_accepts_minimum_or_larger_dimensions() {
        assert!(!restored_window_size_is_too_small(520, 520));
        assert!(!restored_window_size_is_too_small(940, 700));
    }
}
