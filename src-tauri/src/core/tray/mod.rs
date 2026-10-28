use crate::config::{IProfilePreview, IVerge};
use crate::core::sysopt::Sysopt;
use crate::core::tray::menu_def::TrayAction;
use crate::module::lightweight;
use crate::process::AsyncHandler;
use crate::singleton;
use crate::utils::notification::{NotificationEvent, notify_event};
use crate::{Type, cmd, config::Config, feat, logging, module::lightweight::is_in_lightweight_mode, utils::help};
use clash_verge_limiter::{Limiter, SystemClock, SystemLimiter};
use clash_verge_logging::logging_error;
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri_plugin_mihomo::models::Proxies;
use tokio::fs;

use super::handle;
use anyhow::Result;
use smartstring::alias::String;
use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tauri::{
    AppHandle, Wry,
    menu::{CheckMenuItem, IsMenuItem, MenuEvent, MenuItem, PredefinedMenuItem, Submenu},
};

async fn report_log_not_opened(dir: Option<std::path::PathBuf>, error: &anyhow::Error) {
    logging!(error, Type::Tray, "не удалось открыть журнал из трея: {error:#}");
    let path = dir.map(|dir| dir.display().to_string()).unwrap_or_default();
    crate::utils::notification::notify_event(crate::utils::notification::NotificationEvent::LogOpenFailed {
        path: &path,
    })
    .await;
}

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

/// Что о системном прокси показано: меню, значок и подсказка рисуются порознь
/// и могут знать о нём разное.
#[derive(Default)]
struct ShownSystemProxy {
    menu: AtomicBool,
    icon: AtomicBool,
    tooltip: AtomicBool,
}

impl ShownSystemProxy {
    fn agree_with(&self, on: bool) -> bool {
        [&self.menu, &self.icon, &self.tooltip]
            .into_iter()
            .all(|shown| shown.load(Ordering::Relaxed) == on)
    }
}

pub struct Tray {
    limiter: SystemLimiter,
    shown_system_proxy: ShownSystemProxy,
    #[cfg(target_os = "linux")]
    activate_limiter: SystemLimiter,
    #[cfg(target_os = "macos")]
    speed_controller: speed_task::TraySpeedController,
}

fn core_is_down() -> bool {
    crate::core::CoreManager::global().is_down()
}

impl TrayState {
    /// Значок и то, стоял ли в системе наш прокси, когда значок выбирали.
    async fn get_tray_icon(verge: &IVerge) -> (bool, Cow<'_, [u8]>) {
        let tun_mode = feat::tun::is_active_with(verge.enable_tun_mode.unwrap_or(false));
        let system_proxy = Sysopt::global().our_proxy_is_on().await;
        let kind = if core_is_down() {
            IconKind::Common
        } else if tun_mode {
            IconKind::Tun
        } else if system_proxy {
            IconKind::SysProxy
        } else {
            IconKind::Common
        };
        (system_proxy, Self::load_icon(verge, kind).await.1)
    }

    async fn load_icon(verge: &IVerge, kind: IconKind) -> (bool, Cow<'_, [u8]>) {
        let (custom_enabled, icon_name) = match kind {
            IconKind::Common => (verge.common_tray_icon.unwrap_or(false), "common"),
            IconKind::SysProxy => (verge.sysproxy_tray_icon.unwrap_or(false), "sysproxy"),
            IconKind::Tun => (verge.tun_tray_icon.unwrap_or(false), "tun"),
        };

        if custom_enabled
            && let Ok(path) = crate::feat::tray_icon_path(icon_name).await
            && !path.is_empty()
            && let Ok(data) = fs::read(path.as_str()).await
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
            shown_system_proxy: ShownSystemProxy::default(),
            #[cfg(target_os = "linux")]
            activate_limiter: Limiter::new(Duration::from_millis(TRAY_CLICK_DEBOUNCE_MS), SystemClock),
            #[cfg(target_os = "macos")]
            speed_controller: speed_task::TraySpeedController::new(),
        }
    }
}

singleton!(Tray, TRAY);

/// Под Linux запасного трея законно нет, пока свой ждёт службу значков.
#[cfg(target_os = "linux")]
fn the_tray_is_missed() -> bool {
    !linux::still_waits_for_the_watcher()
}
#[cfg(not(target_os = "linux"))]
const fn the_tray_is_missed() -> bool {
    true
}

