use super::CoreManager;
use crate::{
    config::{Config, ConfigType, runtime::IRuntime},
    constants::timing,
    core::{
        handle,
        validate::{CoreConfigValidator, ValidationErrorKind, ValidationOutcome, ValidationSkipReason},
    },
    enhance::Sources,
    utils::{dirs, help},
};
use anyhow::{Result, anyhow};
use clash_verge_logging::{Type, logging};
use clash_verge_service_ipc::StageRuntimeOutcome;
use smartstring::alias::String;
use std::{path::PathBuf, time::Duration, time::Instant};
use tauri_plugin_mihomo::Error as MihomoError;

/// Как отдать ядру проверенный конфиг.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivery {
    /// Мягкая перезагрузка (`PUT /configs`); не прошла — перезапуск. Ядра нет —
    /// сразу старт.
    Reload,
    /// Сразу перезапуск: адрес контроллера, секрет и смену сборки ядра мягко
    /// применить нельзя.
    Restart,
}

/// Сколько обычное применение ждёт занятого признака. Занято — значит кто-то
/// применяет свою правку секунду-другую; отвечать человеку «занято» вместо того,
/// чтобы дождаться, — не честность, а невнимательность.
const DOOR_WAIT: Duration = Duration::from_secs(5);

/// Признак «идёт применение конфига»; снимается при выходе из области на любом пути.
pub(crate) struct ConfigUpdateGuard<'a>(&'a CoreManager);

impl Drop for ConfigUpdateGuard<'_> {
    fn drop(&mut self) {
        self.0.finish_config_update();
    }
}

/// Сборка, которую ядро проверило (`mihomo -t`) и не отвергло.
///
/// Единственная дверь к слоту рантайма: пока `Staged` жив, признак применения
/// держится, другой сборке в слот не попасть. Слот заменяется в `deliver` —
/// только после того, как ядро приняло конфиг; отказ ничего не меняет, откатывать
/// нечего. Брошенный `Staged` — проверка без доставки (подписка не текущая).
#[must_use = "a staged build changes nothing until it is delivered"]
pub struct Staged<'a> {
    manager: &'a CoreManager,
    build: IRuntime,
    _guard: ConfigUpdateGuard<'a>,
}

impl Staged<'_> {
    /// Отдать ядру. `Ok(Valid)` — ядро работает с этой сборкой и слот заменён;
    /// `Ok(Invalid)` — служба отвергла бандл, ядро осталось на прежнем;
    /// `Err` — доставка сорвалась (ядро о содержимом ничего не сказало).
    pub async fn deliver(self, delivery: Delivery) -> Result<ValidationOutcome> {
        self.deliver_committing(delivery, async || Ok(())).await
    }

    /// То же, но после приёма ядром — ещё под признаком применения — выполнить
    /// `commit`: записать в свой слой то, из чего собиралось. Иначе между
    /// освобождением признака и записью чужая сборка читала бы прежнее принятое
    /// и откатывала бы ядру то, что оно только что приняло. Отказ записи — `Err`
    /// с пометкой, что ядро конфиг уже приняло.
    pub async fn deliver_committing(
        self,
        delivery: Delivery,
        commit: impl AsyncFnOnce() -> Result<()>,
    ) -> Result<ValidationOutcome> {
        let Self { manager, build, _guard } = self;
        let outcome = manager.deliver_build(build, delivery).await?;
        if outcome.is_valid() {
            commit().await.map_err(CommitFailed)?;
        }
        Ok(outcome)
    }

    /// Обновление подписки: если собранный конфиг совпал с тем, что уже работает,
    /// ядро не трогаем — перезагрузка стёрла бы историю задержек и заново
    /// проверила бы все авто-группы, ничего не поменяв.
    pub async fn deliver_unless_unchanged(self) -> Result<ValidationOutcome> {
        if self.manager.runtime_unchanged(&self.build).await {
            let Self { manager, build, .. } = self;
            manager.accept_without_the_core(build).await;
            logging!(info, Type::Core, "Runtime config unchanged, core reload skipped");
            return Ok(ValidationOutcome::Valid);
        }
        self.deliver(Delivery::Reload).await
    }
}

