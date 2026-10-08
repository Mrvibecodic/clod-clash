use super::CmdResult;
use super::StringifyErr as _;
use crate::cmd::validate::{ValidationNoticeTarget, handle_validation_notice};
use crate::config::profiles;
use crate::utils::window_manager::WindowManager;
use crate::{
    config::{
        Config, IProfiles, PrfItem, PrfOption,
        profiles::{
            PendingProfileFiles, profiles_delete_item_safe, profiles_patch_item_safe, profiles_reorder_safe,
            profiles_save_file_safe, profiles_set_secure_safe,
        },
    },
    core::{
        CoreManager, handle,
        manager::{CommitFailed, Delivered, Delivery},
        timer::Timer,
        tray::Tray,
        validate::ValidationOutcome,
    },
    enhance::Sources,
    feat,
    utils::{dirs, help},
};
use clash_verge_draft::{Draft, SharedDraft};
use clash_verge_logging::{Type, logging};
use scopeguard::defer;
use smartstring::alias::String;
use std::sync::atomic::{AtomicBool, Ordering};

static CURRENT_SWITCHING_PROFILE: AtomicBool = AtomicBool::new(false);

pub(crate) fn profile_switch_in_progress() -> bool {
    CURRENT_SWITCHING_PROFILE.load(Ordering::Acquire)
}

fn profile_import_error(err: &anyhow::Error) -> std::string::String {
    if let Some(cause) = err.chain().find(|cause| cause.to_string().contains("TLS 1.0/1.1")) {
        return cause.to_string();
    }

    format!("не удалось импортировать подписку: {err:#}")
}

#[tauri::command]
pub async fn get_profiles() -> CmdResult<SharedDraft<IProfiles>> {
    logging!(debug, Type::Cmd, "получение списка файлов конфига");
    let draft = Config::profiles().await;
    let data = draft.data_arc();
    Ok(data)
}

/// Пересобрать конфиг текущей подписки и отдать ядру, если сборка изменилась.
#[tauri::command]
pub async fn enhance_profiles() -> CmdResult<ValidationOutcome> {
    match feat::apply_current_profile().await {
        Ok(outcome) if outcome.is_valid() => Ok(outcome),
        Ok(outcome) => {
            logging!(
                warn,
                Type::Cmd,
                "Reactivate profiles command failed validation: {}",
                outcome
            );
            handle_validation_notice(&outcome, ValidationNoticeTarget::Runtime, "рабочий конфиг");
            Ok(outcome)
        }
        Err(e) => {
            logging!(error, Type::Cmd, "{}", e);
            Err(super::public_error_text(&e))
        }
    }
}

/// Импорт конфига
#[tauri::command]
pub async fn import_profile(url: std::string::String, option: Option<PrfOption>) -> CmdResult {
    logging!(
        info,
        Type::Cmd,
        "[импорт подписки] начало импорта: {}",
        help::mask_url(&url)
    );

    // Лестница маршрутов со своим бюджетом времени на адрес — та же, что у
    // обновления: заблокированный домен подписки достижим через уже поднятый туннель.
    let item = &mut match PrfItem::from_url_for_new(&url, None, None, option.as_ref()).await {
        Ok(fetched) => {
            logging!(
                info,
                Type::Cmd,
                "[импорт подписки] загрузка завершена, сохранение конфига"
            );
            fetched.item
        }
        Err(e) => {
            logging!(error, Type::Cmd, "[импорт подписки] не удалось загрузить: {}", e);
            return Err(super::public_error_text(&profile_import_error(&e)));
        }
    };

    if let Err(e) = feat::add_profile(item).await {
        logging!(error, Type::Cmd, "[импорт подписки] не удалось сохранить конфиг: {}", e);
        return Err(format!("не удалось импортировать подписку: {}", super::public_error_text(&e)).into());
    }
    logging!(info, Type::Cmd, "[импорт подписки] файл конфига сохранён");

    if let Some(uid) = &item.uid {
        logging!(
            info,
            Type::Cmd,
            "[импорт подписки] отправка уведомления об изменении конфига: {}",
            uid
        );
        announce_the_added(uid).await;
    }

    logging!(
        info,
        Type::Cmd,
        "[импорт подписки] импорт завершён: {}",
        help::mask_url(&url)
    );
    Ok(())
}

