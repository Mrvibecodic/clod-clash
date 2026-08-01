//! clod: отчёт для поддержки — всё, что нужно провайдеру, и ничего лишнего.
//!
//! Раньше единственным способом что-то показать поддержке было «пришлите лог»,
//! а в логе лежит URL подписки целиком, вместе с токеном. Здесь собирается
//! готовый текст: версии, состояние ядра и подписки, отчёт фильтра заглушек и
//! хвосты обоих логов — **уже отредактированные**, так что его можно вставить
//! в чат не глядя.
//!
//! Правило одно: всё, что похоже на секрет, маскируется на выходе, а не на
//! входе. Логи остаются подробными для самого пользователя (папка логов рядом),
//! редакция применяется только к тому, что уезжает наружу.

use crate::{
    config::{Config, IVerge},
    core::CoreManager,
    enhance,
    utils::{dirs, help, hwid},
};
use anyhow::Result;
use std::{fmt::Write as _, path::PathBuf};
use tokio::fs;

/// Сколько последних строк каждого лога забираем.
///
/// Больше — лучше: поддержке почти всегда нужен не сам сбой, а то, что
/// происходило за десяток минут до него.
const LOG_TAIL_LINES: usize = 800;

/// Имена полей, значение которых показывать нельзя ни при каких условиях.
const SECRET_KEYS: &[&str] = &[
    "secret",
    "token",
    "password",
    "passwd",
    "uuid",
    "authorization",
    "api-key",
    "api_key",
    "apikey",
    "x-hwid",
    "hwid",
    "sub-url",
];

/// Слова, которые стоят между именем секрета и самим секретом.
const SECRET_PREFIXES: &[&str] = &["bearer", "basic", "token"];

/// Длина, начиная с которой «слово» само по себе считается секретом.
const TOKEN_LEN: usize = 20;

fn normalize_key(word: &str) -> std::string::String {
    word.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_')
        .to_ascii_lowercase()
}

fn is_secret_key(word: &str) -> bool {
    SECRET_KEYS.contains(&normalize_key(word).as_str())
}

/// Похоже на токен: длинное, без пробелов, из «машинного» алфавита и с цифрой
/// или сменой регистра. Обычные слова и пути под это не подходят.
fn looks_like_token(word: &str) -> bool {
    let body = word.trim_matches(|c: char| matches!(c, '"' | '\'' | ',' | ';' | ')' | '(' | '`'));
    if body.len() < TOKEN_LEN {
        return false;
    }
    if !body
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '=' | '.' | '+' | '/'))
    {
        return false;
    }
    let has_digit = body.chars().any(|c| c.is_ascii_digit());
    let mixed_case = body.chars().any(char::is_uppercase) && body.chars().any(char::is_lowercase);
    has_digit || mixed_case
}

/// Замаскировать одно «слово» лога.
fn redact_word(word: &str) -> std::string::String {
    if let Some(start) = word.find("http://").or_else(|| word.find("https://")) {
        let (prefix, url) = word.split_at(start);
        return format!("{prefix}{}", help::mask_url(url));
    }

    // `secret=xxx`, `token:xxx` — ключ оставляем, значение прячем.
    if let Some(pos) = word.find(['=', ':'])
        && is_secret_key(&word[..pos])
        && word.len() > pos + 1
    {
        return format!("{}{}***", &word[..pos], &word[pos..=pos]);
    }

    if looks_like_token(word) {
        return "***".into();
    }

    word.to_owned()
}

