use crate::utils::network::{BodyTooLarge, MAX_REDIRECTS, NetworkManager, ProxyType};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// Срок запроса и потолок картинки провайдера (логотип, значок группы).
const IMAGE_TIMEOUT_SECS: u64 = 15;
pub const MAX_IMAGE_BYTES: usize = 2 * 1024 * 1024;

const NOT_PUBLIC: &str = "image url must be a public https address";
const TOO_LARGE: &str = "image is larger than allowed";
const NOT_AN_IMAGE: &str = "image url did not return an image";

/// Отказ по самому адресу или содержимому картинки: спрашивать снова тот же
/// адрес бесполезно, окну это «картинки не будет», а не «ещё качается».
#[derive(Debug)]
pub struct Refused(&'static str);

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for Refused {}

pub fn is_refused(err: &anyhow::Error) -> bool {
    err.chain().any(|e| e.downcast_ref::<Refused>().is_some())
}

/// Запрос картинки провайдера: адрес — публичный https до запроса и на каждой
/// переадресации, а при соединении мимо прокси — и там, куда указывает имя;
/// адрес в локальной сети не получает ни одного запроса.
pub async fn request_image(
    url: &str,
    proxy: ProxyType,
    headers: reqwest::header::HeaderMap,
) -> anyhow::Result<reqwest::Response> {
    let url = reqwest::Url::parse(url).map_err(|_| Refused(NOT_PUBLIC))?;
    if !is_public_https(&url) {
        return Err(Refused(NOT_PUBLIC).into());
    }
    Ok(public_client(proxy).await?.get(url).headers(headers).send().await?)
}

async fn public_client(proxy: ProxyType) -> anyhow::Result<reqwest::Client> {
    let redirect = reqwest::redirect::Policy::custom(|attempt| {
        if !is_public_https(attempt.url()) {
            attempt.error(Refused(NOT_PUBLIC))
        } else if attempt.previous().len() > MAX_REDIRECTS {
            attempt.error("too many redirects")
        } else {
            attempt.follow()
        }
    });
    NetworkManager::new()
        .create_restricted_request(proxy, Some(IMAGE_TIMEOUT_SECS), redirect, PublicResolver)
        .await
}

/// Разрешение имени без прокси: адреса вне публичной сети отбрасываются.
struct PublicResolver;

impl reqwest::dns::Resolve for PublicResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let host = name.as_str().to_owned();
        Box::pin(async move {
            let public: Vec<_> = tokio::net::lookup_host((host.as_str(), 0))
                .await?
                .filter(|addr| is_public_ip(addr.ip()))
                .collect();
            if public.is_empty() {
                return Err(Refused(NOT_PUBLIC).into());
            }
            Ok(Box::new(public.into_iter()) as reqwest::dns::Addrs)
        })
    }
}

/// Тело ответа с картинкой провайдера и расширение файла для неё. Картинка ли
/// это, решают сами байты, а не Content-Type: хранилища часто отдают
/// картинки как application/octet-stream.
pub async fn read_image(response: reqwest::Response, cap: usize) -> anyhow::Result<(Vec<u8>, &'static str)> {
    let bytes = NetworkManager::read_capped_bytes(response, cap).await.map_err(|err| {
        if err.is::<BodyTooLarge>() {
            Refused(TOO_LARGE).into()
        } else {
            err
        }
    })?;
    let extension = image_extension(&bytes).ok_or(Refused(NOT_AN_IMAGE))?;
    Ok((bytes, extension))
}

