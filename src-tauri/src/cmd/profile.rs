use super::CmdResult;
use super::StringifyErr as _;
use crate::cmd::validate::{ValidationNoticeTarget, handle_validation_notice};
use crate::config::profiles;
use crate::utils::window_manager::WindowManager;
use crate::{
    config::{
        Config, IProfiles, PrfItem, PrfOption,
        profiles::{
            profiles_append_item_with_filedata_safe, profiles_delete_item_safe, profiles_patch_item_safe,
            profiles_reorder_safe, profiles_restore_snapshot_safe, profiles_save_file_safe,
        },
        profiles_append_item_safe,
    },
    core::{CoreManager, handle, timer::Timer, tray::Tray, validate::ValidationOutcome},
    feat,
    utils::{dirs, help},
};
use clash_verge_draft::{Draft, SharedDraft};
use clash_verge_logging::{Type, logging, logging_error};
use scopeguard::defer;
use smartstring::alias::String;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

static CURRENT_SWITCHING_PROFILE: AtomicBool = AtomicBool::new(false);

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

/// Расширенный конфиг
#[tauri::command]
pub async fn enhance_profiles() -> CmdResult<ValidationOutcome> {
    match feat::enhance_profiles().await {
        Ok(outcome) if outcome.is_valid() => {
            handle::Handle::refresh_clash();
            // clod: enhance тоже перезагружает конфиг ядра — вернуть выбор
            // узлов и избранные, как и при обновлении подписки
            if let Err(e) = profiles::activate_selected_nodes() {
                logging!(warn, Type::Cmd, "Warning: restore selection after enhance failed: {e}");
            }
            Ok(outcome)
        }
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

    // Полагаемся напрямую на логику таймаута/повтора PrfItem::from_url,
    // не оборачиваем в tokio::time::timeout
    // clod: сначала напрямую, потом через собственное ядро, потом через
    // системный прокси — заблокированный домен подписки достижим через уже
    // поднятый туннель.
    let item = &mut match PrfItem::from_url_with_ladder(&url, None, None, option.as_ref()).await {
        Ok(it) => {
            logging!(
                info,
                Type::Cmd,
                "[импорт подписки] загрузка завершена, сохранение конфига"
            );
            it
        }
        Err(e) => {
            logging!(error, Type::Cmd, "[импорт подписки] не удалось загрузить: {}", e);
            return Err(super::public_error_text(&profile_import_error(&e)));
        }
    };

    if let Err(e) = profiles_append_item_safe(item).await {
        logging!(error, Type::Cmd, "[импорт подписки] не удалось сохранить конфиг: {}", e);
        return Err(format!("не удалось импортировать подписку: {}", super::public_error_text(&e)).into());
    }

    if let Err(e) = profiles_save_file_safe().await {
        logging!(
            error,
            Type::Cmd,
            "[импорт подписки] не удалось сохранить файл конфига: {}",
            e
        );
        return Err(format!("не удалось импортировать подписку: {}", super::public_error_text(&e)).into());
    }
    logging!(info, Type::Cmd, "[импорт подписки] файл конфига сохранён");
    logging_error!(Type::Timer, Timer::global().refresh().await);

    if let Some(uid) = &item.uid {
        logging!(
            info,
            Type::Cmd,
            "[импорт подписки] отправка уведомления об изменении конфига: {}",
            uid
        );
        handle::Handle::notify_profile_changed(uid);
    }

    logging!(
        info,
        Type::Cmd,
        "[импорт подписки] импорт завершён: {}",
        help::mask_url(&url)
    );
    Ok(())
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
    match profiles_append_item_with_filedata_safe(&item, file_data).await {
        Ok(_) => {
            profiles_save_file_safe().await.stringify_err()?;
            logging_error!(Type::Timer, Timer::global().refresh().await);
            // Отправляем уведомление об изменении конфига
            if let Some(uid) = &item.uid {
                logging!(
                    info,
                    Type::Cmd,
                    "[создание подписки] отправка уведомления об изменении конфига: {}",
                    uid
                );
                handle::Handle::notify_profile_changed(uid);
            }
            Ok(())
        }
        Err(err) => match err.to_string().as_str() {
            "the file already exists" => Err("the file already exists".into()),
            _ => Err(format!("add profile error: {}", super::public_error_text(&err)).into()),
        },
    }
}

/// clod: вернуть сохранённый выбор узлов после теста серверов.
///
/// Групповой тест задержек в mihomo сбрасывает «закреплённый» узел у групп
/// url-test/fallback (`ForceSet("")` в обработчике `/group/{name}/delay`)
/// и затирает его в store-selected кэше ядра. Фронтенд зовёт эту команду
/// сразу после теста, чтобы выбор пользователя строго сохранялся.
#[tauri::command]
pub async fn restore_selected_nodes() -> CmdResult {
    profiles::activate_selected_nodes().stringify_err()
}

/// Обновляет конфиг
#[tauri::command]
pub async fn update_profile(index: String, option: Option<PrfOption>) -> CmdResult {
    // clod:provider-links — `Box::pin`: карточка профиля подросла на ссылки
    // провайдера, и клиппи справедливо не хочет держать такой future на стеке.
    match Box::pin(feat::update_profile(&index, option.as_ref(), true, true, true)).await {
        Ok(_) => Ok(()),
        Err(e) => {
            logging!(error, Type::Cmd, "{}", e);
            Err(super::public_error_text(&e))
        }
    }
}

/// Удаляет конфиг
#[tauri::command]
pub async fn delete_profile(index: String) -> CmdResult {
    // clod: снимок ДО удаления. Если конфиг с оставшимися подписками не
    // соберётся, удаление отменяется целиком — вместе с выбранной подпиской и
    // порядком, — а файлы к этому моменту ещё на диске: их стирают последними.
    let snapshot = (**Config::profiles().await.latest_arc()).clone();

    // Используем Send-safe helper-функцию
    let (should_update, pending_files) = profiles_delete_item_safe(&index).await.stringify_err()?;
    profiles_save_file_safe().await.stringify_err()?;
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
    if should_update {
        match CoreManager::global().update_config_forced().await {
            Ok(outcome) if outcome.is_valid() => {
                handle::Handle::refresh_clash();
                // Отправляем уведомление об изменении конфига
                logging!(
                    info,
                    Type::Cmd,
                    "[удаление подписки] отправка уведомления об изменении конфига: {}",
                    index
                );
                handle::Handle::notify_profile_changed(&index);
            }
            Ok(outcome) => {
                logging!(
                    warn,
                    Type::Cmd,
                    "не удалось обновить конфиг после удаления подписки: {}",
                    outcome
                );
                restore_profiles_after_failed_delete(snapshot).await;
                handle_validation_notice(&outcome, ValidationNoticeTarget::Runtime, "рабочий конфиг");
                return Err(outcome.to_string().into());
            }
            Err(e) => {
                logging!(error, Type::Cmd, "{}", e);
                restore_profiles_after_failed_delete(snapshot).await;
                return Err(super::public_error_text(&e));
            }
        }
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

/// clod: вернуть подписки как было и поднять прежний конфиг.
///
/// Зовётся, только когда конфиг без удалённой подписки не собрался. Файлы к
/// этому моменту ещё целы, поэтому возврата снимка достаточно: пользователь
/// остаётся ровно там, где был до нажатия «удалить».
async fn restore_profiles_after_failed_delete(snapshot: IProfiles) {
    if let Err(err) = profiles_restore_snapshot_safe(snapshot).await {
        logging!(
            error,
            Type::Cmd,
            "не удалось вернуть подписки после неудачного удаления: {err}"
        );
        return;
    }
    match CoreManager::global().update_config_forced().await {
        Ok(outcome) if outcome.is_valid() => {
            handle::Handle::refresh_clash();
            handle::Handle::refresh_profiles();
        }
        Ok(outcome) => logging!(
            warn,
            Type::Cmd,
            "подписки возвращены, но прежний конфиг тоже не собрался: {outcome}"
        ),
        Err(err) => logging!(error, Type::Cmd, "{err}"),
    }
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
    if let Err(e) = crate::core::sysopt::Sysopt::global().reset_sysproxy().await {
        logging!(warn, Type::Cmd, "не удалось снять системный прокси: {e}");
    }
}

/// Выполняет обновление конфига и обрабатывает результат
async fn restore_previous_profile(prev_profile: &String) -> CmdResult<()> {
    logging!(
        info,
        Type::Cmd,
        "попытка восстановить предыдущий конфиг: {}",
        prev_profile
    );
    let restore_profiles = IProfiles {
        current: Some(prev_profile.to_owned()),
        items: None,
    };
    Config::profiles()
        .await
        .edit_draft(|d| d.patch_config(&restore_profiles));
    Config::profiles().await.apply();
    crate::process::AsyncHandler::spawn(|| async move {
        if let Err(e) = profiles_save_file_safe().await {
            logging!(
                warn,
                Type::Cmd,
                "Warning: не удалось асинхронно сохранить восстановленный файл конфига: {e}"
            );
        }
    });
    logging!(info, Type::Cmd, "предыдущий конфиг успешно восстановлен");
    Ok(())
}

async fn commit_current_profile(profiles: &Draft<IProfiles>, current: Option<String>) -> anyhow::Result<()> {
    profiles.discard();
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

async fn handle_success(current_value: Option<&String>) -> CmdResult<ValidationOutcome> {
    commit_current_profile(&Config::profiles().await, current_value.cloned())
        .await
        .stringify_err()?;
    // Runtime refresh and tray rebuilding happen after saved node selections are restored.
    profiles::activate_selected_nodes().stringify_err()?;

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

async fn discard_and_restore(current_profile: Option<&String>) -> CmdResult<()> {
    Config::profiles().await.discard();
    if let Some(prev_profile) = current_profile {
        restore_previous_profile(prev_profile).await?;
    }
    Ok(())
}

async fn handle_validation_failure(
    outcome: ValidationOutcome,
    current_profile: Option<&String>,
) -> CmdResult<ValidationOutcome> {
    logging!(warn, Type::Cmd, "не удалось проверить конфиг: {}", outcome);
    discard_and_restore(current_profile).await?;
    handle_validation_notice(&outcome, ValidationNoticeTarget::Runtime, "рабочий конфиг");
    Ok(outcome)
}

async fn handle_update_error<E: std::fmt::Display>(
    e: E,
    current_profile: Option<&String>,
) -> CmdResult<ValidationOutcome> {
    logging!(warn, Type::Cmd, "ошибка в процессе обновления: {}", e,);
    discard_and_restore(current_profile).await?;
    let message: String = super::public_error_text(&e);
    handle::Handle::notice_message("config_validate::boot_error", message.clone());
    Ok(ValidationOutcome::invalid_from_message(message))
}

async fn handle_timeout(current_profile: Option<&String>) -> CmdResult<ValidationOutcome> {
    let timeout_msg: String =
        "таймаут обновления конфига (30 сек), возможно зависла проверка конфига или связь с ядром".into();
    logging!(error, Type::Cmd, "{}", timeout_msg);
    discard_and_restore(current_profile).await?;
    handle::Handle::notice_message("config_validate::timeout", timeout_msg.clone());
    Ok(ValidationOutcome::invalid_from_message(timeout_msg))
}

async fn perform_config_update(
    current_value: Option<&String>,
    current_profile: Option<&String>,
) -> CmdResult<ValidationOutcome> {
    defer! {
        CURRENT_SWITCHING_PROFILE.store(false, Ordering::Release);
    }
    let update_result =
        tokio::time::timeout(Duration::from_secs(30), CoreManager::global().update_config_forced()).await;

    match update_result {
        Ok(Ok(outcome)) if outcome.is_valid() => handle_success(current_value).await,
        Ok(Ok(outcome)) => handle_validation_failure(outcome, current_profile).await,
        Ok(Err(e)) => handle_update_error(e, current_profile).await,
        Err(_) => handle_timeout(current_profile).await,
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

    // Сохраняем текущий конфиг, чтобы восстановить его при неудачной проверке
    let previous_profile = Config::profiles().await.data_arc().current.clone();
    logging!(info, Type::Cmd, "текущий конфиг: {:?}", previous_profile);

    Config::profiles().await.edit_draft(|d| d.patch_config(&profiles));

    perform_config_update(target_profile, previous_profile.as_ref()).await
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
#[tauri::command]
pub async fn patch_selected_node(group: String, node: String) -> CmdResult {
    profiles::profiles_set_selected_node_safe(&group, &node)
        .await
        .stringify_err()
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
    Ok(crate::module::logo_cache::read_or_fetch(&uid).await)
}

#[cfg(test)]
mod tests {
    use super::commit_current_profile;
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
