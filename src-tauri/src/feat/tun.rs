//! clod:tun-ready — готовность TUN как отдельная величина.
//!
//! Раньше состояние TUN было одним флагом `enable_tun_mode`, и любой код,
//! которому TUN показался недоступным, писал в него `false` — прямо в
//! `verge.yaml`. При автозапуске служба поднимается позже приложения, так что
//! выбор пользователя стирался необратимо.
//!
//! Теперь величины три:
//!   * **желание** — `connect_tun_mode` / `enable_tun_mode` в конфиге, меняет
//!     только пользователь;
//!   * **заявка** — то, что уходит в конфиг ядра: желание И НЕ подавление;
//!   * **факт** — подтверждение от ядра, что интерфейс поднялся; проверяется не
//!     один раз после включения, а по кругу, пока заявка держится (ядро роняет
//!     туннель и позже старта, а его вывод непрерывно читают только у sidecar).
//!
//! Подавление живёт только в памяти процесса: перезапуск приложения (или
//! появление службы) снимает его само собой, а файл конфигурации не трогается.

use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};

use clash_verge_logging::{Type, logging};
use parking_lot::Mutex;
use tauri_plugin_clash_verge_sysinfo::is_current_app_handle_admin;

use crate::{
    config::Config,
    constants::timing,
    core::{
        handle::Handle,
        service::{
            ElevationPending, SERVICE_MANAGER, ServiceBusy, ServiceRegistration, ServiceStatus, elevation_in_flight,
            is_service_available, service_registration,
        },
    },
    process::AsyncHandler,
};

/// TUN недоступен в этой сессии: заявка снимается, желание остаётся.
static SUPPRESSED: AtomicBool = AtomicBool::new(false);
/// Ядро уже сообщило, что поднять устройство не смогло.
static START_FAILED: AtomicBool = AtomicBool::new(false);
/// Установка службы уже идёт — второй UAC не нужен.
static SETUP_RUNNING: AtomicBool = AtomicBool::new(false);
/// The periodic fact check is already running. Same singleton pattern as the
/// handoff watcher (`CoreManager::handoff_watcher_running`): every re-enable of
/// TUN would otherwise spawn one more task reading the same log buffer forever.
static WATCHDOG_RUNNING: AtomicBool = AtomicBool::new(false);
/// Where the current TUN attempt starts in the core log, shared between the
/// one-shot check and the watchdog.
///
/// It has to be shared rather than owned by each task: re-enabling TUN right
/// after a failure (which is exactly what a user does when told the tunnel did
/// not come up) must move the anchor for the watchdog that is already sleeping,
/// or it would wake up, find the *previous* complaint still in the buffer and
/// report it against the new attempt.
static WATCH_ANCHOR: Mutex<Option<String>> = Mutex::new(None);

/// Строки mihomo, по которым видно, что TUN не поднялся. Проверяются в нижнем
/// регистре, поэтому здесь только нижний.
const TUN_FAILURE_MARKERS: &[&str] = &["start tun listening error", "configure tun interface"];

pub fn is_suppressed() -> bool {
    SUPPRESSED.load(Ordering::Acquire)
}

/// Снять заявку на TUN до конца сессии. Конфиг на диске не трогаем.
pub fn suppress(reason: &str) {
    if !SUPPRESSED.swap(true, Ordering::AcqRel) {
        logging!(warn, Type::Core, "TUN suppressed for this session: {}", reason);
    }
}

/// Условия изменились (появилась служба, пользователь переключил тумблер) —
/// заявку снова можно подавать.
pub fn clear_suppression() {
    SUPPRESSED.store(false, Ordering::Release);
    START_FAILED.store(false, Ordering::Release);
}

/// Пользователь хочет TUN (в терминах конфига — заявка сохранена).
pub async fn desired() -> bool {
    Config::verge().await.latest_arc().enable_tun_mode.unwrap_or(false)
}

/// The claim as the core sees it — the same rule `enhance` applies when it
/// builds the core config.
///
/// It is also the only reason to keep watching the core output: with the wish
/// gone there is no tunnel to guard, and with the claim suppressed the failure
/// has already been reported and acted upon. Kept as a pure function so the
/// watchdog's stop condition can be checked without a core or a service.
const fn is_claimed(desired: bool, suppressed: bool) -> bool {
    desired && !suppressed
}

