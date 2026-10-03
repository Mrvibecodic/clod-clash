//! Чистые решения проверки 16–20: кого проверять и что записать по итогу.

use serde::{Deserialize, Serialize};

/// «Работает» и «режется» — повтор не чаще раза в 3 суток.
pub(super) const RECHECK_AFTER: i64 = 3 * 24 * 60 * 60;
/// «Не отвечает» и попытка без итога — повтор через 6 часов.
pub(super) const RETRY_AFTER: i64 = 6 * 60 * 60;

/// Итог проверки узла в сети, который хранится и показывается.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Verdict {
    Ok,
    Frozen,
    Dead,
}

impl Verdict {
    /// Слово пометки для фронта и трея; «работает» пометкой не показывается.
    pub(crate) const fn mark(self) -> Option<&'static str> {
        match self {
            Self::Ok => None,
            Self::Frozen => Some("frozen"),
            Self::Dead => Some("dead"),
        }
    }
}

/// Что ответило ядро на `GET …/download`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Outcome {
    Verdict(Verdict),
    /// Ничего не известно: сайт ответил не тем, узел жив, но не пускает к
    /// сайту, данные ещё шли, когда вышло время. `answered` — сайт ответил
    /// через узел (код не ноль): что-то прошло, повторять сразу незачем.
    Unknown {
        answered: bool,
    },
}

impl Outcome {
    pub(super) fn parse(verdict: &str, status: i64) -> Self {
        match verdict {
            "ok" => Self::Verdict(Verdict::Ok),
            "frozen" => Self::Verdict(Verdict::Frozen),
            "dead" => Self::Verdict(Verdict::Dead),
            _ => Self::Unknown { answered: status != 0 },
        }
    }

    /// Через узел что-то прошло.
    pub(super) const fn answered(self) -> bool {
        matches!(
            self,
            Self::Verdict(Verdict::Ok | Verdict::Frozen) | Self::Unknown { answered: true }
        )
    }
}

/// Что известно об одном отпечатке в одной сети.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Node {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verdict: Option<Verdict>,
    /// Когда вынесен `verdict` (unix-секунды).
    #[serde(default)]
    pub at: i64,
    /// Когда проверяли в последний раз, с итогом или без.
    #[serde(default)]
    pub tried: i64,
    /// Имя узла в подписке на момент последней проверки — для прослойки,
    /// которой отпечатка мало.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<std::string::String>,
    /// Код ответа сайта в последней проверке (0 — ответа не было).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<i64>,
}

/// Ответ ядра на одну проверку: итог и код ответа сайта.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Checked {
    pub outcome: Outcome,
    pub status: i64,
}

/// Пора ли проверять отпечаток снова.
pub(super) const fn is_due(node: Option<&Node>, now: i64) -> bool {
    let Some(node) = node else {
        return true;
    };
    let since_try = now.saturating_sub(node.tried);
    match node.verdict {
        Some(Verdict::Ok | Verdict::Frozen) => since_try >= RECHECK_AFTER,
        Some(Verdict::Dead) | None => since_try >= RETRY_AFTER,
    }
}

/// Записать итог: вердикт — целиком, «неясно» — только время попытки.
pub(super) const fn apply(node: &mut Node, outcome: Outcome, now: i64) {
    node.tried = now;
    if let Outcome::Verdict(verdict) = outcome {
        node.verdict = Some(verdict);
        node.at = now;
    }
}

/// Заход записывается, только если через кого-то что-то прошло: когда не
/// прошло ничего нигде, это скорее сеть, чем серверы.
pub(super) fn worth_recording(outcomes: &[Outcome]) -> bool {
    outcomes.iter().any(|outcome| outcome.answered())
}

#[cfg(test)]
mod tests {
    use super::{Node, Outcome, RECHECK_AFTER, RETRY_AFTER, Verdict, apply, is_due, worth_recording};

    const NOW: i64 = 1_800_000_000;

    fn checked(verdict: Option<Verdict>, ago: i64) -> Node {
        Node {
            verdict,
            at: NOW - ago,
            tried: NOW - ago,
            ..Node::default()
        }
    }

    #[test]
    fn a_node_never_seen_is_due_and_results_wait_their_term() {
        assert!(is_due(None, NOW));
        let cases = [
            (Some(Verdict::Ok), RECHECK_AFTER - 1, false),
            (Some(Verdict::Ok), RECHECK_AFTER, true),
            (Some(Verdict::Frozen), RECHECK_AFTER - 1, false),
            (Some(Verdict::Frozen), RECHECK_AFTER, true),
            (Some(Verdict::Dead), RETRY_AFTER - 1, false),
            (Some(Verdict::Dead), RETRY_AFTER, true),
            (None, RETRY_AFTER - 1, false),
            (None, RETRY_AFTER, true),
        ];
        for (verdict, ago, due) in cases {
            assert_eq!(is_due(Some(&checked(verdict, ago)), NOW), due, "{verdict:?} {ago}");
        }
    }

    #[test]
    fn unknown_keeps_the_mark_but_moves_the_attempt() {
        let mut node = checked(Some(Verdict::Frozen), RECHECK_AFTER);
        apply(&mut node, Outcome::Unknown { answered: false }, NOW);
        assert_eq!(node.verdict, Some(Verdict::Frozen));
        assert_eq!(node.at, NOW - RECHECK_AFTER);
        assert_eq!(node.tried, NOW);
        assert!(!is_due(Some(&node), NOW + RETRY_AFTER - 1));
    }

    #[test]
    fn a_verdict_replaces_the_previous_one() {
        let mut node = checked(Some(Verdict::Dead), RETRY_AFTER);
        apply(&mut node, Outcome::Verdict(Verdict::Ok), NOW);
        assert_eq!(node, checked(Some(Verdict::Ok), 0));
    }

    #[test]
    fn a_pass_where_nothing_passed_anywhere_is_not_recorded() {
        let silent = Outcome::Unknown { answered: false };
        let refused = Outcome::Unknown { answered: true };
        assert!(!worth_recording(&[]));
        assert!(!worth_recording(&[Outcome::Verdict(Verdict::Dead), silent]));
        assert!(worth_recording(&[
            Outcome::Verdict(Verdict::Dead),
            Outcome::Verdict(Verdict::Frozen)
        ]));
        assert!(worth_recording(&[Outcome::Verdict(Verdict::Ok)]));
        // Сайт ответил отказом — через узел прошло, повторять сразу незачем
        assert!(worth_recording(&[Outcome::Verdict(Verdict::Dead), refused]));
    }

    #[test]
    fn verdicts_come_from_the_core_words() {
        assert_eq!(Outcome::parse("ok", 200), Outcome::Verdict(Verdict::Ok));
        assert_eq!(Outcome::parse("frozen", 200), Outcome::Verdict(Verdict::Frozen));
        assert_eq!(Outcome::parse("dead", 0), Outcome::Verdict(Verdict::Dead));
        assert_eq!(Outcome::parse("unknown", 403), Outcome::Unknown { answered: true });
        assert_eq!(Outcome::parse("", 0), Outcome::Unknown { answered: false });
        assert_eq!(Verdict::Ok.mark(), None);
        assert_eq!(Verdict::Frozen.mark(), Some("frozen"));
    }
}
