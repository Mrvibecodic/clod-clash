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
        let shaped_fake_ip = shape_dns_for_tun(&mut config);

        #[cfg(target_os = "macos")]
        if shaped_fake_ip {
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
        #[cfg(not(target_os = "macos"))]
        let _ = shaped_fake_ip;
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

/// clod:tun-dns-floor — DNS, без которого поднятый TUN бесполезен.
///
/// Правило одно на два вызова, поэтому и живёт отдельной функцией: `use_tun`
/// применяет его до страницы DNS, а `ensure_dns_for_tun` — ещё раз после неё
/// (см. комментарий там). Возвращает `true`, если конфиг доведён до fake-ip:
/// на macOS по этому же условию подменяется системный резолвер.
///
/// Что делается:
///   * `dns.enable = true` — БЕЗУСЛОВНО. TUN перехватывает 53-й порт, и с
///     выключенным резолвером запросы уходят в никуда: имена перестают
///     разрешаться совсем. Это верно и для `redir-host`, поэтому включение
///     вынесено из ветки fake-ip, где оно раньше жило.
///   * остальное — только когда `enhanced-mode` это `fake-ip` или не задан.
///     Человек, выбравший `redir-host` осознанно, не должен получить чужой
///     режим обратно только потому, что включил туннель.
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

        // При включённом IPv6 добавляем диапазон fake-ip для IPv6
        if ipv6_val && !dns_val.contains_key(Value::from("fake-ip-range6")) {
            revise!(dns_val, "fake-ip-range6", "fdfe:dcba:9876::1/64");
        }
    }

    revise!(config, "dns", dns_val);
    fake_ip
}

/// clod:tun-dns-floor — тот же пол, но ПОСЛЕ страницы DNS.
///
/// `apply_dns_settings` кладёт блок `dns` из `dns_config.yaml` целиком
/// (`config.insert`, не merge), то есть стирает всё, что `use_tun` только что
/// выставил строкой выше. Пользователь с включённой страницей DNS и файлом без
/// `enable: true` получал поднятый туннель с выключенным резолвером — и это
/// ещё до всяких merge/script. Тот же баг живёт у Koala Clash; у FlClashX его
/// нет ровно потому, что там такой же пол по `dns.enable`.
///
/// Побочный эффект для macOS сюда не переносится: он уже сработал в `use_tun`
/// и защищён своим файлом состояния.
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
        // Пустой конфиг: пол обязан создать блок и включить резолвер.
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
        // Осознанный выбор режима не отбираем...
        let mut dns = Mapping::new();
        dns.insert(Value::from("enhanced-mode"), Value::from("redir-host"));
        dns.insert(Value::from("enable"), Value::from(false));
        let mut config = Mapping::new();
        config.insert(Value::from("dns"), Value::from(dns));

        assert!(!shape_dns_for_tun(&mut config));
        let dns = dns_of(&config);
        assert_eq!(dns.get(Value::from("enhanced-mode")), Some(&Value::from("redir-host")));
        // ...а выключенный резолвер под TUN — это отсутствие имён вообще.
        assert_eq!(dns.get(Value::from("enable")), Some(&Value::from(true)));
        assert!(!dns.contains_key(Value::from("fake-ip-range")));
    }

    #[test]
    fn floor_survives_the_dns_page_overwriting_the_block() {
        // Ровно тот случай, ради которого пол и заведён: страница DNS положила
        // свой блок поверх того, что выставил `use_tun`.
        let mut page_dns = Mapping::new();
        page_dns.insert(Value::from("enable"), Value::from(false));
        page_dns.insert(Value::from("nameserver"), Value::from(vec!["1.1.1.1"]));
        let mut config = Mapping::new();
        config.insert(Value::from("dns"), Value::from(page_dns));

        let config = ensure_dns_for_tun(config, true);
        let dns = dns_of(&config);
        assert_eq!(dns.get(Value::from("enable")), Some(&Value::from(true)));
        // Список серверов пользователя пол не трогает.
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
