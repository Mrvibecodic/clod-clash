//! clod:report — отчёт прослойке о качестве узлов.
//!
//! Копится только у текущей подписки с защищённым каналом, пока прослойка
//! принимает отчёты: о приёме она говорит меткой в ответе подписки по каналу
//! ([`PrfItem::report`]); сказала «нет» — накопленное стирается. Сборщик берёт то,
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
//! считается дважды. Окно кончается на [`LAG`] раньше чтения: часть замеров
//! ядро кладёт в историю позже, чем помечает.
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
//! обновления подписки, а если она обновляется реже раза в 6 часов, — и между
//! обновлениями ([`send_between`]), и не чаще раза в 6 часов,
//! закрытыми часами — от самых старых, не больше двух суток за раз и не
//! больше, чем примет прослойка. Закрытый час — тот, в который уже ничего не
//! ляжет: перед отправкой сборщик дочитывает окно, и граница — час прочитанного.
//! Велик (413) — окно тут же уполовинивается; час, который не влез и один,
//! отбрасывается.
//! Принят (204) — отправленное удаляется, остальное ждёт следующего раза. Приём выключен в прослойке (403), рано (429)
//! или прослойка старая и ответила подпиской — накопленное остаётся, следующая
//! попытка через 6 часов. Каналом не ответил никто — попытка при следующей
//! отправке.

mod ip;
mod store;

