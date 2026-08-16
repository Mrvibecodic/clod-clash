use crate::{
    config::Config,
    constants::timing,
    core::{handle, sysopt::Sysopt},
    process::AsyncHandler,
};
use clash_verge_logging::{Type, logging};
use std::{
    collections::BTreeSet,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant, SystemTime},
};

static WATCHDOG_RUNNING: AtomicBool = AtomicBool::new(false);

const SLEEP_SLACK: Duration = Duration::from_secs(20);

fn network_fingerprint() -> BTreeSet<std::string::String> {
    let Ok(interfaces) = crate::cmd::network::get_network_interfaces_info() else {
        return BTreeSet::from([std::string::String::from("<unknown>")]);
    };

    interfaces
        .into_iter()
        .flat_map(|interface| {
            let name = interface.name.clone();
            interface
                .addr
                .into_iter()
                .map(move |addr| match addr {
                    network_interface::Addr::V4(v4) => format!("{name}:{}", v4.ip),
                    network_interface::Addr::V6(v6) => format!("{name}:{}", v6.ip),
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

async fn reconcile(reason: &str, slept: bool) {
    logging!(info, Type::Core, "[clod] environment changed ({reason}), reconciling");

    let wants_sysproxy = Config::verge().await.latest_arc().enable_system_proxy.unwrap_or(false);
    if wants_sysproxy && let Err(e) = Sysopt::global().update_sysproxy().await {
        logging!(warn, Type::Core, "[clod] failed to re-assert the system proxy: {e}");
    }

    if let Err(e) = handle::Handle::mihomo().await.close_all_connections().await {
        logging!(
            debug,
            Type::Core,
            "[clod] could not close connections after the environment changed: {e}"
        );
    }

    if slept {
        crate::feat::tun::rearm_after_wake().await;
    } else {
        crate::feat::tun::recheck_after_network_change().await;
    }
}

pub fn spawn_environment_watchdog() {
    if WATCHDOG_RUNNING.swap(true, Ordering::AcqRel) {
        return;
    }

    AsyncHandler::spawn(|| async {
        let mut last_tick = Instant::now();
        let mut last_wall = SystemTime::now();
        let mut last_network = network_fingerprint();

        loop {
            tokio::time::sleep(timing::ENVIRONMENT_TICK).await;
            if handle::Handle::global().is_exiting() {
                return;
            }

            let now_tick = Instant::now();
            let now_wall = SystemTime::now();

            let wall_delta = now_wall.duration_since(last_wall).unwrap_or_default();
            let tick_delta = now_tick.duration_since(last_tick);
            let slept = wall_delta.saturating_sub(tick_delta) > SLEEP_SLACK;

            let network = network_fingerprint();
            let network_changed = network != last_network;

            last_tick = now_tick;
            last_wall = now_wall;
            last_network = network;

            let reason = match (slept, network_changed) {
                (true, true) => "woke up, network differs",
                (true, false) => "woke up",
                (false, true) => "network changed",
                (false, false) => continue,
            };

            reconcile(reason, slept).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::SLEEP_SLACK;
    use crate::constants::timing;
    use std::time::Duration;

    #[test]
    fn the_sleep_threshold_leaves_room_for_a_busy_machine() {
        assert!(SLEEP_SLACK > timing::ENVIRONMENT_TICK);
    }

    #[test]
    fn sleep_is_the_gap_between_two_clocks() {
        let slept = |wall: Duration, tick: Duration| wall.saturating_sub(tick) > SLEEP_SLACK;

        assert!(!slept(
            timing::ENVIRONMENT_TICK + Duration::from_millis(40),
            timing::ENVIRONMENT_TICK
        ));
        assert!(!slept(Duration::from_secs(45), Duration::from_secs(45)));
        assert!(slept(Duration::from_secs(3600), timing::ENVIRONMENT_TICK));
    }
}
