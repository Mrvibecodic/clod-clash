use dark_light::{Mode as SystemTheme, detect as detect_system_theme};
use tauri::utils::config::Color;
use tauri::webview::PageLoadEvent;
use tauri::{Theme, WebviewWindow};

use crate::{config::Config, core::handle, utils::resolve::window_script::build_window_initial_script};
use clash_verge_logging::{Type, logging, logging_error};

const DARK_BACKGROUND_COLOR: Color = Color(46, 48, 61, 255); // #2E303D
const LIGHT_BACKGROUND_COLOR: Color = Color(245, 245, 245, 255); // #F5F5F5
const DARK_BACKGROUND_HEX: &str = "#2E303D";
const LIGHT_BACKGROUND_HEX: &str = "#F5F5F5";

// Определение констант размера окна по умолчанию
const DEFAULT_WIDTH: f64 = 940.0;
const DEFAULT_HEIGHT: f64 = 700.0;

const MINIMAL_WIDTH: f64 = 520.0;
const MINIMAL_HEIGHT: f64 = 520.0;

#[cfg(target_os = "linux")]
const DEFAULT_DECORATIONS: bool = false;
#[cfg(not(target_os = "linux"))]
const DEFAULT_DECORATIONS: bool = true;

const fn restored_window_size_is_too_small(width: u32, height: u32) -> bool {
    width < MINIMAL_WIDTH as u32 || height < MINIMAL_HEIGHT as u32
}

fn restore_default_size_if_needed(window: &WebviewWindow) {
    let Ok(size) = window.outer_size() else {
        return;
    };

    if !restored_window_size_is_too_small(size.width, size.height) {
        return;
    }

    logging_error!(
        Type::Window,
        window.set_size(tauri::LogicalSize::new(DEFAULT_WIDTH, DEFAULT_HEIGHT))
    );
    logging_error!(Type::Window, window.center());
}

// clod: minimum part of the window that must land on some monitor for the
// restored position to count as reachable.
const MIN_VISIBLE_WIDTH: i32 = 100;
const MIN_VISIBLE_HEIGHT: i32 = 50;

/// The window-state plugin restores the last position verbatim; after a
/// monitor is unplugged or the display scale changes that spot may no longer
/// exist and the window comes up unreachable off-screen. Centre it instead.
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

// clod:mode-window begin
/// Default logical window sizes per interface mode. The simple mode is a
/// single 520px column plus padding: anything wider is empty margin.
const SIMPLE_MODE_SIZE: (f64, f64) = (560.0, 720.0);
/// clod: расширенный режим — две колонки, и в 940×700 они помещались только
/// пока провайдер не заполнил заголовки: с баннерами, порталом и поддержкой
/// карточка «Сеть» и плитки уезжали за нижний край. Ширина важнее высоты —
/// на 1100 плитки встают в три столбца вместо двух и экран становится ниже.
const ADVANCED_MODE_SIZE: (f64, f64) = (1100.0, 760.0);

/// The advanced mode's first default. Its size is written into the config on
/// any resize or move, so testers who never touched the window still carry
/// exactly this pair — and would keep the cramped window forever. Read it as
/// «never chosen» and hand out the current default instead; a user who really
/// wants 940×700 gets it remembered again the moment they resize to it.
const LEGACY_ADVANCED_SIZE: (u32, u32) = (DEFAULT_WIDTH as u32, DEFAULT_HEIGHT as u32);

/// The mode the interface actually starts in: the user's own choice wins,
/// then the provider's `clod-simple-mode` header, then the simple default.
/// Mirrors `useSimpleMode` on the frontend.
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

/// `const fn` не умеет `filter`, поэтому руками.
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

/// Remember the window's current logical size — and its position — in the
/// given mode's slots, so the next switch back restores where and how the
/// user actually had it. Positions live in the verge config, which survives
/// app updates, so the window does not jump around after an upgrade.
/// Maximized and fullscreen states are transient, not a choice — skipped.
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
    // clod:fit-window — пока окно ведёт автоподгон, его высоту в конфиг писать
    // НЕЛЬЗЯ: там оседала бы высота последнего баннера, и «размер режима»
    // превращался бы в память случайного содержимого. Размер запоминается с
    // того момента, как пользователь взял управление на себя (ручной ресайз
    // гасит автоподгон). Позиция — всегда его выбор, её сохраняем в любом случае.
    let size = (!window_fit_content_enabled().await).then_some((logical.width as u32, logical.height as u32));
    // Physical outer position: monitor coordinates are physical, and the
    // outer frame is what the user actually placed on the screen.
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

