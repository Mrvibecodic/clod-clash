use crate::{
    config::{Config, PrfItem},
    feat,
    process::AsyncHandler,
    singleton,
    utils::resolve::is_resolve_done,
};
use anyhow::Result;
use clash_verge_logging::{Type, logging, logging_error};
use parking_lot::{Mutex, RwLock};
use smartstring::alias::String;
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{
    sync::mpsc,
    time::{sleep, timeout},
};
use tokio_stream::StreamExt as _;
use tokio_util::time::{DelayQueue, delay_queue::Key};

/// clod: запас после срока из `subscription-userinfo`, прежде чем спрашивать
/// панель заново. Remnawave переводит пользователя в EXPIRED не в момент
/// истечения, а кроном раз в 30 с (`FIND_EXPIRED_USERS`); запрос ровно в срок
/// получил бы ещё прежний конфиг.
const EXPIRY_GRACE: Duration = Duration::from_secs(90);

/// Поправка часов снимается из заголовка `Date` с точностью до секунды и
/// между двумя загрузками дрожит на ±1 с; загрузка, ушедшая за считанные
/// секунды до дедлайна, — это загрузка в срок, а не «ещё до него», иначе за
/// ней сразу шла бы вторая. Много меньше запаса `EXPIRY_GRACE`.
const EXPIRY_SLACK: i64 = 5;

/// clod: провал загрузки по расписанию повторяется с бэкоффом от вот столького
/// до `FAILURE_RETRY_MAX`, но не позже обычного тика (как у WorkManager на
/// Android), а не через весь интервал подписки: разовый провал — сеть ещё не
/// поднялась после сна или старта, панель мигнула — иначе стоил бы суток. Цель
/// «загрузить после срока» стоит без сдачи, пока загрузка после срока не удастся.
const FAILURE_RETRY: Duration = Duration::from_secs(15 * 60);
const FAILURE_RETRY_MAX: Duration = Duration::from_secs(5 * 60 * 60);
/// Через сколько повторить задачу, которая не дошла до проверки, и сколько раз подряд.
const RETRY_SOON: Duration = Duration::from_secs(5 * 60);
const RETRIES_SOON_MAX: u32 = 3;

/// Дольше этого очередь не взводится: у `DelayQueue` потолок ≈ 795 суток от
/// момента её создания, а срок подписки бывает и на годы вперёд. Взведённая на
/// потолок задача, проснувшись раньше цели, просто взводится снова.
const MAX_ARM: Duration = Duration::from_secs(24 * 60 * 60);

enum TimerCommand {
    Apply(HashMap<String, TaskSchedule>),
    /// Задача отработала — и чем кончилась.
    TaskFinished {
        uid: String,
        ran: Ran,
    },
    /// Машина спала: таймеры tokio во сне не идут (Linux/macOS), цели
    /// пересчитываются по настенным часам.
    Rearm,
    /// На когда взведена задача (настенное время) — для подсказки «следующее
    /// обновление»; None — задачи нет или она сейчас бежит.
    NextFire {
        uid: String,
        reply: tokio::sync::oneshot::Sender<Option<i64>>,
    },
}

/// Чем кончился запуск задачи — от этого зависит, когда взводить её снова.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ran {
    Done,
    Failed,
    /// До проверки не дошло (профиль уже обновлялся, применение занято):
    /// повторить скоро, не считая провалом.
    NotReached,
}

#[derive(Debug, PartialEq, Eq)]
struct TaskSchedule {
    /// 0 — регулярного расписания нет, задача живёт только ради истечения.
    interval_minutes: u64,
    first_delay: Duration,
    /// clod: момент (unix, часы устройства), к которому подписку надо загрузить
    /// заново из-за названного панелью срока; None — срока нет или загрузка
    /// после него уже была.
    expiry_fetch_at: Option<i64>,
    updated: Option<usize>,
}

impl TaskSchedule {
    fn new(interval_minutes: u64, updated: Option<usize>, now: i64, expiry_fetch_at: Option<i64>) -> Self {
        let regular = (interval_minutes > 0).then(|| {
            let full = Timer::interval_duration(interval_minutes);
            let elapsed = updated
                .filter(|updated| *updated > 0)
                .map_or(0, |updated| now.saturating_sub(updated as i64).max(0) as u64);
            full.saturating_sub(Duration::from_secs(elapsed))
        });
        let expiry = expiry_fetch_at.map(|at| Duration::from_secs(at.saturating_sub(now).max(0) as u64));

        Self {
            interval_minutes,
            first_delay: regular.into_iter().chain(expiry).min().unwrap_or(Duration::MAX),
            expiry_fetch_at,
            updated,
        }
    }
}

/// clod: когда подписку надо загрузить заново из-за истечения срока.
///
/// `expire` — срок по часам панели (секунды; миллисекунды старых записей
/// нормализуются), `clock_skew` — панель минус устройство
/// (`PrfItem::panel_clock_skew`), `updated` — время последней загрузки по часам
/// устройства. Признак «после истечения ещё не загружали» нигде не хранится: он
/// выводится из этих трёх полей, поэтому переживает перезапуск сам собой — при
/// старте условие снова истинно, и загрузка уходит сразу. Удачная загрузка
/// сдвигает `updated` за дедлайн, и повторов больше нет; продление приносит
/// новый `expire` обычной загрузкой.
///
/// Старение поправки (через 30 дней без загрузок — 0) цель не теряет: если
/// очередь взвели по нулевой поправке, а устройство спешит, загрузка уйдёт до
/// срока по часам панели, но её же ответ принесёт свежий замер, и по нему та же
/// загрузка окажется «до дедлайна» — цель переставится уже по свежей поправке.
fn expiry_fetch_at(expire: u64, clock_skew: i64, updated: Option<usize>) -> Option<i64> {
    if expire == 0 {
        return None;
    }
    let deadline = (expire as i64)
        .saturating_sub(clock_skew)
        .saturating_add(EXPIRY_GRACE.as_secs() as i64);
    ((updated.unwrap_or(0) as i64).saturating_add(EXPIRY_SLACK) < deadline).then_some(deadline)
}

struct TaskState {
    key: Option<Key>,
    /// Настенное время (unix), на которое взведён `key`; по нему задача
    /// перевзводится после сна и после срабатывания на потолке `MAX_ARM`.
    fires_at: Option<i64>,
    interval_minutes: u64,
    expiry_fetch_at: Option<i64>,
    updated: Option<usize>,
    /// Сколько запусков подряд не удалось — степень бэкоффа повтора.
    failures: u32,
    /// `key` взведён на потолок `MAX_ARM`, а не на саму цель.
    capped: bool,
    /// Когда началась бегущая загрузка (unix): загрузка, начатая до дедлайна и
    /// закончившаяся после, — не попытка «после истечения».
    started_at: Option<i64>,
    /// Сколько раз подряд задача повторялась «скоро», не дойдя до проверки.
    retries_soon: u32,
    running: bool,
    retired: bool,
}