/// Убрать из строки всё, что нельзя показывать посторонним.
///
/// Сознательно грубо: лучше замазать лишнее, чем отдать наружу токен подписки.
/// Разделители сохраняются, поэтому строка остаётся читаемой.
pub fn redact(line: &str) -> std::string::String {
    let mut out = std::string::String::with_capacity(line.len());
    let mut expect_secret_value = false;

    for word in line.split_inclusive(char::is_whitespace) {
        let (body, spacing) = match word.find(char::is_whitespace) {
            Some(pos) => (&word[..pos], &word[pos..]),
            None => (word, ""),
        };

        if body.is_empty() {
            // Подряд идущие пробелы и табы не должны сбрасывать ожидание
            // значения: `secret:<таб><секрет>` тоже обязан быть замазан.
            out.push_str(spacing);
            continue;
        }

        if expect_secret_value {
            // `Authorization: Bearer <jwt>` — схема стоит между ключом и
            // значением, поэтому ожидание держится до первого настоящего слова.
            if SECRET_PREFIXES.contains(&normalize_key(body).as_str()) {
                out.push_str(body);
                out.push_str(spacing);
                continue;
            }
            out.push_str("***");
            out.push_str(spacing);
            expect_secret_value = false;
            continue;
        }

        expect_secret_value = body.ends_with([':', '=']) && is_secret_key(body);
        out.push_str(&redact_word(body));
        out.push_str(spacing);
    }

    out
}

/// Файлы лога, свежие первыми.
///
/// Лог ротируется по размеру, поэтому «последние N строк» почти всегда лежат
/// в двух файлах: текущем и предыдущем. Берём оба, иначе на подробном уровне
/// в отчёт попадает минута жизни приложения.
async fn log_files(dir: Option<PathBuf>, matches: impl Fn(&str) -> bool + Send) -> Vec<PathBuf> {
    let Some(dir) = dir else {
        return Vec::new();
    };
    let Ok(mut entries) = fs::read_dir(&dir).await else {
        return Vec::new();
    };

    let mut found: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        let name = path.file_name().and_then(|name| name.to_str()).unwrap_or_default();
        if !matches(name) {
            continue;
        }
        let modified = entry
            .metadata()
            .await
            .ok()
            .and_then(|meta| meta.modified().ok())
            .unwrap_or(std::time::UNIX_EPOCH);
        found.push((modified, path));
    }

    // Свежие первыми: reverse-порядок по времени изменения.
    found.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));
    found.into_iter().map(|(_, path)| path).collect()
}

/// Последние `lines` строк из набора файлов (свежие первыми), уже
/// отредактированные.
async fn tail_of(paths: &[PathBuf], lines: usize) -> Option<std::string::String> {
    let mut collected: Vec<std::string::String> = Vec::new();

    for path in paths {
        let Ok(content) = fs::read_to_string(path).await else {
            continue;
        };
        for line in content.lines().rev() {
            collected.push(redact(line));
            if collected.len() >= lines {
                break;
            }
        }
        if collected.len() >= lines {
            break;
        }
    }

    collected.reverse();
    let text = collected.join("\n");
    (!text.trim().is_empty()).then_some(text)
}

const fn yes_no(value: bool) -> &'static str {
    if value { "да" } else { "нет" }
}

fn app_section(out: &mut std::string::String) {
    let _ = writeln!(out, "## Приложение");
    let _ = writeln!(out, "- версия: {}", env!("CARGO_PKG_VERSION"));
    let _ = writeln!(out, "- User-Agent: {}", hwid::user_agent());
    let _ = writeln!(out, "- система: {} {}", hwid::device_os(), hwid::os_version());
    let _ = writeln!(out, "- устройство: {}", hwid::device_model());
    let _ = writeln!(out, "- архитектура: {}", std::env::consts::ARCH);
    let _ = writeln!(out, "- режим работы ядра: {}", CoreManager::global().get_running_mode());
}

