use super::CmdResult;
use crate::feat;
use crate::utils::{dirs, yaml_emitter};
use crate::{
    cmd::StringifyErr as _,
    config::{ClashInfo, Config},
    constants,
    core::{
        CoreManager, handle,
        validate::{CoreConfigValidator, ValidationErrorKind, ValidationOutcome},
    },
};
use clash_verge_logging::{Type, logging, logging_error};
use compact_str::CompactString;
use serde_yaml_ng::Mapping;
use smartstring::alias::String;
use tokio::fs;

#[tauri::command]
pub async fn copy_clash_env() -> CmdResult {
    feat::copy_clash_env().await;
    Ok(())
}

#[tauri::command]
pub async fn get_clash_info() -> CmdResult<ClashInfo> {
    Ok(Config::clash().await.data_arc().get_client_info())
}

#[tauri::command]
pub async fn patch_clash_config(payload: Mapping) -> CmdResult {
    feat::patch_clash(&payload).await.stringify_err()
}

#[tauri::command]
pub async fn patch_clash_mode(payload: String) -> CmdResult {
    feat::change_clash_mode(payload).await
}
#[tauri::command]
pub async fn change_clash_core(clash_core: String) -> CmdResult<Option<String>> {
    logging!(info, Type::Config, "changing core to {clash_core}");

    match CoreManager::global().change_core(&clash_core).await {
        Ok(_) => {
            logging_error!(Type::Core, Config::profiles().await.data_arc().save_file().await);

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
#[tauri::command]
pub async fn stop_core() -> CmdResult {
    logging_error!(Type::Core, Config::profiles().await.data_arc().save_file().await);
    let result = CoreManager::global().stop_core().await.stringify_err();
    if result.is_ok() {
        handle::Handle::refresh_clash();
    }
    result
}

#[tauri::command]
pub async fn restart_core() -> CmdResult {
    logging_error!(Type::Core, Config::profiles().await.data_arc().save_file().await);
    crate::feat::tun::clear_suppression();
    let result = CoreManager::global().restart_core().await.stringify_err();
    if result.is_ok() {
        handle::Handle::refresh_clash();
    }
    result
}
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DnsSaveOutcome {
    saved: bool,
    validation: ValidationOutcome,
}

const fn reached_a_verdict(outcome: &ValidationOutcome) -> bool {
    match outcome {
        ValidationOutcome::Valid => true,
        ValidationOutcome::Invalid { kind, .. } => matches!(
            kind,
            ValidationErrorKind::CoreRejected
                | ValidationErrorKind::YamlSyntax
                | ValidationErrorKind::YamlMapping
                | ValidationErrorKind::ScriptSyntax
                | ValidationErrorKind::ScriptMissingMain
        ),
        ValidationOutcome::Skipped { .. } | ValidationOutcome::Busy => false,
    }
}

#[tauri::command]
pub async fn save_dns_config(dns_config: Mapping) -> CmdResult<DnsSaveOutcome> {
    let app_dir = dirs::app_home_dir().stringify_err()?;
    let dns_path = app_dir.join(constants::files::DNS_CONFIG);
    let check_path = app_dir.join(constants::files::DNS_CHECK_CONFIG);

    let yaml_str = yaml_emitter::to_mihomo_config_string(&dns_config).stringify_err()?;

    let in_context = Config::dns_page_check_config(&dns_config).await;
    let check_yaml = match in_context.as_ref() {
        Some(context) => yaml_emitter::to_mihomo_config_string(context).stringify_err()?,
        None => yaml_str.clone(),
    };

    crate::utils::help::write_atomic(&check_path, check_yaml.as_bytes())
        .await
        .stringify_err()?;

    let outcome =
        CoreConfigValidator::validate_config_file_outcome(check_path.to_str().unwrap_or_default(), None).await;
    let _ = fs::remove_file(&check_path).await;

    let validation = match outcome {
        Ok(outcome) => outcome,
        Err(err) => ValidationOutcome::invalid(
            ValidationErrorKind::ProcessTerminated,
            format!("Configuration check could not be run: {err}"),
        ),
    };

    if !validation.is_valid() {
        if in_context.is_some() && reached_a_verdict(&validation) {
            logging!(warn, Type::Config, "DNS config rejected, nothing written: {validation}");
            return Ok(DnsSaveOutcome {
                saved: false,
                validation,
            });
        }

        logging!(
            warn,
            Type::Config,
            "DNS config check reached no verdict, saving anyway: {validation}"
        );
    }

    crate::utils::help::write_atomic(&dns_path, yaml_str.as_bytes())
        .await
        .stringify_err()?;
    logging!(info, Type::Config, "DNS config saved to {dns_path:?}");

    Ok(DnsSaveOutcome {
        saved: true,
        validation,
    })
}

#[tauri::command]
pub async fn apply_dns_config(apply: bool) -> CmdResult {
    if apply {
        crate::utils::init::ensure_dns_config_file()
            .await
            .stringify_err_log(|e| {
                logging!(error, Type::Config, "Failed to create DNS config: {e}");
            })?;

        logging!(info, Type::Config, "Applying DNS config from file");

        CoreManager::global()
            .update_config_checked()
            .await
            .stringify_err_log(|err| {
                let err = format!("Failed to apply config with DNS: {err}");
                logging!(error, Type::Config, "{err}");
            })?;

        logging!(info, Type::Config, "DNS config successfully applied");
    } else {
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

#[tauri::command]
pub fn check_dns_config_exists() -> CmdResult<bool> {
    use crate::utils::dirs;

    let dns_path = dirs::app_home_dir().stringify_err()?.join(constants::files::DNS_CONFIG);

    Ok(dns_path.exists())
}

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

#[tauri::command]
pub async fn get_clash_logs() -> CmdResult<Vec<CompactString>> {
    let logs = CoreManager::global().get_clash_logs().await.unwrap_or_default();
    Ok(logs)
}

#[cfg(test)]
mod tests {
    use super::reached_a_verdict;
    use crate::core::validate::{ValidationErrorKind, ValidationOutcome, ValidationSkipReason};

    #[test]
    fn the_core_judging_the_config_is_a_verdict() {
        assert!(reached_a_verdict(&ValidationOutcome::Valid));

        for kind in [
            ValidationErrorKind::CoreRejected,
            ValidationErrorKind::YamlSyntax,
            ValidationErrorKind::YamlMapping,
            ValidationErrorKind::ScriptSyntax,
            ValidationErrorKind::ScriptMissingMain,
        ] {
            assert!(
                reached_a_verdict(&ValidationOutcome::invalid(kind, "nope")),
                "{kind:?} is the core rejecting the config"
            );
        }
    }

    #[test]
    fn a_check_that_never_ran_is_not_a_verdict() {
        for kind in [
            ValidationErrorKind::FileMissing,
            ValidationErrorKind::FileRead,
            ValidationErrorKind::ProcessTerminated,
            ValidationErrorKind::Timeout,
        ] {
            assert!(
                !reached_a_verdict(&ValidationOutcome::invalid(kind, "nope")),
                "{kind:?} means the check produced no verdict"
            );
        }

        assert!(!reached_a_verdict(&ValidationOutcome::Busy));
        assert!(!reached_a_verdict(&ValidationOutcome::Skipped {
            reason: ValidationSkipReason::Exiting
        }));
    }
}
