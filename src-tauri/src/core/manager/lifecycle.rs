use super::{Backend, CoreManager, RunningMode};
use crate::config::{Config, IVerge};
use crate::constants::timing;
use crate::core::handle::Handle;
use crate::core::manager::CLASH_LOGGER;
use crate::core::service::{SERVICE_MANAGER, ServiceStatus};
use crate::process::AsyncHandler;
use anyhow::Result;
use clash_verge_logging::{Type, logging};
use scopeguard::defer;
use smartstring::alias::String;
use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::time::Duration;
use tauri_plugin_clash_verge_sysinfo;
use tauri_plugin_clash_verge_sysinfo::is_current_app_handle_admin;

static MIXED_PORT_CHECK_GENERATION: AtomicU64 = AtomicU64::new(0);
static PORT_BUSY_NOTICED: AtomicU32 = AtomicU32::new(0);
/// Порт, ради которого ядро уже пробовали перевести под службу; сбрасывается,
/// когда ядро подтвердило слушателя.
static PORT_RECLAIMED: AtomicU32 = AtomicU32::new(0);
/// Системный прокси ещё указывает на прежний порт, а у ядра уже новый.
///
/// clod:port-ladder — взводится при смене порта, а также когда прокси включён,
/// но ядра в этот момент нет; снимается только после того, как ядро
/// подтвердило слушателя и прокси переписан. Так более новый старт
/// ядра (в том числе передача службе), погасивший прежнюю проверку по
/// поколению, доводит прокси сам, а обычный старт без смены порта в систему
/// не пишет ничего — как и раньше.
static PROXY_AWAITS_THE_NEW_PORT: AtomicBool = AtomicBool::new(false);

#[derive(Debug, PartialEq, Eq)]
enum PortReport {
    Serving,
    Other(u16),
    NotServing,
    Silent,
}

/// Итог проверки «слушает ли ядро свой порт».
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PortVerdict {
    Confirmed,
    Unknown,
    Refuted,
}

const fn port_report(reported: Option<u16>, expected: u16) -> PortReport {
    match reported {
        Some(port) if port == expected => PortReport::Serving,
        Some(0) => PortReport::NotServing,
        Some(port) => PortReport::Other(port),
        None => PortReport::Silent,
    }
}

const fn the_verdict_without_a_diagnosis(called_off: bool, answered: bool) -> Option<PortVerdict> {
    if called_off {
        return Some(PortVerdict::Refuted);
    }
    if answered {
        return None;
    }
    Some(PortVerdict::Unknown)
}

#[cfg(test)]
const fn the_port_check_budget(attempts: u32) -> Duration {
    let probes = timing::CORE_READY_PROBE_TIMEOUT.saturating_mul(attempts);
    let waits = timing::MIXED_PORT_CHECK_INTERVAL.saturating_mul(attempts.saturating_sub(1));
    probes.saturating_add(waits)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PortHolder {
    NotEvenTaken,
    AnotherCoreOfOurs,
    SomeoneElse,
}

async fn who_holds_the_port(
    port_is_taken: impl Future<Output = bool> + Send,
    another_core_of_ours: impl Future<Output = bool> + Send,
) -> PortHolder {
    if !port_is_taken.await {
        return PortHolder::NotEvenTaken;
    }
    if another_core_of_ours.await {
        PortHolder::AnotherCoreOfOurs
    } else {
        PortHolder::SomeoneElse
    }
}

/// Стоит ли переводить ядро своего процесса под службу ради порта.
///
/// clod:port-reclaim — после установки обновления служба отчитывается о
/// запуске раньше, чем открывает свой канал, и сама поднимает прежнее ядро
/// владельца. Приложение, не дождавшись её, стартует своим процессом, и
/// mihomo, не получив порт, к нему больше не возвращается. Под службой ядро
/// поднялось бы само (её запуск останавливает прежнее), а передача ядра
/// службе шла только ради TUN. Теперь у неё есть вторая причина — занятый
/// порт; ждёт она службу тем же сторожем. Свободный порт служба не
/// объясняет. Один раз: если передача не помогла, дальше — как раньше.
const fn worth_asking_the_service(holder: PortHolder, under_service: bool, already_asked: bool) -> bool {
    !under_service && !already_asked && !matches!(holder, PortHolder::NotEvenTaken)
}

/// О каком держателе порта говорить человеку.
///
/// Другое наше ядро лечит передача ядра службе. Если передавать некуда (ядро
/// уже под службой) или передача не удалась, эту копию — переподчинённую
/// системой после перезапуска службы или оставленную обновлением — не уберёт
/// никто, а трафик идёт через неё мимо ядра, которым управляет клиент.
/// «Служба не готова» сюда не относится: на Windows канал службы появляется
/// позже её запуска, и передачу доводит сторож.
const fn port_notice(holder: PortHolder, nothing_will_move_it: bool) -> Option<&'static str> {
    match holder {
        PortHolder::SomeoneElse => Some("core::port_busy"),
        PortHolder::AnotherCoreOfOurs if nothing_will_move_it => Some("core::port_held_by_our_copy"),
        PortHolder::AnotherCoreOfOurs | PortHolder::NotEvenTaken => None,
    }
}

fn say_who_holds_the_port(expected: u16, holder: PortHolder, nothing_will_move_it: bool, the_proxy_is_wanted: bool) {
    match holder {
        PortHolder::NotEvenTaken => {
            logging!(
                warn,
                Type::Core,
                "ядро не слушает порт {}, хотя порт свободен",
                expected
            );
        }
        PortHolder::AnotherCoreOfOurs => {
            logging!(
                warn,
                Type::Core,
                "порт {} занят другим нашим же ядром{}",
                expected,
                if nothing_will_move_it {
                    " — убрать его некому"
                } else {
                    " — оставляем как есть"
                }
            );
        }
        PortHolder::SomeoneElse => {
            logging!(
                error,
                Type::Core,
                "порт {} занят посторонним приложением: ядро его не слушает, трафик через системный прокси не пойдёт",
                expected
            );
        }
    }
    if let Some(status) = port_notice(holder, nothing_will_move_it)
        && the_proxy_is_wanted
        && PORT_BUSY_NOTICED.swap(u32::from(expected), Ordering::AcqRel) != u32::from(expected)
    {
        Handle::notice_message(status, expected.to_string());
    }
}

