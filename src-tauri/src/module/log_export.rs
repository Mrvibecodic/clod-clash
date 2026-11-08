use crate::{
    core::logger::line_stamp,
    utils::{dirs, redact::home_prefix},
};
use anyhow::{Context as _, Result};
use chrono::{NaiveDateTime, Timelike as _};
use std::{
    io::{BufRead, Write},
    path::{Path, PathBuf},
    time::SystemTime,
};
use zip::write::SimpleFileOptions;

/// Строки сеанса программы: от её запуска до выгрузки. Строка без отметки
/// времени (продолжение предыдущей) идёт вместе с предыдущей.
struct SessionWindow {
    started: NaiveDateTime,
    inside: bool,
}

impl SessionWindow {
    const fn new(started: NaiveDateTime) -> Self {
        Self { started, inside: false }
    }

    #[cfg(test)]
    const fn everything() -> Self {
        Self {
            started: NaiveDateTime::MIN,
            inside: true,
        }
    }

    fn keeps(&mut self, line: &str) -> bool {
        if let Some(stamp) = line_stamp(line) {
            self.inside = stamp >= self.started;
        }
        self.inside
    }
}

fn local(time: SystemTime) -> NaiveDateTime {
    chrono::DateTime::<chrono::Local>::from(time)
        .naive_local()
        .with_nanosecond(0)
        .unwrap_or_default()
}

fn written_since(path: &Path, started: SystemTime) -> bool {
    std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .is_ok_and(|modified| modified >= started)
}

fn is_log_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("log"))
}

fn collect_logs(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        // `temp` — копии, которые «Открыть журнал» снимает с этих же файлов.
        if path.is_dir() && entry.file_name() != "temp" {
            collect_logs(&path, out);
        } else if is_log_file(&path) {
            out.push(path);
        }
    }
}

fn is_core_log(root: &Path, path: &Path) -> bool {
    path.strip_prefix(root)
        .ok()
        .and_then(|rel| rel.components().next())
        .and_then(|part| match part {
            std::path::Component::Normal(name) => name.to_str(),
            _ => None,
        })
        .is_some_and(|dir| dir == "sidecar" || dir == "service")
}

/// Один источник строк журнала: свой срез сеанса и время последней строки с
/// отметкой — строка без отметки идёт следом за предыдущей.
struct Source<'a> {
    lines: std::io::Split<Box<dyn BufRead + 'a>>,
    window: SessionWindow,
    last: NaiveDateTime,
    head: Option<(NaiveDateTime, String)>,
}

impl<'a> Source<'a> {
    fn new(reader: Box<dyn BufRead + 'a>, window: SessionWindow) -> std::io::Result<Self> {
        let mut source = Self {
            lines: reader.split(b'\n'),
            last: window.started,
            window,
            head: None,
        };
        source.advance()?;
        Ok(source)
    }

    fn advance(&mut self) -> std::io::Result<()> {
        self.head = None;
        for raw in self.lines.by_ref() {
            let raw = raw?;
            let line = String::from_utf8_lossy(&raw).trim_end_matches('\r').to_owned();
            if !self.window.keeps(&line) {
                continue;
            }
            self.last = line_stamp(&line).unwrap_or(self.last);
            self.head = Some((self.last, line));
            break;
        }
        Ok(())
    }
}

/// Строки источников из окна сеанса, сшитые по времени, — в архив по одной,
/// без чтения файлов целиком; сколько строк записано (ни одной — файл в архив
/// не идёт). Второй источник журнала ядра — файлы соединений (под службой и
/// без неё): на диске они держат последние час-два.
fn export_merged<'a>(
    readers: Vec<Box<dyn BufRead + 'a>>,
    window: impl Fn() -> SessionWindow,
    home: Option<&str>,
    core: bool,
    out: &mut impl Write,
) -> std::io::Result<usize> {
    let mut sources = readers
        .into_iter()
        .map(|reader| Source::new(reader, window()))
        .collect::<std::io::Result<Vec<_>>>()?;
    if sources.iter().all(|source| source.head.is_none()) {
        return Ok(0);
    }
    let mut written = 0usize;
    if let Some(connections) = sources.get(1) {
        match &connections.head {
            Some((since, _)) => writeln!(
                out,
                "[export] connection and DNS lines are kept on the device for the last 1-2 hours only; here they start at {since}"
            )?,
            None => writeln!(out, "[export] no connection or DNS lines in the last 1-2 hours")?,
        }
    }
    while let Some(next) = sources
        .iter()
        .enumerate()
        .filter_map(|(at, source)| source.head.as_ref().map(|(stamp, _)| (*stamp, at)))
        .min()
        .map(|(_, at)| at)
    {
        if let Some((_, line)) = sources[next].head.take() {
            out.write_all(crate::module::support_bundle::archive_line(&line, home, core).as_bytes())?;
            out.write_all(b"\n")?;
            written += 1;
        }
        sources[next].advance()?;
    }
    Ok(written)
}

