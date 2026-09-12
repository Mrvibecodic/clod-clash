use crate::{APP_HANDLE, singleton};
use arc_swap::ArcSwapOption;
use clash_verge_logging::{Type, logging};
use smartstring::alias::String;
use std::sync::{
    Arc, LazyLock,
    atomic::{AtomicBool, Ordering},
};
use tauri::AppHandle;
use tauri_plugin_mihomo::{IpcConnectionPool, Mihomo, MihomoExt as _};
use tokio::sync::RwLock;

use super::notification::{FrontendEvent, NotificationSystem};

static CORE_CLIENT: LazyLock<ArcSwapOption<Mihomo>> = LazyLock::new(ArcSwapOption::empty);

fn respun(source: &Mihomo, socket_path: Option<std::string::String>) -> Mihomo {
    Mihomo {
        protocol: source.protocol.clone(),
        external_host: source.external_host.clone(),
        external_port: source.external_port,
        secret: source.secret.clone(),
        socket_path: socket_path.or_else(|| source.socket_path.clone()),
        connection_manager: Arc::clone(&source.connection_manager),
    }
}

pub(crate) async fn publish_core_client(
    mirror: &RwLock<Mihomo>,
    socket_path: Option<std::string::String>,
) -> Arc<Mihomo> {
    let socket_path_changed = socket_path.is_some();

    let next = match CORE_CLIENT.load_full() {
        Some(current) => respun(&current, socket_path),
        None => {
            let live = mirror.read().await;
            respun(&live, socket_path)
        }
    };
    let next = Arc::new(next);
    CORE_CLIENT.store(Some(Arc::clone(&next)));

    if socket_path_changed {
        match IpcConnectionPool::global() {
            Ok(pool) => pool.clear_pool(),
            Err(err) => logging!(warn, Type::Core, "пул соединений ядра не очищен: {}", err),
        }
    }

    next
}

#[derive(Debug)]
pub struct Handle {
    is_exiting: AtomicBool,
    exit_can_be_cancelled: AtomicBool,
}

impl Default for Handle {
    fn default() -> Self {
        Self {
            is_exiting: AtomicBool::new(false),
            exit_can_be_cancelled: AtomicBool::new(false),
        }
    }
}

singleton!(Handle, HANDLE);

impl Handle {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn app_handle() -> &'static AppHandle {
        #[allow(clippy::expect_used)]
        APP_HANDLE.get().expect("App handle not initialized")
    }

    pub async fn mihomo() -> Arc<Mihomo> {
        match CORE_CLIENT.load_full() {
            Some(client) => client,
            None => publish_core_client(Self::app_handle().mihomo(), None).await,
        }
    }

    pub fn refresh_clash() {
        Self::send_event(FrontendEvent::RefreshClash);
    }

    pub fn refresh_verge() {
        Self::send_event(FrontendEvent::RefreshVerge);
    }

    pub fn refresh_profiles() {
        Self::send_event(FrontendEvent::RefreshProfiles);
    }

    /// clod: то же событие, что фронт слушает после ручного выбора узла.
    /// Трей слал его сырым `emit` из рабочего потока — а именно от этого
    /// `NotificationSystem` и страхует: отправка из воркера умеет схлопнуться
    /// в дедлок с обработчиком WebKit, потому что обе стороны ждут друг друга.
    pub fn refresh_proxy_config() {
        Self::send_event(FrontendEvent::RefreshProxyConfig);
    }

    /// clod: ход обновления ядра — тот же путь через главный поток.
    pub fn notify_core_update_progress(payload: serde_json::Value) {
        Self::send_event(FrontendEvent::CoreUpdateProgress { payload });
    }

    pub fn notify_profile_changed(profile_id: &String) {
        Self::send_event(FrontendEvent::ProfileChanged {
            current_profile_id: profile_id,
        });
    }

    pub fn notify_timer_updated(profile_index: &String) {
        Self::send_event(FrontendEvent::TimerUpdated { profile_index });
    }

    pub fn notify_profile_update_started(uid: &String) {
        Self::send_event(FrontendEvent::ProfileUpdateStarted { uid });
    }

    pub fn notify_profile_update_completed(uid: &String) {
        Self::send_event(FrontendEvent::ProfileUpdateCompleted { uid });
    }

    pub fn notice_message<S: AsRef<str>, M: Into<String>>(status: S, msg: M) {
        let status_str = status.as_ref();
        let msg_str = msg.into();

        Self::send_event(FrontendEvent::NoticeMessage {
            status: status_str,
            message: msg_str,
        });
    }

    // clod:hwid begin
    /// Tell the UI what the panel said about this device.
    pub fn hwid_notice(payload: serde_json::Value) {
        Self::send_event(FrontendEvent::HwidNotice { payload });
    }
    // clod:hwid end

    pub fn set_is_exiting(&self) {
        self.exit_can_be_cancelled.store(false, Ordering::Release);
        self.is_exiting.store(true, Ordering::Release);
    }

    pub fn begin_exiting(&self, can_be_cancelled: bool) -> bool {
        if self.is_exiting.swap(true, Ordering::AcqRel) {
            return false;
        }
        self.exit_can_be_cancelled.store(can_be_cancelled, Ordering::Release);
        true
    }

    pub fn clear_is_exiting(&self) {
        self.exit_can_be_cancelled.store(false, Ordering::Release);
        self.is_exiting.store(false, Ordering::Release);
    }

    pub fn is_exiting(&self) -> bool {
        self.is_exiting.load(Ordering::Acquire)
    }

    pub fn exit_can_be_cancelled(&self) -> bool {
        self.exit_can_be_cancelled.load(Ordering::Acquire)
    }

    fn send_event(event: FrontendEvent) {
        let handle = Self::global();
        if handle.is_exiting() && !NotificationSystem::speaks_to_a_window_that_is_still_up(&event) {
            if handle.exit_can_be_cancelled() {
                NotificationSystem::hold_in_case_the_exit_is_cancelled(&event);
            } else {
                NotificationSystem::lost_to_an_exit_that_cannot_be_cancelled(&event);
            }
            return;
        }

        NotificationSystem::send_event(Self::app_handle().clone(), event);
    }
}

