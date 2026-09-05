use serde_yaml_ng::{Mapping, Value};
use smartstring::alias::String;
use std::collections::{HashMap, HashSet};

use crate::enhance::field::use_keys;

// clod: «mode» в списке обязателен — иначе смена режима маршрутизации не
// попадает в runtime-конфиг, UI читает старое значение, а ближайший reload
// возвращает старый режим в ядро.
const PATCH_CONFIG_INNER: [&str; 6] = ["mode", "allow-lan", "ipv6", "log-level", "unified-delay", "tunnels"];

#[derive(Default, Clone)]
pub struct IRuntime {
    pub config: Option<Mapping>,
    // Ключи, встречавшиеся в подписке (включая сгенерированные merge и script)
    // Эти ключи не обязательно все действуют
    pub exists_keys: HashSet<String>,
    // TODO возможно, стоит хранить в FixMap для повышения эффективности
    pub chain_logs: HashMap<String, Vec<(String, String)>>,
    // clod: отчёт фильтра заглушек — часть этой же сборки конфига. Живёт в
    // драфте и коммитится/откатывается вместе с ним: глобальный слот здесь
    // переживал бы откат неудачной валидации и описывал бы конфиг, который
    // ядро не приняло.
    pub sentinel_report: crate::enhance::SentinelReport,
}

impl IRuntime {
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    // Здесь меняются только allow-lan | ipv6 | log-level | tun | tunnels
    #[inline]
    pub fn patch_config(&mut self, patch: &Mapping) {
        let config = if let Some(config) = self.config.as_mut() {
            config
        } else {
            return;
        };

        for key in PATCH_CONFIG_INNER.iter() {
            if let Some(value) = patch.get(key) {
                if crate::config::IClashTemp::follows_the_subscription(&Value::from(*key), value) {
                    continue;
                }
                config.insert((*key).into(), value.clone());
            }
        }

        let Some(patch_tun) = patch.get("tun") else {
            return;
        };

        let tun_key = Value::from("tun");
        if !matches!(config.get(&tun_key), Some(Value::Mapping(_))) {
            config.insert(tun_key.clone(), Value::Mapping(Mapping::new()));
        }

        if let (Some(patch_tun_mapping), Some(Value::Mapping(tun))) = (patch_tun.as_mapping(), config.get_mut(&tun_key))
        {
            for key in use_keys(patch_tun_mapping) {
                if let Some(value) = patch_tun_mapping.get(key.as_str()) {
                    tun.insert(Value::from(key.as_str()), value.clone());
                }
            }
        }
    }

    /// Обновляет конфигурацию цепочки прокси
    ///
    /// Функция обновляет конфигурацию `proxies` и `proxy-groups`, обрабатывает
    /// изменение цепочки прокси или (при передаче None) её удаление.
    ///
    /// Пример конфигурации:
    ///
    /// ```json
    /// {
    ///     "proxies": [
    ///         {
    ///             "name": "Входной узел",
    ///             "type": "xxx",
    ///             "server": "xxx",
    ///             "port": "xxx",
    ///             "ports": "xxx",
    ///             "password": "xxx",
    ///             "skip-cert-verify": "xxx"
    ///         },
    ///         {
    ///             "name": "hop_node_1_xxxx",
    ///             "type": "xxx",
    ///             "server": "xxx",
    ///             "port": "xxx",
    ///             "ports": "xxx",
    ///             "password": "xxx",
    ///             "skip-cert-verify": "xxx",
    ///             "dialer-proxy": "Входной узел"
    ///         },
    ///         {
    ///             "name": "Выходной узел",
    ///             "type": "xxx",
    ///             "server": "xxx",
    ///             "port": "xxx",
    ///             "ports": "xxx",
    ///             "password": "xxx",
    ///             "skip-cert-verify": "xxx",
    ///             "dialer-proxy": "hop_node_1_xxxx"
    ///         }
    ///     ],
    ///     "proxy-groups": [
    ///         {
    ///             "name": "proxy_chain",
    ///             "type": "select",
    ///             "proxies": ["Выходной узел"]
    ///         }
    ///     ]
    /// }
    /// ```
    #[inline]
    pub fn update_proxy_chain_config(&mut self, proxy_chain_config: Option<Value>) {
        let config = if let Some(config) = self.config.as_mut() {
            config
        } else {
            return;
        };

        if let Some(Value::Sequence(proxies)) = config.get_mut("proxies") {
            proxies.iter_mut().for_each(|proxy| {
                if let Some(proxy) = proxy.as_mapping_mut()
                    && proxy.get("dialer-proxy").is_some()
                {
                    proxy.remove("dialer-proxy");
                }
            });
        }

        if let Some(Value::Sequence(dialer_proxies)) = proxy_chain_config
            && let Some(Value::Sequence(proxies)) = config.get_mut("proxies")
        {
            for (i, dialer_proxy) in dialer_proxies.iter().enumerate() {
                if let Some(Value::Mapping(proxy)) =
                    proxies.iter_mut().find(|proxy| proxy.get("name") == Some(dialer_proxy))
                    && i != 0
                    && let Some(dialer_proxy) = dialer_proxies.get(i - 1)
                {
                    proxy.insert("dialer-proxy".into(), dialer_proxy.to_owned());
                }
            }
        }
    }
}
