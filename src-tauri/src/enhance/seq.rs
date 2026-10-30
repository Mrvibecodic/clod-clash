use serde::{Deserialize, Serialize};
use serde_yaml_ng::{Mapping, Sequence, Value};
use std::collections::HashSet;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SeqMap {
    pub prepend: Sequence,
    pub append: Sequence,
    pub delete: Vec<String>,
}

fn collect_proxy_names(seq: &Sequence) -> Vec<String> {
    seq.iter()
        .filter_map(|item| match item {
            Value::Mapping(map) => map.get("name").and_then(Value::as_str).map(str::to_owned),
            Value::String(name) => Some(name.to_owned()),
            _ => None,
        })
        .collect()
}

fn is_selector_group(group_map: &Mapping) -> bool {
    group_map
        .get("type")
        .and_then(Value::as_str)
        .map(|value| {
            let value = value.to_ascii_lowercase();
            value == "select" || value == "selector"
        })
        .unwrap_or(false)
}

pub fn use_seq(seq: SeqMap, mut config: Mapping, field: &str) -> Mapping {
    // Пустое звено над готовым списком правил или групп не меняет в нём
    // ничего — список не пересобирается, а ключ встаёт в конец, как и после
    // прохода ниже. Нет поля или там не список — проход ставит пустой список;
    // у узлов он ещё пересобирает группы.
    if field != "proxies"
        && seq.prepend.is_empty()
        && seq.append.is_empty()
        && seq.delete.is_empty()
        && matches!(config.get(field), Some(Value::Sequence(_)))
    {
        if let Some(list) = config.remove(field) {
            config.insert(Value::String(field.into()), list);
        }
        return config;
    }
    let SeqMap {
        prepend,
        append,
        delete,
    } = seq;

    let added_proxy_names = if field == "proxies" {
        let mut names = collect_proxy_names(&prepend);
        names.extend(collect_proxy_names(&append));
        let mut seen = HashSet::new();
        names
            .into_iter()
            .filter(|name| seen.insert(name.clone()))
            .collect::<Vec<String>>()
    } else {
        Vec::new()
    };

    let mut updated_items = Sequence::new();
    updated_items.extend(prepend);

    if let Some(Value::Sequence(existing_items)) = config.remove(field) {
        // Filter out deleted items
        let kept_items: Sequence = existing_items
            .into_iter()
            .filter(|item| {
                if let Value::String(s) = item {
                    !delete.contains(s)
                } else if let Value::Mapping(m) = item {
                    if let Some(Value::String(name)) = m.get("name") {
                        !delete.contains(name)
                    } else {
                        true
                    }
                } else {
                    true
                }
            })
            .collect();
        updated_items.extend(kept_items);
    }

    updated_items.extend(append);
    config.insert(Value::String(field.into()), Value::Sequence(updated_items));

    if field != "proxies" {
        return config;
    }

    let Some(proxy_groups_value) = config.remove("proxy-groups") else {
        return config;
    };

    let Value::Sequence(proxy_groups) = proxy_groups_value else {
        config.insert(Value::String("proxy-groups".into()), proxy_groups_value);
        return config;
    };

    let mut updated_groups = Sequence::new();
    let mut appended_to_selector = false;
    for group in proxy_groups {
        if let Value::Mapping(mut group_map) = group {
            let mut group_proxies = match group_map.remove("proxies") {
                Some(Value::Sequence(proxies)) => Some(
                    proxies
                        .into_iter()
                        .filter(|p| {
                            if let Value::String(name) = p {
                                !delete.contains(name)
                            } else {
                                true
                            }
                        })
                        .collect::<Sequence>(),
                ),
                Some(value) => {
                    group_map.insert(Value::String("proxies".into()), value);
                    None
                }
                None => None,
            };

            if !appended_to_selector && !added_proxy_names.is_empty() && is_selector_group(&group_map) {
                let base_group_proxies = group_proxies.unwrap_or_else(Sequence::new);
                let mut merged_proxies = Sequence::new();
                let mut seen_proxy_names = HashSet::new();
                for name in &added_proxy_names {
                    if seen_proxy_names.insert(name.clone()) {
                        merged_proxies.push(Value::String(name.clone()));
                    }
                }
                for value in base_group_proxies {
                    if let Value::String(name) = &value
                        && !seen_proxy_names.insert(name.to_owned())
                    {
                        continue;
                    }

                    merged_proxies.push(value);
                }
                group_proxies = Some(merged_proxies);
                appended_to_selector = true;
            }

            if let Some(group_proxies) = group_proxies {
                group_map.insert(Value::String("proxies".into()), Value::Sequence(group_proxies));
            }
            updated_groups.push(Value::Mapping(group_map));
        } else {
            updated_groups.push(group);
        }
    }
    config.insert(Value::String("proxy-groups".into()), Value::Sequence(updated_groups));

    config
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::unwrap_used)]
    #[allow(clippy::expect_used)]
    fn test_delete_proxy_and_references() {
        let config_str = r#"
proxies:
- name: "proxy1"
  type: "ss"
- name: "proxy2"
  type: "vmess"
proxy-groups:
- name: "group1"
  type: "select"
  proxies:
    - "proxy1"
    - "proxy2"
- name: "group2"
  type: "select"
  proxies:
    - "proxy1"
"#;
        let mut config: Mapping = serde_yaml_ng::from_str(config_str).expect("Failed to parse test config YAML");

        let seq = SeqMap {
            prepend: Sequence::new(),
            append: Sequence::new(),
            delete: vec!["proxy1".to_string()],
        };

        config = use_seq(seq, config, "proxies");

        // Check if proxy1 is removed from proxies
        let proxies = config
            .get("proxies")
            .expect("proxies field should exist")
            .as_sequence()
            .expect("proxies should be a sequence");
        assert_eq!(proxies.len(), 1);
        assert_eq!(
            proxies[0]
                .as_mapping()
                .expect("proxy should be a mapping")
                .get("name")
                .expect("proxy should have name")
                .as_str()
                .expect("name should be string"),
            "proxy2"
        );

        // Check if proxy1 is removed from all groups
        let groups = config
            .get("proxy-groups")
            .expect("proxy-groups field should exist")
            .as_sequence()
            .expect("proxy-groups should be a sequence");
        let group1_proxies = groups[0]
            .as_mapping()
            .expect("group should be a mapping")
            .get("proxies")
            .expect("group should have proxies")
            .as_sequence()
            .expect("group proxies should be a sequence");
        let group2_proxies = groups[1]
            .as_mapping()
            .expect("group should be a mapping")
            .get("proxies")
            .expect("group should have proxies")
            .as_sequence()
            .expect("group proxies should be a sequence");

        assert_eq!(group1_proxies.len(), 1);
        assert_eq!(
            group1_proxies[0].as_str().expect("proxy name should be string"),
            "proxy2"
        );
        assert_eq!(group2_proxies.len(), 0);
    }

    #[test]
    #[allow(clippy::unwrap_used)]
    #[allow(clippy::expect_used)]
    fn test_add_new_proxies_to_first_selector_group() {
        let config_str = r#"
proxies:
- name: "proxy1"
  type: "ss"
proxy-groups:
- name: "group1"
  type: "select"
  proxies:
    - "proxy1"
- name: "group2"
  type: "select"
  proxies:
    - "proxy1"
"#;
        let mut config: Mapping = serde_yaml_ng::from_str(config_str).expect("Failed to parse test config YAML");

        let prepend: Sequence = serde_yaml_ng::from_str(
            r#"
- name: "proxy3"
  type: "ss"
"#,
        )
        .expect("Failed to parse prepend proxies");

        let append: Sequence = serde_yaml_ng::from_str(
            r#"
- name: "proxy4"
  type: "vmess"
"#,
        )
        .expect("Failed to parse append proxies");

        let seq = SeqMap {
            prepend,
            append,
            delete: vec![],
        };

        config = use_seq(seq, config, "proxies");

        let groups = config
            .get("proxy-groups")
            .expect("proxy-groups field should exist")
            .as_sequence()
            .expect("proxy-groups should be a sequence");
        let group1_proxies = groups[0]
            .as_mapping()
            .expect("group should be a mapping")
            .get("proxies")
            .expect("group should have proxies")
            .as_sequence()
            .expect("group proxies should be a sequence");
        let names: Vec<&str> = group1_proxies.iter().filter_map(Value::as_str).collect();
        assert_eq!(names, vec!["proxy3", "proxy4", "proxy1"]);

        let group2_proxies = groups[1]
            .as_mapping()
            .expect("group should be a mapping")
            .get("proxies")
            .expect("group should have proxies")
            .as_sequence()
            .expect("group proxies should be a sequence");
        let names: Vec<&str> = group2_proxies.iter().filter_map(Value::as_str).collect();
        assert_eq!(names, vec!["proxy1"]);
    }

    #[test]
    #[allow(clippy::unwrap_used)]
    #[allow(clippy::expect_used)]
    fn test_preserve_non_sequence_proxy_groups() {
        let config_str = r#"
proxies:
- name: "proxy1"
  type: "ss"
proxy-groups: "invalid"
"#;
        let mut config: Mapping = serde_yaml_ng::from_str(config_str).expect("Failed to parse test config YAML");

        let seq = SeqMap {
            prepend: Sequence::new(),
            append: Sequence::new(),
            delete: vec!["proxy1".to_string()],
        };

        config = use_seq(seq, config, "proxies");

        assert_eq!(config.get("proxy-groups").and_then(Value::as_str), Some("invalid"));
    }

    #[test]
    fn an_empty_link_still_leaves_a_list_in_place() {
        let parsed: Result<Mapping, _> = serde_yaml_ng::from_str("{rules: ~, proxies: [a], mode: rule}");
        assert!(parsed.is_ok());
        let config = parsed.unwrap_or_default();

        let config = use_seq(SeqMap::default(), config, "rules");
        assert_eq!(
            config.get("rules"),
            Some(&Value::Sequence(Sequence::new())),
            "null → []"
        );

        let config = use_seq(SeqMap::default(), config, "proxy-groups");
        assert_eq!(
            config.get("proxy-groups"),
            Some(&Value::Sequence(Sequence::new())),
            "нет поля → []"
        );

        let config = use_seq(SeqMap::default(), config, "proxies");
        assert_eq!(
            config.get("proxies"),
            Some(&Value::Sequence(vec![Value::from("a")])),
            "готовый список — как был"
        );
        assert_eq!(config.get("mode"), Some(&Value::from("rule")));
    }

    /// Порядок ключей виден в окне текущей конфигурации и скрипту подписки:
    /// пустое звено ставит его так же, как полный проход.
    #[test]
    fn an_empty_link_orders_the_keys_as_a_full_pass_does() {
        let parsed: Result<Mapping, _> =
            serde_yaml_ng::from_str("{rules: [r], proxy-groups: [{name: g, proxies: [a], type: select}], sniffer: {}}");
        assert!(parsed.is_ok());
        let config = parsed.unwrap_or_default();
        let keys = |config: &Mapping| {
            config
                .keys()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect::<Vec<_>>()
        };

        let full = SeqMap {
            delete: vec!["absent".to_owned()],
            ..SeqMap::default()
        };
        for field in ["rules", "proxy-groups"] {
            let empty = use_seq(SeqMap::default(), config.clone(), field);
            let passed = use_seq(full.clone(), config.clone(), field);
            assert_eq!(keys(&empty), keys(&passed), "{field}");
            assert_eq!(empty, passed, "{field}");
        }
    }
}
