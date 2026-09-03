use crate::{
    config::{Config, IVerge},
    core::{CoreManager, autostart, handle, hotkey, logger::Logger, sysopt, tray},
    module::{auto_backup::AutoBackupManager, lightweight},
};
use anyhow::Result;
use bitflags::bitflags;
use clash_verge_draft::SharedDraft;
use clash_verge_logging::{Type, logging, logging_error};
use serde_yaml_ng::Mapping;

pub async fn patch_clash(patch: &Mapping) -> Result<()> {
    Config::clash().await.edit_draft(|d| d.patch_config(patch));

    let res = {
        if patch.get("secret").is_some() || patch.get("external-controller").is_some() {
            Config::generate().await?;
            CoreManager::global().restart_core().await?;
        } else if patch.get("allow-lan").is_some() {
            CoreManager::global().update_config_checked().await?;
        } else {
            if patch.get("mode").is_some() {
                tray::Tray::global().update_menu_and_icon().await;
            }
            Config::runtime().await.edit_draft(|d| d.patch_config(patch));
            CoreManager::global().update_config_checked().await?;
        }
        handle::Handle::refresh_clash();
        <Result<()>>::Ok(())
    };
    match res {
        Ok(()) => {
            Config::clash().await.apply();
            let clash_data = Config::clash().await.data_arc();
            clash_data.save_config().await?;
            Ok(())
        }
        Err(err) => {
            Config::clash().await.discard();
            Err(err)
        }
    }
}

bitflags! {
     #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
     struct UpdateFlags: u16 {
        const RESTART_CORE = 1 << 0;
        const CLASH_CONFIG = 1 << 1;
        const VERGE_CONFIG = 1 << 2;
        const LAUNCH = 1 << 3;
        const SYS_PROXY = 1 << 4;
        const SYSTRAY_ICON = 1 << 5;
        const HOTKEY = 1 << 6;
        const SYSTRAY_MENU = 1 << 7;
        const SYSTRAY_TOOLTIP = 1 << 8;
        const SYSTRAY_CLICK_BEHAVIOR = 1 << 9;
        const LIGHT_WEIGHT = 1 << 10;
        const LANGUAGE = 1 << 11;
        const LOG_LEVEL = 1 << 12;
        const LOG_FILE = 1 << 13;

        const GROUP_SYS_TRAY = Self::SYSTRAY_MENU.bits()
                             | Self::SYSTRAY_TOOLTIP.bits()
                             | Self::SYSTRAY_ICON.bits();

        /// clod:e3-04 — шаги, отказ которых не оставляет систему в опасном
        /// состоянии: они либо ничего не меняют за пределами приложения, либо
        /// чинятся сами при следующем обновлении. Только для такого набора
        /// настройки разрешено сохранять после уже случившегося перезапуска
        /// ядра. Системный прокси и автозапуск сюда не входят: сохранённое
        /// «включено» при неприменённом шаге врало бы о трафике.
        const SALVAGEABLE_AFTER_RESTART = Self::RESTART_CORE.bits()
                                        | Self::CLASH_CONFIG.bits()
                                        | Self::VERGE_CONFIG.bits()
                                        | Self::GROUP_SYS_TRAY.bits()
                                        | Self::SYSTRAY_CLICK_BEHAVIOR.bits()
                                        | Self::LANGUAGE.bits()
                                        | Self::LOG_LEVEL.bits()
                                        | Self::LOG_FILE.bits();
     }
}

