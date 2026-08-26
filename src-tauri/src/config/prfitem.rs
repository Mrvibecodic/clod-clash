use crate::{
    config::{chan, profiles, sub_headers},
    utils::{
        dirs, help,
        network::{NetworkManager, ProxyType},
        tmpl,
    },
};
use anyhow::{Context as _, Result, bail};
use reqwest_dav::re_exports::url::form_urlencoded;
use serde::{Deserialize, Serialize};
use serde_yaml_ng::Mapping;
use smartstring::alias::String;
use std::time::Duration;
use tauri::Url;
use tokio::fs;

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct PrfItem {
    pub uid: Option<String>,

    #[serde(rename = "type")]
    pub itype: Option<String>,

    pub name: Option<String>,

    pub file: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub desc: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected: Option<Vec<PrfSelected>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub favorites: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra: Option<PrfExtra>,

    pub updated: Option<usize>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub option: Option<PrfOption>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub home: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub support_url: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub logo: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub announce: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub announce_url: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub portal_url: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub bot_url: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub monitor_url: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub guide_url: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub promo: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub promo_url: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub promo_seen: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub lock_mode: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub show_zero_hosts: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency_style: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub disable_ping: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_remove_url: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub connect_mode: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub refill_date: Option<i64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub clock_skew: Option<i64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub clock_skew_at: Option<i64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub interval_locked: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback_url: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback_domain: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_urls: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub migration_hops: Option<u32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub hwid_state: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub name_customized: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub notify_expire_days: Option<Vec<u32>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub notify_traffic_percent: Option<Vec<u32>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub notified: Option<std::collections::BTreeMap<std::string::String, i64>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_fallback: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub simple_mode: Option<bool>,

    #[serde(skip)]
    pub name_from_header: Option<bool>,

    #[serde(skip)]
    pub migrate_url: Option<String>,

    #[serde(skip)]
    pub hwid_max_devices: Option<u32>,
    #[serde(skip)]
    pub file_data: Option<String>,
}

#[derive(Default, Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct PrfSelected {
    pub name: Option<String>,
    pub now: Option<String>,
}

#[derive(Default, Debug, Clone, Copy, Deserialize, Serialize)]
pub struct PrfExtra {
    pub upload: u64,
    pub download: u64,
    pub total: u64,
    pub expire: u64,
}

#[derive(Default, Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct PrfOption {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub with_proxy: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub self_proxy: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub update_interval: Option<u64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_seconds: Option<u64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub secure: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub chan_pin: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub danger_accept_invalid_certs: Option<bool>,

    #[serde(default = "default_allow_auto_update")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_auto_update: Option<bool>,

    pub merge: Option<String>,

    pub script: Option<String>,

    pub rules: Option<String>,

    pub proxies: Option<String>,

    pub groups: Option<String>,
}

impl PrfOption {
    pub fn merge(one: Option<&Self>, other: Option<&Self>) -> Option<Self> {
        match (one, other) {
            (Some(a_ref), Some(b_ref)) => {
                let mut result = a_ref.clone();
                result.user_agent = b_ref.user_agent.clone().or(result.user_agent);
                result.with_proxy = b_ref.with_proxy.or(result.with_proxy);
                result.self_proxy = b_ref.self_proxy.or(result.self_proxy);
                result.danger_accept_invalid_certs =
                    b_ref.danger_accept_invalid_certs.or(result.danger_accept_invalid_certs);
                result.allow_auto_update = b_ref.allow_auto_update.or(result.allow_auto_update);
                result.update_interval = b_ref.update_interval.or(result.update_interval);
                result.merge = b_ref.merge.clone().or(result.merge);
                result.script = b_ref.script.clone().or(result.script);
                result.rules = b_ref.rules.clone().or(result.rules);
                result.proxies = b_ref.proxies.clone().or(result.proxies);
                result.groups = b_ref.groups.clone().or(result.groups);
                result.timeout_seconds = b_ref.timeout_seconds.or(result.timeout_seconds);
                result.secure = match (result.secure, b_ref.secure) {
                    (Some(true), _) | (_, Some(true)) => Some(true),
                    (a, b) => b.or(a),
                };
                result.chan_pin = b_ref.chan_pin.clone().or(result.chan_pin);
                Some(result)
            }
            (Some(a_ref), None) => Some(a_ref.clone()),
            (None, Some(b_ref)) => Some(b_ref.clone()),
            (None, None) => None,
        }
    }
}

