use crate::{
    config::Config,
    core::handle,
    utils::{
        dirs, help,
        network::ProxyType,
        public_url::{MAX_IMAGE_BYTES, is_refused, read_image, request_image},
        retry::try_strategies_unless,
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
    collections::{BTreeSet, HashMap},
    path::{Path, PathBuf},
};
use tokio::fs;

const MAX_BACKGROUND_BYTES: usize = 4 * 1024 * 1024;

const KNOWN_EXTENSIONS: &[&str] = &["png", "svg", "jpg", "webp", "avif", "gif", "ico", "bmp"];

fn cache_dir() -> Result<PathBuf> {
    Ok(dirs::app_home_dir()?.join("logos"))
}

fn is_safe_uid(uid: &str) -> bool {
    !uid.is_empty() && uid.len() <= 64 && uid.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
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

/// Что окно ставит на место картинки провайдера.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Shown {
    /// Копия с диска, `data:`-URL.
    Ready(String),
    /// Копии пока нет: ещё не скачана или сервер не ответил.
    Pending,
    /// Копии не будет, пока адрес в подписке тот же: картинки нет или
    /// приложение её не берёт (не публичный https, не картинка, сверх потолка).
    Never,
}

impl Shown {
    pub fn ready(self) -> Option<String> {
        match self {
            Self::Ready(src) => Some(src),
            Self::Pending | Self::Never => None,
        }
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
            Self::Logo => MAX_IMAGE_BYTES,
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
    if let Ok(dir) = cache_dir()
        && is_safe_uid(uid)
    {
        for stem in cached_icons(&dir, uid).await {
            remove_stem(&dir, &stem).await;
        }
    }
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
async fn download(stem: &str, max_bytes: usize, url: &str) -> Result<bool> {
    let saved = match cache_dir() {
        Ok(dir) => {
            help::sweep_staging_leftovers_of(&dir, stem).await;
            saved_validators(&dir, stem).await
        }
        Err(_) => None,
    };
    let conditional = saved
        .as_ref()
        .map(|(validators, local)| validators.conditional_for(url, local))
        .unwrap_or_default();
    let conditional = &conditional;
    let attempt = |proxy| async move {
        let mut headers = HeaderMap::new();
        headers.insert(
            reqwest::header::ACCEPT,
            reqwest::header::HeaderValue::from_static("image/*"),
        );
        for (name, value) in conditional {
            headers.insert(name.clone(), reqwest::header::HeaderValue::from_str(value)?);
        }
        let response = request_image(url, proxy, headers).await?;
        match reply_to(!conditional.is_empty(), response.status()) {
            Reply::Unchanged => return Ok(false),
            Reply::Refused => bail!("logo request returned {}", response.status()),
            Reply::Fresh => {}
        }

        let validators_headers = response.headers().clone();
        let (bytes, extension) = read_image(response, max_bytes).await?;
        store_fresh(&cache_dir()?, stem, url, extension, &validators_headers, &bytes).await?;
        Ok(true)
    };
    try_strategies_unless(ProxyType::Localhost, [ProxyType::System], attempt, is_refused).await
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
        Some(url) if !url.trim().is_empty() => {
            match download(&picture.stem(uid), picture.max_bytes(), url.trim()).await {
                Ok(written) => written,
                Err(err) => {
                    logging!(warn, Type::Config, "profile {picture:?} for {uid} not cached: {err:#}");
                    false
                }
            }
        }
        _ => clear_picture(picture, uid).await,
    }
}

/// `true` — было что убрать.
async fn clear_picture(picture: Picture, uid: &str) -> bool {
    let Ok(dir) = cache_dir() else { return false };
    is_safe_uid(uid) && remove_stem(&dir, &picture.stem(uid)).await
}

/// `true` — было что убрать.
async fn remove_stem(dir: &Path, stem: &str) -> bool {
    let mut removed = false;
    for extension in KNOWN_EXTENSIONS {
        removed |= fs::remove_file(dir.join(format!("{stem}.{extension}"))).await.is_ok();
    }
    let _ = fs::remove_file(validators_path(dir, stem)).await;
    help::sweep_staging_leftovers_of(dir, stem).await;
    removed
}

const ICON_INFIX: &str = ".icon-";

/// Значок группы подписки `uid`: имя файла — от подписки и адреса целиком,
/// имя группы в нём не участвует.
fn icon_stem(uid: &str, url: &str) -> std::string::String {
    format!("{uid}{ICON_INFIX}{}", fingerprint(url.as_bytes()))
}

/// Скачанные значки групп подписки `uid` — имена файлов без расширения.
async fn cached_icons(dir: &Path, uid: &str) -> BTreeSet<std::string::String> {
    let prefix = format!("{uid}{ICON_INFIX}");
    let mut found = BTreeSet::new();
    let Ok(mut entries) = fs::read_dir(dir).await else {
        return found;
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name().to_string_lossy().into_owned();
        if let Some(hash) = name.strip_prefix(&prefix).and_then(|rest| rest.split('.').next()) {
            found.insert(format!("{prefix}{hash}"));
        }
    }
    found
}

fn icons_of(groups: Option<&serde_yaml_ng::Value>) -> impl Iterator<Item = std::string::String> + '_ {
    groups
        .and_then(serde_yaml_ng::Value::as_sequence)
        .into_iter()
        .flatten()
        .filter_map(|group| group.get("icon")?.as_str())
        .map(str::trim)
        .filter(|url| !url.is_empty())
        .map(str::to_owned)
}

/// Адреса значков, которые окно может спросить у подписки; `None` — подписку
/// прочитать не удалось, и судить, какие значки ей не нужны, нельзя.
async fn group_icon_urls(uid: &str) -> Option<BTreeSet<std::string::String>> {
    let (item, own_groups) = {
        let profiles = Config::profiles().await;
        let arc = profiles.latest_arc();
        let item = arc.get_item(uid).ok()?.clone();
        let own_groups = item
            .current_groups()
            .and_then(|groups| arc.get_item(groups).ok())
            .cloned();
        drop(arc);
        (item, own_groups)
    };
    let assembled = {
        let runtime = Config::runtime().await.data_arc();
        (runtime.profile_uid.as_deref() == Some(uid))
            .then(|| runtime.config.clone())
            .flatten()
    };
    let own = match own_groups {
        Some(own_groups) => own_groups.read_file().await.ok(),
        None => None,
    };
    wanted_icons(&item.read_file().await.ok()?, own.as_deref(), assembled.as_ref())
}

/// Значки групп файла подписки (с раскрытыми якорями) и её групп человека —
/// так их показывает редактор групп, — а если из подписки собран запущенный
/// конфиг, то и его групп (с Merge и Script), которые показывает окно прокси.
fn wanted_icons(
    subscription: &str,
    own: Option<&str>,
    assembled: Option<&serde_yaml_ng::Mapping>,
) -> Option<BTreeSet<std::string::String>> {
    let mut subscription: serde_yaml_ng::Value = serde_yaml_ng::from_str(subscription).ok()?;
    help::apply_merge_transitively(&mut subscription, "subscription").ok()?;
    let mut urls: BTreeSet<_> = icons_of(subscription.get("proxy-groups")).collect();
    if let Some(own) = own.and_then(|raw| serde_yaml_ng::from_str::<serde_yaml_ng::Mapping>(raw).ok()) {
        urls.extend(icons_of(own.get("prepend")));
        urls.extend(icons_of(own.get("append")));
    }
    if let Some(assembled) = assembled {
        urls.extend(icons_of(assembled.get("proxy-groups")));
    }
    Some(urls)
}

/// Значки групп — как логотип: после обновления подписки скачанные
/// перепроверяются, а тех, что подписке больше не нужны, на диске не остаётся.
async fn sync_icons(uid: &str) -> bool {
    let (Ok(dir), Some(wanted)) = (cache_dir(), group_icon_urls(uid).await) else {
        return false;
    };
    let wanted: HashMap<_, _> = wanted.into_iter().map(|url| (icon_stem(uid, &url), url)).collect();
    let mut changed = false;
    for stem in cached_icons(&dir, uid).await {
        changed |= match wanted.get(&stem) {
            Some(url) => download(&stem, MAX_IMAGE_BYTES, url).await.unwrap_or_else(|err| {
                logging!(warn, Type::Config, "group icon for {uid} not refreshed: {err:#}");
                false
            }),
            None => remove_stem(&dir, &stem).await,
        };
    }
    changed
}

/// После обновления подписки: картинки — к её заголовкам; сменившуюся окно
/// перечитывает по событию, а не по дате обновления подписки, которая меняется
/// раньше, чем картинка докачана.
pub async fn sync(uid: &str) {
    if !is_safe_uid(uid) {
        return;
    }
    for picture in [Picture::Logo, Picture::Background] {
        if sync_picture(picture, uid).await {
            handle::Handle::notify_profile_picture(uid, picture.name());
        }
    }
    if sync_icons(uid).await {
        handle::Handle::notify_profile_picture(uid, "icons");
    }
}

async fn read(stem: &str) -> Option<String> {
    let dir = cache_dir().ok()?;
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

/// Неудачная попытка и то, был ли это отказ (`Shown::Never`).
type ColdMiss = (std::time::Instant, bool);

static COLD_MISSES: std::sync::OnceLock<std::sync::Mutex<HashMap<String, ColdMiss>>> = std::sync::OnceLock::new();

fn cold_misses() -> &'static std::sync::Mutex<HashMap<String, ColdMiss>> {
    COLD_MISSES.get_or_init(|| std::sync::Mutex::new(HashMap::new()))
}

/// Ответ окну, пока прошлая неудача свежая.
fn recent_cold_miss(stem: &str) -> Option<Shown> {
    let (at, refused) = cold_misses().lock().ok()?.get(stem).copied()?;
    (at.elapsed() < RETRY_AFTER).then_some(if refused { Shown::Never } else { Shown::Pending })
}

const MAX_COLD_MISSES: usize = 256;

fn remember_cold_miss(stem: &str, refused: bool) -> Shown {
    if let Ok(mut slot) = cold_misses().lock() {
        if slot.len() >= MAX_COLD_MISSES {
            slot.retain(|_, (at, _)| at.elapsed() < RETRY_AFTER);
        }
        if slot.len() < MAX_COLD_MISSES {
            slot.insert(stem.into(), (std::time::Instant::now(), refused));
        }
    }
    if refused { Shown::Never } else { Shown::Pending }
}

/// Копия есть — она; нет — скачать сейчас же, если прошлая неудача не свежая.
/// Холодный кэш получает картинку самим этим ответом, событие окну не нужно.
async fn cached_or_fetched(stem: &str, url: Option<&str>, max_bytes: usize) -> Shown {
    if let Some(cached) = read(stem).await {
        return Shown::Ready(cached);
    }
    if let Some(shown) = recent_cold_miss(stem) {
        return shown;
    }
    let Some(url) = url else {
        return Shown::Never;
    };
    let refused = match download(stem, max_bytes, url).await {
        Ok(_) => false,
        Err(err) => {
            logging!(warn, Type::Config, "provider picture {stem} not cached: {err:#}");
            is_refused(&err)
        }
    };
    match read(stem).await {
        Some(fresh) => Shown::Ready(fresh),
        None => remember_cold_miss(stem, refused),
    }
}

pub async fn read_or_fetch(picture: Picture, uid: &str) -> Shown {
    if !is_safe_uid(uid) {
        return Shown::Never;
    }
    let url = picture.url(uid).await;
    let url = url.as_deref().map(str::trim).filter(|url| !url.is_empty());
    cached_or_fetched(&picture.stem(uid), url, picture.max_bytes()).await
}

/// Значок группы подписки `uid`; копия своя у каждой подписки и уходит вместе
/// с ней.
pub async fn group_icon(uid: &str, url: &str) -> Shown {
    let url = url.trim();
    if !is_safe_uid(uid) || url.is_empty() || Config::profiles().await.latest_arc().get_item(uid).is_err() {
        return Shown::Never;
    }
    cached_or_fetched(&icon_stem(uid, url), Some(url), MAX_IMAGE_BYTES).await
}

#[allow(clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::{
        KNOWN_EXTENSIONS, Reply, Validators, cached_icons, icon_stem, icons_of, is_safe_uid, mime_for, remove_stem,
        reply_to, saved_validators, store_fresh, validators_path, wanted_icons,
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
        for (_, extension) in crate::utils::public_url::tests::IMAGE_SAMPLES {
            assert!(
                KNOWN_EXTENSIONS.contains(&extension),
                "{extension} is written but never read back"
            );
            assert!(!mime_for(extension).is_empty());
        }
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
    fn a_group_icon_is_stored_under_a_name_every_system_accepts() {
        let tail = "https://cdn.example/a/icon.png";
        for url in [
            tail,
            "https://cdn.example/icon.png?v=2",
            "https://cdn.example/x|y:z.png",
        ] {
            let stem = icon_stem("sub1", url);
            assert!(
                stem.chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
                    && !stem.contains(".."),
                "{url} -> {stem}"
            );
        }
        assert_ne!(
            icon_stem("sub1", tail),
            icon_stem("sub2", tail),
            "у каждой подписки своя копия"
        );
        assert_ne!(
            icon_stem("sub1", tail),
            icon_stem("sub1", "https://cdn.example/b/icon.png"),
            "одинаковый хвост адреса — не тот же значок"
        );
    }

    #[tokio::test]
    async fn the_icons_of_a_subscription_are_its_own() {
        let dir = std::env::temp_dir().join(format!("clod-group-icons-{}", std::process::id()));
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(&dir).await.expect("dir");
        let (first, second) = ("https://cdn.example/a.png", "https://cdn.example/b.svg");
        let ours = icon_stem("sub1", first);
        let names = [
            "sub1.png".to_owned(),
            "sub1.meta".to_owned(),
            "sub1.bg.png".to_owned(),
            format!("{ours}.png"),
            format!("{ours}.meta"),
            format!("{}.svg", icon_stem("sub1", second)),
            format!("{}.png", icon_stem("sub2", first)),
        ];
        for name in &names {
            tokio::fs::write(dir.join(name), b"x").await.expect("written");
        }

        let found = cached_icons(&dir, "sub1").await;
        assert_eq!(found, [ours.clone(), icon_stem("sub1", second)].into_iter().collect());

        assert!(remove_stem(&dir, &ours).await);
        assert!(!dir.join(format!("{ours}.png")).exists());
        assert!(!dir.join(format!("{ours}.meta")).exists());
        assert!(dir.join("sub1.png").exists(), "логотип на месте");
        assert!(
            dir.join(format!("{}.png", icon_stem("sub2", first))).exists(),
            "чужая подписка на месте"
        );

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[test]
    fn the_icons_wanted_are_those_of_the_groups() {
        let config: serde_yaml_ng::Mapping = serde_yaml_ng::from_str(
            "proxy-groups:\n  - {name: 'A | B', icon: ' https://cdn.example/a.png '}\n  - {name: X/Y}\n  - {name: Z, icon: 5}\n  - {name: W, icon: ''}\n",
        )
        .expect("yaml");
        assert_eq!(
            icons_of(config.get("proxy-groups")).collect::<Vec<_>>(),
            ["https://cdn.example/a.png"]
        );
        assert_eq!(icons_of(None).count(), 0);
    }

    #[test]
    fn the_icons_wanted_include_anchors_and_the_running_build() {
        let subscription = "base: &base {type: select, icon: 'https://cdn.example/anchor.png'}\nproxy-groups:\n  - {<<: *base, name: A}\n  - {name: B, icon: 'https://cdn.example/b.png'}\n";
        let own = "prepend:\n  - {name: Mine, icon: 'https://cdn.example/mine.png'}\n";
        let assembled: serde_yaml_ng::Mapping =
            serde_yaml_ng::from_str("proxy-groups:\n  - {name: Merged, icon: 'https://cdn.example/merge.png'}\n")
                .expect("yaml");

        let wanted = |assembled| {
            wanted_icons(subscription, Some(own), assembled)
                .expect("подписка читается")
                .into_iter()
                .collect::<Vec<_>>()
        };
        assert_eq!(
            wanted(None),
            [
                "https://cdn.example/anchor.png",
                "https://cdn.example/b.png",
                "https://cdn.example/mine.png"
            ]
        );
        assert!(
            wanted(Some(&assembled)).contains(&"https://cdn.example/merge.png".to_owned()),
            "значок из Merge запущенной сборки"
        );
        assert_eq!(wanted_icons("proxy-groups: [", None, None), None);
    }
}
