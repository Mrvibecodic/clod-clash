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

#[derive(Default, Debug, Clone, Deserialize, Serialize)]
pub struct IVerge {
    pub app_log_level: Option<String>,

    pub app_log_max_size: Option<u64>,

    pub app_log_max_count: Option<usize>,

    pub language: Option<String>,

    pub theme_mode: Option<String>,

    pub tray_event: Option<String>,

    pub env_type: Option<String>,

    pub start_page: Option<String>,
    pub startup_script: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable_group_icon: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub common_tray_icon: Option<bool>,

    #[cfg(target_os = "macos")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tray_icon: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub notice_position: Option<String>,

    pub sysproxy_tray_icon: Option<bool>,

    pub tun_tray_icon: Option<bool>,

    pub enable_tun_mode: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub tun_stack: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub tun_strict_route: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub tun_dns_hijack: Option<String>,

    pub enable_auto_launch: Option<bool>,

    pub enable_silent_start: Option<bool>,

    pub enable_system_proxy: Option<bool>,

    pub enable_proxy_guard: Option<bool>,

    pub enable_bypass_check: Option<bool>,

    pub enable_dns_settings: Option<bool>,

    pub use_default_bypass: Option<bool>,

    pub system_proxy_bypass: Option<String>,

    pub proxy_guard_duration: Option<u64>,

    pub proxy_auto_config: Option<bool>,

    pub pac_file_content: Option<String>,

    pub proxy_host: Option<String>,

    pub theme_setting: Option<IVergeTheme>,

    pub web_ui_list: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub clash_core: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub hotkeys: Option<Vec<String>>,

    pub enable_global_hotkey: Option<bool>,

    pub auto_close_connection: Option<bool>,

    pub auto_check_update: Option<bool>,

    pub receive_prereleases: Option<bool>,

    pub default_latency_test: Option<String>,

    pub default_latency_timeout: Option<i16>,

    pub enable_builtin_enhanced: Option<bool>,

    pub proxy_layout_column: Option<u8>,

    pub auto_log_clean: Option<i32>,

    pub enable_auto_backup_schedule: Option<bool>,

    pub auto_backup_interval_hours: Option<u64>,

    pub auto_backup_on_change: Option<bool>,

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

    #[serde(
        serialize_with = "serialize_encrypted",
        deserialize_with = "deserialize_encrypted",
        skip_serializing_if = "Option::is_none",
        default
    )]
    pub webdav_url: Option<String>,

    #[serde(
        serialize_with = "serialize_encrypted",
        deserialize_with = "deserialize_encrypted",
        skip_serializing_if = "Option::is_none",
        default
    )]
    pub webdav_username: Option<String>,

    #[serde(
        serialize_with = "serialize_encrypted",
        deserialize_with = "deserialize_encrypted",
        skip_serializing_if = "Option::is_none",
        default
    )]
    pub webdav_password: Option<String>,

    #[cfg(target_os = "macos")]
    pub enable_tray_speed: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub tray_proxy_groups_display_mode: Option<String>,
    pub tray_inline_outbound_modes: Option<bool>,

    pub enable_auto_light_weight_mode: Option<bool>,

    pub auto_light_weight_minutes: Option<u64>,

    pub enable_hover_jump_navigator: Option<bool>,

    pub hover_jump_navigator_delay: Option<u64>,

    pub enable_external_controller: Option<bool>,

    pub enable_hwid: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub hwid: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub simple_mode: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub connect_system_proxy: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub connect_tun_mode: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub connect_on_launch: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub tun_setup_declined: Option<String>,

    #[serde(skip_serializing)]
    pub main_switch_mode: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_size_simple: Option<(u32, u32)>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_size_advanced: Option<(u32, u32)>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_pos_simple: Option<(i32, i32)>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_pos_advanced: Option<(i32, i32)>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_fit_content: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub home_tool_shortcuts: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub use_managed_core: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub managed_core_channel: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub core_auto_check: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable_sub_notifications: Option<bool>,
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
    pub provider_theme: Option<bool>,
}