impl PrfItem {
    fn is_worth_retrying_over_proxy(err: &anyhow::Error) -> bool {
        let text = err.to_string();
        !text.contains("(x-hwid)") && !text.contains("invalid profile item type")
    }

    pub async fn from_url_with_ladder(
        url: &str,
        name: Option<&String>,
        desc: Option<&String>,
        option: Option<&PrfOption>,
    ) -> Result<Self> {
        let url = url.to_owned();
        let mut attempt = option.cloned();
        let mut last_err = match Self::from_url(&url, name, desc, attempt.as_ref()).await {
            Ok(item) => return Ok(item),
            Err(err) => err,
        };

        for (self_proxy, with_proxy) in [(true, false), (false, true)] {
            if !Self::is_worth_retrying_over_proxy(&last_err) {
                break;
            }
            let opt = attempt.get_or_insert_with(PrfOption::default);
            opt.self_proxy = Some(self_proxy);
            opt.with_proxy = Some(with_proxy);

            match Self::from_url(&url, name, desc, attempt.as_ref()).await {
                Ok(item) => return Ok(item),
                Err(err) => last_err = err,
            }
        }

        Err(last_err)
    }

    pub async fn from(item: &Self, file_data: Option<String>) -> Result<Self> {
        if item.itype.is_none() {
            bail!("type should not be null");
        }

        let itype = item
            .itype
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("type should not be null"))?;
        match itype.as_str() {
            "remote" => {
                let url = item
                    .url
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("url should not be null"))?;
                let name = item.name.as_ref();
                let desc = item.desc.as_ref();
                let option = item.option.as_ref();
                Self::from_url_with_ladder(url, name, desc, option).await
            }
            "local" => {
                let name = item.name.clone().unwrap_or_else(|| "Local File".into());
                let desc = item.desc.clone().unwrap_or_else(|| "".into());
                let option = item.option.as_ref();
                Self::from_local(name, desc, file_data, option).await
            }
            typ => bail!("invalid profile item type \"{typ}\""),
        }
    }

    pub async fn from_local(
        name: String,
        desc: String,
        file_data: Option<String>,
        option: Option<&PrfOption>,
    ) -> Result<Self> {
        let uid = help::get_uid("L").into();
        let file = format!("{uid}.yaml").into();
        let opt_ref = option.as_ref();
        let update_interval = opt_ref.and_then(|o| o.update_interval);
        let mut merge = opt_ref.and_then(|o| o.merge.clone());
        let mut script = opt_ref.and_then(|o| o.script.clone());
        let mut rules = opt_ref.and_then(|o| o.rules.clone());
        let mut proxies = opt_ref.and_then(|o| o.proxies.clone());
        let mut groups = opt_ref.and_then(|o| o.groups.clone());

        if merge.is_none() {
            let merge_item = &mut Self::from_merge(None)?;
            profiles::profiles_append_item_safe(merge_item).await?;
            merge = merge_item.uid.clone();
        }
        if script.is_none() {
            let script_item = &mut Self::from_script(None)?;
            profiles::profiles_append_item_safe(script_item).await?;
            script = script_item.uid.clone();
        }
        if rules.is_none() {
            let rules_item = &mut Self::from_rules()?;
            profiles::profiles_append_item_safe(rules_item).await?;
            rules = rules_item.uid.clone();
        }
        if proxies.is_none() {
            let proxies_item = &mut Self::from_proxies()?;
            profiles::profiles_append_item_safe(proxies_item).await?;
            proxies = proxies_item.uid.clone();
        }
        if groups.is_none() {
            let groups_item = &mut Self::from_groups()?;
            profiles::profiles_append_item_safe(groups_item).await?;
            groups = groups_item.uid.clone();
        }
        Ok(Self {
            uid: Some(uid),
            itype: Some("local".into()),
            name: Some(name),
            desc: Some(desc),
            file: Some(file),
            url: None,
            selected: None,
            extra: None,
            option: Some(PrfOption {
                update_interval,
                merge,
                script,
                rules,
                proxies,
                groups,
                ..PrfOption::default()
            }),
            home: None,
            updated: Some(chrono::Local::now().timestamp() as usize),
            file_data: Some(file_data.unwrap_or_else(|| tmpl::ITEM_LOCAL.into())),
            ..Self::default()
        })
    }

    pub async fn from_url(
        url: &str,
        name: Option<&String>,
        desc: Option<&String>,
        option: Option<&PrfOption>,
    ) -> Result<Self> {
        let with_proxy = option.is_some_and(|o| o.with_proxy.unwrap_or(false));
        let self_proxy = option.is_some_and(|o| o.self_proxy.unwrap_or(false));
        let accept_invalid_certs = option.is_some_and(|o| o.danger_accept_invalid_certs.unwrap_or(false));
        let allow_auto_update = Some(allow_auto_update_enabled(option));
        let user_agent = option.and_then(|o| o.user_agent.clone());
        let update_interval = option.and_then(|o| o.update_interval);
        let timeout = option.and_then(|o| o.timeout_seconds).unwrap_or(20);
        let mut merge = option.and_then(|o| o.merge.clone());
        let mut script = option.and_then(|o| o.script.clone());
        let mut rules = option.and_then(|o| o.rules.clone());
        let mut proxies = option.and_then(|o| o.proxies.clone());
        let mut groups = option.and_then(|o| o.groups.clone());

        let proxy_type = if self_proxy {
            ProxyType::Localhost
        } else if with_proxy {
            ProxyType::System
        } else {
            ProxyType::None
        };

        let url = fix_dirty_url(url)?;

        let identity_headers = sub_headers::build_identity_headers().await;

        let (resp, learned_pin) = fetch_for_profile(
            url.as_str(),
            proxy_type,
            timeout,
            user_agent.clone(),
            accept_invalid_certs,
            &identity_headers,
            option,
        )
        .await?;

        let answered_at = chrono::Local::now().timestamp();

        let sub = sub_headers::SubHeaders::parse(resp.headers());
        log_panel_headers(&sub);
        sub.notify_device_state();

        let status_code = resp.status();
        if !status_code.is_success() {
            bail!("failed to fetch remote profile with status {status_code}")
        }

        let header = resp.headers();

        let extra = parse_subscription_userinfo(header);

        let filename = match header.get("Content-Disposition") {
            Some(value) => {
                let filename = format!("{value:?}");
                let filename = filename.trim_matches('"');
                match help::parse_str::<String>(filename, "filename*") {
                    Some(filename) => {
                        let iter = percent_encoding::percent_decode(filename.as_bytes());
                        let filename = iter.decode_utf8().unwrap_or_default();
                        filename.split("''").last().map(|s| s.into())
                    }
                    None => match help::parse_str::<String>(filename, "filename") {
                        Some(filename) => {
                            let filename = filename.trim_matches('"');
                            Some(filename.into())
                        }
                        None => None,
                    },
                }
            }
            None => {
                Some(crate::utils::help::get_last_part_and_decode(url.as_str()).unwrap_or_else(|| "Remote File".into()))
            }
        };
        let (update_interval, interval_locked) = match update_interval {
            Some(val) => (Some(val), None),
            None => match sub.update_interval_hours {
                Some(hours) => (Some(hours.saturating_mul(60)), Some(true)),
                None => (None, None),
            },
        };

        let home = sub.home.clone();

        let uid = help::get_uid("R").into();
        let file = format!("{uid}.yaml").into();
        let (name, name_from_header) = match name {
            Some(user_name) => (user_name.to_owned(), None),
            None => match sub.profile_title.clone() {
                Some(title) => (title, Some(true)),
                None => (
                    filename.map(Into::into).unwrap_or_else(|| String::from("Remote File")),
                    None,
                ),
            },
        };
        let data = resp.text_with_charset()?;

        let data = data.trim_start_matches('\u{feff}');

        let yaml = serde_yaml_ng::from_str::<Mapping>(data).context("the remote profile data is invalid yaml")?;

        if !yaml.contains_key("proxies") && !yaml.contains_key("proxy-providers") {
            bail!("profile does not contain `proxies` or `proxy-providers`");
        }

        if merge.is_none() {
            let merge_item = &mut Self::from_merge(None)?;
            profiles::profiles_append_item_safe(merge_item).await?;
            merge = merge_item.uid.clone();
        }
        if script.is_none() {
            let script_item = &mut Self::from_script(None)?;
            profiles::profiles_append_item_safe(script_item).await?;
            script = script_item.uid.clone();
        }
        if rules.is_none() {
            let rules_item = &mut Self::from_rules()?;
            profiles::profiles_append_item_safe(rules_item).await?;
            rules = rules_item.uid.clone();
        }
        if proxies.is_none() {
            let proxies_item = &mut Self::from_proxies()?;
            profiles::profiles_append_item_safe(proxies_item).await?;
            proxies = proxies_item.uid.clone();
        }
        if groups.is_none() {
            let groups_item = &mut Self::from_groups()?;
            profiles::profiles_append_item_safe(groups_item).await?;
            groups = groups_item.uid.clone();
        }

        const MAX_SKEW_SECS: i64 = 366 * 24 * 60 * 60;
        let measured_skew = sub
            .server_time
            .map(|panel| panel - answered_at)
            .filter(|skew| skew.abs() <= MAX_SKEW_SECS);

        Ok(Self {
            uid: Some(uid),
            itype: Some("remote".into()),
            name: Some(name),
            desc: desc.cloned(),
            file: Some(file),
            url: Some(url.as_str().into()),
            selected: None,
            favorites: None,
            group: None,
            extra,
            option: Some(PrfOption {
                update_interval,
                merge,
                script,
                rules,
                proxies,
                groups,
                allow_auto_update,
                secure: option.and_then(|o| o.secure).filter(|on| *on),
                chan_pin: learned_pin.or_else(|| option.and_then(|o| o.chan_pin.clone())),
                ..PrfOption::default()
            }),
            home,
            support_url: sub.support_url.clone(),
            logo: sub.profile_logo.clone(),
            announce: sub.announce.clone(),
            announce_url: sub.announce_url.clone(),
            portal_url: sub.portal_url.clone(),
            bot_url: sub.bot_url.clone(),
            monitor_url: sub.monitor_url.clone(),
            guide_url: sub.guide_url.clone(),
            promo: sub.promo.clone(),
            promo_url: sub.promo_url.clone(),
            promo_seen: None,
            lock_mode: sub.lock_mode,
            connect_mode: sub.connect_mode.map(|mode| mode.as_str().into()),
            latency_style: sub.latency_style.map(|style| style.as_str().into()),
            disable_ping: sub.disable_ping.then_some(true),
            device_remove_url: sub.device_remove_url.clone(),
            show_zero_hosts: sub.show_zero_hosts,
            refill_date: sub.refill_date,
            clock_skew: measured_skew,
            clock_skew_at: measured_skew.map(|_| answered_at),
            interval_locked,
            fallback_url: sub.fallback_url.clone(),
            fallback_domain: sub.fallback_domain.clone(),
            previous_urls: None,
            migration_hops: None,
            hwid_state: sub.hwid_state.as_str().map(Into::into),
            name_customized: None,
            notify_expire_days: sub.notify_expire_days.clone(),
            notify_traffic_percent: sub.notify_traffic_percent.clone(),
            notified: None,
            from_fallback: None,
            simple_mode: sub.simple_mode,
            name_from_header,
            migrate_url: sub.migration_target(url.as_str()),
            hwid_max_devices: sub.hwid_max_devices,
            updated: Some(chrono::Local::now().timestamp() as usize),
            file_data: Some(data.into()),
        })
    }

    pub fn from_merge(uid: Option<String>) -> Result<Self> {
        let (id, template) = if let Some(uid) = uid {
            (uid, tmpl::ITEM_MERGE.into())
        } else {
            (help::get_uid("m").into(), tmpl::ITEM_MERGE_EMPTY.into())
        };
        let file = format!("{id}.yaml").into();

        Ok(Self {
            uid: Some(id),
            itype: Some("merge".into()),
            file: Some(file),
            updated: Some(chrono::Local::now().timestamp() as usize),
            file_data: Some(template),
            ..Default::default()
        })
    }

    pub fn from_script(uid: Option<String>) -> Result<Self> {
        let id = if let Some(uid) = uid {
            uid
        } else {
            help::get_uid("s").into()
        };
        let file = format!("{id}.js").into();
        Ok(Self {
            uid: Some(id),
            itype: Some("script".into()),
            file: Some(file),
            updated: Some(chrono::Local::now().timestamp() as usize),
            file_data: Some(tmpl::ITEM_SCRIPT.into()),
            ..Default::default()
        })
    }

    pub fn from_rules() -> Result<Self> {
        let uid = help::get_uid("r").into();
        let file = format!("{uid}.yaml").into();

        Ok(Self {
            uid: Some(uid),
            itype: Some("rules".into()),
            file: Some(file),
            updated: Some(chrono::Local::now().timestamp() as usize),
            file_data: Some(tmpl::ITEM_RULES.into()),
            ..Default::default()
        })
    }

    pub fn from_proxies() -> Result<Self> {
        let uid = help::get_uid("p").into();
        let file = format!("{uid}.yaml").into();

        Ok(Self {
            uid: Some(uid),
            itype: Some("proxies".into()),
            file: Some(file),
            updated: Some(chrono::Local::now().timestamp() as usize),
            file_data: Some(tmpl::ITEM_PROXIES.into()),
            ..Default::default()
        })
    }

    pub fn from_groups() -> Result<Self> {
        let uid = help::get_uid("g").into();
        let file = format!("{uid}.yaml").into();

        Ok(Self {
            uid: Some(uid),
            itype: Some("groups".into()),
            file: Some(file),
            updated: Some(chrono::Local::now().timestamp() as usize),
            file_data: Some(tmpl::ITEM_GROUPS.into()),
            ..Default::default()
        })
    }

    pub async fn read_file(&self) -> Result<String> {
        let file = self
            .file
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("could not find the file"))?;
        let path = dirs::app_profiles_dir()?.join(file.as_str());
        let content = fs::read_to_string(&path)
            .await
            .with_context(|| format!("failed to read the file \"{}\"", path.display()))?;
        Ok(content.into())
    }

    pub async fn save_file(&self, data: String) -> Result<()> {
        let file = self
            .file
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("could not find the file"))?;
        let path = dirs::app_profiles_dir()?.join(file.as_str());
        help::write_atomic(&path, data.as_bytes())
            .await
            .context("failed to save the file")
    }
}

