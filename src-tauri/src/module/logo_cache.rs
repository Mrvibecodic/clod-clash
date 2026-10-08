use crate::{
    config::Config,
    core::handle,
    utils::{
        dirs, help,
        network::{NetworkManager, ProxyType},
        retry::try_strategies,
    },
};
use anyhow::{Result, bail};
use base64::{Engine as _, engine::general_purpose};
use clash_verge_logging::{Type, logging};
use reqwest::header::{ETAG, HeaderMap, HeaderName, IF_MODIFIED_SINCE, IF_NONE_MATCH, LAST_MODIFIED};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use smartstring::alias::String;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};
use tokio::fs;

const MAX_LOGO_BYTES: usize = 2 * 1024 * 1024;

const MAX_BACKGROUND_BYTES: usize = 4 * 1024 * 1024;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Picture {
    Logo,
    Background,
}

impl Picture {
    const fn suffix(self) -> &'static str {
        match self {
            Self::Logo => "",
            Self::Background => ".bg",
        }
    }

    const fn max_bytes(self) -> usize {
        match self {
            Self::Logo => MAX_LOGO_BYTES,
            Self::Background => MAX_BACKGROUND_BYTES,
        }
    }

    fn stem(self, uid: &str) -> std::string::String {
        format!("{uid}{}", self.suffix())
    }

    /// Имя картинки для окна.
    const fn name(self) -> &'static str {
        match self {
            Self::Logo => "logo",
            Self::Background => "background",
        }
    }

    async fn url(self, uid: &str) -> Option<String> {
        let profiles = Config::profiles().await;
        let arc = profiles.latest_arc();
        let url = arc.get_item(uid).ok().and_then(|item| match self {
            Self::Logo => item.logo.clone(),
            Self::Background => item.theme_background.clone(),
        });
        drop(arc);
        url
    }
}

/// Валидаторы последнего ответа с картинкой (ETag, Last-Modified) и отпечаток
/// файла, к которому они относятся. Лежат рядом с картинкой, `<stem>.meta`.
///
/// Условный запрос уходит, только если файл на диске цел — его отпечаток тот же,
/// что был при записи (как в ядре: `component/resource/vehicle.go`): иначе ответ
/// 304 оставил бы на экране чужую или битую картинку.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Validators {
    url: std::string::String,
    extension: std::string::String,
    sha256: std::string::String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    etag: Option<std::string::String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_modified: Option<std::string::String>,
}

fn fingerprint(bytes: &[u8]) -> std::string::String {
    format!("{:x}", Sha256::digest(bytes))
}

impl Validators {
    /// Из ответа с картинкой; `None` — сервер не дал ни ETag, ни Last-Modified.
    fn of(url: &str, headers: &HeaderMap, extension: &str, bytes: &[u8]) -> Option<Self> {
        let header = |name| {
            headers
                .get(name)
                .and_then(|value| value.to_str().ok())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
        };
        let (etag, last_modified) = (header(ETAG), header(LAST_MODIFIED));
        (etag.is_some() || last_modified.is_some()).then(|| Self {
            url: url.to_owned(),
            extension: extension.to_owned(),
            sha256: fingerprint(bytes),
            etag,
            last_modified,
        })
    }

    /// Заголовки условного запроса к `url`; пусто — спрашивать без условий.
    /// `local` — файл картинки, к которому относятся валидаторы.
    fn conditional_for(&self, url: &str, local: &[u8]) -> Vec<(HeaderName, &str)> {
        let intact = self.url == url && !local.is_empty() && fingerprint(local) == self.sha256;
        if !intact {
            return Vec::new();
        }
        let etag = self.etag.as_deref().map(|etag| (IF_NONE_MATCH, etag));
        let since = self.last_modified.as_deref().map(|since| (IF_MODIFIED_SINCE, since));
        etag.into_iter().chain(since).collect()
    }
}

fn validators_path(dir: &Path, stem: &str) -> PathBuf {
    dir.join(format!("{stem}.meta"))
}

