use crate::config::{IProfilePreview, IVerge};
use crate::core::service;
use crate::core::tray::menu_def::TrayAction;
use crate::module::lightweight;
use crate::process::AsyncHandler;
use crate::singleton;
use crate::utils::window_manager::WindowManager;
use crate::{
    Type, cmd,
    config::Config,
    feat, logging,
    module::lightweight::is_in_lightweight_mode,
    utils::{dirs::find_target_icons, help},
};
use clash_verge_limiter::{Limiter, SystemClock, SystemLimiter};
use clash_verge_logging::logging_error;
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri_plugin_clash_verge_sysinfo::is_current_app_handle_admin;
use tauri_plugin_mihomo::models::Proxies;
use tokio::fs;

use super::handle;
use anyhow::Result;
use smartstring::alias::String;
use std::borrow::Cow;
use std::collections::HashMap;
use std::time::Duration;
use tauri::{
    AppHandle, Wry,
    menu::{CheckMenuItem, IsMenuItem, MenuEvent, MenuItem, PredefinedMenuItem, Submenu},
};

#[cfg(target_os = "linux")]
mod linux;
mod menu_def;
#[cfg(target_os = "macos")]
mod speed_task;
use menu_def::{MenuIds, MenuNode, MenuTexts};

#[cfg(target_os = "linux")]
use linux::is_active as ksni_active;
#[cfg(not(target_os = "linux"))]
const fn ksni_active() -> bool {
    false
}

const TRAY_CLICK_DEBOUNCE_MS: u64 = 300;
pub const TRAY_ID: &str = "clash-verge-rev-tray";

#[derive(Clone, Copy)]
struct TrayMenuOptions {
    is_lightweight_mode: bool,
    include_proxy_groups: bool,
}

#[derive(Clone)]
struct TrayState {}

enum IconKind {
    Common,
    SysProxy,
    Tun,
}

pub struct Tray {
    limiter: SystemLimiter,
    activate_limiter: SystemLimiter,
    #[cfg(target_os = "macos")]
    speed_controller: speed_task::TraySpeedController,
}

impl TrayState {
    async fn get_tray_icon(verge: &IVerge) -> (bool, Cow<'_, [u8]>) {
        let tun_mode = feat::tun::is_active_with(verge.enable_tun_mode.unwrap_or(false));
        let system_mode = verge.enable_system_proxy.unwrap_or(false);
        let kind = if tun_mode {
            IconKind::Tun
        } else if system_mode {
            IconKind::SysProxy
        } else {
            IconKind::Common
        };
        Self::load_icon(verge, kind).await
    }

    async fn load_icon(verge: &IVerge, kind: IconKind) -> (bool, Cow<'_, [u8]>) {
        let (custom_enabled, icon_name) = match kind {
            IconKind::Common => (verge.common_tray_icon.unwrap_or(false), "common"),
            IconKind::SysProxy => (verge.sysproxy_tray_icon.unwrap_or(false), "sysproxy"),
            IconKind::Tun => (verge.tun_tray_icon.unwrap_or(false), "tun"),
        };

        if custom_enabled
            && let Ok(Some(path)) = find_target_icons(icon_name)
            && let Ok(data) = fs::read(path).await
        {
            return (true, Cow::Owned(data));
        }

        Self::default_icon(verge, kind)
    }

    #[allow(clippy::missing_const_for_fn)]
    fn default_icon(verge: &IVerge, kind: IconKind) -> (bool, Cow<'_, [u8]>) {
        #[cfg(target_os = "macos")]
        {
            let is_mono = verge.tray_icon.as_deref().unwrap_or("monochrome") == "monochrome";
            if is_mono {
                return (
                    false,
                    match kind {
                        IconKind::Common => Cow::Borrowed(include_bytes!("../../../icons/tray-icon-mono.ico")),
                        IconKind::SysProxy => {
                            Cow::Borrowed(include_bytes!("../../../icons/tray-icon-sys-mono-new.ico"))
                        }
                        IconKind::Tun => Cow::Borrowed(include_bytes!("../../../icons/tray-icon-tun-mono-new.ico")),
                    },
                );
            }
        }

        #[cfg(not(target_os = "macos"))]
        let _ = verge;

        (
            false,
            match kind {
                IconKind::Common => Cow::Borrowed(include_bytes!("../../../icons/tray-icon.ico")),
                IconKind::SysProxy => Cow::Borrowed(include_bytes!("../../../icons/tray-icon-sys.ico")),
                IconKind::Tun => Cow::Borrowed(include_bytes!("../../../icons/tray-icon-tun.ico")),
            },
        )
    }
}

