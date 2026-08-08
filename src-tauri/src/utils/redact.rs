//! clod: редакция секретов — одна точка на всё, что мы записываем.
//!
//! Раньше это жило внутри отчёта для поддержки: логи писались сырыми, а
//! маскировались только при выгрузке наружу. Держать так не вышло —
//! `mask_url` звали руками примерно в двадцати местах, каждое новое сообщение
//! приходилось помнить отдельно, а строки ядра форвардились в файл как есть,
//! вместе с адресом подписки, токеном и списком посещённых узлов. Достаточно
//! было отдать кому-то папку логов.
//!
//! Теперь редакция стоит в самом логгере, на форматтере, — то есть на пути
//! КАЖДОЙ строки, попадающей в файл. Отчёт для поддержки продолжает звать те
//! же функции: строка, уже прошедшая редакцию, второй проход переживает без
//! изменений.
//!
//! Секрет — это не только токен. Ядро на уровне `info` пишет строку на каждое
//! соединение (`[TCP] … --> mail.example.com:443 match …`), то есть сырой лог
//! это ещё и список адресов, куда ходил пользователь.
//!
//! Правило простое: лучше замазать лишнее, чем отдать наружу токен подписки.

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
pub fn home_prefix() -> Option<std::string::String> {
    let raw = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).ok()?;
    let trimmed = raw.trim_end_matches(['/', '\\']);
    (trimmed.len() > 3).then(|| trimmed.to_owned())
}

/// Заменить домашний каталог на `~` — в обоих начертаниях разделителя.
pub fn scrub_home(line: &str, home: Option<&str>) -> std::string::String {
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

#[cfg(test)]
mod tests {
    use super::redact;

    /// Отчёт для поддержки редактирует строки повторно — уже после логгера.
    /// Второй проход обязан быть тождественным, иначе `***` разъедало бы
    /// текст с каждым разом.
    #[test]
    fn redaction_survives_a_second_pass() {
        for line in [
            "fetch https://panel.example/sub/AbCd1234EfGh5678 failed",
            "[TCP] 192.168.1.5:51000 --> mail.example.com:443 match Rule",
            r#"resp {"token":"SECRETVALUE1234567890"}"#,
            "Authorization: Bearer eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxIn0.abcdefghij",
            "secret: hunter2hunter2hunter2",
        ] {
            let once = redact(line);
            assert_eq!(redact(&once), once, "{line}");
        }
    }

    /// То, ради чего редакция переехала в логгер: сырые строки ядра больше не
    /// ложатся в файл вместе с адресом подписки и списком посещённых узлов.
    #[test]
    fn core_lines_lose_the_destination_but_stay_readable() {
        let masked = redact("[TCP] 10.0.0.2:44100 --> mail.example.com:443 match GeoIP(private)");
        assert!(!masked.contains("mail.example.com"), "{masked}");
        assert!(masked.contains("match"), "{masked}");
        assert!(masked.contains("[TCP]"), "{masked}");
    }

    /// Короткий токен `mask_url` щадил — он маскировал только сегменты длиннее
    /// шестнадцати символов. У логгера такого послабления нет.
    #[test]
    fn short_tokens_are_masked_too() {
        let masked = redact("updating subscription https://panel.example/s/ab12cd");
        assert!(!masked.contains("ab12cd"), "{masked}");
        assert!(masked.contains("panel.example"), "{masked}");
    }
}
