use super::CmdResult;
use crate::feat;
use crate::utils::{dirs, yaml_emitter};
use crate::{
    cmd::StringifyErr as _,
    config::{ClashInfo, Config},
    constants,
    core::{
        CoreManager, handle,
        validate::{CoreConfigValidator, ValidationOutcome},
    },
};
use clash_verge_logging::{Type, logging, logging_error};
use compact_str::CompactString;
use serde_yaml_ng::Mapping;
use smartstring::alias::String;
use tokio::fs;

/// Скопировать переменные окружения Clash
#[tauri::command]
pub async fn copy_clash_env() -> CmdResult {
    feat::copy_clash_env().await;
    Ok(())
}

/// Получить информацию Clash
#[tauri::command]
pub async fn get_clash_info() -> CmdResult<ClashInfo> {
    Ok(Config::clash().await.data_arc().get_client_info())
}

/// Изменить конфиг Clash
#[tauri::command]
pub async fn patch_clash_config(payload: Mapping) -> CmdResult {
    feat::patch_clash(&payload).await.stringify_err()
}

/// Изменить режим Clash
///
/// Пробрасывает ошибку `change_clash_mode` на фронтенд, чтобы `catch` на фронтенде мог
/// реально узнать о неудаче PATCH на бэкенде и уведомить пользователя (раньше команда
/// всегда возвращала `Ok(())`, скрывая ошибку бэкенда).
#[tauri::command]
pub async fn patch_clash_mode(payload: String) -> CmdResult {
    feat::change_clash_mode(payload).await
}

/// Получить текущий режим Clash (отказоустойчивое чтение)
///
/// Читает `mode` напрямую из сохранённого конфига clash, минуя строгую
/// десериализацию `BaseConfig` из mihomo `/configs`, как резервный источник
/// для отображения mode на главной странице.
#[tauri::command]
pub async fn get_clash_mode() -> CmdResult<Option<String>> {
    Ok(Config::clash().await.data_arc().get_mode().map(Into::into))
}

/// Переключить ядро Clash
#[tauri::command]
pub async fn change_clash_core(clash_core: String) -> CmdResult<Option<String>> {
    logging!(info, Type::Config, "changing core to {clash_core}");

    match CoreManager::global().change_core(&clash_core).await {
        Ok(_) => {
            logging_error!(Type::Core, Config::profiles().await.data_arc().save_file().await);

            // Перезапуск ядра после переключения
            match CoreManager::global().restart_core().await {
                Ok(_) => {
                    logging!(info, Type::Core, "core changed and restarted to {clash_core}");
                    handle::Handle::notice_message("config_core::change_success", clash_core);
                    handle::Handle::refresh_clash();
                    Ok(None)
                }
                Err(err) => {
                    let error_msg: String = format!("Core changed but failed to restart: {err}").into();
                    handle::Handle::notice_message("config_core::change_error", error_msg.clone());
                    logging!(error, Type::Core, "{error_msg}");
                    Ok(Some(error_msg))
                }
            }
        }
        Err(err) => {
            let error_msg: String = err;
            logging!(error, Type::Core, "failed to change core: {error_msg}");
            handle::Handle::notice_message("config_core::change_error", error_msg.clone());
            Ok(Some(error_msg))
        }
    }
}

/// Запустить ядро
#[tauri::command]
pub async fn start_core() -> CmdResult {
    let result = CoreManager::global().start_core().await.stringify_err();
    if result.is_ok() {
        handle::Handle::refresh_clash();
    }
    result
}

/// Остановить ядро
#[tauri::command]
pub async fn stop_core() -> CmdResult {
    logging_error!(Type::Core, Config::profiles().await.data_arc().save_file().await);
    let result = CoreManager::global().stop_core().await.stringify_err();
    if result.is_ok() {
        handle::Handle::refresh_clash();
    }
    result
}

/// Перезапустить ядро
#[tauri::command]
pub async fn restart_core() -> CmdResult {
    logging_error!(Type::Core, Config::profiles().await.data_arc().save_file().await);
    let result = CoreManager::global().restart_core().await.stringify_err();
    if result.is_ok() {
        handle::Handle::refresh_clash();
    }
    result
}

/// Проверить задержку URL
#[tauri::command]
pub async fn test_delay(url: String) -> CmdResult<u32> {
    let result = match feat::test_delay(url).await {
        Ok(delay) => delay,
        Err(e) => {
            logging!(error, Type::Cmd, "{}", e);
            10000u32
        }
    };
    Ok(result)
}