impl Default for Tray {
    #[allow(clippy::unwrap_used)]
    fn default() -> Self {
        Self {
            limiter: Limiter::new(Duration::from_millis(TRAY_CLICK_DEBOUNCE_MS), SystemClock),
            activate_limiter: Limiter::new(Duration::from_millis(TRAY_CLICK_DEBOUNCE_MS), SystemClock),
            #[cfg(target_os = "macos")]
            speed_controller: speed_task::TraySpeedController::new(),
        }
    }
}

singleton!(Tray, TRAY);

impl Tray {
    fn new() -> Self {
        Self::default()
    }

    pub async fn init(&self) -> Result<()> {
        if handle::Handle::global().is_exiting() {
            logging!(
                debug,
                Type::Tray,
                "Приложение завершает работу, пропускаю инициализацию трея"
            );
            return Ok(());
        }

        let app_handle = handle::Handle::app_handle();

        match self.create_tray_from_handle(app_handle).await {
            Ok(_) => {
                logging!(info, Type::Tray, "System tray created successfully");
            }
            Err(e) => {
                logging!(
                    warn,
                    Type::Tray,
                    "System tray creation failed: {e}, Application will continue running without tray icon",
                );
            }
        }
        Ok(())
    }

    pub async fn update_click_behavior(&self) -> Result<()> {
        if handle::Handle::global().is_exiting() {
            logging!(
                debug,
                Type::Tray,
                "Приложение завершает работу, пропускаю обновление поведения клика по трею"
            );
            return Ok(());
        }

        if ksni_active() {
            return Ok(());
        }

        let app_handle = handle::Handle::app_handle();
        let tray_event = { Config::verge().await.latest_arc().tray_event.clone() };
        let tray_event = TrayAction::from(tray_event.as_deref().unwrap_or("main_window"));
        let tray = app_handle
            .tray_by_id(TRAY_ID)
            .ok_or_else(|| anyhow::anyhow!("Failed to get main tray"))?;
        match tray_event {
            TrayAction::TrayMenu => tray.set_show_menu_on_left_click(true)?,
            _ => tray.set_show_menu_on_left_click(false)?,
        }
        Ok(())
    }

    pub async fn update_menu(&self) -> Result<()> {
        if handle::Handle::global().is_exiting() {
            logging!(
                debug,
                Type::Tray,
                "Приложение завершает работу, пропускаю обновление меню трея"
            );
            return Ok(());
        }
        let app_handle = handle::Handle::app_handle();
        self.update_menu_internal(app_handle, true).await
    }

    async fn update_menu_internal(&self, app_handle: &AppHandle, include_proxy_groups: bool) -> Result<()> {
        let tray = if ksni_active() {
            None
        } else {
            let Some(tray) = app_handle.tray_by_id(TRAY_ID) else {
                logging!(warn, Type::Tray, "Failed to update tray menu: tray not found");
                return Ok(());
            };
            Some(tray)
        };

        let verge = Config::verge().await.latest_arc();
        let system_proxy = verge.enable_system_proxy.as_ref().unwrap_or(&false);
        let tun_mode = feat::tun::is_active_with(verge.enable_tun_mode.unwrap_or(false));
        let tun_mode_available =
            is_current_app_handle_admin(app_handle) || service::is_service_available().await.is_ok();
        let mode = {
            Config::clash()
                .await
                .latest_arc()
                .0
                .get("mode")
                .map(|val| val.as_str().unwrap_or("rule"))
                .unwrap_or("rule")
                .to_owned()
        };
        let profiles_config = Config::profiles().await;
        let profiles_arc = profiles_config.latest_arc();
        let profiles_preview = profiles_arc.profiles_preview().unwrap_or_default();
        let is_lightweight_mode = is_in_lightweight_mode();

        let menu_model = create_tray_menu_model(
            Some(mode.as_str()),
            *system_proxy,
            tun_mode,
            tun_mode_available,
            profiles_preview,
            TrayMenuOptions {
                is_lightweight_mode,
                include_proxy_groups,
            },
        )
        .await?;

        #[cfg(target_os = "linux")]
        if ksni_active() {
            linux::update_menu(menu_model).await;
            return Ok(());
        }

        if let Some(tray) = tray {
            logging_error!(
                Type::Tray,
                tray.set_menu(Some(render_tray_menu(app_handle, menu_model)?))
            );
        }

        logging!(debug, Type::Tray, "Меню трея обновлено успешно");
        Ok(())
    }

