//! clod:freeze — проверка 16–20: режется ли трафик через узел в этой сети.
//!
//! Механизм — в ядре Clod Core (`GET /proxies/{name}/download`: узел качает
//! 64 КБ и отвечает ok / frozen / dead / unknown). Здесь — когда звать,
//! хранение по подписке и сети и пометки «режется» / «не отвечает» для
//! интерфейса. С узлами клиент ничего не делает: пинг как был, выбор как был.
//!
//! Поводы захода: сменилась сеть; сменились подписка, её узлы или ядро;
//! подписка обновилась (панель могла включить или выключить проверку); ядро
//! впервые поднялось за сеанс; тик раз в час. Перезапуск ядра или туннеля с
//! той же сборкой поводом не служит. От подключения заход не зависит:
//! проверяется путь от адреса клиента до адреса сервера, туннель тут ни при
//! чём. Проверяются отпечатки без итога в текущей сети, «работает» и
//! «режется» старше 3 суток, «не отвечает» и попытки без итога старше 6 часов;
//! одинаковые узлы делят результат по отпечатку. Если посреди захода сменились
//! сеть или подписка или ядро не ответило, заход бросается. Если через
//! проверенные узлы не прошло ничего, заодно проверяется узел, рабочий в этой
//! сети: прошёл он — итоги в счёт; не прошёл и он (или такого нет) — это сеть,
//! заход не записывается, и эти узлы ждут 6 часов.
//! Ядро без отпечатков (чужое) — функция молчит. Включает проверку только
//! панель — заголовком подписки `clod-16-20-check: true`; без него ни
//! проверок, ни пометок.

mod network;
mod plan;
mod store;

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    time::Duration,
};

use clash_verge_logging::{Type, logging};
use futures::StreamExt as _;
use parking_lot::Mutex;
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use reqwest::Method;

pub(crate) use plan::Verdict;

use crate::{
    config::Config,
    core::{handle, sysopt::verbose_diagnostics, tray::Tray},
    process::AsyncHandler,
};

/// Что качаем через узел: 64 КБ нулей без сжатия.
const DOWNLOAD_URL: &str = "https://speed.cloudflare.com/__down?bytes=65536";
const DOWNLOAD_SIZE: u32 = 65_536;
const DOWNLOAD_TIMEOUT_MS: u32 = 10_000;
const DOWNLOAD_STALL_MS: u32 = 4_000;
/// Ядро ждёт очередь к хосту, качает, потом пингует: 3 + 10 + 5 + 3 с.
const REQUEST_BUDGET: Duration = Duration::from_secs(30);
const LIST_TIMEOUT: Duration = Duration::from_secs(5);
/// Только что поднятое ядро отвечает раньше, чем разложит узлы и провайдеров:
/// столько раз и с такой паузой ждём, пока разложит.
const SETTLE_ATTEMPTS: u32 = 20;
const SETTLE_PAUSE: Duration = Duration::from_millis(1500);
const PARALLEL_CHECKS: usize = 3;
const TICK: Duration = Duration::from_secs(60 * 60);
/// Через сколько переспросить MAC роутера, если он не достался сразу.
const MAC_RETRY_AFTER: Duration = Duration::from_secs(3);
/// «Были в этой сети» обновляется в файле не чаще раза в сутки.
const TOUCH_AFTER: i64 = 24 * 60 * 60;

/// Номер захода: каждый повод поднимает его, заход со старым номером
/// бросается, не записав ничего.
static EPOCH: AtomicU64 = AtomicU64::new(0);
static RUNNING: AtomicBool = AtomicBool::new(false);
static ASKED_AGAIN: AtomicBool = AtomicBool::new(false);
/// Последний повод — для журнала захода, который его обслужит.
static REASON: Mutex<&'static str> = Mutex::new("");
static FOREIGN_CORE_TOLD: AtomicBool = AtomicBool::new(false);
/// Итоги в сети, которую не удалось распознать: живут до конца сеанса, на диск
/// не идут, но повторы в них считаются как везде.
static UNPLACED: Mutex<BTreeMap<std::string::String, store::Network>> = Mutex::new(BTreeMap::new());
/// Следующий заход ждёт, пока ядро разложит узлы: оно только что поднялось
/// или получило новую сборку. Остальным заходам ждать незачем — провайдер,
/// пустой и через минуту, так и останется пустым.
static AWAIT_LAYOUT: AtomicBool = AtomicBool::new(false);
/// Подписка и сеть, где в последнем заходе не прошло ничего ни через кого
/// (нет интернета, страница входа Wi-Fi): какие узлы так проверены и когда.
/// Их повтор — не раньше чем через 6 часов от их проверки; прочие узлы пауза
/// не держит. В памяти.
static QUIET: Mutex<BTreeMap<std::string::String, BTreeMap<std::string::String, i64>>> = Mutex::new(BTreeMap::new());
/// Ядро уже поднималось в этом сеансе — первый подъём служит поводом захода,
/// остальные (перезапуски) нет.
static CORE_CAME_UP: AtomicBool = AtomicBool::new(false);
/// Заход бросили, потому что ядро замолчало: его продолжит следующий подъём
/// ядра — это не новый повод, а недоделанный заход.
static CORE_WAS_SILENT: AtomicBool = AtomicBool::new(false);

