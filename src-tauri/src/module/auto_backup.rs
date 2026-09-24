use crate::{
    config::{Config, IVerge},
    feat::create_local_backup_with_namer,
    process::AsyncHandler,
    utils::dirs::local_backup_dir,
};
use anyhow::Result;
use clash_verge_logging::{Type, logging};
use once_cell::sync::OnceCell;
use std::{
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{fs, sync::watch};

const DEFAULT_INTERVAL_HOURS: u64 = 24;
const MIN_INTERVAL_HOURS: u64 = 1;
const MAX_INTERVAL_HOURS: u64 = 168;
const AUTO_BACKUP_KEEP: usize = 20;
const AUTO_MARKER: &str = "-auto-";
const OVERDUE_BACKUP_DELAY: Duration = Duration::from_secs(60);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct AutoBackupSettings {
    schedule_enabled: bool,
    interval_hours: u64,
}

impl AutoBackupSettings {
    fn from_verge(verge: &IVerge) -> Self {
        let interval = verge
            .auto_backup_interval_hours
            .unwrap_or(DEFAULT_INTERVAL_HOURS)
            .clamp(MIN_INTERVAL_HOURS, MAX_INTERVAL_HOURS);

        Self {
            schedule_enabled: verge.enable_auto_backup_schedule.unwrap_or(false),
            interval_hours: interval,
        }
    }
}

impl Default for AutoBackupSettings {
    fn default() -> Self {
        Self {
            schedule_enabled: false,
            interval_hours: DEFAULT_INTERVAL_HOURS,
        }
    }
}

pub struct AutoBackupManager {
    settings_tx: watch::Sender<AutoBackupSettings>,
    runner_started: AtomicBool,
}

impl AutoBackupManager {
    pub fn global() -> &'static Self {
        static INSTANCE: OnceCell<AutoBackupManager> = OnceCell::new();
        INSTANCE.get_or_init(|| {
            let (tx, _rx) = watch::channel(AutoBackupSettings::default());
            Self {
                settings_tx: tx,
                runner_started: AtomicBool::new(false),
            }
        })
    }

    pub async fn init(&self) -> Result<()> {
        let settings = Self::load_settings().await;
        self.settings_tx.send_if_modified(|current| {
            let changed = *current != settings;
            *current = settings;
            changed
        });
        self.maybe_start_runner(settings);
        Ok(())
    }

    pub async fn refresh_settings(&self) -> Result<()> {
        let settings = Self::load_settings().await;
        self.settings_tx.send_if_modified(|current| {
            let changed = *current != settings;
            *current = settings;
            changed
        });
        self.maybe_start_runner(settings);
        Ok(())
    }

    fn maybe_start_runner(&self, settings: AutoBackupSettings) {
        if settings.schedule_enabled {
            self.ensure_runner();
        }
    }

    fn ensure_runner(&self) {
        if self.runner_started.swap(true, Ordering::SeqCst) {
            return;
        }

        let mut rx = self.settings_tx.subscribe();
        AsyncHandler::spawn(move || async move {
            Self::run_scheduler(&mut rx).await;
        });
    }

    async fn run_scheduler(rx: &mut watch::Receiver<AutoBackupSettings>) {
        let mut current = *rx.borrow();
        let mut last_attempt = None;
        loop {
            if !current.schedule_enabled {
                if rx.changed().await.is_err() {
                    break;
                }
                current = *rx.borrow();
                continue;
            }

            let interval = Duration::from_secs(current.interval_hours.saturating_mul(3600));
            let last_backup = auto_backups()
                .await
                .into_iter()
                .map(|(_, modified)| modified)
                .chain(last_attempt)
                .max();
            let sleeper = tokio::time::sleep(wait_until_due(interval, last_backup, unix_now()));
            tokio::pin!(sleeper);

            tokio::select! {
                _ = &mut sleeper => {
                    last_attempt = Some(unix_now());
                    if let Err(err) = Self::global()
                        .execute_scheduled()
                        .await
                    {
                        logging!(
                            warn,
                            Type::Backup,
                            "Scheduled auto backup failed: {err:#?}"
                        );
                    }
                }
                changed = rx.changed() => {
                    if changed.is_err() {
                        break;
                    }
                    current = *rx.borrow();
                }
            }
        }
    }

    async fn execute_scheduled(&self) -> Result<()> {
        if !self.settings_tx.borrow().schedule_enabled {
            return Ok(());
        }

        let file_name = create_local_backup_with_namer(|name| append_auto_suffix(name).into()).await?;

        if let Err(err) = cleanup_auto_backups().await {
            logging!(warn, Type::Backup, "Failed to cleanup old auto backups: {err:#?}");
        }

        logging!(info, Type::Backup, "Auto backup created: {}", file_name);
        Ok(())
    }

    async fn load_settings() -> AutoBackupSettings {
        let verge = Config::verge().await;
        AutoBackupSettings::from_verge(&verge.latest_arc())
    }
}

