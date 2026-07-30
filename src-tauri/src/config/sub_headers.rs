//! Subscription header support for Remnawave panels and the Happ header set.
//!
//! Upstream only understands `subscription-userinfo`, `profile-update-interval`,
//! `profile-web-page-url` and `content-disposition`. Everything else lives here
//! so the fork keeps a single, mergeable touch point inside
//! [`crate::config::prfitem`].
//!
//! Conventions implemented below (they apply to every header):
//! * lookup is case insensitive and suffix based, so object-storage prefixed
//!   variants (`x-amz-meta-announce`, `x-obs-meta-…`) are accepted;
//! * a value prefixed with `base64:` is base64 decoded to UTF-8, falling back to
//!   the raw value when decoding fails;
//! * empty values are treated as absent.

use crate::utils::hwid;
use base64::engine::general_purpose;
use reqwest::header::{ACCEPT, HeaderMap, HeaderName, HeaderValue};
use sha2::{Digest as _, Sha256};
use smartstring::alias::String;

/// Maximum amount of characters of an announce banner we keep.
pub const ANNOUNCE_MAX_CHARS: usize = 500;

/// Default expiry reminders (days before expiration).
pub const DEFAULT_NOTIFY_EXPIRE_DAYS: &[u32] = &[7, 3, 1];

/// Default traffic reminders (percent of the quota used).
pub const DEFAULT_NOTIFY_TRAFFIC_PERCENT: &[u32] = &[80, 90, 100];

/// Upper bound for the amount of thresholds a panel may configure.
const MAX_THRESHOLDS: usize = 10;

/// Build the identity headers sent with every subscription request.
///
/// `Accept: */*` is mandatory: Remnawave's subscription-response rules serve an
/// HTML landing page when the client looks like a browser.
///
/// The `x-*` device headers are only sent when the user keeps device
/// identification enabled (`verge.enable_hwid`, on by default).
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

/// Panel state for the current device, derived from the `x-hwid-*` response
/// headers.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum HwidState {
    /// Nothing hwid related came back.
    #[default]
    Unknown,
    /// `x-hwid-active: true` — the device is registered.
    Active,
    /// `x-hwid-not-supported: true` — the panel wants an id we did not send.
    NotSupported,
    /// Device limit reached; the body is a stub and must not overwrite anything.
    LimitReached,
}

impl HwidState {
    /// Value persisted in the profile item (`hwid_state`).
    pub const fn as_str(self) -> Option<&'static str> {
        match self {
            Self::Unknown => None,
            Self::Active => Some("ok"),
            Self::NotSupported => Some("not_supported"),
            Self::LimitReached => Some("limit"),
        }
    }
}

/// Everything the panel told us in the response headers.
#[derive(Debug, Clone, Default)]
pub struct SubHeaders {
    /// `profile-title` — provider supplied display name.
    pub profile_title: Option<String>,
    /// `profile-web-page-url` — customer dashboard.
    pub home: Option<String>,
    /// `support-url`.
    pub support_url: Option<String>,
    /// `announce` — provider message, newlines preserved.
    pub announce: Option<String>,
    /// `subscription-refill-date` as a unix timestamp in seconds.
    pub refill_date: Option<i64>,
    /// `profile-update-interval`, in hours as sent by the panel.
    pub update_interval_hours: Option<u64>,
    /// `fallback-url` — alternative host used when the primary one fails.
    pub fallback_url: Option<String>,
    /// `new-url` — full replacement subscription URL.
    pub new_url: Option<String>,
    /// `new-domain` — host (optionally `host:port`) replacement.
    pub new_domain: Option<String>,
    /// Device registration state.
    pub hwid_state: HwidState,
    /// Device limit reported by the panel, when it sends one.
    pub hwid_max_devices: Option<u32>,
    /// `notify-expire-days`; `Some(vec![])` means the panel disabled them.
    pub notify_expire_days: Option<Vec<u32>>,
    /// `notify-traffic-percent`; `Some(vec![])` means the panel disabled them.
    pub notify_traffic_percent: Option<Vec<u32>>,
}