impl CoreManager {
    /// Взять признак применения конфига; `None` — уже идёт другое применение.
    pub(crate) fn claim_config_update(&self) -> Option<ConfigUpdateGuard<'_>> {
        // Гвард создаётся только при удавшемся захвате: `then_some` строил бы его и
        // при отказе — и его Drop снимал бы признак у того, кто его держит.
        self.try_start_config_update().then(|| ConfigUpdateGuard(self))
    }

    /// То же, но подождать освобождения до `wait`: занято — значит «чуть позже»,
    /// а не «в другой раз».
    pub(crate) async fn claim_config_update_within(&self, wait: Duration) -> Option<ConfigUpdateGuard<'_>> {
        let deadline = tokio::time::Instant::now() + wait;
        loop {
            let released = self.config_update_done.notified();
            if let Some(guard) = self.claim_config_update() {
                return Some(guard);
            }
            if tokio::time::timeout_at(deadline, released).await.is_err() {
                return self.claim_config_update();
            }
        }
    }

    /// Собрать конфиг из источников (принятое читается под признаком применения)
    /// и проверить его ядром. Признак живёт до конца доставки.
    pub async fn stage_with(&self, sources: Sources) -> Result<std::result::Result<Staged<'_>, ValidationOutcome>> {
        self.stage_within(sources, DOOR_WAIT).await
    }

    /// Как `stage_with`, но занятого признака применения ждёт до `wait`.
    pub async fn stage_within(
        &self,
        sources: Sources,
        wait: Duration,
    ) -> Result<std::result::Result<Staged<'_>, ValidationOutcome>> {
        let Some(guard) = self.claim_for_an_update(wait).await else {
            return Ok(Err(self.why_not_now()));
        };
        let build = match Config::build(sources).await {
            Ok(build) => build,
            Err(err) => return Ok(Err(ValidationOutcome::invalid_from_message(err.to_string()))),
        };
        self.stage_under(guard, build).await
    }

    /// Признак для применения: не во время выхода и с ожиданием занятого.
    async fn claim_for_an_update(&self, wait: Duration) -> Option<ConfigUpdateGuard<'_>> {
        if handle::Handle::global().is_exiting() {
            return None;
        }
        let guard = self.claim_config_update_within(wait).await?;
        // Выход мог начаться, пока ждали.
        if handle::Handle::global().is_exiting() {
            return None;
        }
        Some(guard)
    }

    fn why_not_now(&self) -> ValidationOutcome {
        if handle::Handle::global().is_exiting() {
            ValidationOutcome::Skipped {
                reason: ValidationSkipReason::Exiting,
            }
        } else {
            logging!(info, Type::Core, "Configuration update is already running");
            ValidationOutcome::Busy
        }
    }

    async fn stage_under<'a>(
        &'a self,
        guard: ConfigUpdateGuard<'a>,
        build: IRuntime,
    ) -> Result<std::result::Result<Staged<'a>, ValidationOutcome>> {
        let Some(config) = build.config.as_ref() else {
            return Ok(Err(ValidationOutcome::invalid_from_message("собранный конфиг пуст")));
        };
        // Любой не-Valid исход — вызывающему: отказ ядра, «занято», прибитая
        // проверка различаются у него по виду (`ValidationErrorKind`). Без слова
        // ядра сборка к нему не едет: слепой перезапуск на непроверенной оставлял
        // бы человека без ядра, а мягкий reload расходился бы с поколением службы.
        let outcome = CoreConfigValidator::global()
            .validate_config_outcome_with(config)
            .await?;
        if !outcome.is_valid() {
            return Ok(Err(outcome));
        }
        Ok(Ok(Staged {
            manager: self,
            build,
            _guard: guard,
        }))
    }

    pub async fn update_config_forced(&self) -> Result<ValidationOutcome> {
        self.update_config(Sources::default(), true, false).await
    }

    /// Применить обновлённую подписку: см. `Staged::deliver_unless_unchanged`.
    pub async fn update_config_with_force(&self, force: bool) -> Result<ValidationOutcome> {
        self.update_config(Sources::default(), force, true).await
    }

    /// Пересобрать из переданных источников, отдать ядру и — ещё под признаком
    /// применения — записать принятое в свой слой (`commit`).
    pub async fn update_config_committing(
        &self,
        sources: Sources,
        delivery: Delivery,
        commit: impl AsyncFnOnce() -> Result<()>,
    ) -> Result<()> {
        let staged = match self.stage_with(sources).await? {
            Ok(staged) => staged,
            Err(outcome) => return Err(anyhow!("{outcome}")),
        };
        let outcome = staged.deliver_committing(delivery, commit).await?;
        if outcome.is_valid() {
            Ok(())
        } else {
            Err(anyhow!("{outcome}"))
        }
    }

    async fn update_config(&self, sources: Sources, force: bool, skip_unchanged: bool) -> Result<ValidationOutcome> {
        let Some(guard) = self.claim_for_an_update(DOOR_WAIT).await else {
            return Ok(self.why_not_now());
        };

        if !force && !self.should_update_config() {
            logging!(debug, Type::Core, "Skipping config update due to debounce");
            return Ok(ValidationOutcome::Skipped {
                reason: ValidationSkipReason::Debounced,
            });
        }

        if force {
            self.set_last_update(Instant::now());
        }

        let build = match Config::build(sources).await {
            Ok(build) => build,
            Err(err) => return Ok(ValidationOutcome::invalid_from_message(err.to_string())),
        };

        if skip_unchanged && self.runtime_unchanged(&build).await {
            self.accept_without_the_core(build).await;
            logging!(info, Type::Core, "Runtime config unchanged, core reload skipped");
            return Ok(ValidationOutcome::Valid);
        }

        match self.stage_under(guard, build).await? {
            Ok(staged) => staged.deliver(Delivery::Reload).await,
            Err(outcome) => Ok(outcome),
        }
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

    /// Собранный кандидат совпал с принятым конфигом, и ядро с ним работает.
    /// Перезагрузка тем же конфигом что-то дала бы только двум случаям: остановленное
    /// ядро она поднимала бы, а пустой http-провайдер узлов или правил (первая
    /// загрузка не удалась, кэша нет) — скачивала заново. Их не пропускаем.
    async fn runtime_unchanged(&self, build: &IRuntime) -> bool {
        if matches!(*self.get_running_mode(), super::RunningMode::NotRunning) {
            return false;
        }
        let same = {
            let prev = Config::runtime().await.data_arc();
            build.config.is_some() && build.config == prev.config
        };
        same && providers_filled().await
    }

    /// Сборка принята без обращения к ядру: конфиг тот же, но подписи заглушек
    /// (`sentinel_report`) могли смениться, а заявка на подмену DNS относится к
    /// работающему конфигу.
    async fn accept_without_the_core(&self, build: IRuntime) {
        let profile_uid = build.profile_uid.clone();
        Self::note_the_accepted(&build).await;
        Config::runtime().await.replace(build);
        forget_the_not_applied_mark(profile_uid.as_ref()).await;
    }

    /// Конфиг проверен при старте приложения (или проверить его не вышло) — в слот,
    /// ядро стартует с него.
    pub(crate) async fn accept_at_boot(&self, build: IRuntime) {
        Self::note_the_accepted(&build).await;
        Config::runtime().await.replace(build);
    }

    /// Что сопровождает принятую сборку: заявка на подмену DNS и слово человеку
    /// о записях цепочек, которые приложение отбросило.
    async fn note_the_accepted(build: &IRuntime) {
        Self::remember_dns_desire(build);
        announce_discarded_keys(&build.discarded_keys).await;
    }

    /// clod:dns-applied — заявка на подмену системного DNS едет вместе со сборкой
    /// и запоминается только для принятой; применяется после того, как ядро с
    /// ней работает (`apply_remembered_desire`).
    #[cfg(target_os = "macos")]
    fn remember_dns_desire(build: &IRuntime) {
        match build.dns_desire {
            Some(desire) => crate::utils::resolve::dns::remember_desire(desire.want_base, desire.shaped_fake_ip),
            None => crate::utils::resolve::dns::forget_desire(),
        }
    }

    #[cfg(not(target_os = "macos"))]
    const fn remember_dns_desire(_build: &IRuntime) {}

    /// Поправить принятый конфиг (цепочки прокси из окна) и отдать ядру.
    pub(crate) async fn update_runtime_config<F>(&self, f: F) -> Result<ValidationOutcome>
    where
        F: FnOnce(&mut IRuntime),
    {
        let Some(guard) = self.claim_for_an_update(DOOR_WAIT).await else {
            return Ok(self.why_not_now());
        };

        let mut build = (**Config::runtime().await.data_arc()).clone();
        f(&mut build);
        match self.stage_under(guard, build).await? {
            Ok(staged) => staged.deliver(Delivery::Reload).await,
            Err(outcome) => Ok(outcome),
        }
    }

    async fn deliver_build(&self, build: IRuntime, delivery: Delivery) -> Result<ValidationOutcome> {
        let Some(config) = build.config.as_ref() else {
            return Ok(ValidationOutcome::invalid_from_message("собранный конфиг пуст"));
        };
        let run_path = Config::write_config_file(ConfigType::Run, config).await?;
        // clod:port-ladder — порт мог приехать из подписки: системный
        // прокси и PAC указывают на него, и после смены их надо
        // переписать, каким бы путём конфиг ни доехал до ядра.
        let (mixed_port_changed, mode_changed, sharing_changed) = {
            let prev = Config::runtime().await.data_arc();
            let changed = |key: &str| prev.config.as_ref().and_then(|config| config.get(key)) != config.get(key);
            (changed("mixed-port"), changed("mode"), changed("allow-lan"))
        };
        let profile_uid = build.profile_uid.clone();
        if let Err(error) = self.apply_config(build, run_path, delivery).await {
            if let Some(refused) = error.downcast_ref::<ServiceRefusedTheBundle>() {
                return Ok(ValidationOutcome::invalid(
                    ValidationErrorKind::CoreRejected,
                    refused.0.clone(),
                ));
            }
            return Err(error);
        }
        forget_the_not_applied_mark(profile_uid.as_ref()).await;
        if mixed_port_changed || sharing_changed {
            Self::spawn_mixed_port_check(true);
        }
        if mode_changed {
            crate::process::AsyncHandler::spawn(|| async {
                let _ = crate::core::tray::Tray::global().update_menu().await;
            });
        }
        #[cfg(target_os = "macos")]
        crate::utils::resolve::dns::apply_remembered_desire();
        crate::process::AsyncHandler::spawn(|| async { crate::feat::tun::enforce_undesired_off().await });
        Ok(ValidationOutcome::Valid)
    }

    async fn apply_config(&self, build: IRuntime, path: PathBuf, delivery: Delivery) -> Result<()> {
        // Ядра нет — перезагружать нечего, сразу старт с новой сборкой.
        if delivery == Delivery::Restart || matches!(*self.get_running_mode(), super::RunningMode::NotRunning) {
            return self.replace_core_and_apply(build).await;
        }
        // clod:svc-2.6 — в service-режиме ядро работает не с нашим файлом, а с
        // копией в «поколении» службы: сначала просим службу привести поколение
        // к новому конфигу (staging), и ядру отдаётся ПУТЬ ИЗ ПОКОЛЕНИЯ.
        // Перезагружать ядро нашим путём нельзя: провайдерские пути в нём не
        // переписаны, а после рестарта ядра службой конфиг откатился бы к
        // прошлому поколению. Отказ staging — не ошибка: медленный путь
        // (полный перезапуск ядра со свежим бандлом) остаётся в фолбэках ниже.
        // clod:Э3-03 — режим читается один раз, и всё решение ниже (путь для
        // ядра, staging у службы) верно только для него. Перепроверяем его после
        // перезагрузки: см. `the_core_changed_hands`.
        let mode_seen = self.get_running_mode();
        let service_mode = matches!(*mode_seen, super::RunningMode::Service);
        let reload_path: String = if service_mode {
            match self.stage_into_service_generation(&path).await {
                StagedPath::Staged(staged) => staged,
                StagedPath::RefusedTheBundle(message) => {
                    logging!(
                        warn,
                        Type::Core,
                        "Service refused the runtime, leaving the core running: {message}"
                    );
                    return Err(anyhow!("{message}"));
                }
                StagedPath::Unbuildable(message) => {
                    logging!(
                        warn,
                        Type::Core,
                        "This configuration cannot be handed to the service, leaving the core running: {message}"
                    );
                    return Err(ServiceRefusedTheBundle(message).into());
                }
                // В service-режиме перезагрузка НАШИМ путём запрещена всегда:
                // мягкий reload с непереписанными провайдерскими путями может
                // «успеть» — и оставить старый бинарь/чужие файлы, а рестарт
                // ядра службой откатит конфиг на прошлое поколение. Любой
                // не-staged исход — сразу полный перезапуск ядра: он
                // материализует свежий бандл сам.
                StagedPath::NotStaged => {
                    logging!(info, Type::Core, "Staging unavailable; replacing the service core");
                    return self.replace_core_and_apply(build).await;
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
            let prev = Config::runtime().await.data_arc();
            listeners_need_recreate(prev.config.as_ref(), build.config.as_ref())
        };

        let reloaded = match self.reload_config(force, path).await {
            Ok(()) => Ok(format!("Configuration applied (force={force})")),
            Err(err) => {
                // Мягкая перезагрузка не прошла — прежде чем перезапускать
                // ядро (и ронять все соединения), пробуем полный reload.
                if !force && matches!(self.reload_config(true, path).await, Ok(())) {
                    Ok("Configuration applied after forced reload".to_owned())
                } else {
                    Err(err)
                }
            }
        };

        match reloaded {
            Ok(message) => {
                // clod:Э3-03 — перезагрузка удалась, но у КАКОГО ядра? Пока конфиг
                // ехал, ядро могло перезапуститься после падения и подняться в
                // другом режиме: наш путь у ядра под службой (после перезапуска
                // службой откатится на прошлое поколение) или путь из поколения
                // службы у sidecar. Единственный честный итог — полный перезапуск:
                // он материализует конфиг под тот режим, который есть на самом деле.
                let mode_now = self.get_running_mode();
                if the_core_changed_hands(&mode_seen, &mode_now) {
                    logging!(
                        warn,
                        Type::Core,
                        "core mode changed while applying the configuration ({mode_seen} -> {mode_now}); restarting the core to apply it"
                    );
                    return self.replace_core_and_apply(build).await;
                }
                Self::note_the_accepted(&build).await;
                Config::runtime().await.replace(build);
                logging!(info, Type::Core, "{message}");
                Ok(())
            }
            Err(err) => {
                logging!(
                    warn,
                    Type::Core,
                    "Failed to apply configuration by mihomo api, restart core to apply it, error msg: {err}"
                );
                self.replace_core_and_apply(build).await
            }
        }
    }

    async fn reload_config(&self, force: bool, path: &str) -> Result<(), MihomoError> {
        crate::feat::environment::detached_core_client()
            .reload_config(force, path)
            .await
    }

    /// Полный перезапуск ядра под новую сборку.
    ///
    /// Старт пишет файл ядра из слота, поэтому сборка ставится в слот до
    /// перезапуска. Не поднялось — слот возвращается к прежнему принятому, и
    /// если ядра при этом не осталось, оно поднимается на прежнем: отказ
    /// новой сборки не должен оставлять человека без интернета с системным
    /// прокси на мёртвом порту (Э3-07).
    async fn replace_core_and_apply(&self, build: IRuntime) -> Result<()> {
        let runtime = Config::runtime().await;
        let previous = runtime.data_arc();
        // Отброшенные ключи объявляются после того, как ядро поднялось на сборке:
        // сорвавшийся перезапуск возвращает прежнее принятое.
        let discarded_keys = build.discarded_keys.clone();
        Self::remember_dns_desire(&build);
        runtime.replace(build);
        // Проверенная сборка есть — отказ старта, если он был, снят: стартуем с неё.
        let was_refused = self.startup_refusal();
        self.lift_startup_refusal();
        match self.restart_core_during_config_update().await {
            Ok(()) => {
                logging!(info, Type::Core, "Configuration applied after restart");
                announce_discarded_keys(&discarded_keys).await;
                Ok(())
            }
            Err(err) => {
                logging!(error, Type::Core, "Failed to restart core: {}", err);
                Self::remember_dns_desire(&previous);
                runtime.replace_shared(previous);
                if was_refused.is_some() {
                    // Прежнего принятого нет — причина отказа старта должна остаться
                    // у человека, а не пропасть вместе с неудавшейся сборкой.
                    self.refuse_to_start(format!("{err:#}"));
                }
                self.bring_back_the_previous_core().await;
                Err(anyhow!("Failed to apply config: {}", err))
            }
        }
    }

    /// Ядро не поднялось на новой сборке и его больше нет — поднять прежнее.
    /// Одна попытка: если не заведётся и оно, причина не в сборке, и ждать
    /// осталось только человека.
    async fn bring_back_the_previous_core(&self) {
        if handle::Handle::global().is_exiting() || !matches!(*self.get_running_mode(), super::RunningMode::NotRunning)
        {
            return;
        }
        if Config::runtime().await.data_arc().config.is_none() {
            logging!(
                warn,
                Type::Core,
                "прежнего принятого конфига нет — поднимать ядро нечем"
            );
            return;
        }
        logging!(
            warn,
            Type::Core,
            "новая сборка не поднялась, ядра нет — поднимаю ядро на прежнем принятом конфиге"
        );
        match self.start_core().await {
            Ok(()) => logging!(info, Type::Core, "ядро снова работает на прежнем конфиге"),
            Err(err) => logging!(
                error,
                Type::Core,
                "прежний конфиг тоже не поднялся — причина не в сборке: {err:#}"
            ),
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
            StageAttempt::Answered(service::StageRequest::Unbuildable(message)) => {
                logging!(
                    warn,
                    Type::Core,
                    "the runtime bundle cannot be built for the service ({message}); leaving the core as it is"
                );
                StagedPath::Unbuildable(message)
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

/// clod:Э3-03 — перезагрузка ушла не тому ядру, которому предназначался путь.
///
/// Sidecar и служба читают конфиг из разных мест: наш файл — у sidecar,
/// поколение службы — у ядра под службой. Если за время применения ядро
/// сменило режим, удачная перезагрузка ничего не доказывает. Ядро, которого
/// уже нет, — не смена рук: сторож поднимет его из свежего файла. А ядро,
/// поднявшееся своим процессом там, где его не было, наш путь читает как надо.
const fn the_core_changed_hands(seen: &super::RunningMode, now: &super::RunningMode) -> bool {
    use super::RunningMode::{NotRunning, Service, Sidecar};
    match (seen, now) {
        (_, NotRunning) => false,
        (Sidecar, Sidecar) | (Service, Service) | (NotRunning, Sidecar) => false,
        (Sidecar, Service) | (Service, Sidecar) | (NotRunning, Service) => true,
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

/// Последний объявленный набор отброшенных ключей — переживает перезапуск,
/// иначе один и тот же merge давал бы предупреждение при каждом старте.
const DISCARDED_KEYS_FILE: &str = "discarded-keys.yaml";

/// clod:tun-owned-keys — цепочка merge или script записала ключ, которым
/// владеет приложение (плоскость управления, свои поля `tun`), и запись
/// отброшена. Говорится один раз на набор ключей: каждая пересборка с тем же
/// набором молчит, новый набор — новое уведомление, и набор помнится на диске.
/// Только для принятой сборки: кандидат, отвергнутый ядром, до человека не доехал.
async fn announce_discarded_keys(discarded: &[String]) {
    static LAST_ANNOUNCED: std::sync::LazyLock<tokio::sync::Mutex<Option<Vec<String>>>> =
        std::sync::LazyLock::new(|| tokio::sync::Mutex::new(None));
    let path = dirs::app_home_dir().ok().map(|dir| dir.join(DISCARDED_KEYS_FILE));
    let mut last = LAST_ANNOUNCED.lock().await;
    if last.is_none() {
        *last = Some(match &path {
            Some(path) => help::read_yaml(path).await.unwrap_or_default(),
            None => Vec::new(),
        });
    }
    let known = last.get_or_insert_with(Vec::new);
    if known.as_slice() == discarded {
        return;
    }
    *known = discarded.to_vec();
    if let Some(path) = &path
        && let Err(err) = help::save_yaml(
            path,
            &*known,
            Some("# clod: набор ключей merge/script, о котором уже сказано"),
        )
        .await
    {
        logging!(warn, Type::Config, "failed to remember the discarded keys: {err:#}");
    }
    drop(last);
    if discarded.is_empty() {
        return;
    }
    logging!(
        warn,
        Type::Config,
        "merge/script wrote keys the app manages; the writes were discarded: {}",
        discarded.join(", ")
    );
    handle::Handle::notice_message("clod_config::keys_discarded", discarded.join(", "));
}

/// Снять с профиля сборки пометку «скачано, но не применено».
///
/// Ставит её приём подписки после отказа ядра (`feat/profile.rs`), а снимать её
/// надо всюду, где ядро конфиг приняло: не только после удачного обновления
/// подписки, но и после переключения профиля, ручной пересборки конфига и правки
/// своих цепочек. Единственное такое место на всех путях — вот это, сразу за
/// успешным применением; профиль — тот, из которого собрано (`IRuntime::profile_uid`).
///
/// Реестр трогаем, только если пометка действительно стоит: иначе на каждое
/// применение конфига приходилась бы лишняя запись `profiles.yaml`.
async fn forget_the_not_applied_mark(profile_uid: Option<&String>) {
    let Some(uid) = profile_uid else { return };
    let marked = Config::profiles()
        .await
        .data_arc()
        .get_item(uid)
        .ok()
        .and_then(|item| item.not_applied)
        .unwrap_or(false);
    if !marked {
        return;
    }

    if let Err(err) = crate::config::profiles::profiles_mark_not_applied(uid, false).await {
        logging!(
            warn,
            Type::Config,
            "Warning: не удалось снять пометку о непринятом профиле: {err}"
        );
    } else {
        handle::Handle::refresh_profiles();
    }
}

const PROVIDERS_LIST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// У ядра нет пустых http-провайдеров — ни прокси, ни правил. Не ответило — считаем, что есть.
async fn providers_filled() -> bool {
    use tauri_plugin_mihomo::models::VehicleType;
    let core = crate::feat::environment::detached_core_client();
    let listed = tokio::time::timeout(PROVIDERS_LIST_TIMEOUT, async {
        tokio::try_join!(core.get_proxy_providers(), core.get_rule_providers())
    })
    .await;
    let Ok(Ok((proxies, rules))) = listed else {
        return false;
    };
    proxies
        .providers
        .values()
        .all(|provider| !provider.proxies.is_empty() || !matches!(provider.vehicle_type, VehicleType::HTTP))
        && rules
            .providers
            .values()
            .all(|provider| provider.rule_count > 0 || !matches!(provider.vehicle_type, VehicleType::HTTP))
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
    /// Бандл нельзя собрать из-за содержимого конфига (провайдерные секции):
    /// это приговор конфигу, а не среде.
    Unbuildable(std::string::String),
}

/// Ядро конфиг приняло, а записать его источник в свой слой не удалось:
/// ядро работает на новом, слой остался на прежнем.
#[derive(Debug)]
pub struct CommitFailed(pub anyhow::Error);

impl std::fmt::Display for CommitFailed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ядро приняло конфиг, но записать его источник не удалось: {:#}",
            self.0
        )
    }
}

impl std::error::Error for CommitFailed {}

#[derive(Debug)]
pub(crate) struct ServiceRefusedTheBundle(pub(crate) std::string::String);

impl std::fmt::Display for ServiceRefusedTheBundle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ServiceRefusedTheBundle {}

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
/// clod:port-ladder — адрес системного прокси обязан совпадать с портом,
/// который ядро подтвердило. Если порт не менялся, запись сама себя
/// пропускает; если прокси выключен, не трогаем ничего.
pub(super) async fn point_system_proxy_at_the_core() {
    if !Config::verge().await.latest_arc().enable_system_proxy.unwrap_or(false) {
        return;
    }
    let sysopt = crate::core::sysopt::Sysopt::global();
    let was_failing = sysopt.write_failed();
    let written = sysopt.update_sysproxy().await;
    sysopt.refresh_guard().await;
    handle::Handle::refresh_verge();
    if let Err(err) = written {
        // Прокси остался на прежнем порту, которого у ядра больше нет:
        // молчать здесь значит оставить человека без интернета и без
        // объяснения. Повторять тост на каждую попытку не нужно — как и у
        // сторожа окружения, говорим один раз, пока запись не заработает.
        logging!(
            warn,
            Type::Core,
            "[clod] failed to point the system proxy at the core's port: {err}"
        );
        if !was_failing {
            handle::Handle::notice_message("sysproxy::write_failed", err.to_string());
        }
    }
}

fn listeners_need_recreate(prev: Option<&serde_yaml_ng::Mapping>, next: Option<&serde_yaml_ng::Mapping>) -> bool {
    let (Some(prev), Some(next)) = (prev, next) else {
        return true;
    };
    LISTENER_KEYS.iter().any(|key| prev.get(*key) != next.get(*key))
}

#[cfg(test)]
mod tests {
    use super::{StageAttempt, listeners_need_recreate, stage_with_confirmation, the_core_changed_hands};
    use crate::core::manager::CoreManager;
    use crate::core::manager::RunningMode::{NotRunning, Service, Sidecar};

    #[tokio::test]
    async fn a_waiting_claim_gets_the_flag_once_the_holder_is_done() {
        let manager = std::sync::Arc::new(CoreManager::default());
        let held = manager.claim_config_update();
        assert!(held.is_some());
        assert!(
            manager.claim_config_update().is_none(),
            "второй захват без ожидания — занято"
        );

        let waiter = {
            let manager = std::sync::Arc::clone(&manager);
            tokio::spawn(async move {
                manager
                    .claim_config_update_within(Duration::from_secs(5))
                    .await
                    .is_some()
            })
        };
        tokio::time::sleep(Duration::from_millis(50)).await;
        drop(held);
        assert!(
            waiter.await.unwrap_or(false),
            "ожидающий должен получить признак после освобождения, а не «занято»"
        );

        let held = manager.claim_config_update();
        assert!(held.is_some());
        assert!(
            manager
                .claim_config_update_within(Duration::from_millis(50))
                .await
                .is_none(),
            "не дождался — честное «занято»"
        );
    }
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

    #[test]
    fn a_reload_that_went_to_a_core_in_another_mode_is_redone_by_a_restart() {
        // Sidecar и служба читают конфиг из разных мест — перезагрузка не в тот
        // режим ничего не доказывает.
        assert!(the_core_changed_hands(&Sidecar, &Service));
        assert!(the_core_changed_hands(&Service, &Sidecar));
        // Наш путь ушёл ядру, которое тем временем поднялось под службой.
        assert!(the_core_changed_hands(&NotRunning, &Service));
    }

    #[test]
    fn a_reload_into_the_same_mode_or_into_no_core_is_left_alone() {
        assert!(!the_core_changed_hands(&Sidecar, &Sidecar));
        assert!(!the_core_changed_hands(&Service, &Service));
        // Ядро умерло после перезагрузки: сторож поднимет его из свежего файла.
        assert!(!the_core_changed_hands(&Sidecar, &NotRunning));
        assert!(!the_core_changed_hands(&Service, &NotRunning));
        // Ядро своим процессом там, где его не было, читает наш путь как надо.
        assert!(!the_core_changed_hands(&NotRunning, &Sidecar));
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