/// Заявка на TUN подана прямо сейчас.
async fn claimed() -> bool {
    is_claimed(desired().await, is_suppressed())
}

/// Приложение уже привилегировано — служба для TUN не нужна.
pub fn is_app_elevated() -> bool {
    is_current_app_handle_admin(Handle::app_handle())
}

/// TUN технически возможен прямо сейчас.
///
/// clod: мало того, что служба отвечает — она должна быть той же версии, что и
/// приложение. Служба, оставшаяся от прошлой версии, отвечает по IPC, но ядро
/// через неё не поднимется: без этой проверки клиент считал бы TUN доступным и
/// упирался бы в «TUN не запустился» по кругу.
pub async fn is_capable() -> bool {
    if is_app_elevated() {
        return true;
    }
    if is_service_available().await.is_err() {
        return false;
    }
    !clash_verge_service_ipc::is_reinstall_service_needed().await
}

/// Служба установлена и отвечает, но устарела — её надо чинить, а не ставить.
async fn service_needs_repair() -> bool {
    is_service_available().await.is_ok() && clash_verge_service_ipc::is_reinstall_service_needed().await
}

/// Строка из вывода ядра похожа на провал старта TUN.
pub fn line_reports_tun_failure(line: &str) -> bool {
    let lowered = line.to_ascii_lowercase();
    TUN_FAILURE_MARKERS.iter().any(|marker| lowered.contains(marker))
}

/// Ядро не смогло поднять устройство: честно гасим заявку и говорим об этом,
/// вместо зелёной кнопки при мёртвом туннеле.
pub fn report_start_failure(detail: &str) {
    if START_FAILED.swap(true, Ordering::AcqRel) {
        return;
    }
    suppress("core failed to start the TUN device");
    logging!(error, Type::Core, "TUN failed to start: {}", detail);
    Handle::notice_message("tun::start_failed", detail.to_owned());

    let detail = detail.to_owned();
    AsyncHandler::spawn(move || async move {
        // Перегенерация уберёт tun из конфига: ядро перестанет пытаться, а UI
        // получит настоящее состояние вместо обещанного.
        if let Err(e) = crate::core::CoreManager::global().update_config_checked().await {
            logging!(
                warn,
                Type::Core,
                "failed to drop TUN from the running config after {}: {}",
                detail,
                e
            );
        }
        Handle::refresh_verge();
        let _ = crate::core::tray::Tray::global().update_menu().await;
    });
}

/// Отметка «до»: последняя строка лога ядра ПЕРЕД тем, как ему отдадут новый
/// конфиг. Всё, что было написано раньше, к этой попытке отношения не имеет.
///
/// Снимать её надо ДО перегенерации конфига: в режиме службы ядро успевает
/// пожаловаться прямо внутри вызова, применяющего конфиг, и отметка, снятая
/// после, накрыла бы собой ровно ту строку, ради которой всё и затевалось.
///
/// Отметка — сама строка, а не её номер: буфер логов кольцевой и переполняется
/// (в sidecar это последняя сотня строк), так что длина «до» ничего не значит
/// уже через несколько секунд обычной работы.
pub async fn log_anchor() -> Option<String> {
    crate::core::CoreManager::global()
        .get_clash_logs()
        .await
        .ok()
        .and_then(|logs| logs.last().map(ToString::to_string))
}

/// После включения TUN дать ядру время и проверить, не ругнулось ли оно, а
/// потом сторожить факт, пока заявка держится.
///
/// `anchor` — отметка из [`log_anchor`], снятая до подачи конфига. `None`
/// означает «смотреть весь буфер»: так зовут после перезапуска ядра, где логи
/// и так почищены.
pub fn spawn_start_verification(anchor: Option<String>) {
    // Publish the anchor before the sleep: a watchdog left from the previous
    // attempt must judge the new one by the new mark, not by the old buffer.
    *WATCH_ANCHOR.lock() = anchor;
    AsyncHandler::spawn(|| async {
        tokio::time::sleep(timing::TUN_VERIFY_DELAY).await;
        if !claimed().await {
            return;
        }
        if matches!(verify_round().await, Round::Watching) {
            spawn_watchdog();
        }
    });
}

/// Итог одного круга сверки.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Round {
    /// Жалоб нет (или спросить было не у кого) — сторожим дальше.
    Watching,
    /// Ядро пожаловалось: заявка снята, сторожить больше нечего.
    Done,
}