impl TaskState {
    const fn new(interval_minutes: u64, expiry_fetch_at: Option<i64>, updated: Option<usize>) -> Self {
        Self {
            key: None,
            fires_at: None,
            interval_minutes,
            expiry_fetch_at,
            updated,
            failures: 0,
            capped: false,
            started_at: None,
            retries_soon: 0,
            running: false,
            retired: false,
        }
    }
}

/// Что известно о задаче снаружи планировщика — для сравнения «есть ли
/// изменения».
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TaskSlot {
    interval_minutes: u64,
    expiry_fetch_at: Option<i64>,
    /// Время последней загрузки: удачная загрузка (и ручная тоже) перевзводит
    /// очередь от себя; провал `updated` не двигает.
    updated: Option<usize>,
}

pub struct Timer {
    command_tx: mpsc::UnboundedSender<TimerCommand>,
    command_rx: Mutex<Option<mpsc::UnboundedReceiver<TimerCommand>>>,
    refresh_lock: tokio::sync::Mutex<()>,
    timer_map: Arc<RwLock<HashMap<String, TaskSlot>>>,
    pub initialized: AtomicBool,
}

singleton!(Timer, TIMER_INSTANCE);

impl Timer {
    fn new() -> Self {
        let (command_tx, command_rx) = mpsc::unbounded_channel();
        Self {
            command_tx,
            command_rx: Mutex::new(Some(command_rx)),
            refresh_lock: tokio::sync::Mutex::new(()),
            timer_map: Arc::new(RwLock::new(HashMap::new())),
            initialized: AtomicBool::new(false),
        }
    }

    pub async fn init(&self) -> Result<()> {
        if self
            .initialized
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            logging!(debug, Type::Timer, "Timer already initialized, skipping...");
            return Ok(());
        }

        let command_rx = { self.command_rx.lock().take() };
        if let Some(command_rx) = command_rx {
            let command_tx = self.command_tx.clone();
            AsyncHandler::spawn(move || async move {
                Self::run_scheduler(command_rx, command_tx).await;
            });
        }

        if let Err(e) = self.refresh().await {
            self.initialized.store(false, Ordering::SeqCst);
            logging_error!(Type::Timer, "Failed to initialize timer: {}", e);
            return Err(e);
        }

        {
            let timer_map = self.timer_map.read();
            logging!(debug, Type::Timer, "Registered timer task count: {}", timer_map.len());
            for (uid, slot) in timer_map.iter() {
                logging!(
                    debug,
                    Type::Timer,
                    "Registered timer task: uid={}, interval={}min, expiry fetch at={:?}",
                    uid,
                    slot.interval_minutes,
                    slot.expiry_fetch_at
                );
            }
        }

