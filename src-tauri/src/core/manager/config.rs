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
use clash_verge_service_ipc::StageRuntimeOutcome;
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
        // clod:svc-2.6 — в service-режиме ядро работает не с нашим файлом, а с
        // копией в «поколении» службы: сначала просим службу привести поколение
        // к новому конфигу (staging), и ядру отдаётся ПУТЬ ИЗ ПОКОЛЕНИЯ.
        // Перезагружать ядро нашим путём нельзя: провайдерские пути в нём не
        // переписаны, а после рестарта ядра службой конфиг откатился бы к
        // прошлому поколению. Отказ staging — не ошибка: медленный путь
        // (полный перезапуск ядра со свежим бандлом) остаётся в фолбэках ниже.
        let service_mode = matches!(*self.get_running_mode(), super::RunningMode::Service);
        let reload_path: String = if service_mode {
            match self.stage_into_service_generation(&path).await {
                StagedPath::Staged(staged) => staged,
                StagedPath::RefusedTheBundle(message) => {
                    logging!(
                        warn,
                        Type::Core,
                        "Service refused the runtime, leaving the core running: {message}"
                    );
                    Config::runtime().await.discard();
                    return Err(anyhow!("{message}"));
                }
                // В service-режиме перезагрузка НАШИМ путём запрещена всегда:
                // мягкий reload с непереписанными провайдерскими путями может
                // «успеть» — и оставить старый бинарь/чужие файлы, а рестарт
                // ядра службой откатит конфиг на прошлое поколение. Любой
                // не-staged исход — сразу полный перезапуск ядра: он
                // материализует свежий бандл сам.
                StagedPath::NotStaged => {
                    logging!(info, Type::Core, "Staging unavailable; replacing the service core");
                    return match self.restart_core().await {
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
                    };
                }
            }
        } else {
            dirs::path_to_str(&path)?.into()
        };
        let path = reload_path.as_str();

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

    /// Попросить службу подготовить поколение под новый конфиг.
    ///
    /// Зовётся только в service-режиме. Любой исход, кроме успеха и
    /// отказа-про-бандл, сводится к `NotStaged` — и вызывающий уходит в
    /// полный перезапуск ядра, который материализует свежий бандл сам.
    async fn stage_into_service_generation(&self, path: &std::path::Path) -> StagedPath {
        use crate::core::service;

        if !service::active_service_supports_runtime_staging() {
            return StagedPath::NotStaged;
        }

        match service::stage_runtime_by_service(path).await {
            Ok(service::StageRequest::Answered(StageRuntimeOutcome::Staged { config_path })) => {
                StagedPath::Staged(config_path.into())
            }
            Ok(service::StageRequest::Answered(StageRuntimeOutcome::RestartRequired { reason })) => {
                logging!(
                    info,
                    Type::Core,
                    "Service declined to stage the runtime ({reason:?}); taking the restart path"
                );
                StagedPath::NotStaged
            }
            Ok(service::StageRequest::Refused { code, message }) => {
                if service::StageRequest::is_about_the_bundle(code) {
                    StagedPath::RefusedTheBundle(message.to_string())
                } else {
                    logging!(
                        warn,
                        Type::Core,
                        "Service refused to stage the runtime ({message}); taking the restart path"
                    );
                    StagedPath::NotStaged
                }
            }
            Err(error) => {
                logging!(
                    warn,
                    Type::Core,
                    "Failed to stage the service runtime ({error:#}); taking the restart path"
                );
                StagedPath::NotStaged
            }
        }
    }
}

/// Каким путём перезагружать ядро после попытки staging.
enum StagedPath {
    /// Служба подготовила поколение — перезагружаемся из него.
    Staged(String),
    /// Staging не случился — в service-режиме это сразу полный перезапуск
    /// ядра; в sidecar-режиме staging не зовётся вовсе.
    NotStaged,
    /// Служба отвергла сам бандл: старт повторил бы отказ, ядро не трогаем.
    RefusedTheBundle(std::string::String),
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