impl SubHeaders {
    /// Parse a subscription response header map.
    pub fn parse(headers: &HeaderMap) -> Self {
        let hwid_limit = flag(headers, "x-hwid-max-devices-reached") || flag(headers, "x-hwid-limit");
        let hwid_not_supported = flag(headers, "x-hwid-not-supported");
        let hwid_active = flag(headers, "x-hwid-active");

        let hwid_state = if hwid_limit {
            HwidState::LimitReached
        } else if hwid_not_supported {
            HwidState::NotSupported
        } else if hwid_active {
            HwidState::Active
        } else {
            HwidState::Unknown
        };

        let notify_expire_days = value(headers, "notify-expire-days")
            .and_then(|raw| thresholds(&raw, 1, 365))
            .or_else(|| {
                // Happ compatibility: a bare toggle enables the defaults.
                (flag(headers, "notification-subs-expire")).then(|| DEFAULT_NOTIFY_EXPIRE_DAYS.to_vec())
            });

        Self {
            profile_title: value(headers, "profile-title"),
            home: value(headers, "profile-web-page-url"),
            support_url: value(headers, "support-url"),
            announce: value(headers, "announce").map(|text| truncate_chars(&text, ANNOUNCE_MAX_CHARS)),
            refill_date: value(headers, "subscription-refill-date").and_then(|raw| raw.trim().parse::<i64>().ok()),
            update_interval_hours: value(headers, "profile-update-interval").and_then(|raw| raw.trim().parse().ok()),
            fallback_url: value(headers, "fallback-url"),
            new_url: value(headers, "new-url"),
            new_domain: value(headers, "new-domain"),
            hwid_state,
            hwid_max_devices: value(headers, "x-hwid-max-devices").and_then(|raw| raw.trim().parse().ok()),
            notify_expire_days,
            notify_traffic_percent: value(headers, "notify-traffic-percent").and_then(|raw| thresholds(&raw, 1, 100)),
        }
    }
}

impl SubHeaders {
    /// Resolve the URL the provider wants us to move to.
    ///
    /// `new-url` wins over `new-domain`. The result is only a *candidate*: the
    /// caller must probe it before persisting (see `feat::profile`).
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

    /// Push the device state to the UI so it can raise the right dialog.
    ///
    /// Informational states stay silent.
    pub fn notify_device_state(&self) {
        let state = match self.hwid_state {
            HwidState::LimitReached | HwidState::NotSupported => self.hwid_state,
            HwidState::Unknown | HwidState::Active => return,
        };

        crate::core::handle::Handle::hwid_notice(serde_json::json!({
            "state": state.as_str(),
            "maxDevices": self.hwid_max_devices,
            "supportUrl": self.support_url.as_deref(),
            "announce": self.announce.as_deref(),
        }));
    }
}

/// Case insensitive, suffix based header lookup with `base64:` decoding.
fn value(headers: &HeaderMap, name: &str) -> Option<String> {
    for (key, raw) in headers.iter() {
        let key_lower = key.as_str().to_ascii_lowercase();
        let matches = key_lower
            .strip_suffix(name)
            .is_some_and(|prefix| prefix.is_empty() || prefix.ends_with('-'));
        if !matches {
            continue;
        }

        // `to_str` rejects bytes above 0x7F, but panels do put raw UTF-8 into
        // `profile-title` / `announce` instead of base64 encoding it. Reading
        // those bytes as UTF-8 is the difference between showing the provider's
        // name and silently ignoring the header.
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

/// Decode `base64:<payload>` values, keeping the raw text on failure.
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

    trimmed.into()
}

/// Tiny abstraction so several base64 alphabets can be tried in a loop.
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
    value(headers, name).is_some_and(|raw| {
        let lowered = raw.trim().to_ascii_lowercase();
        matches!(lowered.as_str(), "true" | "1" | "yes" | "on")
    })
}

/// Parse a comma separated threshold list.
///
/// * `off` (any case) disables the thresholds and yields an empty vector.
/// * Out-of-range and non-numeric entries are dropped.
/// * A value that ends up with no usable entry is treated as a missing header
///   (`None`), so the application defaults stay in effect.
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

fn truncate_chars(value: &str, limit: usize) -> String {
    if value.chars().count() <= limit {
        return value.into();
    }
    value.chars().take(limit).collect::<std::string::String>().into()
}

/// sha256 of an announce text, used to remember which banner was dismissed.
pub fn announce_hash(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    hex::encode(hasher.finalize()).into()
}