        logging!(info, Type::Timer, "Timer initialization completed");
        Ok(())
    }

    pub async fn refresh(&self) -> Result<()> {
        let _refresh_guard = self.refresh_lock.lock().await;
        let new_schedule = self.gen_map().await;
        let new_map: HashMap<String, TaskSlot> = new_schedule
            .iter()
            .map(|(uid, schedule)| {
                (
                    uid.clone(),
                    TaskSlot {
                        interval_minutes: schedule.interval_minutes,
                        expiry_fetch_at: schedule.expiry_fetch_at,
                        updated: schedule.updated,
                    },
                )
            })
            .collect();

        let mut cache = self.timer_map.write();
        if *cache == new_map {
            logging!(debug, Type::Timer, "No timer changes needed");
            return Ok(());
        }

        logging!(
            info,
            Type::Timer,
            "Refreshing timer tasks map, count: {}",
            new_map.len()
        );
        *cache = new_map;
        drop(cache);

        let _ = self.command_tx.send(TimerCommand::Apply(new_schedule));

        Ok(())
    }

    /// clod: машина проснулась — цели задач пересчитываются по настенным часам.
    pub fn rearm_after_wake(&self) {
        let _ = self.command_tx.send(TimerCommand::Rearm);
    }

    async fn gen_map(&self) -> HashMap<String, TaskSchedule> {
        if let Some(items) = Config::profiles().await.data_arc().get_items() {
            return Self::gen_map_from_items(items, crate::utils::help::now_secs());
        }

        HashMap::new()
    }

    fn gen_map_from_items(items: &[PrfItem], now: i64) -> HashMap<String, TaskSchedule> {
        let mut new_map = HashMap::new();

        for item in items {
            let Some(uid) = &item.uid else {
                continue;
            };
            let option = item.option.as_ref();
            if !option.and_then(|option| option.allow_auto_update).unwrap_or(true) {
                continue;
            }
            let interval = option.and_then(|option| option.update_interval).unwrap_or(0);
            // Срок известен только у удалённых подписок — `extra` есть лишь у них.
            let expiry = item
                .extra
                .and_then(|extra| expiry_fetch_at(extra.expire, item.panel_clock_skew(), item.updated));
            if interval == 0 && expiry.is_none() {
                continue;
            }
            new_map.insert(uid.clone(), TaskSchedule::new(interval, item.updated, now, expiry));
        }

        new_map
    }

    async fn run_scheduler(
        mut command_rx: mpsc::UnboundedReceiver<TimerCommand>,
        command_tx: mpsc::UnboundedSender<TimerCommand>,
    ) {
        let mut queue = DelayQueue::new();
        let mut tasks = HashMap::new();

        loop {
            tokio::select! {
                Some(expired) = queue.next() => {
                    let uid = expired.into_inner();
                    Self::run_expired_task(&mut queue, &mut tasks, uid, command_tx.clone());
                }

                command = command_rx.recv() => {
                    match command {
                        Some(TimerCommand::Apply(new_map)) => {
                            Self::apply_timer_map(&mut queue, &mut tasks, new_map);
                        }
                        Some(TimerCommand::TaskFinished { uid, ran }) => {
                            Self::finish_task(&mut queue, &mut tasks, uid, ran);
                        }
                        Some(TimerCommand::Rearm) => {
                            Self::rearm_all(&mut queue, &mut tasks, crate::utils::help::now_secs());
                        }
                        Some(TimerCommand::NextFire { uid, reply }) => {
                            let _ = reply.send(tasks.get(&uid).and_then(|state| state.fires_at));
                        }
                        None => break,
                    }
                }
            }
        }
    }

    fn apply_timer_map(
        queue: &mut DelayQueue<String>,
        tasks: &mut HashMap<String, TaskState>,
        new_map: HashMap<String, TaskSchedule>,
    ) {
        tasks.retain(|uid, state| {
            if new_map.contains_key(uid) {
                return true;
            }

            if state.running {
                state.retired = true;
                logging!(
                    debug,
                    Type::Timer,
                    "Retiring timer task once its in-flight update reports back: uid={}",
                    uid
                );
                return true;
            }

            if let Some(key) = state.key.take() {
                queue.remove(&key);
            }
            logging!(debug, Type::Timer, "Removed timer task for uid={}", uid);
            false
        });

        for (uid, schedule) in new_map {
            let Some(state) = tasks.get_mut(&uid) else {
                Self::insert_task(queue, tasks, uid, &schedule);
                continue;
            };

            state.retired = false;

            let changed = state.interval_minutes != schedule.interval_minutes
                || state.expiry_fetch_at != schedule.expiry_fetch_at
                || state.updated != schedule.updated;
            state.interval_minutes = schedule.interval_minutes;
            if !state.running && state.updated != schedule.updated {
                // Загрузка удалась мимо расписания (кнопкой): серия провалов кончилась.
                state.failures = 0;
            }
            state.updated = schedule.updated;
            if state.expiry_fetch_at != schedule.expiry_fetch_at {
                state.expiry_fetch_at = schedule.expiry_fetch_at;
                state.failures = 0;
            }
            if changed {
                Self::update_task(queue, &uid, state, &schedule);
            }
        }
    }

    fn insert_task(
        queue: &mut DelayQueue<String>,
        tasks: &mut HashMap<String, TaskState>,
        uid: String,
        schedule: &TaskSchedule,
    ) {
        let mut state = TaskState::new(schedule.interval_minutes, schedule.expiry_fetch_at, schedule.updated);
        Self::arm(queue, &uid, &mut state, schedule.first_delay);
        logging!(
            debug,
            Type::Timer,
            "Added timer task: uid={}, interval={}min, first fire in {}s",
            uid,
            schedule.interval_minutes,
            schedule.first_delay.as_secs()
        );
        tasks.insert(uid, state);
    }

    fn update_task(queue: &mut DelayQueue<String>, uid: &str, state: &mut TaskState, schedule: &TaskSchedule) {
        if let Some(key) = state.key.take() {
            queue.remove(&key);
        }
        if !state.running {
            Self::arm(queue, uid, state, schedule.first_delay);
        }

        logging!(
            debug,
            Type::Timer,
            "Updated timer task: uid={}, interval={}min, expiry fetch at={:?}, next fire in {}s",
            uid,
            schedule.interval_minutes,
            schedule.expiry_fetch_at,
            schedule.first_delay.as_secs()
        );
    }

    /// Взвести задачу на `delay` от текущего момента, запомнив настенную цель.
    fn arm(queue: &mut DelayQueue<String>, uid: &str, state: &mut TaskState, delay: Duration) {
        let now = crate::utils::help::now_secs();
        state.fires_at = Some(now.saturating_add(delay.as_secs().min(i64::MAX as u64) as i64));
        state.capped = delay > MAX_ARM;
        state.key = Some(queue.insert(String::from(uid), delay.min(MAX_ARM)));
    }

    /// Взвести задачу на настенную цель `fires_at` от момента `now`.
    fn arm_at(queue: &mut DelayQueue<String>, uid: &str, state: &mut TaskState, fires_at: i64, now: i64) {
        state.fires_at = Some(fires_at);
        let delay = Duration::from_secs(fires_at.saturating_sub(now).max(0) as u64);
        state.capped = delay > MAX_ARM;
        state.key = Some(queue.insert(String::from(uid), delay.min(MAX_ARM)));
    }

    /// После сна цели остаются те же, а очередь взводится заново по часам.
    fn rearm_all(queue: &mut DelayQueue<String>, tasks: &mut HashMap<String, TaskState>, now: i64) {
        for (uid, state) in tasks.iter_mut() {
            let (Some(key), Some(fires_at)) = (state.key.take(), state.fires_at) else {
                continue;
            };
            queue.remove(&key);
            Self::arm_at(queue, uid, state, fires_at, now);
        }
        logging!(debug, Type::Timer, "Timer tasks re-armed after wake: {}", tasks.len());
    }

    /// Очередь сработала раньше настенной цели, потому что была взведена на
    /// потолок `MAX_ARM`, — просто взвести снова; загрузка не нужна. Без
    /// потолка ранний выстрел значит, что настенные часы перевели назад, — тогда
    /// верим очереди, а не часам.
    const fn woke_too_early(capped: bool, fires_at: Option<i64>, now: i64) -> Option<i64> {
        match fires_at {
            Some(at) if capped && at > now + 1 => Some(at),
            _ => None,
        }
    }

    fn run_expired_task(
        queue: &mut DelayQueue<String>,
        tasks: &mut HashMap<String, TaskState>,
        uid: String,
        command_tx: mpsc::UnboundedSender<TimerCommand>,
    ) {
        let Some(state) = tasks.get_mut(&uid) else {
            return;
        };

        state.key = None;
        let now = crate::utils::help::now_secs();
        if let Some(fires_at) = Self::woke_too_early(state.capped, state.fires_at, now) {
            Self::arm_at(queue, &uid, state, fires_at, now);
            return;
        }
        state.fires_at = None;
        if !Self::mark_task_running(state, &uid) {
            return;
        }
        state.started_at = Some(now);
        Self::spawn_update_task(uid, command_tx);
    }

    fn mark_task_running(state: &mut TaskState, uid: &str) -> bool {
        if !state.running {
            state.running = true;
            return true;
        }

        logging!(debug, Type::Timer, "Timer task already running, skip uid={}", uid);
        false
    }

    /// Через сколько взводить задачу после отработавшей загрузки.
    ///
    /// Регулярная часть — полный интервал; пока идёт серия провалов — ещё и
    /// повтор с бэкоффом.
    /// Часть истечения: цель ещё впереди (продление сдвинуло срок) — до неё; цель
    /// позади и всё ещё стоит, а загрузка началась до неё (обычный тик,
    /// растянувшийся через дедлайн) — сразу; иначе загрузка после истечения не
    /// удалась (удачная снимает цель через `refresh()` до этого вызова) — тот же
    /// повтор с бэкоффом. Берётся ближайшее; None — задаче нечего ждать.
    fn delay_after_finish(state: &mut TaskState, now: i64, ran: Ran) -> Option<Duration> {
        let started_at = state.started_at.take().unwrap_or(now);
        match ran {
            Ran::Done => state.failures = 0,
            Ran::Failed => state.failures = state.failures.saturating_add(1),
            Ran::NotReached => {}
        }
        let backoff = FAILURE_RETRY
            .saturating_mul(1u32 << state.failures.saturating_sub(1).min(16))
            .min(FAILURE_RETRY_MAX);

        let regular = (state.interval_minutes > 0).then(|| Self::interval_duration(state.interval_minutes));
        let retry = (state.failures > 0).then_some(backoff);
        let expiry = state.expiry_fetch_at.map(|at| {
            if at > now {
                Duration::from_secs((at - now) as u64)
            } else if started_at < at {
                Duration::ZERO
            } else {
                backoff
            }
        });
        regular.into_iter().chain(retry).chain(expiry).min()
    }

    fn finish_task(queue: &mut DelayQueue<String>, tasks: &mut HashMap<String, TaskState>, uid: String, ran: Ran) {
        let Some(state) = tasks.get_mut(&uid) else {
            return;
        };

        state.running = false;
        if state.retired {
            tasks.remove(&uid);
            logging!(
                debug,
                Type::Timer,
                "Dropped retired timer task now that its update finished: uid={}",
                uid
            );
            return;
        }

        if ran == Ran::NotReached && state.retries_soon < RETRIES_SOON_MAX {
            // До проверки не дошло (профиль уже обновлялся, применение занято) —
            // это не провал: цели и счёт неудач не трогаем, повторяем скоро. Но не
            // без конца: если занято раз за разом, дальше по обычному расписанию.
            state.retries_soon += 1;
            state.started_at = None;
            Self::arm(queue, &uid, state, RETRY_SOON);
            return;
        }
        state.retries_soon = 0;

        match Self::delay_after_finish(state, crate::utils::help::now_secs(), ran) {
            Some(delay) => Self::arm(queue, &uid, state, delay),
            None => {
                tasks.remove(&uid);
                logging!(
                    debug,
                    Type::Timer,
                    "Dropped timer task with nothing left to wait for: uid={}",
                    uid
                );
            }
        }
    }

    fn spawn_update_task(uid: String, command_tx: mpsc::UnboundedSender<TimerCommand>) {
        logging!(info, Type::Timer, "Starting timer task: uid={}", uid);
        AsyncHandler::spawn(move || async move {
            Self::wait_until_resolve_done(Duration::from_millis(5000)).await;
            let ran = Box::pin(Self::async_task(&uid)).await;
            // Сначала перевзвод, потом весть окну: подсказка «следующее
            // обновление», запрошенная по этой вести, встаёт в ту же очередь
            // команд позади него и видит новый срок. О начале и конце самого
            // обновления окну говорит `feat::update_profile`.
            let _ = command_tx.send(TimerCommand::TaskFinished { uid: uid.clone(), ran });
            super::handle::Handle::notify_timer_updated(&uid);
        });
    }

    const fn interval_duration(interval_minutes: u64) -> Duration {
        Duration::from_secs(interval_minutes.saturating_mul(60))
    }

    /// Когда задача сработает на самом деле — со всеми повторами после провала,
    /// «занято» и целью по сроку: планировщик отвечает тем, на что взвёл её сам.
    pub async fn get_next_update_time(&self, uid: &str) -> Option<i64> {
        logging!(debug, Type::Timer, "Getting next update time, uid={}", uid);

        let (reply, answer) = tokio::sync::oneshot::channel();
        self.command_tx
            .send(TimerCommand::NextFire { uid: uid.into(), reply })
            .ok()?;
        answer.await.ok().flatten()
    }

    async fn async_task(uid: &String) -> Ran {
        let task_start = std::time::Instant::now();
        logging!(debug, Type::Timer, "Running timer task for profile: {}", uid);

        // clod:Э10-13 — обещание «замок панели снимется сам» держалось на двух
        // вызовах: при старте приложения и при провале загрузки. Подписка, которую
        // панель бросила, но которая исправно скачивается, замок держала до
        // перезапуска и продолжала диктовать режим Clash. Тик расписания — то самое
        // регулярное место, где просроченному замку и место истечь.
        crate::feat::release_stale_panel_locks().await;

        let result = Box::pin(feat::update_profile(uid, None, false, feat::UpdateTrigger::Scheduled)).await;

        match &result {
            Ok(outcome) => logging!(
                info,
                Type::Timer,
                "Timer task completed for uid: {} ({outcome:?}, took {}ms)",
                uid,
                task_start.elapsed().as_millis()
            ),
            Err(e) => logging_error!(
                Type::Timer,
                "Failed to update profile uid {}: {} (took {}ms)",
                uid,
                e,
                task_start.elapsed().as_millis()
            ),
        }
        Self::ran(&result)
    }

    /// Отказ ядра — не провал для расписания: повтор получил бы тот же ответ
    /// (см. `feat::RefusedByTheCore`), ждём обычного срока. Бэкофф — сбоям,
    /// которые проходят сами: сеть, панель, прибитая проверка, молчащее ядро.
    fn ran(result: &Result<feat::UpdateOutcome>) -> Ran {
        match result {
            Ok(feat::UpdateOutcome::RetrySoon) => Ran::NotReached,
            Ok(feat::UpdateOutcome::Done) => Ran::Done,
            Err(e) if e.is::<feat::RefusedByTheCore>() => Ran::Done,
            Err(_) => Ran::Failed,
        }
    }

    async fn wait_until_resolve_done(max_wait: Duration) {
        let _ = timeout(max_wait, async {
            while !is_resolve_done() {
                logging!(debug, Type::Timer, "Waiting for resolve to be done...");
                sleep(Duration::from_millis(200)).await;
            }
        })
        .await;
    }
}