pub fn image_extension(bytes: &[u8]) -> Option<&'static str> {
    const SIGNATURES: [(&[u8], &str); 6] = [
        (b"\x89PNG\r\n\x1a\n", "png"),
        (b"\xFF\xD8\xFF", "jpg"),
        (b"GIF87a", "gif"),
        (b"GIF89a", "gif"),
        (b"\x00\x00\x01\x00", "ico"),
        (b"BM", "bmp"),
    ];
    if let Some((_, extension)) = SIGNATURES.iter().find(|(magic, _)| bytes.starts_with(magic)) {
        return Some(extension);
    }
    if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP".as_slice()) {
        return Some("webp");
    }
    if matches!(bytes.get(4..12), Some(b"ftypavif" | b"ftypavis")) {
        return Some("avif");
    }
    looks_like_svg(bytes).then_some("svg")
}

fn normalized_text_prefix(content: &[u8]) -> std::string::String {
    let content = content.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(content);
    let start = content
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .unwrap_or(content.len());
    let end = content.len().min(start.saturating_add(2048));
    let prefix = content.get(start..end).unwrap_or_default();
    std::string::String::from_utf8_lossy(prefix).to_ascii_lowercase()
}

fn looks_like_svg(content: &[u8]) -> bool {
    let prefix = normalized_text_prefix(content);
    prefix.starts_with("<svg")
        || ((prefix.starts_with("<?xml") || prefix.starts_with("<!doctype svg")) && prefix.contains("<svg"))
}

pub fn is_public_https(url: &reqwest::Url) -> bool {
    if url.scheme() != "https" {
        return false;
    }
    let Some(host) = url.host_str() else {
        return false;
    };
    let host = host.trim_start_matches('[').trim_end_matches(']');

    if let Ok(ip) = host.parse::<IpAddr>() {
        return is_public_ip(ip);
    }

    let name = host.trim_end_matches('.').to_ascii_lowercase();
    !(name == "localhost"
        || name.ends_with(".localhost")
        || name.ends_with(".local")
        || name.ends_with(".internal")
        || name.ends_with(".home.arpa"))
}

fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => is_public_v4(ip),
        IpAddr::V6(ip) => is_public_v6(ip),
    }
}

const fn is_public_v4(ip: Ipv4Addr) -> bool {
    let [first, second, ..] = ip.octets();
    let shared = first == 100 && second & 0xC0 == 64;
    !(ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_multicast()
        || first == 0
        || first >= 240
        || shared)
}

fn is_public_v6(ip: Ipv6Addr) -> bool {
    let [a, b, c, d, e, f, g, h] = ip.segments();
    let embedded = |high: u16, low: u16| {
        let [h1, h2] = high.to_be_bytes();
        let [l1, l2] = low.to_be_bytes();
        Ipv4Addr::new(h1, h2, l1, l2)
    };
    if let Some(mapped) = ip.to_ipv4_mapped() {
        return is_public_v4(mapped);
    }
    // NAT64 и 6to4 ведут на адрес IPv4, вписанный в сам адрес.
    if [a, b, c, d, e, f] == [0x64, 0xff9b, 0, 0, 0, 0] {
        return is_public_v4(embedded(g, h));
    }
    if a == 0x2002 {
        return is_public_v4(embedded(b, c));
    }
    !(ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_unique_local()
        || ip.is_unicast_link_local()
        || ip.is_multicast())
}

#[allow(clippy::expect_used)]
#[cfg(test)]
pub(crate) mod tests {
    use super::{NOT_PUBLIC, image_extension, is_public_https, is_refused, request_image};
    use crate::utils::network::ProxyType;

    #[test]
    fn every_provider_image_goes_through_the_one_request() {
        let code = crate::utils::source_scan::production_code(include_str!("../module/logo_cache.rs"));
        assert!(code.contains("request_image("));
        assert!(code.contains("read_image("), "картинку признают по байтам");
        assert!(!code.contains("CONTENT_TYPE"), "а не по Content-Type");
        assert!(!code.contains("create_request("));
    }