/// Один круг сверки факта: спросить у ядра его вывод и разобрать всё, что
/// появилось после отметки.
///
/// Тот же способ, что и у разовой проверки, — второго механизма нет: в
/// service-режиме логи отдаёт служба, в sidecar — наш кольцевой буфер.
async fn verify_round() -> Round {
    let anchor = WATCH_ANCHOR.lock().clone();
    let Ok(logs) = crate::core::CoreManager::global().get_clash_logs().await else {
        // Nobody answered (service down, core not up yet): keep the old mark,
        // otherwise the next round would take the whole buffer for fresh output.
        return Round::Watching;
    };
    match verdict(&logs, anchor.as_deref()) {
        Verdict::Failed(line) => {
            // Reuse the single failure path: suppression, notice, config
            // regeneration and `refresh_verge` all live there, and it is
            // idempotent — the sidecar reader may have reported the same line.
            report_start_failure(line);
            Round::Done
        }
        Verdict::Clean(next) => {
            *WATCH_ANCHOR.lock() = next;
            Round::Watching
        }
    }
}

/// Сторож факта: пока заявка держится, раз в [`timing::TUN_WATCH_INTERVAL`]
/// сверяем её с выводом ядра.
///
/// Under the service the core output is only ever read on request, so without
/// this the app would keep showing a green button over a tunnel the core gave
/// up on seconds after our single 3-second check.
fn spawn_watchdog() {
    if WATCHDOG_RUNNING.swap(true, Ordering::AcqRel) {
        return;
    }
    AsyncHandler::spawn(|| async {
        scopeguard::defer! {
            WATCHDOG_RUNNING.store(false, Ordering::Release);
        }
        loop {
            tokio::time::sleep(timing::TUN_WATCH_INTERVAL).await;
            // Stop as soon as the claim is gone. A reported failure suppresses
            // the claim, so the watchdog cannot re-arm itself off its own
            // report; a later re-enable goes through `spawn_start_verification`
            // and starts it again with a fresh anchor.
            if !claimed().await || matches!(verify_round().await, Round::Done) {
                return;
            }
        }
    });
}

/// Что новый вывод ядра говорит о туннеле.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Verdict<'a> {
    /// Свежая жалоба на TUN.
    Failed(&'a str),
    /// Жалоб нет; отметка «до» для следующего круга.
    Clean(Option<String>),
}

/// Правило сверки — отдельно от опроса ядра, чтобы его можно было проверить.
///
/// The clean verdict carries the last line seen, so the next round only judges
/// what appeared after it; an empty buffer keeps the previous anchor, or a
/// transient empty answer from the service would re-open the whole buffer and
/// let an already handled complaint count twice.
fn verdict<'a, S: AsRef<str>>(logs: &'a [S], anchor: Option<&str>) -> Verdict<'a> {
    if let Some(line) = fresh_failure(logs, anchor) {
        return Verdict::Failed(line.as_ref());
    }
    Verdict::Clean(
        logs.last()
            .map(|line| line.as_ref().to_owned())
            .or_else(|| anchor.map(ToOwned::to_owned)),
    )
}

/// Самая свежая жалоба ядра на TUN среди строк, появившихся ПОСЛЕ отметки.
///
/// Отметки нет, она уже вытеснена из кольцевого буфера или буфер почистили
/// (перезапуск ядра чистит его целиком) — смотрим весь буфер: старых строк в
/// нём в этих случаях всё равно нет.
fn fresh_failure<'a, S: AsRef<str>>(logs: &'a [S], anchor: Option<&str>) -> Option<&'a S> {
    let from = anchor
        .and_then(|anchor| logs.iter().rposition(|line| line.as_ref() == anchor))
        .map_or(0, |position| position + 1);
    logs[from..]
        .iter()
        .rev()
        .take(200)
        .find(|line| line_reports_tun_failure(line.as_ref()))
}

