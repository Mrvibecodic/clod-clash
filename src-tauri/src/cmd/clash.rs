use super::CmdResult;
use crate::enhance::dns_page;
use crate::feat;
use crate::{
    cmd::StringifyErr as _,
    config::{ClashInfo, Config},
    core::{
        CoreManager, handle,
        validate::{ValidationErrorKind, ValidationOutcome},
    },
};
use clash_verge_logging::{Type, logging, logging_error};
use compact_str::CompactString;
use serde_yaml_ng::Mapping;
use smartstring::alias::String;
use tokio::fs;

#[derive(serde::Serialize)]
pub struct CoreLadder {
    log_level: Option<std::string::String>,
    unified_delay: Option<bool>,
    mixed_port: Option<u16>,
    /// Закреплённое в `tun` у нас: окно TUN показывает его, а не собранный конфиг.
    tun: Option<Mapping>,
    /// Раздачу просил человек, а принятая сборка её не открыла: этот список
    /// адресов не читается. Тумблер раздачи стоит на просьбе и говорит почему.
    lan_sharing_held: Option<&'static str>,
    /// Туннели работающей сборки задала подписка: человек страницу «Туннели»
    /// не сохранял, и окно не выдаёт их за его выбор.
    tunnels_from_subscription: bool,
}

fn read_ladder(clash: &Mapping) -> CoreLadder {
    let log_level = clash.get("log-level").and_then(|value| match value.as_str() {
        Some(text) => Some(text.to_owned()),
        None => {
            logging!(
                warn,
                Type::Config,
                "log-level in the core config is not a string ({value:?}); the settings page will show it as unset"
            );
            None
        }
    });
    let unified_delay = clash.get("unified-delay").and_then(|value| match value.as_bool() {
        Some(flag) => Some(flag),
        None => {
            logging!(
                warn,
                Type::Config,
                "unified-delay in the core config is not a boolean ({value:?}); the settings page will show it as unset"
            );
            None
        }
    });
    // clod:port-ladder — «как в подписке» это ОТСУТСТВИЕ ключа у нас, поэтому
    // порт читается так же, как остальная лесенка: есть значение — закреплено.
    let mixed_port = clash.get("mixed-port").and_then(|value| {
        let port = match value {
            serde_yaml_ng::Value::Number(number) => number.as_u64(),
            serde_yaml_ng::Value::String(text) => text.parse().ok(),
            _ => None,
        };
        match port.filter(|port| (1..=65535).contains(port)) {
            Some(port) => u16::try_from(port).ok(),
            None => {
                logging!(
                    warn,
                    Type::Config,
                    "mixed-port in the core config is not a usable port ({value:?}); \
                     the settings page will show it as unset"
                );
                None
            }
        }
    });
    CoreLadder {
        log_level,
        unified_delay,
        mixed_port,
        tun: clash.get("tun").and_then(serde_yaml_ng::Value::as_mapping).cloned(),
        lan_sharing_held: None,
        tunnels_from_subscription: false,
    }
}

fn tunnels_from_subscription(clash: &Mapping, running: Option<&Mapping>) -> bool {
    !clash.contains_key("tunnels")
        && running
            .and_then(|config| config.get("tunnels"))
            .and_then(serde_yaml_ng::Value::as_sequence)
            .is_some_and(|tunnels| !tunnels.is_empty())
}

#[tauri::command]
pub async fn get_core_ladder() -> CmdResult<CoreLadder> {
    let clash = Config::clash().await;
    let clash = clash.latest_arc();
    let mut ladder = read_ladder(&clash.0);
    let runtime = Config::runtime().await.data_arc();
    ladder.lan_sharing_held = runtime.unreadable_lan_list;
    ladder.tunnels_from_subscription = tunnels_from_subscription(&clash.0, runtime.config.as_ref());
    Ok(ladder)
}

#[tauri::command]
pub async fn copy_clash_env() -> CmdResult {
    feat::copy_clash_env().await.stringify_err_log(|err| {
        logging!(error, Type::ProxyMode, "{err}");
    })
}

#[tauri::command]
pub async fn get_clash_info() -> CmdResult<ClashInfo> {
    let mut info = Config::clash().await.data_arc().get_client_info();
    // clod:port-ladder — интерфейсу нужен порт, на котором ядро СЕЙЧАС слушает,
    // а не наше умолчание: при «как в подписке» его задаёт провайдер.
    info.mixed_port = Config::effective_mixed_port().await;
    Ok(info)
}

