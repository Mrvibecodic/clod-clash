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

/// Виртуальные коммутаторы и мосты локальных песочниц.
///
/// clod:net-virtual — Docker, WSL, Hyper-V, VirtualBox и VMware поднимают и
/// гасят свои адаптеры по команде пользователя; к пути машины наружу это
/// отношения не имеет. Без этого `wsl --shutdown` или остановка Docker рвали
/// все живые соединения ровно так же, как мигание Teredo.
fn is_a_local_sandbox_adapter(name: &str) -> bool {
    let name = name.to_lowercase();
    // Только имена, которые эти песочницы дают сами. Ни `veth`, ни `br-` сюда
    // не годятся: под них попали бы внешний коммутатор Hyper-V и домашний мост
    // `br-lan`, а на таких машинах это и есть единственный путь наружу.
    ["docker", "virbr", "vboxnet", "vmnet", "vmware", "wsl", "default switch"]
        .iter()
        .any(|known| name.contains(known))
}

fn is_our_tunnel(name: &str) -> bool {
    let name = name.to_lowercase();
    name.contains("mihomo") || name.starts_with("utun") || looks_like_the_core_default_tunnel(&name)
}

const fn v4_carries_traffic(ip: Ipv4Addr) -> bool {
    !ip.is_loopback() && !ip.is_link_local() && !ip.is_unspecified()
}

/// Переходные псевдотуннели IPv6: Teredo, 6to4, ISATAP.
///
/// clod:net-teredo — Windows поднимает и опускает их сама, по своему расписанию,
/// по нескольку раз в час, и путь машины к сети при этом не меняется. Трафик мы
/// через них не пускаем, так что в отпечатке сети им делать нечего: иначе
/// очередная спячка Teredo читается как «пропал адрес, к которому привязаны
/// соединения», и все живые соединения рвутся на ровном месте.
///
/// Определяются по адресу, а не по имени интерфейса: имя на Windows
/// локализуется, префиксы — нет.
const fn v6_is_transition_tunnel(ip: Ipv6Addr) -> bool {
    let segments = ip.segments();
    // Teredo — 2001:0::/32, 6to4 — 2002::/16.
    if (segments[0] == 0x2001 && segments[1] == 0) || segments[0] == 0x2002 {
        return true;
    }
    // ISATAP — идентификатор интерфейса ::0:5efe:a.b.c.d или ::200:5efe:a.b.c.d.
    matches!(segments[4], 0 | 0x0200) && segments[5] == 0x5efe
}

const fn v6_carries_traffic(ip: Ipv6Addr) -> bool {
    !ip.is_loopback() && !ip.is_unspecified() && (ip.segments()[0] & 0xffc0) != 0xfe80 && !v6_is_transition_tunnel(ip)
}

fn v6_prefix(ip: std::net::Ipv6Addr) -> std::string::String {
    let segments = ip.segments();
    format!(
        "{:x}:{:x}:{:x}:{:x}::/64",
        segments[0], segments[1], segments[2], segments[3]
    )
}

