//! clod: local cache for the provider logo (`profile-logo`).
//!
//! Rendering the panel's URL directly means the user's IP reaches a third-party
//! host on every mount, the logo blinks on a cold start and disappears offline.
//! The image is therefore downloaded once per subscription update — through the
//! own core when a direct request fails, like every other panel request — and
//! kept next to the profiles.
//!
//! Guards, mirroring what dropweb and Prizrak-Box learned the hard way: only
//! `image/*` responses are stored, and never more than 2 MiB of them.

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

/// Hard cap for a logo, matching what panels realistically send.
const MAX_LOGO_BYTES: usize = 2 * 1024 * 1024;

/// A logo is decoration: it must never hold up a subscription update.
const TIMEOUT_SECS: u64 = 15;

/// Extensions we may have written, in the order they are probed.
const KNOWN_EXTENSIONS: &[&str] = &["png", "svg", "jpg", "webp", "gif", "ico", "bmp"];

fn cache_dir() -> Result<PathBuf> {
    Ok(dirs::app_home_dir()?.join("logos"))
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

/// The logo URL the panel last sent for this profile.
async fn logo_url(uid: &str) -> Option<String> {
    let profiles = Config::profiles().await;
    let arc = profiles.latest_arc();
    let url = arc.get_item(uid).ok().and_then(|item| item.logo.clone());
    drop(arc);
    url
}

/// Remove every cached file of a profile (the panel stopped sending a logo, or
/// the profile itself is gone).
pub async fn clear(uid: &str) {
    let Ok(dir) = cache_dir() else { return };
    for extension in KNOWN_EXTENSIONS {
        let _ = fs::remove_file(dir.join(format!("{uid}.{extension}"))).await;
    }
}

/// Download the logo of `uid` and replace whatever was cached before.
///
/// Errors are returned for the caller to log, never to surface: a provider with
/// a broken logo URL must not break the subscription update.
async fn download(uid: &str, url: &str) -> Result<()> {
    let mut last_error = None;

    for proxy in [ProxyType::None, ProxyType::Localhost] {
        let attempt = async {
            let client = NetworkManager::new()
                .create_request(proxy, Some(TIMEOUT_SECS), Some(hwid::user_agent()), false)
                .await?;
            let response = client.get(url).send().await?;
            if !response.status().is_success() {
                bail!("logo request returned {}", response.status());
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

            // Размер проверяем до чтения и по ходу чтения: чужой хост не
            // должен уметь раздуть память приложения ответом на гигабайт.
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
            // Write the new file first, then drop the stale ones: a failed
            // write leaves the previous logo in place instead of nothing.
            fs::write(dir.join(format!("{uid}.{extension}")), &bytes).await?;
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

/// Bring the cache in line with what the panel says right now.
///
/// Called after every successful subscription update; a panel that stopped
/// sending `profile-logo` gets its cached image removed on the same pass.
pub async fn sync(uid: &str) {
    match logo_url(uid).await {
        Some(url) if !url.trim().is_empty() => {
            if let Err(err) = download(uid, url.trim()).await {
                // The URL itself is not secret, but it belongs to the panel —
                // keep the log about what happened, not about where.
                logging!(warn, Type::Config, "profile logo for {uid} not cached: {err:#}");
            }
        }
        _ => clear(uid).await,
    }
}

/// Cached logo as a `data:` URL, or `None` when there is nothing cached.
pub async fn read(uid: &str) -> Option<String> {
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

/// Как долго не повторять неудачную попытку скачивания.
const RETRY_AFTER: std::time::Duration = std::time::Duration::from_secs(10 * 60);

/// Профили, для которых холодное чтение уже ходило в сеть и вернулось ни с чем.
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

fn remember_cold_miss(uid: &str) {
    if let Ok(mut slot) = cold_misses().lock() {
        slot.insert(uid.into(), std::time::Instant::now());
    }
}

/// Read the cached logo, downloading it first when the cache is cold.
///
/// This is what makes an imported subscription show its logo without waiting
/// for the first scheduled update. A failed attempt is remembered for a while:
/// the UI asks on every mount, and a dead logo host must not cost it two
/// fifteen-second timeouts each time.
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
