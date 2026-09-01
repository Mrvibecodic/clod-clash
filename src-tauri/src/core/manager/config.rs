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

        // Draft only, no `apply` here: this runs on the boot path before the
        // core exists, so nothing has accepted this build yet. The core starts
        // from the draft (`generate_file` prefers `latest`) and `start_core`
        // commits it once the start succeeded.
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
                    return self.replace_core_and_apply().await;
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
                self.replace_core_and_apply().await
            }
        }
    }

    async fn reload_config(&self, force: bool, path: &str) -> Result<(), MihomoError> {
        handle::Handle::mihomo().await.reload_config(force, path).await
    }

    /// Полный перезапуск ядра и итог по нему: применить черновик или откатить.
    async fn replace_core_and_apply(&self) -> Result<()> {
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

        let attempt = stage_with_confirmation(crate::constants::timing::STAGE_CONFIRM_TIMEOUT, || async {
            match service::stage_runtime_by_service(path).await {
                Ok(request) => StageAttempt::Answered(request),
                Err(error) => StageAttempt::Unanswered(format!("{error:#}")),
            }
        })
        .await;

        match attempt {
            StageAttempt::Answered(service::StageRequest::Answered(StageRuntimeOutcome::Staged { config_path })) => {
                StagedPath::Staged(config_path.into())
            }
            StageAttempt::Answered(service::StageRequest::Answered(StageRuntimeOutcome::RestartRequired {
                reason,
            })) => {
                logging!(
                    info,
                    Type::Core,
                    "Service declined to stage the runtime ({reason:?}); taking the restart path"
                );
                StagedPath::NotStaged
            }
            StageAttempt::Answered(service::StageRequest::Refused { code, message }) => {
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
            StageAttempt::Unanswered(reason) => {
                logging!(
                    warn,
                    Type::Core,
                    "Failed to stage the service runtime ({reason}); taking the restart path"
                );
                StagedPath::NotStaged
            }
        }
    }
}

/// clod: чем закончилась просьба подготовить поколение.
///
/// Отказ и «нужен перезапуск» — это ОТВЕТЫ: служба всё решила сама. Молчание —
/// другое дело: поколение фиксируется ДО ответа, поэтому потерянный ответ не
/// означает, что подготовки не было.
enum StageAttempt {
    Answered(crate::core::service::StageRequest),
    Unanswered(std::string::String),
}