fn determine_update_flags(patch: &IVerge) -> UpdateFlags {
    let tun_mode = patch.enable_tun_mode;
    let auto_launch = patch.enable_auto_launch;
    let system_proxy = patch.enable_system_proxy;
    let pac = patch.proxy_auto_config;
    let pac_content = &patch.pac_file_content;
    let proxy_bypass = &patch.system_proxy_bypass;
    let language = &patch.language;
    let mixed_port = patch.verge_mixed_port;
    #[cfg(target_os = "macos")]
    let tray_icon = &patch.tray_icon;
    #[cfg(not(target_os = "macos"))]
    let tray_icon: Option<String> = None;
    let common_tray_icon = patch.common_tray_icon;
    let sysproxy_tray_icon = patch.sysproxy_tray_icon;
    let tun_tray_icon = patch.tun_tray_icon;
    #[cfg(not(target_os = "windows"))]
    let redir_enabled = patch.verge_redir_enabled;
    #[cfg(not(target_os = "windows"))]
    let redir_port = patch.verge_redir_port;
    #[cfg(target_os = "linux")]
    let tproxy_enabled = patch.verge_tproxy_enabled;
    #[cfg(target_os = "linux")]
    let tproxy_port = patch.verge_tproxy_port;
    let socks_enabled = patch.verge_socks_enabled;
    let socks_port = patch.verge_socks_port;
    let http_enabled = patch.verge_http_enabled;
    let http_port = patch.verge_port;
    #[cfg(target_os = "macos")]
    let enable_tray_speed = patch.enable_tray_speed;
    #[cfg(not(target_os = "macos"))]
    let enable_tray_speed: Option<bool> = None;
    let enable_global_hotkey = patch.enable_global_hotkey;
    let tray_event = &patch.tray_event;
    let enable_auto_light_weight = patch.enable_auto_light_weight_mode;
    let enable_external_controller = patch.enable_external_controller;
    let tray_proxy_groups_display_mode = &patch.tray_proxy_groups_display_mode;
    let tray_inline_outbound_modes = patch.tray_inline_outbound_modes;
    let enable_proxy_guard = patch.enable_proxy_guard;
    let proxy_guard_duration = patch.proxy_guard_duration;
    let log_level = &patch.app_log_level;
    let log_max_size = patch.app_log_max_size;
    let log_max_count = patch.app_log_max_count;

    #[cfg(target_os = "windows")]
    let restart_core_needed = socks_enabled.is_some()
        || http_enabled.is_some()
        || socks_port.is_some()
        || http_port.is_some()
        || mixed_port.is_some()
        || patch.use_managed_core.is_some()
        || enable_external_controller.is_some();
    #[cfg(not(target_os = "windows"))]
    let mut restart_core_needed = socks_enabled.is_some()
        || http_enabled.is_some()
        || socks_port.is_some()
        || http_port.is_some()
        || mixed_port.is_some()
        || patch.use_managed_core.is_some()
        || enable_external_controller.is_some();
    #[cfg(not(target_os = "windows"))]
    {
        restart_core_needed |= redir_enabled.is_some() || redir_port.is_some();
    }
    #[cfg(target_os = "linux")]
    {
        restart_core_needed |= tproxy_enabled.is_some() || tproxy_port.is_some();
        restart_core_needed |= tun_mode == Some(true);
    }

    let mut update_flags = UpdateFlags::empty();
    if restart_core_needed {
        update_flags.insert(UpdateFlags::RESTART_CORE);
    }
    if tun_mode.is_some() {
        update_flags.insert(UpdateFlags::CLASH_CONFIG | UpdateFlags::GROUP_SYS_TRAY);
    }
    if enable_global_hotkey.is_some() {
        update_flags.insert(UpdateFlags::VERGE_CONFIG);
    }
    if auto_launch.is_some() {
        update_flags.insert(UpdateFlags::LAUNCH);
    }
    if system_proxy.is_some() {
        update_flags.insert(UpdateFlags::SYS_PROXY | UpdateFlags::GROUP_SYS_TRAY);
    }
    if proxy_bypass.is_some()
        || pac_content.is_some()
        || pac.is_some()
        || enable_proxy_guard.is_some()
        || proxy_guard_duration.is_some()
        || patch.proxy_host.is_some()
        || patch.use_default_bypass.is_some()
    {
        update_flags.insert(UpdateFlags::SYS_PROXY);
    }
    if language.is_some() {
        update_flags.insert(UpdateFlags::LANGUAGE | UpdateFlags::SYSTRAY_MENU | UpdateFlags::SYSTRAY_TOOLTIP);
    }
    if common_tray_icon.is_some()
        || sysproxy_tray_icon.is_some()
        || tun_tray_icon.is_some()
        || tray_icon.is_some()
        || enable_tray_speed.is_some()
    {
        update_flags.insert(UpdateFlags::SYSTRAY_ICON);
    }
    if patch.hotkeys.is_some() {
        update_flags.insert(UpdateFlags::HOTKEY | UpdateFlags::SYSTRAY_MENU);
    }
    if tray_event.is_some() {
        update_flags.insert(UpdateFlags::SYSTRAY_CLICK_BEHAVIOR);
    }
    if enable_auto_light_weight.is_some() {
        update_flags.insert(UpdateFlags::LIGHT_WEIGHT);
    }
    if tray_proxy_groups_display_mode.is_some() {
        update_flags.insert(UpdateFlags::SYSTRAY_MENU);
    }
    if log_level.is_some() {
        update_flags.insert(UpdateFlags::LOG_LEVEL);
    }
    if log_max_size.is_some() || log_max_count.is_some() {
        update_flags.insert(UpdateFlags::LOG_FILE);
    }
    if patch.enable_verbose_diagnostics.is_some() {
        sysopt::spawn_proxy_observer();
    }
    if tray_inline_outbound_modes.is_some() {
        update_flags.insert(UpdateFlags::SYSTRAY_MENU);
    }

    update_flags
}

