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

use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::{Duration, SystemTime};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::core::handle;
use crate::process::AsyncHandler;
use crate::utils::dirs;
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
/// Как часто перечитываем настройки, когда счёт выключен.
const IDLE_INTERVAL: Duration = Duration::from_secs(300);
/// Раз во столько опросов состояние сбрасывается на диск.
const PERSIST_EVERY_TICKS: u32 = 4;
const STATE_FILE: &str = "traffic_estimate.json";

/// Цепочки, которые не идут через прокси подписки и в расход не попадают.
const BYPASS_CHAINS: [&str; 6] = ["DIRECT", "COMPATIBLE", "REJECT", "REJECT-DROP", "PASS", "PASS-RULE"];

/// Снимок счётчика для фронтенда.
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
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

#[derive(Default)]
struct Runtime {
    estimate: TrafficEstimate,
    /// `id` соединения → уже учтённые байты по нему.
    seen: HashMap<String, u64>,
    /// Первый опрос после запуска только запоминает счётчики соединений.
    /// Без этого уже открытые соединения принесли бы в расход всю свою
    /// историю — единственный способ завысить счёт, и его надо исключить.
    primed: bool,
    ticks: u32,
}

fn runtime() -> &'static Mutex<Runtime> {
    static RUNTIME: OnceLock<Mutex<Runtime>> = OnceLock::new();
    RUNTIME.get_or_init(|| Mutex::new(Runtime::default()))
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |value| i64::try_from(value.as_secs()).unwrap_or(i64::MAX))
}

fn state_path() -> Option<std::path::PathBuf> {
    dirs::app_home_dir().ok().map(|dir| dir.join(STATE_FILE))
}

fn load_persisted() -> Option<TrafficEstimate> {
    let path = state_path()?;
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str::<TrafficEstimate>(&raw).ok()
}

fn persist(estimate: &TrafficEstimate) {
    let Some(path) = state_path() else { return };
    let Ok(raw) = serde_json::to_string(estimate) else {
        return;
    };
    if let Err(err) = std::fs::write(path, raw) {
        logging!(warn, Type::Core, "не удалось сохранить счётчик трафика: {err}");
    }
}

