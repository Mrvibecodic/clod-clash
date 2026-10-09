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
//! - перед тем как служба запустит новое ядро (после падения ядра или после
//!   перезагрузки, когда служба подняла ядро сама);
//! - раз в [`SAVE_EVERY`], пока ядро работает, — на случай выключения
//!   компьютера и перезапуска службы; если с прошлой копии служба успела
//!   начать новый файл, следующая копия — через [`SAVE_SOON`];
//! - при выгрузке отчёта.
//!
//! Копия дописывает в файл запуска только новые строки, файл держит последние
//! [`FILE_CAP`] байт. Хранится [`KEEP`] последних запусков; дальше их чистит и
//! общая уборка журналов по возрасту.

use std::path::Path;
use std::time::Duration;

use clash_verge_logging::{Type, logging};
use parking_lot::Mutex;
use tokio::time::Instant;

use crate::{
    core::{CoreManager, handle, manager::RunningMode},
    process::AsyncHandler,
    utils::{dirs, help::write_atomic},
};

const SAVE_EVERY: Duration = Duration::from_secs(10 * 60);
const SAVE_SOON: Duration = Duration::from_secs(2 * 60);
/// Дольше копия ответа службы не ждёт (её IPC повторяет запрос до 20 раз):
/// ни запуск ядра, ни отчёт не держатся за неотвечающую службу.
const ANSWER_WAIT: Duration = Duration::from_secs(3);
/// Перед остановкой ядра — меньше: у выхода свой срок на остановку.
const STOP_WAIT: Duration = Duration::from_secs(1);
const KEEP: usize = 10;
const PREFIX: &str = "core_";
const FILE_CAP: usize = 4 * 1024 * 1024;

/// Файл запуска ядра, который сейчас пишет служба: все копии этого запуска
/// ложатся в него. Нет — имя даёт первая строка журнала.
static RUN: Mutex<Option<String>> = Mutex::new(None);
/// Копии идут по одной: каждая читает и дописывает файл запуска.
static SAVING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

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

/// Копия раз в [`SAVE_EVERY`], пока ядро работает под службой.
pub fn spawn() {
    AsyncHandler::spawn(|| async {
        let mut wait = SAVE_EVERY;
        loop {
            tokio::time::sleep(wait).await;
            wait = SAVE_EVERY;
            if handle::Handle::global().is_exiting()
                || !matches!(*CoreManager::global().get_running_mode(), RunningMode::Service)
            {
                continue;
            }
            if save_within(ANSWER_WAIT).await == Some(true) {
                wait = SAVE_SOON;
            }
        }
    });
}

/// Копия, если служба ответит до срока; `Some(gap)` — файл записан, `gap` —
/// часть строк до этой копии служба уже убрала.
async fn save_within(wait: Duration) -> Option<bool> {
    let deadline = Instant::now() + wait;
    let _saving = tokio::time::timeout_at(deadline, SAVING.lock()).await.ok()?;
    save_locked(deadline).await
}

async fn save_locked(deadline: Instant) -> Option<bool> {
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
    let name = RUN
        .lock()
        .get_or_insert_with(|| file_name(&text, chrono::Local::now().naive_local()))
        .clone();
    let path = dir.join(name);
    let saved = tokio::fs::read_to_string(&path).await.unwrap_or_default();
    let (merged, gap) = merge(&saved, &text);
    if let Err(error) = write_atomic(&path, merged.as_bytes()).await {
        logging!(
            warn,
            Type::Core,
            "the core log was not saved to {}: {error:#}",
            path.display()
        );
        return None;
    }
    prune(&dir).await;
    Some(gap)
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

/// Сохранённое и новые строки снимка: всё после последней сохранённой строки.
/// Её в снимке нет — служба завела новый файл или ушла дальше 4 МБ (это
/// `gap` в ответе), и снимок дописывается целиком. Файл держит последние
/// [`FILE_CAP`] байт.
fn merge(saved: &str, snapshot: &str) -> (String, bool) {
    let lines: Vec<&str> = snapshot.lines().collect();
    let last = saved.lines().rev().find(|line| !line.trim().is_empty());
    let found = last.and_then(|last| lines.iter().rposition(|line| *line == last));
    let gap = last.is_some() && found.is_none();
    let fresh = found.map_or(0, |at| at + 1);
    let mut merged = saved.trim_end().to_owned();
    for line in &lines[fresh..] {
        if !merged.is_empty() {
            merged.push('\n');
        }
        merged.push_str(line);
    }
    merged.push('\n');
    if merged.len() > FILE_CAP {
        let cut = merged.len() - FILE_CAP;
        // По байтам: граница может прийтись на середину буквы.
        let from = merged.as_bytes()[cut..]
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(merged.len(), |at| cut + at + 1);
        merged.drain(..from);
    }
    (merged, gap)
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
        if let Some(name) = entry.file_name().to_str() {
            names.push(name.to_owned());
        }
    }
    for name in surplus(names) {
        let _ = tokio::fs::remove_file(dir.join(name)).await;
    }
}

