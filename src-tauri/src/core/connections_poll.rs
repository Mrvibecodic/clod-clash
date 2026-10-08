//! Один опрос таблицы соединений ядра на всех, кто считает по ней трафик:
//! сборщик отчёта (`module::client_report`, раз в 5 секунд, пока копится
//! отчёт) и оценку расхода (`core::traffic_estimate`, раз в 30–300 секунд, пока
//! она нужна). К ядру опрос ходит с частотой самого частого из включённых, и
//! каждый снимок получают все включённые: ядро отдаёт только живые соединения,
//! закрытое между чтениями теряется, так что лишний снимок — меньше потерь, а
//! не лишний запрос. Приросты по `id` соединения каждый считает сам. Выключены
//! оба — к ядру не ходим.
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
/// он получил снимок в последний раз.
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

    /// Наступил срок — отметить его и сказать «да».
    fn take(&mut self, now: Instant) -> bool {
        let due = self.is_due(now);
        if due {
            self.last = Some(now);
        }
        due
    }
}

/// Пора ли к ядру: наступил срок хоть у одного. Тогда снимок получают все
/// включённые, и срок каждого отсчитывается от него.
fn poll_now(turns: &mut [Turn], now: Instant) -> bool {
    if !turns.iter().any(|turn| turn.is_due(now)) {
        return false;
    }
    for turn in turns.iter_mut().filter(|turn| turn.every.is_some()) {
        turn.last = Some(now);
    }
    true
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

/// Тик опроса: `None` — к ядру не ходим; иначе кому отдать снимок — отчёту
/// (`true`) и оценке (`Some(срок?)`: наступил ли её срок записать файл).
fn tick(turns: &mut [Turn; 2], estimate_term: &mut Turn, now: Instant) -> Option<(bool, Option<bool>)> {
    if !poll_now(turns, now) {
        return None;
    }
    let report = turns[0].every.is_some();
    let estimate = turns[1].every.is_some().then(|| estimate_term.take(now));
    Some((report, estimate))
}

/// Запустить опрос — один раз, после того как оценка поднята из файла.
pub fn spawn() {
    AsyncHandler::spawn(|| async {
        // [отчёт, оценка]
        let mut turns = [Turn::default(); 2];
        // Свой срок оценки — по нему она пишет файл, сколько бы снимков ни
        // получала между ними.
        let mut estimate_term = Turn::default();
        loop {
            let exiting = handle::Handle::global().is_exiting();
            if exiting {
                traffic_estimate::save().await;
            } else {
                turns[0].every = client_report::traffic_every().await;
                turns[1].every = traffic_estimate::sample_every().await;
                estimate_term.every = turns[1].every;
                if let Some((report, estimate)) = tick(&mut turns, &mut estimate_term, Instant::now()) {
                    let response = handle::Handle::mihomo().get_connections().await.ok();
                    if report {
                        client_report::count_traffic(response.as_ref());
                    }
                    if let Some(term) = estimate {
                        traffic_estimate::count(response.as_ref(), term).await;
                    }
                }
            }
            tokio::time::sleep(sleep_for(exiting, &turns, Instant::now())).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::{LOOK_AGAIN, Turn, poll_now, sleep_for, tick};
    use crate::module::client_report::traffic_every_while;

    const fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    #[test]
    fn the_poll_follows_the_most_frequent_consumer_and_every_consumer_gets_each_snapshot() {
        let start = Instant::now();
        let report = Turn {
            every: Some(secs(5)),
            last: None,
        };
        let estimate = Turn {
            every: Some(secs(300)),
            last: None,
        };
        let mut turns = [report, estimate];
        // Оба только что включились — снимок сразу, обоим.
        assert!(poll_now(&mut turns, start));
        assert!(turns.iter().all(|turn| turn.last == Some(start)));
        // Отчёт включён — к ядру раз в 5 с, и каждый снимок получает и оценка.
        assert_eq!(sleep_for(false, &turns, start), secs(5));
        assert!(!poll_now(&mut turns, start + secs(3)), "срок ещё не наступил");
        let later = start + secs(5);
        assert!(poll_now(&mut turns, later));
        assert_eq!(turns[1].last, Some(later), "оценка получила снимок отчёта");

        // Отчёт выключен — оценка опрашивает со своим интервалом от последнего
        // снимка: нагрузка как без отчёта.
        turns[0] = Turn::default();
        assert!(!turns[0].is_due(later));
        assert_eq!(sleep_for(false, &turns, later + secs(100)), LOOK_AGAIN);
        assert_eq!(sleep_for(false, &turns, later + secs(295)), secs(5));
        assert!(!poll_now(&mut turns, later + secs(299)));
        assert!(poll_now(&mut turns, later + secs(300)));
        assert_eq!(turns[0].last, None, "выключенному снимок не отмечается");

        // Никому не нужно — к ядру не ходим, только спрашиваем снова.
        let mut off = [Turn::default(); 2];
        assert!(!poll_now(&mut off, start));
        assert_eq!(sleep_for(false, &off, start), LOOK_AGAIN);

        // Выход: срок наступил, но снимков не берут — цикл не крутится вхолостую.
        let fresh = Turn {
            every: Some(secs(30)),
            last: None,
        };
        assert_eq!(sleep_for(false, &[fresh], start), Duration::ZERO);
        assert_eq!(sleep_for(true, &[fresh, report], later), LOOK_AGAIN);
    }

    #[test]
    fn the_estimate_term_comes_in_its_own_time_whatever_the_snapshots() {
        let start = Instant::now();
        let mut term = Turn {
            every: Some(secs(30)),
            last: None,
        };
        // Снимки раз в 5 с: срок оценки — раз в 30 с, не на каждом снимке.
        let taken = (0..=12_u64).filter(|n| term.take(start + secs(n * 5))).count();
        assert_eq!(taken, 3);
        assert_eq!(term.last, Some(start + secs(60)));
    }

    /// Минута опроса, как её крутит цикл: сколько раз сходили к ядру, сколько
    /// снимков досталось отчёту и оценке и сколько раз наступил срок оценки.
    fn a_minute(report: Option<Duration>, estimate: Option<Duration>) -> [usize; 4] {
        let start = Instant::now();
        let mut turns = [
            Turn {
                every: report,
                last: None,
            },
            Turn {
                every: estimate,
                last: None,
            },
        ];
        let mut term = Turn {
            every: estimate,
            last: None,
        };
        let mut counted = [0; 4];
        let mut now = start;
        while now <= start + secs(60) {
            if let Some((to_report, to_estimate)) = tick(&mut turns, &mut term, now) {
                counted[0] += 1;
                counted[1] += usize::from(to_report);
                counted[2] += usize::from(to_estimate.is_some());
                counted[3] += usize::from(to_estimate == Some(true));
            }
            now += sleep_for(false, &turns, now);
        }
        counted
    }

    #[test]
    fn a_minute_of_snapshots_goes_to_whoever_needs_them() {
        let estimate = Some(secs(30));
        // Копится отчёт — к ядру раз в 5 с, каждый снимок и отчёту, и оценке;
        // файл оценки — в её срок, раз в 30 с.
        assert_eq!(a_minute(traffic_every_while(true), estimate), [13, 13, 13, 3]);
        // Отчёт не копится — снимки только в срок оценки.
        assert_eq!(a_minute(traffic_every_while(false), estimate), [3, 0, 3, 3]);
        assert_eq!(a_minute(traffic_every_while(true), None), [13, 13, 0, 0]);
        // Никому не нужно — к ядру не ходим.
        assert_eq!(a_minute(traffic_every_while(false), None), [0; 4]);
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
