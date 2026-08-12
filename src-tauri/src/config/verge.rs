use crate::config::Config;
use crate::{
    config::{DEFAULT_PAC, deserialize_encrypted, serialize_encrypted},
    utils::{dirs, help},
};
use anyhow::Result;
use clash_verge_logging::{Type, logging};
use log::LevelFilter;
use serde::{Deserialize, Serialize};
use smartstring::alias::String;

/// ### `verge.yaml` schema
#[derive(Default, Debug, Clone, Deserialize, Serialize)]
pub struct IVerge {
    /// app log level
    /// silent | error | warn | info | debug | trace
    pub app_log_level: Option<String>,

    /// app log max size in KB
    pub app_log_max_size: Option<u64>,

    /// app log max count
    pub app_log_max_count: Option<usize>,

    // i18n
    pub language: Option<String>,

    /// `light` or `dark` or `system`
    pub theme_mode: Option<String>,

    /// tray click event
    pub tray_event: Option<String>,

    /// copy env type
    pub env_type: Option<String>,

    /// start page
    pub start_page: Option<String>,
    /// startup script path
    pub startup_script: Option<String>,

    /// enable group icon
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable_group_icon: Option<bool>,

    /// pause render traffic stats on blur
    pub pause_render_traffic_stats_on_blur: Option<bool>,

    /// common tray icon
    #[serde(skip_serializing_if = "Option::is_none")]
    pub common_tray_icon: Option<bool>,

    /// tray icon
    #[cfg(target_os = "macos")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tray_icon: Option<String>,

    /// menu icon
    #[serde(skip_serializing_if = "Option::is_none")]
    pub menu_icon: Option<String>,

    /// menu order
    #[serde(skip_serializing_if = "Option::is_none")]
    pub menu_order: Option<Vec<String>>,

    /// toast / notice position on screen
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notice_position: Option<String>,

    /// collapse navigation bar
    pub collapse_navbar: Option<bool>,

    /// sysproxy tray icon
    pub sysproxy_tray_icon: Option<bool>,

    /// tun tray icon
    pub tun_tray_icon: Option<bool>,

    /// clash tun mode
    pub enable_tun_mode: Option<bool>,

    /// can the app auto startup
    pub enable_auto_launch: Option<bool>,

    /// not show the window on launch
    pub enable_silent_start: Option<bool>,

    /// set system proxy
    pub enable_system_proxy: Option<bool>,

    /// enable proxy guard
    pub enable_proxy_guard: Option<bool>,

    /// enable bypass format check
    pub enable_bypass_check: Option<bool>,

    /// enable dns settings - this controls whether dns_config.yaml is applied
    pub enable_dns_settings: Option<bool>,

    /// always use default bypass
    pub use_default_bypass: Option<bool>,

    /// set system proxy bypass
    pub system_proxy_bypass: Option<String>,

    /// proxy guard duration
    pub proxy_guard_duration: Option<u64>,

    /// use pac mode
    pub proxy_auto_config: Option<bool>,

    /// pac script content
    pub pac_file_content: Option<String>,

    /// proxy host address
    pub proxy_host: Option<String>,

    /// theme setting
    pub theme_setting: Option<IVergeTheme>,

    /// web ui list
    pub web_ui_list: Option<Vec<String>>,

    /// clash core path
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clash_core: Option<String>,

    /// hotkey map
    /// format: {func},{key}
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hotkeys: Option<Vec<String>>,

    /// enable global hotkey
    pub enable_global_hotkey: Option<bool>,

    /// Настройки карточек на главной странице
    /// Управляет показом и скрытием отдельных карточек на главной странице
    pub home_cards: Option<serde_json::Value>,

    /// Автоматически закрывать соединения при переключении прокси
    pub auto_close_connection: Option<bool>,

    /// Автоматически проверять обновления
    pub auto_check_update: Option<bool>,

    /// clod:prereleases — принимать ли предварительные сборки (`-alpha`,
    /// `-beta`, `-rc`). По умолчанию `true`: сегодня ВСЕ наши релизы
    /// предварительные, и выключенный флаг означал бы «обновлений нет».
    pub receive_prereleases: Option<bool>,

    /// Соединение для теста задержки по умолчанию
    pub default_latency_test: Option<String>,