/// Подписка добавлена: окну — перечитать, окну лимита устройств — отказ панели,
/// ядру — сборку, если она стала текущей.
async fn announce_the_added(uid: &String) {
    handle::Handle::notify_profile_changed(uid);
    crate::feat::announce_device_refusal(uid).await;
    if Config::profiles().await.latest_arc().is_current_profile_index(uid)
        && let Err(err) = enhance_profiles().await
    {
        handle::Handle::notice_message("update_failed", err);
    }
}

/// Изменяет порядок profile
#[tauri::command]
pub async fn reorder_profile(active_id: String, over_id: String) -> CmdResult {
    match profiles_reorder_safe(&active_id, &over_id).await {
        Ok(_) => {
            logging!(info, Type::Cmd, "изменение порядка файлов конфига");
            Ok(())
        }
        Err(err) => {
            logging!(error, Type::Cmd, "не удалось изменить порядок файлов конфига: {}", err);
            Err(format!(
                "не удалось изменить порядок файлов конфига: {}",
                super::public_error_text(&err)
            )
            .into())
        }
    }
}

const LOCAL_PROFILE_MAX_BYTES: u64 = 16 * 1024 * 1024;

#[tauri::command]
pub async fn create_profile_from_file(item: PrfItem, path: String) -> CmdResult {
    let source = std::path::PathBuf::from(path.as_str());
    let is_yaml = source
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("yaml") || ext.eq_ignore_ascii_case("yml"));
    if !is_yaml {
        return Err("only .yaml or .yml files can be imported".into());
    }
    let meta = tokio::fs::metadata(&source).await.stringify_err()?;
    if !meta.is_file() {
        return Err("profile path is not a file".into());
    }
    if meta.len() > LOCAL_PROFILE_MAX_BYTES {
        return Err("profile file is too large".into());
    }
    let data = tokio::fs::read_to_string(&source).await.stringify_err()?;
    create_profile(item, Some(data.into())).await
}

/// Создаёт новый profile
/// Создаёт новый конфиг
#[tauri::command]
pub async fn create_profile(item: PrfItem, file_data: Option<String>) -> CmdResult {
    let added = async {
        let mut created = PrfItem::from(&item, file_data).await?;
        feat::add_profile(&mut created).await?;
        anyhow::Ok(created.uid)
    };
    match added.await {
        Ok(created) => {
            // Отправляем уведомление об изменении конфига
            if let Some(uid) = &created {
                logging!(
                    info,
                    Type::Cmd,
                    "[создание подписки] отправка уведомления об изменении конфига: {}",
                    uid
                );
                announce_the_added(uid).await;
            }
            Ok(())
        }
        Err(err) => match err.to_string().as_str() {
            "the file already exists" => Err("the file already exists".into()),
            _ => Err(format!("add profile error: {}", super::public_error_text(&err)).into()),
        },
    }
}

/// clod:chan — включить или выключить защищённый канал у добавленной
/// подписки. Включение — сначала проба: канала у провайдера нет или он не
/// ответил — ошибка, признак не меняется. Выключение предупреждение показывает
/// окно, здесь только запись.
#[tauri::command]
pub async fn set_secure_channel(index: String, on: bool) -> CmdResult {
    let (url, option) = {
        let profiles = Config::profiles().await.latest_arc();
        let item = profiles.get_item(&index).stringify_err()?;
        if item.itype.as_deref() != Some("remote") {
            return Err("the secure channel needs a remote subscription".into());
        }
        let url = item
            .url
            .clone()
            .ok_or_else(|| String::from("the subscription has no address"))?;
        (url, item.fetch_option())
    };

    let probed = if on {
        Some(
            PrfItem::probe_channel(&url, option.as_ref())
                .await
                .map_err(|err| super::public_error_text(&err))?,
        )
    } else {
        None
    };
    logging!(
        info,
        Type::Cmd,
        "[clod] chan: secure channel {} for {}",
        if on { "on" } else { "off" },
        index
    );

    profiles_set_secure_safe(&index, probed).await.stringify_err()?;
    handle::Handle::refresh_profiles();
    Ok(())
}