/// Сохранённые валидаторы и файл картинки, к которому они относятся.
async fn saved_validators(dir: &Path, stem: &str) -> Option<(Validators, Vec<u8>)> {
    let raw = fs::read(validators_path(dir, stem)).await.ok()?;
    let saved: Validators = serde_json::from_slice(&raw).ok()?;
    if !KNOWN_EXTENSIONS.contains(&saved.extension.as_str()) {
        return None;
    }
    let local = fs::read(dir.join(format!("{stem}.{}", saved.extension))).await.ok()?;
    Some((saved, local))
}

/// Валидаторы — после картинки: прервись запись посередине, отпечаток не
/// совпадёт, и следующий запрос уйдёт без условий.
async fn remember_validators(dir: &Path, stem: &str, validators: Option<Validators>) {
    let path = validators_path(dir, stem);
    let Some(validators) = validators else {
        let _ = fs::remove_file(&path).await;
        return;
    };
    if let Err(err) = help::save_json(&path, &validators).await {
        let _ = fs::remove_file(&path).await;
        logging!(debug, Type::Config, "picture validators not saved: {err:#}");
    }
}

pub async fn clear(uid: &str) {
    clear_picture(Picture::Logo, uid).await;
    clear_picture(Picture::Background, uid).await;
}

/// Что значит ответ сервера на запрос картинки.
#[derive(Debug, PartialEq, Eq)]
enum Reply {
    /// Не менялась: на диске ничего не трогаем, окну не говорим.
    Unchanged,
    /// Пришла картинка — записать.
    Fresh,
    /// Отказ — пробовать следующий маршрут.
    Refused,
}

/// 304 — ответ только на условный запрос: без условий он ничего не говорит о
/// файле на диске.
fn reply_to(asked_conditionally: bool, status: reqwest::StatusCode) -> Reply {
    if asked_conditionally && status == reqwest::StatusCode::NOT_MODIFIED {
        Reply::Unchanged
    } else if status.is_success() {
        Reply::Fresh
    } else {
        Reply::Refused
    }
}

