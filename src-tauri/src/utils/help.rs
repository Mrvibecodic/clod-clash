use crate::{config::with_encryption, enhance::seq::SeqMap};
use anyhow::{Context as _, Result, anyhow, bail};
use clash_verge_logging::{Type, logging};
use nanoid::nanoid;
use serde::{Serialize, de::DeserializeOwned};
use serde_yaml_ng::{Mapping, Value};
use std::{
    path::{Path, PathBuf},
    str::FromStr,
};

/// read data from yaml as struct T
pub async fn read_yaml<T: DeserializeOwned>(path: &PathBuf) -> Result<T> {
    if !tokio::fs::try_exists(path).await.unwrap_or(false) {
        bail!("file not found \"{}\"", path.display());
    }

    let yaml_str = tokio::fs::read_to_string(path).await?;

    Ok(with_encryption(|| async { serde_yaml_ng::from_str::<T>(&yaml_str) }).await?)
}

/// read mapping from yaml
pub async fn read_mapping(path: &PathBuf) -> Result<Mapping> {
    if !tokio::fs::try_exists(path).await.unwrap_or(false) {
        bail!("file not found \"{}\"", path.display());
    }

    let yaml_str = tokio::fs::read_to_string(path)
        .await
        .with_context(|| format!("failed to read the file \"{}\"", path.display()))?;

    // Проверка синтаксиса YAML
    match serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&yaml_str) {
        Ok(mut val) => {
            val.apply_merge()
                .with_context(|| format!("failed to apply merge \"{}\"", path.display()))?;

            match val {
                Value::Mapping(map) => Ok(map),
                _ => Err(anyhow!("failed to transform to yaml mapping \"{}\"", path.display())),
            }
        }
        Err(err) => {
            let error_msg = format!("YAML syntax error in {}: {}", path.display(), err);
            logging!(error, Type::Config, "{}", error_msg);

            crate::core::handle::Handle::notice_message("config_validate::yaml_syntax_error", &error_msg);

            bail!("YAML syntax error: {}", err)
        }
    }
}

/// read mapping from yaml fix #165
pub async fn read_seq_map(path: &PathBuf) -> Result<SeqMap> {
    read_yaml(path).await
}

/// save the data to the file
/// can set `prefix` string to add some comments
pub async fn save_yaml<T: Serialize + Sync>(path: &Path, data: &T, prefix: Option<&str>) -> Result<()> {
    let data_str = with_encryption(|| async { serde_yaml_ng::to_string(data) }).await?;

    let yaml_str = match prefix {
        Some(prefix) => format!("{prefix}\n\n{data_str}"),
        None => data_str,
    };

    write_atomic(path, yaml_str.as_bytes()).await?;
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    Ok(())
}

/// Сколько раз пробуем переименовать файл на место.
///
/// Windows умеет отдать «отказано в доступе», пока свежесозданный файл держит
/// антивирус или индексатор. Это проходит за доли секунды, и ронять из-за
/// этого сохранение профиля нельзя.
const ATOMIC_RENAME_ATTEMPTS: usize = 4;
const ATOMIC_RENAME_PAUSE: std::time::Duration = std::time::Duration::from_millis(50);

/// clod: записать файл целиком или не записать вовсе.
///
/// Прямой `fs::write` усекает файл ДО того, как в него попадут новые байты:
/// сбой питания, вылет процесса или переполненный диск в этот момент оставляли
/// на диске обрезанный конфиг или профиль. Для профиля это потерянная подписка,
/// для `verge.yaml` — сброшенные настройки, и оба чинятся только руками.
///
/// Пишем в соседний временный файл, сбрасываем его на диск и переименовываем:
/// подмена имени атомарна и на POSIX, и на Windows (`MoveFileEx` с заменой),
/// так что читатель видит либо старое содержимое целиком, либо новое целиком.
///
/// Имя временного файла содержит случайную часть — два сохранения одного и
/// того же пути не отберут друг у друга черновик.
pub async fn write_atomic(path: &Path, contents: &[u8]) -> Result<()> {
    let staging = staging_path(path);

    let write_result = async {
        tokio::fs::write(&staging, contents).await?;
        // Без сброса на диск переименование может опередить сами байты, и
        // после выключения питания на месте окажется файл нулевой длины.
        tokio::fs::File::open(&staging).await?.sync_all().await
    }
    .await;

    if let Err(err) = write_result {
        let _ = tokio::fs::remove_file(&staging).await;
        return Err(anyhow!(err)).with_context(|| format!("failed to write file \"{}\"", path.display()));
    }

    // Перезапись оставляла файлу его собственные права, подмена имени — нет.
    // Профиль хранит адрес подписки, и отдавать его остальным пользователям
    // машины только потому, что мы сменили способ записи, нельзя.
    #[cfg(unix)]
    if let Ok(existing) = tokio::fs::metadata(path).await {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = existing.permissions().mode();
        let _ = tokio::fs::set_permissions(&staging, std::fs::Permissions::from_mode(mode)).await;
    }

    let mut last_error = None;
    for attempt in 0..ATOMIC_RENAME_ATTEMPTS {
        match tokio::fs::rename(&staging, path).await {
            Ok(()) => return Ok(()),
            Err(err) => {
                last_error = Some(err);
                if attempt + 1 < ATOMIC_RENAME_ATTEMPTS {
                    tokio::time::sleep(ATOMIC_RENAME_PAUSE).await;
                }
            }
        }
    }

    // Черновик не должен копиться рядом с настоящим файлом.
    let _ = tokio::fs::remove_file(&staging).await;
    let last_error = last_error.unwrap_or_else(|| std::io::Error::other("rename was never attempted"));
    Err(anyhow!(last_error)).with_context(|| format!("failed to move file into place \"{}\"", path.display()))
}

