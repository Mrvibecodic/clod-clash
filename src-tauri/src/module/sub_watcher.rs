//! clod:F7 — subscription expiry and traffic notifications.
//!
//! The key requirement: notifications do NOT depend on subscription
//! refreshes. `expire` is an absolute timestamp known in advance, so the
//! expiry side works entirely offline off the local clock; the traffic side
//! prefers a lightweight `GET {url}/info` (Remnawave) and falls back to the
//! last known counters when the network is away.
//!
//! Checks run at startup (+30 s catch-up pass), then hourly, and after every
//! successful profile update. Each configured threshold fires exactly once
//! per period (`notified` map in the profile data); when several thresholds
//! were missed while the app was off, only the strictest one is shown and
//! the rest are marked silently.

use std::{collections::BTreeMap, time::Duration};

use serde_json::Value;

use crate::{
    config::sub_headers::{DEFAULT_NOTIFY_EXPIRE_DAYS, DEFAULT_NOTIFY_TRAFFIC_PERCENT},
    config::{Config, PrfItem, profiles_patch_item_safe},
    process::AsyncHandler,
    utils::{
        network::{NetworkManager, ProxyType},
        notification::{NotificationEvent, notify_event},
    },
};
use clash_verge_logging::{Type, logging};

const STARTUP_DELAY: Duration = Duration::from_secs(30);
const CHECK_INTERVAL: Duration = Duration::from_secs(60 * 60);
const INFO_TIMEOUT_SECS: u64 = 5;
const DAY_SECS: i64 = 24 * 60 * 60;

/// `notified` key for a passed traffic threshold.
fn traffic_key(percent: u32) -> String {
    format!("traffic_{percent}")
}

/// `notified` key for a passed expiry threshold.
fn expire_key(days: u32) -> String {
    format!("expire_{days}d")
}

/// `notified` key for "the subscription has expired".
const EXPIRED_KEY: &str = "expired";

/// The `expire` timestamp the expiry flags belong to. A renewal moves
/// `expire`, which resets the whole expiry family.
const EXPIRE_BASE_KEY: &str = "expire_base";

// ---------------------------------------------------------------------------
// pure logic
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Alert {
    Expired,
    ExpiresInDays(u32),
    TrafficPercent(u32),
}

struct Snapshot {
    /// Unix seconds; 0 = never expires.
    expire: u64,
    /// Bytes; 0 = unlimited.
    total: u64,
    /// Bytes used (upload + download, or the panel's counter).
    used: u64,
    /// Resolved thresholds; empty = the family is off.
    expire_days: Vec<u32>,
    traffic_percent: Vec<u32>,
    notified: BTreeMap<String, i64>,
}

struct Outcome {
    /// At most one alert per family, the strictest of the missed ones.
    alerts: Vec<Alert>,
    /// The map to persist when `changed`.
    notified: BTreeMap<String, i64>,
    changed: bool,
}

