use crate::utils::network::{NetworkManager, ProxyType};

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
    err.downcast_ref::<Refused>().is_some()
}

/// Запрос картинки провайдера: адрес — публичный https и до запроса, и после
/// переадресаций; адрес в локальной сети не получает ни одного запроса.
pub async fn request_image(
    url: &str,
    proxy: ProxyType,
    headers: reqwest::header::HeaderMap,
) -> anyhow::Result<reqwest::Response> {
    let url = reqwest::Url::parse(url).map_err(|_| Refused(NOT_PUBLIC))?;
    if !is_public_https(&url) {
        return Err(Refused(NOT_PUBLIC).into());
    }
    let client = NetworkManager::new()
        .create_request(proxy, Some(IMAGE_TIMEOUT_SECS), None)
        .await?;
    let response = client.get(url).headers(headers).send().await?;
    if !is_public_https(response.url()) {
        return Err(Refused(NOT_PUBLIC).into());
    }
    Ok(response)
}

/// Тело ответа с картинкой провайдера и расширение файла для неё. Картинка ли
/// это, решают сами байты, а не Content-Type: хранилища часто отдают
/// картинки как application/octet-stream.
pub async fn read_image(response: reqwest::Response, cap: usize) -> anyhow::Result<(Vec<u8>, &'static str)> {
    if response.content_length().is_some_and(|len| len > cap as u64) {
        return Err(Refused(TOO_LARGE).into());
    }
    let bytes = NetworkManager::read_capped_bytes(response, cap).await?;
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

    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        return match ip {
            std::net::IpAddr::V4(ip) => is_public_v4(ip),
            std::net::IpAddr::V6(ip) => {
                if let Some(mapped) = ip.to_ipv4_mapped() {
                    is_public_v4(mapped)
                } else {
                    !(ip.is_loopback() || ip.is_unspecified() || ip.is_unique_local() || ip.is_unicast_link_local())
                }
            }
        };
    }

    let name = host.trim_end_matches('.').to_ascii_lowercase();
    !(name == "localhost"
        || name.ends_with(".localhost")
        || name.ends_with(".local")
        || name.ends_with(".internal")
        || name.ends_with(".home.arpa"))
}

pub const fn is_public_v4(ip: std::net::Ipv4Addr) -> bool {
    !(ip.is_loopback() || ip.is_private() || ip.is_link_local() || ip.is_unspecified() || ip.is_broadcast())
}

#[allow(clippy::expect_used)]
#[cfg(test)]
pub(crate) mod tests {
    use super::{NOT_PUBLIC, image_extension, is_public_https, is_refused, request_image};
    use crate::utils::network::ProxyType;

    #[test]
    fn every_provider_image_goes_through_the_one_request() {
        for (file, source) in [
            ("logo_cache.rs", include_str!("../module/logo_cache.rs")),
            ("icon.rs", include_str!("../feat/icon.rs")),
        ] {
            let code = crate::utils::source_scan::production_code(source);
            assert!(code.contains("request_image("), "{file}");
            assert!(code.contains("read_image("), "{file}: картинку признают по байтам");
            assert!(!code.contains("CONTENT_TYPE"), "{file}: а не по Content-Type");
            assert!(!code.contains("create_request("), "{file}");
        }
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
    fn a_redirect_into_the_local_network_is_refused() {
        let allowed = [
            "https://cdn.example.com/logo.png",
            "https://1.2.3.4/logo.png",
            "https://[2606:4700::6810:84e5]/logo.png",
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
}