#[tauri::command]
pub async fn patch_clash_config(payload: Mapping) -> CmdResult {
    feat::patch_clash(&payload).await.stringify_err()
}

#[tauri::command]
pub async fn patch_clash_mode(payload: String) -> CmdResult {
    feat::change_clash_mode(payload).await
}
#[tauri::command]
pub async fn change_clash_core(clash_core: String) -> CmdResult<Option<String>> {
    feat::refuse_while_exiting().stringify_err()?;
    logging!(info, Type::Config, "changing core to {clash_core}");

    logging_error!(Type::Core, crate::config::profiles::profiles_save_file_safe().await);
    match CoreManager::global().change_core(&clash_core).await {
        Ok(()) => {
            logging!(info, Type::Core, "core changed and restarted to {clash_core}");
            handle::Handle::notice_message("config_core::change_success", "");
            handle::Handle::refresh_clash();
            Ok(None)
        }
        Err(err) => {
            // Текст отказа ядра или сборки несёт пути и адреса: наружу — только
            // очищенный.
            let error_msg: String = super::public_error_text(&err);
            logging!(error, Type::Core, "failed to change core: {error_msg}");
            handle::Handle::notice_message("config_core::change_error", error_msg.clone());
            Ok(Some(error_msg))
        }
    }
}
#[tauri::command]
pub async fn refresh_geo_assets() -> CmdResult<usize> {
    crate::module::geo_assets::refresh_home_copies().await.stringify_err()
}

#[tauri::command]
pub async fn restart_core() -> CmdResult {
    feat::restart_clash_core().await.stringify_err()
}
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DnsSaveOutcome {
    saved: bool,
    validation: ValidationOutcome,
    /// Предостережение, с которым страница всё же сохранена: имена в хвостах
    /// `#имя`, которых ядро не знает как узлы или группы.
    warning: Option<String>,
    /// Файл записан, но доставить сборку ядру не удалось (ядро о содержимом
    /// ничего не сказало): применится при следующей сборке.
    delivery_error: Option<String>,
}

/// Сколько ждать ответа ядра о его группах и наборах правил при сохранении
/// страницы DNS. Не ответило — сверить ссылки нечем, страница сохраняется.
const DNS_REFERENCES_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(1500);

/// Что сверка ссылок страницы сказала: набора правил нет — отказ (ядро всё
/// равно отвергнет весь конфиг, а так причина названа словами); узла или группы
/// нет — только предостережение: хвост `#имя` без такого прокси ядро понимает
/// как сетевой интерфейс, и это штатная возможность.
#[derive(Debug, Default, PartialEq, Eq)]
struct DnsReferenceCheck {
    refusal: Option<String>,
    warning: Option<String>,
}

/// clod:dns-page-diff — сверить ссылки страницы с работающим ядром: `/proxies`
/// знает и узлы из провайдеров, которых в собранном конфиге не видно.
async fn check_dns_references(page: &dns_page::Page) -> DnsReferenceCheck {
    let refs = page.references();
    if refs.proxies.is_empty() && refs.rule_sets.is_empty() {
        return DnsReferenceCheck::default();
    }
    let core = handle::Handle::mihomo();
    let listed = tokio::time::timeout(DNS_REFERENCES_TIMEOUT, async {
        tokio::try_join!(core.get_proxies(), core.get_rule_providers())
    })
    .await;
    let Ok(Ok((proxies, providers))) = listed else {
        logging!(
            info,
            Type::Config,
            "ядро не ответило о группах и наборах правил — ссылки страницы DNS не сверены"
        );
        return DnsReferenceCheck::default();
    };
    let known_proxies: Vec<&str> = proxies.proxies.keys().map(std::string::String::as_str).collect();
    let known_rule_sets: Vec<&str> = providers.providers.keys().map(std::string::String::as_str).collect();
    dns_reference_check(&refs, &known_proxies, &known_rule_sets)
}

