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
//!
//! Секрет — это не только токен. Ядро на уровне `info` пишет строку на каждое
//! соединение (`[TCP] … --> mail.example.com:443 match …`), то есть сырой хвост
//! его лога — это список посещённых адресов. В отчёт такие строки не попадают
//! вовсе: поддержке они не нужны, а пользователь отдаёт отчёт человеку, которого
//! видит первый раз.

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

/// Для чистого hex порог ниже: 16 знаков — это уже полноценный ключ или хеш,
/// а осмысленным словом такая строка быть не может.
const HEX_TOKEN_LEN: usize = 16;

fn normalize_key(word: &str) -> std::string::String {
    word.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_')
        .to_ascii_lowercase()
}

fn is_secret_key(word: &str) -> bool {
    SECRET_KEYS.contains(&normalize_key(word).as_str())
}

/// Похоже на токен: длинное, без пробелов, из «машинного» алфавита и с цифрой
/// или сменой регистра. Обычные слова и пути под это не подходят.
///
/// Отдельная ветка для hex: ключ вида `deadbeefcafebabedeadbeef` не содержит
/// ни цифр в непривычном месте, ни смены регистра, и общее правило его
/// пропускало. Требование хотя бы одной буквы `a-f` оставляет читаемыми
/// обычные длинные числа — таймстампы, счётчики байт.
fn looks_like_token(word: &str) -> bool {
    let body =
        word.trim_matches(|c: char| matches!(c, '"' | '\'' | ',' | ';' | ')' | '(' | '`' | '{' | '}' | '[' | ']'));

    let hex_token = body.len() >= HEX_TOKEN_LEN
        && body.chars().all(|c| c.is_ascii_hexdigit())
        && body.chars().any(|c| c.is_ascii_alphabetic());
    if hex_token {
        return true;
    }

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

/// Оставить от любого URL только схему и хост.
///
/// `help::mask_url` рассчитан на аккуратный адрес подписки и щадит короткие
/// сегменты пути; здесь вход — произвольная строка лога, поэтому режем грубо:
/// всё после хоста и весь `user:pass@` — под `***`. Работает для любой схемы,
/// не только http (`vless://`, `ss://`, `clash://` несут секреты ничуть не хуже).
fn mask_any_url(word: &str) -> Option<std::string::String> {
    // Процент-кодированный адрес внутри deep-link разобрать по частям нельзя —
    // такое слово маскируется целиком, начиная со схемы.
    let lower = word.to_ascii_lowercase();
    if let Some(at) = lower.find("%3a%2f%2f") {
        let start = scheme_start(word, at);
        return Some(format!("{}***", &word[..start]));
    }

    let at = word.find("://")?;
    let start = scheme_start(word, at);
    let authority_at = at + 3;
    let rest = &word[authority_at..];
    let authority_len = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..authority_len];

    // `user:pass@host` — пароль в адресе подписки это рабочая схема, и он
    // не должен уехать в чат вместе с хостом.
    let host = authority.rfind('@').map_or(authority, |pos| &authority[pos + 1..]);

    let tail = if authority_len < rest.len() { "/***" } else { "" };
    let masked = if host.len() == authority.len() { "" } else { "***@" };
    Some(format!(
        "{}{}{masked}{host}{tail}",
        &word[..start],
        &word[start..at + 3]
    ))
}

/// Начало схемы слева от `://` (или от его процент-кодированной формы).
fn scheme_start(word: &str, separator_at: usize) -> usize {
    word[..separator_at]
        .rfind(|c: char| !c.is_ascii_alphanumeric() && !matches!(c, '+' | '-' | '.'))
        .map_or(0, |pos| pos + 1)
}

