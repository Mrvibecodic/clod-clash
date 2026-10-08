use super::CmdResult;
use crate::{cmd::StringifyErr as _, config::Config, core::CoreManager, utils::yaml_emitter};
use anyhow::{Context as _, anyhow};
use clash_verge_logging::{Type, logging};
use serde_yaml_ng::Mapping;
use smartstring::alias::String;
use std::collections::HashMap;

#[tauri::command]
pub async fn get_runtime_config() -> CmdResult<Option<Mapping>> {
    Ok(Config::runtime().await.data_arc().config.clone())
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
pub async fn get_runtime_proxy_chain_config(proxy_chain_exit_node: String) -> CmdResult<String> {
    let runtime = Config::runtime().await;
    let runtime = runtime.data_arc();

    let config = runtime
        .config
        .as_ref()
        .ok_or_else(|| anyhow!("failed to parse config to yaml file"))
        .stringify_err()?;

    if let Some(serde_yaml_ng::Value::Sequence(proxies)) = config.get("proxies") {
        let mut proxy_name = Some(Some(proxy_chain_exit_node.as_str()));
        let mut proxies_chain = Vec::new();

        while let Some(proxy) = proxies.iter().find(|proxy| {
            if let serde_yaml_ng::Value::Mapping(proxy_map) = proxy {
                proxy_map.get("name").map(|x| x.as_str()) == proxy_name && proxy_map.get("dialer-proxy").is_some()
            } else {
                false
            }
        }) {
            proxies_chain.push(proxy.to_owned());
            proxy_name = proxy.get("dialer-proxy").map(|x| x.as_str());
        }

        if let Some(entry_proxy) = proxies
            .iter()
            .find(|proxy| proxy.get("name").map(|x| x.as_str()) == proxy_name)
            && !proxies_chain.is_empty()
        {
            proxies_chain.push(entry_proxy.to_owned());
        }

        proxies_chain.reverse();

        let mut config: HashMap<String, Vec<serde_yaml_ng::Value>> = HashMap::new();

        config.insert("proxies".into(), proxies_chain);

        yaml_emitter::to_mihomo_config_string(&config)
            .context("YAML generation failed")
            .map(|s| s.into())
            .stringify_err()
    } else {
        Err("failed to get proxies or proxy-groups".into())
    }
}

#[tauri::command]
pub async fn update_proxy_chain_config_in_runtime(proxy_chain_config: Option<serde_yaml_ng::Value>) -> CmdResult<()> {
    match CoreManager::global()
        .update_runtime_config(|d| d.update_proxy_chain_config(proxy_chain_config))
        .await
    {
        Ok(Ok(_)) => {}
        Ok(Err(outcome)) => logging!(
            warn,
            Type::Core,
            "Failed to apply runtime proxy chain config: {}",
            outcome
        ),
        Err(err) => logging!(error, Type::Core, "Failed to apply runtime proxy chain config: {}", err),
    }

    Ok(())
}