#[derive(Default)]
struct Marks {
    uid: Option<std::string::String>,
    by_name: BTreeMap<std::string::String, &'static str>,
}

static MARKS: Mutex<Marks> = Mutex::new(Marks {
    uid: None,
    by_name: BTreeMap::new(),
});

/// Пометки текущей подписки в текущей сети: имя узла → `frozen` | `dead`.
pub(crate) fn marks() -> BTreeMap<std::string::String, &'static str> {
    MARKS.lock().by_name.clone()
}

/// Заготовка для прослойки: всё, что клиент знает о проверке 16–20 этой
/// подписки, одним документом — по сетям (хеш) и отпечаткам узлов, с именем
/// узла и кодом ответа последней проверки. Куда и когда отправлять, решится
/// позже; здесь только форма.
#[allow(dead_code)]
pub(crate) async fn report(uid: &str) -> serde_json::Value {
    let saved = store::load(uid).await;
    serde_json::json!({
        "version": 1,
        "platform": "pc",
        "client": env!("CARGO_PKG_VERSION"),
        "subscription": uid,
        "networks": saved.networks,
    })
}

/// Повод для нового захода без новой сборки у ядра — подписка обновилась
/// (панель могла включить или выключить проверку). Перезапуск ядра или
/// туннеля, правки DNS и прочих настроек поводом не служат.
pub fn check_again(reason: &'static str) {
    kick(reason, Standing::Kept);
}

/// Ядру доставлена другая подписка: пометки прежней гаснут сразу, её идущий
/// заход бросается — подписки не делят ничего.
pub fn subscription_changed() {
    AWAIT_LAYOUT.store(true, Ordering::Release);
    // Сначала новый номер захода: идущий заход прежней подписки после этого
    // уже ничего не покажет.
    kick("subscription changed", Standing::Dropped);
    clear_marks();
}

/// Ядру доставлены новые узлы или провайдеры подписки, или сменилось ядро.
pub fn nodes_changed() {
    AWAIT_LAYOUT.store(true, Ordering::Release);
    kick("nodes or core changed", Standing::Kept);
}

/// Новое ядро ответило. Первое за сеанс — повод захода (приложение
/// запустилось); перезапуски — нет, кроме одного: заход, брошенный из-за
/// замолчавшего ядра, продолжается.
pub fn core_came_up() {
    let first = !CORE_CAME_UP.swap(true, Ordering::AcqRel);
    let unfinished = CORE_WAS_SILENT.swap(false, Ordering::AcqRel);
    if first || unfinished {
        AWAIT_LAYOUT.store(true, Ordering::Release);
        kick(
            if first { "app started" } else { "the core answers again" },
            Standing::Kept,
        );
    }
}

/// Сторож среды увидел смену сети или пробуждение — идущий заход бросается.
pub fn network_changed() {
    kick("network changed", Standing::Dropped);
}

/// Тик раз в час: подхватывает просроченные повторы.
pub fn spawn() {
    AsyncHandler::spawn(|| async {
        loop {
            tokio::time::sleep(TICK).await;
            kick("hourly tick", Standing::Kept);
        }
    });
}

/// Что делать с заходом, который идёт сейчас.
#[derive(Clone, Copy)]
enum Standing {
    /// Его итоги ещё верны — пусть дойдёт, следующий пойдёт за ним.
    Kept,
    /// Сеть или узлы уже не те — итоги бросить.
    Dropped,
}

fn kick(reason: &'static str, standing: Standing) {
    if handle::Handle::global().is_exiting() {
        return;
    }
    *REASON.lock() = reason;
    if matches!(standing, Standing::Dropped) {
        EPOCH.fetch_add(1, Ordering::AcqRel);
    }
    ASKED_AGAIN.store(true, Ordering::SeqCst);
    AsyncHandler::spawn(|| async { run_passes().await });
}

/// Один заход за раз; повод, пришедший во время захода, даёт ещё один.
async fn run_passes() {
    loop {
        if RUNNING.swap(true, Ordering::SeqCst) {
            return;
        }
        {
            scopeguard::defer! {
                RUNNING.store(false, Ordering::SeqCst);
            }
            while ASKED_AGAIN.swap(false, Ordering::SeqCst) {
                if handle::Handle::global().is_exiting() {
                    return;
                }
                let reason = *REASON.lock();
                pass(reason).await;
            }
        }
        if !ASKED_AGAIN.load(Ordering::SeqCst) {
            return;
        }
    }
}

fn now_unix_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Узел из списка ядра, у которого есть отпечаток.
#[derive(Debug, Clone, PartialEq, Eq)]
struct NodeRef {
    name: std::string::String,
    provider: Option<std::string::String>,
    fingerprint: std::string::String,
}

/// Что ядро перечислило: узлы с отпечатком и сколько узлов было без него
/// (чужое ядро).
#[derive(Debug, Default, PartialEq, Eq)]
struct Listing {
    nodes: Vec<NodeRef>,
    without_fingerprint: usize,
}

