#![cfg(unix)]

use std::path::{Path, PathBuf};

use clash_verge_logging::{Type, logging};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

fn is_known_core(exe: &Path, known: &[PathBuf]) -> bool {
    known.iter().any(|path| path == exe)
}

async fn known_core_paths() -> Vec<PathBuf> {
    let mut known = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        for name in ["verge-mihomo", "verge-mihomo-alpha"] {
            let path = exe.with_file_name(name);
            known.push(path.canonicalize().unwrap_or(path));
        }
    }
    if let Some(managed) = crate::core::core_updater::managed_core_binary().await {
        known.push(managed.canonicalize().unwrap_or(managed));
    }
    known
}

pub async fn sweep_orphan_cores() {
    let known = known_core_paths().await;
    if known.is_empty() {
        return;
    }

    let swept = tokio::task::spawn_blocking(move || {
        let mut system = System::new();
        system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing().with_exe(UpdateKind::Always),
        );
        let own_pid = sysinfo::get_current_pid().ok();
        let mut swept = 0_u32;
        for (pid, process) in system.processes() {
            if Some(*pid) == own_pid {
                continue;
            }
            let Some(exe) = process.exe() else {
                continue;
            };
            if is_known_core(exe, &known) && process.kill() {
                logging!(warn, Type::Core, "killed an orphan core (pid {})", pid);
                swept += 1;
            }
        }
        swept
    })
    .await
    .unwrap_or(0);

    if swept == 0 {
        logging!(trace, Type::Core, "no orphan cores found at startup");
    }
}

#[cfg(test)]
mod tests {
    use super::is_known_core;
    use std::path::{Path, PathBuf};

    #[test]
    fn only_exact_core_paths_match() {
        let known = [PathBuf::from("/opt/app/verge-mihomo")];
        assert!(is_known_core(Path::new("/opt/app/verge-mihomo"), &known));
        assert!(!is_known_core(Path::new("/opt/app/verge-mihomo-alpha"), &known));
        assert!(!is_known_core(Path::new("/usr/bin/mihomo"), &known));
        assert!(!is_known_core(Path::new("/other/app/verge-mihomo"), &known));
    }
}