/// Replace host (and port when given) of `current`, keeping path and query.
///
/// Returns `None` when the result would not be a usable change.
pub fn swap_domain(current: &str, new_domain: &str) -> Option<String> {
    let mut url = tauri::Url::parse(current).ok()?;
    let domain = new_domain.trim().trim_end_matches('/');
    if domain.is_empty() {
        return None;
    }

    // Accept `example.com`, `example.com:8443` and a full `https://example.com`.
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

/// Validate a `new-url` candidate before it is probed.
///
/// Downgrading https to http is refused; the rest is left to the probe request.
pub fn validate_new_url(current: &str, candidate: &str) -> Option<String> {
    let candidate_url = tauri::Url::parse(candidate.trim()).ok()?;
    if !matches!(candidate_url.scheme(), "http" | "https") {
        return None;
    }
    if candidate_url.host_str().is_none_or(str::is_empty) {
        return None;
    }

    let current_is_https = tauri::Url::parse(current).is_ok_and(|url| url.scheme() == "https");
    if current_is_https && candidate_url.scheme() != "https" {
        return None;
    }

    (candidate_url.as_str() != current).then(|| candidate_url.as_str().into())
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_NOTIFY_EXPIRE_DAYS, HwidState, SubHeaders, announce_hash, decode_value, swap_domain, thresholds,
        validate_new_url,
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
            // A multiline announce can only travel base64 encoded: raw HTTP
            // header values must not contain newlines.
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
        // `renew-url` must not be read as `new-url`.
        let parsed = SubHeaders::parse(&headers(&[("renew-url", "https://evil.example/sub")]));
        assert_eq!(parsed.new_url, None);
    }

    #[test]
    fn decodes_base64_values() {
        // "Тариф Pro\nдо 2027" in base64
        assert_eq!(
            decode_value("base64:0KLQsNGA0LjRhCBQcm8K0LTQviAyMDI3"),
            "Тариф Pro\nдо 2027"
        );
        // url-safe, unpadded
        assert_eq!(decode_value("base64:SGVsbG8_"), "Hello?");
        // broken payload keeps the raw value
        assert_eq!(decode_value("base64:!!!not-base64!!!"), "base64:!!!not-base64!!!");
        // plain values pass through, trimmed
        assert_eq!(decode_value("  plain  "), "plain");
    }

    #[test]
    fn reads_raw_utf8_header_values() {
        // Panels are supposed to base64 encode non-ASCII values, and some do
        // not: they put raw UTF-8 bytes into the header. Those must still be
        // read rather than silently dropped.
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

        // A false-ish flag must not trip anything.
        let parsed = SubHeaders::parse(&headers(&[("x-hwid-limit", "false")]));
        assert_eq!(parsed.hwid_state, HwidState::Unknown);
    }

    #[test]
    fn threshold_lists_are_validated() {
        assert_eq!(thresholds("80,90,100", 1, 100).as_deref(), Some(&[80, 90, 100][..]));
        assert_eq!(thresholds(" 50 , 90 ", 1, 100).as_deref(), Some(&[50, 90][..]));
        assert_eq!(thresholds("off", 1, 100), Some(Vec::new()));
        assert_eq!(thresholds("OFF", 1, 365), Some(Vec::new()));
        // garbage entries dropped, valid ones kept
        assert_eq!(thresholds("abc,90,0,150", 1, 100).as_deref(), Some(&[90][..]));
        // fully invalid header behaves like a missing one
        assert_eq!(thresholds("abc,0,150", 1, 100), None);
        // duplicates removed, list capped at 10
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

        // Our own header wins when both are present.
        let parsed = SubHeaders::parse(&headers(&[
            ("notification-subs-expire", "true"),
            ("notify-expire-days", "off"),
        ]));
        assert_eq!(parsed.notify_expire_days, Some(Vec::new()));
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
        // a full URL as the value is tolerated
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
        // no downgrade from https
        assert_eq!(
            validate_new_url("https://old.example/sub", "http://new.example/sub"),
            None
        );
        // http source may stay http
        assert!(validate_new_url("http://old.example/sub", "http://new.example/sub").is_some());
        // rubbish and unsupported schemes are refused
        assert_eq!(validate_new_url("https://old.example/sub", "not a url"), None);
        assert_eq!(
            validate_new_url("https://old.example/sub", "ftp://new.example/sub"),
            None
        );
        // identical url is not a migration
        assert_eq!(
            validate_new_url("https://old.example/sub", "https://old.example/sub"),
            None
        );
    }

    #[test]
    fn announce_hash_is_stable_and_distinct() {
        assert_eq!(announce_hash("hello"), announce_hash("hello"));
        assert_ne!(announce_hash("hello"), announce_hash("hello!"));
        assert_eq!(announce_hash("hello").len(), 64);
    }
}
