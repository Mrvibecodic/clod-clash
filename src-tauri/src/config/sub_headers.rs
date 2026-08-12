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
//! * a value prefixed with `base64:` is base64 decoded to UTF-8; a payload that
//!   does not decode makes the header count as absent, so the literal
//!   `base64:…` can never reach a banner;
//! * empty values are treated as absent.

use crate::utils::hwid;
use base64::engine::general_purpose;
use reqwest::header::{ACCEPT, HeaderMap, HeaderName, HeaderValue};
use smartstring::alias::String;

/// Maximum amount of characters of an announce banner we keep.
pub const ANNOUNCE_MAX_CHARS: usize = 500;

/// Default expiry reminders (days before expiration).
pub const DEFAULT_NOTIFY_EXPIRE_DAYS: &[u32] = &[7, 3, 1];

/// Default traffic reminders (percent of the quota used).
pub const DEFAULT_NOTIFY_TRAFFIC_PERCENT: &[u32] = &[80, 90, 100];

/// Upper bound for the amount of thresholds a panel may configure.
const MAX_THRESHOLDS: usize = 10;

/// How many times in a row a provider may move the subscription to a new
/// address before we stop following. Guards against two panels bouncing the
/// client back and forth.
pub const MAX_MIGRATION_HOPS: u32 = 3;

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

/// clod:connect-mode — how the provider wants the traffic captured.
///
/// Deliberately a closed list: the Connect button drives exactly two targets,
/// and a free-form value would only mean «we did not understand you».
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectMode {
    /// `tun` — tunnel only, the system proxy stays down.
    Tun,
    /// `proxy` — system proxy only, the tunnel stays down.
    Proxy,
    /// `both` — raise both targets together.
    Both,
}

impl ConnectMode {
    /// Value persisted in the profile item (`connect_mode`).
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Tun => "tun",
            Self::Proxy => "proxy",
            Self::Both => "both",
        }
    }

    /// Parse a header value. Unknown wording is treated as no header at all,
    /// the same rule the boolean headers follow: a typo in the panel must not
    /// silently switch how the client connects.
    fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "tun" | "tunnel" | "vpn" => Some(Self::Tun),
            "proxy" | "system" | "system-proxy" | "sysproxy" => Some(Self::Proxy),
            "both" | "all" => Some(Self::Both),
            _ => None,
        }
    }
}

/// clod:latency-style — как показывать задержку сервера.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LatencyStyle {
    /// `bars` — наши четыре полоски (умолчание, писать явно не обязательно).
    Bars,
    /// `dot` — цветная точка, как у панелей под Happ и Prizrak-Box.
    Dot,
    /// `number` — задержка числом в миллисекундах.
    Number,
}

