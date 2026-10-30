//! clod:traffic-estimate — оценка расхода трафика между обновлениями подписки.
//!
//! Панель Remnawave пересчитывает расход не чаще раза в час, поэтому число из
//! `subscription-userinfo` почти всегда отстаёт. Значение из подписки остаётся
//! истиной; поверх него клиент досчитывает то, что прошло через прокси уже
//! после неё, и интерфейс честно помечает такую сумму как примерную.
//!
//! Считаем по `/connections`: у каждого соединения ядро отдаёт накопленные
//! `upload`/`download`, поэтому достаточно складывать приросты по `id`.
//! Соединения с встроенными исходами ядра пропускаем — панель их тоже не
//! видит. Это `DIRECT`, отбивающие `REJECT` и `REJECT-DROP`, пропускающие
//! `PASS` и `PASS-RULE`, а также `COMPATIBLE`, которым ядро заполняет
//! опустевшую группу. Прямой выход, заведённый в самой подписке под своим
//! именем, счёт от прокси не отличит. Соединение, успевшее открыться и
//! закрыться между двумя опросами, теряется: счёт занижен, но никогда не
//! завышен — именно поэтому он и называется примерным.
//!
//! Таблицу соединений приносит общий опрос (`core::connections_poll`): в срок
//! оценки, а пока ради отчёта он ходит к ядру чаще — каждый его снимок, и
//! потерь меньше. Файл пишется всё равно в срок оценки.
//!
//! У каждой подписки своя оценка и свой файл; файл уходит вместе с подпиской.

use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::Duration;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::utils::{dirs, help};
use clash_verge_logging::{Type, logging};

/// Как часто опрашиваем ядро, когда счёт вообще нужен.
///
/// clod: раньше опрос шёл каждые пять секунд — 720 запросов в час к таблице
/// соединений ради числа, на которое смотрят раз в день. Теперь частота идёт
/// от того, как часто обновляется сама подписка: чем свежее данные от сервиса,
/// тем меньше досчитывать. Верхняя граница нужна не из вредности — соединение,
/// успевшее открыться и закрыться между опросами, в счёт не попадает вовсе,
/// так что редкий опрос занижает результат тем сильнее, чем он реже.
const SAMPLE_MIN: Duration = Duration::from_secs(30);
const SAMPLE_MAX: Duration = Duration::from_secs(300);
/// Подписка обновляется не реже этого — досчитывать нечего, счёт выключен.
const SUBSCRIPTION_FRESH_MINUTES: u64 = 60;
/// Раз во столько сроков оценки состояние сбрасывается на диск (если
/// изменилось) — сколько бы снимков ни пришло между ними.
const PERSIST_EVERY_TICKS: u32 = 4;
/// Общий файл прежних версий: одна оценка на все подписки.
const LEGACY_STATE_FILE: &str = "traffic_estimate.json";

/// Снимок счётчика для фронтенда.
#[derive(Default, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrafficEstimate {
    /// uid профиля, к которому относится счёт.
    pub profile: String,
    /// `upload` из подписки на момент, когда счёт был обнулён.
    pub baseline_upload: u64,
    /// `download` из подписки на тот же момент.
    pub baseline_download: u64,
    /// Сколько байт клиент насчитал сверх базы.
    pub local_bytes: u64,
    /// Unix-секунды: когда база последний раз менялась, то есть когда данные
    /// подписки были точными.
    pub baseline_at: i64,
}

/// Оценка подписки и то, что лежит в её файле.
#[derive(Default)]
struct Kept {
    estimate: TrafficEstimate,
    /// Что лежит в файле: то же — писать нечего.
    saved: Option<TrafficEstimate>,
}

#[derive(Default)]
struct Runtime {
    /// uid подписки → её оценка.
    estimates: HashMap<String, Kept>,
    /// Подписка, на сборке которой работает ядро; `None` — с запуска ядро
    /// другую подписку не принимало.
    counting: Option<String>,
    /// Когда (мс) ядро приняло её сборку: соединения, открытые раньше, — не её.
    since: i64,
    /// `id` соединения → уже учтённые байты по нему.
    seen: HashMap<String, u64>,
    /// Первый опрос после запуска только запоминает счётчики соединений.
    /// Без этого уже открытые соединения принесли бы в расход всю свою
    /// историю — единственный способ завысить счёт, и его надо исключить.
    primed: bool,
    /// Сколько сроков оценки прошло.
    ticks: u32,
    /// Для какой подписки досчитанное уже обнулено, пока счёт выключен.
    cleared: Option<Option<String>>,
}