const fn should_wait_for_service(tun_enabled: bool, service_ready: bool, is_admin: bool) -> bool {
    tun_enabled && !service_ready && !is_admin
}

#[derive(Debug, PartialEq, Eq)]
pub enum ExitStop {
    Stopped,
    LockBusy,
    Failed {
        reason: std::string::String,
        core_alive: bool,
    },
}

/// Зачем ядру своего процесса переезжать под службу.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HandoffReason {
    /// Нужен TUN, а его поднимает только служба.
    Tun,
    /// Порт ядра занят, и держит его, скорее всего, ядро, которое служба
    /// подняла сама; её запуск его остановит. Причина живёт, пока ядро порт
    /// не подтвердило.
    PortTaken,
}

impl HandoffReason {
    async fn still_holds(self) -> bool {
        match self {
            Self::Tun => Config::verge().await.latest_arc().enable_tun_mode.unwrap_or(false),
            Self::PortTaken => PORT_RECLAIMED.load(Ordering::Acquire) != 0,
        }
    }
}

/// Результат передачи sidecar→service
enum HandoffOutcome {
    /// Служба ещё не готова
    NotReady,
    /// Передача завершена или не требуется
    Done,
    /// Передача не удалась, выполнен откат
    Failed,
}

impl CoreManager {
    pub async fn start_core(&self) -> Result<()> {
        let _life = self.lifecycle_lock.lock().await;
        self.start_core_inner().await
    }

    /// После отменённого выхода: пережившее остановку ядро снова рабочее, а
    /// если его тем временем не стало — поднимается заново.
    pub async fn resume_after_a_cancelled_exit(&self) -> Result<()> {
        let _life = self.lifecycle_lock.lock().await;
        // Пока шёл выход, смерть процесса никто не засчитывал: возвращать
        // можно только ядро, которое всё ещё в таблице процессов, — и именно
        // ядро, а не чужой процесс, которому достался его номер.
        if let (Backend::Sidecar, Some(pid)) = (self.backend(), self.sidecar_pid())
            && (matches!(
                crate::core::orphan::look_at_process(pid).await,
                crate::core::orphan::Look::Gone
            ) || !crate::core::orphan::pid_belongs_to_our_core(pid).await)
        {
            self.clear_sidecar_pid();
            self.note_core_is_down();
        }
        if self.take_the_core_back() {
            self.after_core_process();
            return Ok(());
        }
        self.start_core_inner().await
    }

    /// Перезапуск просили ради работающего ядра. Если прежнее не остановилось,
    /// второго поверх него не будет, а прежнее остаётся рабочим: под сторожем,
    /// и его падение снова перезапускается. Вызывающий держит `lifecycle_lock`.
    fn keep_the_core_that_would_not_stop(&self) -> Result<()> {
        if !self.stop_failed() {
            return Ok(());
        }
        self.take_the_core_back();
        self.watch_the_core_again_locked();
        self.after_core_process();
        anyhow::bail!("ядро прежнего запуска не остановилось — перезапуск отменён, оно продолжает работать")
    }

    /// Вызывающий должен уже удерживать `lifecycle_lock`.
    async fn start_core_inner(&self) -> Result<()> {
        // При завершении работы новое ядро больше не запускается.
        if Handle::global().is_exiting() {
            return Ok(());
        }

        self.refuse_to_double_the_core()?;

        // Конфиг при запуске приложения отвергнут ядром (или не собрался), а
        // принятого нет: поднимать нечего. Снимается первой принятой доставкой.
        if let Some(reason) = self.startup_refusal() {
            anyhow::bail!("ядро не запускается: конфиг отвергнут при запуске приложения — {reason}");
        }

        // Идемпотентность при уже работающем ядре; для рестарта использовать restart_core.
        if !matches!(*self.get_running_mode(), RunningMode::NotRunning) {
            logging!(
                info,
                Type::Core,
                "start_core called while a core is running; treated as no-op"
            );
            return Ok(());
        }

        self.mark_starting();
        defer! {
            self.clear_starting();
            self.after_core_process();
        }
        self.prepare_startup().await;

        if Handle::global().is_exiting() {
            return Ok(());
        }

        let attempted_service = matches!(self.backend(), Backend::Service);
        let mut result = self.start_and_confirm(attempted_service).await;

        // clod:tun-ready — служба может отказать (сломана, старой версии,
        // пользователь отклонил переустановку). Раньше это означало «ядро не
        // запущено вообще», то есть отсутствие интернета вместо потери TUN.
        // Падаем в sidecar и продолжаем ждать службу в фоне.
        if let Err(e) = &result
            && attempted_service
        {
            logging!(
                warn,
                Type::Core,
                "service start failed ({}); falling back to sidecar",
                e
            );
            let rejected_bundle = {
                let runtime = Config::runtime().await;
                let accepted = runtime.data_arc();
                accepted
                    .config
                    .as_ref()
                    .and_then(crate::core::service::bundle_rejection_for)
            };
            result = self.start_and_confirm(false).await;
            if result.is_ok()
                && let Some(message) = rejected_bundle
            {
                Handle::notice_message("service::bundle_rejected", message);
            }
        }

        if let Err(error) = &result {
            Handle::notice_message("core::not_ready", error.to_string());
            return result;
        }

        if Handle::global().is_exiting() {
            return result;
        }

        // clod:dns-applied — конфиг живой, теперь можно ставить подмену DNS.
        #[cfg(target_os = "macos")]
        crate::utils::resolve::dns::apply_remembered_desire();

        // clod:tun-ready — проверяем факт, а не заявку: если ядро не смогло
        // поднять устройство, честно гасим TUN и говорим об этом.
        if crate::feat::tun::desired().await && !crate::feat::tun::is_suppressed() {
            crate::feat::tun::spawn_start_verification(crate::feat::tun::log_anchor().await);
        } else {
            AsyncHandler::spawn(|| async { crate::feat::tun::enforce_undesired_off().await });
        }

        crate::feat::environment::spawn_environment_watchdog();

        // После отката к sidecar в фоне ждём готовности службы для передачи
        if matches!(*self.get_running_mode(), RunningMode::Sidecar) {
            self.spawn_service_handoff_watcher(HandoffReason::Tun).await;
        }

        result
    }

