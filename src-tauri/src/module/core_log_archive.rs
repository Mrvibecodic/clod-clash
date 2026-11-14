//! Журнал ядра под службой — по файлу на запуск ядра в каталоге журналов
//! приложения.
//!
//! Служба пишет журнал ядра в свой закрытый каталог и заводит там новый файл
//! на каждый запуск ядра, на собственный старт и по размеру; приложению она
//! отдаёт только последние 4 МБ текущего файла и только пока ядро запущено
//! этим пользователем — после остановки ядра уже ничего. Поэтому приложение
//! копирует журнал к себе:
//! - перед остановкой ядра (выход, перезапуск, обновление), не дольше
//!   [`STOP_WAIT`], чтобы не съедать время выхода;
//! - перед тем как служба запустит новое ядро по нашей просьбе (запуск,
//!   обновление ядра);
//! - сразу, как сторож заметил, что служба подняла ядро сама: копии идут уже
//!   в файл нового запуска. Конец прошлого запуска служба к этому моменту не
//!   отдаёт — в копии он есть до последней плановой копии;
//! - раз в [`SAVE_EVERY`], пока ядро работает, а при большом потоке строк —
//!   раз в [`SAVE_BUSY`]: служба отдаёт лишь последние 4 МБ, и всё, что
//!   ушло дальше, потеряно; такой пропуск помечается в файле строкой;
//! - при выгрузке отчёта.
//!
//! Строки ложатся на диск отредактированными, как у нашего логгера: без
//! секретов, адресов и домашнего каталога. Копия дописывает в файл запуска
//! только новые строки. Файл, доросший до [`RUN_PART`], становится прошлой
//! частью запуска (`….1.log`), прежняя прошлая уходит: на диске конец запуска,
//! около [`CORE_LOG_CAP`]. Строки о соединениях
//! и DNS (девять десятых журнала) идут в соседний файл, который
//! меняется раз в [`CONNECTIONS_SPAN`], и на диске лежат последние один-два
//! таких промежутка — для разбора проблемы этого хватает. Хранится [`KEEP`] последних
//! запусков, кроме запусков текущего сеанса программы, файлы соединений — у
//! [`CONNECTIONS_RUNS`] последних запусков и у запусков этого сеанса; дальше
//! их чистит и общая уборка журналов по возрасту. Новый сеанс программы
//! продолжает файл запуска, который служба всё ещё пишет: его конец ищется в
//! снимке, и только если его там нет, запуск считается новым.

use std::path::{Path, PathBuf};
use std::time::Duration;

use clash_verge_logging::{Type, logging};
use parking_lot::Mutex;
use tokio::time::Instant;

use crate::{
    core::{CoreManager, handle, logger::CORE_LOG_CAP, manager::RunningMode},
    process::AsyncHandler,
    utils::{dirs, help::write_atomic, redact},
};

const SAVE_EVERY: Duration = Duration::from_secs(60);
const SAVE_BUSY: Duration = Duration::from_secs(15);
const BUSY_BYTES: usize = 1024 * 1024;
const TAIL_READ: u64 = 64 * 1024;
/// Сколько последних строк сверяется со снимком: одной мало — одинаковые
/// строки в одну миллисекунду (пачка одних и тех же ошибок) сбили бы место, с
/// которого снимок новый.
const TAIL_LINES: usize = 8;
const CONNECTIONS_SPAN: chrono::TimeDelta = chrono::TimeDelta::hours(1);
const CONNECTIONS_CAP: u64 = 32 * 1024 * 1024;
/// Запуск лежит двумя частями — текущей и прошлой, вместе около [`CORE_LOG_CAP`].
const RUN_PART: u64 = CORE_LOG_CAP / 2;
const OLDER_PART: &str = ".1.log";
const GAP_LINE: &str = "[clod] the service no longer had part of this core run's log; lines are missing here";
/// Дольше копия ответа службы не ждёт (её IPC повторяет запрос до 20 раз):
/// ни запуск ядра, ни отчёт не держатся за неотвечающую службу.
const ANSWER_WAIT: Duration = Duration::from_secs(3);
/// Перед остановкой ядра — меньше: у выхода свой срок на остановку.
const STOP_WAIT: Duration = Duration::from_secs(1);
const KEEP: usize = 10;
const CONNECTIONS_RUNS: usize = 2;
const PREFIX: &str = "core_";
/// Метка в каталоге копий: копии, записанные до редакции, уже переписаны.
const REDACTED_MARK: &str = ".redacted";

/// Файл запуска ядра, который сейчас пишет служба: все копии этого запуска
/// ложатся в него. Нет — имя даёт первая строка журнала.
static RUN: Mutex<Option<Run>> = Mutex::new(None);

/// Файл запуска и последние строки журнала службы, на которых остановилась
/// прошлая копия: по ним, а не по сохранённому, ищется начало новых строк —
/// сохраняется не всё, и сохранённое бывает уже за пределами снимка.
struct Run {
    name: String,
    seen: Vec<String>,
}
/// Копии идут по одной: каждая читает и дописывает файл запуска.
static SAVING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
/// Уборка в этом сеансе программы уже была: убирается только записанное до
/// сеанса, и что из этого лишнее, дальше меняет лишь файл нового запуска.
static PRUNED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Сохранить журнал текущего запуска ядра службы; `true` — файл записан.
/// Служба не ответила или журнал пуст — ничего.
pub async fn save() -> bool {
    save_within(ANSWER_WAIT).await.is_some()
}

