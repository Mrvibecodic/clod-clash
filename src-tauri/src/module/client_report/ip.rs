//! Внешний адрес клиента в текущей сети — тот, с которого идут замеры.
//!
//! Спрашивается у Яндекса (адреса страницы yandex.ru/internet), только у
//! защищённой подписки: сразу, как сменилась сеть, и раз в час в одной сети. Запрос идёт без прокси приложения. При TUN он проходит по
//! правилам подписки, поэтому провайдеру нужно отправить эти два адреса в
//! DIRECT своего шаблона — иначе виден адрес выхода узла, а не клиента.

use std::net::IpAddr;
use std::time::Duration;

use crate::utils::network::{NetworkManager, ProxyType};

const IPV4_URL: &str = "https://ipv4-internet.yandex.net/api/v0/ip";
const IPV6_URL: &str = "https://ipv6-internet.yandex.net/api/v0/ip";
const TIMEOUT_SECS: u64 = 10;
const BROWSER_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64)";

/// Ответ — адрес строкой, обычно в кавычках JSON.
fn parse(body: &str, v6: bool) -> Option<String> {
    let text = body.trim().trim_matches('"').trim();
    let ip: IpAddr = text.parse().ok()?;
    (ip.is_ipv6() == v6).then(|| ip.to_string())
}

async fn ask(url: &str, v6: bool) -> Option<String> {
    let client = NetworkManager::new()
        .create_request(ProxyType::None, Some(TIMEOUT_SECS), Some(BROWSER_UA.into()), false)
        .await
        .ok()?;
    let response = tokio::time::timeout(Duration::from_secs(TIMEOUT_SECS + 2), client.get(url).send())
        .await
        .ok()?
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    parse(&response.text().await.ok()?, v6)
}

/// IPv4 и IPv6 клиента; пустая строка — не узнали (нет IPv6, сайт не ответил).
pub(super) async fn current() -> (String, String) {
    let (ip4, ip6) = tokio::join!(ask(IPV4_URL, false), ask(IPV6_URL, true));
    (ip4.unwrap_or_default(), ip6.unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn the_answer_is_an_address_of_the_asked_family() {
        assert_eq!(parse("\"203.0.113.7\"", false).as_deref(), Some("203.0.113.7"));
        assert_eq!(parse("203.0.113.7\n", false).as_deref(), Some("203.0.113.7"));
        assert_eq!(parse("\"2001:db8::7\"", true).as_deref(), Some("2001:db8::7"));
        assert_eq!(parse("\"203.0.113.7\"", true), None);
        assert_eq!(parse("<html>", false), None);
        assert_eq!(parse("", false), None);
    }
}
