use crate::utils::{
    dirs,
    redact::{home_prefix, redact, scrub_home},
};
use anyhow::{Context as _, Result};
use std::{
    io::Write as _,
    path::{Path, PathBuf},
};
use zip::write::SimpleFileOptions;

const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;

fn is_log_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("log"))
}

fn collect_logs(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_logs(&path, out);
        } else if is_log_file(&path) {
            out.push(path);
        }
    }
}

fn redacted(content: &str, home: Option<&str>) -> String {
    let mut text = String::with_capacity(content.len());
    for line in content.lines() {
        text.push_str(&redact(&scrub_home(line, home)));
        text.push('\n');
    }
    text
}

fn archive_name(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .filter_map(|part| match part {
            std::path::Component::Normal(name) => Some(name.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

pub async fn write_archive(target: PathBuf) -> Result<usize> {
    let report = crate::module::support_bundle::build(None).await?;
    let app_logs = dirs::app_logs_dir()?;
    let service_logs = dirs::service_logs_root_dir()?;

    tokio::task::spawn_blocking(move || {
        let home = home_prefix();
        let mut files = Vec::new();
        collect_logs(&app_logs, &mut files);
        if service_logs != app_logs {
            collect_logs(&service_logs, &mut files);
        }
        files.sort();
        files.dedup();

        let file = std::fs::File::create(&target).with_context(|| format!("cannot create {}", target.display()))?;
        let mut zip = zip::ZipWriter::new(file);
        let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

        zip.start_file("report.md", options)?;
        zip.write_all(report.as_bytes())?;

        let mut count = 0usize;
        for path in files {
            let Ok(meta) = std::fs::metadata(&path) else {
                continue;
            };
            if meta.len() > MAX_FILE_BYTES {
                continue;
            }
            let Ok(raw) = std::fs::read(&path) else {
                continue;
            };
            let content = String::from_utf8_lossy(&raw);
            let root = if path.starts_with(&app_logs) {
                &app_logs
            } else {
                &service_logs
            };
            zip.start_file(archive_name(root, &path), options)?;
            zip.write_all(redacted(&content, home.as_deref()).as_bytes())?;
            count += 1;
        }
        zip.finish()?;
        Ok(count)
    })
    .await?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archive_names_are_relative_with_forward_slashes() {
        let root = Path::new("/logs");
        assert_eq!(archive_name(root, Path::new("/logs/sidecar/a.log")), "sidecar/a.log");
        assert_eq!(archive_name(root, Path::new("/elsewhere/b.log")), "elsewhere/b.log");
    }

    #[test]
    fn only_log_files_are_picked() {
        assert!(is_log_file(Path::new("latest.log")));
        assert!(is_log_file(Path::new("x.LOG")));
        assert!(!is_log_file(Path::new("config.yaml")));
    }

    #[test]
    fn every_line_is_redacted() {
        let text = redacted("a\nb\n", None);
        assert_eq!(text, "a\nb\n");
    }
}
