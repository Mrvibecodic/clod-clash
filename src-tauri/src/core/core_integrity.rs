use std::path::Path;

use anyhow::{Context as _, Result};
use sha2::{Digest as _, Sha256};

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

pub fn digest_of_bytes(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum WriteAccess {
    AdminOnly,
    Unprivileged,
}

/// Путь в сравнимом виде: нижний регистр, разделители в одну сторону, без хвостовой косой черты.
fn comparable(path: &str) -> std::string::String {
    let text = path.to_ascii_lowercase().replace('\\', "/");
    text.trim_end_matches('/').to_owned()
}

/// Корни, куда без прав администратора не пишут.
///
/// На Windows берутся из окружения, а не из английских названий:
/// папка Program Files может быть на другом диске и называться на языке системы.
fn privileged_roots() -> Vec<std::string::String> {
    let mut roots: Vec<std::string::String> = Vec::new();
    #[cfg(target_os = "windows")]
    for name in [
        "ProgramFiles",
        "ProgramFiles(x86)",
        "ProgramW6432",
        "ProgramData",
        "SystemRoot",
    ] {
        if let Ok(value) = std::env::var(name) {
            let root = comparable(&value);
            if !root.is_empty() && !roots.contains(&root) {
                roots.push(root);
            }
        }
    }
    #[cfg(target_os = "macos")]
    roots.extend(
        // /private/var сюда не годится: под ним лежат и /private/var/tmp,
        // и /private/var/folders — оба пишутся без прав администратора.
        ["/applications", "/library", "/system", "/usr", "/opt"]
            .iter()
            .map(|root| (*root).to_owned()),
    );
    #[cfg(all(unix, not(target_os = "macos")))]
    roots.extend(
        ["/usr", "/opt", "/snap", "/var/lib"]
            .iter()
            .map(|root| (*root).to_owned()),
    );
    roots
}

/// Сравнение идёт ПО НАЧАЛУ пути, а не по вхождению подстроки: с вхождением
/// созданная пользователем папка `D:\Program Files\...`, `~/Library/...` или
/// `~/opt/...` выдавала бы себя за системную.
fn under_a_privileged_root(dir: &Path, roots: &[std::string::String]) -> bool {
    let text = comparable(&dir.to_string_lossy());
    roots
        .iter()
        .any(|root| !root.is_empty() && (text == *root || text.starts_with(&format!("{root}/"))))
}

fn write_access_of(dir: &Path, elevated: bool) -> WriteAccess {
    if elevated {
        return if under_a_privileged_root(dir, &privileged_roots()) {
            WriteAccess::AdminOnly
        } else {
            WriteAccess::Unprivileged
        };
    }
    let probe = dir.join(format!(".clod-write-probe-{}", std::process::id()));
    match std::fs::OpenOptions::new().write(true).create_new(true).open(&probe) {
        Ok(file) => {
            drop(file);
            let _ = std::fs::remove_file(&probe);
            WriteAccess::Unprivileged
        }
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => WriteAccess::Unprivileged,
        Err(_) => WriteAccess::AdminOnly,
    }
}

/// Папка бинарника доступна на запись текущему пользователю (без прав
/// администратора) — тот же вопрос, что решает `write_access_of`, но с
/// ответом «да/нет» для тех, кому важна не защита, а возможность записи.
pub fn binary_dir_is_writable(path: &Path) -> bool {
    directory_write_access(path) == WriteAccess::Unprivileged
}

fn directory_write_access(path: &Path) -> WriteAccess {
    let Some(dir) = path.parent() else {
        return WriteAccess::Unprivileged;
    };
    write_access_of(dir, crate::feat::tun::is_app_elevated())
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::{WriteAccess, digest_of, digest_of_bytes, under_a_privileged_root, write_access_of};

    #[test]
    fn a_writable_directory_is_seen_as_writable_and_leaves_nothing_behind() {
        let dir = std::env::temp_dir().join(format!("clod-core-probe-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let binary = dir.join("verge-mihomo");
        std::fs::write(&binary, b"core").expect("write");

        assert_eq!(write_access_of(&dir, false), WriteAccess::Unprivileged);
        let left: Vec<_> = std::fs::read_dir(&dir)
            .expect("read dir")
            .filter_map(Result::ok)
            .map(|entry| entry.file_name())
            .collect();
        assert_eq!(left, vec![binary.file_name().expect("name").to_owned()]);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_directory_that_does_not_exist_counts_as_closed() {
        let missing = std::env::temp_dir().join(format!("clod-core-probe-missing-{}", std::process::id()));
        assert_eq!(write_access_of(&missing, false), WriteAccess::AdminOnly);
    }

    #[tokio::test]
    async fn file_and_bytes_agree_on_the_digest() {
        let dir = std::env::temp_dir().join(format!("clod-core-pin-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let file = dir.join("core-binary");
        let payload = b"not really a core, but hashes the same way";
        std::fs::write(&file, payload).expect("write");

        let from_file = digest_of(&file).await.expect("digest");
        assert_eq!(from_file, digest_of_bytes(payload));
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

    fn sample_roots() -> Vec<std::string::String> {
        [
            "c:/program files",
            "c:/program files (x86)",
            "c:/programdata",
            "c:/windows",
            "/applications",
            "/library",
            "/usr",
            "/opt",
        ]
        .iter()
        .map(|root| (*root).to_owned())
        .collect()
    }

    #[test]
    fn a_protected_folder_is_recognised_by_the_beginning_of_the_path() {
        let roots = sample_roots();
        for dir in [
            r"C:\Program Files\Clod Clash",
            r"C:\Program Files (x86)\Clod Clash",
            r"C:\ProgramData\clod-clash",
            r"C:\Windows\System32\clod",
            "/Applications/Clod Clash.app/Contents/Resources",
            "/usr/lib/clod-clash",
            "/opt",
        ] {
            assert!(
                under_a_privileged_root(std::path::Path::new(dir), &roots),
                "a protected folder was taken for a writable one: {dir}"
            );
        }
    }

    #[test]
    fn a_folder_the_user_can_write_is_never_taken_for_a_protected_one() {
        let roots = sample_roots();
        for dir in [
            r"C:\Users\alex\AppData\Local\clod-clash",
            r"D:\portable\clod-clash",
            "/home/alex/.local/share/clod-clash",
            // Своя папка с тем же именем на другом диске.
            r"D:\Program Files\Clod Clash",
            // Пользовательские Library и Applications на macOS.
            "/Users/alex/Library/Application Support/clod-clash",
            "/Users/alex/Applications/Clod Clash.app/Contents/MacOS",
            // Своя opt в домашнем каталоге.
            "/home/alex/opt/clod-clash",
            // Похожее имя, но другой каталог.
            "/usrlocal/clod-clash",
        ] {
            assert!(
                !under_a_privileged_root(std::path::Path::new(dir), &roots),
                "a user folder was taken for a protected one: {dir}"
            );
        }
    }
}