    /// Таймаут теста задержки по умолчанию
    pub default_latency_timeout: Option<i16>,

    /// Использовать встроенную поддержку скриптов, по умолчанию включено
    pub enable_builtin_enhanced: Option<bool>,

    /// Число колонок в раскладке страницы прокси
    pub proxy_layout_column: Option<u8>,

    /// Список тестовых сайтов
    pub test_list: Option<Vec<IVergeTestItem>>,

    /// Очистка логов
    /// 0: не очищать; 1: 1 день; 2: 7 дней; 3: 30 дней; 4: 90 дней
    pub auto_log_clean: Option<i32>,

    /// Enable scheduled automatic backups
    pub enable_auto_backup_schedule: Option<bool>,

    /// Automatic backup interval in hours
    pub auto_backup_interval_hours: Option<u64>,

    /// Create backups automatically when critical configs change
    pub auto_backup_on_change: Option<bool>,

    /// Различные порты verge для переопределения портов clash
    #[cfg(not(target_os = "windows"))]
    pub verge_redir_port: Option<u16>,

    #[cfg(not(target_os = "windows"))]
    pub verge_redir_enabled: Option<bool>,

    #[cfg(target_os = "linux")]
    pub verge_tproxy_port: Option<u16>,

    #[cfg(target_os = "linux")]
    pub verge_tproxy_enabled: Option<bool>,

    pub verge_mixed_port: Option<u16>,

    pub verge_socks_port: Option<u16>,

    pub verge_socks_enabled: Option<bool>,

    pub verge_port: Option<u16>,

    pub verge_http_enabled: Option<bool>,

