use super::CoreManager;
use crate::{
    config::{Config, ConfigType, runtime::IRuntime},
    constants::timing,
    core::{
        handle,
        validate::{CoreConfigValidator, ValidationOutcome, ValidationSkipReason},
    },
    utils::{dirs, help},
};
use anyhow::{Result, anyhow};
use clash_verge_logging::{Type, logging};
use scopeguard::defer;
use smartstring::alias::String;
use std::{collections::HashSet, path::PathBuf, time::Instant};
use tauri_plugin_mihomo::Error as MihomoError;

impl CoreManager {
    pub async fn use_default_config(&self, error_key: &str, error_msg: &str) -> Result<()> {
        use crate::constants::files::RUNTIME_CONFIG;

        let runtime_path = dirs::app_home_dir()?.join(RUNTIME_CONFIG);
        let clash_config = &Config::clash().await.latest_arc().0;

        Config::runtime().await.edit_draft(|d| {
            *d = IRuntime {
                config: Some(clash_config.to_owned()),
                exists_keys: HashSet::new(),
                chain_logs: Default::default(),
                // Дефолтный конфиг — не от панели: заглушек в нём нет.
                sentinel_report: Default::default(),
            }
        });

        help::save_yaml(&runtime_path, &clash_config, Some("# Clash Verge Runtime")).await?;
        handle::Handle::notice_message(error_key, error_msg);
        Ok(())
    }

    pub async fn update_config_forced(&self) -> Result<ValidationOutcome> {
        self.update_config_with_force(true).await
    }

    pub async fn update_config_with_force(&self, force: bool) -> Result<ValidationOutcome> {
        if handle::Handle::global().is_exiting() {
            return Ok(ValidationOutcome::Skipped {
                reason: ValidationSkipReason::Exiting,
            });
        }

        if !self.try_start_config_update() {
            logging!(info, Type::Core, "Configuration update is already running");
            return Ok(ValidationOutcome::Busy);
        }
        defer! {
            self.finish_config_update();
        }

        if !force && !self.should_update_config() {
            logging!(debug, Type::Core, "Skipping config update due to debounce");
            return Ok(ValidationOutcome::Skipped {
                reason: ValidationSkipReason::Debounced,
            });
        }

        if force {
            self.set_last_update(Instant::now());
        }

        self.perform_config_update().await
    }

    pub async fn update_config_checked(&self) -> Result<()> {
        let outcome = self.update_config_forced().await?;
        if outcome.is_valid() {
            Ok(())
        } else {
            Err(anyhow!("{outcome}"))
        }
    }

    fn should_update_config(&self) -> bool {
        let now = Instant::now();
        let last = self.get_last_update();

        if let Some(last_time) = last
            && now.duration_since(*last_time) < timing::CONFIG_UPDATE_DEBOUNCE
        {
            return false;
        }

        self.set_last_update(now);
        true
    }

    async fn perform_config_update(&self) -> Result<ValidationOutcome> {
        if let Err(err) = Config::generate().await {
            let message: String = err.to_string().into();
            Config::runtime().await.discard();
            return Ok(ValidationOutcome::invalid_from_message(message));
        }

        self.apply_generate_config_inner().await
    }

    pub(crate) async fn update_runtime_config<F>(&self, f: F) -> Result<ValidationOutcome>
    where
        F: FnOnce(&mut IRuntime),
    {
        if !self.try_start_config_update() {
            logging!(info, Type::Core, "Configuration update is already running");
            return Ok(ValidationOutcome::Busy);
        }
        defer! {
            self.finish_config_update();
        }

        Config::runtime().await.edit_draft(f);
        self.apply_generate_config_inner().await
    }

    async fn apply_generate_config_inner(&self) -> Result<ValidationOutcome> {
        match CoreConfigValidator::global().validate_config_outcome().await {
            Ok(outcome) if outcome.is_valid() => {
                let run_path = Config::generate_file(ConfigType::Run).await?;
                self.apply_config(run_path).await?;
                Ok(ValidationOutcome::Valid)
            }
            Ok(outcome) => {
                Config::runtime().await.discard();
                Ok(outcome)
            }
            Err(e) => {
                Config::runtime().await.discard();
                Err(e)
            }
        }
    }