#[cfg(test)]
mod tests {
    use super::{
        EXPIRY_GRACE, EXPIRY_SLACK, FAILURE_RETRY, FAILURE_RETRY_MAX, MAX_ARM, RETRIES_SOON_MAX, RETRY_SOON, Ran,
        TaskSchedule, TaskState, Timer, expiry_fetch_at,
    };
    use crate::config::{PrfExtra, PrfItem, PrfOption};
    use smartstring::alias::String;
    use std::{collections::HashMap, time::Duration};
    use tokio_util::time::DelayQueue;

    const NOW: i64 = 1_700_000_000;
    const HOUR: u64 = 60;
    const GRACE: i64 = EXPIRY_GRACE.as_secs() as i64;

    fn remote_profile(uid: &str, allow_auto_update: Option<bool>, update_interval: Option<u64>) -> PrfItem {
        PrfItem {
            uid: Some(uid.into()),
            itype: Some("remote".into()),
            option: Some(PrfOption {
                allow_auto_update,
                update_interval,
                ..PrfOption::default()
            }),
            ..PrfItem::default()
        }
    }

    fn expiring_profile(uid: &str, update_interval: Option<u64>, expire: u64, updated: i64) -> PrfItem {
        PrfItem {
            extra: Some(PrfExtra {
                expire,
                ..PrfExtra::default()
            }),
            updated: Some(updated as usize),
            ..remote_profile(uid, None, update_interval)
        }
    }

