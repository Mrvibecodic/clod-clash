use serde_yaml_ng::{Mapping, Value};

#[cfg(target_os = "macos")]
use crate::process::AsyncHandler;

macro_rules! revise {
    ($map: expr, $key: expr, $val: expr) => {
        let ret_key = Value::String($key.into());
        $map.insert(ret_key, Value::from($val));
    };
}

// if key not exists then append value
#[allow(unused_macros)]
macro_rules! append {
    ($map: expr, $key: expr, $val: expr) => {
        let ret_key = Value::String($key.into());
        if !$map.contains_key(&ret_key) {
            $map.insert(ret_key, Value::from($val));
        }
    };
}

pub fn use_tun(mut config: Mapping, enable: bool) -> Mapping {
    let tun_key = Value::from("tun");
    let tun_val = config.get(&tun_key);
    let mut tun_val = tun_val.map_or_else(Mapping::new, |val| {
        val.as_mapping().cloned().unwrap_or_else(Mapping::new)
    });

    if enable {
        // Читаем конфигурацию DNS
        let dns_key = Value::from("dns");
        let dns_val = config.get(&dns_key);
        let mut dns_val = dns_val.map_or_else(Mapping::new, |val| {
            val.as_mapping().cloned().unwrap_or_else(Mapping::new)
        });
        let ipv6_key = Value::from("ipv6");
        let ipv6_val = config.get(&ipv6_key).and_then(|v| v.as_bool()).unwrap_or(false);

        // Проверяем текущую настройку enhanced-mode
        let current_mode = dns_val
            .get(Value::from("enhanced-mode"))
            .and_then(|v| v.as_str())
            .unwrap_or("fake-ip");

        // Меняем конфиг DNS только если enhanced-mode - fake-ip или не задан
        if current_mode == "fake-ip" || !dns_val.contains_key(Value::from("enhanced-mode")) {
            revise!(dns_val, "enable", true);
            revise!(dns_val, "ipv6", ipv6_val);

            if !dns_val.contains_key(Value::from("enhanced-mode")) {
                revise!(dns_val, "enhanced-mode", "fake-ip");
            }

            if !dns_val.contains_key(Value::from("fake-ip-range")) {
                revise!(dns_val, "fake-ip-range", "198.18.0.1/16");
            }

            // При включённом IPv6 добавляем диапазон fake-ip для IPv6
            if ipv6_val && !dns_val.contains_key(Value::from("fake-ip-range6")) {
                revise!(dns_val, "fake-ip-range6", "fdfe:dcba:9876::1/64");
            }

            #[cfg(target_os = "macos")]
            {
                // clod: раньше сюда прилетала пара «восстановить + подменить» на
                // КАЖДУЮ генерацию конфига (а это каждый патч настроек и каждое
                // обновление подписки): две задачи гонялись друг с другом, и при
                // неудачном порядке системный DNS оставался нашим навсегда.
                // Теперь подменяем один раз — пока файл состояния на месте,
                // трогать нечего.
                if !crate::utils::resolve::dns::has_pending_restore() {
                    AsyncHandler::spawn(move || async move {
                        crate::utils::resolve::dns::set_public_dns("114.114.114.114".to_string()).await;
                    });
                }
            }
        }

        // При включённом TUN записываем изменённый конфиг DNS обратно
        revise!(config, "dns", dns_val);
    } else {
        // При выключенном TUN только восстанавливаем системный DNS, настройки
        // DNS в конфиге не трогаем
        #[cfg(target_os = "macos")]
        if crate::utils::resolve::dns::has_pending_restore() {
            AsyncHandler::spawn(move || async move {
                crate::utils::resolve::dns::restore_public_dns().await;
            });
        }
    }

    // Обновляем конфигурацию TUN
    revise!(tun_val, "enable", enable);
    revise!(config, "tun", tun_val);

    config
}