    async fn apply_config(&self, path: PathBuf) -> Result<()> {
        let path = dirs::path_to_str(&path)?;

        // clod: обновление подписки не должно рвать активные соединения.
        // `force=true` в mihomo пересоздаёт inbound-листенеры, поэтому
        // передаём его только когда изменилось что-то из «слушающей» части
        // конфига (порты, tun, allow-lan и т.п.). Прокси/группы/правила/DNS
        // mihomo применяет и при force=false.
        let force = {
            let runtime = Config::runtime().await;
            let next = runtime.latest_arc();
            let prev = runtime.data_arc();
            listeners_need_recreate(prev.config.as_ref(), next.config.as_ref())
        };

        match self.reload_config(force, path).await {
            Ok(_) => {
                Config::runtime().await.apply();
                logging!(info, Type::Core, "Configuration applied (force={force})");
                Ok(())
            }
            Err(err) => {
                // Мягкая перезагрузка не прошла — прежде чем перезапускать
                // ядро (и ронять все соединения), пробуем полный reload.
                if !force && matches!(self.reload_config(true, path).await, Ok(())) {
                    Config::runtime().await.apply();
                    logging!(info, Type::Core, "Configuration applied after forced reload");
                    return Ok(());
                }
                logging!(
                    warn,
                    Type::Core,
                    "Failed to apply configuration by mihomo api, restart core to apply it, error msg: {err}"
                );
                match self.restart_core().await {
                    Ok(_) => {
                        Config::runtime().await.apply();
                        logging!(info, Type::Core, "Configuration applied after restart");
                        Ok(())
                    }
                    Err(err) => {
                        logging!(error, Type::Core, "Failed to restart core: {}", err);
                        Config::runtime().await.discard();
                        Err(anyhow!("Failed to apply config: {}", err))
                    }
                }
            }
        }
    }

    async fn reload_config(&self, force: bool, path: &str) -> Result<(), MihomoError> {
        handle::Handle::mihomo().await.reload_config(force, path).await
    }
}

/// Ключи конфига, изменение которых требует пересоздания inbound-листенеров
/// (`PUT /configs?force=true`). Всё остальное mihomo применяет мягко.
const LISTENER_KEYS: &[&str] = &[
    "mixed-port",
    "socks-port",
    "port",
    "redir-port",
    "tproxy-port",
    "tun",
    "allow-lan",
    "bind-address",
    "lan-allowed-ips",
    "lan-disallowed-ips",
    "authentication",
    "skip-auth-prefixes",
    "listeners",
    "external-controller",
    "external-controller-unix",
    "external-controller-pipe",
    "external-controller-cors",
    "secret",
    "ipv6",
];

/// clod: сравнить «слушающую» часть двух runtime-конфигов.
///
/// `prev` — конфиг, применённый в прошлый раз; `None` (первый запуск) всегда
/// означает полный reload.
fn listeners_need_recreate(prev: Option<&serde_yaml_ng::Mapping>, next: Option<&serde_yaml_ng::Mapping>) -> bool {
    let (Some(prev), Some(next)) = (prev, next) else {
        return true;
    };
    LISTENER_KEYS.iter().any(|key| prev.get(*key) != next.get(*key))
}

#[cfg(test)]
mod tests {
    use super::listeners_need_recreate;

    #[allow(clippy::expect_used)]
    fn mapping(yaml: &str) -> serde_yaml_ng::Mapping {
        serde_yaml_ng::from_str(yaml).expect("test yaml should parse")
    }

    #[test]
    fn unchanged_listeners_allow_a_soft_reload() {
        let prev = mapping("{mixed-port: 7890, tun: {enable: true}, proxies: [a], mode: rule}");
        let next = mapping("{mixed-port: 7890, tun: {enable: true}, proxies: [a, b], mode: global}");
        assert!(!listeners_need_recreate(Some(&prev), Some(&next)));
    }

    #[test]
    fn changed_ports_or_tun_force_a_full_reload() {
        let prev = mapping("{mixed-port: 7890, tun: {enable: true}}");
        assert!(listeners_need_recreate(
            Some(&prev),
            Some(&mapping("{mixed-port: 7891, tun: {enable: true}}"))
        ));
        assert!(listeners_need_recreate(
            Some(&prev),
            Some(&mapping("{mixed-port: 7890, tun: {enable: false}}"))
        ));
    }

    #[test]
    fn missing_previous_config_forces_a_full_reload() {
        let next = mapping("{mixed-port: 7890}");
        assert!(listeners_need_recreate(None, Some(&next)));
        assert!(listeners_need_recreate(Some(&next), None));
    }
}
