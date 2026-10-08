use super::CoreManager;
use crate::{
    config::{Config, ConfigType, runtime::IRuntime},
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
use std::sync::atomic::Ordering;
use tauri_plugin_mihomo::Error as MihomoError;

/// Как отдать ядру проверенный конфиг.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivery {
    /// Мягкая перезагрузка (`PUT /configs`); не прошла — перезапуск. Ядра нет —
    /// сразу старт.
    Reload,
    /// Сразу перезапуск — когда вызывающий заранее знает, что мягко нельзя
    /// (смена сборки ядра). Любую смену контроллера — включение, адрес, секрет,
    /// CORS — доставка распознаёт по собранному конфигу сама.
    Restart,
}

/// Место в очереди применения конфига; освобождается при выходе из области на
/// любом пути.
pub(crate) struct ConfigUpdateGuard<'a>(&'a CoreManager);

impl<'a> ConfigUpdateGuard<'a> {
    /// Разрешение очереди переходит гварду: вернёт его `Drop`.
    fn holding(manager: &'a CoreManager, permit: tokio::sync::SemaphorePermit<'_>) -> Self {
        permit.forget();
        Self(manager)
    }
}

impl Drop for ConfigUpdateGuard<'_> {
    fn drop(&mut self) {
        self.0.config_update.add_permits(1);
    }
}

/// Что стало с ядром после доставки, которую оно приняло.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivered {
    /// Мягко перечитало конфиг (`PUT /configs`, в т.ч. из поколения службы):
    /// группы пересобраны, выбор узлов не возвращён.
    Reloaded,
    /// Перезапущено (или поднято) под сборку: выбор узлов вернул сам запуск.
    Restarted,
    /// Сборка совпала с той, на которой ядро уже работает: ядро не трогали.
    Unchanged,
}

/// Исход доставки: что стало с ядром, или почему сборка до него не доехала.
pub type Applied = std::result::Result<Delivered, ValidationOutcome>;

impl Delivered {
    /// Возвращать ли выбор узлов. Перезапуск вернул его сам; ядро, которое не
    /// трогали, держит прежний — кроме случая, когда последний возврат выбора
    /// сдался, не дождавшись наполнения групп (`groups_were_left_filling`).
    const fn brings_selection_back(self, groups_were_left_filling: bool) -> bool {
        match self {
            Self::Reloaded => true,
            Self::Restarted => false,
            Self::Unchanged => groups_were_left_filling,
        }
    }

    /// Единственное место, где после доставки возвращается выбор узлов.
    pub fn restore_selection(self) -> Result<()> {
        let groups_were_left_filling = self == Self::Unchanged && crate::config::profiles::take_groups_left_filling();
        if self.brings_selection_back(groups_were_left_filling) {
            crate::config::profiles::activate_selected_nodes()
        } else {
            Ok(())
        }
    }
}

/// Сравнивать ли сборку с работающей до проверки ядром.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IfUnchanged {
    /// Проверить и доставить в любом случае.
    Deliver,
    /// Совпала с тем, на чём ядро уже работает, — не проверять и не трогать ядро.
    Skip,
}

/// На каком слове ядра стоит сборка.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verdict {
    /// Ядро проверило её (`mihomo -t`) и не отвергло.
    Checked,
    /// Ядро уже работает ровно на ней: проверять и доставлять нечего.
    AlreadyRunning,
}

/// Сборка, которую ядро проверило (`mihomo -t`) и не отвергло — или на которой
/// оно уже работает.
///
/// Единственный путь к слоту рантайма: пока `Staged` жив, он держит очередь
/// применения, другой сборке в слот не попасть. Слот заменяется в `deliver` —
/// только после того, как ядро приняло конфиг; отказ ничего не меняет, откатывать
/// нечего. Брошенный `Staged` — проверка без доставки (подписка не текущая).
#[must_use = "a staged build changes nothing until it is delivered"]
pub struct Staged<'a> {
    manager: &'a CoreManager,
    build: IRuntime,
    verdict: Verdict,
    _guard: ConfigUpdateGuard<'a>,
}

