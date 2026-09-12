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
static PENDING_NOTICES: Mutex<VecDeque<PendingNotice>> = Mutex::new(VecDeque::new());

#[derive(Debug, PartialEq, Eq)]
struct PendingNotice {
    status: std::string::String,
    message: std::string::String,
    repeats: u32,
}

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

fn pending_notices() -> std::sync::MutexGuard<'static, VecDeque<PendingNotice>> {
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
    "clod_core::",
    "update_failed",
    "update::",
    "app_quit::",
];
pub const EXIT_REFUSAL_STATUS: &str = "app_quit::in_progress";
const NEVER_HELD_STATUSES: &[&str] = &["tun::setup_started", "tun::setup_done", EXIT_REFUSAL_STATUS];

fn pointless_to_hold(status: &str) -> bool {
    NEVER_HELD_STATUSES.contains(&status)
}

fn worth_holding(status: &str) -> bool {
    !pointless_to_hold(status) && HELD_STATUS_PREFIXES.iter().any(|prefix| status.starts_with(prefix))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Delivery {
    Send,
    Hold,
    Drop,
}

fn decide_delivery(status: &str, listening: bool, window_exists: bool) -> Delivery {
    if can_reach_the_page(listening, window_exists) {
        return Delivery::Send;
    }
    if window_exists {
        if worth_holding(status) {
            return Delivery::Hold;
        }
        return Delivery::Send;
    }
    if pointless_to_hold(status) {
        return Delivery::Drop;
    }
    Delivery::Hold
}

fn collapse_into(pending: &mut VecDeque<PendingNotice>, status: &str, message: &str) -> u32 {
    if let Some(same) = pending.iter_mut().find(|notice| notice.status == status) {
        same.message = message.to_owned();
        same.repeats = same.repeats.saturating_add(1);
        return same.repeats;
    }
    if pending.len() >= PENDING_NOTICES_CAP
        && let Some(evicted) = pending.pop_front()
    {
        drop_notice(&evicted.status, "очередь заполнена, вытеснено более свежим");
    }
    pending.push_back(PendingNotice {
        status: status.to_owned(),
        message: message.to_owned(),
        repeats: 1,
    });
    1
}

fn hold_notice(status: &str, message: &str) {
    let (repeats, queued) = {
        let mut pending = pending_notices();
        let repeats = collapse_into(&mut pending, status, message);
        (repeats, pending.len())
    };
    logging!(
        info,
        Type::Frontend,
        "страница не слушает — уведомление {} отложено до её появления ({} в очереди, повторов: {})",
        status,
        queued,
        repeats
    );
}

fn drop_notice(status: &str, why: &str) {
    logging!(info, Type::Frontend, "уведомление {} выброшено: {}", status, why);
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
    pub fn take_pending_notices() -> Vec<(std::string::String, std::string::String, u32)> {
        FRONTEND_LISTENING.store(true, Ordering::Release);
        pending_notices()
            .drain(..)
            .map(|notice| (notice.status, notice.message, notice.repeats))
            .collect()
    }

    pub fn speaks_to_a_window_that_is_still_up(event: &FrontendEvent) -> bool {
        matches!(event, FrontendEvent::NoticeMessage { status, .. } if *status == EXIT_REFUSAL_STATUS)
    }

    pub fn hold_in_case_the_exit_is_cancelled(event: &FrontendEvent) {
        if let FrontendEvent::NoticeMessage { status, message } = event {
            if worth_holding(status) {
                hold_notice(status, message);
            } else {
                drop_notice(status, "идёт выход, показать его уже некому");
            }
        }
    }

    pub fn lost_to_an_exit_that_cannot_be_cancelled(event: &FrontendEvent) {
        if let FrontendEvent::NoticeMessage { status, .. } = event {
            drop_notice(status, "выход уже не отменить, придерживать его незачем");
        }
    }

    fn held_for_later(app_handle: &AppHandle, event: &FrontendEvent) -> bool {
        let FrontendEvent::NoticeMessage { status, message } = event else {
            return false;
        };
        let window_exists = app_handle.get_webview_window("main").is_some();
        match decide_delivery(status, FRONTEND_LISTENING.load(Ordering::Acquire), window_exists) {
            Delivery::Send => false,
            Delivery::Hold => {
                hold_notice(status, message);
                true
            }
            Delivery::Drop => {
                drop_notice(status, "окна нет, а придерживать его нет смысла");
                true
            }
        }
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
    use super::{
        Delivery, FrontendEvent, NEVER_HELD_STATUSES, NotificationSystem, PENDING_NOTICES_CAP, PendingNotice,
        can_reach_the_page, collapse_into, decide_delivery, worth_holding,
    };
    use std::collections::{BTreeSet, VecDeque};
    use std::path::{Path, PathBuf};

    const NOTICE_STATUSES: &[&str] = &[
        "app_quit::core_still_running",
        "app_quit::in_progress",
        "clod_config::load_failed",
        "clod_core::update_available",
        "clod_core::updated",
        "clod_sub::fallback_used",
        "clod_sub::url_migrated",
        "config_core::change_error",
        "config_core::change_success",
        "config_validate::boot_error",
        "config_validate::error",
        "config_validate::file_not_found",
        "config_validate::merge_mapping_error",
        "config_validate::merge_syntax_error",
        "config_validate::process_terminated",
        "config_validate::script_error",
        "config_validate::script_missing_main",
        "config_validate::script_syntax_error",
        "config_validate::timeout",
        "config_validate::yaml_mapping_error",
        "config_validate::yaml_read_error",
        "config_validate::yaml_syntax_error",
        "core::binary_changed",
        "core::crashed",
        "core::handoff_failed",
        "core::not_ready",
        "core::port_busy",
        "core::restarted",
        "import_sub_url::error",
        "import_sub_url::ok",
        "reactivate_profiles::error",
        "service::bundle_rejected",
        "service::needs_repair",
        "set_config::error",
        "set_config::ok",
        "sysproxy::core_gave_up",
        "sysproxy::core_not_running",
        "sysproxy::write_failed",
        "tun::adapter_busy",
        "tun::no_rights",
        "tun::no_traffic",
        "tun::rights_declined",
        "tun::service_silent",
        "tun::setup_done",
        "tun::setup_failed",
        "tun::setup_started",
        "tun::start_failed",
        "update::breaking_changes",
        "update_failed",
        "update_with_clash_proxy",
    ];
    const COMMAND_MARKERS: &[&str] = &["tun::setup_busy", "tun::setup_pending"];

    fn rust_sources(dir: &Path, found: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                rust_sources(&path, found);
            } else if path.extension().and_then(std::ffi::OsStr::to_str) == Some("rs") {
                found.push(path);
            }
        }
    }

    fn plain_word(part: &str) -> bool {
        !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_lowercase() || byte == b'_')
    }

    fn status_shaped(literal: &str) -> bool {
        match literal.split_once("::") {
            Some((family, name)) => plain_word(family) && plain_word(name),
            None => plain_word(literal),
        }
    }

    fn literals_after(source: &str, marker: &str) -> Vec<String> {
        source
            .match_indices(marker)
            .filter_map(|(at, _)| {
                let rest = source[at + marker.len()..].trim_start();
                rest.strip_prefix('"')?.split('"').next().map(str::to_owned)
            })
            .filter(|literal| status_shaped(literal))
            .collect()
    }

    /// Статусы, которые код шлёт по имени константы, а не литералом: для теста
    /// покрытия использование константы — такая же отправка, как и литерал,
    /// иначе приведение последнего литерала к константе красит сборку.
    const STATUS_CONSTANTS: &[(&str, &str)] = &[("EXIT_REFUSAL_STATUS", super::EXIT_REFUSAL_STATUS)];

    fn statuses_in(source: &str) -> Vec<String> {
        let mut found = literals_after(source, "notice_message(");
        for (name, status) in STATUS_CONSTANTS {
            if source.contains(name) {
                found.push((*status).to_owned());
            }
        }
        for line in source.lines() {
            if line.trim_start().starts_with("//") {
                continue;
            }
            found.extend(
                line.split('"')
                    .skip(1)
                    .step_by(2)
                    .filter(|piece| piece.contains("::") && status_shaped(piece))
                    .map(str::to_owned),
            );
        }
        found
    }

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
        assert!(worth_holding("update::breaking_changes"));
        assert!(worth_holding("clod_core::updated"));
        assert!(!worth_holding("tun::setup_done"));
        assert!(!worth_holding("app_quit::in_progress"));
        assert!(!worth_holding("set_config::ok"));
        assert!(!worth_holding("clod_sub::url_migrated"));
    }

    #[test]
    fn without_a_window_a_notice_waits_unless_it_describes_this_very_moment() {
        for status in NOTICE_STATUSES {
            let expected = if NEVER_HELD_STATUSES.contains(status) {
                Delivery::Drop
            } else {
                Delivery::Hold
            };
            assert_eq!(
                decide_delivery(status, false, false),
                expected,
                "статус {status} при отсутствии окна"
            );
        }
    }

    #[test]
    fn the_tray_the_hotkey_and_a_system_link_keep_their_answer() {
        for status in [
            "set_config::error",
            "set_config::ok",
            "reactivate_profiles::error",
            "import_sub_url::ok",
            "import_sub_url::error",
            "config_core::change_error",
            "clod_sub::fallback_used",
            "update_with_clash_proxy",
        ] {
            assert_eq!(
                decide_delivery(status, false, false),
                Delivery::Hold,
                "{status} потерян молча"
            );
        }
    }

    #[test]
    fn a_live_but_hidden_page_still_gets_what_is_not_worth_waiting_for() {
        assert_eq!(decide_delivery("core::crashed", true, true), Delivery::Send);
        assert_eq!(decide_delivery("core::crashed", false, true), Delivery::Hold);
        assert_eq!(decide_delivery("set_config::ok", false, true), Delivery::Send);
        assert_eq!(decide_delivery("tun::setup_started", false, true), Delivery::Send);
    }

    #[test]
    fn the_refusal_of_a_tray_click_goes_round_the_exit_fork() {
        let refusal = FrontendEvent::NoticeMessage {
            status: "app_quit::in_progress",
            message: super::String::new(),
        };
        assert!(NotificationSystem::speaks_to_a_window_that_is_still_up(&refusal));
        let problem = FrontendEvent::NoticeMessage {
            status: "core::crashed",
            message: super::String::new(),
        };
        assert!(!NotificationSystem::speaks_to_a_window_that_is_still_up(&problem));
        assert!(!NotificationSystem::speaks_to_a_window_that_is_still_up(
            &FrontendEvent::RefreshClash
        ));
    }

    #[test]
    fn the_boot_fallback_leaves_the_notice_to_its_caller() {
        let source = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/core/manager/config.rs"))
            .unwrap_or_default();
        let body = fn_body(&source, "async fn use_default_config");
        assert!(
            body.is_some_and(|body| !body.contains("notice_message")),
            "запасной конфиг снова шлёт отказ сам — к уведомлению из init_runtime_config добавится второе"
        );
    }

    fn fn_body<'a>(source: &'a str, signature: &str) -> Option<&'a str> {
        let at = source.find(signature)?;
        let rest = &source[at..];
        let open = rest.find('{')?;
        let mut depth = 0usize;
        for (index, byte) in rest.bytes().enumerate().skip(open) {
            match byte {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(&rest[open..=index]);
                    }
                }
                _ => {}
            }
        }
        None
    }

    #[test]
    fn a_repeated_status_grows_a_counter_instead_of_the_queue() {
        let mut pending = VecDeque::new();
        for attempt in 1..=20 {
            assert_eq!(collapse_into(&mut pending, "core::crashed", "код выхода 1"), attempt);
        }
        assert_eq!(collapse_into(&mut pending, "core::crashed", "код выхода 2"), 21);
        assert_eq!(collapse_into(&mut pending, "sysproxy::write_failed", ""), 1);
        assert_eq!(
            pending.into_iter().collect::<Vec<_>>(),
            vec![
                PendingNotice {
                    status: "core::crashed".to_owned(),
                    message: "код выхода 2".to_owned(),
                    repeats: 21,
                },
                PendingNotice {
                    status: "sysproxy::write_failed".to_owned(),
                    message: String::new(),
                    repeats: 1,
                },
            ]
        );
    }

    #[test]
    fn the_queue_still_drops_the_oldest_distinct_status_when_full() {
        let mut pending = VecDeque::new();
        for index in 0..=PENDING_NOTICES_CAP {
            collapse_into(&mut pending, &format!("core::status_{index}"), "");
        }
        assert_eq!(pending.len(), PENDING_NOTICES_CAP);
        assert_eq!(
            pending.front().map(|notice| notice.status.as_str()),
            Some("core::status_1")
        );
    }

    #[test]
    fn every_status_the_backend_sends_is_declared_once() {
        let mut files = Vec::new();
        rust_sources(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/src")), &mut files);
        assert!(!files.is_empty(), "исходники не найдены");
        let mut seen = BTreeSet::new();
        for path in files.iter().filter(|path| !path.ends_with("core/notification.rs")) {
            let source = std::fs::read_to_string(path).unwrap_or_default();
            for status in statuses_in(&source) {
                assert!(
                    NOTICE_STATUSES.contains(&status.as_str()) || COMMAND_MARKERS.contains(&status.as_str()),
                    "{}: статус {status} не объявлен в NOTICE_STATUSES",
                    path.display()
                );
                seen.insert(status);
            }
        }
        for status in NOTICE_STATUSES {
            assert!(seen.contains(*status), "статус {status} объявлен, но никто его не шлёт");
        }
    }
}