/// Записи списка, которые не узлы: группы (у них `all`) и встроенные политики.
fn is_a_node(entry: &serde_json::Value) -> bool {
    if entry.get("all").is_some() {
        return false;
    }
    !matches!(
        entry.get("type").and_then(serde_json::Value::as_str),
        Some("Direct" | "Reject" | "RejectDrop" | "Compatible" | "Pass" | "PassRule" | "Dns")
    )
}

fn nodes_of(proxies: &serde_json::Value, providers: Option<&serde_json::Value>) -> Listing {
    let mut seen = BTreeSet::new();
    let mut listing = Listing::default();
    let mut take = |name: &str, provider: Option<&str>, entry: &serde_json::Value| {
        let fingerprint = entry
            .get("fingerprint")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        if fingerprint.is_empty() {
            listing.without_fingerprint += usize::from(is_a_node(entry));
            return;
        }
        if !seen.insert(name.to_owned()) {
            return;
        }
        listing.nodes.push(NodeRef {
            name: name.to_owned(),
            provider: provider.map(str::to_owned),
            fingerprint: fingerprint.to_owned(),
        });
    };
    if let Some(map) = proxies.get("proxies").and_then(serde_json::Value::as_object) {
        for (name, entry) in map {
            take(name, None, entry);
        }
    }
    let listed = providers
        .and_then(|providers| providers.get("providers"))
        .and_then(serde_json::Value::as_object);
    if let Some(map) = listed {
        for (provider, entry) in map {
            let Some(members) = entry.get("proxies").and_then(serde_json::Value::as_array) else {
                continue;
            };
            for member in members {
                if let Some(name) = member.get("name").and_then(serde_json::Value::as_str) {
                    take(name, Some(provider), member);
                }
            }
        }
    }
    listing
}

async fn core_json(path: &str, budget: Duration) -> Option<serde_json::Value> {
    let response = crate::feat::core_send(Method::GET, path, budget).await.ok()?;
    response.json::<serde_json::Value>().await.ok()
}

/// Все узлы с отпечатком: из списка прокси и из провайдеров, и `true`, если
/// ядро уже разложило сборку. `None` — ядро не ответило.
async fn list_nodes() -> Option<(Listing, bool)> {
    let proxies = core_json("/proxies", LIST_TIMEOUT).await?;
    let providers = core_json("/providers/proxies", LIST_TIMEOUT).await;
    let settled = core_settled(&proxies, providers.as_ref());
    Some((nodes_of(&proxies, providers.as_ref()), settled))
}

/// Ядро разложило сборку: группы на месте (GLOBAL есть всегда) и у каждого
/// провайдера по ссылке есть узлы. Только что поднятое ядро отвечает раньше.
fn core_settled(proxies: &serde_json::Value, providers: Option<&serde_json::Value>) -> bool {
    let groups = proxies.get("proxies").and_then(|map| map.get("GLOBAL")).is_some();
    let filled = providers
        .and_then(|providers| providers.get("providers"))
        .and_then(serde_json::Value::as_object)
        .is_some_and(|map| {
            map.values().all(|provider| {
                provider.get("vehicleType").and_then(serde_json::Value::as_str) != Some("HTTP")
                    || provider
                        .get("proxies")
                        .and_then(serde_json::Value::as_array)
                        .is_some_and(|members| !members.is_empty())
            })
        });
    groups && filled
}

/// Узлы ядра, когда оно разложило сборку, и `true`, если дождались; не
/// дождались — что есть. Новый повод (смена сети) ожидание прерывает.
async fn settled_nodes(epoch: u64) -> Option<(Listing, bool)> {
    let mut listed = list_nodes().await;
    let attempts = if AWAIT_LAYOUT.swap(false, Ordering::AcqRel) {
        SETTLE_ATTEMPTS
    } else {
        0
    };
    for _ in 0..attempts {
        if matches!(listed, Some((_, true)))
            || EPOCH.load(Ordering::Acquire) != epoch
            || handle::Handle::global().is_exiting()
        {
            break;
        }
        tokio::time::sleep(SETTLE_PAUSE).await;
        listed = list_nodes().await;
    }
    listed
}

/// Имя → отпечаток: по этому в конце захода видно, те ли ещё узлы у ядра.
fn names_of(nodes: &[NodeRef]) -> BTreeMap<&str, &str> {
    nodes
        .iter()
        .map(|node| (node.name.as_str(), node.fingerprint.as_str()))
        .collect()
}

fn encoded(text: &str) -> std::string::String {
    utf8_percent_encode(text, NON_ALPHANUMERIC).to_string()
}

fn download_path(node: &NodeRef) -> std::string::String {
    let head = match node.provider.as_deref() {
        Some(provider) => format!(
            "/providers/proxies/{}/{}/download",
            encoded(provider),
            encoded(&node.name)
        ),
        None => format!("/proxies/{}/download", encoded(&node.name)),
    };
    format!(
        "{head}?url={}&size={DOWNLOAD_SIZE}&timeout={DOWNLOAD_TIMEOUT_MS}&stall={DOWNLOAD_STALL_MS}",
        encoded(DOWNLOAD_URL)
    )
}

