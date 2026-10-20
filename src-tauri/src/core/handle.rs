use crate::{APP_HANDLE, singleton};
use smartstring::alias::String;
use std::sync::atomic::{AtomicU8, Ordering};
use tauri::AppHandle;
use tauri_plugin_mihomo::{Mihomo, MihomoExt as _};

use super::notification::{FrontendEvent, NotificationSystem};

/// Выход — одно состояние, а не два флага: «отменяемый выход» без выхода
/// непредставим, и отмена с просьбой системы уйти не разминутся.
const NOT_EXITING: u8 = 0;
const EXITING_CANCELLABLE: u8 = 1;
const EXITING_FOR_GOOD: u8 = 2;

#[derive(Debug)]
pub struct Handle {
    exit: AtomicU8,
}

impl Default for Handle {
    fn default() -> Self {
        Self {
            exit: AtomicU8::new(NOT_EXITING),
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

    pub fn mihomo() -> &'static Mihomo {
        Self::app_handle().mihomo()
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

    /// clod:freeze — пометки «режется» изменились.
    pub fn refresh_freeze_marks() {
        Self::send_event(FrontendEvent::RefreshFreezeMarks);
    }

    /// clod:chan — ход добавления подписки для окна добавления.
    pub fn add_stage(stage: &str, attempt: u8) {
        Self::send_event(FrontendEvent::AddStage { stage, attempt });
    }

    pub fn notify_profile_changed(profile_id: &String) {
        Self::send_event(FrontendEvent::ProfileChanged {
            current_profile_id: profile_id,
        });
    }

    pub fn notify_timer_updated(profile_index: &String) {
        Self::send_event(FrontendEvent::TimerUpdated { profile_index });
    }

    /// clod: набор обновляющихся подписок сменился — окну снимок целиком.
    pub fn notify_updates_in_flight(snapshot: &crate::feat::UpdatesInFlight) {
        Self::send_event(FrontendEvent::UpdatesInFlight { snapshot });
    }

    /// clod: картинка подписки (`logo` / `background`) на диске сменилась.
    pub fn notify_profile_picture(uid: &str, picture: &'static str) {
        Self::send_event(FrontendEvent::ProfilePicture { uid, picture });
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
    pub fn hwid_notice(uid: &str, payload: serde_json::Value) {
        if NotificationSystem::keep_device_refusal_until_seen(Self::app_handle(), uid) {
            return;
        }
        Self::send_event(FrontendEvent::HwidNotice { payload });
    }
    // clod:hwid end

    pub fn set_is_exiting(&self) {
        self.exit.store(EXITING_FOR_GOOD, Ordering::Release);
    }

    pub fn begin_exiting(&self, can_be_cancelled: bool) -> bool {
        let state = if can_be_cancelled {
            EXITING_CANCELLABLE
        } else {
            EXITING_FOR_GOOD
        };
        self.exit
            .compare_exchange(NOT_EXITING, state, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    /// Выход, которого просит система: начать его или сделать неотменяемым
    /// уже идущий. `true` — начат здесь.
    pub fn begin_exiting_for_good(&self) -> bool {
        loop {
            match self
                .exit
                .compare_exchange(NOT_EXITING, EXITING_FOR_GOOD, Ordering::AcqRel, Ordering::Acquire)
            {
                Ok(_) => return true,
                Err(EXITING_FOR_GOOD) => return false,
                Err(_) => {
                    if self
                        .exit
                        .compare_exchange(
                            EXITING_CANCELLABLE,
                            EXITING_FOR_GOOD,
                            Ordering::AcqRel,
                            Ordering::Acquire,
                        )
                        .is_ok()
                    {
                        return false;
                    }
                }
            }
        }
    }

    /// Отменить выход, если он ещё отменяемый. `false` — его уже сделала
    /// неотменяемым просьба системы.
    pub fn call_off_the_exit(&self) -> bool {
        self.exit
            .compare_exchange(EXITING_CANCELLABLE, NOT_EXITING, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    #[cfg(target_os = "windows")]
    pub fn clear_is_exiting(&self) {
        self.exit.store(NOT_EXITING, Ordering::Release);
    }

    pub fn is_exiting(&self) -> bool {
        self.exit.load(Ordering::Acquire) != NOT_EXITING
    }

    pub fn exit_can_be_cancelled(&self) -> bool {
        self.exit.load(Ordering::Acquire) == EXITING_CANCELLABLE
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
    use super::Handle;

    /// Система просит уйти, пока идёт выход, который отменился бы при живом
    /// ядре: он становится неотменяемым, и просьба не теряется.
    #[test]
    fn a_signal_during_a_cancellable_exit_makes_it_final() {
        let handle = Handle::new();
        assert!(handle.begin_exiting(true));
        assert!(!handle.begin_exiting_for_good());
        assert!(!handle.call_off_the_exit());
        assert!(handle.is_exiting());
        assert!(!handle.exit_can_be_cancelled());
    }

    #[test]
    fn an_exit_called_off_lets_the_next_request_begin_a_new_one() {
        let handle = Handle::new();
        assert!(handle.begin_exiting(true));
        assert!(handle.call_off_the_exit());
        assert!(!handle.is_exiting());
        assert!(handle.begin_exiting_for_good());
        assert!(!handle.call_off_the_exit());
        assert!(!handle.begin_exiting(true));
    }
}
