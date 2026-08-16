use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Result;

use crate::{
    config::Config,
    core::{
        CoreManager, Timer,
        handle::Handle,
        hotkey::Hotkey,
        logger::Logger,
        service::{SERVICE_MANAGER, ServiceManager, is_service_ipc_path_exists},
        sysopt,
        tray::Tray,
    },
    feat,
    module::{auto_backup::AutoBackupManager, lightweight::auto_lightweight_boot},
    process::AsyncHandler,
    utils::{init, server, window_manager::WindowManager},
};
use clash_verge_logging::{Type, logging, logging_error};
use clash_verge_signal;

pub mod dns;
pub mod scheme;
pub mod window;
pub mod window_script;

static RESOLVE_DONE: AtomicBool = AtomicBool::new(false);

pub fn init_work_dir_and_logger() -> anyhow::Result<()> {
    AsyncHandler::block_on(async {
        init_work_config().await;
        logging!(info, Type::Setup, "Initializing logger");
        Logger::global().init().await?;
        Ok(())
    })
}

pub fn resolve_setup_sync() {
    AsyncHandler::spawn(|| async {
        AsyncHandler::spawn_blocking(init_scheme);
        AsyncHandler::spawn_blocking(init_embed_server);
    });
}

pub fn resolve_setup_async() {
    AsyncHandler::spawn(|| async {
        logging!(info, Type::ClashVergeRev, "Version: {}", env!("CARGO_PKG_VERSION"));

        #[cfg(target_os = "macos")]
        {
            resolve_dock_show().await;
            dns::restore_public_dns_if_pending().await;
        }
        init_startup_script().await;
        let config_initialized = init_verge_config_before_window().await;
        init_window().await;
        init_resources().await;
        if let Err(e) = init::init_dns_config().await {
            logging!(warn, Type::Setup, "DNS config initialization failed: {}", e);
        }
        if config_initialized {
            init_verge_config().await;
        }
        Config::verify_config_initialization().await;

        let core_init = AsyncHandler::spawn(|| async {
            init_launch_connect_state().await;
            #[cfg(unix)]
            crate::core::orphan::sweep_orphan_cores().await;
            init_service_manager().await;
            init_core_manager().await;
            init_system_proxy().await;
            init_system_proxy_guard().await;
            init_tun_ready().await;
            crate::feat::environment::spawn_environment_watchdog();
        });

        let _ = futures::join!(
            core_init,
            init_tray(),
            init_timer(),
            init_hotkey(),
            init_auto_lightweight_boot(),
            init_auto_backup(),
            init_auto_launch_resync(),
            init_silent_updater(),
        );

        crate::core::traffic_estimate::init();

        crate::feat::release_stale_panel_locks().await;

        Handle::refresh_clash();
        refresh_tray_menu().await;
        resolve_done();
    });
}

pub async fn resolve_reset_async() -> Result<(), anyhow::Error> {
    sysopt::Sysopt::global().reset_sysproxy().await?;
    CoreManager::global().stop_core().await?;

    #[cfg(target_os = "macos")]
    {
        use dns::restore_public_dns;
        restore_public_dns().await;
    }

    Ok(())
}

pub(super) fn init_scheme() {
    logging_error!(Type::Setup, init::init_scheme());
}

pub async fn resolve_scheme(param: &str) -> Result<()> {
    logging_error!(Type::Setup, scheme::resolve_scheme(param).await);
    Ok(())
}

pub(super) fn init_embed_server() {
    server::embed_server();
}

pub(super) async fn init_resources() {
    logging_error!(Type::Setup, init::init_resources().await);
}

pub(super) async fn init_startup_script() {
    logging_error!(Type::Setup, init::startup_script().await);
}

pub(super) async fn init_timer() {
    logging_error!(Type::Setup, Timer::global().init().await);
}

pub(super) async fn init_hotkey() {
    let skip_register_hotkeys = !Config::verge().await.latest_arc().enable_global_hotkey.unwrap_or(true);
    logging_error!(Type::Setup, Hotkey::global().init(skip_register_hotkeys).await);
}

pub(super) async fn init_auto_lightweight_boot() {
    logging_error!(Type::Setup, auto_lightweight_boot().await);
}

pub(super) async fn init_auto_backup() {
    logging_error!(Type::Setup, AutoBackupManager::global().init().await);
}

