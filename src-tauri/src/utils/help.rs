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

pub async fn read_yaml<T: DeserializeOwned>(path: &PathBuf) -> Result<T> {
    if !tokio::fs::try_exists(path).await.unwrap_or(false) {
        bail!("file not found \"{}\"", path.display());
    }

    let yaml_str = tokio::fs::read_to_string(path).await?;

    Ok(with_encryption(|| async { serde_yaml_ng::from_str::<T>(&yaml_str) }).await?)
}

pub async fn read_mapping(path: &PathBuf) -> Result<Mapping> {
    if !tokio::fs::try_exists(path).await.unwrap_or(false) {
        bail!("file not found \"{}\"", path.display());
    }

    let yaml_str = tokio::fs::read_to_string(path)
        .await
        .with_context(|| format!("failed to read the file \"{}\"", path.display()))?;

    match serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&yaml_str) {
        Ok(mut val) => {
            apply_merge_transitively(&mut val, &path.display().to_string())
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

const MAX_MERGE_PASSES: usize = 16;

fn contains_merge_key(root: &Value) -> bool {
    let mut stack = vec![root];

    while let Some(node) = stack.pop() {
        match node {
            Value::Mapping(map) => {
                if map.contains_key("<<") {
                    return true;
                }
                stack.extend(map.values());
            }
            Value::Sequence(items) => stack.extend(items.iter()),
            Value::Tagged(tagged) => stack.push(&tagged.value),
            _ => {}
        }
    }

    false
}

fn apply_merge_transitively(value: &mut Value, source: &str) -> Result<()> {
    for _ in 0..MAX_MERGE_PASSES {
        value.apply_merge()?;
        if !contains_merge_key(value) {
            return Ok(());
        }
    }

    logging!(
        warn,
        Type::Config,
        "YAML merge keys in {} are still not resolved after {} passes",
        source,
        MAX_MERGE_PASSES
    );

    Ok(())
}

pub async fn read_seq_map(path: &PathBuf) -> Result<SeqMap> {
    read_yaml(path).await
}

pub async fn save_yaml<T: Serialize + Sync>(path: &Path, data: &T, prefix: Option<&str>) -> Result<()> {
    let data_str = with_encryption(|| async { serde_yaml_ng::to_string(data) }).await?;

    let yaml_str = match prefix {
        Some(prefix) => format!("{prefix}\n\n{data_str}"),
        None => data_str,
    };

    write_atomic(path, yaml_str.as_bytes()).await?;
    Ok(())
}

const ATOMIC_RENAME_ATTEMPTS: usize = 4;
const ATOMIC_RENAME_PAUSE: std::time::Duration = std::time::Duration::from_millis(50);

/// Подтвердить переименование на диске.
///
/// Само содержимое мы уже подтвердили `sync_all`, но запись в каталоге, которая
/// делает новый файл видимым под старым именем, живёт отдельно. Без этого шага
/// внезапное отключение питания могло вернуть старое имя без файла. На Windows
/// каталог открыть нельзя, поэтому шаг только для unix; ошибку не поднимаем —
/// файл уже на месте, и падать из-за неподтверждённого каталога незачем.
#[cfg(unix)]
async fn sync_parent_directory(path: &Path) {
    let Some(parent) = path.parent() else { return };
    if let Ok(dir) = tokio::fs::File::open(parent).await {
        let _ = dir.sync_all().await;
    }
}

#[cfg(not(unix))]
fn sync_parent_directory(_path: &Path) -> impl std::future::Future<Output = ()> {
    std::future::ready(())
}

pub async fn write_atomic(path: &Path, contents: &[u8]) -> Result<()> {
    let staging = staging_path(path);

    let write_result = async {
        use tokio::io::AsyncWriteExt as _;
        let mut file = tokio::fs::File::create(&staging).await?;
        file.write_all(contents).await?;
        file.sync_all().await
    }
    .await;

    if let Err(err) = write_result {
        let _ = tokio::fs::remove_file(&staging).await;
        return Err(anyhow!(err)).with_context(|| format!("failed to write file \"{}\"", path.display()));
    }

    #[cfg(unix)]
    if let Ok(existing) = tokio::fs::metadata(path).await {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = existing.permissions().mode();
        let _ = tokio::fs::set_permissions(&staging, std::fs::Permissions::from_mode(mode)).await;
    }

    let mut last_error = None;
    for attempt in 0..ATOMIC_RENAME_ATTEMPTS {
        match tokio::fs::rename(&staging, path).await {
            Ok(()) => {
                sync_parent_directory(path).await;
                return Ok(());
            }
            Err(err) => {
                last_error = Some(err);
                if attempt + 1 < ATOMIC_RENAME_ATTEMPTS {
                    tokio::time::sleep(ATOMIC_RENAME_PAUSE).await;
                }
            }
        }
    }

    let _ = tokio::fs::remove_file(&staging).await;
    let last_error = last_error.unwrap_or_else(|| std::io::Error::other("rename was never attempted"));
    Err(anyhow!(last_error)).with_context(|| format!("failed to move file into place \"{}\"", path.display()))
}

fn is_staging_leftover(name: &str) -> bool {
    let Some(stem) = name.strip_prefix('.').and_then(|rest| rest.strip_suffix(".tmp")) else {
        return false;
    };
    stem.rsplit_once('.')
        .is_some_and(|(base, id)| !base.is_empty() && id.len() == 8 && id.chars().all(|c| ALPHABET.contains(&c)))
}

pub async fn sweep_staging_leftovers(dir: &Path) -> usize {
    let Ok(mut entries) = tokio::fs::read_dir(dir).await else {
        return 0;
    };
    let mut removed = 0;
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name().to_string_lossy().into_owned();
        if is_staging_leftover(&name) && tokio::fs::remove_file(entry.path()).await.is_ok() {
            removed += 1;
        }
    }
    removed
}

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

pub fn get_uid(prefix: &str) -> String {
    let id = nanoid!(11, &ALPHABET);
    format!("{prefix}{id}")
}

pub const LEGACY_DEFAULT_SECRET: &str = "set-your-secret";

pub fn random_secret() -> String {
    nanoid!(32, &ALPHABET)
}

pub fn is_placeholder_secret(secret: &str) -> bool {
    secret.trim().is_empty() || secret == LEGACY_DEFAULT_SECRET
}

pub fn parse_str<T: FromStr>(target: &str, key: &str) -> Option<T> {
    target.split(';').map(str::trim).find_map(|s| {
        let mut parts = s.splitn(2, '=');
        match (parts.next(), parts.next()) {
            (Some(k), Some(v)) if k == key => v.parse::<T>().ok(),
            _ => None,
        }
    })
}

pub fn mask_url(url: &str) -> String {
    let (path_part, query_part) = match url.find('?') {
        Some(pos) => (&url[..pos], Some(&url[pos + 1..])),
        None => (url, None),
    };

    let host_end = path_part
        .find("://")
        .and_then(|scheme_end| {
            path_part[scheme_end + 3..]
                .find('/')
                .map(|slash| scheme_end + 3 + slash)
        })
        .unwrap_or(path_part.len());

    let scheme_and_host = &path_part[..host_end];
    let path = &path_part[host_end..];

    let mut result = scheme_and_host.to_owned();

    if !path.is_empty() {
        let masked: Vec<&str> = path
            .split('/')
            .map(|seg| if seg.len() > 16 { "***" } else { seg })
            .collect();
        result.push_str(&masked.join("/"));
    }

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

pub fn get_last_part_and_decode(url: &str) -> Option<String> {
    let path = url.split('?').next().unwrap_or("");
    let segments: Vec<&str> = path.split('/').collect();
    let last_segment = segments.last()?;

    Some(
        percent_encoding::percent_decode_str(last_segment)
            .decode_utf8_lossy()
            .to_string(),
    )
}

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
pub fn linux_elevator() -> Result<String> {
    use std::process::Command;
    let output = Command::new("which").arg("pkexec").output();
    let path = output
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|path| path.trim().to_string())
        .filter(|path| !path.is_empty());
    path.ok_or_else(|| {
        anyhow::anyhow!(
            "pkexec (polkit) is not installed, so the app cannot ask for administrator rights; install polkit or run \
             `sudo /usr/bin/clash-verge-service-install` once"
        )
    })
}

#[cfg(target_os = "windows")]
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

/// Какую из двух причин отказа показать пользователю.
///
/// Названная причина всегда лучше безымянной: `clod-sub-link-list` объясняет, что
/// делать («попросите в панели правило `^ClodClash/`»), а «сбой сети на последней
/// ступени» не объясняет ничего. Отказ по бюджету — самый бедный диагноз из всех: он
/// говорит только, что времени не хватило, и настоящую причину не вытесняет никогда.
pub fn keep_the_clearer_error(previous: anyhow::Error, fresh: anyhow::Error) -> anyhow::Error {
    let names_a_reason = |text: &str| text.contains("clod-sub-") && !text.contains("clod-sub-budget");
    let (previous_text, fresh_text) = (previous.to_string(), fresh.to_string());

    if fresh_text.contains("clod-sub-budget") {
        return previous;
    }

    if !names_a_reason(&fresh_text) && names_a_reason(&previous_text) {
        return previous;
    }

    fresh
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::{
        apply_merge_transitively, contains_merge_key, is_placeholder_secret, is_staging_leftover, random_secret,
        staging_path, sweep_staging_leftovers, write_atomic,
    };
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

    #[test]
    fn only_our_drafts_count_as_leftovers() {
        assert!(is_staging_leftover(".verge.yaml.aB3dE9xZ.tmp"));
        assert!(!is_staging_leftover("verge.yaml"));
        assert!(!is_staging_leftover(".hidden.tmp"));
        assert!(!is_staging_leftover(".verge.yaml.short.tmp"));
        assert!(!is_staging_leftover("notes.aB3dE9xZ.tmp"));
    }

    #[tokio::test]
    async fn sweep_removes_drafts_and_keeps_everything_else() {
        let dir = scratch_dir("sweep");
        std::fs::write(dir.join(".verge.yaml.aB3dE9xZ.tmp"), b"x").expect("draft");
        std::fs::write(dir.join("verge.yaml"), b"y").expect("config");
        std::fs::write(dir.join("keep.tmp"), b"z").expect("foreign tmp");

        assert_eq!(sweep_staging_leftovers(&dir).await, 1);
        assert!(!dir.join(".verge.yaml.aB3dE9xZ.tmp").exists());
        assert!(dir.join("verge.yaml").exists());
        assert!(dir.join("keep.tmp").exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn atomic_write_keeps_the_old_file_when_it_cannot_write() {
        let dir = scratch_dir("atomic-fail");
        let target = dir.join("nested").join("verge.yaml");

        write_atomic(&target, b"payload").await.expect_err("no directory");
        assert!(!target.exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn draft_lives_next_to_the_target() {
        let target = PathBuf::from("/var/lib/clod/verge.yaml");
        let draft = staging_path(&target);
        assert_eq!(draft.parent(), target.parent());
        assert_ne!(draft.file_name(), target.file_name());

        assert_ne!(staging_path(&target), staging_path(&target));
    }

    #[test]
    fn generated_secret_is_not_a_placeholder() {
        assert!(is_placeholder_secret(""));
        assert!(is_placeholder_secret("set-your-secret"));
        assert!(!is_placeholder_secret(&random_secret()));
    }

    #[test]
    fn nested_anchors_are_expanded_for_providers() {
        let mut doc = serde_yaml_ng::from_str::<serde_yaml_ng::Value>(
            r#"
x-anchors:
  pr_http: &pr_http
    type: http
    interval: 86400
  rp_domain: &rp_domain
    <<: *pr_http
    behavior: domain
    proxy: "VPN"
proxy-providers:
  ru-bundle:
    <<: *rp_domain
    url: https://example.net/ru.mrs
    interval: 3600
"#,
        )
        .expect("valid yaml");

        apply_merge_transitively(&mut doc, "test").expect("merge should expand");

        assert!(!contains_merge_key(&doc), "no merge key must survive");

        let provider = doc
            .get("proxy-providers")
            .and_then(|value| value.get("ru-bundle"))
            .and_then(serde_yaml_ng::Value::as_mapping)
            .expect("provider mapping");

        assert_eq!(
            provider.get("type").and_then(serde_yaml_ng::Value::as_str),
            Some("http")
        );
        assert_eq!(
            provider.get("behavior").and_then(serde_yaml_ng::Value::as_str),
            Some("domain")
        );
        assert_eq!(
            provider.get("interval").and_then(serde_yaml_ng::Value::as_u64),
            Some(3600),
            "an explicit key must win over the one inherited from an anchor"
        );
    }
}