    async fn start_and_confirm(&self, use_service: bool) -> Result<()> {
        if use_service {
            self.start_core_by_service().await?;
        } else {
            self.start_core_by_sidecar().await?;
        }

        let Err(error) = self.confirm_core_ready().await else {
            Self::spawn_mixed_port_check(false);
            Self::new_core_is_up();
            return Ok(());
        };

        logging!(
            error,
            Type::Core,
            "ядро запущено, но не отвечает по управляющему каналу: {}",
            error
        );
        if use_service {
            let _ = self.stop_core_by_service().await;
        } else if let Err(stop_error) = self.stop_core_by_sidecar().await {
            logging!(warn, Type::Core, "{stop_error:#}");
        }
        Err(error)
    }

    /// Новое ядро ответило. Процесс поднимается с первым узлом каждой группы —
    /// выбор человека возвращаем здесь, после любого запуска: при старте
    /// приложения, перезапуске, смене ядра, переходе на службу и обратно,
    /// подъёме после падения; ядро, которое перезапустила сама служба, — в
    /// стороже здоровья. Первый подъём за сеанс — ещё и повод проверки 16–20.
    pub(super) fn new_core_is_up() {
        if Handle::global().is_exiting() {
            return;
        }
        if let Err(error) = crate::config::profiles::activate_selected_nodes() {
            logging!(warn, Type::Core, "выбор узлов после запуска ядра не вернулся: {error}");
        }
        crate::module::freeze_check::core_came_up();
    }

    async fn confirm_core_ready(&self) -> Result<()> {
        let mut last: Option<std::string::String> = None;

        for _ in 0..timing::CORE_READY_ATTEMPTS {
            if Handle::global().is_exiting() {
                return Ok(());
            }
            if matches!(*self.get_running_mode(), RunningMode::NotRunning) {
                anyhow::bail!("ядро завершилось, не ответив");
            }

            let probe = {
                let mihomo = Handle::mihomo();
                tokio::time::timeout(timing::CORE_READY_PROBE_TIMEOUT, mihomo.get_version()).await
            };
            match probe {
                Ok(Ok(_)) => return Ok(()),
                Ok(Err(error)) => last = Some(error.to_string()),
                Err(_) => last = Some("ядро не ответило за отведённое время".to_owned()),
            }

            // На процесс смотрим только когда ядро не ответило: здоровый старт
            // обходится без единого обхода таблицы процессов, а не сорока.
            if let Some(pid) = self.sidecar_pid()
                && !crate::core::orphan::process_is_alive(pid).await
            {
                super::state::handle_core_exit(
                    &format!("процесс ядра {} завершился во время проверки готовности", pid),
                    &RunningMode::Sidecar,
                    Some(pid),
                );
                continue;
            }

            tokio::time::sleep(timing::CORE_READY_INTERVAL).await;
        }

        anyhow::bail!("{}", last.unwrap_or_else(|| "причина неизвестна".to_owned()))
    }

    /// Убедиться, что ядро слушает свой порт, и только потом указать на него
    /// системный прокси.
    ///
    /// clod:port-ladder — порт мог приехать из подписки и оказаться занятым
    /// посторонним приложением. Переписать прокси сразу значило бы отправить
    /// весь трафик в это приложение; проверка занятости раньше жила только на
    /// пути полного запуска ядра, а мягкую перезагрузку конфига обходила.
    /// Пока ядро не подтвердило порт, прокси остаётся на прежнем: без
    /// интернета, но и без утечки, а о занятом порте скажет уведомление.
    ///
    /// Зовётся после КАЖДОГО старта ядра и после каждой смены порта: одна и та
    /// же задача, один и тот же номер поколения. Так более новый старт (в том
    /// числе передача ядра службе) гасит прежнюю проверку и заканчивает её
    /// работу сам, а прокси не остаётся на старом порту.
    pub(super) fn spawn_mixed_port_check(the_port_changed: bool) {
        if the_port_changed {
            PROXY_AWAITS_THE_NEW_PORT.store(true, Ordering::Release);
        }
        let generation = MIXED_PORT_CHECK_GENERATION.fetch_add(1, Ordering::AcqRel) + 1;
        AsyncHandler::spawn(move || async move {
            let expected = Config::mixed_port_the_core_was_started_with().await;
            match Self::confirm_mixed_port(generation, expected, timing::MIXED_PORT_CHECK_ATTEMPTS).await {
                PortVerdict::Confirmed | PortVerdict::Unknown => {
                    if MIXED_PORT_CHECK_GENERATION.load(Ordering::Acquire) == generation
                        && PROXY_AWAITS_THE_NEW_PORT.swap(false, Ordering::AcqRel)
                    {
                        super::config::point_system_proxy_at_the_core().await;
                    }
                }
                PortVerdict::Refuted => {}
            }
        });
    }

    pub async fn point_system_proxy_at_the_confirmed_port(&self) {
        let core_is_gone = matches!(*self.get_running_mode(), RunningMode::NotRunning);
        if !core_is_gone {
            Self::spawn_mixed_port_check(true);
            return;
        }
        // Желание не теряется: прокси допишет первый же старт ядра, который
        // подтвердит порт.
        if Config::verge().await.latest_arc().enable_system_proxy.unwrap_or(false) {
            PROXY_AWAITS_THE_NEW_PORT.store(true, Ordering::Release);
            logging!(
                warn,
                Type::Core,
                "ядро не запущено — системный прокси будет записан, когда оно подтвердит порт"
            );
        }
    }

    pub async fn the_core_must_serve_its_mixed_port(&self) -> Result<()> {
        if matches!(*self.get_running_mode(), RunningMode::NotRunning) {
            anyhow::bail!("ядро не запущено — системный прокси оставлен как был");
        }
        let generation = MIXED_PORT_CHECK_GENERATION.fetch_add(1, Ordering::AcqRel) + 1;
        let expected = Config::mixed_port_the_core_was_started_with().await;
        PORT_BUSY_NOTICED.store(0, Ordering::Release);
        match Self::confirm_mixed_port(generation, expected, timing::MIXED_PORT_CONFIRM_ATTEMPTS).await {
            PortVerdict::Confirmed => Ok(()),
            PortVerdict::Unknown => {
                logging!(
                    info,
                    Type::Core,
                    "ядро не ответило про порт {} за отведённое время — проверку продолжит фоновая",
                    expected
                );
                Self::spawn_mixed_port_check(false);
                Ok(())
            }
            PortVerdict::Refuted => {
                anyhow::bail!("ядро не подтвердило порт {expected} — системный прокси оставлен как был")
            }
        }
    }