fn runtime() -> &'static Mutex<Runtime> {
    static RUNTIME: OnceLock<Mutex<Runtime>> = OnceLock::new();
    RUNTIME.get_or_init(|| Mutex::new(Runtime::default()))
}

/// Запись файлов и их удаление вместе с подпиской идут по одной.
static FILES: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn state_path(uid: &str) -> Option<std::path::PathBuf> {
    dirs::app_home_dir()
        .ok()
        .map(|dir| dir.join(format!("traffic-{uid}.json")))
}

async fn load_persisted(path: &std::path::Path) -> Option<TrafficEstimate> {
    let raw = tokio::fs::read_to_string(path).await.ok()?;
    serde_json::from_str::<TrafficEstimate>(&raw).ok()
}

/// Поднять оценку подписки из её файла, если её ещё нет в памяти.
async fn ensure_loaded(uid: &str) {
    if runtime().lock().estimates.contains_key(uid) {
        return;
    }
    let persisted = match state_path(uid) {
        Some(path) => load_persisted(&path).await,
        None => None,
    }
    .unwrap_or_default();
    runtime()
        .lock()
        .estimates
        .entry(uid.to_owned())
        .or_insert_with(|| Kept {
            saved: Some(persisted.clone()),
            estimate: persisted,
        });
}

/// Оценка, которую пора записать: она не та, что в файле. С этого момента
/// считается записанной.
fn unsaved(kept: &mut Kept) -> Option<TrafficEstimate> {
    if kept.saved.as_ref() == Some(&kept.estimate) {
        return None;
    }
    kept.saved = Some(kept.estimate.clone());
    kept.saved.clone()
}

/// Записать оценки, изменившиеся с прошлой записи. Подписки, которой уже нет
/// в реестре, файл не получает.
pub(crate) async fn save() {
    let _files = FILES.lock().await;
    let pending: Vec<(String, TrafficEstimate)> = {
        let mut guard = runtime().lock();
        guard
            .estimates
            .iter_mut()
            .filter_map(|(uid, kept)| unsaved(kept).map(|estimate| (uid.clone(), estimate)))
            .collect()
    };
    for (uid, estimate) in pending {
        if Config::profiles().await.latest_arc().get_item(&uid).is_err() {
            runtime().lock().estimates.remove(&uid);
            continue;
        }
        let Some(path) = state_path(&uid) else { continue };
        if let Err(err) = help::save_json(&path, &estimate).await {
            logging!(warn, Type::Core, "не удалось сохранить счётчик трафика: {err:#}");
            // Записать ещё раз в следующий срок.
            if let Some(kept) = runtime().lock().estimates.get_mut(&uid) {
                kept.saved = None;
            }
        }
    }
}

/// Подписку удалили — её оценка уходит вместе с ней.
pub async fn forget(uid: &str) {
    let _files = FILES.lock().await;
    runtime().lock().estimates.remove(uid);
    if let Some(path) = state_path(uid) {
        let _ = tokio::fs::remove_file(path).await;
    }
}

/// Ядро приняло сборку подписки `uid`: её соединения считаются с этого
/// момента, открытые раньше — прежней подписки.
pub(crate) fn subscription_changed(uid: Option<&str>) {
    let mut guard = runtime().lock();
    guard.counting = uid.map(str::to_owned);
    guard.since = chrono::Utc::now().timestamp_millis();
}

/// Подписка, чьи соединения сейчас идут через ядро.
async fn counted_uid() -> Option<String> {
    let switched = runtime().lock().counting.clone();
    match switched {
        Some(uid) => Some(uid),
        None => Config::runtime()
            .await
            .data_arc()
            .profile_uid
            .as_deref()
            .map(str::to_owned),
    }
}

