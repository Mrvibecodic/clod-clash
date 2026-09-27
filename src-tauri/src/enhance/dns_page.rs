//! Страница DNS подписки: только то, чем человек отличается от подписки.
//!
//! clod:dns-page-diff — прежде страница заводилась полной копией блока `dns`
//! подписки и дальше заменяла его целиком. Копия замирала в момент создания:
//! провайдер переименовывал набор правил или группу — страница держала старое
//! имя, и ядро либо отказывало всему конфигу, либо резолвило в никуда.
//! Теперь файл хранит только ключи, которые человек задал сам; всё, о чём он
//! молчит, каждый раз берётся из свежей подписки. Ключ, равный значению
//! подписки, — не отличие: при сохранении он не пишется, при первом чтении
//! старого файла — вычёркивается.

use serde_yaml_ng::{Mapping, Value};

/// Первая строка файла страницы новой раскладки. По ней же узнаётся старый
/// файл-копия, который надо один раз свести к отличиям.
pub const PAGE_HEADER: &str = "# Clod DNS page: only what differs from the subscription";

/// Файл новой раскладки узнаётся по шапке в первой строке.
pub fn is_a_diff_page(raw: &str) -> bool {
    raw.lines().next().map(str::trim) == Some(PAGE_HEADER)
}

/// Шапка для переписанного файла: наша строка плюс комментарии, которые человек
/// оставил в старом файле, — они не теряются, хоть и переезжают наверх.
pub fn header_keeping_the_comments(raw: &str) -> String {
    let mut header = String::from(PAGE_HEADER);
    for line in raw.lines().map(str::trim) {
        if line.starts_with('#') && line != PAGE_HEADER && line != LEGACY_HEADER {
            header.push('\n');
            header.push_str(line);
        }
    }
    header
}

/// Шапка файлов прежней раскладки — полной копии блока подписки.
pub const LEGACY_HEADER: &str = "# Clash Verge DNS Config";

/// Ключи блока `dns`, значения которых — списки серверов с возможным хвостом
/// `#имя` (группа, узел или сетевой интерфейс).
const SERVER_LIST_KEYS: &[&str] = &[
    "nameserver",
    "default-nameserver",
    "proxy-server-nameserver",
    "direct-nameserver",
    "fallback",
];

/// Содержимое файла страницы: блок `dns` и, отдельно, `hosts`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Page {
    pub dns: Mapping,
    pub hosts: Option<Mapping>,
}

impl Page {
    /// Разобрать файл. Поддерживается и старая плоская раскладка без корня
    /// `dns`: тогда блоком считается весь файл, кроме `hosts`.
    pub fn from_file(file: &Mapping) -> Option<Self> {
        let hosts = file.get("hosts").and_then(Value::as_mapping).cloned();
        let dns = match file.get("dns") {
            Some(dns) => dns.as_mapping()?.clone(),
            None => {
                let mut flat = file.clone();
                flat.remove("hosts");
                flat
            }
        };
        Some(Self { dns, hosts })
    }

    pub fn parse(raw: &str) -> Option<Self> {
        let file = serde_yaml_ng::from_str::<Mapping>(raw).ok()?;
        Self::from_file(&file)
    }

    pub fn is_empty(&self) -> bool {
        self.dns.is_empty() && self.hosts.as_ref().is_none_or(Mapping::is_empty)
    }

    /// Файл на диске: `dns` всегда, `hosts` — только непустой.
    pub fn to_file(&self) -> Mapping {
        let mut file = Mapping::new();
        file.insert("dns".into(), Value::Mapping(self.dns.clone()));
        if let Some(hosts) = self.hosts.as_ref().filter(|hosts| !hosts.is_empty()) {
            file.insert("hosts".into(), Value::Mapping(hosts.clone()));
        }
        file
    }

    /// Оставить только отличия от блока подписки: ключ с тем же значением —
    /// не выбор человека, а копия, и его место — у подписки.
    pub fn differences_from(mut self, base: &Base) -> Self {
        self.dns.retain(|key, value| base.dns.get(key) != Some(value));
        if self
            .hosts
            .as_ref()
            .is_some_and(|hosts| hosts.is_empty() || Some(hosts) == base.hosts.as_ref())
        {
            self.hosts = None;
        }
        self
    }

    /// Наложить страницу на конфиг: заданные ключи `dns` заменяют ключи
    /// подписки по одному, остальные остаются ей; непустой `hosts` заменяет
    /// `hosts` целиком.
    pub fn lay_over(&self, config: &mut Mapping) {
        if !self.dns.is_empty() {
            let dns = config
                .entry("dns".into())
                .or_insert_with(|| Value::Mapping(Mapping::new()));
            match dns.as_mapping_mut() {
                Some(dns) => dns.extend(self.dns.clone()),
                None => *dns = Value::Mapping(self.dns.clone()),
            }
        }
        if let Some(hosts) = self.hosts.as_ref().filter(|hosts| !hosts.is_empty()) {
            config.insert("hosts".into(), Value::Mapping(hosts.clone()));
        }
    }