    fn the_port_check_is_called_off(generation: u64) -> bool {
        Handle::global().is_exiting()
            || MIXED_PORT_CHECK_GENERATION.load(Ordering::Acquire) != generation
            || matches!(*Self::global().get_running_mode(), RunningMode::NotRunning)
    }

    /// Дождаться от ядра ответа, слушает ли оно `expected`.
    ///
    /// `Confirmed` — ядро само сказало «слушаю этот порт». `Unknown` — ядро
    /// приняло конфиг, но так и не ответило на вопрос: занятость порта не
    /// доказана, и с прокси поступаем как до этой проверки. `Refuted` — ядро
    /// ответило другим портом или нулём, порт занят, ядро остановлено,
    /// приложение выходит либо проверку сменила более новая.
    async fn confirm_mixed_port(generation: u64, expected: u16, attempts: u32) -> PortVerdict {
        let manager = Self::global();
        let mut answered = false;
        for attempt in 0..attempts {
            if Self::the_port_check_is_called_off(generation) {
                return PortVerdict::Refuted;
            }

            let reported = {
                let mihomo = Handle::mihomo();
                match tokio::time::timeout(timing::CORE_READY_PROBE_TIMEOUT, mihomo.get_base_config()).await {
                    Ok(Ok(config)) => Some(config.mixed_port),
                    _ => None,
                }
            };
            match port_report(reported, expected) {
                PortReport::Serving => {
                    PORT_BUSY_NOTICED.store(0, Ordering::Release);
                    PORT_RECLAIMED.store(0, Ordering::Release);
                    return PortVerdict::Confirmed;
                }
                PortReport::Other(port) => {
                    logging!(
                        warn,
                        Type::Core,
                        "ядро слушает порт {} вместо запрошенного {}",
                        port,
                        expected
                    );
                    return PortVerdict::Refuted;
                }
                PortReport::NotServing => answered = true,
                PortReport::Silent => {}
            }

            if attempt + 1 < attempts {
                tokio::time::sleep(timing::MIXED_PORT_CHECK_INTERVAL).await;
            }
        }

        match the_verdict_without_a_diagnosis(Self::the_port_check_is_called_off(generation), answered) {
            Some(PortVerdict::Unknown) => {
                logging!(
                    warn,
                    Type::Core,
                    "ядро не ответило, слушает ли оно порт {} — проверку пропускаем",
                    expected
                );
                return PortVerdict::Unknown;
            }
            Some(verdict) => return verdict,
            None => {}
        }

        let mode = manager.get_running_mode();
        let own_pid = if matches!(*mode, RunningMode::Sidecar) {
            manager.sidecar_pid()
        } else {
            None
        };
        // Вердикту диагноз не нужен: обход процессов идёт своей задачей и без
        // срока, а устаревшая проверка виновного уже не называет.
        // Желание читается сейчас: отказ тумблера сбросит черновик настроек
        // раньше, чем закончится обход процессов.
        let under_service = matches!(*mode, RunningMode::Service);
        let the_proxy_is_wanted = Config::verge().await.latest_arc().enable_system_proxy.unwrap_or(false);
        AsyncHandler::spawn(move || async move {
            let holder = who_holds_the_port(
                crate::cmd::network::is_port_in_use(expected),
                crate::core::orphan::another_core_of_ours_is_running(own_pid, under_service),
            )
            .await;
            if Self::the_port_check_is_called_off(generation) {
                return;
            }
            let already_asked = PORT_RECLAIMED.load(Ordering::Acquire) == u32::from(expected);
            let mut nothing_will_move_it = under_service;
            if worth_asking_the_service(holder, under_service, already_asked) {
                PORT_RECLAIMED.store(u32::from(expected), Ordering::Release);
                let manager = Self::global();
                match manager.try_handoff_sidecar_to_service(HandoffReason::PortTaken).await {
                    HandoffOutcome::Done => return,
                    HandoffOutcome::NotReady => manager.spawn_service_handoff_watcher(HandoffReason::PortTaken).await,
                    HandoffOutcome::Failed => nothing_will_move_it = true,
                }
                if Self::the_port_check_is_called_off(generation) {
                    return;
                }
            }
            say_who_holds_the_port(expected, holder, nothing_will_move_it, the_proxy_is_wanted);
        });
        PortVerdict::Refuted
    }

    pub async fn stop_core(&self) -> Result<()> {
        let _life = self.lifecycle_lock.lock().await;
        self.stop_core_inner().await
    }

    pub async fn stop_core_for_exit(&self, lock_wait: Duration, stop_budget: Duration) -> ExitStop {
        // Замок жизненного цикла живёт ровно до конца остановки: проверка
        // живости ниже ходит по сети (статус службы по IPC), а под замком
        // сетевых вызовов не делаем — его ждёт и восстановление после
        // отменённого выхода.
        let (mode_before, pid_before, reason) = {
            let Ok(_life) = tokio::time::timeout(lock_wait, self.lifecycle_lock.lock()).await else {
                return ExitStop::LockBusy;
            };
            let mode_before = self.get_running_mode();
            let pid_before = self.sidecar_pid();
            let reason = match tokio::time::timeout(stop_budget, self.stop_core_inner()).await {
                Ok(Ok(())) => return ExitStop::Stopped,
                Ok(Err(error)) => format!("{error:#}"),
                Err(_) => format!("нет ответа за {} с", stop_budget.as_secs()),
            };
            (mode_before, pid_before, reason)
        };
        let core_alive = match (&*mode_before, pid_before) {
            (RunningMode::Sidecar, Some(pid)) => {
                matches!(
                    crate::core::orphan::look_at_process(pid).await,
                    crate::core::orphan::Look::Alive
                )
            }
            (RunningMode::Service, _) => {
                tokio::time::timeout(timing::SERVICE_STATUS_WAIT, crate::core::service::service_status())
                    .await
                    .ok()
                    .and_then(Result::ok)
                    .is_some_and(|status| status.is_active && status.core_pid.is_some())
            }
            _ => false,
        };
        ExitStop::Failed { reason, core_alive }
    }

