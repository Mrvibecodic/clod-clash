use std::time::Duration;

// clod:branding begin
/// Fork identity. Kept in one place so rebranding never leaks into feature code.
pub mod branding {
    /// Human readable product name (window titles, notifications).
    pub const APP_NAME: &str = "Clod Clash";

    /// Token used in the subscription `User-Agent`. Panels match it case
    /// sensitively in their SRR rules, so it must never be built at runtime
    /// from `APP_NAME`.
    pub const UA_TOKEN: &str = "ClodClash";

    /// Salt mixed into the raw machine id before hashing (see `utils::hwid`).
    pub const HWID_SALT: &str = "clod-clash";

    /// Short slug used for file names / desktop entries. Only the Linux
    /// scheme handler consumes it, so the cfg keeps mac/windows clippy
    /// (`-D dead-code`) quiet.
    #[cfg(target_os = "linux")]
    pub const APP_SLUG: &str = "clod-clash";
}
// clod:branding end

pub mod network {
    pub const DEFAULT_EXTERNAL_CONTROLLER: &str = "127.0.0.1:9097";

    pub mod ports {
        #[cfg(not(target_os = "windows"))]
        pub const DEFAULT_REDIR: u16 = 7895;
        #[cfg(target_os = "linux")]
        pub const DEFAULT_TPROXY: u16 = 7896;
        pub const DEFAULT_MIXED: u16 = 7897;
        pub const DEFAULT_SOCKS: u16 = 7898;
        pub const DEFAULT_HTTP: u16 = 7899;

        #[cfg(not(feature = "verge-dev"))]
        pub const SINGLETON_SERVER: u16 = 33331;
        #[cfg(feature = "verge-dev")]
        pub const SINGLETON_SERVER: u16 = 11233;
    }
}

pub mod timing {
    use super::Duration;

    pub const CONFIG_UPDATE_DEBOUNCE: Duration = Duration::from_millis(300);
    pub const STARTUP_ERROR_DELAY: Duration = Duration::from_secs(2);

    // Служба медленно запускается "с холода" (особенно на Windows), избегаем
    // слишком раннего отката к sidecar.
    // clod:tun-ready — launchd/systemd тоже могут быть готовы позже приложения,
    // ожидание больше не ограничено Windows.
    #[cfg(target_os = "windows")]
    pub const SERVICE_WAIT_MAX: Duration = Duration::from_millis(30000);
    #[cfg(not(target_os = "windows"))]
    pub const SERVICE_WAIT_MAX: Duration = Duration::from_millis(15000);
    pub const SERVICE_WAIT_INTERVAL: Duration = Duration::from_millis(200);

    // После отката к sidecar продолжаем ждать готовности службы и пытаться передать управление.
    pub const SERVICE_HANDOFF_WINDOW: Duration = Duration::from_secs(120);
    pub const SERVICE_HANDOFF_INTERVAL: Duration = Duration::from_secs(2);

    // clod:tun-ready — после поднятия TUN сверяем логи ядра, чтобы убедиться,
    // что устройство действительно создано.
    pub const TUN_VERIFY_DELAY: Duration = Duration::from_secs(3);

    // При передаче ждём, пока sidecar освободит канал ext-controller.
    #[cfg(target_os = "windows")]
    pub const SERVICE_START_RETRIES: usize = 5;
    #[cfg(target_os = "windows")]
    pub const SERVICE_START_RETRY_DELAY: Duration = Duration::from_millis(300);
}

pub mod files {
    pub const RUNTIME_CONFIG: &str = "clash-verge.yaml";
    pub const CHECK_CONFIG: &str = "clash-verge-check.yaml";
    pub const DNS_CONFIG: &str = "dns_config.yaml";
    pub const WINDOW_STATE: &str = "window_state.json";
}

pub mod tun {
    pub const DEFAULT_STACK: &str = "gvisor";

    pub const DNS_HIJACK: &[&str] = &["any:53"];
}
