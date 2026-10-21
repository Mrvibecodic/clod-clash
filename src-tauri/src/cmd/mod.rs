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

    /// Вызовы, чьи строки уходят только в журнал: журнальные макросы и
    /// помощники, которым передают подпись для журнальной строки.
    const QUIET_CALLS: &[&str] = &[
        "logging!",
        "logging_error!",
        "println!",
        "eprintln!",
        "handle_validation_notice",
        "tidy_up_before_an_abrupt_end",
        "skipped_while_exiting",
        "with_handle",
        "drop_notice",
        "failed",
    ];

    /// Функции, чей результат идёт только в журнал.
    const QUIET_FNS: &[&str] = &["fn get_window_status_info(", "fn file_label_for_the_log("];

    /// Файлы, чей текст не звучит в окне: отчёт для поддержки, скрипт страницы
    /// для её консоли, проверка одиночного экземпляра до появления окна.
    const QUIET_FILES: &[&str] = &[
        "module/support_bundle.rs",
        "utils/resolve/window_script.rs",
        "utils/server.rs",
    ];

    type Scan<'a> = (Vec<(usize, &'a str)>, Vec<(usize, usize)>);

    /// Строковые литералы кода (место и текст) и промежутки, где они до
    /// человека не доходят: тестовые модули и вызовы из `QUIET_CALLS`.
    fn literals_and_quiet_spans(code: &str) -> Scan<'_> {
        let bytes = code.as_bytes();
        let mut literals = Vec::new();
        let mut quiet = Vec::new();
        let mut open: Vec<(u8, usize, bool)> = Vec::new();
        let mut at = 0;
        while at < bytes.len() {
            let rest = &code[at..];
            if rest.starts_with("//") {
                at += rest.find('\n').unwrap_or(rest.len());
                continue;
            }
            if rest.starts_with("/*") {
                at += rest.find("*/").map_or(rest.len(), |end| end + 2);
                continue;
            }
            let word_before = at > 0 && (bytes[at - 1].is_ascii_alphanumeric() || bytes[at - 1] == b'_');
            let raw = rest.strip_prefix("br").or_else(|| rest.strip_prefix('r'));
            if let Some(raw) = raw.filter(|_| !word_before) {
                let hashes = raw.len() - raw.trim_start_matches('#').len();
                if raw[hashes..].starts_with('"') {
                    let body_at = at + (rest.len() - raw.len()) + hashes + 1;
                    let closing = format!("\"{}", "#".repeat(hashes));
                    let end = code[body_at..].find(&closing).map_or(code.len(), |end| body_at + end);
                    literals.push((at, &code[body_at..end]));
                    at = end + closing.len();
                    continue;
                }
            }
            match bytes[at] {
                b'"' => {
                    let mut end = at + 1;
                    while end < bytes.len() && bytes[end] != b'"' {
                        end += if bytes[end] == b'\\' { 2 } else { 1 };
                    }
                    literals.push((at, &code[at + 1..end.min(code.len())]));
                    at = end + 1;
                    continue;
                }
                b'\'' => {
                    if rest[1..].starts_with('\\') {
                        at += 3 + rest
                            .get(3..)
                            .and_then(|tail| tail.find('\''))
                            .map_or(rest.len(), |end| end + 1);
                        continue;
                    }
                    if rest.chars().nth(2) == Some('\'') {
                        at += 1 + rest.chars().nth(1).map_or(1, char::len_utf8) + 1;
                        continue;
                    }
                }
                b'(' => {
                    let before = code[..at].trim_end();
                    let name = before
                        .rsplit(|c: char| !(c.is_alphanumeric() || c == '_' || c == '!'))
                        .next()
                        .unwrap_or_default();
                    open.push((b'(', at, QUIET_CALLS.contains(&name)));
                }
                b'{' => {
                    let before = code[..at].trim_end();
                    let mut cut = before.len().saturating_sub(200);
                    while !before.is_char_boundary(cut) {
                        cut += 1;
                    }
                    let header = &before[cut..];
                    let test_module = (header.contains("#[cfg(test)]") || header.contains("#[cfg(all(test"))
                        && header.rsplit(']').next().is_some_and(|tail| {
                            let tail = tail.trim_start();
                            let tail = tail
                                .strip_prefix("pub(crate) ")
                                .or_else(|| tail.strip_prefix("pub "))
                                .unwrap_or(tail);
                            tail.starts_with("mod ")
                        });
                    let quiet_fn = QUIET_FNS.iter().any(|signature| {
                        header
                            .rfind(signature)
                            .is_some_and(|found| !header[found..].contains(['{', '}', ';']))
                    });
                    open.push((b'{', at, test_module || quiet_fn));
                }
                b')' | b'}' => {
                    if let Some((_, start, is_quiet)) = open.pop()
                        && is_quiet
                    {
                        quiet.push((start, at));
                    }
                }
                _ => {}
            }
            at += rest.chars().next().map_or(1, char::len_utf8);
        }
        (literals, quiet)
    }

    /// Ни один русский текст не зашит в код там, где он может дойти до окна,
    /// уведомления или ответа команды: такие тексты берутся из локалей и
    /// звучат на языке окна. Русское допустимо только в журнале, в тестах и в
    /// файлах из `QUIET_FILES`.
    #[test]
    fn no_error_is_hardcoded_in_russian() {
        let cyrillic = |text: &str| text.chars().any(|c| matches!(c, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё'));
        let root = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/src"));
        let mut files = Vec::new();
        rust_sources(root, &mut files);
        assert!(files.len() > 50, "исходники не найдены");
        let mut spoken = Vec::new();
        for path in files {
            let relative = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            if QUIET_FILES.contains(&relative.as_str()) {
                continue;
            }
            let code = std::fs::read_to_string(&path).unwrap_or_default();
            let (literals, quiet) = literals_and_quiet_spans(&code);
            for (at, text) in literals {
                let in_quiet = quiet.iter().any(|(start, end)| *start < at && at < *end);
                if cyrillic(text) && !in_quiet && !text.starts_with("# clod:") {
                    spoken.push(format!("{relative}:{}: {text}", code[..at].lines().count()));
                }
            }
        }
        assert!(spoken.is_empty(), "русский текст вне журнала:\n{}", spoken.join("\n"));
    }

    /// Страж видит любую форму, в которой текст уходит наружу, и не трогает
    /// журнал и тесты.
    #[test]
    fn the_russian_text_guard_sees_every_form() {
        let sample = concat!(
            "fn a() -> String { format!(\"не удалось: {e}\") }\n",
            "fn b(f: &mut F) { write!(f, \"прервано\") }\n",
            "fn c() { invalid(Kind::X, \"пусто\"); }\n",
            "fn d() { logging!(info, Type::Core, \"журнал {}\", if x { \"да\" } else { \"нет\" }); }\n",
            "fn e() { let _ = 'a'; let _ = '\\''; let s: &'static str = r#\"сырой\"#; }\n",
            "#[cfg(test)]\nmod tests { fn t() { assert!(x, \"тест\"); } }\n",
        );
        let (literals, quiet) = literals_and_quiet_spans(sample);
        let spoken: Vec<&str> = literals
            .into_iter()
            .filter(|(at, _)| !quiet.iter().any(|(start, end)| start < at && at < end))
            .map(|(_, text)| text)
            .filter(|text| text.chars().any(|c| ('а'..='я').contains(&c)))
            .collect();
        assert_eq!(spoken, ["не удалось: {e}", "прервано", "пусто", "сырой"]);
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