/// Обновляет конфиг
#[tauri::command]
pub async fn update_profile(index: String, option: Option<PrfOption>) -> CmdResult {
    // clod:provider-links — `Box::pin`: карточка профиля подросла на ссылки
    // провайдера, и клиппи справедливо не хочет держать такой future на стеке.
    match Box::pin(feat::update_profile(
        &index,
        option.as_ref(),
        true,
        feat::UpdateTrigger::Manual,
    ))
    .await
    {
        Ok(_) => Ok(()),
        Err(e) => {
            logging!(error, Type::Cmd, "{}", e);
            Err(super::public_error_text(&e))
        }
    }
}

/// Какие подписки обновляются сейчас — кем угодно, кнопкой или расписанием, —
/// тем же снимком, что окно получает событием.
#[tauri::command]
pub fn get_updating_profiles() -> feat::UpdatesInFlight {
    feat::updates_in_flight()
}

/// Удаляет конфиг
#[tauri::command]
pub async fn delete_profile(index: String) -> CmdResult {
    // clod: если удаляется текущая подписка, ядру сначала уходит сборка без неё,
    // собранная из кандидата реестра. Реестр (одной записью `profiles.yaml`) и
    // файлы меняются, только когда ядро её приняло: отказ ничего не трогает, и
    // откатывать нечего — пользователь остаётся ровно там, где был.
    let changes_current = {
        let profiles = Config::profiles().await.latest_arc();
        profiles.get_item(&index).stringify_err()?;
        profiles.deleting_changes_current(&index)
    };
    let pending_files = if changes_current {
        deliver_without(&index).await?
    } else if let Some(files) = delete_unless_current(&index).await.stringify_err()? {
        files
    } else {
        // Пока шли сюда, её сделали текущей — тогда как с текущей.
        deliver_without(&index).await?
    };

    if let Err(e) = Tray::global().update_tooltip().await {
        logging!(
            warn,
            Type::Cmd,
            "Warning: не удалось асинхронно обновить подсказку трея: {e}"
        );
    }

    if let Err(e) = Tray::global().update_menu().await {
        logging!(
            warn,
            Type::Cmd,
            "Warning: не удалось асинхронно обновить меню трея: {e}"
        );
    }

    // clod: диск трогаем ПОСЛЕДНИМ — когда клиент уже доказал, что живёт без
    // этой подписки. Логотип провайдера лежит отдельным файлом в `logos/` и
    // раньше не удалялся вовсе, поэтому чистится здесь же.
    pending_files.cleanup().await;
    crate::module::logo_cache::clear(&index).await;

    Timer::global().refresh().await.stringify_err()?;
    drop_system_proxy_without_profiles().await;
    Ok(())
}

/// Удалить не текущую подписку одной записью реестра. `None` — пока шли сюда,
/// её сделали текущей: реестр не тронут, ядру сначала нужна сборка без неё.
async fn delete_unless_current(index: &String) -> anyhow::Result<Option<PendingProfileFiles>> {
    Config::profiles()
        .await
        .with_data_modify(|profiles| delete_unless_current_in(profiles, index))
        .await
}

async fn delete_unless_current_in(
    mut profiles: IProfiles,
    index: &String,
) -> anyhow::Result<(IProfiles, Option<PendingProfileFiles>)> {
    if profiles.deleting_changes_current(index) {
        return Ok((profiles, None));
    }
    let (_, files) = profiles.delete_item(index).await?;
    Ok((profiles, Some(files)))
}

/// Реестр-кандидат без подписки `index` и её цепочек; принятый не трогается.
fn without(accepted: &IProfiles, index: &String) -> anyhow::Result<IProfiles> {
    let mut candidate = accepted.clone();
    candidate.plan_delete_item(index)?;
    Ok(candidate)
}

/// Сборку без подписки не довели до проверки — что с удалением.
#[derive(Debug, PartialEq, Eq)]
enum OnStageRefused {
    /// Применение не состоялось (идёт выход) — удалению это не приговор:
    /// следующая сборка пойдёт уже без удалённой подписки.
    DeleteNow,
    /// Сборку отвергли — подписка остаётся, реестр не тронут.
    Refuse,
}

const fn on_stage_refused(outcome: &ValidationOutcome) -> OnStageRefused {
    match outcome {
        ValidationOutcome::Busy | ValidationOutcome::Skipped { .. } => OnStageRefused::DeleteNow,
        _ => OnStageRefused::Refuse,
    }
}