fn dns_reference_check(
    refs: &dns_page::References,
    known_proxies: &[&str],
    known_rule_sets: &[&str],
) -> DnsReferenceCheck {
    let quoted = |missing: Vec<&str>| {
        missing
            .iter()
            .map(|name| format!("«{name}»"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let proxies_missing: Vec<&str> = refs
        .proxies
        .iter()
        .map(std::string::String::as_str)
        .filter(|name| !known_proxies.contains(name))
        .collect();
    let rule_sets_missing: Vec<&str> = refs
        .rule_sets
        .iter()
        .map(std::string::String::as_str)
        .filter(|name| !known_rule_sets.contains(name))
        .collect();
    DnsReferenceCheck {
        refusal: (!rule_sets_missing.is_empty())
            .then(|| clash_verge_i18n::t!("dns.missingRuleSets", names = quoted(rule_sets_missing)).into()),
        warning: (!proxies_missing.is_empty())
            .then(|| clash_verge_i18n::t!("dns.missingProxies", names = quoted(proxies_missing)).into()),
    }
}

/// Сохранить страницу DNS текущей подписки.
///
/// От присланного остаются только отличия от блока подписки; кандидат
/// проверяется той же сборкой и тем же ядром, что потом с ним работает
/// (`stage_with` с наложенной страницей), а не отдельным проверочным файлом.
/// Принято — файл пишется, и при включённом тумблере сборка сразу едет в ядро.
/// Проверка не состоялась (занято, не запустилась) — файл всё равно пишется:
/// её отсутствие не приговор, ядро рассудит при следующей сборке.
#[tauri::command]
pub async fn save_dns_config(dns_config: Mapping) -> CmdResult<DnsSaveOutcome> {
    let (uid, base) = the_dns_page_subject().await;
    let (uid, dns_path) = uid
        .and_then(|uid| Some((uid.clone(), crate::utils::dirs::dns_page_path(&uid).ok()?)))
        .ok_or_else(|| clash_verge_i18n::t!("errors.noSubscriptionSelected").into_owned())?;
    let Some(page) = dns_page::Page::from_file(&dns_config) else {
        return Err(clash_verge_i18n::t!("errors.dnsPageNotMapping").into_owned().into());
    };

    let dns_settings_on = Config::verge().await.latest_arc().enable_dns_settings.unwrap_or(false);
    let page = page.differences_from(&base);

    let references = check_dns_references(&page).await;
    if let Some(refusal) = references.refusal {
        logging!(
            warn,
            Type::Config,
            "DNS page refers to rule sets the core does not know: {refusal}"
        );
        return Ok(DnsSaveOutcome {
            saved: false,
            validation: ValidationOutcome::invalid(ValidationErrorKind::CoreRejected, refusal),
            warning: None,
            delivery_error: None,
        });
    }

    let manager = CoreManager::global();
    let staged = manager
        .stage_unless_unchanged(crate::enhance::Sources::default().with_dns_page(uid, page.clone()))
        .await
        .stringify_err()?;
    let staged = match staged {
        Ok(staged) => staged,
        Err(validation) if validation.judged_the_config() => {
            logging!(warn, Type::Config, "DNS page rejected, nothing written: {validation}");
            return Ok(DnsSaveOutcome {
                saved: false,
                validation,
                warning: None,
                delivery_error: None,
            });
        }
        Err(validation) => {
            logging!(
                warn,
                Type::Config,
                "DNS page check reached no verdict, saving anyway: {validation}"
            );
            write_dns_page(&dns_path, &page).await?;
            // Без вердикта сборку никто не доставил: при включённом тумблере
            // применить страницу обычным путём, отказ — отдельным полем.
            let delivery_error = if dns_settings_on {
                manager
                    .update_config_checked()
                    .await
                    .err()
                    .map(|err| format!("{err:#}").into())
            } else {
                None
            };
            return Ok(DnsSaveOutcome {
                saved: true,
                validation,
                warning: references.warning,
                delivery_error,
            });
        }
    };

    write_dns_page(&dns_path, &page).await?;
    let (validation, delivery_error) = if dns_settings_on {
        match staged.deliver(crate::core::manager::Delivery::Reload).await {
            Ok(applied) => (applied.err().unwrap_or(ValidationOutcome::Valid), None),
            Err(err) => {
                logging!(
                    warn,
                    Type::Config,
                    "DNS page saved but not delivered to the core: {err:#}"
                );
                (ValidationOutcome::Valid, Some(format!("{err:#}").into()))
            }
        }
    } else {
        drop(staged);
        (ValidationOutcome::Valid, None)
    };

    Ok(DnsSaveOutcome {
        saved: true,
        validation,
        warning: references.warning,
        delivery_error,
    })
}

/// Подписка страницы DNS (текущая) и блок, от которого считаются её отличия, —
/// от сборки этой же подписки.
async fn the_dns_page_subject() -> (Option<String>, dns_page::Base) {
    let uid = Config::profiles().await.latest_arc().get_current().cloned();
    let base = base_of_the_page(uid.as_ref(), &Config::runtime().await.data_arc());
    (uid, base)
}

/// Принятая сборка другой подписки — блока для этой страницы нет.
fn base_of_the_page(uid: Option<&String>, runtime: &crate::config::runtime::IRuntime) -> dns_page::Base {
    if runtime.profile_uid.as_ref() == uid {
        runtime.dns_base.clone().unwrap_or_default()
    } else {
        dns_page::Base::default()
    }
}

async fn write_dns_page(dns_path: &std::path::Path, page: &dns_page::Page) -> CmdResult {
    page.save(dns_path).await.stringify_err()?;
    logging!(info, Type::Config, "DNS page saved to {dns_path:?}");
    Ok(())
}

#[tauri::command]
pub async fn apply_dns_config(apply: bool) -> CmdResult {
    let (failed, done) = if apply {
        crate::utils::init::ensure_dns_config_file()
            .await
            .stringify_err_log(|e| {
                logging!(error, Type::Config, "Failed to create DNS config: {e}");
            })?;
        logging!(info, Type::Config, "Applying DNS config from file");
        ("Failed to apply config with DNS", "DNS config successfully applied")
    } else {
        logging!(info, Type::Config, "DNS settings disabled, regenerating config");
        ("Failed to apply regenerated config", "Config regenerated successfully")
    };

    CoreManager::global()
        .update_config_checked()
        .await
        .stringify_err_log(|err| {
            logging!(error, Type::Config, "{failed}: {err}");
        })?;
    logging!(info, Type::Config, "{done}");

    handle::Handle::refresh_clash();
    Ok(())
}

/// Что показать в редакторе страницы DNS: блок подписки, поверх него —
/// отличия со страницы. `bare` — подписка без страницы (кнопка «как в
/// подписке»). Пока конфиг не собран, подписки нет — видна одна страница.
#[tauri::command]
pub async fn get_dns_page_view(bare: bool) -> CmdResult<Mapping> {
    let (uid, base) = the_dns_page_subject().await;
    let page = if bare {
        dns_page::Page::default()
    } else {
        match uid.and_then(|uid| crate::utils::dirs::dns_page_path(&uid).ok()) {
            Some(path) => match fs::read_to_string(&path).await {
                // Неразбираемый файл не прячем за подпиской: сохранение
                // затёрло бы его молча. Человек видит причину.
                Ok(raw) => dns_page::Page::parse(&raw).ok_or_else(|| {
                    clash_verge_i18n::t!("errors.dnsPageUnparsable", path = format!("{path:?}")).into_owned()
                })?,
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => dns_page::Page::default(),
                Err(err) => {
                    return Err(clash_verge_i18n::t!(
                        "errors.dnsPageUnreadable",
                        path = format!("{path:?}"),
                        reason = err.to_string()
                    )
                    .into_owned()
                    .into());
                }
            },
            None => {
                return Err(clash_verge_i18n::t!("errors.noSubscriptionSelected")
                    .into_owned()
                    .into());
            }
        }
    };
    Ok(base.with(&page).to_file())
}

#[tauri::command]
pub async fn get_clash_logs() -> CmdResult<Vec<CompactString>> {
    let logs = CoreManager::global().get_clash_logs().await.unwrap_or_default();
    Ok(logs)
}

#[cfg(test)]
mod tests {
    use super::{base_of_the_page, dns_reference_check, read_ladder, tunnels_from_subscription};
    use crate::enhance::dns_page::References;
    use serde_yaml_ng::{Mapping, Value};

    #[test]
    fn the_page_is_counted_from_its_own_subscriptions_block() {
        let mut dns = Mapping::new();
        dns.insert("ipv6".into(), Value::Bool(false));
        let runtime = crate::config::runtime::IRuntime {
            profile_uid: Some("A".into()),
            dns_base: Some(crate::enhance::dns_page::Base { dns, hosts: None }),
            ..crate::config::runtime::IRuntime::default()
        };
        assert_eq!(
            base_of_the_page(Some(&"A".into()), &runtime),
            runtime.dns_base.clone().unwrap_or_default()
        );
        assert_eq!(
            base_of_the_page(Some(&"B".into()), &runtime),
            crate::enhance::dns_page::Base::default()
        );
    }

    #[test]
    fn a_lost_rule_set_refuses_the_page_but_an_unknown_tail_only_warns() {
        let refs = References {
            proxies: vec!["GRP-A".to_owned(), "en0".to_owned()],
            rule_sets: vec!["rs-one".to_owned()],
        };
        let all_known = dns_reference_check(&refs, &["GRP-A", "en0", "DIRECT"], &["rs-one"]);
        assert_eq!(all_known, super::DnsReferenceCheck::default());

        let tail_unknown = dns_reference_check(&refs, &["GRP-A", "DIRECT"], &["rs-one"]);
        assert!(
            tail_unknown.refusal.is_none(),
            "хвост без прокси — интерфейс, отказывать нельзя"
        );
        assert!(
            tail_unknown
                .warning
                .as_deref()
                .is_some_and(|text| text.contains("«en0»"))
        );

        let rule_set_unknown = dns_reference_check(&refs, &["GRP-A", "en0"], &[]);
        assert!(
            rule_set_unknown
                .refusal
                .as_deref()
                .is_some_and(|text| text.contains("«rs-one»"))
        );
    }

    #[test]
    fn the_tunnels_page_tells_the_subscriptions_tunnels_from_the_users() {
        let running = serde_yaml_ng::from_str::<Mapping>(r#"{tunnels: ["tcp,127.0.0.1:6553,example.com:53"]}"#)
            .unwrap_or_default();
        assert!(tunnels_from_subscription(&Mapping::new(), Some(&running)));
        let saved = clash_with(&[("tunnels", Value::Sequence(Vec::new()))]);
        assert!(!tunnels_from_subscription(&saved, Some(&running)));
        assert!(!tunnels_from_subscription(&Mapping::new(), Some(&Mapping::new())));
        assert!(!tunnels_from_subscription(&Mapping::new(), None));
    }

    fn clash_with(pairs: &[(&str, Value)]) -> Mapping {
        let mut map = Mapping::new();
        for (key, value) in pairs {
            map.insert(Value::from(*key), value.clone());
        }
        map
    }

    #[test]
    fn a_missing_key_reads_as_unset() {
        let ladder = read_ladder(&Mapping::new());
        assert_eq!(ladder.log_level, None);
        assert_eq!(ladder.unified_delay, None);
    }

    #[test]
    fn pinned_values_are_read_as_they_are() {
        let ladder = read_ladder(&clash_with(&[
            ("log-level", Value::from("warn")),
            ("unified-delay", Value::from(false)),
        ]));
        assert_eq!(ladder.log_level.as_deref(), Some("warn"));
        assert_eq!(ladder.unified_delay, Some(false));
    }

    #[test]
    fn a_value_of_the_wrong_type_is_not_passed_on_as_pinned() {
        let ladder = read_ladder(&clash_with(&[
            ("log-level", Value::from(3)),
            ("unified-delay", Value::from("true")),
        ]));
        assert_eq!(ladder.log_level, None);
        assert_eq!(ladder.unified_delay, None);
    }

    #[test]
    fn a_missing_port_reads_as_taken_from_the_subscription() {
        assert_eq!(read_ladder(&Mapping::new()).mixed_port, None);
    }

    #[test]
    fn a_pinned_port_is_read_whether_it_is_a_number_or_a_string() {
        assert_eq!(
            read_ladder(&clash_with(&[("mixed-port", Value::from(7897))])).mixed_port,
            Some(7897)
        );
        assert_eq!(
            read_ladder(&clash_with(&[("mixed-port", Value::from("7897"))])).mixed_port,
            Some(7897)
        );
        assert_eq!(
            read_ladder(&clash_with(&[("mixed-port", Value::from(1))])).mixed_port,
            Some(1)
        );
        assert_eq!(
            read_ladder(&clash_with(&[("mixed-port", Value::from(65535))])).mixed_port,
            Some(65535)
        );
    }

    #[test]
    fn a_port_outside_the_range_is_shown_as_unset_rather_than_pinned() {
        for bad in [
            Value::from(0),
            Value::from(65536),
            Value::from("0"),
            Value::from("70000"),
        ] {
            assert_eq!(
                read_ladder(&clash_with(&[("mixed-port", bad.clone())])).mixed_port,
                None,
                "{bad:?} is not a usable port"
            );
        }
    }

    #[test]
    fn a_port_that_is_not_a_number_at_all_is_shown_as_unset() {
        for bad in [Value::from(true), Value::from("auto"), Value::Sequence(vec![])] {
            assert_eq!(
                read_ladder(&clash_with(&[("mixed-port", bad.clone())])).mixed_port,
                None,
                "{bad:?} is not a port"
            );
        }
    }

    /// Чтение ядра: тело `/proxies` и провайдеры по порядку; `Err` — не ответил.
    fn core_read(proxies: &str, providers: &[(&str, Result<&str, &str>)]) -> crate::feat::CoreProxies {
        crate::feat::CoreProxies {
            proxies: Ok(proxies.as_bytes().to_vec()),
            providers: providers
                .iter()
                .map(|(name, body)| {
                    (
                        (*name).into(),
                        body.map(|body| body.as_bytes().to_vec())
                            .map_err(|error| anyhow::anyhow!("{error}")),
                    )
                })
                .collect(),
        }
    }

    const PROXIES: &str = r#"{"proxies":{
        "GLOBAL":{"name":"GLOBAL","type":"Selector","all":["Auto"],"now":"Auto","history":[]},
        "Auto":{"name":"Auto","type":"URLTest","all":["node-a"],"now":"node-a","history":[]},
        "node-a":{"name":"node-a","type":"FutureProto","dialer-proxy":"hop","provider-name":"sub",
            "routing-mark":3,"fingerprint":"ab12","history":[{"time":"t","delay":42}],"udp":true}}}"#;
    const PROVIDER: &str = r#"{"name":"sub","type":"Proxy","vehicleType":"HTTP",
        "proxies":[{"name":"node-b","type":"Vless","history":[]}],
        "testUrl":"https://example.com/generate_204","expectedStatus":"*"}"#;

    fn order() -> Vec<smartstring::alias::String> {
        vec!["Auto".into()]
    }

    fn stamp_of(read: crate::feat::CoreProxies, order: &[smartstring::alias::String], labels: &str) -> String {
        let no_labels = crate::config::proxy_label::Labels::default();
        super::answer(read, order, (labels, &no_labels), None, None)
            .map(|answer| answer.stamp)
            .unwrap_or_default()
    }

    #[test]
    fn a_snapshot_is_sent_only_when_the_stamp_differs() {
        let no_labels = crate::config::proxy_label::Labels::default();
        let first = super::answer(
            core_read(PROXIES, &[("sub", Ok(PROVIDER))]),
            &order(),
            ("l1", &no_labels),
            None,
            None,
        );
        let first = first.ok();
        let stamp = first.as_ref().map(|answer| answer.stamp.clone()).unwrap_or_default();
        assert!(first.is_some_and(|answer| answer.snapshot.is_some()));
        assert!(!stamp.is_empty());

        let same = super::answer(
            core_read(PROXIES, &[("sub", Ok(PROVIDER))]),
            &order(),
            ("l1", &no_labels),
            Some(&stamp),
            Some("l1"),
        )
        .ok();
        assert!(same.as_ref().is_some_and(|answer| answer.snapshot.is_none()));
        assert_eq!(same.map(|answer| answer.stamp).unwrap_or_default(), stamp);

        let other = super::answer(
            core_read(PROXIES, &[("sub", Ok(PROVIDER))]),
            &order(),
            ("l1", &no_labels),
            Some("0000000000000000"),
            Some("l1"),
        );
        assert!(other.is_ok_and(|answer| answer.snapshot.is_some()));
    }

    #[test]
    fn the_snapshot_is_built_from_the_bytes_that_were_hashed() {
        let no_labels = crate::config::proxy_label::Labels::default();
        let before = stamp_of(core_read(PROXIES, &[]), &order(), "l1");
        let moved = PROXIES.replace(r#""now":"node-a""#, r#""now":"node-z""#);
        let after = super::answer(
            core_read(&moved, &[]),
            &order(),
            ("l1", &no_labels),
            Some(&before),
            None,
        )
        .ok();
        let after_stamp = after.as_ref().map(|answer| answer.stamp.clone()).unwrap_or_default();
        assert_ne!(after_stamp, before);
        assert_eq!(after_stamp, stamp_of(core_read(&moved, &[]), &order(), "l1"));
        let now = after
            .and_then(|answer| answer.snapshot)
            .and_then(|snapshot| snapshot.proxies.proxies.get("Auto").and_then(|auto| auto.now.clone()));
        assert_eq!(now.as_deref(), Some("node-z"));
    }

    #[test]
    fn a_provider_that_did_not_answer_fails_the_whole_read() {
        let no_labels = crate::config::proxy_label::Labels::default();
        let read = core_read(PROXIES, &[("sub", Ok(PROVIDER)), ("other", Err("timed out"))]);
        let answer = super::answer(read, &order(), ("l1", &no_labels), None, None);
        assert!(answer.is_err_and(|error| format!("{error:#}").contains("other")));

        let mut read = core_read(PROXIES, &[]);
        read.proxies = Err(anyhow::anyhow!("refused"));
        assert!(super::answer(read, &order(), ("l1", &no_labels), None, None).is_err());
    }

    #[test]
    fn labels_travel_only_when_their_stamp_differs() {
        let no_labels = crate::config::proxy_label::Labels::default();
        let labels_of = |known_labels: Option<&str>| {
            super::answer(
                core_read(PROXIES, &[]),
                &order(),
                ("l1", &no_labels),
                None,
                known_labels,
            )
            .ok()
            .and_then(|answer| answer.snapshot)
            .map(|snapshot| (snapshot.labels.stamp, snapshot.labels.labels.is_some()))
        };
        assert_eq!(labels_of(Some("l1")), Some(("l1".to_owned(), false)));
        assert_eq!(labels_of(Some("l0")), Some(("l1".to_owned(), true)));
        assert_eq!(labels_of(None), Some(("l1".to_owned(), true)));
    }

    #[test]
    fn the_snapshot_keeps_the_plugin_shape() {
        let no_labels = crate::config::proxy_label::Labels::default();
        let answer = super::answer(
            core_read(PROXIES, &[("sub", Ok(PROVIDER))]),
            &order(),
            ("l1", &no_labels),
            None,
            None,
        )
        .ok()
        .and_then(|answer| serde_json::to_value(answer).ok())
        .unwrap_or_default();
        let node = &answer["snapshot"]["proxies"]["proxies"]["node-a"];
        assert_eq!(node["dialerProxy"], "hop");
        assert_eq!(node["providerName"], "sub");
        assert_eq!(node["routingMark"], 3);
        assert_eq!(node["type"], "FutureProto");
        assert_eq!(node["fingerprint"], "ab12");
        assert_eq!(node["history"][0]["delay"], 42);
        assert_eq!(node["tfo"], false, "отсутствующее поле — по умолчанию, как у плагина");
        assert!(node.get("dialer-proxy").is_none());

        let provider = &answer["snapshot"]["providers"][0];
        assert_eq!(provider[0], "sub");
        assert_eq!(provider[1]["vehicleType"], "HTTP");
        assert_eq!(provider[1]["proxies"][0]["type"], "Vless");
        assert_eq!(answer["snapshot"]["groupOrder"][0], "Auto");
        assert_eq!(answer["snapshot"]["labels"]["stamp"], "l1");
        assert!(answer["stamp"].is_string());
    }

    #[test]
    fn the_stamp_moves_with_order_labels_and_providers() {
        let base = || core_read(PROXIES, &[("sub", Ok(PROVIDER))]);
        let stamp = stamp_of(base(), &order(), "l1");
        assert!(!stamp.is_empty());
        assert_eq!(stamp_of(base(), &order(), "l1"), stamp);
        assert_ne!(stamp_of(base(), &["Other".into(), "Auto".into()], "l1"), stamp);
        assert_ne!(stamp_of(base(), &order(), "l2"), stamp);
        let provider = PROVIDER.replace("node-b", "node-c");
        assert_ne!(
            stamp_of(core_read(PROXIES, &[("sub", Ok(&provider))]), &order(), "l1"),
            stamp
        );
    }

    #[test]
    fn the_first_read_waits_longer() {
        use super::proxies_snapshot_budget as budget;
        // Окну нечего показывать — ждём ядро дольше, чем при показанном.
        assert!(budget(None) > budget(Some("0123abcd")));
        // Решает только то, есть ли показанное, а не какой у него отпечаток.
        assert_eq!(budget(Some("")), budget(Some("0123abcd")));
        // Команда берёт предел по тому, что окно уже показывает.
        // Тело команды стоит в файле ниже тестов: ищем по всему файлу, а
        // подпись собираем по частям, чтобы не найти этот же тест.
        let signature = concat!("pub async fn ", "get_proxies_snapshot(");
        let snapshot = crate::utils::source_scan::fn_body(include_str!("clash.rs"), signature).unwrap_or_default();
        assert!(
            snapshot.contains("read_core_proxies(proxies_snapshot_budget(known.as_deref()))"),
            "{snapshot}"
        );
    }
}

/// clod:Э13-04 — обновление провайдера, набора правил или гео-баз с пределом,
/// которого хватает на загрузку в ядре.
#[tauri::command]
pub async fn download_in_core(what: feat::CoreDownload, name: Option<String>) -> CmdResult {
    feat::download_in_core(what, name.as_deref().unwrap_or_default())
        .await
        .stringify_err()
}

/// Сколько ждём ядро на один запрос: опрос идёт раз в секунду, и зависший
/// запрос не должен копить следующие. Первое чтение — у окна нет показанного
/// (`known`), — ждёт дольше.
const fn proxies_snapshot_budget(known: Option<&str>) -> std::time::Duration {
    std::time::Duration::from_secs(if known.is_some() { 2 } else { 5 })
}

/// Ответ окну: отпечаток и — только если он не тот, что у окна, — сам снимок,
/// разобранный из тех же байтов, по которым посчитан отпечаток.
#[derive(serde::Serialize)]
pub struct ProxiesAnswer {
    stamp: std::string::String,
    #[serde(skip_serializing_if = "Option::is_none")]
    snapshot: Option<ProxiesSnapshot>,
}

/// Группы и узлы в той же форме, что отдаёт плагин ядра.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ProxiesSnapshot {
    proxies: tauri_plugin_mihomo::models::Proxies,
    /// В порядке `proxy-providers` принятой сборки.
    providers: Vec<(String, tauri_plugin_mihomo::models::ProxyProvider)>,
    group_order: Vec<String>,
    labels: super::runtime::ProxyLabelsAnswer,
}

/// Отпечаток всего, что ядро меняет само (выбор url-test и fallback, задержки
/// после проверок, состав узлов после обновления провайдера), и снимок, если
/// окно показывает другое (`known`).
///
/// Ядро об этих переменах не сообщает, а целиком разбирать его ответ раз в
/// секунду дорого. Поэтому ответ разбирается, только когда отпечаток сменился.
/// В отпечатке всё, из чего окно собирает показ: `/proxies`, ответы настоящих
/// провайдеров, порядок групп принятой сборки и подписи узлов. Подписи едут,
/// только если их отпечаток не тот, что у окна (`known_labels`).
fn answer(
    read: feat::CoreProxies,
    group_order: &[String],
    (labels_stamp, labels): (&str, &crate::config::proxy_label::Labels),
    known: Option<&str>,
    known_labels: Option<&str>,
) -> anyhow::Result<ProxiesAnswer> {
    use anyhow::Context as _;
    use std::hash::Hasher as _;

    let proxies = read.proxies?;
    // Сбой одного провайдера — сбой всего чтения: молча выпавший провайдер
    // показал бы свои узлы без типа и пингов.
    let providers = read
        .providers
        .into_iter()
        .map(|(name, body)| {
            body.with_context(|| format!("provider {name}"))
                .map(|body| (name, body))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

    let mut hasher = std::hash::DefaultHasher::new();
    hasher.write(&proxies);
    for group in group_order {
        hasher.write(group.as_bytes());
        hasher.write_u8(0);
    }
    // Подписи узлов берутся из подписки, а не из ядра: обновлённая подписка
    // может не изменить ответ ядра, а подписи — изменить.
    hasher.write(labels_stamp.as_bytes());
    for (_, body) in &providers {
        hasher.write(body);
    }
    let stamp = format!("{:016x}", hasher.finish());
    if known == Some(stamp.as_str()) {
        return Ok(ProxiesAnswer { stamp, snapshot: None });
    }

    let snapshot = ProxiesSnapshot {
        proxies: serde_json::from_slice(&proxies)?,
        providers: providers
            .iter()
            .map(|(name, body)| anyhow::Ok((name.clone(), serde_json::from_slice(body)?)))
            .collect::<anyhow::Result<_>>()?,
        group_order: group_order.to_vec(),
        labels: super::runtime::ProxyLabelsAnswer {
            stamp: labels_stamp.into(),
            labels: (known_labels != Some(labels_stamp)).then(|| labels.clone()),
        },
    };
    Ok(ProxiesAnswer {
        stamp,
        snapshot: Some(snapshot),
    })
}

/// Группы и узлы для окна — одним чтением ядра и для отпечатка, и для показа.
#[tauri::command]
pub async fn get_proxies_snapshot(
    known: Option<std::string::String>,
    known_labels: Option<std::string::String>,
) -> CmdResult<ProxiesAnswer> {
    let (read, group_order, (labels_stamp, labels)) = tokio::join!(
        feat::read_core_proxies(proxies_snapshot_budget(known.as_deref())),
        super::runtime::runtime_proxy_group_order(),
        crate::config::proxy_label::current(),
    );
    answer(
        read,
        &group_order,
        (&labels_stamp, &labels),
        known.as_deref(),
        known_labels.as_deref(),
    )
    .stringify_err()
}