fn parse_subscription_userinfo(headers: &reqwest::header::HeaderMap) -> Option<PrfExtra> {
    for (k, v) in headers.iter() {
        let key_lower = k.as_str().to_ascii_lowercase();
        if !key_lower
            .strip_suffix("subscription-userinfo")
            .is_some_and(|prefix| prefix.is_empty() || prefix.ends_with('-'))
        {
            continue;
        }
        let raw_info = match v.to_str() {
            Ok(text) => std::borrow::Cow::Borrowed(text),
            Err(_) => std::string::String::from_utf8_lossy(v.as_bytes()),
        };
        let sub_info: &str = raw_info.as_ref();
        return Some(PrfExtra {
            upload: help::parse_str(sub_info, "upload").unwrap_or(0),
            download: help::parse_str(sub_info, "download").unwrap_or(0),
            total: help::parse_str(sub_info, "total").unwrap_or(0),
            expire: to_unix_seconds(help::parse_str(sub_info, "expire").unwrap_or(0)),
        });
    }
    None
}

const FETCH_HEAD_START: Duration = Duration::from_millis(250);

async fn fetch_once(
    url: &str,
    proxy_type: ProxyType,
    timeout: u64,
    user_agent: Option<String>,
    accept_invalid_certs: bool,
    headers: &reqwest::header::HeaderMap,
) -> Result<crate::utils::network::HttpResponse> {
    NetworkManager::new()
        .get_with_interrupt_and_headers(
            url,
            proxy_type,
            Some(timeout),
            user_agent,
            accept_invalid_certs,
            Some(headers),
        )
        .await
}

