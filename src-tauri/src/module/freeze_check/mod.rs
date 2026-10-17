//! clod:freeze — проверка 16–20: обрезается ли скачивание через узел в этой сети.
//!
//! Механизм — в ядре Clod Core (`GET /proxies/{name}/download`: узел качает
//! 64 КБ и отвечает ok / frozen / dead / unknown). Здесь — когда звать,
//! хранение по подписке и сети и пометка для интерфейса. Итог один —
//! обрезается ли скачивание: «работает» или «режется»; всё прочее (в том числе
//! dead) итогом не считается и записывается только как попытка. С узлами
//! клиент ничего не делает: пинг как был, выбор как был.
//!
//! Поводы захода: сменилась сеть; сменились подписка, её узлы или ядро;
//! панель включила или выключила проверку; ядро впервые поднялось за сеанс;
//! тик раз в час. Перезапуск ядра или туннеля с
//! той же сборкой поводом не служит. От подключения заход не зависит:
//! проверяется путь от адреса клиента до адреса сервера, туннель тут ни при
//! чём. Проверяются отпечатки без итога в текущей сети, итоги старше 3 суток,
//! попытки без итога старше 6 часов; одинаковые узлы делят результат по
//! отпечатку. Обрезанный узел через 10 с проверяется ещё дважды вместе с
//! контрольным — рабочим в этой сети: «режется» — только если обрезались все
//! три проверки, а контрольный прошёл; прошла последняя — «работает»; иначе
//! (сравнить не с чем, контрольный не прошёл) итога нет. Если посреди захода
//! сменились сеть или подписка или ядро не ответило, заход бросается. Сеть не
//! распознана — ни проверок, ни пометок.
//! Ядро без отпечатков (чужое) — функция молчит. Включает проверку только
//! панель — заголовком подписки `clod-16-20-check: true`; без него ни
//! проверок, ни пометок. Итоги уходят и в отчёт прослойке
//! (`module::client_report`) — там решается, копить ли их.

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
    config::{Config, proxy_label::Sentinels},
    core::{handle, sysopt::verbose_diagnostics, tray::Tray},
    feat::CoreListing,
    process::AsyncHandler,
};

/// Что качаем через узел: 64 КБ нулей без сжатия.
const DOWNLOAD_URL: &str = "https://speed.cloudflare.com/__down?bytes=65536";
const DOWNLOAD_SIZE: u32 = 65_536;
const DOWNLOAD_TIMEOUT_MS: u32 = 10_000;
const DOWNLOAD_STALL_MS: u32 = 4_000;
/// Ядро ждёт очередь к хосту, качает, потом пингует: 3 + 10 + 5 + 3 с.
const REQUEST_BUDGET: Duration = Duration::from_secs(30);
/// Пауза между повторами обрезанных узлов.
const WAVE_PAUSE: Duration = Duration::from_secs(10);
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
/// Сколько раз просили заход: по разнице до и после доставки подписки видно,
/// позвал ли его уже кто-то.
static ASKED: AtomicU64 = AtomicU64::new(0);
static RUNNING: AtomicBool = AtomicBool::new(false);
static ASKED_AGAIN: AtomicBool = AtomicBool::new(false);
/// Последний повод — для журнала захода, который его обслужит.
static REASON: Mutex<&'static str> = Mutex::new("");
/// Ключ сети, в которой идёт текущий заход; `None` — заход его ещё не узнал.
static PASS_KEY: Mutex<Option<std::string::String>> = Mutex::new(None);
static FOREIGN_CORE_TOLD: AtomicBool = AtomicBool::new(false);
/// Следующий заход ждёт, пока ядро разложит узлы: оно только что поднялось
/// или получило новую сборку. Остальным заходам ждать незачем — провайдер,
/// пустой и через минуту, так и останется пустым.
static AWAIT_LAYOUT: AtomicBool = AtomicBool::new(false);
/// Ядро уже поднималось в этом сеансе — первый подъём служит поводом захода,
/// остальные (перезапуски) нет.
static CORE_CAME_UP: AtomicBool = AtomicBool::new(false);
/// Заход бросили, потому что ядро замолчало: его продолжит следующий подъём
/// ядра — это не новый повод, а недоделанный заход.
static CORE_WAS_SILENT: AtomicBool = AtomicBool::new(false);

#[derive(Default)]
struct Marks {
    uid: Option<std::string::String>,
    by_name: BTreeSet<std::string::String>,
}

static MARKS: Mutex<Marks> = Mutex::new(Marks {
    uid: None,
    by_name: BTreeSet::new(),
});

/// Пометки текущей подписки в текущей сети: имена помеченных узлов.
pub(crate) fn marks() -> BTreeSet<std::string::String> {
    MARKS.lock().by_name.clone()
}

/// Текущая сеть для отчёта прослойке: хеш ключа (тот же, что у итогов
/// проверки) и вид сети. `None` — путь наружу не распознан.
pub(crate) async fn current_network() -> Option<(std::string::String, &'static str)> {
    network_key().await.map(|key| (key.hash, key.kind))
}