    /// Конфигурация WebDAV (хранится зашифрованной)
    #[serde(
        serialize_with = "serialize_encrypted",
        deserialize_with = "deserialize_encrypted",
        skip_serializing_if = "Option::is_none",
        default
    )]
    pub webdav_url: Option<String>,

    /// Имя пользователя WebDAV (хранится зашифрованным)
    #[serde(
        serialize_with = "serialize_encrypted",
        deserialize_with = "deserialize_encrypted",
        skip_serializing_if = "Option::is_none",
        default
    )]
    pub webdav_username: Option<String>,

    /// Пароль WebDAV (хранится зашифрованным)
    #[serde(
        serialize_with = "serialize_encrypted",
        deserialize_with = "deserialize_encrypted",
        skip_serializing_if = "Option::is_none",
        default
    )]
    pub webdav_password: Option<String>,

    #[cfg(target_os = "macos")]
    pub enable_tray_speed: Option<bool>,

    // pub enable_tray_icon: Option<bool>,
    /// show proxy groups directly on tray root menu
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tray_proxy_groups_display_mode: Option<String>,
    /// show outbound modes directly on tray root menu
    pub tray_inline_outbound_modes: Option<bool>,

    /// Автоматически переходить в лёгкий режим
    pub enable_auto_light_weight_mode: Option<bool>,

    /// Задержка автоперехода в лёгкий режим (в минутах)
    pub auto_light_weight_minutes: Option<u64>,

    /// Включить автопрокрутку на странице прокси
    pub enable_hover_jump_navigator: Option<bool>,

    /// Задержка автопрокрутки на странице прокси (в миллисекундах)
    pub hover_jump_navigator_delay: Option<u64>,

    /// Включить внешний контроллер
    pub enable_external_controller: Option<bool>,

    // clod:hwid begin
    /// Send the device identity headers (`x-hwid` family) with subscription
    /// requests. Panels with a device limit require them.
    pub enable_hwid: Option<bool>,

    /// Cached device id. Computed once on first use so the value stays stable
    /// even if the underlying machine-id source changes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hwid: Option<String>,
    // clod:hwid end

    // clod:simple-mode begin
    /// Simplified interface: only the home screen and a reduced settings page.
    ///
    /// `None` means the user never chose, so the provider's `clod-simple-mode`
    /// header decides; with neither, the simple mode is on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub simple_mode: Option<bool>,

    /// The Connect button drives the system proxy. On by default; system proxy
    /// and TUN are independent and may be active at the same time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connect_system_proxy: Option<bool>,

    /// The Connect button also drives the TUN device. Off by default.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connect_tun_mode: Option<bool>,

    /// clod: повторять нажатие кнопки «Подключиться» сразу после запуска.
    /// Выключено по умолчанию, и это осознанно: раньше подключение
    /// восстанавливалось само по флагам «поднято сейчас», переживавшим
    /// перезапуск, — клиент прописывал системный прокси в настройки ОС ещё до
    /// того, как показалось окно. Кому нужен автомат, включает его сам.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connect_on_launch: Option<bool>,

    /// clod:tun-ready — the app version whose automatic service setup we have
    /// already attempted: turned down, failed, or reported as done. While it
    /// matches the running version we never raise the elevation prompt on our
    /// own again — an installer that keeps claiming success while the service
    /// stays silent would otherwise ask on every single launch. The TUN switch
    /// still asks, because then the user asked for it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tun_setup_declined: Option<String>,

    /// Legacy field of earlier builds (`sysproxy`/`tun`). Read only for the
    /// migration in [`Self::new`], never written back; without it a user who
    /// picked TUN would be silently downgraded to the system proxy.
    #[serde(skip_serializing)]
    pub main_switch_mode: Option<String>,

    /// Last window size (logical `[width, height]`) the user had in the
    /// simple mode. Each mode remembers its own size so switching modes
    /// resizes the window to what the user actually used there last time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_size_simple: Option<(u32, u32)>,

    /// Last window size (logical `[width, height]`) in the advanced mode.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_size_advanced: Option<(u32, u32)>,

    /// Last window position (physical `[x, y]`) in the simple mode. Saved in
    /// the config (not the window-state cache) so the window comes back to
    /// the same spot after a mode switch and survives app updates.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_pos_simple: Option<(i32, i32)>,

    /// Last window position (physical `[x, y]`) in the advanced mode.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_pos_advanced: Option<(i32, i32)>,
    // clod:simple-mode end

    // clod:core-updater begin
    /// Run the managed (self-downloaded) Mihomo core instead of the bundled
    /// sidecar. Conservative default: off.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub use_managed_core: Option<bool>,

    /// Managed core release channel: `stable` (default) or `alpha`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub managed_core_channel: Option<String>,

    /// Check the channel for a newer core once a day. Only notifies —
    /// downloading always stays a user action.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub core_auto_check: Option<bool>,
    // clod:core-updater end

    // clod:sub-watcher begin
    /// Subscription expiry/traffic notifications (F7). The user's global
    /// switch — off silences the watcher no matter what the panel sends.
    /// Default: on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable_sub_notifications: Option<bool>,
    // clod:sub-watcher end

    // clod:branding begin
    /// White-label display name shown in the sidebar (and, later, the tray
    /// tooltip). `None` falls back to the product name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub brand_name: Option<String>,

    /// White-label brand mark: a `data:` URL or a path the webview can load.
    /// `None` falls back to the bundled placeholder.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub brand_logo: Option<String>,
    // clod:branding end
}

#[derive(Default, Debug, Clone, Deserialize, Serialize)]
pub struct IVergeTestItem {
    pub uid: Option<String>,
    pub name: Option<String>,
    pub icon: Option<String>,
    pub url: Option<String>,
}

#[derive(Default, Debug, Clone, Deserialize, Serialize)]
pub struct IVergeTheme {
    pub primary_color: Option<String>,
    pub secondary_color: Option<String>,
    pub primary_text: Option<String>,
    pub secondary_text: Option<String>,

    pub info_color: Option<String>,
    pub error_color: Option<String>,
    pub warning_color: Option<String>,
    pub success_color: Option<String>,

    pub font_family: Option<String>,
    pub css_injection: Option<String>,
}

