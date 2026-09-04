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

    /// clod: сколько ждать ответа на ПОВТОРНУЮ просьбу подготовить поколение.
    /// Служба фиксирует поколение ДО ответа, поэтому потерянный ответ ещё не
    /// значит, что подготовки не было: переспрашиваем, прежде чем менять
    /// мягкую перезагрузку на полный перезапуск ядра.
    pub const STAGE_CONFIRM_TIMEOUT: Duration = Duration::from_secs(5);

    // После отката к sidecar продолжаем ждать готовности службы и пытаться передать управление.
    pub const SERVICE_HANDOFF_WINDOW: Duration = Duration::from_secs(120);
    pub const SERVICE_HANDOFF_INTERVAL: Duration = Duration::from_secs(2);

    // clod:tun-ready — после поднятия TUN сверяем логи ядра, чтобы убедиться,
    // что устройство действительно создано.
    pub const TUN_VERIFY_DELAY: Duration = Duration::from_secs(3);

    // clod:tun-ready — the first check is not the last one. The core can fail
    // to bring the device up later than `TUN_VERIFY_DELAY` (on Windows the
    // adapter install and a foreign VPN driver routinely take longer), and a
    // live tunnel can die at any point afterwards. Under the service nobody
    // reads the core output continuously — only the sidecar does — so the
    // fact has to be re-checked on a timer for as long as TUN is claimed.
    pub const TUN_WATCH_INTERVAL: Duration = Duration::from_secs(30);

    pub const CORE_HEALTH_INTERVAL: Duration = Duration::from_secs(30);

    pub const CORE_HEALTH_MISSES: u32 = 2;

    pub const CORE_HEALTH_MAX_SKIPS: u32 = 4;

    pub const CORE_READY_ATTEMPTS: u32 = 40;
    pub const CORE_READY_INTERVAL: Duration = Duration::from_millis(200);
    pub const CORE_READY_PROBE_TIMEOUT: Duration = Duration::from_millis(500);

    pub const MIXED_PORT_CHECK_ATTEMPTS: u32 = 12;
    pub const MIXED_PORT_CHECK_INTERVAL: Duration = Duration::from_millis(500);

    // clod:wake-net — как часто сторож окружения сверяет часы и состав сети.
    // Круг стоит один опрос интерфейсов у системы и больше ничего, а платим мы
    // за него задержкой реакции: пятнадцать секунд с чужим системным прокси
    // после пробуждения — это пятнадцать секунд трафика мимо туннеля.
    pub const ENVIRONMENT_TICK: Duration = Duration::from_secs(15);
    pub const TUN_UNWANTED_SWEEP_EVERY_TICKS: u32 = 4;

    // clod:tun-ready — служба поднимается вместе с системой и при автозапуске
    // регулярно отстаёт от приложения. Прежде чем счесть её отсутствующей — а
    // это запрос прав и переустановка, — даём ей время появиться.
    pub const TUN_SERVICE_APPEAR_WAIT: Duration = Duration::from_secs(10);
    pub const TUN_SERVICE_APPEAR_INTERVAL: Duration = Duration::from_millis(500);

    // clod:tun-deadline — how long a reader of `ServiceManager::current()` may
    // wait for an operation that is already running. The deadline is counted
    // from the START of that operation, not from the call, so a caller that
    // asks in a loop (`wait_for_service_if_needed`) cannot stack the wait up.
    //
    // 5 s is the cost of the longest non-privileged operation we have —
    // `wait_for_service_ipc` is 20 × 250 ms — so a normal refresh/install still
    // reports its real result. Past that the operation is waiting on a human
    // (UAC / osascript), and the honest answer is the last known status: the
    // caller falls back to sidecar and the handoff watcher picks the service up
    // later, instead of the whole core lifecycle hanging on a modal dialog.
    pub const SERVICE_STATUS_WAIT: Duration = Duration::from_secs(5);

    // clod:tun-deadline — how long we wait for a privileged helper (UAC prompt
    // on Windows, `osascript … with administrator privileges` on macOS) before
    // we stop holding the rest of the app hostage. The helper is NOT killed:
    // it keeps running on its blocking thread, so a late "Allow" still installs
    // the service — we only stop awaiting it.
    //
    // 2 minutes is deliberately far longer than answering a dialog takes
    // (including typing a password on macOS): the timeout must mean "the dialog
    // has been abandoned", never "the user is slow", because expiring turns
    // into a visible error in the UI.
    pub const SERVICE_ELEVATION_WAIT: Duration = Duration::from_secs(120);

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
    pub const DNS_CHECK_CONFIG: &str = "dns_config-check.yaml";
    pub const WINDOW_STATE: &str = "window_state.json";
}

pub mod tun {
    pub const DEFAULT_STACK: &str = "gvisor";

    pub const DNS_HIJACK: &[&str] = &["any:53"];
}

pub mod policies {
    pub const BUILTIN: &[&str] = &["DIRECT", "REJECT", "REJECT-DROP", "PASS", "COMPATIBLE"];

    pub fn is_builtin(name: &str) -> bool {
        BUILTIN.contains(&name)
    }
}
