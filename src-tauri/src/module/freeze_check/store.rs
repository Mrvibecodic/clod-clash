//! Хранилище итогов проверки 16–20: файл на подписку, внутри — по сетям.
//!
//! clod:freeze — `freeze-<uid>.json` лежит рядом с файлами подписки и уходит
//! вместе с ней. Сеть, где клиент не был 30 дней, забывается; отпечатки,
//! которых в подписке больше нет, — тоже.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context as _, Result};
use clash_verge_logging::{Type, logging};
use serde::{Deserialize, Serialize};

use super::plan::Node;
use crate::utils::dirs;

/// Сеть, где клиент не был столько, забывается.
const FORGET_NETWORK_AFTER: i64 = 30 * 24 * 60 * 60;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Network {
    /// Когда клиент был в этой сети в последний раз (unix-секунды).
    #[serde(default)]
    pub last_seen: i64,
    /// Отпечаток узла → что о нём известно здесь.
    #[serde(default)]
    pub nodes: BTreeMap<std::string::String, Node>,
}

impl Network {
    pub(super) const fn new() -> Self {
        Self {
            last_seen: 0,
            nodes: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Store {
    /// Хеш ключа сети → итоги в ней.
    #[serde(default)]
    pub networks: BTreeMap<std::string::String, Network>,
}

impl Store {
    /// Выбросить забытые сети и отпечатки, которых в подписке больше нет.
    pub(super) fn prune(&mut self, now: i64, live: &BTreeSet<std::string::String>) {
        self.networks
            .retain(|_, network| now.saturating_sub(network.last_seen) < FORGET_NETWORK_AFTER);
        for network in self.networks.values_mut() {
            network.nodes.retain(|fingerprint, _| live.contains(fingerprint));
        }
    }

    pub(super) fn network_mut(&mut self, key: &str) -> &mut Network {
        self.networks.entry(key.to_owned()).or_default()
    }
}

fn file_path(uid: &str) -> Result<std::path::PathBuf> {
    Ok(dirs::app_profiles_dir()?.join(dirs::freeze_file(uid)))
}

/// Прочитать хранилище подписки; нет файла или он испорчен — пустое.
pub(super) async fn load(uid: &str) -> Store {
    let Ok(path) = file_path(uid) else {
        return Store::default();
    };
    let Ok(bytes) = tokio::fs::read(&path).await else {
        return Store::default();
    };
    match serde_json::from_slice::<Store>(&bytes) {
        Ok(store) => store,
        Err(err) => {
            logging!(
                warn,
                Type::Core,
                "[Freeze] the saved results could not be read and start over: {err}"
            );
            Store::default()
        }
    }
}

pub(super) async fn save(uid: &str, store: &Store) -> Result<()> {
    let path = file_path(uid)?;
    let bytes = serde_json::to_vec(store).context("serialize")?;
    tokio::fs::write(&path, bytes)
        .await
        .with_context(|| format!("write {}", path.display()))
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use super::{FORGET_NETWORK_AFTER, Network, Store};
    use crate::module::freeze_check::plan::{Node, Verdict};

    const NOW: i64 = 1_800_000_000;

    fn node(verdict: Verdict) -> Node {
        Node {
            verdict: Some(verdict),
            at: NOW,
            tried: NOW,
            ..Node::default()
        }
    }

    #[test]
    fn old_networks_and_gone_fingerprints_are_forgotten() {
        let mut store = Store::default();
        let mut here = Network {
            last_seen: NOW,
            nodes: BTreeMap::new(),
        };
        here.nodes.insert("live".into(), node(Verdict::Frozen));
        here.nodes.insert("gone".into(), node(Verdict::Dead));
        store.networks.insert("here".into(), here);
        store.networks.insert(
            "old".into(),
            Network {
                last_seen: NOW - FORGET_NETWORK_AFTER,
                nodes: BTreeMap::new(),
            },
        );
        store.networks.insert(
            "recent".into(),
            Network {
                last_seen: NOW - FORGET_NETWORK_AFTER + 1,
                nodes: BTreeMap::new(),
            },
        );

        let live: BTreeSet<std::string::String> = BTreeSet::from(["live".to_owned()]);
        store.prune(NOW, &live);

        assert!(!store.networks.contains_key("old"));
        assert!(store.networks.contains_key("recent"));
        let here = store.networks.get("here").cloned().unwrap_or_default();
        assert_eq!(here.nodes.keys().collect::<Vec<_>>(), vec!["live"]);
    }

    #[test]
    fn the_file_round_trips_and_an_unknown_field_is_tolerated() {
        let mut store = Store::default();
        store.network_mut("k").last_seen = NOW;
        store.network_mut("k").nodes.insert("fp".into(), node(Verdict::Dead));
        let text = serde_json::to_string(&store).unwrap_or_default();
        assert!(text.contains("\"dead\""));
        let back: Store = serde_json::from_str(&text).unwrap_or_default();
        assert_eq!(back, store);

        let tolerant: Store =
            serde_json::from_str(r#"{"networks":{"k":{"last_seen":1,"nodes":{"fp":{"tried":5}}}},"extra":1}"#)
                .unwrap_or_default();
        let node = tolerant.networks.get("k").and_then(|n| n.nodes.get("fp")).cloned();
        assert_eq!(node.map(|n| (n.verdict, n.tried)), Some((None, 5)));
    }
}
