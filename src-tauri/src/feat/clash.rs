use crate::{
    config::Config,
    core::{CoreManager, handle, tray},
    feat::clean_async,
    process::AsyncHandler,
    utils,
};
use clash_verge_logging::{Type, logging, logging_error};
use serde_yaml_ng::{Mapping, Value};
use smartstring::alias::String;

/// Перезапуск ядра по просьбе человека — из трея и из окна. Выбор узлов
/// возвращает сам запуск ядра.
pub async fn restart_clash_core() -> anyhow::Result<()> {
    crate::feat::refuse_while_exiting()?;
    logging_error!(Type::Core, crate::config::profiles::profiles_save_file_safe().await);
    crate::feat::tun::clear_suppression();
    CoreManager::global().restart_core().await?;
    handle::Handle::refresh_clash();
    Ok(())
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

static MODE_CHANGE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
const CORE_MODE_READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);

async fn runtime_mode_is(mode: &str) -> bool {
    Config::runtime()
        .await
        .data_arc()
        .config
        .as_ref()
        .and_then(|config| config.get("mode"))
        .and_then(Value::as_str)
        == Some(mode)
}

async fn core_mode_is(mode: &str) -> bool {
    let config = tokio::time::timeout(CORE_MODE_READ_TIMEOUT, handle::Handle::mihomo().get_base_config()).await;
    matches!(config, Ok(Ok(config)) if config.mode.to_string() == mode)
}

fn refuse_while_the_subscription_switches() -> Result<(), String> {
    if crate::cmd::profile_switch_in_progress() {
        logging!(info, Type::Core, "mode change refused: the subscription is switching");
        return Err(clash_verge_i18n::t!("common.modeSwitching").into_owned().into());
    }
    Ok(())
}

fn refuse_a_locked_mode(owner: Option<&(String, bool)>) -> Result<(), String> {
    if owner.is_some_and(|(_, locked)| *locked) {
        logging!(
            info,
            Type::Core,
            "mode change refused: locked by the panel (clod-lock-mode)"
        );
        return Err(clash_verge_i18n::t!("common.modeLocked").into_owned().into());
    }
    Ok(())
}

pub async fn change_clash_mode(mode: String) -> Result<(), String> {
    let _serialized = MODE_CHANGE_LOCK.lock().await;
    // Идущее переключение подписки — отказ сразу, а не после ожидания очереди:
    // его признак снимается уже после того, как оно освободило очередь.
    refuse_while_the_subscription_switches()?;
    // В очереди применения конфига: пока чужая сборка едет к ядру, режим не
    // переключаем (она уехала бы со старым и вернула его), а пока идёт PATCH —
    // не стартует чужая сборка. Владелец режима, замок панели и «режим уже
    // такой» читаются уже в своей очереди: пока ждали, подписку могли сменить
    // или обновить с новым замком.
    let Some(turn) = CoreManager::global().claim_for_an_update().await else {
        return Err(clash_verge_i18n::t!("common.exitInProgress").into_owned().into());
    };
    let owner = mode_owner().await;
    refuse_a_locked_mode(owner.as_ref())?;
    if runtime_mode_is(&mode).await && core_mode_is(&mode).await {
        // Режим уже такой, но нажатие — всё равно выбор человека: без записи
        // следующая смена `mode` в подписке перебила бы его. Записывается ещё в
        // своей очереди, чтобы следующая сборка его уже видела.
        remember_mode_choice(owner.as_ref(), &mode).await;
        drop(turn);
        logging_error!(Type::Tray, tray::Tray::global().update_menu().await);
        return Ok(());
    }
    switch_clash_mode(mode, owner).await
}