/// Замазать всё, что стоит за именем секрета внутри одного «слова».
///
/// Ловит случаи, где ключ и значение слиплись и пробела между ними нет:
/// `{"token":"…"}`, `?access_token=…`, `secret=…;`.
fn mask_after_secret_key(word: &str) -> Option<std::string::String> {
    let lower = word.to_ascii_lowercase();
    let mut cut: Option<usize> = None;

    for key in SECRET_KEYS {
        let mut from = 0;
        while let Some(rel) = lower[from..].find(key) {
            let after = from + rel + key.len();
            let rest = &lower[after..];
            let trimmed = rest.trim_start_matches(['"', '\'', ' ']);
            if trimmed.starts_with([':', '=']) {
                let at = after + (rest.len() - trimmed.len()) + 1;
                cut = Some(cut.map_or(at, |best: usize| best.min(at)));
                break;
            }
            from = after;
        }
    }

    // Нечего прятать, если за разделителем и так ничего нет.
    cut.filter(|at| *at < word.len())
        .map(|at| format!("{}***", &word[..at]))
}

/// Замаскировать одно «слово» лога.
fn redact_word(word: &str) -> std::string::String {
    if let Some(masked) = mask_any_url(word) {
        return masked;
    }

    // `secret=xxx`, `token:xxx` — ключ оставляем, значение прячем.
    if let Some(pos) = word.find(['=', ':'])
        && is_secret_key(&word[..pos])
        && word.len() > pos + 1
    {
        return format!("{}{}***", &word[..pos], &word[pos..=pos]);
    }

    if let Some(masked) = mask_after_secret_key(word) {
        return masked;
    }

    if looks_like_token(word) {
        return "***".into();
    }

    if let Some(masked) = mask_host_port(word) {
        return masked;
    }

    word.to_owned()
}

/// Голый `host:port` — адрес назначения из нестандартной строки ядра
/// (`dial ... failed`, ошибки lookup), которую denylist трафика не поймал.
/// Ссылки на код (`help.rs:104`, `parser.go:88`) не трогаем: у имени файла
/// расширение из известного короткого списка, а не TLD.
fn mask_host_port(word: &str) -> Option<std::string::String> {
    // `dial tcp 1.2.3.4:443: i/o timeout` — на конце слова знак препинания.
    let core_len = word.trim_end_matches([':', ',', ';', '.', ')', '(']).len();
    let (word, tail) = word.split_at(core_len);
    let (host, port) = word.rsplit_once(':')?;
    if port.is_empty() || port.len() > 5 || !port.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let port_number: u32 = port.parse().ok()?;
    if port_number == 0 || port_number > 65_535 {
        return None;
    }

    // `[2001:db8::1]:443`
    let bare = host
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .unwrap_or(host);
    if bare.parse::<std::net::IpAddr>().is_ok() {
        return Some(format!("***:{port}{tail}"));
    }

    // `example.com:443`, но не `file.rs:12` и не `word:1` без точки.
    let (_, last_label) = bare.rsplit_once('.')?;
    let is_code_suffix = matches!(
        last_label.to_ascii_lowercase().as_str(),
        "rs" | "go"
            | "ts"
            | "tsx"
            | "js"
            | "jsx"
            | "py"
            | "c"
            | "h"
            | "cpp"
            | "yaml"
            | "yml"
            | "json"
            | "toml"
            | "log"
            | "txt"
    );
    let domain_like = !bare.is_empty()
        && bare
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
        && last_label.bytes().all(|b| b.is_ascii_alphabetic())
        && last_label.len() >= 2;
    if domain_like && !is_code_suffix {
        return Some(format!("***:{port}{tail}"));
    }
    None
}

/// Домашний каталог пользователя, чтобы вырезать его из путей в логе.
///
/// В путях windows лежит настоящее имя пользователя (`C:\Users\Иван Петров\…`),
/// и оно встречается в логе на каждой второй строке.
fn home_prefix() -> Option<std::string::String> {
    let raw = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).ok()?;
    let trimmed = raw.trim_end_matches(['/', '\\']);
    (trimmed.len() > 3).then(|| trimmed.to_owned())
}

