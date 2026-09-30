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

    pub enable_verbose_diagnostics: Option<bool>,

    pub language: Option<String>,

    pub theme_mode: Option<String>,

    pub tray_event: Option<String>,

    pub env_type: Option<String>,

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

    /// Человек сам выключил раздачу в локальную сеть.
    ///
    /// clod:lan-share — отличает «выключено, потому что так решил человек» от
    /// «выключено по умолчанию». Подписка со своим списком адресов вправе
    /// открыть раздачу во втором случае и не вправе в первом. Ключа нет —
    /// человек слова не говорил.
    pub lan_sharing_declined: Option<bool>,

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

    pub auto_close_connection_home: Option<bool>,

    pub auto_check_update: Option<bool>,

    pub receive_prereleases: Option<bool>,

    pub default_latency_test: Option<String>,

    pub default_latency_timeout: Option<i16>,

    pub enable_builtin_enhanced: Option<bool>,

    pub proxy_layout_column: Option<u8>,

    pub auto_log_clean: Option<i32>,

    pub enable_auto_backup_schedule: Option<bool>,

    pub auto_backup_interval_hours: Option<u64>,

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

    #[cfg(target_os = "macos")]
    pub enable_dns_override: Option<bool>,

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

    /// Разовая уборка способа подключения уже прошла (см.
    /// `forget_the_template_connect_choice`); у новых установок — с самого начала.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connect_template_cleared: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub connect_on_launch: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub tun_setup_declined: Option<String>,

    #[serde(skip_serializing)]
    pub main_switch_mode: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub core_log_keys_unpinned: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub tun_window_defaults_unpinned: Option<bool>,

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
    pub provider_theme: Option<bool>,
}

fn pac_without_the_frozen_address(text: &str) -> Option<&'static str> {
    const HEAD: &str = "return \"PROXY ";
    const TAIL: &str = "; SOCKS5 ";

    let start = text.find(HEAD)? + HEAD.len();
    let end = text[start..].find(TAIL)? + start;
    let address = text.get(start..end)?;
    let colon = address.rfind(':')?;
    let (host, port) = (address.get(..colon)?, address.get(colon + 1..)?);
    port.parse::<u16>().ok()?;

    let rebuilt = DEFAULT_PAC.replace("%proxy_host%", host).replace("%mixed-port%", port);
    (rebuilt.trim_end() == text.trim_end()).then_some(DEFAULT_PAC)
}

