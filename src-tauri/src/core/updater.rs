use crate::{config::Config, singleton, utils::dirs};
use anyhow::{Result, anyhow};
use chrono::Utc;
use clash_verge_logging::{Type, logging};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use tauri_plugin_updater::{Update, UpdaterExt as _};
use tokio_util::sync::CancellationToken;

pub struct SilentUpdater {
    update_ready: AtomicBool,
    /// Отмена идущей ручной установки: ею владеет одна команда на всё
    /// приложение, а окон обновления два — «Отмена» любого доходит сюда.
    /// Номер — чтобы завершающаяся установка забрала свой жетон, а не чужой.
    manual_cancel: parking_lot::Mutex<Option<(u64, CancellationToken)>>,
}

/// Ход ручной загрузки для окна обновления — в той же форме, что у плагина
/// (`DownloadEvent` из `@tauri-apps/plugin-updater`).
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event", content = "data")]
pub enum DownloadEvent {
    #[serde(rename_all = "camelCase")]
    Started {
        content_length: Option<u64>,
    },
    #[serde(rename_all = "camelCase")]
    Progress {
        chunk_length: usize,
    },
    Finished,
}

/// Установщик запущен и ещё не вернулся. Ставить два обновления разом нельзя:
/// вопрос на старте и окно обновления живут одновременно.
static INSTALLING: AtomicBool = AtomicBool::new(false);

static MANUAL_INSTALLS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Признак «установщик работает», снимаемый при неудаче и при панике. После
/// удачной установки процесс уже на пути к перезапуску — признак остаётся до
/// его конца, и вторая установка той же версии поверх идущего перезапуска не
/// начнётся.
struct InstallerRunning;

impl InstallerRunning {
    fn claim() -> Option<Self> {
        (!INSTALLING.swap(true, Ordering::AcqRel)).then_some(Self)
    }
}

impl InstallerRunning {
    fn run<E>(self, install: impl FnOnce() -> std::result::Result<(), E>) -> std::result::Result<(), E> {
        let installed = install();
        if installed.is_ok() {
            std::mem::forget(self);
        }
        installed
    }
}

impl Drop for InstallerRunning {
    fn drop(&mut self) {
        INSTALLING.store(false, Ordering::Release);
    }
}

/// Место ручной установки: освобождается, как только её не стало, — даже если
/// её уронили. Чужое место (следующей установки) не трогает.
struct ManualSlot {
    ticket: u64,
}

impl Drop for ManualSlot {
    fn drop(&mut self) {
        let mut slot = SilentUpdater::global().manual_cancel.lock();
        if slot.as_ref().is_some_and(|(owner, _)| *owner == self.ticket) {
            slot.take();
        }
    }
}

singleton!(SilentUpdater, SILENT_UPDATER);

impl SilentUpdater {
    const fn new() -> Self {
        Self {
            update_ready: AtomicBool::new(false),
            manual_cancel: parking_lot::Mutex::new(None),
        }
    }

    pub fn is_update_ready(&self) -> bool {
        self.update_ready.load(Ordering::Acquire)
    }
}

#[derive(Serialize, Deserialize)]
struct UpdateCacheMeta {
    version: String,
    downloaded_at: String,
}

impl SilentUpdater {
    fn cache_dir() -> Result<PathBuf> {
        Ok(dirs::app_home_dir()?.join("update_cache"))
    }

    async fn write_cache(bytes: &[u8], version: &str) -> Result<()> {
        let cache_dir = Self::cache_dir()?;
        std::fs::create_dir_all(&cache_dir)?;

        let bin_path = cache_dir.join("pending_update.bin");
        crate::utils::help::write_atomic(&bin_path, bytes).await?;

        let meta = UpdateCacheMeta {
            version: version.to_string(),
            downloaded_at: Utc::now().to_rfc3339(),
        };
        let meta_path = cache_dir.join("pending_update.json");
        crate::utils::help::write_atomic(&meta_path, serde_json::to_string_pretty(&meta)?.as_bytes()).await?;

        logging!(
            info,
            Type::System,
            "Update cache written: version={}, size={} bytes",
            version,
            bytes.len()
        );
        Ok(())
    }

    fn read_cache_bytes() -> Result<Vec<u8>> {
        let bin_path = Self::cache_dir()?.join("pending_update.bin");
        Ok(std::fs::read(bin_path)?)
    }

    fn read_cache_meta() -> Result<UpdateCacheMeta> {
        let meta_path = Self::cache_dir()?.join("pending_update.json");
        let content = std::fs::read_to_string(meta_path)?;
        Ok(serde_json::from_str(&content)?)
    }

    fn delete_cache() {
        if let Ok(cache_dir) = Self::cache_dir()
            && cache_dir.exists()
        {
            if let Err(e) = std::fs::remove_dir_all(&cache_dir) {
                logging!(warn, Type::System, "Failed to delete update cache: {e}");
            } else {
                logging!(info, Type::System, "Update cache deleted");
            }
        }
    }
}

