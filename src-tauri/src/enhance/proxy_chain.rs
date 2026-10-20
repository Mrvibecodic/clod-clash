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

pub fn missing<'a>(config: &Mapping, nodes: &'a [String]) -> Option<&'a String> {
    let proxies = match config.get("proxies") {
        Some(Value::Sequence(proxies)) => proxies.as_slice(),
        _ => &[],
    };
    nodes
        .iter()
        .skip(1)
        .find(|node| !named(proxies, node).is_some_and(Value::is_mapping))
}

pub fn fits(config: &Mapping, nodes: &[String]) -> bool {
    nodes.len() >= 2 && missing(config, nodes).is_none()
}

/// Ложится целиком или не ложится вовсе: половина цепочки пустила бы трафик
/// мимо входного узла.
pub fn apply(config: &mut Mapping, nodes: &[String]) -> bool {
    if !fits(config, nodes) {
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
    use super::{apply, fits};
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
        assert!(apply(&mut config, &nodes(&["A", "B", "C"])));
        assert_eq!(dialer(&config, "A"), None);
        assert_eq!(dialer(&config, "B").as_deref(), Some("A"));
        assert_eq!(dialer(&config, "C").as_deref(), Some("B"));
    }

    #[test]
    fn the_links_of_the_subscription_itself_stay() {
        let mut config = config();
        assert!(apply(&mut config, &nodes(&["B", "C"])));
        assert_eq!(dialer(&config, "X").as_deref(), Some("A"));
    }

    #[test]
    fn a_chain_with_a_node_gone_from_the_subscription_is_not_laid_at_all() {
        let mut config = config();
        let before = config.clone();
        assert!(!fits(&config, &nodes(&["A", "gone", "C"])));
        assert!(!apply(&mut config, &nodes(&["A", "gone", "C"])));
        assert_eq!(config, before);
        assert!(
            !apply(&mut config, &nodes(&["A", "Proxy"])),
            "группе связь не поставить"
        );
        assert!(!apply(&mut config, &nodes(&["A"])));
    }

    #[test]
    fn the_entry_may_be_a_node_the_config_does_not_list() {
        let mut config = config();
        assert!(apply(&mut config, &nodes(&["from-a-provider", "B"])));
        assert_eq!(dialer(&config, "B").as_deref(), Some("from-a-provider"));
    }
}