/// Имя черновика рядом с целевым файлом — на том же томе, иначе переименование
/// превратится в копирование и перестанет быть атомарным.
fn staging_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "config".into());
    let staging = format!(".{name}.{}.tmp", nanoid!(8, &ALPHABET));

    match path.parent() {
        Some(parent) => parent.join(staging),
        None => PathBuf::from(staging),
    }
}

const ALPHABET: [char; 62] = [
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm',
    'n', 'o', 'p', 'q', 'r', 's', 't', 'u', 'v', 'w', 'x', 'y', 'z', 'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'I', 'J',
    'K', 'L', 'M', 'N', 'O', 'P', 'Q', 'R', 'S', 'T', 'U', 'V', 'W', 'X', 'Y', 'Z',
];

/// generate the uid
pub fn get_uid(prefix: &str) -> String {
    let id = nanoid!(11, &ALPHABET);
    format!("{prefix}{id}")
}

/// Значение, которое апстрим писал в шаблон конфига ядра.
pub const LEGACY_DEFAULT_SECRET: &str = "set-your-secret";

/// clod: секрет управляющего интерфейса ядра.
///
/// Апстрим клал в шаблон строку `set-your-secret` — одну и ту же у всех
/// установок. Управляющий интерфейс отдаёт конфиг с адресами серверов, меняет
/// режим и показывает соединения, а CORS в шаблоне пускает несколько внешних
/// панелей, так что общеизвестный секрет здесь не косметика.
///
/// 32 символа из алфавита uid — около 190 бит, перебирать нечего.
pub fn random_secret() -> String {
    nanoid!(32, &ALPHABET)
}

/// Секрет из апстрима и пустая строка равнозначны «не задан».
pub fn is_placeholder_secret(secret: &str) -> bool {
    secret.trim().is_empty() || secret == LEGACY_DEFAULT_SECRET
}

/// parse the string
/// xxx=123123; => 123123
pub fn parse_str<T: FromStr>(target: &str, key: &str) -> Option<T> {
    target.split(';').map(str::trim).find_map(|s| {
        let mut parts = s.splitn(2, '=');
        match (parts.next(), parts.next()) {
            (Some(k), Some(v)) if k == key => v.parse::<T>().ok(),
            _ => None,
        }
    })
}

/// Mask sensitive parts of a subscription URL for safe logging.
/// Examples:
/// - `https://example.com/api/v1/clash?token=abc123` → `https://example.com/api/v1/clash?token=***`
/// - `https://example.com/abc123def456ghi789/clash` → `https://example.com/***/clash`
pub fn mask_url(url: &str) -> String {
    // Split off query string
    let (path_part, query_part) = match url.find('?') {
        Some(pos) => (&url[..pos], Some(&url[pos + 1..])),
        None => (url, None),
    };

    // Extract scheme+host prefix (everything up to the first '/' after "://")
    let host_end = path_part
        .find("://")
        .and_then(|scheme_end| {
            path_part[scheme_end + 3..]
                .find('/')
                .map(|slash| scheme_end + 3 + slash)
        })
        .unwrap_or(path_part.len());

    let scheme_and_host = &path_part[..host_end];
    let path = &path_part[host_end..]; // starts with '/' or empty

    let mut result = scheme_and_host.to_owned();

    // Mask path segments that look like tokens (longer than 16 chars)
    if !path.is_empty() {
        let masked: Vec<&str> = path
            .split('/')
            .map(|seg| if seg.len() > 16 { "***" } else { seg })
            .collect();
        result.push_str(&masked.join("/"));
    }

    // Keep query param keys, mask values
    if let Some(query) = query_part {
        result.push('?');
        let masked_query: Vec<String> = query
            .split('&')
            .map(|param| match param.find('=') {
                Some(eq) => format!("{}=***", &param[..eq]),
                None => param.to_owned(),
            })
            .collect();
        result.push_str(&masked_query.join("&"));
    }

    result
}