/// Попытку автоматической настройки запоминаем вместе с версией приложения:
/// после обновления имеет смысл попробовать ещё раз, до него — нет.
///
/// Пишется после ЛЮБОЙ попытки, потребовавшей прав, — и удачной тоже. Слову
/// установщика тут верить нельзя: на Windows он возвращает 0, даже когда служба
/// уже была зарегистрирована и отвечать так и не начала. Отметку снимает только
/// следующий запуск, увидевший живую службу (`ensure_ready`), — то есть служба
/// должна пережить перезапуск, чтобы получить право на новую попытку. Явные
/// действия пользователя (тумблер TUN, кнопка ремонта) снимают её сразу: он
/// попросил — значит пробуем.
async fn record_setup_attempt() {
    let version = env!("CARGO_PKG_VERSION");
    let verge = Config::verge().await;
    verge.edit_draft(|d| {
        d.tun_setup_declined = Some(version.into());
    });
    verge.apply();
    let data = Config::verge().await.latest_arc();
    if let Err(e) = data.save_file().await {
        logging!(warn, Type::Core, "failed to persist the TUN setup attempt: {}", e);
    }
    Handle::refresh_verge();
}

/// Автоматическую настройку на этой версии уже пробовали.
pub async fn setup_declined_for_this_version() -> bool {
    Config::verge()
        .await
        .latest_arc()
        .tun_setup_declined
        .as_deref()
        .is_some_and(|declined_at| declined_at == env!("CARGO_PKG_VERSION"))
}

/// Забыть прошлую попытку: либо служба доказала, что жива, либо пользователь
/// сам попросил TUN — в обоих случаях пробовать снова можно.
pub async fn clear_setup_declined() {
    if Config::verge().await.latest_arc().tun_setup_declined.is_none() {
        return;
    }
    let verge = Config::verge().await;
    verge.edit_draft(|d| {
        d.tun_setup_declined = None;
    });
    verge.apply();
    let data = Config::verge().await.latest_arc();
    if let Err(e) = data.save_file().await {
        logging!(warn, Type::Core, "failed to clear the TUN setup decline: {}", e);
    }
    Handle::refresh_verge();
}

/// Служба отвечает на автоматическом проходе — значит прошлая попытка своё дело
/// сделала и пережила перезапуск приложения. Отметку можно снять: если служба
/// когда-нибудь отвалится на этой же версии, помочь можно будет ещё раз.
///
/// Только на автоматическом: `ensure_ready(true)` зовут тумблер TUN и трей, в
/// том числе через минуту после установки. Снимать отметку там значило бы
/// поверить установщику на слово в том же запуске — и машина, где служба не
/// переживает перезагрузку, снова получала бы запрос прав на каждом старте.
async fn proven_alive_at_startup(user_initiated: bool) {
    if !user_initiated {
        clear_setup_declined().await;
    }
}

/// Результат попытки подготовить TUN.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupOutcome {
    /// Всё уже готово: приложение привилегировано или служба отвечает.
    AlreadyReady,
    /// Служба только что установлена.
    Installed,
    /// На этой версии автоматическую настройку уже пробовали — молча ничего
    /// не делаем.
    Declined,
    /// Установка не удалась (в том числе отказ в UAC).
    Failed,
    /// The system authorisation dialog is still on screen: we stopped waiting
    /// for it, but the installer was not cancelled and may still succeed.
    ///
    /// Deliberately distinct from [`Self::Failed`]: an unanswered dialog is not
    /// a machine where TUN is impossible, and telling the user "TUN is
    /// unavailable" while his own UAC prompt is waiting for a click would be a
    /// lie.
    Pending,
}

/// Довести TUN до рабочего состояния. Вызывается при старте (автоматически) и
/// из UI, когда пользователь включает TUN на машине без службы.
///
/// `user_initiated` = пользователь сам попросил: тогда прошлая попытка не в счёт.
pub async fn ensure_ready(user_initiated: bool) -> SetupOutcome {
    if already_ready(user_initiated).await {
        return SetupOutcome::AlreadyReady;
    }

    if user_initiated {
        clear_setup_declined().await;
    } else if setup_declined_for_this_version().await {
        logging!(
            info,
            Type::Service,
            "service setup was already attempted on this version; not asking again"
        );
        return SetupOutcome::Declined;
    }

    if SETUP_RUNNING.swap(true, Ordering::AcqRel) {
        return SetupOutcome::Declined;
    }
    scopeguard::defer! {
        SETUP_RUNNING.store(false, Ordering::Release);
    }

    set_up_service().await
}

/// TUN уже можно поднимать — прав ни у кого просить не надо.
///
/// Служба могла просто не успеть подняться: она стартует вместе с системой и при
/// автозапуске регулярно отстаёт от приложения. Ждём её, прежде чем просить
/// прав, — но только если она вообще зарегистрирована.
async fn already_ready(user_initiated: bool) -> bool {
    if is_capable().await {
        clear_suppression();
        proven_alive_at_startup(user_initiated).await;
        return true;
    }

    if wait_until_capable(true).await {
        clear_suppression();
        proven_alive_at_startup(user_initiated).await;
        // Ядро уже могло подняться как sidecar, пока службы не было.
        crate::core::CoreManager::global().handoff_to_service_if_needed().await;
        return true;
    }

    false
}