impl LatencyStyle {
    /// Value persisted in the profile item (`latency_style`).
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

/// Everything the panel told us in the response headers.
#[derive(Debug, Clone, Default)]
pub struct SubHeaders {
    /// `profile-title` — provider supplied display name.
    pub profile_title: Option<String>,
    /// `profile-logo` — provider logo shown next to the subscription.
    pub profile_logo: Option<String>,
    /// `profile-web-page-url` — customer dashboard.
    pub home: Option<String>,
    /// `support-url`.
    pub support_url: Option<String>,
    /// `announce` — provider message, newlines preserved.
    pub announce: Option<String>,
    /// `announce-url` — where the announce banner leads when clicked.
    pub announce_url: Option<String>,
    /// `subscription-refill-date` as a unix timestamp in seconds.
    pub refill_date: Option<i64>,
    /// `profile-update-interval`, in hours as sent by the panel.
    pub update_interval_hours: Option<u64>,
    /// `fallback-url` — alternative full URL used when the primary one fails.
    pub fallback_url: Option<String>,
    /// `fallback-domain` — alternative host for the primary URL, tried after
    /// `fallback-url`.
    pub fallback_domain: Option<String>,
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
    /// The provider's preferred interface mode. `None` means it did not say.
    ///
    /// A user who picked a mode themselves always wins over this.
    pub simple_mode: Option<bool>,
    /// `clod-portal-url` — the customer portal where the plan is renewed.
    ///
    /// Deliberately separate from `profile-web-page-url`: Remnawave panels
    /// usually point that one at the subscription page itself, which is not
    /// where a customer pays.
    pub portal_url: Option<String>,
    /// `clod-promo` — a temporary promotion banner. Unlike `announce` it is
    /// dismissable and expected to disappear once the panel stops sending it.
    pub promo: Option<String>,
    /// `clod-promo-url` — where the promo banner leads when clicked.
    pub promo_url: Option<String>,
    /// `clod-hwid-limit` — provider text shown inside the device dialogs.
    ///
    /// Optional and deliberately separate from [`Self::announce`]: the banner on
    /// the home screen and the explanation of a blocked device are two different
    /// messages, and a provider must be able to write the second one without
    /// putting it in front of everybody else.
    pub hwid_limit_message: Option<String>,
    /// clod:latency-style — `clod-latency-style`: `bars`, `dot` or `number`.
    ///
    /// Purely cosmetic, and that is the point: panels written for Happ and
    /// Prizrak-Box show latency as a coloured dot, and their instructions,
    /// screenshots and support scripts all say «dot». Free compatibility.
    /// Compatibility: `pxa-latency-dots: 1` means the same as `dot`.
    pub latency_style: Option<LatencyStyle>,

    /// clod:device-remove — `clod-device-remove`: where the customer frees a
    /// device slot themselves.
    ///
    /// The device-limit dialog could only offer «Support», so the customer had
    /// to ask a human for something the panel does on its own page. Validated
    /// like every other action link (https only).
    pub device_remove_url: Option<String>,

    /// `clod-lock-mode` — the panel forbids changing proxy/routing modes in
    /// the app. `global-mode: false` (Prizrak-Box) is honoured as a synonym.
    pub lock_mode: Option<bool>,

    /// clod:connect-mode — `clod-connect-mode`: what the Connect button should
    /// raise, one of `tun`, `proxy`, `both`.
    ///
    /// Until this header existed only the user could choose between the two
    /// targets, and a locked profile (`clod-lock-mode`) hides those switches —
    /// every customer of such a panel stayed on the system proxy for good. The
    /// tunnel is the better default for private customers: an application that
    /// died leaves a system proxy pointing at a dead `127.0.0.1` and the whole
    /// machine looks offline, while a tunnel that is gone simply stops routing.
    pub connect_mode: Option<ConnectMode>,