/// Отпечаток сети — адреса, по которым трафик действительно может уйти.
///
/// clod:net-teredo — раньше в набор попадал каждый адрес каждого чужого
/// интерфейса, включая link-local и переходные туннели, а проверки
/// `*_carries_traffic` применялись только к отдельному флагу. Из-за этого
/// исчезновение адреса, которым никто не пользовался, считалось потерей пути.
/// Теперь набор и флаг говорят об одном и том же: пуст — сети нет.
fn network_fingerprint() -> Option<BTreeSet<std::string::String>> {
    let interfaces = crate::cmd::network::get_network_interfaces_info().ok()?;

    let mut entries = BTreeSet::new();

    for interface in interfaces {
        let network_interface::NetworkInterface { name, addr, .. } = interface;
        if is_our_tunnel(&name) || is_a_local_sandbox_adapter(&name) {
            continue;
        }
        for address in addr {
            match address {
                network_interface::Addr::V4(v4) if v4_carries_traffic(v4.ip) => {
                    entries.insert(format!("{name}:{}", v4.ip));
                }
                network_interface::Addr::V6(v6) if v6_carries_traffic(v6.ip) => {
                    entries.insert(format!("{name}:{}", v6_prefix(v6.ip)));
                }
                _ => (),
            }
        }
    }

    Some(entries)
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

async fn reconcile(
    reason: &str,
    slept: bool,
    path_was_lost: bool,
    tun_is_being_rearmed: bool,
    network_carries_traffic: bool,
) {
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

    if network_carries_traffic {
        AsyncHandler::spawn(|| async { refill_empty_rule_sets().await });
    }
}

static RULE_SETS_REFILLING: AtomicBool = AtomicBool::new(false);
static RULE_SETS_REFILL_ASKED_AGAIN: AtomicBool = AtomicBool::new(false);
const RULE_SET_LIST_TIMEOUT: Duration = Duration::from_secs(5);
const RULE_SET_FETCH_TIMEOUT: Duration = Duration::from_secs(25);

async fn refill_empty_rule_sets() {
    RULE_SETS_REFILL_ASKED_AGAIN.store(true, Ordering::SeqCst);
    loop {
        if RULE_SETS_REFILLING.swap(true, Ordering::SeqCst) {
            return;
        }
        {
            scopeguard::defer! {
                RULE_SETS_REFILLING.store(false, Ordering::SeqCst);
            }
            while RULE_SETS_REFILL_ASKED_AGAIN.swap(false, Ordering::SeqCst) {
                if handle::Handle::global().is_exiting() {
                    return;
                }
                refill_empty_rule_sets_once().await;
            }
        }
        if !RULE_SETS_REFILL_ASKED_AGAIN.load(Ordering::SeqCst) {
            return;
        }
    }
}

async fn refill_empty_rule_sets_once() {
    let listed = {
        let mihomo = handle::Handle::mihomo().await;
        tokio::time::timeout(RULE_SET_LIST_TIMEOUT, mihomo.get_rule_providers()).await
    };
    let Ok(Ok(listed)) = listed else {
        return;
    };
    let mut empty: Vec<String> = listed
        .providers
        .values()
        .filter(|provider| {
            provider.rule_count == 0 && matches!(provider.vehicle_type, tauri_plugin_mihomo::models::VehicleType::HTTP)
        })
        .map(|provider| provider.name.clone())
        .collect();
    if empty.is_empty() {
        return;
    }
    empty.sort();
    logging!(
        info,
        Type::Core,
        "[clod] {} rule set(s) are still empty after the network came back; asking the core to fetch them: {}",
        empty.len(),
        empty.join(", ")
    );
    for name in empty {
        if handle::Handle::global().is_exiting() {
            return;
        }
        let fetched = {
            let mihomo = handle::Handle::mihomo().await;
            tokio::time::timeout(RULE_SET_FETCH_TIMEOUT, mihomo.update_rule_provider(&name)).await
        };
        match fetched {
            Ok(Ok(())) => {}
            Ok(Err(e)) => logging!(info, Type::Core, "[clod] rule set {name} is not fetched yet: {e}"),
            Err(_) => logging!(
                info,
                Type::Core,
                "[clod] rule set {name}: no answer from the core within {}s; leaving it to the core's own retry",
                RULE_SET_FETCH_TIMEOUT.as_secs()
            ),
        }
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
    if handle::Handle::global().is_exiting() {
        return;
    }
    if WATCHDOG_RUNNING.swap(true, Ordering::AcqRel) {
        return;
    }

    AsyncHandler::spawn(|| async {
        scopeguard::defer! {
            WATCHDOG_RUNNING.store(false, Ordering::Release);
            if !handle::Handle::global().is_exiting() {
                logging!(warn, Type::Core, "[clod] the environment watchdog stopped; it comes back with the next core start");
            }
        }
        let mut last_tick = Instant::now();
        let mut last_awake = sleep_clock::reading();
        let mut last_network = network_fingerprint().unwrap_or_default();
        let mut ticks: u32 = 0;

        loop {
            tokio::time::sleep(timing::ENVIRONMENT_TICK).await;
            ticks = ticks.wrapping_add(1);
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

            if slept || ticks.is_multiple_of(timing::TUN_UNWANTED_SWEEP_EVERY_TICKS) {
                crate::feat::tun::enforce_undesired_off().await;
            }

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
                    reconcile("woke up", true, false, false, false).await;
                }
                continue;
            };
            LISTING_FAILED.store(false, Ordering::Release);

            let network_changed = view != last_network;
            let path_was_lost = path_was_lost(&last_network, &view);
            if network_changed {
                report_fingerprint_change(&last_network, &view, verbose_diagnostics().await);
            }
            let view_carries_traffic = !view.is_empty();
            last_network = view;

            let rearm_is_now = rearm_is_due(view_carries_traffic);

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

            reconcile(reason, slept, path_was_lost, rearm_is_now, view_carries_traffic).await;
            if rearm_is_now {
                rearm_the_tun_after_wake().await;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{
        CORE_TUNNEL_BASE, FINGERPRINT_ENTRIES_SHOWN, SLEEP_SLACK, interface_of, is_a_local_sandbox_adapter,
        is_our_tunnel, listed, looks_like_the_core_default_tunnel, path_was_lost, sleep_gap, slept_through,
        spelled_out, v4_carries_traffic, v6_carries_traffic, v6_is_transition_tunnel, worth_spelling_out,
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

    #[test]
    fn a_sandbox_adapter_is_not_a_path_of_its_own() {
        assert!(is_a_local_sandbox_adapter("docker0"));
        assert!(is_a_local_sandbox_adapter("docker_gwbridge"));
        assert!(is_a_local_sandbox_adapter("vEthernet (WSL (Hyper-V firewall))"));
        assert!(is_a_local_sandbox_adapter("vEthernet (Default Switch)"));
        assert!(is_a_local_sandbox_adapter("vboxnet0"));
        assert!(is_a_local_sandbox_adapter("virbr0"));
        assert!(is_a_local_sandbox_adapter("VMware Network Adapter VMnet8"));

        assert!(!is_a_local_sandbox_adapter("eth0"));
        assert!(!is_a_local_sandbox_adapter("wlan0"));
        assert!(!is_a_local_sandbox_adapter("Ethernet 2"));
        assert!(!is_a_local_sandbox_adapter("Wi-Fi"));
        assert!(!is_a_local_sandbox_adapter("en0"));
        assert!(!is_a_local_sandbox_adapter("bridge0"));
        // Домашний мост и внешний коммутатор Hyper-V — это путь наружу.
        assert!(!is_a_local_sandbox_adapter("br-lan"));
        assert!(!is_a_local_sandbox_adapter("br0"));
        assert!(!is_a_local_sandbox_adapter("vEthernet (External Switch)"));
    }

    #[test]
    fn a_transition_tunnel_is_not_a_path_of_its_own() {
        let teredo = Ipv6Addr::new(0x2001, 0, 0x0a0a, 0x0a0a, 0, 0, 0, 1);
        let six_to_four = Ipv6Addr::new(0x2002, 0xc000, 0x0204, 0, 0, 0, 0, 1);
        let isatap = Ipv6Addr::new(0x2a02, 1, 2, 3, 0, 0x5efe, 0xc0a8, 0x0105);
        let isatap_public = Ipv6Addr::new(0x2a02, 1, 2, 3, 0x0200, 0x5efe, 0xc0a8, 0x0105);

        assert!(v6_is_transition_tunnel(teredo));
        assert!(v6_is_transition_tunnel(six_to_four));
        assert!(v6_is_transition_tunnel(isatap));
        assert!(v6_is_transition_tunnel(isatap_public));

        assert!(!v6_carries_traffic(teredo));
        assert!(!v6_carries_traffic(six_to_four));
        assert!(!v6_carries_traffic(isatap));
        assert!(!v6_carries_traffic(isatap_public));

        // Соседние адреса из тех же диапазонов остаются обычной сетью.
        assert!(!v6_is_transition_tunnel(Ipv6Addr::new(
            0x2001, 0x0db8, 0, 0, 0, 0, 0, 1
        )));
        assert!(!v6_is_transition_tunnel(Ipv6Addr::new(0x2003, 1, 2, 3, 4, 5, 6, 7)));
        assert!(v6_carries_traffic(Ipv6Addr::new(0x2001, 0x0db8, 0, 0, 0, 0, 0, 1)));
    }
}