/// Mask all URLs embedded in an error/log string for safe logging.
///
/// Scans the string for `http://` or `https://` and replaces each URL
/// (terminated by whitespace or `)`, `]`, `"`, `'`) with its masked form.
/// Text between URLs is copied verbatim.
pub fn mask_err(err: &str) -> String {
    let mut result = String::with_capacity(err.len());
    let mut remaining = err;

    loop {
        let http = remaining.find("http://");
        let https = remaining.find("https://");
        let start = match (http, https) {
            (None, None) => {
                result.push_str(remaining);
                break;
            }
            (Some(a), None) | (None, Some(a)) => a,
            (Some(a), Some(b)) => a.min(b),
        };

        result.push_str(&remaining[..start]);
        remaining = &remaining[start..];

        let url_end = remaining
            .find(|c: char| c.is_whitespace() || matches!(c, ')' | ']' | '"' | '\''))
            .unwrap_or(remaining.len());

        result.push_str(&mask_url(&remaining[..url_end]));
        remaining = &remaining[url_end..];
    }

    result
}

/// get the last part of the url, if not found, return empty string
pub fn get_last_part_and_decode(url: &str) -> Option<String> {
    let path = url.split('?').next().unwrap_or(""); // Splits URL and takes the path part
    let segments: Vec<&str> = path.split('/').collect();
    let last_segment = segments.last()?;

    Some(
        percent_encoding::percent_decode_str(last_segment)
            .decode_utf8_lossy()
            .to_string(),
    )
}

/// open file
pub fn open_file(path: PathBuf) -> Result<()> {
    open::that_detached(path.as_os_str())?;
    Ok(())
}

pub fn open_latest_log(path: PathBuf) -> Result<()> {
    #[cfg(target_os = "windows")]
    let path = snapshot_path(&path)?;
    open_file(path)
}

pub fn open_app_latest_log() -> Result<()> {
    let path = crate::utils::dirs::app_latest_log()?;
    open_latest_log(path)
}

pub fn open_core_latest_log() -> Result<()> {
    let path = crate::utils::dirs::clash_latest_log()?;
    open_latest_log(path)
}

#[cfg(target_os = "linux")]
pub fn linux_elevator() -> String {
    use std::process::Command;
    match Command::new("which").arg("pkexec").output() {
        Ok(output) => {
            if !output.stdout.is_empty() {
                // Convert the output to a string slice
                if let Ok(path) = std::str::from_utf8(&output.stdout) {
                    path.trim().to_string()
                } else {
                    "sudo".to_string()
                }
            } else {
                "sudo".to_string()
            }
        }
        Err(_) => "sudo".to_string(),
    }
}

#[cfg(target_os = "windows")]
/// copy the file to the dist path and return the dist path
pub fn snapshot_path(original_path: &Path) -> Result<PathBuf> {
    let temp_dir = original_path
        .parent()
        .ok_or_else(|| anyhow!("Invalid log path"))?
        .join("temp");

    std::fs::create_dir_all(&temp_dir)?;

    let temp_path = temp_dir.join(format!(
        "{}_{}.log",
        original_path.file_stem().unwrap_or_default().to_string_lossy(),
        chrono::Local::now().format("%Y-%m-%d_%H-%M-%S")
    ));

    std::fs::copy(original_path, &temp_path)?;

    Ok(temp_path)
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::{is_placeholder_secret, random_secret, staging_path, write_atomic};
    use std::path::PathBuf;

    fn scratch_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("clod-help-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    #[tokio::test]
    async fn atomic_write_replaces_content_and_leaves_no_drafts() {
        let dir = scratch_dir("atomic");
        let target = dir.join("verge.yaml");

        write_atomic(&target, b"first").await.expect("first write");
        assert_eq!(std::fs::read(&target).expect("read"), b"first");

        // Второй заход не дописывает и не оставляет обрезка от прошлого
        // содержимого: длина короче, но хвост «rst» пережить не должен.
        write_atomic(&target, b"two").await.expect("second write");
        assert_eq!(std::fs::read(&target).expect("read"), b"two");

        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .expect("list")
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "черновики остались: {leftovers:?}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn atomic_write_keeps_the_old_file_when_it_cannot_write() {
        let dir = scratch_dir("atomic-fail");
        let target = dir.join("nested").join("verge.yaml");

        // Каталога нет — записать некуда, и это должно быть ошибкой, а не
        // молчаливой потерей файла.
        write_atomic(&target, b"payload").await.expect_err("no directory");
        assert!(!target.exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn draft_lives_next_to_the_target() {
        // Переименование атомарно только в пределах тома, поэтому черновик
        // обязан лежать в том же каталоге.
        let target = PathBuf::from("/var/lib/clod/verge.yaml");
        let draft = staging_path(&target);
        assert_eq!(draft.parent(), target.parent());
        assert_ne!(draft.file_name(), target.file_name());

        // Два черновика одного файла не совпадают — параллельные сохранения
        // не отберут друг у друга временное имя.
        assert_ne!(staging_path(&target), staging_path(&target));
    }

    #[test]
    fn generated_secret_is_not_a_placeholder() {
        assert!(is_placeholder_secret(""));
        assert!(is_placeholder_secret("set-your-secret"));
        assert!(!is_placeholder_secret(&random_secret()));
    }
}
