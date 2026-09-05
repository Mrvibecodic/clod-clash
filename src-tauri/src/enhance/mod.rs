mod chain;
pub mod field;
mod merge;
mod script;
pub mod seq;
mod tun;

use self::{
    chain::{AsyncChainItemFrom as _, ChainItem, ChainType},
    field::{use_keys, use_lowercase, use_sort},
    merge::use_merge,
    script::use_script,
    seq::{SeqMap, use_seq},
    tun::{ensure_dns_for_tun, use_tun},
};
use crate::utils::dirs;
use crate::{
    config::{Config, IVerge, PrfItem, runtime::IRuntime},
    constants,
    utils::tmpl,
};
use anyhow::{Context as _, Result};
use clash_verge_draft::Draft;
use clash_verge_logging::{Type, logging};
use serde_yaml_ng::{Mapping, Value};
use smartstring::alias::String;
use std::collections::{HashMap, HashSet};
use tokio::fs;

type ResultLog = Vec<(String, String)>;

#[derive(Debug, Default)]
struct TunOverrides {
    stack: Option<Value>,
    strict_route: Option<Value>,
    dns_hijack: Option<Value>,
}

fn parse_tun_overrides(stack: Option<&str>, strict_route: Option<&str>, dns_hijack: Option<&str>) -> TunOverrides {
    TunOverrides {
        stack: match stack.map(str::trim) {
            Some(value @ ("gvisor" | "system" | "mixed")) => Some(Value::from(value)),
            _ => None,
        },
        strict_route: match strict_route.map(str::trim) {
            Some("on") => Some(Value::from(true)),
            Some("off") => Some(Value::from(false)),
            _ => None,
        },
        dns_hijack: match dns_hijack.map(str::trim) {
            None | Some("auto") => None,
            Some(list) => Some(Value::Sequence(
                list.split(',')
                    .map(str::trim)
                    .filter(|entry| !entry.is_empty())
                    .map(Value::from)
                    .collect(),
            )),
        },
    }
}

fn ladder_tun(tun: &mut Mapping, app_tun: Mapping, overrides: &TunOverrides) {
    ladder_tun_on(tun, app_tun, overrides, cfg!(target_os = "windows"));
}

fn subscription_stack_is_capped(tun: &Mapping) -> bool {
    tun.get("stack")
        .and_then(Value::as_str)
        .map(str::to_ascii_lowercase)
        .is_some_and(|stack| stack == "system" || stack == "mixed")
}

fn ladder_tun_on(tun: &mut Mapping, app_tun: Mapping, overrides: &TunOverrides, cap_subscription_stack: bool) {
    if cap_subscription_stack && subscription_stack_is_capped(tun) {
        tun.remove("stack");
    }
    for (key, value) in app_tun.into_iter() {
        let deferred = matches!(key.as_str(), Some("stack" | "strict-route" | "dns-hijack"));
        if deferred && tun.contains_key(&key) {
            continue;
        }
        tun.insert(key, value);
    }
    if !tun.contains_key("stack") {
        tun.insert("stack".into(), crate::constants::tun::DEFAULT_STACK.into());
    }
    if !tun.contains_key("strict-route") {
        tun.insert("strict-route".into(), Value::from(false));
    }
    if !tun.contains_key("dns-hijack") {
        tun.insert(
            "dns-hijack".into(),
            Value::Sequence(
                crate::constants::tun::DNS_HIJACK
                    .iter()
                    .copied()
                    .map(Value::from)
                    .collect(),
            ),
        );
    }
    if let Some(stack) = overrides.stack.clone() {
        tun.insert("stack".into(), stack);
    }
    if let Some(strict_route) = overrides.strict_route.clone() {
        tun.insert("strict-route".into(), strict_route);
    }
    if let Some(dns_hijack) = overrides.dns_hijack.clone() {
        tun.insert("dns-hijack".into(), dns_hijack);
    }
}

#[derive(Debug)]
struct ConfigValues {
    clash_config: Mapping,
    clash_core: Option<String>,
    enable_tun: bool,
    enable_builtin: bool,
    socks_enabled: bool,
    http_enabled: bool,
    enable_dns_settings: bool,
    tun_overrides: TunOverrides,
    #[cfg(not(target_os = "windows"))]
    redir_enabled: bool,
    #[cfg(target_os = "linux")]
    tproxy_enabled: bool,
}

#[derive(Debug)]
struct ProfileItems {
    config: Mapping,
    merge_item: ChainItem,
    script_item: ChainItem,
    rules_item: ChainItem,
    proxies_item: ChainItem,
    groups_item: ChainItem,
    global_merge: ChainItem,
    global_script: ChainItem,
    profile_name: String,
    profile_is_remote: bool,
    profile_shows_zero_hosts: bool,
}

impl Default for ProfileItems {
    fn default() -> Self {
        Self {
            config: Default::default(),
            profile_name: Default::default(),
            profile_is_remote: false,
            profile_shows_zero_hosts: false,
            merge_item: ChainItem {
                uid: "".into(),
                data: ChainType::Merge(Mapping::new()),
            },
            script_item: ChainItem {
                uid: "".into(),
                data: ChainType::Script(tmpl::ITEM_SCRIPT.into()),
            },
            rules_item: ChainItem {
                uid: "".into(),
                data: ChainType::Rules(SeqMap::default()),
            },
            proxies_item: ChainItem {
                uid: "".into(),
                data: ChainType::Proxies(SeqMap::default()),
            },
            groups_item: ChainItem {
                uid: "".into(),
                data: ChainType::Groups(SeqMap::default()),
            },
            global_merge: ChainItem {
                uid: "Merge".into(),
                data: ChainType::Merge(Mapping::new()),
            },
            global_script: ChainItem {
                uid: "Script".into(),
                data: ChainType::Script(tmpl::ITEM_SCRIPT.into()),
            },
        }
    }
}

async fn chain_item_or_default(item: Option<&PrfItem>, default_item: impl FnOnce() -> ChainItem) -> ChainItem {
    if let Some(item) = item {
        <Option<ChainItem>>::from_async(item).await.unwrap_or_else(default_item)
    } else {
        default_item()
    }
}

async fn get_config_values() -> ConfigValues {
    let clash = Config::clash().await;
    let clash_arc = clash.latest_arc();
    let clash_config = clash_arc.0.clone();
    drop(clash_arc);
    drop(clash);

    let verge = Config::verge().await;

    let verge_arc = verge.latest_arc();
    let IVerge {
        ref enable_tun_mode,
        ref enable_builtin_enhanced,
        ref verge_socks_enabled,
        ref verge_http_enabled,
        ref enable_dns_settings,
        ref tun_stack,
        ref tun_strict_route,
        ref tun_dns_hijack,
        ..
    } = **verge_arc;

    let tun_overrides = parse_tun_overrides(
        tun_stack.as_deref(),
        tun_strict_route.as_deref(),
        tun_dns_hijack.as_deref(),
    );

    let (clash_core, enable_tun, enable_builtin, socks_enabled, http_enabled, enable_dns_settings) = (
        Some(verge_arc.get_valid_clash_core()),
        enable_tun_mode.unwrap_or(false) && !crate::feat::tun::is_suppressed(),
        enable_builtin_enhanced.unwrap_or(true),
        verge_socks_enabled.unwrap_or(false),
        verge_http_enabled.unwrap_or(false),
        enable_dns_settings.unwrap_or(false),
    );

    #[cfg(not(target_os = "windows"))]
    let redir_enabled = verge_arc.verge_redir_enabled.unwrap_or(false);

    #[cfg(target_os = "linux")]
    let tproxy_enabled = verge_arc.verge_tproxy_enabled.unwrap_or(false);

    drop(verge_arc);
    drop(verge);

    ConfigValues {
        clash_config,
        clash_core,
        enable_tun,
        enable_builtin,
        socks_enabled,
        http_enabled,
        enable_dns_settings,
        tun_overrides,
        #[cfg(not(target_os = "windows"))]
        redir_enabled,
        #[cfg(target_os = "linux")]
        tproxy_enabled,
    }
}

#[allow(clippy::cognitive_complexity)]
async fn collect_profile_items() -> Result<ProfileItems> {
    let profiles = Config::profiles().await;
    let profiles_arc = profiles.latest_arc();
    drop(profiles);

    let current_profile_uid = match profiles_arc.get_current().cloned() {
        Some(uid) => uid,
        None => {
            drop(profiles_arc);
            return Ok(ProfileItems::default());
        }
    };

    let mut current = profiles_arc
        .current_mapping()
        .await
        .with_context(|| format!("failed to read current profile \"{current_profile_uid}\""))?;

    let dropped_ui_keys = EXTERNAL_UI_KEYS
        .iter()
        .filter(|&&key| current.remove(key).is_some())
        .copied()
        .collect::<Vec<&str>>();
    if !dropped_ui_keys.is_empty() {
        logging!(
            info,
            Type::Config,
            "drop `{}` imposed by the subscription",
            dropped_ui_keys.join("`, `")
        );
    }

    let current_item = match profiles_arc.get_item(&current_profile_uid) {
        Ok(item) => item,
        Err(err) => {
            return Err(err).with_context(|| format!("failed to get current profile \"{current_profile_uid}\""));
        }
    };

    let merge_uid = current_item.current_merge().cloned().unwrap_or_else(|| "Merge".into());
    let script_uid = current_item
        .current_script()
        .cloned()
        .unwrap_or_else(|| "Script".into());
    let rules_uid = current_item.current_rules().cloned().unwrap_or_else(|| "Rules".into());
    let proxies_uid = current_item
        .current_proxies()
        .cloned()
        .unwrap_or_else(|| "Proxies".into());
    let groups_uid = current_item
        .current_groups()
        .cloned()
        .unwrap_or_else(|| "Groups".into());

    let name = current_item.name.clone().unwrap_or_default();
    let profile_is_remote = current_item.itype.as_deref() == Some("remote");
    let profile_shows_zero_hosts = profile_is_remote && current_item.show_zero_hosts.unwrap_or(false);

    let (merge_item, script_item, rules_item, proxies_item, groups_item, global_merge, global_script) = tokio::join!(
        chain_item_or_default(profiles_arc.get_item(&merge_uid).ok(), || ChainItem {
            uid: "".into(),
            data: ChainType::Merge(Mapping::new()),
        },),
        chain_item_or_default(profiles_arc.get_item(&script_uid).ok(), || ChainItem {
            uid: "".into(),
            data: ChainType::Script(tmpl::ITEM_SCRIPT.into()),
        },),
        chain_item_or_default(profiles_arc.get_item(&rules_uid).ok(), || ChainItem {
            uid: "".into(),
            data: ChainType::Rules(SeqMap::default()),
        },),
        chain_item_or_default(profiles_arc.get_item(&proxies_uid).ok(), || ChainItem {
            uid: "".into(),
            data: ChainType::Proxies(SeqMap::default()),
        },),
        chain_item_or_default(profiles_arc.get_item(&groups_uid).ok(), || ChainItem {
            uid: "".into(),
            data: ChainType::Groups(SeqMap::default()),
        },),
        chain_item_or_default(profiles_arc.get_item("Merge").ok(), || ChainItem {
            uid: "Merge".into(),
            data: ChainType::Merge(Mapping::new()),
        },),
        chain_item_or_default(profiles_arc.get_item("Script").ok(), || ChainItem {
            uid: "Script".into(),
            data: ChainType::Script(tmpl::ITEM_SCRIPT.into()),
        },),
    );

    drop(profiles_arc);

    Ok(ProfileItems {
        config: current,
        merge_item,
        script_item,
        rules_item,
        proxies_item,
        groups_item,
        global_merge,
        global_script,
        profile_name: name,
        profile_is_remote,
        profile_shows_zero_hosts,
    })
}

async fn process_global_items(
    mut config: Mapping,
    mut exists_keys: Vec<String>,
    mut result_map: HashMap<String, ResultLog>,
    global_merge: ChainItem,
    global_script: ChainItem,
    profile_name: &String,
) -> (Mapping, Vec<String>, HashMap<String, ResultLog>) {
    if let ChainType::Merge(merge) = global_merge.data {
        exists_keys.extend(use_keys(&merge));
        config = use_merge(&merge, config);
    }

    if let ChainType::Script(script) = global_script.data {
        let mut logs = vec![];
        match use_script(script, config.clone(), profile_name.clone()).await {
            Ok((res_config, res_logs)) => {
                extend_changed_keys(&mut exists_keys, &config, &res_config);
                config = res_config;
                logs.extend(res_logs);
            }
            Err(err) => logs.push(("exception".into(), err.to_string().into())),
        }
        result_map.insert(global_script.uid, logs);
    }

    (config, exists_keys, result_map)
}