    /// clod:show-0hosts — `clod-show-0hosts`: провайдер просит показывать его
    /// узлы-заглушки как есть, вместо наших экранов «нет серверов».
    pub show_zero_hosts: Option<bool>,
    /// `Date` — the panel's own clock at the moment it answered, unix seconds.
    ///
    /// Plain HTTP, sent by every server that has a clock, so it costs neither a
    /// request nor a header agreement with the provider. Compared with the
    /// device clock at the same moment it says how far the device is off; see
    /// [`crate::config::prfitem::PrfItem::clock_skew`].
    pub server_time: Option<i64>,
}

impl SubHeaders {
    /// Parse a subscription response header map.
    pub fn parse(headers: &HeaderMap) -> Self {
        let hwid_limit = flag(headers, "x-hwid-max-devices-reached") || flag(headers, "x-hwid-limit");
        let hwid_not_supported = flag(headers, "x-hwid-not-supported");
        let hwid_active = flag(headers, "x-hwid-active");

        // `x-hwid-not-supported` идёт первым намеренно. Remnawave 3.x в ветке
        // блокировки по устройствам ставит `x-hwid-limit: true` **всегда**, а
        // `x-hwid-max-devices-reached` — только при настоящем превышении. Пара
        // «not-supported + limit» означает «клиент не прислал идентификатор», и
        // пользователю надо предложить включить его, а не сообщать о лимите,
        // которого он не достигал. Тело в обеих ветках — заглушка, поэтому
        // `PrfItem::from_url` прерывает обновление и на `NotSupported` тоже.
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
            .or_else(|| {
                // Happ compatibility: a bare toggle enables the defaults.
                (flag(headers, "notification-subs-expire")).then(|| DEFAULT_NOTIFY_EXPIRE_DAYS.to_vec())
            });

        Self {
            profile_title: value(headers, "profile-title"),
            profile_logo: value(headers, "profile-logo").and_then(|raw| https_url(&raw)),
            home: value(headers, "profile-web-page-url").and_then(|raw| https_url(&raw)),
            support_url: value(headers, "support-url").and_then(|raw| contact_url(&raw)),
            announce: value(headers, "announce").map(|text| truncate_banner(&text, ANNOUNCE_MAX_CHARS)),
            announce_url: value(headers, "announce-url").and_then(|raw| https_url(&raw)),
            refill_date: value(headers, "subscription-refill-date").and_then(|raw| raw.trim().parse::<i64>().ok()),
            update_interval_hours: value(headers, "profile-update-interval").and_then(|raw| raw.trim().parse().ok()),
            // clod: запасной адрес — такой же путь к подписке, как основной, и
            // проверять его надо не слабее. Без этого панель (или тот, кто её
            // подменил) могла заголовком увести загрузку подписки на plain
            // HTTP: для `new-url` даунгрейд запрещён явно, а здесь значение
            // уходило в профиль как есть.
            fallback_url: value(headers, "fallback-url").and_then(|raw| https_url(&raw)),
            fallback_domain: value(headers, "fallback-domain"),
            new_url: value(headers, "new-url"),
            new_domain: value(headers, "new-domain"),
            hwid_state,
            hwid_max_devices: value(headers, "x-hwid-max-devices").and_then(|raw| raw.trim().parse().ok()),
            notify_expire_days,
            notify_traffic_percent: value(headers, "notify-traffic-percent").and_then(|raw| thresholds(&raw, 1, 100)),
            // Our own header wins; the other two keep panels that were already
            // configured for FlClashX or Prizrak-Box working out of the box.
            simple_mode: bool_value(headers, "clod-simple-mode")
                .or_else(|| bool_value(headers, "pxa-simple-mode"))
                .or_else(|| bool_value(headers, "flclashx-newboard")),
            portal_url: value(headers, "clod-portal-url").and_then(|raw| https_url(&raw)),
            promo: value(headers, "clod-promo").map(|text| truncate_banner(&text, ANNOUNCE_MAX_CHARS)),
            promo_url: value(headers, "clod-promo-url").and_then(|raw| https_url(&raw)),
            // clod:latency-style — закрытый список; синоним Prizrak-Box
            // включает точку, но выключить наш вид собой не может (у него нет
            // отдельного значения «полоски»).
            latency_style: value(headers, "clod-latency-style")
                .as_deref()
                .and_then(LatencyStyle::parse)
                .or_else(|| bool_value(headers, "pxa-latency-dots").and_then(|dots| dots.then_some(LatencyStyle::Dot))),
            // clod:device-remove — та же проверка, что у портала и поддержки.
            device_remove_url: value(headers, "clod-device-remove").and_then(|raw| https_url(&raw)),
            hwid_limit_message: value(headers, "clod-hwid-limit")
                .map(|text| truncate_banner(&text, ANNOUNCE_MAX_CHARS)),
            // `global-mode: false` means "hide the mode switch" for Prizrak-Box
            // configured panels, which is exactly our lock.
            // clod:show-0hosts — без заголовка ведём себя как раньше: узнаём
            // заглушки и объясняем словами. `true` отдаёт экран панели ей же.
            show_zero_hosts: bool_value(headers, "clod-show-0hosts"),
            lock_mode: bool_value(headers, "clod-lock-mode")
                .or_else(|| bool_value(headers, "global-mode").map(|allowed| !allowed)),
            // clod:connect-mode — закрытый список значений; всё остальное
            // считается отсутствующим заголовком.
            connect_mode: value(headers, "clod-connect-mode")
                .as_deref()
                .and_then(ConnectMode::parse),
            // Straight `get`: `Date` is a standard header, so neither the
            // suffix lookup nor the base64 decoding above applies to it.
            //
            // A non-zero `Age` means the answer sat in a cache: its `Date` is
            // the moment the cache took it, not now, and reading a clock off
            // it would put the whole cache lifetime into the offset. Better no
            // measurement than one that is a day out by construction.
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

/// Whether the answer came out of a cache rather than from the panel itself.
///
/// `Age` is only sent by caches, and a value above zero says how long the copy
/// has been sitting there. Absent or `0` — treat it as first hand.
fn is_cached(headers: &HeaderMap) -> bool {
    headers
        .get(reqwest::header::AGE)
        .and_then(|raw| raw.to_str().ok())
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .is_some_and(|age| age > 0)
}

/// Unix seconds from an HTTP-date (RFC 9110 §5.6.7).
///
/// Servers send the IMF-fixdate form (`Tue, 04 Aug 2026 17:24:33 GMT`), which
/// is parsed explicitly; the RFC 2822 pass after it covers the numeric-offset
/// spelling some proxies produce. The two obsolete formats the RFC still
/// requires readers to accept come last.
///
/// A value outside 2020…2100 is dropped: a header that far off is a broken
/// clock or a mangled proxy value, and correcting a countdown by it would be
/// worse than not correcting it at all.
fn http_date_secs(raw: &str) -> Option<i64> {
    const MIN: i64 = 1_577_836_800; // 2020-01-01
    const MAX: i64 = 4_102_444_800; // 2100-01-01

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
            // clod:device-remove — прямая дорога к отвязке устройства. Без неё
            // единственной кнопкой в диалоге лимита была «Поддержка»: человек
            // писал в чат ровно за тем, что панель умеет сделать сама.
            "removeUrl": self.device_remove_url.as_deref(),
            // `clod-hwid-limit`, not `announce`: the dialog explains a blocked
            // device, and that text has no business on the home banner.
            "message": self.hwid_limit_message.as_deref(),
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

    // The value claimed to be base64 and was not: treat it as absent rather
    // than leaking the literal `base64:...` string into a user-facing banner.
    String::new()
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
    bool_value(headers, name).unwrap_or(false)
}

/// Tri-state flag: `None` when the header is absent or not a boolean at all.
fn bool_value(headers: &HeaderMap, name: &str) -> Option<bool> {
    let raw = value(headers, name)?;
    match raw.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Some(true),
        "false" | "0" | "no" | "off" => Some(false),
        _ => None,
    }
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

/// Strictly-https validation for every link and logo the panel sends.
///
/// The UI loads or opens these values on a single click from content the
/// panel fully controls, so `javascript:`, `file:` **and plain `http:`** must
/// never survive: an http link is both a downgrade and a marker of a
/// misconfigured panel.
fn https_url(value: &str) -> Option<String> {
    let parsed = tauri::Url::parse(value.trim()).ok()?;
    if parsed.scheme() != "https" {
        return None;
    }
    parsed.host_str().filter(|host| !host.is_empty())?;
    Some(parsed.as_str().into())
}

/// Like `https_url`, plus the contact schemes a support link legitimately
/// uses (`tg:`, `mailto:`). The value ends up behind a prominent one-click
/// button, so a compromised panel must not be able to smuggle `file:`, a UNC
/// path or a plain-http downgrade in.
fn contact_url(value: &str) -> Option<String> {
    let parsed = tauri::Url::parse(value.trim()).ok()?;
    match parsed.scheme() {
        "https" => {
            parsed.host_str().filter(|host| !host.is_empty())?;
        }
        "tg" | "mailto" => {}
        _ => return None,
    }
    // The original text, not `parsed.as_str()`: parsing is validation here,
    // and normalising would append a trailing slash the panel never sent.
    Some(value.trim().into())
}

/// Length of a `#RRGGBB` colour marker.
const COLOUR_MARKER_LEN: usize = 7;

/// Is there a `#RRGGBB` colour marker at `chars[index]`?
///
/// clod: a banner may paint single words — `#EF4444ВАЖНО` shows `ВАЖНО` in
/// red. The syntax is Prizrak-Box's, so panels already configured for it work
/// with us as they are: the marker binds to the word right after it, and a
/// marker followed by a space is plain text.
fn colour_marker_at(chars: &[char], index: usize) -> bool {
    if chars.get(index) != Some(&'#') || index + COLOUR_MARKER_LEN > chars.len() {
        return false;
    }
    // Same condition as the renderer: no word after the marker, no marker.
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

/// Trim a banner to `limit` *visible* characters.
///
/// Colour markers are formatting, not content, so they pass through without
/// eating the budget — otherwise a provider who colours a few words silently
/// loses the tail of the text.
fn truncate_banner(value: &str, limit: usize) -> String {
    if value.chars().count() <= limit {
        return value.into();
    }

    let chars: Vec<char> = value.chars().collect();
    let mut out = std::string::String::with_capacity(value.len());
    let mut visible = 0;
    let mut index = 0;

    while index < chars.len() {
        // Бюджет проверяется первым: иначе за границей лимита оставался бы
        // висячий маркер, который рендерер показал бы как обычный текст.
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

#[allow(clippy::expect_used)]
#[cfg(test)]
mod tests {
    use super::{
        ANNOUNCE_MAX_CHARS, ConnectMode, DEFAULT_NOTIFY_EXPIRE_DAYS, HwidState, LatencyStyle, SubHeaders, contact_url,
        decode_value, swap_domain, thresholds, validate_new_url,
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
        // A value that claims to be base64 but is not decodable is dropped —
        // the literal prefix must never surface in a banner.
        assert_eq!(decode_value("base64:!!!not-base64!!!"), "");
        // plain values pass through, trimmed
        assert_eq!(decode_value("  plain  "), "plain");
    }

    #[test]
    fn simple_mode_reads_our_header_first_and_tolerates_the_others() {
        let parsed = SubHeaders::parse(&headers(&[("clod-simple-mode", "1")]));
        assert_eq!(parsed.simple_mode, Some(true));

        let parsed = SubHeaders::parse(&headers(&[("clod-simple-mode", "false")]));
        assert_eq!(parsed.simple_mode, Some(false));

        // Panels already configured for other clients keep working.
        let parsed = SubHeaders::parse(&headers(&[("pxa-simple-mode", "1")]));
        assert_eq!(parsed.simple_mode, Some(true));
        let parsed = SubHeaders::parse(&headers(&[("flclashx-newboard", "true")]));
        assert_eq!(parsed.simple_mode, Some(true));

        // Ours wins when several are present.
        let parsed = SubHeaders::parse(&headers(&[("clod-simple-mode", "0"), ("pxa-simple-mode", "1")]));
        assert_eq!(parsed.simple_mode, Some(false));

        // Absent or nonsense means "the provider did not say".
        assert_eq!(SubHeaders::parse(&headers(&[])).simple_mode, None);
        let parsed = SubHeaders::parse(&headers(&[("clod-simple-mode", "maybe")]));
        assert_eq!(parsed.simple_mode, None);
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

        // The portal is our own header on purpose; `profile-web-page-url`
        // usually points at the subscription page and must not leak into it.
        let parsed = SubHeaders::parse(&headers(&[("profile-web-page-url", "https://panel.example/sub/abc")]));
        assert_eq!(parsed.portal_url, None);

        // Action URLs go through the same https-only filter as the logo.
        let parsed = SubHeaders::parse(&headers(&[("clod-portal-url", "javascript:alert(1)")]));
        assert_eq!(parsed.portal_url, None);

        // Absent headers mean absent buttons — that is the default.
        let parsed = SubHeaders::parse(&headers(&[]));
        assert_eq!(parsed.portal_url, None);
        assert_eq!(parsed.promo, None);
    }

    // clod: текст для диалогов устройства — свой заголовок, а не `announce`.
    #[test]
    fn hwid_limit_message_is_its_own_header() {
        let parsed = SubHeaders::parse(&headers(&[
            ("announce", "banner for everybody"),
            // "Отвязать устройство можно в кабинете" in base64
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

        // Необязательный: без него диалог просто без пояснения провайдера, а
        // `announce` в него больше не подставляется.
        let parsed = SubHeaders::parse(&headers(&[("announce", "banner for everybody")]));
        assert_eq!(parsed.hwid_limit_message, None);

        // Суффиксный поиск не должен спутать его с флагом `x-hwid-limit`.
        let parsed = SubHeaders::parse(&headers(&[("x-hwid-limit", "true")]));
        assert_eq!(parsed.hwid_limit_message, None);
        assert_eq!(parsed.hwid_state, HwidState::LimitReached);
        let parsed = SubHeaders::parse(&headers(&[("clod-hwid-limit", "текст для диалога")]));
        assert_eq!(parsed.hwid_state, HwidState::Unknown);

        // Тот же лимит, что и у баннеров.
        let long = "я".repeat(700);
        let parsed = SubHeaders::parse(&headers(&[("clod-hwid-limit", long.as_str())]));
        assert_eq!(
            parsed.hwid_limit_message.map(|text| text.chars().count()),
            Some(ANNOUNCE_MAX_CHARS)
        );
    }

    #[test]
    fn show_zero_hosts_is_off_unless_the_panel_asks() {
        // Заголовка нет — прежнее поведение: заглушки разбираем сами.
        assert_eq!(SubHeaders::parse(&headers(&[])).show_zero_hosts, None);
        // Значения — те же, что у остальных наших булевых заголовков.
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
        // Мусор — это не «включено»: молча включить чужой экран страшнее, чем
        // проигнорировать кривую настройку панели.
        assert_eq!(
            SubHeaders::parse(&headers(&[("clod-show-0hosts", "maybe")])).show_zero_hosts,
            None
        );
        // Суффиксный поиск работает и здесь: объектные хранилища добавляют свой
        // префикс к пользовательским метаданным.
        assert_eq!(
            SubHeaders::parse(&headers(&[("x-amz-meta-clod-show-0hosts", "true")])).show_zero_hosts,
            Some(true)
        );
    }

    #[test]
    fn lock_mode_reads_our_header_and_the_prizrak_synonym() {
        let parsed = SubHeaders::parse(&headers(&[("clod-lock-mode", "1")]));
        assert_eq!(parsed.lock_mode, Some(true));

        // Prizrak-Box panels say `global-mode: false` to hide the switch.
        let parsed = SubHeaders::parse(&headers(&[("global-mode", "false")]));
        assert_eq!(parsed.lock_mode, Some(true));
        let parsed = SubHeaders::parse(&headers(&[("global-mode", "true")]));
        assert_eq!(parsed.lock_mode, Some(false));

        // Ours wins when both are present.
        let parsed = SubHeaders::parse(&headers(&[("clod-lock-mode", "0"), ("global-mode", "false")]));
        assert_eq!(parsed.lock_mode, Some(false));

        assert_eq!(SubHeaders::parse(&headers(&[])).lock_mode, None);
    }

    // clod:connect-mode — способ подключения задаётся закрытым списком.
    #[test]
    fn connect_mode_takes_a_closed_list_of_values() {
        let parsed = SubHeaders::parse(&headers(&[("clod-connect-mode", "TUN")]));
        assert_eq!(parsed.connect_mode, Some(ConnectMode::Tun));
        let parsed = SubHeaders::parse(&headers(&[("clod-connect-mode", " proxy ")]));
        assert_eq!(parsed.connect_mode, Some(ConnectMode::Proxy));
        let parsed = SubHeaders::parse(&headers(&[("clod-connect-mode", "both")]));
        assert_eq!(parsed.connect_mode, Some(ConnectMode::Both));

        // Синонимы, на которых спотыкается живой администратор панели.
        assert_eq!(
            SubHeaders::parse(&headers(&[("clod-connect-mode", "system-proxy")])).connect_mode,
            Some(ConnectMode::Proxy)
        );
        assert_eq!(
            SubHeaders::parse(&headers(&[("clod-connect-mode", "vpn")])).connect_mode,
            Some(ConnectMode::Tun)
        );

        // Опечатка НЕ меняет способ подключения молча: заголовок с непонятным
        // значением считается отсутствующим, выбор остаётся за пользователем.
        assert_eq!(
            SubHeaders::parse(&headers(&[("clod-connect-mode", "tunnel-only")])).connect_mode,
            None
        );
        assert_eq!(SubHeaders::parse(&headers(&[])).connect_mode, None);
    }

    // clod:latency-style + clod:device-remove — косметика и выход из тупика.
    #[test]
    fn latency_style_and_device_removal_link() {
        let parsed = SubHeaders::parse(&headers(&[("clod-latency-style", "Dot")]));
        assert_eq!(parsed.latency_style, Some(LatencyStyle::Dot));
        let parsed = SubHeaders::parse(&headers(&[("clod-latency-style", "number")]));
        assert_eq!(parsed.latency_style, Some(LatencyStyle::Number));

        // Панель под Prizrak-Box просит точку своим заголовком.
        let parsed = SubHeaders::parse(&headers(&[("pxa-latency-dots", "1")]));
        assert_eq!(parsed.latency_style, Some(LatencyStyle::Dot));
        // `0` у синонима означает «ничего не прошу», а не «верни полоски»:
        // отдельного значения для полосок у него нет, и выдумывать его —
        // значит спорить с нашим же умолчанием.
        assert_eq!(
            SubHeaders::parse(&headers(&[("pxa-latency-dots", "0")])).latency_style,
            None
        );
        // Наш заголовок сильнее синонима.
        let parsed = SubHeaders::parse(&headers(&[("clod-latency-style", "bars"), ("pxa-latency-dots", "1")]));
        assert_eq!(parsed.latency_style, Some(LatencyStyle::Bars));
        assert_eq!(
            SubHeaders::parse(&headers(&[("clod-latency-style", "blink")])).latency_style,
            None
        );

        // Ссылка отвязки проверяется как любая другая: только https.
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
    fn promo_does_not_shadow_its_url_and_a_renew_url_is_not_new_url() {
        let parsed = SubHeaders::parse(&headers(&[
            ("clod-promo", "sale"),
            ("clod-promo-url", "https://p.example/sale"),
            // Заголовка `clod-renew-url` у нас больше нет, но панель может
            // слать его для других клиентов — и он не должен читаться как
            // запрос на переезд.
            ("clod-renew-url", "https://p.example/renew"),
        ]));
        assert_eq!(parsed.promo.as_deref(), Some("sale"));
        assert_eq!(parsed.promo_url.as_deref(), Some("https://p.example/sale"));
        assert_eq!(parsed.new_url, None);
    }

    #[test]
    fn every_link_header_requires_https() {
        // clod: голый http запрещён во ВСЕХ URL-хедерах (решение 31.07) —
        // это и защита от даунгрейда, и маркер кривой настройки панели.
        let parsed = SubHeaders::parse(&headers(&[
            ("profile-logo", "http://cdn.example/logo.png"),
            ("profile-web-page-url", "http://panel.example/home"),
            ("support-url", "http://help.example/chat"),
            ("clod-portal-url", "http://my.provider.example/cabinet"),
            // clod: запасной адрес подписки — такой же путь к конфигу, как
            // основной. Раньше он один уходил в профиль без проверки, то есть
            // заголовком можно было увести загрузку подписки на plain HTTP.
            ("fallback-url", "http://backup.panel.example/sub/token"),
        ]));
        assert_eq!(parsed.profile_logo, None);
        assert_eq!(parsed.home, None);
        assert_eq!(parsed.support_url, None);
        assert_eq!(parsed.portal_url, None);
        assert_eq!(parsed.fallback_url, None);

        // https-versions of the same values survive untouched.
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

        // A compromised panel must not be able to hand the UI a script or a
        // local file to open.
        let parsed = SubHeaders::parse(&headers(&[
            ("profile-logo", "javascript:alert(1)"),
            ("announce-url", "file:///etc/passwd"),
        ]));
        assert_eq!(parsed.profile_logo, None);
        assert_eq!(parsed.announce_url, None);
    }

    #[test]
    fn banner_links_require_https() {
        // clod: `announce-url` / `clod-promo-url` открываются в один клик из
        // баннера — голый http там не принимается вовсе (решение 31.07).
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

    // clod: `#RRGGBB` — разметка, а не текст: она не должна съедать бюджет в
    // 500 символов, иначе провайдер, покрасивший пару слов, молча теряет хвост
    // объявления. Синтаксис общий с Prizrak-Box, поэтому маркер без слова
    // после него (перед пробелом) — обычный текст и считается как текст.
    #[test]
    fn colour_markers_do_not_eat_the_announce_budget() {
        let word = "СЛОВО";
        let coloured = format!("#EF4444{word}");
        // 100 покрашенных слов = 500 видимых символов ровно на лимите.
        let announce = coloured.repeat(100);
        let parsed = SubHeaders::parse(&headers(&[("announce", announce.as_str())]));
        let kept = parsed.announce.expect("announce should be parsed");

        assert_eq!(kept.as_str(), announce);
        assert_eq!(kept.matches("#EF4444").count(), 100);

        // Сто первое слово уже за лимитом: маркер сохранён быть не может,
        // потому что красить нечего.
        let overflow = format!("{announce}{coloured}");
        let trimmed = SubHeaders::parse(&headers(&[("announce", overflow.as_str())]))
            .announce
            .expect("announce should be parsed");
        assert_eq!(trimmed.as_str(), announce);
    }

    #[test]
    fn colour_markers_only_count_when_a_word_follows() {
        // Маркер перед пробелом и невалидный hex — это просто текст, поэтому
        // они занимают место в лимите как обычные символы.
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
        // Видимых символов ровно лимит: семь символов маркера не в счёт.
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

        // Remnawave шлёт `x-hwid-limit` в обеих ветках блокировки, поэтому
        // «идентификатор не прислан» должен побеждать: пользователю надо
        // предложить включить идентификацию, а не сообщить о чужом лимите.
        let parsed = SubHeaders::parse(&headers(&[("x-hwid-not-supported", "true"), ("x-hwid-limit", "true")]));
        assert_eq!(parsed.hwid_state, HwidState::NotSupported);
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
        // one-click button — no local execution vectors from a panel
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
    fn reads_the_panel_clock_from_the_date_header() {
        // IMF-fixdate: what every panel actually sends.
        assert_eq!(
            SubHeaders::parse(&headers(&[("date", "Tue, 04 Aug 2026 17:24:33 GMT")])).server_time,
            Some(1_785_864_273)
        );
        // Numeric offset, produced by some proxies in front of the panel.
        assert_eq!(
            SubHeaders::parse(&headers(&[("date", "Tue, 04 Aug 2026 20:24:33 +0300")])).server_time,
            Some(1_785_864_273)
        );
        // Obsolete formats readers must still accept.
        assert_eq!(
            super::http_date_secs("Tuesday, 04-Aug-26 17:24:33 GMT"),
            Some(1_785_864_273)
        );
        assert_eq!(super::http_date_secs("Tue Aug  4 17:24:33 2026"), Some(1_785_864_273));
        // Rubbish, and a clock so far off that correcting by it would be worse
        // than leaving the device clock alone.
        assert_eq!(super::http_date_secs("not a date"), None);
        assert_eq!(super::http_date_secs("Thu, 01 Jan 1970 00:00:00 GMT"), None);
        assert_eq!(SubHeaders::parse(&headers(&[])).server_time, None);
    }

    #[test]
    fn a_cached_answer_is_not_used_as_a_clock() {
        // `Age` above zero: the date belongs to the cache, not to the panel.
        assert_eq!(
            SubHeaders::parse(&headers(
                &[("date", "Tue, 04 Aug 2026 17:24:33 GMT"), ("age", "86400"),]
            ))
            .server_time,
            None
        );
        // `Age: 0` and rubbish values mean nothing was cached.
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
}
