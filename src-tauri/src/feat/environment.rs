use crate::{
    config::Config,
    constants::timing,
    core::{handle, sysopt::Sysopt},
    process::AsyncHandler,
};
use clash_verge_logging::{Type, logging};
use std::{
    collections::BTreeSet,
    fmt::Write as _,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant, SystemTime},
};

static WATCHDOG_RUNNING: AtomicBool = AtomicBool::new(false);

const SLEEP_SLACK: Duration = Duration::from_secs(20);

const FINGERPRINT_ENTRIES_SHOWN: usize = 8;

const CORE_TUNNEL_BASE: &str = "meta";

fn looks_like_the_core_default_tunnel(name: &str) -> bool {
    name.strip_prefix(CORE_TUNNEL_BASE)
        .is_some_and(|index| index.chars().all(|c| c.is_ascii_digit()))
}

fn is_our_tunnel(name: &str) -> bool {
    let name = name.to_lowercase();
    name.contains("mihomo") || name.starts_with("utun") || looks_like_the_core_default_tunnel(&name)
}

fn v6_prefix(ip: std::net::Ipv6Addr) -> std::string::String {
    let segments = ip.segments();
    format!(
        "{:x}:{:x}:{:x}:{:x}::/64",
        segments[0], segments[1], segments[2], segments[3]
    )
}