/// Ядро вот-вот остановят: после остановки служба его журнал не отдаст.
pub async fn before_stop() {
    let _ = save_within(STOP_WAIT).await;
}

/// Служба вот-вот запустит новое ядро: журнал прошлого запуска — в его файл,
/// следующие копии — в новый.
pub async fn before_new_run() {
    let deadline = Instant::now() + ANSWER_WAIT;
    // Новое имя — под тем же замком, что и копия прошлого запуска.
    let saving = tokio::time::timeout_at(deadline, SAVING.lock()).await.ok();
    if saving.is_some() {
        let _ = save_locked(deadline).await;
    }
    *RUN.lock() = None;
    drop(saving);
}

/// Служба сама подняла ядро заново и пишет новый файл: копии — в файл нового
/// запуска.
pub async fn after_a_restart_by_the_service() {
    let deadline = Instant::now() + ANSWER_WAIT;
    let saving = tokio::time::timeout_at(deadline, SAVING.lock()).await.ok();
    *RUN.lock() = None;
    if saving.is_some() {
        let _ = save_locked(deadline).await;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Saved {
    gap: bool,
    fresh: usize,
}

const fn next_wait(saved: Option<Saved>) -> Duration {
    match saved {
        Some(saved) if saved.gap || saved.fresh >= BUSY_BYTES => SAVE_BUSY,
        _ => SAVE_EVERY,
    }
}

/// Копии, пока ядро работает под службой: раз в [`SAVE_EVERY`], при большом
/// потоке строк или после пропуска — раз в [`SAVE_BUSY`].
pub fn spawn() {
    AsyncHandler::spawn(|| async {
        redact_old_copies().await;
        let mut wait = SAVE_EVERY;
        loop {
            tokio::time::sleep(wait).await;
            wait = SAVE_EVERY;
            if handle::Handle::global().is_exiting()
                || !matches!(*CoreManager::global().get_running_mode(), RunningMode::Service)
            {
                continue;
            }
            wait = next_wait(save_within(ANSWER_WAIT).await);
        }
    });
}

/// Копия, если служба ответит до срока; `None` — служба не ответила или
/// журнал пуст.
async fn save_within(wait: Duration) -> Option<Saved> {
    let deadline = Instant::now() + wait;
    let _saving = tokio::time::timeout_at(deadline, SAVING.lock()).await.ok()?;
    save_locked(deadline).await
}

async fn save_locked(deadline: Instant) -> Option<Saved> {
    let text = match tokio::time::timeout_at(deadline, CoreManager::global().get_clash_log_snapshot()).await {
        Ok(Ok(text)) => text,
        Ok(Err(error)) => {
            logging!(debug, Type::Core, "the core log was not saved: {error:#}");
            return None;
        }
        Err(_) => {
            logging!(
                debug,
                Type::Core,
                "the core log was not saved: the service did not answer in time"
            );
            return None;
        }
    };
    if text.trim().is_empty() {
        return None;
    }
    let dir = dirs::service_log_dir().ok()?;
    let home = redact::home_prefix();
    let (name, seen, saved) = match known_run() {
        Some((name, seen)) if !seen.is_empty() => (name, seen, Vec::new()),
        // Запуск известен, а его снимка ещё не видели (первая копия не записалась):
        // новые строки — от сохранённого конца файла.
        Some((name, _)) => {
            let saved = last_lines_of(&dir.join(&name)).await;
            (name, Vec::new(), saved)
        }
        None => {
            let (name, saved) = run_continued_by(&text, &dir, home.as_deref()).await;
            RUN.lock().get_or_insert_with(|| Run {
                name: name.clone(),
                seen: Vec::new(),
            });
            (name, Vec::new(), saved)
        }
    };
    let path = dir.join(&name);
    let (lines, gap) = new_lines(&text, &seen, &saved, home.as_deref());
    let fresh: usize = lines.iter().map(|line| line.len() + 1).sum();
    let mut chunk = String::new();
    let mut connections = String::new();
    if gap {
        chunk.push_str(GAP_LINE);
        chunk.push('\n');
    }
    for line in &lines {
        let (clean, connection) = for_disk_line(line, home.as_deref());
        let out = if connection { &mut connections } else { &mut chunk };
        out.push_str(&clean);
        out.push('\n');
    }
    let new_run_file = !chunk.is_empty() && !tokio::fs::try_exists(&path).await.unwrap_or(false);
    let written = if chunk.is_empty() {
        Ok(())
    } else {
        append_to_the_run(&path, chunk.as_bytes(), RUN_PART).await
    };
    if let Err(error) = match written {
        Ok(()) => append_connections(&path, &connections).await,
        Err(error) => Err(error),
    } {
        logging!(
            warn,
            Type::Core,
            "the core log was not saved to {}: {error:#}",
            path.display()
        );
        return None;
    }
    if let Some(run) = RUN.lock().as_mut().filter(|run| run.name == name) {
        run.seen = last_raw_lines(&text);
    }
    if prune_is_due(new_run_file, PRUNED.swap(true, std::sync::atomic::Ordering::AcqRel)) {
        prune(&dir).await;
    }
    Some(Saved { gap, fresh })
}

const fn prune_is_due(new_run_file: bool, pruned_before: bool) -> bool {
    new_run_file || !pruned_before
}

fn known_run() -> Option<(String, Vec<String>)> {
    RUN.lock().as_ref().map(|run| (run.name.clone(), run.seen.clone()))
}

/// Файл запуска для снимка, когда текущий запуск неизвестен (старт программы,
/// ожидание нового запуска): самая свежая копия, чей сохранённый конец есть в
/// снимке, — тот же запуск; иначе имя даёт первая строка снимка. Вместе с
/// именем — сохранённый конец файла, от которого считаются новые строки.
async fn run_continued_by(text: &str, dir: &Path, home: Option<&str>) -> (String, Vec<String>) {
    if let Some(newest) = newest_run_copy(dir).await {
        let saved = last_lines_of(&dir.join(&newest)).await;
        if !saved.is_empty() && !new_lines(text, &[], &saved, home).1 {
            return (newest, saved);
        }
    }
    let name = file_name(text, chrono::Local::now().naive_local());
    let saved = last_lines_of(&dir.join(&name)).await;
    (name, saved)
}

async fn newest_run_copy(dir: &Path) -> Option<String> {
    let mut entries = tokio::fs::read_dir(dir).await.ok()?;
    let mut newest: Option<String> = None;
    while let Ok(Some(entry)) = entries.next_entry().await {
        if let Some(name) = entry.file_name().to_str()
            && is_run_copy(name)
            && newest.as_deref().is_none_or(|current| name > current)
        {
            newest = Some(name.to_owned());
        }
    }
    newest
}

/// Копия запуска ядра, её текущая часть — не прошлая часть, не файл
/// соединений и не что-то другое из каталога.
pub(crate) fn is_run_copy(name: &str) -> bool {
    name.starts_with(PREFIX) && name.ends_with(".log") && !name.contains(".conn") && !name.ends_with(OLDER_PART)
}

/// Прошлая часть запуска: `core_<запуск>.1.log`.
pub(crate) fn older_run_path(run: &Path) -> PathBuf {
    run.with_extension("1.log")
}

/// Файл запуска, чья это прошлая часть; у остальных файлов — `None`.
pub(crate) fn run_of_older_part(part: &Path) -> Option<PathBuf> {
    let run = part.file_name()?.to_str()?.strip_suffix(OLDER_PART)?;
    (run.starts_with(PREFIX) && !run.contains(".conn")).then(|| part.with_file_name(format!("{run}.log")))
}

/// Строки — в файл запуска; доросший до `part` файл перед этим становится
/// прошлой частью. Это переименование, а не чтение и запись: копии перед
/// остановкой ядра и перед новым запуском оно не задерживает.
async fn append_to_the_run(run: &Path, bytes: &[u8], part: u64) -> std::io::Result<()> {
    if tokio::fs::metadata(run).await.is_ok_and(|meta| meta.len() >= part) {
        // Не вышло (файл держит открытым другая программа) — строки идут в
        // него же, смена — с одной из следующих копий.
        let _ = tokio::fs::rename(run, older_run_path(run)).await;
    }
    append(run, bytes).await
}

/// Запуск, которому принадлежит файл соединений: `core_<запуск>.conn.1.log` → `core_<запуск>`.
fn run_of_connections(name: &str) -> &str {
    name.split(".conn").next().unwrap_or(name)
}

/// Строка журнала ядра в том виде, в каком она ложится на диск, и строка ли
/// это о соединении (идёт в файл соединений).
fn for_disk_line(line: &str, home: Option<&str>) -> (String, bool) {
    let clean = redact::core_line_for_disk(line, home);
    let connection = crate::module::support_bundle::is_connection_line(&clean);
    (clean, connection)
}

/// Файл соединений запуска: `core_<запуск>.conn.log`, прошлый промежуток —
/// `core_<запуск>.conn.1.log`.
pub(crate) fn connections_path(run: &Path) -> PathBuf {
    run.with_extension("conn.log")
}

pub(crate) fn older_connections_path(run: &Path) -> PathBuf {
    run.with_extension("conn.1.log")
}

async fn append_connections(run: &Path, chunk: &str) -> std::io::Result<()> {
    if chunk.is_empty() {
        return Ok(());
    }
    let current = connections_path(run);
    if connections_are_due(&current, chrono::Local::now().naive_local()).await {
        tokio::fs::rename(&current, older_connections_path(run)).await?;
    }
    append(&current, chunk.as_bytes()).await
}

/// Пора начинать новый файл соединений: текущий начат [`CONNECTIONS_SPAN`]
/// назад или дорос до [`CONNECTIONS_CAP`].
async fn connections_are_due(path: &Path, now: chrono::NaiveDateTime) -> bool {
    use tokio::io::AsyncReadExt as _;
    let Ok(mut file) = tokio::fs::File::open(path).await else {
        return false;
    };
    if file.metadata().await.is_ok_and(|meta| meta.len() >= CONNECTIONS_CAP) {
        return true;
    }
    let mut head = [0u8; 64];
    let read = file.read(&mut head).await.unwrap_or_default();
    let head = String::from_utf8_lossy(&head[..read]);
    crate::core::logger::line_stamp(&head).is_some_and(|started| now - started >= CONNECTIONS_SPAN)
}

fn last_raw_lines(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = text
        .lines()
        .rev()
        .filter(|line| !line.trim().is_empty())
        .take(TAIL_LINES)
        .map(str::to_owned)
        .collect();
    lines.reverse();
    lines
}

async fn append(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use tokio::io::AsyncWriteExt as _;
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .await?;
    file.write_all(bytes).await?;
    file.flush().await
}

/// До [`TAIL_LINES`] последних сохранённых строк файла запуска, по порядку;
/// читается только хвост файла.
async fn last_lines_of(path: &Path) -> Vec<String> {
    use tokio::io::{AsyncReadExt as _, AsyncSeekExt as _};
    let Ok(mut file) = tokio::fs::File::open(path).await else {
        return Vec::new();
    };
    let length = file.metadata().await.map(|meta| meta.len()).unwrap_or_default();
    let mut tail = Vec::new();
    if file
        .seek(std::io::SeekFrom::Start(length.saturating_sub(TAIL_READ)))
        .await
        .is_err()
        || file.read_to_end(&mut tail).await.is_err()
    {
        return Vec::new();
    }
    tail_lines(&String::from_utf8_lossy(&tail))
}

fn tail_lines(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = text
        .lines()
        .rev()
        .filter(|line| !line.trim().is_empty() && *line != GAP_LINE)
        .take(TAIL_LINES)
        .map(str::to_owned)
        .collect();
    lines.reverse();
    lines
}

fn for_disk(text: &str, home: Option<&str>) -> String {
    let mut clean: String = text
        .lines()
        .map(|line| redact::core_line_for_disk(line, home))
        .collect::<Vec<_>>()
        .join("\n");
    clean.push('\n');
    clean
}

/// Копии, которые прежние версии писали без редакции, переписываются один раз.
async fn redact_old_copies() {
    let Ok(dir) = dirs::service_log_dir() else {
        return;
    };
    let mark = dir.join(REDACTED_MARK);
    if tokio::fs::try_exists(&mark).await.unwrap_or(false) {
        return;
    }
    let Ok(mut entries) = tokio::fs::read_dir(&dir).await else {
        return;
    };
    let home = redact::home_prefix();
    while let Ok(Some(entry)) = entries.next_entry().await {
        let is_copy = entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with(PREFIX) && name.ends_with(".log"));
        if !is_copy {
            continue;
        }
        // Замок — на файл, а не на всю уборку: копия перед новым запуском ядра
        // ждёт его недолго и не теряет конец прошлого запуска.
        let _saving = SAVING.lock().await;
        let Ok(text) = tokio::fs::read_to_string(entry.path()).await else {
            continue;
        };
        let home = home.clone();
        let Ok(clean) = tokio::task::spawn_blocking(move || {
            let clean = for_disk(&text, home.as_deref());
            (clean != text).then_some(clean)
        })
        .await
        else {
            continue;
        };
        if let Some(clean) = clean
            && let Err(error) = write_atomic(&entry.path(), clean.as_bytes()).await
        {
            logging!(
                warn,
                Type::Core,
                "an old core log copy was not redacted: {}: {error:#}",
                entry.path().display()
            );
            return;
        }
    }
    let _ = tokio::fs::write(&mark, b"").await;
}

/// Файл называется моментом запуска ядра — первой строкой журнала, — так и
/// после перезапуска приложения копии того же запуска идут в его файл.
fn file_name(text: &str, now: chrono::NaiveDateTime) -> String {
    let started = text
        .lines()
        .find(|line| !line.trim().is_empty())
        .and_then(started_at)
        .unwrap_or(now);
    format!("{PREFIX}{}.log", started.format("%Y-%m-%d_%H-%M-%S"))
}

/// Строки снимка, которых прошлая копия ещё не видела. `seen` — последние
/// строки прошлого снимка; их нет (первая копия за жизнь программы) —
/// сверяется сохранённый хвост файла `saved`. Ни того ни другого в снимке нет
/// — служба завела новый файл или ушла дальше 4 МБ (`gap`), снимок идёт
/// целиком.
fn new_lines<'s>(snapshot: &'s str, seen: &[String], saved: &[String], home: Option<&str>) -> (Vec<&'s str>, bool) {
    let raw: Vec<&str> = snapshot.lines().filter(|line| !line.trim().is_empty()).collect();
    if !seen.is_empty() {
        let found = (seen.len()..=raw.len()).rev().find(|end| {
            raw[end - seen.len()..*end]
                .iter()
                .zip(seen)
                .all(|(line, kept)| line == kept)
        });
        return match found {
            Some(end) => (raw[end..].to_vec(), false),
            None => (raw, true),
        };
    }
    if saved.is_empty() {
        return (raw, false);
    }
    let mut kept_newest_first: Vec<(usize, String)> = Vec::new();
    for (at, line) in raw.iter().enumerate().rev() {
        let (clean, connection) = for_disk_line(line, home);
        if connection {
            continue;
        }
        kept_newest_first.push((at, clean));
        if let Some(start) = kept_newest_first.len().checked_sub(saved.len())
            && saved
                .iter()
                .rev()
                .zip(&kept_newest_first[start..])
                .all(|(kept, (_, seen))| kept == seen)
        {
            let after = kept_newest_first[start].0 + 1;
            return (raw[after..].to_vec(), false);
        }
    }
    // Снимок начинается внутри сохранённого хвоста: его первые строки — конец хвоста.
    let oldest_first: Vec<(usize, String)> = kept_newest_first.into_iter().rev().collect();
    for matched in (1..saved.len().min(oldest_first.len() + 1)).rev() {
        if saved[saved.len() - matched..]
            .iter()
            .zip(&oldest_first)
            .all(|(kept, (_, seen))| kept == seen)
        {
            let after = oldest_first[matched - 1].0 + 1;
            return (raw[after..].to_vec(), false);
        }
    }
    (raw, true)
}