/// Единственное место, где приложение просит права.
async fn set_up_service() -> SetupOutcome {
    let action = required_action().await;
    logging!(
        info,
        Type::Service,
        "preparing the background service for TUN: {:?}",
        action
    );
    // Ждём, пока менеджер освободится: ожидание службы и хэндофф зовут
    // `refresh()` в фоне, и налететь на занятую операцию легко. Ожидание
    // ограничено самим менеджером (`SERVICE_STATUS_WAIT`) — иначе чужая
    // привилегированная операция держала бы нас здесь, сколько висит её диалог.
    let _ = SERVICE_MANAGER.current().await;
    Handle::notice_message("tun::setup_started", "");

    if let Err(e) = SERVICE_MANAGER.handle_service_status(action).await {
        // Занятый менеджер — не провал: записать здесь попытку значило бы
        // выключить автонастройку до конца версии, ни разу не показав
        // пользователю запрос прав.
        if e.downcast_ref::<ServiceBusy>().is_some() {
            // Busy *because* a dialog is up is a different answer: the user is
            // being asked right now, and telling him TUN is unavailable while
            // his own prompt waits for a click would be plainly wrong.
            if elevation_in_flight() {
                logging!(
                    info,
                    Type::Service,
                    "an authorisation dialog is already open; not asking a second time"
                );
                return SetupOutcome::Pending;
            }
            logging!(info, Type::Service, "the service manager is busy; leaving it be");
            return SetupOutcome::Declined;
        }
        // The authorisation dialog outlived our patience. The attempt still
        // counts — the prompt WAS shown, and re-asking on every restart is
        // exactly what `record_setup_attempt` exists to prevent — but the setup
        // is not a failure: the elevated helper is still running, and the next
        // startup that sees a live service clears the mark by itself.
        if e.downcast_ref::<ElevationPending>().is_some() {
            logging!(
                warn,
                Type::Service,
                "the authorisation dialog is still open; not waiting for it any longer"
            );
            record_setup_attempt().await;
            return SetupOutcome::Pending;
        }
        let detail = format!("{e}");
        logging!(warn, Type::Service, "background service setup failed: {}", detail);
        record_setup_attempt().await;
        Handle::notice_message("tun::setup_failed", detail);
        return SetupOutcome::Failed;
    }

    // Права спрошены — попытка засчитана в любом случае. Снимет отметку только
    // следующий запуск, увидевший живую службу (см. `already_ready`).
    record_setup_attempt().await;

    // Верим не слову установщика, а факту. На Windows установщик идемпотентен и
    // возвращает 0, даже когда служба уже была и отвечать так и не начала:
    // «успех» без работающего IPC — это провал, и именно он раньше приводил к
    // установке по кругу на каждом запуске.
    if !wait_until_capable(false).await {
        logging!(
            warn,
            Type::Service,
            "the service setup reported success, but the service still does not answer"
        );
        Handle::notice_message("tun::setup_failed", "service is silent after setup".to_owned());
        return SetupOutcome::Failed;
    }

    clear_suppression();
    logging!(info, Type::Service, "background service is up");
    Handle::notice_message("tun::setup_done", "");
    Handle::refresh_verge();
    // Ядро уже могло подняться как sidecar — переезжаем на службу без разрыва,
    // если TUN нужен.
    crate::core::CoreManager::global().handoff_to_service_if_needed().await;
    SetupOutcome::Installed
}

/// Что именно надо сделать со службой, чтобы TUN заработал.
///
/// Раньше выбор был из двух: «служба отвечает, но старая» → ремонт, иначе →
/// установка. То есть остановленная служба (и служба, которая просто молчит)
/// лечилась установкой — с запросом прав и на каждом запуске. Теперь сначала
/// спрашиваем систему, что она вообще знает о службе.
async fn required_action() -> ServiceStatus {
    action_for(service_registration(), service_needs_repair().await)
}

