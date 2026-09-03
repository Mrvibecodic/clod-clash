use crate::{
    config::Config,
    constants::timing,
    core::{
        handle,
        sysopt::{Sysopt, verbose_diagnostics},
    },
    process::AsyncHandler,
};
use clash_verge_logging::{Type, logging};
use parking_lot::Mutex;
use std::{
    collections::BTreeSet,
    fmt::Write as _,
    net::{Ipv4Addr, Ipv6Addr},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

static WATCHDOG_RUNNING: AtomicBool = AtomicBool::new(false);
static WAKE_REARM_PENDING: AtomicBool = AtomicBool::new(false);
static LISTING_FAILED: AtomicBool = AtomicBool::new(false);
static CLOCK_FAILED: AtomicBool = AtomicBool::new(false);
static WAKE_REARM_SINCE: Mutex<Option<Instant>> = Mutex::new(None);

const SLEEP_SLACK: Duration = Duration::from_secs(20);

const FINGERPRINT_ENTRIES_SHOWN: usize = 8;

const CORE_TUNNEL_BASE: &str = "meta";

const WAITED_WORTH_SPELLING_OUT: Duration = Duration::from_secs(1);

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod sleep_clock {
    use std::time::Duration;

    #[cfg(target_os = "linux")]
    const CLOCK_COUNTING_SLEEP: libc::clockid_t = libc::CLOCK_BOOTTIME;
    #[cfg(target_os = "macos")]
    const CLOCK_COUNTING_SLEEP: libc::clockid_t = libc::CLOCK_MONOTONIC;

    pub fn reading() -> Option<Duration> {
        let mut moment = libc::timespec { tv_sec: 0, tv_nsec: 0 };
        let asked = unsafe { libc::clock_gettime(CLOCK_COUNTING_SLEEP, &raw mut moment) };
        if asked != 0 || moment.tv_sec < 0 || moment.tv_nsec < 0 {
            return None;
        }
        Some(Duration::new(moment.tv_sec as u64, moment.tv_nsec as u32))
    }

    pub const fn asleep(instant_delta: Duration, before: Duration, after: Duration) -> Duration {
        after.saturating_sub(before).saturating_sub(instant_delta)
    }
}

#[cfg(target_os = "windows")]
mod sleep_clock {
    use std::time::Duration;
    use windows_sys::Win32::System::WindowsProgramming::QueryUnbiasedInterruptTime;

    pub fn reading() -> Option<Duration> {
        let mut awake_in_100ns: u64 = 0;
        if unsafe { QueryUnbiasedInterruptTime(&raw mut awake_in_100ns) } == 0 {
            return None;
        }
        Some(Duration::from_nanos(awake_in_100ns.saturating_mul(100)))
    }

    pub const fn asleep(instant_delta: Duration, before: Duration, after: Duration) -> Duration {
        instant_delta.saturating_sub(after.saturating_sub(before))
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
mod sleep_clock {
    use std::time::Duration;

    pub const fn reading() -> Option<Duration> {
        None
    }

    pub const fn asleep(_instant_delta: Duration, _before: Duration, _after: Duration) -> Duration {
        Duration::ZERO
    }
}

fn sleep_gap(instant_delta: Duration, before: Option<Duration>, after: Option<Duration>) -> Option<Duration> {
    let (Some(before), Some(after)) = (before, after) else {
        if !CLOCK_FAILED.swap(true, Ordering::AcqRel) {
            logging!(
                warn,
                Type::Core,
                "[clod] the system did not tell how long it stayed awake; sleep goes unnoticed on this machine"
            );
        }
        return None;
    };
    CLOCK_FAILED.store(false, Ordering::Release);
    Some(sleep_clock::asleep(instant_delta, before, after))
}

fn slept_through(gap: Option<Duration>) -> bool {
    gap.is_some_and(|gap| gap > SLEEP_SLACK)
}

fn spelled_out(span: Duration) -> std::string::String {
    let seconds = span.as_secs();
    if seconds < 60 {
        return format!("{seconds}s");
    }
    if seconds < 3600 {
        return format!("{}m {}s", seconds / 60, seconds % 60);
    }
    format!("{}h {}m", seconds / 3600, (seconds % 3600) / 60)
}

fn looks_like_the_core_default_tunnel(name: &str) -> bool {
    name.strip_prefix(CORE_TUNNEL_BASE)
        .is_some_and(|index| index.chars().all(|c| c.is_ascii_digit()))
}

fn is_our_tunnel(name: &str) -> bool {
    let name = name.to_lowercase();
    name.contains("mihomo") || name.starts_with("utun") || looks_like_the_core_default_tunnel(&name)
}

const fn v4_carries_traffic(ip: Ipv4Addr) -> bool {
    !ip.is_loopback() && !ip.is_link_local() && !ip.is_unspecified()
}

const fn v6_carries_traffic(ip: Ipv6Addr) -> bool {
    !ip.is_loopback() && !ip.is_unspecified() && (ip.segments()[0] & 0xffc0) != 0xfe80
}

fn v6_prefix(ip: std::net::Ipv6Addr) -> std::string::String {
    let segments = ip.segments();
    format!(
        "{:x}:{:x}:{:x}:{:x}::/64",
        segments[0], segments[1], segments[2], segments[3]
    )
}

struct NetworkView {
    entries: BTreeSet<std::string::String>,
    carries_traffic: bool,
}

fn network_fingerprint() -> Option<NetworkView> {
    let interfaces = crate::cmd::network::get_network_interfaces_info().ok()?;

    let mut view = NetworkView {
        entries: BTreeSet::new(),
        carries_traffic: false,
    };

    for interface in interfaces {
        let network_interface::NetworkInterface { name, addr, .. } = interface;
        if is_our_tunnel(&name) {
            continue;
        }
        for address in addr {
            match address {
                network_interface::Addr::V4(v4) => {
                    view.carries_traffic |= v4_carries_traffic(v4.ip);
                    view.entries.insert(format!("{name}:{}", v4.ip));
                }
                network_interface::Addr::V6(v6) => {
                    view.carries_traffic |= v6_carries_traffic(v6.ip);
                    view.entries.insert(format!("{name}:{}", v6_prefix(v6.ip)));
                }
            }
        }
    }

    Some(view)
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

fn interface_of(entry: &str) -> &str {
    entry.split_once(':').map_or(entry, |(name, _)| name)
}

fn path_was_lost(before: &BTreeSet<std::string::String>, after: &BTreeSet<std::string::String>) -> bool {
    before.difference(after).next().is_some()
}

fn interfaces_of<'a>(entries: impl Iterator<Item = &'a std::string::String>) -> Vec<std::string::String> {
    let mut names: Vec<std::string::String> = entries.map(|entry| interface_of(entry).to_owned()).collect();
    names.dedup();
    names
}

fn report_fingerprint_change(
    before: &BTreeSet<std::string::String>,
    after: &BTreeSet<std::string::String>,
    verbose: bool,
) {
    if verbose {
        logging!(
            info,
            Type::Core,
            "[clod] network fingerprint: {} entries before, {} after; appeared {}; gone {}",
            before.len(),
            after.len(),
            listed(after.difference(before)),
            listed(before.difference(after))
        );
        return;
    }

    logging!(
        info,
        Type::Core,
        "[clod] network fingerprint: {} entries before, {} after; appeared on {}; gone from {}",
        before.len(),
        after.len(),
        listed(interfaces_of(after.difference(before)).iter()),
        listed(interfaces_of(before.difference(after)).iter())
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

async fn reconcile(reason: &str, slept: bool, path_was_lost: bool, tun_is_being_rearmed: bool) {
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

    if !may_close_connections {
        if verbose {
            logging!(
                info,
                Type::Core,
                "[clod] live connections left alone: closing them on network events is turned off"
            );
        }
    } else if slept || path_was_lost {
        logging!(
            info,
            Type::Core,
            "[clod] closing live connections: {}",
            match (slept, path_was_lost) {
                (true, true) => "the machine was asleep and an address is gone",
                (true, false) => "the machine was asleep",
                _ => "an address the connections were bound to is gone",
            }
        );
        close_live_connections(verbose).await;
    } else if verbose {
        logging!(
            info,
            Type::Core,
            "[clod] live connections left alone: no address went away, the old path still stands"
        );
    }

    if !slept && !tun_is_being_rearmed {
        crate::feat::tun::recheck_after_network_change().await;
    }
}

fn hold_the_tun_rearm(tun_is_wanted: bool) {
    *WAKE_REARM_SINCE.lock() = Some(Instant::now());
    if WAKE_REARM_PENDING.swap(true, Ordering::AcqRel) || !tun_is_wanted {
        return;
    }
    logging!(
        info,
        Type::Core,
        "[clod] the TUN device waits for a routable address before it is re-created"
    );
}

fn rearm_is_due(carries_traffic: bool) -> bool {
    carries_traffic && WAKE_REARM_PENDING.swap(false, Ordering::AcqRel)
}

fn worth_spelling_out(waited: Duration) -> Option<Duration> {
    (waited >= WAITED_WORTH_SPELLING_OUT).then_some(waited)
}

async fn rearm_the_tun_after_wake() {
    let waited = WAKE_REARM_SINCE
        .lock()
        .take()
        .map(|since| since.elapsed())
        .and_then(worth_spelling_out);
    if crate::feat::tun::desired().await {
        match waited {
            Some(waited) => logging!(
                info,
                Type::Core,
                "[clod] the network came back {} after the machine woke up; the TUN device gets a fresh budget",
                spelled_out(waited)
            ),
            None => logging!(
                info,
                Type::Core,
                "[clod] the machine woke up with the network already there; the TUN device gets a fresh budget"
            ),
        }
    } else {
        logging!(
            info,
            Type::Core,
            "[clod] the machine woke up with the network already there; the TUN device is switched off, nothing to re-create"
        );
    }
    AsyncHandler::spawn(|| async { crate::feat::tun::rearm_after_wake().await });
}

pub fn spawn_environment_watchdog() {
    if WATCHDOG_RUNNING.swap(true, Ordering::AcqRel) {
        return;
    }

    AsyncHandler::spawn(|| async {
        let mut last_tick = Instant::now();
        let mut last_awake = sleep_clock::reading();
        let mut last_network = network_fingerprint().map(|view| view.entries).unwrap_or_default();

        loop {
            tokio::time::sleep(timing::ENVIRONMENT_TICK).await;
            if handle::Handle::global().is_exiting() {
                return;
            }

            let now_tick = Instant::now();
            let now_awake = sleep_clock::reading();
            let instant_delta = now_tick.duration_since(last_tick);
            let gap = sleep_gap(instant_delta, last_awake, now_awake);
            let slept = slept_through(gap);
            if slept && let Some(gap) = gap {
                logging!(
                    info,
                    Type::Core,
                    "[clod] the machine was asleep for {}",
                    spelled_out(gap)
                );
            }

            let view = network_fingerprint();

            last_tick = now_tick;
            last_awake = now_awake;

            crate::feat::tun::enforce_undesired_off().await;

            if slept {
                hold_the_tun_rearm(crate::feat::tun::desired().await);
            }

            let Some(view) = view else {
                if !LISTING_FAILED.swap(true, Ordering::AcqRel) {
                    logging!(
                        warn,
                        Type::Core,
                        "[clod] the network interfaces could not be listed; the previous fingerprint stands"
                    );
                }
                if slept {
                    reconcile("woke up", true, false, false).await;
                }
                continue;
            };
            LISTING_FAILED.store(false, Ordering::Release);

            let network_changed = view.entries != last_network;
            let path_was_lost = path_was_lost(&last_network, &view.entries);
            if network_changed {
                report_fingerprint_change(&last_network, &view.entries, verbose_diagnostics().await);
            }
            last_network = view.entries;

            let rearm_is_now = rearm_is_due(view.carries_traffic);

            let reason = match (slept, network_changed) {
                (true, true) => "woke up, network differs",
                (true, false) => "woke up",
                (false, true) => "network changed",
                (false, false) => {
                    if rearm_is_now {
                        rearm_the_tun_after_wake().await;
                    }
                    continue;
                }
            };

            reconcile(reason, slept, path_was_lost, rearm_is_now).await;
            if rearm_is_now {
                rearm_the_tun_after_wake().await;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{
        CORE_TUNNEL_BASE, FINGERPRINT_ENTRIES_SHOWN, SLEEP_SLACK, interface_of, is_our_tunnel, listed,
        looks_like_the_core_default_tunnel, path_was_lost, sleep_gap, slept_through, spelled_out, v4_carries_traffic,
        v6_carries_traffic, worth_spelling_out,
    };
    use crate::constants::timing;
    use std::{
        net::{Ipv4Addr, Ipv6Addr},
        time::Duration,
    };

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
    fn an_entry_keeps_its_interface_name_even_when_the_address_has_colons() {
        assert_eq!(interface_of("eth0:192.168.1.5"), "eth0");
        assert_eq!(interface_of("eth0:2a02:1:2:3::/64"), "eth0");
        assert_eq!(interface_of("Ethernet 2:10.0.0.4"), "Ethernet 2");
        assert_eq!(interface_of("nothing"), "nothing");
    }

    #[test]
    fn only_a_vanished_entry_counts_as_a_lost_path() {
        let before: std::collections::BTreeSet<std::string::String> =
            ["eth0:10.0.0.2".into(), "wlan0:192.168.1.7".into()]
                .into_iter()
                .collect();
        let same = before.clone();
        let with_one_more: std::collections::BTreeSet<std::string::String> = [
            "eth0:10.0.0.2".into(),
            "wlan0:192.168.1.7".into(),
            "docker0:172.17.0.1".into(),
        ]
        .into_iter()
        .collect();
        let without_wlan: std::collections::BTreeSet<std::string::String> =
            std::iter::once("eth0:10.0.0.2".into()).collect();
        let readdressed: std::collections::BTreeSet<std::string::String> =
            ["eth0:10.0.0.2".into(), "wlan0:192.168.1.9".into()]
                .into_iter()
                .collect();

        assert!(!path_was_lost(&before, &same));
        assert!(!path_was_lost(&before, &with_one_more));
        assert!(path_was_lost(&before, &without_wlan));
        assert!(path_was_lost(&before, &readdressed));
    }

    #[test]
    fn the_sleep_threshold_leaves_room_for_a_busy_machine() {
        assert!(SLEEP_SLACK > timing::ENVIRONMENT_TICK);
    }

    #[test]
    fn a_missing_clock_reading_is_never_a_sleep() {
        assert!(!slept_through(sleep_gap(
            Duration::from_secs(3600),
            None,
            Some(Duration::from_secs(1))
        )));
        assert!(!slept_through(sleep_gap(
            Duration::from_secs(3600),
            Some(Duration::from_secs(1)),
            None
        )));
    }

    #[test]
    fn a_span_is_spelled_out_for_a_person() {
        assert_eq!(spelled_out(Duration::ZERO), "0s");
        assert_eq!(spelled_out(Duration::from_secs(9)), "9s");
        assert_eq!(spelled_out(Duration::from_secs(59)), "59s");
        assert_eq!(spelled_out(Duration::from_secs(60)), "1m 0s");
        assert_eq!(spelled_out(Duration::from_secs(75)), "1m 15s");
        assert_eq!(spelled_out(Duration::from_secs(3599)), "59m 59s");
        assert_eq!(spelled_out(Duration::from_secs(3600)), "1h 0m");
        assert_eq!(spelled_out(Duration::from_secs(7325)), "2h 2m");
    }

    #[test]
    fn a_wait_shorter_than_a_second_is_not_worth_a_number() {
        assert_eq!(worth_spelling_out(Duration::ZERO), None);
        assert_eq!(worth_spelling_out(Duration::from_millis(999)), None);
        assert_eq!(worth_spelling_out(Duration::from_secs(1)), Some(Duration::from_secs(1)));
        assert_eq!(
            worth_spelling_out(Duration::from_secs(45)),
            Some(Duration::from_secs(45))
        );
    }

    #[test]
    fn the_measured_sleep_is_what_gets_printed() {
        let tick = timing::ENVIRONMENT_TICK;
        let hour = Duration::from_secs(3600);

        #[cfg(any(target_os = "linux", target_os = "macos"))]
        let gap = sleep_gap(tick, Some(Duration::ZERO), Some(hour));
        #[cfg(target_os = "windows")]
        let gap = sleep_gap(hour, Some(Duration::ZERO), Some(tick));
        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        let gap = sleep_gap(hour, Some(Duration::ZERO), Some(tick));

        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        assert_eq!(gap, Some(Duration::ZERO));
        #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
        assert_eq!(gap, Some(hour - tick));
        assert_eq!(sleep_gap(tick, None, Some(hour)), None);
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn sleep_is_what_the_awake_clock_missed() {
        let tick = timing::ENVIRONMENT_TICK;
        assert!(!slept_through(sleep_gap(tick, Some(Duration::ZERO), Some(tick))));
        assert!(!slept_through(sleep_gap(
            Duration::from_secs(45),
            Some(Duration::ZERO),
            Some(Duration::from_secs(45))
        )));
        assert!(slept_through(sleep_gap(
            tick,
            Some(Duration::ZERO),
            Some(Duration::from_secs(3600))
        )));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn sleep_is_what_the_awake_clock_missed() {
        let tick = timing::ENVIRONMENT_TICK;
        assert!(!slept_through(sleep_gap(tick, Some(Duration::ZERO), Some(tick))));
        assert!(!slept_through(sleep_gap(
            Duration::from_secs(45),
            Some(Duration::ZERO),
            Some(Duration::from_secs(45))
        )));
        assert!(slept_through(sleep_gap(
            Duration::from_secs(3600),
            Some(Duration::ZERO),
            Some(tick)
        )));
    }

    #[test]
    fn only_a_routable_address_counts_as_a_network() {
        assert!(v4_carries_traffic(Ipv4Addr::new(192, 168, 1, 10)));
        assert!(!v4_carries_traffic(Ipv4Addr::LOCALHOST));
        assert!(!v4_carries_traffic(Ipv4Addr::UNSPECIFIED));
        assert!(!v4_carries_traffic(Ipv4Addr::new(169, 254, 3, 7)));

        assert!(v6_carries_traffic(Ipv6Addr::new(0x2a02, 1, 2, 3, 4, 5, 6, 7)));
        assert!(!v6_carries_traffic(Ipv6Addr::LOCALHOST));
        assert!(!v6_carries_traffic(Ipv6Addr::UNSPECIFIED));
        assert!(!v6_carries_traffic(Ipv6Addr::new(0xfe80, 0, 0, 0, 1, 2, 3, 4)));
    }
}