    pub async fn update_icon(&self, verge: &IVerge) -> Result<()> {
        if handle::Handle::global().is_exiting() {
            logging!(
                debug,
                Type::Tray,
                "Приложение завершает работу, пропускаю обновление иконки трея"
            );
            return Ok(());
        }

        let app_handle = handle::Handle::app_handle();

        let tray = if ksni_active() {
            None
        } else {
            let Some(tray) = app_handle.tray_by_id(TRAY_ID) else {
                logging!(warn, Type::Tray, "Failed to update tray icon: tray not found");
                return Ok(());
            };
            Some(tray)
        };

        let (_is_custom_icon, icon_bytes) = TrayState::get_tray_icon(verge).await;

        #[cfg(target_os = "linux")]
        if ksni_active() {
            linux::update_icon(&icon_bytes).await;
            return Ok(());
        }

        if let Some(tray) = tray {
            let template = {
                #[cfg(target_os = "macos")]
                {
                    verge.tray_icon.as_ref().is_none_or(|v| v == "monochrome")
                }
                #[cfg(not(target_os = "macos"))]
                {
                    false
                }
            };
            let icon = Some(tauri::image::Image::from_bytes(&icon_bytes)?);

            logging_error!(Type::Tray, tray.set_icon_with_as_template(icon, template));
        }

        Ok(())
    }

    pub async fn update_tooltip(&self) -> Result<()> {
        if handle::Handle::global().is_exiting() {
            logging!(
                debug,
                Type::Tray,
                "Приложение завершает работу, пропускаю обновление подсказки трея"
            );
            return Ok(());
        }

        let app_handle = handle::Handle::app_handle();

        let verge = Config::verge().await.latest_arc();
        let system_proxy = verge.enable_system_proxy.unwrap_or(false);
        let tun_mode = feat::tun::is_active_with(verge.enable_tun_mode.unwrap_or(false));

        let switch_str = |flag: bool| {
            if flag { "on" } else { "off" }
        };

        let mut current_profile_name = "None".into();
        {
            let profiles = Config::profiles().await;
            let profiles = profiles.latest_arc();
            if let Some(current_profile_uid) = profiles.get_current()
                && let Ok(profile) = profiles.get_item(current_profile_uid)
            {
                current_profile_name = match &profile.name {
                    Some(profile_name) => profile_name.to_string(),
                    None => current_profile_name,
                };
            }
        }

        let sys_proxy_text = clash_verge_i18n::t!("tray.tooltip.systemProxy");
        let tun_text = clash_verge_i18n::t!("tray.tooltip.tun");
        let profile_text = clash_verge_i18n::t!("tray.tooltip.profile");

        let v = env!("CARGO_PKG_VERSION");
        let reassembled_version = v.split_once('+').map_or_else(
            || v.into(),
            |(main, rest)| format!("{main}+{}", rest.split('.').next().unwrap_or("")),
        );

        let tooltip = format!(
            "{} {}\n{}: {}\n{}: {}\n{}: {}",
            crate::constants::branding::APP_NAME,
            reassembled_version,
            sys_proxy_text,
            switch_str(system_proxy),
            tun_text,
            switch_str(tun_mode),
            profile_text,
            current_profile_name
        );

        #[cfg(target_os = "linux")]
        if ksni_active() {
            linux::update_tooltip(tooltip).await;
            return Ok(());
        }

        let Some(tray) = app_handle.tray_by_id(TRAY_ID) else {
            logging!(warn, Type::Tray, "Failed to update tray tooltip: tray not found");
            return Ok(());
        };

        logging_error!(Type::Tray, tray.set_tooltip(Some(&tooltip)));

        Ok(())
    }