#[cfg(target_os = "macos")]
impl Handle {
    pub fn set_activation_policy(&self, policy: tauri::ActivationPolicy) -> Result<(), String> {
        Self::app_handle()
            .set_activation_policy(policy)
            .map_err(|e| e.to_string().into())
    }

    pub fn set_activation_policy_regular(&self) {
        let _ = self.set_activation_policy(tauri::ActivationPolicy::Regular);
    }

    pub fn set_activation_policy_accessory(&self) {
        let _ = self.set_activation_policy(tauri::ActivationPolicy::Accessory);
    }
}

#[cfg(test)]
mod tests {
    use super::{CORE_CLIENT, publish_core_client, respun};
    use std::{sync::Arc, time::Duration};
    use tauri_plugin_mihomo::{Mihomo, models::Protocol};
    use tokio::sync::RwLock;

    const NO_WAIT: Duration = Duration::from_millis(200);

    fn seed(socket_path: &str) -> Mihomo {
        Mihomo {
            protocol: Protocol::LocalSocket,
            external_host: None,
            external_port: None,
            secret: None,
            socket_path: Some(socket_path.to_owned()),
            connection_manager: Default::default(),
        }
    }

    #[test]
    fn a_respun_client_keeps_the_live_websocket_registry() {
        let source = seed("/old");
        let next = respun(&source, Some("/new".to_owned()));

        assert_eq!(next.socket_path.as_deref(), Some("/new"));
        assert!(Arc::ptr_eq(&next.connection_manager, &source.connection_manager));
    }

    #[tokio::test]
    async fn a_new_socket_path_is_published_while_the_mirror_is_busy() {
        CORE_CLIENT.store(None);
        let mirror = RwLock::new(seed("/old"));

        let seeded = publish_core_client(&mirror, None).await;
        assert_eq!(seeded.socket_path.as_deref(), Some("/old"));

        let plugin_command = mirror.read().await;

        let published = tokio::time::timeout(NO_WAIT, publish_core_client(&mirror, Some("/new".to_owned())))
            .await
            .ok();

        assert!(
            published
                .as_ref()
                .is_some_and(|client| client.socket_path.as_deref() == Some("/new")),
            "публикация снимка не должна ждать зеркало"
        );
        assert!(
            published.is_some_and(|client| Arc::ptr_eq(&client.connection_manager, &seeded.connection_manager)),
            "снимок обязан переиспользовать прежний менеджер соединений"
        );

        let read_back = CORE_CLIENT.load_full();
        assert_eq!(
            read_back.as_deref().and_then(|client| client.socket_path.as_deref()),
            Some("/new")
        );

        drop(plugin_command);
        CORE_CLIENT.store(None);
    }
}
