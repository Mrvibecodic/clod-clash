//! clod:F5 — managed Mihomo core.
//!
//! Downloads an official core build from MetaCubeX/mihomo (stable release or
//! the Prerelease-Alpha channel) and runs it instead of the bundled sidecar,
//! so the core can move faster than the application. The core itself is never
//! modified — this module only delivers the binary.
//!
//! Layout under `{app_home}/cores/`:
//!   `mihomo-{version}/verge-mihomo(.exe)` — one directory per version;
//!   `current` / `previous` — plain text pointer files with a version each.
//! Pointer files instead of symlinks: on Windows symlinks need privileges,
//! and a text file survives every filesystem.
//!
//! Safety order on apply: download → unpack → `-v` probe → stop core → move
//! pointers → start core; any failure rolls the pointers back and restarts
//! whatever ran before. With `use_managed_core` off (the default) every code
//! path falls through to the bundled sidecar untouched.

use std::{
    io::Read as _,
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

use anyhow::{Context as _, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use sha2::Digest as _;
use tauri::Emitter as _;

use crate::{
    config::{Config, IVerge},
    core::{CoreManager, handle},
    utils::{
        dirs,
        network::{NetworkManager, ProxyType},
    },
};
use clash_verge_logging::{Type, logging, logging_error};

const RELEASE_API_STABLE: &str = "https://api.github.com/repos/MetaCubeX/mihomo/releases/latest";
const RELEASE_API_ALPHA: &str = "https://api.github.com/repos/MetaCubeX/mihomo/releases/tags/Prerelease-Alpha";
const DOWNLOAD_TIMEOUT_SECS: u64 = 300;
const API_TIMEOUT_SECS: u64 = 30;
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);
/// Progress event listened to by the settings dialog.
const PROGRESS_EVENT: &str = "clod://core-update-progress";

/// One update at a time; the flag also guards revert.
static UPDATING: AtomicBool = AtomicBool::new(false);

// ---------------------------------------------------------------------------
// public state types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct CoreUpdaterStatus {
    /// `use_managed_core` and a usable binary both present.
    pub managed_active: bool,
    /// Version the `current` pointer names, if its binary exists.
    pub current: Option<String>,
    /// Version available for rollback.
    pub previous: Option<String>,
    /// What the running core reports through its API, whoever provides it.
    pub running: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CoreUpdateCheck {
    pub channel: String,
    /// Version of the running core (None when the core is not reachable).
    pub current: Option<String>,
    pub latest: String,
    pub update_available: bool,
}

#[derive(Debug, Clone, Serialize)]
struct Progress<'a> {
    phase: &'a str,
    received: u64,
    total: u64,
}

// ---------------------------------------------------------------------------
// GitHub release model
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct GhRelease {
    assets: Vec<GhAsset>,
}

#[derive(Debug, Clone, Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
    size: u64,
}

// ---------------------------------------------------------------------------
// paths and pointers
// ---------------------------------------------------------------------------

fn core_binary_file_name() -> String {
    format!("verge-mihomo{}", std::env::consts::EXE_SUFFIX)
}

pub fn cores_dir() -> Result<PathBuf> {
    Ok(dirs::app_home_dir()?.join("cores"))
}