/// Само правило — отдельно от опроса системы, чтобы его можно было проверить.
const fn action_for(registration: ServiceRegistration, needs_repair: bool) -> ServiceStatus {
    // Служба отвечает, но старая (обычно после обновления приложения) — это
    // ремонт, а не установка с нуля, и что там думает система, уже неважно.
    if needs_repair {
        return ServiceStatus::ReinstallRequired;
    }

    match registration {
        // Установщик службы идемпотентен: зарегистрированную, но остановленную
        // он просто запускает — переустановка тут была бы лишней.
        ServiceRegistration::Missing | ServiceRegistration::Stopped => ServiceStatus::InstallRequired,
        // Система считает службу работающей, а канала нет — чинить.
        ServiceRegistration::Running => ServiceStatus::ForceReinstallRequired,
        // Спросить не вышло — ведём себя как раньше.
        ServiceRegistration::Unknown => ServiceStatus::InstallRequired,
    }
}

/// Дождаться, пока служба поднимет канал.
///
/// Служба зарегистрирована как автозапускаемая, но приложение, поднятое вместе
/// с системой, регулярно оказывается быстрее неё: IPC ещё нет, и без ожидания
/// приложение сочло бы, что службы не существует. Тем же ожиданием проверяется
/// результат установки — она заканчивается раньше, чем служба готова отвечать.
/// `trust_registration` = верить опросу системы и не ждать того, что само не
/// поднимется (службы нет, остановлена, устарела). После установки службы этому
/// опросу верить рано: она успевает отчитаться раньше, чем система пометит её
/// работающей, — там ждём всё окно целиком.
async fn wait_until_capable(trust_registration: bool) -> bool {
    // Срок считаем по часам, а не сложением пауз: каждый круг ещё и опрашивает
    // IPC (у него свои ретраи) и спрашивает систему, так что сумма пауз врёт в
    // меньшую сторону, а ждёт приложение при этом дольше обещанного.
    let deadline = Instant::now() + timing::TUN_SERVICE_APPEAR_WAIT;
    loop {
        if is_capable().await {
            return true;
        }
        let registration = service_registration();
        // Ждать имеет смысл только то, что вот-вот поднимется само. Службы нет —
        // не поднимется никогда, и это верно даже сразу после установки (пути,
        // по которым это определяется, взяты из самого установщика службы —
        // см. `service_registration`).
        let pointless = matches!(registration, ServiceRegistration::Missing)
            || (trust_registration
                && (matches!(registration, ServiceRegistration::Stopped) || service_needs_repair().await));
        if pointless || Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(timing::TUN_SERVICE_APPEAR_INTERVAL).await;
    }
}

