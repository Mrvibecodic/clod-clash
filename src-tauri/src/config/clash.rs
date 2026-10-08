use crate::constants::{network, tun as tun_const};
use crate::utils::dirs::{path_to_str, sidecar_ipc_path};
use crate::utils::{dirs, help};
use anyhow::Result;
use clash_verge_logging::{Type, logging};
use serde::{Deserialize, Serialize};
use serde_yaml_ng::{Mapping, Value};
use std::{
    borrow::Cow,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    str::FromStr as _,
};

/// Порт из значения конфига: число или строка с числом; нет, не разобрать,
/// ноль или больше 65535 — `default`.
fn port_value(value: Option<&Value>, default: u16) -> u16 {
    let port = value
        .and_then(|value| match value {
            Value::String(val_str) => val_str.parse().ok(),
            Value::Number(val_num) => val_num.as_u64().and_then(|u| u16::try_from(u).ok()),
            _ => None,
        })
        .unwrap_or(default);
    if port == 0 { default } else { port }
}

#[derive(Default, Debug, Clone)]
pub struct IClashTemp(pub Mapping);

impl IClashTemp {
    pub async fn new() -> Self {
        let clash_path_result = dirs::clash_path();
        let map_result = match clash_path_result.as_ref() {
            Ok(path) => help::read_mapping(path).await,
            Err(_) => Err(anyhow::anyhow!("Failed to get clash path")),
        };

        match map_result {
            Ok(mut map) => {
                map.remove("mode");
                let regenerated = Self::ensure_own_secret(&mut map);

                let template_map = Self::template().0;
                for (key, value) in template_map.into_iter() {
                    if !map.contains_key(&key) {
                        map.insert(key, value);
                    }
                }

                let config = Self(Self::guard(map));
                if regenerated && let Err(err) = config.save_config().await {
                    logging!(error, Type::Config, "failed to persist generated secret: {err}");
                }
                config
            }
            Err(err) => {
                logging!(error, Type::Config, "{err}");
                if let Ok(path) = clash_path_result.as_ref() {
                    crate::config::load_failures::keep_a_copy(path).await;
                }
                crate::config::load_failures::mark(crate::config::load_failures::ConfigFile::Clash);
                Self::template()
            }
        }
    }

    fn ensure_own_secret(map: &mut Mapping) -> bool {
        let needs_new = match map.get("secret") {
            Some(Value::String(secret)) => help::is_placeholder_secret(secret),
            Some(_) => false,
            None => true,
        };

        if needs_new {
            map.insert("secret".into(), help::random_secret().into());
        }
        needs_new
    }

    pub fn template() -> Self {
        let mut map = Mapping::new();
        let mut tun_config = Mapping::new();
        let mut cors_map = Mapping::new();

        tun_config.insert("enable".into(), false.into());
        tun_config.insert("stack".into(), tun_const::DEFAULT_STACK.into());
        tun_config.insert("auto-route".into(), true.into());
        tun_config.insert("strict-route".into(), false.into());
        tun_config.insert("auto-detect-interface".into(), true.into());
        tun_config.insert("dns-hijack".into(), tun_const::DNS_HIJACK.into());

        #[cfg(not(target_os = "windows"))]
        map.insert("redir-port".into(), network::ports::DEFAULT_REDIR.into());
        #[cfg(target_os = "linux")]
        map.insert("tproxy-port".into(), network::ports::DEFAULT_TPROXY.into());

        map.insert("socks-port".into(), network::ports::DEFAULT_SOCKS.into());
        map.insert("port".into(), network::ports::DEFAULT_HTTP.into());
        map.insert("allow-lan".into(), false.into());
        map.insert("ipv6".into(), false.into());
        map.insert(
            "external-controller".into(),
            network::DEFAULT_EXTERNAL_CONTROLLER.into(),
        );
        #[cfg(unix)]
        map.insert(
            "external-controller-unix".into(),
            Self::guard_external_controller_ipc().into(),
        );
        #[cfg(windows)]
        map.insert(
            "external-controller-pipe".into(),
            Self::guard_external_controller_ipc().into(),
        );
        map.insert("tun".into(), tun_config.into());
        cors_map.insert("allow-private-network".into(), true.into());
        cors_map.insert(
            "allow-origins".into(),
            vec![
                // Заглушка: пустой список ядро понимает как «пускать всех».
                "tauri://localhost",
                "https://yacd.metacubex.one",
                "https://metacubex.github.io",
                "https://board.zash.run.place",
            ]
            .into(),
        );
        map.insert("secret".into(), help::random_secret().into());
        map.insert("external-controller-cors".into(), cors_map.into());
        Self(map)
    }