async fn fetch_subscription(
    url: &str,
    preferred: ProxyType,
    timeout: u64,
    user_agent: Option<String>,
    accept_invalid_certs: bool,
    headers: &reqwest::header::HeaderMap,
) -> Result<crate::utils::network::HttpResponse> {
    if matches!(preferred, ProxyType::None) {
        return fetch_once(url, ProxyType::None, timeout, user_agent, accept_invalid_certs, headers).await;
    }

    let chosen = std::pin::pin!(fetch_once(
        url,
        preferred,
        timeout,
        user_agent.clone(),
        accept_invalid_certs,
        headers
    ));
    let direct = std::pin::pin!(async {
        tokio::time::sleep(FETCH_HEAD_START).await;
        fetch_once(url, ProxyType::None, timeout, user_agent, accept_invalid_certs, headers).await
    });
    let (mut chosen, mut direct) = (chosen, direct);

    let (mut chosen_done, mut direct_done) = (false, false);
    let (mut chosen_error, mut direct_error) = (None, None);

    while !(chosen_done && direct_done) {
        tokio::select! {
            biased;
            result = &mut chosen, if !chosen_done => {
                chosen_done = true;
                match result {
                    Ok(response) => return Ok(response),
                    Err(e) => {
                        clash_verge_logging::logging!(
                            info,
                            clash_verge_logging::Type::Config,
                            "[clod] subscription fetch failed on the chosen route, waiting for the direct one: {e}"
                        );
                        chosen_error = Some(e);
                    }
                }
            }
            result = &mut direct, if !direct_done => {
                direct_done = true;
                match result {
                    Ok(response) => {
                        clash_verge_logging::logging!(
                            info,
                            clash_verge_logging::Type::Config,
                            "[clod] subscription answered on the direct route"
                        );
                        return Ok(response);
                    }
                    Err(e) => direct_error = Some(e),
                }
            }
        }
    }

    Err(chosen_error
        .or(direct_error)
        .unwrap_or_else(|| anyhow::anyhow!("subscription fetch produced no result")))
}

