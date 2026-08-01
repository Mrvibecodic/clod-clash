//! clod: local cache for the provider logo (`profile-logo`).
//!
//! Rendering the panel's URL directly means the user's IP reaches a third-party
//! host on every mount, the logo blinks on a cold start and disappears offline.
//! The image is therefore downloaded once per subscription update — through the
//! own core first, falling back to the system route — and kept next to the
//! profiles.
//!
//! Guards, mirroring what dropweb and Prizrak-Box learned the hard way: only a
//! known image content type is stored, never more than 2 MiB of it, only from a
//! public `https` host (a redirect into the local network is refused rather than
//! cached), and only under a profile id that cannot leave the cache directory.

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
///
/// Must stay in sync with `extension_for` — an extension it can return but this
/// list does not mention would be written once and then never read back, never
/// cleaned up and re-downloaded forever. `extensions_round_trip` guards that.
const KNOWN_EXTENSIONS: &[&str] = &["png", "svg", "jpg", "webp", "avif", "gif", "ico", "bmp"];

fn cache_dir() -> Result<PathBuf> {
    Ok(dirs::app_home_dir()?.join("logos"))
}

/// clod: адрес, с которого мы согласны забрать картинку.
///
/// `profile-logo` проверяется на `https` при разборе заголовков, но это проверка
/// только первого адреса: клиент ходит общим клиентом, который следует за
/// редиректами, и панель может увести его на `http://127.0.0.1:9090/…` или в
/// локальную сеть. Ответ оттуда попал бы в кэш и уехал в webview как `data:`-URL,
/// то есть редирект стал бы каналом чтения внутренней сети. Поэтому конечный
/// адрес ответа проверяется отдельно, и всё непубличное отбрасывается.
fn is_public_https(url: &reqwest::Url) -> bool {
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
                // `::ffff:127.0.0.1` для `Ipv6Addr::is_loopback` не loopback —
                // v4-mapped адрес обязан проходить проверки как его V4-форма.
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

fn is_public_v4(ip: std::net::Ipv4Addr) -> bool {
    !(ip.is_loopback() || ip.is_private() || ip.is_link_local() || ip.is_unspecified() || ip.is_broadcast())
}

/// clod: `uid` попадает в имя файла, поэтому в нём не должно быть ничего,
/// кроме алфавита nanoid. Иначе `../../` в аргументе команды из webview
/// превращает чтение кэша в чтение произвольного файла, а очистку — в удаление.
fn is_safe_uid(uid: &str) -> bool {
    !uid.is_empty()
        && uid.len() <= 64
        && uid.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
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
    if !is_safe_uid(uid) {
        return;
    }
    let Ok(dir) = cache_dir() else { return };
    for extension in KNOWN_EXTENSIONS {
        let _ = fs::remove_file(dir.join(format!("{uid}.{extension}"))).await;
    }
    sweep_parts(&dir, uid).await;
}

/// Убрать осиротевшие временные файлы профиля: оборванная запись прошлого
/// запуска иначе копилась бы на диске вечно — `read` такие файлы не видит.
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

/// Download the logo of `uid` and replace whatever was cached before.
///
/// Errors are returned for the caller to log, never to surface: a provider with
/// a broken logo URL must not break the subscription update.
async fn download(uid: &str, url: &str) -> Result<()> {
    if !is_safe_uid(uid) {
        bail!("refusing to cache a logo under an unexpected profile id");
    }
    if let Ok(dir) = cache_dir() {
        sweep_parts(&dir, uid).await;
    }
    let mut last_error = None;

    // clod: сначала через собственное ядро, и только потом напрямую. Смысл
    // кэша в том, чтобы чужой хост не видел адрес пользователя, — начинать
    // с прямого запроса значило бы отдавать ему настоящий IP ради картинки.
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
            if !is_public_https(response.url()) {
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
            // Пишем через временный файл: оборванная на середине запись иначе
            // оставила бы обрезанную картинку под тем же именем, и `read`
            // спокойно отдал бы её в интерфейс.
            let target = dir.join(format!("{uid}.{extension}"));
            // Имя временного файла уникально на процесс и на попытку: общий
            // `.part` при двух параллельных загрузках одного профиля дал бы
            // гонку записи и rename рваного файла.
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

/// Нумерует временные файлы записи внутри процесса.
static PART_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

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

/// Сколько профилей помним. Ключ приходит аргументом команды, так что расти
/// без предела карта не должна — при переполнении выкидываем протухшие записи.
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

#[allow(clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::{KNOWN_EXTENSIONS, extension_for, is_public_https, is_safe_uid, mime_for};

    /// Расширение, которое умеет вернуть `extension_for`, обязано быть в
    /// `KNOWN_EXTENSIONS`: иначе файл пишется, но не читается, не чистится и
    /// качается заново на каждом холодном чтении.
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
}