/// Как часто опрашивать ядро для подписки, на которой оно работает.
///
/// `None` — счёт не нужен: подписка сама обновляется достаточно часто, и число
/// от сервиса и так свежее нашей оценки.
async fn sample_interval() -> Option<Duration> {
    let uid = counted_uid().await?;
    let profiles = Config::profiles().await.latest_arc();
    let minutes = profiles
        .get_item(&uid)
        .ok()
        .and_then(|item| item.option.as_ref().and_then(|option| option.update_interval))
        .unwrap_or(0) as u64;

    interval_for_minutes(minutes)
}

/// Правило частоты в чистом виде — его и проверяет тест.
const fn interval_for_minutes(minutes: u64) -> Option<Duration> {
    // Автообновления нет вовсе — данные от сервиса стареют сами по себе,
    // считаем с обычной частотой.
    if minutes == 0 {
        return Some(SAMPLE_MIN);
    }
    if minutes <= SUBSCRIPTION_FRESH_MINUTES {
        return None;
    }
    // Половина интервала подписки, но не чаще SAMPLE_MIN и не реже SAMPLE_MAX.
    let half = Duration::from_secs(minutes * 30);
    Some(if half.as_secs() < SAMPLE_MIN.as_secs() {
        SAMPLE_MIN
    } else if half.as_secs() > SAMPLE_MAX.as_secs() {
        SAMPLE_MAX
    } else {
        half
    })
}

/// Подписка, чьи соединения идут через ядро, и её данные из подписки.
async fn current_subscription() -> Option<(String, u64, u64)> {
    let uid = counted_uid().await?;
    let profiles = Config::profiles().await.latest_arc();
    let item = profiles.get_item(&uid).ok()?;
    let extra = item.extra.as_ref()?;
    Some((uid, extra.upload, extra.download))
}

/// Считается ли трафик этого соединения расходом подписки.
fn counts_as_proxy(chains: &[String]) -> bool {
    // `chains[0]` — исходящий, на котором соединение реально держится.
    chains
        .first()
        .is_some_and(|outbound| !crate::constants::policies::is_builtin(outbound))
}

/// Сверить базу с подпиской. База меняется только когда изменилось само
/// значение — не на каждое обновление подписки: панель отдаёт одно и то же
/// число до конца часа, а сброс «на каждый апдейт» стирал бы весь счёт.
fn reconcile(runtime: &mut Runtime, uid: &str, upload: u64, download: u64) -> bool {
    let estimate = &mut runtime.estimates.entry(uid.to_owned()).or_default().estimate;
    if estimate.profile == uid && estimate.baseline_upload == upload && estimate.baseline_download == download {
        return false;
    }
    estimate.profile = uid.to_owned();
    estimate.baseline_upload = upload;
    estimate.baseline_download = download;
    estimate.local_bytes = 0;
    estimate.baseline_at = help::now_secs();
    // `seen` НЕ чистим: там лежат текущие счётчики открытых соединений, и
    // именно от них надо считать дальше. Очистка означала бы «посчитать их
    // с нуля ещё раз» — то есть удвоить трафик долгоживущих соединений.
    true
}

/// Как часто оценке нужен снимок соединений; `None` — счёт не нужен, и
/// досчитанное сбрасывается.
pub(crate) async fn sample_every() -> Option<Duration> {
    let every = sample_interval().await;
    if every.is_none() {
        // Счёт выключен: обнуляем досчитанное, чтобы интерфейс показывал число
        // подписки как есть, без пометки «≈».
        clear_local().await;
    } else {
        runtime().lock().cleared = None;
    }
    every
}

/// Пора ли писать файл: `term` — снимок пришёл в срок оценки, а не чаще.
const fn persist_now(runtime: &mut Runtime, term: bool) -> bool {
    if !term {
        return false;
    }
    runtime.ticks = runtime.ticks.wrapping_add(1);
    runtime.ticks.is_multiple_of(PERSIST_EVERY_TICKS)
}