    #[tokio::test]
    async fn an_image_in_the_local_network_is_refused_before_any_request() {
        for raw in [
            "https://127.0.0.1/logo.png",
            "https://192.168.1.1/logo.png",
            "https://router.local/logo.png",
            "http://cdn.example.com/logo.png",
        ] {
            let refused = request_image(raw, ProxyType::None, reqwest::header::HeaderMap::new()).await;
            let err = refused.err();
            assert_eq!(
                err.as_ref().map(ToString::to_string).as_deref(),
                Some(NOT_PUBLIC),
                "{raw}"
            );
            assert!(
                err.as_ref().is_some_and(is_refused),
                "{raw}: картинки не будет, а не «ещё качается»"
            );
        }
    }

    #[test]
    fn only_a_public_https_address_is_allowed() {
        let allowed = [
            "https://cdn.example.com/logo.png",
            "https://1.2.3.4/logo.png",
            "https://198.18.0.7/logo.png",
            "https://[2606:4700::6810:84e5]/logo.png",
            "https://[64:ff9b::808:808]/logo.png",
            "https://[2002:808:808::1]/logo.png",
        ];
        for raw in allowed {
            let url = reqwest::Url::parse(raw).expect("test url");
            assert!(is_public_https(&url), "{raw}");
        }

        let refused = [
            "http://cdn.example.com/logo.png",
            "https://127.0.0.1:9090/configs",
            "https://localhost/logo.png",
            "https://192.168.1.1/logo.png",
            "https://10.0.0.5/logo.png",
            "https://169.254.169.254/latest/meta-data",
            "https://[::1]/logo.png",
            "https://[fd00::1]/logo.png",
            "https://[fe80::1]/logo.png",
            "https://[::ffff:127.0.0.1]/logo.png",
            "https://[::ffff:192.168.1.1]/logo.png",
            "https://router.local/logo.png",
            "https://100.64.0.1/logo.png",
            "https://100.127.255.254/logo.png",
            "https://0.1.2.3/logo.png",
            "https://224.0.0.251/logo.png",
            "https://240.0.0.1/logo.png",
            "https://255.255.255.255/logo.png",
            "https://[ff02::1]/logo.png",
            "https://[64:ff9b::c0a8:101]/logo.png",
            "https://[2002:c0a8:101::1]/logo.png",
            "https://[2002:7f00:1::1]/logo.png",
        ];
        for raw in refused {
            let url = reqwest::Url::parse(raw).expect("test url");
            assert!(!is_public_https(&url), "{raw}");
        }
    }