/// Отдать ядру сборку без подписки `index` и, когда ядро её приняло, — ещё в
/// своей очереди — удалить подписку из реестра.
async fn deliver_without(index: &String) -> CmdResult<PendingProfileFiles> {
    let sources = {
        let index = index.clone();
        Sources::default().with_profiles_derived(move |accepted| without(accepted, &index))
    };
    let staged = match CoreManager::global().stage_with(sources).await {
        Ok(Ok(staged)) => staged,
        Ok(Err(outcome)) => match on_stage_refused(&outcome) {
            OnStageRefused::DeleteNow => {
                logging!(info, Type::Cmd, "[удаление подписки] применение отложено: {}", outcome);
                return Ok(profiles_delete_item_safe(index).await.stringify_err()?.1);
            }
            OnStageRefused::Refuse => return Err(refused_delete(&outcome)),
        },
        Err(e) => {
            logging!(error, Type::Cmd, "{}", e);
            return Err(super::public_error_text(&e));
        }
    };

    let mut pending = None;
    let commit = async || {
        pending = Some(profiles_delete_item_safe(index).await?.1);
        Ok(())
    };
    let delivered = match staged.deliver_committing(Delivery::Reload, commit).await {
        Ok(Ok(delivered)) => delivered,
        Ok(Err(outcome)) => return Err(refused_delete(&outcome)),
        // Ядро уже без этой подписки — реестр обязан догнать: одна повторная запись.
        Err(e) => match e.downcast_ref::<CommitFailed>().map(|failed| failed.1) {
            Some(delivered) => match profiles_delete_item_safe(index).await {
                Ok((_, files)) => {
                    pending = Some(files);
                    delivered
                }
                Err(err) => {
                    let message: String = super::public_error_text(&format!("{e}; повтор: {err:#}"));
                    logging!(error, Type::Cmd, "{message}");
                    return Err(message);
                }
            },
            None => {
                logging!(error, Type::Cmd, "{}", e);
                return Err(super::public_error_text(&e));
            }
        },
    };
    feat::settle_after_delivery(delivered);
    logging!(
        info,
        Type::Cmd,
        "[удаление подписки] отправка уведомления об изменении конфига: {}",
        index
    );
    handle::Handle::notify_profile_changed(index);
    pending.ok_or_else(|| String::from("the profile registry was not updated"))
}

fn refused_delete(outcome: &ValidationOutcome) -> String {
    logging!(
        warn,
        Type::Cmd,
        "не удалось обновить конфиг после удаления подписки: {}",
        outcome
    );
    handle_validation_notice(outcome, ValidationNoticeTarget::Runtime, "рабочий конфиг");
    outcome.to_string().into()
}

/// clod: удалили последнюю подписку — маршрутизировать больше нечего, а
/// системный прокси остался бы прописанным в настройках ОС и уводил бы весь
/// трафик машины в порт, за которым уже нет ни одного сервера. Флаг в конфиге
/// не трогаем: он снова вступит в силу, когда появится подписка.
async fn drop_system_proxy_without_profiles() {
    let profiles = Config::profiles().await.latest_arc();
    let has_profile = profiles
        .current
        .as_ref()
        .is_some_and(|uid| profiles.get_item(uid).is_ok());
    if has_profile {
        return;
    }
    logging!(info, Type::Cmd, "подписок не осталось — снимаем системный прокси");
    if let Err(e) = crate::core::sysopt::Sysopt::global().reset_sysproxy_if_ours().await {
        logging!(warn, Type::Cmd, "не удалось снять системный прокси: {e}");
    }
}

/// Записать выбор профиля в принятое состояние — только после того, как ядро
/// приняло собранный из кандидата конфиг, и ещё под признаком применения
/// (`deliver_committing`): до этого реестр не трогается, а после — чужая сборка
/// уже не прочитает прежний `current`.
async fn commit_current_profile(profiles: &Draft<IProfiles>, current: Option<String>) -> anyhow::Result<()> {
    let Some(current) = current else {
        return Ok(());
    };

    profiles
        .with_data_modify(|mut committed| async move {
            committed.patch_config(&IProfiles {
                current: Some(current),
                items: None,
            });
            Ok((committed, ()))
        })
        .await
}