/// Снимок соединений; `term` — наступил срок оценки. `None` — ядро не ответило.
pub(crate) async fn count(response: Option<&tauri_plugin_mihomo::models::Connections>, term: bool) {
    let Some((uid, upload, download)) = current_subscription().await else {
        return;
    };
    ensure_loaded(&uid).await;

    let reset = {
        let mut guard = runtime().lock();
        reconcile(&mut guard, &uid, upload, download)
    };
    if reset {
        save().await;
    }

    // Ядро может быть ещё не поднято или уже остановлено — это штатно,
    // шуметь в лог на каждый опрос не за чем.
    let Some(connections) = response.and_then(|response| response.connections.as_deref()) else {
        return;
    };

    let should_persist = {
        let mut guard = runtime().lock();
        if !take_traffic(&mut guard, &uid, connections) {
            return;
        }
        persist_now(&mut guard, term)
    };

    if should_persist {
        save().await;
    }
}

/// Прирост соединений подписки `uid` с прошлого снимка — в её оценку.
/// `false` — ядро уже приняло другую подписку, и снимок не её.
fn take_traffic(runtime: &mut Runtime, uid: &str, connections: &[tauri_plugin_mihomo::models::Connection]) -> bool {
    if runtime.counting.as_deref().is_some_and(|counting| counting != uid) {
        return false;
    }
    let mut alive = HashMap::with_capacity(connections.len());
    let mut added = 0_u64;
    for connection in connections {
        if !counts_as_proxy(&connection.chains)
            || crate::module::client_report::opened_before(&connection.start, runtime.since)
        {
            continue;
        }
        let total = connection.upload.saturating_add(connection.download);
        let counted = runtime.seen.get(&connection.id).copied().unwrap_or_default();
        // Перезапуск ядра раздаёт новые `id`, так что отрицательных
        // приростов быть не может; `saturating_sub` — страховка.
        added = added.saturating_add(total.saturating_sub(counted));
        alive.insert(connection.id.clone(), total);
    }
    if !runtime.primed {
        // Первый опрос: соединения могли жить ещё до запуска приложения,
        // их прошлое в расход не идёт.
        added = 0;
        runtime.primed = true;
    }
    // Закрытые соединения уходят из снимка: их последние байты уже учтены,
    // держать их в карте больше не нужно.
    runtime.seen = alive;
    let estimate = &mut runtime.estimates.entry(uid.to_owned()).or_default().estimate;
    estimate.local_bytes = estimate.local_bytes.saturating_add(added);
    true
}

/// Состояние счётчика подписки `uid`.
pub async fn snapshot(uid: &str) -> TrafficEstimate {
    if Config::profiles().await.latest_arc().get_item(uid).is_err() {
        return TrafficEstimate::default();
    }
    ensure_loaded(uid).await;
    runtime()
        .lock()
        .estimates
        .get(uid)
        .map(|kept| kept.estimate.clone())
        .unwrap_or_default()
}

/// Сбросить досчитанное, оставив базу подписки. Нужно, когда счёт выключается:
/// иначе последняя оценка застыла бы на карточке как вечная правда.
async fn clear_local() {
    let uid = counted_uid().await;
    if runtime().lock().cleared.as_ref() == Some(&uid) {
        return;
    }
    if let Some(uid) = &uid {
        ensure_loaded(uid).await;
    }
    {
        let mut guard = runtime().lock();
        if let Some(kept) = uid.as_ref().and_then(|uid| guard.estimates.get_mut(uid)) {
            kept.estimate.local_bytes = 0;
        }
        guard.seen.clear();
        guard.primed = false;
    }
    save().await;
    // Не записалось — обнулить и записать ещё раз на следующем круге.
    let mut guard = runtime().lock();
    if zero_is_written(&guard, uid.as_ref()) {
        guard.cleared = Some(uid);
    }
}

/// Обнулённое досчитанное подписки `uid` лежит в файле (запись не сорвалась).
fn zero_is_written(runtime: &Runtime, uid: Option<&String>) -> bool {
    uid.is_none_or(|uid| runtime.estimates.get(uid).is_none_or(|kept| kept.saved.is_some()))
}

