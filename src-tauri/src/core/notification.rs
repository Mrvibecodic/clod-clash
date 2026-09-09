use clash_verge_logging::{Type, logging};
use serde_json::json;
use smartstring::alias::String;
use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{AppHandle, Emitter as _, Manager as _, WebviewWindow};

#[cfg(not(target_os = "macos"))]
use std::sync::{OnceLock, mpsc};

const PENDING_NOTICES_CAP: usize = 20;
static PENDING_NOTICES: Mutex<VecDeque<(std::string::String, std::string::String)>> = Mutex::new(VecDeque::new());

/// Страница на месте и слушает уведомления.
///
/// Очередь придерживала уведомления только при отсутствии окна, а штатное
/// сворачивание в трей окно не уничтожает, а прячет: уведомление уходило в
/// спрятанную страницу и пропадало. Плюс окно создаётся раньше, чем страница
/// повесит слушатель, — уведомления старта в очередь тоже не попадали.
/// Флаг ставит сама страница, забирая очередь; снимается при скрытии окна.
/// Спрашивать окно «видимо ли ты» из потока отправки нельзя — это синхронный
/// вызов к главному потоку, тот самый класс дедлоков, от которого страхуется
/// `window_manager`.
static FRONTEND_LISTENING: AtomicBool = AtomicBool::new(false);

pub fn frontend_stopped_listening() {
    FRONTEND_LISTENING.store(false, Ordering::Release);
}

/// Может ли уведомление дойти до страницы прямо сейчас.
const fn can_reach_the_page(listening: bool, window_exists: bool) -> bool {
    listening && window_exists
}

fn pending_notices() -> std::sync::MutexGuard<'static, VecDeque<(std::string::String, std::string::String)>> {
    match PENDING_NOTICES.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

const HELD_STATUS_PREFIXES: &[&str] = &[
    "core::",
    "sysproxy::",
    "service::",
    "tun::",
    "config_validate::",
    "clod_config::",
    "update_failed",
    "app_quit::",
];
const NEVER_HELD_STATUSES: &[&str] = &["tun::setup_started", "tun::setup_done"];

fn worth_holding(status: &str) -> bool {
    !NEVER_HELD_STATUSES.contains(&status) && HELD_STATUS_PREFIXES.iter().any(|prefix| status.starts_with(prefix))
}

fn hold_notice(status: &str, message: &str) {
    let queued = {
        let mut pending = pending_notices();
        if pending.len() >= PENDING_NOTICES_CAP {
            pending.pop_front();
        }
        pending.push_back((status.to_owned(), message.to_owned()));
        pending.len()
    };
    logging!(
        info,
        Type::Frontend,
        "окна нет — уведомление {} отложено до его появления ({} в очереди)",
        status,
        queued
    );
}