    fn guard(mut config: Mapping) -> Mapping {
        #[cfg(not(target_os = "windows"))]
        let redir_port = Self::guard_redir_port(&config);
        #[cfg(target_os = "linux")]
        let tproxy_port = Self::guard_tproxy_port(&config);
        let socks_port = Self::guard_socks_port(&config);
        let port = Self::guard_port(&config);
        let ctrl = Self::guard_external_controller(&config);
        #[cfg(unix)]
        let external_controller_unix = Self::guard_external_controller_ipc();
        #[cfg(windows)]
        let external_controller_pipe = Self::guard_external_controller_ipc();

        #[cfg(not(target_os = "windows"))]
        config.insert("redir-port".into(), redir_port.into());
        #[cfg(target_os = "linux")]
        config.insert("tproxy-port".into(), tproxy_port.into());
        // clod:port-ladder — порт закрепляем, только если он у нас уже задан.
        // Отсутствие ключа означает «как в подписке», и подставлять сюда своё
        // умолчание нельзя: оно перебило бы порт из шаблона провайдера.
        if config.contains_key("mixed-port") {
            let mixed_port = Self::guard_mixed_port(&config);
            config.insert("mixed-port".into(), mixed_port.into());
        }
        config.insert("socks-port".into(), socks_port.into());
        config.insert("port".into(), port.into());
        config.insert("external-controller".into(), ctrl.into());

        #[cfg(unix)]
        config.insert("external-controller-unix".into(), external_controller_unix.into());
        #[cfg(windows)]
        config.insert("external-controller-pipe".into(), external_controller_pipe.into());
        config
    }

    pub fn patch_config(&mut self, patch: &Mapping) {
        for (key, value) in patch.iter() {
            if key.as_str() == Some("mode") {
                continue;
            }
            if Self::follows_the_subscription(key, value) {
                self.0.remove(key);
                continue;
            }
            // Окно TUN правит свои поля, а не весь блок: всё, что лежит в `tun`,
            // становится ключом приложения поверх подписки. `null` снимает поле.
            if key.as_str() == Some("tun")
                && let Some(fields) = value.as_mapping()
            {
                let mut tun = self.0.get(key).and_then(Value::as_mapping).cloned().unwrap_or_default();
                for (field, value) in fields {
                    if value.is_null() {
                        tun.remove(field);
                    } else {
                        tun.insert(field.to_owned(), value.to_owned());
                    }
                }
                self.0.insert(key.to_owned(), Value::Mapping(tun));
                continue;
            }
            self.0.insert(key.to_owned(), value.to_owned());
        }
    }

    pub const SUBSCRIPTION_LADDER_KEYS: &[&str] = &["log-level", "unified-delay", "mixed-port"];
    pub const FOLLOW_THE_SUBSCRIPTION: &str = "auto";

    pub fn follows_the_subscription(key: &Value, value: &Value) -> bool {
        key.as_str()
            .is_some_and(|key| Self::SUBSCRIPTION_LADDER_KEYS.contains(&key))
            && value.as_str() == Some(Self::FOLLOW_THE_SUBSCRIPTION)
    }

    pub fn unpin_legacy_defaults(map: &mut Mapping) -> bool {
        let stock_log_level = map.get("log-level").and_then(Value::as_str) == Some("info");
        let stock_unified_delay = map.get("unified-delay").and_then(Value::as_bool) == Some(true);
        if stock_log_level {
            map.remove("log-level");
        }
        if stock_unified_delay {
            map.remove("unified-delay");
        }
        stock_log_level || stock_unified_delay
    }

    /// Окно TUN прошлых версий при любом «Сохранить» и «Сбросить» записывало
    /// свои умолчания, и они перебивали подписку. Имя адаптера не снимаем: на
    /// него могут опираться правила брандмауэра и маршрутов вне клиента.
    pub fn unpin_tun_window_defaults(map: &mut Mapping) -> bool {
        let Some(tun) = map.get_mut("tun").and_then(Value::as_mapping_mut) else {
            return false;
        };
        let stale: Vec<Value> = tun
            .iter()
            .filter(|(key, value)| match key.as_str() {
                Some("mtu") => value.as_u64() == Some(1500),
                Some("route-exclude-address") => value.as_sequence().is_some_and(Vec::is_empty),
                Some("auto-redirect") => value.as_bool() == Some(false),
                _ => false,
            })
            .map(|(key, _)| key.to_owned())
            .collect();
        for key in &stale {
            tun.remove(key);
        }
        !stale.is_empty()
    }