/// Сохранить DNS-конфиг в отдельный файл
#[tauri::command]
pub async fn save_dns_config(dns_config: Mapping) -> CmdResult {
    use crate::utils::dirs;

    // Получаем путь к файлу DNS-конфига
    let dns_path = dirs::app_home_dir().stringify_err()?.join(constants::files::DNS_CONFIG);

    // Сохраняем DNS-конфиг в файл
    let yaml_str = yaml_emitter::to_mihomo_config_string(&dns_config).stringify_err()?;
    crate::utils::help::write_atomic(&dns_path, yaml_str.as_bytes())
        .await
        .stringify_err()?;
    logging!(info, Type::Config, "DNS config saved to {dns_path:?}");

    Ok(())
}

/// Применить или отменить DNS-конфиг
#[tauri::command]
pub async fn apply_dns_config(apply: bool) -> CmdResult {
    if apply {
        // Читаем файл DNS-конфига
        let dns_path = dirs::app_home_dir().stringify_err()?.join(constants::files::DNS_CONFIG);

        if !dns_path.exists() {
            logging!(warn, Type::Config, "DNS config file not found");
            return Err("DNS config file not found".into());
        }

        let dns_yaml = fs::read_to_string(&dns_path).await.stringify_err_log(|e| {
            logging!(error, Type::Config, "Failed to read DNS config: {e}");
        })?;

        // Разбираем DNS-конфиг
        let patch_config = serde_yaml_ng::from_str::<serde_yaml_ng::Mapping>(&dns_yaml).stringify_err_log(|e| {
            logging!(error, Type::Config, "Failed to parse DNS config: {e}");
        })?;

        logging!(info, Type::Config, "Applying DNS config from file");

        // Создаём patch с DNS-конфигом
        let mut patch = serde_yaml_ng::Mapping::new();
        patch.insert("dns".into(), patch_config.into());

        // Применяем DNS-конфиг к рабочему конфигу
        Config::runtime().await.edit_draft(|d| {
            d.patch_config(&patch);
        });

        // Применяем новый конфиг
        CoreManager::global()
            .update_config_checked()
            .await
            .stringify_err_log(|err| {
                let err = format!("Failed to apply config with DNS: {err}");
                logging!(error, Type::Config, "{err}");
            })?;

        logging!(info, Type::Config, "DNS config successfully applied");
    } else {
        // При отключении настроек DNS конфиг пересоздаётся заново (без загрузки файла DNS-конфига)
        logging!(info, Type::Config, "DNS settings disabled, regenerating config");

        CoreManager::global()
            .update_config_checked()
            .await
            .stringify_err_log(|err| {
                let err = format!("Failed to apply regenerated config: {err}");
                logging!(error, Type::Config, "{err}");
            })?;

        logging!(info, Type::Config, "Config regenerated successfully");
    }

    handle::Handle::refresh_clash();
    Ok(())
}

/// Проверить, существует ли файл DNS-конфига
#[tauri::command]
pub fn check_dns_config_exists() -> CmdResult<bool> {
    use crate::utils::dirs;

    let dns_path = dirs::app_home_dir().stringify_err()?.join(constants::files::DNS_CONFIG);

    Ok(dns_path.exists())
}

/// Получить содержимое файла DNS-конфига
#[tauri::command]
pub async fn get_dns_config_content() -> CmdResult<String> {
    use crate::utils::dirs;
    use tokio::fs;

    let dns_path = dirs::app_home_dir().stringify_err()?.join(constants::files::DNS_CONFIG);

    if !fs::try_exists(&dns_path).await.stringify_err()? {
        return Err("DNS config file not found".into());
    }

    let content = fs::read_to_string(&dns_path).await.stringify_err()?.into();
    Ok(content)
}

/// Проверить файл DNS-конфига
#[tauri::command]
pub async fn validate_dns_config() -> CmdResult<ValidationOutcome> {
    let app_dir = dirs::app_home_dir().stringify_err()?;
    let dns_path = app_dir.join(constants::files::DNS_CONFIG);
    let dns_path_str = dns_path.to_str().unwrap_or_default();

    if !dns_path.exists() {
        return Ok(ValidationOutcome::invalid_from_message("DNS config file not found"));
    }

    CoreConfigValidator::validate_config_file_outcome(dns_path_str, None)
        .await
        .stringify_err()
}

#[tauri::command]
pub async fn get_clash_logs() -> CmdResult<Vec<CompactString>> {
    let logs = CoreManager::global().get_clash_logs().await.unwrap_or_default();
    Ok(logs)
}
