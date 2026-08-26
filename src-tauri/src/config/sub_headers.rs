use crate::utils::hwid;
use base64::engine::general_purpose;
use reqwest::header::{ACCEPT, HeaderMap, HeaderName, HeaderValue};
use smartstring::alias::String;

pub const ANNOUNCE_MAX_CHARS: usize = 500;

pub const DEFAULT_NOTIFY_EXPIRE_DAYS: &[u32] = &[7, 3, 1];

pub const DEFAULT_NOTIFY_TRAFFIC_PERCENT: &[u32] = &[80, 90, 100];

const MAX_THRESHOLDS: usize = 10;

pub const MAX_MIGRATION_HOPS: u32 = 3;

pub async fn build_identity_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(ACCEPT, HeaderValue::from_static("*/*"));

    if let Some(id) = hwid::hwid().await {
        insert_str(&mut headers, "x-hwid", id.as_str());
        insert_str(&mut headers, "x-device-os", hwid::device_os());
        insert_str(&mut headers, "x-ver-os", hwid::os_version().as_str());
        insert_str(&mut headers, "x-device-model", hwid::device_model().as_str());
    }

    headers
}

fn insert_str(headers: &mut HeaderMap, name: &'static str, value: &str) {
    if value.is_empty() {
        return;
    }
    if let Ok(header_value) = HeaderValue::from_str(value) {
        headers.insert(HeaderName::from_static(name), header_value);
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum HwidState {
    #[default]
    Unknown,
    Active,
    NotSupported,
    LimitReached,
}

impl HwidState {
    pub const fn as_str(self) -> Option<&'static str> {
        match self {
            Self::Unknown => None,
            Self::Active => Some("ok"),
            Self::NotSupported => Some("not_supported"),
            Self::LimitReached => Some("limit"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectMode {
    Tun,
    Proxy,
    Both,
}

impl ConnectMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Tun => "tun",
            Self::Proxy => "proxy",
            Self::Both => "both",
        }
    }

    fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "tun" | "tunnel" | "vpn" => Some(Self::Tun),
            "proxy" | "system" | "system-proxy" | "sysproxy" => Some(Self::Proxy),
            "both" | "all" => Some(Self::Both),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LatencyStyle {
    Bars,
    Dot,
    Number,
}

impl LatencyStyle {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Bars => "bars",
            Self::Dot => "dot",
            Self::Number => "number",
        }
    }

    fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "bars" | "signal" => Some(Self::Bars),
            "dot" | "dots" => Some(Self::Dot),
            "number" | "ms" | "latency" => Some(Self::Number),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct SubHeaders {
    pub profile_title: Option<String>,
    pub profile_logo: Option<String>,
    pub home: Option<String>,
    pub support_url: Option<String>,
    pub announce: Option<String>,
    pub announce_url: Option<String>,
    pub refill_date: Option<i64>,
    pub update_interval_hours: Option<u64>,
    pub fallback_url: Option<String>,
    pub fallback_domain: Option<String>,
    pub new_url: Option<String>,
    pub new_domain: Option<String>,
    pub hwid_state: HwidState,
    pub hwid_max_devices: Option<u32>,
    pub notify_expire_days: Option<Vec<u32>>,
    pub notify_traffic_percent: Option<Vec<u32>>,
    pub simple_mode: Option<bool>,
    pub portal_url: Option<String>,
    pub bot_url: Option<String>,
    pub monitor_url: Option<String>,
    pub guide_url: Option<String>,
    pub promo: Option<String>,
    pub promo_url: Option<String>,
    pub hwid_limit_message: Option<String>,
    pub latency_style: Option<LatencyStyle>,

    pub disable_ping: bool,

    pub device_remove_url: Option<String>,

    pub lock_mode: Option<bool>,

    pub connect_mode: Option<ConnectMode>,

    pub show_zero_hosts: Option<bool>,
    pub server_time: Option<i64>,
}

impl SubHeaders {
    pub fn parse(headers: &HeaderMap) -> Self {
        let hwid_limit = flag(headers, "x-hwid-max-devices-reached") || flag(headers, "x-hwid-limit");
        let hwid_not_supported = flag(headers, "x-hwid-not-supported");
        let hwid_active = flag(headers, "x-hwid-active");

        let hwid_state = if hwid_not_supported {
            HwidState::NotSupported
        } else if hwid_limit {
            HwidState::LimitReached
        } else if hwid_active {
            HwidState::Active
        } else {
            HwidState::Unknown
        };

        let notify_expire_days = value(headers, "notify-expire-days")
            .and_then(|raw| thresholds(&raw, 1, 365))
            .or_else(|| (flag(headers, "notification-subs-expire")).then(|| DEFAULT_NOTIFY_EXPIRE_DAYS.to_vec()));

        Self {
            profile_title: value(headers, "profile-title"),
            profile_logo: value(headers, "profile-logo").and_then(|raw| https_url(&raw)),
            home: value(headers, "profile-web-page-url").and_then(|raw| https_url(&raw)),
            support_url: value(headers, "support-url").and_then(|raw| contact_url(&raw)),
            announce: value(headers, "announce").map(|text| truncate_banner(&text, ANNOUNCE_MAX_CHARS)),
            announce_url: value(headers, "announce-url").and_then(|raw| https_url(&raw)),
            refill_date: value(headers, "subscription-refill-date").and_then(|raw| raw.trim().parse::<i64>().ok()),
            update_interval_hours: value(headers, "profile-update-interval").and_then(|raw| raw.trim().parse().ok()),
            fallback_url: value(headers, "fallback-url").and_then(|raw| https_url(&raw)),
            fallback_domain: value(headers, "fallback-domain"),
            new_url: value(headers, "new-url"),
            new_domain: value(headers, "new-domain"),
            hwid_state,
            hwid_max_devices: value(headers, "x-hwid-max-devices").and_then(|raw| raw.trim().parse().ok()),
            notify_expire_days,
            notify_traffic_percent: value(headers, "notify-traffic-percent").and_then(|raw| thresholds(&raw, 1, 100)),
            simple_mode: bool_value(headers, "clod-simple-mode")
                .or_else(|| bool_value(headers, "pxa-simple-mode"))
                .or_else(|| bool_value(headers, "flclashx-newboard")),
            portal_url: value(headers, "clod-portal-url").and_then(|raw| https_url(&raw)),
            bot_url: value(headers, "clod-bot-url").and_then(|raw| contact_url(&raw)),
            monitor_url: value(headers, "clod-monitor-url").and_then(|raw| https_url(&raw)),
            guide_url: value(headers, "clod-guide-url").and_then(|raw| https_url(&raw)),
            promo: value(headers, "clod-promo").map(|text| truncate_banner(&text, ANNOUNCE_MAX_CHARS)),
            promo_url: value(headers, "clod-promo-url").and_then(|raw| https_url(&raw)),
            latency_style: value(headers, "clod-latency-style")
                .as_deref()
                .and_then(LatencyStyle::parse)
                .or_else(|| bool_value(headers, "pxa-latency-dots").and_then(|dots| dots.then_some(LatencyStyle::Dot))),
            disable_ping: value(headers, "clod-disable-ping")
                .is_some_and(|raw| raw.trim().eq_ignore_ascii_case("true")),
            device_remove_url: value(headers, "clod-device-remove").and_then(|raw| https_url(&raw)),
            hwid_limit_message: value(headers, "clod-hwid-limit")
                .map(|text| truncate_banner(&text, ANNOUNCE_MAX_CHARS)),
            show_zero_hosts: bool_value(headers, "clod-show-0hosts"),
            lock_mode: bool_value(headers, "clod-lock-mode")
                .or_else(|| bool_value(headers, "global-mode").map(|allowed| !allowed)),
            connect_mode: value(headers, "clod-connect-mode")
                .as_deref()
                .and_then(ConnectMode::parse),
            server_time: (!is_cached(headers))
                .then(|| {
                    headers
                        .get(reqwest::header::DATE)
                        .and_then(|raw| raw.to_str().ok())
                        .and_then(http_date_secs)
                })
                .flatten(),
        }
    }
}

fn is_cached(headers: &HeaderMap) -> bool {
    headers
        .get(reqwest::header::AGE)
        .and_then(|raw| raw.to_str().ok())
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .is_some_and(|age| age > 0)
}

fn http_date_secs(raw: &str) -> Option<i64> {
    const MIN: i64 = 1_577_836_800;
    const MAX: i64 = 4_102_444_800;

    let raw = raw.trim();
    let naive = |fmt: &str| chrono::NaiveDateTime::parse_from_str(raw, fmt).map(|dt| dt.and_utc().timestamp());

    let secs = naive("%a, %d %b %Y %H:%M:%S GMT")
        .or_else(|_| chrono::DateTime::parse_from_rfc2822(raw).map(|dt| dt.timestamp()))
        .or_else(|_| naive("%A, %d-%b-%y %H:%M:%S GMT"))
        .or_else(|_| naive("%a %b %e %H:%M:%S %Y"))
        .ok()?;

    (MIN..MAX).contains(&secs).then_some(secs)
}

impl SubHeaders {
    pub fn migration_target(&self, current: &str) -> Option<String> {
        if let Some(new_url) = self.new_url.as_deref()
            && let Some(validated) = validate_new_url(current, new_url)
        {
            return Some(validated);
        }

        self.new_domain
            .as_deref()
            .and_then(|domain| swap_domain(current, domain))
            .filter(|candidate| candidate.as_str() != current)
    }

    pub fn notify_device_state(&self) {
        let state = match self.hwid_state {
            HwidState::LimitReached | HwidState::NotSupported => self.hwid_state,
            HwidState::Unknown | HwidState::Active => return,
        };

        crate::core::handle::Handle::hwid_notice(serde_json::json!({
            "state": state.as_str(),
            "maxDevices": self.hwid_max_devices,
            "supportUrl": self.support_url.as_deref(),
            "removeUrl": self.device_remove_url.as_deref(),
            "message": self.hwid_limit_message.as_deref(),
        }));
    }
}

fn value(headers: &HeaderMap, name: &str) -> Option<String> {
    for (key, raw) in headers.iter() {
        let key_lower = key.as_str().to_ascii_lowercase();
        let matches = key_lower
            .strip_suffix(name)
            .is_some_and(|prefix| prefix.is_empty() || prefix.ends_with('-'));
        if !matches {
            continue;
        }

        let text = match raw.to_str() {
            Ok(text) => std::borrow::Cow::Borrowed(text),
            Err(_) => std::string::String::from_utf8_lossy(raw.as_bytes()),
        };

        let decoded = decode_value(&text);
        if !decoded.is_empty() {
            return Some(decoded);
        }
    }
    None
}

fn decode_value(raw: &str) -> String {
    let trimmed = raw.trim();
    let Some(payload) = strip_base64_prefix(trimmed) else {
        return trimmed.into();
    };

    let payload_compact: std::string::String = payload.chars().filter(|c| !c.is_whitespace()).collect();
    for engine in [
        &general_purpose::STANDARD as &dyn Base64Decode,
        &general_purpose::STANDARD_NO_PAD,
        &general_purpose::URL_SAFE,
        &general_purpose::URL_SAFE_NO_PAD,
    ] {
        if let Some(bytes) = engine.try_decode(&payload_compact)
            && let Ok(text) = std::string::String::from_utf8(bytes)
        {
            let text = text.trim();
            if !text.is_empty() {
                return text.into();
            }
        }
    }

    String::new()
}

trait Base64Decode {
    fn try_decode(&self, input: &str) -> Option<Vec<u8>>;
}

impl<T: base64::Engine> Base64Decode for T {
    fn try_decode(&self, input: &str) -> Option<Vec<u8>> {
        self.decode(input).ok()
    }
}

fn strip_base64_prefix(value: &str) -> Option<&str> {
    let (prefix, rest) = value.split_at_checked(7)?;
    prefix.eq_ignore_ascii_case("base64:").then_some(rest)
}

fn flag(headers: &HeaderMap, name: &str) -> bool {
    bool_value(headers, name).unwrap_or(false)
}

fn bool_value(headers: &HeaderMap, name: &str) -> Option<bool> {
    let raw = value(headers, name)?;
    match raw.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Some(true),
        "false" | "0" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn thresholds(raw: &str, min: u32, max: u32) -> Option<Vec<u32>> {
    let trimmed = raw.trim();
    if trimmed.eq_ignore_ascii_case("off") || trimmed.eq_ignore_ascii_case("false") {
        return Some(Vec::new());
    }

    let mut values: Vec<u32> = trimmed
        .split(',')
        .filter_map(|part| part.trim().parse::<u32>().ok())
        .filter(|value| (min..=max).contains(value))
        .collect();

    values.sort_unstable();
    values.dedup();
    values.truncate(MAX_THRESHOLDS);

    (!values.is_empty()).then_some(values)
}

fn https_url(value: &str) -> Option<String> {
    let parsed = tauri::Url::parse(value.trim()).ok()?;
    if parsed.scheme() != "https" {
        return None;
    }
    parsed.host_str().filter(|host| !host.is_empty())?;
    Some(parsed.as_str().into())
}

fn contact_url(value: &str) -> Option<String> {
    let parsed = tauri::Url::parse(value.trim()).ok()?;
    match parsed.scheme() {
        "https" => {
            parsed.host_str().filter(|host| !host.is_empty())?;
        }
        "tg" | "mailto" => {}
        _ => return None,
    }
    Some(value.trim().into())
}

const COLOUR_MARKER_LEN: usize = 7;

fn colour_marker_at(chars: &[char], index: usize) -> bool {
    if chars.get(index) != Some(&'#') || index + COLOUR_MARKER_LEN > chars.len() {
        return false;
    }
    if chars
        .get(index + COLOUR_MARKER_LEN)
        .is_none_or(|next| next.is_whitespace())
    {
        return false;
    }
    chars[index + 1..index + COLOUR_MARKER_LEN]
        .iter()
        .all(char::is_ascii_hexdigit)
}

fn truncate_banner(value: &str, limit: usize) -> String {
    if value.chars().count() <= limit {
        return value.into();
    }

    let chars: Vec<char> = value.chars().collect();
    let mut out = std::string::String::with_capacity(value.len());
    let mut visible = 0;
    let mut index = 0;

    while index < chars.len() {
        if visible == limit {
            break;
        }
        if colour_marker_at(&chars, index) {
            out.extend(&chars[index..index + COLOUR_MARKER_LEN]);
            index += COLOUR_MARKER_LEN;
            continue;
        }
        out.push(chars[index]);
        visible += 1;
        index += 1;
    }

    out.into()
}

pub fn swap_domain(current: &str, new_domain: &str) -> Option<String> {
    let mut url = tauri::Url::parse(current).ok()?;
    let domain = new_domain.trim().trim_end_matches('/');
    if domain.is_empty() {
        return None;
    }

    let domain = domain
        .split_once("://")
        .map_or(domain, |(_scheme, rest)| rest)
        .split('/')
        .next()?;

    let (host, port) = match domain.rsplit_once(':') {
        Some((host, port)) if port.chars().all(|c| c.is_ascii_digit()) && !port.is_empty() => {
            (host, Some(port.parse::<u16>().ok()?))
        }
        _ => (domain, None),
    };

    if host.is_empty() {
        return None;
    }

    url.set_host(Some(host)).ok()?;
    url.set_port(port).ok()?;

    Some(url.as_str().into())
}

pub fn validate_new_url(current: &str, candidate: &str) -> Option<String> {
    let candidate_url = tauri::Url::parse(candidate.trim()).ok()?;
    if candidate_url.scheme() != "https" {
        return None;
    }
    if candidate_url.host_str().is_none_or(str::is_empty) {
        return None;
    }

    (candidate_url.as_str() != current).then(|| candidate_url.as_str().into())
}

#[allow(clippy::expect_used)]
#[cfg(test)]
mod tests {
    use super::{
        ANNOUNCE_MAX_CHARS, ConnectMode, DEFAULT_NOTIFY_EXPIRE_DAYS, HwidState, LatencyStyle, SubHeaders, contact_url,
        decode_value, swap_domain, thresholds, truncate_banner, validate_new_url,
    };
    use reqwest::header::{HeaderMap, HeaderName, HeaderValue};

    #[allow(clippy::expect_used)]
    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (key, value) in pairs {
            map.insert(
                HeaderName::from_bytes(key.as_bytes()).expect("test fixture header name must be valid"),
                HeaderValue::from_str(value).expect("test fixture header value must be valid"),
            );
        }
        map
    }

    #[test]
    fn parses_the_full_remnawave_header_set() {
        let parsed = SubHeaders::parse(&headers(&[
            ("profile-title", "My VPN"),
            ("profile-web-page-url", "https://panel.example/cab"),
            ("support-url", "https://t.me/support"),
            ("announce", "base64:bGluZSBvbmUKbGluZSB0d28="),
            ("subscription-refill-date", "1785340800"),
            ("profile-update-interval", "12"),
            ("fallback-url", "https://backup.example/sub"),
            ("x-hwid-active", "true"),
            ("notify-expire-days", "7,3,1"),
            ("notify-traffic-percent", "80,90,100"),
        ]));

        assert_eq!(parsed.profile_title.as_deref(), Some("My VPN"));
        assert_eq!(parsed.home.as_deref(), Some("https://panel.example/cab"));
        assert_eq!(parsed.support_url.as_deref(), Some("https://t.me/support"));
        assert_eq!(parsed.announce.as_deref(), Some("line one\nline two"));
        assert_eq!(parsed.refill_date, Some(1_785_340_800));
        assert_eq!(parsed.update_interval_hours, Some(12));
        assert_eq!(parsed.fallback_url.as_deref(), Some("https://backup.example/sub"));
        assert_eq!(parsed.hwid_state, HwidState::Active);
        assert_eq!(parsed.notify_expire_days.as_deref(), Some(&[1, 3, 7][..]));
        assert_eq!(parsed.notify_traffic_percent.as_deref(), Some(&[80, 90, 100][..]));
    }

    #[test]
    fn accepts_object_storage_prefixes_and_mixed_case() {
        let parsed = SubHeaders::parse(&headers(&[
            ("X-Amz-Meta-Profile-Title", "Prefixed"),
            ("X-OBS-META-SUPPORT-URL", "https://support.example"),
        ]));

        assert_eq!(parsed.profile_title.as_deref(), Some("Prefixed"));
        assert_eq!(parsed.support_url.as_deref(), Some("https://support.example"));
    }

    #[test]
    fn does_not_match_a_header_that_merely_contains_the_name() {
        let parsed = SubHeaders::parse(&headers(&[("renew-url", "https://evil.example/sub")]));
        assert_eq!(parsed.new_url, None);
    }

    #[test]
    fn decodes_base64_values() {
        assert_eq!(
            decode_value("base64:0KLQsNGA0LjRhCBQcm8K0LTQviAyMDI3"),
            "Тариф Pro\nдо 2027"
        );
        assert_eq!(decode_value("base64:SGVsbG8_"), "Hello?");
        assert_eq!(decode_value("base64:!!!not-base64!!!"), "");
        assert_eq!(decode_value("  plain  "), "plain");
    }

    #[test]
    fn simple_mode_reads_our_header_first_and_tolerates_the_others() {
        let parsed = SubHeaders::parse(&headers(&[("clod-simple-mode", "1")]));
        assert_eq!(parsed.simple_mode, Some(true));

        let parsed = SubHeaders::parse(&headers(&[("clod-simple-mode", "false")]));
        assert_eq!(parsed.simple_mode, Some(false));

        let parsed = SubHeaders::parse(&headers(&[("pxa-simple-mode", "1")]));
        assert_eq!(parsed.simple_mode, Some(true));
        let parsed = SubHeaders::parse(&headers(&[("flclashx-newboard", "true")]));
        assert_eq!(parsed.simple_mode, Some(true));

        let parsed = SubHeaders::parse(&headers(&[("clod-simple-mode", "0"), ("pxa-simple-mode", "1")]));
        assert_eq!(parsed.simple_mode, Some(false));

        assert_eq!(SubHeaders::parse(&headers(&[])).simple_mode, None);
        let parsed = SubHeaders::parse(&headers(&[("clod-simple-mode", "maybe")]));
        assert_eq!(parsed.simple_mode, None);
    }

    #[test]
    fn parses_the_provider_link_headers() {
        let parsed = SubHeaders::parse(&headers(&[
            ("clod-bot-url", "tg://resolve?domain=provider_bot"),
            ("clod-monitor-url", "https://status.provider.example"),
            ("clod-guide-url", "https://provider.example/help/setup"),
        ]));

        assert_eq!(parsed.bot_url.as_deref(), Some("tg://resolve?domain=provider_bot"));
        assert_eq!(parsed.monitor_url.as_deref(), Some("https://status.provider.example/"));
        assert_eq!(parsed.guide_url.as_deref(), Some("https://provider.example/help/setup"));

        let parsed = SubHeaders::parse(&headers(&[("clod-bot-url", "https://t.me/provider_bot")]));
        assert_eq!(parsed.bot_url.as_deref(), Some("https://t.me/provider_bot"));

        let parsed = SubHeaders::parse(&headers(&[
            ("clod-monitor-url", "http://status.provider.example"),
            ("clod-guide-url", "javascript:alert(1)"),
        ]));
        assert_eq!(parsed.monitor_url, None);
        assert_eq!(parsed.guide_url, None);

        let parsed = SubHeaders::parse(&headers(&[
            ("clod-bot-url", "file:///etc/passwd"),
            ("clod-monitor-url", "file://server/share"),
        ]));
        assert_eq!(parsed.bot_url, None);
        assert_eq!(parsed.monitor_url, None);

        let parsed = SubHeaders::parse(&headers(&[]));
        assert_eq!(parsed.bot_url, None);
        assert_eq!(parsed.monitor_url, None);
        assert_eq!(parsed.guide_url, None);
    }

    #[test]
    fn parses_the_clod_action_headers() {
        let parsed = SubHeaders::parse(&headers(&[
            ("clod-portal-url", "https://my.provider.example/cabinet"),
            ("clod-promo", "base64:0KHQutC40LTQutCwIDIwICU="),
            ("clod-promo-url", "https://my.provider.example/promo"),
        ]));

        assert_eq!(
            parsed.portal_url.as_deref(),
            Some("https://my.provider.example/cabinet")
        );
        assert_eq!(parsed.promo.as_deref(), Some("Скидка 20 %"));
        assert_eq!(parsed.promo_url.as_deref(), Some("https://my.provider.example/promo"));

        let parsed = SubHeaders::parse(&headers(&[("profile-web-page-url", "https://panel.example/sub/abc")]));
        assert_eq!(parsed.portal_url, None);

        let parsed = SubHeaders::parse(&headers(&[("clod-portal-url", "javascript:alert(1)")]));
        assert_eq!(parsed.portal_url, None);

        let parsed = SubHeaders::parse(&headers(&[]));
        assert_eq!(parsed.portal_url, None);
        assert_eq!(parsed.promo, None);
    }

    #[test]
    fn hwid_limit_message_is_its_own_header() {
        let parsed = SubHeaders::parse(&headers(&[
            ("announce", "banner for everybody"),
            (
                "clod-hwid-limit",
                "base64:0J7RgtCy0Y/Qt9Cw0YLRjCDRg9GB0YLRgNC+0LnRgdGC0LLQviDQvNC+0LbQvdC+INCyINC60LDQsdC40L3QtdGC0LU=",
            ),
        ]));
        assert_eq!(parsed.announce.as_deref(), Some("banner for everybody"));
        assert_eq!(
            parsed.hwid_limit_message.as_deref(),
            Some("Отвязать устройство можно в кабинете")
        );

        let parsed = SubHeaders::parse(&headers(&[("announce", "banner for everybody")]));
        assert_eq!(parsed.hwid_limit_message, None);

        let parsed = SubHeaders::parse(&headers(&[("x-hwid-limit", "true")]));
        assert_eq!(parsed.hwid_limit_message, None);
        assert_eq!(parsed.hwid_state, HwidState::LimitReached);
        let parsed = SubHeaders::parse(&headers(&[("clod-hwid-limit", "текст для диалога")]));
        assert_eq!(parsed.hwid_state, HwidState::Unknown);

        let long = "я".repeat(700);
        let parsed = SubHeaders::parse(&headers(&[("clod-hwid-limit", long.as_str())]));
        assert_eq!(
            parsed.hwid_limit_message.map(|text| text.chars().count()),
            Some(ANNOUNCE_MAX_CHARS)
        );
    }

    #[test]
    fn show_zero_hosts_is_off_unless_the_panel_asks() {
        assert_eq!(SubHeaders::parse(&headers(&[])).show_zero_hosts, None);
        for raw in ["true", "1", "yes", "on", "TRUE", " On "] {
            assert_eq!(
                SubHeaders::parse(&headers(&[("clod-show-0hosts", raw)])).show_zero_hosts,
                Some(true),
                "{raw} должно читаться как включено"
            );
        }
        for raw in ["false", "0", "no", "off"] {
            assert_eq!(
                SubHeaders::parse(&headers(&[("clod-show-0hosts", raw)])).show_zero_hosts,
                Some(false),
                "{raw} должно читаться как выключено"
            );
        }
        assert_eq!(
            SubHeaders::parse(&headers(&[("clod-show-0hosts", "maybe")])).show_zero_hosts,
            None
        );
        assert_eq!(
            SubHeaders::parse(&headers(&[("x-amz-meta-clod-show-0hosts", "true")])).show_zero_hosts,
            Some(true)
        );
    }

    #[test]
    fn lock_mode_reads_our_header_and_the_prizrak_synonym() {
        let parsed = SubHeaders::parse(&headers(&[("clod-lock-mode", "1")]));
        assert_eq!(parsed.lock_mode, Some(true));

        let parsed = SubHeaders::parse(&headers(&[("global-mode", "false")]));
        assert_eq!(parsed.lock_mode, Some(true));
        let parsed = SubHeaders::parse(&headers(&[("global-mode", "true")]));
        assert_eq!(parsed.lock_mode, Some(false));

        let parsed = SubHeaders::parse(&headers(&[("clod-lock-mode", "0"), ("global-mode", "false")]));
        assert_eq!(parsed.lock_mode, Some(false));

        assert_eq!(SubHeaders::parse(&headers(&[])).lock_mode, None);
    }

    #[test]
    fn connect_mode_takes_a_closed_list_of_values() {
        let parsed = SubHeaders::parse(&headers(&[("clod-connect-mode", "TUN")]));
        assert_eq!(parsed.connect_mode, Some(ConnectMode::Tun));
        let parsed = SubHeaders::parse(&headers(&[("clod-connect-mode", " proxy ")]));
        assert_eq!(parsed.connect_mode, Some(ConnectMode::Proxy));
        let parsed = SubHeaders::parse(&headers(&[("clod-connect-mode", "both")]));
        assert_eq!(parsed.connect_mode, Some(ConnectMode::Both));

        assert_eq!(
            SubHeaders::parse(&headers(&[("clod-connect-mode", "system-proxy")])).connect_mode,
            Some(ConnectMode::Proxy)
        );
        assert_eq!(
            SubHeaders::parse(&headers(&[("clod-connect-mode", "vpn")])).connect_mode,
            Some(ConnectMode::Tun)
        );

        assert_eq!(
            SubHeaders::parse(&headers(&[("clod-connect-mode", "tunnel-only")])).connect_mode,
            None
        );
        assert_eq!(SubHeaders::parse(&headers(&[])).connect_mode, None);
    }

    #[test]
    fn latency_style_and_device_removal_link() {
        let parsed = SubHeaders::parse(&headers(&[("clod-latency-style", "Dot")]));
        assert_eq!(parsed.latency_style, Some(LatencyStyle::Dot));
        let parsed = SubHeaders::parse(&headers(&[("clod-latency-style", "number")]));
        assert_eq!(parsed.latency_style, Some(LatencyStyle::Number));

        let parsed = SubHeaders::parse(&headers(&[("pxa-latency-dots", "1")]));
        assert_eq!(parsed.latency_style, Some(LatencyStyle::Dot));
        assert_eq!(
            SubHeaders::parse(&headers(&[("pxa-latency-dots", "0")])).latency_style,
            None
        );
        let parsed = SubHeaders::parse(&headers(&[("clod-latency-style", "bars"), ("pxa-latency-dots", "1")]));
        assert_eq!(parsed.latency_style, Some(LatencyStyle::Bars));
        assert_eq!(
            SubHeaders::parse(&headers(&[("clod-latency-style", "blink")])).latency_style,
            None
        );

        let parsed = SubHeaders::parse(&headers(&[("clod-device-remove", "https://panel.example/devices")]));
        assert_eq!(
            parsed.device_remove_url.as_deref(),
            Some("https://panel.example/devices")
        );
        assert_eq!(
            SubHeaders::parse(&headers(&[("clod-device-remove", "javascript:alert(1)")])).device_remove_url,
            None
        );
    }

    #[test]
    fn disable_ping_takes_only_a_literal_true() {
        assert!(SubHeaders::parse(&headers(&[("clod-disable-ping", "true")])).disable_ping);
        assert!(SubHeaders::parse(&headers(&[("clod-disable-ping", " True ")])).disable_ping);
        assert!(!SubHeaders::parse(&headers(&[("clod-disable-ping", "1")])).disable_ping);
        assert!(!SubHeaders::parse(&headers(&[("clod-disable-ping", "false")])).disable_ping);
        assert!(!SubHeaders::parse(&headers(&[])).disable_ping);
    }

    #[test]
    fn promo_does_not_shadow_its_url_and_a_renew_url_is_not_new_url() {
        let parsed = SubHeaders::parse(&headers(&[
            ("clod-promo", "sale"),
            ("clod-promo-url", "https://p.example/sale"),
            ("clod-renew-url", "https://p.example/renew"),
        ]));
        assert_eq!(parsed.promo.as_deref(), Some("sale"));
        assert_eq!(parsed.promo_url.as_deref(), Some("https://p.example/sale"));
        assert_eq!(parsed.new_url, None);
    }

    #[test]
    fn every_link_header_requires_https() {
        let parsed = SubHeaders::parse(&headers(&[
            ("profile-logo", "http://cdn.example/logo.png"),
            ("profile-web-page-url", "http://panel.example/home"),
            ("support-url", "http://help.example/chat"),
            ("clod-portal-url", "http://my.provider.example/cabinet"),
            ("fallback-url", "http://backup.panel.example/sub/token"),
        ]));
        assert_eq!(parsed.profile_logo, None);
        assert_eq!(parsed.home, None);
        assert_eq!(parsed.support_url, None);
        assert_eq!(parsed.portal_url, None);
        assert_eq!(parsed.fallback_url, None);

        let parsed = SubHeaders::parse(&headers(&[
            ("profile-logo", "https://cdn.example/logo.png"),
            ("profile-web-page-url", "https://panel.example/home"),
            ("support-url", "https://help.example/chat"),
            ("clod-portal-url", "https://my.provider.example/cabinet"),
            ("fallback-url", "https://backup.panel.example/sub/token"),
        ]));
        assert_eq!(parsed.profile_logo.as_deref(), Some("https://cdn.example/logo.png"));
        assert_eq!(parsed.home.as_deref(), Some("https://panel.example/home"));
        assert_eq!(parsed.support_url.as_deref(), Some("https://help.example/chat"));
        assert_eq!(
            parsed.portal_url.as_deref(),
            Some("https://my.provider.example/cabinet")
        );
        assert_eq!(
            parsed.fallback_url.as_deref(),
            Some("https://backup.panel.example/sub/token")
        );
    }

    #[test]
    fn only_http_links_survive_for_logo_and_announce_url() {
        let parsed = SubHeaders::parse(&headers(&[
            ("profile-logo", "https://cdn.example/logo.png"),
            ("announce-url", "https://panel.example/news"),
        ]));
        assert_eq!(parsed.profile_logo.as_deref(), Some("https://cdn.example/logo.png"));
        assert_eq!(parsed.announce_url.as_deref(), Some("https://panel.example/news"));

        let parsed = SubHeaders::parse(&headers(&[
            ("profile-logo", "javascript:alert(1)"),
            ("announce-url", "file:///etc/passwd"),
        ]));
        assert_eq!(parsed.profile_logo, None);
        assert_eq!(parsed.announce_url, None);
    }

    #[test]
    fn banner_links_require_https() {
        let parsed = SubHeaders::parse(&headers(&[
            ("announce-url", "http://panel.example/news"),
            ("clod-promo-url", "http://p.example/sale"),
        ]));
        assert_eq!(parsed.announce_url, None);
        assert_eq!(parsed.promo_url, None);

        let parsed = SubHeaders::parse(&headers(&[
            ("announce-url", "https://panel.example/news"),
            ("clod-promo-url", "https://p.example/sale"),
        ]));
        assert_eq!(parsed.announce_url.as_deref(), Some("https://panel.example/news"));
        assert_eq!(parsed.promo_url.as_deref(), Some("https://p.example/sale"));
    }

    #[test]
    fn colour_markers_do_not_eat_the_announce_budget() {
        let word = "СЛОВО";
        let coloured = format!("#EF4444{word}");
        let announce = coloured.repeat(100);
        let parsed = SubHeaders::parse(&headers(&[("announce", announce.as_str())]));
        let kept = parsed.announce.expect("announce should be parsed");

        assert_eq!(kept.as_str(), announce);
        assert_eq!(kept.matches("#EF4444").count(), 100);

        let overflow = format!("{announce}{coloured}");
        let trimmed = SubHeaders::parse(&headers(&[("announce", overflow.as_str())]))
            .announce
            .expect("announce should be parsed");
        assert_eq!(trimmed.as_str(), announce);
    }

    #[test]
    fn colour_markers_only_count_when_a_word_follows() {
        let text = format!("#EF4444 {}", "a".repeat(ANNOUNCE_MAX_CHARS));
        let kept = SubHeaders::parse(&headers(&[("announce", text.as_str())]))
            .announce
            .expect("announce should be parsed");
        assert_eq!(kept.chars().count(), ANNOUNCE_MAX_CHARS);
        assert!(kept.starts_with("#EF4444 "));

        let junk = format!("#XYZ123{}", "b".repeat(ANNOUNCE_MAX_CHARS));
        let kept = SubHeaders::parse(&headers(&[("announce", junk.as_str())]))
            .announce
            .expect("announce should be parsed");
        assert_eq!(kept.chars().count(), ANNOUNCE_MAX_CHARS);
        assert!(kept.starts_with("#XYZ123"));
    }

    #[test]
    fn promo_shares_the_announce_colour_rules() {
        let promo = format!("#22C55EСкидка {}", "x".repeat(ANNOUNCE_MAX_CHARS));
        let kept = SubHeaders::parse(&headers(&[("clod-promo", promo.as_str())]))
            .promo
            .expect("promo should be parsed");
        assert!(kept.starts_with("#22C55EСкидка"));
        assert_eq!(kept.chars().count() - "#22C55E".chars().count(), ANNOUNCE_MAX_CHARS);
    }

    #[test]
    fn announce_and_announce_url_do_not_shadow_each_other() {
        let parsed = SubHeaders::parse(&headers(&[
            ("announce", "text"),
            ("announce-url", "https://panel.example/news"),
        ]));
        assert_eq!(parsed.announce.as_deref(), Some("text"));
        assert_eq!(parsed.announce_url.as_deref(), Some("https://panel.example/news"));
    }

    #[test]
    fn fallback_domain_is_kept_separate_from_fallback_url() {
        let parsed = SubHeaders::parse(&headers(&[
            ("fallback-url", "https://spare.example/sub"),
            ("fallback-domain", "spare2.example:8443"),
        ]));
        assert_eq!(parsed.fallback_url.as_deref(), Some("https://spare.example/sub"));
        assert_eq!(parsed.fallback_domain.as_deref(), Some("spare2.example:8443"));
        assert_eq!(
            swap_domain("https://old.example/sub?t=1", "spare2.example:8443").as_deref(),
            Some("https://spare2.example:8443/sub?t=1")
        );
    }

    #[test]
    fn reads_raw_utf8_header_values() {
        let mut map = HeaderMap::new();
        let Ok(name) = HeaderName::from_bytes(b"profile-title") else {
            unreachable!("static header name")
        };
        let Ok(raw) = HeaderValue::from_bytes("Тариф Pro".as_bytes()) else {
            unreachable!("byte header value")
        };
        map.insert(name, raw);

        assert_eq!(SubHeaders::parse(&map).profile_title.as_deref(), Some("Тариф Pro"));
    }

    #[test]
    fn empty_values_are_treated_as_absent() {
        let parsed = SubHeaders::parse(&headers(&[("profile-title", "   "), ("announce", "")]));
        assert_eq!(parsed.profile_title, None);
        assert_eq!(parsed.announce, None);
    }

    #[test]
    fn announce_is_capped() {
        let long = "я".repeat(700);
        let parsed = SubHeaders::parse(&headers(&[(
            "announce",
            &format!(
                "base64:{}",
                base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &long)
            ),
        )]));
        assert_eq!(parsed.announce.map(|text| text.chars().count()), Some(500));
    }

    #[test]
    fn hwid_limit_wins_over_other_states() {
        let parsed = SubHeaders::parse(&headers(&[
            ("x-hwid-active", "true"),
            ("x-hwid-max-devices-reached", "true"),
            ("x-hwid-max-devices", "3"),
        ]));
        assert_eq!(parsed.hwid_state, HwidState::LimitReached);
        assert_eq!(parsed.hwid_max_devices, Some(3));
        assert_eq!(parsed.hwid_state.as_str(), Some("limit"));

        let parsed = SubHeaders::parse(&headers(&[("x-hwid-limit", "true")]));
        assert_eq!(parsed.hwid_state, HwidState::LimitReached);

        let parsed = SubHeaders::parse(&headers(&[("x-hwid-not-supported", "true")]));
        assert_eq!(parsed.hwid_state, HwidState::NotSupported);

        let parsed = SubHeaders::parse(&headers(&[("x-hwid-limit", "false")]));
        assert_eq!(parsed.hwid_state, HwidState::Unknown);

        let parsed = SubHeaders::parse(&headers(&[("x-hwid-not-supported", "true"), ("x-hwid-limit", "true")]));
        assert_eq!(parsed.hwid_state, HwidState::NotSupported);
    }

    #[test]
    fn threshold_lists_are_validated() {
        assert_eq!(thresholds("80,90,100", 1, 100).as_deref(), Some(&[80, 90, 100][..]));
        assert_eq!(thresholds(" 50 , 90 ", 1, 100).as_deref(), Some(&[50, 90][..]));
        assert_eq!(thresholds("off", 1, 100), Some(Vec::new()));
        assert_eq!(thresholds("OFF", 1, 365), Some(Vec::new()));
        assert_eq!(thresholds("abc,90,0,150", 1, 100).as_deref(), Some(&[90][..]));
        assert_eq!(thresholds("abc,0,150", 1, 100), None);
        assert_eq!(thresholds("5,5,5", 1, 365).as_deref(), Some(&[5][..]));
        assert_eq!(
            thresholds("1,2,3,4,5,6,7,8,9,10,11,12", 1, 365).map(|v| v.len()),
            Some(10)
        );
    }

    #[test]
    fn happ_expire_toggle_enables_defaults() {
        let parsed = SubHeaders::parse(&headers(&[("notification-subs-expire", "true")]));
        assert_eq!(parsed.notify_expire_days.as_deref(), Some(DEFAULT_NOTIFY_EXPIRE_DAYS));

        let parsed = SubHeaders::parse(&headers(&[
            ("notification-subs-expire", "true"),
            ("notify-expire-days", "off"),
        ]));
        assert_eq!(parsed.notify_expire_days, Some(Vec::new()));
    }

    #[test]
    fn support_url_allows_contact_schemes_only() {
        assert_eq!(
            contact_url("https://help.example/chat").as_deref(),
            Some("https://help.example/chat")
        );
        assert_eq!(
            contact_url("tg://resolve?domain=support").as_deref(),
            Some("tg://resolve?domain=support")
        );
        assert_eq!(
            contact_url("mailto:help@example.com").as_deref(),
            Some("mailto:help@example.com")
        );
        assert_eq!(contact_url("file:///etc/passwd"), None);
        assert_eq!(contact_url(r"\\attacker\share\payload.exe"), None);
        assert_eq!(contact_url("javascript:alert(1)"), None);

        let parsed = SubHeaders::parse(&headers(&[("support-url", "file:///tmp/x")]));
        assert_eq!(parsed.support_url, None);
        let parsed = SubHeaders::parse(&headers(&[("support-url", "https://t.me/support")]));
        assert_eq!(parsed.support_url.as_deref(), Some("https://t.me/support"));
    }

    #[test]
    fn swaps_host_and_port_keeping_path_and_query() {
        assert_eq!(
            swap_domain("https://old.example/sub/abc?token=1", "new.example").as_deref(),
            Some("https://new.example/sub/abc?token=1")
        );
        assert_eq!(
            swap_domain("https://old.example/sub", "new.example:8443").as_deref(),
            Some("https://new.example:8443/sub")
        );
        assert_eq!(
            swap_domain("https://old.example/sub", "https://new.example/ignored").as_deref(),
            Some("https://new.example/sub")
        );
        assert_eq!(swap_domain("https://old.example/sub", "   "), None);
    }

    #[test]
    fn validates_new_url_candidates() {
        assert_eq!(
            validate_new_url("https://old.example/sub", "https://new.example/sub").as_deref(),
            Some("https://new.example/sub")
        );
        assert_eq!(
            validate_new_url("https://old.example/sub", "http://new.example/sub"),
            None
        );
        assert_eq!(
            validate_new_url("http://old.example/sub", "http://new.example/sub"),
            None
        );
        assert!(validate_new_url("http://old.example/sub", "https://new.example/sub").is_some());
        assert_eq!(validate_new_url("https://old.example/sub", "not a url"), None);
        assert_eq!(
            validate_new_url("https://old.example/sub", "ftp://new.example/sub"),
            None
        );
        assert_eq!(
            validate_new_url("https://old.example/sub", "https://old.example/sub"),
            None
        );
    }

    #[test]
    fn reads_the_panel_clock_from_the_date_header() {
        assert_eq!(
            SubHeaders::parse(&headers(&[("date", "Tue, 04 Aug 2026 17:24:33 GMT")])).server_time,
            Some(1_785_864_273)
        );
        assert_eq!(
            SubHeaders::parse(&headers(&[("date", "Tue, 04 Aug 2026 20:24:33 +0300")])).server_time,
            Some(1_785_864_273)
        );
        assert_eq!(
            super::http_date_secs("Tuesday, 04-Aug-26 17:24:33 GMT"),
            Some(1_785_864_273)
        );
        assert_eq!(super::http_date_secs("Tue Aug  4 17:24:33 2026"), Some(1_785_864_273));
        assert_eq!(super::http_date_secs("not a date"), None);
        assert_eq!(super::http_date_secs("Thu, 01 Jan 1970 00:00:00 GMT"), None);
        assert_eq!(SubHeaders::parse(&headers(&[])).server_time, None);
    }

    #[test]
    fn a_cached_answer_is_not_used_as_a_clock() {
        assert_eq!(
            SubHeaders::parse(&headers(
                &[("date", "Tue, 04 Aug 2026 17:24:33 GMT"), ("age", "86400"),]
            ))
            .server_time,
            None
        );
        assert_eq!(
            SubHeaders::parse(&headers(&[("date", "Tue, 04 Aug 2026 17:24:33 GMT"), ("age", "0"),])).server_time,
            Some(1_785_864_273)
        );
        assert_eq!(
            SubHeaders::parse(&headers(&[
                ("date", "Tue, 04 Aug 2026 17:24:33 GMT"),
                ("age", "not a number"),
            ]))
            .server_time,
            Some(1_785_864_273)
        );
    }

    #[test]
    fn banner_fixtures_match_the_frontend_scanner() {
        let raw = include_str!("../../../src/utils/banner-text.fixtures.json");
        let cases: Vec<serde_json::Value> = serde_json::from_str(raw).expect("fixtures must parse");
        assert!(cases.len() >= 5);
        for case in &cases {
            let name = case["name"].as_str().expect("fixture name");
            let input = case["input"].as_str().expect("fixture input");
            let limit = usize::try_from(case["limit"].as_u64().expect("fixture limit")).expect("limit fits usize");
            let expected = case["truncated"].as_str().expect("fixture truncated");
            assert_eq!(truncate_banner(input, limit), expected, "{name}");
        }
    }
}
