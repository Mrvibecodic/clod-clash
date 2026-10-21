use crate::utils::redact;
use anyhow::Result;
use smartstring::alias::String;

pub type CmdResult<T = ()> = Result<T, String>;

pub mod app;
pub mod backup;
pub mod clash;
pub mod core_updater;
pub mod lightweight;
pub mod network;
pub mod profile;
pub mod proxy;
pub mod runtime;
pub mod save_profile;
pub mod service;
pub mod system;
pub mod updater;
pub mod uwp;
pub mod validate;
pub mod verge;
pub mod webdav;

pub use app::*;
pub use backup::*;
pub use clash::*;
pub use core_updater::*;
pub use lightweight::*;
pub use network::*;
pub use profile::*;
pub use proxy::*;
pub use runtime::*;
pub use save_profile::*;
pub use service::*;
pub use system::*;
pub use updater::*;
pub use uwp::*;
pub use verge::*;
pub use webdav::*;

pub trait StringifyErr<T> {
    fn stringify_err(self) -> CmdResult<T>;
    fn stringify_err_log<F>(self, log_fn: F) -> CmdResult<T>
    where
        F: Fn(&str);
}

pub(crate) fn public_error_text(error: &impl std::fmt::Display) -> String {
    let raw = error.to_string();
    let home = redact::home_prefix();
    String::from(redact::redact(&redact::scrub_home(&raw, home.as_deref())))
}

impl<T, E: std::fmt::Display> StringifyErr<T> for Result<T, E> {
    fn stringify_err(self) -> CmdResult<T> {
        self.map_err(|e| public_error_text(&e))
    }

    fn stringify_err_log<F>(self, log_fn: F) -> CmdResult<T>
    where
        F: Fn(&str),
    {
        self.map_err(|e| {
            let msg = public_error_text(&e);
            log_fn(&msg);
            msg
        })
    }
}

#[allow(clippy::unwrap_used)]
#[cfg(test)]
mod tests {
    use super::StringifyErr as _;

    #[test]
    fn error_text_for_the_frontend_is_redacted() {
        let err: Result<(), anyhow::Error> = Err(anyhow::anyhow!("token: abcdefghij0123456789XYZ failed"));
        let text = err.stringify_err().unwrap_err();
        assert!(!text.contains("abcdefghij0123456789XYZ"), "{text}");
        assert!(text.contains("failed"));
    }

    fn rust_sources(dir: &std::path::Path, found: &mut Vec<std::path::PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                rust_sources(&path, found);
            } else if path.extension().and_then(std::ffi::OsStr::to_str) == Some("rs") {
                found.push(path);
            }
        }
    }

    fn production_sources() -> Vec<(std::path::PathBuf, std::string::String)> {
        let mut files = Vec::new();
        rust_sources(
            std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/src")),
            &mut files,
        );
        files
            .into_iter()
            .map(|path| {
                let source = std::fs::read_to_string(&path).unwrap_or_default();
                let code = crate::utils::source_scan::without_test_modules(&source).to_owned();
                (path, code)
            })
            .collect()
    }

    /// Ошибки, которые доходят до окна или уведомления, не зашиты в код
    /// по-русски: текст берётся из локалей и звучит на языке окна. Проверка
    /// одиночного экземпляра идёт до окна — её ответ только в журнал.
    #[test]
    fn no_error_is_hardcoded_in_russian() {
        let cyrillic = |text: &str| text.chars().any(|c| matches!(c, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё'));
        let sources = production_sources();
        assert!(sources.len() > 50, "исходники не найдены");
        for (path, code) in sources {
            if path.ends_with("utils/server.rs") {
                continue;
            }
            for opening in [
                "bail!(",
                "anyhow!(",
                ".context(\"",
                "with_context(|| \"",
                "with_context(|| format!(",
                "Err(\"",
                "Err(format!(",
                ".ok_or(\"",
                "ok_or_else(|| \"",
                "ok_or_else(|| format!(",
                "map_err(|e| format!(\"",
                "map_err(|err| format!(\"",
                "map_err(|error| format!(\"",
            ] {
                for (at, _) in code.match_indices(opening) {
                    let call = code[at..].split(';').next().unwrap_or_default();
                    let literal = call.split('"').nth(1).unwrap_or_default();
                    assert!(!cyrillic(literal), "{}: {call}", path.display());
                }
            }
        }
    }

    /// Каждый ключ, которым бэкенд говорит с человеком, есть во всех локалях:
    /// иначе вместо фразы показалось бы имя ключа.
    #[test]
    fn every_key_the_backend_speaks_is_in_every_locale() {
        let locales = std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../crates/clash-verge-i18n/locales"
        ));
        let mut tables = Vec::new();
        for entry in std::fs::read_dir(locales).into_iter().flatten().flatten() {
            let text = std::fs::read_to_string(entry.path()).unwrap_or_default();
            let table: serde_yaml_ng::Value = serde_yaml_ng::from_str(&text).unwrap_or_default();
            tables.push((entry.path(), table));
        }
        assert_eq!(tables.len(), 13, "локалей не тринадцать");
        let mut checked = 0_usize;
        for (path, code) in production_sources() {
            for (at, _) in code.match_indices("t!(\"") {
                if code[..at]
                    .chars()
                    .next_back()
                    .is_some_and(|c| c.is_alphanumeric() || c == '_')
                {
                    continue;
                }
                let key = code[at + 4..].split('"').next().unwrap_or_default();
                for (locale, table) in &tables {
                    let found = key
                        .split('.')
                        .try_fold(table, |node, part| node.get(part))
                        .is_some_and(serde_yaml_ng::Value::is_string);
                    assert!(
                        found,
                        "{}: ключ {key} отсутствует в {}",
                        path.display(),
                        locale.display()
                    );
                }
                checked += 1;
            }
        }
        assert!(checked > 100, "ключей не нашлось — тест ослеп");
    }
}