impl IVerge {
    /// Допустимые имена ядра clash
    pub const VALID_CLASH_CORES: &'static [&'static str] = &["verge-mihomo", "verge-mihomo-alpha"];

    // clod:prereleases begin
    /// Предварительные сборки принимаются по умолчанию: пока проект в альфе,
    /// стабильных релизов не существует вовсе, и `false` здесь означал бы
    /// «обновлений не будет никогда». Выключается вручную теми, кто хочет
    /// дождаться первой стабильной версии.
    pub const DEFAULT_RECEIVE_PRERELEASES: bool = true;
    // clod:prereleases end

    // clod:hwid begin
    /// Device identification is on by default: panels with a device limit do
    /// not serve the subscription at all without `x-hwid`.
    pub const DEFAULT_ENABLE_HWID: bool = true;
    // clod:hwid end

    // clod:simple-mode begin
    /// Used when neither the user nor the provider expressed a preference.
    pub const DEFAULT_SIMPLE_MODE: bool = true;

    /// The Connect button drives the system proxy unless turned off. When both
    /// targets end up disabled the app falls back to this one, because a
    /// Connect button that switches nothing is a broken promise.
    pub const DEFAULT_CONNECT_SYSTEM_PROXY: bool = true;

    /// TUN participation of the Connect button is opt-in.
    pub const DEFAULT_CONNECT_TUN_MODE: bool = false;
    // clod:simple-mode end

    /// Проверяет и исправляет значение clash_core в конфиге
    pub async fn validate_and_fix_config() -> Result<()> {
        let config_path = dirs::verge_path()?;
        let mut config = match help::read_yaml::<Self>(&config_path).await {
            Ok(config) => config,
            Err(_) => Self::template(),
        };

        let mut needs_fix = false;

        if let Some(ref core) = config.clash_core {
            let core_str = core.trim();
            if core_str.is_empty() || !Self::VALID_CLASH_CORES.contains(&core_str) {
                logging!(
                    warn,
                    Type::Config,
                    "При запуске обнаружена недопустимая конфигурация clash_core: '{}', автоматически исправлено на 'verge-mihomo'",
                    core
                );
                config.clash_core = Some("verge-mihomo".into());
                needs_fix = true;
            }
        } else {
            logging!(
                info,
                Type::Config,
                "При запуске обнаружено, что clash_core не задан, устанавливаю значение по умолчанию 'verge-mihomo'"
            );
            config.clash_core = Some("verge-mihomo".into());
            needs_fix = true;
        }

        // Сохраняем конфиг после исправления
        if needs_fix {
            logging!(info, Type::Config, "Сохраняю исправленный конфиг...");
            help::save_yaml(&config_path, &config, Some("# Clash Verge Config")).await?;
            logging!(
                info,
                Type::Config,
                "Исправление конфига завершено, требуется перезагрузка конфига"
            );

            Self::reload_config_after_fix(config).await?;
        } else {
            logging!(
                info,
                Type::Config,
                "Проверка clash_core пройдена: {:?}",
                config.clash_core
            );
        }

        Ok(())
    }

    /// Перезагружает конфиг после исправления
    async fn reload_config_after_fix(updated_config: Self) -> Result<()> {
        logging!(
            info,
            Type::Config,
            "Конфиг в памяти принудительно обновлён, новый clash_core: {:?}",
            &updated_config.clash_core
        );

        let config_draft = Config::verge().await;
        config_draft.edit_draft(|d| {
            *d = updated_config;
        });
        config_draft.apply();

        Ok(())
    }

    pub fn get_valid_clash_core(&self) -> String {
        self.clash_core.clone().unwrap_or_else(|| "verge-mihomo".into())
    }

    pub async fn new() -> Self {
        match dirs::verge_path() {
            Ok(path) => match help::read_yaml::<Self>(&path).await {
                Ok(mut config) => {
                    // compatibility
                    if let Some(start_page) = config.start_page.clone()
                        && start_page == "/home"
                    {
                        config.start_page = Some(String::from("/"));
                    }
                    // clod:simple-mode — migrate the legacy Connect target to
                    // the two independent toggles, once, and only when the
                    // user has not touched the new fields yet.
                    if let Some(legacy) = config.main_switch_mode.take()
                        && config.connect_system_proxy.is_none()
                        && config.connect_tun_mode.is_none()
                        && legacy == "tun"
                    {
                        config.connect_system_proxy = Some(false);
                        config.connect_tun_mode = Some(true);
                    }
                    config
                }
                Err(err) => {
                    logging!(error, Type::Config, "{err}");
                    Self::template()
                }
            },
            Err(err) => {
                logging!(error, Type::Config, "{err}");
                Self::template()
            }
        }
    }

    pub fn template() -> Self {
        Self {
            // clod: подробный лог по умолчанию — отчёт для поддержки полезен
            // ровно настолько, насколько подробен лог, который в него попал.
            // Ротация не даёт этому вырасти в проблему: 1 МБ × 8 файлов, а
            // уровень остаётся настраиваемым в «Ядро → уровень логов».
            app_log_level: Some("debug".into()),
            app_log_max_size: Some(1024),
            app_log_max_count: Some(8),
            clash_core: Some("verge-mihomo".into()),
            language: Some(clash_verge_i18n::system_language().into()),
            theme_mode: Some("system".into()),
            #[cfg(not(target_os = "windows"))]
            env_type: Some("bash".into()),
            #[cfg(target_os = "windows")]
            env_type: Some("powershell".into()),
            start_page: Some("/".into()),
            enable_group_icon: Some(true),
            pause_render_traffic_stats_on_blur: Some(true),
            #[cfg(target_os = "macos")]
            tray_icon: Some("monochrome".into()),
            menu_icon: Some("monochrome".into()),
            notice_position: Some("top-right".into()),
            collapse_navbar: Some(false),
            common_tray_icon: Some(false),
            sysproxy_tray_icon: Some(false),
            tun_tray_icon: Some(false),
            enable_auto_launch: Some(false),
            enable_silent_start: Some(false),
            enable_hover_jump_navigator: Some(true),
            hover_jump_navigator_delay: Some(280),
            enable_system_proxy: Some(false),
            proxy_auto_config: Some(false),
            pac_file_content: Some(DEFAULT_PAC.into()),
            proxy_host: Some("127.0.0.1".into()),
            #[cfg(not(target_os = "windows"))]
            verge_redir_port: Some(7895),
            #[cfg(not(target_os = "windows"))]
            verge_redir_enabled: Some(false),
            #[cfg(target_os = "linux")]
            verge_tproxy_port: Some(7896),
            #[cfg(target_os = "linux")]
            verge_tproxy_enabled: Some(false),
            verge_mixed_port: Some(7897),
            verge_socks_port: Some(7898),
            verge_socks_enabled: Some(false),
            verge_port: Some(7899),
            verge_http_enabled: Some(false),
            enable_proxy_guard: Some(false),
            enable_bypass_check: Some(true),
            use_default_bypass: Some(true),
            proxy_guard_duration: Some(30),
            auto_close_connection: Some(true),
            auto_check_update: Some(true),
            receive_prereleases: Some(Self::DEFAULT_RECEIVE_PRERELEASES),
            enable_builtin_enhanced: Some(true),
            auto_log_clean: Some(2), // 1: 1 день, 2: 7 дней, 3: 30 дней, 4: 90 дней
            enable_auto_backup_schedule: Some(false),
            auto_backup_interval_hours: Some(24),
            auto_backup_on_change: Some(true),
            webdav_url: None,
            webdav_username: None,
            webdav_password: None,
            #[cfg(target_os = "macos")]
            enable_tray_speed: Some(false),
            // enable_tray_icon: Some(true),
            tray_proxy_groups_display_mode: Some("default".into()),
            tray_inline_outbound_modes: Some(false),
            enable_global_hotkey: Some(true),
            enable_auto_light_weight_mode: Some(false),
            auto_light_weight_minutes: Some(10),
            enable_dns_settings: Some(false),
            home_cards: None,
            enable_external_controller: Some(false),
            // clod:hwid begin
            enable_hwid: Some(Self::DEFAULT_ENABLE_HWID),
            // clod:hwid end
            // clod:simple-mode begin
            // `simple_mode` stays unset on purpose: that is what lets the
            // provider's header decide until the user picks a mode.
            connect_system_proxy: Some(Self::DEFAULT_CONNECT_SYSTEM_PROXY),
            connect_tun_mode: Some(Self::DEFAULT_CONNECT_TUN_MODE),
            connect_on_launch: Some(false),
            // clod:simple-mode end
            ..Self::default()
        }
    }

    /// Save IVerge App Config
    pub async fn save_file(&self) -> Result<()> {
        help::save_yaml(&dirs::verge_path()?, &self, Some("# Clash Verge Config")).await
    }

    /// patch verge config
    /// only save to file
    #[allow(clippy::cognitive_complexity)]
    pub fn patch_config(&mut self, patch: &Self) {
        macro_rules! patch {
            ($key: tt) => {
                if patch.$key.is_some() {
                    self.$key = patch.$key.clone();
                }
            };
        }

        patch!(app_log_level);
        patch!(app_log_max_size);
        patch!(app_log_max_count);

        patch!(language);
        patch!(theme_mode);
        patch!(tray_event);
        patch!(env_type);
        patch!(start_page);
        patch!(startup_script);
        patch!(enable_group_icon);
        patch!(pause_render_traffic_stats_on_blur);
        #[cfg(target_os = "macos")]
        patch!(tray_icon);
        patch!(menu_icon);
        patch!(menu_order);
        patch!(notice_position);
        patch!(collapse_navbar);
        patch!(common_tray_icon);
        patch!(sysproxy_tray_icon);
        patch!(tun_tray_icon);

        patch!(enable_tun_mode);
        patch!(enable_auto_launch);
        patch!(enable_silent_start);
        patch!(enable_hover_jump_navigator);
        patch!(hover_jump_navigator_delay);
        #[cfg(not(target_os = "windows"))]
        patch!(verge_redir_port);
        #[cfg(not(target_os = "windows"))]
        patch!(verge_redir_enabled);
        #[cfg(target_os = "linux")]
        patch!(verge_tproxy_port);
        #[cfg(target_os = "linux")]
        patch!(verge_tproxy_enabled);
        patch!(verge_mixed_port);
        patch!(verge_socks_port);
        patch!(verge_socks_enabled);
        patch!(verge_port);
        patch!(verge_http_enabled);
        patch!(enable_system_proxy);
        patch!(enable_proxy_guard);
        patch!(enable_bypass_check);
        patch!(use_default_bypass);
        patch!(system_proxy_bypass);
        patch!(proxy_guard_duration);
        patch!(proxy_auto_config);
        patch!(pac_file_content);
        patch!(proxy_host);
        patch!(theme_setting);
        patch!(web_ui_list);
        patch!(clash_core);
        patch!(hotkeys);
        patch!(enable_global_hotkey);

        patch!(auto_close_connection);
        patch!(auto_check_update);
        patch!(receive_prereleases);
        patch!(default_latency_test);
        patch!(default_latency_timeout);
        patch!(enable_builtin_enhanced);
        patch!(proxy_layout_column);
        patch!(test_list);
        patch!(auto_log_clean);
        patch!(enable_auto_backup_schedule);
        patch!(auto_backup_interval_hours);
        patch!(auto_backup_on_change);

        patch!(webdav_url);
        patch!(webdav_username);
        patch!(webdav_password);
        #[cfg(target_os = "macos")]
        patch!(enable_tray_speed);
        // patch!(enable_tray_icon);
        patch!(tray_proxy_groups_display_mode);
        patch!(tray_inline_outbound_modes);
        patch!(enable_auto_light_weight_mode);
        patch!(auto_light_weight_minutes);
        patch!(enable_dns_settings);
        patch!(home_cards);
        patch!(enable_external_controller);
        // clod:hwid begin
        patch!(enable_hwid);
        patch!(hwid);
        // clod:hwid end
        // clod:simple-mode begin
        patch!(simple_mode);
        patch!(connect_system_proxy);
        patch!(connect_tun_mode);
        patch!(connect_on_launch);
        patch!(tun_setup_declined);
        patch!(window_size_simple);
        patch!(window_size_advanced);
        patch!(window_pos_simple);
        patch!(window_pos_advanced);
        // clod:simple-mode end
        // clod:core-updater begin
        patch!(use_managed_core);
        patch!(managed_core_channel);
        patch!(core_auto_check);
        // clod:core-updater end
        // clod:sub-watcher begin
        patch!(enable_sub_notifications);
        // clod:sub-watcher end
        // clod:branding begin
        patch!(brand_name);
        patch!(brand_logo);
        // clod:branding end
    }

    pub const fn get_singleton_port() -> u16 {
        crate::constants::network::ports::SINGLETON_SERVER
    }

    /// Получить уровень логирования
    pub fn get_log_level(&self) -> LevelFilter {
        if let Some(level) = self.app_log_level.as_ref() {
            match level.to_lowercase().as_str() {
                "silent" => LevelFilter::Off,
                "error" => LevelFilter::Error,
                "warn" => LevelFilter::Warn,
                "info" => LevelFilter::Info,
                "debug" => LevelFilter::Debug,
                "trace" => LevelFilter::Trace,
                _ => LevelFilter::Info,
            }
        } else {
            LevelFilter::Info
        }
    }
}
