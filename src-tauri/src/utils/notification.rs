use std::borrow::Cow;

use crate::core::handle;
use clash_verge_i18n;
use tauri_plugin_notification::NotificationExt as _;

pub enum NotificationEvent<'a> {
    DashboardToggled,
    ClashModeChanged {
        mode: &'a str,
    },
    SystemProxyToggled(bool),
    TunModeToggled(bool),
    LightweightModeEntered,
    ProfilesReactivated,
    AppQuit,
    QuitCancelled,
    /// Системный прокси не снялся при выходе и остаётся в системе.
    SysproxyLeftBehind,
    #[cfg(target_os = "macos")]
    AppHidden,
    LogOpenFailed {
        path: &'a str,
    },
    // clod:F7 — subscription watcher alerts.
    SubExpired,
    SubExpiresIn {
        days: u32,
    },
    SubTraffic {
        percent: u32,
    },
}

fn notify(title: Cow<'_, str>, body: Cow<'_, str>) {
    let app_handle = handle::Handle::app_handle();
    app_handle.notification().builder().title(title).body(body).show().ok();
}

pub async fn notify_event<'a>(event: NotificationEvent<'a>) {
    match event {
        NotificationEvent::DashboardToggled => {
            let title = clash_verge_i18n::t!("notifications.dashboardToggled.title");
            let body = clash_verge_i18n::t!("notifications.dashboardToggled.body");
            notify(title, body);
        }
        NotificationEvent::ClashModeChanged { mode } => {
            let title = clash_verge_i18n::t!("notifications.clashModeChanged.title");
            let body = clash_verge_i18n::t!("notifications.clashModeChanged.body")
                .replace("{mode}", mode)
                .into();
            notify(title, body);
        }
        NotificationEvent::SystemProxyToggled(enabled) => {
            let title = clash_verge_i18n::t!("notifications.systemProxyToggled.title");
            let key = if enabled {
                "notifications.systemProxyToggled.on"
            } else {
                "notifications.systemProxyToggled.off"
            };

            let body = clash_verge_i18n::t!(key);
            notify(title, body);
        }
        NotificationEvent::TunModeToggled(enabled) => {
            let title = clash_verge_i18n::t!("notifications.tunModeToggled.title");
            let key = if enabled {
                "notifications.tunModeToggled.on"
            } else {
                "notifications.tunModeToggled.off"
            };
            let body = clash_verge_i18n::t!(key);
            notify(title, body);
        }
        NotificationEvent::LightweightModeEntered => {
            let title = clash_verge_i18n::t!("notifications.lightweightModeEntered.title");
            let body = clash_verge_i18n::t!("notifications.lightweightModeEntered.body");
            notify(title, body);
        }
        NotificationEvent::ProfilesReactivated => {
            let title = clash_verge_i18n::t!("notifications.profilesReactivated.title");
            let body = clash_verge_i18n::t!("notifications.profilesReactivated.body");
            notify(title, body);
        }
        NotificationEvent::AppQuit => {
            let title = clash_verge_i18n::t!("notifications.appQuit.title");
            let body = clash_verge_i18n::t!("notifications.appQuit.body");
            notify(title, body);
        }
        NotificationEvent::QuitCancelled => {
            let title = clash_verge_i18n::t!("notifications.quitCancelled.title");
            let body = clash_verge_i18n::t!("notifications.quitCancelled.body");
            notify(title, body);
        }
        NotificationEvent::SysproxyLeftBehind => {
            let title = clash_verge_i18n::t!("notifications.systemProxyToggled.title");
            let body = clash_verge_i18n::t!("notifications.sysproxyLeftBehind.body");
            notify(title, body);
        }
        #[cfg(target_os = "macos")]
        NotificationEvent::AppHidden => {
            let title = clash_verge_i18n::t!("notifications.appHidden.title");
            let body = clash_verge_i18n::t!("notifications.appHidden.body");
            notify(title, body);
        }
        NotificationEvent::LogOpenFailed { path } => {
            let title = clash_verge_i18n::t!("notifications.logOpenFailed.title");
            let body = clash_verge_i18n::t!("notifications.logOpenFailed.body")
                .replace("{path}", path)
                .into();
            notify(title, body);
        }
        // clod:F7
        NotificationEvent::SubExpired => {
            let title = clash_verge_i18n::t!("notifications.subExpired.title");
            let body = clash_verge_i18n::t!("notifications.subExpired.body");
            notify(title, body);
        }
        NotificationEvent::SubExpiresIn { days } => {
            let title = clash_verge_i18n::t!("notifications.subExpiresIn.title");
            let body = clash_verge_i18n::t!("notifications.subExpiresIn.body")
                .replace("{days}", &days.to_string())
                .into();
            notify(title, body);
        }
        NotificationEvent::SubTraffic { percent } => {
            let title = clash_verge_i18n::t!("notifications.subTraffic.title");
            let body = clash_verge_i18n::t!("notifications.subTraffic.body")
                .replace("{percent}", &percent.to_string())
                .into();
            notify(title, body);
        }
    }
}

#[allow(clippy::expect_used)]
#[cfg(test)]
mod tests {
    const LOCALES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../crates/clash-verge-i18n/locales");

    #[test]
    fn the_log_open_failure_speaks_every_language() {
        let mut checked = 0usize;
        for entry in std::fs::read_dir(LOCALES).expect("cannot read the locales directory") {
            let path = entry.expect("cannot read a locale entry").path();
            if path.extension().and_then(std::ffi::OsStr::to_str) != Some("yml") {
                continue;
            }
            let text = std::fs::read_to_string(&path).expect("cannot read a locale file");
            assert!(text.contains("logOpenFailed:"), "{}", path.display());
            assert!(text.contains("{path}"), "{}", path.display());
            checked += 1;
        }
        assert_eq!(checked, 13, "все 13 локалей обязаны нести ключ");
    }
}