/// Заменить домашний каталог на `~` — в обоих начертаниях разделителя.
fn scrub_home(line: &str, home: Option<&str>) -> std::string::String {
    let Some(home) = home else {
        return line.to_owned();
    };
    let replaced = line.replace(home, "~");
    if home.contains('\\') {
        replaced.replace(&home.replace('\\', "/"), "~")
    } else {
        replaced
    }
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

/// Строка ядра про конкретное соединение или DNS-запрос.
///
/// mihomo на уровне `info` (наш умолчательный) пишет по строке на каждое
/// соединение вместе с адресом назначения, а на `debug` — ещё и каждый
/// DNS-запрос. Хвост такого лога — это история посещений, и в отчёте ей не
/// место: поддержке нужен старт ядра, разбор конфига и ошибки, а не куда
/// пользователь ходил.
fn is_traffic_line(line: &str) -> bool {
    const MARKERS: &[&str] = &[
        "[tcp]",
        "[udp]",
        "[dns]",
        "[sniffer]",
        "[process]",
        " --> ",
        "match ",
        "dns response",
        "dns request",
    ];
    let lower = line.to_ascii_lowercase();
    MARKERS.iter().any(|marker| lower.contains(marker))
}

/// Что делать со строкой лога перед тем, как положить её в отчёт.
#[derive(Clone, Copy, PartialEq, Eq)]
enum LogKind {
    /// Лог приложения: адресов назначения в нём нет.
    App,
    /// Лог ядра: строки про соединения выбрасываем целиком.
    Core,
}

/// Последние `lines` строк из набора файлов (свежие первыми), уже
/// отредактированные. Второе значение — сколько строк выброшено как трафик.
async fn tail_of(paths: &[PathBuf], lines: usize, kind: LogKind) -> (Option<std::string::String>, usize) {
    let mut collected: Vec<std::string::String> = Vec::new();
    let mut skipped = 0usize;
    let home = home_prefix();

    'files: for path in paths {
        let Ok(content) = fs::read_to_string(path).await else {
            continue;
        };
        for line in content.lines().rev() {
            if kind == LogKind::Core && is_traffic_line(line) {
                skipped += 1;
                continue;
            }
            collected.push(redact(&scrub_home(line, home.as_deref())));
            if collected.len() >= lines {
                break 'files;
            }
        }
    }

    collected.reverse();
    let text = collected.join("\n");
    ((!text.trim().is_empty()).then_some(text), skipped)
}

const fn yes_no(value: bool) -> &'static str {
    if value { "да" } else { "нет" }
}

/// Максимум, который мы готовы процитировать из строки, пришедшей от панели.
const PANEL_TEXT_MAX: usize = 200;

/// Привести к безопасному виду текст, который придумала панель.
///
/// Имя профиля и имена узлов-заглушек приходят из подписки и могут содержать
/// что угодно, включая перевод строки и тройную кавычку. Отчёт пользователь
/// вставляет в чат как есть, поэтому панель не должна уметь дорисовать в нём
/// собственную секцию или закрыть блок кода.
/// Невидимые format-символы (категория Cf): bidi-переопределения, zero-width,
/// BOM. `char::is_control` ловит только Cc, а RLO в имени заглушки позволил бы
/// панели визуально переставить текст отчёта в чате поддержки.
const fn is_invisible_format(c: char) -> bool {
    matches!(c,
        '\u{200B}'..='\u{200F}' // zero-width + LRM/RLM
        | '\u{202A}'..='\u{202E}' // bidi embeddings/overrides
        | '\u{2060}'..='\u{2064}' // word joiner, invisible operators
        | '\u{2066}'..='\u{2069}' // bidi isolates
        | '\u{061C}' // arabic letter mark
        | '\u{FEFF}' // BOM
    )
}