/// One watcher pass over a snapshot: which alerts to show and what the
/// anti-spam state becomes. Pure — the clock comes in as an argument.
fn evaluate(snap: &Snapshot, now_secs: i64) -> Outcome {
    let mut map = snap.notified.clone();

    // Thresholds the panel no longer configures lose their state; new ones
    // start from zero simply by not being in the map yet.
    let valid_key = |key: &str| -> bool {
        key == EXPIRE_BASE_KEY
            || (key == EXPIRED_KEY && !snap.expire_days.is_empty())
            || snap.traffic_percent.iter().any(|p| key == traffic_key(*p))
            || snap.expire_days.iter().any(|d| key == expire_key(*d))
    };
    map.retain(|key, _| valid_key(key));

    let mut alerts = Vec::new();

    // --- expiry family: local clock only, works offline -------------------
    if snap.expire != 0 && !snap.expire_days.is_empty() {
        // A moved expire (renewal) resets the family.
        let base = snap.expire as i64;
        if map.get(EXPIRE_BASE_KEY) != Some(&base) {
            map.retain(|key, _| !key.starts_with("expire_") && key != EXPIRED_KEY);
            map.insert(EXPIRE_BASE_KEY.into(), base);
        }

        let remaining = base - now_secs;

        // Everything that has already passed, strictest first.
        let mut passed: Vec<(String, Option<Alert>)> = Vec::new();
        if remaining <= 0 {
            passed.push((EXPIRED_KEY.into(), Some(Alert::Expired)));
        }
        let mut days_sorted = snap.expire_days.clone();
        days_sorted.sort_unstable();
        for days in days_sorted {
            if remaining > 0 && remaining <= i64::from(days) * DAY_SECS {
                passed.push((expire_key(days), Some(Alert::ExpiresInDays(days))));
            } else if remaining <= 0 {
                // Expired covers every day-threshold; mark them silently.
                passed.push((expire_key(days), None));
            }
        }

        // Show the strictest unnotified one, mark all passed.
        let mut shown = false;
        for (key, alert) in passed {
            let already = map.contains_key(&key);
            if !already {
                if !shown && let Some(alert) = alert {
                    alerts.push(alert);
                    shown = true;
                }
                map.insert(key, now_secs);
            } else if alert.is_some() {
                // The strictest passed threshold was already notified; the
                // milder ones below it cannot be "news" either.
                shown = true;
            }
        }
    } else if snap.expire == 0 {
        // Became unlimited: drop the family state including the base.
        map.retain(|key, _| !key.starts_with("expire_") && key != EXPIRED_KEY && key != EXPIRE_BASE_KEY);
    }

    // --- traffic family ----------------------------------------------------
    if snap.total != 0 && !snap.traffic_percent.is_empty() {
        let percent = (snap.used.saturating_mul(100) / snap.total) as u32;

        // Refill or quota raise: the percentage dropped below a threshold
        // that had fired — arm it again.
        for p in &snap.traffic_percent {
            if percent < *p {
                map.remove(&traffic_key(*p));
            }
        }

        let mut passed: Vec<u32> = snap.traffic_percent.iter().copied().filter(|p| percent >= *p).collect();
        passed.sort_unstable_by(|a, b| b.cmp(a)); // strictest (largest) first

        let mut shown = false;
        for p in passed {
            let key = traffic_key(p);
            let already = map.contains_key(&key);
            if !already {
                if !shown {
                    alerts.push(Alert::TrafficPercent(p));
                    shown = true;
                }
                map.insert(key, now_secs);
            } else {
                shown = true;
            }
        }
    }

    let changed = map != snap.notified;
    Outcome {
        alerts,
        notified: map,
        changed,
    }
}

// ---------------------------------------------------------------------------
// data plumbing
// ---------------------------------------------------------------------------

/// Best-effort fresh traffic counters from the panel: Remnawave answers
/// `GET {url}/info` with `user.trafficUsedBytes` / `user.trafficLimitBytes`.
/// Tolerant parsing (numbers or numeric strings); any failure means "use the
/// stored counters" — never an error the user sees.
async fn fetch_fresh_traffic(url: &str) -> Option<(u64, u64)> {
    let info_url = format!("{}/info", url.trim_end_matches('/'));
    for proxy in [ProxyType::None, ProxyType::Localhost] {
        let attempt = async {
            let client = NetworkManager::new()
                .create_request(
                    proxy,
                    Some(INFO_TIMEOUT_SECS),
                    Some(crate::utils::hwid::user_agent()),
                    false,
                )
                .await
                .ok()?;
            let response = client.get(&info_url).send().await.ok()?;
            if !response.status().is_success() {
                return None;
            }
            let value: Value = response.json().await.ok()?;
            let user = value.get("user")?;
            let as_u64 =
                |v: &Value| -> Option<u64> { v.as_u64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok())) };
            let used = as_u64(user.get("trafficUsedBytes")?)?;
            let limit = user.get("trafficLimitBytes").and_then(&as_u64).unwrap_or(0);
            Some((used, limit))
        };
        if let Some(pair) = attempt.await {
            return Some(pair);
        }
    }
    None
}

async fn notify_alert(alert: Alert) {
    match alert {
        Alert::Expired => notify_event(NotificationEvent::SubExpired).await,
        Alert::ExpiresInDays(days) => notify_event(NotificationEvent::SubExpiresIn { days }).await,
        Alert::TrafficPercent(percent) => notify_event(NotificationEvent::SubTraffic { percent }).await,
    }
}

