use clash_verge_logging::{Type, logging};

use super::use_lowercase;
use serde_yaml_ng::{self, Mapping, Value};

fn deep_merge(a: &mut Value, b: Value) {
    match (a, b) {
        (Value::Mapping(a_map), Value::Mapping(b_map)) => {
            for (key, value) in b_map {
                if let Some(existing) = a_map.get_mut(&key) {
                    deep_merge(existing, value);
                } else {
                    a_map.insert(key, value);
                }
            }
        }
        (a, b) => *a = b,
    }
}

pub fn use_merge(merge: &Mapping, config: Mapping) -> Mapping {
    let mut config = Value::from(config);
    let merge = use_lowercase(merge);

    deep_merge(&mut config, Value::from(merge));

    config.as_mapping().cloned().unwrap_or_else(|| {
        logging!(
            error,
            Type::Core,
            "Failed to convert merged config to mapping, using empty mapping"
        );
        Mapping::new()
    })
}

#[cfg(test)]
mod tests {
    use super::use_merge;
    use serde_yaml_ng::Mapping;

    fn mapping(yaml: &str) -> Mapping {
        #[allow(clippy::expect_used)]
        serde_yaml_ng::from_str(yaml).expect("test yaml should parse")
    }

    #[test]
    fn merge_replaces_scalars_and_sequences_but_descends_into_mappings() {
        let merged = use_merge(
            &mapping("{mode: global, rules: [replace], tun: {enable: true}}"),
            mapping("{mode: rule, rules: [old, other], tun: {stack: gvisor}, untouched: 1}"),
        );

        assert_eq!(merged.get("mode"), Some(&serde_yaml_ng::Value::from("global")));
        assert_eq!(merged.get("untouched"), Some(&serde_yaml_ng::Value::from(1)));

        let rules = merged.get("rules").and_then(serde_yaml_ng::Value::as_sequence);
        assert_eq!(
            rules.map(Vec::len),
            Some(1),
            "sequences are replaced whole, not concatenated"
        );

        let tun = merged.get("tun").and_then(serde_yaml_ng::Value::as_mapping);
        assert_eq!(
            tun.and_then(|tun| tun.get("stack")),
            Some(&serde_yaml_ng::Value::from("gvisor")),
            "keys the merge does not mention survive"
        );
        assert_eq!(
            tun.and_then(|tun| tun.get("enable")),
            Some(&serde_yaml_ng::Value::from(true)),
        );
    }

    #[test]
    fn merge_keys_are_lowercased_before_they_are_applied() {
        let merged = use_merge(&mapping("{MODE: global}"), mapping("{mode: rule}"));

        assert_eq!(merged.get("mode"), Some(&serde_yaml_ng::Value::from("global")));
        assert!(!merged.contains_key("MODE"));
    }
}
