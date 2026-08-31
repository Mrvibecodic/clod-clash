use crate::{
    config::{Config, IVerge},
    core::{CoreManager, manager::RunningMode},
    enhance,
    utils::{
        dirs, help, hwid,
        redact::{home_prefix, redact, scrub_home},
    },
};
use anyhow::Result;
use std::{fmt::Write as _, path::PathBuf};
use tokio::fs;

const LOG_TAIL_LINES: usize = 800;

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

    found.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));
    found.into_iter().map(|(_, path)| path).collect()
}

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

#[derive(Clone, Copy, PartialEq, Eq)]
enum LogKind {
    App,
    Core,
}

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

const PANEL_TEXT_MAX: usize = 200;

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
        "- системный прокси: {} (адрес {}, PAC {})",
        yes_no(data.enable_system_proxy.unwrap_or(false)),
        data.proxy_host.as_deref().unwrap_or("127.0.0.1"),
        yes_no(data.proxy_auto_config.unwrap_or(false))
    );
    let _ = writeln!(
        out,
        "- защита прокси: {} (период {} с)",
        yes_no(data.enable_proxy_guard.unwrap_or(false)),
        data.proxy_guard_duration.unwrap_or(30)
    );
    let _ = writeln!(
        out,
        "- исключения прокси: встроенные {}, свои: {}",
        yes_no(data.use_default_bypass.unwrap_or(true)),
        match data.system_proxy_bypass.as_deref() {
            Some(bypass) if !bypass.is_empty() => bypass,
            _ => "—",
        }
    );
    let mode = Config::clash()
        .await
        .latest_arc()
        .0
        .get("mode")
        .and_then(serde_yaml_ng::Value::as_str)
        .unwrap_or("—")
        .to_owned();
    let _ = writeln!(out, "- режим ядра: {mode}");
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
        "- заголовки провайдера: логотип {}, объявление {}, промо {}, кабинет {}, поддержка {}",
        yes_no(item.logo.is_some()),
        yes_no(item.announce.is_some()),
        yes_no(item.promo.is_some()),
        yes_no(item.portal_url.is_some()),
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

    let described = enhance::server_descriptions().await.len();
    let _ = writeln!(
        out,
        "- узлов с описанием (serverDescription): {described}{}",
        if described == 0 {
            " — панель отдаёт его только клиентам из additionalExtendedClientsRegex (^ClodClash/)"
        } else {
            ""
        }
    );
}

fn core_log_dir() -> Option<PathBuf> {
    match *CoreManager::global().get_running_mode() {
        RunningMode::Service => dirs::service_log_dir().ok(),
        RunningMode::Sidecar | RunningMode::NotRunning => dirs::sidecar_log_dir().ok(),
    }
}

async fn core_tail_from_running_core(lines: usize) -> (Option<std::string::String>, usize) {
    let Ok(logs) = CoreManager::global().get_clash_logs().await else {
        return (None, 0);
    };

    let home = home_prefix();
    let mut collected: Vec<std::string::String> = Vec::new();
    let mut skipped = 0_usize;

    for line in logs.iter().rev() {
        if is_traffic_line(line.as_str()) {
            skipped += 1;
            continue;
        }
        collected.push(redact(&scrub_home(line.as_str(), home.as_deref())));
        if collected.len() >= lines {
            break;
        }
    }

    collected.reverse();
    let text = collected.join("\n");
    ((!text.trim().is_empty()).then_some(text), skipped)
}

async fn logs_section(out: &mut std::string::String, lines: usize) {
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

    let core_logs = log_files(core_log_dir(), |name| name.ends_with(".log")).await;
    let _ = writeln!(out, "\n## Лог ядра (последние {lines} строк, без строк о соединениях)");
    let (mut tail, mut skipped) = tail_of(&core_logs, lines, LogKind::Core).await;
    if tail.is_none() {
        let (from_core, also_skipped) = core_tail_from_running_core(lines).await;
        tail = from_core;
        skipped += also_skipped;
    }
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
            let _ = writeln!(
                out,
                "лог ядра не найден или состоял только из строк о соединениях (режим работы ядра: {})",
                CoreManager::global().get_running_mode()
            );
        }
    }
}

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
        let line =
            "[обновление подписки] URL = https://panel.example.com/sub/9f8e7d6c5b4a3f2e1d0c9b8a7?token=abcdef123456";
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
        let hostile = super::panel_text("тариф \u{202E}нэлто\u{202C} про\u{200B}длить\u{FEFF}");
        assert!(!hostile.contains('\u{202E}'), "{hostile:?}");
        assert!(!hostile.contains('\u{200B}'), "{hostile:?}");
        assert!(!hostile.contains('\u{FEFF}'), "{hostile:?}");
        assert!(hostile.contains("продлить"), "{hostile:?}");
    }

    #[test]
    fn bare_destination_addresses_are_masked() {
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