    /// Убрать ключи, ссылающиеся через `rule-set:` на наборы правил, которых
    /// подписка не объявляет (`declared` — есть ли такое имя). Возвращает
    /// имена убранных ключей вида `dns.nameserver-policy`.
    pub fn drop_keys_referring_to_missing_rule_sets(&mut self, declared: impl Fn(&str) -> bool) -> Vec<String> {
        let mut dropped = Vec::new();
        let mut policy_refs = Vec::new();
        if let Some(policy) = self.dns.get("nameserver-policy").and_then(Value::as_mapping) {
            for matcher in policy.keys().filter_map(Value::as_str) {
                collect_rule_sets(matcher, &mut policy_refs);
            }
        }
        if policy_refs.iter().any(|name| !declared(name)) {
            self.dns.remove("nameserver-policy");
            dropped.push("dns.nameserver-policy".to_owned());
        }
        let mut filter_refs = Vec::new();
        if let Some(filter) = self.dns.get("fake-ip-filter").and_then(Value::as_sequence) {
            for entry in filter.iter().filter_map(Value::as_str) {
                collect_rule_sets(entry, &mut filter_refs);
            }
        }
        if filter_refs.iter().any(|name| !declared(name)) {
            self.dns.remove("fake-ip-filter");
            dropped.push("dns.fake-ip-filter".to_owned());
        }
        dropped
    }

    /// Имена, на которые ссылается страница и которых в подписке может не
    /// быть: группы/узлы из хвостов `#имя` у серверов и наборы правил из
    /// `rule-set:` в фильтре fake-ip и политике серверов.
    pub fn references(&self) -> References {
        let mut refs = References::default();
        for key in SERVER_LIST_KEYS {
            if let Some(servers) = self.dns.get(*key) {
                collect_server_names(servers, &mut refs.proxies);
            }
        }
        if let Some(policy) = self.dns.get("nameserver-policy").and_then(Value::as_mapping) {
            for (matcher, servers) in policy {
                if let Some(matcher) = matcher.as_str() {
                    collect_rule_sets(matcher, &mut refs.rule_sets);
                }
                collect_server_names(servers, &mut refs.proxies);
            }
        }
        if let Some(filter) = self.dns.get("fake-ip-filter").and_then(Value::as_sequence) {
            for entry in filter.iter().filter_map(Value::as_str) {
                collect_rule_sets(entry, &mut refs.rule_sets);
            }
        }
        refs.proxies.sort();
        refs.proxies.dedup();
        refs.rule_sets.sort();
        refs.rule_sets.dedup();
        refs
    }
}

/// Блок `dns` подписки (после наших умолчаний и формовки под TUN) и её
/// `hosts` — то, поверх чего ложится страница и от чего считаются отличия.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Base {
    pub dns: Mapping,
    pub hosts: Option<Mapping>,
}

impl Base {
    pub fn of(config: &Mapping) -> Self {
        Self {
            dns: config
                .get("dns")
                .and_then(Value::as_mapping)
                .cloned()
                .unwrap_or_default(),
            hosts: config.get("hosts").and_then(Value::as_mapping).cloned(),
        }
    }

