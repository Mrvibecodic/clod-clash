use crate::{config::Config, singleton, utils::dirs};
use anyhow::{Result, anyhow};
use chrono::Utc;
use clash_verge_logging::{Type, logging};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
};
use tauri_plugin_updater::{Update, UpdaterExt as _};

pub struct SilentUpdater {
    update_ready: AtomicBool,
    pending_bytes: RwLock<Option<Vec<u8>>>,
    pending_update: RwLock<Option<Update>>,
    pending_version: RwLock<Option<String>>,
}

singleton!(SilentUpdater, SILENT_UPDATER);

impl SilentUpdater {
    const fn new() -> Self {
        Self {
            update_ready: AtomicBool::new(false),
            pending_bytes: RwLock::new(None),
            pending_update: RwLock::new(None),
            pending_version: RwLock::new(None),
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

    fn write_cache(bytes: &[u8], version: &str) -> Result<()> {
        let cache_dir = Self::cache_dir()?;
        std::fs::create_dir_all(&cache_dir)?;

        let bin_path = cache_dir.join("pending_update.bin");
        std::fs::write(&bin_path, bytes)?;

        let meta = UpdateCacheMeta {
            version: version.to_string(),
            downloaded_at: Utc::now().to_rfc3339(),
        };
        let meta_path = cache_dir.join("pending_update.json");
        std::fs::write(&meta_path, serde_json::to_string_pretty(&meta)?)?;

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
    pub async fn try_install_on_startup(&self, app_handle: &tauri::AppHandle) -> bool {
        let current_version = env!("CARGO_PKG_VERSION");

        let meta = match Self::read_cache_meta() {
            Ok(meta) => meta,
            Err(_) => return false,
        };

        let cached_version = &meta.version;

        if version_lte(cached_version, current_version) {
            logging!(
                info,
                Type::System,
                "Update cache version ({}) <= current ({}), cleaning up",
                cached_version,
                current_version
            );
            Self::delete_cache();
            return false;
        }

        if is_prerelease_version(cached_version)
            && !Config::verge()
                .await
                .latest_arc()
                .receive_prereleases
                .unwrap_or(crate::config::IVerge::DEFAULT_RECEIVE_PRERELEASES)
        {
            logging!(
                info,
                Type::System,
                "Cached update ({}) is a pre-release and pre-releases are off, cleaning up",
                cached_version
            );
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

        let install_result = tokio::task::spawn_blocking({
            let bytes = bytes.clone();
            let update = update.clone();
            move || update.install(&bytes)
        });

        let success = match tokio::time::timeout(std::time::Duration::from_secs(30), install_result).await {
            Ok(Ok(Ok(()))) => {
                logging!(info, Type::System, "Update v{version} install triggered at startup");
                Self::delete_cache();
                true
            }
            Ok(Ok(Err(e))) => {
                logging!(
                    warn,
                    Type::System,
                    "Startup install failed: {e}, will retry next launch"
                );
                false
            }
            Ok(Err(e)) => {
                logging!(
                    warn,
                    Type::System,
                    "Startup install task panicked: {e}, will retry next launch"
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

        let window = match WebviewWindowBuilder::new(app_handle, "update-splash", WebviewUrl::App("index.html".into()))
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
            .build()
        {
            Ok(w) => w,
            Err(e) => {
                logging!(warn, Type::System, "Failed to create update splash: {e}");
                return;
            }
        };

        let js = format!(
            r#"
            document.documentElement.innerHTML = `
            <head><meta charset="utf-8"/><style>
              *{{margin:0;padding:0;box-sizing:border-box}}
              html,body{{height:100%;overflow:hidden;user-select:none;-webkit-user-select:none;
                font-family:-apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,"Helvetica Neue",Arial,sans-serif}}
              body{{display:flex;flex-direction:column;align-items:center;justify-content:center;
                background:#1e1e2e;color:#cdd6f4}}
              @media(prefers-color-scheme:light){{
                body{{background:#eff1f5;color:#4c4f69}}
                .bar{{background:#dce0e8}}.fill{{background:#1e66f5}}.sub{{color:#6c6f85}}
              }}
              .icon{{width:48px;height:48px;margin-bottom:16px;animation:pulse 2s ease-in-out infinite}}
              .title{{font-size:16px;font-weight:600;margin-bottom:6px}}
              .sub{{font-size:13px;color:#a6adc8;margin-bottom:20px}}
              .bar{{width:200px;height:4px;background:#313244;border-radius:2px;overflow:hidden}}
              .fill{{height:100%;width:30%;background:#89b4fa;border-radius:2px;animation:ind 1.5s ease-in-out infinite}}
              @keyframes ind{{0%{{width:0;margin-left:0}}50%{{width:40%;margin-left:30%}}100%{{width:0;margin-left:100%}}}}
              @keyframes pulse{{0%,100%{{opacity:1}}50%{{opacity:.6}}}}
            </style></head>
            <body>
              <svg class="icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/>
                <polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/>
              </svg>
              <div class="title">Installing Update...</div>
              <div class="sub">v{version}</div>
              <div class="bar"><div class="fill"></div></div>
            </body>`;
            "#
        );

        std::thread::spawn(move || {
            for i in 0..10 {
                std::thread::sleep(std::time::Duration::from_millis(100 * (i + 1)));
                if window.eval(&js).is_ok() {
                    return;
                }
            }
        });

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

fn updater_builder(app_handle: &tauri::AppHandle, language: Option<&str>) -> tauri_plugin_updater::UpdaterBuilder {
    let _ = language;
    let builder = app_handle.updater_builder();
    #[cfg(target_os = "windows")]
    let builder = {
        let lang_id = nsis_language_id(&clash_verge_i18n::current_language(language));
        builder.installer_arg(format!("/LANG={lang_id}"))
    };
    builder
}

async fn check_update_with_fallback(app_handle: &tauri::AppHandle) -> Result<Option<Update>> {
    let language = Config::verge().await.latest_arc().language.clone();
    let updater = updater_builder(app_handle, language.as_deref()).build()?;
    match updater.check().await {
        Ok(found) => Ok(found),
        Err(direct_error) => {
            let port = Config::verge().await.latest_arc().verge_mixed_port.unwrap_or(7897);
            let proxy = format!("http://127.0.0.1:{port}");
            logging!(
                warn,
                Type::System,
                "update check failed directly ({direct_error}), retrying via {proxy}"
            );
            let updater = updater_builder(app_handle, language.as_deref())
                .proxy(tauri::Url::parse(&proxy)?)
                .build()?;
            Ok(updater.check().await?)
        }
    }
}

impl SilentUpdater {
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
            super::handle::Handle::notice_message(
                "info",
                format!("New version v{version} contains breaking changes. Please update manually."),
            );
            return Ok(());
        }

        logging!(info, Type::System, "Silent updater: downloading v{version}...");
        let bytes = update
            .download(
                |chunk_len, content_len| {
                    logging!(
                        debug,
                        Type::System,
                        "Silent updater download progress: chunk={chunk_len}, total={content_len:?}"
                    );
                },
                || {
                    logging!(info, Type::System, "Silent updater: download complete");
                },
            )
            .await?;

        if let Err(e) = Self::write_cache(&bytes, &version) {
            logging!(warn, Type::System, "Silent updater: failed to write cache: {e}");
        }

        *self.pending_bytes.write() = Some(bytes);
        *self.pending_update.write() = Some(update);
        *self.pending_version.write() = Some(version.clone());
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
