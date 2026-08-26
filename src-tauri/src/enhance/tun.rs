use serde_yaml_ng::{Mapping, Value};

#[cfg(target_os = "macos")]
use crate::process::AsyncHandler;

macro_rules! revise {
    ($map: expr, $key: expr, $val: expr) => {
        let ret_key = Value::String($key.into());
        $map.insert(ret_key, Value::from($val));
    };
}

pub fn use_tun(mut config: Mapping, enable: bool) -> Mapping {
    let tun_key = Value::from("tun");
    let tun_val = config.get(&tun_key);
    let mut tun_val = tun_val.map_or_else(Mapping::new, |val| {
        val.as_mapping().cloned().unwrap_or_else(Mapping::new)
    });

    if enable {
        let shaped_fake_ip = shape_dns_for_tun(&mut config);

        #[cfg(target_os = "macos")]
        if shaped_fake_ip && !crate::utils::resolve::dns::has_pending_restore() {
            AsyncHandler::spawn(move || async move {
                crate::utils::resolve::dns::set_public_dns("114.114.114.114".to_string()).await;
            });
        }
        #[cfg(not(target_os = "macos"))]
        let _ = shaped_fake_ip;
    } else {
        #[cfg(target_os = "macos")]
        if crate::utils::resolve::dns::has_pending_restore() {
            AsyncHandler::spawn(move || async move {
                crate::utils::resolve::dns::restore_public_dns().await;
            });
        }
    }

    revise!(tun_val, "enable", enable);
    revise!(config, "tun", tun_val);

    config
}

fn shape_dns_for_tun(config: &mut Mapping) -> bool {
    let dns_key = Value::from("dns");
    let mut dns_val = config.get(&dns_key).map_or_else(Mapping::new, |val| {
        val.as_mapping().cloned().unwrap_or_else(Mapping::new)
    });
    let ipv6_val = config
        .get(Value::from("ipv6"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    revise!(dns_val, "enable", true);

    let mode_key = Value::from("enhanced-mode");
    let current_mode = dns_val.get(&mode_key).and_then(|v| v.as_str()).unwrap_or("fake-ip");
    let fake_ip = current_mode == "fake-ip" || !dns_val.contains_key(&mode_key);

    if fake_ip {
        revise!(dns_val, "ipv6", ipv6_val);

        if !dns_val.contains_key(&mode_key) {
            revise!(dns_val, "enhanced-mode", "fake-ip");
        }

        if !dns_val.contains_key(Value::from("fake-ip-range")) {
            revise!(dns_val, "fake-ip-range", "198.18.0.1/16");
        }

        if ipv6_val && !dns_val.contains_key(Value::from("fake-ip-range6")) {
            revise!(dns_val, "fake-ip-range6", "2001:2::0/64");
        }
    }

    revise!(config, "dns", dns_val);
    fake_ip
}

pub fn ensure_dns_for_tun(mut config: Mapping, enable: bool) -> Mapping {
    if enable {
        shape_dns_for_tun(&mut config);
    }
    config
}

#[cfg(test)]
mod tests {
    use super::{ensure_dns_for_tun, shape_dns_for_tun};
    use serde_yaml_ng::{Mapping, Value};

    fn dns_of(config: &Mapping) -> Mapping {
        config
            .get(Value::from("dns"))
            .and_then(|v| v.as_mapping().cloned())
            .unwrap_or_default()
    }

    #[test]
    fn tun_always_turns_the_resolver_on() {
        let mut config = Mapping::new();
        assert!(shape_dns_for_tun(&mut config));
        let dns = dns_of(&config);
        assert_eq!(dns.get(Value::from("enable")), Some(&Value::from(true)));
        assert_eq!(dns.get(Value::from("enhanced-mode")), Some(&Value::from("fake-ip")));
        assert_eq!(
            dns.get(Value::from("fake-ip-range")),
            Some(&Value::from("198.18.0.1/16"))
        );
    }

    #[test]
    fn redir_host_keeps_its_mode_but_not_its_off_switch() {
        let mut dns = Mapping::new();
        dns.insert(Value::from("enhanced-mode"), Value::from("redir-host"));
        dns.insert(Value::from("enable"), Value::from(false));
        let mut config = Mapping::new();
        config.insert(Value::from("dns"), Value::from(dns));

        assert!(!shape_dns_for_tun(&mut config));
        let dns = dns_of(&config);
        assert_eq!(dns.get(Value::from("enhanced-mode")), Some(&Value::from("redir-host")));
        assert_eq!(dns.get(Value::from("enable")), Some(&Value::from(true)));
        assert!(!dns.contains_key(Value::from("fake-ip-range")));
    }

    #[test]
    fn floor_survives_the_dns_page_overwriting_the_block() {
        let mut page_dns = Mapping::new();
        page_dns.insert(Value::from("enable"), Value::from(false));
        page_dns.insert(Value::from("nameserver"), Value::from(vec!["1.1.1.1"]));
        let mut config = Mapping::new();
        config.insert(Value::from("dns"), Value::from(page_dns));

        let config = ensure_dns_for_tun(config, true);
        let dns = dns_of(&config);
        assert_eq!(dns.get(Value::from("enable")), Some(&Value::from(true)));
        assert!(dns.contains_key(Value::from("nameserver")));
    }

    #[test]
    fn tun_off_leaves_dns_alone() {
        let mut dns = Mapping::new();
        dns.insert(Value::from("enable"), Value::from(false));
        let mut config = Mapping::new();
        config.insert(Value::from("dns"), Value::from(dns));

        let config = ensure_dns_for_tun(config, false);
        assert_eq!(dns_of(&config).get(Value::from("enable")), Some(&Value::from(false)));
    }
}