    /// Образцы картинок и расширение, под которым их кладут на диск.
    pub const IMAGE_SAMPLES: [(&[u8], &str); 10] = [
        (b"\x89PNG\r\n\x1a\n....", "png"),
        (b"\xFF\xD8\xFF\xE0....", "jpg"),
        (b"GIF89a....", "gif"),
        (b"RIFF\x10\x00\x00\x00WEBPVP8 ", "webp"),
        (b"\x00\x00\x00\x1cftypavif....", "avif"),
        (b"\x00\x00\x01\x00\x01\x00", "ico"),
        (b"BM\x36\x00\x00\x00", "bmp"),
        (br#"<svg xmlns="http://www.w3.org/2000/svg"></svg>"#, "svg"),
        (
            br#"<?xml version="1.0" encoding="UTF-8"?><svg xmlns="http://www.w3.org/2000/svg"></svg>"#,
            "svg",
        ),
        (
            b"\xEF\xBB\xBF \n\t<!DOCTYPE svg PUBLIC \"-//W3C//DTD SVG 1.1//EN\"><svg></svg>",
            "svg",
        ),
    ];

    #[test]
    fn a_picture_is_recognised_by_its_bytes() {
        for (bytes, extension) in IMAGE_SAMPLES {
            assert_eq!(image_extension(bytes), Some(extension), "{extension}");
        }
        for bytes in [
            b"<!DOCTYPE html><html><svg></svg></html>".as_slice(),
            b"<html><body>oops</body></html>",
            br#"{"status":"ok"}"#,
            b"",
            b"RIFF\x10\x00\x00\x00WAVEfmt ",
        ] {
            assert_eq!(image_extension(bytes), None, "{}", String::from_utf8_lossy(bytes));
        }
    }

    /// Настоящие обмены по сети: под Windows такой тестовый бинарник на раннере
    /// CI не запускается, как и обмены в `network`.
    #[cfg(not(windows))]
    mod transport {
        use super::super::{MAX_IMAGE_BYTES, is_refused, public_client, read_image};
        use crate::utils::network::ProxyType;

        /// Слушатель, который считает, пришло ли к нему соединение.
        async fn untouched() -> (tokio::net::TcpListener, u16) {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("порт для теста");
            let port = listener.local_addr().expect("адрес слушателя").port();
            (listener, port)
        }

        async fn never_reached(listener: &tokio::net::TcpListener) -> bool {
            tokio::time::timeout(std::time::Duration::from_millis(200), listener.accept())
                .await
                .is_err()
        }

        #[tokio::test]
        async fn a_redirect_into_the_local_network_gets_no_request() {
            use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

            for target in ["https", "http"] {
                let (local, local_port) = untouched().await;
                let (cdn, cdn_port) = untouched().await;
                let head = format!(
                    "HTTP/1.1 302 Found\r\nLocation: {target}://127.0.0.1:{local_port}/logo.png\r\nContent-Length: 0\r\n\r\n"
                );
                tokio::spawn(async move {
                    let Ok((mut stream, _)) = cdn.accept().await else {
                        return;
                    };
                    let mut buffer = [0_u8; 1024];
                    let _ = stream.read(&mut buffer).await;
                    let _ = stream.write_all(head.as_bytes()).await;
                });

                let client = public_client(ProxyType::None).await.expect("клиент");
                let err = client
                    .get(format!("http://127.0.0.1:{cdn_port}/logo.png"))
                    .send()
                    .await
                    .map_err(anyhow::Error::new)
                    .expect_err("переадресация в локальную сеть");
                assert!(is_refused(&err), "{target}: {err:?}");
                assert!(never_reached(&local).await, "{target}: запрос дошёл до локальной сети");
            }
        }

        #[tokio::test]
        async fn a_name_that_points_into_the_local_network_gets_no_request() {
            let (local, port) = untouched().await;
            let client = public_client(ProxyType::None).await.expect("клиент");
            let err = client
                .get(format!("http://localhost:{port}/logo.png"))
                .send()
                .await
                .map_err(anyhow::Error::new)
                .expect_err("имя указывает в локальную сеть");
            assert!(is_refused(&err), "{err:?}");
            assert!(never_reached(&local).await, "запрос дошёл до локальной сети");
        }

        #[tokio::test]
        async fn a_picture_over_the_cap_is_refused_with_or_without_its_size() {
            use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

            for head in [
                "HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n".to_owned(),
                format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", MAX_IMAGE_BYTES + 1),
            ] {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                    .await
                    .expect("порт для теста");
                let port = listener.local_addr().expect("адрес слушателя").port();
                tokio::spawn(async move {
                    let Ok((mut stream, _)) = listener.accept().await else {
                        return;
                    };
                    let mut buffer = [0_u8; 1024];
                    let _ = stream.read(&mut buffer).await;
                    let _ = stream.write_all(head.as_bytes()).await;
                    let _ = stream.write_all(&vec![0_u8; MAX_IMAGE_BYTES + 1]).await;
                    let _ = stream.shutdown().await;
                });
                let response = crate::utils::network::NetworkManager::new()
                    .create_request(ProxyType::None, Some(10), Some("test".into()))
                    .await
                    .expect("клиент")
                    .get(format!("http://127.0.0.1:{port}/logo.png"))
                    .send()
                    .await
                    .expect("ответ пришёл");
                let err = read_image(response, MAX_IMAGE_BYTES).await.expect_err("сверх потолка");
                assert!(is_refused(&err), "картинки не будет, а не «ещё качается»: {err:?}");
            }
        }
    }
}