fn open_chain(paths: &[PathBuf]) -> Option<Box<dyn BufRead>> {
    use std::io::Read as _;
    let mut files = paths.iter().filter_map(|path| std::fs::File::open(path).ok());
    let first = files.next()?;
    let reader: Box<dyn std::io::Read> = match files.next() {
        Some(second) => Box::new(first.chain(second)),
        None => Box::new(first),
    };
    Some(Box::new(std::io::BufReader::new(reader)))
}

fn file_name(path: &Path) -> &str {
    path.file_name().and_then(|name| name.to_str()).unwrap_or_default()
}

fn is_connections_file(path: &Path) -> bool {
    crate::module::support_bundle::is_connections_file(file_name(path))
}

fn is_run_copy(path: &Path) -> bool {
    crate::module::core_log_archive::is_run_copy(file_name(path))
}

/// Файлы соединений, которые сшиваются с этим файлом журнала ядра: у копии
/// запуска под службой — свои `.conn`, у текущего журнала ядра без службы —
/// `connections_*` того же каталога (прошлый час и текущий, по порядку).
fn connections_of(path: &Path, files: &[PathBuf]) -> Vec<PathBuf> {
    if is_run_copy(path) {
        return [
            crate::module::core_log_archive::older_connections_path(path),
            crate::module::core_log_archive::connections_path(path),
        ]
        .into_iter()
        .filter(|part| part.exists())
        .collect();
    }
    if file_name(path) != "sidecar_latest.log" {
        return Vec::new();
    }
    let mut connections: Vec<PathBuf> = files
        .iter()
        .filter(|file| file.parent() == path.parent() && file_name(file).starts_with("connections_"))
        .cloned()
        .collect();
    connections.sort();
    connections.split_off(connections.len().saturating_sub(2))
}

