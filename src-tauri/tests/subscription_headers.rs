//! End-to-end check of the subscription header plumbing.
//!
//! The unit tests in `config::sub_headers` cover parsing in isolation; this one
//! drives a real HTTP request through `NetworkManager` against a throwaway
//! server so a regression in the wiring (headers dropped on the way out,
//! response headers not reaching the parser) is caught as well.

use std::time::Duration;

use app_lib::{
    config::{
        PrfItem,
        sub_headers::{HwidState, SubHeaders},
    },
    utils::network::{NetworkManager, ProxyType},
};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::TcpListener,
};

/// A valid, minimal mihomo config so the response passes the profile check.
const BODY: &str = "proxies:\n  - name: node\n    type: socks5\n    server: 127.0.0.1\n    port: 1080\n";

/// The header set a Remnawave panel would answer with.
const RESPONSE_HEADERS: &str = concat!(
    "subscription-userinfo: upload=1; download=2; total=0; expire=0\r\n",
    // "Тариф Pro\nдо 2027" — a multiline value can only travel base64 encoded
    "profile-title: base64:0KLQsNGA0LjRhCBQcm8=\r\n",
    "profile-web-page-url: https://panel.example/cab\r\n",
    "support-url: https://t.me/support\r\n",
    // "Плановые работы 20 августа\nс 02:00 до 04:00 МСК"
    "announce: base64:0J/Qu9Cw0L3QvtCy0YvQtSDRgNCw0LHQvtGC0YsgMjAg0LDQstCz0YPRgdGC0LAK0YEgMDI6MDAg0LTQviAwNDowMCDQnNCh0Jo=\r\n",
    "subscription-refill-date: 1785340800\r\n",
    "profile-update-interval: 12\r\n",
    "fallback-url: https://backup.example/sub\r\n",
    "x-amz-meta-new-domain: moved.example:8443\r\n",
    "x-hwid-active: true\r\n",
    "notify-traffic-percent: 50,90\r\n",
    "notify-expire-days: off\r\n",
);

/// Serve exactly one request and hand the raw request text back to the caller.
async fn serve_once(listener: TcpListener) -> String {
    let Ok((mut stream, _)) = listener.accept().await else {
        return String::new();
    };

    let mut request = Vec::new();
    let mut buffer = [0_u8; 1024];
    while let Ok(read) = stream.read(&mut buffer).await {
        if read == 0 {
            break;
        }
        request.extend_from_slice(&buffer[..read]);
        if request.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }

    let response = format!(
        "HTTP/1.1 200 OK\r\ncontent-type: text/yaml\r\ncontent-length: {}\r\n{RESPONSE_HEADERS}\r\n{BODY}",
        BODY.len()
    );
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.flush().await;

    String::from_utf8_lossy(&request).into_owned()
}

#[tokio::test]
async fn identity_headers_go_out_and_panel_headers_come_back() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap_or_else(|err| {
        unreachable!("could not bind a loopback listener: {err}");
    });
    let Ok(address) = listener.local_addr() else {
        unreachable!("listener has no local address");
    };
    let server = tokio::spawn(serve_once(listener));

    let mut identity = HeaderMap::new();
    identity.insert(HeaderName::from_static("accept"), HeaderValue::from_static("*/*"));
    identity.insert(
        HeaderName::from_static("x-hwid"),
        HeaderValue::from_static("0123456789abcdef0123456789abcdef"),
    );
    identity.insert(
        HeaderName::from_static("x-device-os"),
        HeaderValue::from_static("Linux"),
    );

    let url = format!("http://{address}/sub/token");
    let response = NetworkManager::new()
        .get_with_interrupt_and_headers(&url, ProxyType::None, Some(5), None, false, Some(&identity))
        .await
        .unwrap_or_else(|err| unreachable!("request failed: {err}"));

    // --- outgoing ---
    let request = tokio::time::timeout(Duration::from_secs(5), server)
        .await
        .unwrap_or_else(|_| unreachable!("mock server timed out"))
        .unwrap_or_else(|err| unreachable!("mock server panicked: {err}"))
        .to_ascii_lowercase();

    assert!(
        request.contains("x-hwid: 0123456789abcdef0123456789abcdef"),
        "{request}"
    );
    assert!(request.contains("x-device-os: linux"), "{request}");
    assert!(request.contains("accept: */*"), "{request}");
    // The panel matches the fork token case-sensitively in its SRR rules.
    // koala-style plain `name/version` — no `(Mihomo; os)` suffix anymore,
    // so the panel's device list shows the client version cleanly.
    assert!(
        request.contains(&format!("user-agent: clodclash/{}", env!("CARGO_PKG_VERSION"))),
        "{request}"
    );
    assert!(!request.contains("(mihomo"), "{request}");

    // --- incoming ---
    assert!(response.status().is_success());
    let parsed = SubHeaders::parse(response.headers());

    assert_eq!(parsed.profile_title.as_deref(), Some("Тариф Pro"));
    assert_eq!(parsed.home.as_deref(), Some("https://panel.example/cab"));
    assert_eq!(parsed.support_url.as_deref(), Some("https://t.me/support"));
    assert_eq!(
        parsed.announce.as_deref(),
        Some("Плановые работы 20 августа\nс 02:00 до 04:00 МСК")
    );
    assert_eq!(parsed.refill_date, Some(1_785_340_800));
    assert_eq!(parsed.update_interval_hours, Some(12));
    assert_eq!(parsed.fallback_url.as_deref(), Some("https://backup.example/sub"));
    assert_eq!(parsed.hwid_state, HwidState::Active);
    assert_eq!(parsed.notify_traffic_percent.as_deref(), Some(&[50, 90][..]));
    // `off` disables the reminders instead of falling back to the defaults.
    assert_eq!(parsed.notify_expire_days, Some(Vec::new()));

    // The object-storage prefixed `new-domain` must be picked up and applied to
    // the current URL, keeping path and query.
    assert_eq!(
        parsed.migration_target("https://old.example/sub/token?x=1").as_deref(),
        Some("https://moved.example:8443/sub/token?x=1")
    );

    assert_eq!(
        response.text_with_charset().unwrap_or_default().trim_start(),
        BODY.trim_start()
    );
}

