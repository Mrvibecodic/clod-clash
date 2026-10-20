//! Обновление двух встроенных ядер — стокового Mihomo (`verge-mihomo`) и Clod Core
//! (`verge-mihomo-alpha`, имя файла историческое). Других ядер у приложения нет.
//!
//! Где папка программы доступна на запись без прав администратора (обычно macOS;
//! на Linux пакет ставит ядро в системную папку), ядро обновляет само
//! приложение: скачивает релиз источника этого ядра, сверяет sha256, кладёт новый
//! файл рядом и подменяет его переименованием при остановленном ядре; не
//! поднялось — возвращает прежний.
//! Ядро, переписывающее работающий файл поверх себя (`/upgrade`), здесь не
//! участвует: обрыв оставлял битый файл без отката, а на macOS переписанный на
//! месте исполняемый файл система может убить при запуске.
//!
//! Где папка программы только для администратора (Program Files, /usr/lib), ядро
//! обновляет себя само через службу (`/upgrade`, от её имени); итог решает то,
//! какое ядро ответит после перезапуска.

use std::{
    io::Read as _,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

use anyhow::{Context as _, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use sha2::Digest as _;

use crate::{
    config::Config,
    constants::timing,
    core::{CoreManager, core_integrity, handle},
    utils::{
        dirs,
        network::{NetworkManager, ProxyType},
        retry::try_strategies,
    },
};
use clash_verge_logging::{Type, logging};

const DOWNLOAD_TIMEOUT_SECS: u64 = 300;
const API_TIMEOUT_SECS: u64 = 30;
const REACHABILITY_TIMEOUT_SECS: u64 = 15;
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);
/// Новое ядро прижилось, если столько проработало без падений; до тех пор
/// копия прежнего лежит рядом, и падение возвращает его.
const NEW_CORE_TRIAL: Duration = Duration::from_secs(60);
const TRIAL_LOOK_EVERY: Duration = Duration::from_secs(1);

static UPDATING: AtomicBool = AtomicBool::new(false);

/// Кто может заменить файл встроенного ядра.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateMethod {
    /// Приложение: папка программы доступна на запись.
    App,
    /// Само ядро через службу (`/upgrade`, от имени службы).
    Core,
    /// Некому: обновится вместе с приложением.
    Unavailable,
}

/// Решение одно на все места: на Windows работающий файл ядра переименованием не
/// подменить, там только служба.
const fn update_method(windows: bool, service_mode: bool, core_dir_writable: bool) -> UpdateMethod {
    if !windows && core_dir_writable {
        UpdateMethod::App
    } else if service_mode {
        UpdateMethod::Core
    } else {
        UpdateMethod::Unavailable
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CoreUpdaterStatus {
    pub method: UpdateMethod,
    /// Ядро работает (или прямо сейчас поднимается): обновить силами приложения
    /// можно только его.
    pub core_running: bool,
    pub running: Option<String>,
    pub updating: bool,
}

/// Итог обновления силами приложения.
#[derive(Debug, Clone, Serialize)]
pub struct BundledCoreUpdate {
    pub updated: bool,
    pub version: String,
}

/// Какое из двух встроенных ядер.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CoreSlot {
    /// `verge-mihomo` — стоковое Mihomo от MetaCubeX.
    Stock,
    /// `verge-mihomo-alpha` — Clod Core, наш форк с патчами.
    Clod,
}

impl CoreSlot {
    fn of(clash_core: &str) -> Self {
        if clash_core == "verge-mihomo-alpha" {
            Self::Clod
        } else {
            Self::Stock
        }
    }

    const fn release_api(self) -> &'static str {
        match self {
            Self::Stock => "https://api.github.com/repos/MetaCubeX/mihomo/releases/latest",
            Self::Clod => "https://api.github.com/repos/Mrvibecodic/clod-core/releases/latest",
        }
    }

    /// Имя сборки без версии — тот же вариант, что кладёт в установщик
    /// `scripts/prebuild.mjs` (`META_MAP` / `CLOD_MAP`): обновление не меняет
    /// сборку, а только версию.
    fn asset_base(self, os: &str, arch: &str) -> Option<&'static str> {
        let name = match (self, os, arch) {
            (Self::Clod, "windows", "x86_64") => "mihomo-windows-amd64",
            (Self::Clod, "windows", "aarch64") => "mihomo-windows-arm64",
            (Self::Clod, "macos", "x86_64") => "mihomo-darwin-amd64",
            (Self::Clod, "macos", "aarch64") => "mihomo-darwin-arm64",
            (Self::Clod, "linux", "x86_64") => "mihomo-linux-amd64",
            (Self::Clod, "linux", "aarch64") => "mihomo-linux-arm64",
            (Self::Stock, "windows", "x86_64") => "mihomo-windows-amd64-v2",
            (Self::Stock, "windows", "x86") => "mihomo-windows-386",
            (Self::Stock, "windows", "aarch64") => "mihomo-windows-arm64",
            (Self::Stock, "macos", "x86_64") => "mihomo-darwin-amd64-v2-go122",
            (Self::Stock, "macos", "aarch64") => "mihomo-darwin-arm64-go122",
            (Self::Stock, "linux", "x86_64") => "mihomo-linux-amd64-v2",
            (Self::Stock, "linux", "x86") => "mihomo-linux-386",
            (Self::Stock, "linux", "aarch64") => "mihomo-linux-arm64",
            (Self::Stock, "linux", "arm") => "mihomo-linux-armv7",
            (Self::Stock, "linux", "riscv64") => "mihomo-linux-riscv64",
            (Self::Stock, "linux", "loongarch64") => "mihomo-linux-loong64",
            _ => return None,
        };
        Some(name)
    }
}