async fn fetch_for_profile(
    url: &str,
    proxy_type: ProxyType,
    timeout: u64,
    user_agent: Option<String>,
    accept_invalid_certs: bool,
    identity_headers: &reqwest::header::HeaderMap,
    option: Option<&PrfOption>,
) -> Result<(crate::utils::network::HttpResponse, Option<String>)> {
    let secure = option.is_some_and(|o| o.secure.unwrap_or(false));

    if !secure {
        return match fetch_subscription(
            url,
            proxy_type,
            timeout,
            user_agent,
            accept_invalid_certs,
            identity_headers,
        )
        .await
        {
            Ok(response) => Ok((response, None)),
            Err(e) => {
                tokio::time::sleep(Duration::from_millis(100)).await;
                Err(e).context("failed to fetch remote profile")
            }
        };
    }

    let pinned = option.and_then(|o| o.chan_pin.as_ref()).and_then(|raw| {
        use base64::Engine as _;
        let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(raw.as_str())
            .ok()?;
        <[u8; 32]>::try_from(decoded).ok()
    });

    let mut outcome = fetch_secure(
        url,
        proxy_type,
        timeout,
        user_agent.clone(),
        accept_invalid_certs,
        identity_headers,
        pinned,
    )
    .await;

    if outcome.is_err() && pinned.is_some() {
        clash_verge_logging::logging!(
            warn,
            clash_verge_logging::Type::Config,
            "[clod] chan: закреплённый ключ прослойки не принят, повтор без закрепления"
        );

        outcome = fetch_secure(
            url,
            proxy_type,
            timeout,
            user_agent,
            accept_invalid_certs,
            identity_headers,
            None,
        )
        .await;
    }

    match outcome {
        Ok((response, pin)) => {
            use base64::Engine as _;
            let pin = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(pin);
            Ok((response, Some(String::from(pin.as_str()))))
        }
        Err(e) => {
            tokio::time::sleep(Duration::from_millis(100)).await;
            Err(e).context("failed to fetch remote profile over the secure channel")
        }
    }
}