fn process_seq_items(
    mut config: Mapping,
    rules_item: ChainItem,
    proxies_item: ChainItem,
    groups_item: ChainItem,
) -> Mapping {
    if let ChainType::Rules(rules) = rules_item.data {
        config = use_seq(rules, config, "rules");
    }

    if let ChainType::Proxies(proxies) = proxies_item.data {
        config = use_seq(proxies, config, "proxies");
    }

    if let ChainType::Groups(groups) = groups_item.data {
        config = use_seq(groups, config, "proxy-groups");
    }

    config
}

fn extend_changed_keys(exists_keys: &mut Vec<String>, config: &Mapping, res_config: &Mapping) {
    exists_keys.extend(res_config.iter().filter_map(|(key, value)| {
        if config.get(key) == Some(value) {
            return None;
        }

        key.as_str().map(|key| {
            let mut key: String = key.into();
            key.make_ascii_lowercase();
            key
        })
    }));
}

const EXTERNAL_UI_KEYS: &[&str] = &["external-ui", "external-ui-url", "external-ui-name"];

const CONTROL_PLANE_KEYS: &[&str] = &[
    "external-controller",
    #[cfg(unix)]
    "external-controller-unix",
    #[cfg(windows)]
    "external-controller-pipe",
    "external-controller-cors",
    "secret",
    "mixed-port",
    "socks-port",
    "port",
    #[cfg(not(target_os = "windows"))]
    "redir-port",
    #[cfg(target_os = "linux")]
    "tproxy-port",
    "mode",
    "allow-lan",
    "log-level",
    "ipv6",
    "unified-delay",
    "tun",
];

fn control_plane_keys() -> impl Iterator<Item = &'static str> {
    CONTROL_PLANE_KEYS.iter().chain(EXTERNAL_UI_KEYS.iter()).copied()
}

fn snapshot_control_plane(config: &Mapping) -> Mapping {
    let mut snapshot = Mapping::new();
    for key in control_plane_keys() {
        let key = Value::from(key);
        if let Some(value) = config.get(&key) {
            snapshot.insert(key, value.clone());
        }
    }
    snapshot
}

fn enforce_control_plane(mut config: Mapping, snapshot: Mapping) -> Mapping {
    for key in control_plane_keys() {
        let key = Value::from(key);
        if !snapshot.contains_key(&key) {
            config.remove(&key);
        }
    }
    config.extend(snapshot);
    config
}

const DNS_PAGE_KEYS: &[&str] = &["dns"];

fn snapshot_dns_page(config: &Mapping) -> Mapping {
    let mut snapshot = Mapping::new();
    for &key in DNS_PAGE_KEYS {
        let key = Value::from(key);
        if let Some(value) = config.get(&key) {
            snapshot.insert(key, value.clone());
        }
    }
    snapshot
}

fn enforce_dns_page(mut config: Mapping, snapshot: Mapping) -> Mapping {
    config.extend(snapshot);
    config
}

fn is_loopback_bind_address(addr: &str) -> bool {
    let addr = addr.trim();
    let addr = addr
        .strip_prefix('[')
        .and_then(|addr| addr.strip_suffix(']'))
        .unwrap_or(addr);

    addr.eq_ignore_ascii_case("localhost")
        || addr.parse::<std::net::IpAddr>().is_ok_and(|addr| addr.is_loopback())
        || is_ipv4_shorthand_loopback(addr)
}

fn is_ipv4_shorthand_loopback(addr: &str) -> bool {
    let parts = addr.split('.').map(str::parse::<u32>).collect::<Result<Vec<_>, _>>();

    let Ok(parts) = parts else {
        return false;
    };

    match parts.as_slice() {
        [first, rest] => *first == 127 && *rest <= 0x00ff_ffff,
        [first, second, rest] => *first == 127 && *second <= 0xff && *rest <= 0xffff,
        [first, second, third, fourth] => *first == 127 && *second <= 0xff && *third <= 0xff && *fourth <= 0xff,
        _ => false,
    }
}

fn ensure_lan_bind_address(mut config: Mapping) -> Mapping {
    let allow_lan = config.get("allow-lan").and_then(Value::as_bool).unwrap_or(false);

    if allow_lan
        && config
            .get("bind-address")
            .and_then(Value::as_str)
            .is_some_and(is_loopback_bind_address)
    {
        config.insert(Value::from("bind-address"), Value::from("*"));
    }

    config
}

fn ensure_store_selected(mut config: Mapping) -> Mapping {
    let key = Value::from("profile");
    match config.get_mut(&key) {
        Some(Value::Mapping(profile)) => {
            profile.insert(Value::from("store-selected"), Value::from(true));
            profile.insert(Value::from("store-fake-ip"), Value::from(true));
        }
        _ => {
            let mut profile = Mapping::new();
            profile.insert(Value::from("store-selected"), Value::from(true));
            profile.insert(Value::from("store-fake-ip"), Value::from(true));
            config.insert(key, Value::from(profile));
        }
    }
    config
}

async fn process_profile_items(
    mut config: Mapping,
    mut exists_keys: Vec<String>,
    mut result_map: HashMap<String, ResultLog>,
    merge_item: ChainItem,
    script_item: ChainItem,
    profile_name: &String,
) -> (Mapping, Vec<String>, HashMap<String, ResultLog>) {
    if let ChainType::Merge(merge) = merge_item.data {
        exists_keys.extend(use_keys(&merge));
        config = use_merge(&merge, config);
    }

    if let ChainType::Script(script) = script_item.data {
        let mut logs = vec![];
        match use_script(script, config.clone(), profile_name.clone()).await {
            Ok((res_config, res_logs)) => {
                extend_changed_keys(&mut exists_keys, &config, &res_config);
                config = res_config;
                logs.extend(res_logs);
            }
            Err(err) => logs.push(("exception".into(), err.to_string().into())),
        }
        result_map.insert(script_item.uid, logs);
    }

    (config, exists_keys, result_map)
}

const SUBSCRIPTION_DECIDES: &[&str] = &["ipv6"];

const LADDER_DEFAULTS: &[(&str, &str)] = &[("log-level", "info"), ("unified-delay", "true")];

fn fill_the_ladder_defaults(config: &mut Mapping) {
    for (key, default) in LADDER_DEFAULTS {
        if config.contains_key(*key) {
            continue;
        }
        let value = match *default {
            "true" => Value::Bool(true),
            other => Value::String(other.to_owned()),
        };
        config.insert((*key).into(), value);
    }
}

fn subscription_or_app(subscription: &Mapping, key: &str, app_value: &Value) -> Value {
    subscription.get(key).cloned().unwrap_or_else(|| app_value.clone())
}

async fn merge_default_config(
    mut config: Mapping,
    clash_config: Mapping,
    socks_enabled: bool,
    http_enabled: bool,
    tun_overrides: &TunOverrides,
    #[cfg(not(target_os = "windows"))] redir_enabled: bool,
    #[cfg(target_os = "linux")] tproxy_enabled: bool,
) -> Mapping {
    for (key, value) in clash_config.into_iter() {
        if key.as_str() == Some("tun") {
            let mut tun = config.get_mut("tun").map_or_else(Mapping::new, |val| {
                val.as_mapping().cloned().unwrap_or_else(Mapping::new)
            });
            let patch_tun = value.as_mapping().cloned().unwrap_or_else(Mapping::new);
            ladder_tun(&mut tun, patch_tun, tun_overrides);
            config.insert("tun".into(), tun.into());
        } else {
            if let Some(name) = key.as_str().filter(|name| SUBSCRIPTION_DECIDES.contains(name)) {
                let decided = subscription_or_app(&config, name, &value);
                config.insert(key, decided);
                continue;
            }
            if key.as_str() == Some("socks-port") && !socks_enabled {
                config.remove("socks-port");
                continue;
            }
            if key.as_str() == Some("port") && !http_enabled {
                config.remove("port");
                continue;
            }
            #[cfg(target_os = "windows")]
            {
                if key.as_str() == Some("redir-port") {
                    continue;
                }
            }
            #[cfg(not(target_os = "windows"))]
            {
                if key.as_str() == Some("redir-port") && !redir_enabled {
                    config.remove("redir-port");
                    continue;
                }
            }
            #[cfg(target_os = "linux")]
            {
                if key.as_str() == Some("tproxy-port") && !tproxy_enabled {
                    config.remove("tproxy-port");
                    continue;
                }
            }
            #[cfg(not(target_os = "linux"))]
            {
                if key.as_str() == Some("tproxy-port") {
                    config.remove("tproxy-port");
                    continue;
                }
            }
            if key.as_str() == Some("external-controller") {
                let enable_external_controller = Config::verge()
                    .await
                    .latest_arc()
                    .enable_external_controller
                    .unwrap_or(false);

                if enable_external_controller {
                    config.insert(key, value);
                } else {
                    config.insert(key, "".into());
                }
            } else {
                config.insert(key, value);
            }
        }
    }

    fill_the_ladder_defaults(&mut config);
    config
}

