//! Один опрос таблицы соединений ядра на всех, кто считает по ней трафик:
//! сборщик отчёта (`module::client_report`, раз в 10 секунд, пока копится
//! отчёт) и оценку расхода (`core::traffic_estimate`, раз в 30–300 секунд, пока
//! она нужна). К ядру опрос ходит с частотой самого частого из включённых;
//! каждый получает снимок в свой срок и считает приросты по `id` соединения
//! сам. Выключены оба — к ядру не ходим.
//!
//! Цикл один на процесс и не кончается: на выходе тик пропускается (оценка
//! только дописывается в файл), после отменённого выхода опрос идёт дальше сам.

use std::time::{Duration, Instant};

use crate::{
    core::{handle, traffic_estimate},
    module::client_report,
    process::AsyncHandler,
};

/// Не дольше этого опрос спит, не спросив, кому нужен снимок: потребитель
/// мог включиться.
const LOOK_AGAIN: Duration = Duration::from_secs(10);

/// Потребитель снимка: как часто он ему нужен (`None` — выключен) и когда
/// снимок для него брали в последний раз.
#[derive(Debug, Clone, Copy, Default)]
struct Turn {
    every: Option<Duration>,
    last: Option<Instant>,
}

impl Turn {
    /// Через сколько ему снимок; `None` — выключен.
    fn left(self, now: Instant) -> Option<Duration> {
        let every = self.every?;
        Some(self.last.map_or(Duration::ZERO, |last| {
            every.saturating_sub(now.saturating_duration_since(last))
        }))
    }

    fn is_due(self, now: Instant) -> bool {
        self.left(now) == Some(Duration::ZERO)
    }
}

/// Сколько спать: до ближайшего срока, но не дольше [`LOOK_AGAIN`]. На выходе
/// сроки не считаются — снимков никто не берёт, и наступивший срок крутил бы
/// цикл без паузы.
fn sleep_for(exiting: bool, turns: &[Turn], now: Instant) -> Duration {
    if exiting {
        return LOOK_AGAIN;
    }
    turns
        .iter()
        .filter_map(|turn| turn.left(now))
        .fold(LOOK_AGAIN, Duration::min)
}

/// Запустить опрос — один раз, после того как оценка поднята из файла.
pub fn spawn() {
    AsyncHandler::spawn(|| async {
        let mut report = Turn::default();
        let mut estimate = Turn::default();
        loop {
            let exiting = handle::Handle::global().is_exiting();
            if exiting {
                traffic_estimate::save().await;
            } else {
                report.every = client_report::traffic_every().await;
                estimate.every = traffic_estimate::sample_every().await;
                let now = Instant::now();
                let (to_report, to_estimate) = (report.is_due(now), estimate.is_due(now));
                if to_report || to_estimate {
                    let response = handle::Handle::mihomo().get_connections().await.ok();
                    if to_report {
                        report.last = Some(now);
                        client_report::count_traffic(response.as_ref());
                    }
                    if to_estimate {
                        estimate.last = Some(now);
                        traffic_estimate::count(response.as_ref()).await;
                    }
                }
            }
            tokio::time::sleep(sleep_for(exiting, &[report, estimate], Instant::now())).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::{LOOK_AGAIN, Turn, sleep_for};

    const fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    #[test]
    fn the_poll_follows_the_most_frequent_consumer_and_each_gets_its_own_turn() {
        let start = Instant::now();
        let report = Turn {
            every: Some(secs(10)),
            last: Some(start),
        };
        let estimate = Turn {
            every: Some(secs(300)),
            last: Some(start),
        };
        // Отчёт включён — к ядру раз в 10 с; оценке её снимок — в её срок.
        assert_eq!(sleep_for(false, &[report, estimate], start), secs(10));
        let later = start + secs(10);
        assert!(report.is_due(later));
        assert!(!estimate.is_due(later));
        assert!(estimate.is_due(start + secs(300)));

        // Отчёт выключен — оценка опрашивает со своим интервалом.
        let off = Turn::default();
        assert!(!off.is_due(start));
        let at = start + secs(295);
        assert_eq!(sleep_for(false, &[off, estimate], at), secs(5));
        assert_eq!(sleep_for(false, &[off, estimate], start + secs(100)), LOOK_AGAIN);

        // Никому не нужно — к ядру не ходим, только спрашиваем снова.
        assert_eq!(sleep_for(false, &[off, off], start), LOOK_AGAIN);
        // Только что включился — снимок сразу.
        let fresh = Turn {
            every: Some(secs(30)),
            last: None,
        };
        assert!(fresh.is_due(start));
        assert_eq!(sleep_for(false, &[fresh], start), Duration::ZERO);

        // Выход: срок наступил, но снимков не берут — цикл не крутится вхолостую.
        assert_eq!(sleep_for(true, &[fresh, report], later), LOOK_AGAIN);
    }

    #[test]
    fn the_connections_table_is_read_here_only() {
        let poll = include_str!("connections_poll.rs");
        assert!(poll.contains("get_connections()"));
        for (file, source) in [
            ("traffic_estimate.rs", include_str!("traffic_estimate.rs")),
            ("client_report/mod.rs", include_str!("../module/client_report/mod.rs")),
        ] {
            let production = source.split("#[cfg(test)]").next().unwrap_or_default();
            assert!(
                !production.contains("get_connections"),
                "{file}: таблицу соединений читает общий опрос"
            );
        }
        let window = include_str!("../feat/window.rs");
        assert!(
            !window.contains(concat!("traffic_estimate", "::", "resume")),
            "опрос после отменённого выхода идёт сам"
        );
    }
}