/// Общий файл прежних версий становится файлом своей подписки. Опрос ядра для
/// оценки ведёт `core::connections_poll`.
pub async fn init() {
    let Ok(home) = dirs::app_home_dir() else { return };
    let legacy = home.join(LEGACY_STATE_FILE);
    let Some(persisted) = load_persisted(&legacy).await else {
        let _ = tokio::fs::remove_file(&legacy).await;
        return;
    };
    let uid = persisted.profile.clone();
    if Config::profiles().await.latest_arc().get_item(&uid).is_ok()
        && let Some(path) = state_path(&uid)
        && !tokio::fs::try_exists(&path).await.unwrap_or(true)
        && help::save_json(&path, &persisted).await.is_err()
    {
        return;
    }
    // Окно могло спросить оценку раньше: пусть перечитает её из файла.
    runtime().lock().estimates.remove(&uid);
    let _ = tokio::fs::remove_file(&legacy).await;
}

#[cfg(test)]
mod tests {
    use super::{
        Kept, PERSIST_EVERY_TICKS, Runtime, SAMPLE_MAX, SAMPLE_MIN, counts_as_proxy, interval_for_minutes, persist_now,
        reconcile, take_traffic, unsaved, zero_is_written,
    };
    use tauri_plugin_mihomo::models::Connection;

    fn local(runtime: &Runtime, uid: &str) -> u64 {
        runtime
            .estimates
            .get(uid)
            .map(|kept| kept.estimate.local_bytes)
            .unwrap_or_default()
    }

    fn connection(id: &str, start: &str, bytes: u64) -> Connection {
        Connection {
            id: id.to_owned(),
            start: start.to_owned(),
            download: bytes,
            chains: vec!["Netherlands".to_owned()],
            ..Connection::default()
        }
    }

    #[test]
    fn sampling_follows_the_subscription_interval() {
        // Подписка свежая — считать нечего.
        assert_eq!(interval_for_minutes(30), None);
        assert_eq!(interval_for_minutes(60), None);
        // Раз в два часа — опрос раз в час, но не реже потолка.
        assert_eq!(interval_for_minutes(120), Some(SAMPLE_MAX));
        // Чуть больше часа — половина интервала, пока она укладывается в потолок.
        assert_eq!(
            interval_for_minutes(70),
            Some(super::Duration::from_secs(2100).min(SAMPLE_MAX))
        );
        // Автообновления нет — считаем с обычной частотой.
        assert_eq!(interval_for_minutes(0), Some(SAMPLE_MIN));
    }

