use serde_yaml_ng::{Mapping, Value};
use smartstring::alias::String;

fn named<'a>(proxies: &'a [Value], name: &str) -> Option<&'a Value> {
    proxies
        .iter()
        .find(|proxy| proxy.get("name").and_then(Value::as_str) == Some(name))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dropped {
    pub nodes: Vec<String>,
    pub missing: String,
}

fn listed<'a>(config: &'a Mapping, key: &str) -> &'a [Value] {
    match config.get(key) {
        Some(Value::Sequence(list)) => list.as_slice(),
        _ => &[],
    }
}

pub fn is_own(config: &Mapping, name: &str) -> bool {
    named(listed(config, "proxies"), name).is_some_and(Value::is_mapping)
        || named(listed(config, "proxy-groups"), name).is_some()
}

/// Вход из набора узлов (proxy-providers) в сборке не виден, поэтому вход
/// проверяется, только если при подключении он был узлом самой подписки.
pub fn missing<'a>(config: &Mapping, nodes: &'a [String], own_entry: bool) -> Option<&'a String> {
    let proxies = listed(config, "proxies");
    nodes
        .first()
        .filter(|entry| own_entry && !is_own(config, entry))
        .or_else(|| {
            nodes
                .iter()
                .skip(1)
                .find(|node| !named(proxies, node).is_some_and(Value::is_mapping))
        })
}

pub fn fits(config: &Mapping, nodes: &[String], own_entry: bool) -> bool {
    nodes.len() >= 2 && missing(config, nodes, own_entry).is_none()
}

/// Ложится целиком или не ложится вовсе: половина цепочки пустила бы трафик
/// мимо входного узла.
pub fn apply(config: &mut Mapping, nodes: &[String], own_entry: bool) -> bool {
    if !fits(config, nodes, own_entry) {
        return false;
    }
    let Some(Value::Sequence(proxies)) = config.get_mut("proxies") else {
        return false;
    };
    for pair in nodes.windows(2) {
        let [dialer, node] = pair else {
            continue;
        };
        if let Some(Value::Mapping(proxy)) = proxies
            .iter_mut()
            .find(|proxy| proxy.get("name").and_then(Value::as_str) == Some(node.as_str()))
        {
            proxy.insert("dialer-proxy".into(), Value::from(dialer.as_str()));
        }
    }
    true
}

#[allow(clippy::expect_used)]
#[cfg(test)]
mod tests {
    use super::{apply, fits, missing};
    use serde_yaml_ng::{Mapping, Value};
    use smartstring::alias::String;

    fn config() -> Mapping {
        serde_yaml_ng::from_str(
            "proxies:
  - {name: A, type: ss}
  - {name: B, type: ss}
  - {name: C, type: ss}
  - {name: X, type: ss, dialer-proxy: A}
proxy-groups:
  - {name: Proxy, type: select, proxies: [A, B, C, X]}",
        )
        .expect("test config should be valid")
    }

    fn nodes(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).into()).collect()
    }

    fn dialer(config: &Mapping, name: &str) -> Option<std::string::String> {
        let Some(Value::Sequence(proxies)) = config.get("proxies") else {
            return None;
        };
        proxies
            .iter()
            .find(|proxy| proxy.get("name").and_then(Value::as_str) == Some(name))
            .and_then(|proxy| proxy.get("dialer-proxy"))
            .and_then(Value::as_str)
            .map(Into::into)
    }

    #[test]
    fn each_node_of_the_chain_goes_out_through_the_one_before() {
        let mut config = config();
        assert!(apply(&mut config, &nodes(&["A", "B", "C"]), false));
        assert_eq!(dialer(&config, "A"), None);
        assert_eq!(dialer(&config, "B").as_deref(), Some("A"));
        assert_eq!(dialer(&config, "C").as_deref(), Some("B"));
    }

    #[test]
    fn the_links_of_the_subscription_itself_stay() {
        let mut config = config();
        assert!(apply(&mut config, &nodes(&["B", "C"]), false));
        assert_eq!(dialer(&config, "X").as_deref(), Some("A"));
    }

    #[test]
    fn a_chain_with_a_node_gone_from_the_subscription_is_not_laid_at_all() {
        let mut config = config();
        let before = config.clone();
        assert!(!fits(&config, &nodes(&["A", "gone", "C"]), false));
        assert!(!apply(&mut config, &nodes(&["A", "gone", "C"]), false));
        assert_eq!(config, before);
        assert!(
            !apply(&mut config, &nodes(&["A", "Proxy"]), false),
            "группе связь не поставить"
        );
        assert!(!apply(&mut config, &nodes(&["A"]), false));
    }

    #[test]
    fn an_own_entry_gone_from_the_subscription_is_missing() {
        let config = config();
        let chain = nodes(&["gone", "B"]);
        assert_eq!(missing(&config, &chain, true).map(String::as_str), Some("gone"));
        assert!(!fits(&config, &chain, true));
        assert_eq!(missing(&config, &chain, false), None, "вход из набора узлов");
        assert_eq!(
            missing(&config, &nodes(&["Proxy", "B"]), true),
            None,
            "вход — группа подписки"
        );
    }

    #[test]
    fn the_entry_may_be_a_node_the_config_does_not_list() {
        let mut config = config();
        assert!(apply(&mut config, &nodes(&["from-a-provider", "B"]), false));
        assert_eq!(dialer(&config, "B").as_deref(), Some("from-a-provider"));
    }
}