    pub async fn save_config(&self) -> Result<()> {
        help::save_yaml(&dirs::clash_path()?, &self.0, Some(help::CLASH_CONFIG_HEADER)).await
    }

    pub fn get_mixed_port(&self) -> u16 {
        Self::guard_mixed_port(&self.0)
    }

    pub fn get_client_info(&self) -> ClashInfo {
        let config = &self.0;

        ClashInfo {
            mixed_port: Self::guard_mixed_port(config),
            socks_port: Self::guard_socks_port(config),
            port: Self::guard_port(config),
            server: Self::guard_client_ctrl(config),
            secret: config.get("secret").and_then(|value| match value {
                Value::String(val_str) => Some(val_str.clone()),
                Value::Bool(val_bool) => Some(val_bool.to_string()),
                Value::Number(val_num) => Some(val_num.to_string()),
                _ => None,
            }),
        }
    }

    #[cfg(not(target_os = "windows"))]
    pub fn guard_redir_port(config: &Mapping) -> u16 {
        port_value(config.get("redir-port"), 7895)
    }

    #[cfg(target_os = "linux")]
    pub fn guard_tproxy_port(config: &Mapping) -> u16 {
        port_value(config.get("tproxy-port"), network::ports::DEFAULT_TPROXY)
    }

    pub fn guard_mixed_port(config: &Mapping) -> u16 {
        port_value(config.get("mixed-port"), network::ports::DEFAULT_MIXED)
    }

    pub fn guard_socks_port(config: &Mapping) -> u16 {
        port_value(config.get("socks-port"), 7898)
    }

    pub fn guard_port(config: &Mapping) -> u16 {
        port_value(config.get("port"), 7899)
    }

    pub fn guard_server_ctrl(config: &Mapping) -> String {
        config
            .get("external-controller")
            .and_then(|value| match value.as_str() {
                Some(val_str) => {
                    let val_str = val_str.trim();

                    let val = match val_str.starts_with(':') {
                        true => Cow::Owned(format!("127.0.0.1{val_str}")),
                        false => Cow::Borrowed(val_str),
                    };

                    SocketAddr::from_str(&val).ok().map(|s| s.to_string())
                }
                None => None,
            })
            .unwrap_or_else(|| "127.0.0.1:9097".into())
    }

    pub fn guard_external_controller(config: &Mapping) -> String {
        Self::guard_server_ctrl(config)
    }

    pub fn guard_client_ctrl(config: &Mapping) -> String {
        let value = Self::guard_server_ctrl(config);
        match SocketAddr::from_str(value.as_str()) {
            Ok(mut socket) => {
                if socket.ip().is_unspecified() {
                    socket.set_ip(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)));
                }
                socket.to_string()
            }
            Err(_) => "127.0.0.1:9097".into(),
        }
    }

    pub fn guard_external_controller_ipc() -> String {
        sidecar_ipc_path()
            .ok()
            .and_then(|path| path_to_str(&path).ok().map(|s| s.into()))
            .unwrap_or_else(|| {
                logging!(error, Type::Config, "Failed to get IPC path");
                crate::constants::network::DEFAULT_EXTERNAL_CONTROLLER.into()
            })
    }
}

#[derive(Default, Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct ClashInfo {
    pub mixed_port: u16,
    pub socks_port: u16,
    pub port: u16,
    pub server: String,
    pub secret: Option<String>,
}

#[test]
fn test_clash_info() {
    fn get_case<T: Into<Value>, D: Into<Value>>(mp: T, ec: D) -> ClashInfo {
        let mut map = Mapping::new();
        map.insert("mixed-port".into(), mp.into());
        map.insert("external-controller".into(), ec.into());

        IClashTemp(IClashTemp::guard(map)).get_client_info()
    }

    fn get_result<S: Into<String>>(port: u16, server: S) -> ClashInfo {
        ClashInfo {
            mixed_port: port,
            socks_port: 7898,
            port: 7899,
            server: server.into(),
            secret: None,
        }
    }

    assert_eq!(
        IClashTemp(IClashTemp::guard(Mapping::new())).get_client_info(),
        get_result(7897, "127.0.0.1:9097")
    );

    assert_eq!(get_case("", ""), get_result(7897, "127.0.0.1:9097"));

    assert_eq!(get_case(65537, ""), get_result(7897, "127.0.0.1:9097"));

    assert_eq!(get_case(8888, "127.0.0.1:8888"), get_result(8888, "127.0.0.1:8888"));

    assert_eq!(get_case(8888, "   :98888 "), get_result(8888, "127.0.0.1:9097"));

    assert_eq!(get_case(8888, "0.0.0.0:8080  "), get_result(8888, "127.0.0.1:8080"));

    assert_eq!(get_case(8888, "0.0.0.0:8080"), get_result(8888, "127.0.0.1:8080"));

    assert_eq!(get_case(8888, "[::]:8080"), get_result(8888, "127.0.0.1:8080"));

    assert_eq!(get_case(8888, "192.168.1.1:8080"), get_result(8888, "192.168.1.1:8080"));

    assert_eq!(get_case(8888, "192.168.1.1:80800"), get_result(8888, "127.0.0.1:9097"));
}

