use crate::{
    config::{Config, IVerge},
    core::handle,
    feat::tun::SetupAnswer,
};
use clash_verge_logging::{Type, logging};
use futures::StreamExt as _;
use std::env;
use tauri_plugin_clipboard_manager::ClipboardExt as _;

/// Сколько закрытий держим в полёте одновременно.
const CLOSE_AT_ONCE: usize = 16;

fn goes_through(chains: &[std::string::String], previous_proxy: &str) -> bool {
    chains.iter().any(|hop| hop == previous_proxy)
}

pub async fn close_connections_via(previous_proxy: &str) -> usize {
    if previous_proxy.trim().is_empty() {
        return 0;
    }
    if !Config::verge().await.latest_arc().auto_close_connection() {
        return 0;
    }
    let listed = match handle::Handle::mihomo().get_connections().await {
        Ok(listed) => listed,
        Err(err) => {
            logging!(
                warn,
                Type::ProxyMode,
                "could not list connections after the node change: {err}"
            );
            return 0;
        }
    };
    let ids: Vec<String> = listed
        .connections
        .unwrap_or_default()
        .into_iter()
        .filter(|conn| goes_through(&conn.chains, previous_proxy))
        .map(|conn| conn.id)
        .collect();
    // clod:node-switch — соединения закрываются пачкой: на сотне-другой
    // последовательные запросы к ядру растягивают разрыв на секунды, и всё это
    // время трафик продолжает идти через прежний узел.
    let total = ids.len();
    let closed = futures::stream::iter(ids)
        .map(|id| async move {
            match handle::Handle::mihomo().close_connection(&id).await {
                Ok(()) => 1_usize,
                Err(err) => {
                    logging!(debug, Type::ProxyMode, "connection {id} was not closed: {err}");
                    0
                }
            }
        })
        .buffer_unordered(CLOSE_AT_ONCE)
        .fold(0_usize, |sum, one| async move { sum + one })
        .await;
    logging!(
        info,
        Type::ProxyMode,
        "node change: closed {closed} of {total} connection(s) that went through {previous_proxy}"
    );
    closed
}

/// Чем кончилось нажатие переключателя в трее или горячей клавишей.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Toggled {
    /// Настройку переключили; внутри — что теперь стоит.
    To(bool),
    /// Ничего не переключали; причину, если она есть, уже сказали.
    Untouched,
    /// Ничего не переключали: способ подключения запер провайдер.
    Locked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SystemProxyStep {
    LeaveEverythingAlone,
    RefuseLocked,
    DropConnectionsThenSwitch,
    Switch,
}

const fn system_proxy_step(
    exiting: bool,
    current: bool,
    allowed: bool,
    auto_close_connection: bool,
    tun_carries_traffic: bool,
) -> SystemProxyStep {
    if exiting {
        return SystemProxyStep::LeaveEverythingAlone;
    }
    if !current && !allowed {
        return SystemProxyStep::RefuseLocked;
    }
    if current && auto_close_connection && !tun_carries_traffic {
        SystemProxyStep::DropConnectionsThenSwitch
    } else {
        SystemProxyStep::Switch
    }
}

fn exit_is_under_way() -> bool {
    match super::refuse_while_exiting() {
        Ok(()) => false,
        Err(err) => {
            logging!(info, Type::ProxyMode, "{err}");
            true
        }
    }
}

pub async fn toggle_system_proxy() -> Toggled {
    // Направление — по тому, что стоит в системе, как у кнопки в окне: прокси
    // мог снять кто-то другой, и тогда нажатие должно его включить.
    let current = crate::core::sysopt::Sysopt::global().our_proxy_is_on().await;
    let (allowed, _) = super::current_allowed_targets().await;
    let (auto_close_connection, tun_carries_traffic) = {
        let snapshot = Config::verge().await.latest_arc();
        (
            snapshot.auto_close_connection(),
            snapshot.enable_tun_mode.unwrap_or(false),
        )
    };

    match system_proxy_step(
        exit_is_under_way(),
        current,
        allowed,
        auto_close_connection,
        tun_carries_traffic,
    ) {
        SystemProxyStep::LeaveEverythingAlone => return Toggled::Untouched,
        SystemProxyStep::RefuseLocked => return Toggled::Locked,
        SystemProxyStep::DropConnectionsThenSwitch => {
            if let Err(err) = handle::Handle::mihomo().close_all_connections().await {
                logging!(error, Type::ProxyMode, "Failed to close all connections: {err}");
            }
        }
        SystemProxyStep::Switch => {}
    }

    let requested = !current;
    let patch_result = super::patch_verge(
        &IVerge {
            enable_system_proxy: Some(requested),
            connect_system_proxy: Some(requested),
            ..IVerge::default()
        },
        false,
    )
    .await;

    match patch_result {
        Ok(_) => {
            handle::Handle::refresh_verge();
            toggled_as_asked(
                requested,
                Config::verge()
                    .await
                    .latest_arc()
                    .enable_system_proxy
                    .unwrap_or(requested),
            )
        }
        Err(err) => {
            logging!(error, Type::ProxyMode, "{err}");
            let system_refused = crate::core::sysopt::Sysopt::global().write_failed();
            let reason = if system_refused {
                crate::cmd::public_error_text(&crate::core::sysopt::under_the_write_failed_title(&err))
            } else {
                crate::cmd::public_error_text(&err)
            };
            crate::core::notification::tell_the_outcome(
                system_proxy_failure_status(system_refused),
                reason.to_string(),
                crate::utils::notification::NotificationEvent::SystemProxyNotSwitched {
                    system_refused,
                    reason: &reason,
                },
            )
            .await;
            Toggled::Untouched
        }
    }
}

