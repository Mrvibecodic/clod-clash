use crate::{
    config::{Config, IProfiles, PrfItem, PrfProxyChain},
    core::{CoreManager, handle, manager::Delivery},
    enhance::{Sources, proxy_chain},
};
use anyhow::{Context as _, Result, bail};
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
    let fits = Config::runtime()
        .await
        .data_arc()
        .config
        .as_ref()
        .is_some_and(|config| proxy_chain::fits(config, &nodes));
    if !fits {
        bail!("узлов цепочки нет среди узлов подписки");
    }
    rebuild_with(move |registry| connect_in(registry, &group, &nodes)).await
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
    rebuild_with(move |registry| {
        for chain in &chains {
            adopt_in(registry, chain);
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
    let current = registry.get_current().cloned().context("нет текущей подписки")?;
    registry.item_mut(&current)
}

fn release(registry: &mut IProfiles, chain: &PrfProxyChain) -> Result<()> {
    let chosen = chosen_in(current_item(registry)?, &chain.group);
    if let Some(back) = choice_after_the_chain(chain, chosen.as_deref()) {
        registry.set_selected_node(&chain.group, &back);
    }
    Ok(())
}

fn connect_in(registry: &mut IProfiles, group: &str, nodes: &[String]) -> Result<()> {
    let exit = nodes.last().context("пустая цепочка")?.clone();
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
    }));
    if let Some(old) = old.filter(|old| old.group != group) {
        release(registry, &old)?;
    }
    registry.set_selected_node(group, &exit);
    Ok(())
}

fn disconnect_in(registry: &mut IProfiles) -> Result<()> {
    let Some(chain) = current_item(registry)?.proxy_chain.take() else {
        return Ok(());
    };
    release(registry, &chain)
}

fn adopt_in(registry: &mut IProfiles, chain: &AdoptedChain) {
    let Ok(item) = registry.item_mut(&chain.uid) else {
        return;
    };
    if item.proxy_chain.is_none() && chain.nodes.len() >= 2 {
        item.proxy_chain = Some(Box::new(PrfProxyChain {
            group: chain.group.clone(),
            nodes: chain.nodes.clone(),
            previous: None,
        }));
    }
}

#[allow(clippy::expect_used)]
#[cfg(test)]
mod tests {
    use super::{AdoptedChain, adopt_in, chosen_in, connect_in, disconnect_in};
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
    fn disconnecting_gives_the_group_back_the_choice_it_had_before_the_chain() {
        let mut registry = registry(&[("Proxy", "Berlin")]);
        connect_in(&mut registry, "Proxy", &names(&["a", "b"])).expect("connects");
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
        connect_in(&mut registry, "Proxy", &names(&["a", "b"])).expect("connects");
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
        );
        disconnect_in(&mut registry).expect("disconnects");
        assert_eq!(chosen(&registry, "Proxy").as_deref(), Some("b"));
    }

    #[test]
    fn reconnecting_keeps_the_choice_from_before_the_first_chain() {
        let mut registry = registry(&[("Proxy", "Berlin")]);
        connect_in(&mut registry, "Proxy", &names(&["a", "b"])).expect("connects");
        connect_in(&mut registry, "Proxy", &names(&["a", "c"])).expect("reconnects");
        disconnect_in(&mut registry).expect("disconnects");
        assert_eq!(chosen(&registry, "Proxy").as_deref(), Some("Berlin"));
    }

    #[test]
    fn a_group_moved_away_from_the_chain_by_hand_is_left_alone() {
        let mut registry = registry(&[("Proxy", "Berlin")]);
        connect_in(&mut registry, "Proxy", &names(&["a", "b"])).expect("connects");
        registry.set_selected_node("Proxy", "Paris");
        disconnect_in(&mut registry).expect("disconnects");
        assert_eq!(chosen(&registry, "Proxy").as_deref(), Some("Paris"));
    }

    #[test]
    fn a_chain_moved_to_another_group_releases_the_first_one() {
        let mut registry = registry(&[("Proxy", "Berlin"), ("GLOBAL", "Auto")]);
        connect_in(&mut registry, "Proxy", &names(&["a", "b"])).expect("connects");
        connect_in(&mut registry, "GLOBAL", &names(&["a", "c"])).expect("moves");
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
        connect_in(&mut registry, "Proxy", &names(&["a", "b"])).expect("connects");
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
            adopt_in(&mut registry, &chain);
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
            }))
        );
    }
}
