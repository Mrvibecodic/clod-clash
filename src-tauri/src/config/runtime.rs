use serde_yaml_ng::{Mapping, Value};
use smartstring::alias::String;

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
    pub proxy_chain_dropped: Option<crate::enhance::proxy_chain::Dropped>,
}

impl IRuntime {
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Режим маршрутизации, который ядро уже приняло.
    pub fn set_mode(&mut self, mode: &str) {
        if let Some(config) = self.config.as_mut() {
            config.insert("mode".into(), Value::from(mode));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::IRuntime;
    use serde_yaml_ng::{Mapping, Value};

    #[test]
    fn the_accepted_mode_lands_only_in_an_existing_runtime_config() {
        let mut empty = IRuntime::new();
        empty.set_mode("global");
        assert!(empty.config.is_none());

        let mut built = IRuntime {
            config: Some(Mapping::new()),
            ..IRuntime::new()
        };
        built.set_mode("global");
        assert_eq!(
            built.config.as_ref().and_then(|config| config.get("mode")),
            Some(&Value::from("global"))
        );
    }
}