impl Staged<'_> {
    /// Отдать ядру. `Ok(Ok(_))` — ядро работает с этой сборкой и слот заменён;
    /// `Ok(Err(_))` — служба отвергла бандл, ядро осталось на прежнем;
    /// `Err` — доставка сорвалась (ядро о содержимом ничего не сказало).
    pub async fn deliver(self, delivery: Delivery) -> Result<Applied> {
        self.deliver_committing(delivery, async || Ok(())).await
    }

    /// То же, но после приёма ядром — ещё в своей очереди — выполнить `commit`:
    /// записать в свой слой то, из чего собиралось. Иначе между освобождением
    /// очереди и записью чужая сборка читала бы прежнее принятое
    /// и откатывала бы ядру то, что оно только что приняло. Отказ записи — `Err`
    /// с пометкой, что ядро конфиг уже приняло.
    pub async fn deliver_committing(
        self,
        delivery: Delivery,
        commit: impl AsyncFnOnce() -> Result<()>,
    ) -> Result<Applied> {
        let Self {
            manager,
            build,
            verdict,
            _guard,
        } = self;
        let applied = match verdict {
            // Перезагрузка тем же конфигом стёрла бы историю задержек и заново
            // проверила бы все авто-группы, ничего не поменяв.
            Verdict::AlreadyRunning => {
                manager.accept_without_the_core(build).await;
                logging!(info, Type::Core, "Runtime config unchanged, core reload skipped");
                Ok(Delivered::Unchanged)
            }
            Verdict::Checked => manager.deliver_build(build, delivery).await?,
        };
        commit_after(applied, commit).await
    }
}

/// Записать источник (`commit`) — только если ядро сборку приняло, и один раз.
/// Отказ записи — `Err` с пометкой, что ядро конфиг уже приняло.
async fn commit_after(applied: Applied, commit: impl AsyncFnOnce() -> Result<()>) -> Result<Applied> {
    if let Ok(delivered) = applied {
        commit().await.map_err(|error| CommitFailed(error, delivered))?;
    }
    Ok(applied)
}

