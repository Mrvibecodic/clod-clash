use crate::utils::{
    dirs,
    redact::{home_prefix, redact, scrub_home},
};
use anyhow::{Context as _, Result};
use chrono::Local;
use std::{
    fs::OpenOptions,
    io::Write as _,
    path::{Path, PathBuf},
};

const STARTUP_LOG_FILE: &str = "startup.log";

pub fn report_failure(error: &anyhow::Error) {
    let detail = format!("{error:#}");
    let safe_detail = safe_for_the_local_log(&detail, home_prefix().as_deref());
    let mut stderr = std::io::stderr();
    let _ = writeln!(stderr, "[clod-clash] startup failed: {safe_detail}");

    match startup_log_path().and_then(|path| {
        append_failure(&path, &detail)?;
        Ok(path)
    }) {
        Ok(path) => {
            let _ = writeln!(stderr, "[clod-clash] diagnostic log: {}", path.display());
        }
        Err(log_error) => {
            let _ = writeln!(stderr, "[clod-clash] failed to write the startup log: {log_error:#}");
        }
    }
}

fn startup_log_path() -> Result<PathBuf> {
    Ok(dirs::preinit_app_home_dir()?.join("logs").join(STARTUP_LOG_FILE))
}

fn append_failure(path: &Path, detail: &str) -> Result<()> {
    let parent = path.parent().context("startup log has no parent directory")?;
    std::fs::create_dir_all(parent).context("failed to create the startup log directory")?;

    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = options.open(path).context("failed to open the startup log")?;
    let home = home_prefix();
    for line in clash_verge_logging::startup::held_lines() {
        writeln!(file, "{}", safe_for_the_local_log(&line, home.as_deref()))
            .context("failed to write the startup log")?;
    }
    writeln!(
        file,
        "[{}] [ERROR] [Startup] {}",
        Local::now().format("%Y-%m-%d %H:%M:%S%.3f"),
        safe_for_the_local_log(detail, home.as_deref())
    )
    .context("failed to write the startup log")?;
    file.flush().context("failed to flush the startup log")?;
    Ok(())
}

fn safe_for_the_local_log(text: &str, home: Option<&str>) -> String {
    text.split('\n')
        .map(|line| redact(&scrub_home(line, home)))
        .collect::<Vec<_>>()
        .join("\n")
}

#[allow(clippy::expect_used)]
#[cfg(test)]
mod tests {
    use super::safe_for_the_local_log;

    #[test]
    fn the_startup_log_is_edited_like_every_other_log() {
        let text = "loading /home/ivan/.config/clod/profiles.yaml\nfetch https://panel.example.com/sub/AbCd1234EfGh5678 failed";
        let safe = safe_for_the_local_log(text, Some("/home/ivan"));
        assert!(!safe.contains("/home/ivan"), "{safe}");
        assert!(!safe.contains("AbCd1234EfGh5678"), "{safe}");
        assert!(safe.contains("panel.example.com"), "{safe}");
        assert_eq!(safe.lines().count(), 2, "{safe}");
    }
}