    /// Задача ушла в загрузку: ключ снят с очереди, `running` взведён.
    fn pretend_running(queue: &mut DelayQueue<String>, tasks: &mut HashMap<String, TaskState>, uid: &String) {
        pretend_running_since(queue, tasks, uid, crate::utils::help::now_secs());
    }

    fn pretend_running_since(
        queue: &mut DelayQueue<String>,
        tasks: &mut HashMap<String, TaskState>,
        uid: &String,
        started_at: i64,
    ) {
        if let Some(state) = tasks.get_mut(uid) {
            if let Some(key) = state.key.take() {
                queue.remove(&key);
            }
            state.running = true;
            state.started_at = Some(started_at);
        }
    }

    /// Сколько ждать взведённой задаче, с точностью до секунды: без `test-util`
    /// часы tokio не останавливаются, и между взводом и проверкой они идут.
    fn queued_delay(queue: &DelayQueue<String>, state: &TaskState) -> Option<Duration> {
        state.key.as_ref().map(|key| {
            let left = queue
                .deadline(key)
                .saturating_duration_since(tokio::time::Instant::now());
            Duration::from_secs(left.as_secs_f64().round() as u64)
        })
    }

    #[test]
    fn timer_map_only_contains_enabled_profiles_with_positive_intervals() {
        let items = vec![
            remote_profile("enabled", Some(true), Some(30)),
            remote_profile("disabled", Some(false), Some(30)),
            remote_profile("missing-flag", None, Some(30)),
            remote_profile("zero-interval", Some(true), Some(0)),
            remote_profile("missing-interval", Some(true), None),
        ];

        let map = Timer::gen_map_from_items(&items, NOW);

        assert_eq!(map.len(), 2);
        assert_eq!(map.get("enabled").map(|s| s.interval_minutes), Some(30));
        assert_eq!(map.get("missing-flag").map(|s| s.interval_minutes), Some(30));
    }

    #[test]
    fn an_expiring_subscription_is_scheduled_even_without_an_interval() {
        let expire = (NOW + 3600) as u64;
        let items = vec![
            expiring_profile("no-interval", None, expire, NOW - 60),
            expiring_profile("zero-interval", Some(0), expire, NOW - 60),
            expiring_profile("with-interval", Some(HOUR * 24), expire, NOW - 60),
            // Загрузка после срока уже была — ждать нечего.
            expiring_profile("already-fetched", None, (NOW - 3600) as u64, NOW - 60),
            PrfItem {
                option: Some(PrfOption {
                    allow_auto_update: Some(false),
                    ..PrfOption::default()
                }),
                ..expiring_profile("forbidden", None, expire, NOW - 60)
            },
        ];

        let map = Timer::gen_map_from_items(&items, NOW);

        assert_eq!(map.len(), 3);
        let deadline = Some(NOW + 3600 + GRACE);
        for uid in ["no-interval", "zero-interval", "with-interval"] {
            assert_eq!(map.get(uid).and_then(|s| s.expiry_fetch_at), deadline, "{uid}");
            assert_eq!(
                map.get(uid).map(|s| s.first_delay),
                Some(Duration::from_secs(3600 + GRACE as u64)),
                "{uid}"
            );
        }
        assert_eq!(map.get("zero-interval").map(|s| s.interval_minutes), Some(0));
    }

    #[test]
    fn the_expiry_fetch_is_due_once_after_the_panel_deadline() {
        let expire = (NOW + 600) as u64;
        // Срок уже в секундах: миллисекунды приводит чтение записи (`PrfExtra`).
        assert_eq!(
            expiry_fetch_at(expire, 0, Some((NOW - 1) as usize)),
            Some(NOW + 600 + GRACE)
        );
        // Поправка часов: панель спешит на минуту → по часам устройства срок раньше.
        assert_eq!(expiry_fetch_at(expire, 60, Some(NOW as usize)), Some(NOW + 540 + GRACE));
        // Бессрочная подписка.
        assert_eq!(expiry_fetch_at(0, 0, Some(NOW as usize)), None);
        // Загрузка уже была после дедлайна — второй раз не нужна.
        assert_eq!(expiry_fetch_at(expire, 0, Some((NOW + 600 + GRACE) as usize)), None);
        // Загрузка за считанные секунды до дедлайна (дрожание поправки часов) — тоже в срок.
        assert_eq!(
            expiry_fetch_at(expire, 0, Some((NOW + 600 + GRACE - EXPIRY_SLACK) as usize)),
            None
        );
        // Загрузка заметно раньше дедлайна — ещё нужна.
        assert_eq!(
            expiry_fetch_at(expire, 0, Some((NOW + 600 + GRACE - EXPIRY_SLACK - 1) as usize)),
            Some(NOW + 600 + GRACE)
        );
        // Ни разу не загружали.
        assert_eq!(expiry_fetch_at(expire, 0, None), Some(NOW + 600 + GRACE));
    }

    #[test]
    fn a_fetch_made_by_a_stale_clock_is_re_judged_by_the_fresh_measurement() {
        // Поправка состарилась (0), устройство спешит на 5 минут: очередь взвелась
        // на expire + 90 по часам устройства, загрузка ушла и удалась…
        let expire = NOW as u64;
        let fetched = NOW + GRACE;
        assert_eq!(
            expiry_fetch_at(expire, 0, Some((NOW - 3600) as usize)),
            Some(NOW + GRACE)
        );
        // …а её ответ принёс свежий замер −300: по нему та же загрузка была до
        // срока панели, цель стоит и переставлена на expire + 300 + 90.
        assert_eq!(
            expiry_fetch_at(expire, -300, Some(fetched as usize)),
            Some(NOW + 300 + GRACE)
        );
    }

    #[test]
    fn first_fire_counts_from_the_last_update_not_from_now() {
        assert_eq!(
            TaskSchedule::new(HOUR, Some(NOW as usize), NOW, None).first_delay,
            Duration::from_secs(3600)
        );
        assert_eq!(
            TaskSchedule::new(HOUR, Some((NOW - 1800) as usize), NOW, None).first_delay,
            Duration::from_secs(1800)
        );
        assert_eq!(
            TaskSchedule::new(1440, Some((NOW - 1439 * 60) as usize), NOW, None).first_delay,
            Duration::from_secs(60)
        );
        assert_eq!(
            TaskSchedule::new(HOUR, Some((NOW - 7200) as usize), NOW, None).first_delay,
            Duration::ZERO
        );
        for unknown in [None, Some(0)] {
            assert_eq!(
                TaskSchedule::new(HOUR, unknown, NOW, None).first_delay,
                Duration::from_secs(3600)
            );
        }
        assert_eq!(
            TaskSchedule::new(HOUR, Some((NOW + 9999) as usize), NOW, None).first_delay,
            Duration::from_secs(3600)
        );
    }