#[derive(Debug, Deserialize)]
struct GhRelease {
    assets: Vec<GhAsset>,
}

#[derive(Debug, Clone, Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
    size: u64,
    #[serde(default)]
    digest: Option<String>,
}

/// Версия в имени сборки: `v1.19.31`, `v1.19.31-clod.8`. Отсекает соседние
/// варианты с тем же началом имени (`…-go120-v1.19.31`, `…-v3-v1.19.31`).
fn is_plain_version(version: &str) -> bool {
    version.strip_prefix('v').is_some_and(|rest| {
        let digits = rest.chars().take_while(char::is_ascii_digit).count();
        digits > 0 && rest[digits..].starts_with('.')
    })
}

/// Одна и та же ли версия: ядро может сообщать её без ведущей `v`.
fn same_version(running: &str, released: &str) -> bool {
    let bare = |version: &str| version.trim().trim_start_matches('v').to_owned();
    bare(running) == bare(released)
}

/// Сборка этого варианта и её версия.
fn pick_asset(assets: &[GhAsset], base: &str) -> Result<(GhAsset, String)> {
    let prefix = format!("{base}-");
    assets
        .iter()
        .find_map(|asset| {
            let rest = asset.name.strip_prefix(&prefix)?;
            let version = rest.strip_suffix(".gz").or_else(|| rest.strip_suffix(".zip"))?;
            is_plain_version(version).then(|| (asset.clone(), version.to_owned()))
        })
        .ok_or_else(|| anyhow!("{}", clash_verge_i18n::t!("core.noBuildInRelease", base = base)))
}

async fn http_client(proxy: ProxyType, timeout: u64) -> Result<reqwest::Client> {
    NetworkManager::new()
        .create_request(
            proxy,
            Some(timeout),
            Some(format!("clod-clash/{}", env!("CARGO_PKG_VERSION")).into()),
        )
        .await
}

async fn fetch_release(url: &str) -> Result<GhRelease> {
    try_strategies(ProxyType::None, [ProxyType::Localhost], |proxy| async move {
        let attempt = async {
            let client = http_client(proxy, API_TIMEOUT_SECS).await?;
            let response = client.get(url).send().await?;
            if !response.status().is_success() {
                bail!("GitHub API returned {}", response.status());
            }
            Ok::<GhRelease, anyhow::Error>(response.json::<GhRelease>().await?)
        }
        .await;
        if let Err(err) = &attempt {
            logging!(warn, Type::Core, "core release fetch via {proxy:?} failed: {err:#}");
        }
        attempt
    })
    .await
    .context("failed to reach the core release channel")
}

/// Ядро заменило свой файл само (`/upgrade` через службу): отпечаток встроенного
/// ядра снимается заново с того, что оно себе скачало.
async fn repin_core_binaries() {
    if let Ok(path) = crate::core::service::bundled_core_path().await {
        core_integrity::repin_binary(&path).await;
    }
}

/// Какое ядро запущено сейчас. Ставит старт ядра, читает отчёт для поддержки: так
/// подпись в отчёте — то, что реально запущено, а не то, что выбрано сейчас.
#[derive(Clone)]
pub struct StartedCore {
    pub core: String,
}

static STARTED_CORE: parking_lot::Mutex<Option<StartedCore>> = parking_lot::const_mutex(None);

pub fn note_started_core(core: String) {
    *STARTED_CORE.lock() = Some(StartedCore { core });
}

pub fn started_core() -> Option<StartedCore> {
    STARTED_CORE.lock().clone()
}

pub async fn running_core_version() -> Option<String> {
    let version = handle::Handle::mihomo().get_version().await.ok()?;
    Some(version.version)
}

async fn current_update_method() -> UpdateMethod {
    let service_mode = matches!(
        *CoreManager::global().get_running_mode(),
        crate::core::manager::RunningMode::Service
    );
    let core_dir_writable = replaceable_core_file().await.is_some();
    update_method(cfg!(windows), service_mode, core_dir_writable)
}

pub async fn status() -> CoreUpdaterStatus {
    CoreUpdaterStatus {
        method: current_update_method().await,
        core_running: !CoreManager::global().is_down(),
        running: running_core_version().await,
        updating: is_updating(),
    }
}

async fn route_is_alive(proxy: ProxyType, url: &str) -> bool {
    let attempt = async {
        let client = http_client(proxy, REACHABILITY_TIMEOUT_SECS).await?;
        let response = client.head(url).send().await?;
        Ok::<bool, anyhow::Error>(response.status().is_success() || response.status().is_redirection())
    };
    attempt.await.unwrap_or(false)
}