    /// Вызывающий должен уже удерживать `lifecycle_lock`.
    async fn stop_core_inner(&self) -> Result<()> {
        CLASH_LOGGER.clear_logs().await;
        defer! {
            self.after_core_process();
        }

        match *self.get_running_mode() {
            RunningMode::Service => self.stop_core_by_service().await,
            RunningMode::Sidecar => self.stop_core_by_sidecar().await,
            RunningMode::NotRunning => Ok(()),
        }
    }

    /// Перезапуск ядра по просьбе снаружи: кнопка, трей, смена сборки ядра,
    /// настройка, требующая перезапуска.
    ///
    /// clod:Э3-03 — перезапуск идёт в той же очереди, что и применение
    /// конфига: пока конфиг едет к ядру, режим работы (sidecar/служба) менять
    /// нельзя — иначе staged-путь службы уезжал бы в sidecar, а наш путь — в
    /// ядро под службой. Идёт применение — перезапуск дожидается его, а не
    /// вклинивается. Применение конфига само перезапускает ядро через
    /// `restart_core_during_config_update`, очередь у него уже есть.
    pub async fn restart_core(&self) -> Result<()> {
        let Some(_turn) = self.queue_for_config_update().await else {
            anyhow::bail!("очередь применения конфига закрыта");
        };
        self.restart_core_during_config_update().await
    }

    /// Вызывающий уже держит признак применения конфига.
    pub(super) async fn restart_core_during_config_update(&self) -> Result<()> {
        // Во время выхода перезапуск занял бы замок жизненного цикла на весь
        // старт, остановка ядра при выходе упёрлась бы в занятый замок — и ядро
        // осталось бы жить после закрытия приложения.
        if Handle::global().is_exiting() {
            anyhow::bail!("перезапуск ядра пропущен: выход уже идёт");
        }
        // Блокировка удерживается на весь stop+start, чтобы избежать вклинивания
        // других операций жизненного цикла.
        let _life = self.lifecycle_lock.lock().await;
        let _pause = self.planned_pause();
        logging!(info, Type::Core, "Restarting core");
        // Ошибка остановки при мёртвом ядре перезапуска не отменяет: новое
        // ядро всё равно нужно. Отменяет его только живое прежнее ядро.
        if let Err(error) = self.stop_core_inner().await {
            logging!(warn, Type::Core, "ядро не остановилось перед перезапуском: {error:#}");
        }
        self.keep_the_core_that_would_not_stop()?;
        self.start_core_inner().await
    }

    /// clod:core-updater — остановить ядро, выполнить `swap` (замена файла ядра),
    /// поднять снова; новое не поднялось — выполнить `rollback` и поднять ещё раз.
    /// Замок жизненного цикла держится на всю последовательность, чтобы между
    /// остановкой и стартом не вклинилась другая операция и не подняла ядро
    /// посреди замены. Любой путь ошибки старается оставить ядро работающим:
    /// обновление не должно стоить человеку связи.
    pub async fn restart_core_swapped(
        &self,
        swap: impl AsyncFnOnce() -> Result<()>,
        rollback: impl AsyncFnOnce() -> Result<()>,
    ) -> Result<()> {
        // clod:Э3-03 — подмена сборки тоже меняет ядро под ногами у применения
        // конфига: ждёт своей очереди.
        let Some(_turn) = self.queue_for_config_update().await else {
            anyhow::bail!("очередь применения конфига закрыта");
        };
        // Выход мог начаться, пока ждали очереди: ядро на выходе не стартует, и
        // замена без проверки запуска осталась бы до следующего запуска.
        if Handle::global().is_exiting() {
            anyhow::bail!("замена ядра пропущена: выход уже идёт");
        }
        let _life = self.lifecycle_lock.lock().await;
        let _pause = self.planned_pause();
        if let Err(error) = self.stop_core_inner().await {
            logging!(warn, Type::Core, "ядро не остановилось перед заменой: {error:#}");
        }
        // Прежнее ядро живо — новая сборка не запустится: отказ до обеих
        // записей указателей, откатывать нечего.
        self.keep_the_core_that_would_not_stop()?;

        // clod:tun-ready — новая сборка ядра заслуживает честной попытки.
        // Подавление ставится на сессию (ядро не смогло поднять устройство) и
        // в конфиг не пишется; пережив подмену бинаря, оно означало бы «TUN не
        // работает, потому что не работал у ПРОШЛОГО ядра» — а обновление ядра
        // как раз и берут ради таких починок. Проверку факта после старта
        // делает `start_core_inner`.
        crate::feat::tun::clear_suppression();

        if let Err(swap_error) = swap().await {
            // Nothing switched; bring the old core back before reporting.
            if let Err(start_error) = self.start_core_inner().await {
                return Err(swap_error.context(format!("and restarting the old core failed too: {start_error:#}")));
            }
            return Err(swap_error);
        }

        match self.start_core_inner().await {
            Ok(()) => Ok(()),
            Err(start_error) => {
                logging!(
                    error,
                    Type::Core,
                    "new core failed to start, rolling back: {start_error:#}"
                );
                let rollback_result = rollback().await;
                // Поднять ядро даже при неудавшемся возврате: какой бы файл ядра
                // ни стоял на месте, работающее ядро лучше, чем никакого.
                let restart_result = self.start_core_inner().await;
                match (rollback_result, restart_result) {
                    (Ok(()), Ok(())) => {
                        Err(start_error.context("the new core failed to start; the previous one is back"))
                    }
                    (Ok(()), Err(restart_error)) => Err(start_error.context(format!(
                        "and restarting the previous core failed too: {restart_error:#}"
                    ))),
                    (Err(rollback_error), Ok(())) => Err(start_error.context(format!(
                        "the rollback write failed ({rollback_error:#}) but a core is running again"
                    ))),
                    (Err(rollback_error), Err(restart_error)) => Err(start_error.context(format!(
                        "the rollback failed ({rollback_error:#}) and so did the restart: {restart_error:#}"
                    ))),
                }
            }
        }
    }

