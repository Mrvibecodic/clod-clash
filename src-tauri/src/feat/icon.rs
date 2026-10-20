use crate::{
    cmd::{CmdResult, StringifyErr as _},
    utils::{
        dirs::{self, PathBufExec as _},
        network::ProxyType,
        public_url::{MAX_IMAGE_BYTES, read_image, request_image},
        retry::try_strategies,
    },
};
use clash_verge_logging::{Type, logging};
use smartstring::alias::String;
use std::path::{Component, Path, PathBuf};
use tokio::fs;
use tokio::io::AsyncWriteExt as _;

fn normalize_icon_segment(name: &str) -> CmdResult<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed.contains('/') || trimmed.contains('\\') || trimmed.contains("..") {
        return Err("invalid icon cache file name".into());
    }

    let mut components = Path::new(trimmed).components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(_)), None) => Ok(trimmed.into()),
        _ => Err("invalid icon cache file name".into()),
    }
}

fn ensure_icon_cache_target(icon_cache_dir: &Path, file_name: &str) -> CmdResult<PathBuf> {
    let icon_path = icon_cache_dir.join(file_name);
    let is_direct_child =
        icon_path.parent().is_some_and(|parent| parent == icon_cache_dir) && icon_path.starts_with(icon_cache_dir);

    if !is_direct_child {
        return Err("invalid icon cache file name".into());
    }

    Ok(icon_path)
}

async fn fetch_icon(url: &str) -> anyhow::Result<Vec<u8>> {
    try_strategies(ProxyType::Localhost, [ProxyType::System], |proxy| async move {
        let response = request_image(url, proxy, reqwest::header::HeaderMap::new())
            .await?
            .error_for_status()?;
        Ok(read_image(response, MAX_IMAGE_BYTES).await?.0)
    })
    .await
}

pub async fn download_icon_cache(url: String, name: String) -> CmdResult<String> {
    let icon_cache_dir = dirs::app_home_dir().stringify_err()?.join("icons").join("cache");
    let icon_name = normalize_icon_segment(name.as_str())?;
    let icon_path = ensure_icon_cache_target(&icon_cache_dir, icon_name.as_str())?;

    if icon_path.exists() {
        return Ok(icon_path.to_string_lossy().into());
    }

    if !icon_cache_dir.exists() {
        fs::create_dir_all(&icon_cache_dir).await.stringify_err()?;
    }

    let temp_name = format!("{icon_name}.downloading");
    let temp_path = ensure_icon_cache_target(&icon_cache_dir, temp_name.as_str())?;

    let content = fetch_icon(url.as_str()).await.stringify_err()?;

    {
        let mut file = match fs::File::create(&temp_path).await {
            Ok(file) => file,
            Err(_) => {
                if icon_path.exists() {
                    return Ok(icon_path.to_string_lossy().into());
                }
                return Err("Failed to create temporary file".into());
            }
        };
        file.write_all(content.as_ref()).await.stringify_err()?;
        file.flush().await.stringify_err()?;
    }

    if !icon_path.exists() {
        match fs::rename(&temp_path, &icon_path).await {
            Ok(_) => {}
            Err(_) => {
                let _ = temp_path.remove_if_exists().await;
                if icon_path.exists() {
                    return Ok(icon_path.to_string_lossy().into());
                }
            }
        }
    } else {
        let _ = temp_path.remove_if_exists().await;
    }

    Ok(icon_path.to_string_lossy().into())
}

fn icon_stamp<'a>(path: &'a Path, icon_name: &str) -> &'a str {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .and_then(|stem| stem.strip_prefix(icon_name))
        .map_or("", |rest| rest.strip_prefix('-').unwrap_or(rest))
}

/// Расширения, которые умеет найти `stored_icons`, — и потому единственные,
/// под которыми имеет смысл сохранять выбранный файл.
const ICON_EXTENSIONS: [&str; 2] = ["ico", "png"];

fn supported_icon_extension(path: &Path) -> Option<&'static str> {
    let ext = path.extension().and_then(|ext| ext.to_str())?;
    ICON_EXTENSIONS
        .into_iter()
        .find(|supported| ext.eq_ignore_ascii_case(supported))
}

async fn stored_icons(icon_dir: &Path, icon_name: &str) -> Vec<PathBuf> {
    let Ok(mut entries) = fs::read_dir(icon_dir).await else {
        return Vec::new();
    };
    let mut found = Vec::new();
    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        let matches_name = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .is_some_and(|stem| stem == icon_name || stem.starts_with(&format!("{icon_name}-")));
        let matches_ext = supported_icon_extension(&path).is_some();
        if matches_name && matches_ext {
            found.push(path);
        }
    }
    found.sort_by(|left, right| icon_stamp(left, icon_name).cmp(icon_stamp(right, icon_name)));
    found
}