/// Перезапуск ядра под новый черновик настроек.
///
/// clod:e3-04 — вынесен из `process_terminated_flags` отдельным шагом: после
/// удавшегося перезапуска ядро уже обслуживает трафик по новым настройкам, и
/// откатывать черновик из-за отказа любого следующего шага нельзя.
async fn restart_core_for_patch() -> Result<()> {
    Config::generate().await?;
    CoreManager::global().restart_core().await
}

#[allow(clippy::cognitive_complexity)]
async fn process_terminated_flags(update_flags: UpdateFlags, patch: &IVerge) -> Result<()> {
    if update_flags.contains(UpdateFlags::CLASH_CONFIG) {
        CoreManager::global().update_config_checked().await?;
        handle::Handle::refresh_clash();
    }
    if update_flags.contains(UpdateFlags::VERGE_CONFIG) {
        handle::Handle::refresh_verge();
    }
    if update_flags.contains(UpdateFlags::LAUNCH) {
        autostart::update_launch().await?;
    }
    if update_flags.contains(UpdateFlags::LANGUAGE)
        && let Some(language) = &patch.language
    {
        clash_verge_i18n::set_locale(language.as_str());
    }
    if update_flags.contains(UpdateFlags::SYS_PROXY) {
        if patch.enable_system_proxy == Some(true)
            && matches!(
                *CoreManager::global().get_running_mode(),
                crate::core::manager::RunningMode::NotRunning
            )
        {
            logging!(
                error,
                Type::Setup,
                "ядро не запущено — системный прокси в систему не пишем"
            );
            Config::verge().await.edit_draft(|draft| {
                draft.patch_config(&IVerge {
                    enable_system_proxy: Some(false),
                    ..IVerge::default()
                });
            });
            handle::Handle::notice_message("sysproxy::core_not_running", "");
        } else {
            sysopt::Sysopt::global().update_sysproxy().await?;
            sysopt::Sysopt::global().refresh_guard().await;
        }
    }
    if update_flags.contains(UpdateFlags::HOTKEY)
        && let Some(hotkeys) = &patch.hotkeys
    {
        hotkey::Hotkey::global().update(hotkeys.to_owned()).await?;
    }
    if update_flags.contains(UpdateFlags::SYSTRAY_MENU) {
        tray::Tray::global().update_menu().await?;
    }
    if update_flags.contains(UpdateFlags::SYSTRAY_ICON) {
        tray::Tray::global()
            .update_icon(&Config::verge().await.latest_arc())
            .await?;
        #[cfg(target_os = "macos")]
        if patch.enable_tray_speed.is_some() {
            tray::Tray::global().update_speed_task(patch.enable_tray_speed.unwrap_or(false));
        }
    }
    if update_flags.contains(UpdateFlags::SYSTRAY_TOOLTIP) {
        tray::Tray::global().update_tooltip().await?;
    }
    if update_flags.contains(UpdateFlags::SYSTRAY_CLICK_BEHAVIOR) {
        tray::Tray::global().update_click_behavior().await?;
    }
    if update_flags.contains(UpdateFlags::LIGHT_WEIGHT) {
        if patch.enable_auto_light_weight_mode.unwrap_or(false) {
            lightweight::enable_auto_light_weight_mode().await;
        } else {
            lightweight::disable_auto_light_weight_mode();
        }
    }
    if update_flags.contains(UpdateFlags::LOG_LEVEL) {
        Logger::global().update_log_level(patch.get_log_level())?;
    }
    if update_flags.contains(UpdateFlags::LOG_FILE) {
        let log_max_size = patch.app_log_max_size.unwrap_or(128);
        let log_max_count = patch.app_log_max_count.unwrap_or(8);
        Logger::global().update_log_config(log_max_size, log_max_count).await?;
    }
    Ok(())
}

