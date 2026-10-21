use crate::{
    config::{Config, IProfiles, PrfItem, PrfProxyChain, runtime::IRuntime},
    core::{CoreManager, handle, manager::Delivery},
    enhance::{Sources, proxy_chain},
};
use anyhow::{Context as _, Result, bail};
use clash_verge_logging::{Type, logging};
use serde::{Deserialize, Serialize};
use smartstring::alias::String;

#[derive(Debug, Serialize)]
pub struct ProxyChainView {
    group: String,
    nodes: Vec<String>,
    applied: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AdoptedChain {
    uid: String,
    group: String,
    nodes: Vec<String>,
}

pub async fn proxy_chain_view() -> Option<ProxyChainView> {
    let profiles = Config::profiles().await.data_arc();
    let current = profiles.get_current()?;
    let chain = *profiles.get_item(current).ok()?.proxy_chain.clone()?;
    let runtime = Config::runtime().await.data_arc();
    let applied = runtime.profile_uid.as_ref() == Some(current) && runtime.proxy_chain.as_ref() == Some(&chain.nodes);
    Some(ProxyChainView {
        group: chain.group,
        nodes: chain.nodes,
        applied,
    })
}

pub async fn connect_proxy_chain(group: String, nodes: Vec<String>) -> Result<()> {
    super::refuse_while_exiting()?;
    let runtime = Config::runtime().await.data_arc();
    let Some(config) = runtime.config.as_ref() else {
        bail!("{}", clash_verge_i18n::t!("errors.chainNodesMissing"));
    };
    let own_entry = nodes.first().is_some_and(|entry| proxy_chain::is_own(config, entry));
    if !proxy_chain::fits(config, &nodes, own_entry) {
        bail!("{}", clash_verge_i18n::t!("errors.chainNodesMissing"));
    }
    drop(runtime);
    rebuild_with(move |registry| connect_in(registry, &group, &nodes, own_entry)).await
}

pub async fn disconnect_proxy_chain() -> Result<()> {
    let connected = {
        let profiles = Config::profiles().await.data_arc();
        profiles
            .get_current()
            .and_then(|current| profiles.get_item(current).ok())
            .is_some_and(|item| item.proxy_chain.is_some())
    };
    if !connected {
        return Ok(());
    }
    super::refuse_while_exiting()?;
    rebuild_with(disconnect_in).await
}

pub async fn adopt_proxy_chains(chains: Vec<AdoptedChain>) -> Result<()> {
    super::refuse_while_exiting()?;
    let own_entries: Vec<bool> = {
        let current = Config::profiles().await.data_arc().get_current().cloned();
        let runtime = Config::runtime().await.data_arc();
        chains
            .iter()
            .map(|chain| {
                current.as_ref() == Some(&chain.uid)
                    && runtime.config.as_ref().is_some_and(|config| {
                        chain
                            .nodes
                            .first()
                            .is_some_and(|entry| proxy_chain::is_own(config, entry))
                    })
            })
            .collect()
    };
    rebuild_with(move |registry| {
        for (chain, own_entry) in chains.iter().zip(&own_entries) {
            adopt_in(registry, chain, *own_entry);
        }
        Ok(())
    })
    .await
}

async fn rebuild_with(change: impl Fn(&mut IProfiles) -> Result<()> + Clone + Send + Sync + 'static) -> Result<()> {
    let derive = change.clone();
    let sources = Sources::default().with_profiles_derived(move |accepted| {
        let mut candidate = accepted.clone();
        derive(&mut candidate)?;
        Ok(candidate)
    });
    let staged = match CoreManager::global().stage_with(sources).await? {
        Ok(staged) => staged,
        Err(outcome) => bail!("{outcome}"),
    };
    let profiles = Config::profiles().await;
    let commit = async || {
        profiles
            .with_data_modify(|mut committed| async move {
                change(&mut committed)?;
                Ok((committed, ()))
            })
            .await
    };
    match staged.deliver_committing(Delivery::Reload, commit).await? {
        Ok(_) => {
            crate::config::profiles_save_file_safe().await?;
            handle::Handle::refresh_profiles();
            handle::Handle::refresh_clash();
            // Перезапущенное ядро возвращает выбор по реестру ещё до этой записи.
            crate::config::profiles::activate_selected_nodes();
            Ok(())
        }
        Err(outcome) => bail!("{outcome}"),
    }
}

fn chosen_in(item: &PrfItem, group: &str) -> Option<String> {
    item.selected
        .as_ref()?
        .iter()
        .find(|entry| entry.name.as_deref() == Some(group))?
        .now
        .clone()
}

fn choice_after_the_chain(chain: &PrfProxyChain, chosen: Option<&str>) -> Option<String> {
    let exit = chain.nodes.last()?;
    if chosen.is_some_and(|chosen| chosen != exit) {
        return None;
    }
    Some(chain.previous.clone().unwrap_or_else(|| exit.clone()))
}

fn current_item(registry: &mut IProfiles) -> Result<&mut PrfItem> {
    let current = registry
        .get_current()
        .cloned()
        .context(clash_verge_i18n::t!("errors.noCurrentSubscription"))?;
    registry.item_mut(&current)
}

fn release(item: &mut PrfItem, chain: &PrfProxyChain) {
    let chosen = chosen_in(item, &chain.group);
    if let Some(back) = choice_after_the_chain(chain, chosen.as_deref()) {
        item.choose_node(&chain.group, &back);
    }
}

fn connect_in(registry: &mut IProfiles, group: &str, nodes: &[String], own_entry: bool) -> Result<()> {
    let exit = nodes.last().context(clash_verge_i18n::t!("errors.emptyChain"))?.clone();
    let item = current_item(registry)?;
    let chosen = chosen_in(item, group);
    let old = item.proxy_chain.take();
    let previous = match &old {
        Some(old) if old.group == group => old.previous.clone(),
        _ => chosen.filter(|chosen| chosen != "DIRECT"),
    };
    item.proxy_chain = Some(Box::new(PrfProxyChain {
        group: group.into(),
        nodes: nodes.to_vec(),
        previous,
        own_entry,
    }));
    if let Some(old) = old.filter(|old| old.group != group) {
        release(item, &old);
    }
    item.choose_node(group, &exit);
    Ok(())
}

fn disconnect_in(registry: &mut IProfiles) -> Result<()> {
    let item = current_item(registry)?;
    if let Some(chain) = item.proxy_chain.take() {
        release(item, &chain);
    }
    Ok(())
}

fn drop_in(registry: &mut IProfiles, uid: &str, nodes: &[String]) -> bool {
    let Ok(item) = registry.item_mut(uid) else {
        return false;
    };
    let Some(chain) = item.proxy_chain.take_if(|chain| chain.nodes == nodes) else {
        return false;
    };
    release(item, &chain);
    true
}

pub async fn forget_a_dropped_chain(build: &IRuntime) {
    let (Some(uid), Some(dropped)) = (build.profile_uid.clone(), build.proxy_chain_dropped.clone()) else {
        return;
    };
    let nodes = dropped.nodes.clone();
    let released = Config::profiles()
        .await
        .with_data_modify(|mut profiles| async move {
            let released = drop_in(&mut profiles, &uid, &nodes);
            if released {
                profiles.save_file().await?;
            }
            Ok((profiles, released))
        })
        .await;
    match released {
        Ok(true) => {
            handle::Handle::refresh_profiles();
            handle::Handle::notice_message("clod_config::proxy_chain_dropped", dropped.missing);
            if !matches!(
                *CoreManager::global().get_running_mode(),
                crate::core::manager::RunningMode::NotRunning
            ) {
                crate::config::profiles::activate_selected_nodes();
            }
        }
        Ok(false) => {}
        Err(err) => logging!(warn, Type::Config, "не удалось снять цепочку прокси: {err:#}"),
    }
}

fn adopt_in(registry: &mut IProfiles, chain: &AdoptedChain, own_entry: bool) {
    let Ok(item) = registry.item_mut(&chain.uid) else {
        return;
    };
    if item.proxy_chain.is_none() && chain.nodes.len() >= 2 {
        item.proxy_chain = Some(Box::new(PrfProxyChain {
            group: chain.group.clone(),
            nodes: chain.nodes.clone(),
            previous: None,
            own_entry,
        }));
    }
}

#[allow(clippy::expect_used)]
#[cfg(test)]
mod tests {
    use super::{AdoptedChain, adopt_in, chosen_in, connect_in, disconnect_in, drop_in};
    use crate::config::{IProfiles, PrfItem, PrfProxyChain, PrfSelected};
    use smartstring::alias::String;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|name| (*name).into()).collect()
    }

    fn registry(selected: &[(&str, &str)]) -> IProfiles {
        IProfiles {
            current: Some("A".into()),
            items: Some(vec![
                PrfItem {
                    uid: Some("A".into()),
                    selected: Some(
                        selected
                            .iter()
                            .map(|(group, now)| PrfSelected {
                                name: Some((*group).into()),
                                now: Some((*now).into()),
                            })
                            .collect(),
                    ),
                    ..PrfItem::default()
                },
                PrfItem {
                    uid: Some("B".into()),
                    ..PrfItem::default()
                },
            ]),
        }
    }

    fn item<'a>(registry: &'a IProfiles, uid: &str) -> &'a PrfItem {
        registry.get_item(uid).expect("the item is in the registry")
    }

    fn chosen(registry: &IProfiles, group: &str) -> Option<String> {
        chosen_in(item(registry, "A"), group)
    }

    #[test]
    fn the_new_choice_reaches_the_core_whatever_happened_to_it() {
        let body = crate::utils::source_scan::fn_body(include_str!("proxy_chain.rs"), "async fn rebuild_with(")
            .expect("rebuild_with() is in proxy_chain.rs");
        assert!(body.contains("activate_selected_nodes()"), "{body}");
        assert!(!body.contains("settle_after_delivery"), "{body}");
    }

    #[test]
    fn a_chain_that_no_longer_fits_gives_the_group_back_its_choice() {
        let mut registry = registry(&[("Proxy", "Berlin")]);
        connect_in(&mut registry, "Proxy", &names(&["a", "b"]), false).expect("connects");
        assert!(!drop_in(&mut registry, "A", &names(&["a", "x"])), "другая цепочка");
        assert!(drop_in(&mut registry, "A", &names(&["a", "b"])));
        assert!(item(&registry, "A").proxy_chain.is_none());
        assert_eq!(chosen(&registry, "Proxy").as_deref(), Some("Berlin"));
        assert!(!drop_in(&mut registry, "A", &names(&["a", "b"])), "уже снята");
    }

    #[test]
    fn every_accepted_build_releases_a_chain_that_did_not_fit() {
        let config = include_str!("../core/manager/config.rs");
        for signature in ["async fn note_the_accepted(", "async fn replace_core_and_apply("] {
            let body = crate::utils::source_scan::fn_body(config, signature).expect("the function is in config.rs");
            assert!(body.contains("forget_a_dropped_chain("), "{signature}");
        }
    }

    #[test]
    fn disconnecting_gives_the_group_back_the_choice_it_had_before_the_chain() {
        let mut registry = registry(&[("Proxy", "Berlin")]);
        connect_in(&mut registry, "Proxy", &names(&["a", "b"]), false).expect("connects");
        assert_eq!(chosen(&registry, "Proxy").as_deref(), Some("b"));
        assert_eq!(
            item(&registry, "A")
                .proxy_chain
                .as_ref()
                .and_then(|chain| chain.previous.as_deref()),
            Some("Berlin")
        );
        disconnect_in(&mut registry).expect("disconnects");
        assert_eq!(chosen(&registry, "Proxy").as_deref(), Some("Berlin"));
        assert!(item(&registry, "A").proxy_chain.is_none());
    }

    #[test]
    fn disconnecting_never_turns_the_group_to_direct() {
        let mut registry = registry(&[("Proxy", "DIRECT")]);
        connect_in(&mut registry, "Proxy", &names(&["a", "b"]), false).expect("connects");
        disconnect_in(&mut registry).expect("disconnects");
        assert_eq!(chosen(&registry, "Proxy").as_deref(), Some("b"));
    }

    #[test]
    fn a_chain_built_by_an_earlier_version_leaves_the_group_on_its_exit() {
        let mut registry = registry(&[("Proxy", "b")]);
        adopt_in(
            &mut registry,
            &AdoptedChain {
                uid: "A".into(),
                group: "Proxy".into(),
                nodes: names(&["a", "b"]),
            },
            false,
        );
        disconnect_in(&mut registry).expect("disconnects");
        assert_eq!(chosen(&registry, "Proxy").as_deref(), Some("b"));
    }

    #[test]
    fn reconnecting_keeps_the_choice_from_before_the_first_chain() {
        let mut registry = registry(&[("Proxy", "Berlin")]);
        connect_in(&mut registry, "Proxy", &names(&["a", "b"]), false).expect("connects");
        connect_in(&mut registry, "Proxy", &names(&["a", "c"]), false).expect("reconnects");
        disconnect_in(&mut registry).expect("disconnects");
        assert_eq!(chosen(&registry, "Proxy").as_deref(), Some("Berlin"));
    }

    #[test]
    fn a_group_moved_away_from_the_chain_by_hand_is_left_alone() {
        let mut registry = registry(&[("Proxy", "Berlin")]);
        connect_in(&mut registry, "Proxy", &names(&["a", "b"]), false).expect("connects");
        registry.set_selected_node("Proxy", "Paris");
        disconnect_in(&mut registry).expect("disconnects");
        assert_eq!(chosen(&registry, "Proxy").as_deref(), Some("Paris"));
    }

    #[test]
    fn a_chain_moved_to_another_group_releases_the_first_one() {
        let mut registry = registry(&[("Proxy", "Berlin"), ("GLOBAL", "Auto")]);
        connect_in(&mut registry, "Proxy", &names(&["a", "b"]), false).expect("connects");
        connect_in(&mut registry, "GLOBAL", &names(&["a", "c"]), false).expect("moves");
        assert_eq!(chosen(&registry, "Proxy").as_deref(), Some("Berlin"));
        assert_eq!(chosen(&registry, "GLOBAL").as_deref(), Some("c"));
        assert_eq!(
            item(&registry, "A")
                .proxy_chain
                .as_ref()
                .and_then(|chain| chain.previous.as_deref()),
            Some("Auto")
        );
    }

    #[test]
    fn adopting_does_not_override_a_chain_the_subscription_already_keeps() {
        let mut registry = registry(&[]);
        connect_in(&mut registry, "Proxy", &names(&["a", "b"]), false).expect("connects");
        for chain in [
            AdoptedChain {
                uid: "A".into(),
                group: "Proxy".into(),
                nodes: names(&["x", "y"]),
            },
            AdoptedChain {
                uid: "B".into(),
                group: "GLOBAL".into(),
                nodes: names(&["x", "y"]),
            },
            AdoptedChain {
                uid: "gone".into(),
                group: "GLOBAL".into(),
                nodes: names(&["x", "y"]),
            },
        ] {
            adopt_in(&mut registry, &chain, false);
        }
        assert_eq!(
            item(&registry, "A")
                .proxy_chain
                .as_ref()
                .map(|chain| chain.nodes.clone()),
            Some(names(&["a", "b"]))
        );
        assert_eq!(
            item(&registry, "B").proxy_chain,
            Some(Box::new(PrfProxyChain {
                group: "GLOBAL".into(),
                nodes: names(&["x", "y"]),
                previous: None,
                own_entry: false,
            }))
        );
    }
}