pub async fn tray_icon_path(name: &str) -> CmdResult<String> {
    let icon_name = normalize_icon_segment(name)?;
    let icon_dir = dirs::app_icons_dir().stringify_err()?;
    Ok(stored_icons(&icon_dir, icon_name.as_str())
        .await
        .pop()
        .map(|path| path.to_string_lossy().into())
        .unwrap_or_default())
}

async fn remove_other_icons(icon_dir: &Path, icon_name: &str, keep: &Path) {
    for path in stored_icons(icon_dir, icon_name).await {
        if path == keep {
            continue;
        }
        path.remove_if_exists().await.unwrap_or_default();
    }
}

pub async fn copy_icon_file(path: String, name: String) -> CmdResult<String> {
    let file_path = Path::new(path.as_str());
    let icon_name = normalize_icon_segment(name.as_str())?;

    if !file_path.exists() {
        return Err("file not found".into());
    }

    let icon_dir = dirs::app_icons_dir().stringify_err()?;
    if !icon_dir.exists() {
        fs::create_dir_all(&icon_dir).await.stringify_err()?;
    }

    let Some(ext) = supported_icon_extension(file_path) else {
        return Err(format!(
            "clod-icon-rejected: icon file must be one of: {}",
            ICON_EXTENSIONS.join(", ")
        )
        .into());
    };

    let content = fs::read(file_path).await.stringify_err()?;
    if tauri::image::Image::from_bytes(&content).is_err() {
        return Err("clod-icon-rejected: the picked file is not a readable icon".into());
    }

    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_millis());
    let dest_file_name = format!("{icon_name}-{stamp}.{ext}");
    let dest_path = ensure_icon_cache_target(&icon_dir, dest_file_name.as_str())?;

    logging!(
        info,
        Type::Cmd,
        "Copying icon file path: {:?} -> file dist: {:?}",
        path,
        dest_path
    );

    crate::utils::help::write_atomic(&dest_path, &content)
        .await
        .map_err(|error| crate::cmd::public_error_text(&format!("{error:#}")))?;
    remove_other_icons(&icon_dir, icon_name.as_str(), &dest_path).await;
    Ok(dest_path.to_string_lossy().into())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn an_unfinished_icon_write_never_becomes_the_tray_icon() {
        let dir = std::env::temp_dir().join(format!("clod-icon-staging-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let old = dir.join("common-1000.png");
        std::fs::write(&old, b"old").unwrap();
        let staging = crate::utils::help::staging_path(&dir.join("common-2000.png"));
        std::fs::write(&staging, b"half").unwrap();

        let found = stored_icons(&dir, "common").await;
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(found, vec![old]);
    }

    #[test]
    fn normalize_icon_segment_accepts_single_name() {
        assert!(normalize_icon_segment("group-icon.png").is_ok());
        assert!(normalize_icon_segment("alpha_1.webp").is_ok());
    }

    #[test]
    fn normalize_icon_segment_rejects_traversal_and_separators() {
        for name in ["../x", "..\\x", "a/b", "a\\b", "..", "a..b"] {
            assert!(normalize_icon_segment(name).is_err(), "name should be rejected: {name}");
        }
    }

    #[test]
    fn normalize_icon_segment_rejects_empty() {
        assert!(normalize_icon_segment("").is_err());
        assert!(normalize_icon_segment("   ").is_err());
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn normalize_icon_segment_rejects_windows_absolute_names() {
        for name in [r"C:\temp\icon.png", r"\\server\share\icon.png"] {
            assert!(normalize_icon_segment(name).is_err(), "name should be rejected: {name}");
        }
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn normalize_icon_segment_rejects_unix_absolute_names() {
        assert!(normalize_icon_segment("/tmp/icon.png").is_err());
    }

    #[test]
    fn ensure_icon_cache_target_accepts_direct_child_only() {
        let base = PathBuf::from("icons").join("cache");
        let valid = ensure_icon_cache_target(&base, "ok.png");
        assert_eq!(valid.unwrap(), base.join("ok.png"));

        let nested = base.join("nested").join("ok.png");
        assert!(ensure_icon_cache_target(&base, nested.to_string_lossy().as_ref()).is_err());
        assert!(ensure_icon_cache_target(&base, "../ok.png").is_err());
    }
}