fn network_fingerprint() -> BTreeSet<std::string::String> {
    let Ok(interfaces) = crate::cmd::network::get_network_interfaces_info() else {
        return BTreeSet::from([std::string::String::from("<unknown>")]);
    };

    interfaces
        .into_iter()
        .filter(|interface| !is_our_tunnel(&interface.name))
        .flat_map(|interface| {
            let name = interface.name.clone();
            interface
                .addr
                .into_iter()
                .map(move |addr| match addr {
                    network_interface::Addr::V4(v4) => format!("{name}:{}", v4.ip),
                    network_interface::Addr::V6(v6) => format!("{name}:{}", v6_prefix(v6.ip)),
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

fn listed<'a>(entries: impl Iterator<Item = &'a std::string::String>) -> std::string::String {
    let mut shown = 0_usize;
    let mut extra = 0_usize;
    let mut out = std::string::String::new();

    for entry in entries {
        if shown < FINGERPRINT_ENTRIES_SHOWN {
            if shown > 0 {
                out.push_str(", ");
            }
            out.push_str(entry);
            shown += 1;
        } else {
            extra += 1;
        }
    }

    if shown == 0 {
        return std::string::String::from("nothing");
    }
    if extra > 0 {
        let _ = write!(out, " and {extra} more");
    }
    out
}

fn report_fingerprint_change(before: &BTreeSet<std::string::String>, after: &BTreeSet<std::string::String>) {
    logging!(
        info,
        Type::Core,
        "[clod] network fingerprint: {} entries before, {} after; appeared {}; gone {}",
        before.len(),
        after.len(),
        listed(after.difference(before)),
        listed(before.difference(after))
    );
}

const CONNECTIONS_CALL_TIMEOUT: Duration = Duration::from_secs(3);

async fn close_live_connections(verbose: bool) {
    let live = if verbose {
        let mihomo = handle::Handle::mihomo().await;
        tokio::time::timeout(CONNECTIONS_CALL_TIMEOUT, mihomo.get_connections())
            .await
            .ok()
            .and_then(Result::ok)
            .and_then(|response| response.connections)
            .map_or(0, |connections| connections.len())
    } else {
        0
    };

    let outcome = {
        let mihomo = handle::Handle::mihomo().await;
        tokio::time::timeout(CONNECTIONS_CALL_TIMEOUT, mihomo.close_all_connections()).await
    };

    match outcome {
        Ok(Ok(())) if verbose => logging!(
            info,
            Type::Core,
            "[clod] closed {live} live connections after the environment changed"
        ),
        Ok(Ok(())) => (),
        Ok(Err(e)) => logging!(
            debug,
            Type::Core,
            "[clod] could not close connections after the environment changed: {e}"
        ),
        Err(_) => logging!(
            debug,
            Type::Core,
            "[clod] the core did not answer closing connections after the environment changed"
        ),
    }
}

async fn reconcile(reason: &str, slept: bool) {
    logging!(info, Type::Core, "[clod] environment changed ({reason}), reconciling");

    let verge = Config::verge().await.latest_arc();
    let wants_sysproxy = verge.enable_system_proxy.unwrap_or(false);
    let verbose = verge.verbose_diagnostics();
    let may_close_connections = verge.auto_close_connection();
    drop(verge);
    if wants_sysproxy {
        let was_failing = Sysopt::global().write_failed();
        match Sysopt::global().update_sysproxy().await {
            Ok(()) => Sysopt::global().refresh_guard().await,
            Err(e) => {
                logging!(warn, Type::Core, "[clod] failed to re-assert the system proxy: {e}");
                if !was_failing {
                    handle::Handle::notice_message("sysproxy::write_failed", e.to_string());
                }
            }
        }
    }

    if may_close_connections {
        close_live_connections(verbose).await;
    }

    if slept {
        AsyncHandler::spawn(|| async { crate::feat::tun::rearm_after_wake().await });
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
            if network_changed && crate::core::sysopt::verbose_diagnostics().await {
                report_fingerprint_change(&last_network, &network);
            }

            last_tick = now_tick;
            last_wall = now_wall;
            last_network = network;

            crate::feat::tun::enforce_undesired_off().await;

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
    use super::{
        CORE_TUNNEL_BASE, FINGERPRINT_ENTRIES_SHOWN, SLEEP_SLACK, is_our_tunnel, listed,
        looks_like_the_core_default_tunnel,
    };
    use crate::constants::timing;
    use std::time::Duration;

    #[test]
    fn an_empty_difference_is_spelled_out() {
        assert_eq!(listed(std::iter::empty()), "nothing");
    }

    #[test]
    fn a_long_difference_is_cut_and_counted() {
        let entries: Vec<std::string::String> = (0..FINGERPRINT_ENTRIES_SHOWN + 3)
            .map(|i| format!("if{i}:10.0.0.{i}"))
            .collect();
        let text = listed(entries.iter());

        assert!(text.starts_with("if0:10.0.0.0, if1:10.0.0.1"));
        assert!(text.ends_with("and 3 more"));
    }

    #[test]
    fn the_core_default_tunnel_is_the_base_name_with_an_index() {
        assert!(looks_like_the_core_default_tunnel(CORE_TUNNEL_BASE));
        assert!(looks_like_the_core_default_tunnel(&format!("{CORE_TUNNEL_BASE}0")));
        assert!(looks_like_the_core_default_tunnel(&format!("{CORE_TUNNEL_BASE}12")));
        assert!(!looks_like_the_core_default_tunnel(&format!("{CORE_TUNNEL_BASE}-work")));
        assert!(!looks_like_the_core_default_tunnel("tun0"));
        assert!(!looks_like_the_core_default_tunnel("wg0"));
        assert!(!looks_like_the_core_default_tunnel("eth0"));
    }

    #[test]
    fn the_tunnel_filter_does_not_depend_on_anything_that_can_change() {
        let ours = format!("{CORE_TUNNEL_BASE}0");

        assert!(is_our_tunnel("Mihomo"));
        assert!(is_our_tunnel("mihomo-tun"));
        assert!(is_our_tunnel("Meta"));
        assert!(is_our_tunnel(&ours));
        assert!(is_our_tunnel("utun4"));
        assert!(!is_our_tunnel("wg0"));
        assert!(!is_our_tunnel("eth0"));
        assert!(!is_our_tunnel("en0"));
        assert!(!is_our_tunnel("metavpn"));
    }

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