    fn chains(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn builtin_outbounds_are_not_traffic() {
        assert!(!counts_as_proxy(&chains(&["DIRECT"])));
        assert!(!counts_as_proxy(&chains(&["COMPATIBLE", "Основная"])));
        assert!(!counts_as_proxy(&chains(&["REJECT", "Правила"])));
        assert!(!counts_as_proxy(&chains(&["REJECT-DROP"])));
        assert!(!counts_as_proxy(&chains(&["PASS"])));
        assert!(!counts_as_proxy(&chains(&["PASS-RULE"])));
        assert!(!counts_as_proxy(&[]));
    }

    #[test]
    fn proxied_connections_are_traffic() {
        assert!(counts_as_proxy(&chains(&["Netherlands", "Основная"])));
    }

    #[test]
    fn baseline_resets_only_when_the_subscription_value_changes() {
        let mut runtime = Runtime::default();
        assert!(reconcile(&mut runtime, "uid", 10, 20));
        runtime
            .estimates
            .entry("uid".to_owned())
            .or_default()
            .estimate
            .local_bytes = 4096;

        // тот же ответ панели — счёт продолжается
        assert!(!reconcile(&mut runtime, "uid", 10, 20));
        assert_eq!(local(&runtime, "uid"), 4096);

        // панель пересчитала расход — начинаем заново
        assert!(reconcile(&mut runtime, "uid", 10, 40));
        assert_eq!(local(&runtime, "uid"), 0);
    }

    #[test]
    fn baseline_reset_keeps_connection_counters() {
        let mut runtime = Runtime::default();
        runtime.seen.insert("conn-1".to_owned(), 1024);
        reconcile(&mut runtime, "uid", 1, 2);
        // счётчики открытых соединений переживают сверку — иначе их трафик
        // будет посчитан заново поверх нового значения подписки
        assert_eq!(runtime.seen.get("conn-1"), Some(&1024));
    }

    #[test]
    fn the_file_is_written_only_when_the_estimate_changed() {
        let mut kept = Kept::default();
        assert!(unsaved(&mut kept).is_some(), "в файле ещё ничего");
        assert!(unsaved(&mut kept).is_none(), "то же — писать нечего");
        kept.estimate.local_bytes = 1;
        assert_eq!(unsaved(&mut kept).map(|estimate| estimate.local_bytes), Some(1));
        assert!(unsaved(&mut kept).is_none());

        let source = include_str!("traffic_estimate.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap_or_default();
        assert!(!source.contains("std::fs::write"), "запись — только атомарная");
        assert!(source.contains("help::save_json("), "атомарная запись — общая");
    }

    #[test]
    fn the_file_is_written_in_the_estimate_term_not_on_every_snapshot() {
        let mut runtime = Runtime::default();
        // Снимки чаще срока оценки (опрос ради отчёта) файл не пишут.
        for _ in 0..PERSIST_EVERY_TICKS * 10 {
            assert!(!persist_now(&mut runtime, false));
        }
        // В срок — как прежде: раз в PERSIST_EVERY_TICKS сроков.
        let written = (0..PERSIST_EVERY_TICKS * 3)
            .filter(|_| persist_now(&mut runtime, true))
            .count();
        assert_eq!(written, 3);
    }

    #[test]
    fn a_zero_that_was_not_written_is_written_again() {
        let mut runtime = Runtime::default();
        let uid = "first".to_owned();
        assert!(zero_is_written(&runtime, None), "подписки нет — писать нечего");
        assert!(zero_is_written(&runtime, Some(&uid)), "оценки нет — писать нечего");

        runtime.estimates.insert(uid.clone(), Kept::default());
        runtime.estimates.entry(uid.clone()).or_default().saved = Some(Default::default());
        assert!(zero_is_written(&runtime, Some(&uid)));

        runtime.estimates.entry(uid.clone()).or_default().saved = None;
        assert!(!zero_is_written(&runtime, Some(&uid)), "запись сорвалась — повторить");
    }

    #[test]
    fn each_subscription_keeps_its_own_count() {
        let mut runtime = Runtime {
            primed: true,
            ..Runtime::default()
        };
        reconcile(&mut runtime, "first", 1, 2);
        assert!(take_traffic(
            &mut runtime,
            "first",
            &[connection("a", "2026-01-01T09:00:00Z", 512)]
        ));
        reconcile(&mut runtime, "second", 1, 2);
        assert_eq!(local(&runtime, "second"), 0);
        assert!(!reconcile(&mut runtime, "first", 1, 2), "та же база — счёт тот же");
        assert_eq!(local(&runtime, "first"), 512);
    }

    #[test]
    fn connections_of_the_previous_subscription_do_not_count_for_the_next() {
        let mut runtime = Runtime {
            primed: true,
            ..Runtime::default()
        };
        let before = "2026-01-01T09:00:00Z";
        assert!(take_traffic(&mut runtime, "first", &[connection("old", before, 100)]));
        assert_eq!(local(&runtime, "first"), 100);

        runtime.counting = Some("second".to_owned());
        runtime.since = chrono::DateTime::parse_from_rfc3339("2026-01-01T10:00:00Z")
            .map(|at| at.timestamp_millis())
            .unwrap_or_default();
        assert!(take_traffic(
            &mut runtime,
            "second",
            &[
                connection("old", before, 500),
                connection("new", "2026-01-01T10:00:01Z", 70)
            ]
        ));
        assert_eq!(local(&runtime, "second"), 70, "соединение прежней подписки не в счёт");
        assert_eq!(local(&runtime, "first"), 100);

        assert!(
            !take_traffic(&mut runtime, "first", &[connection("new", before, 900)]),
            "снимок, снятый до смены, к новой подписке не идёт"
        );
        assert_eq!(local(&runtime, "first"), 100);
    }
}