const CHAN_NEUTRAL_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64)";

async fn fetch_secure(
    url: &str,
    proxy_type: ProxyType,
    timeout: u64,
    user_agent: Option<String>,
    accept_invalid_certs: bool,
    identity: &reqwest::header::HeaderMap,
    pin: Option<[u8; 32]>,
) -> Result<(crate::utils::network::HttpResponse, [u8; 32])> {
    let get = |name: &str| -> std::string::String {
        identity
            .get(name)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_owned()
    };

    let fields = chan::Fields {
        hwid: get("x-hwid"),
        os: get("x-device-os"),
        osv: get("x-ver-os"),
        model: get("x-device-model"),
        ua: user_agent
            .clone()
            .map_or_else(|| crate::utils::hwid::user_agent().to_string(), |ua| ua.to_string()),
        acc: "*/*".to_owned(),
        q: std::string::String::new(),
    };

    let now = chrono::Local::now().timestamp();
    let (secure_url, session) = chan::build(url, pin, &fields, now)?;

    let response = fetch_subscription(
        secure_url.as_str(),
        proxy_type,
        timeout,
        Some(CHAN_NEUTRAL_UA.into()),
        accept_invalid_certs,
        &reqwest::header::HeaderMap::new(),
    )
    .await?;

    if !response.status().is_success() {
        bail!(
            "clod-chan: прослойка не приняла защищённый запрос ({})",
            response.status()
        );
    }

    let answer = session.open(response.text_with_charset()?, chrono::Local::now().timestamp())?;

    let mut headers = reqwest::header::HeaderMap::new();
    for (name, values) in &answer.meta {
        let Ok(name) = reqwest::header::HeaderName::from_bytes(name.as_bytes()) else {
            continue;
        };
        for value in values {
            if let Ok(value) = reqwest::header::HeaderValue::from_str(value) {
                headers.append(name.clone(), value);
            }
        }
    }

    Ok((
        crate::utils::network::HttpResponse::new(
            reqwest::StatusCode::from_u16(answer.status).unwrap_or(reqwest::StatusCode::OK),
            headers,
            answer.body.into(),
        ),
        answer.sp,
    ))
}