    #[test]
    fn the_first_fire_is_the_earlier_of_the_interval_and_the_expiry() {
        // Срок раньше очередного обновления.
        assert_eq!(
            TaskSchedule::new(HOUR, Some(NOW as usize), NOW, Some(NOW + 600)).first_delay,
            Duration::from_secs(600)
        );
        // Очередное обновление раньше срока.
        assert_eq!(
            TaskSchedule::new(HOUR, Some(NOW as usize), NOW, Some(NOW + 9000)).first_delay,
            Duration::from_secs(3600)
        );
        // Срок уже прошёл (приложение было выключено) — сразу.
        assert_eq!(
            TaskSchedule::new(HOUR, Some(NOW as usize), NOW, Some(NOW - 5)).first_delay,
            Duration::ZERO
        );
        // Без интервала ждём только срок.
        assert_eq!(
            TaskSchedule::new(0, Some(NOW as usize), NOW, Some(NOW + 600)).first_delay,
            Duration::from_secs(600)
        );
    }

    #[tokio::test]
    async fn a_failed_expiry_fetch_is_retried_soon_and_a_done_one_waits_for_the_interval() {
        let mut queue = DelayQueue::new();
        let mut tasks = HashMap::new();
        let uid = String::from("uid");
        let now = crate::utils::help::now_secs();

        // Срок позади и всё ещё стоит — загрузка не удалась: повтор через 15 мин.
        let mut map = HashMap::new();
        map.insert(uid.clone(), TaskSchedule::new(HOUR * 24, None, now, Some(now - 10)));
        Timer::apply_timer_map(&mut queue, &mut tasks, map);
        pretend_running(&mut queue, &mut tasks, &uid);
        Timer::finish_task(&mut queue, &mut tasks, uid.clone(), Ran::Failed);
        assert_eq!(
            tasks.get(&uid).and_then(|state| queued_delay(&queue, state)),
            Some(FAILURE_RETRY)
        );

        // Удачная загрузка сняла цель (refresh → Apply) до TaskFinished: полный интервал.
        let mut map = HashMap::new();
        map.insert(uid.clone(), TaskSchedule::new(HOUR * 24, Some(now as usize), now, None));
        Timer::apply_timer_map(&mut queue, &mut tasks, map);
        pretend_running(&mut queue, &mut tasks, &uid);
        Timer::finish_task(&mut queue, &mut tasks, uid.clone(), Ran::Done);
        assert_eq!(
            tasks.get(&uid).and_then(|state| queued_delay(&queue, state)),
            Some(Duration::from_secs(24 * 3600))
        );
    }

    #[tokio::test]
    async fn a_failed_regular_fetch_is_retried_with_backoff_within_the_interval() {
        let mut queue = DelayQueue::new();
        let mut tasks = HashMap::new();
        let uid = String::from("uid");
        let now = crate::utils::help::now_secs();

        // Провал обычного тика (сеть не поднялась после сна, панель мигнула) не
        // стоит суток: повтор через 15, 30, 60… минут, но не позже самого тика.
        let mut map = HashMap::new();
        map.insert(uid.clone(), TaskSchedule::new(HOUR * 3, Some(now as usize), now, None));
        Timer::apply_timer_map(&mut queue, &mut tasks, map);
        let expected = [15, 30, 60, 120, 180, 180].map(|minutes| Duration::from_secs(minutes * 60));
        for (attempt, delay) in expected.into_iter().enumerate() {
            pretend_running(&mut queue, &mut tasks, &uid);
            Timer::finish_task(&mut queue, &mut tasks, uid.clone(), Ran::Failed);
            assert_eq!(
                tasks.get(&uid).and_then(|state| queued_delay(&queue, state)),
                Some(delay),
                "attempt {attempt}"
            );
        }

        // Удача обнуляет счёт: следующий тик — через полный интервал, а новый
        // провал снова повторяется через 15 минут.
        pretend_running(&mut queue, &mut tasks, &uid);
        Timer::finish_task(&mut queue, &mut tasks, uid.clone(), Ran::Done);
        assert_eq!(
            tasks.get(&uid).and_then(|state| queued_delay(&queue, state)),
            Some(Duration::from_secs(3 * 3600))
        );
        pretend_running(&mut queue, &mut tasks, &uid);
        Timer::finish_task(&mut queue, &mut tasks, uid.clone(), Ran::Failed);
        assert_eq!(
            tasks.get(&uid).and_then(|state| queued_delay(&queue, state)),
            Some(FAILURE_RETRY)
        );
        // Подсказка «следующее обновление» берёт этот же срок, а не полный интервал.
        let fires_at = tasks.get(&uid).and_then(|state| state.fires_at).unwrap_or_default();
        assert!((fires_at - crate::utils::help::now_secs() - FAILURE_RETRY.as_secs() as i64).abs() <= 1);
    }

    #[test]
    fn a_refusal_by_the_core_waits_for_the_interval_and_a_glitch_is_retried() {
        use crate::feat::{RefusedByTheCore, UpdateOutcome};
        assert_eq!(Timer::ran(&Ok(UpdateOutcome::Done)), Ran::Done);
        assert_eq!(Timer::ran(&Ok(UpdateOutcome::RetrySoon)), Ran::NotReached);
        assert_eq!(
            Timer::ran(&Err(RefusedByTheCore("invalid config".into()).into())),
            Ran::Done
        );
        assert_eq!(Timer::ran(&Err(anyhow::anyhow!("dns lookup failed"))), Ran::Failed);
    }

