use crate::{
    config::{profiles, sub_headers},
    utils::{
        dirs, help,
        network::{NetworkManager, ProxyType},
        tmpl,
    },
};
use anyhow::{Context as _, Result, bail};
use serde::{Deserialize, Serialize};
use serde_yaml_ng::Mapping;
use smartstring::alias::String;
use std::time::Duration;
use tokio::fs;
// TODO, use other re-export
use reqwest_dav::re_exports::url::form_urlencoded;
use tauri::Url;

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct PrfItem {
    pub uid: Option<String>,

    /// profile item type
    /// enum value: remote | local | script | merge
    #[serde(rename = "type")]
    pub itype: Option<String>,

    /// profile name
    pub name: Option<String>,

    /// profile file
    pub file: Option<String>,

    /// profile description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub desc: Option<String>,

    /// source url
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,

    /// selected information
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected: Option<Vec<PrfSelected>>,

    /// clod: node names the user starred; shown on top of the server list.
    /// Never touched by subscription updates (`update_item` copies fields
    /// explicitly), so the stars survive refreshes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub favorites: Option<Vec<String>>,

    /// subscription user info
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra: Option<PrfExtra>,

    /// updated time
    pub updated: Option<usize>,

    /// some options of the item
    #[serde(skip_serializing_if = "Option::is_none")]
    pub option: Option<PrfOption>,

    /// profile web page url
    #[serde(skip_serializing_if = "Option::is_none")]
    pub home: Option<String>,

    // clod:headers begin
    /// `support-url` header — provider support contact.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub support_url: Option<String>,

    /// `profile-logo` header — provider logo, an http(s) URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logo: Option<String>,

    /// `announce` header — provider message shown as a banner.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub announce: Option<String>,

    /// `announce-url` header — where the announce banner leads when clicked.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub announce_url: Option<String>,

    /// `clod-portal-url` header — the customer portal (renewal, payments).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub portal_url: Option<String>,

    /// `clod-promo` header — temporary promotion banner, dismissable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub promo: Option<String>,

    /// `clod-promo-url` header — where the promo banner leads when clicked.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub promo_url: Option<String>,

    /// Set once the user dismissed the current promo. Cleared automatically
    /// when the provider changes the text, so a new promotion shows up again.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub promo_seen: Option<bool>,

    /// `clod-renew-url` header — target of the "renew" action button.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub renew_url: Option<String>,

    /// `clod-topup-url` header — target of the "buy more traffic" button.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub topup_url: Option<String>,

    /// `clod-lock-mode` header — the panel forbids changing proxy and routing
    /// modes inside the app.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lock_mode: Option<bool>,

    /// `subscription-refill-date` header, unix seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refill_date: Option<i64>,

    /// The update interval was dictated by the provider, so the UI locks it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interval_locked: Option<bool>,

    /// `fallback-url` header — full spare address used when the primary fails.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback_url: Option<String>,

    /// `fallback-domain` header — spare host for the primary URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback_domain: Option<String>,

    /// URLs this subscription was migrated away from, oldest first.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_urls: Option<Vec<String>>,

    /// Consecutive provider-driven URL migrations, reset once an update brings
    /// no further migration. Guards against a redirect loop between panels.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub migration_hops: Option<u32>,

    /// Device registration state: `ok` | `limit` | `not_supported`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hwid_state: Option<String>,

    /// The user renamed the profile, so `profile-title` must not overwrite it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name_customized: Option<bool>,

    /// Expiry reminder thresholds in days; empty vector = disabled by provider.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notify_expire_days: Option<Vec<u32>>,

    /// Traffic reminder thresholds in percent; empty vector = disabled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notify_traffic_percent: Option<Vec<u32>>,

    /// Reminder bookkeeping: threshold key -> unix seconds it fired at.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notified: Option<std::collections::BTreeMap<std::string::String, i64>>,

    /// The payload came from `fallback_url` instead of `url`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_fallback: Option<bool>,

    /// Interface mode the provider prefers for this subscription. Only a hint:
    /// a user who picked a mode in the settings always wins.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub simple_mode: Option<bool>,

    /// Transient: the name was taken from `profile-title` on this fetch.
    #[serde(skip)]
    pub name_from_header: Option<bool>,

    /// Transient: URL the provider asked us to migrate to (`new-url` /
    /// `new-domain`). Applied only after a successful probe download.
    #[serde(skip)]
    pub migrate_url: Option<String>,

    /// Transient: device limit reported alongside a `limit` state.
    #[serde(skip)]
    pub hwid_max_devices: Option<u32>,
    // clod:headers end
    /// the file data
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
    /// for `remote` profile's http request
    /// see issue #13
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,

    /// for `remote` profile
    /// use system proxy
    #[serde(skip_serializing_if = "Option::is_none")]
    pub with_proxy: Option<bool>,

    /// for `remote` profile
    /// use self proxy
    #[serde(skip_serializing_if = "Option::is_none")]
    pub self_proxy: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub update_interval: Option<u64>,

    /// for `remote` profile
    /// HTTP request timeout in seconds
    /// default is 60 seconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_seconds: Option<u64>,

    /// for `remote` profile
    /// disable certificate validation
    /// default is `false`
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
                Some(result)
            }
            (Some(a_ref), None) => Some(a_ref.clone()),
            (None, Some(b_ref)) => Some(b_ref.clone()),
            (None, None) => None,
        }
    }
}