/// Переключено — только если записано то, о чём просили: шаг, который вернул
/// настройку назад (ядро не запущено, ядро отвергло конфиг), о себе уже сказал.
const fn toggled_as_asked(requested: bool, recorded: bool) -> Toggled {
    if requested == recorded {
        Toggled::To(recorded)
    } else {
        Toggled::Untouched
    }
}

/// Чем объяснить человеку несработавшее переключение системного прокси.
const fn system_proxy_failure_status(the_system_refused_the_write: bool) -> &'static str {
    if the_system_refused_the_write {
        "sysproxy::write_failed"
    } else {
        "sysproxy::switch_failed"
    }
}

/// Почему по нажатию «TUN» ничего не записано.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TunRefusal {
    /// Сказать нечего или уже сказано.
    Quiet,
    /// Причина отказа, которую человеку ещё не сказали.
    Untold(&'static str),
    /// Способ подключения запер провайдер.
    Locked,
}

/// Что записать по нажатию «TUN» в трее или клавишей. Включается, как в окне,
/// только при готовой службе и вне замка провайдера: отказ в правах, неудача
/// установки, уже идущая или отложенная подготовка TUN не включают.
async fn tun_switch_to(
    exiting: bool,
    active: bool,
    allowed: bool,
    prepare: impl AsyncFnOnce() -> SetupAnswer,
) -> Result<bool, TunRefusal> {
    if exiting {
        return Err(TunRefusal::Quiet);
    }
    if active {
        return Ok(false);
    }
    if !allowed {
        return Err(TunRefusal::Locked);
    }
    match prepare().await {
        SetupAnswer::Ready => Ok(true),
        SetupAnswer::NotReady => Err(TunRefusal::Quiet),
        SetupAnswer::Refused(marker) => Err(TunRefusal::Untold(marker)),
    }
}

pub async fn toggle_tun_mode() -> Toggled {
    let desired = Config::verge().await.latest_arc().enable_tun_mode.unwrap_or(false);
    let active = crate::feat::tun::is_active_with(desired);
    let (_, allowed) = super::current_allowed_targets().await;

    let enable = match tun_switch_to(exit_is_under_way(), active, allowed, async || {
        crate::feat::tun::ensure_ready(true).await.answer()
    })
    .await
    {
        Ok(enable) => enable,
        Err(TunRefusal::Quiet) => return Toggled::Untouched,
        Err(TunRefusal::Untold(marker)) => {
            crate::core::notification::tell_the_outcome(
                marker,
                std::string::String::new(),
                crate::utils::notification::NotificationEvent::TunSetupRefused { marker },
            )
            .await;
            return Toggled::Untouched;
        }
        Err(TunRefusal::Locked) => return Toggled::Locked,
    };

    match super::patch_verge(
        &IVerge {
            enable_tun_mode: Some(enable),
            connect_tun_mode: Some(enable),
            ..IVerge::default()
        },
        false,
    )
    .await
    {
        Ok(_) => {
            handle::Handle::refresh_verge();
            Toggled::To(enable)
        }
        Err(err) => {
            logging!(error, Type::ProxyMode, "{err}");
            // clod:e3-04 — отказ шага уже не значит, что настройка откатилась:
            // после удавшегося перезапуска ядра `patch_verge` сохраняет её и
            // всё равно возвращает ошибку. Отвечаем тем, что реально записано,
            // иначе уведомление скажет «TUN выключен» при работающем TUN.
            toggled_as_asked(
                enable,
                Config::verge().await.data_arc().enable_tun_mode.unwrap_or(desired),
            )
        }
    }
}

