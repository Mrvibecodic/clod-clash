use crate::{
    config::Config,
    utils::{
        dirs, hwid,
        network::{NetworkManager, ProxyType},
    },
};
use anyhow::{Result, bail};
use base64::{Engine as _, engine::general_purpose};
use clash_verge_logging::{Type, logging};
use smartstring::alias::String;
use std::{collections::HashMap, path::PathBuf};
use tokio::fs;

const MAX_LOGO_BYTES: usize = 2 * 1024 * 1024;

const TIMEOUT_SECS: u64 = 15;

const KNOWN_EXTENSIONS: &[&str] = &["png", "svg", "jpg", "webp", "avif", "gif", "ico", "bmp"];

fn cache_dir() -> Result<PathBuf> {
    Ok(dirs::app_home_dir()?.join("logos"))
}

fn is_safe_uid(uid: &str) -> bool {
    !uid.is_empty() && uid.len() <= 64 && uid.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
}

fn extension_for(content_type: &str) -> Option<&'static str> {
    let kind = content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    match kind.as_str() {
        "image/png" => Some("png"),
        "image/svg+xml" => Some("svg"),
        "image/jpeg" | "image/jpg" => Some("jpg"),
        "image/webp" => Some("webp"),
        "image/avif" => Some("avif"),
        "image/gif" => Some("gif"),
        "image/x-icon" | "image/vnd.microsoft.icon" => Some("ico"),
        "image/bmp" => Some("bmp"),
        _ => None,
    }
}

const fn mime_for(extension: &str) -> &'static str {
    match extension.as_bytes() {
        b"svg" => "image/svg+xml",
        b"jpg" => "image/jpeg",
        b"webp" => "image/webp",
        b"avif" => "image/avif",
        b"gif" => "image/gif",
        b"ico" => "image/x-icon",
        b"bmp" => "image/bmp",
        _ => "image/png",
    }
}

async fn logo_url(uid: &str) -> Option<String> {
    let profiles = Config::profiles().await;
    let arc = profiles.latest_arc();
    let url = arc.get_item(uid).ok().and_then(|item| item.logo.clone());
    drop(arc);
    url
}

pub async fn clear(uid: &str) {
    if !is_safe_uid(uid) {
        return;
    }
    let Ok(dir) = cache_dir() else { return };
    for extension in KNOWN_EXTENSIONS {
        let _ = fs::remove_file(dir.join(format!("{uid}.{extension}"))).await;
    }
    sweep_parts(&dir, uid).await;
}

async fn sweep_parts(dir: &std::path::Path, uid: &str) {
    let Ok(mut entries) = fs::read_dir(dir).await else {
        return;
    };
    let prefix = format!("{uid}.");
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if name.starts_with(&prefix) && name.ends_with(".part") {
            let _ = fs::remove_file(entry.path()).await;
        }
    }
}

async fn download(uid: &str, url: &str) -> Result<()> {
    if !is_safe_uid(uid) {
        bail!("refusing to cache a logo under an unexpected profile id");
    }
    if let Ok(dir) = cache_dir() {
        sweep_parts(&dir, uid).await;
    }
    let mut last_error = None;

    for proxy in [ProxyType::Localhost, ProxyType::System] {
        let attempt = async {
            let client = NetworkManager::new()
                .create_request(proxy, Some(TIMEOUT_SECS), Some(hwid::user_agent()), false)
                .await?;
            let response = client
                .get(url)
                .header(reqwest::header::ACCEPT, "image/*")
                .send()
                .await?;
            if !response.status().is_success() {
                bail!("logo request returned {}", response.status());
            }
            if !crate::utils::public_url::is_public_https(response.url()) {
                bail!("logo redirected somewhere we will not read from");
            }

            let content_type = response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_owned();
            let Some(extension) = extension_for(&content_type) else {
                bail!("logo is not an image ({content_type})");
            };

            if response
                .content_length()
                .is_some_and(|length| length > MAX_LOGO_BYTES as u64)
            {
                bail!("logo is too large ({} bytes)", response.content_length().unwrap_or(0));
            }

            let mut bytes: Vec<u8> = Vec::new();
            let mut response = response;
            while let Some(chunk) = response.chunk().await? {
                bytes.extend_from_slice(&chunk);
                if bytes.len() > MAX_LOGO_BYTES {
                    bail!("logo is too large (over {MAX_LOGO_BYTES} bytes)");
                }
            }
            if bytes.is_empty() {
                bail!("logo response is empty");
            }

            let dir = cache_dir()?;
            fs::create_dir_all(&dir).await?;
            let target = dir.join(format!("{uid}.{extension}"));
            let attempt_id = PART_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let temporary = dir.join(format!("{uid}.{extension}.{}-{attempt_id}.part", std::process::id()));
            if let Err(err) = fs::write(&temporary, &bytes).await {
                let _ = fs::remove_file(&temporary).await;
                return Err(err.into());
            }
            if let Err(err) = fs::rename(&temporary, &target).await {
                let _ = fs::remove_file(&temporary).await;
                return Err(err.into());
            }
            for stale in KNOWN_EXTENSIONS.iter().filter(|item| **item != extension) {
                let _ = fs::remove_file(dir.join(format!("{uid}.{stale}"))).await;
            }
            Ok::<(), anyhow::Error>(())
        };

        match attempt.await {
            Ok(()) => return Ok(()),
            Err(err) => last_error = Some(err),
        }
    }

    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("logo download was not attempted")))
}