impl PrfItem {
    /// From partial item
    /// must contain `itype`
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
                Self::from_url(url, name, desc, option).await
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

    /// ## Local type
    /// create a new item from name/desc
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
            // clod: local profiles carry no panel metadata
            ..Self::default()
        })
    }

    /// ## Remote type
    /// create a new item from url
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

        // 选择代理类型
        let proxy_type = if self_proxy {
            ProxyType::Localhost
        } else if with_proxy {
            ProxyType::System
        } else {
            ProxyType::None
        };

        let url = fix_dirty_url(url)?;

        // clod:headers begin
        // Device identity + `Accept: */*` on every subscription request.
        let identity_headers = sub_headers::build_identity_headers().await;
        // clod:headers end

        // 使用网络管理器发送请求
        let resp = match NetworkManager::new()
            .get_with_interrupt_and_headers(
                url.as_str(),
                proxy_type,
                Some(timeout),
                user_agent.clone(),
                accept_invalid_certs,
                Some(&identity_headers),
            )
            .await
        {
            Ok(r) => r,
            Err(e) => {
                tokio::time::sleep(Duration::from_millis(100)).await;
                return Err(e).context("failed to fetch remote profile");
            }
        };

        // clod:headers begin
        // Parsed before the status check: a device-limit response carries a stub
        // body (and sometimes a non-2xx status) but still tells us what happened.
        let sub = sub_headers::SubHeaders::parse(resp.headers());
        sub.notify_device_state();
        if sub.hwid_state == sub_headers::HwidState::LimitReached {
            bail!("device limit reached for this subscription (x-hwid)")
        }
        // clod:headers end

        let status_code = resp.status();
        if !status_code.is_success() {
            bail!("failed to fetch remote profile with status {status_code}")
        }

        let header = resp.headers();

        // parse the Subscription UserInfo
        let extra;
        'extra: {
            for (k, v) in header.iter() {
                let key_lower = k.as_str().to_ascii_lowercase();
                // Accept standard custom-metadata prefixes (x-amz-meta-, x-obs-meta-, x-cos-meta-, etc.).
                if key_lower
                    .strip_suffix("subscription-userinfo")
                    .is_some_and(|prefix| prefix.is_empty() || prefix.ends_with('-'))
                {
                    let sub_info = v.to_str().unwrap_or("");
                    extra = Some(PrfExtra {
                        upload: help::parse_str(sub_info, "upload").unwrap_or(0),
                        download: help::parse_str(sub_info, "download").unwrap_or(0),
                        total: help::parse_str(sub_info, "total").unwrap_or(0),
                        expire: help::parse_str(sub_info, "expire").unwrap_or(0),
                    });
                    break 'extra;
                }
            }
            extra = None;
        }

        // parse the Content-Disposition
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
        // clod:headers begin
        // `profile-update-interval` is now read through SubHeaders so we can also
        // record that the provider — not the user — decided the value.
        let (update_interval, interval_locked) = match update_interval {
            Some(val) => (Some(val), None),
            None => match sub.update_interval_hours {
                // saturating: the header is parsed as an arbitrary u64 and
                // release builds run without overflow checks — a hostile
                // value must not wrap into a tiny interval.
                Some(hours) => (Some(hours.saturating_mul(60)), Some(true)), // hour -> min
                None => (None, None),
            },
        };

        let home = sub.home.clone();
        // clod:headers end

        let uid = help::get_uid("R").into();
        let file = format!("{uid}.yaml").into();
        // clod:headers begin
        // Name priority: explicit user name > `profile-title` > content-disposition.
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
        // clod:headers end
        let data = resp.text_with_charset()?;

        // process the charset "UTF-8 with BOM"
        let data = data.trim_start_matches('\u{feff}');

        // check the data whether the valid yaml format
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

        Ok(Self {
            uid: Some(uid),
            itype: Some("remote".into()),
            name: Some(name),
            desc: desc.cloned(),
            file: Some(file),
            url: Some(url.as_str().into()),
            selected: None,
            favorites: None,
            extra,
            option: Some(PrfOption {
                update_interval,
                merge,
                script,
                rules,
                proxies,
                groups,
                allow_auto_update,
                ..PrfOption::default()
            }),
            home,
            // clod:headers begin
            support_url: sub.support_url.clone(),
            logo: sub.profile_logo.clone(),
            announce: sub.announce.clone(),
            announce_url: sub.announce_url.clone(),
            portal_url: sub.portal_url.clone(),
            promo: sub.promo.clone(),
            promo_url: sub.promo_url.clone(),
            promo_seen: None,
            renew_url: sub.renew_url.clone(),
            topup_url: sub.topup_url.clone(),
            lock_mode: sub.lock_mode,
            refill_date: sub.refill_date,
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
            // clod:headers end
            updated: Some(chrono::Local::now().timestamp() as usize),
            file_data: Some(data.into()),
        })
    }

    /// ## Merge type (enhance)
    /// create the enhanced item by using `merge` rule
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

    /// ## Script type (enhance)
    /// create the enhanced item by using javascript quick.js
    pub fn from_script(uid: Option<String>) -> Result<Self> {
        let id = if let Some(uid) = uid {
            uid
        } else {
            help::get_uid("s").into()
        };
        let file = format!("{id}.js").into(); // js ext
        Ok(Self {
            uid: Some(id),
            itype: Some("script".into()),
            file: Some(file),
            updated: Some(chrono::Local::now().timestamp() as usize),
            file_data: Some(tmpl::ITEM_SCRIPT.into()),
            ..Default::default()
        })
    }

    /// ## Rules type (enhance)
    pub fn from_rules() -> Result<Self> {
        let uid = help::get_uid("r").into();
        let file = format!("{uid}.yaml").into(); // yaml ext

        Ok(Self {
            uid: Some(uid),
            itype: Some("rules".into()),
            file: Some(file),
            updated: Some(chrono::Local::now().timestamp() as usize),
            file_data: Some(tmpl::ITEM_RULES.into()),
            ..Default::default()
        })
    }

    /// ## Proxies type (enhance)
    pub fn from_proxies() -> Result<Self> {
        let uid = help::get_uid("p").into();
        let file = format!("{uid}.yaml").into(); // yaml ext

        Ok(Self {
            uid: Some(uid),
            itype: Some("proxies".into()),
            file: Some(file),
            updated: Some(chrono::Local::now().timestamp() as usize),
            file_data: Some(tmpl::ITEM_PROXIES.into()),
            ..Default::default()
        })
    }

    /// ## Groups type (enhance)
    pub fn from_groups() -> Result<Self> {
        let uid = help::get_uid("g").into();
        let file = format!("{uid}.yaml").into(); // yaml ext

        Ok(Self {
            uid: Some(uid),
            itype: Some("groups".into()),
            file: Some(file),
            updated: Some(chrono::Local::now().timestamp() as usize),
            file_data: Some(tmpl::ITEM_GROUPS.into()),
            ..Default::default()
        })
    }

    /// get the file data
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

    /// save the file data
    pub async fn save_file(&self, data: String) -> Result<()> {
        let file = self
            .file
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("could not find the file"))?;
        let path = dirs::app_profiles_dir()?.join(file.as_str());
        fs::write(path, data.as_bytes())
            .await
            .context("failed to save the file")
    }
}

