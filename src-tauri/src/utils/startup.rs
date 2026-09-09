use crate::utils::dirs;
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
    let mut stderr = std::io::stderr();
    let _ = writeln!(stderr, "[clod-clash] startup failed: {detail}");

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
    for line in clash_verge_logging::startup::held_lines() {
        writeln!(file, "{line}").context("failed to write the startup log")?;
    }
    writeln!(
        file,
        "[{}] [ERROR] [Startup] {detail}",
        Local::now().format("%Y-%m-%d %H:%M:%S%.3f")
    )
    .context("failed to write the startup log")?;
    file.flush().context("failed to flush the startup log")?;
    Ok(())
}