    #[tokio::test]
    async fn a_failure_series_survives_a_busy_run_and_ends_with_a_manual_success() {
        let mut queue = DelayQueue::new();
        let mut tasks = HashMap::new();
        let uid = String::from("uid");
        let now = crate::utils::help::now_secs();

        let mut map = HashMap::new();
        map.insert(uid.clone(), TaskSchedule::new(HOUR * 24, Some(now as usize), now, None));
        Timer::apply_timer_map(&mut queue, &mut tasks, map);
        for _ in 0..2 {
            pretend_running(&mut queue, &mut tasks, &uid);
            Timer::finish_task(&mut queue, &mut tasks, uid.clone(), Ran::Failed);
        }

        // «Занято» раз за разом посреди серии провалов — повтор серии, а не сутки.
        for _ in 0..=RETRIES_SOON_MAX {
            pretend_running(&mut queue, &mut tasks, &uid);
            Timer::finish_task(&mut queue, &mut tasks, uid.clone(), Ran::NotReached);
        }
        assert_eq!(
            tasks.get(&uid).and_then(|state| queued_delay(&queue, state)),
            Some(FAILURE_RETRY * 2)
        );

        // Кнопка обновила подписку — серия кончилась: новый провал снова через 15 минут.
        let mut map = HashMap::new();
        map.insert(
            uid.clone(),
            TaskSchedule::new(HOUR * 24, Some((now + 1) as usize), now, None),
        );
        Timer::apply_timer_map(&mut queue, &mut tasks, map);
        assert_eq!(tasks.get(&uid).map(|state| state.failures), Some(0));
        pretend_running(&mut queue, &mut tasks, &uid);
        Timer::finish_task(&mut queue, &mut tasks, uid.clone(), Ran::Failed);
        assert_eq!(
            tasks.get(&uid).and_then(|state| queued_delay(&queue, state)),
            Some(FAILURE_RETRY)
        );
    }

    #[tokio::test]
    async fn a_task_that_never_reached_the_check_is_retried_soon_but_not_forever() {
        let mut queue = DelayQueue::new();
        let mut tasks = HashMap::new();
        let uid = String::from("uid");
        let now = crate::utils::help::now_secs();

        // Срок позади: провал считался бы «после истечения» с бэкоффом, а
        // «не дошло до проверки» — нет: повтор скоро, счётчик неудач не растёт.
        let mut map = HashMap::new();
        map.insert(uid.clone(), TaskSchedule::new(HOUR * 24, None, now, Some(now - 10)));
        Timer::apply_timer_map(&mut queue, &mut tasks, map);
        for _ in 0..RETRIES_SOON_MAX {
            pretend_running(&mut queue, &mut tasks, &uid);
            Timer::finish_task(&mut queue, &mut tasks, uid.clone(), Ran::NotReached);
            assert_eq!(
                tasks.get(&uid).and_then(|state| queued_delay(&queue, state)),
                Some(RETRY_SOON)
            );
            assert_eq!(tasks.get(&uid).map(|state| state.failures), Some(0));
        }

        // Занято раз за разом — дальше по обычному расписанию истечения.
        pretend_running(&mut queue, &mut tasks, &uid);
        Timer::finish_task(&mut queue, &mut tasks, uid.clone(), Ran::NotReached);
        assert_eq!(
            tasks.get(&uid).and_then(|state| queued_delay(&queue, state)),
            Some(FAILURE_RETRY)
        );
        assert_eq!(tasks.get(&uid).map(|state| state.retries_soon), Some(0));
    }

    #[tokio::test]
    async fn expiry_retries_back_off_up_to_the_cap_and_never_give_up() {
        let mut queue = DelayQueue::new();
        let mut tasks = HashMap::new();
        let uid = String::from("uid");
        let now = crate::utils::help::now_secs();

        let mut map = HashMap::new();
        map.insert(uid.clone(), TaskSchedule::new(0, None, now, Some(now - 10)));
        Timer::apply_timer_map(&mut queue, &mut tasks, map);
        let expected = [15, 30, 60, 120, 240, 300, 300, 300].map(|minutes| Duration::from_secs(minutes * 60));
        for (attempt, delay) in expected.into_iter().enumerate() {
            pretend_running(&mut queue, &mut tasks, &uid);
            Timer::finish_task(&mut queue, &mut tasks, uid.clone(), Ran::Failed);
            assert_eq!(
                tasks.get(&uid).and_then(|state| queued_delay(&queue, state)),
                Some(delay),
                "attempt {attempt}"
            );
        }
        assert_eq!(expected[5], FAILURE_RETRY_MAX);
        assert_eq!(expected[0], FAILURE_RETRY);

        // Обычный интервал короче бэкоффа — ждём его.
        let mut map = HashMap::new();
        map.insert(uid.clone(), TaskSchedule::new(HOUR, None, now, Some(now - 10)));
        Timer::apply_timer_map(&mut queue, &mut tasks, map);
        pretend_running(&mut queue, &mut tasks, &uid);
        Timer::finish_task(&mut queue, &mut tasks, uid.clone(), Ran::Failed);
        assert_eq!(
            tasks.get(&uid).and_then(|state| queued_delay(&queue, state)),
            Some(Duration::from_secs(3600))
        );

        // Новая цель (продление) обнуляет счёт попыток.
        let mut map = HashMap::new();
        map.insert(
            uid.clone(),
            TaskSchedule::new(HOUR * 24, Some(now as usize), now, Some(now + 60)),
        );
        Timer::apply_timer_map(&mut queue, &mut tasks, map);
        assert_eq!(tasks.get(&uid).map(|state| state.failures), Some(0));
    }

    #[tokio::test]
    async fn a_regular_fetch_that_ran_across_the_deadline_is_followed_by_one_at_once() {
        let mut queue = DelayQueue::new();
        let mut tasks = HashMap::new();
        let uid = String::from("uid");
        let now = crate::utils::help::now_secs();

        let mut map = HashMap::new();
        map.insert(
            uid.clone(),
            TaskSchedule::new(HOUR * 24, Some((now - 60) as usize), now, Some(now - 10)),
        );
        Timer::apply_timer_map(&mut queue, &mut tasks, map);
        pretend_running_since(&mut queue, &mut tasks, &uid, now - 60);
        Timer::finish_task(&mut queue, &mut tasks, uid.clone(), Ran::Done);
        assert_eq!(
            tasks.get(&uid).and_then(|state| queued_delay(&queue, state)),
            Some(Duration::ZERO)
        );
        assert_eq!(tasks.get(&uid).map(|state| state.failures), Some(0));
    }

    #[tokio::test]
    async fn a_successful_fetch_re_arms_the_interval_from_itself() {
        let mut queue = DelayQueue::new();
        let mut tasks = HashMap::new();
        let uid = String::from("uid");
        let now = crate::utils::help::now_secs();

        let mut map = HashMap::new();
        map.insert(
            uid.clone(),
            TaskSchedule::new(HOUR, Some((now - 1800) as usize), now, None),
        );
        Timer::apply_timer_map(&mut queue, &mut tasks, map);
        assert_eq!(
            tasks.get(&uid).and_then(|state| queued_delay(&queue, state)),
            Some(Duration::from_secs(1800))
        );

        // Ручная загрузка удалась: очередь считает от неё, как и подсказка на карточке.
        let mut map = HashMap::new();
        map.insert(uid.clone(), TaskSchedule::new(HOUR, Some(now as usize), now, None));
        Timer::apply_timer_map(&mut queue, &mut tasks, map);
        assert_eq!(
            tasks.get(&uid).and_then(|state| queued_delay(&queue, state)),
            Some(Duration::from_secs(3600))
        );
        assert_eq!(queue.len(), 1);
    }