const MILLIS_THRESHOLD: u64 = 1_000_000_000_000;

const fn to_unix_seconds(ts: u64) -> u64 {
    if ts > MILLIS_THRESHOLD { ts / 1000 } else { ts }
}

fn log_panel_headers(sub: &sub_headers::SubHeaders) {
    clash_verge_logging::logging!(
        info,
        clash_verge_logging::Type::Config,
        "[clod] panel headers in response: title={} logo={} announce={} promo={} portal={} hwid_limit={} lock={} show0hosts={} simple={}",
        sub.profile_title.is_some(),
        sub.profile_logo.is_some(),
        sub.announce.is_some(),
        sub.promo.is_some(),
        sub.portal_url.is_some(),
        sub.hwid_limit_message.is_some(),
        sub.lock_mode.is_some(),
        sub.show_zero_hosts.is_some(),
        sub.simple_mode.is_some()
    );
}

impl PrfItem {
    pub fn merge_panel_meta(&mut self, fresh: &Self) {
        if fresh.name_from_header == Some(true) && self.name_customized != Some(true) && fresh.name.is_some() {
            self.name = fresh.name.clone();
        }

        self.support_url = fresh.support_url.clone();
        self.logo = fresh.logo.clone();
        self.announce_url = fresh.announce_url.clone();
        self.refill_date = fresh.refill_date;
        self.interval_locked = fresh.interval_locked;
        self.fallback_url = fresh.fallback_url.clone();
        self.fallback_domain = fresh.fallback_domain.clone();
        self.hwid_state = fresh.hwid_state.clone();
        if fresh.clock_skew.is_some() {
            self.clock_skew = fresh.clock_skew;
            self.clock_skew_at = fresh.clock_skew_at;
        }
        self.notify_expire_days = fresh.notify_expire_days.clone();
        self.notify_traffic_percent = fresh.notify_traffic_percent.clone();
        self.from_fallback = fresh.from_fallback;
        self.simple_mode = fresh.simple_mode;
        self.portal_url = fresh.portal_url.clone();
        self.bot_url = fresh.bot_url.clone();
        self.monitor_url = fresh.monitor_url.clone();
        self.guide_url = fresh.guide_url.clone();
        self.lock_mode = fresh.lock_mode;
        self.connect_mode = fresh.connect_mode.clone();
        self.latency_style = fresh.latency_style.clone();
        self.disable_ping = fresh.disable_ping;
        self.device_remove_url = fresh.device_remove_url.clone();
        self.show_zero_hosts = fresh.show_zero_hosts;

        self.announce = fresh.announce.clone();

        if self.promo != fresh.promo {
            self.promo_seen = None;
        }
        self.promo = fresh.promo.clone();
        self.promo_url = fresh.promo_url.clone();

        if fresh.migrate_url.is_none() {
            self.migration_hops = None;
        }
    }

    pub fn panel_clock_skew(&self) -> i64 {
        const MAX_AGE_SECS: i64 = 30 * 24 * 60 * 60;

        let (Some(skew), Some(measured_at)) = (self.clock_skew, self.clock_skew_at) else {
            return 0;
        };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        let age = now - measured_at;
        if !(0..=MAX_AGE_SECS).contains(&age) { 0 } else { skew }
    }

    pub fn promo_pending(&self) -> bool {
        self.promo.as_deref().is_some_and(|text| !text.is_empty()) && !self.promo_seen.unwrap_or(false)
    }

    pub fn record_url_migration(&mut self, new_url: String) {
        if let Some(previous) = self.url.take() {
            let history = self.previous_urls.get_or_insert_with(Vec::new);
            if !history.iter().any(|entry| entry == &previous) {
                history.push(previous);
            }
            if history.len() > 10 {
                let overflow = history.len() - 10;
                history.drain(0..overflow);
            }
        }
        self.url = Some(new_url);
        self.migration_hops = Some(self.migration_hops.unwrap_or(0).saturating_add(1));
    }
}

impl PrfItem {
    pub fn current_merge(&self) -> Option<&String> {
        self.option.as_ref().and_then(|o| o.merge.as_ref())
    }

    pub fn current_script(&self) -> Option<&String> {
        self.option.as_ref().and_then(|o| o.script.as_ref())
    }