pub async fn copy_clash_env() -> anyhow::Result<()> {
    let env_ip = env::var("CLASH_VERGE_REV_IP").ok();
    let verge_cfg = Config::verge().await.latest_arc();
    let reachable = Config::our_proxy_host(&verge_cfg).await;
    let ip = env_ip.as_deref().unwrap_or(&reachable);

    let app_handle = handle::Handle::app_handle();
    let port = Config::effective_mixed_port().await;
    let http_proxy = format!("http://{ip}:{port}");
    let socks5_proxy = format!("socks5://{ip}:{port}");

    let clipboard = app_handle.clipboard();

    let default_env = {
        #[cfg(not(target_os = "windows"))]
        {
            "bash"
        }
        #[cfg(target_os = "windows")]
        {
            "powershell"
        }
    };
    let env_type = verge_cfg.env_type.as_deref().unwrap_or(default_env);

    let export_text = match env_type {
        "bash" => format!("export https_proxy={http_proxy} http_proxy={http_proxy} all_proxy={socks5_proxy}"),
        "cmd" => format!("set http_proxy={http_proxy}\r\nset https_proxy={http_proxy}"),
        "powershell" => {
            format!("$env:HTTP_PROXY=\"{http_proxy}\"; $env:HTTPS_PROXY=\"{http_proxy}\"")
        }
        "nushell" => {
            format!("load-env {{ http_proxy: \"{http_proxy}\", https_proxy: \"{http_proxy}\" }}")
        }
        "fish" => format!("set -x http_proxy {http_proxy}; set -x https_proxy {http_proxy}"),
        _ => anyhow::bail!("copy_clash_env: invalid env type {env_type}"),
    };

    clipboard.write_text(&export_text).map_err(|err| {
        anyhow::anyhow!(
            "{}",
            clash_verge_i18n::t!("errors.clipboardNotWritten", reason = err.to_string())
        )
    })
}

#[cfg(test)]
mod tests {
    use super::{
        SetupAnswer, SystemProxyStep, Toggled, TunRefusal, goes_through, system_proxy_failure_status,
        system_proxy_step, toggled_as_asked, tun_switch_to,
    };

    fn chain(hops: &[&str]) -> Vec<std::string::String> {
        hops.iter().map(|hop| (*hop).to_owned()).collect()
    }

    async fn tun_switch(exiting: bool, active: bool, answer: SetupAnswer) -> (Result<bool, TunRefusal>, bool) {
        tun_switch_allowed(exiting, active, true, answer).await
    }

    async fn tun_switch_allowed(
        exiting: bool,
        active: bool,
        allowed: bool,
        answer: SetupAnswer,
    ) -> (Result<bool, TunRefusal>, bool) {
        let asked = std::sync::atomic::AtomicBool::new(false);
        let target = tun_switch_to(exiting, active, allowed, async || {
            asked.store(true, std::sync::atomic::Ordering::Relaxed);
            answer
        })
        .await;
        (target, asked.load(std::sync::atomic::Ordering::Relaxed))
    }

    #[tokio::test]
    async fn while_the_app_is_leaving_the_tun_switch_never_reaches_the_rights_prompt() {
        for active in [false, true] {
            assert_eq!(
                tun_switch(true, active, SetupAnswer::Ready).await,
                (Err(TunRefusal::Quiet), false),
                "active={active}: во время выхода подготовка службы не запускается"
            );
        }
    }

    #[tokio::test]
    async fn the_tray_switches_tun_on_only_when_the_service_is_ready() {
        assert_eq!(tun_switch(false, false, SetupAnswer::Ready).await, (Ok(true), true));
        assert_eq!(tun_switch(false, true, SetupAnswer::Ready).await, (Ok(false), false));
    }

    #[tokio::test]
    async fn a_refused_service_setup_leaves_tun_off_and_names_the_reason_as_in_the_window() {
        for (answer, untold) in [
            (SetupAnswer::NotReady, TunRefusal::Quiet),
            (
                SetupAnswer::Refused("tun::setup_busy"),
                TunRefusal::Untold("tun::setup_busy"),
            ),
            (
                SetupAnswer::Refused("tun::setup_pending"),
                TunRefusal::Untold("tun::setup_pending"),
            ),
        ] {
            assert_eq!(
                tun_switch(false, false, answer).await,
                (Err(untold), true),
                "{answer:?}: TUN не включается, а причину отказа окно показало бы"
            );
        }
    }

    #[tokio::test]
    async fn a_tun_the_provider_locked_out_is_never_switched_on_but_can_be_switched_off() {
        assert_eq!(
            tun_switch_allowed(false, false, false, SetupAnswer::Ready).await,
            (Err(TunRefusal::Locked), false),
            "замок «только прокси»: TUN не включается и права не спрашиваются"
        );
        assert_eq!(
            tun_switch_allowed(false, true, false, SetupAnswer::Ready).await,
            (Ok(false), false),
            "включённый вне замка TUN выключить можно"
        );
    }

