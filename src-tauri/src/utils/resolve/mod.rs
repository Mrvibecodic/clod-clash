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
        // #[cfg(not(feature = "tokio-trace"))]
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
            // clod: до всего прочего — если прошлый запуск не успел вернуть
            // системный DNS (упали, убили, выключили питание), возвращаем его
            // сейчас. Иначе весь резолв так и идёт на подменённый сервер.
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
            // clod: первым делом — что вообще должно быть поднято этим
            // запуском. Ядро генерирует свой конфиг из `enable_tun_mode`,
            // поэтому решение принимается до его старта.
            init_launch_connect_state().await;
            init_service_manager().await;
            init_core_manager().await;
            init_system_proxy().await;
            init_system_proxy_guard().await;
            // clod:tun-ready — последним шагом: окно уже показано, ядро и
            // прокси подняты, поэтому запрос прав не блокирует старт и
            // пользователь видит, кто именно его просит.
            init_tun_ready().await;
            // clod:wake-net — сторож ставится ПОСЛЕ подъёма прокси и ядра:
            // первый отпечаток сети снимается с уже настроенной машины, иначе
            // первый же круг счёл бы наш собственный старт сменой сети.
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

        // clod:traffic-estimate — счётчик поднимаем после ядра: до него
        // опрашивать нечего.
        crate::core::traffic_estimate::init();

        // clod:lock-expiry — проверка на старте нужна отдельно от обновлений:
        // подписку с выключенным автообновлением клиент панели не показывает
        // вовсе, и без этого круга её замок не протухал бы никогда.
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
    // if hotkey is not use by global, skip init it
    let skip_register_hotkeys = !Config::verge().await.latest_arc().enable_global_hotkey.unwrap_or(true);
    logging_error!(Type::Setup, Hotkey::global().init(skip_register_hotkeys).await);
}

pub(super) async fn init_auto_lightweight_boot() {
    logging_error!(Type::Setup, auto_lightweight_boot().await);
}

pub(super) async fn init_auto_backup() {
    logging_error!(Type::Setup, AutoBackupManager::global().init().await);
}

// clod:branding — re-register auto-launch with the current executable path.
// An app update can move or rename the binary (clash-verge → clod-clash),
// and the scheduled task / autostart entry would silently keep pointing at
// the dead path otherwise.
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

    // Check for cached update and attempt install before main app initialization.
    // If install succeeds:
    //   - Windows: NSIS takes over and the process exits automatically
    //   - macOS/Linux: binary is replaced, we restart the app
    if SilentUpdater::global().try_install_on_startup(app_handle).await {
        logging!(info, Type::Setup, "Update installed at startup, restarting...");
        app_handle.restart();
    }

    // No pending install — start background check/download loop
    let app_handle = app_handle.clone();
    tokio::spawn(async move {
        SilentUpdater::global().start_background_check(app_handle).await;
    });

    // clod:F5 — daily managed-core check (notification only, opt-in)
    crate::core::core_updater::spawn_auto_check();

    // clod:F7 — subscription expiry/traffic watcher (startup catch-up +30 s,
    // then hourly; deliberately independent of profile updates)
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
    // clod: with the core up, replay the saved node selection. The core's own
    // store-selected cache covers plain restarts; this also applies the
    // starred-server defaults and repairs choices that no longer exist.
    logging_error!(Type::Setup, crate::config::profiles::activate_selected_nodes());
}

/// clod: состояние подключения при старте задаётся ЗАНОВО, а не достаётся из
/// переживших перезапуск флагов «поднято сейчас». Иначе клиент прописывал
/// системный прокси в настройки ОС сам, ещё до того как показалось окно, —
/// пользователь видел настройку, которой не включал.
///
/// Считается ДО подъёма ядра: конфиг ядра генерируется из `enable_tun_mode`,
/// и решать после запуска означало бы поднять туннель, чтобы тут же его снять.
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

    // clod:simple-mode — when the app boots into an already-active proxy/TUN
    // state, the Connect session starts now, not at some later toggle.
    let verge = Config::verge().await.latest_arc();
    let active = verge.enable_system_proxy.unwrap_or(false) || verge.enable_tun_mode.unwrap_or(false);
    crate::feat::record_connect_targets(active);
}

// clod:tun-ready — один раз на установку доводим TUN до рабочего состояния:
// служба ставится сама (один запрос прав), дальше остаются только проверки.
pub(super) async fn init_tun_ready() {
    crate::feat::tun::init_startup_setup().await;
}

pub(super) async fn init_system_proxy_guard() {
    // Сторож смотрит на тот же `enable_system_proxy`: при выключенном он сам
    // останавливается, поэтому отдельной проверки здесь не нужно.
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
