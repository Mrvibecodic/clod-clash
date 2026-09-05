use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use anyhow::{Context as _, Result};
use clash_verge_logging::{Type, logging};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::utils::{dirs, help};

const PINS_FILE: &str = "core-pins.yaml";

const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct CorePins {
    #[serde(default)]
    app_version: String,
    #[serde(default)]
    cores: BTreeMap<String, String>,
}

static PINS_LOCK: std::sync::LazyLock<tokio::sync::Mutex<()>> =
    std::sync::LazyLock::new(|| tokio::sync::Mutex::new(()));

fn pins_path() -> Result<PathBuf> {
    Ok(dirs::app_home_dir()?.join(PINS_FILE))
}

fn pin_key(path: &Path) -> String {
    std::fs::canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

pub async fn digest_of(path: &Path) -> Result<String> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || -> Result<String> {
        let mut file = std::fs::File::open(&path).with_context(|| format!("failed to open core binary {path:?}"))?;
        let mut hasher = Sha256::new();
        std::io::copy(&mut file, &mut hasher).with_context(|| format!("failed to read core binary {path:?}"))?;
        Ok(format!("{:x}", hasher.finalize()))
    })
    .await
    .context("core digest task panicked")?
}

pub fn digest_of_bytes(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}

async fn load_pins() -> CorePins {
    let Ok(path) = pins_path() else {
        return CorePins::default();
    };
    let mut pins: CorePins = help::read_yaml(&path).await.unwrap_or_default();

    if pins.app_version != APP_VERSION {
        pins = CorePins {
            app_version: APP_VERSION.to_owned(),
            cores: BTreeMap::new(),
        };
    }
    pins
}

async fn store_pins(pins: &CorePins) -> Result<()> {
    let path = pins_path()?;
    help::save_yaml(
        &path,
        pins,
        Some("# clod: отпечатки ядра, снятые этой версией приложения"),
    )
    .await
}

pub async fn pin_known_binary(path: &Path, digest: &str) {
    let _guard = PINS_LOCK.lock().await;
    let mut pins = load_pins().await;
    pins.app_version = APP_VERSION.to_owned();
    pins.cores.insert(pin_key(path), digest.to_owned());

    if let Err(err) = store_pins(&pins).await {
        logging!(warn, Type::Core, "failed to record core digest: {err:#}");
    }
}