fn archive_name(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .filter_map(|part| match part {
            std::path::Component::Normal(name) => Some(name.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

pub async fn write_archive(target: PathBuf) -> Result<usize> {
    let report = crate::module::support_bundle::build().await?;
    let app_logs = dirs::app_logs_dir()?;
    let service_logs = dirs::service_logs_root_dir()?;
    let started = crate::core::logger::session_started();

    tokio::task::spawn_blocking(move || {
        let home = home_prefix();
        let mut files = Vec::new();
        collect_logs(&app_logs, &mut files);
        if service_logs != app_logs {
            collect_logs(&service_logs, &mut files);
        }
        files.sort();
        files.dedup();

        let file = std::fs::File::create(&target)
            .with_context(|| clash_verge_i18n::t!("errors.fileNotCreated", path = target.display().to_string()))?;
        let mut zip = zip::ZipWriter::new(file);
        let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

        zip.start_file("report.md", options)?;
        zip.write_all(report.as_bytes())?;

        let mut count = 0usize;
        for path in &files {
            if is_connections_file(path) {
                continue;
            }
            let connections = connections_of(path, &files);
            if !std::iter::once(path)
                .chain(&connections)
                .any(|part| written_since(part, started))
            {
                continue;
            }
            let Some(main) = open_chain(std::slice::from_ref(path)) else {
                continue;
            };
            let mut readers = vec![main];
            readers.extend(open_chain(&connections));
            let root = if path.starts_with(&app_logs) {
                &app_logs
            } else {
                &service_logs
            };
            zip.start_file(archive_name(root, path), options)?;
            let written = export_merged(
                readers,
                || SessionWindow::new(local(started)),
                home.as_deref(),
                is_core_log(root, path),
                &mut zip,
            )?;
            // Файл, начатый этим сеансом, но ещё пустой (ядро в другом режиме) —
            // пустой записи в архиве не оставляет.
            if written == 0 {
                zip.abort_file()?;
                continue;
            }
            count += 1;
        }
        zip.finish()?;
        Ok(count)
    })
    .await?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exported(readers: Vec<&str>, window: impl Fn() -> SessionWindow, core: bool) -> String {
        let mut out = Vec::new();
        let readers = readers
            .into_iter()
            .map(|raw| Box::new(raw.as_bytes()) as Box<dyn BufRead>)
            .collect();
        let written = export_merged(readers, window, None, core, &mut out);
        assert!(written.is_ok());
        let text = String::from_utf8_lossy(&out).into_owned();
        let lines = text.lines().filter(|line| !line.starts_with("[export]")).count();
        assert_eq!(written.unwrap_or_default(), lines, "{text}");
        text
    }

    fn redacted(raw: &str, home: Option<&str>, core: bool) -> (String, usize) {
        let mut out = Vec::new();
        let readers = vec![Box::new(raw.as_bytes()) as Box<dyn BufRead>];
        let _ = export_merged(readers, SessionWindow::everything, home, core, &mut out);
        (String::from_utf8_lossy(&out).into_owned(), 0)
    }

    #[test]
    fn archive_names_are_relative_with_forward_slashes() {
        let root = Path::new("/logs");
        assert_eq!(archive_name(root, Path::new("/logs/sidecar/a.log")), "sidecar/a.log");
        assert_eq!(archive_name(root, Path::new("/elsewhere/b.log")), "elsewhere/b.log");
    }

    #[test]
    fn only_log_files_are_picked() {
        assert!(is_log_file(Path::new("latest.log")));
        assert!(is_log_file(Path::new("x.LOG")));
        assert!(!is_log_file(Path::new("config.yaml")));
    }

    #[test]
    fn the_archive_takes_the_whole_session_and_nothing_before_it() {
        let started = line_stamp("[2026-11-08 16:06:29.000]").unwrap_or_default();
        let log = "[2026-11-08 15:54:16.554] INFO previous session\n\
                   continuation of the previous line\n\
                   [2026-11-08 16:06:29.763] INFO this session\n\
                   continuation of this session\n\
                   [2026-11-08 18:00:00.000] INFO much later\n";
        let mut window = SessionWindow::new(started);
        let kept: Vec<&str> = log.lines().filter(|line| window.keeps(line)).collect();
        assert_eq!(
            kept,
            vec![
                "[2026-11-08 16:06:29.763] INFO this session",
                "continuation of this session",
                "[2026-11-08 18:00:00.000] INFO much later",
            ]
        );
        assert_eq!(line_stamp("no stamp"), None);
    }

    #[test]
    fn copies_made_for_viewing_are_not_exported_twice() {
        let root = std::env::temp_dir().join(format!("clod-log-collect-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for dir in ["temp", "sidecar/temp", "service"] {
            let _ = std::fs::create_dir_all(root.join(dir));
        }
        for file in [
            "latest.log",
            "temp/latest_1.log",
            "sidecar/sidecar_latest.log",
            "sidecar/temp/x.log",
            "service/core_1.log",
        ] {
            let _ = std::fs::write(root.join(file), b"line\n");
        }
        let mut found = Vec::new();
        collect_logs(&root, &mut found);
        let mut names: Vec<String> = found.iter().map(|path| archive_name(&root, path)).collect();
        names.sort();
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(
            names,
            vec!["latest.log", "service/core_1.log", "sidecar/sidecar_latest.log"]
        );
    }

    #[test]
    fn the_run_and_its_connections_are_stitched_by_time_within_the_session() {
        let started = line_stamp("[2026-11-08 16:06:29]").unwrap_or_default();
        let run = "[2026-11-08 16:00:00.000] previous session\r\n\
                   [2026-11-08 16:06:40.000] TUN started\n\
                   [2026-11-08 16:08:00.000] warning\n\
                   continuation\n";
        let connections = "[2026-11-08 16:07:00.000] [TCP] 10.0.0.5:41230(chrome.exe) --> mail.example.com:443 match DomainSuffix(example.com) using PROXY[NL]\n\
                           [2026-11-08 16:09:00.000] [DNS] news.example.org --> 93.184.216.34\n";
        let text = exported(vec![run, connections], || SessionWindow::new(started), true);
        assert_eq!(
            text,
            "[export] connection and DNS lines are kept on the device for the last 1-2 hours only; here they start at 2026-11-08 16:07:00\n\
             [2026-11-08 16:06:40.000] TUN started\n\
             [2026-11-08 16:07:00.000] [TCP] ***:41230 --> ***:443 match DomainSuffix(***) using PROXY[NL]\n\
             [2026-11-08 16:08:00.000] warning\n\
             continuation\n\
             [2026-11-08 16:09:00.000] [DNS] *** --> ***\n"
        );
        let quiet = exported(vec![run, ""], || SessionWindow::new(started), true);
        assert!(
            quiet.starts_with("[export] no connection or DNS lines in the last 1-2 hours\n"),
            "{quiet}"
        );
        // Ни одной строки сеанса ни в журнале, ни в соединениях — в архив ничего, даже заголовка
        let before = line_stamp("[2026-11-09 00:00:00]").unwrap_or_default();
        assert_eq!(
            exported(vec![run, connections], || SessionWindow::new(before), true),
            ""
        );
    }

    #[test]
    fn the_sidecar_log_is_stitched_with_its_connection_files() {
        let dir = std::env::temp_dir().join(format!("clod-sidecar-conn-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let sidecar = dir.join("sidecar");
        let service = dir.join("service");
        let _ = std::fs::create_dir_all(&sidecar);
        let _ = std::fs::create_dir_all(&service);
        let files: Vec<PathBuf> = [
            "sidecar/connections_2026-11-08_14-00-00.log",
            "sidecar/connections_2026-11-08_15-00-00.log",
            "sidecar/connections_latest.log",
            "sidecar/sidecar_2026-11-08_10-00-00.log",
            "sidecar/sidecar_latest.log",
            "service/core_2026-11-08_09-00-00.log",
            "service/core_2026-11-08_09-00-00.conn.log",
            "service/core_2026-11-08_09-00-00.conn.1.log",
            "latest.log",
        ]
        .into_iter()
        .map(|name| dir.join(name))
        .collect();
        for file in &files {
            let _ = std::fs::write(file, b"line\n");
        }
        let of = |name: &str| connections_of(&dir.join(name), &files);
        let stitched = (
            of("sidecar/sidecar_latest.log"),
            of("sidecar/sidecar_2026-11-08_10-00-00.log"),
            of("service/core_2026-11-08_09-00-00.log"),
            of("latest.log"),
        );
        let standalone: Vec<bool> = files.iter().map(|file| is_connections_file(file)).collect();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(
            stitched.0,
            vec![
                dir.join("sidecar/connections_2026-11-08_15-00-00.log"),
                dir.join("sidecar/connections_latest.log")
            ]
        );
        assert!(stitched.1.is_empty(), "{:?}", stitched.1);
        assert_eq!(
            stitched.2,
            vec![
                dir.join("service/core_2026-11-08_09-00-00.conn.1.log"),
                dir.join("service/core_2026-11-08_09-00-00.conn.log")
            ]
        );
        assert!(stitched.3.is_empty(), "{:?}", stitched.3);
        assert_eq!(
            standalone,
            vec![true, true, true, false, false, false, true, true, false]
        );
    }

    #[test]
    fn every_line_is_redacted() {
        let (text, skipped) = redacted("a\nb\n", None, false);
        assert_eq!(text, "a\nb\n");
        assert_eq!(skipped, 0);
    }

    #[test]
    fn only_core_logs_lose_traffic_lines() {
        let raw = "signature does not match the bytes\n[TCP] 10.0.0.5:41230 --> mail.example.com:443 match Rule\n";
        let (text, skipped) = redacted(raw, None, false);
        assert_eq!(skipped, 0);
        assert!(text.contains("signature does not match the bytes"), "{text}");
        let root = Path::new("/logs");
        assert!(is_core_log(root, Path::new("/logs/sidecar/sidecar_latest.log")));
        assert!(is_core_log(root, Path::new("/logs/service/service_latest.log")));
        assert!(!is_core_log(root, Path::new("/logs/latest.log")));
        assert!(!is_core_log(root, Path::new("/elsewhere/sidecar/a.log")));
    }

    #[test]
    fn dial_errors_keep_the_reason_but_not_the_addresses() {
        let cases = [
            (
                r#"time="t" level=warning msg="[TCP] dial Авто выбор (match DomainSuffix/mail.example.com) 127.0.0.1:52100(Mail App.exe) --> mail.example.com:443 error: dial tcp 93.184.216.34:443: i/o timeout""#,
                "[TCP] dial Авто выбор --> ***:443 error: dial tcp ***:443: i/o timeout",
            ),
            (
                "[TCP] dial PROXY[NL] 1 127.0.0.1:52100(chrome.exe) --> news.example.org:8443 error: context deadline exceeded",
                "[TCP] dial PROXY[NL] 1 --> ***:8443 error: context deadline exceeded",
            ),
            (
                "[TCP] dial Узел 2 mihomo --> rules.example.net:443 error: EOF",
                "[TCP] dial Узел 2 --> ***:443 error: EOF",
            ),
            (
                "[UDP] dial DIRECT (match Match/) [::1]:5353 --> [2001:db8::1]:443 error: lookup news.example.org: no such host",
                "[UDP] dial DIRECT --> ***:443 error: lookup ***: no such host",
            ),
        ];
        let leaks = [
            "example.",
            "93.184",
            "2001:db8",
            "52100",
            "5353",
            "chrome",
            "Mail App",
            "DomainSuffix",
            "mihomo",
        ];
        for (raw, kept) in cases {
            let (text, skipped) = redacted(raw, None, true);
            assert_eq!(skipped, 0, "{raw}");
            assert!(text.contains(kept), "{text}");
            for leak in leaks {
                assert!(!text.contains(leak), "{leak} in {text}");
            }
        }
    }

    #[test]
    fn a_dial_error_from_a_redacted_copy_keeps_its_reason() {
        let copy = crate::utils::redact::core_line_for_disk(
            "[TCP] dial PROXY[NL] 1 10.0.0.2:52100 --> news.example.org:8443 error: context deadline exceeded",
            None,
        );
        let (text, skipped) = redacted(&copy, None, true);
        assert_eq!(skipped, 0, "{copy}");
        assert!(
            text.contains("[TCP] dial PROXY[NL] 1 --> ***:8443 error: context deadline exceeded"),
            "{text}"
        );
    }

    #[test]
    fn unrecognised_dial_lines_keep_no_addresses() {
        let raw = "[TCP] dial PROXY broken --> mail.example.com:443\n[TCP] dial chrome opened mail.example.com:443 error: EOF\n";
        let (text, _) = redacted(raw, None, true);
        assert_eq!(text.lines().count(), 2, "{text}");
        assert!(!text.contains("example."), "{text}");
    }

    #[test]
    fn connection_lines_reach_the_archive_without_addresses_or_programs() {
        let raw = "[core] mihomo started\n\
                   [TCP] 10.0.0.5:41230(Mail App.exe) --> mail.example.com:443 match GeoSite(category-mail) using Авто[NL]\n\
                   [UDP] [::1]:5353 --> [2001:db8::1]:443 doesn't match any rule using DIRECT\n\
                   [DNS] resolve news.example.org A from 93.184.216.34\n\
                   [Sniffer] Sniff TCP [198.18.0.1:52134]-->[93.184.216.34:443] success\n\
                   configuration loaded\n";
        let (text, _) = redacted(raw, None, true);
        assert_eq!(text.lines().count(), 6, "{text}");
        for kept in [
            "mihomo started",
            "[TCP] ***:41230 --> ***:443 match GeoSite(***) using Авто[NL]",
            "using DIRECT",
            "configuration loaded",
        ] {
            assert!(text.contains(kept), "{kept} not in {text}");
        }
        for leak in [
            "example.",
            "93.184",
            "2001:db8",
            "10.0.0.5",
            "Mail App",
            "category-mail",
        ] {
            assert!(!text.contains(leak), "{leak} in {text}");
        }
    }
}