    /// То, что видит человек в редакторе: подписка, поверх неё его отличия.
    pub fn with(&self, page: &Page) -> Page {
        let mut dns = self.dns.clone();
        dns.extend(page.dns.clone());
        Page {
            dns,
            hosts: page
                .hosts
                .as_ref()
                .filter(|hosts| !hosts.is_empty())
                .or(self.hosts.as_ref())
                .cloned(),
        }
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct References {
    pub proxies: Vec<String>,
    pub rule_sets: Vec<String>,
}

/// Хвост адреса сервера после `#`: части без `=` — имя прокси (или
/// интерфейса), части с `=` — параметры ядра вроде `skip-cert-verify=true`.
fn collect_server_names(servers: &Value, out: &mut Vec<String>) {
    let each: Vec<&str> = match servers {
        Value::String(one) => vec![one.as_str()],
        Value::Sequence(many) => many.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    };
    for server in each {
        let Some((_, tail)) = server.split_once('#') else {
            continue;
        };
        for part in tail.split('&') {
            let part = part.trim();
            if !part.is_empty() && !part.contains('=') {
                out.push(part.to_owned());
            }
        }
    }
}

/// `rule-set:a,b` → `a`, `b`.
fn collect_rule_sets(matcher: &str, out: &mut Vec<String>) {
    let Some(names) = matcher.trim().strip_prefix("rule-set:") else {
        return;
    };
    out.extend(
        names
            .split(',')
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_owned),
    );
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    fn mapping(yaml: &str) -> Mapping {
        serde_yaml_ng::from_str(yaml).expect("yaml")
    }

    #[test]
    fn a_page_keeps_only_what_differs_from_the_subscription() {
        let base = Base::of(&mapping(
            "dns: {enable: true, ipv6: false, nameserver: [1.1.1.1]}\nhosts: {a.test: 1.2.3.4}\n",
        ));
        let page = Page::parse("dns: {enable: true, ipv6: true, nameserver: [1.1.1.1]}\nhosts: {a.test: 1.2.3.4}\n")
            .expect("page");

        let diff = page.differences_from(&base);

        assert_eq!(diff.dns, mapping("ipv6: true\n"));
        assert!(diff.hosts.is_none(), "такие же hosts — копия, а не выбор");
    }

    #[test]
    fn the_page_lays_over_the_subscription_key_by_key() {
        let mut config = mapping("dns: {enable: true, nameserver: [1.1.1.1], nameserver-policy: {'+.lan': system}}\n");
        let page = Page::parse("dns: {nameserver: [9.9.9.9]}\n").expect("page");

        page.lay_over(&mut config);

        let dns = config.get("dns").and_then(Value::as_mapping).expect("dns");
        assert_eq!(dns.get("nameserver"), Some(&Value::Sequence(vec!["9.9.9.9".into()])));
        assert_eq!(
            dns.get("enable"),
            Some(&Value::Bool(true)),
            "чужие ключи остаются подписке"
        );
        assert!(dns.contains_key("nameserver-policy"));
    }

    #[test]
    fn a_false_on_the_page_is_a_value_too() {
        let mut config = mapping("dns: {ipv6: true}\n");
        Page::parse("dns: {ipv6: false}\n").expect("page").lay_over(&mut config);
        assert_eq!(config["dns"]["ipv6"], Value::Bool(false));
    }

    #[test]
    fn the_legacy_flat_layout_is_still_read() {
        let page = Page::parse("enable: true\nhosts: {a.test: 1.1.1.1}\n").expect("page");
        assert_eq!(page.dns, mapping("enable: true\n"));
        assert_eq!(page.hosts, Some(mapping("a.test: 1.1.1.1\n")));
    }

    #[test]
    fn the_editor_sees_the_subscription_under_the_differences() {
        let base = Base::of(&mapping("dns: {enable: true, ipv6: false}\nhosts: {a.test: 1.1.1.1}\n"));
        let page = Page::parse("dns: {ipv6: true}\n").expect("page");
        let seen = base.with(&page);
        assert_eq!(seen.dns, mapping("enable: true\nipv6: true\n"));
        assert_eq!(seen.hosts, base.hosts);
    }

    #[test]
    fn references_name_groups_and_rule_sets_but_not_core_parameters() {
        let page = Page::parse(
            "dns:\n  nameserver: ['https://dns.example/dns-query#GRP-A', 'tls://1.1.1.1#GRP-B&skip-cert-verify=true', 'udp://9.9.9.9']\n  nameserver-policy:\n    'rule-set:rs-one,rs-two': 'https://dns.example/dns-query#GRP-A'\n    '+.example.test': ['system#h3=true']\n  fake-ip-filter: ['*.lan', 'rule-set:rs-three', 'geosite:private']\n",
        )
        .expect("page");

        let refs = page.references();

        assert_eq!(refs.proxies, vec!["GRP-A".to_owned(), "GRP-B".to_owned()]);
        assert_eq!(
            refs.rule_sets,
            vec!["rs-one".to_owned(), "rs-three".to_owned(), "rs-two".to_owned()]
        );
    }

    #[test]
    fn keys_pointing_at_a_lost_rule_set_go_back_to_the_subscription() {
        let mut page = Page::parse(
            "dns:\n  ipv6: true\n  nameserver-policy: {'rule-set:rs-old': system, '+.lan': system}\n  fake-ip-filter: ['*.lan', 'rule-set:rs-kept']\n",
        )
        .expect("page");
        let dropped = page.drop_keys_referring_to_missing_rule_sets(|name| name == "rs-kept");
        assert_eq!(dropped, vec!["dns.nameserver-policy".to_owned()]);
        assert!(!page.dns.contains_key("nameserver-policy"));
        assert!(
            page.dns.contains_key("fake-ip-filter"),
            "набор объявлен — ключ на месте"
        );
        assert!(page.dns.contains_key("ipv6"));
    }

    #[test]
    fn a_legacy_page_is_told_apart_and_keeps_its_comments_in_the_header() {
        assert!(is_a_diff_page(&format!("{PAGE_HEADER}\n\ndns: {{}}\n")));
        assert!(!is_a_diff_page("# Clash Verge DNS Config\n\ndns: {enable: true}\n"));
        let header = header_keeping_the_comments("# Clash Verge DNS Config\n\ndns:\n  # my note\n  ipv6: true\n");
        assert_eq!(header, format!("{PAGE_HEADER}\n# my note"));
    }

    #[test]
    fn an_empty_page_changes_nothing() {
        let mut config = mapping("dns: {enable: true}\nhosts: {a.test: 1.1.1.1}\n");
        let before = config.clone();
        Page::default().lay_over(&mut config);
        assert_eq!(config, before);
        assert!(Page::default().is_empty());
    }
}