/// Итог проверки узла; `None` — не ответило само ядро (например, его
/// перезапустили посреди захода): это не итог узла.
async fn check(node: &NodeRef) -> Option<plan::Checked> {
    let answer = core_json(&download_path(node), REQUEST_BUDGET).await?;
    let status = answer.get("status").and_then(serde_json::Value::as_i64).unwrap_or(0);
    let outcome = answer
        .get("verdict")
        .and_then(serde_json::Value::as_str)
        .map_or(plan::Outcome::Unknown { answered: false }, |verdict| {
            plan::Outcome::parse(verdict, status)
        });
    Some(plan::Checked { outcome, status })
}

/// Кого проверять: один узел на отпечаток, только просроченные.
fn due_of(nodes: &[NodeRef], network: &store::Network, now: i64) -> Vec<NodeRef> {
    let mut taken = BTreeSet::new();
    nodes
        .iter()
        .filter(|node| taken.insert(node.fingerprint.clone()))
        .filter(|node| plan::is_due(network.nodes.get(&node.fingerprint), now))
        .cloned()
        .collect()
}

/// Кого проверять сейчас и `true`, если некого из-за паузы: в этой подписке и
/// сети через эти узлы недавно не прошло ничего.
fn due_now(quiet_key: &str, nodes: &[NodeRef], network: &store::Network, now: i64) -> (Vec<NodeRef>, bool) {
    let due = due_of(nodes, network, now);
    let Some(held) = QUIET.lock().get(quiet_key).cloned() else {
        return (due, false);
    };
    let paused = |node: &NodeRef| {
        held.get(&node.fingerprint)
            .is_some_and(|at| now.saturating_sub(*at) < plan::RETRY_AFTER)
    };
    let free: Vec<NodeRef> = due.iter().filter(|node| !paused(node)).cloned().collect();
    let hushed = free.is_empty() && !due.is_empty();
    (free, hushed)
}

/// После захода: не прошло ничего — эти узлы на паузе 6 часов; прошло хоть
/// что-то — паузы в этой подписке и сети нет.
fn remember_quiet(quiet_key: std::string::String, nodes: &[NodeRef], stored: bool, now: i64) {
    if stored {
        QUIET.lock().remove(&quiet_key);
        return;
    }
    QUIET
        .lock()
        .entry(quiet_key)
        .or_default()
        .extend(nodes.iter().map(|node| (node.fingerprint.clone(), now)));
}

const fn hushed_note(hushed: bool) -> &'static str {
    if hushed { " (nothing passed here last time)" } else { "" }
}

fn marks_of(nodes: &[NodeRef], network: &store::Network) -> BTreeMap<std::string::String, &'static str> {
    nodes
        .iter()
        .filter_map(|node| {
            let mark = network.nodes.get(&node.fingerprint)?.verdict?.mark()?;
            Some((node.name.clone(), mark))
        })
        .collect()
}

/// Запомнить пометки для фронта и трея; `true` — они изменились.
/// Показать пометки захода с номером `epoch`; заход устарел (сменились сеть
/// или подписка) — ничего не показывать.
fn publish(uid: &str, by_name: BTreeMap<std::string::String, &'static str>, epoch: u64) -> bool {
    let mut marks = MARKS.lock();
    if EPOCH.load(Ordering::Acquire) != epoch {
        return false;
    }
    let changed = marks.uid.as_deref() != Some(uid) || marks.by_name != by_name;
    marks.uid = Some(uid.to_owned());
    marks.by_name = by_name;
    changed
}

fn clear_marks() {
    let mut marks = MARKS.lock();
    let changed = marks.uid.is_some() || !marks.by_name.is_empty();
    *marks = Marks::default();
    drop(marks);
    if changed {
        handle::Handle::refresh_freeze_marks();
    }
}

async fn announce(changed: bool) {
    if !changed {
        return;
    }
    handle::Handle::refresh_freeze_marks();
    if let Err(err) = Tray::global().update_menu().await {
        logging!(debug, Type::Core, "[Freeze] the tray menu did not refresh: {err:#}");
    }
}

fn counted(outcomes: &[plan::Outcome]) -> std::string::String {
    let mut ok = 0_usize;
    let mut frozen = 0_usize;
    let mut dead = 0_usize;
    let mut unknown = 0_usize;
    for outcome in outcomes {
        match outcome {
            plan::Outcome::Verdict(Verdict::Ok) => ok += 1,
            plan::Outcome::Verdict(Verdict::Frozen) => frozen += 1,
            plan::Outcome::Verdict(Verdict::Dead) => dead += 1,
            plan::Outcome::Unknown { .. } => unknown += 1,
        }
    }
    format!("{ok} ok, {frozen} frozen, {dead} dead, {unknown} unknown")
}

/// Узлы с отпечатком у ядра; `None` — ядро не ответило или отпечатков нет.
async fn listed_nodes(uid: &str, reason: &str, verbose: bool, epoch: u64) -> Option<(Vec<NodeRef>, bool)> {
    let Some((listing, settled)) = settled_nodes(epoch).await else {
        CORE_WAS_SILENT.store(true, Ordering::Release);
        if verbose {
            logging!(
                info,
                Type::Core,
                "[Freeze] {reason}: the core did not answer, nothing checked"
            );
        }
        return None;
    };
    if listing.nodes.is_empty() {
        if listing.without_fingerprint > 0 {
            if !FOREIGN_CORE_TOLD.swap(true, Ordering::AcqRel) {
                logging!(
                    info,
                    Type::Core,
                    "[Freeze] the core lists no node fingerprints; the check stays silent on this core"
                );
            }
        } else if verbose {
            logging!(info, Type::Core, "[Freeze] {reason}: the subscription has no nodes");
        }
        announce(publish(uid, BTreeMap::new(), epoch)).await;
        return None;
    }
    FOREIGN_CORE_TOLD.store(false, Ordering::Release);
    Some((listing.nodes, settled))
}