async fn download_asset(asset: &GhAsset) -> Result<Vec<u8>> {
    try_strategies(ProxyType::None, [ProxyType::Localhost], |proxy| async move {
        if matches!(proxy, ProxyType::None) && !route_is_alive(proxy, &asset.browser_download_url).await {
            logging!(
                warn,
                Type::Core,
                "direct route to GitHub looks dead, trying the core tunnel"
            );
            bail!("direct connection to GitHub timed out");
        }
        let attempt = async {
            let client = http_client(proxy, DOWNLOAD_TIMEOUT_SECS).await?;
            let mut response = client.get(&asset.browser_download_url).send().await?;
            if !response.status().is_success() {
                bail!("asset download returned {}", response.status());
            }
            let total = response.content_length().unwrap_or(asset.size);
            let mut bytes: Vec<u8> = Vec::with_capacity(total.min(64 * 1024 * 1024) as usize);
            while let Some(chunk) = response.chunk().await? {
                bytes.extend_from_slice(&chunk);
            }
            Ok::<Vec<u8>, anyhow::Error>(bytes)
        }
        .await;
        if let Err(err) = &attempt {
            logging!(warn, Type::Core, "core download via {proxy:?} failed: {err:#}");
        }
        attempt
    })
    .await
    .context("failed to download the core archive")
}

fn api_digest(asset: &GhAsset) -> Option<String> {
    let hex = asset.digest.as_deref()?.strip_prefix("sha256:")?;
    if hex.len() == 64 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
        Some(hex.to_ascii_lowercase())
    } else {
        None
    }
}

async fn verify_sha256(release: &GhRelease, asset: &GhAsset, bytes: &[u8]) -> Result<()> {
    let actual = format!("{:x}", sha2::Sha256::digest(bytes));

    if let Some(expected) = api_digest(asset) {
        if actual != expected {
            bail!("sha256 mismatch: expected {expected}, got {actual}");
        }
        logging!(
            info,
            Type::Core,
            "core archive matches the digest published by the release API"
        );
        return Ok(());
    }

    let checksum_name = format!("{}.sha256", asset.name);
    let Some(checksum_asset) = release.assets.iter().find(|a| a.name == checksum_name) else {
        bail!("release publishes neither a digest nor {checksum_name}, refusing to install an unverified core");
    };

    let text = try_strategies(ProxyType::None, [ProxyType::Localhost], |proxy| async move {
        let client = http_client(proxy, API_TIMEOUT_SECS).await?;
        let response = client.get(&checksum_asset.browser_download_url).send().await?;
        if !response.status().is_success() {
            bail!("checksum download returned {}", response.status());
        }
        Ok::<String, anyhow::Error>(response.text().await?)
    })
    .await?;

    let expected = text
        .split_whitespace()
        .find(|token| token.len() == 64 && token.chars().all(|c| c.is_ascii_hexdigit()))
        .ok_or_else(|| anyhow!("no sha256 digest inside {checksum_name}"))?
        .to_ascii_lowercase();

    if actual != expected {
        bail!("sha256 mismatch: expected {expected}, got {actual}");
    }
    Ok(())
}

fn unpack_binary(asset_name: &str, bytes: &[u8]) -> Result<Vec<u8>> {
    if asset_name.ends_with(".zip") {
        let reader = std::io::Cursor::new(bytes);
        let mut archive = zip::ZipArchive::new(reader).context("broken zip archive")?;
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index)?;
            if entry.is_file() {
                let mut data = Vec::with_capacity(entry.size() as usize);
                entry.read_to_end(&mut data)?;
                return Ok(data);
            }
        }
        bail!("zip archive holds no file");
    }

    let mut decoder = flate2::read::GzDecoder::new(bytes);
    let mut data = Vec::new();
    decoder.read_to_end(&mut data).context("broken gzip archive")?;
    Ok(data)
}