    #[tokio::test]
    async fn a_task_kept_only_for_the_expiry_is_dropped_once_the_fetch_succeeded() {
        let mut queue = DelayQueue::new();
        let mut tasks = HashMap::new();
        let uid = String::from("uid");
        let now = crate::utils::help::now_secs();

        let mut map = HashMap::new();
        map.insert(uid.clone(), TaskSchedule::new(0, None, now, Some(now)));
        Timer::apply_timer_map(&mut queue, &mut tasks, map);
        pretend_running(&mut queue, &mut tasks, &uid);
        // refresh() после удачи выкидывает подписку из карты целиком.
        Timer::apply_timer_map(&mut queue, &mut tasks, HashMap::new());
        Timer::finish_task(&mut queue, &mut tasks, uid.clone(), Ran::Done);
        assert!(!tasks.contains_key(&uid));
        assert_eq!(queue.len(), 0);
    }

    #[tokio::test]
    async fn a_moved_deadline_re_arms_a_waiting_task() {
        let mut queue = DelayQueue::new();
        let mut tasks = HashMap::new();
        let uid = String::from("uid");
        let now = crate::utils::help::now_secs();

        let mut map = HashMap::new();
        map.insert(
            uid.clone(),
            TaskSchedule::new(HOUR * 24, Some(now as usize), now, Some(now + 7200)),
        );
        Timer::apply_timer_map(&mut queue, &mut tasks, map);
        assert_eq!(
            tasks.get(&uid).and_then(|state| queued_delay(&queue, state)),
            Some(Duration::from_secs(7200))
        );

        let mut map = HashMap::new();
        map.insert(
            uid.clone(),
            TaskSchedule::new(HOUR * 24, Some(now as usize), now, Some(now + 600)),
        );
        Timer::apply_timer_map(&mut queue, &mut tasks, map);
        assert_eq!(
            tasks.get(&uid).and_then(|state| queued_delay(&queue, state)),
            Some(Duration::from_secs(600))
        );
        assert_eq!(queue.len(), 1);
    }

    #[tokio::test]
    async fn a_far_deadline_is_armed_to_the_cap_and_re_armed_when_it_wakes_early() {
        let mut queue = DelayQueue::new();
        let mut tasks = HashMap::new();
        let uid = String::from("uid");
        let now = crate::utils::help::now_secs();
        let far = now + 5 * 365 * 24 * 3600;

        let mut map = HashMap::new();
        map.insert(uid.clone(), TaskSchedule::new(0, None, now, Some(far)));
        Timer::apply_timer_map(&mut queue, &mut tasks, map);
        let state = tasks.get(&uid);
        assert_eq!(state.and_then(|state| queued_delay(&queue, state)), Some(MAX_ARM));
        assert_eq!(state.and_then(|state| state.fires_at), Some(far));

        assert_eq!(state.map(|state| state.capped), Some(true));
        assert_eq!(Timer::woke_too_early(true, Some(far), now), Some(far));
        assert_eq!(Timer::woke_too_early(true, Some(now), now), None);
        // Часы перевели назад: невзведённая на потолок задача верит очереди.
        assert_eq!(Timer::woke_too_early(false, Some(now + 3600), now), None);
        assert_eq!(Timer::woke_too_early(true, None, now), None);
    }

    #[tokio::test]
    async fn waking_up_re_arms_by_the_wall_clock() {
        let mut queue = DelayQueue::new();
        let mut tasks = HashMap::new();
        let uid = String::from("uid");
        let now = crate::utils::help::now_secs();

        let mut map = HashMap::new();
        map.insert(uid.clone(), TaskSchedule::new(HOUR, Some(now as usize), now, None));
        Timer::apply_timer_map(&mut queue, &mut tasks, map);

        // Машина проспала 50 минут: монотонный таймер не сдвинулся, настенные часы — да.
        Timer::rearm_all(&mut queue, &mut tasks, now + 50 * 60);
        assert_eq!(
            tasks.get(&uid).and_then(|state| queued_delay(&queue, state)),
            Some(Duration::from_secs(10 * 60))
        );
        // Проспала дольше цели — сразу.
        Timer::rearm_all(&mut queue, &mut tasks, now + 90 * 60);
        assert_eq!(
            tasks.get(&uid).and_then(|state| queued_delay(&queue, state)),
            Some(Duration::ZERO)
        );
        assert_eq!(queue.len(), 1);
    }

    #[tokio::test]
    async fn a_task_disabled_mid_update_is_dropped_when_the_update_finishes() {
        let mut queue = DelayQueue::new();
        let mut tasks = HashMap::new();
        let uid = String::from("uid");

        let mut map = HashMap::new();
        map.insert(uid.clone(), TaskSchedule::new(60, None, 0, None));
        Timer::apply_timer_map(&mut queue, &mut tasks, map);
        if let Some(state) = tasks.get_mut(&uid) {
            if let Some(key) = state.key.take() {
                queue.remove(&key);
            }
            state.running = true;
        }

        Timer::apply_timer_map(&mut queue, &mut tasks, HashMap::new());
        assert!(tasks.get(&uid).is_some_and(|state| state.retired));

        Timer::finish_task(&mut queue, &mut tasks, uid.clone(), Ran::Done);
        assert!(!tasks.contains_key(&uid));
    }

    #[tokio::test]
    async fn a_task_re_enabled_before_its_update_finishes_is_re_armed_once() {
        let mut queue = DelayQueue::new();
        let mut tasks = HashMap::new();
        let uid = String::from("uid");

        let mut map = HashMap::new();
        map.insert(uid.clone(), TaskSchedule::new(60, None, 0, None));
        Timer::apply_timer_map(&mut queue, &mut tasks, map);
        if let Some(state) = tasks.get_mut(&uid) {
            if let Some(key) = state.key.take() {
                queue.remove(&key);
            }
            state.running = true;
        }

        Timer::apply_timer_map(&mut queue, &mut tasks, HashMap::new());
        let mut map = HashMap::new();
        map.insert(uid.clone(), TaskSchedule::new(60, None, 0, None));
        Timer::apply_timer_map(&mut queue, &mut tasks, map);
        assert!(
            tasks
                .get(&uid)
                .is_some_and(|state| state.running && !state.retired && state.key.is_none())
        );

        Timer::finish_task(&mut queue, &mut tasks, uid.clone(), Ran::Done);
        assert!(
            tasks
                .get(&uid)
                .is_some_and(|state| !state.running && state.key.is_some())
        );
        assert_eq!(queue.len(), 1);
    }
}