    #[test]
    fn a_system_proxy_the_provider_locked_out_is_never_switched_on_but_can_be_switched_off() {
        for auto_close_connection in [false, true] {
            for tun_carries_traffic in [false, true] {
                assert_eq!(
                    system_proxy_step(false, false, false, auto_close_connection, tun_carries_traffic),
                    SystemProxyStep::RefuseLocked,
                    "замок «только TUN»: системный прокси не включается"
                );
                assert!(
                    !matches!(
                        system_proxy_step(false, true, false, auto_close_connection, tun_carries_traffic),
                        SystemProxyStep::LeaveEverythingAlone | SystemProxyStep::RefuseLocked
                    ),
                    "включённый вне замка системный прокси выключить можно"
                );
            }
        }
    }

    #[test]
    fn a_switch_that_was_turned_back_is_not_reported_as_done() {
        assert_eq!(toggled_as_asked(true, true), Toggled::To(true));
        assert_eq!(toggled_as_asked(false, false), Toggled::To(false));
        assert_eq!(
            toggled_as_asked(true, false),
            Toggled::Untouched,
            "ядро не запущено — прокси не включён, «выключен» не пишем"
        );
        assert_eq!(
            toggled_as_asked(false, true),
            Toggled::Untouched,
            "выключение откатилось — «включён» не пишем"
        );
    }

    #[test]
    fn a_failed_system_proxy_switch_is_always_explained() {
        assert_eq!(system_proxy_failure_status(true), "sysproxy::write_failed");
        assert_eq!(
            system_proxy_failure_status(false),
            "sysproxy::switch_failed",
            "отказ не в записи — тоже со своей причиной, а не только в журнале"
        );
    }

    #[test]
    fn while_the_app_is_leaving_the_system_proxy_switch_never_drops_connections() {
        for current in [false, true] {
            for auto_close_connection in [false, true] {
                for tun_carries_traffic in [false, true] {
                    assert_eq!(
                        system_proxy_step(true, current, true, auto_close_connection, tun_carries_traffic),
                        SystemProxyStep::LeaveEverythingAlone,
                        "{current}/{auto_close_connection}/{tun_carries_traffic}: \
                         соединения не рвутся ради настройки, которая не применится"
                    );
                }
            }
        }
    }

    const EVERY_SETTING: [(bool, bool, bool, SystemProxyStep); 8] = [
        (false, false, false, SystemProxyStep::Switch),
        (false, false, true, SystemProxyStep::Switch),
        (false, true, false, SystemProxyStep::Switch),
        (false, true, true, SystemProxyStep::Switch),
        (true, false, false, SystemProxyStep::Switch),
        (true, false, true, SystemProxyStep::Switch),
        (true, true, false, SystemProxyStep::DropConnectionsThenSwitch),
        (true, true, true, SystemProxyStep::Switch),
    ];

    #[test]
    fn connections_are_dropped_only_when_the_proxy_that_carried_them_is_switched_off() {
        let mut covered = [false; 8];
        for (current, auto_close_connection, tun_carries_traffic, expected) in EVERY_SETTING {
            let slot =
                usize::from(current) * 4 + usize::from(auto_close_connection) * 2 + usize::from(tun_carries_traffic);
            assert!(
                !covered[slot],
                "{current}/{auto_close_connection}/{tun_carries_traffic}: сочетание описано дважды"
            );
            covered[slot] = true;
            assert_eq!(
                system_proxy_step(false, current, true, auto_close_connection, tun_carries_traffic),
                expected,
                "{current}/{auto_close_connection}/{tun_carries_traffic}"
            );
        }
        assert!(
            covered.iter().all(|seen| *seen),
            "таблица обычного случая заявлена исчерпывающей, но сочетание пропущено"
        );
    }

    #[test]
    fn a_connection_through_the_previous_node_is_recognised() {
        assert!(goes_through(&chain(&["Germany 01"]), "Germany 01"));
    }

    #[test]
    fn a_relay_counts_wherever_the_node_sits_in_the_chain() {
        assert!(goes_through(&chain(&["Germany 01", "Relay", "GLOBAL"]), "GLOBAL"));
        assert!(goes_through(&chain(&["Germany 01", "Relay", "GLOBAL"]), "Relay"));
    }

    #[test]
    fn other_connections_are_left_alone() {
        assert!(!goes_through(&chain(&["Germany 02"]), "Germany 01"));
        assert!(!goes_through(&chain(&[]), "Germany 01"));
        assert!(!goes_through(&chain(&["DIRECT"]), "Germany 01"));
    }

    #[test]
    fn the_node_name_is_matched_whole_not_by_prefix() {
        assert!(!goes_through(&chain(&["Germany 011"]), "Germany 01"));
        assert!(!goes_through(&chain(&["Germany 0"]), "Germany 01"));
        assert!(!goes_through(&chain(&["germany 01"]), "Germany 01"));
    }
}
