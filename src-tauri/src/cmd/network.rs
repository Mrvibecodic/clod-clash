use super::CmdResult;
use crate::cmd::StringifyErr as _;
use crate::core::sysopt::Sysopt;
use clash_verge_logging::{Type, logging};
use gethostname::gethostname;
use network_interface::NetworkInterface;
use serde_yaml_ng::Mapping;
use std::net::TcpListener;
use sysproxy::{Autoproxy, Sysproxy};
use tauri_plugin_clash_verge_sysinfo;

/// get the system proxy
#[tauri::command]
pub async fn get_sys_proxy() -> CmdResult<Mapping> {
    logging!(debug, Type::Network, "асинхронное получение конфига системного прокси");

    Sysopt::global().wait_idle().await;
    let sys_proxy = Sysproxy::get_system_proxy().stringify_err()?;
    let Sysproxy {
        ref host,
        ref bypass,
        ref port,
        ref enable,
    } = sys_proxy;

    let mut map = Mapping::new();
    map.insert("enable".into(), (*enable).into());
    map.insert("server".into(), format!("{}:{}", host, port).into());
    map.insert("bypass".into(), bypass.as_str().into());

    logging!(
        debug,
        Type::Network,
        "возврат конфига системного прокси: enable={}, {}:{}",
        sys_proxy.enable,
        sys_proxy.host,
        sys_proxy.port
    );
    Ok(map)
}

/// Получить конфиг авто-прокси
#[tauri::command]
pub async fn get_auto_proxy() -> CmdResult<Mapping> {
    Sysopt::global().wait_idle().await;
    let auto_proxy = Autoproxy::get_auto_proxy().stringify_err()?;
    let Autoproxy { ref enable, ref url } = auto_proxy;

    let mut map = Mapping::new();
    map.insert("enable".into(), (*enable).into());
    map.insert("url".into(), url.as_str().into());

    logging!(
        debug,
        Type::Network,
        "возврат конфига авто-прокси (кэш): enable={}, url={}",
        auto_proxy.enable,
        auto_proxy.url
    );
    Ok(map)
}

/// Получить имя хоста системы
#[tauri::command]
pub fn get_system_hostname() -> String {
    // Получаем имя хоста системы, обрабатываем возможные не-UTF-8 символы
    match gethostname().into_string() {
        Ok(name) => name,
        Err(os_string) => {
            // Для имени хоста с не-UTF-8 символами используем debug-форматирование
            let fallback = format!("{os_string:?}");
            // Убираем возможные кавычки
            fallback.trim_matches('"').to_string()
        }
    }
}

/// Получить список сетевых интерфейсов
#[tauri::command]
pub fn get_network_interfaces() -> Vec<String> {
    tauri_plugin_clash_verge_sysinfo::list_network_interfaces()
}

/// Получить подробную информацию о сетевых интерфейсах
#[tauri::command]
pub fn get_network_interfaces_info() -> CmdResult<Vec<NetworkInterface>> {
    use network_interface::{NetworkInterface, NetworkInterfaceConfig as _};

    let names = get_network_interfaces();
    let interfaces = NetworkInterface::show().stringify_err()?;

    let mut result = Vec::new();

    for interface in interfaces {
        if names.contains(&interface.name) {
            result.push(interface);
        }
    }

    Ok(result)
}

#[tauri::command]
pub fn is_port_in_use(port: u16) -> bool {
    TcpListener::bind(("127.0.0.1", port)).is_err()
}