impl IVerge {
    pub const VALID_CLASH_CORES: &'static [&'static str] = &["verge-mihomo", "verge-mihomo-alpha"];

    pub const DEFAULT_RECEIVE_PRERELEASES: bool = false;

    pub const DEFAULT_ENABLE_HWID: bool = true;

    #[cfg(target_os = "macos")]
    pub const DEFAULT_ENABLE_DNS_OVERRIDE: bool = true;

    pub const DEFAULT_CONNECT_SYSTEM_PROXY: bool = true;

    pub const DEFAULT_CONNECT_TUN_MODE: bool = false;

    pub const DEFAULT_AUTO_CLOSE_CONNECTION: bool = true;

    /// Единственное умолчание размера файла журнала: шаблон настроек, запасные
    /// значения в коде и начальное состояние формы обязаны говорить одно и то же,
    /// иначе у установки без ключа ротация идёт в разы чаще, чем у соседней.
    pub const DEFAULT_APP_LOG_MAX_SIZE: u64 = 1024;

    pub const DEFAULT_APP_LOG_MAX_COUNT: usize = 8;

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

    /// Только те поля, которые уезжают ядру в сборке (`enhance` их читает).
    ///
    /// Их принимает ядро, и они записываются в момент приёма; остальные поля
    /// настроек принимает свой шаг (системный прокси, автозапуск, горячие
    /// клавиши), и если тот шаг не прошёл, они к прежним и возвращаются.
    pub fn core_facing(&self) -> Self {
        Self {
            enable_tun_mode: self.enable_tun_mode,
            enable_builtin_enhanced: self.enable_builtin_enhanced,
            verge_socks_enabled: self.verge_socks_enabled,
            verge_http_enabled: self.verge_http_enabled,
            enable_dns_settings: self.enable_dns_settings,
            lan_sharing_declined: self.lan_sharing_declined,
            tun_stack: self.tun_stack.clone(),
            tun_strict_route: self.tun_strict_route.clone(),
            tun_dns_hijack: self.tun_dns_hijack.clone(),
            #[cfg(target_os = "macos")]
            enable_dns_override: self.enable_dns_override,
            #[cfg(not(target_os = "windows"))]
            verge_redir_enabled: self.verge_redir_enabled,
            #[cfg(target_os = "linux")]
            verge_tproxy_enabled: self.verge_tproxy_enabled,
            enable_external_controller: self.enable_external_controller,
            clash_core: self.clash_core.clone(),
            ..Self::default()
        }
    }

    pub async fn new() -> Self {
        match dirs::verge_path() {
            Ok(path) => match help::read_yaml::<Self>(&path).await {
                Ok(mut config) => {
                    if let Some(pac) = config.pac_file_content.as_deref()
                        && let Some(restored) = pac_without_the_frozen_address(pac)
                    {
                        config.pac_file_content = Some(restored.into());
                    }
                    if let Some(legacy) = config.main_switch_mode.take()
                        && config.connect_system_proxy.is_none()
                        && config.connect_tun_mode.is_none()
                        && legacy == "tun"
                    {
                        config.connect_system_proxy = Some(false);
                        config.connect_tun_mode = Some(true);
                    }
                    forget_the_template_connect_choice(&mut config);
                    config
                }
                Err(err) => {
                    logging!(error, Type::Config, "{err}");
                    // Шаблон вместо прочитанных настроек — это не «настроек нет».
                    // Откладываем копию файла до того, как шаблон успеет записаться
                    // поверх: путей сохранения настроек по ходу работы много.
                    crate::config::load_failures::keep_a_copy(&path).await;
                    crate::config::load_failures::mark(crate::config::load_failures::ConfigFile::Verge);
                    Self::template()
                }
            },
            Err(err) => {
                logging!(error, Type::Config, "{err}");
                crate::config::load_failures::mark(crate::config::load_failures::ConfigFile::Verge);
                Self::template()
            }
        }
    }

    pub fn template() -> Self {
        Self {
            app_log_level: Some("debug".into()),
            app_log_max_size: Some(Self::DEFAULT_APP_LOG_MAX_SIZE),
            app_log_max_count: Some(Self::DEFAULT_APP_LOG_MAX_COUNT),
            clash_core: Some("verge-mihomo".into()),
            language: Some(clash_verge_i18n::system_language().into()),
            theme_mode: Some("system".into()),
            #[cfg(not(target_os = "windows"))]
            env_type: Some("bash".into()),
            #[cfg(target_os = "windows")]
            env_type: Some("powershell".into()),
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
            // clod:port-ladder — у новой установки порта нет: решает подписка,
            // а если она молчит — умолчание ядра из лесенки.
            verge_mixed_port: None,
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
            webdav_url: None,
            webdav_username: None,
            webdav_password: None,
            #[cfg(target_os = "macos")]
            enable_tray_speed: Some(false),
            #[cfg(target_os = "macos")]
            enable_dns_override: Some(Self::DEFAULT_ENABLE_DNS_OVERRIDE),
            tray_proxy_groups_display_mode: Some("default".into()),
            tray_inline_outbound_modes: Some(false),
            enable_global_hotkey: Some(true),
            enable_auto_light_weight_mode: Some(false),
            auto_light_weight_minutes: Some(10),
            enable_dns_settings: Some(false),
            enable_external_controller: Some(false),
            enable_hwid: Some(Self::DEFAULT_ENABLE_HWID),
            connect_on_launch: Some(false),
            connect_template_cleared: Some(true),
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
        patch!(enable_verbose_diagnostics);

        patch!(language);
        patch!(theme_mode);
        patch!(tray_event);
        patch!(env_type);
        patch!(core_log_keys_unpinned);
        patch!(tun_window_defaults_unpinned);
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
        // clod:port-ladder — ноль означает «закрепление снято, порт как в
        // подписке»: у `patch!` нет способа убрать значение, а без этого
        // старый порт оставался в файле настроек навсегда.
        if let Some(port) = patch.verge_mixed_port {
            self.verge_mixed_port = (port != 0).then_some(port);
        }
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
        patch!(auto_close_connection_home);
        patch!(auto_check_update);
        patch!(receive_prereleases);
        patch!(default_latency_test);
        patch!(default_latency_timeout);
        patch!(enable_builtin_enhanced);
        patch!(proxy_layout_column);
        patch!(auto_log_clean);
        patch!(enable_auto_backup_schedule);
        patch!(auto_backup_interval_hours);

        patch!(webdav_url);
        patch!(webdav_username);
        patch!(webdav_password);
        #[cfg(target_os = "macos")]
        patch!(enable_tray_speed);
        #[cfg(target_os = "macos")]
        patch!(enable_dns_override);
        patch!(tray_proxy_groups_display_mode);
        patch!(tray_inline_outbound_modes);
        patch!(enable_auto_light_weight_mode);
        patch!(auto_light_weight_minutes);
        patch!(enable_dns_settings);
        patch!(lan_sharing_declined);
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
        patch!(enable_sub_notifications);
    }

    pub const fn get_singleton_port() -> u16 {
        crate::constants::network::ports::SINGLETON_SERVER
    }

    pub const fn verbose_diagnostics(&self) -> bool {
        matches!(self.enable_verbose_diagnostics, Some(true))
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

/// Р27-05: прежний шаблон настроек записывал в `verge.yaml` способ подключения
/// по умолчанию (системный прокси, без TUN), и у таких установок пожелание панели
/// `clod-connect-mode` без замка не действовало — значение выглядело выбором
/// человека. Отличить шаблон от выбора, совпавшего с умолчанием, по файлу нельзя,
/// поэтому пара, равная умолчанию, снимается один раз; дальше её пишет только
/// человек, и она остаётся его выбором.
fn forget_the_template_connect_choice(config: &mut IVerge) {
    if config.connect_template_cleared.is_some() {
        return;
    }
    config.connect_template_cleared = Some(true);
    if config.connect_system_proxy == Some(IVerge::DEFAULT_CONNECT_SYSTEM_PROXY)
        && config.connect_tun_mode == Some(IVerge::DEFAULT_CONNECT_TUN_MODE)
    {
        config.connect_system_proxy = None;
        config.connect_tun_mode = None;
    }
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_PAC, IVerge, forget_the_template_connect_choice, pac_without_the_frozen_address};

    #[test]
    fn core_facing_keeps_what_the_core_reads_and_drops_the_rest() {
        let patch = IVerge {
            enable_tun_mode: Some(true),
            verge_socks_enabled: Some(true),
            enable_dns_settings: Some(true),
            clash_core: Some("verge-mihomo-alpha".into()),
            enable_system_proxy: Some(true),
            enable_auto_launch: Some(true),
            enable_global_hotkey: Some(false),
            language: Some("ru".into()),
            ..IVerge::default()
        };
        let core = patch.core_facing();
        assert_eq!(core.enable_tun_mode, Some(true));
        assert_eq!(core.verge_socks_enabled, Some(true));
        assert_eq!(core.enable_dns_settings, Some(true));
        assert_eq!(core.clash_core.as_deref(), Some("verge-mihomo-alpha"));
        assert_eq!(
            core.enable_system_proxy, None,
            "системный прокси принимает свой шаг, не ядро"
        );
        assert_eq!(core.enable_auto_launch, None);
        assert_eq!(core.enable_global_hotkey, None);
        assert_eq!(core.language, None);
    }

    fn frozen(host: &str, port: &str) -> std::string::String {
        DEFAULT_PAC.replace("%proxy_host%", host).replace("%mixed-port%", port)
    }

    #[test]
    fn a_pac_with_a_frozen_address_goes_back_to_placeholders() {
        assert_eq!(
            pac_without_the_frozen_address(&frozen("127.0.0.1", "7890")),
            Some(DEFAULT_PAC)
        );
        assert_eq!(
            pac_without_the_frozen_address(&frozen("[::1]", "7897")),
            Some(DEFAULT_PAC)
        );
        assert_eq!(
            pac_without_the_frozen_address(frozen("127.0.0.1", "7890").trim_end()),
            Some(DEFAULT_PAC)
        );
    }

    #[test]
    fn the_template_connect_choice_is_forgotten_once() {
        let stored = |sys, tun, cleared| IVerge {
            connect_system_proxy: sys,
            connect_tun_mode: tun,
            connect_template_cleared: cleared,
            ..IVerge::default()
        };
        let migrated = |mut config: IVerge| {
            forget_the_template_connect_choice(&mut config);
            (
                config.connect_system_proxy,
                config.connect_tun_mode,
                config.connect_template_cleared,
            )
        };

        // Старая установка с парой из прежнего шаблона — снимается.
        assert_eq!(
            migrated(stored(Some(true), Some(false), None)),
            (None, None, Some(true))
        );
        // Любой другой выбор остаётся.
        assert_eq!(
            migrated(stored(Some(false), Some(true), None)),
            (Some(false), Some(true), Some(true))
        );
        assert_eq!(
            migrated(stored(Some(true), Some(true), None)),
            (Some(true), Some(true), Some(true))
        );
        assert_eq!(migrated(stored(Some(true), None, None)), (Some(true), None, Some(true)));
        // Уборка прошла — выбор умолчания руками больше не снимается.
        assert_eq!(
            migrated(stored(Some(true), Some(false), Some(true))),
            (Some(true), Some(false), Some(true))
        );
        // Новая установка несёт отметку с самого начала.
        assert_eq!(IVerge::template().connect_template_cleared, Some(true));
    }

    #[test]
    fn the_migration_changes_nothing_the_second_time() {
        let once = pac_without_the_frozen_address(&frozen("127.0.0.1", "7890"));

        assert_eq!(once, Some(DEFAULT_PAC));
        assert_eq!(pac_without_the_frozen_address(DEFAULT_PAC), None);
    }

    #[test]
    fn a_pac_the_person_edited_is_left_alone() {
        let edited = frozen("127.0.0.1", "7890").replace("DIRECT;", "DIRECT; // мой");

        assert_eq!(pac_without_the_frozen_address(&edited), None);
        assert_eq!(
            pac_without_the_frozen_address("function FindProxyForURL() { return \"DIRECT\"; }"),
            None
        );
        assert_eq!(
            pac_without_the_frozen_address(&frozen("127.0.0.1", "%mixed-port%")),
            None
        );
    }
}