pub async fn sync(uid: &str) {
    match logo_url(uid).await {
        Some(url) if !url.trim().is_empty() => {
            if let Err(err) = download(uid, url.trim()).await {
                logging!(warn, Type::Config, "profile logo for {uid} not cached: {err:#}");
            }
        }
        _ => clear(uid).await,
    }
}

pub async fn read(uid: &str) -> Option<String> {
    if !is_safe_uid(uid) {
        return None;
    }
    let dir = cache_dir().ok()?;
    for extension in KNOWN_EXTENSIONS {
        let path = dir.join(format!("{uid}.{extension}"));
        let Ok(bytes) = fs::read(&path).await else {
            continue;
        };
        if bytes.is_empty() {
            continue;
        }
        let encoded = general_purpose::STANDARD.encode(&bytes);
        return Some(format!("data:{};base64,{encoded}", mime_for(extension)).into());
    }
    None
}

static PART_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

const RETRY_AFTER: std::time::Duration = std::time::Duration::from_secs(10 * 60);

static COLD_MISSES: std::sync::OnceLock<std::sync::Mutex<HashMap<String, std::time::Instant>>> =
    std::sync::OnceLock::new();

fn cold_misses() -> &'static std::sync::Mutex<HashMap<String, std::time::Instant>> {
    COLD_MISSES.get_or_init(|| std::sync::Mutex::new(HashMap::new()))
}

fn cold_miss_recently(uid: &str) -> bool {
    cold_misses()
        .lock()
        .ok()
        .and_then(|slot| slot.get(uid).copied())
        .is_some_and(|at| at.elapsed() < RETRY_AFTER)
}

const MAX_COLD_MISSES: usize = 64;

fn remember_cold_miss(uid: &str) {
    if let Ok(mut slot) = cold_misses().lock() {
        if slot.len() >= MAX_COLD_MISSES {
            slot.retain(|_, at| at.elapsed() < RETRY_AFTER);
        }
        if slot.len() < MAX_COLD_MISSES {
            slot.insert(uid.into(), std::time::Instant::now());
        }
    }
}

pub async fn read_or_fetch(uid: &str) -> Option<String> {
    if let Some(cached) = read(uid).await {
        return Some(cached);
    }
    if cold_miss_recently(uid) {
        return None;
    }
    sync(uid).await;
    let fresh = read(uid).await;
    if fresh.is_none() {
        remember_cold_miss(uid);
    }
    fresh
}

#[allow(clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::{KNOWN_EXTENSIONS, extension_for, is_safe_uid, mime_for};

    #[test]
    fn extensions_round_trip() {
        for content_type in [
            "image/png",
            "image/svg+xml",
            "image/jpeg",
            "image/jpg",
            "image/webp",
            "image/avif",
            "image/gif",
            "image/x-icon",
            "image/vnd.microsoft.icon",
            "image/bmp",
            "image/PNG; charset=binary",
        ] {
            let extension = extension_for(content_type).unwrap_or_else(|| panic!("{content_type} must be accepted"));
            assert!(
                KNOWN_EXTENSIONS.contains(&extension),
                "{content_type} -> {extension} is written but never read back"
            );
            assert!(!mime_for(extension).is_empty());
        }

        assert_eq!(extension_for("text/html"), None);
        assert_eq!(extension_for("application/octet-stream"), None);
    }

    #[test]
    fn profile_ids_never_leave_the_cache_directory() {
        for uid in ["RRuvIA", "a-b_c9"] {
            assert!(is_safe_uid(uid), "{uid}");
        }
        for uid in ["", "../../etc/passwd", "/etc/hosts", r"..\..\win.ini", "a/b", "a.b"] {
            assert!(!is_safe_uid(uid), "{uid}");
        }
    }
}