    pub async fn update_part(&self) -> Result<()> {
        if handle::Handle::global().is_exiting() {
            logging!(
                debug,
                Type::Tray,
                "Приложение завершает работу, пропускаю частичное обновление трея"
            );
            return Ok(());
        }
        let verge = Config::verge().await.data_arc();
        let app_handle = handle::Handle::app_handle();
        self.update_menu_internal(app_handle, false).await?;
        AsyncHandler::spawn(|| async {
            logging_error!(Type::Tray, Self::global().update_menu().await);
        });
        self.update_icon(&verge).await?;
        #[cfg(target_os = "macos")]
        self.update_speed_task(verge.enable_tray_speed.unwrap_or(false));
        self.update_tooltip().await?;
        Ok(())
    }

    pub async fn update_menu_and_icon(&self) {
        logging_error!(Type::Tray, self.update_menu().await);
        let verge = Config::verge().await.data_arc();
        logging_error!(Type::Tray, self.update_icon(&verge).await);
    }

    async fn create_tray_from_handle(&self, app_handle: &AppHandle) -> Result<()> {
        if handle::Handle::global().is_exiting() {
            logging!(
                debug,
                Type::Tray,
                "Приложение завершает работу, пропускаю создание трея"
            );
            return Ok(());
        }

        logging!(info, Type::Tray, "Создаю системный трей из AppHandle");

        let verge = Config::verge().await.data_arc();

        let icon_bytes = TrayState::get_tray_icon(&verge).await.1;

        #[cfg(target_os = "linux")]
        if linux::create_tray(&icon_bytes).await {
            return Ok(());
        }

        let icon = tauri::image::Image::from_bytes(&icon_bytes)?;

        #[cfg(target_os = "linux")]
        let builder = TrayIconBuilder::with_id(TRAY_ID).icon(icon).icon_as_template(false);

        #[cfg(any(target_os = "macos", target_os = "windows"))]
        let show_menu_on_left_click = verge.tray_event.as_ref().is_some_and(|v| v == "tray_menu");

        #[cfg(not(target_os = "linux"))]
        let mut builder = TrayIconBuilder::with_id(TRAY_ID).icon(icon).icon_as_template(false);
        #[cfg(target_os = "macos")]
        {
            let is_monochrome = verge.tray_icon.as_ref().is_none_or(|v| v == "monochrome");
            builder = builder.icon_as_template(is_monochrome);
        }

        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            if !show_menu_on_left_click {
                builder = builder.show_menu_on_left_click(false);
            }
        }

        let tray = builder.build(app_handle)?;
        tray.on_tray_icon_event(on_tray_icon_event);
        tray.on_menu_event(on_menu_event);
        Ok(())
    }

    fn should_handle_tray_click(&self) -> bool {
        let allow = self.limiter.check();
        if !allow {
            logging!(debug, Type::Tray, "tray click rate limited");
        }
        allow
    }

    fn should_handle_tray_activate(&self) -> bool {
        let allow = self.activate_limiter.check();
        if !allow {
            logging!(debug, Type::Tray, "tray activate rate limited");
        }
        allow
    }

    #[cfg(target_os = "macos")]
    pub fn update_speed_task(&self, enable_tray_speed: bool) {
        self.speed_controller.update_task(enable_tray_speed);
    }
}

fn create_hotkeys(hotkeys: &Option<Vec<String>>) -> HashMap<&str, &str> {
    hotkeys
        .as_ref()
        .map(|h| {
            h.iter()
                .filter_map(|item| {
                    let mut parts = item.split(',');
                    match (parts.next(), parts.next()) {
                        (Some(func), Some(key)) => {
                            if key.to_uppercase().contains("NUMPAD") {
                                None
                            } else {
                                Some((func, key))
                            }
                        }
                        _ => None,
                    }
                })
                .collect::<HashMap<&str, &str>>()
        })
        .unwrap_or_default()
}

fn create_profile_menu_item(profiles_preview: Vec<IProfilePreview<'_>>) -> Vec<MenuNode> {
    profiles_preview
        .into_iter()
        .map(|profile| {
            MenuNode::check(
                format!("profiles_{}", profile.uid),
                profile.name.to_string(),
                profile.is_current,
            )
        })
        .collect()
}

