//! clod:report — отчёт прослойке о качестве узлов.
//!
//! Копится только у текущей подписки с защищённым каналом: сборщик берёт то,
//! что ядро уже намерило (история задержек узлов из `/proxies`, байты
//! соединений по узлам из `/connections`), и раскладывает по сети, часу и
//! внешнему адресу клиента. Своих проб нет. Проверка 16–20 приносит свои итоги
//! сама ([`note_freeze`]). Храним не больше 7 суток: накопленное живёт в
//! памяти, в файл уходит раз в пять минут, при закрытии часа, после отправки
//! и на выходе.
//!
//! Все замеры: ядро держит у узла только 10 последних, поэтому сборщик читает
//! их тем чаще, чем чаще узлы проверяются (от 20 секунд до 5 минут), и
//! отсчитывает окно по миллисекундам — ни один замер не теряется и не
//! считается дважды.
//!
//! Замер лежит в той сети и при том адресе, где сделан. Сеть с адресом —
//! «место» — узнаётся один раз и держится, пока сторож среды не увидит смену
//! сети или пробуждение ([`network_changed`]): тогда всё намеренное до смены
//! уходит в старое место (сторож этого не ждёт: хвост трафика последних
//! секунд дороже, чем задержка переподключения), последние секунды перед
//! тем, как смену заметили, отбрасываются (неясно, в какой сети они
//! сделаны), а новое место узнаётся сразу, с новым адресом. Если же сеть при очередном чтении оказалась другой,
//! а сторож смолчал, окно с неизвестным моментом смены отбрасывается целиком.
//! В одной сети адрес переспрашивается раз в час.
//!
//! Уходит отчёт только по защищённому каналу, только после удачного планового
//! обновления подписки и не чаще раза в 6 часов, закрытыми часами — от самых
//! старых, не больше двух суток за раз и не больше, чем примет прослойка.
//! Принят (204) — отправленное удаляется, остальное ждёт следующего раза. Приём выключен в прослойке (403), рано (429)
//! или прослойка старая и ответила подпиской — накопленное остаётся, следующая
//! попытка через 6 часов. Каналом не ответил никто — попытка при следующем
//! плановом обновлении.

mod ip;
mod store;

use std::collections::{HashMap, HashSet};
use std::io::Write as _;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use clash_verge_logging::{Type, logging};
use parking_lot::Mutex;

use crate::{
    config::{Config, PrfItem, proxy_label::Address},
    constants::timing,
    core::handle,
    module::freeze_check::Verdict,
    process::AsyncHandler,
    utils::{dirs, help},
};
use store::{NodeInfo, Place, Use};

/// Чтения истории задержек — не реже этого.
const TICK: Duration = Duration::from_secs(5 * 60);
/// И не чаще этого.
const TICK_MIN: Duration = Duration::from_secs(20);
/// Снимок соединений (его приносит общий опрос, `core::connections_poll`):
/// байты по узлам. Ядро отдаёт только живые соединения, закрытое между
/// чтениями теряется целиком — поэтому часто: теряется лишь хвост последних
/// секунд каждого соединения.
const TRAFFIC_TICK: Duration = Duration::from_secs(10);
/// Столько последних замеров ядро держит у узла (`defaultHistoriesNum`).
const HISTORY_CAP: usize = 10;
/// Сторож среды замечает смену сети не позже чем через тик и паузу на
/// устоявшуюся сеть; замеры за столько до того, как смену заметили, не
/// привязать ни к старой сети, ни к новой.
const CHANGE_GUARD_MS: i64 = timing::ENVIRONMENT_TICK.as_millis() as i64 + 5_000;
/// Отчёт уходит не чаще этого.
const SEND_EVERY: i64 = 6 * 60 * 60;
/// За один отчёт уходит не больше этого: от самого старого закрытого часа.
/// Остальное — следующим отчётом; недельный завал расходится за сутки.
const SEND_WINDOW: i64 = 48 * 60 * 60;
/// Больше этого сжатый отчёт прослойка не принимает (`CHAN_REP_MAX_WIRE`):
/// длиннее — окно уполовинивается, пока не влезет.
const REPORT_MAX_GZ: usize = 240 * 1024;
/// Внешний адрес в одной сети переспрашивается не чаще этого.
const IP_EVERY: i64 = 60 * 60;
/// Не узнался — переспрашивается не чаще этого.
const IP_RETRY: i64 = 5 * 60;
const CORE_TIMEOUT: Duration = Duration::from_secs(5);
/// Накопленное пишется в файл не чаще этого (ещё — при закрытии часа, после
/// отправки и на выходе): при аварийном завершении теряется не больше этого.
const SAVE_EVERY: i64 = 5 * 60;
const DEVICE_FILE: &str = "report-device";

/// Накопленное одной подписки в памяти: файл читается один раз, пишется по
/// [`SAVE_EVERY`]. Сборщик, проверка 16–20 и отправка ходят сюда по одному.
struct Cached {
    uid: String,
    store: store::Store,
    dirty: bool,
    /// Когда файл писали в последний раз (или читали).
    saved_at: i64,
    /// Начало часа, в котором накопленное чистили в последний раз.
    pruned_hour: i64,
}