fn now_unix_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// One full watcher pass over the current profile.
pub async fn run_check() {
    // The user's global switch outranks whatever the panel configured.
    if !Config::verge()
        .await
        .latest_arc()
        .enable_sub_notifications
        .unwrap_or(true)
    {
        return;
    }

    let (uid, item) = {
        let profiles = Config::profiles().await.latest_arc();
        let Some(uid) = profiles.get_current().cloned() else {
            return;
        };
        let Ok(item) = profiles.get_item(&uid) else {
            return;
        };
        (uid, item.clone())
    };

    let Some(extra) = item.extra else {
        // No panel metadata and no thresholds — nothing to watch.
        return;
    };

    let expire_days = item
        .notify_expire_days
        .clone()
        .unwrap_or_else(|| DEFAULT_NOTIFY_EXPIRE_DAYS.to_vec());
    let traffic_percent = item
        .notify_traffic_percent
        .clone()
        .unwrap_or_else(|| DEFAULT_NOTIFY_TRAFFIC_PERCENT.to_vec());

    // Fresh counters only when the traffic side is actually watched.
    let stored = (extra.upload.saturating_add(extra.download), extra.total);
    let (used, total) = if extra.total != 0 && !traffic_percent.is_empty() {
        match item.url.as_deref() {
            Some(url) => fetch_fresh_traffic(url).await.unwrap_or(stored),
            None => stored,
        }
    } else {
        stored
    };

    let snap = Snapshot {
        expire: extra.expire,
        total,
        used,
        expire_days,
        traffic_percent,
        notified: item.notified.clone().unwrap_or_default(),
    };

    let outcome = evaluate(&snap, now_unix_secs());

    for alert in &outcome.alerts {
        logging!(info, Type::System, "subscription alert: {alert:?}");
        notify_alert(*alert).await;
    }

    if outcome.changed {
        let patch = PrfItem {
            notified: Some(outcome.notified),
            ..PrfItem::default()
        };
        if let Err(err) = profiles_patch_item_safe(&uid, &patch).await {
            logging!(warn, Type::System, "failed to persist notification state: {err:#}");
        }
    }
}