pub(super) async fn init_auto_launch_resync() {
    let enabled = Config::verge().await.latest_arc().enable_auto_launch.unwrap_or(false);
    if enabled {
        logging_error!(Type::Setup, crate::core::autostart::update_launch().await);
    }
}

async fn init_silent_updater() {
    use crate::core::SilentUpdater;
    use crate::core::handle::Handle;

    logging!(info, Type::Setup, "Initializing silent updater...");

    let app_handle = Handle::app_handle();

    if SilentUpdater::global().try_install_on_startup(app_handle).await {
        logging!(info, Type::Setup, "Update installed at startup, restarting...");
        app_handle.restart();
    }

    let app_handle = app_handle.clone();
    tokio::spawn(async move {
        SilentUpdater::global().start_background_check(app_handle).await;
    });

    crate::core::core_updater::spawn_auto_check();

    crate::module::sub_watcher::spawn();

    logging!(info, Type::Setup, "Silent updater initialized");
}

pub fn init_signal() {
    logging!(info, Type::Setup, "Initializing signal handlers...");
    clash_verge_signal::register(feat::quit);
}

pub async fn init_work_config() {
    logging_error!(Type::Setup, init::init_config().await);
}

pub(super) async fn init_tray() {
    logging_error!(Type::Setup, Tray::global().init().await);
}

pub(super) async fn init_verge_config() {
    logging_error!(Type::Setup, Config::init_runtime_config().await);
}

pub(super) async fn init_verge_config_before_window() -> bool {
    let result = Config::init_config_before_window().await;
    let success = result.is_ok();
    logging_error!(Type::Setup, result);
    success
}

pub(super) async fn init_service_manager() {
    clash_verge_service_ipc::set_config(Some(ServiceManager::config())).await;
    if is_service_ipc_path_exists() && SERVICE_MANAGER.init().await.is_ok() {
        logging_error!(Type::Setup, SERVICE_MANAGER.refresh().await);
    }
}

pub(super) async fn init_core_manager() {
    logging_error!(Type::Setup, CoreManager::global().init().await);
    logging_error!(Type::Setup, crate::config::profiles::activate_selected_nodes());
}

pub(super) async fn init_launch_connect_state() {
    let (sys, tun) = crate::feat::launch_connect_state().await;

    let current = {
        let verge = Config::verge().await.latest_arc();
        (
            verge.enable_system_proxy.unwrap_or(false),
            verge.enable_tun_mode.unwrap_or(false),
        )
    };
    if current == (sys, tun) {
        return;
    }

    logging!(
        info,
        Type::Setup,
        "состояние подключения при запуске: системный прокси {sys}, TUN {tun}"
    );
    let patch = crate::config::IVerge {
        enable_system_proxy: Some(sys),
        enable_tun_mode: Some(tun),
        ..Default::default()
    };
    let verge = Config::verge().await;
    verge.edit_draft(|draft| draft.patch_config(&patch));
    verge.apply();
    logging_error!(Type::Setup, verge.data_arc().save_file().await);
}

pub(super) async fn init_system_proxy() {
    logging_error!(Type::Setup, sysopt::Sysopt::global().update_sysproxy().await);

    let verge = Config::verge().await.latest_arc();
    let active = verge.enable_system_proxy.unwrap_or(false) || verge.enable_tun_mode.unwrap_or(false);
    crate::feat::record_connect_targets(active);
}

pub(super) async fn init_tun_ready() {
    crate::feat::tun::init_startup_setup().await;
}

pub(super) async fn init_system_proxy_guard() {
    sysopt::Sysopt::global().refresh_guard().await;
}

pub(super) async fn refresh_tray_menu() {
    logging_error!(Type::Setup, Tray::global().update_part().await);
}

pub(super) async fn init_window() {
    let is_silent_start = Config::verge().await.data_arc().enable_silent_start.unwrap_or(false);
    WindowManager::create_window(!is_silent_start).await;
}

#[cfg(target_os = "macos")]
pub(super) async fn resolve_dock_show() {
    let is_silent_start = Config::verge().await.data_arc().enable_silent_start.unwrap_or(false);
    if is_silent_start {
        Handle::global().set_activation_policy_accessory();
    }
}

pub fn resolve_done() {
    RESOLVE_DONE.store(true, Ordering::Release);
}

pub fn is_resolve_done() -> bool {
    RESOLVE_DONE.load(Ordering::Acquire)
}