/// Serve one request whose response has NO panel headers at all.
async fn serve_once_bare(listener: TcpListener) {
    let Ok((mut stream, _)) = listener.accept().await else {
        return;
    };

    let mut buffer = [0_u8; 1024];
    let mut request = Vec::new();
    while let Ok(read) = stream.read(&mut buffer).await {
        if read == 0 {
            break;
        }
        request.extend_from_slice(&buffer[..read]);
        if request.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }

    let response = format!(
        "HTTP/1.1 200 OK\r\ncontent-type: text/yaml\r\ncontent-length: {}\r\n\r\n{BODY}",
        BODY.len()
    );
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.flush().await;
}

/// Регрессия «удалил хедер в панели, а он висит в клиенте»: ответ панели без
/// clod-заголовков, пришедший поверх сохранённого профиля с промо/анонсом,
/// обязан ОЧИСТИТЬ эти поля (merge_panel_meta — замена, не слияние).
#[tokio::test]
async fn removed_panel_headers_clear_stored_values() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap_or_else(|err| {
        unreachable!("could not bind a loopback listener: {err}");
    });
    let Ok(address) = listener.local_addr() else {
        unreachable!("listener has no local address");
    };
    let server = tokio::spawn(serve_once_bare(listener));

    let url = format!("http://{address}/sub/token");
    let response = NetworkManager::new()
        .get_with_interrupt_and_headers(&url, ProxyType::None, Some(5), None, false, None)
        .await
        .unwrap_or_else(|err| unreachable!("request failed: {err}"));
    let _ = tokio::time::timeout(Duration::from_secs(5), server).await;

    // Parse the header-less response exactly the way `from_url` does.
    let sub = SubHeaders::parse(response.headers());
    assert_eq!(sub.promo, None);
    assert_eq!(sub.announce, None);
    assert_eq!(sub.portal_url, None);

    // The fresh item a subscription update would build from these headers.
    let fresh = PrfItem {
        promo: sub.promo.clone(),
        promo_url: sub.promo_url.clone(),
        announce: sub.announce.clone(),
        announce_url: sub.announce_url.clone(),
        portal_url: sub.portal_url.clone(),
        renew_url: sub.renew_url.clone(),
        topup_url: sub.topup_url.clone(),
        lock_mode: sub.lock_mode,
        ..PrfItem::default()
    };

    // The stored profile as it was before the panel dropped the headers.
    let mut stored = PrfItem {
        promo: Some("test block".into()),
        promo_url: Some("https://p.example/promo".into()),
        promo_seen: Some(true),
        announce: Some("Возникли проблемы?".into()),
        announce_url: Some("https://p.example/help".into()),
        portal_url: Some("https://p.example/cab".into()),
        renew_url: Some("https://p.example/renew".into()),
        topup_url: Some("https://p.example/topup".into()),
        lock_mode: Some(true),
        ..PrfItem::default()
    };

    stored.merge_panel_meta(&fresh);

    assert_eq!(stored.promo, None, "a promo the panel stopped sending must disappear");
    assert_eq!(stored.promo_url, None);
    assert_eq!(stored.announce, None, "a removed announce must disappear");
    assert_eq!(stored.announce_url, None);
    assert_eq!(stored.portal_url, None);
    assert_eq!(stored.renew_url, None);
    assert_eq!(stored.topup_url, None);
    assert_eq!(stored.lock_mode, None);
}

/// The one field deliberately NOT replaced: the panel-vs-device clock offset is
/// a measurement, not a provider setting. An answer that arrived without a
/// `Date` header says nothing about the device clock, so the last real reading
/// has to survive it — otherwise a single such answer would silently throw the
/// countdown back onto an unchecked clock.
#[test]
fn a_date_less_answer_keeps_the_last_clock_reading() {
    let measured = PrfItem {
        clock_skew: Some(-4_000),
        ..PrfItem::default()
    };
    let mut stored = PrfItem::default();
    stored.merge_panel_meta(&measured);
    assert_eq!(stored.clock_skew, Some(-4_000));

    // No `Date` in this answer: keep what we had.
    stored.merge_panel_meta(&PrfItem::default());
    assert_eq!(stored.clock_skew, Some(-4_000));

    // A fresh reading replaces the old one.
    stored.merge_panel_meta(&PrfItem {
        clock_skew: Some(7),
        ..PrfItem::default()
    });
    assert_eq!(stored.clock_skew, Some(7));
}

/// Ageing rule: a reading is applied while it is fresh and ignored once the
/// user has had a month to move the clock under it.
#[test]
fn a_stale_clock_reading_is_not_applied() {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default();

    let fresh = PrfItem {
        clock_skew: Some(900),
        updated: Some((now - 3_600) as usize),
        ..PrfItem::default()
    };
    assert_eq!(fresh.panel_clock_skew(), 900);

    let stale = PrfItem {
        clock_skew: Some(900),
        updated: Some((now - 31 * 24 * 60 * 60) as usize),
        ..PrfItem::default()
    };
    assert_eq!(stale.panel_clock_skew(), 0);

    // Never measured at all.
    assert_eq!(PrfItem::default().panel_clock_skew(), 0);
}
