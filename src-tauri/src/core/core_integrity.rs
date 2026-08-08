//! clod: запускаем только то ядро, которое уже видели.
//!
//! Служба работает с правами системы и запускает бинарь по пути, который ей
//! назовём мы, — сам она ничего не проверяет. Значит подменивший файл ядра
//! получает SYSTEM на Windows и root на остальных: файл лежит рядом с
//! приложением, а на macOS и Linux каталог установки бывает и пользовательским.
//!
//! Настоящее место для проверки — внутри службы, но её протокол чужой
//! (`clash-verge-service-ipc`), и передать туда ожидаемый отпечаток нам некуда.
//! Поэтому проверяем на своей стороне, до просьбы о запуске. Это слабее: между
//! нашим чтением файла и стартом службы остаётся окно, в которое файл можно
//! подменить. Но realistic-атака это не гонка на миллисекундах, а дроппер,
//! который переписал ядро час назад и ждёт следующего запуска, — и её такая
//! проверка ловит.
//!
//! Отпечаток берётся при первой встрече с файлом и живёт до смены версии
//! приложения: обновление законно приносит новый бинарь, и держать старый
//! отпечаток означало бы отказ запускаться после каждого апдейта. Обновление
//! managed-ядра записывает отпечаток само — там мы знаем байты ещё до записи на
//! диск, и доверия «при первой встрече» не требуется вовсе.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use anyhow::{Context as _, Result, bail};
use clash_verge_logging::{Type, logging};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::utils::{dirs, help};

/// Файл с отпечатками рядом с остальным состоянием приложения.
const PINS_FILE: &str = "core-pins.yaml";

/// Версия, под которой сняты отпечатки. Меняется — снимаем заново.
const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct CorePins {
    /// Версия приложения, при которой отпечатки снимались.
    #[serde(default)]
    app_version: String,
    /// Путь к бинарю — отпечаток в hex.
    #[serde(default)]
    cores: BTreeMap<String, String>,
}

/// Один замок на чтение-правку-запись: старт ядра и обновление managed-ядра
/// могут прийти одновременно, а файл маленький и трогается редко.
static PINS_LOCK: std::sync::LazyLock<tokio::sync::Mutex<()>> =
    std::sync::LazyLock::new(|| tokio::sync::Mutex::new(()));

fn pins_path() -> Result<PathBuf> {
    Ok(dirs::app_home_dir()?.join(PINS_FILE))
}

/// Ключ должен пережить `/./`, лишние слэши и симлинк на каталог установки,
/// иначе один и тот же бинарь получит два отпечатка.
fn pin_key(path: &Path) -> String {
    std::fs::canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

/// SHA-256 файла. Читаем потоком: ядро весит десятки мегабайт, и поднимать
/// его целиком в память ради одного хеша незачем.
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

/// SHA-256 уже прочитанных байтов — для только что скачанного ядра.
pub fn digest_of_bytes(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}

async fn load_pins() -> CorePins {
    let Ok(path) = pins_path() else {
        return CorePins::default();
    };
    let mut pins: CorePins = help::read_yaml(&path).await.unwrap_or_default();

    // Обновление приложения законно приносит другой бинарь. Старые отпечатки
    // после него означают только одно — отказ запускаться.
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

/// Запомнить отпечаток заранее известных байтов.
///
/// Зовётся при установке managed-ядра: там доверять «первой встрече» не нужно,
/// байты только что приехали и уже проверены по контрольной сумме релиза.
pub async fn pin_known_binary(path: &Path, digest: &str) {
    let _guard = PINS_LOCK.lock().await;
    let mut pins = load_pins().await;
    pins.app_version = APP_VERSION.to_owned();
    pins.cores.insert(pin_key(path), digest.to_owned());

    if let Err(err) = store_pins(&pins).await {
        // Не запомнили — значит следующая проверка снимет отпечаток сама.
        // Ронять из-за этого установку ядра нечестно.
        logging!(warn, Type::Core, "failed to record core digest: {err:#}");
    }
}

/// Что показала сверка отпечатка.
#[derive(Debug, PartialEq, Eq)]
pub enum PinCheck {
    /// Отпечаток совпал с запомненным.
    Match,
    /// Файла раньше не видели — отпечаток снят и записан.
    Recorded,
    /// Файл изменился с тех пор, как мы его запомнили.
    Changed { expected: String, actual: String },
}

/// Сверить бинарь ядра с запомненным отпечатком.
///
/// Ошибку возвращает только когда файл не прочитать: «не знаю» и «не совпало» —
/// разные вещи, и решать, что с ними делать, вызывающему.
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

/// Сверка перед запуском ядра С ПРАВАМИ.
///
/// Здесь расхождение — отказ. Ядро под службой получает SYSTEM, и «наверное,
/// это антивирус переписал файл» не тот случай, когда стоит рискнуть: если файл
/// действительно законно другой, пользователь переустановит приложение и
/// отпечаток снимется заново.
pub async fn ensure_elevated_binary_is_known(path: &Path) -> Result<()> {
    match check_binary(path).await {
        Ok(PinCheck::Match) => Ok(()),
        Ok(PinCheck::Recorded) => {
            logging!(info, Type::Core, "recorded core digest for {path:?}");
            Ok(())
        }
        Ok(PinCheck::Changed { expected, actual }) => {
            logging!(
                error,
                Type::Core,
                "core binary at {path:?} changed since it was pinned: expected {expected}, got {actual}"
            );
            crate::core::handle::Handle::notice_message("core::binary_changed", path.to_string_lossy().into_owned());
            bail!("core binary changed since installation, refusing to start it with system privileges")
        }
        Err(err) => {
            // Файл не прочитать — запускать его тем более незачем.
            logging!(error, Type::Core, "failed to verify core binary {path:?}: {err:#}");
            Err(err)
        }
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::{digest_of, digest_of_bytes};

    #[tokio::test]
    async fn file_and_bytes_agree_on_the_digest() {
        let dir = std::env::temp_dir().join(format!("clod-core-pin-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let file = dir.join("core-binary");
        let payload = b"not really a core, but hashes the same way";
        std::fs::write(&file, payload).expect("write");

        let from_file = digest_of(&file).await.expect("digest");
        assert_eq!(from_file, digest_of_bytes(payload));
        // Известный вектор: пустой вход даёт канонический хеш SHA-256.
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
}
