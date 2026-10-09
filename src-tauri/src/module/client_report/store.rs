//! Накопленное для отчёта прослойке: файл на подписку, внутри — по сетям и
//! часам. Чистые решения (куда лечь замеру, что уходит в отчёт, что убрать
//! после) живут здесь же и проверяются тестами без ядра и сети.

use std::collections::BTreeMap;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::utils::{dirs, help};

/// Старше этого накопленное не хранится и не отправляется.
pub(super) const KEEP: i64 = 7 * 24 * 60 * 60;
pub(super) const HOUR: i64 = 60 * 60;
/// Границы корзин задержки, мс: <100, <200, <400, <800, <1500, остальное.
/// 200 и 400 — те же границы, что у цвета пинга в клиенте.
const BUCKET_EDGES: [u64; 5] = [100, 200, 400, 800, 1500];
pub(super) const BUCKETS: usize = BUCKET_EDGES.len() + 1;

/// Узел, как его узнаёт прослойка: тип, адрес и порт из файла подписки.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct NodeInfo {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub server: String,
    pub port: u16,
}

impl NodeInfo {
    /// Узел из общего разбора сборки.
    pub(super) fn of(name: &str, address: &crate::config::proxy_label::Address) -> Self {
        Self {
            name: name.to_owned(),
            kind: address.kind.clone(),
            server: address.server.clone(),
            port: address.port,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Ping {
    pub n: u64,
    pub fail: u64,
    pub b: [u64; BUCKETS],
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Use {
    pub up: u64,
    pub down: u64,
    /// Секунды с трафиком через узел за этот час; в отчёт — минутами.
    pub sec: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Freeze {
    pub verdict: String,
    pub status: i64,
    pub at: i64,
}

/// Где сделан замер: сеть, её вид и внешний адрес клиента в ней.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Place {
    pub net: String,
    pub kind: &'static str,
    pub ip4: String,
    pub ip6: String,
}

/// Час в одной сети с одним внешним адресом. Сменился адрес внутри часа —
/// у того же часа будет вторая запись: замер всегда лежит при том адресе,
/// с которого сделан.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Hour {
    pub h: i64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub ip4: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub ip6: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub ping: BTreeMap<String, Ping>,
    #[serde(default, rename = "use", skip_serializing_if = "BTreeMap::is_empty")]
    pub used: BTreeMap<String, Use>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub freeze: BTreeMap<String, Freeze>,
}

impl Hour {
    fn has_data(&self) -> bool {
        !self.ping.is_empty() || !self.used.is_empty() || !self.freeze.is_empty()
    }

    fn keys(&self) -> impl Iterator<Item = &String> {
        self.ping.keys().chain(self.used.keys()).chain(self.freeze.keys())
    }

    /// Запись часа в форме отчёта (`lib/reports.php`): трафик — минутами.
    fn to_report(&self) -> serde_json::Value {
        let used: serde_json::Map<String, serde_json::Value> = self
            .used
            .iter()
            .map(|(key, used)| {
                (
                    key.clone(),
                    serde_json::json!({ "up": used.up, "down": used.down, "min": used.sec.div_ceil(60).min(60) }),
                )
            })
            .collect();
        let mut entry = serde_json::json!({ "h": self.h });
        if let Some(map) = entry.as_object_mut() {
            if !self.ip4.is_empty() {
                map.insert("ip4".into(), serde_json::json!(self.ip4));
            }
            if !self.ip6.is_empty() {
                map.insert("ip6".into(), serde_json::json!(self.ip6));
            }
            if !self.ping.is_empty() {
                map.insert("ping".into(), serde_json::json!(self.ping));
            }
            if !used.is_empty() {
                map.insert("use".into(), serde_json::Value::Object(used));
            }
            if !self.freeze.is_empty() {
                map.insert("freeze".into(), serde_json::json!(self.freeze));
            }
        }
        entry
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Network {
    pub kind: String,
    /// Часы по порядку появления: (начало часа, адрес) — одна запись.
    #[serde(default)]
    pub hours: Vec<Hour>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Store {
    /// Когда прослойка в последний раз ответила на отчёт каналом (любым кодом).
    #[serde(default)]
    pub last_try: i64,
    /// Когда отчёт в последний раз был принят: начало следующего периода.
    #[serde(default)]
    pub last_sent: i64,
    /// Ключ узла → тип, адрес и порт.
    #[serde(default)]
    pub nodes: BTreeMap<String, NodeInfo>,
    /// Хеш ключа сети → замеры в ней.
    #[serde(default)]
    pub networks: BTreeMap<String, Network>,
}

/// Начало часа, в который попадает момент.
pub(super) const fn hour_of(at: i64) -> i64 {
    at - at.rem_euclid(HOUR)
}

/// Ключ узла для прослойки: тот же расчёт, что у неё (`rep_node_key`).
pub(super) fn node_key(info: &NodeInfo) -> String {
    use sha2::{Digest as _, Sha256};
    let text = format!("{}|{}|{}", info.kind, info.server.to_lowercase(), info.port);
    hex::encode(Sha256::digest(text.as_bytes()))[..12].to_owned()
}

pub(super) fn bucket_of(delay_ms: u64) -> usize {
    BUCKET_EDGES
        .iter()
        .position(|edge| delay_ms < *edge)
        .unwrap_or(BUCKET_EDGES.len())
}

impl Store {
    fn hour_mut(&mut self, place: &Place, at: i64) -> &mut Hour {
        let network = self.networks.entry(place.net.clone()).or_default();
        place.kind.clone_into(&mut network.kind);
        let h = hour_of(at);
        let found = network
            .hours
            .iter()
            .position(|hour| hour.h == h && hour.ip4 == place.ip4 && hour.ip6 == place.ip6);
        let index = found.unwrap_or_else(|| {
            network.hours.push(Hour {
                h,
                ip4: place.ip4.clone(),
                ip6: place.ip6.clone(),
                ..Hour::default()
            });
            network.hours.len() - 1
        });
        &mut network.hours[index]
    }

    pub(super) fn remember_node(&mut self, key: &str, info: &NodeInfo) {
        self.nodes.insert(key.to_owned(), info.clone());
    }

    /// Один замер задержки: 0 — неудача.
    pub(super) fn add_ping(&mut self, place: &Place, at: i64, key: &str, delay_ms: u64) {
        let ping = self.hour_mut(place, at).ping.entry(key.to_owned()).or_default();
        ping.n += 1;
        if delay_ms == 0 {
            ping.fail += 1;
        } else {
            ping.b[bucket_of(delay_ms)] += 1;
        }
    }

    pub(super) fn add_use(&mut self, place: &Place, at: i64, key: &str, delta: &Use) {
        let used = self.hour_mut(place, at).used.entry(key.to_owned()).or_default();
        used.up = used.up.saturating_add(delta.up);
        used.down = used.down.saturating_add(delta.down);
        used.sec = used.sec.saturating_add(delta.sec).min(HOUR as u64);
    }

    pub(super) fn add_freeze(&mut self, place: &Place, at: i64, key: &str, verdict: &str, status: i64) {
        self.hour_mut(place, at).freeze.insert(
            key.to_owned(),
            Freeze {
                verdict: verdict.to_owned(),
                status,
                at,
            },
        );
    }

    /// Выбросить старше [`KEEP`], пустые часы и сети и узлы, на которые никто
    /// не ссылается; `true` — что-то выброшено.
    pub(super) fn prune(&mut self, now: i64) -> bool {
        let oldest = hour_of(now - KEEP);
        let mut removed = false;
        for network in self.networks.values_mut() {
            let had = network.hours.len();
            network.hours.retain(|hour| hour.h >= oldest && hour.has_data());
            removed |= network.hours.len() != had;
        }
        self.networks.retain(|_, network| !network.hours.is_empty());
        let used = self.referenced();
        let had = self.nodes.len();
        self.nodes.retain(|key, _| used.contains(key));
        removed || self.nodes.len() != had
    }

    fn referenced(&self) -> std::collections::BTreeSet<String> {
        self.networks
            .values()
            .flat_map(|network| network.hours.iter())
            .flat_map(Hour::keys)
            .cloned()
            .collect()
    }

    /// Самый старый закрытый час — тот, что уже не пополнится; `None` — отправлять нечего.
    pub(super) fn oldest_closed(&self, now: i64) -> Option<i64> {
        let current = hour_of(now);
        self.networks
            .values()
            .flat_map(|network| network.hours.iter())
            .map(|hour| hour.h)
            .filter(|h| *h < current)
            .min()
    }

    /// Сколько записей часов раньше `until`.
    pub(super) fn hours_before(&self, until: i64) -> usize {
        self.networks
            .values()
            .map(|network| network.hours.iter().filter(|hour| hour.h < until).count())
            .sum()
    }

    /// Отчёт из часов раньше `until` (закрытых). Форма — та, что ждёт прослойка (`lib/reports.php`).
    pub(super) fn report(&self, now: i64, until: i64, dev: &str) -> serde_json::Value {
        let mut networks = serde_json::Map::new();
        let mut nodes = serde_json::Map::new();
        let mut from = i64::MAX;
        for (net, network) in &self.networks {
            let mut closed: Vec<&Hour> = network.hours.iter().filter(|hour| hour.h < until).collect();
            closed.sort_by_key(|hour| hour.h);
            let mut hours = Vec::with_capacity(closed.len());
            for hour in closed {
                from = from.min(hour.h);
                for key in hour.keys() {
                    if let Some(info) = self.nodes.get(key) {
                        nodes.insert(key.clone(), serde_json::json!(info));
                    }
                }
                hours.push(hour.to_report());
            }
            if !hours.is_empty() {
                networks.insert(net.clone(), serde_json::json!({ "kind": network.kind, "hours": hours }));
            }
        }
        serde_json::json!({
            "v": 1,
            "platform": "pc",
            "client": env!("CARGO_PKG_VERSION"),
            "dev": dev,
            "from": if from == i64::MAX { now } else { from },
            "to": now,
            "nodes": nodes,
            "networks": networks,
        })
    }

    /// Отчёт принят: отправленные часы (раньше `until`) уходят, остальное остаётся.
    pub(super) fn drop_sent(&mut self, now: i64, until: i64) {
        for network in self.networks.values_mut() {
            network.hours.retain(|hour| hour.h >= until);
        }
        self.last_sent = now;
        self.prune(now);
    }
}

fn file_path(uid: &str) -> Result<std::path::PathBuf> {
    Ok(dirs::app_profiles_dir()?.join(dirs::report_file(uid)))
}

/// Накопленное подписки есть на диске.
pub(super) fn exists(uid: &str) -> bool {
    file_path(uid).is_ok_and(|path| path.is_file())
}

/// Прочитать накопленное; нет файла или он испорчен — пустое.
pub(super) async fn load(uid: &str) -> Store {
    let Ok(path) = file_path(uid) else {
        return Store::default();
    };
    help::load_json_or_default(&path, "[Report] the saved measurements").await
}

pub(super) async fn save(uid: &str, store: &Store) -> Result<()> {
    help::save_json(&file_path(uid)?, store).await
}

#[cfg(test)]
mod tests {
    use super::{HOUR, KEEP, NodeInfo, Place, Store, Use, bucket_of, hour_of, node_key};

    const fn used(up: u64, down: u64, sec: u64) -> Use {
        Use { up, down, sec }
    }

    const NOW: i64 = 1_800_000_000;

    fn info() -> NodeInfo {
        NodeInfo {
            name: "Узел".into(),
            kind: "vless".into(),
            server: "Node.Example.com".into(),
            port: 443,
        }
    }

    fn place(net: &str, ip4: &str) -> Place {
        Place {
            net: net.into(),
            kind: "wifi",
            ip4: ip4.into(),
            ip6: String::new(),
        }
    }

    #[test]
    fn the_node_key_matches_the_middleware() {
        // php: substr(hash('sha256', 'vless|node.example.com|443'), 0, 12) — см. lib/reports.php
        assert_eq!(node_key(&info()), "5b9a8536deff");
    }

    #[test]
    fn delays_fall_into_the_colour_buckets() {
        assert_eq!(bucket_of(1), 0);
        assert_eq!(bucket_of(99), 0);
        assert_eq!(bucket_of(100), 1);
        assert_eq!(bucket_of(399), 2);
        assert_eq!(bucket_of(799), 3);
        assert_eq!(bucket_of(1499), 4);
        assert_eq!(bucket_of(5000), 5);
    }

    #[test]
    fn only_closed_hours_are_reported_and_dropped() {
        let mut store = Store::default();
        let key = node_key(&info());
        store.remember_node(&key, &info());
        let past = hour_of(NOW) - HOUR;
        let here = place("net0000000000000", "203.0.113.7");
        store.add_ping(&here, past + 10, &key, 150);
        store.add_ping(&here, past + 20, &key, 0);
        store.add_use(&here, past + 30, &key, &used(10, 20, 61));
        store.add_ping(&here, NOW, &key, 50);

        assert_eq!(store.oldest_closed(NOW), Some(past));
        assert_eq!(store.hours_before(hour_of(NOW)), 1);
        let report = store.report(NOW, hour_of(NOW), "0123456789abcdef");
        let hours = &report["networks"]["net0000000000000"]["hours"];
        assert_eq!(hours.as_array().map(Vec::len), Some(1));
        assert_eq!(hours[0]["h"], past);
        assert_eq!(hours[0]["ip4"], "203.0.113.7");
        assert_eq!(hours[0]["ip6"], serde_json::Value::Null);
        assert_eq!(hours[0]["ping"][&key]["n"], 2);
        assert_eq!(hours[0]["ping"][&key]["fail"], 1);
        assert_eq!(hours[0]["ping"][&key]["b"][1], 1);
        assert_eq!(hours[0]["use"][&key]["min"], 2);
        assert_eq!(hours[0]["use"][&key]["sec"], serde_json::Value::Null);
        assert_eq!(report["nodes"][&key]["type"], "vless");
        assert_eq!(report["networks"]["net0000000000000"]["kind"], "wifi");

        store.drop_sent(NOW, hour_of(NOW));
        assert_eq!(store.oldest_closed(NOW), None);
        assert_eq!(store.last_sent, NOW);
        assert!(store.nodes.contains_key(&key), "узел текущего часа остаётся");
    }

    #[test]
    fn a_new_address_within_the_hour_gets_its_own_record() {
        let mut store = Store::default();
        let past = hour_of(NOW) - HOUR;
        store.add_ping(&place("n", "203.0.113.7"), past + 10, "k", 100);
        store.add_ping(&place("n", "198.51.100.9"), past + 900, "k", 0);
        store.add_ping(&place("n", "203.0.113.7"), past + 1800, "k", 100);

        let report = store.report(NOW, hour_of(NOW), "");
        let hours = report["networks"]["n"]["hours"].as_array().cloned().unwrap_or_default();
        assert_eq!(hours.len(), 2);
        assert_eq!(hours[0]["ip4"], "203.0.113.7");
        assert_eq!(hours[0]["ping"]["k"]["n"], 2);
        assert_eq!(hours[1]["ip4"], "198.51.100.9");
        assert_eq!(hours[1]["ping"]["k"]["fail"], 1);
    }

    #[test]
    fn old_hours_and_orphan_nodes_are_forgotten() {
        let mut store = Store::default();
        let key = node_key(&info());
        store.remember_node(&key, &info());
        store.remember_node("orphan", &info());
        store.add_use(&place("n", ""), NOW - KEEP - HOUR, &key, &used(1, 2, 300));
        store.prune(NOW);
        assert!(store.networks.is_empty());
        assert!(store.nodes.is_empty());
    }

    #[test]
    fn pruning_tells_whether_it_removed_anything() {
        let mut store = Store::default();
        let key = node_key(&info());
        store.remember_node(&key, &info());
        store.add_ping(&place("n", ""), NOW, &key, 100);
        assert!(!store.prune(NOW), "свежее не трогается");
        store.remember_node("orphan", &info());
        assert!(store.prune(NOW), "узел без ссылок выброшен");
        assert!(!store.prune(NOW));
        assert!(store.prune(NOW + KEEP + 2 * HOUR), "старый час выброшен");
        assert!(store.networks.is_empty() && store.nodes.is_empty());
    }

    #[test]
    fn time_of_use_stays_within_the_hour() {
        let mut store = Store::default();
        for _ in 0..20 {
            store.add_use(&place("n", ""), NOW, "k", &used(1, 1, 300));
        }
        let hour = &store.networks["n"].hours[0];
        assert_eq!(hour.used["k"].sec, 3600);
        assert_eq!(hour.used["k"].up, 20);
        assert_eq!(hour.to_report()["use"]["k"]["min"], 60);
    }
}