async fn apply_builtin_scripts(mut config: Mapping, clash_core: Option<String>, enable_builtin: bool) -> Mapping {
    if enable_builtin {
        let items: Vec<_> = ChainItem::builtin()
            .into_iter()
            .filter(|(s, _)| s.is_support(clash_core.as_ref()))
            .map(|(_, c)| c)
            .collect();
        for item in items {
            logging!(debug, Type::Core, "run builtin script {}", item.uid);
            if let ChainType::Script(script) = item.data {
                match use_script(script, config.clone(), String::from("")).await {
                    Ok((res_config, _)) => {
                        config = res_config;
                    }
                    Err(err) => {
                        logging!(error, Type::Core, "builtin script error `{err}`");
                    }
                }
            }
        }
    }

    config
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct SentinelReport {
    pub remarks: Vec<String>,
    pub only_sentinels: bool,
    pub partially_dropped: bool,
    pub dropped_total: usize,
}

const MAX_REPORTED_REMARKS: usize = 4;

pub async fn sentinel_report() -> SentinelReport {
    crate::config::Config::runtime()
        .await
        .data_arc()
        .sentinel_report
        .clone()
}

pub async fn server_descriptions() -> HashMap<String, String> {
    server_descriptions_of(&Config::runtime().await)
}

fn server_descriptions_of(runtime: &Draft<IRuntime>) -> HashMap<String, String> {
    let committed = runtime.data_arc();
    let draft = runtime.latest_arc();
    committed
        .config
        .as_ref()
        .or_else(|| draft.config.as_ref())
        .map(collect_server_descriptions)
        .unwrap_or_default()
}

fn collect_server_descriptions(config: &Mapping) -> HashMap<String, String> {
    const KEYS: [&str; 3] = ["serverDescription", "server_description", "server-description"];

    let mut descriptions = HashMap::new();
    let Some(Value::Sequence(proxies)) = config.get("proxies") else {
        return descriptions;
    };

    for proxy in proxies {
        let Some(proxy) = proxy.as_mapping() else {
            continue;
        };
        let Some(name) = proxy.get("name").and_then(Value::as_str) else {
            continue;
        };
        let description = KEYS
            .iter()
            .find_map(|key| proxy.get(*key).and_then(Value::as_str))
            .map(str::trim)
            .filter(|text| !text.is_empty());
        if let Some(description) = description {
            descriptions.insert(String::from(name), String::from(description));
        }
    }

    descriptions
}

const SERVERLESS_TYPES: &[&str] = &["direct", "reject", "reject-drop", "pass", "dns"];

fn is_serverless_proxy(proxy: &Mapping) -> bool {
    proxy
        .get("type")
        .and_then(Value::as_str)
        .is_some_and(|kind| SERVERLESS_TYPES.contains(&kind.trim().to_ascii_lowercase().as_str()))
}

fn is_sentinel_proxy(proxy: &Mapping) -> bool {
    const NIL_UUID: &str = "00000000-0000-0000-0000-000000000000";

    if is_serverless_proxy(proxy) {
        return false;
    }

    let unspecified_host = proxy
        .get("server")
        .map(|value| match value {
            Value::String(host) => {
                matches!(host.trim(), "" | "0.0.0.0" | "::" | "[::]" | "0:0:0:0:0:0:0:0")
            }
            Value::Null => true,
            _ => false,
        })
        .unwrap_or(false);

    let dead_port = proxy
        .get("port")
        .map(|value| match value {
            Value::Number(port) => port.as_u64().map(|port| port <= 1).unwrap_or(true),
            Value::String(port) => port.trim().parse::<u64>().map(|port| port <= 1).unwrap_or(true),
            _ => true,
        })
        .unwrap_or(false);

    let nil_uuid = proxy
        .get("uuid")
        .and_then(Value::as_str)
        .map(|uuid| uuid.trim().eq_ignore_ascii_case(NIL_UUID))
        .unwrap_or(false);

    unspecified_host || nil_uuid || (dead_port && missing_credentials(proxy))
}

fn missing_credentials(proxy: &Mapping) -> bool {
    ["uuid", "password", "psk", "private-key", "auth", "auth-str", "token"]
        .iter()
        .all(|key| {
            proxy
                .get(*key)
                .and_then(Value::as_str)
                .is_none_or(|value| value.trim().is_empty())
        })
}

fn group_fills_at_runtime(group: &Mapping, providers: &HashSet<String>) -> bool {
    let flag = |key: &str| matches!(group.get(key), Some(Value::Bool(true)));

    if flag("include-all") || flag("include-all-proxies") {
        return true;
    }
    if flag("include-all-providers") && !providers.is_empty() {
        return true;
    }

    group.get("use").and_then(Value::as_sequence).is_some_and(|names| {
        names
            .iter()
            .filter_map(Value::as_str)
            .any(|name| providers.contains(name))
    })
}

fn provider_names(config: &Mapping, only_from_the_network: bool) -> HashSet<String> {
    config
        .get("proxy-providers")
        .and_then(Value::as_mapping)
        .map(|map| {
            map.iter()
                .filter(|(_, provider)| {
                    !only_from_the_network
                        || provider
                            .as_mapping()
                            .and_then(|provider| provider.get("type"))
                            .and_then(Value::as_str)
                            .is_some_and(|kind| kind.eq_ignore_ascii_case("http"))
                })
                .filter_map(|(name, _)| name.as_str())
                .map(Into::into)
                .collect()
        })
        .unwrap_or_default()
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct Rejections {
    emptied: HashSet<String>,
    waiting: HashSet<String>,
}

impl Rejections {
    fn for_section(&self, section: &str) -> HashSet<String> {
        let mut groups = self.emptied.clone();
        if section == "proxy-providers" {
            groups.extend(self.waiting.iter().cloned());
        }
        groups
    }
}

fn backfill_empty_groups(mut config: Mapping) -> (Mapping, Rejections) {
    let providers = provider_names(&config, false);
    let network_providers = provider_names(&config, true);
    let mut rejected = Rejections::default();

    if let Some(Value::Sequence(groups)) = config.get_mut("proxy-groups") {
        for group in groups {
            let Some(group_map) = group.as_mapping_mut() else {
                continue;
            };

            let has_members = group_map
                .get("proxies")
                .and_then(Value::as_sequence)
                .is_some_and(|items| !items.is_empty());

            let fed_by_a_provider = group_fills_at_runtime(group_map, &providers);
            if group_fills_at_runtime(group_map, &network_providers) {
                // clod:Э11-09 — такая группа наполнится, когда ядро дотянет провайдера,
                // и пустой её мы не считаем. Но пока провайдер тянется из сети, группа
                // пуста, и ядро подставляет вместо неё встроенную заглушку — а та по
                // умолчанию `COMPATIBLE`, то есть прямое соединение. Трафик группы
                // молча уходил мимо туннеля. Говорим ядру подставлять `REJECT`:
                // видимый отказ лучше тихого директа. Шаблон подписки при этом не
                // трогается — правится уже собранный конфиг, тут же, где и остальное.
                let ours = group_map.get("empty-fallback").is_none();
                group_map
                    .entry(Value::from("empty-fallback"))
                    .or_insert_with(|| Value::from("REJECT"));

                // Пока группа не наполнилась, она отвергает — в том числе и загрузку
                // того самого провайдера, если провайдер ходит через неё. Такую
                // привязку снимаем ниже, вместе с привязками к пустым группам:
                // иначе получился бы замкнутый круг «группа ждёт провайдера, а
                // провайдер не грузится, потому что группа отвергает».
                if ours && let Some(name) = group_map.get("name").and_then(Value::as_str) {
                    rejected.waiting.insert(name.into());
                }
                continue;
            }

            if has_members || fed_by_a_provider {
                continue;
            }

            group_map.insert(Value::from("proxies"), Value::Sequence(vec![Value::from("REJECT")]));
            if let Some(name) = group_map.get("name").and_then(Value::as_str) {
                rejected.emptied.insert(name.into());
            }
        }
    }

    (config, rejected)
}

struct GroupOutlets {
    picks_the_first: bool,
    members: Vec<String>,
}

fn group_outlets(config: &Mapping) -> HashMap<String, GroupOutlets> {
    config
        .get("proxy-groups")
        .and_then(Value::as_sequence)
        .map(|groups| {
            groups
                .iter()
                .filter_map(|group| {
                    let group = group.as_mapping()?;
                    let name = group.get("name").and_then(Value::as_str)?;
                    let picks_the_first = group
                        .get("type")
                        .and_then(Value::as_str)
                        .is_none_or(|kind| kind.trim().eq_ignore_ascii_case("select"));
                    let members = group
                        .get("proxies")
                        .and_then(Value::as_sequence)
                        .map(|items| items.iter().filter_map(Value::as_str).map(Into::into).collect())
                        .unwrap_or_default();
                    Some((
                        name.into(),
                        GroupOutlets {
                            picks_the_first,
                            members,
                        },
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn walks_into_rejection<'a>(
    name: &'a str,
    outlets: &'a HashMap<String, GroupOutlets>,
    rejected: &HashSet<String>,
    visited: &mut HashSet<&'a str>,
) -> bool {
    if rejected.contains(name) || matches!(name, "REJECT" | "REJECT-DROP") {
        return true;
    }
    let Some(outlet) = outlets.get(name) else {
        return false;
    };
    if !visited.insert(name) {
        return false;
    }
    if outlet.picks_the_first {
        return outlet
            .members
            .first()
            .is_some_and(|first| walks_into_rejection(first, outlets, rejected, visited));
    }
    !outlet.members.is_empty()
        && outlet
            .members
            .iter()
            .all(|member| walks_into_rejection(member, outlets, rejected, visited))
}

fn resolves_to_rejection(start: &str, outlets: &HashMap<String, GroupOutlets>, rejected: &HashSet<String>) -> bool {
    walks_into_rejection(start, outlets, rejected, &mut HashSet::new())
}

fn unpin_providers_from_rejection(mut config: Mapping, rejected: &Rejections) -> Mapping {
    if rejected.emptied.is_empty() && rejected.waiting.is_empty() {
        return config;
    }

    let outlets = group_outlets(&config);

    for section in ["rule-providers", "proxy-providers"] {
        let rejected = rejected.for_section(section);
        if rejected.is_empty() {
            continue;
        }
        let Some(Value::Mapping(providers)) = config.get_mut(section) else {
            continue;
        };
        for provider in providers.values_mut() {
            let Some(provider) = provider.as_mapping_mut() else {
                continue;
            };
            let pinned = provider
                .get("proxy")
                .and_then(Value::as_str)
                .is_some_and(|name| resolves_to_rejection(name, &outlets, &rejected));
            if pinned {
                provider.remove("proxy");
            }
        }
    }

    config
}

fn retain_live_proxies(
    proxies: &mut Vec<Value>,
    dropped: &mut HashSet<String>,
    already_reported: &HashSet<String>,
    remarks: &mut Vec<String>,
) -> usize {
    let mut removed = 0;
    proxies.retain(|item| {
        let Some(proxy) = item.as_mapping() else {
            return true;
        };
        if !is_sentinel_proxy(proxy) {
            return true;
        }
        removed += 1;
        if let Some(name) = proxy.get("name").and_then(Value::as_str)
            && dropped.insert(name.into())
            && !already_reported.contains(name)
        {
            remarks.push(name.into());
        }
        false
    });
    removed
}

fn is_live_proxy(item: &Value) -> bool {
    item.as_mapping()
        .is_none_or(|proxy| !is_serverless_proxy(proxy) && !is_sentinel_proxy(proxy))
}

fn live_proxy_count(proxies: Option<&Value>) -> usize {
    proxies
        .and_then(Value::as_sequence)
        .map_or(0, |items| items.iter().filter(|item| is_live_proxy(item)).count())
}

fn inline_provider_live_count(config: &Mapping) -> usize {
    config
        .get("proxy-providers")
        .and_then(Value::as_mapping)
        .map_or(0, |providers| {
            providers
                .values()
                .map(|provider| live_proxy_count(provider.as_mapping().and_then(|map| map.get("payload"))))
                .sum()
        })
}

fn has_opaque_provider(config: &Mapping) -> bool {
    config
        .get("proxy-providers")
        .and_then(Value::as_mapping)
        .is_some_and(|providers| {
            providers.values().any(|provider| {
                provider
                    .as_mapping()
                    .and_then(|map| map.get("type"))
                    .and_then(Value::as_str)
                    .is_none_or(|kind| !kind.trim().eq_ignore_ascii_case("inline"))
            })
        })
}

fn filter_sentinel_proxies(mut config: Mapping) -> (Mapping, SentinelReport) {
    let mut dropped: HashSet<String> = HashSet::new();
    let mut provider_dropped: HashSet<String> = HashSet::new();
    let mut remarks: Vec<String> = Vec::new();
    let mut dropped_total = 0;

    if let Some(Value::Sequence(proxies)) = config.get_mut("proxies") {
        dropped_total += retain_live_proxies(proxies, &mut dropped, &provider_dropped, &mut remarks);
    }

    if let Some(Value::Mapping(providers)) = config.get_mut("proxy-providers") {
        for provider in providers.values_mut() {
            let Some(Value::Sequence(payload)) = provider.as_mapping_mut().and_then(|map| map.get_mut("payload"))
            else {
                continue;
            };
            let keeps_a_node = payload
                .iter()
                .any(|item| item.as_mapping().is_none_or(|proxy| !is_sentinel_proxy(proxy)));
            if !keeps_a_node {
                continue;
            }
            dropped_total += retain_live_proxies(payload, &mut provider_dropped, &dropped, &mut remarks);
        }
    }

    let survivors = live_proxy_count(config.get("proxies")) + inline_provider_live_count(&config);
    let only_sentinels = survivors == 0 && !has_opaque_provider(&config);
    let report = SentinelReport {
        remarks: remarks.into_iter().take(MAX_REPORTED_REMARKS).collect(),
        only_sentinels,
        partially_dropped: dropped_total > 0 && !only_sentinels,
        dropped_total,
    };

    if dropped_total > 0 {
        logging!(
            info,
            Type::Core,
            "drop {} sentinel proxies from the subscription",
            dropped_total
        );
    }

    if dropped.is_empty() {
        return (config, report);
    }

    if let Some(Value::Sequence(groups)) = config.get_mut("proxy-groups") {
        for group in groups {
            let Some(Value::Sequence(items)) = group.as_mapping_mut().and_then(|map| map.get_mut("proxies")) else {
                continue;
            };
            items.retain(|item| match item.as_str() {
                Some(name) => !dropped.contains(name),
                None => true,
            });
        }
    }

    (config, report)
}

fn cleanup_proxy_groups(mut config: Mapping) -> Mapping {
    const BUILTIN_POLICIES: &[&str] = &["DIRECT", "REJECT", "REJECT-DROP", "PASS"];

    let proxy_names = config
        .get("proxies")
        .and_then(|v| v.as_sequence())
        .map(|seq| {
            seq.iter()
                .filter_map(|item| match item {
                    Value::Mapping(map) => map
                        .get("name")
                        .and_then(Value::as_str)
                        .map(|name| name.to_owned().into()),
                    Value::String(name) => Some(name.to_owned().into()),
                    _ => None,
                })
                .collect::<HashSet<String>>()
        })
        .unwrap_or_default();

    let group_names = config
        .get("proxy-groups")
        .and_then(|v| v.as_sequence())
        .map(|seq| {
            seq.iter()
                .filter_map(|item| {
                    item.as_mapping()
                        .and_then(|map| map.get("name"))
                        .and_then(Value::as_str)
                        .map(std::convert::Into::into)
                })
                .collect::<HashSet<String>>()
        })
        .unwrap_or_default();

    let provider_names = config
        .get("proxy-providers")
        .and_then(Value::as_mapping)
        .map(|map| {
            map.keys()
                .filter_map(Value::as_str)
                .map(std::convert::Into::into)
                .collect::<HashSet<String>>()
        })
        .unwrap_or_default();

    let mut allowed_names = proxy_names;
    allowed_names.extend(group_names);
    allowed_names.extend(provider_names.iter().cloned());
    allowed_names.extend(BUILTIN_POLICIES.iter().map(|p| (*p).into()));

    if let Some(Value::Sequence(groups)) = config.get_mut("proxy-groups") {
        for group in groups {
            if let Some(group_map) = group.as_mapping_mut() {
                let mut has_valid_provider = false;

                if let Some(Value::Sequence(uses)) = group_map.get_mut("use") {
                    uses.retain(|provider| match provider {
                        Value::String(name) => {
                            let exists = provider_names.contains(name.as_str());
                            has_valid_provider = has_valid_provider || exists;
                            exists
                        }
                        _ => false,
                    });
                }

                if let Some(Value::Sequence(proxies)) = group_map.get_mut("proxies") {
                    proxies.retain(|proxy| match proxy {
                        Value::String(name) => allowed_names.contains(name.as_str()) || has_valid_provider,
                        _ => true,
                    });
                }
            }
        }
    }

    config
}

fn ensure_fake_ip_range6(dns: &mut Mapping) {
    use serde_yaml_ng::Value;

    let ipv6_enabled = dns.get("ipv6").and_then(|v| v.as_bool()).unwrap_or(false);
    let is_fake_ip = dns
        .get("enhanced-mode")
        .and_then(|v| v.as_str())
        .map(|m| m == "fake-ip")
        .unwrap_or(true);

    let range6_missing = dns
        .get("fake-ip-range6")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().is_empty())
        .unwrap_or(true);

    if ipv6_enabled && is_fake_ip && range6_missing {
        dns.insert(Value::from("fake-ip-range6"), Value::from("2001:2::0/64"));
    }
}

async fn apply_dns_settings(mut config: Mapping, enable_dns_settings: bool) -> Mapping {
    if enable_dns_settings && let Ok(app_dir) = dirs::app_home_dir() {
        let dns_path = app_dir.join(constants::files::DNS_CONFIG);

        if dns_path.exists()
            && let Ok(dns_yaml) = fs::read_to_string(&dns_path).await
            && let Ok(dns_config) = serde_yaml_ng::from_str::<serde_yaml_ng::Mapping>(&dns_yaml)
        {
            if let Some(hosts_value) = dns_config.get("hosts")
                && hosts_value.is_mapping()
            {
                config.insert("hosts".into(), hosts_value.clone());
                logging!(info, Type::Core, "apply hosts configuration");
            }

            if let Some(dns_value) = dns_config.get("dns") {
                if let Some(dns_mapping) = dns_value.as_mapping() {
                    let mut dns_mapping = dns_mapping.clone();
                    ensure_fake_ip_range6(&mut dns_mapping);
                    config.insert("dns".into(), dns_mapping.into());
                    logging!(info, Type::Core, "apply dns_config.yaml (dns section)");
                }
            } else {
                let mut dns_config = dns_config;
                ensure_fake_ip_range6(&mut dns_config);
                config.insert("dns".into(), dns_config.into());
                logging!(info, Type::Core, "apply dns_config.yaml");
            }
        }
    }

    config
}

pub async fn enhance() -> Result<(Mapping, HashSet<String>, HashMap<String, ResultLog>, SentinelReport)> {
    let cfg_vals = get_config_values().await;
    let ConfigValues {
        clash_config,
        clash_core,
        enable_tun,
        enable_builtin,
        socks_enabled,
        http_enabled,
        enable_dns_settings,
        tun_overrides,
        #[cfg(not(target_os = "windows"))]
        redir_enabled,
        #[cfg(target_os = "linux")]
        tproxy_enabled,
    } = cfg_vals;

    let profile = collect_profile_items().await?;
    let config = profile.config;
    let merge_item = profile.merge_item;
    let script_item = profile.script_item;
    let rules_item = profile.rules_item;
    let proxies_item = profile.proxies_item;
    let groups_item = profile.groups_item;
    let global_merge = profile.global_merge;
    let global_script = profile.global_script;
    let profile_name = profile.profile_name;
    let profile_is_remote = profile.profile_is_remote;
    let profile_shows_zero_hosts = profile.profile_shows_zero_hosts;

    let result_map = HashMap::new();

    let exists_keys = use_keys(&config).collect::<Vec<_>>();
    let config = process_seq_items(config, rules_item, proxies_item, groups_item);

    let config = merge_default_config(
        config,
        clash_config,
        socks_enabled,
        http_enabled,
        &tun_overrides,
        #[cfg(not(target_os = "windows"))]
        redir_enabled,
        #[cfg(target_os = "linux")]
        tproxy_enabled,
    )
    .await;

    let config = apply_builtin_scripts(config, clash_core, enable_builtin).await;
    let config = use_tun(config, enable_tun);
    let config = apply_dns_settings(config, enable_dns_settings).await;
    let config = ensure_dns_for_tun(config, enable_tun);

    let control_plane = snapshot_control_plane(&config);
    let dns_page = if enable_dns_settings {
        snapshot_dns_page(&config)
    } else {
        Mapping::new()
    };

    let (config, exists_keys, result_map) = process_global_items(
        config,
        exists_keys,
        result_map,
        global_merge,
        global_script,
        &profile_name,
    )
    .await;

    let (config, exists_keys, result_map) =
        process_profile_items(config, exists_keys, result_map, merge_item, script_item, &profile_name).await;

    let config = enforce_control_plane(config, control_plane);
    let config = enforce_dns_page(config, dns_page);
    let config = ensure_dns_for_tun(config, enable_tun);
    let config = ensure_lan_bind_address(config);
    let config = ensure_store_selected(config);

    let (config, sentinel_report) = if profile_is_remote && !profile_shows_zero_hosts {
        filter_sentinel_proxies(config)
    } else {
        (config, SentinelReport::default())
    };
    let config = cleanup_proxy_groups(config);
    let (config, rejected_groups) = backfill_empty_groups(config);
    let config = unpin_providers_from_rejection(config, &rejected_groups);
    let config = use_sort(config);

    if profile_is_remote {
        let described = collect_server_descriptions(&config).len();
        if described == 0 {
            logging!(
                info,
                Type::Config,
                "no serverDescription in the subscription: the panel serves it only to clients \
                 matched by additionalExtendedClientsRegex (^ClodClash/)"
            );
        } else {
            logging!(info, Type::Config, "server descriptions in the config: {}", described);
        }
    }

    let mut exists_keys_set = HashSet::new();
    exists_keys_set.extend(exists_keys);

    Ok((config, exists_keys_set, result_map, sentinel_report))
}

#[allow(clippy::expect_used)]
#[cfg(test)]
mod tests {
    use super::{
        ChainItem, ChainType, Draft, IRuntime, MAX_REPORTED_REMARKS, backfill_empty_groups, cleanup_proxy_groups,
        collect_server_descriptions, ensure_lan_bind_address, ensure_store_selected, filter_sentinel_proxies,
        process_global_items, process_profile_items, server_descriptions_of, unpin_providers_from_rejection, use_keys,
    };
    use std::collections::HashMap;

    fn mapping(yaml: &str) -> serde_yaml_ng::Mapping {
        serde_yaml_ng::from_str(yaml).expect("test config should be valid")
    }

    fn mapping_value(yaml: &str) -> serde_yaml_ng::Value {
        serde_yaml_ng::from_str(yaml).expect("test value should be valid")
    }

    #[test]
    fn store_selected_is_forced_in_final_config() {
        for source in [
            "{mode: rule}",
            "{profile: {store-fake-ip: false}}",
            "{profile: {store-selected: false}}",
        ] {
            let config = ensure_store_selected(mapping(source));
            let profile = config
                .get("profile")
                .and_then(|value| value.as_mapping())
                .expect("profile block should exist");
            assert_eq!(
                profile.get("store-selected").and_then(|value| value.as_bool()),
                Some(true),
                "store-selected should be true for source {source}"
            );
            assert_eq!(
                profile.get("store-fake-ip").and_then(|value| value.as_bool()),
                Some(true),
                "store-fake-ip should be true for source {source}"
            );
        }
    }

    #[tokio::test]
    async fn manual_overrides_follow_expected_priority() {
        let mut config = mapping(
            r"{global-merge-wins: other, global-script-wins: other, profile-merge-wins: other,
               profile-script-wins: other, nested: {winner: other}, dns: {enable: true}, tun: {enable: true}}",
        );
        let exists_keys = use_keys(&config).collect();
        config.insert("application-only".into(), true.into());

        let global_merge = ChainItem {
            uid: "Merge".into(),
            data: ChainType::Merge(mapping(
                r"{global-merge-wins: global-merge, global-script-wins: global-merge,
                   profile-merge-wins: global-merge, profile-script-wins: global-merge,
                   nested: {winner: global-merge}, dns: {enable: false}, tun: {enable: false}}",
            )),
        };
        let global_script = ChainItem::to_script(
            "Script",
            r#"function main(config) {
              config["global-script-wins"] = "global-script";
              config["profile-merge-wins"] = "global-script";
              config["profile-script-wins"] = "global-script";
              config.nested.winner = "global-script";
              return config;
            }"#,
        );
        let profile_merge = ChainItem {
            uid: "profile-merge".into(),
            data: ChainType::Merge(mapping(
                r"{profile-merge-wins: profile-merge, profile-script-wins: profile-merge,
                   nested: {winner: profile-merge}}",
            )),
        };
        let profile_script = ChainItem::to_script(
            "profile-script",
            r#"function main(config) {
              config["profile-script-wins"] = "profile-script";
              config.nested.winner = "profile-script";
              return config;
            }"#,
        );

        let profile_name = "test-profile".into();
        let (config, exists_keys, result_map) = process_global_items(
            config,
            exists_keys,
            HashMap::new(),
            global_merge,
            global_script,
            &profile_name,
        )
        .await;
        let (config, exists_keys, _) = process_profile_items(
            config,
            exists_keys,
            result_map,
            profile_merge,
            profile_script,
            &profile_name,
        )
        .await;

        let string_value = |key| config.get(key).and_then(serde_yaml_ng::Value::as_str);
        assert_eq!(string_value("global-merge-wins"), Some("global-merge"));
        assert_eq!(string_value("global-script-wins"), Some("global-script"));
        assert_eq!(string_value("profile-merge-wins"), Some("profile-merge"));
        assert_eq!(string_value("profile-script-wins"), Some("profile-script"));
        assert_eq!(
            config
                .get("nested")
                .and_then(|value| value.get("winner"))
                .and_then(serde_yaml_ng::Value::as_str),
            Some("profile-script")
        );
        assert!(!exists_keys.contains(&"application-only".into()));
    }

    #[test]
    fn tun_defaults_yield_to_the_subscription() {
        let mut tun = mapping("{strict-route: true, dns-hijack: [\"any:53\", \"tcp://any:53\"], mtu: 9000}");
        let app = mapping("{stack: gvisor, strict-route: false, dns-hijack: [\"any:53\"], auto-route: true}");
        super::ladder_tun(&mut tun, app, &super::TunOverrides::default());
        assert_eq!(tun.get("strict-route"), Some(&serde_yaml_ng::Value::from(true)));
        assert_eq!(
            tun.get("dns-hijack"),
            Some(&mapping_value("[\"any:53\", \"tcp://any:53\"]"))
        );
        assert_eq!(tun.get("stack"), Some(&serde_yaml_ng::Value::from("gvisor")));
        assert_eq!(tun.get("auto-route"), Some(&serde_yaml_ng::Value::from(true)));
        assert_eq!(tun.get("mtu"), Some(&serde_yaml_ng::Value::from(9000)));
    }

    #[test]
    fn windows_takes_only_gvisor_from_the_subscription() {
        let mut tun = mapping("{stack: system}");
        let app = mapping("{stack: gvisor}");
        super::ladder_tun_on(&mut tun, app, &super::TunOverrides::default(), true);
        assert_eq!(tun.get("stack"), Some(&serde_yaml_ng::Value::from("gvisor")));

        let mut tun = mapping("{stack: mixed}");
        let app = mapping("{stack: gvisor}");
        let chosen = super::parse_tun_overrides(Some("mixed"), None, None);
        super::ladder_tun_on(&mut tun, app, &chosen, true);
        assert_eq!(tun.get("stack"), Some(&serde_yaml_ng::Value::from("mixed")));

        let mut tun = mapping("{stack: system}");
        let app = mapping("{stack: gvisor}");
        super::ladder_tun_on(&mut tun, app, &super::TunOverrides::default(), false);
        assert_eq!(tun.get("stack"), Some(&serde_yaml_ng::Value::from("system")));
    }

    #[test]
    fn the_cap_ignores_the_case_of_the_stack_name_just_like_the_core() {
        for written in ["System", "MIXED", "Mixed"] {
            let mut tun = mapping(&format!("{{stack: \"{written}\"}}"));
            let app = mapping("{stack: gvisor}");
            super::ladder_tun_on(&mut tun, app, &super::TunOverrides::default(), true);
            assert_eq!(
                tun.get("stack"),
                Some(&serde_yaml_ng::Value::from("gvisor")),
                "stack written as {written} slipped past the cap"
            );
        }
    }

    #[test]
    fn the_subscription_decides_ipv6() {
        let subscription = mapping("{ipv6: true}");
        assert_eq!(
            super::subscription_or_app(&subscription, "ipv6", &serde_yaml_ng::Value::from(false)),
            serde_yaml_ng::Value::from(true)
        );
        let silent = mapping("{}");
        assert_eq!(
            super::subscription_or_app(&silent, "ipv6", &serde_yaml_ng::Value::from(false)),
            serde_yaml_ng::Value::from(false)
        );
        let explicit_off = mapping("{ipv6: false}");
        assert_eq!(
            super::subscription_or_app(&explicit_off, "ipv6", &serde_yaml_ng::Value::from(true)),
            serde_yaml_ng::Value::from(false)
        );
    }

    #[test]
    fn the_ladder_keys_follow_the_subscription_unless_the_user_chose() {
        let mut silent_everywhere = mapping("{}");
        super::fill_the_ladder_defaults(&mut silent_everywhere);
        assert_eq!(
            silent_everywhere
                .get("log-level")
                .and_then(serde_yaml_ng::Value::as_str),
            Some("info")
        );
        assert_eq!(
            silent_everywhere
                .get("unified-delay")
                .and_then(serde_yaml_ng::Value::as_bool),
            Some(true)
        );

        let mut from_subscription = mapping("{log-level: error, unified-delay: false}");
        super::fill_the_ladder_defaults(&mut from_subscription);
        assert_eq!(
            from_subscription
                .get("log-level")
                .and_then(serde_yaml_ng::Value::as_str),
            Some("error")
        );
        assert_eq!(
            from_subscription
                .get("unified-delay")
                .and_then(serde_yaml_ng::Value::as_bool),
            Some(false)
        );

        assert!(!super::SUBSCRIPTION_DECIDES.contains(&"log-level"));
        assert!(super::SUBSCRIPTION_DECIDES.contains(&"ipv6"));

        let mut stock = mapping("{log-level: info, unified-delay: true, mixed-port: 7897}");
        assert!(crate::config::IClashTemp::unpin_legacy_defaults(&mut stock));
        assert!(stock.get("log-level").is_none());
        assert!(stock.get("unified-delay").is_none());
        assert!(stock.get("mixed-port").is_some());
        let mut chosen = mapping("{log-level: debug, unified-delay: false}");
        assert!(!crate::config::IClashTemp::unpin_legacy_defaults(&mut chosen));

        let mut app = crate::config::IClashTemp(mapping("{log-level: debug}"));
        app.patch_config(&mapping("{log-level: auto}"));
        assert!(app.0.get("log-level").is_none());
        app.patch_config(&mapping("{log-level: warning}"));
        assert_eq!(
            app.0.get("log-level").and_then(serde_yaml_ng::Value::as_str),
            Some("warning")
        );
    }

    #[test]
    fn a_silent_subscription_gets_the_app_defaults() {
        let mut tun = mapping("{}");
        let app = mapping("{stack: gvisor, strict-route: false, dns-hijack: [\"any:53\"]}");
        super::ladder_tun(&mut tun, app, &super::TunOverrides::default());
        assert_eq!(tun.get("strict-route"), Some(&serde_yaml_ng::Value::from(false)));
        assert_eq!(tun.get("stack"), Some(&serde_yaml_ng::Value::from("gvisor")));
    }

    #[test]
    fn the_user_override_beats_the_subscription() {
        let overrides = super::parse_tun_overrides(Some("system"), Some("off"), Some("any:53"));
        let mut tun = mapping("{stack: gvisor, strict-route: true, dns-hijack: [\"tcp://any:53\"]}");
        let app = mapping("{stack: gvisor, strict-route: false, dns-hijack: [\"any:53\"]}");
        super::ladder_tun(&mut tun, app, &overrides);
        assert_eq!(tun.get("stack"), Some(&serde_yaml_ng::Value::from("system")));
        assert_eq!(tun.get("strict-route"), Some(&serde_yaml_ng::Value::from(false)));
        assert_eq!(tun.get("dns-hijack"), Some(&mapping_value("[\"any:53\"]")));
    }

    #[test]
    fn auto_and_junk_mean_no_override() {
        let overrides = super::parse_tun_overrides(Some("auto"), Some("auto"), Some("auto"));
        assert!(overrides.stack.is_none());
        assert!(overrides.strict_route.is_none());
        assert!(overrides.dns_hijack.is_none());

        let junk = super::parse_tun_overrides(Some("weird"), Some("yes"), None);
        assert!(junk.stack.is_none());
        assert!(junk.strict_route.is_none());
        assert!(junk.dns_hijack.is_none());

        let empty = super::parse_tun_overrides(None, None, Some(""));
        assert_eq!(empty.dns_hijack, Some(serde_yaml_ng::Value::Sequence(Vec::new())));
    }

    #[test]
    fn a_profile_cannot_switch_tun_off() {
        let app_config = mapping(r"{tun: {enable: true, stack: gvisor}, mixed-port: 7890}");
        let snapshot = super::snapshot_control_plane(&app_config);

        let hijacked = mapping(r"{tun: {enable: false}, mixed-port: 1080}");
        let result = super::enforce_control_plane(hijacked, snapshot);

        let tun = result.get("tun").and_then(serde_yaml_ng::Value::as_mapping);
        assert_eq!(
            tun.and_then(|tun| tun.get("enable"))
                .and_then(serde_yaml_ng::Value::as_bool),
            Some(true)
        );
    }

    #[test]
    fn a_profile_cannot_switch_tun_on() {
        let app_config = mapping(r"{mixed-port: 7890}");
        let snapshot = super::snapshot_control_plane(&app_config);

        let hijacked = mapping(r"{tun: {enable: true}, mixed-port: 7890}");
        let result = super::enforce_control_plane(hijacked, snapshot);

        assert!(result.get("tun").is_none());
    }

    #[test]
    fn control_plane_survives_manual_overrides() {
        let app_config = mapping(
            r#"{external-controller: "",
                external-controller-cors: {allow-origins: ["app-only"]},
                mixed-port: 7890, socks-port: 7891, secret: "app-secret", mode: rule, allow-lan: false,
                log-level: info, ipv6: false, unified-delay: true,
                dns: {proxy-server-nameserver: ["1.1.1.1"]}}"#,
        );
        let snapshot = super::snapshot_control_plane(&app_config);

        let hijacked = mapping(
            r#"{external-controller: "0.0.0.0:9090",
                external-controller-cors: {allow-origins: ["*"]},
                mixed-port: 1080, socks-port: 1080, secret: "hijacked", mode: global, allow-lan: true,
                log-level: debug, ipv6: true, unified-delay: false,
                dns: {proxy-server-nameserver: ["8.8.8.8"]}}"#,
        );

        let result = super::enforce_control_plane(hijacked, snapshot);

        let as_str = |key| result.get(key).and_then(serde_yaml_ng::Value::as_str);
        assert_eq!(as_str("external-controller"), Some(""));
        assert_eq!(
            result.get("mixed-port").and_then(serde_yaml_ng::Value::as_u64),
            Some(7890)
        );
        assert_eq!(
            result.get("socks-port").and_then(serde_yaml_ng::Value::as_u64),
            Some(7891)
        );
        assert_eq!(
            result
                .get("external-controller-cors")
                .and_then(|value| value.get("allow-origins"))
                .and_then(serde_yaml_ng::Value::as_sequence)
                .and_then(|seq| seq.first())
                .and_then(serde_yaml_ng::Value::as_str),
            Some("app-only")
        );
        assert_eq!(as_str("secret"), Some("app-secret"));
        assert_eq!(as_str("mode"), Some("rule"));
        assert_eq!(
            result.get("allow-lan").and_then(serde_yaml_ng::Value::as_bool),
            Some(false)
        );
        assert_eq!(as_str("log-level"), Some("info"));
        assert_eq!(result.get("ipv6").and_then(serde_yaml_ng::Value::as_bool), Some(false));
        assert_eq!(
            result.get("unified-delay").and_then(serde_yaml_ng::Value::as_bool),
            Some(true)
        );

        assert_eq!(
            result
                .get("dns")
                .and_then(|value| value.get("proxy-server-nameserver"))
                .and_then(serde_yaml_ng::Value::as_sequence)
                .and_then(|seq| seq.first())
                .and_then(serde_yaml_ng::Value::as_str),
            Some("8.8.8.8")
        );
    }

    #[test]
    fn lan_bind_address_loopback_is_widened() {
        for bind_address in [
            "localhost",
            "127.0.0.1",
            "127.0.0.2",
            "127.1",
            "::1",
            "[::1]",
            "0:0:0:0:0:0:0:1",
        ] {
            let result = ensure_lan_bind_address(mapping(&format!(
                r#"{{allow-lan: true, bind-address: "{bind_address}"}}"#
            )));

            assert_eq!(
                result.get("bind-address").and_then(serde_yaml_ng::Value::as_str),
                Some("*"),
                "bind-address {bind_address} should be widened"
            );
        }
    }

    #[test]
    fn lan_bind_address_preserves_custom_or_disabled() {
        let custom = ensure_lan_bind_address(mapping(r#"{allow-lan: true, bind-address: "192.168.1.2"}"#));
        assert_eq!(
            custom.get("bind-address").and_then(serde_yaml_ng::Value::as_str),
            Some("192.168.1.2")
        );

        let disabled = ensure_lan_bind_address(mapping(r#"{allow-lan: false, bind-address: "127.0.0.1"}"#));
        assert_eq!(
            disabled.get("bind-address").and_then(serde_yaml_ng::Value::as_str),
            Some("127.0.0.1")
        );
    }

    #[test]
    fn control_plane_removes_reenabled_disabled_port() {
        let app_config = mapping(r"{mixed-port: 7890, mode: rule}");
        let snapshot = super::snapshot_control_plane(&app_config);

        let hijacked = mapping(r"{mixed-port: 7890, mode: rule, socks-port: 1080}");
        let result = super::enforce_control_plane(hijacked, snapshot);

        assert!(!result.contains_key("socks-port"));
        assert_eq!(
            result.get("mixed-port").and_then(serde_yaml_ng::Value::as_u64),
            Some(7890)
        );
    }

    #[test]
    fn dns_page_owns_the_dns_block_but_not_hosts() {
        let app_config = mapping(
            r#"{dns: {ipv6: false, enhanced-mode: fake-ip, proxy-server-nameserver: ["1.1.1.1"]}, hosts: {a.test: 1.2.3.4}}"#,
        );
        let snapshot = super::snapshot_dns_page(&app_config);
        assert!(!snapshot.contains_key("hosts"));

        let hijacked = mapping(
            r#"{dns: {ipv6: true, enhanced-mode: redir-host, proxy-server-nameserver: ["8.8.8.8"]}, hosts: {a.test: 9.9.9.9}}"#,
        );
        let result = super::enforce_dns_page(hijacked, snapshot);

        let dns = result.get("dns").expect("dns block");
        assert_eq!(dns.get("ipv6").and_then(serde_yaml_ng::Value::as_bool), Some(false));
        assert_eq!(
            dns.get("enhanced-mode").and_then(serde_yaml_ng::Value::as_str),
            Some("fake-ip")
        );
        assert_eq!(
            dns.get("proxy-server-nameserver")
                .and_then(serde_yaml_ng::Value::as_sequence)
                .and_then(|seq| seq.first())
                .and_then(serde_yaml_ng::Value::as_str),
            Some("1.1.1.1")
        );
        assert_eq!(
            result
                .get("hosts")
                .and_then(|value| value.get("a.test"))
                .and_then(serde_yaml_ng::Value::as_str),
            Some("9.9.9.9")
        );
    }

    #[test]
    fn dns_page_never_removes_what_it_did_not_write() {
        let snapshot = super::snapshot_dns_page(&mapping(r"{mode: rule}"));
        assert!(snapshot.is_empty());

        let from_merge = mapping(r#"{dns: {enable: true, nameserver: ["9.9.9.9"]}}"#);
        let result = super::enforce_dns_page(from_merge, snapshot);
        assert_eq!(
            result
                .get("dns")
                .and_then(|value| value.get("nameserver"))
                .and_then(serde_yaml_ng::Value::as_sequence)
                .and_then(|seq| seq.first())
                .and_then(serde_yaml_ng::Value::as_str),
            Some("9.9.9.9")
        );
    }

    #[test]
    fn a_profile_cannot_impose_an_external_ui() {
        let app_config = mapping(r"{mixed-port: 7890}");
        let snapshot = super::snapshot_control_plane(&app_config);

        let hijacked = mapping(
            r#"{mixed-port: 7890, external-ui: ./ui, external-ui-url: "https://evil.example/ui.zip",
                external-ui-name: dashboard}"#,
        );
        let result = super::enforce_control_plane(hijacked, snapshot);

        assert!(result.get("external-ui").is_none());
        assert!(result.get("external-ui-url").is_none());
        assert!(result.get("external-ui-name").is_none());
    }

    #[test]
    fn snapshot_control_plane_skips_absent_keys() {
        let app_config = mapping(r"{mode: rule, mixed-port: 7890}");
        let snapshot = super::snapshot_control_plane(&app_config);
        assert!(snapshot.contains_key("mode"));
        assert!(snapshot.contains_key("mixed-port"));
        assert!(!snapshot.contains_key("secret"));
        assert!(!snapshot.contains_key("allow-lan"));
    }

    #[test]
    fn remove_missing_proxies_from_groups() {
        let config_str = r#"
proxies:
  - name: "alive-node"
    type: ss
proxy-groups:
  - name: "manual"
    type: select
    proxies:
      - "alive-node"
      - "missing-node"
      - "DIRECT"
  - name: "nested"
    type: select
    proxies:
      - "manual"
      - "ghost"
"#;

        let mut config: serde_yaml_ng::Mapping =
            serde_yaml_ng::from_str(config_str).expect("Failed to parse test yaml");
        config = cleanup_proxy_groups(config);

        let groups = config
            .get("proxy-groups")
            .and_then(|v| v.as_sequence())
            .cloned()
            .expect("proxy-groups should be a sequence");

        let manual_group = groups
            .iter()
            .find(|group| group.get("name").and_then(serde_yaml_ng::Value::as_str) == Some("manual"))
            .and_then(|group| group.as_mapping())
            .expect("manual group should exist");

        let manual_proxies = manual_group
            .get("proxies")
            .and_then(|v| v.as_sequence())
            .expect("manual proxies should be a sequence");

        assert_eq!(manual_proxies.len(), 2);
        assert!(manual_proxies.iter().any(|p| p.as_str() == Some("alive-node")));
        assert!(manual_proxies.iter().any(|p| p.as_str() == Some("DIRECT")));

        let nested_group = groups
            .iter()
            .find(|group| group.get("name").and_then(serde_yaml_ng::Value::as_str) == Some("nested"))
            .and_then(|group| group.as_mapping())
            .expect("nested group should exist");

        let nested_proxies = nested_group
            .get("proxies")
            .and_then(|v| v.as_sequence())
            .expect("nested proxies should be a sequence");

        assert_eq!(nested_proxies.len(), 1);
        assert_eq!(nested_proxies[0].as_str(), Some("manual"));
    }

    #[test]
    fn keep_provider_backed_groups_intact() {
        let config_str = r#"
proxy-providers:
  providerA:
    type: http
    url: https://example.com
    path: ./providerA.yaml
proxies: []
proxy-groups:
  - name: "manual"
    type: select
    use:
      - "providerA"
      - "ghostProvider"
    proxies:
      - "dynamic-node"
      - "DIRECT"
"#;

        let mut config: serde_yaml_ng::Mapping =
            serde_yaml_ng::from_str(config_str).expect("Failed to parse test yaml");
        config = cleanup_proxy_groups(config);

        let groups = config
            .get("proxy-groups")
            .and_then(|v| v.as_sequence())
            .cloned()
            .expect("proxy-groups should be a sequence");

        let manual_group = groups
            .iter()
            .find(|group| group.get("name").and_then(serde_yaml_ng::Value::as_str) == Some("manual"))
            .and_then(|group| group.as_mapping())
            .expect("manual group should exist");

        let uses = manual_group
            .get("use")
            .and_then(|v| v.as_sequence())
            .expect("use should be a sequence");
        assert_eq!(uses.len(), 1);
        assert_eq!(uses[0].as_str(), Some("providerA"));

        let proxies = manual_group
            .get("proxies")
            .and_then(|v| v.as_sequence())
            .expect("proxies should be a sequence");
        assert_eq!(proxies.len(), 2);
        assert!(proxies.iter().any(|p| p.as_str() == Some("dynamic-node")));
        assert!(proxies.iter().any(|p| p.as_str() == Some("DIRECT")));
    }

    #[test]
    fn prune_invalid_provider_and_proxies_without_provider() {
        let config_str = r#"
proxy-groups:
  - name: "manual"
    type: select
    use:
      - "ghost-provider"
    proxies:
      - "ghost-node"
      - "DIRECT"
"#;

        let mut config: serde_yaml_ng::Mapping =
            serde_yaml_ng::from_str(config_str).expect("Failed to parse test yaml");
        config = cleanup_proxy_groups(config);

        let groups = config
            .get("proxy-groups")
            .and_then(|v| v.as_sequence())
            .cloned()
            .expect("proxy-groups should be a sequence");

        let manual_group = groups
            .iter()
            .find(|group| group.get("name").and_then(serde_yaml_ng::Value::as_str) == Some("manual"))
            .and_then(|group| group.as_mapping())
            .expect("manual group should exist");

        let uses = manual_group
            .get("use")
            .and_then(|v| v.as_sequence())
            .expect("use should be a sequence");
        assert_eq!(uses.len(), 0);

        let proxies = manual_group
            .get("proxies")
            .and_then(|v| v.as_sequence())
            .expect("proxies should be a sequence");
        assert_eq!(proxies.len(), 1);
        assert_eq!(proxies[0].as_str(), Some("DIRECT"));
    }

    #[test]
    fn a_group_waiting_for_its_provider_rejects_instead_of_going_direct() {
        // Пока провайдер тянется из сети, группа пуста, и ядро подставляет вместо
        // неё встроенную заглушку. По умолчанию это `COMPATIBLE`, а он ведёт себя
        // как прямое соединение: трафик группы молча уходил мимо туннеля.
        let config: serde_yaml_ng::Mapping = serde_yaml_ng::from_str(
            "proxy-providers:\n  panel:\n    type: http\nproxy-groups:\n  - name: Авто\n    type: url-test\n    use: [panel]\n",
        )
        .expect("тестовый конфиг разбирается");

        let (config, rejected) = backfill_empty_groups(config);

        // Группу не считаем пустой — она наполнится сама, — но подмену задаём свою.
        // И помечаем её как отвергающую, чтобы провайдер, который ходит через эту же
        // группу, не остался без загрузки: иначе круг замкнётся.
        assert!(rejected.waiting.contains("Авто"));
        assert!(rejected.emptied.is_empty());
        let group = config
            .get("proxy-groups")
            .and_then(|v| v.as_sequence())
            .and_then(|groups| groups.first())
            .and_then(serde_yaml_ng::Value::as_mapping)
            .expect("группа на месте");
        assert_eq!(
            group.get("empty-fallback").and_then(serde_yaml_ng::Value::as_str),
            Some("REJECT")
        );
    }

    #[test]
    fn a_provider_is_not_left_downloading_through_the_group_it_fills() {
        // Замкнутый круг: провайдер `panel` качается через группу `Авто`, а `Авто`
        // наполняется из `panel`. Пока группа пуста и отвергает, провайдер не
        // загрузится никогда — значит привязку надо снять.
        let config: serde_yaml_ng::Mapping = serde_yaml_ng::from_str(
            "proxy-providers:\n  panel:\n    type: http\n    proxy: Авто\nproxy-groups:\n  - name: Авто\n    type: url-test\n    use: [panel]\n",
        )
        .expect("тестовый конфиг разбирается");

        let (config, rejected) = backfill_empty_groups(config);
        let config = unpin_providers_from_rejection(config, &rejected);

        let provider = config
            .get("proxy-providers")
            .and_then(serde_yaml_ng::Value::as_mapping)
            .and_then(|providers| providers.get("panel"))
            .and_then(serde_yaml_ng::Value::as_mapping)
            .expect("провайдер на месте");
        assert!(
            provider.get("proxy").is_none(),
            "провайдер не должен качаться через группу, которую сам же наполняет"
        );
    }

    #[test]
    fn a_template_that_chose_its_own_empty_fallback_keeps_it() {
        let config: serde_yaml_ng::Mapping = serde_yaml_ng::from_str(
            "proxy-providers:\n  panel:\n    type: http\nproxy-groups:\n  - name: Авто\n    type: url-test\n    use: [panel]\n    empty-fallback: DIRECT\n",
        )
        .expect("тестовый конфиг разбирается");

        let (config, _) = backfill_empty_groups(config);

        let group = config
            .get("proxy-groups")
            .and_then(|v| v.as_sequence())
            .and_then(|groups| groups.first())
            .and_then(serde_yaml_ng::Value::as_mapping)
            .expect("группа на месте");
        assert_eq!(
            group.get("empty-fallback").and_then(serde_yaml_ng::Value::as_str),
            Some("DIRECT"),
            "выбор шаблона не переписываем"
        );
    }

    #[test]
    fn a_group_fed_by_an_inline_provider_is_not_treated_as_waiting() {
        let config: serde_yaml_ng::Mapping = serde_yaml_ng::from_str(
            "proxy-providers:\n  local:\n    type: inline\n    payload:\n      - {name: a, type: ss, server: 1.1.1.1, port: 1, cipher: aes-128-gcm, password: x}\nproxy-groups:\n  - name: Авто\n    type: select\n    use: [local]\n",
        )
        .expect("тестовый конфиг разбирается");

        let (config, rejected) = backfill_empty_groups(config);

        assert!(rejected.waiting.is_empty());
        assert!(rejected.emptied.is_empty());
        let group = config
            .get("proxy-groups")
            .and_then(|v| v.as_sequence())
            .and_then(|groups| groups.first())
            .and_then(serde_yaml_ng::Value::as_mapping)
            .expect("группа на месте");
        assert!(group.get("empty-fallback").is_none());
    }

    #[test]
    fn rule_sets_keep_their_route_through_a_group_that_is_only_waiting_for_its_provider() {
        let config: serde_yaml_ng::Mapping = serde_yaml_ng::from_str(
            "proxy-providers:\n  panel:\n    type: http\n    proxy: Авто\nrule-providers:\n  ads:\n    type: http\n    proxy: Авто\nproxy-groups:\n  - name: Авто\n    type: select\n    proxies: [DIRECT]\n    use: [panel]\n",
        )
        .expect("тестовый конфиг разбирается");

        let (config, rejected) = backfill_empty_groups(config);
        let config = unpin_providers_from_rejection(config, &rejected);

        let route_of = |section: &str, name: &str| {
            config
                .get(section)
                .and_then(serde_yaml_ng::Value::as_mapping)
                .and_then(|providers| providers.get(name))
                .and_then(serde_yaml_ng::Value::as_mapping)
                .and_then(|provider| provider.get("proxy"))
                .and_then(serde_yaml_ng::Value::as_str)
                .map(std::string::ToString::to_string)
        };
        assert_eq!(route_of("rule-providers", "ads").as_deref(), Some("Авто"));
        assert_eq!(route_of("proxy-providers", "panel"), None);
    }

    fn sentinel_pass(config: serde_yaml_ng::Mapping) -> (serde_yaml_ng::Mapping, super::SentinelReport) {
        let (config, report) = filter_sentinel_proxies(config);
        let (config, rejected) = backfill_empty_groups(cleanup_proxy_groups(config));
        (unpin_providers_from_rejection(config, &rejected), report)
    }

    fn group_members(config: &serde_yaml_ng::Mapping, name: &str) -> Vec<std::string::String> {
        config
            .get("proxy-groups")
            .and_then(|v| v.as_sequence())
            .expect("proxy-groups should be a sequence")
            .iter()
            .find(|group| group.get("name").and_then(serde_yaml_ng::Value::as_str) == Some(name))
            .and_then(|group| group.get("proxies"))
            .and_then(|v| v.as_sequence())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(std::borrow::ToOwned::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn proxy_names(config: &serde_yaml_ng::Mapping) -> Vec<std::string::String> {
        config
            .get("proxies")
            .and_then(|v| v.as_sequence())
            .expect("proxies should be a sequence")
            .iter()
            .filter_map(|item| {
                item.get("name")
                    .and_then(serde_yaml_ng::Value::as_str)
                    .map(std::borrow::ToOwned::to_owned)
            })
            .collect()
    }

    #[test]
    fn server_descriptions_are_collected_from_proxies() {
        let config = mapping(
            r#"
proxies:
  - name: "Netherlands 01"
    type: vless
    server: nl-01.example.com
    serverDescription: "10 Гбит · без лимита"
  - name: "Germany 02"
    type: vless
    server: de-02.example.com
    server_description: "  Для игр, низкий пинг  "
  - name: "USA 01"
    type: vless
    server: us-01.example.com
    server-description: "Netflix, Disney+"
  - name: "Turkey 01"
    type: vless
    server: tr-01.example.com
  - name: "Finland 03"
    type: vless
    server: fi-03.example.com
    serverDescription: "   "
"#,
        );

        let descriptions = collect_server_descriptions(&config);

        let text = |name: &str| descriptions.get(name).map(|value| value.as_str());

        assert_eq!(descriptions.len(), 3);
        assert_eq!(text("Netherlands 01"), Some("10 Гбит · без лимита"));
        assert_eq!(text("Germany 02"), Some("Для игр, низкий пинг"));
        assert_eq!(text("USA 01"), Some("Netflix, Disney+"));
        assert!(!descriptions.contains_key("Turkey 01"));
        assert!(!descriptions.contains_key("Finland 03"));
    }

    #[test]
    fn server_descriptions_are_empty_without_the_field() {
        let config = mapping(
            r#"
proxies:
  - name: "Netherlands 01"
    type: vless
    server: nl-01.example.com
"#,
        );

        assert!(collect_server_descriptions(&config).is_empty());
    }

    #[test]
    fn server_descriptions_survive_a_draft_that_was_never_committed() {
        let runtime = Draft::new(IRuntime::new());
        runtime.edit_draft(|draft| {
            draft.config = Some(mapping(
                r#"
proxies:
  - name: "Netherlands 01"
    type: vless
    server: nl-01.example.com
    serverDescription: "10 Гбит · без лимита"
"#,
            ));
        });

        let descriptions = server_descriptions_of(&runtime);

        assert_eq!(
            descriptions.get("Netherlands 01").map(|value| value.as_str()),
            Some("10 Гбит · без лимита")
        );
    }

    #[test]
    fn committed_server_descriptions_win_over_a_pending_draft() {
        let runtime = Draft::new(IRuntime::new());
        runtime.edit_draft(|draft| {
            draft.config = Some(mapping(
                r#"
proxies:
  - name: "Netherlands 01"
    type: vless
    serverDescription: "applied"
"#,
            ));
        });
        runtime.apply();
        runtime.edit_draft(|draft| {
            draft.config = Some(mapping(
                r#"
proxies:
  - name: "Netherlands 01"
    type: vless
    serverDescription: "not applied yet"
"#,
            ));
        });

        let descriptions = server_descriptions_of(&runtime);

        assert_eq!(
            descriptions.get("Netherlands 01").map(|value| value.as_str()),
            Some("applied")
        );
    }

    #[test]
    fn sentinel_proxies_are_dropped_and_empty_group_rejects() {
        let config = mapping(
            r#"
proxies:
  - name: "⌛ Subscription expired"
    type: vless
    server: 0.0.0.0
    port: 1
    uuid: 00000000-0000-0000-0000-000000000000
  - name: "Contact support"
    type: vless
    server: 0.0.0.0
    port: 1
    uuid: 00000000-0000-0000-0000-000000000000
proxy-groups:
  - name: "VPN"
    type: select
    proxies:
      - "⌛ Subscription expired"
      - "Contact support"
"#,
        );

        let (config, report) = sentinel_pass(config);

        assert!(proxy_names(&config).is_empty());
        assert_eq!(group_members(&config, "VPN"), vec!["REJECT".to_owned()]);
        assert!(report.only_sentinels);
        assert_eq!(report.remarks, vec!["⌛ Subscription expired", "Contact support"]);
    }

    #[test]
    fn sentinels_are_dropped_next_to_live_nodes() {
        let config = mapping(
            r#"
proxies:
  - name: "🇳🇱 Amsterdam"
    type: vless
    server: nl02.example.net
    port: 443
    uuid: 6f1c0f6d-1a2b-4c3d-8e9f-0a1b2c3d4e5f
  - name: "⌛ Subscription expired"
    type: vless
    server: 0.0.0.0
    port: 1
    uuid: 00000000-0000-0000-0000-000000000000
  - name: "🇩🇪 Frankfurt"
    type: vless
    server: de01.example.net
    port: 443
    uuid: 7a2d1e7e-2b3c-4d5e-9f0a-1b2c3d4e5f60
proxy-groups:
  - name: "VPN"
    type: select
    proxies:
      - "🇳🇱 Amsterdam"
      - "⌛ Subscription expired"
      - "🇩🇪 Frankfurt"
      - "DIRECT"
"#,
        );

        let (config, report) = sentinel_pass(config);

        assert!(!report.only_sentinels);
        assert_eq!(report.remarks, vec!["⌛ Subscription expired"]);
        assert_eq!(proxy_names(&config), vec!["🇳🇱 Amsterdam", "🇩🇪 Frankfurt"]);
        assert_eq!(
            group_members(&config, "VPN"),
            vec!["🇳🇱 Amsterdam", "🇩🇪 Frankfurt", "DIRECT"]
        );
    }

    #[test]
    fn empty_proxy_list_counts_as_no_servers() {
        let config = mapping(
            r#"
proxies: []
proxy-groups:
  - name: "VPN"
    type: select
    proxies:
      - "DIRECT"
"#,
        );

        let (_, report) = sentinel_pass(config);

        assert!(report.only_sentinels);
        assert!(report.remarks.is_empty());
    }

    #[test]
    fn real_proxies_survive_sentinel_filter() {
        let config = mapping(
            r#"
proxies:
  - name: "🇳🇱 Amsterdam"
    type: vless
    server: nl02.example.net
    port: 443
    uuid: 6f1c0f6d-1a2b-4c3d-8e9f-0a1b2c3d4e5f
  - name: "local-relay"
    type: socks5
    server: 127.0.0.1
    port: 40000
  - name: "shadow"
    type: ss
    server: ss.example.net
    port: 8388
    password: secret
  - name: "direct-out"
    type: direct
proxy-groups:
  - name: "VPN"
    type: select
    proxies:
      - "🇳🇱 Amsterdam"
      - "local-relay"
      - "shadow"
      - "direct-out"
"#,
        );

        let expected = proxy_names(&config);
        let (config, report) = sentinel_pass(config);

        assert_eq!(proxy_names(&config), expected);
        assert_eq!(group_members(&config, "VPN").len(), 4);
        assert!(!report.only_sentinels);
        assert!(report.remarks.is_empty());
    }

    #[test]
    fn provider_backed_group_is_left_alone() {
        let config = mapping(
            r#"
proxies:
  - name: "🚧 Subscription limited"
    type: vless
    server: 0.0.0.0
    port: 1
    uuid: 00000000-0000-0000-0000-000000000000
proxy-providers:
  main:
    type: http
    url: https://example.net/sub
proxy-groups:
  - name: "AUTO"
    type: url-test
    use:
      - main
    proxies:
      - "🚧 Subscription limited"
  - name: "ALL"
    type: select
    include-all: true
    proxies:
      - "🚧 Subscription limited"
"#,
        );

        let (config, report) = sentinel_pass(config);

        assert!(!report.only_sentinels);
        assert!(proxy_names(&config).is_empty());
        assert!(group_members(&config, "AUTO").is_empty());
        assert!(group_members(&config, "ALL").is_empty());
    }

    #[test]
    fn include_all_providers_without_providers_still_needs_reject() {
        let config = mapping(
            r#"
proxies:
  - name: "⌛ Expired"
    type: vless
    server: 0.0.0.0
    port: 1
    uuid: 00000000-0000-0000-0000-000000000000
proxy-groups:
  - name: "ALL"
    type: select
    include-all-providers: true
    proxies:
      - "⌛ Expired"
"#,
        );

        let (config, report) = sentinel_pass(config);

        assert!(report.only_sentinels);
        assert_eq!(group_members(&config, "ALL"), vec!["REJECT".to_owned()]);
    }

    #[test]
    fn group_using_a_ghost_provider_is_repaired() {
        let config = mapping(
            r#"
proxies: []
proxy-groups:
  - name: "VPN"
    type: select
    use:
      - nowhere
    proxies: []
"#,
        );

        let (config, _) = sentinel_pass(config);

        assert_eq!(group_members(&config, "VPN"), vec!["REJECT".to_owned()]);
    }

    #[test]
    fn stale_template_group_is_repaired_even_when_nothing_was_dropped() {
        let config = mapping(
            r#"
proxies: []
proxy-groups:
  - name: "→ Remnawave"
    type: select
    proxies:
      - "NL-01"
      - "DE-01"
"#,
        );

        let (config, report) = sentinel_pass(config);

        assert!(report.only_sentinels);
        assert_eq!(group_members(&config, "→ Remnawave"), vec!["REJECT".to_owned()]);
    }

    #[test]
    fn service_nodes_do_not_hide_the_no_servers_screen() {
        let config = mapping(
            r#"
proxies:
  - name: "DIRECT"
    type: direct
  - name: "dns-out"
    type: dns
  - name: "⌛ Subscription expired"
    type: vless
    server: 0.0.0.0
    port: 1
    uuid: 00000000-0000-0000-0000-000000000000
proxy-groups:
  - name: "🎲 Авто"
    type: select
    proxies:
      - "⌛ Subscription expired"
"#,
        );

        let (config, report) = sentinel_pass(config);

        assert!(
            report.only_sentinels,
            "only service nodes are left, so there are no servers"
        );
        assert!(!report.partially_dropped);
        assert_eq!(proxy_names(&config), vec!["DIRECT", "dns-out"]);
    }

    #[test]
    fn sentinels_are_dropped_inside_an_inline_provider() {
        let config = mapping(
            r#"
proxies: []
proxy-providers:
  bundled:
    type: inline
    payload:
      - name: "⌛ Expired"
        type: vless
        server: 0.0.0.0
        port: 1
        uuid: 00000000-0000-0000-0000-000000000000
      - name: "🇳🇱 Amsterdam"
        type: vless
        server: nl02.example.net
        port: 443
        uuid: 6f1c0f6d-1a2b-4c3d-8e9f-0a1b2c3d4e5f
proxy-groups:
  - name: "VPN"
    type: select
    use:
      - bundled
"#,
        );

        let (config, report) = sentinel_pass(config);

        let payload = config
            .get("proxy-providers")
            .and_then(|value| value.get("bundled"))
            .and_then(|value| value.get("payload"))
            .and_then(|value| value.as_sequence())
            .expect("payload should stay a sequence");
        assert_eq!(payload.len(), 1);
        assert_eq!(
            payload[0].get("name").and_then(serde_yaml_ng::Value::as_str),
            Some("🇳🇱 Amsterdam")
        );
        assert!(!report.only_sentinels, "the provider still has a live node");
        assert!(report.partially_dropped);
        assert_eq!(report.remarks, vec!["⌛ Expired"]);
    }

    #[test]
    fn the_dropped_count_is_not_capped_by_the_reported_names() {
        let config = mapping(
            r#"
proxies:
  - name: "⌛ 1"
    type: vless
    server: 0.0.0.0
    port: 1
    uuid: 00000000-0000-0000-0000-000000000000
  - name: "⌛ 2"
    type: vless
    server: 0.0.0.0
    port: 1
    uuid: 00000000-0000-0000-0000-000000000000
  - name: "⌛ 3"
    type: vless
    server: 0.0.0.0
    port: 1
    uuid: 00000000-0000-0000-0000-000000000000
  - name: "⌛ 4"
    type: vless
    server: 0.0.0.0
    port: 1
    uuid: 00000000-0000-0000-0000-000000000000
  - name: "⌛ 5"
    type: vless
    server: 0.0.0.0
    port: 1
    uuid: 00000000-0000-0000-0000-000000000000
  - name: "⌛ 6"
    type: vless
    server: 0.0.0.0
    port: 1
    uuid: 00000000-0000-0000-0000-000000000000
  - name: "🇳🇱 Amsterdam"
    type: vless
    server: nl02.example.net
    port: 443
    uuid: 6f1c0f6d-1a2b-4c3d-8e9f-0a1b2c3d4e5f
proxy-groups:
  - name: "🎲 Авто"
    type: select
    proxies:
      - "⌛ 1"
      - "🇳🇱 Amsterdam"
"#,
        );

        let (_, report) = sentinel_pass(config);

        assert!(report.partially_dropped);
        assert_eq!(report.remarks.len(), MAX_REPORTED_REMARKS);
        assert_eq!(report.dropped_total, 6);
    }

    #[test]
    fn nameless_sentinels_are_still_counted() {
        let config = mapping(
            r"
proxies:
  - type: vless
    server: 0.0.0.0
    port: 1
    uuid: 00000000-0000-0000-0000-000000000000
  - type: vless
    server: 0.0.0.0
    port: 1
    uuid: 00000000-0000-0000-0000-000000000000
  - type: vless
    server: 0.0.0.0
    port: 1
    uuid: 00000000-0000-0000-0000-000000000000
",
        );

        let (_, report) = sentinel_pass(config);

        assert_eq!(report.dropped_total, 3);
        assert!(report.remarks.is_empty());
    }

    #[test]
    fn a_name_shared_by_a_proxy_and_a_provider_entry_is_reported_once() {
        let config = mapping(
            r#"
proxies:
  - name: "⌛ Expired"
    type: vless
    server: 0.0.0.0
    port: 1
    uuid: 00000000-0000-0000-0000-000000000000
  - name: "🇳🇱 Amsterdam"
    type: vless
    server: nl02.example.net
    port: 443
    uuid: 6f1c0f6d-1a2b-4c3d-8e9f-0a1b2c3d4e5f
proxy-providers:
  bundled:
    type: inline
    payload:
      - name: "⌛ Expired"
        type: vless
        server: 0.0.0.0
        port: 1
        uuid: 00000000-0000-0000-0000-000000000000
      - name: "🇩🇪 Berlin"
        type: vless
        server: de01.example.net
        port: 443
        uuid: 6f1c0f6d-1a2b-4c3d-8e9f-0a1b2c3d4e60
proxy-groups:
  - name: "VPN"
    type: select
    use:
      - bundled
"#,
        );

        let (_, report) = sentinel_pass(config);

        assert_eq!(report.dropped_total, 2);
        assert_eq!(report.remarks, vec!["⌛ Expired"]);
    }

    #[test]
    fn a_remote_provider_still_suppresses_the_no_servers_screen() {
        let config = mapping(
            r#"
proxies: []
proxy-providers:
  main:
    type: http
    url: https://example.net/sub
proxy-groups:
  - name: "VPN"
    type: select
    use:
      - main
"#,
        );

        let (_, report) = sentinel_pass(config);

        assert!(!report.only_sentinels);
    }

    #[test]
    fn a_rejected_group_unpins_the_providers_that_download_through_it() {
        let config = mapping(
            r#"
proxies: []
proxy-groups:
  - name: "🎲 Авто"
    type: select
    proxies: []
  - name: "🚫 Недоступные сайты"
    type: select
    proxies:
      - "🎲 Авто"
      - "DIRECT"
rule-providers:
  ru-blocked:
    type: http
    behavior: domain
    url: https://example.net/ru.mrs
    proxy: "🚫 Недоступные сайты"
  local-list:
    type: http
    behavior: classical
    url: https://example.net/local.yaml
    proxy: "DIRECT"
proxy-providers:
  extra:
    type: http
    url: https://example.net/extra
    proxy: "🎲 Авто"
"#,
        );

        let (config, _) = sentinel_pass(config);

        assert_eq!(group_members(&config, "🎲 Авто"), vec!["REJECT".to_owned()]);
        assert!(
            config
                .get("rule-providers")
                .and_then(|value| value.get("ru-blocked"))
                .and_then(|value| value.get("proxy"))
                .is_none(),
            "a provider that downloads through a dead group must go direct"
        );
        assert_eq!(
            config
                .get("rule-providers")
                .and_then(|value| value.get("local-list"))
                .and_then(|value| value.get("proxy"))
                .and_then(serde_yaml_ng::Value::as_str),
            Some("DIRECT"),
            "a healthy path must be left alone"
        );
        assert!(
            config
                .get("proxy-providers")
                .and_then(|value| value.get("extra"))
                .and_then(|value| value.get("proxy"))
                .is_none()
        );
    }

    #[test]
    fn a_live_subscription_leaves_provider_pins_alone() {
        let config = mapping(
            r#"
proxies:
  - name: "🇳🇱 Amsterdam"
    type: vless
    server: nl02.example.net
    port: 443
    uuid: 6f1c0f6d-1a2b-4c3d-8e9f-0a1b2c3d4e5f
proxy-groups:
  - name: "🎲 Авто"
    type: select
    proxies:
      - "🇳🇱 Amsterdam"
  - name: "🚫 Недоступные сайты"
    type: select
    proxies:
      - "🎲 Авто"
rule-providers:
  ru-blocked:
    type: http
    behavior: domain
    url: https://example.net/ru.mrs
    proxy: "🚫 Недоступные сайты"
"#,
        );

        let expected = config.clone();
        let (config, report) = sentinel_pass(config);

        assert_eq!(config, expected, "a live config must not be touched");
        assert!(!report.only_sentinels);
        assert!(!report.partially_dropped);
    }

    #[test]
    fn a_cycle_between_groups_does_not_unpin_providers() {
        let config = mapping(
            r#"
proxies: []
proxy-groups:
  - name: "dead"
    type: select
    proxies: []
  - name: "a"
    type: select
    proxies:
      - "b"
  - name: "b"
    type: select
    proxies:
      - "a"
rule-providers:
  looped:
    type: http
    behavior: domain
    url: https://example.net/list.mrs
    proxy: "a"
"#,
        );

        let (config, _) = sentinel_pass(config);

        assert_eq!(
            config
                .get("rule-providers")
                .and_then(|value| value.get("looped"))
                .and_then(|value| value.get("proxy"))
                .and_then(serde_yaml_ng::Value::as_str),
            Some("a")
        );
    }

    #[test]
    fn an_expired_inline_payload_is_kept_so_the_core_still_starts() {
        let config = mapping(
            r#"
proxies: []
proxy-providers:
  bundled:
    type: inline
    payload:
      - name: "⌛ Expired"
        type: vless
        server: 0.0.0.0
        port: 1
        uuid: 00000000-0000-0000-0000-000000000000
proxy-groups:
  - name: "VPN"
    type: select
    use:
      - bundled
"#,
        );

        let expected = config.clone();
        let (config, report) = sentinel_pass(config);

        assert_eq!(
            config.get("proxy-providers"),
            expected.get("proxy-providers"),
            "an emptied payload makes the core reject the whole config"
        );
        assert!(report.only_sentinels);
        assert!(report.remarks.is_empty());
        assert!(!report.partially_dropped);
    }

    #[test]
    fn a_file_provider_also_suppresses_the_no_servers_screen() {
        let config = mapping(
            r#"
proxies: []
proxy-providers:
  bundled:
    type: file
    path: ./bundled.yaml
proxy-groups:
  - name: "VPN"
    type: select
    use:
      - bundled
"#,
        );

        let (_, report) = sentinel_pass(config);

        assert!(
            !report.only_sentinels,
            "the core reads those nodes from a file we cannot look into"
        );
    }

    #[test]
    fn a_fallback_group_with_a_live_member_keeps_the_provider_pins() {
        let config = mapping(
            r#"
proxies:
  - name: "🇳🇱 Amsterdam"
    type: vless
    server: nl02.example.net
    port: 443
    uuid: 6f1c0f6d-1a2b-4c3d-8e9f-0a1b2c3d4e5f
proxy-groups:
  - name: "🎲 Авто"
    type: select
    proxies: []
  - name: "🛡 Резерв"
    type: fallback
    proxies:
      - "🎲 Авто"
      - "🇳🇱 Amsterdam"
  - name: "🕳 Пусто"
    type: fallback
    proxies:
      - "🎲 Авто"
rule-providers:
  ru-blocked:
    type: http
    behavior: domain
    url: https://example.net/ru.mrs
    proxy: "🛡 Резерв"
  ua-blocked:
    type: http
    behavior: domain
    url: https://example.net/ua.mrs
    proxy: "🕳 Пусто"
"#,
        );

        let (config, _) = sentinel_pass(config);

        assert_eq!(group_members(&config, "🎲 Авто"), vec!["REJECT".to_owned()]);
        assert_eq!(
            config
                .get("rule-providers")
                .and_then(|value| value.get("ru-blocked"))
                .and_then(|value| value.get("proxy"))
                .and_then(serde_yaml_ng::Value::as_str),
            Some("🛡 Резерв"),
            "a fallback group picks its outlet by health, so a dead member does not kill it"
        );
        assert!(
            config
                .get("rule-providers")
                .and_then(|value| value.get("ua-blocked"))
                .and_then(|value| value.get("proxy"))
                .is_none(),
            "every member of that fallback group is dead"
        );
    }

    #[test]
    fn a_compatible_member_is_dropped_from_groups() {
        let config = mapping(
            r#"
proxies:
  - name: "🇳🇱 Amsterdam"
    type: vless
    server: nl02.example.net
    port: 443
    uuid: 6f1c0f6d-1a2b-4c3d-8e9f-0a1b2c3d4e5f
proxy-groups:
  - name: "VPN"
    type: select
    proxies:
      - "🇳🇱 Amsterdam"
      - "COMPATIBLE"
      - "DIRECT"
"#,
        );

        let (config, _) = sentinel_pass(config);

        assert_eq!(
            group_members(&config, "VPN"),
            vec!["🇳🇱 Amsterdam".to_owned(), "DIRECT".to_owned()],
            "COMPATIBLE is a silent DIRECT, a subscription must not smuggle it into a group"
        );
    }

    #[test]
    fn key_based_nodes_are_not_sentinels() {
        let config = mapping(
            r#"
proxies:
  - name: "WG relay"
    type: wireguard
    server: relay.example.net
    port: 1
    private-key: aGVsbG8gd29ybGQgdGhpcyBpcyBhIGtleQ==
proxy-groups:
  - name: "VPN"
    type: select
    proxies:
      - "WG relay"
"#,
        );

        let (config, report) = sentinel_pass(config);

        assert!(!report.only_sentinels);
        assert_eq!(group_members(&config, "VPN"), vec!["WG relay".to_owned()]);
    }
}