/// `[2026-10-09 17:29:15.415] …` — так служба начинает каждую строку.
fn started_at(line: &str) -> Option<chrono::NaiveDateTime> {
    let stamp = line.strip_prefix('[')?.get(..19)?;
    chrono::NaiveDateTime::parse_from_str(stamp, "%Y-%m-%d %H:%M:%S").ok()
}

/// Под замком копий: своей записи в этот момент нет, и недописанный файл
/// копии (выход оборвал запись) можно убрать.
async fn prune(dir: &Path) {
    let Ok(mut entries) = tokio::fs::read_dir(dir).await else {
        return;
    };
    let mut names = Vec::new();
    while let Ok(Some(entry)) = entries.next_entry().await {
        let modified = entry
            .metadata()
            .await
            .and_then(|meta| meta.modified())
            .unwrap_or(std::time::UNIX_EPOCH);
        if let Some(name) = entry.file_name().to_str() {
            names.push((name.to_owned(), modified));
        }
    }
    for name in surplus(names, crate::core::logger::session_started()) {
        let _ = tokio::fs::remove_file(dir.join(name)).await;
    }
}

/// Копии сверх [`KEEP`], самые старые (имя — момент запуска, порядок по нему),
/// кроме запусков, которые писались за этот сеанс программы; прошлые части
/// запусков, которые уходят сами или которых уже нет; файлы соединений
/// всех запусков, кроме [`CONNECTIONS_RUNS`] последних и запусков этого сеанса
/// (перезапуск программы не отнимает час-два соединений у идущего запуска);
/// недописанные файлы копий.
fn surplus(names: Vec<(String, std::time::SystemTime)>, session_started: std::time::SystemTime) -> Vec<String> {
    let torn = format!(".{PREFIX}");
    let (copies, others): (Vec<_>, Vec<_>) = names
        .into_iter()
        .partition(|(name, _)| name.starts_with(PREFIX) && name.ends_with(".log"));
    let (connections, copies): (Vec<_>, Vec<_>) = copies.into_iter().partition(|(name, _)| name.contains(".conn"));
    let (older_parts, mut copies): (Vec<_>, Vec<_>) =
        copies.into_iter().partition(|(name, _)| name.ends_with(OLDER_PART));
    copies.sort();
    let recent_runs: Vec<String> = copies
        .iter()
        .rev()
        .take(CONNECTIONS_RUNS)
        .filter_map(|(name, _)| name.strip_suffix(".log").map(str::to_owned))
        .collect();
    let connections = connections
        .into_iter()
        .filter(|(name, _)| !recent_runs.iter().any(|run| run == run_of_connections(name)));
    let kept = copies.split_off(copies.len().saturating_sub(KEEP));
    // Прошлая часть остаётся, пока остаётся её запуск: он среди последних или
    // писался за этот сеанс.
    let staying: Vec<&str> = kept
        .iter()
        .chain(copies.iter().filter(|(_, modified)| *modified >= session_started))
        .filter_map(|(name, _)| name.strip_suffix(".log"))
        .collect();
    let older_parts: Vec<_> = older_parts
        .into_iter()
        .filter(|(name, _)| !staying.iter().any(|run| name.strip_suffix(OLDER_PART) == Some(*run)))
        .collect();
    copies
        .into_iter()
        .chain(older_parts)
        .chain(connections)
        .filter(|(_, modified)| *modified < session_started)
        .chain(
            others
                .into_iter()
                .filter(|(name, _)| name.starts_with(&torn) && name.ends_with(".tmp")),
        )
        .map(|(name, _)| name)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        BUSY_BYTES, GAP_LINE, KEEP, SAVE_BUSY, SAVE_EVERY, Saved, append_to_the_run, connections_are_due,
        connections_path, file_name, for_disk, for_disk_line, is_run_copy, new_lines, newest_run_copy, next_wait,
        older_connections_path, older_run_path, prune_is_due, run_continued_by, run_of_connections, run_of_older_part,
        surplus, tail_lines,
    };

    #[test]
    fn the_copies_are_pruned_once_a_session_and_with_every_new_run_file() {
        assert!(prune_is_due(false, false));
        assert!(prune_is_due(true, false));
        assert!(prune_is_due(true, true));
        assert!(!prune_is_due(false, true));
    }

    /// Разовая уборка старых копий держит замок копий по файлу: копия перед
    /// новым запуском ядра ждёт замок лишь [`super::ANSWER_WAIT`].
    #[test]
    fn the_one_time_redaction_lets_a_copy_in_between_files() {
        let body =
            crate::utils::source_scan::fn_body(include_str!("core_log_archive.rs"), "async fn redact_old_copies(")
                .unwrap_or_default();
        let each_file = body.find("while let").unwrap_or(usize::MAX);
        let locked = body.find("SAVING.lock()").unwrap_or_default();
        assert!(locked > each_file, "{body}");
    }

    fn now() -> chrono::NaiveDateTime {
        chrono::NaiveDateTime::parse_from_str("2026-01-02 03:04:05", "%Y-%m-%d %H:%M:%S").unwrap_or_default()
    }

    #[test]
    fn a_copy_goes_to_disk_redacted_and_a_second_pass_changes_nothing() {
        let raw = "[2026-10-09 17:29:15.415] [TCP] 10.0.0.2:44100 --> mail.example.com:443 match Rule\n\
                   [2026-10-09 17:29:16.000] open /home/someone/.config/app/core.yaml\n";
        let once = for_disk(raw, Some("/home/someone"));
        assert!(!once.contains("mail.example.com"), "{once}");
        assert!(!once.contains("/home/someone"), "{once}");
        assert_eq!(file_name(&once, now()), "core_2026-10-09_17-29-15.log");
        assert_eq!(for_disk(&once, Some("/home/someone")), once);
    }

    #[test]
    fn a_run_is_named_by_its_first_line() {
        let text = "\n[2026-10-09 17:29:15.415] time=\"…\" level=info msg=\"Start\"\n[2026-10-09 17:31:00.000] …\n";
        assert_eq!(file_name(text, now()), "core_2026-10-09_17-29-15.log");
        assert_eq!(file_name("no stamp here", now()), "core_2026-01-02_03-04-05.log");
    }

    fn saved(lines: &[&str]) -> Vec<String> {
        lines.iter().map(|line| (*line).to_owned()).collect()
    }

    fn new(snapshot: &str, seen: &[&str], kept: &[&str]) -> (Vec<String>, bool) {
        let (lines, gap) = new_lines(snapshot, &saved(seen), &saved(kept), None);
        (lines.into_iter().map(str::to_owned).collect(), gap)
    }

    #[test]
    fn a_new_copy_takes_only_the_lines_the_last_one_did_not_see() {
        assert_eq!(new("[1] a\n[2] b\n", &[], &[]), (saved(&["[1] a", "[2] b"]), false));
        assert_eq!(new("[1] a\n[2] b\n", &["[1] a", "[2] b"], &[]), (Vec::new(), false));
        assert_eq!(new("[2] b\r\n[3] c\r\n", &["[2] b"], &[]), (saved(&["[3] c"]), false));
        // Служба завела новый файл: снимок идёт целиком, пропуск замечен
        assert_eq!(new("[9] z\n", &["[2] b"], &[]), (saved(&["[9] z"]), true));
    }

    #[test]
    fn a_repeat_of_the_last_seen_line_does_not_cut_the_lines_before_it() {
        assert_eq!(
            new(
                "[0] start\n[1] error\n[2] other\n[1] error\n[3] next\n",
                &["[0] start", "[1] error"],
                &[]
            ),
            (saved(&["[2] other", "[1] error", "[3] next"]), false)
        );
        assert_eq!(
            tail_lines(&format!("[1] a\n\n{GAP_LINE}\n[2] b\n")),
            saved(&["[1] a", "[2] b"])
        );
    }

    #[test]
    fn connection_lines_go_to_their_own_file_and_do_not_fake_a_gap() {
        let tcp = "[1] [TCP] 10.0.0.2:44100 --> mail.example.com:443 match Rule using NL";
        let dial = "[2] [TCP] dial NL 10.0.0.2:44101 --> mail.example.com:443 error: EOF";
        let (clean, connection) = for_disk_line(tcp, None);
        assert!(connection && !clean.contains("mail.example.com"), "{clean}");
        assert!(!for_disk_line(dial, None).1);
        assert_eq!(
            for_disk_line("[3] TUN started", None),
            ("[3] TUN started".to_owned(), false)
        );

        // Сохранённое давно ушло за пределы снимка, но прошлая копия видела его
        // конец — пропуска нет
        let snapshot = format!("{tcp}\n{tcp}\n[4] later\n");
        assert_eq!(
            new(&snapshot, &[tcp, tcp], &["[0] long ago"]),
            (saved(&["[4] later"]), false)
        );
        // Первая копия за жизнь программы сверяется с сохранённым основным файлом
        assert_eq!(
            new(
                &format!("[3] TUN started\n{tcp}\n[4] later\n"),
                &[],
                &["[3] TUN started"]
            ),
            (saved(&[tcp, "[4] later"]), false)
        );
        // Снимок начинается внутри сохранённого хвоста — это тот же запуск, не пропуск
        assert_eq!(
            new(&format!("[2] b\n{tcp}\n[3] c\n"), &[], &["[0] z", "[1] a", "[2] b"]),
            (saved(&[tcp, "[3] c"]), false)
        );
        assert_eq!(new("[7] q\n", &[], &["[1] a", "[2] b"]), (saved(&["[7] q"]), true));
    }

    #[test]
    fn the_connections_file_turns_over_hourly_and_old_sessions_lose_theirs() {
        let run = std::path::Path::new("/logs/service/core_2026-11-08_16-06-39.log");
        assert_eq!(
            connections_path(run),
            std::path::Path::new("/logs/service/core_2026-11-08_16-06-39.conn.log")
        );
        assert_eq!(
            older_connections_path(run),
            std::path::Path::new("/logs/service/core_2026-11-08_16-06-39.conn.1.log")
        );
        let dir = std::env::temp_dir().join(format!("clod-conn-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("core_x.conn.log");
        let _ = std::fs::write(&file, "[2026-11-08 16:00:00.000] [TCP] a --> b\n");
        let at = |time: &str| chrono::NaiveDateTime::parse_from_str(time, "%Y-%m-%d %H:%M:%S").unwrap_or_default();
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build();
        let due = runtime.map(|runtime| {
            (
                runtime.block_on(connections_are_due(&file, at("2026-11-08 16:59:59"))),
                runtime.block_on(connections_are_due(&file, at("2026-11-08 17:00:00"))),
                runtime.block_on(connections_are_due(&dir.join("missing.log"), at("2026-11-09 00:00:00"))),
            )
        });
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(due.ok(), Some((false, true, false)));

        // Соединения двух последних запусков переживают перезапуск программы;
        // более ранних и без своей копии — уходят; этого сеанса — остаются.
        let old = std::time::UNIX_EPOCH;
        let started = old + std::time::Duration::from_secs(10);
        let names = vec![
            ("core_2026-11-01_00-00-00.log".to_owned(), old),
            ("core_2026-11-01_00-00-00.conn.log".to_owned(), old),
            ("core_2026-11-02_00-00-00.log".to_owned(), old),
            ("core_2026-11-02_00-00-00.conn.log".to_owned(), old),
            ("core_2026-11-02_00-00-00.conn.1.log".to_owned(), old),
            ("core_2026-11-03_00-00-00.log".to_owned(), old),
            ("core_2026-11-03_00-00-00.conn.log".to_owned(), old),
            ("core_2026-10-20_00-00-00.conn.log".to_owned(), old),
            ("core_2026-11-08_00-00-00.conn.log".to_owned(), started),
        ];
        assert_eq!(
            surplus(names, started),
            vec![
                "core_2026-11-01_00-00-00.conn.log".to_owned(),
                "core_2026-10-20_00-00-00.conn.log".to_owned()
            ]
        );
        assert_eq!(
            run_of_connections("core_2026-11-02_00-00-00.conn.1.log"),
            "core_2026-11-02_00-00-00"
        );
        assert!(is_run_copy("core_2026-11-02_00-00-00.log"));
        assert!(!is_run_copy("core_2026-11-02_00-00-00.conn.log"));
        assert!(!is_run_copy("core_2026-11-02_00-00-00.1.log"));
        let dir = std::path::Path::new("service");
        assert_eq!(
            run_of_older_part(&older_run_path(&dir.join("core_2026-11-02_00-00-00.log"))),
            Some(dir.join("core_2026-11-02_00-00-00.log"))
        );
        for name in [
            "core_2026-11-02_00-00-00.log",
            "core_2026-11-02_00-00-00.conn.1.log",
            "service_latest.1.log",
        ] {
            assert_eq!(run_of_older_part(&dir.join(name)), None, "{name}");
        }
        assert!(!is_run_copy("service_latest.log"));
    }

    /// Новый сеанс программы застаёт запуск, который служба всё ещё пишет:
    /// копии идут в его файл, а не в новый с именем из середины журнала.
    #[test]
    fn a_new_session_continues_the_run_the_service_still_writes() {
        let dir = std::env::temp_dir().join(format!("clod-run-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(
            dir.join("core_2026-11-08_09-00-00.log"),
            "[2026-11-08 09:00:00.000] Start\n[2026-11-08 09:00:01.000] TUN started\n",
        );
        let _ = std::fs::write(
            dir.join("core_2026-11-07_09-00-00.log"),
            "[2026-11-07 09:00:00.000] Start\n",
        );
        // Прошлая часть запуска — не запуск: её имя новее по алфавиту, но
        // копии продолжают текущую часть.
        let _ = std::fs::write(
            dir.join("core_2026-11-08_09-00-00.1.log"),
            "[2026-11-08 08:59:00.000] an earlier part\n",
        );
        let continued = "[2026-11-08 09:00:01.000] TUN started\n\
                         [2026-11-08 12:00:00.000] [TCP] a --> b match Rule using X\n\
                         [2026-11-08 12:00:01.000] warning\n";
        let fresh = "[2026-11-08 13:00:00.000] Start\n[2026-11-08 13:00:01.000] TUN started\n";
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build();
        let runs = runtime.map(|runtime| {
            (
                runtime.block_on(newest_run_copy(&dir)),
                runtime.block_on(run_continued_by(continued, &dir, None)),
                runtime.block_on(run_continued_by(fresh, &dir, None)),
            )
        });
        let _ = std::fs::remove_dir_all(&dir);
        let (newest, continued, fresh) = runs.unwrap_or_default();
        assert_eq!(newest.as_deref(), Some("core_2026-11-08_09-00-00.log"));
        assert_eq!(continued.0, "core_2026-11-08_09-00-00.log");
        assert_eq!(
            continued.1,
            saved(&[
                "[2026-11-08 09:00:00.000] Start",
                "[2026-11-08 09:00:01.000] TUN started"
            ])
        );
        assert_eq!(fresh, ("core_2026-11-08_13-00-00.log".to_owned(), Vec::new()));
    }

    /// Запуск на диске — конец журнала двумя частями: доросший файл уходит в
    /// прошлую часть целиком, прежняя прошлая пропадает, строки не рвутся.
    #[test]
    fn a_run_file_that_reached_its_size_becomes_the_older_part() {
        let dir = std::env::temp_dir().join(format!("clod-run-parts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        let run = dir.join("core_2026-11-14_20-00-00.log");
        let older = older_run_path(&run);
        let read = |path: &std::path::Path| std::fs::read_to_string(path).ok();
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build();
        let steps = runtime.map(|runtime| {
            let mut steps = Vec::new();
            for line in ["first\n", "second\n", "third is long\n", "fourth\n", "fifth\n"] {
                let written = runtime.block_on(append_to_the_run(&run, line.as_bytes(), 12)).is_ok();
                steps.push((written, read(&run), read(&older)));
            }
            steps
        });
        let _ = std::fs::remove_dir_all(&dir);

        let step = |current: &str, older: Option<&str>| (true, Some(current.to_owned()), older.map(str::to_owned));
        assert_eq!(
            steps.ok(),
            Some(vec![
                step("first\n", None),
                step("first\nsecond\n", None),
                step("third is long\n", Some("first\nsecond\n")),
                step("fourth\n", Some("third is long\n")),
                step("fourth\nfifth\n", Some("third is long\n")),
            ])
        );
        assert_eq!(older, std::path::Path::new(&dir).join("core_2026-11-14_20-00-00.1.log"));
    }

    #[test]
    fn a_long_run_is_kept_whole() {
        let snapshot: String = (0..50_000)
            .map(|i| format!("[{i}] level=info msg=\"a line of a long run\"\n"))
            .collect();
        let (lines, gap) = new_lines(&snapshot, &[], &[], None);
        assert!(!gap);
        assert_eq!(lines.len(), 50_000);
    }

    #[test]
    fn a_busy_log_or_a_gap_is_copied_sooner() {
        assert_eq!(next_wait(None), SAVE_EVERY);
        assert_eq!(next_wait(Some(Saved { gap: false, fresh: 10 })), SAVE_EVERY);
        assert_eq!(next_wait(Some(Saved { gap: true, fresh: 10 })), SAVE_BUSY);
        assert_eq!(
            next_wait(Some(Saved {
                gap: false,
                fresh: BUSY_BYTES
            })),
            SAVE_BUSY
        );
    }

    #[test]
    fn only_the_oldest_copies_over_the_limit_and_torn_writes_go() {
        let mut names: Vec<String> = (0..KEEP + 2)
            .map(|day| format!("core_2026-10-{:02}_00-00-00.log", day + 1))
            .collect();
        names.push("service_latest.log".to_owned());
        names.push(".core_2026-10-12_00-00-00.log.Ab3dEf9h.tmp".to_owned());
        names.push(".verge.yaml.Ab3dEf9h.tmp".to_owned());
        names.reverse();
        let old = std::time::UNIX_EPOCH;
        let started = old + std::time::Duration::from_secs(10);
        let dated = |names: &[String], at| names.iter().map(|name| (name.clone(), at)).collect::<Vec<_>>();
        assert_eq!(
            surplus(dated(&names, old), started),
            vec![
                "core_2026-10-01_00-00-00.log".to_owned(),
                "core_2026-10-02_00-00-00.log".to_owned(),
                ".core_2026-10-12_00-00-00.log.Ab3dEf9h.tmp".to_owned(),
            ]
        );
        // Запуски этого сеанса программы не удаляются, сколько бы их ни было
        assert_eq!(
            surplus(dated(&names, started), started),
            vec![".core_2026-10-12_00-00-00.log.Ab3dEf9h.tmp".to_owned()]
        );
    }

    /// Прошлая часть живёт столько же, сколько её запуск, и за запуск не
    /// считается: десять запусков — это десять запусков, а не пять.
    #[test]
    fn the_older_part_of_a_run_stays_and_goes_with_its_run() {
        let run = |day: usize| format!("core_2026-10-{day:02}_00-00-00.log");
        let part = |day: usize| format!("core_2026-10-{day:02}_00-00-00.1.log");
        let old = std::time::UNIX_EPOCH;
        let started = old + std::time::Duration::from_secs(10);
        let mut names: Vec<(String, std::time::SystemTime)> = (1..=KEEP + 1).map(|day| (run(day), old)).collect();
        names.extend([1, 2, KEEP + 1].map(|day| (part(day), old)));
        // Копии этого запуска уже нет: его прошлая часть — ничья.
        names.push(("core_2026-09-01_00-00-00.1.log".to_owned(), old));
        // Записанное за этот сеанс программы остаётся и без копии рядом.
        names.push(("core_2026-09-02_00-00-00.1.log".to_owned(), started));
        assert_eq!(
            surplus(names.clone(), started),
            vec![run(1), part(1), "core_2026-09-01_00-00-00.1.log".to_owned()]
        );
        // Запуск сверх предела, но писавшийся за этот сеанс, остаётся — и его
        // прошлая часть с ним, когда бы её ни записали.
        for (name, modified) in &mut names {
            if *name == run(1) {
                *modified = started;
            }
        }
        assert_eq!(
            surplus(names, started),
            vec!["core_2026-09-01_00-00-00.1.log".to_owned()]
        );
    }
}