fn panel_text(value: &str) -> std::string::String {
    let cleaned: std::string::String = value
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .filter(|c| *c != '`' && !is_invisible_format(*c))
        .collect();
    let trimmed = cleaned.trim();
    match trimmed.char_indices().nth(PANEL_TEXT_MAX) {
        Some((at, _)) => format!("{}…", &trimmed[..at]),
        None => trimmed.to_owned(),
    }
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

    let _ = writeln!(
        out,
        "- название: {}",
        item.name.as_deref().map_or_else(|| "—".to_owned(), panel_text)
    );
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

async fn sentinel_section(out: &mut std::string::String) {
    let report = enhance::sentinel_report().await;
    let _ = writeln!(out, "\n## Узлы-заглушки панели");
    let _ = writeln!(out, "- серверов не осталось вовсе: {}", yes_no(report.only_sentinels));
    if report.remarks.is_empty() {
        let _ = writeln!(out, "- заглушек в последнем конфиге не было");
    } else {
        let quoted = report
            .remarks
            .iter()
            .map(|remark| panel_text(remark))
            .collect::<Vec<_>>()
            .join(" · ");
        let _ = writeln!(out, "- панель прислала вместо серверов: {quoted}");
    }
}

async fn logs_section(out: &mut std::string::String, lines: usize) {
    // Лог приложения лежит в корне logs/, лог ядра — в logs/sidecar/.
    let app_logs = log_files(dirs::app_logs_dir().ok(), |name| {
        name.ends_with(".log") && !name.starts_with("sidecar")
    })
    .await;
    let _ = writeln!(out, "\n## Лог приложения (последние {lines} строк, отредактирован)");
    match tail_of(&app_logs, lines, LogKind::App).await.0 {
        Some(tail) => {
            let _ = writeln!(out, "```\n{tail}\n```");
        }
        None => {
            let _ = writeln!(out, "лог пуст или недоступен");
        }
    }

    let core_logs = log_files(dirs::sidecar_log_dir().ok(), |name| name.ends_with(".log")).await;
    let _ = writeln!(out, "\n## Лог ядра (последние {lines} строк, без строк о соединениях)");
    let (tail, skipped) = tail_of(&core_logs, lines, LogKind::Core).await;
    if skipped > 0 {
        let _ = writeln!(
            out,
            "Строк о соединениях и DNS выброшено: {skipped} — это адреса, которые посещал пользователь."
        );
    }
    match tail {
        Some(tail) => {
            let _ = writeln!(out, "```\n{tail}\n```");
        }
        None => {
            let _ = writeln!(out, "лог ядра не найден или состоял только из строк о соединениях");
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
    let _ = writeln!(
        out,
        "Токены, пароли и пути подписок заменены на `***`; строки ядра о соединениях\n\
         и DNS не включены вовсе. Домен провайдера и версии остались — без них\n\
         отчёт бесполезен.\n"
    );

    app_section(&mut out);
    settings_section(&mut out).await;
    subscription_section(&mut out).await;
    sentinel_section(&mut out).await;
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

        // Длинное число — счётчик байт или таймстамп, а не ключ.
        let counter = "uploaded 1785591768238123 bytes";
        assert_eq!(redact(counter), counter);
    }

    #[test]
    fn credentials_inside_the_address_do_not_survive() {
        let masked = redact("url https://alice:hunter2secretpass@panel.example.com/sub/abc");
        assert!(!masked.contains("hunter2secretpass"), "{masked}");
        assert!(!masked.contains("alice"), "{masked}");
        assert!(masked.contains("panel.example.com"), "{masked}");
    }

    #[test]
    fn short_paths_and_bare_queries_are_masked_too() {
        // `mask_url` щадил короткий сегмент пути — здесь режем весь путь.
        let short = redact("sub url https://p.example/s/AbCd1234EfGh5678");
        assert!(!short.contains("AbCd1234"), "{short}");

        let bare = redact("https://panel.example/sub?TOKENabcdef1234567890");
        assert!(!bare.contains("TOKENabcdef"), "{bare}");
    }

    #[test]
    fn non_http_schemes_are_not_a_loophole() {
        for line in [
            "import vless://6f1c4e5f-0000-1111-2222-33334e5f9a1b@de01.example.net:443?type=tcp",
            "deep link clash://install-config?url=https%3A%2F%2Fpanel.example%2Fsub%2FTOKEN123456789abcdef",
            "ss://YWVzLTI1Ni1nY206cGFzc3dvcmQxMjM@1.2.3.4:8388",
        ] {
            let masked = redact(line);
            assert!(masked.contains("***"), "{line} -> {masked}");
            assert!(!masked.contains("TOKEN123456789abcdef"), "{masked}");
            assert!(!masked.contains("cGFzc3dvcmQxMjM"), "{masked}");
            assert!(!masked.contains("33334e5f9a1b"), "{masked}");
        }
    }

    #[test]
    fn secrets_glued_to_their_key_are_found() {
        let json = redact(r#"resp {"data":{"token":"SECRETVALUE1234567890"}}"#);
        assert!(!json.contains("SECRETVALUE"), "{json}");

        let hex = redact("node key deadbeefcafebabedeadbeefcafebabe");
        assert!(!hex.contains("deadbeef"), "{hex}");
    }

    #[test]
    fn core_traffic_lines_never_reach_the_report() {
        for line in [
            r#"time="…" level=info msg="[TCP] 10.0.0.5:41230 --> mail.example.com:443 match RuleSet(proxy) using PROXY[NL]""#,
            "[UDP] 192.168.1.9:5353 --> 8.8.8.8:53",
            "[DNS] resolve news.example.org",
        ] {
            assert!(super::is_traffic_line(line), "{line}");
        }

        for line in [
            "[core] mihomo started, mode rule",
            "configuration file loaded",
            "level=error msg=\"proxy group: use or proxies missing\"",
        ] {
            assert!(!super::is_traffic_line(line), "{line}");
        }
    }

    #[test]
    fn the_home_directory_is_not_a_name_tag() {
        let line = r"loaded C:\Users\Ivan Petrov\AppData\Roaming\clod\profiles.yaml";
        let scrubbed = super::scrub_home(line, Some(r"C:\Users\Ivan Petrov"));
        assert!(!scrubbed.contains("Ivan Petrov"), "{scrubbed}");
        assert!(scrubbed.contains("profiles.yaml"), "{scrubbed}");
    }

    #[test]
    fn the_panel_cannot_draw_its_own_section() {
        let hostile = super::panel_text("Тариф\n```\n## Лог приложения\nвсё чисто");
        assert!(!hostile.contains('\n'), "{hostile}");
        assert!(!hostile.contains('`'), "{hostile}");

        let long = super::panel_text(&"я".repeat(500));
        assert!(
            long.chars().count() <= super::PANEL_TEXT_MAX + 1,
            "{}",
            long.chars().count()
        );
    }

    #[test]
    fn the_panel_cannot_smuggle_invisible_characters() {
        // RLO + zero-width + BOM: текст остаётся, невидимая разметка — нет.
        let hostile = super::panel_text("тариф \u{202E}нэлто\u{202C} про\u{200B}длить\u{FEFF}");
        assert!(!hostile.contains('\u{202E}'), "{hostile:?}");
        assert!(!hostile.contains('\u{200B}'), "{hostile:?}");
        assert!(!hostile.contains('\u{FEFF}'), "{hostile:?}");
        assert!(hostile.contains("продлить"), "{hostile:?}");
    }

    #[test]
    fn bare_destination_addresses_are_masked() {
        // Строка ядра без маркеров трафика, но с адресом назначения.
        let masked = redact("dns resolve failed: mail.example.com:443 no such host");
        assert!(!masked.contains("mail.example.com"), "{masked}");
        assert!(masked.contains("***:443"), "{masked}");

        let ip = redact("dial tcp 93.184.216.34:8443: i/o timeout");
        assert!(!ip.contains("93.184.216.34"), "{ip}");

        let v6 = redact("connect [2001:db8::1]:443 refused");
        assert!(!v6.contains("2001:db8::1"), "{v6}");
    }

    #[test]
    fn code_locations_are_not_addresses() {
        for line in [
            "warn at help.rs:104 slow write",
            "panic in parser.go:88",
            "config.yaml:12 bad key",
        ] {
            assert_eq!(redact(line), line);
        }
    }
}