pub async fn repin_binary(path: &Path) {
    match digest_of(path).await {
        Ok(digest) => {
            pin_known_binary(path, &digest).await;
            logging!(info, Type::Core, "re-pinned core binary {path:?}");
        }
        Err(err) => {
            logging!(warn, Type::Core, "failed to re-pin core binary {path:?}: {err:#}");
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum PinCheck {
    Match,
    Recorded,
    Changed { expected: String, actual: String },
}

pub async fn check_binary(path: &Path) -> Result<PinCheck> {
    let actual = digest_of(path).await?;

    let _guard = PINS_LOCK.lock().await;
    let mut pins = load_pins().await;
    let key = pin_key(path);

    match pins.cores.get(&key) {
        Some(expected) if *expected == actual => Ok(PinCheck::Match),
        Some(expected) => Ok(PinCheck::Changed {
            expected: expected.clone(),
            actual,
        }),
        None => {
            pins.app_version = APP_VERSION.to_owned();
            pins.cores.insert(key, actual);
            if let Err(err) = store_pins(&pins).await {
                logging!(warn, Type::Core, "failed to record core digest: {err:#}");
            }
            Ok(PinCheck::Recorded)
        }
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum WriteAccess {
    AdminOnly,
    Unprivileged,
}

fn looks_like_a_privileged_root(dir: &Path) -> bool {
    let text = dir.to_string_lossy().to_ascii_lowercase().replace('\\', "/");
    [
        "/program files",
        "/programdata",
        "/windows/",
        "/applications/",
        "/library/",
        "/usr/",
        "/opt/",
    ]
    .iter()
    .any(|root| text.contains(root))
}

fn write_access_of(dir: &Path, elevated: bool) -> WriteAccess {
    if elevated {
        return if looks_like_a_privileged_root(dir) {
            WriteAccess::AdminOnly
        } else {
            WriteAccess::Unprivileged
        };
    }
    let probe = dir.join(format!(".clod-write-probe-{}", std::process::id()));
    match std::fs::OpenOptions::new().write(true).create_new(true).open(&probe) {
        Ok(file) => {
            drop(file);
            let _ = std::fs::remove_file(&probe);
            WriteAccess::Unprivileged
        }
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => WriteAccess::Unprivileged,
        Err(_) => WriteAccess::AdminOnly,
    }
}

fn directory_write_access(path: &Path) -> WriteAccess {
    let Some(dir) = path.parent() else {
        return WriteAccess::Unprivileged;
    };
    write_access_of(dir, crate::feat::tun::is_app_elevated())
}

#[derive(Debug)]
pub struct CoreBinaryChanged {
    pub path: PathBuf,
}

impl std::fmt::Display for CoreBinaryChanged {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "core binary at {:?} changed since installation, refusing to start it with system privileges",
            self.path
        )
    }
}

impl std::error::Error for CoreBinaryChanged {}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub fn is_core_binary_changed(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| cause.is::<CoreBinaryChanged>())
}

pub async fn ensure_elevated_binary_is_known(path: &Path) -> Result<()> {
    match check_binary(path).await {
        Ok(PinCheck::Match) => Ok(()),
        Ok(PinCheck::Recorded) => {
            logging!(info, Type::Core, "recorded core digest for {path:?}");
            Ok(())
        }
        Ok(PinCheck::Changed { expected, actual }) => {
            if directory_write_access(path) == WriteAccess::AdminOnly {
                logging!(
                    warn,
                    Type::Core,
                    "core binary at {path:?} changed (expected {expected}, got {actual}), \
                     but only an administrator can write there — accepting it and re-pinning"
                );
                pin_known_binary(path, &actual).await;
                return Ok(());
            }

            logging!(
                error,
                Type::Core,
                "core binary at {path:?} changed since it was pinned and its directory is writable \
                 without privileges: expected {expected}, got {actual}"
            );
            crate::core::handle::Handle::notice_message("core::binary_changed", path.to_string_lossy().into_owned());
            Err(anyhow::Error::new(CoreBinaryChanged {
                path: path.to_path_buf(),
            }))
        }
        Err(err) => {
            logging!(error, Type::Core, "failed to verify core binary {path:?}: {err:#}");
            Err(err)
        }
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::{
        CoreBinaryChanged, WriteAccess, digest_of, digest_of_bytes, is_core_binary_changed, write_access_of,
    };

    #[test]
    fn a_writable_directory_is_seen_as_writable_and_leaves_nothing_behind() {
        let dir = std::env::temp_dir().join(format!("clod-core-probe-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let binary = dir.join("verge-mihomo");
        std::fs::write(&binary, b"core").expect("write");

        assert_eq!(write_access_of(&dir, false), WriteAccess::Unprivileged);
        let left: Vec<_> = std::fs::read_dir(&dir)
            .expect("read dir")
            .filter_map(Result::ok)
            .map(|entry| entry.file_name())
            .collect();
        assert_eq!(left, vec![binary.file_name().expect("name").to_owned()]);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_directory_that_does_not_exist_counts_as_closed() {
        let missing = std::env::temp_dir().join(format!("clod-core-probe-missing-{}", std::process::id()));
        assert_eq!(write_access_of(&missing, false), WriteAccess::AdminOnly);
    }

    #[test]
    fn the_refusal_survives_being_wrapped_in_context() {
        let err = anyhow::Error::new(CoreBinaryChanged {
            path: std::path::PathBuf::from("/somewhere/verge-mihomo"),
        })
        .context("failed to start the core through the service");
        assert!(is_core_binary_changed(&err));
        assert!(!is_core_binary_changed(&anyhow::anyhow!("service is not running")));
    }

    #[tokio::test]
    async fn file_and_bytes_agree_on_the_digest() {
        let dir = std::env::temp_dir().join(format!("clod-core-pin-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let file = dir.join("core-binary");
        let payload = b"not really a core, but hashes the same way";
        std::fs::write(&file, payload).expect("write");

        let from_file = digest_of(&file).await.expect("digest");
        assert_eq!(from_file, digest_of_bytes(payload));
        assert_eq!(
            digest_of_bytes(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn missing_file_is_an_error_not_a_silent_pass() {
        let missing = std::env::temp_dir().join("clod-core-pin-does-not-exist");
        assert!(digest_of(&missing).await.is_err());
    }

    #[test]
    fn run_as_administrator_no_longer_calls_a_protected_folder_open_to_everyone() {
        for dir in [
            r"C:\Program Files\Clod Clash",
            r"C:\Program Files (x86)\Clod Clash",
            r"C:\ProgramData\clod-clash",
            "/Applications/Clod Clash.app/Contents/Resources",
            "/usr/lib/clod-clash",
        ] {
            assert_eq!(
                write_access_of(std::path::Path::new(dir), true),
                WriteAccess::AdminOnly,
                "a protected folder was taken for a writable one: {dir}"
            );
        }
    }

    #[test]
    fn run_as_administrator_still_distrusts_a_user_writable_folder() {
        for dir in [
            r"C:\Users\alex\AppData\Local\clod-clash",
            r"D:\portable\clod-clash",
            "/home/alex/.local/share/clod-clash",
        ] {
            assert_eq!(
                write_access_of(std::path::Path::new(dir), true),
                WriteAccess::Unprivileged,
                "a user folder was taken for a protected one: {dir}"
            );
        }
    }
}