impl CoreManager {
    /// Занять очередь, если она свободна прямо сейчас; `None` — идёт другое
    /// применение. Только для фоновых дел, которые и так повторяют себя сами.
    pub(crate) fn claim_config_update(&self) -> Option<ConfigUpdateGuard<'_>> {
        let permit = self.config_update.try_acquire().ok()?;
        Some(ConfigUpdateGuard::holding(self, permit))
    }

    /// Встать в очередь применения конфига и дождаться своей очереди. Очередь
    /// честная: кто раньше встал, тот раньше и применяет. `None` — очередь
    /// закрыта (этого не бывает: закрывать её некому).
    pub(crate) async fn queue_for_config_update(&self) -> Option<ConfigUpdateGuard<'_>> {
        let permit = self.config_update.acquire().await.ok()?;
        Some(ConfigUpdateGuard::holding(self, permit))
    }

    /// Собрать конфиг из источников (принятое читается уже в своей очереди) и
    /// проверить его ядром. Очередь держится до конца доставки.
    pub async fn stage_with(&self, sources: Sources) -> Result<std::result::Result<Staged<'_>, ValidationOutcome>> {
        self.stage(sources, IfUnchanged::Deliver).await
    }

    /// Как `stage_with`, но сборку, на которой ядро уже работает, не проверять:
    /// её доставка ядро не тронет (`Delivered::Unchanged`).
    pub async fn stage_unless_unchanged(
        &self,
        sources: Sources,
    ) -> Result<std::result::Result<Staged<'_>, ValidationOutcome>> {
        self.stage(sources, IfUnchanged::Skip).await
    }

    async fn stage(
        &self,
        sources: Sources,
        if_unchanged: IfUnchanged,
    ) -> Result<std::result::Result<Staged<'_>, ValidationOutcome>> {
        let Some(guard) = self.claim_for_an_update().await else {
            return Ok(Err(self.why_not_now()));
        };
        self.build_and_stage(guard, sources, if_unchanged).await
    }

    /// Как `stage_with`, но место в очереди уже взято: вызывающему нужно что-то
    /// сделать в своей очереди до сборки (смена ядра кладёт выбор в черновик).
    /// Проверка идёт всегда: новое ядро обязано проверить конфиг само.
    pub(crate) async fn stage_in_turn<'a>(
        &'a self,
        guard: ConfigUpdateGuard<'a>,
        sources: Sources,
    ) -> Result<std::result::Result<Staged<'a>, ValidationOutcome>> {
        self.build_and_stage(guard, sources, IfUnchanged::Deliver).await
    }

    async fn build_and_stage<'a>(
        &'a self,
        guard: ConfigUpdateGuard<'a>,
        sources: Sources,
        if_unchanged: IfUnchanged,
    ) -> Result<std::result::Result<Staged<'a>, ValidationOutcome>> {
        let build = match Config::build(sources).await {
            Ok(build) => build,
            Err(err) => return Ok(Err(ValidationOutcome::invalid_from_message(err.to_string()))),
        };
        self.stage_under(guard, build, if_unchanged).await
    }

    /// Очередь для применения — не во время выхода.
    pub(crate) async fn claim_for_an_update(&self) -> Option<ConfigUpdateGuard<'_>> {
        if handle::Handle::global().is_exiting() {
            return None;
        }
        let guard = self.queue_for_config_update().await?;
        // Выход мог начаться, пока ждали.
        if handle::Handle::global().is_exiting() {
            return None;
        }
        Some(guard)
    }

    /// Применение не состоялось: единственная причина — идёт выход.
    pub(crate) const fn why_not_now(&self) -> ValidationOutcome {
        ValidationOutcome::Skipped {
            reason: ValidationSkipReason::Exiting,
        }
    }

    /// Единственное место решения «сборка не изменилась» — до проверки ядром:
    /// то, на чём ядро уже работает, проверять незачем.
    async fn stage_under<'a>(
        &'a self,
        guard: ConfigUpdateGuard<'a>,
        build: IRuntime,
        if_unchanged: IfUnchanged,
    ) -> Result<std::result::Result<Staged<'a>, ValidationOutcome>> {
        let Some(config) = build.config.as_ref() else {
            return Ok(Err(ValidationOutcome::invalid_from_message("собранный конфиг пуст")));
        };
        let verdict = if if_unchanged == IfUnchanged::Skip && self.same_as_running(&build).await {
            logging!(debug, Type::Core, "Runtime config unchanged, check skipped");
            Verdict::AlreadyRunning
        } else {
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
            Verdict::Checked
        };
        Ok(Ok(Staged {
            manager: self,
            build,
            verdict,
            _guard: guard,
        }))
    }

    /// Пересобрать и отдать ядру, даже если сборка совпала с работающей.
    pub async fn update_config_forced(&self) -> Result<Applied> {
        self.update_config(IfUnchanged::Deliver).await
    }

    /// Пересобрать; совпала с той, на которой ядро уже работает, — ядро не трогать.
    pub async fn update_config_unless_unchanged(&self) -> Result<Applied> {
        self.update_config(IfUnchanged::Skip).await
    }

    /// Пересобрать из переданных источников, отдать ядру и — ещё в своей очереди
    /// — записать принятое в свой слой (`commit`).
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
        match staged.deliver_committing(delivery, commit).await? {
            Ok(_) => Ok(()),
            Err(outcome) => Err(anyhow!("{outcome}")),
        }
    }

    async fn update_config(&self, if_unchanged: IfUnchanged) -> Result<Applied> {
        match self.stage(Sources::default(), if_unchanged).await? {
            Ok(staged) => staged.deliver(Delivery::Reload).await,
            Err(outcome) => Ok(Err(outcome)),
        }
    }

    pub async fn update_config_checked(&self) -> Result<()> {
        match self.update_config_forced().await? {
            Ok(_) => Ok(()),
            Err(outcome) => Err(anyhow!("{outcome}")),
        }
    }

    /// Собранный кандидат совпал с принятым конфигом, и ядро с ним работает.
    /// Перезагрузка тем же конфигом что-то дала бы только двум случаям: остановленное
    /// ядро она поднимала бы, а пустой http-провайдер узлов или правил (первая
    /// загрузка не удалась, кэша нет) — скачивала заново. Их не пропускаем.
    async fn same_as_running(&self, build: &IRuntime) -> bool {
        let same = matches_the_running(build, &Config::runtime().await.data_arc(), &self.get_running_mode());
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
    pub(crate) async fn update_runtime_config<F>(&self, f: F) -> Result<Applied>
    where
        F: FnOnce(&mut IRuntime),
    {
        let Some(guard) = self.claim_for_an_update().await else {
            return Ok(Err(self.why_not_now()));
        };

        let mut build = (**Config::runtime().await.data_arc()).clone();
        f(&mut build);
        match self.stage_under(guard, build, IfUnchanged::Deliver).await? {
            Ok(staged) => staged.deliver(Delivery::Reload).await,
            Err(outcome) => Ok(Err(outcome)),
        }
    }

    async fn deliver_build(&self, build: IRuntime, delivery: Delivery) -> Result<Applied> {
        let Some(config) = build.config.as_ref() else {
            return Ok(Err(ValidationOutcome::invalid_from_message("собранный конфиг пуст")));
        };
        // clod:port-ladder — порт мог приехать из подписки: системный
        // прокси и PAC указывают на него, и после смены их надо
        // переписать, каким бы путём конфиг ни доехал до ядра.
        let (
            mixed_port_changed,
            mode_changed,
            sharing_changed,
            controller_needs_restart,
            subscription_changed,
            nodes_may_change,
        ) = {
            let prev = Config::runtime().await.data_arc();
            let changed = |key: &str| prev.config.as_ref().and_then(|config| config.get(key)) != config.get(key);
            (
                changed("mixed-port"),
                changed("mode"),
                changed("allow-lan"),
                controller_changed(prev.config.as_ref(), config),
                // Поводы для проверки 16–20: другая подписка; её узлы или
                // провайдеры, другое ядро. Туннель, DNS и прочие настройки — нет.
                prev.profile_uid != build.profile_uid,
                changed("proxies") || changed("proxy-providers") || self.core_switch.load(Ordering::Acquire),
            )
        };
        let delivery = if controller_needs_restart {
            Delivery::Restart
        } else {
            delivery
        };
        let profile_uid = build.profile_uid.clone();
        let delivered = match self.apply_config(build, delivery).await {
            Ok(delivered) => delivered,
            Err(error) => {
                if let Some(refused) = error.downcast_ref::<ServiceRefusedTheBundle>() {
                    return Ok(Err(ValidationOutcome::invalid(
                        ValidationErrorKind::CoreRejected,
                        refused.0.clone(),
                    )));
                }
                return Err(error);
            }
        };
        forget_the_not_applied_mark(profile_uid.as_ref()).await;
        if subscription_changed {
            crate::module::freeze_check::subscription_changed();
        } else if nodes_may_change {
            crate::module::freeze_check::nodes_changed();
        }
        if mixed_port_changed || sharing_changed {
            Self::spawn_mixed_port_check(true);
        }
        if mode_changed {
            crate::process::AsyncHandler::spawn(|| async {
                let _ = crate::core::tray::Tray::global().update_menu().await;
            });
        }
        Ok(Ok(delivered))
    }

    async fn apply_config(&self, build: IRuntime, delivery: Delivery) -> Result<Delivered> {
        // Ядра нет — перезагружать нечего, сразу старт с новой сборкой: рабочий
        // файл ядра пишет сам старт, из слота.
        if delivery == Delivery::Restart || matches!(*self.get_running_mode(), super::RunningMode::NotRunning) {
            return self.replace_core_and_apply(build).await;
        }
        let config = build.config.as_ref().ok_or_else(|| anyhow!("собранный конфиг пуст"))?;
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
            match self.stage_into_service_generation(config).await {
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
            // Ядро своим процессом читает наш файл — и при перезагрузке, и при
            // самоперезапуске.
            dirs::path_to_str(&Config::write_config_file(ConfigType::Run, config).await?)?.into()
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
                // Новый процесс не поднимался — хвост принятого конфига здесь,
                // а не в `new_core_is_up`: подмена DNS (macOS), снятие ненужного TUN.
                #[cfg(target_os = "macos")]
                crate::utils::resolve::dns::apply_remembered_desire();
                crate::process::AsyncHandler::spawn(|| async { crate::feat::tun::enforce_undesired_off().await });
                Ok(Delivered::Reloaded)
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
        crate::core::handle::Handle::mihomo().reload_config(force, path).await
    }

    /// Полный перезапуск ядра под новую сборку.
    ///
    /// Старт пишет файл ядра из слота, поэтому сборка ставится в слот до
    /// перезапуска. Не поднялось — слот возвращается к прежнему принятому, и
    /// если ядра при этом не осталось, оно поднимается на прежнем: отказ
    /// новой сборки не должен оставлять человека без интернета с системным
    /// прокси на мёртвом порту (Э3-07).
    async fn replace_core_and_apply(&self, build: IRuntime) -> Result<Delivered> {
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
                Ok(Delivered::Restarted)
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
                if !self.core_switch.load(Ordering::Acquire) {
                    self.bring_back_the_previous_core().await;
                }
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
    async fn stage_into_service_generation(&self, config: &serde_yaml_ng::Mapping) -> StagedPath {
        use crate::core::service;

        if !service::active_service_supports_runtime_staging() {
            return StagedPath::NotStaged;
        }

        let attempt = stage_with_confirmation(crate::constants::timing::STAGE_CONFIRM_TIMEOUT, || async {
            match service::stage_runtime_by_service(config).await {
                Ok(request) => StageAttempt::Answered(request),
                Err(error) => StageAttempt::Unanswered(format!("{error:#}")),
            }
        })
        .await;

        match attempt {
            StageAttempt::Answered(service::StageRequest::Answered(StageRuntimeOutcome::Staged { config_path })) => {
                // Ядро службы наш файл не читает; на диске — то, что оно получит.
                // Не записался — доставке это не мешает.
                if let Err(err) = Config::write_config_file(ConfigType::Run, config).await {
                    logging!(warn, Type::Core, "failed to write the runtime config file: {err:#}");
                }
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

/// Кандидат — ровно то, на чём ядро уже работает: ядро живо, а слот держит
/// тот же конфиг той же подписки. Подписка сверяется отдельно: совпавший конфиг
/// другой подписки (обновление нетекущей) ядро не проверяло.
fn matches_the_running(build: &IRuntime, running: &IRuntime, mode: &super::RunningMode) -> bool {
    !matches!(mode, super::RunningMode::NotRunning)
        && build.config.is_some()
        && build.config == running.config
        && build.profile_uid == running.profile_uid
}

const PROVIDERS_LIST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// У ядра нет пустых http-провайдеров — ни прокси, ни правил. Не ответило — считаем, что есть.
async fn providers_filled() -> bool {
    use tauri_plugin_mihomo::models::VehicleType;
    let core = crate::core::handle::Handle::mihomo();
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
/// ядро работает на новом, слой остался на прежнем. Второе поле — что стало
/// с ядром при доставке.
#[derive(Debug)]
pub struct CommitFailed(pub anyhow::Error, pub Delivered);

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

/// Контроллер ядро поднимает только при старте: смену адреса, а у слушающего
/// контроллера — и секрета с CORS мягкая перезагрузка не применит. Выключенному
/// контроллеру сборка адрес не отдаёт, и его секрет ядру не нужен.
fn controller_changed(prev: Option<&serde_yaml_ng::Mapping>, next: &serde_yaml_ng::Mapping) -> bool {
    let changed = |key: &str| prev.and_then(|config| config.get(key)) != next.get(key);
    let listening = next
        .get("external-controller")
        .and_then(serde_yaml_ng::Value::as_str)
        .is_some_and(|address| !address.is_empty());
    changed("external-controller") || (listening && (changed("secret") || changed("external-controller-cors")))
}

fn listeners_need_recreate(prev: Option<&serde_yaml_ng::Mapping>, next: Option<&serde_yaml_ng::Mapping>) -> bool {
    let (Some(prev), Some(next)) = (prev, next) else {
        return true;
    };
    LISTENER_KEYS.iter().any(|key| prev.get(*key) != next.get(*key))
}

#[cfg(test)]
mod tests {
    use super::{
        Delivered, StageAttempt, controller_changed, listeners_need_recreate, matches_the_running,
        stage_with_confirmation, the_core_changed_hands,
    };
    use crate::core::manager::CoreManager;
    use crate::core::manager::RunningMode::{NotRunning, Service, Sidecar};

    #[tokio::test]
    async fn an_action_waits_its_turn_however_long_the_holder_takes() {
        let manager = std::sync::Arc::new(CoreManager::default());
        let held = manager.claim_config_update();
        assert!(held.is_some());
        assert!(manager.is_config_update_in_progress());
        assert!(
            manager.claim_config_update().is_none(),
            "фоновое дело без ожидания видит занятую очередь"
        );

        let waiter = {
            let manager = std::sync::Arc::clone(&manager);
            tokio::spawn(async move { manager.queue_for_config_update().await.is_some() })
        };
        // Срока у ожидания нет: пока держатель работает (перезапуск ядра под
        // применением идёт десятки секунд), ожидающий стоит в очереди, а не
        // получает «занято».
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(!waiter.is_finished(), "очередь не пропускает вперёд держателя");
        drop(held);
        assert!(waiter.await.unwrap_or(false), "ожидающий получает свою очередь");
        assert!(!manager.is_config_update_in_progress());
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

    fn runtime(config: Option<&str>, profile_uid: Option<&str>) -> crate::config::runtime::IRuntime {
        crate::config::runtime::IRuntime {
            config: config.map(mapping),
            profile_uid: profile_uid.map(Into::into),
            ..Default::default()
        }
    }

    #[test]
    fn only_the_build_the_core_already_runs_skips_the_check() {
        let running = runtime(Some("{mixed-port: 7890, proxies: [a]}"), Some("sub-a"));
        let same = runtime(Some("{mixed-port: 7890, proxies: [a]}"), Some("sub-a"));
        assert!(matches_the_running(&same, &running, &Sidecar));
        assert!(matches_the_running(&same, &running, &Service));
        // Ядра нет — доставка его поднимет.
        assert!(!matches_the_running(&same, &running, &NotRunning));
        // Тот же конфиг другой подписки (обновление нетекущей): ядро его не проверяло.
        let other = runtime(Some("{mixed-port: 7890, proxies: [a]}"), Some("sub-b"));
        assert!(!matches_the_running(&other, &running, &Sidecar));
        // Слот пуст (отказ старта) или сборка пуста.
        assert!(!matches_the_running(&same, &runtime(None, Some("sub-a")), &Sidecar));
        assert!(!matches_the_running(
            &runtime(None, Some("sub-a")),
            &runtime(None, Some("sub-a")),
            &Sidecar
        ));
        let changed = runtime(Some("{mixed-port: 7890, proxies: [a, b]}"), Some("sub-a"));
        assert!(!matches_the_running(&changed, &running, &Sidecar));
    }

    #[test]
    fn the_selection_comes_back_only_where_nothing_brought_it_back() {
        // Мягкая перезагрузка пересобрала группы — выбор возвращает доставка.
        assert!(Delivered::Reloaded.brings_selection_back(false));
        // Перезапуск вернул его сам — вторая волна стёрла бы ручной выбор.
        assert!(!Delivered::Restarted.brings_selection_back(false));
        assert!(!Delivered::Restarted.brings_selection_back(true));
        // Ядро не трогали — выбор на месте, если последний возврат дождался групп.
        assert!(!Delivered::Unchanged.brings_selection_back(false));
        assert!(Delivered::Unchanged.brings_selection_back(true));
    }

    fn production(source: &'static str) -> &'static str {
        crate::utils::source_scan::production_code(source)
    }

    fn body_of(source: &'static str, signature: &str) -> &'static str {
        let body = crate::utils::source_scan::fn_body(production(source), signature).unwrap_or_default();
        assert!(!body.is_empty(), "тело {signature} не найдено — тест ослеп");
        body
    }

    #[test]
    fn an_unchanged_build_is_decided_before_the_core_checks_it() {
        let stage_under = body_of(include_str!("config.rs"), "async fn stage_under");
        assert!(
            matches!(
                (stage_under.find("same_as_running"), stage_under.find("validate_config_outcome_with")),
                (Some(compare), Some(check)) if compare < check
            ),
            "сравнение с работающим должно стоять до проверки ядром"
        );
        assert!(
            !production(include_str!("config.rs")).contains("deliver_unless_unchanged"),
            "решение «не изменилось» — только в stage_under"
        );
    }

    #[test]
    fn every_restart_of_the_core_reports_itself_as_one() {
        let apply = body_of(include_str!("config.rs"), "async fn apply_config");
        assert_eq!(
            apply.matches("Delivered::").count(),
            1,
            "у apply_config один свой исход — удачная мягкая перезагрузка"
        );
        assert!(apply.contains("Ok(Delivered::Reloaded)"));
        let replace = body_of(include_str!("config.rs"), "async fn replace_core_and_apply");
        assert!(replace.contains("Ok(Delivered::Restarted)"));
        assert!(!replace.contains("Delivered::Reloaded") && !replace.contains("Delivered::Unchanged"));
    }

    #[test]
    fn the_runtime_file_is_written_where_it_is_read() {
        let deliver = body_of(include_str!("config.rs"), "async fn deliver_build");
        assert!(!deliver.contains("write_config_file"), "доставка не пишет файл заранее");
        let apply = body_of(include_str!("config.rs"), "async fn apply_config");
        let write = "write_config_file(ConfigType::Run";
        assert_eq!(apply.matches(write).count(), 1, "{apply}");
        let early_restart = apply
            .find("return self.replace_core_and_apply(build)")
            .unwrap_or(usize::MAX);
        let written = apply.find(write).unwrap_or(usize::MAX);
        let reload = apply.find("self.reload_config(").unwrap_or_default();
        assert!(
            early_restart < written && written < reload,
            "sidecar: запись после раннего перезапуска, до перезагрузки"
        );
        let stage = body_of(include_str!("config.rs"), "async fn stage_into_service_generation");
        let staged = stage.find("StageRuntimeOutcome::Staged").unwrap_or(usize::MAX);
        let written = stage.find(write).unwrap_or_default();
        assert!(
            stage.matches(write).count() == 1 && staged < written,
            "служба: запись только после ответа Staged"
        );
    }

    #[test]
    fn the_tail_of_the_config_follows_only_a_reload() {
        let deliver = body_of(include_str!("config.rs"), "async fn deliver_build");
        let apply = body_of(include_str!("config.rs"), "async fn apply_config");
        for tail in ["apply_remembered_desire()", "enforce_undesired_off()"] {
            assert!(!deliver.contains(tail), "доставка не знает, был ли перезапуск: {tail}");
            assert_eq!(apply.matches(tail).count(), 1, "только удачная перезагрузка: {tail}");
        }
    }

    #[test]
    fn the_callers_restore_the_selection_through_the_delivery() {
        for (file, source) in [
            ("feat/profile.rs", include_str!("../../feat/profile.rs")),
            ("cmd/profile.rs", include_str!("../../cmd/profile.rs")),
        ] {
            assert!(
                !production(source).contains("activate_selected_nodes"),
                "{file}: выбор узлов возвращает Delivered::restore_selection, а не вызывающий"
            );
        }
    }

    #[test]
    fn editing_the_current_card_rebuilds_only_a_changed_config() {
        let command = body_of(include_str!("../../cmd/profile.rs"), "pub async fn enhance_profiles");
        assert!(command.contains("feat::apply_current_profile()"));
        assert!(!command.contains("feat::enhance_profiles()"));
    }

    #[test]
    fn the_flag_of_groups_left_filling_is_set_by_the_restore_itself() {
        let worker = body_of(
            include_str!("../../config/profiles.rs"),
            "async fn activate_selected_nodes_worker",
        );
        let worker: std::string::String = worker.split_whitespace().collect();
        // Ставится до ожидания: ядро, не ответившее списком вовсе, — тоже
        // невозвращённый выбор.
        let set = worker.find("GROUPS_LEFT_FILLING.store(true,");
        let wait = worker.find("fetch_settled_proxies(");
        assert!(
            matches!((set, wait), (Some(set), Some(wait)) if set < wait),
            "признак ставится до ожидания групп: {worker}"
        );
        let cleared =
            "GROUPS_LEFT_FILLING.store(selection_left_undone(left_filling,&plan.activations,&completed_activations";
        assert!(
            matches!(
                (worker.rfind("if!is_activation_current(generation)"), worker.find(cleared)),
                (Some(current), Some(store)) if current < store
            ),
            "снимает признак только текущий возврат, по тому, что он вернул: {worker}"
        );
    }

    #[test]
    fn only_a_controller_that_listens_restarts_the_core_for_its_secret() {
        let off = mapping("external-controller: ''\nsecret: a\n");
        let off_new_secret = mapping("external-controller: ''\nsecret: b\n");
        let on = mapping("external-controller: 127.0.0.1:9097\nsecret: a\n");
        let on_new_secret = mapping("external-controller: 127.0.0.1:9097\nsecret: b\n");
        let on_new_cors =
            mapping("external-controller: 127.0.0.1:9097\nsecret: a\nexternal-controller-cors: {allow-origins: [x]}\n");
        let moved = mapping("external-controller: 127.0.0.1:9098\nsecret: a\n");
        assert!(!controller_changed(Some(&off), &off_new_secret));
        assert!(!controller_changed(Some(&on), &on));
        assert!(controller_changed(Some(&off), &on));
        assert!(controller_changed(Some(&on), &off));
        assert!(controller_changed(Some(&on), &on_new_secret));
        assert!(controller_changed(Some(&on), &on_new_cors));
        assert!(controller_changed(Some(&on), &moved));
    }
}

#[cfg(test)]
mod commit_tests {
    use super::{CommitFailed, Delivered, commit_after};
    use crate::core::validate::ValidationOutcome;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn the_source_is_written_once_and_only_after_the_core_took_the_build() {
        let commits = AtomicUsize::new(0);
        let commit = async || {
            commits.fetch_add(1, Ordering::SeqCst);
            Ok(())
        };
        let applied = commit_after(Ok(Delivered::Reloaded), commit).await;
        assert!(matches!(applied, Ok(Ok(Delivered::Reloaded))), "{applied:?}");
        assert_eq!(commits.load(Ordering::SeqCst), 1);

        let commit = async || {
            commits.fetch_add(1, Ordering::SeqCst);
            Ok(())
        };
        let applied = commit_after(Err(ValidationOutcome::Busy), commit).await;
        assert!(matches!(applied, Ok(Err(ValidationOutcome::Busy))), "{applied:?}");
        assert_eq!(commits.load(Ordering::SeqCst), 1, "отвергнутую сборку не записываем");
    }

    #[tokio::test]
    async fn a_failed_write_says_what_the_core_already_took() {
        let failed = commit_after(Ok(Delivered::Restarted), async || Err(anyhow::anyhow!("disk full"))).await;
        let delivered = failed
            .as_ref()
            .err()
            .and_then(|error| error.downcast_ref::<CommitFailed>())
            .map(|failed| failed.1);
        assert_eq!(delivered, Some(Delivered::Restarted), "{failed:?}");
    }
}
