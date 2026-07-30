//! End-to-end check of the subscription header plumbing.
//!
//! The unit tests in `config::sub_headers` cover parsing in isolation; this one
//! drives a real HTTP request through `NetworkManager` against a throwaway
//! server so a regression in the wiring (headers dropped on the way out,
//! response headers not reaching the parser) is caught as well.

use std::time::Duration;

use app_lib::{
    config::sub_headers::{HwidState, SubHeaders},
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
    assert!(request.contains("user-agent: clodclash/"), "{request}");
    assert!(request.contains("(mihomo; "), "{request}");

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