/// Служба значков пришла или ушла: виден должен быть ровно один трей — свой,
/// пока она на связи, запасной — когда её нет дольше отведённого срока. Тот,
/// что был не у дел, обновлений не получал, поэтому показанный обновляется
/// целиком. Из нескольких событий подряд дело доводит последнее.
#[cfg(target_os = "linux")]
fn show_the_tray_that_works() {
    static TURN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    static ONE_AT_A_TIME: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    let turn = TURN.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
    AsyncHandler::spawn(move || async move {
        if !ksni_active() {
            logging!(
                warn,
                Type::Tray,
                "Служба значков не на связи, ждём её {} с до запасного трея",
                linux::WATCHER_GRACE.as_secs()
            );
            tokio::time::sleep(linux::WATCHER_GRACE).await;
        }
        let _one = ONE_AT_A_TIME.lock().await;
        if TURN.load(std::sync::atomic::Ordering::SeqCst) != turn || handle::Handle::global().is_exiting() {
            return;
        }
        let own_tray_works = ksni_active();
        logging!(info, Type::Tray, "Служба значков на связи: {own_tray_works}");
        let app_handle = handle::Handle::app_handle();
        let tray = Tray::global();
        if !own_tray_works {
            linux::the_wait_is_over();
            logging_error!(Type::Tray, tray.create_tray_from_handle(app_handle).await);
        }
        if let Some(fallback) = app_handle.tray_by_id(TRAY_ID) {
            logging_error!(Type::Tray, fallback.set_visible(!own_tray_works));
        }
        tray.redraw().await;
    });
}

impl Tray {
    fn new() -> Self {
        Self::default()
    }

    /// Событие службы значков, пришедшее во время выхода, пропускается; если
    /// выход отменён, трей доводится до нынешнего положения дел.
    #[cfg(target_os = "linux")]
    pub fn catch_up_after_a_cancelled_exit() {
        show_the_tray_that_works();
    }