/// Повод для нового захода без новой сборки у ядра — панель включила или
/// выключила проверку: идущий заход бросается, пометки гаснут, и новый заход
/// решает заново. Перезапуск ядра или туннеля, правки DNS и прочих настроек
/// поводом не служат.
pub fn check_again(reason: &'static str) {
    kick(reason, Standing::Dropped);
}

/// Сколько раз просили заход: каждая просьба даёт заход, который начнётся
/// после неё.
pub fn passes_asked() -> u64 {
    ASKED.load(Ordering::Acquire)
}

/// Ядру доставлена другая подписка: её идущий заход бросается и пометки
/// гаснут — подписки не делят ничего.
pub fn subscription_changed() {
    AWAIT_LAYOUT.store(true, Ordering::Release);
    kick("subscription changed", Standing::Dropped);
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

/// Сторож среды увидел смену сети или пробуждение. Сеть по ключу та же
/// (с MAC роутера на месте) и машина не спала — идущий заход доходит: его
/// итоги о той же сети, а бросать его на каждую перемену отпечатка значило бы
/// начинать проверки заново без конца. Иначе — в том числе пока ключ без
/// MAC и сеть ещё не опознана — заход бросается сразу, без ожидания. Захода
/// нет — ключ сети не нужен: сравнивать не с чем.
pub fn network_changed(woke_up: bool) {
    if !compares_the_network(woke_up, RUNNING.load(Ordering::Acquire)) {
        kick("network changed", Standing::Dropped);
        return;
    }
    AsyncHandler::spawn(move || async move {
        let same_network = {
            let key = AsyncHandler::spawn_blocking(network::current).await.ok().flatten();
            key.is_some_and(|key| !key.mac_missing && PASS_KEY.lock().as_deref() == Some(key.hash.as_str()))
        };
        kick(
            "network changed",
            if same_network {
                Standing::Kept
            } else {
                Standing::Dropped
            },
        );
    });
}

/// Сверять ли сеть с идущим заходом: только когда он идёт и машина не спала.
const fn compares_the_network(woke_up: bool, running: bool) -> bool {
    running && !woke_up
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
    /// Сеть или подписка уже не те — итоги бросить, пометки погасить: следующий
    /// заход покажет пометки новой сети или подписки.
    Dropped,
}

fn kick(reason: &'static str, standing: Standing) {
    if handle::Handle::global().is_exiting() {
        return;
    }
    *REASON.lock() = reason;
    if matches!(standing, Standing::Dropped) {
        // Сначала новый номер захода: идущий заход после этого уже ничего не покажет.
        EPOCH.fetch_add(1, Ordering::AcqRel);
        clear_marks();
    }
    ASKED.fetch_add(1, Ordering::AcqRel);
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

/// Узлы ядра без заглушек провайдера (`sentinels` — общий разбор сборки).
fn nodes_of(listed: &CoreListing, sentinels: &Sentinels) -> Listing {
    let mut seen = BTreeSet::new();
    let mut listing = Listing::default();
    for (name, provider, entry) in listed.entries() {
        if sentinels.contains(&(provider.unwrap_or_default().to_owned(), name.to_owned())) {
            continue;
        }
        let fingerprint = entry
            .get("fingerprint")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        if fingerprint.is_empty() {
            listing.without_fingerprint += usize::from(is_a_node(entry));
            continue;
        }
        if !seen.insert(name.to_owned()) {
            continue;
        }
        listing.nodes.push(NodeRef {
            name: name.to_owned(),
            provider: provider.map(str::to_owned),
            fingerprint: fingerprint.to_owned(),
        });
    }
    listing
}

async fn core_json(path: &str, budget: Duration) -> Option<serde_json::Value> {
    let response = crate::feat::core_send(Method::GET, path, budget).await.ok()?;
    response.json::<serde_json::Value>().await.ok()
}

/// Все узлы с отпечатком: из списка прокси и из провайдеров сборки (каждый
/// своим запросом — в общем `/providers/proxies` узел повторяется по разу на
/// группу), и `true`, если ядро уже разложило сборку. `None` — ядро не ответило.
async fn list_nodes() -> Option<(Listing, bool)> {
    let listed = crate::feat::read_core_proxies(LIST_TIMEOUT).await.parsed()?;
    let sentinels = crate::config::proxy_label::sentinels().await;
    Some(listing_of(&listed, &sentinels))
}

/// Узлы — из всех прочитанных провайдеров; разложена ли сборка — только когда
/// прочитались все: сбой одного не выбрасывает из захода узлы остальных.
fn listing_of(listed: &CoreListing, sentinels: &Sentinels) -> (Listing, bool) {
    let settled = listed.failed == 0 && core_settled(listed);
    (nodes_of(listed, sentinels), settled)
}

/// Ядро разложило сборку: группы на месте (GLOBAL есть всегда) и у каждого
/// провайдера по ссылке есть узлы. Только что поднятое ядро отвечает раньше.
fn core_settled(listed: &CoreListing) -> bool {
    let groups = listed
        .proxies
        .get("proxies")
        .and_then(|map| map.get("GLOBAL"))
        .is_some();
    let filled = listed.providers.iter().all(|(_, provider)| {
        provider.get("vehicleType").and_then(serde_json::Value::as_str) != Some("HTTP")
            || provider
                .get("proxies")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|members| !members.is_empty())
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
    let verdict = answer
        .get("verdict")
        .and_then(serde_json::Value::as_str)
        .and_then(Verdict::parse);
    Some(plan::Checked { verdict, status })
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

fn marks_of(nodes: &[NodeRef], network: &store::Network) -> BTreeSet<std::string::String> {
    nodes
        .iter()
        .filter(|node| {
            network
                .nodes
                .get(&node.fingerprint)
                .and_then(|checked| checked.verdict)
                .is_some_and(plan::Verdict::is_frozen)
        })
        .map(|node| node.name.clone())
        .collect()
}

/// Запомнить пометки для фронта и трея; `true` — они изменились.
/// Показать пометки захода с номером `epoch`; заход устарел (сменились сеть
/// или подписка) — ничего не показывать.
fn publish(uid: &str, by_name: BTreeSet<std::string::String>, epoch: u64) -> bool {
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
        AsyncHandler::spawn(|| async { announce(true).await });
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

fn counted(checks: &[plan::Checked]) -> std::string::String {
    let count = |verdict| checks.iter().filter(|checked| checked.verdict == verdict).count();
    format!(
        "{} ok, {} frozen, {} without a result",
        count(Some(Verdict::Ok)),
        count(Some(Verdict::Frozen)),
        count(None)
    )
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
        announce(publish(uid, BTreeSet::new(), epoch)).await;
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

/// Записать проверки захода в сеть.
fn recorded(network: &mut store::Network, outcomes: &[(NodeRef, plan::Checked)], now: i64, verbose: bool) {
    for (node, checked) in outcomes {
        if verbose {
            logging!(
                info,
                Type::Core,
                "[Freeze] {}: {:?} ({})",
                node.name,
                checked.verdict,
                checked.status
            );
        }
        let record = network.nodes.entry(node.fingerprint.clone()).or_default();
        plan::apply(record, checked.verdict, now);
        record.name = Some(node.name.clone());
        record.status = Some(checked.status);
    }
}

/// Итоги захода — в отчёт прослойке.
async fn report_verdicts(uid: &str, key: &str, kind: &'static str, outcomes: &[(NodeRef, plan::Checked)]) {
    let verdicts: Vec<(std::string::String, Verdict, i64)> = outcomes
        .iter()
        .filter_map(|(node, checked)| Some((node.name.clone(), checked.verdict?, checked.status)))
        .collect();
    crate::module::client_report::note_freeze(uid, key, kind, &verdicts).await;
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

/// Тот ли ещё мир, в котором шёл заход: та же подписка, те же проверенные
/// узлы, та же сеть. Узлы ядра сейчас — пометки ставятся по ним: имена прочих
/// узлов могли смениться, пока шли проверки; `None` — мир уже не тот.
async fn still_the_same(uid: &str, checked: &[NodeRef], key: &str) -> Option<Vec<NodeRef>> {
    if running_subscription().await.as_deref() != Some(uid) {
        return None;
    }
    let (listing, _) = list_nodes().await?;
    // Проверенные узлы на месте и те же; что ядро тем временем догрузило
    // других, итогов не отменяет.
    let now = names_of(&listing.nodes);
    if !checked
        .iter()
        .all(|node| now.get(node.name.as_str()) == Some(&node.fingerprint.as_str()))
    {
        return None;
    }
    let now_key = AsyncHandler::spawn_blocking(network::current).await.ok().flatten();
    now_key
        .is_some_and(|now_key| now_key.hash == key)
        .then_some(listing.nodes)
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
        announce(publish(&uid, BTreeSet::new(), epoch)).await;
    }
    enabled.then_some(uid)
}

/// Итоги подписки с диска — без узлов, которых у ядра больше нет. Если ядро не
/// разложило узлы до конца, по неполному списку не чистим: стёрлись бы итоги
/// узлов, которых ядро просто ещё не показало.
async fn saved_results(uid: &str, nodes: &[NodeRef], settled: bool, now: i64) -> store::Store {
    let mut saved = store::load(uid).await;
    if settled {
        let live: BTreeSet<std::string::String> = nodes.iter().map(|node| node.fingerprint.clone()).collect();
        saved.forget_nodes_except(&live);
    }
    saved.forget_old_networks(now);
    saved
}

/// Контрольный узел к повторам обрезанных: прошедший в этом заходе, иначе
/// рабочий в этой сети свежее всех, чья последняя попытка дала итог, и в
/// заходе не проверенный. Обрезанные и их отпечатки не годятся. Нет такого —
/// сравнить не с чем.
fn wave_control(
    nodes: &[NodeRef],
    network: &store::Network,
    outcomes: &[(NodeRef, plan::Checked)],
    suspects: &[NodeRef],
) -> Option<NodeRef> {
    let doubted: BTreeSet<&str> = suspects.iter().map(|node| node.fingerprint.as_str()).collect();
    let passed = outcomes
        .iter()
        .filter(|(node, checked)| checked.verdict == Some(Verdict::Ok) && !doubted.contains(node.fingerprint.as_str()))
        .map(|(node, _)| node)
        .min_by(|a, b| a.name.cmp(&b.name));
    if let Some(passed) = passed {
        return Some(passed.clone());
    }
    let tried: BTreeSet<&str> = outcomes.iter().map(|(node, _)| node.fingerprint.as_str()).collect();
    nodes
        .iter()
        .filter(|node| !doubted.contains(node.fingerprint.as_str()) && !tried.contains(node.fingerprint.as_str()))
        .filter_map(|node| {
            let record = network.nodes.get(&node.fingerprint)?;
            (record.verdict == Some(Verdict::Ok) && record.at >= record.tried).then_some((record.at, node))
        })
        .max_by_key(|(at, _)| *at)
        .map(|(_, node)| node.clone())
}

/// Проверки узлов захода.
type Checks = Vec<(NodeRef, plan::Checked)>;

/// Повтор обрезанных: контрольный не прошёл — `None` (это сеть, а не узлы);
/// иначе снова обрезанные и прошедшие. Повтор без итога — узел выбывает.
fn after_wave(left: &[NodeRef], control: &NodeRef, again: &[(NodeRef, plan::Checked)]) -> Option<(Checks, Checks)> {
    let of = |node: &NodeRef| {
        again
            .iter()
            .find(|(checked, _)| checked.name == node.name)
            .map(|(_, checked)| *checked)
    };
    if of(control).and_then(|checked| checked.verdict) != Some(Verdict::Ok) {
        return None;
    }
    let mut cut = Vec::new();
    let mut passed = Vec::new();
    for node in left {
        match of(node) {
            Some(checked) if checked.verdict == Some(Verdict::Frozen) => cut.push((node.clone(), checked)),
            Some(checked) if checked.verdict == Some(Verdict::Ok) => passed.push((node.clone(), checked)),
            _ => {}
        }
    }
    Some((cut, passed))
}

/// Повторы обрезанных в первой проверке, `again` — одна проверка узлов
/// (контрольный первым). Итоги: прошедшие в повторе — «работает», обрезанные
/// во всех [`plan::WAVES`] проверках — «режется»; контрольный не прошёл или
/// повтор без итога — итога нет. `None` — заход не в счёт.
async fn repeated<F, Fut>(mut left: Vec<NodeRef>, control: &NodeRef, reason: &str, mut again: F) -> Option<Checks>
where
    F: FnMut(Vec<NodeRef>) -> Fut,
    Fut: std::future::Future<Output = Option<Checks>>,
{
    let mut settled = Vec::new();
    for wave in 2..=plan::WAVES {
        let mut batch = vec![control.clone()];
        batch.extend(left.iter().cloned());
        let checked = again(batch).await?;
        let Some((cut, passed)) = after_wave(&left, control, &checked) else {
            logging!(
                info,
                Type::Core,
                "[Freeze] {reason}: check {wave} of {}: {} that works here did not pass, the cut nodes are not marked",
                plan::WAVES,
                control.name
            );
            break;
        };
        logging!(
            info,
            Type::Core,
            "[Freeze] {reason}: check {wave} of {}: {} cut node(s) — {} cut again, {} passed",
            plan::WAVES,
            left.len(),
            cut.len(),
            passed.len()
        );
        settled.extend(passed);
        if wave == plan::WAVES {
            settled.extend(cut);
            break;
        }
        left = cut.into_iter().map(|(node, _)| node).collect();
        if left.is_empty() {
            break;
        }
    }
    Some(settled)
}

/// Записать итог узла в проверки захода.
fn settle(outcomes: &mut [(NodeRef, plan::Checked)], settled: Vec<(NodeRef, plan::Checked)>) {
    for (node, checked) in settled {
        if let Some(slot) = outcomes.iter_mut().find(|(known, _)| known.name == node.name) {
            slot.1 = checked;
        }
    }
}

/// Обрезанным в первой проверке не верим: через 10 с они проверяются ещё
/// дважды вместе с контрольным. «Режется» — только если обрезались все три
/// проверки, а контрольный оба раза прошёл; прошла последняя — «работает»;
/// иначе итога нет. Контрольный дописывается в `checked_nodes`. `None` — заход
/// не в счёт.
async fn confirmed<F, Fut>(
    mut outcomes: Vec<(NodeRef, plan::Checked)>,
    nodes: &[NodeRef],
    network: &store::Network,
    checked_nodes: &mut Vec<NodeRef>,
    reason: &str,
    again: F,
) -> Option<Vec<(NodeRef, plan::Checked)>>
where
    F: FnMut(Vec<NodeRef>) -> Fut,
    Fut: std::future::Future<Output = Option<Checks>>,
{
    let mut left: Vec<NodeRef> = Vec::new();
    for (node, checked) in &mut outcomes {
        if checked.verdict == Some(Verdict::Frozen) {
            checked.verdict = None;
            left.push(node.clone());
        }
    }
    if left.is_empty() {
        return Some(outcomes);
    }
    let Some(control) = wave_control(nodes, network, &outcomes, &left) else {
        logging!(
            info,
            Type::Core,
            "[Freeze] {reason}: {} node(s) cut, no node works here to compare with, not marked",
            left.len()
        );
        return Some(outcomes);
    };
    checked_nodes.push(control.clone());
    let settled = repeated(left, &control, reason, again).await?;
    settle(&mut outcomes, settled);
    Some(outcomes)
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

/// Итоги проверок, если они ещё о тех же узлах в той же сети и подписке, и
/// узлы ядра на конец захода; `None` — заход не в счёт.
async fn checked_in_place(
    uid: &str,
    due: Vec<NodeRef>,
    nodes: &[NodeRef],
    network: &store::Network,
    key: &str,
    epoch: u64,
    reason: &str,
) -> Option<(Checks, Vec<NodeRef>)> {
    let mut checked_nodes = due.clone();
    let outcomes = checked_or_dropped(due, epoch, reason).await?;
    // Повтор — через паузу; за неё могли смениться сеть или подписка, могли
    // выйти из приложения.
    let again = |batch| async move {
        tokio::time::sleep(WAVE_PAUSE).await;
        if handle::Handle::global().is_exiting() {
            return None;
        }
        if EPOCH.load(Ordering::Acquire) != epoch {
            logging!(
                info,
                Type::Core,
                "[Freeze] {reason}: the network or the subscription changed mid-pass, results dropped"
            );
            return None;
        }
        checked_or_dropped(batch, epoch, reason).await
    };
    let outcomes = confirmed(outcomes, nodes, network, &mut checked_nodes, reason, again).await?;
    let Some(now) = still_the_same(uid, &checked_nodes, key).await else {
        logging!(
            info,
            Type::Core,
            "[Freeze] {reason}: the network, the subscription or its checked nodes changed mid-pass, results dropped"
        );
        return None;
    };
    Some((outcomes, now))
}

async fn pass(reason: &'static str) {
    let epoch = EPOCH.load(Ordering::Acquire);
    *PASS_KEY.lock() = None;
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

    // Сеть не распознана — итоги не к чему привязать: ни проверок, ни пометок.
    let Some(key) = network_key().await else {
        if verbose {
            logging!(
                info,
                Type::Core,
                "[Freeze] {reason}: the network is not recognized, nothing checked"
            );
        }
        announce(publish(&uid, BTreeSet::new(), epoch)).await;
        return;
    };
    *PASS_KEY.lock() = Some(key.hash.clone());
    let now = crate::utils::help::now_secs();
    let mut saved = saved_results(&uid, &nodes, settled, now).await;
    let network = saved.network_mut(&key.hash);
    // Пометки этой сети показываются сразу, не дожидаясь проверок.
    announce(publish(&uid, marks_of(&nodes, network), epoch)).await;

    let due = due_of(&nodes, network, now);
    let checked_any = !due.is_empty();
    let mut nodes = nodes;
    if checked_any {
        let Some((outcomes, nodes_now)) = checked_in_place(&uid, due, &nodes, network, &key.hash, epoch, reason).await
        else {
            return;
        };
        recorded(network, &outcomes, now, verbose);
        report_verdicts(&uid, &key.hash, key.kind, &outcomes).await;
        let checks: Vec<plan::Checked> = outcomes.iter().map(|(_, checked)| *checked).collect();
        logging!(
            info,
            Type::Core,
            "[Freeze] {reason}: {} of {} node(s) checked in network {} — {}",
            outcomes.len(),
            nodes.len(),
            key.hash,
            counted(&checks)
        );
        nodes = nodes_now;
    } else if verbose {
        logging!(
            info,
            Type::Core,
            "[Freeze] {reason}: nothing due in network {}",
            key.hash
        );
    }

    if EPOCH.load(Ordering::Acquire) != epoch {
        return;
    }
    let by_name = marks_of(&nodes, network);
    let known = network.last_seen != 0;
    if checked_any || (known && now.saturating_sub(network.last_seen) >= TOUCH_AFTER) {
        network.last_seen = now;
        if let Err(err) = store::save(&uid, &saved).await {
            logging!(warn, Type::Core, "[Freeze] the results were not saved: {err:#}");
        }
    }
    announce(publish(&uid, by_name, epoch)).await;
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use serde_json::json;

    use super::{
        Checks, Listing, NodeRef, compares_the_network, core_settled, download_path, due_of, listing_of, marks_of,
        names_of, nodes_of, plan, recorded, repeated, store, wave_control,
    };
    use crate::config::proxy_label::Sentinels;
    use crate::feat::CoreListing;

    fn listed(proxies: serde_json::Value, providers: serde_json::Value, failed: usize) -> CoreListing {
        CoreListing {
            proxies,
            providers: providers
                .as_object()
                .into_iter()
                .flatten()
                .map(|(name, provider)| (name.as_str().into(), provider.clone()))
                .collect(),
            failed,
        }
    }

    fn production() -> &'static str {
        crate::utils::source_scan::without_test_modules(include_str!("mod.rs"))
    }

    #[test]
    fn nodes_are_listed_by_the_builds_own_providers_and_any_failed_one_means_not_laid_out() {
        use crate::utils::source_scan::fn_body;
        let list = fn_body(production(), "async fn list_nodes()").unwrap_or_default();
        assert!(list.contains("read_core_proxies("), "{list}");
        assert!(
            !list.contains("\"/providers/proxies\""),
            "общий список провайдеров повторяет узел на каждую группу: {list}"
        );

        let groups = json!({ "proxies": { "GLOBAL": { "all": [] } } });
        let sub = json!({ "sub": { "vehicleType": "HTTP", "proxies": [{ "name": "a" }] } });
        assert!(!listing_of(&listed(groups.clone(), sub.clone(), 1), &Sentinels::new()).1);
        assert!(listing_of(&listed(groups.clone(), sub, 0), &Sentinels::new()).1);
        // Провайдеров у сборки нет — ждать нечего.
        assert!(listing_of(&listed(groups.clone(), json!({}), 0), &Sentinels::new()).1);
        // Один провайдер не прочитался — не разложено, но узлы прочитанных в
        // заходе остаются.
        let read = json!({ "sub": { "vehicleType": "HTTP", "proxies": [{ "name": "a", "fingerprint": "f" }] } });
        let (listing, settled) = listing_of(&listed(groups, read, 1), &Sentinels::new());
        assert!(!settled);
        assert_eq!(names_of(&listing.nodes).get("a"), Some(&"f"), "{:?}", listing.nodes);
    }

    #[test]
    fn the_network_key_is_read_only_against_a_running_pass() {
        use crate::utils::source_scan::fn_body;
        assert!(compares_the_network(false, true));
        assert!(!compares_the_network(false, false), "захода нет — сравнивать не с чем");
        assert!(!compares_the_network(true, true), "после сна заход бросается сразу");
        let changed = fn_body(production(), "pub fn network_changed(").unwrap_or_default();
        let gate = changed.find("compares_the_network(").unwrap_or(usize::MAX);
        let read = changed.find("spawn_blocking(").unwrap_or(0);
        assert!(gate < read, "ключ сети — только при идущем заходе: {changed}");
    }

    fn node(name: &str, fingerprint: &str) -> NodeRef {
        NodeRef {
            name: name.into(),
            provider: None,
            fingerprint: fingerprint.into(),
        }
    }

    fn result(verdict: Option<plan::Verdict>) -> plan::Checked {
        plan::Checked { verdict, status: 200 }
    }

    #[test]
    fn the_wave_control_passed_in_this_pass_or_worked_here_last_time_and_is_never_a_cut_one() {
        use super::plan::{Node, Verdict};
        let nodes = [
            node("cut", "f1"),
            node("twin", "f1"),
            node("ok", "f2"),
            node("old", "f3"),
            node("silent", "f4"),
        ];
        let worked = |at, tried| Node {
            verdict: Some(Verdict::Ok),
            at,
            tried,
            ..Node::default()
        };
        let mut network = store::Network::default();
        network.nodes.insert("f1".into(), worked(300, 300));
        network.nodes.insert("f3".into(), worked(100, 100));
        // Рабочий, но последняя попытка итога не дала — в контрольные не годится.
        network.nodes.insert("f4".into(), worked(200, 250));
        let suspects = [node("cut", "f1")];
        let cut = (node("cut", "f1"), result(None));
        let passed = [cut.clone(), (node("ok", "f2"), result(Some(Verdict::Ok)))];
        assert_eq!(
            wave_control(&nodes, &network, &passed, &suspects),
            Some(node("ok", "f2"))
        );
        // Никто не прошёл — рабочий здесь, но не одноимённый обрезанному.
        let only_cut = [cut.clone()];
        assert_eq!(
            wave_control(&nodes, &network, &only_cut, &suspects),
            Some(node("old", "f3"))
        );
        // Проверенный в этом заходе и не прошедший — не контрольный.
        let old_checked = [cut, (node("old", "f3"), result(None))];
        assert_eq!(wave_control(&nodes, &network, &old_checked, &suspects), None);
        assert_eq!(wave_control(&nodes[..2], &network, &only_cut, &suspects), None);
    }

    /// Повторы на заготовленных ответах ядра: каждый ответ — итоги одной проверки.
    fn repeated_on(left: &[NodeRef], control: &NodeRef, answers: Vec<Vec<(NodeRef, plan::Checked)>>) -> Option<Checks> {
        let mut answers = answers.into_iter();
        let mut asked = 0;
        let settled = futures::executor::block_on(repeated(left.to_vec(), control, "test", |batch| {
            asked += 1;
            assert_eq!(batch.first(), Some(control), "контрольный проверяется в каждом повторе");
            let answer = answers.next();
            async move { answer }
        }));
        assert!(asked < plan::WAVES, "повторов не больше двух");
        settled
    }

    #[test]
    fn a_node_is_cut_only_when_all_three_checks_were_and_the_control_passed() {
        use super::plan::Verdict;
        let frozen = result(Some(Verdict::Frozen));
        let ok = result(Some(Verdict::Ok));
        let silent = result(None);
        let (a, k) = (node("a", "f1"), node("k", "f9"));
        let verdict_of = |settled: Option<Checks>| -> Option<Option<Verdict>> {
            settled.map(|settled| {
                settled
                    .iter()
                    .find(|(node, _)| node.name == "a")
                    .and_then(|(_, checked)| checked.verdict)
            })
        };
        let wave = |a_was: plan::Checked, k_was: plan::Checked| vec![(k.clone(), k_was), (a.clone(), a_was)];
        let cases = [
            (
                "обрезан трижды, контрольный прошёл",
                vec![wave(frozen, ok), wave(frozen, ok)],
                Some(Some(Verdict::Frozen)),
            ),
            (
                "последняя прошла",
                vec![wave(frozen, ok), wave(ok, ok)],
                Some(Some(Verdict::Ok)),
            ),
            ("прошёл во втором", vec![wave(ok, ok)], Some(Some(Verdict::Ok))),
            (
                "последняя без итога",
                vec![wave(frozen, ok), wave(silent, ok)],
                Some(None),
            ),
            (
                "контрольный не прошёл во втором",
                vec![wave(frozen, silent)],
                Some(None),
            ),
            (
                "контрольный обрезан в третьем",
                vec![wave(frozen, ok), wave(frozen, frozen)],
                Some(None),
            ),
            ("ядро замолчало", vec![wave(frozen, ok)], None),
        ];
        for (name, answers, expected) in cases {
            assert_eq!(
                verdict_of(repeated_on(std::slice::from_ref(&a), &k, answers)),
                expected,
                "{name}"
            );
        }
    }

    /// Обрезанные в первой проверке — через `confirmed` на заготовленных повторах.
    fn confirmed_on(
        outcomes: Checks,
        nodes: &[NodeRef],
        network: &store::Network,
        answers: Vec<Checks>,
    ) -> (Option<Vec<Option<plan::Verdict>>>, Vec<NodeRef>) {
        let mut answers = answers.into_iter();
        let mut checked_nodes = Vec::new();
        let settled = futures::executor::block_on(super::confirmed(
            outcomes,
            nodes,
            network,
            &mut checked_nodes,
            "test",
            |_batch| {
                let answer = answers.next();
                async move { answer }
            },
        ));
        let verdicts = settled.map(|settled| settled.iter().map(|(_, checked)| checked.verdict).collect());
        (verdicts, checked_nodes)
    }

    #[test]
    fn three_cuts_with_a_control_mark_the_node_and_one_cut_without_a_control_marks_nothing() {
        use super::plan::{Node, Verdict};
        let frozen = result(Some(Verdict::Frozen));
        let ok = result(Some(Verdict::Ok));
        let (a, b, k) = (node("a", "f1"), node("b", "f2"), node("k", "f9"));
        let nodes = [a.clone(), b.clone(), k.clone()];
        let first = vec![(a.clone(), frozen), (b.clone(), frozen)];

        // Сравнить не с чем — ни значка, ни повторов.
        let (verdicts, checked) = confirmed_on(first.clone(), &nodes[..2], &store::Network::default(), Vec::new());
        assert_eq!(verdicts, Some(vec![None, None]));
        assert!(checked.is_empty());

        // Контрольный — рабочий здесь по прошлой проверке: a обрезан трижды, b прошёл.
        let mut network = store::Network::default();
        network.nodes.insert(
            "f9".into(),
            Node {
                verdict: Some(Verdict::Ok),
                at: 100,
                tried: 100,
                ..Node::default()
            },
        );
        let answers = vec![
            vec![(k.clone(), ok), (a.clone(), frozen), (b, ok)],
            vec![(k.clone(), ok), (a, frozen)],
        ];
        let (verdicts, checked) = confirmed_on(first, &nodes, &network, answers);
        assert_eq!(verdicts, Some(vec![Some(Verdict::Frozen), Some(Verdict::Ok)]));
        assert_eq!(
            checked,
            vec![k],
            "контрольный сверяется в конце захода вместе с проверенными"
        );
    }

    #[test]
    fn a_check_without_a_result_is_written_as_an_attempt_only() {
        use super::plan::{Node, Verdict};
        const NOW: i64 = 1_800_000_000;
        let mut network = store::Network::default();
        let old = Node {
            verdict: Some(Verdict::Frozen),
            at: NOW - plan::RECHECK_AFTER,
            tried: NOW - plan::RECHECK_AFTER,
            ..Node::default()
        };
        network.nodes.insert("f1".into(), old);
        let outcomes = [
            (node("a", "f1"), result(None)),
            (node("b", "f2"), result(Some(Verdict::Ok))),
        ];
        recorded(&mut network, &outcomes, NOW, false);
        let a = network.nodes.get("f1").cloned().unwrap_or_default();
        assert_eq!(
            (a.verdict, a.at, a.tried),
            (Some(Verdict::Frozen), NOW - plan::RECHECK_AFTER, NOW)
        );
        assert!(!plan::is_due(Some(&a), NOW));
        assert!(plan::is_due(Some(&a), NOW + plan::RETRY_AFTER));
        assert_eq!(network.nodes.get("f2").and_then(|node| node.verdict), Some(Verdict::Ok));
    }

    #[test]
    fn a_freshly_started_core_is_settled_once_groups_and_link_providers_are_filled() {
        let groups = json!({ "proxies": { "GLOBAL": { "all": [] } } });
        let empty = json!({ "sub": { "vehicleType": "HTTP", "proxies": [] } });
        let filled = json!({
            "sub": { "vehicleType": "HTTP", "proxies": [{ "name": "a" }] },
            "default": { "vehicleType": "Compatible", "proxies": [] },
        });
        assert!(!core_settled(&listed(json!({ "proxies": {} }), filled.clone(), 0)));
        assert!(!core_settled(&listed(groups.clone(), empty, 0)));
        assert!(core_settled(&listed(groups, filled, 0)));
    }

    #[test]
    fn only_nodes_with_a_fingerprint_are_listed_once_each() {
        let proxies = json!({ "proxies": {
            "GLOBAL": { "type": "Selector", "all": ["a"] },
            "DIRECT": { "type": "Direct" },
            "a": { "type": "VLESS", "fingerprint": "f1" },
            "b": { "type": "VLESS", "fingerprint": "" },
        }});
        let providers = json!({
            "default": { "proxies": [ { "name": "a", "fingerprint": "f1" } ] },
            "sub": { "proxies": [ { "name": "c", "fingerprint": "f2" }, { "name": "d", "type": "Trojan" } ] },
        });
        let listing = nodes_of(&listed(proxies.clone(), providers, 0), &Sentinels::new());
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
        assert_eq!(
            nodes_of(&listed(proxies, json!({}), 0), &Sentinels::new()).nodes.len(),
            1
        );
    }

    #[test]
    fn the_providers_placeholders_are_never_checked() {
        let proxies = json!({ "proxies": {
            "Срок истёк": { "type": "VLESS", "fingerprint": "f0" },
            "a": { "type": "VLESS", "fingerprint": "f1" },
        }});
        let providers = json!({
            "sub": { "proxies": [
                { "name": "Лимит устройств", "fingerprint": "f2" },
                { "name": "c", "fingerprint": "f3" },
            ] },
        });
        let sentinels: Sentinels = [
            (String::new(), "Срок истёк".to_owned()),
            ("sub".to_owned(), "Лимит устройств".to_owned()),
            // Заглушка другого провайдера с тем же именем — не эта.
            ("other".to_owned(), "c".to_owned()),
        ]
        .into_iter()
        .collect();
        let listing = nodes_of(&listed(proxies, providers, 0), &sentinels);
        let names: Vec<&str> = listing.nodes.iter().map(|node| node.name.as_str()).collect();
        assert_eq!(names, ["a", "c"]);
        assert_eq!(listing.without_fingerprint, 0);
    }

    #[test]
    fn a_foreign_core_differs_from_an_empty_subscription() {
        let foreign = json!({ "proxies": {
            "GLOBAL": { "type": "Selector", "all": ["a"] },
            "a": { "type": "VLESS" },
        }});
        assert_eq!(
            nodes_of(&listed(foreign, json!({}), 0), &Sentinels::new()),
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
        assert_eq!(
            nodes_of(&listed(empty, json!({}), 0), &Sentinels::new()),
            Listing::default()
        );
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
        let nodes = vec![
            node("a", "f1"),
            node("a2", "f1"),
            node("b", "f2"),
            node("c", "f3"),
            node("d", "f4"),
        ];
        let marks = marks_of(&nodes, &network);
        let expected: BTreeSet<std::string::String> = ["a".to_owned(), "a2".to_owned()].into_iter().collect();
        assert_eq!(marks, expected);
    }
}