/// Проверить просроченные узлы по три за раз. Итоги захода; `None` — заход не в счёт: сменилась сеть, идёт выход или
/// ядро не ответило хоть на одну проверку.
async fn checked(due: Vec<NodeRef>, epoch: u64) -> Option<Vec<(NodeRef, plan::Checked)>> {
    // Ядро не ответило — заход уже не в счёт, остальные проверки не гоняем.
    let silent = AtomicBool::new(false);
    let silent = &silent;
    let outcomes: Vec<Option<(NodeRef, plan::Checked)>> = futures::stream::iter(due)
        .map(|node| async move {
            if silent.load(Ordering::Acquire)
                || EPOCH.load(Ordering::Acquire) != epoch
                || handle::Handle::global().is_exiting()
            {
                return None;
            }
            let outcome = check(&node).await;
            if outcome.is_none() {
                silent.store(true, Ordering::Release);
            }
            outcome.map(|outcome| (node, outcome))
        })
        .buffer_unordered(PARALLEL_CHECKS)
        .collect()
        .await;
    if EPOCH.load(Ordering::Acquire) != epoch {
        return None;
    }
    outcomes.into_iter().collect()
}

/// Записать итоги в сеть; `true` — записано.
fn recorded(network: &mut store::Network, outcomes: &[(NodeRef, plan::Checked)], now: i64, verbose: bool) -> bool {
    let verdicts: Vec<plan::Outcome> = outcomes.iter().map(|(_, checked)| checked.outcome).collect();
    if !plan::worth_recording(&verdicts) {
        return false;
    }
    for (node, checked) in outcomes {
        if verbose {
            logging!(
                info,
                Type::Core,
                "[Freeze] {}: {:?} ({})",
                node.name,
                checked.outcome,
                checked.status
            );
        }
        let record = network.nodes.entry(node.fingerprint.clone()).or_default();
        plan::apply(record, checked.outcome, now);
        record.name = Some(node.name.clone());
        record.status = Some(checked.status);
    }
    true
}

/// Ключ текущей сети; без MAC роутера там, где он положен, — переспросить
/// чуть позже: запись о шлюзе в таблице соседей появляется с первым трафиком.
async fn network_key() -> Option<network::Key> {
    let key = AsyncHandler::spawn_blocking(network::current).await.ok().flatten();
    if !key.as_ref().is_some_and(|key| key.mac_missing) {
        return key;
    }
    tokio::time::sleep(MAC_RETRY_AFTER).await;
    AsyncHandler::spawn_blocking(network::current).await.ok().flatten()
}

/// Тот ли ещё мир, в котором шёл заход: та же подписка, те же узлы, та же сеть.
async fn still_the_same(uid: &str, checked: &[NodeRef], key: Option<&str>) -> bool {
    if running_subscription().await.as_deref() != Some(uid) {
        return false;
    }
    let Some((listing, _)) = list_nodes().await else {
        return false;
    };
    // Проверенные узлы на месте и те же; что ядро тем временем догрузило
    // других, итогов не отменяет.
    let now = names_of(&listing.nodes);
    if !checked
        .iter()
        .all(|node| now.get(node.name.as_str()) == Some(&node.fingerprint.as_str()))
    {
        return false;
    }
    let now_key = AsyncHandler::spawn_blocking(network::current).await.ok().flatten();
    now_key.as_ref().map(|key| key.hash.as_str()) == key
}

/// Подписка, на которой работает ядро. Не текущая в реестре: при смене
/// подписки реестр записывает её только после того, как ядро её приняло.
async fn running_subscription() -> Option<std::string::String> {
    Config::runtime()
        .await
        .data_arc()
        .profile_uid
        .as_ref()
        .map(ToString::to_string)
}

/// Подписка ядра, если панель включила проверку заголовком
/// `clod-16-20-check: true`; иначе пометки гасятся и делать нечего.
async fn enabled_subscription(reason: &str, verbose: bool, epoch: u64) -> Option<std::string::String> {
    let uid = running_subscription().await?;
    let enabled = Config::profiles()
        .await
        .latest_arc()
        .get_item(&uid)
        .is_ok_and(|item| item.freeze_check == Some(true));
    if !enabled && verbose {
        logging!(
            info,
            Type::Core,
            "[Freeze] {reason}: the panel has not turned the check on (clod-16-20-check), nothing to do"
        );
    }
    // Другая подписка: пометки прежней гаснут сразу, до её собственного захода —
    // одноимённые узлы двух подписок не делят ничего. Выключенная — тоже без пометок.
    let other = MARKS.lock().uid.as_deref() != Some(uid.as_str());
    if !enabled || other {
        announce(publish(&uid, BTreeMap::new(), epoch)).await;
    }
    enabled.then_some(uid)
}