use std::collections::{BTreeMap, HashMap, HashSet};
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
/// Окно замеров кончается на столько (мс) раньше чтения. Clod Core кладёт
/// неудачу пробы, которую спасла повторная, в историю, когда та ответила, а
/// помечает началом первой: позже не больше чем на 1,25 с и тайм-аут проверки
/// группы (5 с по умолчанию). Больше не надо: история узла — 10 записей, и
/// лишнее отставание теряло бы замеры у часто проверяемых узлов.
const LAG: i64 = 20 * 1000;
/// Снимок соединений (его приносит общий опрос, `core::connections_poll`):
/// байты по узлам. Ядро отдаёт только живые соединения, закрытое между
/// чтениями теряется целиком — поэтому часто: теряется лишь хвост последних
/// секунд каждого соединения.
const TRAFFIC_TICK: Duration = Duration::from_secs(5);
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
/// Отправки (после обновления и между обновлениями) идут по одной.
static SENDING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
/// Подписка → когда (с) снова пробовать отправить между обновлениями, см. [`send_between`].
static NEXT: Mutex<BTreeMap<String, i64>> = Mutex::new(BTreeMap::new());

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
    /// Часы ушли назад — спросить сейчас, а не ждать, пока догонят.
    const fn ip_is_due(&self, now: i64) -> bool {
        let unknown = self.place.ip4.is_empty() && self.place.ip6.is_empty();
        self.ip_at > now || now.saturating_sub(self.ip_at) >= if unknown { IP_RETRY } else { IP_EVERY }
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
/// Подписка, по которой отчёт больше не копится (удалена, прослойка не
/// принимает отчёты, канал выключен), файла не получает, а прежний теряет.
async fn flush(entry: &mut Cached, now: i64) {
    if !entry.dirty {
        return;
    }
    entry.store.prune(now);
    entry.pruned_hour = store::hour_of(now);
    let collected = Config::profiles()
        .await
        .latest_arc()
        .get_item(&entry.uid)
        .is_ok_and(is_collected);
    let written = if collected {
        store::save(&entry.uid, &entry.store).await
    } else {
        store::remove(&entry.uid).await
    };
    if let Err(err) = written {
        logging!(warn, Type::Core, "[Report] the measurements were not written: {err:#}");
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

/// Копится ли отчёт по подписке: удалённая, с защищённым каналом — только им
/// отчёт и может уйти — и прослойка не сказала, что отчёты не принимает. Метки
/// не было (прослойка старее или подписка обновлялась до неё) — копится.
fn is_collected(item: &PrfItem) -> bool {
    item.itype.as_deref() == Some("remote")
        && item.option.as_ref().is_some_and(|o| o.secure == Some(true))
        && item.report != Some(false)
}

/// Текущая подписка, если по ней копится отчёт.
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
/// провайдеров подписки (узлы провайдеров — в `/proxies` их нет) — у того же
/// узла, чей адрес у `wanted`: одноимённый узел другого провайдера не в счёт.
fn histories(
    proxies: &serde_json::Value,
    providers: Option<&serde_json::Value>,
    wanted: &HashMap<String, Address>,
) -> HashMap<String, Vec<(i64, u64)>> {
    let mut out = HashMap::new();
    if let Some(map) = proxies.get("proxies").and_then(serde_json::Value::as_object) {
        for (name, entry) in map {
            if wanted.get(name).is_some_and(|address| address.provider.is_empty()) {
                out.insert(name.clone(), history_of(entry));
            }
        }
    }
    let listed = providers
        .and_then(|providers| providers.get("providers"))
        .and_then(serde_json::Value::as_object);
    for (provider, listing) in listed.into_iter().flatten() {
        let Some(members) = listing.get("proxies").and_then(serde_json::Value::as_array) else {
            continue;
        };
        for member in members {
            let Some(name) = member.get("name").and_then(serde_json::Value::as_str) else {
                continue;
            };
            if wanted.get(name).is_some_and(|address| address.provider == *provider) && !out.contains_key(name) {
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
/// полной историей десять замеров уложились в `span`, а окно отстаёт на
/// [`LAG`] — читать вдвое чаще, чем `span` без него.
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
        let half = Duration::from_millis(u64::try_from((newest - oldest - LAG) / 2).unwrap_or(0));
        next = next.min(half);
    }
    (next.clamp(TICK_MIN, TICK), overflowed)
}

/// Сколько ждать между снимками соединений: [`TRAFFIC_TICK`], пока по
/// текущей подписке копится отчёт; `None` — снимки не нужны.
pub(crate) async fn traffic_every() -> Option<Duration> {
    traffic_every_while(collecting_uid().await.is_some())
}

pub(crate) fn traffic_every_while(collecting: bool) -> Option<Duration> {
    collecting.then_some(TRAFFIC_TICK)
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

/// Прочитать у ядра замеры с прошлого чтения по `until` (мс; окно назад не
/// сдвигается) и забрать накопленный трафик. `None` — ядро не ответило: окно
/// не сдвигается, замеры дождутся следующего чтения.
async fn read_window(uid: &str, until: i64) -> Option<Window> {
    let nodes = crate::config::proxy_label::addresses(uid)
        .await
        .filter(|nodes| !nodes.is_empty())?;
    // Сбойный провайдер пропускается: его замеры дождутся следующего чтения.
    let (proxies, providers, _failed) = crate::feat::read_core_proxies(CORE_TIMEOUT).await.into_json()?;
    let listed = histories(&proxies, Some(&providers), &nodes);
    let since = with_runtime(|runtime| runtime.pings_until);
    let until = until.max(since);
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

/// Прочитать окно (по [`LAG`] назад от нынешнего) и разложить в место; под
/// замком сборщика. Что делать с адресом — решает вызвавший.
async fn collect() -> IpWork {
    let Some(uid) = collecting_uid().await else {
        with_runtime(|runtime| *runtime = Runtime::default());
        forget_store().await;
        return IpWork::None;
    };
    start_over_for(&uid);
    let now = now_millis();
    // Часы ушли назад: окно — от нынешнего момента, иначе замеров не было бы,
    // пока часы не догонят прежнее.
    with_runtime(|runtime| runtime.pings_until = runtime.pings_until.min(now));
    let Some(window) = read_window(&uid, now - LAG).await else {
        return IpWork::None;
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
}

async fn tick() {
    let work = {
        let _collect = COLLECT.lock().await;
        collect().await
    };
    settle(work).await;
}

/// Узнать место или переспросить адрес места — уже без замка сборщика.
async fn settle(work: IpWork) {
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
/// в старое место, последние [`CHANGE_GUARD_MS`] (не меньше [`LAG`]) перед ней отбрасываются, новое
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
    // Раньше среза история уже полна: позже помеченное ядро могло ещё не положить.
    let cut = seen_at - CHANGE_GUARD_MS.max(LAG);
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

/// Проверка 16–20 вынесла вердикты в сети `net`: в отчёт, если по этой подписке
/// он копится. Вердикт ложится в час, когда записан: в отправленный час уже
/// ничего не ложится.
pub(crate) async fn note_freeze(uid: &str, net: &str, kind: &'static str, verdicts: &[(String, Verdict, i64)]) {
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

    with_store(uid, help::now_secs(), true, |saved| {
        let now = help::now_secs();
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

/// Отчёт к отправке: сжатый, граница отправленного (часы раньше неё), сколько
/// часов он охватывает и сколько в нём записей часов.
struct Packed {
    gz: Vec<u8>,
    until: i64,
    span: i64,
    records: usize,
}

/// Отчёт из закрытых часов (раньше `closed`): от самого старого на `hours`
/// часов; не влез в [`REPORT_MAX_GZ`] — окно уполовинивается. `None` —
/// отправлять нечего.
fn pack_window(saved: &store::Store, now: i64, closed: i64, hours: i64, dev: &str) -> Option<std::io::Result<Packed>> {
    pack_within(saved, now, closed, hours, dev, REPORT_MAX_GZ, |report| {
        serde_json::to_vec(report)
            .map_err(std::io::Error::other)
            .and_then(|json| gzip(&json))
    })
}

fn pack_within(
    saved: &store::Store,
    now: i64,
    closed: i64,
    hours: i64,
    dev: &str,
    limit: usize,
    pack: impl Fn(&serde_json::Value) -> std::io::Result<Vec<u8>>,
) -> Option<std::io::Result<Packed>> {
    let oldest = saved.oldest_before(closed)?;
    let mut hours = hours.max(1);
    Some(loop {
        let until = (oldest + hours * store::HOUR).min(closed);
        let gz = match pack(&saved.report(now, until, dev)) {
            Ok(gz) => gz,
            Err(err) => break Err(err),
        };
        let span = (until - oldest) / store::HOUR;
        if gz.len() <= limit || span <= 1 {
            break Ok(Packed {
                gz,
                until,
                span,
                records: saved.hours_before(until),
            });
        }
        hours = (span / 2).max(1);
    })
}

/// Граница закрытых часов подписки: в часы раньше неё уже ничего не ляжет.
/// Замер задержки ложится в час, когда сделан, но не раньше прочитанного окна
/// (`pings_until`), всё прочее — в час, когда записано. Окно прочитано по
/// подписке, которую собирают, — граница по нему; по остальным — нынешний час.
fn closed_before(uid: &str, now: i64) -> i64 {
    let read = with_runtime(|runtime| (runtime.uid.as_deref() == Some(uid)).then_some(runtime.pings_until / 1000));
    store::hour_of(read.map_or(now, |read| read.min(now)))
}

/// Подписка обновилась. Отчёт по ней больше не копится (прослойка сказала, что
/// не принимает, канал выключили) — накопленное стирается; обновление плановое —
/// отчёт уходит, если подошло время.
pub(crate) async fn after_update(uid: String, scheduled: bool) {
    let collected = Config::profiles()
        .await
        .latest_arc()
        .get_item(&uid)
        .is_ok_and(is_collected);
    if !collected {
        forget(&uid).await;
        return;
    }
    if scheduled {
        let at = send(&uid).await;
        NEXT.lock().insert(uid, at);
    }
}

/// Накопленное подписки — вон из памяти и с диска.
async fn forget(uid: &str) {
    let mut guard = CACHE.lock().await;
    if guard.as_ref().is_some_and(|entry| entry.uid == uid) {
        *guard = None;
    }
    if let Err(err) = store::remove(uid).await {
        logging!(warn, Type::Core, "[Report] the measurements were not removed: {err:#}");
    }
    drop(guard);
}

/// Обновления подписки реже, чем отчёт может уходить, и последнее удалось.
fn sends_between(item: &PrfItem) -> bool {
    let option = item.option.as_ref();
    is_collected(item)
        && item.update_failed != Some(true)
        && option.and_then(|option| option.allow_auto_update).unwrap_or(true)
        && option
            .and_then(|option| option.update_interval)
            .is_some_and(|minutes| i64::try_from(minutes).unwrap_or(i64::MAX).saturating_mul(60) > SEND_EVERY)
}

/// Подписки, которым пора попробовать отправить между обновлениями: срок из
/// [`NEXT`] подошёл (или его нет — после запуска), накопленное есть на диске.
/// Срок ставится сразу, чтобы следующий круг не позвал отправку повторно.
fn due_between(next: &mut BTreeMap<String, i64>, wanted: &[(String, bool)], now: i64) -> Vec<String> {
    next.retain(|uid, _| wanted.iter().any(|(wanted, _)| wanted == uid));
    let mut due = Vec::new();
    for (uid, stored) in wanted {
        // Срок дальше, чем ставится, — часы ушли назад: пора.
        if *stored && next.get(uid).is_none_or(|at| *at <= now || *at > now + SEND_EVERY) {
            next.insert(uid.clone(), now + SEND_EVERY);
            due.push(uid.clone());
        }
    }
    due
}

/// Отправка между редкими обновлениями. Решают сохранённое — отметка о
/// неудачном обновлении в подписке и время прошлой попытки в накопленном, —
/// поэтому перезапуск приложения отправкам не мешает; в памяти только срок
/// следующей проверки, чтобы не читать накопленное каждый круг. Сети нет —
/// попытка, как только она появится (проверка раз в [`TICK`]).
async fn send_between() {
    let wanted: Vec<(String, bool)> = Config::profiles()
        .await
        .latest_arc()
        .get_items()
        .into_iter()
        .flatten()
        .filter(|item| sends_between(item))
        .filter_map(|item| item.uid.as_deref().map(str::to_owned))
        .map(|uid| {
            let stored = store::exists(&uid);
            (uid, stored)
        })
        .collect();
    let due = due_between(&mut NEXT.lock(), &wanted, help::now_secs());
    for uid in due {
        AsyncHandler::spawn(move || async move {
            let at = if crate::module::freeze_check::current_network().await.is_some() {
                send(&uid).await
            } else {
                help::now_secs() + TICK.as_secs().cast_signed()
            };
            NEXT.lock().insert(uid, at);
        });
    }
}

/// Если подошло время, отчёт подписки уходит. Возвращает, когда пробовать
/// снова: через 6 часов от прошлой попытки или от этой.
async fn send(uid: &str) -> i64 {
    let _sending = SENDING.lock().await;
    let Some((url, option, spare)) = Config::profiles()
        .await
        .latest_arc()
        .get_item(uid)
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
        return help::now_secs() + SEND_EVERY;
    };

    let now = help::now_secs();
    let early = with_store(uid, now, false, |saved| {
        (too_early(saved.last_try, now), saved.prune(now))
    })
    .await;
    if let Some(at) = early {
        return at;
    }
    // Сборщик дочитывает окно: граница закрытых часов — по прочитанному.
    let (closed, work) = {
        let _collect = COLLECT.lock().await;
        let work = collect().await;
        (closed_before(uid, now), work)
    };
    settle(work).await;

    let dev = device_id();
    let mut hours = SEND_WINDOW / store::HOUR;
    loop {
        let packed = with_store(uid, now, false, |saved| {
            (pack_window(saved, now, closed, hours, &dev), false)
        })
        .await;
        let packed = match packed {
            None => return now + SEND_EVERY,
            Some(Ok(packed)) => packed,
            Some(Err(err)) => {
                logging!(warn, Type::Core, "[Report] the report was not packed: {err}");
                return now + SEND_EVERY;
            }
        };

        let mut sent = crate::config::send_report(&url, option.as_ref(), &packed.gz).await;
        if sent.is_err()
            && let Some(spare) = spare.as_deref()
        {
            sent = crate::config::send_report(spare, option.as_ref(), &packed.gz).await;
        }
        let status = match sent {
            Ok(status) => status,
            Err(err) => {
                logging!(
                    info,
                    Type::Core,
                    "[Report] the middleware did not answer over the secure channel, next try with the next send: {}",
                    crate::utils::help::mask_err(&err.to_string())
                );
                return now + SEND_EVERY;
            }
        };
        // Велик — тут же вдвое меньшее окно.
        if status == 413 && packed.span > 1 {
            hours = packed.span / 2;
            continue;
        }
        // Час, не влезший и один, отброшен ниже — дальше снова полное окно.
        if status == 413 {
            hours = SEND_WINDOW / store::HOUR;
        }

        with_store(uid, now, true, |saved| {
            saved.last_try = now;
            // Час, который не влез и один, не уйдёт никогда — он отбрасывается.
            if status == 204 || status == 413 {
                saved.drop_sent(now, packed.until);
            }
            ((), true)
        })
        .await;
        let outcome = match status {
            204 => "accepted",
            403 => "the middleware does not take reports",
            413 => "too big for the middleware even alone, dropped",
            429 => "too early for the middleware",
            _ => "the middleware does not know reports",
        };
        logging!(
            info,
            Type::Core,
            "[Report] {} hour(s) of measurements sent: {outcome} ({status})",
            packed.records
        );
        if status != 413 {
            return now + SEND_EVERY;
        }
    }
}

/// Рано ли отправлять: меньше 6 часов с прошлого ответа прослойки — тогда
/// когда. Часы ушли назад (прошлый ответ «в будущем») — не рано.
const fn too_early(last_try: i64, now: i64) -> Option<i64> {
    if last_try <= now && now - last_try < SEND_EVERY {
        Some(last_try + SEND_EVERY)
    } else {
        None
    }
}

/// Сборщик истории задержек: от раза в 20 секунд до раза в 5 минут (первое
/// чтение сразу, чтобы место узналось). Соединения раз в 5 секунд приносит
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
            send_between().await;
            next = with_runtime(|runtime| runtime.next).unwrap_or(TICK);
        }
    });
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::time::Duration;

    use super::store::{HOUR, Place, Store, hour_of};
    use super::{
        Address, Packed, REPORT_MAX_GZ, SEND_EVERY, SEND_WINDOW, Spot, TICK, TICK_MIN, closed_before, due_between,
        histories, next_read, pack_within, pings_of, sends_between, too_early, with_runtime,
    };
    use crate::config::{PrfItem, PrfOption};

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
            // Одноимённый узел провайдера, чьего адреса у отчёта нет (имена
            // переписаны), — не тот узел.
            "a-renamed": {"proxies": [{"name": "Чужой", "history": [{"time": "2026-10-04T10:00:03Z", "delay": 99}]}]},
            "default": {"proxies": [{"name": "Группа", "history": []}]}
        }});
        let mut wanted = wanted(&["Свой", "Чужой"]);
        if let Some(address) = wanted.get_mut("Чужой") {
            address.provider = "panel".into();
        }
        let listed = histories(&proxies, Some(&providers), &wanted);
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
        // Десять замеров за 9 минут, окно отстаёт на 20 с — читать раз в
        // 4 мин 20 с. Прошлое чтение застало самый старый из десяти — ничего не ушло.
        let (next, lost) = next_read(&listed, ms("2026-10-04T10:00:30Z"));
        assert_eq!(next, Duration::from_secs(260));
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

    /// Граница отправленного, сколько часов охвачено и записей — или ничего.
    fn packed(
        store: &Store,
        now: i64,
        closed: i64,
        hours: i64,
        limit: usize,
        size: impl Fn(&serde_json::Value) -> Vec<u8>,
    ) -> Option<(i64, i64, usize)> {
        pack_within(store, now, closed, hours, "", limit, |report| Ok(size(report)))
            .and_then(Result::ok)
            .map(
                |Packed {
                     until, span, records, ..
                 }| (until, span, records),
            )
    }

    fn plain(report: &serde_json::Value) -> Vec<u8> {
        report.to_string().into_bytes()
    }

    #[test]
    fn a_report_takes_the_oldest_two_days_and_nothing_from_the_open_hours() {
        let now = 1_800_000_000;
        let store = backlog(7 * 24, now);
        let oldest = store.oldest_before(hour_of(now)).unwrap_or(0);
        let hours = SEND_WINDOW / HOUR;
        assert_eq!(
            packed(&store, now, hour_of(now), hours, REPORT_MAX_GZ, plain),
            Some((oldest + SEND_WINDOW, hours, 48))
        );

        // Завал меньше окна — граница на закрытых часах, остальное не уходит.
        let small = backlog(3, now);
        assert_eq!(
            packed(&small, now, hour_of(now), hours, REPORT_MAX_GZ, plain),
            Some((hour_of(now), 3, 3))
        );
        // Граница раньше нынешнего часа (окно ещё не дочитано) — час перед ней не уходит.
        assert_eq!(
            packed(&small, now, hour_of(now) - HOUR, hours, REPORT_MAX_GZ, plain),
            Some((hour_of(now) - HOUR, 2, 2))
        );
        // Закрытых часов нет — отправлять нечего.
        assert_eq!(
            packed(&small, now, hour_of(now) - 3 * HOUR, hours, REPORT_MAX_GZ, plain),
            None
        );
        // Окно задано меньше — столько часов и уходит (прослойка сказала «велик»).
        assert_eq!(
            packed(&store, now, hour_of(now), 6, REPORT_MAX_GZ, plain),
            Some((oldest + 6 * HOUR, 6, 6))
        );
    }

    #[test]
    fn a_report_that_does_not_fit_shrinks_its_window() {
        let now = 1_800_000_000;
        let store = backlog(7 * 24, now);
        let oldest = store.oldest_before(hour_of(now)).unwrap_or(0);
        // «Сжатие» — число записей часов: лимит в 10 записей.
        let count = |report: &serde_json::Value| {
            vec![0u8; report["networks"]["n"]["hours"].as_array().map_or(0, Vec::len) * 100]
        };
        assert_eq!(
            packed(&store, now, hour_of(now), SEND_WINDOW / HOUR, 1000, count),
            Some((oldest + 6 * HOUR, 6, 6))
        );
        // Даже одна запись не влезает — уходит всё равно она одна, не бесконечный цикл.
        assert_eq!(
            packed(&store, now, hour_of(now), SEND_WINDOW / HOUR, 1, count),
            Some((oldest + HOUR, 1, 1))
        );

        // Окно больше завала уполовинивается от того, что охвачено, а не от заданного.
        let small = backlog(3, now);
        let shrunk = packed(&small, now, hour_of(now), SEND_WINDOW / HOUR, 250, count);
        assert_eq!(shrunk, Some((hour_of(now) - 2 * HOUR, 1, 1)));
    }

    #[test]
    fn hours_are_closed_by_what_the_collector_has_read() {
        let now = 1_800_000_000;
        let read = (hour_of(now) - HOUR + 600) * 1000;
        with_runtime(|runtime| {
            runtime.uid = Some("собираемая".into());
            runtime.pings_until = read;
        });
        assert_eq!(closed_before("собираемая", now), hour_of(now) - HOUR);
        assert_eq!(closed_before("другая", now), hour_of(now));
        // Прочитанное «впереди» нынешнего — часы ушли назад: граница по нынешнему.
        with_runtime(|runtime| runtime.pings_until = (now + 2 * HOUR) * 1000);
        assert_eq!(closed_before("собираемая", now), hour_of(now));
        with_runtime(|runtime| *runtime = super::Runtime::default());
    }

    #[test]
    fn a_clock_turned_back_does_not_hold_up_sends_or_the_address() {
        let now = 1_800_000_000;
        assert_eq!(too_early(now - 60, now), Some(now - 60 + SEND_EVERY));
        assert_eq!(too_early(now - SEND_EVERY, now), None);
        assert_eq!(too_early(0, now), None);
        // Прошлый ответ «в будущем» — часы ушли назад: не рано.
        assert_eq!(too_early(now + 3 * HOUR, now), None);

        let mut next = std::collections::BTreeMap::new();
        next.insert("a".to_owned(), now + 3 * SEND_EVERY);
        assert_eq!(
            due_between(&mut next, &[("a".to_owned(), true)], now),
            vec!["a".to_owned()]
        );

        let spot = |ip4: &str, ip_at| Spot {
            place: Place {
                net: "n".into(),
                kind: "wifi",
                ip4: ip4.into(),
                ip6: String::new(),
            },
            changes: 0,
            ip_at,
        };
        assert!(!spot("203.0.113.7", now - 60).ip_is_due(now));
        assert!(spot("203.0.113.7", now - HOUR).ip_is_due(now));
        assert!(spot("203.0.113.7", now + HOUR).ip_is_due(now), "часы ушли назад");
    }

    fn subscription(secure: bool, minutes: Option<u64>, auto: Option<bool>) -> PrfItem {
        PrfItem {
            itype: Some("remote".into()),
            report: Some(true),
            option: Some(PrfOption {
                secure: Some(secure),
                update_interval: minutes,
                allow_auto_update: auto,
                ..PrfOption::default()
            }),
            ..PrfItem::default()
        }
    }

    #[test]
    fn only_rare_successful_updates_of_a_secure_subscription_send_between() {
        assert!(sends_between(&subscription(true, Some(12 * 60), None)));
        assert!(!sends_between(&subscription(true, Some(6 * 60), None)));
        assert!(!sends_between(&subscription(true, Some(12 * 60), Some(false))));
        assert!(!sends_between(&subscription(true, None, None)));
        assert!(!sends_between(&subscription(false, Some(12 * 60), None)));
        let failed = PrfItem {
            update_failed: Some(true),
            ..subscription(true, Some(12 * 60), None)
        };
        assert!(!sends_between(&failed));
        // Прослойка сказала, что отчёты не принимает, — отчёт не копится и не
        // уходит; метки не было — копится.
        let refused = PrfItem {
            report: Some(false),
            ..subscription(true, Some(12 * 60), None)
        };
        assert!(!sends_between(&refused));
        let unknown = PrfItem {
            report: None,
            ..subscription(true, Some(12 * 60), None)
        };
        assert!(sends_between(&unknown));
    }

    #[test]
    fn sends_between_updates_are_tried_once_per_term() {
        let mut next = std::collections::BTreeMap::new();
        let wanted = vec![("a".to_owned(), true), ("b".to_owned(), false)];

        // После запуска срока нет — проверка сразу; без накопленного — нет
        assert_eq!(due_between(&mut next, &wanted, 0), vec!["a".to_owned()]);
        // Срок поставлен сразу: следующий круг не зовёт отправку повторно
        assert!(due_between(&mut next, &wanted, 60).is_empty());
        assert_eq!(next.get("a"), Some(&SEND_EVERY));
        assert_eq!(due_between(&mut next, &wanted, SEND_EVERY), vec!["a".to_owned()]);

        // Подписка выпала (обновление не удалось, частые обновления) — срок снят
        assert!(due_between(&mut next, &[], SEND_EVERY).is_empty());
        assert!(next.is_empty());
    }
}