/// Resize the window to the given mode's remembered (or default) size, move
/// it to the mode's remembered position, and make sure the result stays on
/// the screen (a vanished monitor or changed scale must not strand it).
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
        // The stored spot may belong to a monitor that is gone — recentre
        // rather than restore the window somewhere unreachable.
        restore_position_if_offscreen(window);
    } else {
        // clod: `set_size` растит окно вправо и вниз от прежнего левого верхнего
        // угла. Места, выбранного пользователем, здесь нет — значит окно только
        // что открыли по центру под ДРУГОЙ размер, и без этого оно съезжает
        // ровно на половину прибавки.
        logging_error!(Type::Window, window.center());
    }
    keep_window_on_screen(window);
}

/// Nudge (and if needed shrink) the window so it sits inside the current
/// monitor's work area — a programmatic resize must never push it off-screen
/// or under the taskbar.
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

    // Wider or taller than the work area: shrink to fit first.
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
// clod:mode-window end

// clod:fit-window begin
/// Дыхание между нижним краем окна и краем рабочей области: окно, прижатое
/// вплотную к панели задач, выглядит застрявшим.
const FIT_BOTTOM_MARGIN: f64 = 8.0;

/// Автоподгон окна под содержимое. Работает из коробки; выключается либо
/// тумблером в настройках, либо ручным изменением размера окна — с этого
/// момента размеры принадлежат пользователю, и прокрутка в них законна.
pub async fn window_fit_content_enabled() -> bool {
    Config::verge().await.latest_arc().window_fit_content.unwrap_or(true)
}

/// Наибольшая логическая высота СОДЕРЖИМОГО, которая помещается в рабочую
/// область текущего монитора: из неё вычтены рамка с заголовком окна и запас
/// снизу. Это и есть потолок, выше которого прокрутки не избежать.
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
    // Рамка и заголовок: у окна без декораций это ноль, у обычного — высота
    // системной шапки. Константой это писать нельзя, она разная на каждой ОС.
    let frame = f64::from(outer.height.saturating_sub(inner.height)) / scale;
    Some((area_height - frame - FIT_BOTTOM_MARGIN).max(MINIMAL_HEIGHT))
}

/// Высота окна, которую применяем: столько, сколько просит содержимое, но не
/// ниже минимума окна и не выше потолка рабочей области.
const fn clamp_fit_height(content: f64, ceiling: f64) -> f64 {
    if !content.is_finite() {
        return MINIMAL_HEIGHT;
    }
    content.clamp(MINIMAL_HEIGHT, ceiling.max(MINIMAL_HEIGHT))
}

/// Посадить окно ровно на высоту содержимого и вернуть потолок, чтобы фронт
/// знал, когда пора включать компактную вёрстку.
///
/// Ширину не трогаем: вширь содержимое переливается само, а дёрганье окна по
/// горизонтали пользователь читает как дефект.
pub async fn fit_window_to_content(window: &WebviewWindow, content_height: f64) -> f64 {
    let ceiling = content_height_ceiling(window).unwrap_or(MINIMAL_HEIGHT);
    if window.is_maximized().unwrap_or(false) || window.is_fullscreen().unwrap_or(false) {
        return ceiling;
    }
    if !window_fit_content_enabled().await {
        return ceiling;
    }
    let target = clamp_fit_height(content_height, ceiling);
    let Ok(inner) = window.inner_size() else { return ceiling };
    let scale = window.scale_factor().unwrap_or(1.0);
    let current: tauri::LogicalSize<f64> = inner.to_logical(scale);
    // Разница меньше пикселя — не трогаем окно вовсе: каждый `set_size`
    // порождает событие `onResized`, а на нём висит и сохранение размера.
    if (current.height - target).abs() < 1.0 {
        return ceiling;
    }
    logging_error!(
        Type::Window,
        window.set_size(tauri::LogicalSize::new(current.width, target))
    );
    // Окно растёт вниз от левого верхнего угла — у нижнего края экрана его
    // надо подвинуть вверх, иначе прибавка уедет под панель задач.
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
        // Экран ниже минимального окна: минимум важнее потолка, иначе
        // `clamp` получил бы перевёрнутые границы и запаниковал.
        assert!((clamp_fit_height(700.0, 300.0) - MINIMAL_HEIGHT).abs() < f64::EPSILON);
    }

    #[test]
    fn broken_measurement_falls_back_to_the_minimum() {
        assert!((clamp_fit_height(f64::NAN, 900.0) - MINIMAL_HEIGHT).abs() < f64::EPSILON);
    }
}
// clod:fit-window end

