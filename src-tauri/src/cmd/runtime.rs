use super::CmdResult;
use crate::{cmd::StringifyErr as _, config::Config, feat, utils::yaml_emitter};
use anyhow::{Context as _, anyhow};
use serde_yaml_ng::Mapping;
use smartstring::alias::String;

#[tauri::command]
pub async fn get_runtime_config() -> CmdResult<Option<Mapping>> {
    Ok(Config::runtime().await.data_arc().config.clone())
}

/// Ключи, которые окну нужны только целиком в режиме цепочки: у большой
/// подписки это мегабайты правил и узлов на каждое открытие экрана.
const OVERVIEW_SKIPS: [&str; 3] = ["rules", "sub-rules", "proxies"];

fn overview(config: &Mapping) -> Mapping {
    config
        .iter()
        .filter(|(key, _)| key.as_str().is_none_or(|key| !OVERVIEW_SKIPS.contains(&key)))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

/// Срез рабочего конфига для окна — без правил и тел узлов.
#[tauri::command]
pub async fn get_runtime_overview() -> CmdResult<Option<Mapping>> {
    Ok(Config::runtime().await.data_arc().config.as_ref().map(overview))
}

/// Имя из YAML: строка, а число — как ядро его прочтёт, строкой.
fn yaml_name(value: &serde_yaml_ng::Value) -> Option<String> {
    match value {
        serde_yaml_ng::Value::String(name) => Some(name.as_str().into()),
        serde_yaml_ng::Value::Number(name) => Some(name.to_string().into()),
        _ => None,
    }
}

/// Порядок групп (`proxy-groups`) принятой сборки.
pub(crate) async fn runtime_proxy_group_order() -> Vec<String> {
    let runtime = Config::runtime().await;
    let runtime = runtime.data_arc();

    runtime
        .config
        .as_ref()
        .and_then(|config| config.get("proxy-groups"))
        .and_then(|groups| groups.as_sequence())
        .map(|groups| {
            groups
                .iter()
                .filter_map(|group| group.get("name"))
                .filter_map(yaml_name)
                .collect()
        })
        .unwrap_or_default()
}

/// Имена провайдеров узлов (`proxy-providers`) принятой сборки — то, что ядро
/// отдаёт по `/providers/proxies/{имя}`.
pub(crate) async fn runtime_proxy_provider_names() -> Vec<String> {
    let runtime = Config::runtime().await;
    let runtime = runtime.data_arc();

    runtime
        .config
        .as_ref()
        .and_then(|config| config.get("proxy-providers"))
        .and_then(|providers| providers.as_mapping())
        .map(|providers| providers.keys().filter_map(yaml_name).collect())
        .unwrap_or_default()
}

/// Подписи узлов принятой сборки и их отпечаток. Сами подписи — только если
/// отпечаток не тот, что уже есть у окна.
#[derive(serde::Serialize)]
pub(crate) struct ProxyLabelsAnswer {
    pub(crate) stamp: std::string::String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) labels: Option<crate::config::proxy_label::Labels>,
}

#[tauri::command]
pub async fn get_runtime_yaml() -> CmdResult<String> {
    let runtime = Config::runtime().await;
    let runtime = runtime.data_arc();

    let config = runtime.config.as_ref();
    config
        .ok_or_else(|| anyhow!("failed to parse config to yaml file"))
        .and_then(|config| {
            yaml_emitter::to_mihomo_config_string(config)
                .context("failed to convert config to yaml")
                .map(|s| s.into())
        })
        .stringify_err()
}

#[tauri::command]
pub async fn get_proxy_chain() -> CmdResult<Option<feat::ProxyChainView>> {
    Ok(feat::proxy_chain_view().await)
}

#[tauri::command]
pub async fn connect_proxy_chain(group: String, nodes: Vec<String>) -> CmdResult {
    feat::connect_proxy_chain(group, nodes).await.stringify_err()
}

#[tauri::command]
pub async fn disconnect_proxy_chain() -> CmdResult {
    feat::disconnect_proxy_chain().await.stringify_err()
}

#[tauri::command]
pub async fn adopt_proxy_chains(chains: Vec<feat::AdoptedChain>) -> CmdResult {
    feat::adopt_proxy_chains(chains).await.stringify_err()
}

#[cfg(test)]
mod tests {
    use super::overview;
    use serde_yaml_ng::{Mapping, Value};

    #[test]
    fn overview_drops_rules_and_node_bodies_only() {
        let mut config = Mapping::new();
        for key in ["mode", "rules", "sub-rules", "proxies", "proxy-groups", "tunnels"] {
            config.insert(Value::from(key), Value::from(key));
        }
        config.insert(Value::from(7890), Value::from("number key"));

        let slice = overview(&config);

        for key in ["rules", "sub-rules", "proxies"] {
            assert!(!slice.contains_key(key), "{key}");
        }
        for key in ["mode", "proxy-groups", "tunnels"] {
            assert_eq!(slice.get(key), config.get(key), "{key}");
        }
        assert!(slice.contains_key(Value::from(7890)));
    }
}