    pub fn current_rules(&self) -> Option<&String> {
        self.option.as_ref().and_then(|o| o.rules.as_ref())
    }

    pub fn current_proxies(&self) -> Option<&String> {
        self.option.as_ref().and_then(|o| o.proxies.as_ref())
    }

    pub fn current_groups(&self) -> Option<&String> {
        self.option.as_ref().and_then(|o| o.groups.as_ref())
    }
}

#[allow(clippy::unnecessary_wraps)]
const fn default_allow_auto_update() -> Option<bool> {
    Some(true)
}

fn allow_auto_update_enabled(option: Option<&PrfOption>) -> bool {
    option.and_then(|o| o.allow_auto_update).unwrap_or(true)
}

fn fix_dirty_url(input: &str) -> Result<Url> {
    let mut url = match Url::parse(input) {
        Ok(u) => u,
        Err(e) => {
            return Err(anyhow::anyhow!(
                "failed to parse subscription URL: {:?}, input: {}",
                e,
                help::mask_url(input)
            ));
        }
    };

    if url.scheme() != "https" {
        anyhow::bail!(
            "subscription URL must use https, got scheme \"{}\": {}",
            url.scheme(),
            help::mask_url(input)
        );
    }

    if url.host_str().is_none_or(str::is_empty) {
        anyhow::bail!("subscription URL has no host: {}", help::mask_url(input));
    }

    if url.query().is_none() && url.path().contains('&') {
        let path = url.path().to_string();

        if let Some((clean_path, dirty_params)) = path.split_once('&') {
            url.set_path(clean_path);

            url.query_pairs_mut()
                .extend_pairs(form_urlencoded::parse(dirty_params.as_bytes()));
        }
    }

    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::{PrfItem, PrfOption, allow_auto_update_enabled, fix_dirty_url, to_unix_seconds};

    #[test]
    fn provider_links_are_replaced_not_merged() {
        let mut stored = PrfItem {
            portal_url: Some("https://first.example/cabinet".into()),
            support_url: Some("https://first.example/help".into()),
            bot_url: Some("tg://resolve?domain=first_bot".into()),
            monitor_url: Some("https://status.first.example".into()),
            guide_url: Some("https://first.example/guide".into()),
            ..PrfItem::default()
        };

        let fresh = PrfItem {
            bot_url: Some("tg://resolve?domain=second_bot".into()),
            ..PrfItem::default()
        };
        stored.merge_panel_meta(&fresh);

        assert_eq!(stored.bot_url.as_deref(), Some("tg://resolve?domain=second_bot"));
        assert_eq!(stored.portal_url, None);
        assert_eq!(stored.support_url, None);
        assert_eq!(stored.monitor_url, None);
        assert_eq!(stored.guide_url, None);

        stored.merge_panel_meta(&PrfItem::default());
        assert_eq!(stored.bot_url, None);
    }

    #[test]
    fn recognises_milliseconds_in_expire() {
        assert_eq!(to_unix_seconds(1_754_000_000), 1_754_000_000);
        assert_eq!(to_unix_seconds(1_754_000_000_000), 1_754_000_000);
        assert_eq!(to_unix_seconds(0), 0);
    }

    #[test]
    fn auto_update_defaults_to_enabled_and_preserves_explicit_false() {
        assert!(allow_auto_update_enabled(None));

        let disabled = PrfOption {
            allow_auto_update: Some(false),
            ..PrfOption::default()
        };
        assert!(!allow_auto_update_enabled(Some(&disabled)));
    }

    #[test]
    #[allow(clippy::expect_used)]
    fn subscription_urls_are_limited_to_https() {
        assert!(fix_dirty_url("https://panel.example/sub/token").is_ok());
        assert!(fix_dirty_url("https://panel.example/sub").is_ok());
        assert!(fix_dirty_url("http://panel.example/sub").is_err());

        let fixed = fix_dirty_url("https://panel.example/sub&flow=xtls").expect("dirty url");
        assert_eq!(fixed.query(), Some("flow=xtls"));

        for hostile in [
            "file:///etc/passwd",
            "file://server/share/config.yaml",
            "javascript:alert(1)",
            "data:text/yaml,proxies: []",
            "ftp://panel.example/sub",
            "clodclash://install-config?url=x",
        ] {
            let error = fix_dirty_url(hostile).expect_err(hostile).to_string();
            assert!(error.contains("must use https"), "{hostile} -> {error}");
        }

        assert!(fix_dirty_url("https://").is_err());
    }
}