static CACHE: tokio::sync::Mutex<Option<Cached>> = tokio::sync::Mutex::const_new(None);
/// Чтение замеров и смена места идут по одному.
static COLLECT: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
/// Сколько раз сторож среды видел смену сети или пробуждение.
static CHANGES: AtomicU64 = AtomicU64::new(0);

/// Место, где сейчас делаются замеры.
#[derive(Clone)]
struct Spot {
    place: Place,
    /// Значение [`CHANGES`], при котором место узнано: другое — место устарело.
    changes: u64,
    /// Когда адрес спрашивали в последний раз.
    ip_at: i64,
}

impl Spot {
    const fn ip_is_due(&self, now: i64) -> bool {
        let unknown = self.place.ip4.is_empty() && self.place.ip6.is_empty();
        now.saturating_sub(self.ip_at) >= if unknown { IP_RETRY } else { IP_EVERY }
    }
}

/// Байты через узел с прошлого сброса в файл: выгрузка, загрузка, секунды с трафиком.
type Traffic = HashMap<String, (u64, u64, u64)>;

/// Узлы подписки, которую крутит ядро: имя, каким его называет ядро, → тип,
/// адрес и порт (общий разбор сборки, `config::proxy_label`).
type Nodes = Arc<HashMap<String, Address>>;

#[derive(Default)]
struct Runtime {
    /// Подписка, для которой идёт сбор; сменилась — всё заново.
    uid: Option<String>,
    /// Замеры задержки до этого момента (мс) уже разложены или отброшены.
    pings_until: i64,
    /// `id` соединения → учтённые байты (выгрузка, загрузка).
    seen: HashMap<String, (u64, u64)>,
    /// Когда в последний раз читались соединения (мс); 0 — ещё не читались.
    read_at: i64,
    /// Накопленный с прошлого сброса трафик по узлам.
    traffic: Traffic,
    spot: Option<Spot>,
    /// Через сколько читать историю задержек снова.
    next: Option<Duration>,
}

static RUNTIME: Mutex<Option<Runtime>> = Mutex::new(None);

fn with_runtime<T>(f: impl FnOnce(&mut Runtime) -> T) -> T {
    let mut guard = RUNTIME.lock();
    f(guard.get_or_insert_with(Runtime::default))
}

fn now_millis() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// Записать накопленное в файл, если есть что (перед записью — почистить).
/// Подписки, которой в реестре уже нет, файл не возвращается: его убрало
/// удаление подписки.
async fn flush(entry: &mut Cached, now: i64) {
    if !entry.dirty {
        return;
    }
    entry.store.prune(now);
    entry.pruned_hour = store::hour_of(now);
    let known = Config::profiles().await.latest_arc().get_item(&entry.uid).is_ok();
    if known && let Err(err) = store::save(&entry.uid, &entry.store).await {
        logging!(warn, Type::Core, "[Report] the measurements were not saved: {err:#}");
        return;
    }
    entry.dirty = false;
    entry.saved_at = now;
}

/// Накопленное подписки — в работу; `work` отвечает и тем, изменила ли она
/// накопленное. `save` — писать в файл сразу (после отправки, вердикты 16–20),
/// иначе по [`SAVE_EVERY`] и при закрытии часа. Старое чистится раз в час и
/// перед записью в файл.
async fn with_store<T>(uid: &str, now: i64, save: bool, work: impl FnOnce(&mut store::Store) -> (T, bool)) -> T {
    let mut guard = CACHE.lock().await;
    if guard.as_ref().is_none_or(|entry| entry.uid != uid) {
        if let Some(old) = guard.as_mut() {
            flush(old, now).await;
        }
        let store = store::load(uid).await;
        *guard = Some(Cached {
            uid: uid.to_owned(),
            store,
            dirty: false,
            saved_at: now,
            pruned_hour: i64::MIN,
        });
    }
    // Запись только что положена, закрытие не выполнится.
    let entry = guard.get_or_insert_with(|| Cached {
        uid: uid.to_owned(),
        store: store::Store::default(),
        dirty: false,
        saved_at: now,
        pruned_hour: i64::MIN,
    });
    let (out, changed) = work(&mut entry.store);
    entry.dirty |= changed;
    let hour = store::hour_of(now);
    if hour != entry.pruned_hour {
        entry.dirty |= entry.store.prune(now);
        entry.pruned_hour = hour;
    }
    let due = now.saturating_sub(entry.saved_at) >= SAVE_EVERY || store::hour_of(now) != store::hour_of(entry.saved_at);
    if save || due {
        flush(entry, now).await;
    }
    drop(guard);
    out
}

/// Накопленное — в файл: на выходе из приложения.
pub async fn flush_at_exit() {
    let mut guard = CACHE.lock().await;
    if let Some(entry) = guard.as_mut() {
        flush(entry, help::now_secs()).await;
    }
    drop(guard);
}