/// Копии сверх [`KEEP`], самые старые (имя — момент запуска, порядок по нему),
/// и недописанные файлы копий.
fn surplus(names: Vec<String>) -> Vec<String> {
    let torn = format!(".{PREFIX}");
    let (mut copies, others): (Vec<String>, Vec<String>) = names
        .into_iter()
        .partition(|name| name.starts_with(PREFIX) && name.ends_with(".log"));
    copies.sort();
    copies.truncate(copies.len().saturating_sub(KEEP));
    copies.extend(
        others
            .into_iter()
            .filter(|name| name.starts_with(&torn) && name.ends_with(".tmp")),
    );
    copies
}

#[cfg(test)]
mod tests {
    use super::{FILE_CAP, KEEP, file_name, merge, surplus};

    fn now() -> chrono::NaiveDateTime {
        chrono::NaiveDateTime::parse_from_str("2026-01-02 03:04:05", "%Y-%m-%d %H:%M:%S").unwrap_or_default()
    }

    #[test]
    fn a_run_is_named_by_its_first_line() {
        let text = "\n[2026-10-09 17:29:15.415] time=\"…\" level=info msg=\"Start\"\n[2026-10-09 17:31:00.000] …\n";
        assert_eq!(file_name(text, now()), "core_2026-10-09_17-29-15.log");
        assert_eq!(file_name("no stamp here", now()), "core_2026-01-02_03-04-05.log");
    }

    #[test]
    fn a_new_copy_adds_only_the_lines_the_file_lacks() {
        let first = "[1] a\n[2] b\n";
        let (saved, gap) = merge("", first);
        assert_eq!((saved.as_str(), gap), (first, false));
        // Та же копия ничего не добавляет, следующая — только новое
        assert_eq!(merge(&saved, first), (first.to_owned(), false));
        assert_eq!(
            merge(&saved, "[2] b\r\n[3] c\r\n"),
            ("[1] a\n[2] b\n[3] c\n".to_owned(), false)
        );
        // Служба завела новый файл: снимок дописывается целиком, пробел замечен
        assert_eq!(merge(&saved, "[9] z\n"), ("[1] a\n[2] b\n[9] z\n".to_owned(), true));
    }

    #[test]
    fn a_run_file_keeps_its_last_bytes_from_a_line_start() {
        let line = "x".repeat(999);
        let snapshot: String = (0..FILE_CAP / 1000 + 10).map(|i| format!("{i:08}{line}\n")).collect();
        let (merged, _) = merge("", &snapshot);
        assert!(merged.len() <= FILE_CAP);
        assert!(merged.ends_with(&format!("{:08}{line}\n", FILE_CAP / 1000 + 9)));
        assert!(merged.lines().all(|kept| kept.len() == 1007));

        // Граница посреди буквы не роняет обрезку
        let cyrillic = "ж".repeat(1001);
        let snapshot: String = (0..FILE_CAP / 2000 + 10)
            .map(|i| format!("{i:08}{cyrillic}\n"))
            .collect();
        let (merged, _) = merge("", &snapshot);
        assert!(merged.len() <= FILE_CAP);
        assert!(merged.lines().all(|kept| kept.ends_with('ж') && kept.len() == 8 + 2002));
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
        assert_eq!(
            surplus(names),
            vec![
                "core_2026-10-01_00-00-00.log".to_owned(),
                "core_2026-10-02_00-00-00.log".to_owned(),
                ".core_2026-10-12_00-00-00.log.Ab3dEf9h.tmp".to_owned(),
            ]
        );
    }
}
