use crate::{
    config::{Config, IVerge},
    core::handle,
};
use clash_verge_logging::{Type, logging};
use std::env;
use tauri_plugin_clipboard_manager::ClipboardExt as _;

pub async fn close_connections_via(previous_proxy: &str) -> usize {
    if previous_proxy.trim().is_empty() {
        return 0;
    }
    if !Config::verge().await.latest_arc().auto_close_connection() {
        return 0;
    }
    let listed = match handle::Handle::mihomo().await.get_connections().await {
        Ok(listed) => listed,
        Err(err) => {
            logging!(
                warn,
                Type::ProxyMode,
                "could not list connections after the node change: {err}"
            );
            return 0;
        }
    };
    let ids: Vec<String> = listed
        .connections
        .unwrap_or_default()
        .into_iter()
        .filter(|conn| conn.chains.iter().any(|hop| hop == previous_proxy))
        .map(|conn| conn.id)
        .collect();
    let mut closed = 0;
    for id in &ids {
        match handle::Handle::mihomo().await.close_connection(id).await {
            Ok(()) => closed += 1,
            Err(err) => logging!(debug, Type::ProxyMode, "connection {id} was not closed: {err}"),
        }
    }
    logging!(
        info,
        Type::ProxyMode,
        "node change: closed {closed} of {} connection(s) that went through {previous_proxy}",
        ids.len()
    );
    closed
}

pub async fn toggle_system_proxy() -> bool {
    let verge = Config::verge().await;
    let current = verge.latest_arc().enable_system_proxy.unwrap_or(false);
    let auto_close_connection = verge.latest_arc().auto_close_connection();
    let tun_carries_traffic = verge.latest_arc().enable_tun_mode.unwrap_or(false);

    if current
        && auto_close_connection
        && !tun_carries_traffic
        && let Err(err) = handle::Handle::mihomo().await.close_all_connections().await
    {
        logging!(error, Type::ProxyMode, "Failed to close all connections: {err}");
    }

    let requested = !current;
    let patch_result = super::patch_verge(
        &IVerge {
            enable_system_proxy: Some(requested),
            connect_system_proxy: Some(requested),
            ..IVerge::default()
        },
        false,
    )
    .await;

    match patch_result {
        Ok(_) => {
            handle::Handle::refresh_verge();
            Config::verge()
                .await
                .latest_arc()
                .enable_system_proxy
                .unwrap_or(requested)
        }
        Err(err) => {
            logging!(error, Type::ProxyMode, "{err}");
            if crate::core::sysopt::Sysopt::global().write_failed() {
                handle::Handle::notice_message("sysproxy::write_failed", err.to_string());
            }
            current
        }
    }
}

pub async fn toggle_tun_mode(not_save_file: Option<bool>) -> bool {
    let desired = Config::verge().await.latest_arc().enable_tun_mode.unwrap_or(false);
    let enable = !crate::feat::tun::is_active_with(desired);

    if enable {
        crate::feat::tun::ensure_ready(true).await;
    }

    match super::patch_verge(
        &IVerge {
            enable_tun_mode: Some(enable),
            connect_tun_mode: Some(enable),
            ..IVerge::default()
        },
        not_save_file.unwrap_or(false),
    )
    .await
    {
        Ok(_) => {
            handle::Handle::refresh_verge();
            enable
        }
        Err(err) => {
            logging!(error, Type::ProxyMode, "{err}");
            // clod:e3-04 — отказ шага уже не значит, что настройка откатилась:
            // после удавшегося перезапуска ядра `patch_verge` сохраняет её и
            // всё равно возвращает ошибку. Отвечаем тем, что реально записано,
            // иначе уведомление скажет «TUN выключен» при работающем TUN.
            Config::verge().await.data_arc().enable_tun_mode.unwrap_or(desired)
        }
    }
}

pub async fn copy_clash_env() {
    let env_ip = env::var("CLASH_VERGE_REV_IP").ok();
    let verge_cfg = Config::verge().await.latest_arc();
    let ip = env_ip
        .as_deref()
        .unwrap_or_else(|| verge_cfg.proxy_host.as_deref().unwrap_or("127.0.0.1"));

    let app_handle = handle::Handle::app_handle();
    let port = verge_cfg.verge_mixed_port.unwrap_or(7897);
    let http_proxy = format!("http://{ip}:{port}");
    let socks5_proxy = format!("socks5://{ip}:{port}");

    let clipboard = app_handle.clipboard();

    let default_env = {
        #[cfg(not(target_os = "windows"))]
        {
            "bash"
        }
        #[cfg(target_os = "windows")]
        {
            "powershell"
        }
    };
    let env_type = verge_cfg.env_type.as_deref().unwrap_or(default_env);

    let export_text = match env_type {
        "bash" => format!("export https_proxy={http_proxy} http_proxy={http_proxy} all_proxy={socks5_proxy}"),
        "cmd" => format!("set http_proxy={http_proxy}\r\nset https_proxy={http_proxy}"),
        "powershell" => {
            format!("$env:HTTP_PROXY=\"{http_proxy}\"; $env:HTTPS_PROXY=\"{http_proxy}\"")
        }
        "nushell" => {
            format!("load-env {{ http_proxy: \"{http_proxy}\", https_proxy: \"{http_proxy}\" }}")
        }
        "fish" => format!("set -x http_proxy {http_proxy}; set -x https_proxy {http_proxy}"),
        _ => {
            logging!(error, Type::ProxyMode, "copy_clash_env: Invalid env type! {env_type}");
            return;
        }
    };

    if clipboard.write_text(&export_text).is_err() {
        logging!(error, Type::ProxyMode, "Failed to write to clipboard");
    }
}