async fn handle_success(delivered: Delivered, current_value: Option<&String>) -> CmdResult<ValidationOutcome> {
    // Runtime refresh and tray rebuilding happen after saved node selections are restored.
    delivered.restore_selection().stringify_err()?;
    if delivered == Delivered::Restarted {
        // Выбор вернул старт ядра — раньше, чем реестр записал новый профиль, и
        // трей мог показать прежний.
        if let Err(e) = Tray::global().update_tooltip().await {
            logging!(warn, Type::Cmd, "Warning: не удалось обновить подсказку трея: {e}");
        }
        if let Err(e) = Tray::global().update_menu().await {
            logging!(warn, Type::Cmd, "Warning: не удалось обновить меню трея: {e}");
        }
    }

    if let Err(e) = profiles_save_file_safe().await {
        logging!(
            warn,
            Type::Cmd,
            "Warning: не удалось асинхронно сохранить файл конфига: {e}"
        );
    }

    if let Some(current) = current_value
        && WindowManager::get_main_window().is_some()
    {
        logging!(
            info,
            Type::Cmd,
            "отправка фронтенду события изменения конфига: {}",
            current
        );
        handle::Handle::notify_profile_changed(current);
    }

    Ok(ValidationOutcome::Valid)
}

fn handle_validation_failure(outcome: ValidationOutcome) -> ValidationOutcome {
    logging!(warn, Type::Cmd, "не удалось проверить конфиг: {}", outcome);
    handle_validation_notice(&outcome, ValidationNoticeTarget::Runtime, "рабочий конфиг");
    outcome
}

fn handle_update_error<E: std::fmt::Display>(e: E) -> ValidationOutcome {
    logging!(warn, Type::Cmd, "ошибка в процессе обновления: {}", e,);
    let message: String = super::public_error_text(&e);
    // Ядро чаще всего работает на прежнем профиле: это «изменения отменены»,
    // а не отказ старта.
    handle::Handle::notice_message("config_validate::error", message.clone());
    ValidationOutcome::invalid_from_message(message)
}

/// Собрать конфиг из кандидата реестра (`patch` поверх принятого — уже в своей
/// очереди применения, чтобы реестр не уехал из-под кандидата за время ожидания)
/// и отдать ядру. Реестр в памяти и на диске меняется только на успехе
/// (`handle_success`): при отказе ядро остаётся на прежнем профиле, и откатывать
/// нечего.
///
/// Применение идёт отдельной задачей: отменять его посреди перезапуска ядра нельзя
/// — слот и очередь остались бы в промежуточном состоянии. Ответ приходит, когда
/// задача довела дело до конца: время ожидания очереди — не зависание, и потолок
/// на него давал бы ложный «таймаут» при успешном переключении.
async fn perform_config_update(patch: IProfiles, current_value: Option<String>) -> CmdResult<ValidationOutcome> {
    let sources = Sources::default().with_profiles_derived(move |accepted| {
        let mut candidate = accepted.clone();
        candidate.patch_config(&patch);
        Ok(candidate)
    });
    let task = tauri::async_runtime::spawn(async move {
        defer! {
            CURRENT_SWITCHING_PROFILE.store(false, Ordering::Release);
        }
        match CoreManager::global().stage_with(sources).await {
            Ok(Ok(staged)) => {
                let target = current_value.clone();
                let commit = async || commit_current_profile(&Config::profiles().await, target).await;
                match staged.deliver_committing(Delivery::Reload, commit).await {
                    Ok(Ok(delivered)) => handle_success(delivered, current_value.as_ref()).await,
                    Ok(Err(outcome)) => Ok(handle_validation_failure(outcome)),
                    // Ядро уже на новом профиле — реестр обязан догнать: одна повторная
                    // запись, и только если не вышло — честная ошибка, а не «отменено».
                    Err(e) => match e.downcast_ref::<CommitFailed>().map(|failed| failed.1) {
                        Some(delivered) => {
                            match commit_current_profile(&Config::profiles().await, current_value.clone()).await {
                                Ok(()) => handle_success(delivered, current_value.as_ref()).await,
                                Err(err) => {
                                    let message: String = super::public_error_text(&format!("{e}; повтор: {err:#}"));
                                    logging!(error, Type::Cmd, "{message}");
                                    handle::Handle::notice_message("update_failed", message.clone());
                                    Ok(ValidationOutcome::invalid_from_message(message))
                                }
                            }
                        }
                        None => Ok(handle_update_error(e)),
                    },
                }
            }
            Ok(Err(outcome)) => Ok(handle_validation_failure(outcome)),
            Err(e) => Ok(handle_update_error(e)),
        }
    });

    match task.await {
        Ok(result) => result,
        Err(join_error) => Ok(handle_update_error(join_error)),
    }
}