    /// Сменить встроенное ядро.
    ///
    /// clod:core-choice — тот же путь, что у правки настроек: выбор ложится в
    /// черновик, принятый конфиг проверяется новым ядром, ядро перезапускается уже
    /// им, и только тогда выбор записывается. Новое ядро отвергло конфиг — ничего
    /// не тронуто. Не поднялось — выбор прежний, и поднимается прежнее ядро.
    /// Так выбранное в настройках и работающее ядро не расходятся ни на каком пути.
    pub async fn change_core(&self, clash_core: &String) -> Result<(), String> {
        if !IVerge::VALID_CLASH_CORES.contains(&clash_core.as_str()) {
            return Err(format!("Invalid clash core: {}", clash_core).into());
        }

        // Пока обновляется файл ядра, ядро не меняем: обновление подменило бы файл
        // уже не выбранного ядра, а проверкой ему служил бы старт другого.
        if crate::core::core_updater::is_updating() {
            return Err("идёт обновление ядра — смените ядро, когда оно закончится".into());
        }
        let _serialized = crate::feat::patch_verge_lock().lock().await;
        // Выбор ложится в черновик уже в своей очереди: всё, что шло впереди,
        // собиралось и запускалось с прежним ядром.
        let Some(turn) = self.claim_for_an_update().await else {
            return Err(self.why_not_now().to_string().into());
        };
        let verge = Config::verge().await;
        verge.edit_draft(|draft| draft.clash_core = Some(clash_core.to_owned()));
        let sources = crate::enhance::Sources::default().with_verge(verge.latest_arc());

        let staged = match self.stage_in_turn(turn, sources).await {
            Ok(Ok(staged)) => staged,
            Ok(Err(outcome)) => {
                verge.discard();
                return Err(outcome.to_string().into());
            }
            Err(err) => {
                verge.discard();
                return Err(format!("{err:#}").into());
            }
        };

        let delivered = {
            self.core_switch.store(true, Ordering::Release);
            defer! {
                self.core_switch.store(false, Ordering::Release);
            }
            staged
                .deliver_committing(super::Delivery::Restart, async || {
                    verge.apply();
                    Ok(())
                })
                .await
        };

        let failure: String = match delivered {
            Ok(Ok(_)) => {
                // Ядро уже работает новым и выбор принят. Не записался файл — это
                // не «не сменилось»: настройки допишутся при следующей записи или
                // на выходе, а до тех пор работает выбранное.
                if let Err(err) = verge.data_arc().save_file().await {
                    logging!(warn, Type::Core, "выбор ядра не записан на диск: {err:#}");
                }
                return Ok(());
            }
            Ok(Err(outcome)) => outcome.to_string().into(),
            Err(err) => format!("{err:#}").into(),
        };

        verge.discard();
        logging!(
            warn,
            Type::Core,
            "ядро {clash_core} не поднялось, возвращаю прежнее: {failure}"
        );
        if let Err(err) = self.restart_core().await {
            logging!(error, Type::Core, "прежнее ядро тоже не поднялось: {err:#}");
        }
        Err(failure)
    }

    async fn prepare_startup(&self) {
        self.wait_for_service_if_needed().await;
        self.aim_at(match SERVICE_MANAGER.current().await {
            ServiceStatus::Ready => Backend::Service,
            _ => Backend::Sidecar,
        });
    }

    pub(super) fn after_core_process(&self) {
        if Handle::global().is_exiting() {
            return;
        }
        AsyncHandler::spawn(|| async {
            crate::core::tray::Tray::global().refresh_core_state().await;
            Handle::refresh_verge();
        });
    }

    async fn wait_for_service_if_needed(&self) {
        use crate::{config::Config, constants::timing, core::service};
        use backon::{ConstantBuilder, Retryable as _};

        // TUN, подавленный на этот сеанс (службы нет), ждать службу не просит.
        let tun_enabled =
            Config::verge().await.latest_arc().enable_tun_mode.unwrap_or(false) && !crate::feat::tun::is_suppressed();
        let service_ready = matches!(SERVICE_MANAGER.current().await, ServiceStatus::Ready);
        let is_admin = is_current_app_handle_admin(Handle::app_handle());

        if !should_wait_for_service(tun_enabled, service_ready, is_admin) {
            if tun_enabled && !service_ready && is_admin {
                logging!(
                    info,
                    Type::Core,
                    "service unavailable while app is elevated; starting sidecar immediately"
                );
            }
            return;
        }
        if service::was_uninstalled_this_session() {
            logging!(
                info,
                Type::Core,
                "служба удалена в этом сеансе — ждать её незачем, поднимаемся своим процессом"
            );
            return;
        }

        let max_times = timing::SERVICE_WAIT_MAX.as_millis() / timing::SERVICE_WAIT_INTERVAL.as_millis();
        let backoff = ConstantBuilder::default()
            .with_delay(timing::SERVICE_WAIT_INTERVAL)
            .with_max_times(max_times as usize);

        let attempts = (|| async {
            if Handle::global().is_exiting() {
                return Ok(());
            }
            if matches!(SERVICE_MANAGER.current().await, ServiceStatus::Ready) {
                return Ok(());
            }

            // If the service IPC path is not ready yet, treat it as transient and retry.
            // Running refresh too early can mark service state unavailable and break later config reloads.
            if !service::is_service_ipc_path_exists() {
                return Err(anyhow::anyhow!("Service IPC not ready"));
            }

            let _ = SERVICE_MANAGER.refresh().await;

            match SERVICE_MANAGER.current().await {
                ServiceStatus::Ready => Ok(()),
                // Служба внятно ответила «устарела» — ждать её незачем: ядро
                // поднимется своим процессом, а случайный ответ поправит
                // наблюдатель передачи.
                ServiceStatus::NeedsReinstall => {
                    logging!(
                        info,
                        Type::Core,
                        "служба ответила, что устарела, — не ждём её, поднимаемся своим процессом"
                    );
                    Ok(())
                }
                _ => Err(anyhow::anyhow!("Service not ready")),
            }
        })
        .retry(backoff);

        // clod: число попыток ограничивало ТОЛЬКО паузы между ними, а сама
        // попытка ходит в службу по IPC и может ждать сколько угодно — при этом
        // весь цикл стоит на пути запуска приложения. Общий потолок — вдвое от
        // отведённого на ожидание: дальше поднимаемся как sidecar, а служба,
        // когда очнётся, подхватится хэндоффом.
        let ceiling = timing::SERVICE_WAIT_MAX * 2;
        if tokio::time::timeout(ceiling, attempts).await.is_err() {
            logging!(
                warn,
                Type::Core,
                "служба не ответила за {:?} — продолжаем запуск без неё",
                ceiling
            );
        }
    }

