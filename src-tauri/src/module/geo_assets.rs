use crate::{
    config::Config,
    core::{CoreManager, manager::RunningMode},
    utils::{
        dirs, help, hwid,
        network::{NetworkManager, ProxyType},
    },
};
use anyhow::{Result, bail};
use clash_verge_logging::{Type, logging};
use serde_yaml_ng::Value;

const TIMEOUT_SECS: u64 = 60;
const MAX_BYTES: usize = 256 * 1024 * 1024;
const MMDB_MARKER: &[u8] = b"\xab\xcd\xefMaxMind.com";

const HOME_FILES: &[(&str, &str)] = &[
    ("Country.mmdb", "mmdb"),
    ("geoip.metadb", "mmdb"),
    ("geoip.dat", "geoip"),
    ("geosite.dat", "geosite"),
    ("GeoSite.dat", "geosite"),
    ("ASN.mmdb", "asn"),
];

const DEFAULT_URLS: &[(&str, &str)] = &[
    (
        "mmdb",
        "https://github.com/MetaCubeX/meta-rules-dat/releases/download/latest/geoip.metadb",
    ),
    (
        "geoip",
        "https://github.com/MetaCubeX/meta-rules-dat/releases/download/latest/geoip.dat",
    ),
    (
        "geosite",
        "https://github.com/MetaCubeX/meta-rules-dat/releases/download/latest/geosite.dat",
    ),
    (
        "asn",
        "https://github.com/MetaCubeX/meta-rules-dat/releases/download/latest/GeoLite2-ASN.mmdb",
    ),
];

fn url_for(kind: &str, geox: Option<&serde_yaml_ng::Mapping>) -> Option<String> {
    if let Some(url) = geox
        .and_then(|geox| geox.get(kind))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|url| url.starts_with("https://"))
    {
        return Some(url.to_owned());
    }
    DEFAULT_URLS
        .iter()
        .find(|(name, _)| *name == kind)
        .map(|(_, url)| (*url).to_owned())
}

fn looks_like_a_database(file: &str, data: &[u8]) -> bool {
    if data.len() < 1024 || data.starts_with(b"<") || data.starts_with(b"{") {
        return false;
    }
    if file.to_ascii_lowercase().ends_with(".mmdb") || file.to_ascii_lowercase().ends_with(".metadb") {
        let tail = &data[data.len().saturating_sub(128 * 1024)..];
        return tail.windows(MMDB_MARKER.len()).any(|window| window == MMDB_MARKER);
    }
    true
}

async fn download(url: &str) -> Result<Vec<u8>> {
    let mut last_error = None;
    for proxy in [ProxyType::Localhost, ProxyType::System, ProxyType::None] {
        let attempt = async {
            let client = NetworkManager::new()
                .create_request(proxy, Some(TIMEOUT_SECS), Some(hwid::user_agent()), false)
                .await?;
            let response = client.get(url).send().await?;
            if !response.status().is_success() {
                bail!("geo download returned {}", response.status());
            }
            if response
                .content_length()
                .is_some_and(|length| length as usize > MAX_BYTES)
            {
                bail!("geo file is larger than {MAX_BYTES} bytes");
            }
            let data = response.bytes().await?;
            if data.len() > MAX_BYTES {
                bail!("geo file is larger than {MAX_BYTES} bytes");
            }
            Ok(data.to_vec())
        };
        match attempt.await {
            Ok(data) => return Ok(data),
            Err(error) => {
                logging!(debug, Type::Core, "geo download via {proxy:?} failed: {error:#}");
                last_error = Some(error);
            }
        }
    }
    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("geo download failed")))
}

pub async fn refresh_home_copies() -> Result<usize> {
    if !matches!(*CoreManager::global().get_running_mode(), RunningMode::Service) {
        return Ok(0);
    }
    let home = dirs::app_home_dir()?;
    let geox = Config::runtime()
        .await
        .latest_arc()
        .config
        .as_ref()
        .and_then(|config| config.get("geox-url"))
        .and_then(Value::as_mapping)
        .cloned();

    let mut refreshed = 0;
    let mut failures = Vec::new();
    for (file, kind) in HOME_FILES {
        let path = home.join(file);
        if !path.is_file() {
            continue;
        }
        let Some(url) = url_for(kind, geox.as_ref()) else {
            continue;
        };
        let outcome = async {
            let data = download(&url).await?;
            if !looks_like_a_database(file, &data) {
                bail!("the downloaded {file} does not look like a geo database");
            }
            help::write_atomic(&path, &data).await
        }
        .await;
        match outcome {
            Ok(()) => {
                refreshed += 1;
                logging!(info, Type::Core, "geo database {file} refreshed in the app directory");
            }
            Err(error) => {
                logging!(warn, Type::Core, "geo database {file} was not refreshed: {error:#}");
                failures.push(format!("{file}: {error:#}"));
            }
        }
    }
    if refreshed == 0 && !failures.is_empty() {
        bail!("{}", failures.join("; "));
    }
    Ok(refreshed)
}

#[cfg(test)]
mod tests {
    use super::{looks_like_a_database, url_for};
    use serde_yaml_ng::Value;

    #[test]
    fn the_subscription_url_wins_over_the_default() {
        let mut geox = serde_yaml_ng::Mapping::new();
        geox.insert(
            Value::from("geosite"),
            Value::from("https://mirror.example/geosite.dat"),
        );
        assert_eq!(
            url_for("geosite", Some(&geox)).as_deref(),
            Some("https://mirror.example/geosite.dat")
        );
        assert_eq!(
            url_for("geoip", Some(&geox)).as_deref(),
            Some("https://github.com/MetaCubeX/meta-rules-dat/releases/download/latest/geoip.dat")
        );
    }

    #[test]
    fn a_plain_http_url_from_the_subscription_is_ignored() {
        let mut geox = serde_yaml_ng::Mapping::new();
        geox.insert(Value::from("mmdb"), Value::from("http://mirror.example/geoip.metadb"));
        assert_eq!(
            url_for("mmdb", Some(&geox)).as_deref(),
            Some("https://github.com/MetaCubeX/meta-rules-dat/releases/download/latest/geoip.metadb")
        );
        assert_eq!(url_for("unknown", None), None);
    }

    #[test]
    fn an_error_page_or_a_stub_is_not_a_database() {
        assert!(!looks_like_a_database("geosite.dat", b"<html>rate limited</html>"));
        assert!(!looks_like_a_database("geosite.dat", &[0u8; 100]));
        assert!(looks_like_a_database("geosite.dat", &[7u8; 4096]));
    }

    #[test]
    fn an_mmdb_needs_the_maxmind_marker() {
        let mut fake = vec![1u8; 4096];
        assert!(!looks_like_a_database("Country.mmdb", &fake));
        fake.extend_from_slice(b"\xab\xcd\xefMaxMind.com");
        fake.extend_from_slice(&[0u8; 64]);
        assert!(looks_like_a_database("Country.mmdb", &fake));
    }
}