fn create_subcreate_proxy_menu_item(
    proxy_mode: &str,
    proxy_group_order_map: Option<HashMap<String, usize>>,
    proxy_nodes_data: Option<Proxies>,
) -> Vec<MenuNode> {
    let mut submenus: Vec<(String, usize, MenuNode)> = Vec::new();

    if let Some(proxy_nodes_data) = proxy_nodes_data {
        for (group_name, group_data) in proxy_nodes_data.proxies.iter() {
            let should_show = match proxy_mode {
                "global" => group_name == "GLOBAL",
                _ => group_name != "GLOBAL",
            } && !group_data.hidden.unwrap_or_default();

            if !should_show {
                continue;
            }

            let Some(all_proxies) = group_data.all.as_ref() else {
                continue;
            };

            let now_proxy = group_data.now.as_deref().unwrap_or_default();

            let group_items: Vec<MenuNode> = all_proxies
                .iter()
                .map(|proxy_str| {
                    let is_selected = *proxy_str == now_proxy;
                    let item_id = format!("proxy_{}_{}", group_name, proxy_str);

                    let delay_text = proxy_nodes_data
                        .proxies
                        .get(proxy_str)
                        .and_then(|h| h.history.last())
                        .map(|h| match h.delay {
                            0 => "-ms".into(),
                            delay if delay >= 10000 => "-ms".into(),
                            _ => format!("{}ms", h.delay),
                        })
                        .unwrap_or_else(|| "-ms".into());

                    MenuNode::check(item_id, format!("{}   | {}", proxy_str, delay_text), is_selected)
                })
                .collect();

            if group_items.is_empty() {
                continue;
            }

            let insertion_index = submenus.len();
            submenus.push((
                group_name.into(),
                insertion_index,
                MenuNode::sub(
                    format!("proxy_group_{}", group_name),
                    group_name.to_string(),
                    group_items,
                ),
            ));
        }
    }

    if let Some(order_map) = proxy_group_order_map.as_ref() {
        submenus.sort_by(|(name_a, original_index_a, _), (name_b, original_index_b, _)| {
            match (order_map.get(name_a), order_map.get(name_b)) {
                (Some(index_a), Some(index_b)) => index_a.cmp(index_b),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => original_index_a.cmp(original_index_b),
            }
        });
    }

    submenus.into_iter().map(|(_, _, submenu)| submenu).collect()
}

fn create_proxy_menu_item(
    show_proxy_groups_inline: bool,
    proxy_submenus: Vec<MenuNode>,
    proxies_text: &str,
) -> Vec<MenuNode> {
    if show_proxy_groups_inline {
        proxy_submenus
    } else if proxy_submenus.is_empty() {
        Vec::new()
    } else {
        vec![MenuNode::sub(MenuIds::PROXIES, proxies_text, proxy_submenus)]
    }
}