    /// clod:tun-ready — служба появилась (например, мы её только что
    /// установили): переезжаем на неё сразу, не дожидаясь окна watcher-а.
    pub async fn handoff_to_service_if_needed(&self) {
        if Handle::global().is_exiting() {
            logging!(info, Type::Core, "передача ядра службе пропущена: выход уже идёт");
            return;
        }
        if !matches!(*self.get_running_mode(), RunningMode::Sidecar) {
            return;
        }
        if !crate::feat::tun::desired().await {
            return;
        }
        match self.try_handoff_sidecar_to_service(HandoffReason::Tun).await {
            HandoffOutcome::Done => {
                crate::feat::tun::spawn_bringing_tun_back_if_the_config_lacks_it("the core is under the service now");
            }
            HandoffOutcome::NotReady => self.spawn_service_handoff_watcher(HandoffReason::Tun).await,
            HandoffOutcome::Failed => {
                logging!(warn, Type::Core, "immediate handoff failed; staying in sidecar mode");
            }
        }
    }

    /// Ждёт готовности службы в течение окна времени, затем передаёт от sidecar к service
    async fn spawn_service_handoff_watcher(&self, reason: HandoffReason) {
        use crate::constants::timing;
        use crate::process::AsyncHandler;
        use std::sync::atomic::Ordering;
        use std::time::Instant;

        if !reason.still_holds().await {
            return;
        }

        let generation = self.handoff_watcher_generation.fetch_add(1, Ordering::AcqRel) + 1;

        logging!(
            info,
            Type::Core,
            "service not ready; sidecar active, watching for handoff ({:?})",
            reason
        );

        AsyncHandler::spawn(move || async move {
            let manager = Self::global();
            let started = Instant::now();
            loop {
                if manager.handoff_watcher_generation.load(Ordering::Acquire) != generation {
                    return;
                }
                if started.elapsed() >= timing::SERVICE_HANDOFF_WINDOW {
                    logging!(
                        info,
                        Type::Core,
                        "service handoff window elapsed; staying in sidecar mode"
                    );
                    break;
                }
                tokio::time::sleep(timing::SERVICE_HANDOFF_INTERVAL).await;

                if manager.handoff_watcher_generation.load(Ordering::Acquire) != generation {
                    return;
                }
                if Handle::global().is_exiting() {
                    continue;
                }

                // Выходим, если режим уже изменился
                if !matches!(*manager.get_running_mode(), RunningMode::Sidecar) {
                    break;
                }
                match manager.try_handoff_sidecar_to_service(reason).await {
                    // Передано или не требуется
                    HandoffOutcome::Done => {
                        crate::feat::tun::spawn_bringing_tun_back_if_the_config_lacks_it(
                            "the core is under the service now",
                        );
                        break;
                    }
                    // Откат к sidecar выполнен, прекращаем попытки
                    HandoffOutcome::Failed => {
                        logging!(warn, Type::Core, "handoff attempt failed; staying in sidecar mode");
                        break;
                    }
                    HandoffOutcome::NotReady => {}
                }
            }
        });
    }

    /// Проверяет, готова ли служба принять ядро; `Some` — передачу начинать нельзя
    async fn handoff_is_out_of_reach(&self) -> Option<HandoffOutcome> {
        use crate::core::service;

        if Handle::global().is_exiting() {
            return Some(HandoffOutcome::NotReady);
        }

        // Принудительно обновляем состояние службы, чтобы кэшированное состояние
        // не блокировало передачу
        if !service::is_service_ipc_path_exists() {
            return Some(HandoffOutcome::NotReady);
        }
        let _ = SERVICE_MANAGER.refresh().await;
        if !matches!(SERVICE_MANAGER.current().await, ServiceStatus::Ready) {
            return Some(HandoffOutcome::NotReady);
        }
        if service::bundle_rejection_for_the_running_config().await.is_some() {
            logging!(
                info,
                Type::Core,
                "the current configuration cannot run under the service; staying in sidecar mode until it changes"
            );
            return Some(HandoffOutcome::Failed);
        }

        None
    }

    /// После готовности службы останавливает sidecar и перезапускает ядро через service
    async fn try_handoff_sidecar_to_service(&self, reason: HandoffReason) -> HandoffOutcome {
        if let Some(outcome) = self.handoff_is_out_of_reach().await {
            return outcome;
        }

        // Сначала очередь применения конфига; занята — уступаем идущему применению,
        // передача повторит себя сама.
        let Some(_turn) = self.claim_config_update() else {
            return HandoffOutcome::NotReady;
        };

        // Затем захватываем блокировку lifecycle; порядок блокировок фиксирован: config→lifecycle.
        let _life = self.lifecycle_lock.lock().await;
        let _pause = self.planned_pause();

        if Handle::global().is_exiting() {
            return HandoffOutcome::NotReady;
        }

        // После захвата блокировки повторно проверяем режим работы и причину
        if !matches!(*self.get_running_mode(), RunningMode::Sidecar) || !reason.still_holds().await {
            return HandoffOutcome::Done;
        }

        logging!(
            info,
            Type::Core,
            "service became ready; handing off from sidecar to service"
        );
        if let Err(error) = self.stop_core_by_sidecar().await {
            logging!(warn, Type::Core, "handoff aborted: {error:#}");
            return HandoffOutcome::Failed;
        }

        if Handle::global().is_exiting() {
            logging!(info, Type::Core, "передача ядра службе прервана: выход уже идёт");
            return HandoffOutcome::NotReady;
        }

        match self.start_and_confirm(true).await {
            Ok(()) => {
                logging!(info, Type::Core, "handoff to service mode succeeded");
                HandoffOutcome::Done
            }
            Err(e) => {
                logging!(
                    error,
                    Type::Core,
                    "handoff to service failed: {}; restarting sidecar",
                    e
                );
                self.roll_back_to_sidecar().await;
                HandoffOutcome::Failed
            }
        }
    }