/// Изменяет конфиг profiles
#[tauri::command]
pub async fn patch_profiles_config(profiles: IProfiles) -> CmdResult<ValidationOutcome> {
    if CURRENT_SWITCHING_PROFILE
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        logging!(info, Type::Cmd, "конфиг уже переключается, запрос отклонён");
        return Ok(ValidationOutcome::Busy);
    }

    let target_profile = profiles.current.as_ref();

    logging!(
        info,
        Type::Cmd,
        "начало изменения файла конфига, целевой profile: {:?}",
        target_profile
    );

    logging!(
        info,
        Type::Cmd,
        "текущий конфиг: {:?}",
        Config::profiles().await.data_arc().current
    );

    let target_profile = target_profile.cloned();
    perform_config_update(profiles, target_profile).await
}

/// Изменяет profiles по имени profile
pub async fn patch_profiles_config_by_profile_index(profile_index: String) -> CmdResult<ValidationOutcome> {
    logging!(info, Type::Cmd, "переключение конфига на: {}", profile_index);

    let profiles = IProfiles {
        current: Some(profile_index),
        items: None,
    };
    patch_profiles_config(profiles).await
}

/// clod: запомнить выбранный узел одной парой «группа + узел».
///
/// Интерфейс раньше пересылал весь список `selected`, собранный из отрисованной
/// подписки: два быстрых переключения подряд читали один снимок, и второе
/// сохранение затирало первое. Слияние делается на бэкенде, поэтому гонки нет
/// ни у интерфейса, ни у трея.
///
/// Выбор, сделанный в окне, сохраняется только здесь, — здесь же о нём узнаёт
/// трей, с какого бы экрана ни выбирали.
#[tauri::command]
pub async fn patch_selected_node(group: String, node: String) -> CmdResult {
    let saved = profiles::profiles_set_selected_node_safe(&group, &node)
        .await
        .stringify_err();
    super::proxy::sync_tray_proxy_selection();
    saved
}

/// Изменяет отдельный profile item
#[tauri::command]
pub async fn patch_profile(index: String, profile: PrfItem) -> CmdResult {
    // Перед изменением проверяем, обновился ли update_interval
    let profiles = Config::profiles().await;
    let should_refresh_timer = if let Ok(old_profile) = profiles.latest_arc().get_item(&index)
        && let Some(new_option) = profile.option.as_ref()
    {
        let old_interval = old_profile.option.as_ref().and_then(|o| o.update_interval);
        let new_interval = new_option.update_interval;
        let old_allow_auto_update = old_profile.option.as_ref().and_then(|o| o.allow_auto_update);
        let new_allow_auto_update = new_option.allow_auto_update;
        (old_interval != new_interval) || (old_allow_auto_update != new_allow_auto_update)
    } else {
        false
    };

    profiles_patch_item_safe(&index, &profile).await.stringify_err()?;

    // Если интервал обновления или разрешение автообновления изменились,
    // асинхронно обновляем таймер
    if should_refresh_timer {
        crate::process::AsyncHandler::spawn(move || async move {
            logging!(info, Type::Timer, "Timer update settings changed, refreshing timer...");
            if let Err(e) = crate::core::Timer::global().refresh().await {
                logging!(error, Type::Timer, "Failed to refresh timer: {}", e);
            } else {
                // После успешного обновления отправляем кастомное событие, не перезагружая конфиг
                crate::core::handle::Handle::notify_timer_updated(&index);
            }
        });
    }

    Ok(())
}