/// Шаг старта приложения: если TUN нужен, но недоступен, один раз довести его
/// до рабочего состояния. Результат разбирать некому — всё, что важно, уже
/// ушло в уведомления и лог.
///
/// clod: раньше этот шаг выполнялся всегда — и на машине, где TUN выключен,
/// приложение при каждом запуске ставило службу заново, каждый раз спрашивая
/// права. Служба нужна ровно для TUN, поэтому:
///   * TUN выключен — службу не трогаем вовсе;
///   * TUN включён — сперва даём службе шанс подняться самой (она стартует с
///     системой и часто отстаёт от приложения);
///   * и только если её действительно нет — один раз на версию приложения
///     предлагаем поставить (см. `record_setup_attempt`).
pub async fn init_startup_setup() {
    if !desired().await {
        logging!(
            info,
            Type::Service,
            "TUN is off; leaving the background service alone at startup"
        );
        return;
    }

    let outcome = ensure_ready(false).await;
    logging!(info, Type::Service, "startup TUN readiness: {:?}", outcome);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_the_core_tun_failures() {
        assert!(line_reports_tun_failure(
            "Start TUN listening error: configure tun interface: Connect: operation not permitted"
        ));
        assert!(line_reports_tun_failure("configure tun interface: Access is denied."));
        assert!(!line_reports_tun_failure("[TCP] tun accept connection"));
        assert!(!line_reports_tun_failure("Start initial provider default"));
    }

    #[test]
    fn asks_for_the_smallest_service_action() {
        // Ничего не знаем — как раньше: установщик, он же запускает уже
        // зарегистрированную службу.
        assert_eq!(
            action_for(ServiceRegistration::Unknown, false),
            ServiceStatus::InstallRequired
        );
        assert_eq!(
            action_for(ServiceRegistration::Missing, false),
            ServiceStatus::InstallRequired
        );
        assert_eq!(
            action_for(ServiceRegistration::Stopped, false),
            ServiceStatus::InstallRequired
        );
        // Система говорит «работает», а канала нет — это уже поломка.
        assert_eq!(
            action_for(ServiceRegistration::Running, false),
            ServiceStatus::ForceReinstallRequired
        );
        // Устаревшая служба чинится независимо от того, что думает система.
        for registration in [
            ServiceRegistration::Missing,
            ServiceRegistration::Stopped,
            ServiceRegistration::Running,
            ServiceRegistration::Unknown,
        ] {
            assert_eq!(action_for(registration, true), ServiceStatus::ReinstallRequired);
        }
    }

    #[test]
    fn old_complaints_do_not_count_against_a_new_attempt() {
        const FAILURE: &str = "Start TUN listening error: configure tun interface: Access is denied.";
        let logs = [FAILURE, "[TCP] tun accept connection"];
        // Обе строки были до попытки — жаловаться не на что.
        assert!(fresh_failure(&logs, Some("[TCP] tun accept connection")).is_none());
        // Провал написан после отметки — он и есть ответ.
        assert_eq!(
            fresh_failure(&logs, Some("Start initial provider default")).copied(),
            Some(FAILURE)
        );
        assert_eq!(fresh_failure(&logs, None).copied(), Some(FAILURE));
        // Отметка вытеснена из кольцевого буфера (или буфер почистили при
        // перезапуске ядра) — смотрим весь: иначе свежий провал остался бы
        // незамеченным.
        assert_eq!(fresh_failure(&logs, Some("evicted line")).copied(), Some(FAILURE));
        // Отметка — ПОСЛЕДНЕЕ вхождение: строки повторяются, и старое
        // совпадение не должно открывать окно шире, чем было на самом деле.
        let repeated = ["[TCP] tun accept connection", FAILURE, "[TCP] tun accept connection"];
        assert!(fresh_failure(&repeated, Some("[TCP] tun accept connection")).is_none());
    }

    #[test]
    fn the_watchdog_runs_exactly_while_tun_is_claimed() {
        // Заявка = желание И НЕ подавление: ровно то, что уходит в конфиг ядра,
        // — сторожить имеет смысл только это.
        assert!(is_claimed(true, false));
        // Пользователь выключил TUN — сторожить нечего.
        assert!(!is_claimed(false, false));
        // Провал уже отработан (подавление): второй раз докладывать не о чем,
        // и это же не даёт сторожу перезапустить себя по собственному отчёту.
        assert!(!is_claimed(true, true));
        assert!(!is_claimed(false, true));
    }

    #[test]
    fn each_round_moves_the_anchor_to_the_last_seen_line() {
        const FAILURE: &str = "Start TUN listening error: configure tun interface: Access is denied.";
        let logs = ["[TCP] tun accept connection", "Start initial provider default"];
        // Жалоб нет — следующий круг смотрит только то, что появится после
        // последней уже прочитанной строки.
        assert_eq!(
            verdict(&logs, Some("[TCP] tun accept connection")),
            Verdict::Clean(Some("Start initial provider default".to_owned()))
        );
        // Буфер пуст (служба ответила пустотой, ядро ещё не поднялось) —
        // отметку держим прежнюю, иначе следующий круг счёл бы весь буфер
        // свежим и повторно доложил бы о разобранной жалобе.
        let empty: [&str; 0] = [];
        assert_eq!(
            verdict(&empty, Some("anchor")),
            Verdict::Clean(Some("anchor".to_owned()))
        );
        assert_eq!(verdict(&empty, None), Verdict::Clean(None));
        // Жалоба после отметки — это ответ, а не новая отметка.
        let broken = ["[TCP] tun accept connection", FAILURE];
        assert_eq!(
            verdict(&broken, Some("[TCP] tun accept connection")),
            Verdict::Failed(FAILURE)
        );
        // Та же жалоба, но она уже была разобрана предыдущим кругом (отметка
        // стоит после неё) — сторож молчит.
        assert_eq!(
            verdict(&broken, Some(FAILURE)),
            Verdict::Clean(Some(FAILURE.to_owned()))
        );
    }

    #[test]
    fn suppression_is_a_session_flag() {
        clear_suppression();
        assert!(!is_suppressed());
        suppress("test");
        assert!(is_suppressed());
        clear_suppression();
        assert!(!is_suppressed());
    }
}