/// Как часто опрашивать ядро для текущего профиля.
///
/// `None` — счёт не нужен: подписка сама обновляется достаточно часто, и число
/// от сервиса и так свежее нашей оценки.
async fn sample_interval() -> Option<Duration> {
    let profiles = Config::profiles().await.latest_arc();
    let uid = profiles.current.clone()?;
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

/// Текущий профиль и его данные из подписки.
async fn current_subscription() -> Option<(String, u64, u64)> {
    let profiles = Config::profiles().await.latest_arc();
    let uid = profiles.current.clone()?;
    let item = profiles.get_item(&uid).ok()?;
    let extra = item.extra.as_ref()?;
    Some((uid.to_string(), extra.upload, extra.download))
}

/// Считается ли трафик этого соединения расходом подписки.
fn counts_as_proxy(chains: &[String]) -> bool {
    // `chains[0]` — исходящий, на котором соединение реально держится.
    chains
        .first()
        .is_some_and(|outbound| !BYPASS_CHAINS.contains(&outbound.as_str()))
}

/// Сверить базу с подпиской. База меняется только когда изменилось само
/// значение — не на каждое обновление подписки: панель отдаёт одно и то же
/// число до конца часа, а сброс «на каждый апдейт» стирал бы весь счёт.
fn reconcile(runtime: &mut Runtime, uid: &str, upload: u64, download: u64) -> bool {
    let estimate = &mut runtime.estimate;
    if estimate.profile == uid && estimate.baseline_upload == upload && estimate.baseline_download == download {
        return false;
    }
    estimate.profile = uid.to_owned();
    estimate.baseline_upload = upload;
    estimate.baseline_download = download;
    estimate.local_bytes = 0;
    estimate.baseline_at = now_secs();
    // `seen` НЕ чистим: там лежат текущие счётчики открытых соединений, и
    // именно от них надо считать дальше. Очистка означала бы «посчитать их
    // с нуля ещё раз» — то есть удвоить трафик долгоживущих соединений.
    true
}

async fn tick() {
    let Some((uid, upload, download)) = current_subscription().await else {
        return;
    };

    let reset = {
        let mut guard = runtime().lock();
        reconcile(&mut guard, &uid, upload, download)
    };
    if reset {
        persist(&snapshot());
    }

    let mihomo = handle::Handle::mihomo().await;
    let Ok(response) = mihomo.get_connections().await else {
        // Ядро может быть ещё не поднято или уже остановлено — это штатно,
        // шуметь в лог на каждый опрос не за чем.
        return;
    };
    drop(mihomo);
    let Some(connections) = response.connections else {
        return;
    };

    let (estimate, should_persist) = {
        let mut guard = runtime().lock();
        let mut alive = HashMap::with_capacity(connections.len());
        let mut added = 0_u64;
        for connection in connections {
            if !counts_as_proxy(&connection.chains) {
                continue;
            }
            let total = connection.upload.saturating_add(connection.download);
            let counted = guard.seen.get(&connection.id).copied().unwrap_or_default();
            // Перезапуск ядра раздаёт новые `id`, так что отрицательных
            // приростов быть не может; `saturating_sub` — страховка.
            added = added.saturating_add(total.saturating_sub(counted));
            alive.insert(connection.id, total);
        }
        if !guard.primed {
            // Первый опрос: соединения могли жить ещё до запуска приложения,
            // их прошлое в расход не идёт.
            added = 0;
            guard.primed = true;
        }
        // Закрытые соединения уходят из снимка: их последние байты уже учтены,
        // держать их в карте больше не нужно.
        guard.seen = alive;
        guard.estimate.local_bytes = guard.estimate.local_bytes.saturating_add(added);
        guard.ticks = guard.ticks.wrapping_add(1);
        let due = guard.ticks.is_multiple_of(PERSIST_EVERY_TICKS);
        (guard.estimate.clone(), due)
    };

    if should_persist {
        persist(&estimate);
    }
}

/// Текущее состояние счётчика.
pub fn snapshot() -> TrafficEstimate {
    runtime().lock().estimate.clone()
}

/// Сбросить досчитанное, оставив базу подписки. Нужно, когда счёт выключается:
/// иначе последняя оценка застыла бы на карточке как вечная правда.
fn clear_local() {
    let changed = {
        let mut guard = runtime().lock();
        let had = guard.estimate.local_bytes != 0;
        guard.estimate.local_bytes = 0;
        guard.seen.clear();
        guard.primed = false;
        had
    };
    if changed {
        persist(&snapshot());
    }
}

/// Поднять счётчик и запустить опрос ядра.
pub fn init() {
    if let Some(persisted) = load_persisted() {
        runtime().lock().estimate = persisted;
    }

    AsyncHandler::spawn(|| async {
        loop {
            if handle::Handle::global().is_exiting() {
                let estimate = snapshot();
                persist(&estimate);
                break;
            }
            match sample_interval().await {
                Some(interval) => {
                    tick().await;
                    tokio::time::sleep(interval).await;
                }
                None => {
                    // Счёт выключен: обнуляем досчитанное, чтобы интерфейс
                    // показывал число подписки как есть, без пометки «≈».
                    clear_local();
                    tokio::time::sleep(IDLE_INTERVAL).await;
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{Runtime, SAMPLE_MAX, SAMPLE_MIN, counts_as_proxy, interval_for_minutes, reconcile};

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
        runtime.estimate.local_bytes = 4096;

        // тот же ответ панели — счёт продолжается
        assert!(!reconcile(&mut runtime, "uid", 10, 20));
        assert_eq!(runtime.estimate.local_bytes, 4096);

        // панель пересчитала расход — начинаем заново
        assert!(reconcile(&mut runtime, "uid", 10, 40));
        assert_eq!(runtime.estimate.local_bytes, 0);
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
    fn switching_profile_resets_the_counter() {
        let mut runtime = Runtime::default();
        reconcile(&mut runtime, "first", 1, 2);
        runtime.estimate.local_bytes = 512;
        assert!(reconcile(&mut runtime, "second", 1, 2));
        assert_eq!(runtime.estimate.local_bytes, 0);
    }
}