/// Просмотр конфига
#[tauri::command]
pub async fn view_profile(index: String) -> CmdResult {
    let profiles = Config::profiles().await;
    let profiles_ref = profiles.latest_arc();
    let file = profiles_ref
        .get_item(&index)
        .stringify_err()?
        .file
        .as_ref()
        .ok_or("the file field is null")?;

    let path = dirs::app_profiles_dir().stringify_err()?.join(file.as_str());
    if !path.exists() {
        return CmdResult::Err(super::public_error_text(&format!(
            "file not found \"{}\"",
            path.display()
        )));
    }

    help::open_file(path).stringify_err()
}

/// Читает содержимое конфига
#[tauri::command]
pub async fn read_profile_file(index: String) -> CmdResult<String> {
    let item = {
        let profiles = Config::profiles().await;
        let profiles_ref = profiles.latest_arc();
        PrfItem {
            file: profiles_ref.get_item(&index).stringify_err()?.file.to_owned(),
            ..Default::default()
        }
    };

    if let Some(file) = item.file.as_ref() {
        let path = dirs::app_profiles_dir().stringify_err()?.join(file.as_str());
        match tokio::fs::try_exists(&path).await {
            Ok(true) => {}
            Ok(false) => return Ok(String::new()),
            Err(err) => {
                return Err(super::public_error_text(&format!(
                    "failed to check profile file \"{}\": {err}",
                    path.display()
                )));
            }
        }
    }

    let data = item.read_file().await.stringify_err()?;
    Ok(data)
}

/// Получает время следующего обновления
#[tauri::command]
pub async fn get_next_update_time(uid: String) -> CmdResult<Option<i64>> {
    let timer = Timer::global();
    let next_time = timer.get_next_update_time(&uid).await;
    Ok(next_time)
}

/// clod: что фильтр заглушек увидел в последнем ПРИМЕНЁННОМ конфиге.
///
/// Нужен интерфейсу, чтобы отличить «панель не выдала серверы» от «в шаблоне
/// просто нет групп», и чтобы процитировать сообщение панели, когда причину
/// нельзя вывести из срока и трафика.
#[tauri::command]
pub async fn get_sentinel_report() -> CmdResult<crate::enhance::SentinelReport> {
    Ok(crate::enhance::sentinel_report().await)
}

/// clod: описания серверов из панели — карта `имя узла → описание`.
///
/// Список серверов интерфейс берёт у ядра (`/proxies`), а там описания нет: оно
/// живёт только в самой подписке. Отдаём его отдельно, чтобы строка списка
/// говорила словами провайдера («10 Гбит · без лимита»), а не типом узла.
#[tauri::command]
pub async fn get_server_descriptions() -> CmdResult<std::collections::HashMap<String, String>> {
    Ok(crate::enhance::server_descriptions().await)
}

/// clod: логотип провайдера из локального кэша (`data:`-URL).
///
/// Холодный кэш наполняется здесь же, поэтому только что импортированная
/// подписка показывает логотип, не дожидаясь первого обновления.
#[tauri::command]
pub async fn get_profile_logo(uid: String) -> CmdResult<Option<String>> {
    Ok(crate::module::logo_cache::read_or_fetch(crate::module::logo_cache::Picture::Logo, &uid).await)
}

#[tauri::command]
pub async fn get_profile_background(uid: String) -> CmdResult<Option<String>> {
    Ok(crate::module::logo_cache::read_or_fetch(crate::module::logo_cache::Picture::Background, &uid).await)
}

#[cfg(test)]
mod delete_tests {
    use super::{OnStageRefused, delete_unless_current_in, on_stage_refused, without};
    use crate::config::{IProfiles, PrfItem, PrfOption};
    use crate::core::validate::{ValidationErrorKind, ValidationOutcome, ValidationSkipReason};

    fn item(uid: &str, itype: &str, merge: Option<&str>) -> PrfItem {
        PrfItem {
            uid: Some(uid.into()),
            itype: Some(itype.into()),
            file: Some(format!("{uid}.yaml").into()),
            option: merge.map(|merge| PrfOption {
                merge: Some(merge.into()),
                ..PrfOption::default()
            }),
            ..PrfItem::default()
        }
    }

    fn registry(current: Option<&str>) -> IProfiles {
        IProfiles {
            current: current.map(Into::into),
            items: Some(vec![
                item("m1", "merge", None),
                item("a", "remote", Some("m1")),
                item("b", "local", None),
            ]),
        }
    }