async fn probe_binary(binary: &Path) -> Result<String> {
    let mut command = tokio::process::Command::new(binary);
    command.arg("-v");
    #[cfg(target_os = "windows")]
    {
        command.creation_flags(0x08000000);
    }
    let output = tokio::time::timeout(PROBE_TIMEOUT, command.output())
        .await
        .context("core -v probe timed out")??;
    if !output.status.success() {
        bail!("core -v probe exited with {}", output.status);
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

struct UpdateGuard;

impl UpdateGuard {
    fn acquire() -> Result<Self> {
        if UPDATING.swap(true, Ordering::AcqRel) {
            bail!("{}", clash_verge_i18n::t!("core.updateAlreadyRunning"));
        }
        Ok(Self)
    }
}

impl Drop for UpdateGuard {
    fn drop(&mut self) {
        UPDATING.store(false, Ordering::Release);
    }
}

/// Файл рядом с ядром: `verge-mihomo.new`, `verge-mihomo.old`.
fn beside(target: &Path, suffix: &str) -> Result<PathBuf> {
    let name = target.file_name().ok_or_else(|| {
        anyhow!(
            "{}",
            clash_verge_i18n::t!("core.noFileName", path = target.display().to_string())
        )
    })?;
    let mut name = name.to_os_string();
    name.push(".");
    name.push(suffix);
    Ok(target.with_file_name(name))
}

async fn write_executable(path: &Path, data: &[u8]) -> Result<()> {
    tokio::fs::write(path, data)
        .await
        .with_context(|| clash_verge_i18n::t!("core.writeFailed", path = path.display().to_string()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).await?;
    }

    #[cfg(target_os = "macos")]
    {
        let _ = tokio::process::Command::new("xattr").arg("-c").arg(path).output().await;
    }
    Ok(())
}

/// Файл встроенного ядра, который приложение может заменить само: папка
/// доступна на запись, и это тот самый файл, который запускает и приложение, и
/// служба. У копии, запущенной macOS из временного места (транслокация), путь
/// службы ведёт в другую установку — чужую заменять нельзя.
async fn replaceable_core_file() -> Option<PathBuf> {
    let path = crate::core::service::bundled_core_path().await.ok()?;
    let started_from = std::env::current_exe().ok()?.with_file_name(path.file_name()?);
    (path == started_from && core_integrity::binary_dir_is_writable(&path)).then_some(path)
}

/// Выбранное ядро, если сейчас оно не меняется: пока идёт смена, выбор в
/// черновике расходится с принятым, и файл, и сборка были бы не того ядра.
async fn the_settled_choice() -> Result<String> {
    let verge = Config::verge().await;
    let accepted = verge.data_arc().get_valid_clash_core();
    if verge.latest_arc().get_valid_clash_core() != accepted {
        bail!("{}", clash_verge_i18n::t!("core.switchInProgress"));
    }
    Ok(accepted.to_string())
}

/// Идёт ли обновление ядра прямо сейчас.
pub fn is_updating() -> bool {
    UPDATING.load(Ordering::Acquire)
}

/// Копия прежнего ядра лишняя, если совпадает с действующим файлом (остаток
/// оборванного обновления: выход между копией и подменой) или если на месте
/// стоит ядро, чья проверка после обновления не успела закончиться (выход,
/// новое обновление) — его приняли.
fn the_leftover_backup_is_spare(current: Option<&str>, copy: Option<&str>, on_trial: Option<&str>) -> bool {
    current.is_some() && (current == copy || current == on_trial)
}

/// Копия прежнего ядра рядом с ним: лишняя — убираем. Иначе прошлый возврат
/// не удался, и она единственная: новое обновление затёрло бы её, поэтому отказ.
async fn keep_or_drop_a_leftover_backup(target: &Path, backup: &Path) -> Result<()> {
    let trial = beside(target, "trial")?;
    if !tokio::fs::try_exists(backup).await.unwrap_or(false) {
        let _ = tokio::fs::remove_file(&trial).await;
        return Ok(());
    }
    let current = core_integrity::digest_of(target).await.ok();
    let copy = core_integrity::digest_of(backup).await.ok();
    let on_trial = tokio::fs::read_to_string(&trial).await.ok();
    if the_leftover_backup_is_spare(current.as_deref(), copy.as_deref(), on_trial.as_deref()) {
        tokio::fs::remove_file(backup)
            .await
            .with_context(|| clash_verge_i18n::t!("core.spareCopyNotRemoved", path = backup.display().to_string()))?;
        let _ = tokio::fs::remove_file(&trial).await;
        return Ok(());
    }
    bail!(
        "{}",
        clash_verge_i18n::t!("core.foreignBackup", path = backup.display().to_string())
    );
}

/// Обновить выбранное встроенное ядро силами приложения.
pub async fn update_bundled_core() -> Result<BundledCoreUpdate> {
    let _guard = UpdateGuard::acquire()?;

    // Остановленное ядро не поднимаем обновлением: его остановил человек или
    // конфиг, и проверить, поднимется ли новое, было бы не на чем.
    if CoreManager::global().is_down() {
        bail!("{}", clash_verge_i18n::t!("core.notRunningForUpdate"));
    }
    let chosen = the_settled_choice().await?;
    let target = replaceable_core_file()
        .await
        .ok_or_else(|| anyhow!("{}", clash_verge_i18n::t!("core.fileNotReplaceable")))?;
    let staged = beside(&target, "new")?;
    let backup = beside(&target, "old")?;
    keep_or_drop_a_leftover_backup(&target, &backup).await?;
    let slot = CoreSlot::of(&chosen);
    let base = slot
        .asset_base(std::env::consts::OS, std::env::consts::ARCH)
        .ok_or_else(|| anyhow!("{}", clash_verge_i18n::t!("core.noBuildForPlatform")))?;

    let release = fetch_release(slot.release_api()).await?;
    let (asset, version) = pick_asset(&release.assets, base)?;
    if running_core_version()
        .await
        .is_some_and(|running| same_version(&running, &version))
    {
        return Ok(BundledCoreUpdate {
            updated: false,
            version,
        });
    }

    logging!(
        info,
        Type::Core,
        "обновляю встроенное ядро до {version} ({})",
        asset.name
    );
    let archive = download_asset(&asset).await?;
    verify_sha256(&release, &asset, &archive).await?;
    let binary = unpack_binary(&asset.name, &archive)?;

    // Пока качали, ядро могли остановить (остановленное не поднимаем) или
    // сменить (файл был бы уже не выбранного ядра).
    if CoreManager::global().is_down() {
        bail!("{}", clash_verge_i18n::t!("core.stoppedDuringUpdate"));
    }
    if the_settled_choice().await? != chosen {
        bail!("{}", clash_verge_i18n::t!("core.switchedDuringUpdate"));
    }
    let old_digest = core_integrity::digest_of(&target).await.ok();
    let new_digest = core_integrity::digest_of_bytes(&binary);
    write_executable(&staged, &binary).await?;
    let probed = probe_binary(&staged).await;
    let swapped = match probed {
        Ok(probe) => {
            logging!(info, Type::Core, "новое ядро отвечает: {probe}");
            swap_in(&target, &staged, &backup, &new_digest, old_digest.as_deref()).await
        }
        Err(err) => Err(err.context(clash_verge_i18n::t!("core.newCoreDoesNotRun"))),
    };
    let _ = tokio::fs::remove_file(&staged).await;
    match (&swapped, old_digest) {
        // Ответ версии ещё не значит, что ядро работает: копия прежнего
        // остаётся, пока новое не проработает без падений.
        (Ok(()), Some(old_digest)) => {
            put_the_new_core_on_trial(chosen, target, backup, new_digest, old_digest).await;
        }
        (_, old_digest) => {
            // Копия прежнего ядра не нужна, только когда на месте стоит либо
            // новое рабочее, либо уже возвращённое прежнее. Иначе (возврат не
            // удался) она — единственное, из чего прежнее ядро можно поставить
            // руками.
            let backup_is_redundant =
                swapped.is_ok() || core_integrity::digest_of(&target).await.ok().as_deref() == old_digest.as_deref();
            if backup_is_redundant {
                let _ = tokio::fs::remove_file(&backup).await;
            }
        }
    }
    swapped?;

    Ok(BundledCoreUpdate { updated: true, version })
}

/// Проверка нового ядра после обновления: падение в первые [`NEW_CORE_TRIAL`]
/// возвращает прежнее с сообщением, иначе копия прежнего убирается. Отметка
/// проверки лежит рядом с ядром: оборванную выходом проверку закончит
/// следующее обновление (`keep_or_drop_a_leftover_backup`).
async fn put_the_new_core_on_trial(
    chosen: String,
    target: PathBuf,
    backup: PathBuf,
    new_digest: String,
    old_digest: String,
) {
    let Ok(trial) = beside(&target, "trial") else {
        let _ = tokio::fs::remove_file(&backup).await;
        return;
    };
    if tokio::fs::write(&trial, new_digest.as_bytes()).await.is_err() {
        let _ = tokio::fs::remove_file(&backup).await;
        return;
    }
    let since = tokio::time::Instant::now();
    crate::process::AsyncHandler::spawn(move || async move {
        loop {
            if handle::Handle::global().is_exiting() || !tokio::fs::try_exists(&trial).await.unwrap_or(false) {
                return;
            }
            if CoreManager::global().fell_since(since.into_std()) {
                give_the_old_core_back(&chosen, &target, &backup, &trial, &new_digest, &old_digest).await;
                return;
            }
            if since.elapsed() >= NEW_CORE_TRIAL {
                break;
            }
            tokio::time::sleep(TRIAL_LOOK_EVERY).await;
        }
        let _ = tokio::fs::remove_file(&backup).await;
        let _ = tokio::fs::remove_file(&trial).await;
        logging!(
            info,
            Type::Core,
            "новое ядро проработало без падений — копия прежнего убрана"
        );
    });
}

async fn give_the_old_core_back(
    chosen: &str,
    target: &Path,
    backup: &Path,
    trial: &Path,
    new_digest: &str,
    old_digest: &str,
) {
    // Ядро сменили или обновляют снова — проверка этого файла кончилась.
    let Ok(_guard) = UpdateGuard::acquire() else {
        return;
    };
    let still_on_trial = the_settled_choice().await.is_ok_and(|now| now == chosen)
        && core_integrity::digest_of(target)
            .await
            .is_ok_and(|now| now == new_digest);
    if !still_on_trial {
        let _ = tokio::fs::remove_file(backup).await;
        let _ = tokio::fs::remove_file(trial).await;
        return;
    }
    logging!(
        warn,
        Type::Core,
        "новое ядро упало вскоре после обновления — возвращаю прежнее"
    );
    let back = CoreManager::global()
        .restart_core_swapped(
            async || {
                tokio::fs::rename(backup, target)
                    .await
                    .context(clash_verge_i18n::t!("core.previousNotRestored"))?;
                core_integrity::pin_known_binary(target, old_digest).await;
                Ok(())
            },
            async || Ok(()),
        )
        .await;
    let _ = tokio::fs::remove_file(trial).await;
    match back {
        Ok(()) => {
            handle::Handle::refresh_clash();
            handle::Handle::refresh_verge();
            crate::utils::notification::notify_event(
                crate::utils::notification::NotificationEvent::CoreUpdateRolledBack,
            )
            .await;
        }
        Err(err) => logging!(error, Type::Core, "прежнее ядро вернуть не удалось: {err:#}"),
    }
}

/// Итог обновления ядром самого себя — по тому, чем ядро ответило после.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum SelfUpgrade {
    Updated {
        version: String,
    },
    AlreadyLatest,
    /// Ядро снова отвечает, но прежней версией: обновление не применилось.
    StillOld {
        version: String,
    },
    /// За отведённое время ядро не ответило вовсе.
    Silent {
        waited_secs: u64,
    },
}

/// Спрашивать версию, пока ядро не ответит иной, чем `before`. Не дождались —
/// ответ последнего вопроса: прежняя версия или молчание.
async fn wait_for_the_new_core<F, Fut>(before: &str, budget: Duration, step: Duration, mut ask: F) -> Option<String>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Option<String>>,
{
    let deadline = tokio::time::Instant::now() + budget;
    loop {
        let seen = ask().await;
        if seen.as_deref().is_some_and(|version| !same_version(version, before))
            || tokio::time::Instant::now() >= deadline
        {
            return seen;
        }
        tokio::time::sleep(step).await;
    }
}

/// Попросить ядро обновить себя (`/upgrade` через службу). Обновилось ли оно,
/// видно не по ответу на запрос, а по версии ядра, которое ответит после.
pub async fn upgrade_through_core() -> Result<SelfUpgrade> {
    let _guard = UpdateGuard::acquire()?;
    if current_update_method().await != UpdateMethod::Core {
        bail!("{}", clash_verge_i18n::t!("core.noSelfUpdate"));
    }
    let before = running_core_version()
        .await
        .ok_or_else(|| anyhow!("{}", clash_verge_i18n::t!("core.notAnswering")))?;
    // Обновлённое ядро служба поднимет в новый файл журнала.
    if matches!(
        *CoreManager::global().get_running_mode(),
        crate::core::manager::RunningMode::Service
    ) {
        crate::module::core_log_archive::before_new_run().await;
    }
    let request = handle::Handle::mihomo()
        .load_ctx()
        .build_request(reqwest::Method::POST, "/upgrade")
        .map_err(|error| anyhow!("{error}"))?
        .query(&[("channel", "auto"), ("force", "false")])
        .timeout(timing::CORE_SELF_UPGRADE);
    match request.send().await {
        Ok(response) if response.status().is_success() => {}
        Ok(response) => {
            let message = crate::feat::core_error_message(response).await;
            if message.to_lowercase().contains("already using latest version") {
                return Ok(SelfUpgrade::AlreadyLatest);
            }
            bail!(message);
        }
        // Обрыв запроса ядро не останавливает: оно может докачать и перезапуститься.
        Err(err) => logging!(
            warn,
            Type::Core,
            "no answer to /upgrade ({err}); waiting for the core to report its version"
        ),
    }
    // До выхода ядра: сторож может заметить перезапуск раньше, чем мы дождёмся.
    #[cfg(windows)]
    CoreManager::global().a_restart_the_user_knows_of();
    let seen = wait_for_the_new_core(
        &before,
        timing::CORE_BACK_AFTER_SELF_UPGRADE,
        timing::CORE_READY_PROBE_TIMEOUT,
        || async {
            tokio::time::timeout(timing::CORE_READY_PROBE_TIMEOUT, running_core_version())
                .await
                .ok()
                .flatten()
        },
    )
    .await;
    // Ядро подменяет файл до ответа на запрос, а служба в следующий раз
    // поднимет то, что лежит на диске, — чем бы ядро ни ответило и успело ли
    // оно ответить. Отпечаток снимается с этого файла: со старым отпечатком в
    // папке, доступной на запись, служба ядро больше не запустит.
    repin_core_binaries().await;
    Ok(match seen {
        Some(version) if !same_version(&version, &before) => SelfUpgrade::Updated { version },
        Some(version) => SelfUpgrade::StillOld { version },
        None => SelfUpgrade::Silent {
            waited_secs: timing::CORE_BACK_AFTER_SELF_UPGRADE.as_secs(),
        },
    })
}

/// Остановить ядро, поставить новый файл на место прежнего переименованием и
/// поднять ядро снова. Не поднялось — вернуть прежний файл и поднять его.
/// Отпечаток ядра меняется вместе с файлом: служба не стартует ядро, чей файл
/// разошёлся с записанным отпечатком.
async fn swap_in(
    target: &Path,
    staged: &Path,
    backup: &Path,
    new_digest: &str,
    old_digest: Option<&str>,
) -> Result<()> {
    CoreManager::global()
        .restart_core_swapped(
            async || {
                tokio::fs::copy(target, backup)
                    .await
                    .context(clash_verge_i18n::t!("core.previousNotSaved"))?;
                tokio::fs::rename(staged, target)
                    .await
                    .context(clash_verge_i18n::t!("core.newNotPlaced"))?;
                core_integrity::pin_known_binary(target, new_digest).await;
                Ok(())
            },
            async || {
                tokio::fs::rename(backup, target)
                    .await
                    .context(clash_verge_i18n::t!("core.previousNotRestored"))?;
                if let Some(digest) = old_digest {
                    core_integrity::pin_known_binary(target, digest).await;
                }
                Ok(())
            },
        )
        .await
}

/// Каталог прежнего «управляемого ядра» — отдельной скачанной копии стокового
/// Mihomo, которая подменяла выбранное ядро. Его больше нет; копии не нужны.
pub fn remove_leftover_managed_cores() {
    crate::process::AsyncHandler::spawn(|| async {
        let Ok(dir) = dirs::app_home_dir().map(|home| home.join("cores")) else {
            return;
        };
        if tokio::fs::try_exists(&dir).await.unwrap_or(false)
            && let Err(err) = tokio::fs::remove_dir_all(&dir).await
        {
            logging!(warn, Type::Core, "не удалось убрать прежние копии ядра: {err}");
        }
    });
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn a_leftover_copy_of_the_old_core_is_spare_only_beside_a_settled_or_trial_core() {
        assert!(the_leftover_backup_is_spare(Some("a"), Some("a"), None));
        assert!(the_leftover_backup_is_spare(Some("new"), Some("old"), Some("new")));
        assert!(!the_leftover_backup_is_spare(Some("new"), Some("old"), None));
        assert!(!the_leftover_backup_is_spare(Some("other"), Some("old"), Some("new")));
        assert!(!the_leftover_backup_is_spare(None, None, None));
    }

    fn asset(name: &str) -> GhAsset {
        GhAsset {
            name: name.into(),
            browser_download_url: format!("https://example.com/{name}"),
            size: 1,
            digest: None,
        }
    }

    fn asset_with_digest(name: &str, digest: &str) -> GhAsset {
        GhAsset {
            digest: Some(digest.into()),
            ..asset(name)
        }
    }

    #[test]
    fn the_app_updates_the_core_wherever_it_may_write_the_file() {
        use UpdateMethod::{App, Core, Unavailable};
        for (windows, service, writable, expected) in [
            (false, false, true, App),
            (false, true, true, App),
            (false, true, false, Core),
            (false, false, false, Unavailable),
            (true, true, true, Core),
            (true, true, false, Core),
            (true, false, true, Unavailable),
            (true, false, false, Unavailable),
        ] {
            assert_eq!(
                update_method(windows, service, writable),
                expected,
                "windows={windows} service={service} writable={writable}"
            );
        }
    }

    #[test]
    fn each_core_updates_from_its_own_source_to_the_same_build_variant() {
        assert!(
            CoreSlot::of("verge-mihomo-alpha")
                .release_api()
                .contains("Mrvibecodic/clod-core")
        );
        assert!(CoreSlot::of("verge-mihomo").release_api().contains("MetaCubeX/mihomo"));
        assert!(CoreSlot::of("anything-else").release_api().contains("MetaCubeX/mihomo"));
        assert_eq!(
            CoreSlot::Stock.asset_base("macos", "aarch64"),
            Some("mihomo-darwin-arm64-go122")
        );
        assert_eq!(
            CoreSlot::Clod.asset_base("macos", "aarch64"),
            Some("mihomo-darwin-arm64")
        );
        assert_eq!(
            CoreSlot::Stock.asset_base("linux", "x86_64"),
            Some("mihomo-linux-amd64-v2")
        );
        assert_eq!(CoreSlot::Clod.asset_base("linux", "x86_64"), Some("mihomo-linux-amd64"));
        assert_eq!(CoreSlot::Clod.asset_base("linux", "riscv64"), None);
    }

    #[test]
    fn the_exact_build_variant_is_picked_among_its_neighbours() {
        let assets = vec![
            asset("mihomo-darwin-arm64-v1.19.31.gz"),
            asset("mihomo-darwin-arm64-go120-v1.19.31.gz"),
            asset("mihomo-darwin-arm64-go122-v1.19.31.gz"),
            asset("mihomo-darwin-arm64-go122-v1.19.31.gz.sha256"),
        ];
        let (picked, version) = pick_asset(&assets, "mihomo-darwin-arm64-go122").expect("go122");
        assert_eq!(picked.name, "mihomo-darwin-arm64-go122-v1.19.31.gz");
        assert_eq!(version, "v1.19.31");

        let (picked, _) = pick_asset(&assets, "mihomo-darwin-arm64").expect("plain");
        assert_eq!(picked.name, "mihomo-darwin-arm64-v1.19.31.gz");

        let assets = vec![
            asset("mihomo-linux-amd64-v3-v1.19.31.gz"),
            asset("mihomo-linux-amd64-v2-v1.19.31.gz"),
        ];
        let (picked, _) = pick_asset(&assets, "mihomo-linux-amd64-v2").expect("v2");
        assert_eq!(picked.name, "mihomo-linux-amd64-v2-v1.19.31.gz");
        assert!(pick_asset(&assets, "mihomo-linux-arm64").is_err());
    }

    #[test]
    fn the_clod_core_version_comes_from_the_asset_name() {
        let assets = vec![
            asset("version.txt"),
            asset("mihomo-darwin-arm64-v1.19.31-clod.8.gz"),
            asset("mihomo-darwin-arm64-v1.19.31-clod.8.gz.sha256"),
            asset("mihomo-windows-amd64-v1.19.31-clod.8.zip"),
        ];
        let (_, version) = pick_asset(&assets, "mihomo-darwin-arm64").expect("clod darwin");
        assert_eq!(version, "v1.19.31-clod.8");
        let (picked, _) = pick_asset(&assets, "mihomo-windows-amd64").expect("clod windows");
        assert_eq!(picked.name, "mihomo-windows-amd64-v1.19.31-clod.8.zip");
    }

    #[test]
    fn a_version_reported_without_its_v_is_still_the_same() {
        assert!(same_version("v1.19.31", "v1.19.31"));
        assert!(same_version("1.19.31-clod.8", "v1.19.31-clod.8"));
        assert!(same_version(" v1.19.31\n", "v1.19.31"));
        assert!(!same_version("v1.19.31", "v1.19.31-clod.8"));
        assert!(!same_version("v1.19.30", "v1.19.31"));
    }

    /// Ответы ядра по очереди; кончились — повторяется последний.
    async fn answers(before: &str, script: &[Option<&str>]) -> Option<String> {
        let script: Vec<Option<String>> = script.iter().map(|seen| seen.map(str::to_owned)).collect();
        let mut asked = 0;
        wait_for_the_new_core(
            before,
            Duration::from_millis(200),
            Duration::from_millis(5),
            move || {
                let seen = script[asked.min(script.len() - 1)].clone();
                asked += 1;
                async move { seen }
            },
        )
        .await
    }

    #[tokio::test]
    async fn the_update_is_confirmed_by_the_new_version_not_by_the_reply() {
        let seen = answers("v1.19.31", &[None, Some("v1.19.31"), None, Some("v1.19.32")]).await;
        assert_eq!(seen.as_deref(), Some("v1.19.32"));
    }

    #[tokio::test]
    async fn the_old_core_answering_is_not_an_update() {
        assert_eq!(
            answers("v1.19.31", &[Some("1.19.31")]).await.as_deref(),
            Some("1.19.31")
        );
    }

    #[tokio::test]
    async fn silence_is_reported_as_silence() {
        assert_eq!(answers("v1.19.31", &[None]).await, None);
        // Прежнее ядро ответило до выхода, новое так и не поднялось.
        assert_eq!(answers("v1.19.31", &[Some("v1.19.31"), None]).await, None);
    }

    #[test]
    fn the_request_budget_covers_the_core_download() {
        // update_core.go: до 5 с на version.txt и до 90 с на загрузку.
        assert!(timing::CORE_SELF_UPGRADE > Duration::from_secs(5 + 90));
    }

    #[test]
    fn the_new_and_old_files_live_next_to_the_core() {
        let target = Path::new("/Applications/Clod Clash.app/Contents/MacOS/verge-mihomo-alpha");
        assert_eq!(
            beside(target, "new").expect("name"),
            Path::new("/Applications/Clod Clash.app/Contents/MacOS/verge-mihomo-alpha.new")
        );
        assert_eq!(
            beside(target, "old").expect("name"),
            Path::new("/Applications/Clod Clash.app/Contents/MacOS/verge-mihomo-alpha.old")
        );
    }

    #[test]
    fn api_digest_is_read_from_the_release_response() {
        let hex = "6B55C5C3C2F12EC2D020C64548D3E313A39ACEACC4E2471B33041FE7CB9E2F10";
        let picked = asset_with_digest("mihomo-linux-amd64-v1.19.2.gz", &format!("sha256:{hex}"));
        assert_eq!(api_digest(&picked), Some(hex.to_ascii_lowercase()));
    }

    #[test]
    fn api_digest_ignores_shapes_it_does_not_understand() {
        let name = "mihomo-linux-amd64-v1.19.2.gz";
        assert_eq!(api_digest(&asset(name)), None);
        assert_eq!(api_digest(&asset_with_digest(name, "sha512:abcdef")), None);
        assert_eq!(api_digest(&asset_with_digest(name, "sha256:abcdef")), None);
        let not_hex = "z".repeat(64);
        assert_eq!(api_digest(&asset_with_digest(name, &format!("sha256:{not_hex}"))), None);
    }

    #[test]
    fn assets_without_a_digest_field_still_deserialize() {
        let parsed: GhAsset = serde_json::from_str(
            r#"{"name":"mihomo-linux-amd64-v1.19.2.gz","browser_download_url":"https://example.com/a","size":7}"#,
        )
        .expect("an asset without a digest must still parse");
        assert_eq!(parsed.digest, None);
    }

    #[test]
    fn unpacks_gzip() {
        use flate2::{Compression, write::GzEncoder};
        use std::io::Write as _;
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(b"fake-binary").expect("gz write");
        let packed = encoder.finish().expect("gz finish");
        let unpacked = unpack_binary("mihomo-linux-amd64-v1.gz", &packed).expect("gz unpack");
        assert_eq!(unpacked, b"fake-binary");
    }

    #[test]
    fn unpacks_zip() {
        use std::io::Write as _;
        let mut buffer = std::io::Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut buffer);
            writer
                .start_file::<_, ()>("verge-mihomo.exe", Default::default())
                .expect("zip entry");
            writer.write_all(b"fake-exe").expect("zip write");
            writer.finish().expect("zip finish");
        }
        let unpacked = unpack_binary("mihomo-windows-amd64-v1.zip", buffer.get_ref()).expect("zip unpack");
        assert_eq!(unpacked, b"fake-exe");
    }

    #[test]
    fn broken_archives_are_rejected() {
        assert!(unpack_binary("mihomo-linux-amd64-v1.gz", b"garbage").is_err());
        assert!(unpack_binary("mihomo-windows-amd64-v1.zip", b"garbage").is_err());
    }

    /// Отказы обновления и смены ядра, его старта и перезапуска доходят до
    /// окна: их текст берётся из локалей, а не зашит в код на одном языке.
    #[test]
    fn the_core_refusals_speak_the_language_of_the_window() {
        let cyrillic = |text: &str| text.chars().any(|c| matches!(c, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё'));
        for source in [
            include_str!("core_updater.rs"),
            include_str!("manager/lifecycle.rs"),
            include_str!("manager/mod.rs"),
            include_str!("manager/state.rs"),
        ] {
            let code = crate::utils::source_scan::without_test_modules(source);
            for opening in [
                "bail!(",
                "anyhow!(",
                ".context(\"",
                "Err(\"",
                "Err(format!(",
                "Some(\"",
                "format!(\"",
            ] {
                for (at, _) in code.match_indices(opening) {
                    let call = code[at..].split(';').next().unwrap_or_default();
                    let literal = call.split('"').nth(1).unwrap_or_default();
                    assert!(!cyrillic(literal), "{call}");
                }
            }
        }
    }
}