    async fn roll_back_to_sidecar(&self) {
        if Handle::global().is_exiting() {
            return;
        }
        if let Err(error) = self.start_and_confirm(false).await {
            logging!(
                error,
                Type::Core,
                "failed to restart sidecar after handoff failure: {}",
                error
            );
            Handle::notice_message("core::handoff_failed", error.to_string());
            if let Err(last) = self.start_core_by_sidecar().await {
                logging!(
                    error,
                    Type::Core,
                    "sidecar did not come back after the handoff at all: {}",
                    last
                );
                return;
            }
            // Поднят без подтверждения готовности — выбор возвращаем сами.
            Self::new_core_is_up();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        PortHolder, PortReport, PortVerdict, port_notice, port_report, should_wait_for_service, the_port_check_budget,
        the_verdict_without_a_diagnosis, who_holds_the_port, worth_asking_the_service,
    };
    use crate::constants::timing;
    use crate::utils::source_scan::fn_body;
    use std::time::Duration;

    #[test]
    fn the_check_that_holds_the_settings_lock_is_measured_in_seconds() {
        let asking_the_core = the_port_check_budget(timing::MIXED_PORT_CONFIRM_ATTEMPTS);

        assert!(
            asking_the_core <= Duration::from_secs(4),
            "проверка под замком настроек стоит {asking_the_core:?}"
        );
        assert!(
            the_port_check_budget(timing::MIXED_PORT_CHECK_ATTEMPTS) >= asking_the_core * 4,
            "фоновая проверка ждёт ядро дольше, чем правка настроек"
        );
    }

    #[test]
    fn a_check_called_off_after_the_last_question_names_nobody() {
        assert_eq!(
            the_verdict_without_a_diagnosis(true, true),
            Some(PortVerdict::Refuted),
            "выход начался после последней попытки — обход процессов уже никому не нужен"
        );
        assert_eq!(the_verdict_without_a_diagnosis(true, false), Some(PortVerdict::Refuted));
    }

    #[test]
    fn a_check_nobody_called_off_still_asks_who_holds_the_port() {
        assert_eq!(the_verdict_without_a_diagnosis(false, true), None);
        assert_eq!(
            the_verdict_without_a_diagnosis(false, false),
            Some(PortVerdict::Unknown)
        );
    }

    #[tokio::test]
    async fn a_free_port_is_blamed_on_nobody() {
        let asked_about_processes = std::sync::atomic::AtomicBool::new(false);
        let holder = who_holds_the_port(async { false }, async {
            asked_about_processes.store(true, std::sync::atomic::Ordering::SeqCst);
            true
        })
        .await;

        assert_eq!(holder, PortHolder::NotEvenTaken);
        assert!(
            !asked_about_processes.load(std::sync::atomic::Ordering::SeqCst),
            "свободный порт незачем искать среди процессов"
        );
    }

    #[tokio::test]
    async fn a_busy_port_names_a_stranger_only_when_it_is_not_our_own_core() {
        assert_eq!(
            who_holds_the_port(async { true }, async { true }).await,
            PortHolder::AnotherCoreOfOurs
        );
        assert_eq!(
            who_holds_the_port(async { true }, async { false }).await,
            PortHolder::SomeoneElse
        );
    }

    /// Под службу переводят, только когда ядро своим процессом, порт кем-то
    /// занят и ради него ещё не пробовали.
    #[test]
    fn the_service_is_asked_once_and_only_for_a_core_of_its_own_process_left_without_its_port() {
        use PortHolder::{AnotherCoreOfOurs, NotEvenTaken, SomeoneElse};
        assert!(worth_asking_the_service(SomeoneElse, false, false));
        assert!(worth_asking_the_service(AnotherCoreOfOurs, false, false));
        for (holder, under_service, already_restarted) in [
            (NotEvenTaken, false, false),
            (SomeoneElse, true, false),
            (SomeoneElse, false, true),
        ] {
            assert!(
                !worth_asking_the_service(holder, under_service, already_restarted),
                "{holder:?} служба={under_service} уже={already_restarted}"
            );
        }
    }

    /// Человеку говорят о постороннем держателе всегда, а о другом нашем ядре —
    /// только когда его уже никто не уберёт: ядро под службой или передача
    /// службе не удалась.
    #[test]
    fn a_copy_of_our_core_is_reported_only_when_nothing_will_move_it() {
        use PortHolder::{AnotherCoreOfOurs, NotEvenTaken, SomeoneElse};
        for (holder, nothing_will_move_it, expected) in [
            (SomeoneElse, false, Some("core::port_busy")),
            (SomeoneElse, true, Some("core::port_busy")),
            (AnotherCoreOfOurs, true, Some("core::port_held_by_our_copy")),
            (AnotherCoreOfOurs, false, None),
            (NotEvenTaken, false, None),
            (NotEvenTaken, true, None),
        ] {
            assert_eq!(
                port_notice(holder, nothing_will_move_it),
                expected,
                "{holder:?} убрать некому={nothing_will_move_it}"
            );
        }
    }

    #[test]
    fn a_single_attempt_costs_only_its_probe() {
        assert_eq!(the_port_check_budget(1), timing::CORE_READY_PROBE_TIMEOUT);
        assert_eq!(the_port_check_budget(0), Duration::ZERO);
    }

    /// Дефект был не в таблице истинности, а в проводке: хвост проверки не
    /// спрашивал гейт и шёл обходить процессы у проверки, которую уже отменили.
    /// Чистая функция этого не ловит — её можно оставить на месте и вернуть хвост.
    #[test]
    fn the_tail_of_the_port_check_still_asks_the_gate_before_naming_a_culprit() {
        let source = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/core/manager/lifecycle.rs"))
            .unwrap_or_default();
        let body = fn_body(&source, "async fn confirm_mixed_port").unwrap_or_default();

        assert!(!body.is_empty(), "тело confirm_mixed_port не найдено — тест ослеп");
        assert!(
            body.contains("the_verdict_without_a_diagnosis("),
            "хвост проверки порта больше не спрашивает вердикт без диагноза"
        );
        assert!(
            body.matches("the_port_check_is_called_off(").count() >= 3,
            "гейт стоит в начале круга, перед диагнозом и перед словом о виновном: без \
             любого из них отменённая проверка назовёт виновного"
        );
    }

    #[test]
    fn a_silent_core_is_not_a_busy_port() {
        assert_eq!(port_report(None, 7897), PortReport::Silent);
        assert_eq!(port_report(Some(0), 7897), PortReport::NotServing);
        assert_eq!(port_report(Some(7897), 7897), PortReport::Serving);
        assert_eq!(port_report(Some(7890), 7897), PortReport::Other(7890));
    }

    #[test]
    fn service_wait_is_only_required_for_non_admin_tun() {
        assert!(should_wait_for_service(true, false, false));
        assert!(!should_wait_for_service(true, false, true));
        assert!(!should_wait_for_service(true, true, false));
        assert!(!should_wait_for_service(false, false, false));
    }
}