/// Startup catch-up pass, then an hourly cadence. Deliberately not tied to
/// the profile-update timer: the expiry side must fire even with the network
/// long gone. (A resume from sleep is covered by the next hourly tick.)
pub fn spawn() {
    AsyncHandler::spawn(|| async {
        tokio::time::sleep(STARTUP_DELAY).await;
        loop {
            run_check().await;
            tokio::time::sleep(CHECK_INTERVAL).await;
        }
    });
}

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const GB: u64 = 1024 * 1024 * 1024;

    fn snap() -> Snapshot {
        Snapshot {
            expire: 0,
            total: 0,
            used: 0,
            expire_days: DEFAULT_NOTIFY_EXPIRE_DAYS.to_vec(),
            traffic_percent: DEFAULT_NOTIFY_TRAFFIC_PERCENT.to_vec(),
            notified: BTreeMap::new(),
        }
    }

    #[test]
    fn unlimited_and_never_expiring_stay_silent() {
        let outcome = evaluate(&snap(), 1_000_000);
        assert!(outcome.alerts.is_empty());
        assert!(!outcome.changed);
    }

    #[test]
    fn traffic_thresholds_fire_once_and_escalate() {
        let now = 1_000_000;
        let mut s = snap();
        s.total = 100 * GB;
        s.used = 85 * GB;

        // 85% passes only the 80 threshold.
        let first = evaluate(&s, now);
        assert_eq!(first.alerts, vec![Alert::TrafficPercent(80)]);

        // Same numbers again: nothing new.
        s.notified = first.notified;
        let second = evaluate(&s, now + 60);
        assert!(second.alerts.is_empty());

        // 95%: one alert for 90, not another for 80.
        s.used = 95 * GB;
        s.notified = second.notified;
        let third = evaluate(&s, now + 120);
        assert_eq!(third.alerts, vec![Alert::TrafficPercent(90)]);
    }

    #[test]
    fn missed_traffic_thresholds_collapse_into_the_strictest() {
        let mut s = snap();
        s.total = 100 * GB;
        s.used = 95 * GB; // both 80 and 90 passed while the app was off
        let outcome = evaluate(&s, 1_000_000);
        assert_eq!(outcome.alerts, vec![Alert::TrafficPercent(90)]);
        // ...but both are marked.
        assert!(outcome.notified.contains_key("traffic_80"));
        assert!(outcome.notified.contains_key("traffic_90"));
    }

    #[test]
    fn refill_rearms_traffic_thresholds() {
        let now = 1_000_000;
        let mut s = snap();
        s.total = 100 * GB;
        s.used = 85 * GB;
        s.notified = evaluate(&s, now).notified;

        // Refill: usage drops to 5%.
        s.used = 5 * GB;
        let after_refill = evaluate(&s, now + 60);
        assert!(after_refill.alerts.is_empty());
        assert!(!after_refill.notified.contains_key("traffic_80"));

        // The threshold fires again on the next climb.
        s.notified = after_refill.notified;
        s.used = 85 * GB;
        let again = evaluate(&s, now + 120);
        assert_eq!(again.alerts, vec![Alert::TrafficPercent(80)]);
    }

    #[test]
    fn expiry_uses_the_local_clock_and_catches_up_with_the_strictest() {
        let now = 1_000_000_000i64;
        let mut s = snap();
        // Expires in 2 days: both the 7- and the 3-day thresholds passed.
        s.expire = (now + 2 * DAY_SECS) as u64;
        let outcome = evaluate(&s, now);
        assert_eq!(outcome.alerts, vec![Alert::ExpiresInDays(3)]);
        assert!(outcome.notified.contains_key("expire_3d"));
        assert!(outcome.notified.contains_key("expire_7d"));
        // The 1-day threshold has not passed and stays armed.
        assert!(!outcome.notified.contains_key("expire_1d"));

        // A day later: the 1-day threshold is news.
        s.notified = outcome.notified;
        let later = evaluate(&s, now + DAY_SECS + DAY_SECS / 2);
        assert_eq!(later.alerts, vec![Alert::ExpiresInDays(1)]);
    }

    #[test]
    fn expired_shows_once_and_covers_the_day_thresholds() {
        let now = 1_000_000_000i64;
        let mut s = snap();
        s.expire = (now - 10) as u64;
        let outcome = evaluate(&s, now);
        assert_eq!(outcome.alerts, vec![Alert::Expired]);
        // Day thresholds are silently marked, no second alert later.
        s.notified = outcome.notified;
        let later = evaluate(&s, now + DAY_SECS);
        assert!(later.alerts.is_empty());
    }

    #[test]
    fn renewal_resets_the_expiry_family() {
        let now = 1_000_000_000i64;
        let mut s = snap();
        s.expire = (now + DAY_SECS) as u64;
        s.notified = evaluate(&s, now).notified; // 7/3/1 marked

        // Renewal: a month of headroom — no alerts, flags dropped.
        s.expire = (now + 30 * DAY_SECS) as u64;
        let renewed = evaluate(&s, now + 60);
        assert!(renewed.alerts.is_empty());
        assert!(!renewed.notified.contains_key("expire_1d"));

        // The 7-day threshold arms again as the new period runs out.
        s.notified = renewed.notified;
        let close_again = evaluate(&s, now + 25 * DAY_SECS);
        assert_eq!(close_again.alerts, vec![Alert::ExpiresInDays(7)]);
    }

    #[test]
    fn off_lists_silence_their_family() {
        let now = 1_000_000_000i64;
        let mut s = snap();
        s.expire = (now - 10) as u64; // expired
        s.total = 100 * GB;
        s.used = 99 * GB;
        s.expire_days = Vec::new(); // notify-expire-days: off
        s.traffic_percent = Vec::new(); // notify-traffic-percent: off
        let outcome = evaluate(&s, now);
        assert!(outcome.alerts.is_empty());
    }

    #[test]
    fn changed_threshold_lists_drop_stale_state() {
        let now = 1_000_000_000i64;
        let mut s = snap();
        s.total = 100 * GB;
        s.used = 85 * GB;
        s.notified = evaluate(&s, now).notified; // traffic_80 marked

        // The panel switches to 50,90: the 80-state is stale; 50 has
        // passed long ago from the user's point of view — one alert for
        // the strictest passed (50 < 85 < 90 → only 50 passed).
        s.traffic_percent = vec![50, 90];
        let outcome = evaluate(&s, now + 60);
        assert_eq!(outcome.alerts, vec![Alert::TrafficPercent(50)]);
        assert!(!outcome.notified.contains_key("traffic_80"));
    }
}