    pub async fn init(&self) -> Result<()> {
        if skipped_while_exiting("инициализацию трея") {
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
        if skipped_while_exiting("обновление поведения клика по трею") {
            return Ok(());
        }

        if ksni_active() {
            return Ok(());
        }

        let app_handle = handle::Handle::app_handle();
        #[cfg(target_os = "linux")]
        if app_handle.tray_by_id(TRAY_ID).is_none() {
            return Ok(());
        }
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
        if skipped_while_exiting("обновление меню трея") {
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
                if the_tray_is_missed() {
                    logging!(warn, Type::Tray, "Failed to update tray menu: tray not found");
                }
                return Ok(());
            };
            Some(tray)
        };

        let verge = Config::verge().await.latest_arc();
        let system_proxy = Sysopt::global().our_proxy_is_on().await;
        self.shown_system_proxy.menu.store(system_proxy, Ordering::Relaxed);
        let tun_mode = feat::tun::is_active_with(verge.enable_tun_mode.unwrap_or(false));
        let mode = Config::runtime()
            .await
            .latest_arc()
            .config
            .as_ref()
            .and_then(|config| config.get("mode"))
            .and_then(|val| val.as_str())
            .unwrap_or("rule")
            .to_owned();
        let profiles_config = Config::profiles().await;
        let profiles_arc = profiles_config.latest_arc();
        let profiles_preview = profiles_arc.profiles_preview().unwrap_or_default();
        let is_lightweight_mode = is_in_lightweight_mode();

        let menu_model = create_tray_menu_model(
            Some(mode.as_str()),
            system_proxy,
            tun_mode,
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
        if skipped_while_exiting("обновление иконки трея") {
            return Ok(());
        }

        let app_handle = handle::Handle::app_handle();

        let tray = if ksni_active() {
            None
        } else {
            let Some(tray) = app_handle.tray_by_id(TRAY_ID) else {
                if the_tray_is_missed() {
                    logging!(warn, Type::Tray, "Failed to update tray icon: tray not found");
                }
                return Ok(());
            };
            Some(tray)
        };

        let (system_proxy, icon_bytes) = TrayState::get_tray_icon(verge).await;
        self.shown_system_proxy.icon.store(system_proxy, Ordering::Relaxed);

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
        if skipped_while_exiting("обновление подсказки трея") {
            return Ok(());
        }

        let app_handle = handle::Handle::app_handle();

        let verge = Config::verge().await.latest_arc();
        let system_proxy = Sysopt::global().our_proxy_is_on().await;
        self.shown_system_proxy.tooltip.store(system_proxy, Ordering::Relaxed);
        let tun_mode = feat::tun::is_active_with(verge.enable_tun_mode.unwrap_or(false));

        let switch_str = |flag: bool| {
            if flag {
                clash_verge_i18n::t!("tray.tooltip.on")
            } else {
                clash_verge_i18n::t!("tray.tooltip.off")
            }
        };

        let mut current_profile_name = clash_verge_i18n::t!("tray.tooltip.noProfile").to_string();
        {
            let profiles = Config::profiles().await;
            let profiles = profiles.latest_arc();
            if let Some(current_profile_uid) = profiles.get_current()
                && let Ok(profile) = profiles.get_item(current_profile_uid)
            {
                current_profile_name = match profile.display_name() {
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

        let mut tooltip = format!(
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
        if core_is_down() {
            tooltip.push_str(&format!(
                "\n{}: {}",
                clash_verge_i18n::t!("tray.tooltip.core"),
                clash_verge_i18n::t!("tray.tooltip.coreStopped")
            ));
        }

        #[cfg(target_os = "linux")]
        if ksni_active() {
            linux::update_tooltip(tooltip).await;
            return Ok(());
        }

        let Some(tray) = app_handle.tray_by_id(TRAY_ID) else {
            if the_tray_is_missed() {
                logging!(warn, Type::Tray, "Failed to update tray tooltip: tray not found");
            }
            return Ok(());
        };

        logging_error!(Type::Tray, tray.set_tooltip(Some(&tooltip)));

        Ok(())
    }

    pub async fn update_part(&self) -> Result<()> {
        if skipped_while_exiting("частичное обновление трея") {
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

    pub async fn refresh_core_state(&self) {
        if handle::Handle::global().is_exiting() {
            return;
        }
        if !ksni_active() && handle::Handle::app_handle().tray_by_id(TRAY_ID).is_none() {
            return;
        }
        let verge = Config::verge().await.latest_arc();
        logging_error!(Type::Tray, self.update_icon(&verge).await);
        logging_error!(Type::Tray, self.update_tooltip().await);
    }

    pub async fn update_menu_and_icon(&self) {
        logging_error!(Type::Tray, self.update_menu().await);
        let verge = Config::verge().await.data_arc();
        logging_error!(Type::Tray, self.update_icon(&verge).await);
    }

    /// Трей целиком — меню, значок и подсказка.
    pub async fn redraw(&self) {
        self.update_menu_and_icon().await;
        logging_error!(Type::Tray, self.update_tooltip().await);
    }

    async fn create_tray_from_handle(&self, app_handle: &AppHandle) -> Result<()> {
        if skipped_while_exiting("создание трея") {
            return Ok(());
        }

        logging!(info, Type::Tray, "Создаю системный трей из AppHandle");

        let verge = Config::verge().await.data_arc();

        let (system_proxy, icon_bytes) = TrayState::get_tray_icon(&verge).await;
        self.shown_system_proxy.icon.store(system_proxy, Ordering::Relaxed);

        #[cfg(target_os = "linux")]
        if linux::create_tray(&icon_bytes).await || app_handle.tray_by_id(TRAY_ID).is_some() {
            return Ok(());
        }
        #[cfg(target_os = "linux")]
        if linux::still_waits_for_the_watcher() {
            show_the_tray_that_works();
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
        #[cfg(not(target_os = "linux"))]
        return self.should_handle_tray_click();

        #[cfg(target_os = "linux")]
        {
            let allow = self.activate_limiter.check();
            if !allow {
                logging!(debug, Type::Tray, "tray activate rate limited");
            }
            allow
        }
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
                            tray_menu_accepts(key, cfg!(target_os = "windows")).then_some((func, key))
                        }
                        _ => None,
                    }
                })
                .collect::<HashMap<&str, &str>>()
        })
        .unwrap_or_default()
}

// Сочетание, которое меню трея не примет, роняет сборку всего меню: клавиши
// цифрового блока не разбираются нигде, а на Windows ещё и модификатор Win
// (vendor/muda отвергает его как модификатор пункта меню).
fn tray_menu_accepts(key: &str, windows: bool) -> bool {
    key.split('+').map(str::trim).all(|token| {
        let token = token.to_uppercase();
        let win_key = windows && matches!(token.as_str(), "CMD" | "COMMAND" | "SUPER" | "META");
        !(token.contains("NUMPAD") || win_key)
    })
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

/// Имена группы и узла бывают с любыми знаками, поэтому в номере пункта перед
/// ними стоит длина имени группы — иначе их не разделить обратно.
fn proxy_item_id(group: &str, proxy: &str) -> std::string::String {
    format!("proxy_{}_{group}{proxy}", group.len())
}

fn proxy_item_names(id: &str) -> Option<(&str, &str)> {
    let (group_len, names) = id.strip_prefix("proxy_")?.split_once('_')?;
    names.split_at_checked(group_len.parse().ok()?)
}

fn node_title(name: &str) -> std::string::String {
    if name == "DIRECT" {
        clash_verge_i18n::t!("tray.directNode").to_string()
    } else {
        name.to_owned()
    }
}

pub(crate) fn group_decides_traffic(proxy_mode: &str, group_name: &str) -> bool {
    match proxy_mode {
        "global" => group_name == "GLOBAL",
        "direct" => false,
        _ => group_name != "GLOBAL",
    }
}

fn create_subcreate_proxy_menu_item(
    proxy_mode: &str,
    proxy_group_order_map: Option<HashMap<String, usize>>,
    proxy_nodes_data: Option<Proxies>,
) -> Vec<MenuNode> {
    let mut submenus: Vec<(String, usize, MenuNode)> = Vec::new();
    // clod:freeze — в меню картинок нет, поэтому пометка значком после имени.
    let freeze_marks = crate::module::freeze_check::marks();

    if let Some(proxy_nodes_data) = proxy_nodes_data {
        for (group_name, group_data) in proxy_nodes_data.proxies.iter() {
            let should_show = group_decides_traffic(proxy_mode, group_name) && !group_data.hidden.unwrap_or_default();

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
                    let item_id = proxy_item_id(group_name, proxy_str);

                    // Задержек в трее нет: меню собирается по событиям, и цифры в нём
                    // устаревали, а у узлов провайдеров их не было вовсе
                    let title = node_title(proxy_str);
                    let text = if freeze_marks.contains(proxy_str.as_str()) {
                        format!("{title} ❄")
                    } else {
                        title
                    };
                    MenuNode::check(item_id, text, is_selected).skippable()
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
                )
                .skippable(),
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
        let proxy_nodes_data =
            tokio::time::timeout(Duration::from_millis(1000), handle::Handle::mihomo().get_proxies())
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

    let (mode_locked, (system_proxy_allowed, tun_allowed)) = {
        let profiles = Config::profiles().await.latest_arc();
        let current = profiles.get_current().and_then(|uid| profiles.get_item(uid).ok());
        (
            current.and_then(|item| item.lock_mode).unwrap_or(false),
            feat::allowed_targets(current),
        )
    };

    let version = env!("CARGO_PKG_VERSION");

    let hotkeys = create_hotkeys(&verge_settings.hotkeys);

    let texts = MenuTexts::new();

    let mut menu_items: Vec<MenuNode> = vec![
        MenuNode::item(MenuIds::DASHBOARD, texts.dashboard.as_ref())
            .with_accelerator(hotkeys.get("open_or_close_dashboard").copied()),
        MenuNode::Separator,
    ];

    // В подменю «Режим: …» пункты — одним словом, прямо в меню — «Режим: …».
    let mode_items = |labels: [std::borrow::Cow<'static, str>; 3]| {
        let [rule, global, direct] = labels;
        vec![
            MenuNode::check(MenuIds::RULE_MODE, rule.as_ref(), current_proxy_mode == "rule")
                .with_accelerator(hotkeys.get("clash_mode_rule").copied()),
            MenuNode::check(MenuIds::GLOBAL_MODE, global.as_ref(), current_proxy_mode == "global")
                .with_accelerator(hotkeys.get("clash_mode_global").copied()),
            MenuNode::check(MenuIds::DIRECT_MODE, direct.as_ref(), current_proxy_mode == "direct")
                .with_accelerator(hotkeys.get("clash_mode_direct").copied()),
        ]
    };

    if mode_locked {
    } else if show_outbound_modes_inline {
        menu_items.extend(mode_items([
            texts.rule_mode.clone(),
            texts.global_mode.clone(),
            texts.direct_mode.clone(),
        ]));
    } else {
        let current_mode_text = match current_proxy_mode {
            "global" => clash_verge_i18n::t!("tray.global"),
            "direct" => clash_verge_i18n::t!("tray.direct"),
            _ => clash_verge_i18n::t!("tray.rule"),
        };
        menu_items.push(MenuNode::sub(
            MenuIds::OUTBOUND_MODES,
            format!("{}: {}", texts.outbound_modes, current_mode_text),
            mode_items([
                clash_verge_i18n::t!("tray.rule"),
                clash_verge_i18n::t!("tray.global"),
                clash_verge_i18n::t!("tray.direct"),
            ]),
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

    menu_items.push(MenuNode::Separator);
    if system_proxy_allowed {
        menu_items.push(
            MenuNode::check(MenuIds::SYSTEM_PROXY, texts.system_proxy.as_ref(), system_proxy_enabled)
                .with_accelerator(hotkeys.get("toggle_system_proxy").copied()),
        );
    }
    if tun_allowed {
        menu_items.push(
            MenuNode::check(MenuIds::TUN_MODE, texts.tun_mode.as_ref(), tun_mode_enabled)
                .with_accelerator(hotkeys.get("toggle_tun_mode").copied()),
        );
    }
    menu_items.extend([
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

fn render_menu_node(
    app_handle: &AppHandle,
    separator: &PredefinedMenuItem<Wry>,
    node: MenuNode,
) -> Result<Option<Box<dyn IsMenuItem<Wry>>>> {
    let (skippable, group_name, rendered): (
        bool,
        Option<std::string::String>,
        tauri::Result<Box<dyn IsMenuItem<Wry>>>,
    ) = match node {
        MenuNode::Separator => (false, None, Ok(Box::new(separator.clone()))),
        MenuNode::Item {
            id,
            label,
            enabled,
            accelerator,
            skippable,
        } => (
            skippable,
            None,
            MenuItem::with_id(app_handle, id.as_ref(), label, enabled, accelerator.as_deref())
                .map(|item| Box::new(item) as _),
        ),
        MenuNode::Check {
            id,
            label,
            enabled,
            checked,
            accelerator,
            skippable,
        } => (
            skippable,
            None,
            CheckMenuItem::with_id(app_handle, id.as_ref(), label, enabled, checked, accelerator.as_deref())
                .map(|item| Box::new(item) as _),
        ),
        MenuNode::Sub {
            id,
            label,
            enabled,
            children,
            skippable,
        } => {
            let expected = children.len();
            let rendered = render_menu_nodes(app_handle, separator, children)?;
            if expected > 0 && rendered.is_empty() {
                return Ok(None);
            }
            let refs: Vec<&dyn IsMenuItem<Wry>> = rendered.iter().map(AsRef::as_ref).collect();
            let group_name = skippable.then(|| label.clone());
            (
                skippable,
                group_name,
                Submenu::with_id_and_items(app_handle, id.as_ref(), label, enabled, &refs)
                    .map(|item| Box::new(item) as _),
            )
        }
    };

    match rendered {
        Ok(item) => Ok(Some(item)),
        Err(e) if skippable => {
            match group_name {
                Some(group_name) => logging!(warn, Type::Tray, "Failed to create proxy group submenu: {group_name}"),
                None => logging!(warn, Type::Tray, "Failed to create proxy menu item: {e}"),
            }
            Ok(None)
        }
        Err(e) => Err(e.into()),
    }
}

fn render_menu_nodes(
    app_handle: &AppHandle,
    separator: &PredefinedMenuItem<Wry>,
    nodes: Vec<MenuNode>,
) -> Result<Vec<Box<dyn IsMenuItem<Wry>>>> {
    let mut rendered = Vec::with_capacity(nodes.len());
    for node in nodes {
        if let Some(item) = render_menu_node(app_handle, separator, node)? {
            rendered.push(item);
        }
    }
    Ok(rendered)
}

fn render_tray_menu(app_handle: &AppHandle, nodes: Vec<MenuNode>) -> Result<tauri::menu::Menu<Wry>> {
    let separator = PredefinedMenuItem::separator(app_handle)?;
    let rendered = render_menu_nodes(app_handle, &separator, nodes)?;
    let refs: Vec<&dyn IsMenuItem<Wry>> = rendered.iter().map(AsRef::as_ref).collect();
    Ok(tauri::menu::MenuBuilder::new(app_handle).items(&refs).build()?)
}

fn skipped_while_exiting(what: &str) -> bool {
    if !handle::Handle::global().is_exiting() {
        return false;
    }
    logging!(debug, Type::Tray, "Приложение завершает работу, пропускаю {what}");
    true
}

fn refused_while_exiting() -> bool {
    if !handle::Handle::global().is_exiting() {
        return false;
    }
    logging!(info, Type::Tray, "действие из трея отклонено: выход уже идёт");
    handle::Handle::notice_message(crate::core::notification::EXIT_REFUSAL_STATUS, "");
    true
}

fn handle_primary_click() {
    #[allow(clippy::use_self)]
    if !Tray::global().should_handle_tray_activate() {
        return;
    }
    if refused_while_exiting() {
        return;
    }

    AsyncHandler::spawn(|| async move {
        let verge = Config::verge().await.data_arc();
        let verge_tray_event = verge.tray_event.clone().unwrap_or_else(|| "main_window".into());
        let verge_tray_action = TrayAction::from(verge_tray_event.as_str());
        logging!(debug, Type::Tray, "tray event: {verge_tray_action:?}");
        match verge_tray_action {
            TrayAction::SystemProxy => {
                if feat::toggle_system_proxy().await == feat::Toggled::Locked {
                    notify_event(NotificationEvent::ConnectMethodLocked { tun: false }).await;
                }
            }
            TrayAction::TunMode => {
                if feat::toggle_tun_mode().await == feat::Toggled::Locked {
                    notify_event(NotificationEvent::ConnectMethodLocked { tun: true }).await;
                }
            }
            TrayAction::MainWindow => lightweight::wake_or_show().await,
            TrayAction::TrayMenu => {
                #[cfg(target_os = "linux")]
                lightweight::wake_or_show().await;
            }
            TrayAction::Disabled => {}
            TrayAction::Unknown => {
                logging!(warn, Type::Tray, "invalid tray event: {}", verge_tray_event);
            }
        };
    });
}

/// Прокси могут снять мимо приложения: с системой трей сверяется на каждом
/// тике сторожа среды, а там, где значок сообщает о наведении мыши (Windows,
/// macOS), ещё и перед открытием меню. Сверка — с каждым из трёх элементов:
/// меню обновляется чаще и само по себе значок и подсказку не чинит. Одна
/// сверка за раз: зависшее чтение системы не копит за собой новые.
pub fn catch_up_with_the_system_proxy() {
    static CATCHING_UP: AtomicBool = AtomicBool::new(false);
    if handle::Handle::global().is_exiting() || CATCHING_UP.swap(true, Ordering::AcqRel) {
        return;
    }
    AsyncHandler::spawn(|| async {
        let _done = scopeguard::guard((), |()| CATCHING_UP.store(false, Ordering::Release));
        let tray = Tray::global();
        if tray
            .shown_system_proxy
            .agree_with(Sysopt::global().our_proxy_is_on().await)
        {
            return;
        }
        tray.redraw().await;
    });
}

fn on_tray_icon_event(_tray_icon: &TrayIcon, tray_event: TrayIconEvent) {
    match tray_event {
        TrayIconEvent::Enter { .. } => catch_up_with_the_system_proxy(),
        TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Down,
            ..
        } => handle_primary_click(),
        _ => {}
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
    if refused_while_exiting() {
        return;
    }
    AsyncHandler::spawn(|| async move {
        match id.as_str() {
            mode @ (MenuIds::RULE_MODE | MenuIds::GLOBAL_MODE | MenuIds::DIRECT_MODE) => {
                if let Some(stripped) = mode.strip_prefix("tray_")
                    && let Some(final_mode) = stripped.strip_suffix("_mode")
                {
                    logging!(info, Type::ProxyMode, "Switch Proxy Mode To: {}", final_mode);
                    if feat::change_clash_mode(final_mode.into()).await.is_err() {
                        logging_error!(Type::Tray, Tray::global().update_menu().await);
                    }
                }
            }
            MenuIds::DASHBOARD => {
                logging!(info, Type::Tray, "Клик по меню трея: открываю окно");
                lightweight::wake_or_show().await;
            }
            // Галочку у пункта меню система переворачивает сама, не дожидаясь
            // ответа: при отказе её возвращает только пересборка меню.
            MenuIds::SYSTEM_PROXY => {
                feat::toggle_system_proxy().await;
                logging_error!(Type::Tray, Tray::global().update_menu().await);
            }
            MenuIds::TUN_MODE => {
                feat::toggle_tun_mode().await;
                logging_error!(Type::Tray, Tray::global().update_menu().await);
            }
            MenuIds::CLOSE_ALL_CONNECTIONS => {
                if let Err(err) = handle::Handle::mihomo().close_all_connections().await {
                    logging!(error, Type::Tray, "Failed to close all connections from tray: {err}");
                }
            }
            MenuIds::COPY_ENV => {
                if let Err(err) = feat::copy_clash_env().await {
                    logging!(error, Type::Tray, "{err}");
                }
            }
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
                if let Err(err) = help::open_app_latest_log() {
                    report_log_not_opened(crate::utils::dirs::app_logs_dir().ok(), &err).await;
                }
            }
            MenuIds::CORE_LOG => {
                if let Err(err) = help::open_core_log().await {
                    report_log_not_opened(
                        crate::utils::dirs::clash_latest_log()
                            .ok()
                            .and_then(|path| path.parent().map(std::path::Path::to_path_buf)),
                        &err,
                    )
                    .await;
                }
            }
            MenuIds::RESTART_CLASH => match feat::restart_clash_core().await {
                Ok(()) => {
                    crate::core::notification::tell_the_outcome(
                        "set_config::ok",
                        "ok".to_owned(),
                        crate::utils::notification::NotificationEvent::CoreRestarted,
                    )
                    .await;
                }
                Err(err) => {
                    logging!(error, Type::Core, "{err}");
                    let reason = crate::cmd::public_error_text(&err);
                    crate::core::notification::tell_the_outcome(
                        "set_config::error",
                        format!("{err}"),
                        crate::utils::notification::NotificationEvent::CoreNotRestarted { reason: &reason },
                    )
                    .await;
                }
            },
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
                let Some((group_name, proxy_name)) = proxy_item_names(id) else {
                    return;
                };
                feat::switch_proxy_node(group_name, proxy_name).await;
            }
            _ => {
                logging!(
                    debug,
                    Type::Tray,
                    "Unhandled tray menu event: {:?}",
                    tauri::menu::MenuId::new(&id)
                );
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{
        ShownSystemProxy, group_decides_traffic, node_title, proxy_item_id, proxy_item_names, tray_menu_accepts,
    };
    use std::sync::atomic::AtomicBool;

    #[test]
    fn the_tray_menu_skips_keys_it_cannot_show() {
        for (key, windows, accepted) in [
            ("CTRL+SHIFT+P", true, true),
            ("CMD+SHIFT+P", true, false),
            ("CMD+SHIFT+P", false, true),
            ("Super+P", true, false),
            ("META+P", true, false),
            ("Command+P", true, false),
            ("CmdOrCtrl+P", true, true),
            ("CTRL+NUMPAD1", true, false),
            ("CTRL+NUMPAD1", false, false),
            ("ALT+F4", false, true),
        ] {
            assert_eq!(tray_menu_accepts(key, windows), accepted, "{key} {windows}");
        }
    }

    #[test]
    fn the_groups_of_the_tray_follow_the_mode() {
        assert!(group_decides_traffic("rule", "Proxy"));
        assert!(!group_decides_traffic("rule", "GLOBAL"));
        assert!(group_decides_traffic("global", "GLOBAL"));
        assert!(!group_decides_traffic("global", "Proxy"));
        assert!(!group_decides_traffic("direct", "Proxy"));
        assert!(!group_decides_traffic("direct", "GLOBAL"));
    }

    #[test]
    fn the_tray_agrees_with_the_system_only_when_every_element_does() {
        for (menu, icon, tooltip) in [(true, true, true), (false, false, false)] {
            let shown = ShownSystemProxy {
                menu: AtomicBool::new(menu),
                icon: AtomicBool::new(icon),
                tooltip: AtomicBool::new(tooltip),
            };
            assert!(shown.agree_with(menu));
            assert!(!shown.agree_with(!menu));
        }
        for (menu, icon, tooltip) in [(true, false, true), (false, true, true), (true, true, false)] {
            let shown = ShownSystemProxy {
                menu: AtomicBool::new(menu),
                icon: AtomicBool::new(icon),
                tooltip: AtomicBool::new(tooltip),
            };
            assert!(!shown.agree_with(true), "{menu} {icon} {tooltip}");
            assert!(!shown.agree_with(false), "{menu} {icon} {tooltip}");
        }
    }

    /// Системный прокси на старте и при смене порта пишется в фоне, после
    /// подтверждения порта: трей узнаёт об этом от самой записи, а каждый
    /// элемент трея сам помнит, что он показал.
    #[test]
    fn the_tray_learns_of_the_system_proxy_from_the_write_and_every_element_remembers_it() {
        use crate::utils::source_scan::{fn_body, without_test_modules};
        let tray = without_test_modules(include_str!("mod.rs"));
        for (signature, element) in [
            ("async fn update_menu_internal(", "menu"),
            ("pub async fn update_icon(", "icon"),
            ("async fn create_tray_from_handle(", "icon"),
            ("pub async fn update_tooltip(", "tooltip"),
        ] {
            let body = fn_body(tray, signature).unwrap_or_default();
            assert!(
                body.contains(&format!("shown_system_proxy.{element}.store(")),
                "{signature}"
            );
        }

        let config = without_test_modules(include_str!("../manager/config.rs"));
        let pointed = fn_body(config, "pub(super) async fn point_system_proxy_at_the_core()").unwrap_or_default();
        let written = pointed.find("update_sysproxy()");
        let redrawn = pointed.find("Tray::global().redraw()");
        assert!(
            written.zip(redrawn).is_some_and(|(written, redrawn)| written < redrawn),
            "{pointed}"
        );
    }

    #[test]
    fn a_node_item_gives_back_the_group_and_the_node_whatever_their_names_hold() {
        for (group, proxy) in [
            ("Auto_Select", "Netherlands"),
            ("Auto", "Select_Netherlands"),
            ("Группа_1", "Узел_2_example.com"),
            ("12_3", "4_5"),
            ("GLOBAL", ""),
        ] {
            assert_eq!(
                proxy_item_names(&proxy_item_id(group, proxy)),
                Some((group, proxy)),
                "{group} / {proxy}"
            );
        }
    }

    #[test]
    fn only_the_core_direct_gets_a_translated_title() {
        assert_ne!(node_title("DIRECT"), "DIRECT");
        for name in ["REJECT", "direct", "DIRECT-1", "Узел"] {
            assert_eq!(node_title(name), name);
        }
    }

    #[test]
    fn a_group_submenu_is_not_taken_for_a_node() {
        assert_eq!(proxy_item_names("proxy_group_Auto_Select"), None);
        assert_eq!(proxy_item_names("proxy_99_Auto"), None);
    }

    #[test]
    fn the_english_interface_and_help_spell_tun_one_way() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let mut files = vec![root.join("crates/clash-verge-i18n/locales/en.yml")];
        for dir in [
            "src/locales/en",
            "website/content/docs/en/user",
            "website/content/docs/en/provider",
        ] {
            files.extend(
                std::fs::read_dir(root.join(dir))
                    .into_iter()
                    .flatten()
                    .flatten()
                    .map(|entry| entry.path()),
            );
        }
        assert!(files.len() > 3, "{files:?}");
        for file in files {
            let text = std::fs::read_to_string(&file).unwrap_or_default();
            assert!(
                !text.split(|c: char| !c.is_alphanumeric()).any(|word| word == "Tun"),
                "{}: «Tun» вместо «TUN»",
                file.display()
            );
        }
    }

    /// Одна сущность — одно слово: пункт трея и окно на каждом языке.
    #[test]
    fn the_tray_names_things_with_the_words_of_the_window() {
        const PAIRS: [(&str, &str, &str); 15] = [
            ("tray.profiles", "profiles", "/page/title"),
            ("tray.rule", "home", "/components/clashMode/labels/rule"),
            ("tray.global", "home", "/components/clashMode/labels/global"),
            ("tray.direct", "home", "/components/clashMode/labels/direct"),
            ("tray.ruleMode", "settings", "/modals/hotkey/functions/rule"),
            ("tray.globalMode", "settings", "/modals/hotkey/functions/global"),
            ("tray.directMode", "settings", "/modals/hotkey/functions/direct"),
            ("tray.directNode", "shared", "/labels/direct"),
            ("tray.tunMode", "settings", "/sections/proxyControl/fields/tunMode"),
            (
                "notifications.tunModeToggled.title",
                "settings",
                "/sections/proxyControl/fields/tunMode",
            ),
            ("tray.tooltip.tun", "home", "/components/quickActions/tun"),
            (
                "tray.systemProxy",
                "settings",
                "/sections/proxyControl/fields/systemProxy",
            ),
            (
                "tray.tooltip.systemProxy",
                "settings",
                "/sections/proxyControl/fields/systemProxy",
            ),
            (
                "notifications.systemProxyToggled.title",
                "settings",
                "/sections/proxyControl/fields/systemProxy",
            ),
            ("tray.proxies", "layout", "/components/navigation/tabs/proxies"),
        ];
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let read = |path: std::path::PathBuf| std::fs::read_to_string(path).unwrap_or_default();
        // Пробелы — вёрстка, а не слово: «代 理» в боковой колонке.
        let word = |text: Option<&str>| text.map(|text| text.split_whitespace().collect::<String>());
        let mut locales: Vec<_> = std::fs::read_dir(root.join("src/locales"))
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        locales.sort();
        assert_eq!(locales.len(), 13, "{locales:?}");
        let tray_of = |locale: &str| -> serde_yaml_ng::Value {
            serde_yaml_ng::from_str(&read(
                root.join(format!("crates/clash-verge-i18n/locales/{locale}.yml")),
            ))
            .unwrap_or_default()
        };
        let lite_mode_in_the_window = |locale: &str| {
            serde_json::from_str::<serde_json::Value>(&read(root.join(format!("src/locales/{locale}/settings.json"))))
                .unwrap_or_default()
                .pointer("/components/verge/advanced/fields/liteModeSettings")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        };
        let tray_text = |tray: &serde_yaml_ng::Value, key: &str| {
            key.split('.')
                .try_fold(tray, |node, key| node.get(key))
                .and_then(serde_yaml_ng::Value::as_str)
                .map(str::to_owned)
        };
        let english_tray = tray_of("en");
        let english_window = lite_mode_in_the_window("en");
        assert!(english_window.is_some());
        for locale in &locales {
            let tray = tray_of(locale);
            // Окно называет облегчённый режим по-английски — трей и его
            // уведомление тоже: голого названия режима у окна нет.
            if lite_mode_in_the_window(locale) == english_window {
                for key in [
                    "tray.lightweightMode",
                    "notifications.lightweightModeEntered.title",
                    "notifications.lightweightModeEntered.body",
                ] {
                    assert_eq!(tray_text(&tray, key), tray_text(&english_tray, key), "{locale}: {key}");
                }
            }
            for (tray_key, namespace, window_key) in PAIRS {
                let window: serde_json::Value =
                    serde_json::from_str(&read(root.join(format!("src/locales/{locale}/{namespace}.json"))))
                        .unwrap_or_default();
                let tray_word = tray_key
                    .split('.')
                    .try_fold(&tray, |node, key| node.get(key))
                    .and_then(serde_yaml_ng::Value::as_str);
                let window_word = window.pointer(window_key).and_then(serde_json::Value::as_str);
                assert!(tray_word.is_some(), "{locale}: {tray_key}");
                assert_eq!(
                    word(tray_word),
                    word(window_word),
                    "{locale}: {tray_key} / {namespace}{window_key}"
                );
            }
        }
    }
}