/// Создаёт новое окно WebView
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

    let mut builder = tauri::WebviewWindowBuilder::new(
        app_handle,
        "main", /* the unique window label */
        tauri::WebviewUrl::App(start_page.into()),
    )
    .title(crate::constants::branding::APP_NAME)
    .center()
    .decorations(DEFAULT_DECORATIONS)
    .fullscreen(false)
    .inner_size(DEFAULT_WIDTH, DEFAULT_HEIGHT)
    .min_inner_size(MINIMAL_WIDTH, MINIMAL_HEIGHT)
    .visible(false) // ждём готовности цвета темы перед показом, чтобы избежать скачка цвета
    .initialization_script(&initial_script)
    .general_autofill_enabled(false) // отключаем автозаполнение
    .on_page_load(move |window, payload| {
        if payload.event() != PageLoadEvent::Finished {
            return;
        }

        logging_error!(Type::Window, window.show());
        logging_error!(Type::Window, window.set_focus());
    });

    if let Some(theme) = resolved_theme {
        builder = builder.theme(Some(theme));
    }

    builder = builder.background_color(background_color);

    match builder.build() {
        Ok(window) => {
            logging_error!(Type::Window, window.set_background_color(Some(background_color)));
            restore_default_size_if_needed(&window);
            // clod:mode-window — the window-state plugin restores one global
            // size; the interface mode knows better what it needs, so the
            // per-mode size (saved or default) wins before the window shows.
            apply_window_size_for_mode(&window, effective_simple_mode().await).await;
            restore_position_if_offscreen(&window);
            // Страница нового окна и так актуальна — сбрасываем оставшийся от
            // старого окна флаг ожидающей перезагрузки, чтобы избежать лишнего reload
            #[cfg(target_os = "macos")]
            take_webview_needs_reload();
            Ok(window)
        }
        Err(e) => Err(e.to_string()),
    }
}

/// Флаг гибели процесса рендеринга и ожидающей перезагрузки страницы (macOS)
///
/// Устанавливается, когда система завершает процесс рендеринга при невидимом
/// окне; снимается и обрабатывается путём активации окна при следующем открытии.
#[cfg(target_os = "macos")]
static WEBVIEW_NEEDS_RELOAD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Забирает и сбрасывает флаг "страница ожидает перезагрузки"
///
/// # Returns
/// * `bool` - был ли процесс рендеринга завершён системой при невидимом окне,
///   нужен ли странице reload
#[cfg(target_os = "macos")]
pub fn take_webview_needs_reload() -> bool {
    WEBVIEW_NEEDS_RELOAD.swap(false, std::sync::atomic::Ordering::SeqCst)
}

/// Восстановление после завершения процесса рендеринга WebView системой (macOS)
///
/// macOS может убить процесс рендеринга WebContent у WKWebView при нехватке памяти:
/// 1. слой содержимого страницы исчезает, окно при открытии показывает белый экран;
/// 2. состояние фронтенд JS теряется, невозможно вызвать `ws_disconnect` для очистки
///    подписок Mihomo WebSocket, осиротевшие подписки продолжают закидывать payload
///    больше 1KB (например, полный снапшот `/connections`) в `ChannelDataIpcQueue`
///    tauri, а забирать их некому — память основного процесса растёт бесконечно.
///
/// Стратегия восстановления:
/// * окно видно (редкий случай убийства на переднем плане) — сразу reload страницы;
/// * окно скрыто/свёрнуто (частый случай, окно висит в трее) — только ставим флаг
///   ожидающей перезагрузки, reload выполнится при следующем открытии окна.
///   Система убивает процесс рендеринга невидимого окна именно из-за нехватки
///   памяти, поэтому немедленное пересоздание процесса и тратит память, и может
///   привести к циклу "система убила → подняли → снова убила".
///
/// Примечание: после регистрации `on_web_content_process_terminate` на уровне
/// приложения переопределяется поведение автоматического reload по умолчанию
/// у tauri-runtime-wry, поэтому состояние "страница мертва" сохраняется, пока
/// мы не выполним reload сами.
///
/// # Arguments
/// * `webview` - WebView, чей процесс рендеринга был завершён
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

    // Ленивый флаг перезагрузки только для главного окна; у прочих webview
    // (update-splash) нет канала потребления — при невидимости сразу reload
    let is_main_window = webview.label() == "main";
    let reload_now = is_user_visible || !is_main_window;

    if !reload_now {
        // Главное окно не видно: ставим флаг, откладываем до следующего
        // activate_window / reload_main_window_if_needed
        WEBVIEW_NEEDS_RELOAD.store(true, std::sync::atomic::Ordering::SeqCst);
        logging!(
            info,
            Type::Window,
            "окно не видно, страница перезагрузится при следующем открытии окна"
        );
    }

    // Очищаем все подписки Mihomo WS, чтобы не допустить утечки ChannelDataIpcQueue
    // (задача скорости в трее переподключится сама примерно через 1с).
    // reload обязан идти в той же задаче после очистки, иначе очистка может
    // случайно снести подписки новой страницы после перезагрузки (гонка).
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

/// Забирает флаг ожидающей перезагрузки и делает reload главного окна (macOS).
/// Подстраховка для случаев, когда нативное снятие минимизации (миниатюра в Dock /
/// Mission Control / меню окон) вызывает только Focused(true), минуя activate_window.
/// Использует тот же swap-флаг, что и activate_window: кто забрал первым, тот и
/// перезагружает, повторного reload не будет.
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