#[test]
fn own_secret_replaces_the_upstream_placeholder_only() {
    fn secret_of(map: &Mapping) -> &str {
        map.get("secret").and_then(Value::as_str).unwrap_or_default()
    }

    let mut fresh = Mapping::new();
    assert!(IClashTemp::ensure_own_secret(&mut fresh));
    assert_eq!(secret_of(&fresh).len(), 32);

    for stale in [help::LEGACY_DEFAULT_SECRET, "", "   "] {
        let mut map = Mapping::new();
        map.insert("secret".into(), stale.into());
        assert!(IClashTemp::ensure_own_secret(&mut map), "{stale:?}");
        assert_ne!(secret_of(&map), stale);
    }

    let mut own = Mapping::new();
    own.insert("secret".into(), "hunter2".into());
    assert!(!IClashTemp::ensure_own_secret(&mut own));
    assert_eq!(secret_of(&own), "hunter2");

    let mut numeric = Mapping::new();
    numeric.insert("secret".into(), 42.into());
    assert!(!IClashTemp::ensure_own_secret(&mut numeric));

    let mut first = Mapping::new();
    let mut second = Mapping::new();
    IClashTemp::ensure_own_secret(&mut first);
    IClashTemp::ensure_own_secret(&mut second);
    assert_ne!(secret_of(&first), secret_of(&second));
}

#[cfg(test)]
mod tests {
    use super::{IClashTemp, port_value};
    use crate::constants::network;
    use serde_yaml_ng::{Mapping, Value};

    #[test]
    fn a_port_is_read_from_a_number_or_a_string_and_falls_back_on_zero_or_junk() {
        assert_eq!(port_value(Some(&Value::from(7890_u64)), 1), 7890);
        assert_eq!(port_value(Some(&Value::from("7891")), 1), 7891);
        assert_eq!(port_value(Some(&Value::from(0_u64)), 1), 1);
        assert_eq!(port_value(Some(&Value::from("порт")), 1), 1);
        assert_eq!(port_value(Some(&Value::Bool(true)), 1), 1);
        assert_eq!(port_value(None, 1), 1);
        // Не порт — как и та же строка: не обрезок по модулю 65536 (4464).
        assert_eq!(port_value(Some(&Value::from(70_000_u64)), 1), 1);
        assert_eq!(port_value(Some(&Value::from("70000")), 1), 1);
    }

    #[test]
    fn every_listener_port_reads_its_own_key() {
        type Guard = fn(&Mapping) -> u16;
        #[cfg_attr(target_os = "windows", allow(unused_mut))]
        let mut guards: Vec<(Guard, &str, u16)> = vec![
            (
                IClashTemp::guard_mixed_port,
                "mixed-port",
                network::ports::DEFAULT_MIXED,
            ),
            (IClashTemp::guard_socks_port, "socks-port", 7898),
            (IClashTemp::guard_port, "port", 7899),
        ];
        #[cfg(not(target_os = "windows"))]
        guards.push((IClashTemp::guard_redir_port, "redir-port", 7895));
        #[cfg(target_os = "linux")]
        guards.push((
            IClashTemp::guard_tproxy_port,
            "tproxy-port",
            network::ports::DEFAULT_TPROXY,
        ));
        let keys: Vec<&str> = guards.iter().map(|(_, key, _)| *key).collect();
        for (guard, key, default) in guards {
            // Остальные порты заданы — свой ключ не подменяется чужим.
            let read = |value: Option<Value>| {
                let mut config = Mapping::new();
                for other in keys.iter().filter(|other| **other != key) {
                    config.insert((*other).into(), 1111_u64.into());
                }
                if let Some(value) = value {
                    config.insert(key.into(), value);
                }
                guard(&config)
            };
            assert_eq!(read(Some(20_001_u64.into())), 20_001, "{key}");
            assert_eq!(read(Some("20002".into())), 20_002, "{key}");
            assert_eq!(read(Some(0_u64.into())), default, "{key}");
            assert_eq!(read(Some("порт".into())), default, "{key}");
            assert_eq!(read(Some(70_000_u64.into())), default, "{key}");
            assert_eq!(read(None), default, "{key}");
        }
    }
}