impl IVerge {
    pub const VALID_CLASH_CORES: &'static [&'static str] = &["verge-mihomo", "verge-mihomo-alpha"];

    pub const DEFAULT_RECEIVE_PRERELEASES: bool = false;

    pub const DEFAULT_ENABLE_HWID: bool = true;

    pub const DEFAULT_CONNECT_SYSTEM_PROXY: bool = true;

    pub const DEFAULT_CONNECT_TUN_MODE: bool = false;

    pub const DEFAULT_AUTO_CLOSE_CONNECTION: bool = true;

    pub fn auto_close_connection(&self) -> bool {
        self.auto_close_connection
            .unwrap_or(Self::DEFAULT_AUTO_CLOSE_CONNECTION)
    }

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
                    if let Some(start_page) = config.start_page.clone()
                        && start_page == "/home"
                    {
                        config.start_page = Some(String::from("/"));
                    }
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
            #[cfg(target_os = "macos")]
            tray_icon: Some("monochrome".into()),
            notice_position: Some("top-right".into()),
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
            auto_close_connection: Some(Self::DEFAULT_AUTO_CLOSE_CONNECTION),
            auto_check_update: Some(true),
            receive_prereleases: Some(Self::DEFAULT_RECEIVE_PRERELEASES),
            enable_builtin_enhanced: Some(true),
            auto_log_clean: Some(2),
            enable_auto_backup_schedule: Some(false),
            auto_backup_interval_hours: Some(24),
            auto_backup_on_change: Some(true),
            webdav_url: None,
            webdav_username: None,
            webdav_password: None,
            #[cfg(target_os = "macos")]
            enable_tray_speed: Some(false),
            tray_proxy_groups_display_mode: Some("default".into()),
            tray_inline_outbound_modes: Some(false),
            enable_global_hotkey: Some(true),
            enable_auto_light_weight_mode: Some(false),
            auto_light_weight_minutes: Some(10),
            enable_dns_settings: Some(false),
            enable_external_controller: Some(false),
            enable_hwid: Some(Self::DEFAULT_ENABLE_HWID),
            connect_system_proxy: Some(Self::DEFAULT_CONNECT_SYSTEM_PROXY),
            connect_tun_mode: Some(Self::DEFAULT_CONNECT_TUN_MODE),
            connect_on_launch: Some(false),
            ..Self::default()
        }
    }

    pub async fn save_file(&self) -> Result<()> {
        help::save_yaml(&dirs::verge_path()?, &self, Some("# Clash Verge Config")).await
    }

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
        #[cfg(target_os = "macos")]
        patch!(tray_icon);
        patch!(notice_position);
        patch!(common_tray_icon);
        patch!(sysproxy_tray_icon);
        patch!(tun_tray_icon);

        patch!(enable_tun_mode);
        patch!(tun_stack);
        patch!(tun_strict_route);
        patch!(tun_dns_hijack);
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
        patch!(auto_log_clean);
        patch!(enable_auto_backup_schedule);
        patch!(auto_backup_interval_hours);
        patch!(auto_backup_on_change);

        patch!(webdav_url);
        patch!(webdav_username);
        patch!(webdav_password);
        #[cfg(target_os = "macos")]
        patch!(enable_tray_speed);
        patch!(tray_proxy_groups_display_mode);
        patch!(tray_inline_outbound_modes);
        patch!(enable_auto_light_weight_mode);
        patch!(auto_light_weight_minutes);
        patch!(enable_dns_settings);
        patch!(enable_external_controller);
        patch!(enable_hwid);
        patch!(hwid);
        patch!(simple_mode);
        patch!(connect_system_proxy);
        patch!(connect_tun_mode);
        patch!(connect_on_launch);
        patch!(tun_setup_declined);
        patch!(window_size_simple);
        patch!(window_size_advanced);
        patch!(window_pos_simple);
        patch!(window_pos_advanced);
        patch!(window_fit_content);
        patch!(home_tool_shortcuts);
        patch!(use_managed_core);
        patch!(managed_core_channel);
        patch!(core_auto_check);
        patch!(enable_sub_notifications);
    }

    pub const fn get_singleton_port() -> u16 {
        crate::constants::network::ports::SINGLETON_SERVER
    }

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