/// Скачать картинку. `true` — файл записан заново, `false` — сервер ответил,
/// что она не менялась (304), и на диске ничего не тронуто.
async fn download(picture: Picture, uid: &str, url: &str) -> Result<bool> {
    if !is_safe_uid(uid) {
        bail!("refusing to cache a logo under an unexpected profile id");
    }
    let stem = picture.stem(uid);
    let max_bytes = picture.max_bytes();
    let saved = match cache_dir() {
        Ok(dir) => {
            help::sweep_staging_leftovers_of(&dir, &stem).await;
            saved_validators(&dir, &stem).await
        }
        Err(_) => None,
    };
    let conditional = saved
        .as_ref()
        .map(|(validators, local)| validators.conditional_for(url, local))
        .unwrap_or_default();
    let (conditional, stem) = (&conditional, stem.as_str());
    try_strategies(ProxyType::Localhost, [ProxyType::System], |proxy| async move {
        let client = NetworkManager::new()
            .create_request(proxy, Some(TIMEOUT_SECS), None, false)
            .await?;
        let mut request = client.get(url).header(reqwest::header::ACCEPT, "image/*");
        for (name, value) in conditional {
            request = request.header(name, *value);
        }
        let response = request.send().await?;
        match reply_to(!conditional.is_empty(), response.status()) {
            Reply::Unchanged => return Ok(false),
            Reply::Refused => bail!("logo request returned {}", response.status()),
            Reply::Fresh => {}
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

        let validators_headers = response.headers().clone();
        let bytes = NetworkManager::read_capped_bytes(response, max_bytes).await?;
        if bytes.is_empty() {
            bail!("logo response is empty");
        }

        store_fresh(&cache_dir()?, stem, url, extension, &validators_headers, &bytes).await?;
        Ok(true)
    })
    .await
}

/// Записать свежую картинку: файл, затем убрать её прежнюю под другим
/// расширением, затем валидаторы ответа (нет их — убрать прежние).
async fn store_fresh(
    dir: &Path,
    stem: &str,
    url: &str,
    extension: &str,
    headers: &HeaderMap,
    bytes: &[u8],
) -> Result<()> {
    fs::create_dir_all(dir).await?;
    help::write_atomic(&dir.join(format!("{stem}.{extension}")), bytes).await?;
    for stale in KNOWN_EXTENSIONS.iter().filter(|item| **item != extension) {
        let _ = fs::remove_file(dir.join(format!("{stem}.{stale}"))).await;
    }
    remember_validators(dir, stem, Validators::of(url, headers, extension, bytes)).await;
    Ok(())
}

/// Привести кэш картинки к подписке. `true` — картинка на диске сменилась
/// (скачана заново или убрана).
async fn sync_picture(picture: Picture, uid: &str) -> bool {
    match picture.url(uid).await {
        Some(url) if !url.trim().is_empty() => match download(picture, uid, url.trim()).await {
            Ok(written) => written,
            Err(err) => {
                logging!(warn, Type::Config, "profile {picture:?} for {uid} not cached: {err:#}");
                false
            }
        },
        _ => clear_picture(picture, uid).await,
    }
}

/// `true` — было что убрать.
async fn clear_picture(picture: Picture, uid: &str) -> bool {
    if !is_safe_uid(uid) {
        return false;
    }
    let Ok(dir) = cache_dir() else { return false };
    let stem = picture.stem(uid);
    let mut removed = false;
    for extension in KNOWN_EXTENSIONS {
        removed |= fs::remove_file(dir.join(format!("{stem}.{extension}"))).await.is_ok();
    }
    let _ = fs::remove_file(validators_path(&dir, &stem)).await;
    help::sweep_staging_leftovers_of(&dir, &stem).await;
    removed
}

/// После обновления подписки: картинки — к её заголовкам; сменившуюся окно
/// перечитывает по событию, а не по дате обновления подписки, которая меняется
/// раньше, чем картинка докачана.
pub async fn sync(uid: &str) {
    for picture in [Picture::Logo, Picture::Background] {
        if sync_picture(picture, uid).await {
            handle::Handle::notify_profile_picture(uid, picture.name());
        }
    }
}

pub async fn read(picture: Picture, uid: &str) -> Option<String> {
    if !is_safe_uid(uid) {
        return None;
    }
    let dir = cache_dir().ok()?;
    let stem = picture.stem(uid);
    for extension in KNOWN_EXTENSIONS {
        let path = dir.join(format!("{stem}.{extension}"));
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

pub async fn read_or_fetch(picture: Picture, uid: &str) -> Option<String> {
    if let Some(cached) = read(picture, uid).await {
        return Some(cached);
    }
    let miss_key = picture.stem(uid);
    if cold_miss_recently(&miss_key) {
        return None;
    }
    // Холодный кэш: картинку получает сам этот ответ, событие окну не нужно.
    let _ = sync_picture(picture, uid).await;
    let fresh = read(picture, uid).await;
    if fresh.is_none() {
        remember_cold_miss(&miss_key);
    }
    fresh
}

#[allow(clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::{
        KNOWN_EXTENSIONS, Reply, Validators, extension_for, is_safe_uid, mime_for, reply_to, saved_validators,
        store_fresh, validators_path,
    };
    use reqwest::header::{ETAG, HeaderMap, HeaderValue, IF_MODIFIED_SINCE, IF_NONE_MATCH, LAST_MODIFIED};

    const URL: &str = "https://cdn.example/logo.png";

    fn served(etag: Option<&str>, last_modified: Option<&str>) -> HeaderMap {
        let mut headers = HeaderMap::new();
        if let Some(etag) = etag {
            headers.insert(ETAG, HeaderValue::from_str(etag).expect("test etag"));
        }
        if let Some(since) = last_modified {
            headers.insert(LAST_MODIFIED, HeaderValue::from_str(since).expect("test date"));
        }
        headers
    }

    #[test]
    fn a_picture_is_asked_conditionally_only_while_the_file_is_intact() {
        let picture = b"png bytes".as_slice();
        let saved = Validators::of(
            URL,
            &served(Some("\"v1\""), Some("Wed, 01 Oct 2026 10:00:00 GMT")),
            "png",
            picture,
        )
        .expect("validators were sent");

        let asked = saved.conditional_for(URL, picture);
        assert_eq!(
            asked,
            [
                (IF_NONE_MATCH, "\"v1\""),
                (IF_MODIFIED_SINCE, "Wed, 01 Oct 2026 10:00:00 GMT")
            ]
        );
        assert!(saved.conditional_for(URL, b"other bytes").is_empty(), "файл подменён");
        assert!(saved.conditional_for(URL, b"").is_empty(), "файл пуст");
        assert!(
            saved.conditional_for("https://cdn.example/new.png", picture).is_empty(),
            "адрес сменился"
        );
    }

    #[test]
    fn a_server_without_validators_is_asked_as_before() {
        assert_eq!(Validators::of(URL, &served(None, None), "png", b"png"), None);
        let only_date = Validators::of(URL, &served(None, Some("Wed, 01 Oct 2026 10:00:00 GMT")), "png", b"png")
            .expect("a date is enough");
        assert_eq!(only_date.conditional_for(URL, b"png").len(), 1);
    }

    #[test]
    fn the_window_hears_only_of_a_changed_picture() {
        let source = crate::utils::source_scan::production_code(include_str!("logo_cache.rs"));
        let sync = crate::utils::source_scan::fn_body(source, "pub async fn sync(").unwrap_or_default();
        assert!(
            sync.contains("if sync_picture(") && sync.contains("notify_profile_picture("),
            "{sync}"
        );
        let download = crate::utils::source_scan::fn_body(source, "async fn download(").unwrap_or_default();
        assert!(download.contains("Reply::Unchanged => return Ok(false)"), "{download}");
    }

    #[tokio::test]
    async fn the_next_request_is_conditional_only_for_the_file_that_was_stored() {
        let dir = std::env::temp_dir().join(format!("clod-logo-store-{}", std::process::id()));
        let _ = tokio::fs::remove_dir_all(&dir).await;
        let conditional = async || {
            saved_validators(&dir, "sub")
                .await
                .map(|(saved, local)| saved.conditional_for(URL, &local).len())
                .unwrap_or_default()
        };

        let etag = served(Some("\"v1\""), None);
        store_fresh(&dir, "sub", URL, "png", &etag, b"png bytes")
            .await
            .expect("stored");
        assert_eq!(conditional().await, 1, "следующий запрос — с If-None-Match");

        // Файл переписан мимо валидаторов — спрашиваем без условий.
        tokio::fs::write(dir.join("sub.png"), b"other bytes")
            .await
            .expect("rewritten");
        assert_eq!(conditional().await, 0);

        // Новая картинка без валидаторов — прежние не остаются.
        store_fresh(&dir, "sub", URL, "png", &served(None, None), b"png bytes")
            .await
            .expect("stored");
        assert!(!validators_path(&dir, "sub").exists());
        assert_eq!(conditional().await, 0);

        // Картинка сменила формат — прежний файл убран, валидаторы — к новому.
        store_fresh(&dir, "sub", URL, "svg", &etag, b"<svg/>")
            .await
            .expect("stored");
        assert!(!dir.join("sub.png").exists());
        assert!(dir.join("sub.svg").exists());
        assert_eq!(conditional().await, 1);

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[test]
    fn not_modified_counts_only_as_the_answer_to_a_conditional_request() {
        use reqwest::StatusCode;
        assert_eq!(reply_to(true, StatusCode::NOT_MODIFIED), Reply::Unchanged);
        assert_eq!(
            reply_to(false, StatusCode::NOT_MODIFIED),
            Reply::Refused,
            "без условий 304 о файле на диске ничего не говорит"
        );
        assert_eq!(reply_to(true, StatusCode::OK), Reply::Fresh);
        assert_eq!(reply_to(false, StatusCode::OK), Reply::Fresh);
        assert_eq!(reply_to(true, StatusCode::NOT_FOUND), Reply::Refused);
    }

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