/// Итоги подписки с диска и итоги неопознанной сети (живут в памяти до конца
/// сеанса) — без узлов, которых у ядра больше нет. Если ядро не разложило узлы
/// до конца, по неполному списку не чистим: стёрлись бы итоги узлов, которых
/// ядро просто ещё не показало.
async fn saved_results(uid: &str, nodes: &[NodeRef], settled: bool, now: i64) -> (store::Store, store::Network) {
    let mut saved = store::load(uid).await;
    let mut unplaced = UNPLACED.lock().get(uid).cloned().unwrap_or_default();
    if settled {
        let live: BTreeSet<std::string::String> = nodes.iter().map(|node| node.fingerprint.clone()).collect();
        saved.forget_nodes_except(&live);
        unplaced.nodes.retain(|fingerprint, _| live.contains(fingerprint));
    }
    saved.forget_old_networks(now);
    (saved, unplaced)
}

/// Через проверенные узлы не прошло ничего — это сеть или только они? Узел,
/// который в этой сети отмечен рабочим (свежее всех), — контрольный: прошёл
/// он, значит сеть есть и итоги остальных в счёт. Без такого узла или если
/// что-то уже прошло — проверять нечего.
fn control_node(nodes: &[NodeRef], network: &store::Network, outcomes: &[(NodeRef, plan::Checked)]) -> Option<NodeRef> {
    let verdicts: Vec<plan::Outcome> = outcomes.iter().map(|(_, checked)| checked.outcome).collect();
    if plan::worth_recording(&verdicts) {
        return None;
    }
    let tried: BTreeSet<&str> = outcomes.iter().map(|(node, _)| node.fingerprint.as_str()).collect();
    nodes
        .iter()
        .filter(|node| !tried.contains(node.fingerprint.as_str()))
        .filter_map(|node| {
            let record = network.nodes.get(&node.fingerprint)?;
            (record.verdict == Some(plan::Verdict::Ok)).then_some((record.at, node))
        })
        .max_by_key(|(at, _)| *at)
        .map(|(_, node)| node.clone())
}

/// Проверить узлы; ядро замолчало или сменилась сеть — `None`.
async fn checked_or_dropped(due: Vec<NodeRef>, epoch: u64, reason: &str) -> Option<Vec<(NodeRef, plan::Checked)>> {
    let outcomes = checked(due, epoch).await;
    if outcomes.is_none() {
        // Номер тот же — заход бросило молчание ядра: доделает следующий подъём.
        if EPOCH.load(Ordering::Acquire) == epoch {
            CORE_WAS_SILENT.store(true, Ordering::Release);
        }
        logging!(
            info,
            Type::Core,
            "[Freeze] {reason}: the network changed or the core did not answer mid-pass, results dropped"
        );
    }
    outcomes
}

/// Итоги проверок (с контрольным узлом, если через проверенные не прошло
/// ничего), если они ещё о тех же узлах в той же сети и подписке; `None` —
/// заход не в счёт.
async fn checked_in_place(
    uid: &str,
    due: Vec<NodeRef>,
    nodes: &[NodeRef],
    network: &store::Network,
    key: Option<&str>,
    epoch: u64,
    reason: &str,
) -> Option<Vec<(NodeRef, plan::Checked)>> {
    let mut checked_nodes = due.clone();
    let mut outcomes = checked_or_dropped(due, epoch, reason).await?;
    if let Some(control) = control_node(nodes, network, &outcomes) {
        logging!(
            info,
            Type::Core,
            "[Freeze] {reason}: nothing passed, checking {} that worked here to tell the network from the nodes",
            control.name
        );
        checked_nodes.push(control.clone());
        outcomes.extend(checked_or_dropped(vec![control], epoch, reason).await?);
    }
    if !still_the_same(uid, &checked_nodes, key).await {
        logging!(
            info,
            Type::Core,
            "[Freeze] {reason}: the network, the subscription or its checked nodes changed mid-pass, results dropped"
        );
        return None;
    }
    Some(outcomes)
}