async fn remember_mode_choice<'a>(
    owner: Option<&'a (String, bool)>,
    mode: &str,
) -> Option<(&'a String, Option<String>)> {
    match owner {
        Some((uid, _)) => match crate::config::profiles::profiles_set_mode_choice_safe(uid, Some(mode.into())).await {
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
    }
}

/// Вызывающий держит очередь применения конфига.
async fn switch_clash_mode(mode: String, owner: Option<(String, bool)>) -> Result<(), String> {
    let previous = remember_mode_choice(owner.as_ref(), &mode).await;
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

    // Ядро режим приняло — правим принятый слот и рабочий файл под ним.
    let runtime = Config::runtime().await;
    let mut accepted = (**runtime.data_arc()).clone();
    accepted.patch_config(&mapping);
    runtime.replace(accepted);
    if let Err(err) = Config::write_accepted_runtime_file().await {
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
    core_send(method, &path, budget).await.map(drop)
}

/// Запрос к ядру мимо готовых методов плагина — тем же клиентом и каналом, но
/// со своим пределом ожидания. Отказ ядра — его же словами.
pub async fn core_send(
    method: reqwest::Method,
    path: &str,
    budget: std::time::Duration,
) -> anyhow::Result<reqwest::Response> {
    let response = handle::Handle::mihomo()
        .load_ctx()
        .build_request(method, path)
        .map_err(|error| anyhow::anyhow!("{error}"))?
        .timeout(budget)
        .send()
        .await?;
    if response.status().is_success() {
        return Ok(response);
    }
    anyhow::bail!("{}", core_error_message(response).await)
}

/// `/proxies` и провайдеры узлов принятой сборки — каждый своим запросом, все
/// разом. Общий `/providers/proxies` не годится: ядро заводит провайдер ещё и на
/// каждую группу, и узел в нём повторяется по разу на группу. Как понимать сбой
/// провайдера — решает тот, кто читает.
pub struct CoreProxies {
    pub proxies: anyhow::Result<Vec<u8>>,
    /// В порядке `proxy-providers` принятой сборки.
    pub providers: Vec<(String, anyhow::Result<Vec<u8>>)>,
}

pub async fn read_core_proxies(budget: std::time::Duration) -> CoreProxies {
    use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};

    let read = |path: std::string::String| async move {
        anyhow::Ok(Vec::from(
            core_send(reqwest::Method::GET, &path, budget).await?.bytes().await?,
        ))
    };
    let names = crate::cmd::runtime::runtime_proxy_provider_names().await;
    let (proxies, providers) = tokio::join!(
        read("/proxies".into()),
        futures::future::join_all(
            names
                .iter()
                .map(|name| format!("/providers/proxies/{}", utf8_percent_encode(name, NON_ALPHANUMERIC)))
                .map(read)
        )
    );
    CoreProxies {
        proxies,
        providers: names.into_iter().zip(providers).collect(),
    }
}

impl CoreProxies {
    /// Прочитанное — в форме общих ответов ядра, как их разбирают отчёт и
    /// проверка узлов: `/proxies` и `{"providers": {имя: ответ}}`, и сколько
    /// провайдеров не прочиталось или не разобралось (их в ответе нет). `None` —
    /// не прочитался `/proxies`.
    pub fn into_json(self) -> Option<(serde_json::Value, serde_json::Value, usize)> {
        let proxies = serde_json::from_slice(&self.proxies.ok()?).ok()?;
        let mut failed = 0;
        let mut providers = serde_json::Map::with_capacity(self.providers.len());
        for (name, body) in self.providers {
            match body.ok().and_then(|body| serde_json::from_slice(&body).ok()) {
                Some(provider) => {
                    providers.insert(name.into(), provider);
                }
                None => failed += 1,
            }
        }
        Some((proxies, serde_json::json!({ "providers": providers }), failed))
    }
}

/// Чем ядро объяснило отказ: его `message`, а без него — код ответа.
pub async fn core_error_message(response: reqwest::Response) -> std::string::String {
    let status = response.status();
    response
        .json::<serde_json::Value>()
        .await
        .ok()
        .and_then(|body| {
            body.get("message")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .unwrap_or_else(|| status.to_string())
}

#[cfg(test)]
mod tests {
    use super::CoreProxies;

    fn read(proxies: anyhow::Result<&str>, providers: &[(&str, anyhow::Result<&str>)]) -> CoreProxies {
        CoreProxies {
            proxies: proxies.map(|body| body.as_bytes().to_vec()),
            providers: providers
                .iter()
                .map(|(name, body)| {
                    let body = match body {
                        Ok(body) => Ok(body.as_bytes().to_vec()),
                        Err(err) => Err(anyhow::anyhow!("{err}")),
                    };
                    ((*name).into(), body)
                })
                .collect(),
        }
    }

    #[test]
    fn the_read_takes_the_shape_of_the_cores_listing_and_counts_failed_providers() {
        let (proxies, providers, failed) = read(
            Ok(r#"{"proxies":{"a":{}}}"#),
            &[
                ("sub", Ok(r#"{"proxies":[{"name":"n"}]}"#)),
                ("down", Err(anyhow::anyhow!("404"))),
                ("broken", Ok("{")),
            ],
        )
        .into_json()
        .unwrap_or_default();
        assert_eq!(proxies["proxies"]["a"], serde_json::json!({}));
        assert_eq!(providers["providers"]["sub"]["proxies"][0]["name"], "n");
        assert_eq!(providers["providers"].as_object().map(serde_json::Map::len), Some(1));
        assert_eq!(failed, 2);

        let (_, providers, failed) = read(Ok("{}"), &[]).into_json().unwrap_or_default();
        assert_eq!(providers, serde_json::json!({ "providers": {} }));
        assert_eq!(failed, 0);
        assert!(read(Err(anyhow::anyhow!("down")), &[]).into_json().is_none());
    }
}