async fn settings_section(out: &mut std::string::String) {
    let verge = Config::verge().await;
    let data = verge.latest_arc();
    let _ = writeln!(out, "\n## Настройки");
    let _ = writeln!(
        out,
        "- ядро: {} (управляемое: {})",
        data.clash_core.as_deref().unwrap_or("—"),
        yes_no(data.use_managed_core.unwrap_or(false))
    );
    let _ = writeln!(
        out,
        "- канал управляемого ядра: {}",
        data.managed_core_channel.as_deref().unwrap_or("—")
    );
    let _ = writeln!(out, "- TUN: {}", yes_no(data.enable_tun_mode.unwrap_or(false)));
    let _ = writeln!(
        out,
        "- Connect дёргает: системный прокси {}, TUN {}",
        yes_no(data.connect_system_proxy.unwrap_or(true)),
        yes_no(data.connect_tun_mode.unwrap_or(false))
    );
    let _ = writeln!(
        out,
        "- идентификация устройства: {}",
        yes_no(data.enable_hwid.unwrap_or(IVerge::DEFAULT_ENABLE_HWID))
    );
    let _ = writeln!(
        out,
        "- уведомления подписки: {}",
        yes_no(data.enable_sub_notifications.unwrap_or(true))
    );
    let _ = writeln!(out, "- mixed-port: {}", data.verge_mixed_port.unwrap_or(0));
    let _ = writeln!(
        out,
        "- переопределение DNS: {}",
        yes_no(data.enable_dns_settings.unwrap_or(false))
    );
    let _ = writeln!(
        out,
        "- автозапуск: {}",
        yes_no(data.enable_auto_launch.unwrap_or(false))
    );
    let _ = writeln!(out, "- язык: {}", data.language.as_deref().unwrap_or("—"));
    let _ = writeln!(
        out,
        "- уровень логов: {}",
        data.app_log_level.as_deref().unwrap_or("info")
    );
}

async fn subscription_section(out: &mut std::string::String) {
    let profiles = Config::profiles().await;
    let data = profiles.latest_arc();
    let _ = writeln!(out, "\n## Подписка");

    let Some(uid) = data.get_current().cloned() else {
        let _ = writeln!(out, "- профиль не выбран");
        return;
    };
    let Ok(item) = data.get_item(&uid) else {
        let _ = writeln!(out, "- профиль {uid} не найден");
        return;
    };

    let _ = writeln!(out, "- название: {}", item.name.as_deref().unwrap_or("—"));
    let _ = writeln!(
        out,
        "- адрес: {}",
        item.url.as_deref().map(help::mask_url).unwrap_or_else(|| "—".into())
    );
    let _ = writeln!(out, "- обновлена: {}", item.updated.unwrap_or(0));
    if let Some(extra) = item.extra.as_ref() {
        let _ = writeln!(
            out,
            "- трафик: {} + {} из {} (0 = безлимит)",
            extra.upload, extra.download, extra.total
        );
        let _ = writeln!(out, "- срок (unix): {} (0 = бессрочно)", extra.expire);
    } else {
        let _ = writeln!(out, "- subscription-userinfo панель не прислала");
    }
    let _ = writeln!(out, "- состояние HWID: {}", item.hwid_state.as_deref().unwrap_or("—"));
    let _ = writeln!(
        out,
        "- заголовки провайдера: логотип {}, объявление {}, промо {}, кабинет {}, продление {}, докупка {}, поддержка {}",
        yes_no(item.logo.is_some()),
        yes_no(item.announce.is_some()),
        yes_no(item.promo.is_some()),
        yes_no(item.portal_url.is_some()),
        yes_no(item.renew_url.is_some()),
        yes_no(item.topup_url.is_some()),
        yes_no(item.support_url.is_some())
    );
    let _ = writeln!(
        out,
        "- запасной адрес использован: {}, переездов подряд: {}",
        yes_no(item.from_fallback.unwrap_or(false)),
        item.migration_hops.unwrap_or(0)
    );
}

fn sentinel_section(out: &mut std::string::String) {
    let report = enhance::sentinel_report();
    let _ = writeln!(out, "\n## Узлы-заглушки панели");
    let _ = writeln!(out, "- серверов не осталось вовсе: {}", yes_no(report.only_sentinels));
    if report.remarks.is_empty() {
        let _ = writeln!(out, "- заглушек в последнем конфиге не было");
    } else {
        let _ = writeln!(out, "- панель прислала вместо серверов: {}", report.remarks.join(" · "));
    }
}