/// Спросить ещё раз, если служба промолчала.
///
/// Полный перезапуск ядра рвёт все соединения, и менять на него мягкую
/// перезагрузку из-за потерянного по дороге ответа — слишком дорого. Повтор
/// безопасен: подготовка идемпотентна, служба просто зафиксирует поколение
/// заново. Второй вопрос ограничен по времени, чтобы молчащая служба не
/// задержала применение конфига насовсем.
async fn stage_with_confirmation<Ask, Fut>(confirm_within: std::time::Duration, ask: Ask) -> StageAttempt
where
    Ask: Fn() -> Fut,
    Fut: std::future::Future<Output = StageAttempt>,
{
    let first = match ask().await {
        StageAttempt::Unanswered(reason) => reason,
        answered => return answered,
    };
    logging!(
        warn,
        Type::Core,
        "Staging did not answer ({first}); asking once more before restarting the core"
    );
    match tokio::time::timeout(confirm_within, ask()).await {
        Ok(StageAttempt::Unanswered(again)) => StageAttempt::Unanswered(format!("{first}; asked again: {again}")),
        Ok(answered) => answered,
        Err(_) => StageAttempt::Unanswered(format!("{first}; the second ask did not answer either")),
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
    // clod:e3-05 — mihomo пересоздаёт эти inbound-ы только под `force`.
    "ss-config",
    "vmess-config",
    "tuic-server",
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
    use super::{StageAttempt, listeners_need_recreate, stage_with_confirmation};
    use crate::core::service::StageRequest;
    use clash_verge_service_ipc::StageRuntimeOutcome;
    use std::{
        sync::atomic::{AtomicUsize, Ordering},
        time::Duration,
    };

    #[allow(clippy::expect_used)]
    fn mapping(yaml: &str) -> serde_yaml_ng::Mapping {
        serde_yaml_ng::from_str(yaml).expect("test yaml should parse")
    }

    const CONFIRM_WITHIN: Duration = Duration::from_millis(200);

    fn staged() -> StageAttempt {
        StageAttempt::Answered(StageRequest::Answered(StageRuntimeOutcome::Staged {
            config_path: "/service/runtime.generation-1/config.yaml".to_owned(),
        }))
    }

    fn staged_path(attempt: &StageAttempt) -> Option<&str> {
        match attempt {
            StageAttempt::Answered(StageRequest::Answered(StageRuntimeOutcome::Staged { config_path })) => {
                Some(config_path.as_str())
            }
            _ => None,
        }
    }

    #[tokio::test]
    async fn an_answered_request_is_not_asked_twice() {
        let asks = AtomicUsize::new(0);
        let attempt = stage_with_confirmation(CONFIRM_WITHIN, || {
            asks.fetch_add(1, Ordering::Relaxed);
            async { staged() }
        })
        .await;

        assert!(staged_path(&attempt).is_some());
        assert_eq!(asks.load(Ordering::Relaxed), 1, "лишний запрос службе не нужен");
    }

    #[tokio::test]
    async fn a_lost_answer_is_confirmed_by_asking_again() {
        // Ради этого случая всё и сделано: служба зафиксировала поколение, а
        // ответ не доехал. Полный перезапуск ядра здесь был бы напрасным.
        let asks = AtomicUsize::new(0);
        let attempt = stage_with_confirmation(CONFIRM_WITHIN, || {
            let first = asks.fetch_add(1, Ordering::Relaxed) == 0;
            async move {
                if first {
                    StageAttempt::Unanswered("ipc timeout".to_owned())
                } else {
                    staged()
                }
            }
        })
        .await;

        assert_eq!(staged_path(&attempt), Some("/service/runtime.generation-1/config.yaml"));
        assert_eq!(asks.load(Ordering::Relaxed), 2);
    }

    #[tokio::test]
    async fn two_silences_keep_the_restart_path_and_name_both() {
        let attempt = stage_with_confirmation(CONFIRM_WITHIN, || async {
            StageAttempt::Unanswered("ipc timeout".to_owned())
        })
        .await;

        let StageAttempt::Unanswered(reason) = attempt else {
            unreachable!("молчание не должно превращаться в ответ")
        };
        assert!(reason.contains("asked again"), "в причине видно обе попытки: {reason}");
    }

    #[tokio::test]
    async fn a_hanging_second_ask_does_not_block_the_config() {
        let asks = AtomicUsize::new(0);
        let attempt = stage_with_confirmation(CONFIRM_WITHIN, || {
            let first = asks.fetch_add(1, Ordering::Relaxed) == 0;
            async move {
                if first {
                    return StageAttempt::Unanswered("ipc timeout".to_owned());
                }
                // Второй вопрос повис: служба не отвечает вовсе.
                tokio::time::sleep(Duration::from_secs(30)).await;
                staged()
            }
        })
        .await;

        let StageAttempt::Unanswered(reason) = attempt else {
            unreachable!("зависший повтор обязан упереться в таймаут")
        };
        assert!(reason.contains("did not answer either"));
    }

    #[test]
    fn unchanged_listeners_allow_a_soft_reload() {
        let prev = mapping("{mixed-port: 7890, tun: {enable: true}, proxies: [a], mode: rule}");
        let next = mapping("{mixed-port: 7890, tun: {enable: true}, proxies: [a, b], mode: global}");
        assert!(!listeners_need_recreate(Some(&prev), Some(&next)));

        // Лишний `force` пересоздал бы все inbound-ы и порвал соединения:
        // неизменные ss/vmess/tuic обязаны оставаться мягкой перезагрузкой.
        let prev = mapping(
            "{mixed-port: 7890, ss-config: 'ss://a@:1080', vmess-config: 'vmess://b@:1081', tuic-server: {enable: true, token: [t]}, proxies: [a]}",
        );
        let next = mapping(
            "{mixed-port: 7890, ss-config: 'ss://a@:1080', vmess-config: 'vmess://b@:1081', tuic-server: {token: [t], enable: true}, proxies: [a, b]}",
        );
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
    fn changed_extra_inbounds_force_a_full_reload() {
        // clod:e3-05 — эти три inbound-а mihomo пересоздаёт только под `force`.
        let prev = mapping(
            "{mixed-port: 7890, ss-config: 'ss://a@:1080', vmess-config: 'vmess://b@:1081', tuic-server: {enable: false}}",
        );
        assert!(listeners_need_recreate(
            Some(&prev),
            Some(&mapping(
                "{mixed-port: 7890, ss-config: 'ss://z@:1080', vmess-config: 'vmess://b@:1081', tuic-server: {enable: false}}"
            ))
        ));
        assert!(listeners_need_recreate(
            Some(&prev),
            Some(&mapping(
                "{mixed-port: 7890, ss-config: 'ss://a@:1080', vmess-config: 'vmess://z@:1081', tuic-server: {enable: false}}"
            ))
        ));
        assert!(listeners_need_recreate(
            Some(&prev),
            Some(&mapping(
                "{mixed-port: 7890, ss-config: 'ss://a@:1080', vmess-config: 'vmess://b@:1081', tuic-server: {enable: true}}"
            ))
        ));
    }

    #[test]
    fn missing_previous_config_forces_a_full_reload() {
        let next = mapping("{mixed-port: 7890}");
        assert!(listeners_need_recreate(None, Some(&next)));
        assert!(listeners_need_recreate(Some(&next), None));
    }
}