static PATCH_VERGE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub(crate) const fn patch_verge_lock() -> &'static tokio::sync::Mutex<()> {
    &PATCH_VERGE_LOCK
}

pub async fn patch_verge(patch: &IVerge, not_save_file: bool) -> Result<()> {
    let _serialized = PATCH_VERGE_LOCK.lock().await;

    Config::verge().await.edit_draft(|d| d.patch_config(patch));

    let tun_log_anchor = if patch.enable_tun_mode == Some(true) {
        crate::feat::tun::clear_suppression();
        crate::core::CoreManager::global().handoff_to_service_if_needed().await;
        crate::feat::tun::log_anchor().await
    } else {
        None
    };

    let update_flags = determine_update_flags(patch);
    logging!(debug, Type::Setup, "Determined update flags: {:?}", update_flags);

    // clod:e3-04 — перезапуск ядра отделён от остальных шагов: после него ядро
    // уже обслуживает трафик по новым настройкам, и откат черновика развёл бы
    // сохранённое с работающим (в файле старый порт или старое ядро — в памяти
    // новое). Поэтому дальше черновик не откатывается, но только если отказать
    // могли лишь безопасные шаги (см. SALVAGEABLE_AFTER_RESTART).
    let core_restarted = if update_flags.contains(UpdateFlags::RESTART_CORE) {
        if let Err(err) = restart_core_for_patch().await {
            Config::verge().await.discard();
            return Err(err);
        }
        if Config::verge().await.latest_arc().enable_system_proxy.unwrap_or(false) {
            match sysopt::Sysopt::global().update_sysproxy().await {
                Ok(()) => sysopt::Sysopt::global().refresh_guard().await,
                Err(err) => logging!(error, Type::Setup, "{err}"),
            }
        }
        true
    } else {
        false
    };

    let flags_result = process_terminated_flags(update_flags, patch).await;
    if let Err(err) = flags_result {
        let keep_settings = core_restarted && UpdateFlags::SALVAGEABLE_AFTER_RESTART.contains(update_flags);
        if !keep_settings {
            Config::verge().await.discard();
            return Err(err);
        }
        logging!(
            warn,
            Type::Setup,
            "шаг после перезапуска ядра не прошёл, настройки всё равно сохраняем: {err:#}"
        );
        Config::verge().await.apply();
        // Хвост общий: настройки оставлены жить, значит и обвязка вокруг них
        // (цели кнопки Connect, проверка TUN, автобэкап, запись на диск)
        // должна отработать — иначе сохранённое разошлось бы с приложением.
        let finished = finish_patch_verge(patch, tun_log_anchor, not_save_file).await;
        // Фронтенд обязан увидеть то, что реально сохранено и работает:
        // вызывающий получит ошибку и сам по себе тумблер не обновит.
        handle::Handle::refresh_verge();
        return match finished {
            Ok(()) => Err(err),
            // Обещание «настройки всё равно сохраняем» не выполнено — это
            // должно быть видно вызывающему, а не только в логе.
            Err(save_error) => Err(err.context(format!("и настройки не сохранились: {save_error:#}"))),
        };
    }
    Config::verge().await.apply();

    finish_patch_verge(patch, tun_log_anchor, not_save_file).await
}

/// Обвязка вокруг уже зафиксированных настроек и запись их на диск.
async fn finish_patch_verge(patch: &IVerge, tun_log_anchor: Option<String>, not_save_file: bool) -> Result<()> {
    if patch.enable_system_proxy.is_some() || patch.enable_tun_mode.is_some() {
        let latest = Config::verge().await.latest_arc();
        let active = latest.enable_system_proxy.unwrap_or(false) || latest.enable_tun_mode.unwrap_or(false);
        crate::feat::record_connect_targets(active);
    }

    if patch.enable_tun_mode == Some(true) {
        crate::feat::tun::spawn_start_verification(tun_log_anchor);
    }

    logging_error!(Type::Backup, AutoBackupManager::global().refresh_settings().await);

    if not_save_file {
        return Ok(());
    }
    let verge_data = Config::verge().await.data_arc();
    logging!(debug, Type::Setup, "Saving Verge configuration to file...");
    verge_data.save_file().await
}

pub async fn fetch_verge_config() -> Result<SharedDraft<IVerge>> {
    let draft = Config::verge().await;
    let data = draft.data_arc();
    Ok(data)
}