fn parse_version(raw: &str) -> Option<(Vec<u64>, Option<Vec<std::string::String>>)> {
    let body = raw.trim().trim_start_matches('v');
    let body = body.split_once('+').map_or(body, |(core, _)| core);
    let (core, prerelease) = match body.split_once('-') {
        Some((core, pre)) => (core, Some(pre)),
        None => (body, None),
    };
    let numbers = core
        .split('.')
        .map(|part| part.parse::<u64>().ok())
        .collect::<Option<Vec<_>>>()?;
    if numbers.is_empty() {
        return None;
    }
    let prerelease = match prerelease {
        Some(pre) if !pre.is_empty() => Some(pre.split('.').map(std::string::String::from).collect()),
        Some(_) => return None,
        None => None,
    };
    Some((numbers, prerelease))
}

fn compare_prerelease(a: &[std::string::String], b: &[std::string::String]) -> std::cmp::Ordering {
    use std::cmp::Ordering;

    for (x, y) in a.iter().zip(b) {
        let order = match (x.parse::<u64>(), y.parse::<u64>()) {
            (Ok(x), Ok(y)) => x.cmp(&y),
            (Ok(_), Err(_)) => Ordering::Less,
            (Err(_), Ok(_)) => Ordering::Greater,
            (Err(_), Err(_)) => x.cmp(y),
        };
        if order != Ordering::Equal {
            return order;
        }
    }
    a.len().cmp(&b.len())
}

fn version_lte(a: &str, b: &str) -> bool {
    use std::cmp::Ordering;

    let (Some((a_num, a_pre)), Some((b_num, b_pre))) = (parse_version(a), parse_version(b)) else {
        return true;
    };
    let len = a_num.len().max(b_num.len());
    for i in 0..len {
        let av = a_num.get(i).copied().unwrap_or(0);
        let bv = b_num.get(i).copied().unwrap_or(0);
        match av.cmp(&bv) {
            Ordering::Less => return true,
            Ordering::Greater => return false,
            Ordering::Equal => {}
        }
    }
    match (a_pre, b_pre) {
        (None, None) => true,
        (None, Some(_)) => false,
        (Some(_), None) => true,
        (Some(a_pre), Some(b_pre)) => compare_prerelease(&a_pre, &b_pre) != Ordering::Greater,
    }
}

fn is_prerelease_version(version: &str) -> bool {
    version
        .trim_start_matches('v')
        .split_once('-')
        .is_some_and(|(_, suffix)| !suffix.is_empty())
}

fn manifest_answers_channel(served_version: Option<&str>, receive_prereleases: bool) -> bool {
    match served_version {
        Some(version) => receive_prereleases || !is_prerelease_version(version),
        None => true,
    }
}

fn verify_minisign(pubkey_b64: &str, signature_b64: &str, bytes: &[u8]) -> Result<()> {
    use base64::Engine as _;

    let decode_block = |value: &str, what: &str| -> Result<String> {
        let raw = base64::engine::general_purpose::STANDARD
            .decode(value)
            .map_err(|e| anyhow!("{what} is not valid base64: {e}"))?;
        String::from_utf8(raw).map_err(|e| anyhow!("{what} is not valid utf-8: {e}"))
    };

    let public_key = minisign_verify::PublicKey::decode(&decode_block(pubkey_b64, "updater pubkey")?)
        .map_err(|e| anyhow!("updater pubkey is malformed: {e}"))?;
    let signature = minisign_verify::Signature::decode(&decode_block(signature_b64, "update signature")?)
        .map_err(|e| anyhow!("update signature is malformed: {e}"))?;

    public_key
        .verify(bytes, &signature, true)
        .map_err(|e| anyhow!("signature does not match the bytes: {e}"))
}

const PRERELEASE_UPDATER_ENDPOINT: &str =
    "https://github.com/Mrvibecodic/clod-clash/releases/download/updater-prerelease/latest.json";

