//! Ключ сети — по чему результаты проверки 16–20 привязываются к месту.
//!
//! clod:freeze — берётся настоящий путь машины наружу: интерфейсы со шлюзом,
//! кроме нашего туннеля и адаптеров песочниц (те же правила, что у сторожа
//! среды). Признаки — тип подключения и MAC роутера; без MAC (PPPoE, модем) —
//! адрес шлюза и DNS. Имя Wi-Fi не берётся: Windows 11 24H2 и macOS 14+ отдают
//! его только с геолокацией, а MAC роутера сети и так различает. Два
//! подключения сразу (кабель и Wi-Fi) — отдельная сеть: трафик может уйти
//! любым из них.
//!
//! В файлы и журнал уходит только хеш ключа, MAC и адреса — никогда.

use std::collections::BTreeSet;

use netdev::interface::{state::OperState, types::InterfaceType};
use sha2::{Digest as _, Sha256};

const KEY_HEX_LEN: usize = 16;

/// Что из интерфейса нужно ключу: без зависимости от `netdev` в тестах.
pub(super) struct Seen {
    pub names: Vec<std::string::String>,
    pub kind: InterfaceType,
    pub up: bool,
    pub gateway_mac: Option<std::string::String>,
    pub gateway_ips: Vec<std::string::String>,
    pub dns: Vec<std::string::String>,
}

const fn kind_carries_the_path(kind: InterfaceType) -> bool {
    !matches!(
        kind,
        InterfaceType::Loopback | InterfaceType::Tunnel | InterfaceType::ProprietaryVirtual
    )
}

fn kind_label(kind: InterfaceType) -> std::string::String {
    format!("{kind:?}")
}

/// Ключ сети и признак «MAC роутера не достался там, где он обычно есть».
///
/// clod:freeze — на Linux и macOS MAC шлюза берётся из таблицы соседей, и
/// сразу после пробуждения или подъёма линка записи там может ещё не быть:
/// ключ сложился бы из шлюза и DNS и выглядел бы новой сетью. Заход смотрит
/// на признак и переспрашивает чуть позже.
pub(super) struct Key {
    pub hash: std::string::String,
    pub mac_missing: bool,
}

/// MAC шлюза положен кабелю и Wi-Fi; у PPPoE и модема его не бывает.
const fn kind_has_a_router_mac(kind: InterfaceType) -> bool {
    matches!(
        kind,
        InterfaceType::Ethernet
            | InterfaceType::Ethernet3Megabit
            | InterfaceType::FastEthernetT
            | InterfaceType::FastEthernetFx
            | InterfaceType::GigabitEthernet
            | InterfaceType::Wireless80211
    )
}

/// Признак одного интерфейса со шлюзом или `None`, если он пути наружу не несёт.
fn part_of(seen: &Seen) -> Option<std::string::String> {
    if !seen.up || !kind_carries_the_path(seen.kind) {
        return None;
    }
    if seen
        .names
        .iter()
        .any(|name| crate::feat::environment::stands_apart(name))
    {
        return None;
    }
    let kind = kind_label(seen.kind);
    if let Some(mac) = seen.gateway_mac.as_deref() {
        return Some(format!("{kind};mac={mac}"));
    }
    if seen.gateway_ips.is_empty() {
        return None;
    }
    let mut ips = seen.gateway_ips.clone();
    ips.sort();
    let mut dns = seen.dns.clone();
    dns.sort();
    Some(format!("{kind};gw={};dns={}", ips.join(","), dns.join(",")))
}

/// Ключ сети по увиденным интерфейсам: `None` — пути наружу нет или он не
/// распознан.
pub(super) fn key_of(seen: &[Seen]) -> Option<Key> {
    let mut parts = BTreeSet::new();
    let mut mac_missing = false;
    for one in seen {
        let Some(part) = part_of(one) else {
            continue;
        };
        mac_missing |= one.gateway_mac.is_none() && kind_has_a_router_mac(one.kind);
        parts.insert(part);
    }
    if parts.is_empty() {
        return None;
    }
    let joined = parts.into_iter().collect::<Vec<_>>().join("|");
    let digest = Sha256::digest(joined.as_bytes());
    Some(Key {
        hash: hex::encode(digest)[..KEY_HEX_LEN].to_owned(),
        mac_missing,
    })
}

fn seen_of(interface: netdev::Interface) -> Seen {
    let netdev::Interface {
        name,
        friendly_name,
        if_type,
        oper_state,
        gateway,
        dns_servers,
        ..
    } = interface;
    let mut names = vec![name];
    names.extend(friendly_name);
    let (gateway_mac, gateway_ips) = gateway.map_or((None, Vec::new()), |gateway| {
        let mac = (gateway.mac_addr != netdev::MacAddr::zero()).then(|| gateway.mac_addr.to_string());
        let mut ips: Vec<std::string::String> = gateway.ipv4.iter().map(ToString::to_string).collect();
        ips.extend(gateway.ipv6.iter().map(ToString::to_string));
        (mac, ips)
    });
    Seen {
        names,
        kind: if_type,
        up: !matches!(
            oper_state,
            OperState::Down | OperState::LowerLayerDown | OperState::NotPresent
        ),
        gateway_mac,
        gateway_ips,
        dns: dns_servers.iter().map(ToString::to_string).collect(),
    }
}