// clod:headers begin
impl PrfItem {
    /// Copy provider metadata from a freshly fetched item onto the stored one.
    ///
    /// Called from [`crate::config::profiles::IProfiles::update_item`] so a
    /// subscription refresh picks up header changes without losing local state
    /// (dismissed announce, migration history, a name the user chose).
    pub fn merge_panel_meta(&mut self, fresh: &Self) {
        // A provider name only applies while the user has not renamed the profile.
        if fresh.name_from_header == Some(true) && self.name_customized != Some(true) && fresh.name.is_some() {
            self.name = fresh.name.clone();
        }

        // Replace, do not merge: a header that disappeared must clear its value.
        self.support_url = fresh.support_url.clone();
        self.logo = fresh.logo.clone();
        self.announce_url = fresh.announce_url.clone();
        self.refill_date = fresh.refill_date;
        self.interval_locked = fresh.interval_locked;
        self.fallback_url = fresh.fallback_url.clone();
        self.fallback_domain = fresh.fallback_domain.clone();
        self.hwid_state = fresh.hwid_state.clone();
        self.notify_expire_days = fresh.notify_expire_days.clone();
        self.notify_traffic_percent = fresh.notify_traffic_percent.clone();
        self.from_fallback = fresh.from_fallback;
        self.simple_mode = fresh.simple_mode;
        self.portal_url = fresh.portal_url.clone();
        self.renew_url = fresh.renew_url.clone();
        self.topup_url = fresh.topup_url.clone();
        self.lock_mode = fresh.lock_mode;

        // The announce is permanent and never dismissable; it simply follows
        // whatever the panel currently says.
        self.announce = fresh.announce.clone();

        // A changed promo must be shown again, so drop the dismissal. A promo
        // the panel stopped sending disappears entirely.
        if self.promo != fresh.promo {
            self.promo_seen = None;
        }
        self.promo = fresh.promo.clone();
        self.promo_url = fresh.promo_url.clone();

        // An update that asks for no migration ends the current migration chain,
        // so the hop guard starts from zero again next time.
        if fresh.migrate_url.is_none() {
            self.migration_hops = None;
        }

        // `previous_urls` and `notified` are owned locally and never come from
        // the panel; `url` migration is applied by `feat::profile`.
    }