async fn create_tray_menu_model(
    mode: Option<&str>,
    system_proxy_enabled: bool,
    tun_mode_enabled: bool,
    tun_mode_available: bool,
    profiles_preview: Vec<IProfilePreview<'_>>,
    options: TrayMenuOptions,
) -> Result<Vec<MenuNode>> {
    let current_proxy_mode = mode.unwrap_or("");

    let verge_settings = Config::verge().await.latest_arc();
    let tray_proxy_groups_display_mode = verge_settings
        .tray_proxy_groups_display_mode
        .as_deref()
        .unwrap_or("default");
    let include_proxy_groups = options.include_proxy_groups && tray_proxy_groups_display_mode != "disable";

    let (proxy_nodes_data, runtime_proxy_groups_order) = if include_proxy_groups {
        let proxy_nodes_data = tokio::time::timeout(
            Duration::from_millis(1000),
            handle::Handle::mihomo().await.get_proxies(),
        )
        .await
        .map_or(None, |res| res.ok());

        let runtime_proxy_groups_order = cmd::get_runtime_config()
            .await
            .map_err(|e| {
                logging!(
                    error,
                    Type::Cmd,
                    "Failed to fetch runtime proxy groups for tray menu: {e}"
                );
            })
            .ok()
            .flatten()
            .map(|config| {
                config
                    .get("proxy-groups")
                    .and_then(|groups| groups.as_sequence())
                    .map(|groups| {
                        groups
                            .iter()
                            .filter_map(|group| group.get("name"))
                            .filter_map(|name| name.as_str())
                            .map(|name| name.into())
                            .collect::<Vec<String>>()
                    })
                    .unwrap_or_default()
            });

        (proxy_nodes_data, runtime_proxy_groups_order)
    } else {
        (None, None)
    };

    let proxy_group_order_map: Option<HashMap<smartstring::SmartString<smartstring::LazyCompact>, usize>> =
        runtime_proxy_groups_order.as_ref().map(|group_names| {
            group_names
                .iter()
                .enumerate()
                .map(|(index, name)| (name.clone(), index))
                .collect::<HashMap<String, usize>>()
        });

    let show_outbound_modes_inline = verge_settings.tray_inline_outbound_modes.unwrap_or(false);

    let mode_locked = {
        let profiles = Config::profiles().await.latest_arc();
        profiles
            .get_current()
            .and_then(|uid| profiles.get_item(uid).ok())
            .and_then(|item| item.lock_mode)
            .unwrap_or(false)
    };

    let version = env!("CARGO_PKG_VERSION");

    let hotkeys = create_hotkeys(&verge_settings.hotkeys);

    let texts = MenuTexts::new();

    let mut menu_items: Vec<MenuNode> = vec![
        MenuNode::item(MenuIds::DASHBOARD, texts.dashboard.as_ref())
            .with_accelerator(hotkeys.get("open_or_close_dashboard").copied()),
        MenuNode::Separator,
    ];

    let mode_items = || {
        vec![
            MenuNode::check(
                MenuIds::RULE_MODE,
                texts.rule_mode.as_ref(),
                current_proxy_mode == "rule",
            )
            .with_accelerator(hotkeys.get("clash_mode_rule").copied()),
            MenuNode::check(
                MenuIds::GLOBAL_MODE,
                texts.global_mode.as_ref(),
                current_proxy_mode == "global",
            )
            .with_accelerator(hotkeys.get("clash_mode_global").copied()),
            MenuNode::check(
                MenuIds::DIRECT_MODE,
                texts.direct_mode.as_ref(),
                current_proxy_mode == "direct",
            )
            .with_accelerator(hotkeys.get("clash_mode_direct").copied()),
        ]
    };

    if mode_locked {
    } else if show_outbound_modes_inline {
        menu_items.extend(mode_items());
    } else {
        let current_mode_text = match current_proxy_mode {
            "global" => clash_verge_i18n::t!("tray.global"),
            "direct" => clash_verge_i18n::t!("tray.direct"),
            _ => clash_verge_i18n::t!("tray.rule"),
        };
        menu_items.push(MenuNode::sub(
            MenuIds::OUTBOUND_MODES,
            format!("{} ({})", texts.outbound_modes, current_mode_text),
            mode_items(),
        ));
    }

    menu_items.push(MenuNode::Separator);
    menu_items.push(MenuNode::sub(
        MenuIds::PROFILES,
        texts.profiles.as_ref(),
        create_profile_menu_item(profiles_preview),
    ));

    if include_proxy_groups {
        let proxy_sub_menus =
            create_subcreate_proxy_menu_item(current_proxy_mode, proxy_group_order_map, proxy_nodes_data);

        match tray_proxy_groups_display_mode {
            "default" => menu_items.extend(create_proxy_menu_item(false, proxy_sub_menus, &texts.proxies)),
            "inline" => menu_items.extend(create_proxy_menu_item(true, proxy_sub_menus, &texts.proxies)),
            _ => {}
        }
    }

    let quit_accelerator = hotkeys.get("quit").copied();

    #[cfg(target_os = "macos")]
    let quit_accelerator = quit_accelerator.or(Some("Cmd+Q"));

    menu_items.extend([
        MenuNode::Separator,
        MenuNode::check(MenuIds::SYSTEM_PROXY, texts.system_proxy.as_ref(), system_proxy_enabled)
            .with_accelerator(hotkeys.get("toggle_system_proxy").copied()),
        MenuNode::check(MenuIds::TUN_MODE, texts.tun_mode.as_ref(), tun_mode_enabled)
            .with_enabled(tun_mode_available)
            .with_accelerator(hotkeys.get("toggle_tun_mode").copied()),
        MenuNode::Separator,
        MenuNode::check(
            MenuIds::LIGHTWEIGHT_MODE,
            texts.lightweight_mode.as_ref(),
            options.is_lightweight_mode,
        )
        .with_accelerator(hotkeys.get("entry_lightweight_mode").copied()),
        MenuNode::sub(
            MenuIds::OPEN_DIR,
            texts.open_dir.as_ref(),
            vec![
                MenuNode::item(MenuIds::CONF_DIR, texts.conf_dir.as_ref()),
                MenuNode::item(MenuIds::CORE_DIR, texts.core_dir.as_ref()),
                MenuNode::item(MenuIds::LOGS_DIR, texts.logs_dir.as_ref()),
                MenuNode::item(MenuIds::APP_LOG, texts.app_log.as_ref()),
                MenuNode::item(MenuIds::CORE_LOG, texts.core_log.as_ref()),
            ],
        ),
        MenuNode::sub(
            MenuIds::MORE,
            texts.more.as_ref(),
            vec![
                MenuNode::item(MenuIds::COPY_ENV, texts.copy_env.as_ref()),
                MenuNode::item(MenuIds::CLOSE_ALL_CONNECTIONS, texts.close_all_connections.as_ref()),
                MenuNode::item(MenuIds::RESTART_CLASH, texts.restart_clash.as_ref()),
                MenuNode::item(MenuIds::RESTART_APP, texts.restart_app.as_ref()),
                MenuNode::item(MenuIds::VERGE_VERSION, format!("{} {version}", texts.verge_version)),
            ],
        ),
        MenuNode::Separator,
        MenuNode::item(MenuIds::EXIT, texts.exit.as_ref()).with_accelerator(quit_accelerator),
    ]);

    Ok(menu_items)
}