#[derive(Debug)]
pub enum FrontendEvent<'a> {
    RefreshClash,
    RefreshVerge,
    RefreshProfiles,
    NoticeMessage { status: &'a str, message: String },
    ProfileChanged { current_profile_id: &'a String },
    TimerUpdated { profile_index: &'a String },
    ProfileUpdateStarted { uid: &'a String },
    ProfileUpdateCompleted { uid: &'a String },
    HwidNotice { payload: serde_json::Value },
    RefreshProxyConfig,
    CoreUpdateProgress { payload: serde_json::Value },
    WindowShown,
}

#[derive(Debug)]
pub struct NotificationSystem {}

impl NotificationSystem {
    pub fn take_pending_notices() -> Vec<(std::string::String, std::string::String)> {
        FRONTEND_LISTENING.store(true, Ordering::Release);
        pending_notices().drain(..).collect()
    }

    fn held_for_later(app_handle: &AppHandle, event: &FrontendEvent) -> bool {
        let FrontendEvent::NoticeMessage { status, message } = event else {
            return false;
        };
        let window_exists = app_handle.get_webview_window("main").is_some();
        if can_reach_the_page(FRONTEND_LISTENING.load(Ordering::Acquire), window_exists) {
            return false;
        }
        // Спрятанное окно: то, что стоит подождать, ждёт показа; остальное
        // уходит в страницу как раньше — она жива, просто не на экране.
        if window_exists && !worth_holding(status) {
            return false;
        }
        if worth_holding(status) {
            hold_notice(status, message);
        }
        true
    }

    fn emit_to_window(window: &WebviewWindow, event_name: &'static str, payload: serde_json::Value) {
        if let Err(e) = window.emit(event_name, payload) {
            logging!(warn, Type::Frontend, "Event emit failed: {}", e);
        }
    }

    fn serialize_event(event: FrontendEvent) -> (&'static str, Result<serde_json::Value, serde_json::Error>) {
        match event {
            FrontendEvent::RefreshClash => ("verge://refresh-clash-config", Ok(json!("yes"))),
            FrontendEvent::RefreshVerge => ("verge://refresh-verge-config", Ok(json!("yes"))),
            FrontendEvent::RefreshProfiles => ("verge://refresh-profiles", Ok(json!("yes"))),
            FrontendEvent::NoticeMessage { status, message } => {
                ("verge://notice-message", serde_json::to_value((status, message)))
            }
            FrontendEvent::ProfileChanged { current_profile_id } => ("profile-changed", Ok(json!(current_profile_id))),
            FrontendEvent::TimerUpdated { profile_index } => ("verge://timer-updated", Ok(json!(profile_index))),
            FrontendEvent::ProfileUpdateStarted { uid } => ("profile-update-started", Ok(json!({ "uid": uid }))),
            FrontendEvent::ProfileUpdateCompleted { uid } => ("profile-update-completed", Ok(json!({ "uid": uid }))),
            FrontendEvent::HwidNotice { payload } => ("clod://hwid-notice", Ok(payload)),
            FrontendEvent::RefreshProxyConfig => ("verge://refresh-proxy-config", Ok(json!("yes"))),
            FrontendEvent::CoreUpdateProgress { payload } => ("clod://core-update-progress", Ok(payload)),
            FrontendEvent::WindowShown => ("verge://window-shown", Ok(json!(null))),
        }
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn send_event(app_handle: AppHandle, event: FrontendEvent) {
        if Self::held_for_later(&app_handle, &event) {
            return;
        }
        let (event_name, Ok(payload)) = Self::serialize_event(event) else {
            return;
        };
        let dispatch_handle = app_handle.clone();
        if let Err(err) = app_handle.run_on_main_thread(move || {
            if let Some(window) = dispatch_handle.get_webview_window("main") {
                Self::emit_to_window(&window, event_name, payload);
            }
        }) {
            logging!(warn, Type::Frontend, "Failed to dispatch event on main thread: {err}");
        }
    }

    #[cfg(not(target_os = "macos"))]
    pub(crate) fn send_event(app_handle: AppHandle, event: FrontendEvent) {
        if Self::held_for_later(&app_handle, &event) {
            return;
        }
        let (event_name, Ok(payload)) = Self::serialize_event(event) else {
            return;
        };
        if let Err(returned) = Self::emitter().send(QueuedEvent {
            app_handle,
            event_name,
            payload,
        }) {
            logging!(warn, Type::Frontend, "The frontend event thread is gone: {event_name}");
            let event = returned.0;
            if let Some(window) = event.app_handle.get_webview_window("main") {
                Self::emit_to_window(&window, event.event_name, event.payload);
            }
        }
    }

    #[cfg(not(target_os = "macos"))]
    fn emitter() -> &'static mpsc::Sender<QueuedEvent> {
        static TX: OnceLock<mpsc::Sender<QueuedEvent>> = OnceLock::new();
        TX.get_or_init(|| {
            let (tx, rx) = mpsc::channel::<QueuedEvent>();
            let spawned = std::thread::Builder::new()
                .name("frontend-events".into())
                .spawn(move || {
                    while let Ok(event) = rx.recv() {
                        let name = event.event_name;
                        if let Some(window) = event.app_handle.get_webview_window("main") {
                            Self::emit_to_window(&window, name, event.payload);
                            if name == "verge://window-shown" {
                                logging!(info, Type::Window, "Странице сообщено о показе");
                            }
                        }
                    }
                });
            if let Err(e) = spawned {
                logging!(warn, Type::Frontend, "Failed to start the frontend event thread: {e}");
            }
            tx
        })
    }
}

#[cfg(not(target_os = "macos"))]
struct QueuedEvent {
    app_handle: AppHandle,
    event_name: &'static str,
    payload: serde_json::Value,
}

#[cfg(test)]
mod tests {
    use super::{can_reach_the_page, worth_holding};

    #[test]
    fn a_notice_reaches_the_page_only_when_it_exists_and_listens() {
        assert!(can_reach_the_page(true, true));
        assert!(!can_reach_the_page(true, false), "окна нет — тихий старт в трей");
        assert!(
            !can_reach_the_page(false, true),
            "окно спрятано или страница ещё не слушает"
        );
        assert!(!can_reach_the_page(false, false));
    }

    #[test]
    fn only_problems_wait_for_the_window() {
        assert!(worth_holding("core::not_ready"));
        assert!(worth_holding("sysproxy::core_not_running"));
        assert!(worth_holding("app_quit::core_still_running"));
        assert!(worth_holding("update_failed"));
        assert!(!worth_holding("tun::setup_done"));
        assert!(!worth_holding("set_config::ok"));
        assert!(!worth_holding("clod_sub::url_migrated"));
    }
}