    /// Whether the stored promo still needs to be shown.
    ///
    /// Dismissal is a plain marker rather than a comparison: `merge_panel_meta`
    /// already clears it whenever the promo text changes, so a new promotion
    /// reappears without the UI having to hash anything.
    pub fn promo_pending(&self) -> bool {
        self.promo.as_deref().is_some_and(|text| !text.is_empty()) && !self.promo_seen.unwrap_or(false)
    }

    /// Record that the primary URL was replaced by `new_url`.
    pub fn record_url_migration(&mut self, new_url: String) {
        if let Some(previous) = self.url.take() {
            let history = self.previous_urls.get_or_insert_with(Vec::new);
            if !history.iter().any(|entry| entry == &previous) {
                history.push(previous);
            }
            // Keep the history bounded.
            if history.len() > 10 {
                let overflow = history.len() - 10;
                history.drain(0..overflow);
            }
        }
        self.url = Some(new_url);
        self.migration_hops = Some(self.migration_hops.unwrap_or(0).saturating_add(1));
    }
}
// clod:headers end

impl PrfItem {
    /// 获取current指向的订阅的merge
    pub fn current_merge(&self) -> Option<&String> {
        self.option.as_ref().and_then(|o| o.merge.as_ref())
    }

    /// 获取current指向的订阅的script
    pub fn current_script(&self) -> Option<&String> {
        self.option.as_ref().and_then(|o| o.script.as_ref())
    }

    /// 获取current指向的订阅的rules
    pub fn current_rules(&self) -> Option<&String> {
        self.option.as_ref().and_then(|o| o.rules.as_ref())
    }

    /// 获取current指向的订阅的proxies
    pub fn current_proxies(&self) -> Option<&String> {
        self.option.as_ref().and_then(|o| o.proxies.as_ref())
    }

    /// 获取current指向的订阅的groups
    pub fn current_groups(&self) -> Option<&String> {
        self.option.as_ref().and_then(|o| o.groups.as_ref())
    }
}

// 向前兼容，默认为订阅启用自动更新
#[allow(clippy::unnecessary_wraps)]
const fn default_allow_auto_update() -> Option<bool> {
    Some(true)
}

fn allow_auto_update_enabled(option: Option<&PrfOption>) -> bool {
    option.and_then(|o| o.allow_auto_update).unwrap_or(true)
}

/// Fix URLs where query parameters are incorrectly appended to the path segment
///
/// Incorrect Example: https://example.com/path&param1=value1
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
    use super::{PrfOption, allow_auto_update_enabled};

    #[test]
    fn auto_update_defaults_to_enabled_and_preserves_explicit_false() {
        assert!(allow_auto_update_enabled(None));

        let disabled = PrfOption {
            allow_auto_update: Some(false),
            ..PrfOption::default()
        };
        assert!(!allow_auto_update_enabled(Some(&disabled)));
    }
}