    fn uids(profiles: &IProfiles) -> Vec<&str> {
        profiles
            .items
            .iter()
            .flatten()
            .filter_map(|item| item.uid.as_deref())
            .collect()
    }

    #[test]
    fn the_candidate_goes_without_the_subscription_and_its_chains() {
        let accepted = registry(Some("a"));
        let candidate = without(&accepted, &"a".into()).unwrap_or_default();
        assert_eq!(uids(&candidate), ["b"]);
        assert_eq!(
            candidate.current.as_deref(),
            Some("b"),
            "текущей стала следующая подписка"
        );
        assert_eq!(uids(&accepted), ["m1", "a", "b"], "принятый реестр не тронут");
        assert_eq!(accepted.current.as_deref(), Some("a"));

        let candidate = without(&accepted, &"b".into()).unwrap_or_default();
        assert_eq!(uids(&candidate), ["m1", "a"]);
        assert_eq!(candidate.current.as_deref(), Some("a"), "текущая не сменилась");
        assert!(without(&accepted, &"ghost".into()).is_err());
    }

    #[test]
    fn a_build_that_was_not_checked_still_deletes_and_a_refused_one_does_not() {
        assert_eq!(on_stage_refused(&ValidationOutcome::Busy), OnStageRefused::DeleteNow);
        assert_eq!(
            on_stage_refused(&ValidationOutcome::Skipped {
                reason: ValidationSkipReason::Exiting
            }),
            OnStageRefused::DeleteNow
        );
        assert_eq!(
            on_stage_refused(&ValidationOutcome::invalid(ValidationErrorKind::CoreRejected, "why")),
            OnStageRefused::Refuse
        );
    }

    #[tokio::test]
    async fn a_subscription_that_became_current_is_not_deleted_from_the_registry() {
        for current in [Some("a"), None] {
            let result = delete_unless_current_in(registry(current), &"a".into()).await;
            let (kept, files) = result.unwrap_or_else(|_| (IProfiles::default(), Some(Default::default())));
            assert!(files.is_none(), "{current:?}: ядру сначала нужна сборка без неё");
            assert_eq!(uids(&kept), ["m1", "a", "b"], "{current:?}");
            assert_eq!(kept.current.as_deref(), current);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::commit_current_profile;

    #[test]
    fn every_way_to_add_a_subscription_shares_one_tail() {
        let commands = include_str!("profile.rs");
        for signature in ["pub async fn import_profile(", "pub async fn create_profile("] {
            let body = crate::utils::source_scan::fn_body(commands, signature).unwrap_or_default();
            assert!(body.contains("feat::add_profile("), "{signature}");
            assert!(body.contains("announce_the_added("), "{signature}");
        }
        let deep_link = crate::utils::source_scan::fn_body(
            include_str!("../utils/resolve/scheme.rs"),
            "async fn import_subscription(",
        )
        .unwrap_or_default();
        assert!(deep_link.contains("feat::add_profile("), "{deep_link}");
        assert!(!deep_link.contains("profiles_save_file_safe"), "{deep_link}");
    }
    use crate::config::{IProfiles, PrfItem};
    use clash_verge_draft::Draft;

    fn profile(uid: &str) -> PrfItem {
        PrfItem {
            uid: Some(uid.into()),
            ..PrfItem::default()
        }
    }

    #[tokio::test]
    async fn committing_profile_switch_preserves_profiles_added_after_draft_creation() -> anyhow::Result<()> {
        let profiles = Draft::new(IProfiles {
            current: Some("a".into()),
            items: Some(vec![profile("a"), profile("b")]),
        });
        profiles.edit_draft(|draft| {
            draft.patch_config(&IProfiles {
                current: Some("b".into()),
                items: None,
            });
        });
        profiles
            .with_data_modify(|mut committed| async move {
                committed.items.get_or_insert_with(Vec::new).push(profile("new"));
                Ok((committed, ()))
            })
            .await?;

        commit_current_profile(&profiles, Some("b".into())).await?;

        let committed = profiles.data_arc();
        assert_eq!(committed.current.as_deref(), Some("b"));
        assert!(committed.get_item("new").is_ok());
        Ok(())
    }
}