fn render_menu_node(app_handle: &AppHandle, node: MenuNode) -> Option<Box<dyn IsMenuItem<Wry>>> {
    let rendered: tauri::Result<Box<dyn IsMenuItem<Wry>>> = match node {
        MenuNode::Separator => PredefinedMenuItem::separator(app_handle).map(|item| Box::new(item) as _),
        MenuNode::Item {
            id,
            label,
            enabled,
            accelerator,
        } => MenuItem::with_id(app_handle, id.as_ref(), label, enabled, accelerator.as_deref())
            .map(|item| Box::new(item) as _),
        MenuNode::Check {
            id,
            label,
            enabled,
            checked,
            accelerator,
        } => CheckMenuItem::with_id(app_handle, id.as_ref(), label, enabled, checked, accelerator.as_deref())
            .map(|item| Box::new(item) as _),
        MenuNode::Sub {
            id,
            label,
            enabled,
            children,
        } => {
            let rendered = render_menu_nodes(app_handle, children);
            let refs: Vec<&dyn IsMenuItem<Wry>> = rendered.iter().map(AsRef::as_ref).collect();
            Submenu::with_id_and_items(app_handle, id.as_ref(), label, enabled, &refs).map(|item| Box::new(item) as _)
        }
    };

    rendered
        .map_err(|e| logging!(warn, Type::Tray, "Failed to create tray menu item: {e}"))
        .ok()
}

fn render_menu_nodes(app_handle: &AppHandle, nodes: Vec<MenuNode>) -> Vec<Box<dyn IsMenuItem<Wry>>> {
    nodes
        .into_iter()
        .filter_map(|node| render_menu_node(app_handle, node))
        .collect()
}

fn render_tray_menu(app_handle: &AppHandle, nodes: Vec<MenuNode>) -> Result<tauri::menu::Menu<Wry>> {
    let expected = nodes.len();
    let rendered = render_menu_nodes(app_handle, nodes);
    if rendered.len() != expected {
        anyhow::bail!("не удалось построить {} пунктов меню трея", expected - rendered.len());
    }
    let refs: Vec<&dyn IsMenuItem<Wry>> = rendered.iter().map(AsRef::as_ref).collect();
    Ok(tauri::menu::MenuBuilder::new(app_handle).items(&refs).build()?)
}

fn handle_primary_click() {
    #[allow(clippy::use_self)]
    if !Tray::global().should_handle_tray_activate() {
        return;
    }

    AsyncHandler::spawn(|| async move {
        let verge = Config::verge().await.data_arc();
        let verge_tray_event = verge.tray_event.clone().unwrap_or_else(|| "main_window".into());
        let verge_tray_action = TrayAction::from(verge_tray_event.as_str());
        logging!(debug, Type::Tray, "tray event: {verge_tray_action:?}");
        let show_main_window = || async {
            if !lightweight::exit_lightweight_mode().await {
                WindowManager::show_main_window().await;
            }
        };
        match verge_tray_action {
            TrayAction::SystemProxy => {
                let _ = feat::toggle_system_proxy().await;
            }
            TrayAction::TunMode => {
                let _ = feat::toggle_tun_mode(None).await;
            }
            TrayAction::MainWindow => show_main_window().await,
            TrayAction::TrayMenu => {
                #[cfg(target_os = "linux")]
                show_main_window().await;
            }
            TrayAction::Unknown => {
                logging!(warn, Type::Tray, "invalid tray event: {}", verge_tray_event);
            }
        };
    });
}

