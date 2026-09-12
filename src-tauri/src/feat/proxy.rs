use crate::{
    config::{Config, IVerge},
    core::handle,
};
use clash_verge_logging::{Type, logging};
use futures::StreamExt as _;
use std::env;
use tauri_plugin_clipboard_manager::ClipboardExt as _;

/// Сколько закрытий держим в полёте одновременно.
const CLOSE_AT_ONCE: usize = 16;

fn goes_through(chains: &[std::string::String], previous_proxy: &str) -> bool {
    chains.iter().any(|hop| hop == previous_proxy)
}

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
        .filter(|conn| goes_through(&conn.chains, previous_proxy))
        .map(|conn| conn.id)
        .collect();
    // clod:node-switch — соединения закрываются пачкой: на сотне-другой
    // последовательные запросы к ядру растягивают разрыв на секунды, и всё это
    // время трафик продолжает идти через прежний узел.
    let total = ids.len();
    let closed = futures::stream::iter(ids)
        .map(|id| async move {
            match handle::Handle::mihomo().await.close_connection(&id).await {
                Ok(()) => 1_usize,
                Err(err) => {
                    logging!(debug, Type::ProxyMode, "connection {id} was not closed: {err}");
                    0
                }
            }
        })
        .buffer_unordered(CLOSE_AT_ONCE)
        .fold(0_usize, |sum, one| async move { sum + one })
        .await;
    logging!(
        info,
        Type::ProxyMode,
        "node change: closed {closed} of {total} connection(s) that went through {previous_proxy}"
    );
    closed
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SystemProxyStep {
    LeaveEverythingAlone,
    DropConnectionsThenSwitch,
    Switch,
}

const fn system_proxy_step(
    exiting: bool,
    current: bool,
    auto_close_connection: bool,
    tun_carries_traffic: bool,
) -> SystemProxyStep {
    if exiting {
        return SystemProxyStep::LeaveEverythingAlone;
    }
    if current && auto_close_connection && !tun_carries_traffic {
        SystemProxyStep::DropConnectionsThenSwitch
    } else {
        SystemProxyStep::Switch
    }
}

fn exit_is_under_way() -> bool {
    match super::refuse_while_exiting() {
        Ok(()) => false,
        Err(err) => {
            logging!(info, Type::ProxyMode, "{err}");
            true
        }
    }
}

pub async fn toggle_system_proxy() -> bool {
    let (current, auto_close_connection, tun_carries_traffic) = {
        let snapshot = Config::verge().await.latest_arc();
        (
            snapshot.enable_system_proxy.unwrap_or(false),
            snapshot.auto_close_connection(),
            snapshot.enable_tun_mode.unwrap_or(false),
        )
    };

    match system_proxy_step(exit_is_under_way(), current, auto_close_connection, tun_carries_traffic) {
        SystemProxyStep::LeaveEverythingAlone => return current,
        SystemProxyStep::DropConnectionsThenSwitch => {
            if let Err(err) = handle::Handle::mihomo().await.close_all_connections().await {
                logging!(error, Type::ProxyMode, "Failed to close all connections: {err}");
            }
        }
        SystemProxyStep::Switch => {}
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TunStep {
    LeaveEverythingAlone,
    PrepareServiceThenSwitchOn,
    SwitchOff,
}

const fn tun_step(exiting: bool, active: bool) -> TunStep {
    if exiting {
        return TunStep::LeaveEverythingAlone;
    }
    if active {
        TunStep::SwitchOff
    } else {
        TunStep::PrepareServiceThenSwitchOn
    }
}

pub async fn toggle_tun_mode(not_save_file: Option<bool>) -> bool {
    let desired = Config::verge().await.latest_arc().enable_tun_mode.unwrap_or(false);

    let enable = match tun_step(exit_is_under_way(), crate::feat::tun::is_active_with(desired)) {
        TunStep::LeaveEverythingAlone => return desired,
        TunStep::PrepareServiceThenSwitchOn => {
            crate::feat::tun::ensure_ready(true).await;
            true
        }
        TunStep::SwitchOff => false,
    };

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
    let port = Config::effective_mixed_port().await;
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

#[cfg(test)]
mod tests {
    use super::{SystemProxyStep, TunStep, goes_through, system_proxy_step, tun_step};

    fn chain(hops: &[&str]) -> Vec<std::string::String> {
        hops.iter().map(|hop| (*hop).to_owned()).collect()
    }

    #[test]
    fn while_the_app_is_leaving_the_tun_switch_never_reaches_the_rights_prompt() {
        for active in [false, true] {
            assert_eq!(
                tun_step(true, active),
                TunStep::LeaveEverythingAlone,
                "active={active}: во время выхода подготовка службы не запускается"
            );
        }
    }

    #[test]
    fn with_no_exit_under_way_the_tun_switch_flips_and_prepares_only_when_turning_on() {
        assert_eq!(tun_step(false, false), TunStep::PrepareServiceThenSwitchOn);
        assert_eq!(tun_step(false, true), TunStep::SwitchOff);
    }

    #[test]
    fn while_the_app_is_leaving_the_system_proxy_switch_never_drops_connections() {
        for current in [false, true] {
            for auto_close_connection in [false, true] {
                for tun_carries_traffic in [false, true] {
                    assert_eq!(
                        system_proxy_step(true, current, auto_close_connection, tun_carries_traffic),
                        SystemProxyStep::LeaveEverythingAlone,
                        "{current}/{auto_close_connection}/{tun_carries_traffic}: \
                         соединения не рвутся ради настройки, которая не применится"
                    );
                }
            }
        }
    }

    #[test]
    fn connections_are_dropped_only_when_the_proxy_that_carried_them_is_switched_off() {
        assert_eq!(
            system_proxy_step(false, true, true, false),
            SystemProxyStep::DropConnectionsThenSwitch
        );
        for (current, auto_close_connection, tun_carries_traffic) in [
            (false, true, false),
            (true, false, false),
            (true, true, true),
            (false, false, false),
        ] {
            assert_eq!(
                system_proxy_step(false, current, auto_close_connection, tun_carries_traffic),
                SystemProxyStep::Switch,
                "{current}/{auto_close_connection}/{tun_carries_traffic}"
            );
        }
    }

    #[test]
    fn a_connection_through_the_previous_node_is_recognised() {
        assert!(goes_through(&chain(&["Germany 01"]), "Germany 01"));
    }

    #[test]
    fn a_relay_counts_wherever_the_node_sits_in_the_chain() {
        assert!(goes_through(&chain(&["Germany 01", "Relay", "GLOBAL"]), "GLOBAL"));
        assert!(goes_through(&chain(&["Germany 01", "Relay", "GLOBAL"]), "Relay"));
    }

    #[test]
    fn other_connections_are_left_alone() {
        assert!(!goes_through(&chain(&["Germany 02"]), "Germany 01"));
        assert!(!goes_through(&chain(&[]), "Germany 01"));
        assert!(!goes_through(&chain(&["DIRECT"]), "Germany 01"));
    }

    #[test]
    fn the_node_name_is_matched_whole_not_by_prefix() {
        assert!(!goes_through(&chain(&["Germany 011"]), "Germany 01"));
        assert!(!goes_through(&chain(&["Germany 0"]), "Germany 01"));
        assert!(!goes_through(&chain(&["germany 01"]), "Germany 01"));
    }
}