fn configured_endpoints(app_handle: &tauri::AppHandle) -> Vec<String> {
    app_handle
        .config()
        .plugins
        .0
        .get("updater")
        .and_then(|cfg| cfg.get("endpoints"))
        .and_then(serde_json::Value::as_array)
        .map(|endpoints| {
            endpoints
                .iter()
                .filter_map(|endpoint| endpoint.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

fn parse_endpoints(endpoints: &[String]) -> Result<Vec<tauri::Url>> {
    endpoints
        .iter()
        .map(|endpoint| tauri::Url::parse(endpoint).map_err(|e| anyhow!("bad updater endpoint {endpoint}: {e}")))
        .collect()
}

fn stable_endpoints(app_handle: &tauri::AppHandle) -> Result<Vec<tauri::Url>> {
    let endpoints = configured_endpoints(app_handle);
    if endpoints.is_empty() {
        return Err(anyhow!("tauri.conf.json has no plugins.updater.endpoints"));
    }

    parse_endpoints(&endpoints)
}

fn updater_pubkey(app_handle: &tauri::AppHandle) -> Result<String> {
    app_handle
        .config()
        .plugins
        .0
        .get("updater")
        .and_then(|cfg| cfg.get("pubkey"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| anyhow!("tauri.conf.json has no plugins.updater.pubkey"))
}

impl SilentUpdater {
    /// Нужна ли ещё скачанная версия; нет — кэш выбрасывает вызывающий.
    async fn cache_still_wanted(cached_version: &str, current_version: &str) -> bool {
        let verge = Config::verge().await.latest_arc();

        // Кэш наполняет только автопроверка: выключил её человек — и готового
        // обновления он не ждёт, спрашивать на старте незачем.
        if !verge.auto_check_update.unwrap_or(true) {
            logging!(
                info,
                Type::System,
                "Auto update check is off, discarding the cached update ({})",
                cached_version
            );
            return false;
        }

        if version_lte(cached_version, current_version) {
            logging!(
                info,
                Type::System,
                "Update cache version ({}) <= current ({}), cleaning up",
                cached_version,
                current_version
            );
            return false;
        }

        if is_prerelease_version(cached_version)
            && !verge
                .receive_prereleases
                .unwrap_or(crate::config::IVerge::DEFAULT_RECEIVE_PRERELEASES)
        {
            logging!(
                info,
                Type::System,
                "Cached update ({}) is a pre-release and pre-releases are off, cleaning up",
                cached_version
            );
            return false;
        }

        true
    }
}

impl SilentUpdater {
    pub async fn try_install_on_startup(&self, app_handle: &tauri::AppHandle) -> bool {
        let current_version = env!("CARGO_PKG_VERSION");

        let meta = match Self::read_cache_meta() {
            Ok(meta) => meta,
            Err(_) => return false,
        };

        let cached_version = &meta.version;

        if !Self::cache_still_wanted(cached_version, current_version).await {
            Self::delete_cache();
            return false;
        }

        logging!(
            info,
            Type::System,
            "Update cache version ({}) > current ({}), asking user to install",
            cached_version,
            current_version
        );

        if !Self::ask_user_to_install(app_handle, cached_version).await {
            logging!(info, Type::System, "User skipped update install, starting normally");
            return false;
        }

        let bytes = match Self::read_cache_bytes() {
            Ok(b) => b,
            Err(e) => {
                logging!(
                    warn,
                    Type::System,
                    "Failed to read cached update bytes: {e}, cleaning up"
                );
                Self::delete_cache();
                return false;
            }
        };

        let update = match check_update_with_fallback(app_handle).await {
            Ok(Some(u)) => u,
            Ok(None) => {
                logging!(
                    info,
                    Type::System,
                    "No update available from server, cache may be stale, cleaning up"
                );
                Self::delete_cache();
                return false;
            }
            Err(e) => {
                logging!(
                    warn,
                    Type::System,
                    "Failed to check for update at startup: {e}, will retry next launch"
                );
                return false;
            }
        };

        if update.version != *cached_version {
            logging!(
                info,
                Type::System,
                "Server version ({}) != cached version ({}), cache is stale, cleaning up",
                update.version,
                cached_version
            );
            Self::delete_cache();
            return false;
        }

        if let Err(e) =
            updater_pubkey(app_handle).and_then(|pubkey| verify_minisign(&pubkey, &update.signature, &bytes))
        {
            logging!(
                error,
                Type::System,
                "Cached update v{} failed the signature check ({e}), discarding it",
                cached_version
            );
            Self::delete_cache();
            return false;
        }

        let version = update.version.clone();
        logging!(info, Type::System, "Installing cached update v{version} at startup...");

        Self::show_update_splash(app_handle, &version);

        let install = tokio::time::timeout(std::time::Duration::from_secs(30), Self::install(update, bytes));
        let success = match install.await {
            Ok(Ok(())) => {
                logging!(info, Type::System, "Update v{version} install triggered at startup");
                Self::delete_cache();
                true
            }
            Ok(Err(e)) => {
                logging!(
                    warn,
                    Type::System,
                    "Startup install failed: {e}, will retry next launch"
                );
                false
            }
            Err(_) => {
                logging!(
                    warn,
                    Type::System,
                    "Startup install timed out (30s), will retry next launch"
                );
                false
            }
        };

        if !success {
            Self::close_update_splash(app_handle);
        }

        success
    }
}

impl SilentUpdater {
    async fn ask_user_to_install(app_handle: &tauri::AppHandle, version: &str) -> bool {
        use tauri_plugin_dialog::{DialogExt as _, MessageDialogButtons, MessageDialogKind};

        let title = clash_verge_i18n::t!("notifications.updateReady.title");
        let body = clash_verge_i18n::t!("notifications.updateReady.body").replace("{version}", version);
        let install_now = clash_verge_i18n::t!("notifications.updateReady.installNow").into_owned();
        let later = clash_verge_i18n::t!("notifications.updateReady.later").into_owned();

        let (tx, rx) = tokio::sync::oneshot::channel();

        app_handle
            .dialog()
            .message(body)
            .title(title)
            .buttons(MessageDialogButtons::OkCancelCustom(install_now, later))
            .kind(MessageDialogKind::Info)
            .show(move |confirmed| {
                let _ = tx.send(confirmed);
            });

        rx.await.unwrap_or(false)
    }
}

impl SilentUpdater {
    fn show_update_splash(app_handle: &tauri::AppHandle, version: &str) {
        use tauri::{WebviewUrl, WebviewWindowBuilder};

        // Заставка — своя статичная страница: страница приложения запустила бы
        // в этом окне весь интерфейс с его командами и забором уведомлений.
        let version_script = format!("window.__CLOD_UPDATE_VERSION__ = {};", serde_json::Value::from(version));
        if let Err(e) = WebviewWindowBuilder::new(
            app_handle,
            "update-splash",
            WebviewUrl::App("update-splash.html".into()),
        )
        .title(format!("{} - Updating", crate::constants::branding::APP_NAME))
        .inner_size(300.0, 180.0)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .closable(false)
        .decorations(false)
        .center()
        .always_on_top(true)
        .visible(true)
        .initialization_script(&version_script)
        .build()
        {
            logging!(warn, Type::System, "Failed to create update splash: {e}");
            return;
        }

        logging!(info, Type::System, "Update splash window shown");
    }

    fn close_update_splash(app_handle: &tauri::AppHandle) {
        use tauri::Manager as _;
        if let Some(window) = app_handle.get_webview_window("update-splash") {
            let _ = window.close();
            logging!(info, Type::System, "Update splash window closed");
        }
    }
}

#[cfg(target_os = "windows")]
fn nsis_language_id(app_language: &str) -> &'static str {
    match app_language {
        "zh" | "zhtw" => "2052",
        "ru" => "1049",
        _ => "1033",
    }
}

fn updater_builder(
    app_handle: &tauri::AppHandle,
    language: Option<&str>,
    endpoints: Vec<tauri::Url>,
    proxy: Option<&tauri::Url>,
) -> Result<tauri_plugin_updater::UpdaterBuilder> {
    let _ = language;
    let builder = app_handle.updater_builder();
    #[cfg(target_os = "windows")]
    let builder = {
        let lang_id = nsis_language_id(&clash_verge_i18n::current_language(language));
        // Плагин перед установщиком завершает процесс сам, `exit(0)`, и наш
        // выход не выполняется: уборка здесь та же, что перед аварийным
        // перезапуском. Ядро под службой не трогаем — если человек откажет
        // установщику в правах, процесс всё равно завершится, а VPN должен
        // это пережить. Ядро, которое служба после установки поднимет сама,
        // заменит запуск новой копии под службой, а если служба к её старту
        // ещё не готова — перезапуск ядра, если к проверке порта она уже
        // готова. Собственный хук плагина (уборка ресурсов Tauri)
        // заменяется, поэтому зовётся следом.
        let app = app_handle.clone();
        builder
            .installer_arg(format!("/LANG={lang_id}"))
            .on_before_exit(move || {
                crate::feat::tidy_up_before_an_abrupt_end("установкой обновления");
                app.cleanup_before_exit();
            })
    };
    // Без прокси — по-настоящему напрямую: иначе запрос молча ушёл бы через
    // системный прокси, если он включён.
    let builder = match proxy {
        Some(proxy) => builder.proxy(proxy.clone()),
        None => builder.no_proxy(),
    };
    builder
        .endpoints(endpoints)
        .map_err(|e| anyhow!("failed to point the updater at the update manifest: {e}"))
}

async fn check_endpoints(
    app_handle: &tauri::AppHandle,
    language: Option<&str>,
    endpoints: Vec<tauri::Url>,
    proxy: Option<&tauri::Url>,
) -> Result<(Option<Update>, Option<String>)> {
    let served_version: Arc<RwLock<Option<String>>> = Arc::default();
    let recorder = Arc::clone(&served_version);
    let updater = updater_builder(app_handle, language, endpoints, proxy)?
        .version_comparator(move |current, release| {
            *recorder.write() = Some(release.version.to_string());
            release.version > current
        })
        .build()?;
    let found = updater.check().await?;
    let served = served_version.read().clone();
    Ok((found, served))
}

async fn check_update_on_channel(
    app_handle: &tauri::AppHandle,
    language: Option<&str>,
    receive_prereleases: bool,
    proxy: Option<&tauri::Url>,
) -> Result<Option<Update>> {
    if receive_prereleases {
        // Только манифест канала пре-релизов: релиз кладёт туда любую свежую версию,
        // стабильную тоже. Запасной стабильный манифест при сбое этого подсунул бы
        // старую версию — «обновлений нет» вместо ошибки и попытки другим путём.
        let endpoints = parse_endpoints(&[PRERELEASE_UPDATER_ENDPOINT.to_owned()])?;
        let (found, _) = check_endpoints(app_handle, language, endpoints, proxy).await?;
        return Ok(found);
    }

    let mut last_error = None;
    for endpoint in stable_endpoints(app_handle)? {
        let url = endpoint.to_string();
        match check_endpoints(app_handle, language, vec![endpoint], proxy).await {
            Ok((found, served)) => {
                if manifest_answers_channel(served.as_deref(), receive_prereleases) {
                    return Ok(found);
                }
                logging!(
                    info,
                    Type::System,
                    "{url} serves a pre-release manifest ({}) while pre-releases are off, trying the next endpoint",
                    served.unwrap_or_default()
                );
            }
            Err(e) => {
                logging!(warn, Type::System, "update check against {url} failed: {e}");
                last_error = Some(e);
            }
        }
    }

    match last_error {
        Some(e) => Err(e),
        None => Ok(None),
    }
}

pub async fn check_update_with_fallback(app_handle: &tauri::AppHandle) -> Result<Option<Update>> {
    let verge = Config::verge().await.latest_arc();
    let language = verge.language.clone();
    let receive_prereleases = verge
        .receive_prereleases
        .unwrap_or(crate::config::IVerge::DEFAULT_RECEIVE_PRERELEASES);
    // Сначала через своё ядро: GitHub может быть недоступен напрямую. Ядро не
    // запущено — отказ соединения приходит сразу, и дело идёт напрямую.
    let port = crate::config::Config::effective_mixed_port().await;
    let core = tauri::Url::parse(&format!("http://127.0.0.1:{port}"))?;
    let via_core = check_update_on_channel(app_handle, language.as_deref(), receive_prereleases, Some(&core));
    match within_check_deadline(via_core).await {
        Ok(found) => Ok(found),
        Err(core_error) => {
            logging!(
                warn,
                Type::System,
                "update check via {core} failed ({core_error}), retrying directly"
            );
            within_check_deadline(check_update_on_channel(
                app_handle,
                language.as_deref(),
                receive_prereleases,
                None,
            ))
            .await
        }
    }
}

/// Сколько ждать ответа манифеста по одному пути (через ядро или напрямую).
const CHECK_ATTEMPT_DEADLINE: std::time::Duration = std::time::Duration::from_secs(30);

/// Сколько загрузка обновления может не получать ни байта, прежде чем её бросить.
const DOWNLOAD_STALL_LIMIT: std::time::Duration = std::time::Duration::from_secs(120);

/// У запросов плагина нет своего срока: зависшее соединение держало бы
/// проверку вечно — и с ней суточный цикл фоновой проверки, и запуск модулей
/// после вопроса об установке на старте, и кнопку проверки в настройках.
async fn within_check_deadline<F>(check: F) -> Result<Option<Update>>
where
    F: std::future::Future<Output = Result<Option<Update>>>,
{
    tokio::time::timeout(CHECK_ATTEMPT_DEADLINE, check)
        .await
        .unwrap_or_else(|_| Err(anyhow!("no answer within {}s", CHECK_ATTEMPT_DEADLINE.as_secs())))
}

/// Загрузка обновления со сторожем тишины. У загрузки плагина нет срока:
/// замершее соединение держало бы её вечно. Срок — на тишину, а не на всю
/// загрузку: медленная, но идущая не обрывается.
async fn download_watched(update: &Update, mut on_chunk: impl FnMut(usize, Option<u64>) + Send) -> Result<Vec<u8>> {
    let progress = Arc::new(tokio::sync::Notify::new());
    let download = update.download(
        {
            let progress = Arc::clone(&progress);
            move |chunk_len, content_len| {
                progress.notify_one();
                on_chunk(chunk_len, content_len);
            }
        },
        || {},
    );
    tokio::pin!(download);
    loop {
        tokio::select! {
            result = &mut download => return Ok(result?),
            () = progress.notified() => {}
            () = tokio::time::sleep(DOWNLOAD_STALL_LIMIT) => {
                return Err(anyhow!(
                    "the download of v{} received nothing for {}s",
                    update.version,
                    DOWNLOAD_STALL_LIMIT.as_secs()
                ));
            }
        }
    }
}

impl SilentUpdater {
    /// Байты этого обновления из кэша, если он хранит именно эту версию и её
    /// подпись сходится; кэш, который не сошёлся, выбрасывается.
    async fn cached_bytes_for(app_handle: &tauri::AppHandle, update: &Update) -> Option<Vec<u8>> {
        let meta = Self::read_cache_meta().ok()?;
        if meta.version != update.version {
            return None;
        }
        let pubkey = updater_pubkey(app_handle).ok()?;
        let signature = update.signature.clone();
        let verified = tokio::task::spawn_blocking(move || {
            Self::read_cache_bytes().and_then(|bytes| {
                verify_minisign(&pubkey, &signature, &bytes)?;
                Ok(bytes)
            })
        })
        .await;
        match verified {
            Ok(Ok(bytes)) => Some(bytes),
            Ok(Err(e)) => {
                logging!(
                    warn,
                    Type::System,
                    "Cached update v{} does not match the published one ({e}), downloading it again",
                    meta.version
                );
                Self::delete_cache();
                None
            }
            Err(_) => None,
        }
    }

    /// Запустить установщик. Одновременно — только один: признак снимается
    /// там же, где установщик вернулся, а не там, где его перестали ждать.
    async fn install(update: Update, bytes: Vec<u8>) -> Result<()> {
        let Some(running) = InstallerRunning::claim() else {
            return Err(anyhow!("another update is being installed right now"));
        };
        let installed = tokio::task::spawn_blocking(move || running.run(|| update.install(&bytes))).await;
        Ok(installed??)
    }

    /// Установка из окна обновления. `false` — отменили до запуска установщика;
    /// идущую установку отмена уже не останавливает.
    pub async fn install_manually(
        &self,
        app_handle: &tauri::AppHandle,
        update: Update,
        on_event: impl Fn(DownloadEvent) + Send + Sync,
    ) -> Result<bool> {
        // Установщик уже работает (например, тот, что ставят на старте) —
        // отказ сразу, а не после загрузки.
        if INSTALLING.load(Ordering::Acquire) {
            return Err(anyhow!("another update is being installed right now"));
        }
        let cancel = CancellationToken::new();
        let ticket = MANUAL_INSTALLS.fetch_add(1, Ordering::Relaxed);
        {
            let mut slot = self.manual_cancel.lock();
            if slot.is_some() {
                return Err(anyhow!("the update is already being downloaded"));
            }
            *slot = Some((ticket, cancel.clone()));
        }
        let held = ManualSlot { ticket };
        let version = update.version.clone();
        let prepared = tokio::select! {
            biased;
            () = cancel.cancelled() => None,
            bytes = Self::bytes_to_install(app_handle, &update, &on_event) => Some(bytes),
        };
        // Отмена ставится под тем же замком: после того как место освобождено,
        // состояние жетона окончательное.
        drop(held);
        let Some(bytes) = prepared.filter(|_| !cancel.is_cancelled()).transpose()? else {
            logging!(info, Type::System, "Update v{version} cancelled");
            return Ok(false);
        };

        logging!(info, Type::System, "Installing update v{version}...");
        Self::install(update, bytes).await?;
        Self::delete_cache();
        Ok(true)
    }

    /// Отменяет под замком: `install_manually`, забрав жетон под тем же
    /// замком, видит отмену либо уже случившейся, либо не видит её вовсе.
    pub fn cancel_manual_install(&self) {
        let mut slot = self.manual_cancel.lock();
        if let Some((_, cancel)) = slot.take() {
            cancel.cancel();
        }
    }

    /// Уже скачанное фоном — сразу, иначе загрузка с ходом для окна.
    async fn bytes_to_install(
        app_handle: &tauri::AppHandle,
        update: &Update,
        on_event: &(impl Fn(DownloadEvent) + Sync),
    ) -> Result<Vec<u8>> {
        if let Some(bytes) = Self::cached_bytes_for(app_handle, update).await {
            logging!(info, Type::System, "Update v{} is taken from the cache", update.version);
            on_event(DownloadEvent::Started {
                content_length: Some(bytes.len() as u64),
            });
            on_event(DownloadEvent::Progress {
                chunk_length: bytes.len(),
            });
            on_event(DownloadEvent::Finished);
            return Ok(bytes);
        }
        let mut started = false;
        let bytes = download_watched(update, |chunk_length, content_length| {
            if !std::mem::replace(&mut started, true) {
                on_event(DownloadEvent::Started { content_length });
            }
            on_event(DownloadEvent::Progress { chunk_length });
        })
        .await?;
        on_event(DownloadEvent::Finished);
        Ok(bytes)
    }

    async fn check_and_download(&self, app_handle: &tauri::AppHandle) -> Result<()> {
        let is_portable = *dirs::PORTABLE_FLAG.get().unwrap_or(&false);
        if is_portable {
            logging!(debug, Type::System, "Silent update skipped: portable build");
            return Ok(());
        }

        let auto_check = Config::verge().await.latest_arc().auto_check_update.unwrap_or(true);
        if !auto_check {
            logging!(debug, Type::System, "Silent update skipped: auto_check_update is false");
            return Ok(());
        }

        if self.is_update_ready() {
            logging!(debug, Type::System, "Silent update skipped: update already pending");
            return Ok(());
        }

        logging!(info, Type::System, "Silent updater: checking for updates...");

        let update = match check_update_with_fallback(app_handle).await {
            Ok(Some(update)) => update,
            Ok(None) => {
                logging!(info, Type::System, "Silent updater: no update available");
                return Ok(());
            }
            Err(e) => {
                logging!(warn, Type::System, "Silent updater: check failed: {e}");
                return Err(e);
            }
        };

        let version = update.version.clone();
        logging!(info, Type::System, "Silent updater: update available: v{version}");

        if is_prerelease_version(&version)
            && !Config::verge()
                .await
                .latest_arc()
                .receive_prereleases
                .unwrap_or(crate::config::IVerge::DEFAULT_RECEIVE_PRERELEASES)
        {
            logging!(
                info,
                Type::System,
                "Silent updater: v{version} is a pre-release and pre-releases are off"
            );
            return Ok(());
        }

        if let Some(body) = &update.body
            && body.to_lowercase().contains("break change")
        {
            logging!(
                info,
                Type::System,
                "Silent updater: breaking change detected in v{version}, notifying frontend"
            );
            super::handle::Handle::notice_message("update::breaking_changes", version.as_str());
            return Ok(());
        }

        // Готовность жила только в памяти, а кэш — на диске: после «Позже» на
        // старте то же обновление качалось заново при каждом запуске.
        if Self::cached_bytes_for(app_handle, &update).await.is_some() {
            self.update_ready.store(true, Ordering::Release);
            logging!(
                info,
                Type::System,
                "Silent updater: v{version} is already downloaded and verified, ready for startup install"
            );
            return Ok(());
        }

        logging!(info, Type::System, "Silent updater: downloading v{version}...");
        let bytes = download_watched(&update, |chunk_len, content_len| {
            logging!(
                debug,
                Type::System,
                "Silent updater download progress: chunk={chunk_len}, total={content_len:?}"
            );
        })
        .await?;
        logging!(info, Type::System, "Silent updater: download complete");

        if let Err(e) = Self::write_cache(&bytes, &version).await {
            logging!(warn, Type::System, "Silent updater: failed to write cache: {e}");
        }

        self.update_ready.store(true, Ordering::Release);

        logging!(
            info,
            Type::System,
            "Silent updater: v{version} ready for startup install on next launch"
        );
        Ok(())
    }

    pub async fn start_background_check(&self, app_handle: tauri::AppHandle) {
        logging!(info, Type::System, "Silent updater: background task started");

        tokio::time::sleep(std::time::Duration::from_secs(10)).await;

        loop {
            if let Err(e) = self.check_and_download(&app_handle).await {
                logging!(warn, Type::System, "Silent updater: cycle error: {e}");
            }

            tokio::time::sleep(std::time::Duration::from_secs(24 * 60 * 60)).await;
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    #[test]
    fn the_installer_flag_drops_after_a_failure_or_panic_and_stays_after_success() {
        let running = InstallerRunning::claim();
        assert!(running.is_some());
        assert!(InstallerRunning::claim().is_none(), "a second installer must not start");

        let crashed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            running.map(|running| running.run(|| -> Result<(), ()> { std::panic::resume_unwind(Box::new("crashed")) }))
        }));
        assert!(crashed.is_err());

        let failed = InstallerRunning::claim().map(|running| running.run(|| Err::<(), _>("failed")));
        assert_eq!(failed, Some(Err("failed")));

        let installed = InstallerRunning::claim().map(|running| running.run(|| Ok::<(), ()>(())));
        assert_eq!(installed, Some(Ok(())));
        assert!(
            InstallerRunning::claim().is_none(),
            "after a successful install the process is restarting — no second install"
        );
        INSTALLING.store(false, Ordering::Release);
    }

    #[test]
    fn a_finished_manual_install_frees_only_its_own_slot() {
        let updater = SilentUpdater::global();
        *updater.manual_cancel.lock() = Some((7, CancellationToken::new()));

        drop(ManualSlot { ticket: 6 });
        assert!(
            updater.manual_cancel.lock().is_some(),
            "an older install must not free a newer one"
        );

        drop(ManualSlot { ticket: 7 });
        assert!(updater.manual_cancel.lock().is_none());
    }

    #[test]
    fn download_events_keep_the_shape_the_update_dialog_reads() {
        use serde_json::json;

        let cases = [
            (
                DownloadEvent::Started {
                    content_length: Some(5),
                },
                json!({"event": "Started", "data": {"contentLength": 5}}),
            ),
            (
                DownloadEvent::Started { content_length: None },
                json!({"event": "Started", "data": {"contentLength": null}}),
            ),
            (
                DownloadEvent::Progress { chunk_length: 7 },
                json!({"event": "Progress", "data": {"chunkLength": 7}}),
            ),
            (DownloadEvent::Finished, json!({"event": "Finished"})),
        ];
        for (event, expected) in cases {
            assert_eq!(serde_json::to_value(&event).unwrap(), expected);
        }
    }

    use super::*;

    const TEST_PUBKEY: &str = "untrusted comment: minisign public key E7620F1842B4E81F\nRWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3";
    const TEST_SIGNATURE: &str = "untrusted comment: signature from minisign secret key\nRWQf6LRCGA9i59SLOFxz6NxvASXDJeRtuZykwQepbDEGt87ig1BNpWaVWuNrm73YiIiJbq71Wi+dP9eKL8OC351vwIasSSbXxwA=\ntrusted comment: timestamp:1555779966\tfile:test\nQtKMXWyYcwdpZAlPF7tE2ENJkRd1ujvKjlj1m9RtHTBnZPa5WKU5uWRs5GoP5M/VqE81QFuMKI5k/SfNQUaOAA==";

    fn wrap(block: &str) -> String {
        use base64::Engine as _;
        base64::engine::general_purpose::STANDARD.encode(block)
    }

    #[test]
    fn signature_matches_the_signed_bytes() {
        verify_minisign(&wrap(TEST_PUBKEY), &wrap(TEST_SIGNATURE), b"test")
            .expect("the official minisign vector must verify");
    }

    #[test]
    fn signature_is_rejected_when_the_bytes_differ() {
        let err = verify_minisign(&wrap(TEST_PUBKEY), &wrap(TEST_SIGNATURE), b"Test")
            .expect_err("tampered bytes must not verify");
        assert!(err.to_string().contains("does not match"), "unexpected error: {err}");
    }

    #[test]
    fn malformed_key_or_signature_is_an_error_not_a_pass() {
        assert!(verify_minisign("not base64!", &wrap(TEST_SIGNATURE), b"test").is_err());
        assert!(verify_minisign(&wrap(TEST_PUBKEY), "not base64!", b"test").is_err());
        assert!(verify_minisign(&wrap("garbage"), &wrap(TEST_SIGNATURE), b"test").is_err());
    }

    #[test]
    fn the_shipped_pubkey_is_a_usable_minisign_key() {
        use base64::Engine as _;
        let conf: serde_json::Value = serde_json::from_str(include_str!("../../tauri.conf.json")).unwrap();
        let pubkey = conf["plugins"]["updater"]["pubkey"].as_str().expect("pubkey in config");
        let decoded = base64::engine::general_purpose::STANDARD.decode(pubkey).unwrap();
        minisign_verify::PublicKey::decode(std::str::from_utf8(&decoded).unwrap())
            .expect("the shipped pubkey must decode");
    }

    #[test]
    fn test_version_equal() {
        assert!(version_lte("2.4.7", "2.4.7"));
    }

    #[test]
    fn test_version_less() {
        assert!(version_lte("2.4.7", "2.4.8"));
        assert!(version_lte("2.4.7", "2.5.0"));
        assert!(version_lte("2.4.7", "3.0.0"));
    }

    #[test]
    fn test_version_greater() {
        assert!(!version_lte("2.4.8", "2.4.7"));
        assert!(!version_lte("2.5.0", "2.4.7"));
        assert!(!version_lte("3.0.0", "2.4.7"));
    }

    #[test]
    fn test_prerelease_is_older_than_the_release_it_precedes() {
        assert!(version_lte("0.1.6-alpha", "0.1.6"));
        assert!(!version_lte("0.1.6", "0.1.6-alpha"));
        assert!(version_lte("0.1.6-alpha", "0.1.6-beta"));
        assert!(version_lte("1.0.0-alpha", "1.0.0-alpha.1"));
        assert!(version_lte("1.0.0-alpha.1", "1.0.0-alpha.beta"));
        assert!(version_lte("1.0.0-rc.1", "1.0.0"));
        assert!(!version_lte("0.1.7-alpha", "0.1.6"));
    }

    #[test]
    fn test_unparseable_versions_never_install() {
        assert!(version_lte("1.x.5", "1.2.0"));
        assert!(version_lte("", "1.2.0"));
        assert!(version_lte("1.2.0", "garbage"));
    }

    #[test]
    fn test_version_with_v_prefix() {
        assert!(version_lte("v2.4.7", "2.4.8"));
        assert!(version_lte("2.4.7", "v2.4.8"));
        assert!(version_lte("v2.4.7", "v2.4.8"));
    }

    #[test]
    fn test_prerelease_is_recognised_by_the_semver_suffix() {
        assert!(is_prerelease_version("0.0.24-alpha"));
        assert!(is_prerelease_version("v0.0.24-alpha"));
        assert!(is_prerelease_version("1.0.0-rc.1"));
        assert!(is_prerelease_version("1.0.0-beta"));
        assert!(!is_prerelease_version("1.0.0"));
        assert!(!is_prerelease_version("v1.0.0"));
        assert!(!is_prerelease_version("1.0.0-"));
    }

    #[test]
    fn stable_channel_refuses_a_prerelease_manifest_as_an_answer() {
        assert!(!manifest_answers_channel(Some("0.1.10-alpha.2"), false));
        assert!(!manifest_answers_channel(Some("v0.1.10-alpha.2"), false));
        assert!(manifest_answers_channel(Some("0.1.8"), false));
        assert!(manifest_answers_channel(None, false));
    }

    #[test]
    fn prerelease_channel_accepts_any_manifest_as_an_answer() {
        assert!(manifest_answers_channel(Some("0.1.10-alpha.2"), true));
        assert!(manifest_answers_channel(Some("0.1.8"), true));
    }

    #[test]
    fn stable_channel_walks_past_the_prerelease_endpoint_to_the_next_one() {
        let served = [
            ("releases/download/updater/latest.json", Some("0.1.10-alpha.2")),
            ("releases/latest/download/latest.json", Some("0.1.8")),
        ];
        let answered = served
            .iter()
            .find(|(_, version)| manifest_answers_channel(*version, false))
            .map(|(endpoint, _)| *endpoint);
        assert_eq!(answered, Some("releases/latest/download/latest.json"));
    }

    #[test]
    fn test_version_with_prerelease() {
        assert!(version_lte("2.4.7", "2.4.8-alpha"));
        assert!(version_lte("2.4.8-alpha", "2.4.8"));
        assert!(version_lte("2.4.8-alpha", "2.4.8-beta"));
    }

    #[test]
    fn test_version_different_lengths() {
        assert!(version_lte("2.4", "2.4.1"));
        assert!(!version_lte("2.4.1", "2.4"));
        assert!(version_lte("2.4.0", "2.4"));
    }

    #[test]
    fn test_cache_meta_serialize_roundtrip() {
        let meta = UpdateCacheMeta {
            version: "2.5.0".to_string(),
            downloaded_at: "2026-03-31T00:00:00Z".to_string(),
        };
        let json = serde_json::to_string(&meta).unwrap();
        let parsed: UpdateCacheMeta = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.version, "2.5.0");
        assert_eq!(parsed.downloaded_at, "2026-03-31T00:00:00Z");
    }

    #[test]
    fn test_cache_meta_invalid_json() {
        let result = serde_json::from_str::<UpdateCacheMeta>("not valid json");
        assert!(result.is_err());
    }

    #[test]
    fn test_cache_meta_missing_required_field() {
        let result = serde_json::from_str::<UpdateCacheMeta>(r#"{"version":"2.5.0"}"#);
        assert!(result.is_err());
    }
}