/// Ключ текущей сети. Перечисление интерфейсов — системные вызовы, звать под
/// `spawn_blocking`.
pub(super) fn current() -> Option<Key> {
    let seen: Vec<Seen> = netdev::get_interfaces().into_iter().map(seen_of).collect();
    key_of(&seen)
}

#[cfg(test)]
mod tests {
    use super::{InterfaceType, KEY_HEX_LEN, Seen, key_of};

    fn hash(seen: &[Seen]) -> Option<std::string::String> {
        key_of(seen).map(|key| key.hash)
    }

    fn wifi(mac: &str) -> Seen {
        Seen {
            names: vec!["wlan0".into()],
            kind: InterfaceType::Wireless80211,
            up: true,
            gateway_mac: Some(mac.into()),
            gateway_ips: vec!["192.168.0.1".into()],
            dns: vec!["192.168.0.1".into()],
        }
    }

    #[test]
    fn the_router_mac_tells_networks_apart_and_the_address_does_not() {
        let home = hash(&[wifi("aa:bb:cc:00:00:01")]);
        let cafe = hash(&[wifi("aa:bb:cc:00:00:02")]);
        assert!(home.is_some());
        assert_ne!(home, cafe);
        assert_eq!(home.as_ref().map(std::string::String::len), Some(KEY_HEX_LEN));
    }

    fn pppoe(ips: &[&str], dns: &[&str]) -> Seen {
        let mut seen = wifi("");
        seen.kind = InterfaceType::Ppp;
        seen.gateway_mac = None;
        seen.gateway_ips = ips.iter().map(|ip| (*ip).into()).collect();
        seen.dns = dns.iter().map(|ip| (*ip).into()).collect();
        seen
    }

    #[test]
    fn without_a_mac_the_gateway_and_dns_stand_in() {
        let one = hash(&[pppoe(&["10.0.0.1"], &["8.8.8.8", "1.1.1.1"])]);
        assert!(one.is_some());
        assert_eq!(one, hash(&[pppoe(&["10.0.0.1"], &["1.1.1.1", "8.8.8.8"])]));
        assert_ne!(one, hash(&[pppoe(&["10.0.0.2"], &["8.8.8.8", "1.1.1.1"])]));
        assert_ne!(one, hash(&[pppoe(&["10.0.0.1"], &["9.9.9.9"])]));
    }

    #[test]
    fn our_tunnel_sandboxes_and_down_links_do_not_count() {
        let mut tunnel = wifi("aa:bb:cc:00:00:09");
        tunnel.names = vec!["Mihomo".into()];
        let mut docker = wifi("aa:bb:cc:00:00:08");
        docker.names = vec!["docker0".into()];
        let mut down = wifi("aa:bb:cc:00:00:07");
        down.up = false;
        let mut no_gateway = wifi("");
        no_gateway.gateway_mac = None;
        no_gateway.gateway_ips.clear();
        assert_eq!(hash(&[tunnel, docker, down, no_gateway]), None);
    }

    #[test]
    fn a_missing_router_mac_is_flagged_only_where_one_is_expected() {
        let mut wifi_without_mac = wifi("");
        wifi_without_mac.gateway_mac = None;
        assert_eq!(key_of(&[wifi_without_mac]).map(|key| key.mac_missing), Some(true));
        assert_eq!(
            key_of(&[wifi("aa:bb:cc:00:00:01")]).map(|key| key.mac_missing),
            Some(false)
        );
        assert_eq!(
            key_of(&[pppoe(&["10.0.0.1"], &["8.8.8.8"])]).map(|key| key.mac_missing),
            Some(false)
        );
    }

    #[test]
    fn the_windows_friendly_name_is_checked_too() {
        let mut tunnel = wifi("aa:bb:cc:00:00:09");
        tunnel.names = vec!["{GUID}".into(), "Mihomo".into()];
        assert_eq!(hash(&[tunnel]), None);
    }

    #[test]
    fn cable_and_wifi_together_are_their_own_network() {
        let mut cable = wifi("aa:bb:cc:00:00:01");
        cable.kind = InterfaceType::Ethernet;
        cable.names = vec!["eth0".into()];
        let only_wifi = hash(&[wifi("aa:bb:cc:00:00:01")]);
        let both = hash(&[wifi("aa:bb:cc:00:00:01"), cable]);
        assert_ne!(only_wifi, both);
    }

    #[test]
    fn the_order_of_interfaces_does_not_matter() {
        let mut cable = wifi("aa:bb:cc:00:00:01");
        cable.kind = InterfaceType::Ethernet;
        let a = hash(&[wifi("aa:bb:cc:00:00:02"), cable]);
        let mut cable = wifi("aa:bb:cc:00:00:01");
        cable.kind = InterfaceType::Ethernet;
        let b = hash(&[cable, wifi("aa:bb:cc:00:00:02")]);
        assert_eq!(a, b);
    }
}