fn on_tray_icon_event(_tray_icon: &TrayIcon, tray_event: TrayIconEvent) {
    if matches!(
        tray_event,
        TrayIconEvent::Move { .. } | TrayIconEvent::Leave { .. } | TrayIconEvent::Enter { .. }
    ) {
        return;
    }

    if let TrayIconEvent::Click {
        button: MouseButton::Left,
        button_state: MouseButtonState::Down,
        ..
    } = tray_event
    {
        handle_primary_click();
    }
}

fn on_menu_event(_: &AppHandle, event: MenuEvent) {
    handle_menu_click(event.id.as_ref().into());
}

fn handle_menu_click(id: std::string::String) {
    if !Tray::global().should_handle_tray_click() {
        return;
    }
    if id.is_empty() {
        return;
    }
    AsyncHandler::spawn(|| async move {
        match id.as_str() {
            mode @ (MenuIds::RULE_MODE | MenuIds::GLOBAL_MODE | MenuIds::DIRECT_MODE) => {
                if let Some(stripped) = mode.strip_prefix("tray_")
                    && let Some(final_mode) = stripped.strip_suffix("_mode")
                {
                    logging!(info, Type::ProxyMode, "Switch Proxy Mode To: {}", final_mode);
                    let _ = feat::change_clash_mode(final_mode.into()).await;
                }
            }
            MenuIds::DASHBOARD => {
                logging!(info, Type::Tray, "Клик по меню трея: открываю окно");
                if !lightweight::exit_lightweight_mode().await {
                    WindowManager::show_main_window().await;
                };
            }
            MenuIds::SYSTEM_PROXY => {
                feat::toggle_system_proxy().await;
            }
            MenuIds::TUN_MODE => {
                feat::toggle_tun_mode(None).await;
            }
            MenuIds::CLOSE_ALL_CONNECTIONS => {
                if let Err(err) = handle::Handle::mihomo().await.close_all_connections().await {
                    logging!(error, Type::Tray, "Failed to close all connections from tray: {err}");
                }
            }
            MenuIds::COPY_ENV => feat::copy_clash_env().await,
            MenuIds::CONF_DIR => {
                let _ = cmd::open_app_dir().await;
            }
            MenuIds::CORE_DIR => {
                let _ = cmd::open_core_dir().await;
            }
            MenuIds::LOGS_DIR => {
                let _ = cmd::open_logs_dir().await;
            }
            MenuIds::APP_LOG => {
                let _ = help::open_app_latest_log();
            }
            MenuIds::CORE_LOG => {
                let _ = help::open_core_latest_log();
            }
            MenuIds::RESTART_CLASH => feat::restart_clash_core().await,
            MenuIds::RESTART_APP => feat::restart_app().await,
            MenuIds::LIGHTWEIGHT_MODE => {
                if !is_in_lightweight_mode() {
                    lightweight::entry_lightweight_mode().await;
                } else {
                    lightweight::exit_lightweight_mode().await;
                }
            }
            MenuIds::EXIT => {
                feat::quit().await;
            }
            id if id.starts_with("profiles_") => {
                let profile_index = match id.strip_prefix("profiles_") {
                    Some(index_str) => index_str,
                    None => return,
                };
                feat::toggle_proxy_profile(profile_index.into()).await;
            }
            id if id.starts_with("proxy_") => {
                let rest = match id.strip_prefix("proxy_") {
                    Some(r) => r,
                    None => return,
                };
                let (group_name, proxy_name) = match rest.split_once('_') {
                    Some((g, p)) => (g, p),
                    None => return,
                };
                feat::switch_proxy_node(group_name, proxy_name).await;
            }
            _ => {
                logging!(debug, Type::Tray, "Unhandled tray menu event: {id}");
            }
        }
    });
}
