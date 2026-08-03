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

use std::sync::atomic::{AtomicBool, Ordering};

use clash_verge_logging::{Type, logging};
use tauri_plugin_clash_verge_sysinfo::is_current_app_handle_admin;

use crate::{
    config::Config,
    constants::timing,
    core::{
        handle::Handle,
        service::{SERVICE_MANAGER, ServiceStatus, is_service_available},
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
pub async fn is_capable() -> bool {
    is_app_elevated() || is_service_available().await.is_ok()
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

/// Отказ пользователя запоминаем вместе с версией приложения: после
/// обновления имеет смысл спросить ещё раз, до него — нет.
async fn record_setup_declined() {
    let version = env!("CARGO_PKG_VERSION");
    let verge = Config::verge().await;
    verge.edit_draft(|d| {
        d.tun_setup_declined = Some(version.into());
    });
    verge.apply();
    let data = Config::verge().await.latest_arc();
    if let Err(e) = data.save_file().await {
        logging!(warn, Type::Core, "failed to persist the TUN setup decline: {}", e);
    }
    Handle::refresh_verge();
}

pub async fn setup_declined_for_this_version() -> bool {
    Config::verge()
        .await
        .latest_arc()
        .tun_setup_declined
        .as_deref()
        .is_some_and(|declined_at| declined_at == env!("CARGO_PKG_VERSION"))
}

/// Забыть отказ: пользователь сам попросил TUN, значит спрашивать снова можно.
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
}

/// Результат попытки подготовить TUN.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupOutcome {
    /// Всё уже готово: приложение привилегировано или служба отвечает.
    AlreadyReady,
    /// Служба только что установлена.
    Installed,
    /// Пользователь ранее отказал — молча ничего не делаем.
    Declined,
    /// Установка не удалась (в том числе отказ в UAC).
    Failed,
}

/// Довести TUN до рабочего состояния. Вызывается при старте (автоматически) и
/// из UI, когда пользователь включает TUN на машине без службы.
///
/// `user_initiated` = пользователь сам попросил: тогда прошлый отказ не в счёт.
pub async fn ensure_ready(user_initiated: bool) -> SetupOutcome {
    if is_capable().await {
        clear_suppression();
        return SetupOutcome::AlreadyReady;
    }

    if user_initiated {
        clear_setup_declined().await;
    } else if setup_declined_for_this_version().await {
        logging!(
            info,
            Type::Service,
            "service setup was declined for this version; not asking again"
        );
        return SetupOutcome::Declined;
    }

    if SETUP_RUNNING.swap(true, Ordering::AcqRel) {
        return SetupOutcome::Declined;
    }
    scopeguard::defer! {
        SETUP_RUNNING.store(false, Ordering::Release);
    }

    logging!(info, Type::Service, "installing the background service for TUN");
    Handle::notice_message("tun::setup_started", "");

    match SERVICE_MANAGER
        .handle_service_status(ServiceStatus::InstallRequired)
        .await
    {
        Ok(()) => {
            clear_suppression();
            logging!(info, Type::Service, "background service installed");
            Handle::notice_message("tun::setup_done", "");
            Handle::refresh_verge();
            // Ядро уже могло подняться как sidecar — переезжаем на службу без
            // разрыва, если TUN нужен.
            crate::core::CoreManager::global().handoff_to_service_if_needed().await;
            SetupOutcome::Installed
        }
        Err(e) => {
            let detail = format!("{e}");
            logging!(warn, Type::Service, "background service setup failed: {}", detail);
            record_setup_declined().await;
            Handle::notice_message("tun::setup_failed", detail);
            SetupOutcome::Failed
        }
    }
}

/// Шаг старта приложения: если TUN недоступен, один раз довести его до
/// рабочего состояния. Результат разбирать некому — всё, что важно, уже ушло
/// в уведомления и лог.
pub async fn init_startup_setup() {
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
    fn suppression_is_a_session_flag() {
        clear_suppression();
        assert!(!is_suppressed());
        suppress("test");
        assert!(is_suppressed());
        clear_suppression();
        assert!(!is_suppressed());
    }
}
