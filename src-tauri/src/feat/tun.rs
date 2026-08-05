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
//!   * **факт** — подтверждение от ядра, что интерфейс поднялся.
//!
//! Подавление живёт только в памяти процесса: перезапуск приложения (или
//! появление службы) снимает его само собой, а файл конфигурации не трогается.

use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};

use clash_verge_logging::{Type, logging};
use tauri_plugin_clash_verge_sysinfo::is_current_app_handle_admin;

use crate::{
    config::Config,
    constants::timing,
    core::{
        handle::Handle,
        service::{
            SERVICE_MANAGER, ServiceBusy, ServiceRegistration, ServiceStatus, is_service_available,
            service_registration,
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

/// После включения TUN дать ядру время и проверить, не ругнулось ли оно.
pub fn spawn_start_verification() {
    AsyncHandler::spawn(|| async {
        tokio::time::sleep(timing::TUN_VERIFY_DELAY).await;
        if !desired().await || is_suppressed() {
            return;
        }
        // Берём логи через менеджер: в service-режиме их отдаёт служба, в
        // sidecar — наш кольцевой буфер.
        let Ok(logs) = crate::core::CoreManager::global().get_clash_logs().await else {
            return;
        };
        if let Some(line) = logs
            .iter()
            .rev()
            .take(200)
            .find(|line| line_reports_tun_failure(line.as_str()))
        {
            report_start_failure(line);
        }
    });
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
    // `refresh()` в фоне, и налететь на занятую операцию легко.
    let _ = SERVICE_MANAGER.current().await;
    Handle::notice_message("tun::setup_started", "");

    if let Err(e) = SERVICE_MANAGER.handle_service_status(action).await {
        // Занятый менеджер — не провал: записать здесь попытку значило бы
        // выключить автонастройку до конца версии, ни разу не показав
        // пользователю запрос прав.
        if e.downcast_ref::<ServiceBusy>().is_some() {
            logging!(info, Type::Service, "the service manager is busy; leaving it be");
            return SetupOutcome::Declined;
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
    fn suppression_is_a_session_flag() {
        clear_suppression();
        assert!(!is_suppressed());
        suppress("test");
        assert!(is_suppressed());
        clear_suppression();
        assert!(!is_suppressed());
    }
}
