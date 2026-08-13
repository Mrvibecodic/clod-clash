use super::CmdResult;
use crate::cmd::StringifyErr as _;
use crate::core::sysopt::Sysopt;
use clash_verge_logging::{Type, logging};
use gethostname::gethostname;
use network_interface::NetworkInterface;
use serde_yaml_ng::Mapping;
use std::net::{IpAddr, Ipv4Addr, TcpListener};
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

/// Порт занят для слушателя на конкретном адресе?
///
/// clod: проверять всегда `127.0.0.1` было неверно. Ядро слушает тот адрес,
/// который задан конфигом: с выключенным `allow-lan` — петлю, с включённым —
/// все интерфейсы. Занятый `0.0.0.0` при свободной петле — обычное дело на
/// Windows: проба на петле проходила, порт объявлялся свободным, а ядро потом
/// не поднималось. Спрашиваем ровно тот адрес, на который будет вставать
/// слушатель, поэтому ложных срабатываний в обратную сторону тоже нет.
pub fn port_is_taken_at(address: IpAddr, port: u16) -> bool {
    TcpListener::bind((address, port)).is_err()
}

/// Адрес, на котором ядро поднимет слушатель прокси.
const fn listener_address(allow_lan: bool) -> IpAddr {
    if allow_lan {
        IpAddr::V4(Ipv4Addr::UNSPECIFIED)
    } else {
        IpAddr::V4(Ipv4Addr::LOCALHOST)
    }
}

#[tauri::command]
pub async fn is_port_in_use(port: u16) -> bool {
    let allow_lan = crate::config::Config::clash()
        .await
        .latest_arc()
        .0
        .get("allow-lan")
        .and_then(serde_yaml_ng::Value::as_bool)
        .unwrap_or(false);
    port_is_taken_at(listener_address(allow_lan), port)
}

#[cfg(test)]
mod tests {
    use super::{listener_address, port_is_taken_at};
    use std::net::{IpAddr, Ipv4Addr, TcpListener};

    #[test]
    fn listener_address_follows_allow_lan() {
        assert_eq!(listener_address(false), IpAddr::V4(Ipv4Addr::LOCALHOST));
        assert_eq!(listener_address(true), IpAddr::V4(Ipv4Addr::UNSPECIFIED));
    }

    #[test]
    fn a_busy_port_is_reported_busy_and_a_free_one_is_not() {
        #[allow(clippy::unwrap_used)]
        let held = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        #[allow(clippy::unwrap_used)]
        let port = held.local_addr().unwrap().port();

        assert!(port_is_taken_at(IpAddr::V4(Ipv4Addr::LOCALHOST), port));

        drop(held);
        assert!(!port_is_taken_at(IpAddr::V4(Ipv4Addr::LOCALHOST), port));
    }

    #[test]
    fn a_wildcard_listener_hides_behind_a_loopback_probe_on_some_systems() {
        // Проба на петле и проба на всех интерфейсах — РАЗНЫЕ вопросы; именно
        // поэтому адрес выбирается по конфигу, а не берётся всегда петлевым.
        #[allow(clippy::unwrap_used)]
        let held = TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0)).unwrap();
        #[allow(clippy::unwrap_used)]
        let port = held.local_addr().unwrap().port();

        assert!(
            port_is_taken_at(IpAddr::V4(Ipv4Addr::UNSPECIFIED), port),
            "занятый wildcard обязан читаться как занятый"
        );
    }
}
