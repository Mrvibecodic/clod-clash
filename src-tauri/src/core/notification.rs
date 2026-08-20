use clash_verge_logging::{Type, logging};
use serde_json::json;
use smartstring::alias::String;

use tauri::{AppHandle, Emitter as _, Manager as _, WebviewWindow};

#[cfg(not(target_os = "macos"))]
use std::sync::{OnceLock, mpsc};

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
        if crate::core::handle::Handle::global().is_exiting() {
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
        if crate::core::handle::Handle::global().is_exiting() {
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