async fn pass(reason: &'static str) {
    let epoch = EPOCH.load(Ordering::Acquire);
    let verbose = verbose_diagnostics().await;

    if Config::profiles().await.latest_arc().get_current().is_none() {
        clear_marks();
        return;
    }
    let Some(uid) = enabled_subscription(reason, verbose, epoch).await else {
        return;
    };
    let Some((nodes, settled)) = listed_nodes(&uid, reason, verbose, epoch).await else {
        return;
    };
    // Пока ждали ядро, сменились сеть или подписка — этот заход уже не их.
    if EPOCH.load(Ordering::Acquire) != epoch {
        return;
    }

    let key = network_key().await.map(|key| key.hash);
    let shown_key = key.as_deref().unwrap_or("?");
    let now = now_unix_secs();
    let (mut saved, mut unplaced) = saved_results(&uid, &nodes, settled, now).await;
    let network = match key.as_deref() {
        Some(key) => saved.network_mut(key),
        None => &mut unplaced,
    };
    // Пометки этой сети показываются сразу, не дожидаясь проверок.
    announce(publish(&uid, marks_of(&nodes, network), epoch)).await;

    let quiet_key = format!("{uid}/{shown_key}");
    let (due, hushed) = due_now(&quiet_key, &nodes, network, now);
    let mut stored = false;
    if due.is_empty() {
        if verbose {
            logging!(
                info,
                Type::Core,
                "[Freeze] {reason}: nothing due in network {shown_key}{}",
                hushed_note(hushed)
            );
        }
    } else {
        let due_nodes = due.clone();
        let Some(outcomes) = checked_in_place(&uid, due, &nodes, network, key.as_deref(), epoch, reason).await else {
            return;
        };
        stored = recorded(network, &outcomes, now, verbose);
        remember_quiet(quiet_key, &due_nodes, stored, now);
        let verdicts: Vec<plan::Outcome> = outcomes.iter().map(|(_, checked)| checked.outcome).collect();
        let tail = if stored {
            ""
        } else {
            "; nothing passed anywhere, not recorded, next try in 6 h"
        };
        logging!(
            info,
            Type::Core,
            "[Freeze] {reason}: {} of {} node(s) checked in network {shown_key} — {}{tail}",
            outcomes.len(),
            nodes.len(),
            counted(&verdicts)
        );
    }

    if EPOCH.load(Ordering::Acquire) != epoch {
        return;
    }
    let by_name = marks_of(&nodes, network);
    let known = network.last_seen != 0;
    if key.is_some() && (stored || (known && now.saturating_sub(network.last_seen) >= TOUCH_AFTER)) {
        network.last_seen = now;
        if let Err(err) = store::save(&uid, &saved).await {
            logging!(warn, Type::Core, "[Freeze] the results were not saved: {err:#}");
        }
    }
    if key.is_none() {
        UNPLACED.lock().insert(uid.clone(), unplaced);
    }
    announce(publish(&uid, by_name, epoch)).await;
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;

    use super::{
        Listing, NodeRef, control_node, core_settled, download_path, due_now, due_of, marks_of, names_of, nodes_of,
        plan, remember_quiet, store,
    };

    fn node(name: &str, fingerprint: &str) -> NodeRef {
        NodeRef {
            name: name.into(),
            provider: None,
            fingerprint: fingerprint.into(),
        }
    }

    #[test]
    fn when_nothing_passed_the_freshest_node_that_worked_here_is_the_control() {
        use super::plan::{Checked, Node, Outcome, Verdict};
        let silent = Checked {
            outcome: Outcome::Unknown { answered: false },
            status: 0,
        };
        let nodes = [
            node("new", "f0"),
            node("old", "f1"),
            node("fresh", "f2"),
            node("cut", "f3"),
        ];
        let mut network = store::Network::default();
        let record = |verdict, at| Node {
            verdict: Some(verdict),
            at,
            ..Node::default()
        };
        network.nodes.insert("f1".into(), record(Verdict::Ok, 100));
        network.nodes.insert("f2".into(), record(Verdict::Ok, 200));
        network.nodes.insert("f3".into(), record(Verdict::Frozen, 300));

        let nothing = [(node("new", "f0"), silent)];
        assert_eq!(control_node(&nodes, &network, &nothing), Some(node("fresh", "f2")));

        let passed = [(
            node("new", "f0"),
            Checked {
                outcome: Outcome::Verdict(Verdict::Frozen),
                status: 200,
            },
        )];
        assert_eq!(control_node(&nodes, &network, &passed), None);

        let checked_too = [(node("new", "f0"), silent), (node("fresh", "f2"), silent)];
        assert_eq!(control_node(&nodes, &network, &checked_too), Some(node("old", "f1")));
    }

    #[test]
    fn a_freshly_started_core_is_settled_once_groups_and_link_providers_are_filled() {
        let groups = json!({ "proxies": { "GLOBAL": { "all": [] } } });
        let empty = json!({ "providers": { "sub": { "vehicleType": "HTTP", "proxies": [] } } });
        let filled = json!({ "providers": {
            "sub": { "vehicleType": "HTTP", "proxies": [{ "name": "a" }] },
            "default": { "vehicleType": "Compatible", "proxies": [] },
        } });
        assert!(!core_settled(&json!({ "proxies": {} }), Some(&filled)));
        assert!(!core_settled(&groups, Some(&empty)));
        assert!(!core_settled(&groups, None));
        assert!(core_settled(&groups, Some(&filled)));
    }

    #[test]
    fn a_quiet_pause_holds_its_nodes_and_lets_new_ones_through() {
        const AT: i64 = 1_800_000_000;
        let key = "test-subscription/test-network".to_owned();
        let network = store::Network::default();
        let held = [node("a", "f1"), node("b", "f2")];
        remember_quiet(key.clone(), &held, false, AT);

        let (due, hushed) = due_now(&key, &held, &network, AT + 60);
        assert!(due.is_empty() && hushed);

        let grown = [node("a", "f1"), node("c", "f3")];
        let (due, hushed) = due_now(&key, &grown, &network, AT + 60);
        assert_eq!(due, vec![node("c", "f3")]);
        assert!(!hushed);

        let (due, hushed) = due_now(&key, &held, &network, AT + plan::RETRY_AFTER);
        assert_eq!(due.len(), 2);
        assert!(!hushed);

        // У каждого узла своя пауза: узел, проверенный позже, держится дольше.
        remember_quiet(key.clone(), &[node("c", "f3")], false, AT + 600);
        let (due, _) = due_now(&key, &grown, &network, AT + plan::RETRY_AFTER);
        assert_eq!(due, vec![node("a", "f1")]);

        remember_quiet(key.clone(), &held, true, AT + 120);
        let (due, _) = due_now(&key, &held, &network, AT + 180);
        assert_eq!(due.len(), 2);
    }

    #[test]
    fn only_nodes_with_a_fingerprint_are_listed_once_each() {
        let proxies = json!({ "proxies": {
            "GLOBAL": { "type": "Selector", "all": ["a"] },
            "DIRECT": { "type": "Direct" },
            "a": { "type": "VLESS", "fingerprint": "f1" },
            "b": { "type": "VLESS", "fingerprint": "" },
        }});
        let providers = json!({ "providers": {
            "default": { "proxies": [ { "name": "a", "fingerprint": "f1" } ] },
            "sub": { "proxies": [ { "name": "c", "fingerprint": "f2" }, { "name": "d", "type": "Trojan" } ] },
        }});
        let listing = nodes_of(&proxies, Some(&providers));
        assert_eq!(
            listing.nodes,
            vec![
                node("a", "f1"),
                NodeRef {
                    name: "c".into(),
                    provider: Some("sub".into()),
                    fingerprint: "f2".into(),
                },
            ]
        );
        // Узлы без отпечатка — `b` и `d`; группа и DIRECT не в счёт.
        assert_eq!(listing.without_fingerprint, 2);
        assert_eq!(nodes_of(&proxies, None).nodes.len(), 1);
    }

    #[test]
    fn a_foreign_core_differs_from_an_empty_subscription() {
        let foreign = json!({ "proxies": {
            "GLOBAL": { "type": "Selector", "all": ["a"] },
            "a": { "type": "VLESS" },
        }});
        assert_eq!(
            nodes_of(&foreign, None),
            Listing {
                nodes: Vec::new(),
                without_fingerprint: 1
            }
        );
        let empty = json!({ "proxies": {
            "GLOBAL": { "type": "Selector", "all": ["DIRECT"] },
            "DIRECT": { "type": "Direct" },
            "REJECT": { "type": "Reject" },
            "PASS-RULE": { "type": "PassRule" },
        }});
        assert_eq!(nodes_of(&empty, None), Listing::default());
    }

    #[test]
    fn the_same_nodes_under_other_names_count_as_a_change() {
        let before = vec![node("a", "f1"), node("b", "f2")];
        let same = vec![node("b", "f2"), node("a", "f1")];
        let renamed = vec![node("a", "f2"), node("b", "f1")];
        assert_eq!(names_of(&before), names_of(&same));
        assert_ne!(names_of(&before), names_of(&renamed));
        assert_ne!(names_of(&before), names_of(&before[..1]));
    }

    #[test]
    fn the_download_path_escapes_names_and_the_url() {
        assert_eq!(
            download_path(&node("Node 1/ü", "f")),
            "/proxies/Node%201%2F%C3%BC/download?url=https%3A%2F%2Fspeed%2Ecloudflare%2Ecom%2F%5F%5Fdown%3Fbytes%3D65536&size=65536&timeout=10000&stall=4000"
        );
        let provided = download_path(&NodeRef {
            name: "n".into(),
            provider: Some("p q".into()),
            fingerprint: "f".into(),
        });
        assert!(provided.starts_with("/providers/proxies/p%20q/n/download?"));
    }

    #[test]
    fn one_check_per_fingerprint_and_only_when_due() {
        let now = 1_800_000_000;
        let mut network = store::Network::default();
        network.nodes.insert(
            "fresh".into(),
            plan::Node {
                verdict: Some(plan::Verdict::Ok),
                at: now,
                tried: now,
                ..plan::Node::default()
            },
        );
        let nodes = vec![
            node("a", "new"),
            node("a2", "new"),
            node("b", "fresh"),
            node("c", "other"),
        ];
        let due = due_of(&nodes, &network, now);
        assert_eq!(due, vec![node("a", "new"), node("c", "other")]);
    }

    #[test]
    fn marks_follow_the_fingerprint_and_skip_ok() {
        let now = 1_800_000_000;
        let mut network = store::Network::default();
        let verdict = |verdict| plan::Node {
            verdict: Some(verdict),
            at: now,
            tried: now,
            ..plan::Node::default()
        };
        network.nodes.insert("f1".into(), verdict(plan::Verdict::Frozen));
        network.nodes.insert("f2".into(), verdict(plan::Verdict::Ok));
        network.nodes.insert("f3".into(), verdict(plan::Verdict::Dead));
        let nodes = vec![
            node("a", "f1"),
            node("a2", "f1"),
            node("b", "f2"),
            node("c", "f3"),
            node("d", "f4"),
        ];
        let marks = marks_of(&nodes, &network);
        let expected: BTreeMap<std::string::String, &'static str> = [
            ("a".to_owned(), "frozen"),
            ("a2".to_owned(), "frozen"),
            ("c".to_owned(), "dead"),
        ]
        .into_iter()
        .collect();
        assert_eq!(marks, expected);
    }
}
