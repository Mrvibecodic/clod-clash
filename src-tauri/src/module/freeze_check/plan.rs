//! Чистые решения проверки 16–20: кого проверять и что записать по итогу.

use serde::{Deserialize, Deserializer, Serialize};

/// «Работает» и «режется» — повтор не чаще раза в 3 суток.
pub(super) const RECHECK_AFTER: i64 = 3 * 24 * 60 * 60;
/// Попытка без итога — повтор через 6 часов.
pub(super) const RETRY_AFTER: i64 = 6 * 60 * 60;
/// Сколько проверок подряд должны обрезаться, чтобы узел считался «режется».
pub(super) const WAVES: usize = 3;

/// Итог проверки узла в сети. Итог один — обрезается ли скачивание через
/// узел; всё прочее, что отвечает ядро (в том числе dead), итогом не считается.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Verdict {
    Ok,
    Frozen,
}

impl Verdict {
    /// Слово ядра; прочие слова итогом не считаются.
    pub(super) fn parse(word: &str) -> Option<Self> {
        match word {
            "ok" => Some(Self::Ok),
            "frozen" => Some(Self::Frozen),
            _ => None,
        }
    }

    /// Слово пометки для фронта и трея; «работает» пометкой не показывается.
    pub(crate) const fn mark(self) -> Option<&'static str> {
        match self {
            Self::Ok => None,
            Self::Frozen => Some("frozen"),
        }
    }
}

/// Прежние файлы хранили и `dead` — такой итог читается как никакой.
fn known_verdict<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<Verdict>, D::Error> {
    let word = Option::<std::string::String>::deserialize(deserializer)?;
    Ok(word.as_deref().and_then(Verdict::parse))
}

/// Что известно об одном отпечатке в одной сети.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Node {
    #[serde(default, deserialize_with = "known_verdict", skip_serializing_if = "Option::is_none")]
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

/// Проверка узла: итог (`None` — итога нет) и код ответа сайта.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Checked {
    pub verdict: Option<Verdict>,
    pub status: i64,
}

/// Пора ли проверять отпечаток снова: итогу 3 суток, попытке без итога 6 часов.
pub(super) const fn is_due(node: Option<&Node>, now: i64) -> bool {
    let Some(node) = node else {
        return true;
    };
    if now.saturating_sub(node.tried) < RETRY_AFTER {
        return false;
    }
    match node.verdict {
        Some(_) => now.saturating_sub(node.at) >= RECHECK_AFTER,
        None => true,
    }
}

/// Записать проверку: итог — целиком, без итога — только время попытки,
/// прежний итог остаётся.
pub(super) const fn apply(node: &mut Node, verdict: Option<Verdict>, now: i64) {
    node.tried = now;
    if let Some(verdict) = verdict {
        node.verdict = Some(verdict);
        node.at = now;
    }
}

#[cfg(test)]
mod tests {
    use super::{Node, RECHECK_AFTER, RETRY_AFTER, Verdict, apply, is_due};

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
            (None, RETRY_AFTER - 1, false),
            (None, RETRY_AFTER, true),
        ];
        for (verdict, ago, due) in cases {
            assert_eq!(is_due(Some(&checked(verdict, ago)), NOW), due, "{verdict:?} {ago}");
        }
    }

    #[test]
    fn a_check_without_a_result_keeps_the_verdict_and_is_retried_in_6_hours() {
        let mut node = checked(Some(Verdict::Frozen), RECHECK_AFTER);
        apply(&mut node, None, NOW);
        assert_eq!(
            (node.verdict, node.at, node.tried),
            (Some(Verdict::Frozen), NOW - RECHECK_AFTER, NOW)
        );
        assert!(!is_due(Some(&node), NOW + RETRY_AFTER - 1));
        assert!(
            is_due(Some(&node), NOW + RETRY_AFTER),
            "итогу 3 суток — повтор не ждёт ещё 3"
        );
    }

    #[test]
    fn a_verdict_replaces_the_previous_one() {
        let mut node = checked(Some(Verdict::Frozen), RECHECK_AFTER);
        apply(&mut node, Some(Verdict::Ok), NOW);
        assert_eq!(node, checked(Some(Verdict::Ok), 0));
    }

    #[test]
    fn only_ok_and_frozen_are_results() {
        assert_eq!(Verdict::parse("ok"), Some(Verdict::Ok));
        assert_eq!(Verdict::parse("frozen"), Some(Verdict::Frozen));
        for word in ["dead", "unknown", ""] {
            assert_eq!(Verdict::parse(word), None, "{word}");
        }
        assert_eq!(Verdict::Ok.mark(), None);
        assert_eq!(Verdict::Frozen.mark(), Some("frozen"));
    }

    #[test]
    fn a_saved_not_responding_reads_as_no_verdict() {
        let node: Node = serde_json::from_str(r#"{"verdict":"dead","at":5,"tried":7}"#).unwrap_or_default();
        assert_eq!((node.verdict, node.tried), (None, 7));
        assert!(is_due(Some(&node), 7 + RETRY_AFTER));
        let kept: Node = serde_json::from_str(r#"{"verdict":"frozen","tried":7}"#).unwrap_or_default();
        assert_eq!(kept.verdict, Some(Verdict::Frozen));
    }
}