/// Сбор кончился — накопленное в файл, из памяти вон.
async fn forget_store() {
    let mut guard = CACHE.lock().await;
    if let Some(entry) = guard.as_mut() {
        flush(entry, help::now_secs()).await;
    }
    *guard = None;
    drop(guard);
}

fn is_collected(item: &PrfItem) -> bool {
    item.itype.as_deref() == Some("remote") && item.option.as_ref().is_some_and(|o| o.secure == Some(true))
}

/// Текущая подписка, если по ней копится отчёт: удалённая и с защищённым
/// каналом — только им отчёт и может уйти.
async fn collecting_uid() -> Option<String> {
    let profiles = Config::profiles().await.latest_arc();
    let uid = profiles.get_current()?.to_string();
    profiles.get_item(&uid).ok().filter(|item| is_collected(item))?;
    Some(uid)
}

/// Моменты (мс) и задержки истории узла по порядку записи.
fn history_of(entry: &serde_json::Value) -> Vec<(i64, u64)> {
    entry
        .get("history")
        .and_then(serde_json::Value::as_array)
        .map(|history| {
            history
                .iter()
                .filter_map(|record| {
                    let at = record
                        .get("time")
                        .and_then(serde_json::Value::as_str)
                        .and_then(|time| chrono::DateTime::parse_from_rfc3339(time).ok())?
                        .timestamp_millis();
                    let delay = record.get("delay").and_then(serde_json::Value::as_u64).unwrap_or(0);
                    Some((at, delay))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// История задержек узлов из `wanted`: из `/proxies` (узлы самой подписки) и
/// провайдеров подписки (узлы провайдеров — в `/proxies` их нет).
fn histories(
    proxies: &serde_json::Value,
    providers: Option<&serde_json::Value>,
    wanted: &HashMap<String, Address>,
) -> HashMap<String, Vec<(i64, u64)>> {
    let mut out = HashMap::new();
    if let Some(map) = proxies.get("proxies").and_then(serde_json::Value::as_object) {
        for (name, entry) in map {
            if wanted.contains_key(name) {
                out.insert(name.clone(), history_of(entry));
            }
        }
    }
    let listed = providers
        .and_then(|providers| providers.get("providers"))
        .and_then(serde_json::Value::as_object);
    for provider in listed.into_iter().flat_map(|map| map.values()) {
        let Some(members) = provider.get("proxies").and_then(serde_json::Value::as_array) else {
            continue;
        };
        for member in members {
            let Some(name) = member.get("name").and_then(serde_json::Value::as_str) else {
                continue;
            };
            if wanted.contains_key(name) && !out.contains_key(name) {
                out.insert(name.to_owned(), history_of(member));
            }
        }
    }
    out
}

/// Замеры задержки: (имя, когда в мс, мс; 0 — неудача) в окне `(since, until]`.
fn pings_of(histories: &HashMap<String, Vec<(i64, u64)>>, since: i64, until: i64) -> Vec<(String, i64, u64)> {
    histories
        .iter()
        .flat_map(|(name, history)| {
            history
                .iter()
                .filter(|(at, _)| *at > since && *at <= until)
                .map(|(at, delay)| (name.clone(), *at, *delay))
        })
        .collect()
}

/// Через сколько читать историю снова, чтобы застать каждый замер: у узла с
/// полной историей десять замеров уложились в `span` — читать вдвое чаще.
/// Второе — сколько узлов, возможно, уже потеряли замеры с окна `since`.
fn next_read(histories: &HashMap<String, Vec<(i64, u64)>>, since: i64) -> (Duration, usize) {
    let mut next = TICK;
    let mut overflowed = 0;
    for history in histories.values() {
        if history.len() < HISTORY_CAP {
            continue;
        }
        let oldest = history.iter().map(|(at, _)| *at).min().unwrap_or(0);
        let newest = history.iter().map(|(at, _)| *at).max().unwrap_or(0);
        if since > 0 && oldest > since {
            overflowed += 1;
        }
        let half = Duration::from_millis(u64::try_from((newest - oldest) / 2).unwrap_or(0));
        next = next.min(half);
    }
    (next.clamp(TICK_MIN, TICK), overflowed)
}

/// Сколько ждать между снимками соединений: [`TRAFFIC_TICK`], пока по
/// текущей подписке копится отчёт; `None` — снимки не нужны.
pub(crate) async fn traffic_every() -> Option<Duration> {
    collecting_uid().await.map(|_| TRAFFIC_TICK)
}

/// Прирост байтов соединений с прошлого снимка — в копилку по узлам. Первый
/// снимок только запоминает счётчики: прошлое соединений не в счёт. `None` —
/// ядро не ответило.
pub(crate) fn count_traffic(response: Option<&tauri_plugin_mihomo::models::Connections>) {
    let Some(response) = response else {
        return;
    };
    let connections = response.connections.as_deref().unwrap_or_default();
    let now = now_millis();
    with_runtime(|runtime| {
        let was = std::mem::replace(&mut runtime.read_at, now);
        let secs = u64::try_from((now - was) / 1000)
            .unwrap_or(0)
            .clamp(1, TRAFFIC_TICK.as_secs() * 3);
        let mut alive = HashMap::with_capacity(connections.len());
        // Секунды с трафиком узлу засчитываются раз за чтение, сколько бы
        // соединений через него ни шло.
        let mut counted = HashSet::new();
        for connection in connections {
            let Some(node) = connection.chains.first() else {
                continue;
            };
            let (up_was, down_was) = runtime.seen.get(&connection.id).copied().unwrap_or_default();
            let up = connection.upload.saturating_sub(up_was);
            let down = connection.download.saturating_sub(down_was);
            if was > 0 && (up > 0 || down > 0) {
                let sum = runtime.traffic.entry(node.clone()).or_default();
                sum.0 = sum.0.saturating_add(up);
                sum.1 = sum.1.saturating_add(down);
                if counted.insert(node.clone()) {
                    sum.2 = sum.2.saturating_add(secs);
                }
            }
            alive.insert(connection.id.clone(), (connection.upload, connection.download));
        }
        runtime.seen = alive;
    });
}

/// Что прочитано за окно: замеры задержки и накопленный трафик.
struct Window {
    nodes: Nodes,
    pings: Vec<(String, i64, u64)>,
    traffic: Traffic,
}

impl Window {
    fn is_empty(&self) -> bool {
        self.pings.is_empty() && self.traffic.is_empty()
    }
}

/// Прочитать у ядра замеры с прошлого чтения по `until` (мс) и забрать
/// накопленный трафик. `None` — ядро не ответило: окно не сдвигается, замеры
/// дождутся следующего чтения.
async fn read_window(uid: &str, until: i64) -> Option<Window> {
    let nodes = crate::config::proxy_label::addresses(uid)
        .await
        .filter(|nodes| !nodes.is_empty())?;
    // Сбойный провайдер пропускается: его замеры дождутся следующего чтения.
    let (proxies, providers, _failed) = crate::feat::read_core_proxies(CORE_TIMEOUT).await.into_json()?;
    let listed = histories(&proxies, Some(&providers), &nodes);
    let since = with_runtime(|runtime| runtime.pings_until);
    let pings = pings_of(&listed, since, until);
    let (next, overflowed) = next_read(&listed, since);
    if overflowed > 0 {
        logging!(
            debug,
            Type::Core,
            "[Report] {overflowed} node(s) were checked more often than read, reading every {next:?} now"
        );
    }
    let traffic = with_runtime(|runtime| {
        runtime.pings_until = until;
        runtime.next = Some(next);
        std::mem::take(&mut runtime.traffic)
    });
    Some(Window { nodes, pings, traffic })
}

/// Только накопленный трафик, без чтения ядра.
fn take_traffic(nodes: Nodes) -> Window {
    Window {
        nodes,
        pings: Vec::new(),
        traffic: with_runtime(|runtime| std::mem::take(&mut runtime.traffic)),
    }
}

/// Разложить окно в место и сохранить.
async fn record(uid: &str, place: &Place, window: Window) {
    if window.is_empty() {
        return;
    }
    let Window { nodes, pings, traffic } = window;
    let now = help::now_secs();
    with_store(uid, now, false, |saved| {
        let mut changed = !pings.is_empty();
        for (name, at, delay) in pings {
            let info = NodeInfo::of(&name, &nodes[&name]);
            let key = store::node_key(&info);
            saved.remember_node(&key, &info);
            saved.add_ping(place, at / 1000, &key, delay);
        }
        for (name, (up, down, sec)) in traffic {
            let Some(address) = nodes.get(&name) else {
                continue;
            };
            let info = NodeInfo::of(&name, address);
            let key = store::node_key(&info);
            saved.remember_node(&key, &info);
            changed = true;
            saved.add_use(
                place,
                now,
                &key,
                &Use {
                    up,
                    down,
                    sec: sec.max(1),
                },
            );
        }
        ((), changed)
    })
    .await;
}

/// Подписка сменилась (или сбор начался) — всё с чистого листа; замеры
/// считаются с этого момента.
fn start_over_for(uid: &str) {
    with_runtime(|runtime| {
        if runtime.uid.as_deref() != Some(uid) {
            *runtime = Runtime {
                uid: Some(uid.to_owned()),
                pings_until: now_millis(),
                ..Runtime::default()
            };
        }
    });
}

/// Что делать с адресом после чтения — уже без замка сборщика: запрос к
/// сайту долгий, а сторож среды ждёт замок считаные секунды.
enum IpWork {
    None,
    /// Места нет — узнать сеть и адрес.
    Locate(u64),
    /// Адрес места пора переспросить.
    Refresh(Spot),
}

async fn tick() {
    let work = {
        let _collect = COLLECT.lock().await;
        let Some(uid) = collecting_uid().await else {
            with_runtime(|runtime| *runtime = Runtime::default());
            forget_store().await;
            return;
        };
        start_over_for(&uid);
        let until = now_millis();
        let Some(window) = read_window(&uid, until).await else {
            return;
        };
        let changes = CHANGES.load(Ordering::Acquire);
        let here = crate::module::freeze_check::current_network().await;
        let spot = with_runtime(|runtime| runtime.spot.clone())
            .filter(|spot| spot.changes == changes && here.as_ref().is_some_and(|(net, _)| *net == spot.place.net));
        match spot {
            Some(spot) => {
                record(&uid, &spot.place, window).await;
                if spot.ip_is_due(help::now_secs()) {
                    IpWork::Refresh(spot)
                } else {
                    IpWork::None
                }
            }
            None => {
                // Места нет или сеть уже другая, а когда сменилась — неизвестно:
                // окно не к чему привязать. Узнаём место заново.
                if !window.is_empty() {
                    logging!(
                        debug,
                        Type::Core,
                        "[Report] {} measurement(s) dropped: the network they were made in is not known for sure",
                        window.pings.len() + window.traffic.len()
                    );
                }
                if here.is_some() {
                    IpWork::Locate(changes)
                } else {
                    IpWork::None
                }
            }
        }
    };
    match work {
        IpWork::None => {}
        IpWork::Locate(changes) => {
            let spot = locate(changes).await;
            with_runtime(|runtime| {
                if CHANGES.load(Ordering::Acquire) == changes && runtime.spot.is_none() {
                    runtime.spot = spot;
                }
            });
        }
        IpWork::Refresh(mut spot) => {
            let (ip4, ip6) = ip::current().await;
            // Не ответил сайт — прежний адрес остаётся: сеть та же.
            if !ip4.is_empty() || !ip6.is_empty() {
                spot.place.ip4 = ip4;
                spot.place.ip6 = ip6;
            }
            spot.ip_at = help::now_secs();
            with_runtime(|runtime| {
                if runtime.spot.as_ref().is_some_and(|kept| kept.changes == spot.changes) {
                    runtime.spot = Some(spot);
                }
            });
        }
    }
}

/// Узнать место: текущую сеть и внешний адрес в ней.
async fn locate(changes: u64) -> Option<Spot> {
    let (net, kind) = crate::module::freeze_check::current_network().await?;
    let (ip4, ip6) = ip::current().await;
    Some(Spot {
        place: Place { net, kind, ip4, ip6 },
        changes,
        ip_at: help::now_secs(),
    })
}

/// Сторож среды увидел смену сети или пробуждение. Намеренное до смены уходит
/// в старое место, последние [`CHANGE_GUARD_MS`] перед ней отбрасываются, новое
/// место узнаётся сразу. Сторожа это не задерживает: байты соединений,
/// закрытых им раньше, чем дошёл сброс, — хвост последних секунд, не больше.
pub(crate) fn network_changed() {
    let seen_at = now_millis();
    let before = CHANGES.fetch_add(1, Ordering::AcqRel);
    AsyncHandler::spawn(move || async move {
        flush_before_change(seen_at, before).await;
        tick().await;
    });
}

async fn flush_before_change(seen_at: i64, before: u64) {
    let _collect = COLLECT.lock().await;
    let Some(uid) = collecting_uid().await else {
        return;
    };
    start_over_for(&uid);
    let old = with_runtime(|runtime| runtime.spot.take()).filter(|spot| spot.changes == before);
    let cut = seen_at - CHANGE_GUARD_MS;
    let Some(old) = old else {
        with_runtime(|runtime| {
            runtime.pings_until = runtime.pings_until.max(seen_at);
            runtime.traffic.clear();
        });
        return;
    };
    let since = with_runtime(|runtime| runtime.pings_until);
    // Трафик копился в старой сети — уходит туда даже без окна замеров.
    let window = if cut > since {
        read_window(&uid, cut).await
    } else {
        Some(take_traffic(
            crate::config::proxy_label::addresses(&uid).await.unwrap_or_default(),
        ))
    };
    with_runtime(|runtime| runtime.pings_until = runtime.pings_until.max(seen_at));
    if let Some(window) = window {
        record(&uid, &old.place, window).await;
    }
}

/// Проверка 16–20 вынесла вердикты в сети `net`: в отчёт, если по этой подписке он копится.
pub(crate) async fn note_freeze(
    uid: &str,
    net: &str,
    kind: &'static str,
    verdicts: &[(String, Verdict, i64)],
    now: i64,
) {
    if verdicts.is_empty() {
        return;
    }
    let collected = Config::profiles()
        .await
        .latest_arc()
        .get_item(uid)
        .is_ok_and(is_collected);
    if !collected {
        return;
    }
    let nodes = crate::config::proxy_label::addresses(uid).await.unwrap_or_default();
    let known: Vec<_> = verdicts
        .iter()
        .filter_map(|(name, verdict, status)| {
            nodes
                .get(name)
                .map(|address| (NodeInfo::of(name, address), verdict, *status))
        })
        .collect();
    if known.is_empty() {
        return;
    }
    // Адрес — места этой сети, если оно уже узнано; иначе спросить сейчас.
    let changes = CHANGES.load(Ordering::Acquire);
    let kept = with_runtime(|runtime| runtime.spot.clone())
        .filter(|spot| spot.changes == changes && spot.place.net == net)
        .map(|spot| spot.place);
    let place = match kept {
        Some(place) => place,
        None => {
            let (ip4, ip6) = ip::current().await;
            Place {
                net: net.to_owned(),
                kind,
                ip4,
                ip6,
            }
        }
    };

    with_store(uid, now, true, |saved| {
        for (info, verdict, status) in known {
            let key = store::node_key(&info);
            saved.remember_node(&key, &info);
            let word = match verdict {
                Verdict::Ok => "ok",
                Verdict::Frozen => "frozen",
                Verdict::Dead => "dead",
            };
            saved.add_freeze(&place, now, &key, word, status);
        }
        ((), true)
    })
    .await;
}

/// Случайная метка установки: ею прослойка различает устройства, когда
/// опознание устройства (x-hwid) выключено.
fn device_id() -> String {
    let Ok(path) = dirs::app_home_dir().map(|dir| dir.join(DEVICE_FILE)) else {
        return String::new();
    };
    if let Ok(text) = std::fs::read_to_string(&path) {
        let text = text.trim();
        if text.len() == 32 && text.bytes().all(|b| b.is_ascii_hexdigit()) {
            return text.to_owned();
        }
    }
    let mut raw = [0u8; 16];
    if getrandom::fill(&mut raw).is_err() {
        return String::new();
    }
    let id = hex::encode(raw);
    if let Err(err) = std::fs::write(&path, &id) {
        logging!(warn, Type::Core, "[Report] the device mark was not saved: {err}");
    }
    id
}

fn gzip(bytes: &[u8]) -> std::io::Result<Vec<u8>> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
    encoder.write_all(bytes)?;
    encoder.finish()
}

/// Сжатый отчёт, граница отправленного (часы раньше неё) и число записей в нём.
/// Окно — от самого старого закрытого часа на [`SEND_WINDOW`], но не дальше
/// текущего часа; не влезло в [`REPORT_MAX_GZ`] — окно уполовинивается.
fn pack_window(saved: &store::Store, now: i64, oldest: i64, dev: &str) -> std::io::Result<(Vec<u8>, i64, usize)> {
    pack_within(saved, now, oldest, dev, REPORT_MAX_GZ, |report| {
        serde_json::to_vec(report)
            .map_err(std::io::Error::other)
            .and_then(|json| gzip(&json))
    })
}

fn pack_within(
    saved: &store::Store,
    now: i64,
    oldest: i64,
    dev: &str,
    limit: usize,
    pack: impl Fn(&serde_json::Value) -> std::io::Result<Vec<u8>>,
) -> std::io::Result<(Vec<u8>, i64, usize)> {
    let current = store::hour_of(now);
    let mut hours = SEND_WINDOW / store::HOUR;
    loop {
        let until = (oldest + hours * store::HOUR).min(current);
        let gz = pack(&saved.report(now, until, dev))?;
        if gz.len() <= limit || hours <= 1 {
            return Ok((gz, until, saved.hours_before(until)));
        }
        hours = (hours / 2).max(1);
    }
}

/// Плановое обновление подписки удалось: если подошло время, отправить отчёт.
/// Зовётся только для обновления по расписанию.
pub(crate) async fn after_scheduled_update(uid: String) {
    let Some((url, option, spare)) = Config::profiles()
        .await
        .latest_arc()
        .get_item(&uid)
        .ok()
        .filter(|item| is_collected(item))
        .and_then(|item| {
            let url = item.url.clone()?;
            // Ключ прослойки ещё не закреплён — отчёту не с чем идти.
            item.option.as_ref()?.chan_pin.as_ref()?;
            let spare = item
                .new_sub
                .as_deref()
                .and_then(|domain| crate::config::sub_headers::spare_address(&url, domain));
            Some((url, item.fetch_option(), spare))
        })
    else {
        return;
    };

    let now = help::now_secs();
    let packed = with_store(&uid, now, false, |saved| {
        let pruned = saved.prune(now);
        let packed = if now.saturating_sub(saved.last_try) < SEND_EVERY {
            None
        } else {
            saved
                .oldest_closed(now)
                .map(|oldest| pack_window(saved, now, oldest, &device_id()))
        };
        (packed, pruned)
    })
    .await;
    let (gz, until, hours) = match packed {
        None => return,
        Some(Ok(packed)) => packed,
        Some(Err(err)) => {
            logging!(warn, Type::Core, "[Report] the report was not packed: {err}");
            return;
        }
    };

    let mut sent = crate::config::send_report(&url, option.as_ref(), &gz).await;
    if sent.is_err()
        && let Some(spare) = spare
    {
        sent = crate::config::send_report(&spare, option.as_ref(), &gz).await;
    }
    let status = match sent {
        Ok(status) => status,
        Err(err) => {
            logging!(
                info,
                Type::Core,
                "[Report] the middleware did not answer over the secure channel, next try with the next scheduled update: {}",
                crate::utils::help::mask_err(&err.to_string())
            );
            return;
        }
    };

    with_store(&uid, now, true, |saved| {
        saved.last_try = now;
        if status == 204 {
            saved.drop_sent(now, until);
        }
        ((), true)
    })
    .await;
    let outcome = match status {
        204 => "accepted",
        403 => "the middleware does not take reports",
        429 => "too early for the middleware",
        _ => "the middleware does not know reports",
    };
    logging!(
        info,
        Type::Core,
        "[Report] {hours} hour(s) of measurements sent: {outcome} ({status})"
    );
}

/// Сборщик истории задержек: от раза в 20 секунд до раза в 5 минут (первое
/// чтение сразу, чтобы место узналось). Соединения раз в 10 секунд приносит
/// общий опрос ([`count_traffic`]). На выходе тик пропускается, а не кончает
/// цикл: после отменённого выхода сбор идёт дальше сам.
pub fn spawn() {
    AsyncHandler::spawn(|| async {
        let mut next = TICK_MIN;
        loop {
            tokio::time::sleep(next).await;
            if handle::Handle::global().is_exiting() {
                continue;
            }
            tick().await;
            next = with_runtime(|runtime| runtime.next).unwrap_or(TICK);
        }
    });
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::time::Duration;

    use super::store::{HOUR, Place, Store, hour_of};
    use super::{Address, REPORT_MAX_GZ, SEND_WINDOW, TICK, TICK_MIN, histories, next_read, pack_within, pings_of};

    fn ms(text: &str) -> i64 {
        chrono::DateTime::parse_from_rfc3339(text)
            .map(|t| t.timestamp_millis())
            .unwrap_or(0)
    }

    fn wanted(names: &[&str]) -> HashMap<String, Address> {
        names
            .iter()
            .map(|name| ((*name).to_owned(), Address::default()))
            .collect()
    }

    #[test]
    fn the_window_is_counted_in_milliseconds_and_takes_only_nodes() {
        let proxies = serde_json::json!({"proxies": {
            "Узел": {"history": [
                {"time": "2026-10-04T10:00:00.5+03:00", "delay": 120},
                {"time": "2026-10-04T10:04:00.250Z", "delay": 0},
                {"time": "2026-10-04T10:04:00.750Z", "delay": 80},
                {"time": "2026-10-04T10:06:00Z", "delay": 80}
            ]},
            "Группа": {"all": ["Узел"], "history": [{"time": "2026-10-04T10:04:30Z", "delay": 10}]}
        }});
        let listed = histories(&proxies, None, &wanted(&["Узел"]));
        let since = ms("2026-10-04T07:00:00.5Z");
        let until = ms("2026-10-04T10:04:00.5Z");
        let got = pings_of(&listed, since, until);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].0, "Узел");
        assert_eq!(got[0].2, 0);
        // Следующее окно начинается ровно там, где кончилось это: замер через
        // полсекунды не теряется и не считается дважды.
        let next = pings_of(&listed, until, ms("2026-10-04T10:05:00Z"));
        assert_eq!(next.len(), 1);
        assert_eq!(next[0].2, 80);
    }

    #[test]
    fn provider_nodes_are_read_from_the_providers_listing() {
        // В `/proxies` узлов провайдера нет — только в `/providers/proxies`.
        let proxies =
            serde_json::json!({"proxies": {"Свой": {"history": [{"time": "2026-10-04T10:00:00Z", "delay": 5}]}}});
        let providers = serde_json::json!({"providers": {
            "panel": {"proxies": [
                {"name": "Чужой", "history": [{"time": "2026-10-04T10:00:01Z", "delay": 7}]},
                {"name": "Свой", "history": [{"time": "2026-10-04T10:00:02Z", "delay": 9}]}
            ]},
            "default": {"proxies": [{"name": "Группа", "history": []}]}
        }});
        let listed = histories(&proxies, Some(&providers), &wanted(&["Свой", "Чужой"]));
        assert_eq!(listed.len(), 2);
        // Одноимённый узел подписки первее провайдерского.
        assert_eq!(listed["Свой"][0].1, 5);
        assert_eq!(listed["Чужой"][0].1, 7);
    }

    #[test]
    fn a_node_checked_often_is_read_often_enough() {
        let history: Vec<_> = (0..10)
            .map(|i| serde_json::json!({"time": format!("2026-10-04T10:0{i}:00Z"), "delay": 50}))
            .collect();
        let proxies = serde_json::json!({"proxies": {
            "Частый": {"history": history},
            "Редкий": {"history": [{"time": "2026-10-04T10:00:00Z", "delay": 50}]}
        }});
        let listed = histories(&proxies, None, &wanted(&["Частый", "Редкий"]));
        // Десять замеров за 9 минут — читать раз в 4,5 минуты. Прошлое чтение
        // застало самый старый из десяти — ничего не ушло.
        let (next, lost) = next_read(&listed, ms("2026-10-04T10:00:30Z"));
        assert_eq!(next, Duration::from_secs(270));
        assert_eq!(lost, 0);
        // Прошлое чтение было раньше самого старого из десяти — часть могла уйти.
        let (_, lost) = next_read(&listed, ms("2026-10-04T09:59:00Z"));
        assert_eq!(lost, 1);

        let dense: Vec<_> = (0..10)
            .map(|i| serde_json::json!({"time": format!("2026-10-04T10:00:0{i}Z"), "delay": 50}))
            .collect();
        let proxies = serde_json::json!({"proxies": {"Частый": {"history": dense}}});
        assert_eq!(
            next_read(&histories(&proxies, None, &wanted(&["Частый"])), 0).0,
            TICK_MIN
        );
        assert_eq!(next_read(&HashMap::new(), 0).0, TICK);
    }

    fn production() -> &'static str {
        crate::utils::source_scan::without_test_modules(include_str!("mod.rs"))
    }

    #[test]
    fn nodes_and_providers_come_from_the_shared_readers() {
        let source = production();
        // Узлы — из общего разбора сборки (имена как у ядра, с приставками
        // провайдеров), а не своим обходом файлов подписки.
        assert!(source.contains("proxy_label::addresses("), "общий разбор узлов");
        for copy in [
            "serde_yaml_ng",
            "fn node_of",
            "fn provider_file",
            "provider_files",
            "md5",
        ] {
            assert!(!source.contains(copy), "своя копия разбора узлов: {copy}");
        }
        // Ядро — общим читателем, а не своим помощником запросов.
        assert!(source.contains("feat::read_core_proxies("));
        assert!(!source.contains("build_request"), "свой помощник запросов к ядру");
    }

    #[test]
    fn the_store_change_comes_from_the_work_itself() {
        use crate::utils::source_scan::fn_body;
        let source = production();
        let with_store = fn_body(source, "async fn with_store").unwrap_or_default();
        assert!(!with_store.is_empty());
        assert!(
            !with_store.contains("store.clone()") && !with_store.contains("!= before"),
            "накопленное не копируется ради признака: {with_store}"
        );
        let record = fn_body(source, "async fn record(").unwrap_or_default();
        assert!(!record.is_empty());
        assert!(
            !record.contains("prune("),
            "чистка — раз в час и перед записью: {record}"
        );
    }

    #[test]
    fn the_collector_skips_ticks_while_exiting_instead_of_stopping() {
        use crate::utils::source_scan::fn_body;
        let spawn = fn_body(production(), "pub fn spawn()").unwrap_or_default();
        let the_loop = fn_body(spawn, "loop {").unwrap_or_default();
        assert!(the_loop.contains("is_exiting()"), "{the_loop}");
        assert!(
            !spawn.contains("return"),
            "после отменённого выхода сбор идёт дальше: {spawn}"
        );
    }

    fn backlog(hours: i64, now: i64) -> Store {
        let mut store = Store::default();
        let place = Place {
            net: "n".into(),
            kind: "wifi",
            ip4: "203.0.113.7".into(),
            ip6: String::new(),
        };
        for i in 1..=hours {
            store.add_ping(&place, hour_of(now) - i * HOUR, "k", 100);
        }
        store
    }

    #[test]
    fn a_report_takes_the_oldest_two_days_and_nothing_newer_than_the_closed_hour() {
        let now = 1_800_000_000;
        let store = backlog(7 * 24, now);
        let oldest = store.oldest_closed(now).unwrap_or(0);
        let size = |report: &serde_json::Value| Ok(report.to_string().into_bytes());
        let (_, until, hours) = pack_within(&store, now, oldest, "", REPORT_MAX_GZ, size).unwrap_or_default();
        assert_eq!(until, oldest + SEND_WINDOW);
        assert_eq!(hours, 48);

        // Завал меньше окна — граница на текущем часе, он сам не уходит.
        let small = backlog(3, now);
        let oldest = small.oldest_closed(now).unwrap_or(0);
        let (_, until, hours) = pack_within(&small, now, oldest, "", REPORT_MAX_GZ, size).unwrap_or_default();
        assert_eq!(until, hour_of(now));
        assert_eq!(hours, 3);
    }

    #[test]
    fn a_report_that_does_not_fit_shrinks_its_window() {
        let now = 1_800_000_000;
        let store = backlog(7 * 24, now);
        let oldest = store.oldest_closed(now).unwrap_or(0);
        // «Сжатие» — число записей часов: лимит в 10 записей.
        let count = |report: &serde_json::Value| {
            Ok(vec![
                0u8;
                report["networks"]["n"]["hours"].as_array().map_or(0, Vec::len)
                    * 100
            ])
        };
        let (gz, until, hours) = pack_within(&store, now, oldest, "", 1000, count).unwrap_or_default();
        assert!(
            gz.len() <= 1000 && hours <= 10,
            "{hours} записей, «сжато» до {}",
            gz.len()
        );
        assert_eq!(hours, 6);
        assert_eq!(until, oldest + 6 * HOUR);

        // Даже одна запись не влезает — уходит всё равно она одна, не бесконечный цикл.
        let (_, until, hours) = pack_within(&store, now, oldest, "", 1, count).unwrap_or_default();
        assert_eq!(hours, 1);
        assert_eq!(until, oldest + HOUR);
    }
}
