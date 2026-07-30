//! clod:simple-mode — when the Connect targets last came up.
//!
//! The session timer under the Connect button needs the moment the system
//! proxy or TUN went active. The frontend cannot remember it reliably — the
//! home screen unmounts, while connects and disconnects also happen from the
//! settings page and the tray — so the application records the transition
//! here, at the one place every toggle funnels through (`feat::patch_verge`).

use std::sync::atomic::{AtomicI64, Ordering};

/// Epoch milliseconds of the current session start; `0` = not connected.
static SESSION_START_MS: AtomicI64 = AtomicI64::new(0);

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
        .max(1) // never collide with the "not connected" sentinel
}

/// Record the combined state of the Connect targets. Sets the start stamp on
/// the off→on edge, keeps it while the state stays on, clears it on off.
pub fn record_connect_targets(active: bool) {
    if active {
        let _ = SESSION_START_MS.compare_exchange(0, now_ms(), Ordering::AcqRel, Ordering::Acquire);
    } else {
        SESSION_START_MS.store(0, Ordering::Release);
    }
}

/// The session start in epoch milliseconds, when a session is running.
pub fn connect_session_start() -> Option<i64> {
    match SESSION_START_MS.load(Ordering::Acquire) {
        0 => None,
        ms => Some(ms),
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn edge_transitions_set_and_clear_the_stamp() {
        record_connect_targets(false);
        assert_eq!(connect_session_start(), None);

        record_connect_targets(true);
        let first = connect_session_start().expect("stamp after on-edge");

        // staying on keeps the original stamp
        record_connect_targets(true);
        assert_eq!(connect_session_start(), Some(first));

        record_connect_targets(false);
        assert_eq!(connect_session_start(), None);
    }
}