/// Versions land in the filesystem, so anything path-hostile is replaced.
fn sanitize_version(version: &str) -> String {
    version
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '.' || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn version_dir(version: &str) -> Result<PathBuf> {
    Ok(cores_dir()?.join(format!("mihomo-{}", sanitize_version(version))))
}

fn version_binary(version: &str) -> Result<PathBuf> {
    Ok(version_dir(version)?.join(core_binary_file_name()))
}

fn pointer_file(name: &str) -> Result<PathBuf> {
    Ok(cores_dir()?.join(name))
}

fn read_pointer(name: &str) -> Option<String> {
    let path = pointer_file(name).ok()?;
    let content = std::fs::read_to_string(path).ok()?;
    let trimmed = content.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn write_pointer(name: &str, version: Option<&str>) -> Result<()> {
    let path = pointer_file(name)?;
    match version {
        Some(v) => std::fs::write(&path, v).context("failed to write core pointer")?,
        None => {
            let _ = std::fs::remove_file(&path);
        }
    }
    Ok(())
}

/// The managed binary the core should run with, or `None` for the bundled
/// sidecar. `None` on any doubt — a broken managed state must never keep the
/// user offline.
pub async fn managed_core_binary() -> Option<PathBuf> {
    let verge = Config::verge().await.latest_arc();
    if !verge.use_managed_core.unwrap_or(false) {
        return None;
    }
    let version = read_pointer("current")?;
    let binary = version_binary(&version).ok()?;
    if binary.is_file() {
        Some(binary)
    } else {
        logging!(
            warn,
            Type::Core,
            "managed core {} is missing on disk, falling back to the bundled sidecar",
            version
        );
        None
    }
}

// ---------------------------------------------------------------------------
// release discovery
// ---------------------------------------------------------------------------

fn configured_channel(verge: &IVerge) -> String {
    match verge.managed_core_channel.as_deref() {
        Some("alpha") => "alpha".into(),
        _ => "stable".into(),
    }
}

fn release_url(channel: &str) -> &'static str {
    if channel == "alpha" {
        RELEASE_API_ALPHA
    } else {
        RELEASE_API_STABLE
    }
}

fn target_os_arch() -> Result<(&'static str, &'static str)> {
    let os = match std::env::consts::OS {
        os @ ("windows" | "linux") => os,
        "macos" => "darwin",
        other => bail!("unsupported platform for managed core: {other}"),
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        other => bail!("unsupported architecture for managed core: {other}"),
    };
    Ok((os, arch))
}

/// Choose the plain build for this machine and derive its version from the
/// asset name (`mihomo-{os}-{arch}-{version}.{ext}`); the alpha channel has
/// no usable tag, the hash lives only in the file name.
fn pick_asset(assets: &[GhAsset], os: &str, arch: &str) -> Result<(GhAsset, String)> {
    let prefix = format!("mihomo-{os}-{arch}-");
    let ext = if os == "windows" { ".zip" } else { ".gz" };

    let asset = assets
        .iter()
        .filter(|a| a.name.starts_with(&prefix) && a.name.ends_with(ext))
        .find(|a| !a.name.contains("compatible") && !a.name.contains("-go1"))
        .or_else(|| {
            assets
                .iter()
                .find(|a| a.name.starts_with(&prefix) && a.name.ends_with(ext))
        })
        .cloned()
        .ok_or_else(|| anyhow!("no release asset for {os}/{arch}"))?;

    let version = asset.name[prefix.len()..asset.name.len() - ext.len()].to_string();
    if version.is_empty() {
        bail!("could not derive a version from asset name {}", asset.name);
    }
    Ok((asset, version))
}

/// GitHub may be unreachable directly for exactly the people this app is
/// built for, so every request falls back to going through the own core.
async fn http_client(proxy: ProxyType, timeout: u64) -> Result<reqwest::Client> {
    NetworkManager::new()
        .create_request(
            proxy,
            Some(timeout),
            Some(format!("clod-clash/{}", env!("CARGO_PKG_VERSION")).into()),
            false,
        )
        .await
}

async fn fetch_release(channel: &str) -> Result<GhRelease> {
    let url = release_url(channel);
    let mut last_error = anyhow!("release request not attempted");
    for proxy in [ProxyType::None, ProxyType::Localhost] {
        let attempt = async {
            let client = http_client(proxy, API_TIMEOUT_SECS).await?;
            let response = client.get(url).send().await?;
            if !response.status().is_success() {
                bail!("GitHub API returned {}", response.status());
            }
            Ok::<GhRelease, anyhow::Error>(response.json::<GhRelease>().await?)
        };
        match attempt.await {
            Ok(release) => return Ok(release),
            Err(err) => {
                logging!(warn, Type::Core, "core release fetch via {proxy:?} failed: {err:#}");
                last_error = err;
            }
        }
    }
    Err(last_error.context("failed to reach the Mihomo release channel"))
}

async fn running_core_version() -> Option<String> {
    let version = handle::Handle::mihomo().await.get_version().await.ok()?;
    Some(version.version)
}

pub async fn status() -> CoreUpdaterStatus {
    let verge = Config::verge().await.latest_arc();
    let enabled = verge.use_managed_core.unwrap_or(false);
    let current = read_pointer("current").filter(|v| version_binary(v).map(|p| p.is_file()).unwrap_or(false));
    let previous = read_pointer("previous").filter(|v| version_binary(v).map(|p| p.is_file()).unwrap_or(false));
    CoreUpdaterStatus {
        managed_active: enabled && current.is_some(),
        current,
        previous,
        running: running_core_version().await,
    }
}

pub async fn check_core_update() -> Result<CoreUpdateCheck> {
    let verge = Config::verge().await.latest_arc();
    let channel = configured_channel(&verge);
    let (os, arch) = target_os_arch()?;
    let release = fetch_release(&channel).await?;
    let (_, latest) = pick_asset(&release.assets, os, arch)?;
    let current = running_core_version().await;
    let update_available = current.as_deref() != Some(latest.as_str());
    Ok(CoreUpdateCheck {
        channel,
        current,
        latest,
        update_available,
    })
}

// ---------------------------------------------------------------------------
// download / unpack / verify
// ---------------------------------------------------------------------------

fn emit_progress(phase: &str, received: u64, total: u64) {
    let payload = Progress { phase, received, total };
    let _ = handle::Handle::app_handle().emit(PROGRESS_EVENT, &payload);
}

async fn download_asset(asset: &GhAsset) -> Result<Vec<u8>> {
    let mut last_error = anyhow!("download not attempted");
    for proxy in [ProxyType::None, ProxyType::Localhost] {
        let attempt = async {
            let client = http_client(proxy, DOWNLOAD_TIMEOUT_SECS).await?;
            let mut response = client.get(&asset.browser_download_url).send().await?;
            if !response.status().is_success() {
                bail!("asset download returned {}", response.status());
            }
            let total = response.content_length().unwrap_or(asset.size);
            let mut bytes: Vec<u8> = Vec::with_capacity(total.min(64 * 1024 * 1024) as usize);
            let mut last_emitted = 0u64;
            while let Some(chunk) = response.chunk().await? {
                bytes.extend_from_slice(&chunk);
                let received = bytes.len() as u64;
                if received - last_emitted >= 256 * 1024 || received == total {
                    emit_progress("downloading", received, total);
                    last_emitted = received;
                }
            }
            Ok::<Vec<u8>, anyhow::Error>(bytes)
        };
        match attempt.await {
            Ok(bytes) => return Ok(bytes),
            Err(err) => {
                logging!(warn, Type::Core, "core download via {proxy:?} failed: {err:#}");
                last_error = err;
            }
        }
    }
    Err(last_error.context("failed to download the core archive"))
}

/// Verify against `{asset}.sha256` when the release ships one; the alpha
/// channel sometimes does not, and then the `-v` probe of the unpacked
/// binary is the integrity check.
async fn verify_sha256(release: &GhRelease, asset: &GhAsset, bytes: &[u8]) -> Result<()> {
    let checksum_name = format!("{}.sha256", asset.name);
    let Some(checksum_asset) = release.assets.iter().find(|a| a.name == checksum_name) else {
        logging!(
            info,
            Type::Core,
            "release has no {checksum_name}, skipping checksum verification"
        );
        return Ok(());
    };

    let text = {
        let mut last_error = anyhow!("checksum download not attempted");
        let mut result = None;
        for proxy in [ProxyType::None, ProxyType::Localhost] {
            let attempt = async {
                let client = http_client(proxy, API_TIMEOUT_SECS).await?;
                let response = client.get(&checksum_asset.browser_download_url).send().await?;
                if !response.status().is_success() {
                    bail!("checksum download returned {}", response.status());
                }
                Ok::<String, anyhow::Error>(response.text().await?)
            };
            match attempt.await {
                Ok(t) => {
                    result = Some(t);
                    break;
                }
                Err(err) => last_error = err,
            }
        }
        result.ok_or(last_error)?
    };

    let expected = text
        .split_whitespace()
        .find(|token| token.len() == 64 && token.chars().all(|c| c.is_ascii_hexdigit()))
        .ok_or_else(|| anyhow!("no sha256 digest inside {checksum_name}"))?
        .to_ascii_lowercase();

    let actual = format!("{:x}", sha2::Sha256::digest(bytes));
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

async fn install_binary(version: &str, data: &[u8]) -> Result<PathBuf> {
    let dir = version_dir(version)?;
    tokio::fs::create_dir_all(&dir).await?;
    let target = version_binary(version)?;
    let staging = dir.join(format!("{}.tmp", core_binary_file_name()));
    tokio::fs::write(&staging, data).await?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        tokio::fs::set_permissions(&staging, std::fs::Permissions::from_mode(0o755)).await?;
    }

    tokio::fs::rename(&staging, &target).await?;

    // Gatekeeper: a freshly written binary may carry the quarantine flag when
    // the bytes travelled through certain APIs. Best effort — app_data is
    // normally fine.
    #[cfg(target_os = "macos")]
    {
        let _ = tokio::process::Command::new("xattr")
            .arg("-c")
            .arg(&target)
            .output()
            .await;
    }

    Ok(target)
}

/// The unpacked binary must at least answer `-v`; everything else stays the
/// core's own business.
async fn probe_binary(binary: &PathBuf) -> Result<String> {
    let output = tokio::time::timeout(PROBE_TIMEOUT, tokio::process::Command::new(binary).arg("-v").output())
        .await
        .context("core -v probe timed out")??;
    if !output.status.success() {
        bail!("core -v probe exited with {}", output.status);
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Drop version directories no pointer names any more.
fn cleanup_versions() {
    let Ok(dir) = cores_dir() else { return };
    let keep: Vec<String> = ["current", "previous"]
        .iter()
        .filter_map(|name| read_pointer(name))
        .map(|v| format!("mihomo-{}", sanitize_version(&v)))
        .collect();

    let Ok(entries) = std::fs::read_dir(&dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if entry.path().is_dir() && name.starts_with("mihomo-") && !keep.iter().any(|k| k == &name) {
            logging_error!(Type::Core, std::fs::remove_dir_all(entry.path()));
        }
    }
}

// ---------------------------------------------------------------------------
// apply / revert
// ---------------------------------------------------------------------------

struct UpdateGuard;

impl UpdateGuard {
    fn acquire() -> Result<Self> {
        if UPDATING.swap(true, Ordering::AcqRel) {
            bail!("a core update is already running");
        }
        Ok(Self)
    }
}

impl Drop for UpdateGuard {
    fn drop(&mut self) {
        UPDATING.store(false, Ordering::Release);
    }
}

async fn ensure_managed_enabled() -> Result<()> {
    let verge = Config::verge().await.latest_arc();
    if verge.use_managed_core.unwrap_or(false) {
        return Ok(());
    }
    let patch = IVerge {
        use_managed_core: Some(true),
        ..IVerge::default()
    };
    crate::feat::patch_verge(&patch, false).await
}

/// Switch the pointers and restart the core on the new binary; roll back and
/// restart the old state when anything on the way explodes.
async fn swap_to_version(version: &str) -> Result<()> {
    let old_current = read_pointer("current");
    let old_previous = read_pointer("previous");

    CoreManager::global().stop_core().await?;

    write_pointer("previous", old_current.as_deref())?;
    write_pointer("current", Some(version))?;

    match CoreManager::global().start_core().await {
        Ok(()) => {
            cleanup_versions();
            Ok(())
        }
        Err(start_error) => {
            logging!(
                error,
                Type::Core,
                "new core failed to start, rolling back: {start_error:#}"
            );
            write_pointer("current", old_current.as_deref())?;
            write_pointer("previous", old_previous.as_deref())?;
            if let Err(rollback_error) = CoreManager::global().start_core().await {
                bail!("new core failed to start ({start_error:#}) and the rollback failed too: {rollback_error:#}");
            }
            Err(start_error.context("the new core failed to start; the previous one is back"))
        }
    }
}

/// Full update: resolve the channel's latest build, download, verify, unpack,
/// probe and switch over.
pub async fn download_and_apply_core() -> Result<CoreUpdateCheck> {
    let _guard = UpdateGuard::acquire()?;

    let verge = Config::verge().await.latest_arc();
    let channel = configured_channel(&verge);
    let (os, arch) = target_os_arch()?;

    emit_progress("checking", 0, 0);
    let release = fetch_release(&channel).await?;
    let (asset, version) = pick_asset(&release.assets, os, arch)?;

    if read_pointer("current").as_deref() == Some(version.as_str()) && managed_core_binary().await.is_some() {
        emit_progress("done", 0, 0);
        return Ok(CoreUpdateCheck {
            channel,
            current: Some(version.clone()),
            latest: version,
            update_available: false,
        });
    }

    logging!(info, Type::Core, "updating managed core to {version} ({})", asset.name);

    emit_progress("downloading", 0, asset.size);
    let bytes = download_asset(&asset).await?;

    emit_progress("verifying", 0, 0);
    verify_sha256(&release, &asset, &bytes).await?;
    let binary_data = unpack_binary(&asset.name, &bytes)?;
    let binary = install_binary(&version, &binary_data).await?;
    let probed = probe_binary(&binary).await?;
    logging!(info, Type::Core, "managed core probe: {probed}");

    ensure_managed_enabled().await?;

    emit_progress("applying", 0, 0);
    swap_to_version(&version).await?;

    emit_progress("done", 0, 0);
    handle::Handle::notice_message("clod_core::updated", version.clone());

    Ok(CoreUpdateCheck {
        channel,
        current: Some(version.clone()),
        latest: version,
        update_available: false,
    })
}

/// Swap back to the previous managed version.
pub async fn revert_core() -> Result<()> {
    let _guard = UpdateGuard::acquire()?;

    let previous = read_pointer("previous")
        .filter(|v| version_binary(v).map(|p| p.is_file()).unwrap_or(false))
        .ok_or_else(|| anyhow!("no previous core version to revert to"))?;

    swap_to_version(&previous).await
}

/// Turn the managed core off and go back to the bundled sidecar.
pub async fn disable_managed_core() -> Result<()> {
    let _guard = UpdateGuard::acquire()?;
    let patch = IVerge {
        use_managed_core: Some(false),
        ..IVerge::default()
    };
    crate::feat::patch_verge(&patch, false).await?;
    CoreManager::global().restart_core().await
}

// ---------------------------------------------------------------------------
// background check
// ---------------------------------------------------------------------------

const AUTO_CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
const AUTO_CHECK_STARTUP_DELAY: Duration = Duration::from_secs(90);

/// Once a day, and only when the user opted in: check the channel and raise
/// a notice — never download anything behind the user's back.
pub fn spawn_auto_check() {
    crate::process::AsyncHandler::spawn(|| async {
        tokio::time::sleep(AUTO_CHECK_STARTUP_DELAY).await;
        loop {
            let verge = Config::verge().await.latest_arc();
            let enabled = verge.use_managed_core.unwrap_or(false) && verge.core_auto_check.unwrap_or(true);
            drop(verge);

            if enabled {
                match check_core_update().await {
                    Ok(check) if check.update_available => {
                        handle::Handle::notice_message("clod_core::update_available", check.latest.clone());
                    }
                    Ok(_) => {}
                    Err(err) => {
                        logging!(warn, Type::Core, "core auto-check failed: {err:#}");
                    }
                }
            }

            tokio::time::sleep(AUTO_CHECK_INTERVAL).await;
        }
    });
}

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    fn asset(name: &str) -> GhAsset {
        GhAsset {
            name: name.into(),
            browser_download_url: format!("https://example.com/{name}"),
            size: 1,
        }
    }

    #[test]
    fn picks_plain_stable_asset_and_version() {
        let assets = vec![
            asset("mihomo-linux-amd64-compatible-v1.19.2.gz"),
            asset("mihomo-linux-amd64-v1.19.2.gz"),
            asset("mihomo-linux-arm64-v1.19.2.gz"),
            asset("mihomo-linux-amd64-v1.19.2.gz.sha256"),
        ];
        let (picked, version) = pick_asset(&assets, "linux", "amd64").expect("linux asset");
        assert_eq!(picked.name, "mihomo-linux-amd64-v1.19.2.gz");
        assert_eq!(version, "v1.19.2");
    }

    #[test]
    fn picks_windows_zip() {
        let assets = vec![
            asset("mihomo-windows-amd64-v1.19.2.zip"),
            asset("mihomo-windows-amd64-v1.19.2.zip.sha256"),
        ];
        let (picked, version) = pick_asset(&assets, "windows", "amd64").expect("windows asset");
        assert_eq!(picked.name, "mihomo-windows-amd64-v1.19.2.zip");
        assert_eq!(version, "v1.19.2");
    }

    #[test]
    fn derives_alpha_version_from_asset_name() {
        let assets = vec![
            asset("mihomo-darwin-arm64-alpha-g0a1b2c3.gz"),
            asset("mihomo-darwin-amd64-alpha-g0a1b2c3.gz"),
        ];
        let (_, version) = pick_asset(&assets, "darwin", "arm64").expect("darwin asset");
        assert_eq!(version, "alpha-g0a1b2c3");
    }

    #[test]
    fn falls_back_to_compatible_when_nothing_else_fits() {
        let assets = vec![asset("mihomo-linux-amd64-compatible-v1.19.2.gz")];
        let (picked, version) = pick_asset(&assets, "linux", "amd64").expect("linux asset");
        assert_eq!(picked.name, "mihomo-linux-amd64-compatible-v1.19.2.gz");
        assert_eq!(version, "compatible-v1.19.2");
    }

    #[test]
    fn rejects_missing_platform() {
        let assets = vec![asset("mihomo-linux-amd64-v1.19.2.gz")];
        assert!(pick_asset(&assets, "windows", "arm64").is_err());
    }

    #[test]
    fn sanitizes_hostile_versions() {
        assert_eq!(sanitize_version("v1.19.2"), "v1.19.2");
        assert_eq!(sanitize_version("../../evil"), ".._.._evil");
        assert_eq!(sanitize_version("alpha-g0a1b2c3"), "alpha-g0a1b2c3");
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
}
