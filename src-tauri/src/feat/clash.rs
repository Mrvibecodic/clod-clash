use crate::{
    config::Config,
    core::{CoreManager, handle, tray},
    feat::clean_async,
    process::AsyncHandler,
    utils,
};
use clash_verge_logging::{Type, logging};
use serde_yaml_ng::{Mapping, Value};
use smartstring::alias::String;

pub async fn restart_clash_core() {
    crate::feat::tun::clear_suppression();
    match CoreManager::global().restart_core().await {
        Ok(_) => {
            handle::Handle::refresh_clash();
            if let Err(err) = crate::config::profiles::activate_selected_nodes() {
                logging!(
                    warn,
                    Type::Core,
                    "Warning: restore selection after core restart failed: {err}"
                );
            }
            handle::Handle::notice_message("set_config::ok", "ok");
        }
        Err(err) => {
            handle::Handle::notice_message("set_config::error", format!("{err}"));
            logging!(error, Type::Core, "{err}");
        }
    }
}

pub async fn restart_app() {
    // Уборка выхода больше не держит главный поток, и окно с треем живы все её
    // секунды. Перезапуск посреди выхода гасил бы встроенный сервер и пускал
    // вторую уборку наперегонки с первой.
    if handle::Handle::global().is_exiting() {
        logging!(info, Type::System, "перезапуск приложения пропущен: выход уже идёт");
        return;
    }
    logging!(debug, Type::System, "Запуск процесса перезапуска приложения");
    handle::Handle::global().set_is_exiting();

    utils::server::shutdown_embedded_server();

    // Настройки сохраняет сама уборка, наравне с остальными шагами.
    logging!(info, Type::System, "Начало асинхронной очистки ресурсов");
    let cleanup_result = clean_async().await;

    logging!(
        info,
        Type::System,
        "Очистка ресурсов завершена, код выхода: {}",
        if cleanup_result { 0 } else { 1 }
    );

    let app_handle = handle::Handle::app_handle();
    app_handle.restart();
}

/// clod:Э11-10 — один запрос вместо сотен.
///
/// Раньше здесь забирался весь список соединений и каждое закрывалось отдельным
/// запросом: на нагруженной машине это сотни последовательных обращений к ядру,
/// притом что `DELETE /connections` закрывает всё разом. Делать это на бэкенде
/// по-прежнему нужно — режим меняют и трей, и горячие клавиши, мимо интерфейса.
fn close_connections_after_mode_change() {
    AsyncHandler::spawn(|| async {
        if let Err(err) = handle::Handle::mihomo().close_all_connections().await {
            logging!(warn, Type::Core, "Warning: не удалось разорвать соединения: {err}");
        }
    });
}

async fn mode_owner() -> Option<(String, bool)> {
    let profiles = Config::profiles().await.latest_arc();
    let uid = profiles.get_current()?.clone();
    let locked = profiles.get_item(&uid).ok()?.lock_mode.unwrap_or(false);
    Some((uid, locked))
}

pub async fn change_clash_mode(mode: String) -> Result<(), String> {
    let owner = mode_owner().await;
    if owner.as_ref().is_some_and(|(_, locked)| *locked) {
        logging!(
            info,
            Type::Core,
            "mode change refused: locked by the panel (clod-lock-mode)"
        );
        return Err(clash_verge_i18n::t!("common.modeLocked").into_owned().into());
    }
    if crate::cmd::profile_switch_in_progress() {
        logging!(info, Type::Core, "mode change refused: the subscription is switching");
        return Err(clash_verge_i18n::t!("common.modeSwitching").into_owned().into());
    }
    let previous = match &owner {
        Some((uid, _)) => match crate::config::profiles::profiles_set_mode_choice_safe(uid, Some(mode.clone())).await {
            Ok(previous) => Some((uid, previous)),
            Err(err) => {
                logging!(warn, Type::Core, "Warning: mode choice not saved to the profile: {err}");
                None
            }
        },
        None => {
            logging!(info, Type::Core, "mode choice not remembered: no current profile");
            None
        }
    };
    let mut mapping = Mapping::new();
    mapping.insert(Value::from("mode"), Value::from(mode.as_str()));
    let json_value = serde_json::json!({
        "mode": mode
    });
    logging!(debug, Type::Core, "change clash mode to {mode}");
    if let Err(err) = handle::Handle::mihomo().patch_base_config(&json_value).await {
        logging!(error, Type::Core, "{err}");
        if let Some((uid, previous)) = previous
            && let Err(restore) = crate::config::profiles::profiles_set_mode_choice_safe(uid, previous).await
        {
            logging!(warn, Type::Core, "Warning: mode choice not restored: {restore}");
        }
        return Err(err.to_string().into());
    }

    // clod:Э3-06 — под замком правки Clash: иначе `apply()` зафиксировал бы
    // чужую правку, которая ещё ждёт проверки ядром.
    let _serialized = crate::feat::patch_clash_lock().lock().await;
    let runtime = Config::runtime().await;
    runtime.edit_draft(|d| d.patch_config(&mapping));
    runtime.apply();
    if let Err(err) = Config::generate_file(crate::config::ConfigType::Run).await {
        logging!(
            warn,
            Type::Core,
            "Warning: failed to refresh runtime config file after mode change: {err}"
        );
    }

    handle::Handle::refresh_clash();
    tray::Tray::global().update_menu_and_icon().await;

    if Config::verge().await.data_arc().auto_close_connection() {
        close_connections_after_mode_change();
    }

    Ok(())
}

/// Что ядро по нашему запросу качает из сети.
#[derive(Debug, Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CoreDownload {
    Rules,
    Proxies,
    Geo,
}

/// Попросить ядро обновить провайдера, набор правил или гео-базы и дождаться
/// его ответа столько, сколько ядро само ждёт загрузку.
///
/// clod:Э13-04 — запрос строится тем же клиентом плагина (тот же канал к
/// ядру), но со своим пределом: у быстрых запросов он остаётся прежним.
pub async fn download_in_core(what: CoreDownload, name: &str) -> anyhow::Result<()> {
    use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
    use reqwest::Method;

    let name_in_path = utf8_percent_encode(name, NON_ALPHANUMERIC).to_string();
    let (method, path, budget) = match what {
        CoreDownload::Rules => (
            Method::PUT,
            format!("/providers/rules/{name_in_path}"),
            crate::constants::timing::CORE_PROVIDER_DOWNLOAD,
        ),
        CoreDownload::Proxies => (
            Method::PUT,
            format!("/providers/proxies/{name_in_path}"),
            crate::constants::timing::CORE_PROVIDER_DOWNLOAD,
        ),
        CoreDownload::Geo => (
            Method::POST,
            "/configs/geo".to_owned(),
            crate::constants::timing::CORE_GEO_DOWNLOAD,
        ),
    };
    let request = handle::Handle::mihomo()
        .load_ctx()
        .build_request(method, &path)
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    let response = request.timeout(budget).send().await?;
    if response.status().is_success() {
        return Ok(());
    }
    let status = response.status();
    let message = response
        .json::<serde_json::Value>()
        .await
        .ok()
        .and_then(|body| {
            body.get("message")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .unwrap_or_else(|| status.to_string());
    anyhow::bail!("{message}")
}