async fn logs_section(out: &mut std::string::String, lines: usize) {
    // Лог приложения лежит в корне logs/, лог ядра — в logs/sidecar/.
    let app_logs = log_files(dirs::app_logs_dir().ok(), |name| {
        name.ends_with(".log") && !name.starts_with("sidecar")
    })
    .await;
    let _ = writeln!(out, "\n## Лог приложения (последние {lines} строк, отредактирован)");
    match tail_of(&app_logs, lines).await {
        Some(tail) => {
            let _ = writeln!(out, "```\n{tail}\n```");
        }
        None => {
            let _ = writeln!(out, "лог пуст или недоступен");
        }
    }

    let core_logs = log_files(dirs::sidecar_log_dir().ok(), |name| name.ends_with(".log")).await;
    let _ = writeln!(out, "\n## Лог ядра (последние {lines} строк, отредактирован)");
    match tail_of(&core_logs, lines).await {
        Some(tail) => {
            let _ = writeln!(out, "```\n{tail}\n```");
        }
        None => {
            let _ = writeln!(out, "лог ядра не найден");
        }
    }
}

/// Собрать отчёт целиком.
///
/// `lines` — сколько строк каждого лога взять; `None` — столько, сколько
/// обычно хватает поддержке.
pub async fn build(lines: Option<usize>) -> Result<std::string::String> {
    let lines = lines.unwrap_or(LOG_TAIL_LINES).clamp(50, 5000);
    let mut out = std::string::String::with_capacity(64 * 1024);

    let _ = writeln!(out, "# Clod Clash — отчёт для поддержки");
    let _ = writeln!(out, "Секреты (адреса подписок, токены, пароли) заменены на `***`.\n");

    app_section(&mut out);
    settings_section(&mut out).await;
    subscription_section(&mut out).await;
    sentinel_section(&mut out);
    logs_section(&mut out, lines).await;

    Ok(out)
}

#[allow(clippy::expect_used)]
#[cfg(test)]
mod tests {
    use super::redact;

    #[test]
    fn subscription_url_loses_its_token() {
        let line = "[订阅更新] URL = https://panel.example.com/sub/9f8e7d6c5b4a3f2e1d0c9b8a7?token=abcdef123456";
        let masked = redact(line);
        assert!(!masked.contains("9f8e7d6c5b4a3f2e1d0c9b8a7"));
        assert!(!masked.contains("abcdef123456"));
        assert!(masked.contains("panel.example.com"));
    }

    #[test]
    fn bearer_tokens_and_odd_spacing_survive_nothing() {
        let jwt = redact("Authorization: Bearer eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxIn0.abcdefghij");
        assert!(!jwt.contains("eyJhbGciOiJIUzI1NiJ9"), "{jwt}");
        assert!(jwt.contains("Bearer"), "{jwt}");

        let spaced = redact("secret:  hunter2hunter2hunter2");
        assert!(!spaced.contains("hunter2"), "{spaced}");

        let tabbed = redact("token:\tZm9vYmFyYmF6cXV4MTIzNDU2");
        assert!(!tabbed.contains("Zm9vYmFy"), "{tabbed}");
    }

    #[test]
    fn named_secrets_are_dropped() {
        for line in [
            "secret: set-your-secret",
            "external-controller token=Zm9vYmFyYmF6",
            "password: hunter2hunter2hunter2",
            "x-hwid: 4f9c1ab0e37d5c82b6a1d40f9e77c3b1",
        ] {
            let masked = redact(line);
            assert!(masked.contains("***"), "{line} -> {masked}");
            assert!(!masked.contains("hunter2"), "{line} -> {masked}");
        }
    }

    #[test]
    fn long_random_words_are_dropped_even_without_a_key() {
        let masked = redact("resolved node id 8f14e45fceea167a5a36dedd4bea2543aa");
        assert!(masked.ends_with("***"), "{masked}");
    }

    #[test]
    fn ordinary_lines_stay_readable() {
        let line = "[core] mihomo started, mode rule, mixed-port 7897";
        assert_eq!(redact(line), line);
    }
}
