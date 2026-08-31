use std::path::Path;

use clash_verge_logging::{Type, logging};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

#[cfg(windows)]
const CORE_FILE_NAMES: &[&str] = &["verge-mihomo.exe", "verge-mihomo-alpha.exe"];
#[cfg(not(windows))]
const CORE_FILE_NAMES: &[&str] = &["verge-mihomo", "verge-mihomo-alpha"];

const SERVICE_FILE_STEM: &str = "clash-verge-service";

fn comparable(path: &Path) -> String {
    let text = path.to_string_lossy();
    if cfg!(windows) {
        text.strip_prefix(r"\\?\").unwrap_or(&text).to_lowercase()
    } else {
        text.into_owned()
    }
}

fn is_known_core(exe: &Path, known: &[String]) -> bool {
    known.contains(&comparable(exe))
}

fn parent_shields_the_core(parent_name: Option<&std::ffi::OsStr>) -> bool {
    let Some(stem) = parent_name.map(Path::new).and_then(Path::file_stem) else {
        return false;
    };
    let stem = stem.to_string_lossy().to_ascii_lowercase();
    stem == SERVICE_FILE_STEM || (stem.len() >= 15 && SERVICE_FILE_STEM.starts_with(stem.as_str()))
}

async fn known_core_paths() -> Vec<String> {
    let mut known = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        for name in CORE_FILE_NAMES {
            let path = exe.with_file_name(name);
            known.push(comparable(&path.canonicalize().unwrap_or(path)));
        }
    }
    if let Some(managed) = crate::core::core_updater::managed_core_binary().await {
        known.push(comparable(&managed.canonicalize().unwrap_or(managed)));
    }
    known
}

fn owned_by_current_user(process: &sysinfo::Process, own_user: Option<&sysinfo::Uid>) -> bool {
    match (process.user_id(), own_user) {
        (Some(user), Some(own)) => user == own,
        (Some(_), None) => false,
        (None, _) => true,
    }
}

fn current_user(system: &System, own_pid: Option<sysinfo::Pid>) -> Option<&sysinfo::Uid> {
    own_pid
        .and_then(|pid| system.processes().get(&pid))
        .and_then(|process| process.user_id())
}

fn sweep_processes(system: &System, known: &[String], own_pid: Option<sysinfo::Pid>) -> u32 {
    let mut swept = 0_u32;
    let own_user = current_user(system, own_pid);
    for (pid, process) in system.processes() {
        if Some(*pid) == own_pid {
            continue;
        }
        let Some(exe) = process.exe() else {
            continue;
        };
        if !is_known_core(exe, known) {
            continue;
        }
        if !owned_by_current_user(process, own_user) {
            logging!(
                trace,
                Type::Core,
                "core pid {} belongs to another user, leaving it alone",
                pid
            );
            continue;
        }
        let parent_name = process
            .parent()
            .and_then(|ppid| system.processes().get(&ppid))
            .map(|parent| parent.name());
        if parent_shields_the_core(parent_name) {
            logging!(
                trace,
                Type::Core,
                "core pid {} belongs to the background service, leaving it alone",
                pid
            );
            continue;
        }
        if process.kill() {
            logging!(warn, Type::Core, "killed an orphan core (pid {})", pid);
            swept += 1;
        }
    }
    swept
}

pub async fn another_core_of_ours_is_running(own_sidecar_pid: Option<u32>, under_service: bool) -> bool {
    let known = known_core_paths().await;
    if known.is_empty() {
        return false;
    }

    tokio::task::spawn_blocking(move || {
        let mut system = System::new();
        system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing()
                .with_exe(UpdateKind::Always)
                .with_user(UpdateKind::Always),
        );
        let own_pid = sysinfo::get_current_pid().ok();
        system.processes().iter().any(|(pid, process)| {
            if Some(*pid) == own_pid || own_sidecar_pid.is_some_and(|target| pid.as_u32() == target) {
                return false;
            }
            let Some(exe) = process.exe() else {
                return false;
            };
            if !is_known_core(exe, &known) {
                return false;
            }
            if under_service {
                let parent_name = process
                    .parent()
                    .and_then(|ppid| system.processes().get(&ppid))
                    .map(|parent| parent.name());
                if parent_shields_the_core(parent_name) {
                    return false;
                }
            }
            true
        })
    })
    .await
    .unwrap_or(false)
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
            ProcessRefreshKind::nothing()
                .with_exe(UpdateKind::Always)
                .with_user(UpdateKind::Always),
        );
        let own_pid = sysinfo::get_current_pid().ok();
        sweep_processes(&system, &known, own_pid)
    })
    .await
    .unwrap_or(0);

    if swept == 0 {
        logging!(trace, Type::Core, "no orphan cores found at startup");
    }
}

#[cfg(test)]
mod tests {
    use super::{comparable, is_known_core, parent_shields_the_core};
    use std::ffi::OsStr;
    use std::path::Path;

    #[test]
    fn only_exact_core_paths_match() {
        let known = [comparable(Path::new("/opt/app/verge-mihomo"))];
        assert!(is_known_core(Path::new("/opt/app/verge-mihomo"), &known));
        assert!(!is_known_core(Path::new("/opt/app/verge-mihomo-alpha"), &known));
        assert!(!is_known_core(Path::new("/usr/bin/mihomo"), &known));
        assert!(!is_known_core(Path::new("/other/app/verge-mihomo"), &known));
    }

    #[test]
    fn the_services_own_core_is_shielded() {
        assert!(parent_shields_the_core(Some(OsStr::new("clash-verge-service"))));
        assert!(parent_shields_the_core(Some(OsStr::new("clash-verge-service.exe"))));
        assert!(parent_shields_the_core(Some(OsStr::new("Clash-Verge-Service.exe"))));
        assert!(parent_shields_the_core(Some(OsStr::new("clash-verge-ser"))));
        assert!(!parent_shields_the_core(Some(OsStr::new("clash-verge-serXXXX"))));
        assert!(!parent_shields_the_core(Some(OsStr::new("clash"))));
        assert!(!parent_shields_the_core(Some(OsStr::new("systemd"))));
        assert!(!parent_shields_the_core(Some(OsStr::new("explorer.exe"))));
        assert!(!parent_shields_the_core(None));
    }

    #[test]
    fn windows_paths_compare_without_prefix_or_case() {
        if cfg!(windows) {
            assert_eq!(
                comparable(Path::new(r"\\?\C:\Apps\Verge-Mihomo.EXE")),
                comparable(Path::new(r"c:\apps\verge-mihomo.exe"))
            );
        } else {
            assert_ne!(
                comparable(Path::new("/opt/App/verge-mihomo")),
                comparable(Path::new("/opt/app/verge-mihomo"))
            );
        }
    }
}
