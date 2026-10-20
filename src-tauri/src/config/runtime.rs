use serde_yaml_ng::{Mapping, Value};
use smartstring::alias::String;

use crate::enhance::field::use_keys;

// clod: «mode» в списке обязателен — иначе смена режима маршрутизации не
// попадает в runtime-конфиг, UI читает старое значение, а ближайший reload
// возвращает старый режим в ядро.
const PATCH_CONFIG_INNER: [&str; 6] = ["mode", "allow-lan", "ipv6", "log-level", "unified-delay", "tunnels"];

/// Чего собранный конфиг просит от системного DNS (macOS): подменять ли его и
/// «сформирован» ли fake-ip под туннель. Часть сборки, а не глобальное состояние:
/// заявка относится ровно к тому конфигу, с которым едет, и применяется только
/// после того, как ядро его приняло.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct DnsDesire {
    pub want_base: bool,
    pub shaped_fake_ip: bool,
}

#[derive(Default, Clone)]
pub struct IRuntime {
    pub config: Option<Mapping>,
    /// Профиль, из которого собран конфиг. Пометка «не применено» снимается с
    /// него, а не с того, кто окажется текущим в реестре в момент доставки.
    pub profile_uid: Option<String>,
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub dns_desire: Option<DnsDesire>,
    /// Блок `dns` и `hosts` подписки до страницы DNS: от него редактор
    /// показывает значения, а сохранение считает отличия.
    pub dns_base: Option<crate::enhance::dns_page::Base>,
    /// Ключи, которые цепочки merge и script записали, а приложение отбросило
    /// (плоскость управления и свои поля `tun`) — человеку об этом говорят.
    pub discarded_keys: Vec<String>,
    /// Список раздачи, который не читается, когда раздачу включил человек:
    /// раздача не открыта, человеку об этом говорят.
    pub unreadable_lan_list: Option<&'static str>,
    // clod: отчёт фильтра заглушек — часть этой же сборки конфига. Живёт в
    // драфте и коммитится/откатывается вместе с ним: глобальный слот здесь
    // переживал бы откат неудачной валидации и описывал бы конфиг, который
    // ядро не приняло.
    pub sentinel_report: crate::enhance::SentinelReport,
    /// Узлы и провайдеры самой подписки — до «Прокси», Merge и Script.
    pub own_nodes: crate::config::proxy_label::OwnNodes,
    pub proxy_chain: Option<Vec<String>>,
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
}