fn append_auto_suffix(file_name: &str) -> String {
    match file_name.rsplit_once('.') {
        Some((stem, ext)) => format!("{stem}{AUTO_MARKER}scheduled.{ext}"),
        None => format!("{file_name}{AUTO_MARKER}scheduled"),
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|dur| dur.as_secs())
        .unwrap_or(0)
}

fn wait_until_due(interval: Duration, last_backup: Option<u64>, now: u64) -> Duration {
    let Some(last_backup) = last_backup.filter(|modified| *modified > 0) else {
        return interval;
    };
    let elapsed = Duration::from_secs(now.saturating_sub(last_backup));
    interval.saturating_sub(elapsed).max(OVERDUE_BACKUP_DELAY)
}

async fn auto_backups() -> Vec<(PathBuf, u64)> {
    match list_auto_backups().await {
        Ok(files) => files,
        Err(err) => {
            logging!(warn, Type::Backup, "Failed to list auto backups: {err:#?}");
            Vec::new()
        }
    }
}

async fn cleanup_auto_backups() -> Result<()> {
    if AUTO_BACKUP_KEEP == 0 {
        return Ok(());
    }

    let mut files = list_auto_backups().await?;

    if files.len() <= AUTO_BACKUP_KEEP {
        return Ok(());
    }

    files.sort_by_key(|(_, ts)| *ts);
    let remove_count = files.len() - AUTO_BACKUP_KEEP;
    for (path, _) in files.into_iter().take(remove_count) {
        if let Err(err) = fs::remove_file(&path).await {
            logging!(
                warn,
                Type::Backup,
                "Failed to remove auto backup {}: {err:#?}",
                path.display()
            );
        }
    }

    Ok(())
}

async fn list_auto_backups() -> Result<Vec<(PathBuf, u64)>> {
    let backup_dir = local_backup_dir()?;
    if !backup_dir.exists() {
        return Ok(Vec::new());
    }

    let mut entries = match fs::read_dir(&backup_dir).await {
        Ok(dir) => dir,
        Err(err) => {
            logging!(warn, Type::Backup, "Failed to read backup directory: {err:#?}");
            return Ok(Vec::new());
        }
    };

    let mut files: Vec<(PathBuf, u64)> = Vec::new();

    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let file_name = match entry.file_name().into_string() {
            Ok(name) => name,
            Err(_) => continue,
        };

        if !file_name.contains(AUTO_MARKER) {
            continue;
        }

        let modified = entry
            .metadata()
            .await
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|dur| dur.as_secs())
            .unwrap_or(0);

        files.push((path, modified));
    }

    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::{OVERDUE_BACKUP_DELAY, wait_until_due};
    use std::time::Duration;

    const DAY: Duration = Duration::from_secs(24 * 3600);

    #[test]
    fn without_a_previous_backup_the_whole_interval_is_waited() {
        assert_eq!(wait_until_due(DAY, None, 1_000_000), DAY);
        assert_eq!(wait_until_due(DAY, Some(0), 1_000_000), DAY);
    }

    #[test]
    fn the_interval_counts_from_the_last_backup_not_from_the_start() {
        let now = 1_000_000;
        assert_eq!(
            wait_until_due(DAY, Some(now - 3600), now),
            DAY - Duration::from_secs(3600)
        );
    }

    #[test]
    fn an_overdue_backup_runs_shortly_after_the_start() {
        let now = 1_000_000;
        assert_eq!(
            wait_until_due(DAY, Some(now - 3 * 24 * 3600), now),
            OVERDUE_BACKUP_DELAY
        );
    }

    #[test]
    fn a_backup_dated_in_the_future_waits_the_whole_interval() {
        assert_eq!(wait_until_due(DAY, Some(2_000_000), 1_000_000), DAY);
    }
}
