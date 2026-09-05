use crate::{
    cmd,
    config::{Config, PrfItem, PrfOption, profiles::profiles_draft_update_item_safe, sub_headers},
    core::{CoreManager, handle, tray, validate::ValidationOutcome},
    utils::help::{keep_the_clearer_error, mask_err, mask_url},
};
use anyhow::{Result, bail};
use clash_verge_logging::{Type, logging, logging_error};
use smartstring::alias::String;

pub async fn toggle_proxy_profile(profile_index: String) {
    logging_error!(
        Type::Config,
        cmd::patch_profiles_config_by_profile_index(profile_index).await
    );
}

pub async fn switch_proxy_node(group_name: &str, proxy_name: &str) {
    let previous = handle::Handle::mihomo()
        .await
        .get_group_by_name(group_name)
        .await
        .ok()
        .and_then(|group| group.now)
        .filter(|now| now != proxy_name);
    match handle::Handle::mihomo()
        .await
        .select_node_for_group(group_name, proxy_name)
        .await
    {
        Ok(_) => {
            logging!(
                info,
                Type::Tray,
                "Переключение прокси успешно: {} -> {}",
                group_name,
                proxy_name
            );
            if let Some(previous) = previous {
                crate::process::AsyncHandler::spawn(move || async move {
                    crate::feat::close_connections_via(&previous).await;
                });
            }
            if let Err(err) = crate::config::profiles::profiles_set_selected_node_safe(group_name, proxy_name).await {
                logging!(
                    warn,
                    Type::Tray,
                    "Warning: не удалось запомнить выбор узла из трея: {err}"
                );
            }
            handle::Handle::refresh_proxy_config();
            let _ = tray::Tray::global().update_menu().await;
            return;
        }
        Err(err) => {
            logging!(
                error,
                Type::Tray,
                "Не удалось переключить прокси: {} -> {}, ошибка: {:?}",
                group_name,
                proxy_name,
                err
            );
        }
    }

    match handle::Handle::mihomo()
        .await
        .select_node_for_group(group_name, proxy_name)
        .await
    {
        Ok(_) => {
            logging!(
                info,
                Type::Tray,
                "Откат переключения прокси успешен: {} -> {}",
                group_name,
                proxy_name
            );
            let _ = tray::Tray::global().update_menu().await;
        }
        Err(err) => {
            logging!(
                error,
                Type::Tray,
                "Переключение прокси окончательно не удалось: {} -> {}, ошибка: {:?}",
                group_name,
                proxy_name,
                err
            );
        }
    }
}

struct UpdateTarget {
    url: String,
    option: Option<PrfOption>,
    fallback_url: Option<String>,
    fallback_domain: Option<String>,
}

async fn should_update_profile(uid: &String, ignore_auto_update: bool) -> Result<Option<UpdateTarget>> {
    let profiles = Config::profiles().await;
    let profiles = profiles.latest_arc();
    let item = profiles.get_item(uid)?;
    let is_remote = item.itype.as_ref().is_some_and(|s| s == "remote");

    if !is_remote {
        logging!(
            info,
            Type::Config,
            "[Обновление подписки] {uid} не является удалённой подпиской, пропускаю обновление"
        );
        Ok(None)
    } else if item.url.is_none() {
        logging!(
            warn,
            Type::Config,
            "Warning: [Обновление подписки] {uid} отсутствует URL, обновление невозможно"
        );
        bail!("failed to get the profile item url");
    } else if !ignore_auto_update && !item.option.as_ref().and_then(|o| o.allow_auto_update).unwrap_or(true) {
        logging!(
            info,
            Type::Config,
            "[Обновление подписки] {} автообновление запрещено, пропускаю обновление",
            uid
        );
        Ok(None)
    } else {
        logging!(
            info,
            Type::Config,
            "[Обновление подписки] {} является удалённой подпиской, URL: {}",
            uid,
            mask_url(
                item.url
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("Profile URL is None"))?
            )
        );
        Ok(Some(UpdateTarget {
            url: item.url.clone().ok_or_else(|| anyhow::anyhow!("Profile URL is None"))?,
            option: item.option.clone(),
            fallback_url: item.fallback_url.clone(),
            fallback_domain: item.fallback_domain.clone(),
        }))
    }
}

async fn apply_updated_item(uid: &String, item: &mut PrfItem) -> Result<()> {
    let migrate_url = item.migrate_url.clone();
    let request_option = item.option.clone();

    profiles_draft_update_item_safe(uid, item).await?;

    let Some(candidate) = migrate_url else {
        return Ok(());
    };

    let hops = Config::profiles()
        .await
        .latest_arc()
        .get_item(uid)
        .map_or(0, |item| item.migration_hops.unwrap_or(0));
    if hops >= sub_headers::MAX_MIGRATION_HOPS {
        logging!(
            warn,
            Type::Config,
            "Warning: [Обновление подписки] [clod] ignoring migration to {}: {} consecutive hops already followed",
            mask_url(&candidate),
            hops
        );
        return Ok(());
    }

    match PrfItem::from_url(&candidate, None, None, request_option.as_ref()).await {
        Ok(_) => match crate::config::profiles::profiles_migrate_url_safe(uid, candidate.clone()).await {
            Ok(()) => {
                logging!(
                    info,
                    Type::Config,
                    "[Обновление подписки] [clod] provider migrated the subscription URL to {}",
                    mask_url(&candidate)
                );
                handle::Handle::notice_message("clod_sub::url_migrated", mask_url(&candidate));
            }
            Err(err) => logging!(
                warn,
                Type::Config,
                "Warning: [Обновление подписки] [clod] failed to persist the migrated subscription URL: {}",
                mask_err(&err.to_string())
            ),
        },
        Err(err) => logging!(
            warn,
            Type::Config,
            "Warning: [Обновление подписки] [clod] candidate URL {} failed verification, keeping the current one: {}",
            mask_url(&candidate),
            mask_err(&err.to_string())
        ),
    }

    Ok(())
}

/// Ступеней лестницы маршрутов на один адрес.
const LADDER_STEPS: u64 = 3;

/// Любой запрос может быть повторён с запасными корнями TLS
/// (`utils/network.rs`, `should_retry_with_static_webpki_roots`).
const TLS_FALLBACK_ATTEMPTS: u64 = 2;

/// Защищённый канал при неудаче повторяет запрос без закрепления ключа прослойки
/// (`config/prfitem.rs`, `fetch_for_profile`) — ради ротации ключа он и заведён.
const SECURE_CHANNEL_ATTEMPTS: u64 = 2;

/// Запас поверх суммы ступеней: фора прямого маршрута в гонке, паузы между
/// попытками и разбор ответа.
const LADDER_SLACK: std::time::Duration = std::time::Duration::from_secs(10);

/// Потолок самого бюджета. Существует только затем, чтобы прибавление к `Instant`
/// не переполнилось: `Instant + Duration::MAX` — паника. Тридцать лет — то же
/// значение, которое tokio берёт в `Instant::far_future` со ссылкой на переполнение
/// на macOS и FreeBSD. Ни одна работающая настройка сюда не упирается.
const BUDGET_CEILING_SECS: u64 = 30 * 365 * 24 * 60 * 60;

/// Сколько времени отводится на ОДИН адрес подписки — основной или запасной.
///
/// Потолка не было вовсе: сумма таймаутов ступеней ничем не ограничивалась, и
/// отменить ожидание было нечем. Бюджет считается от таймаута, который выбрал сам
/// пользователь в карточке профиля, и от числа законных попыток внутри ступени,
/// поэтому ни один работавший путь не укорачивается: обрывается только зависание
/// сверх того, что лестница может занять честно.
///
/// У каждого адреса бюджет свой — иначе основной адрес съедал бы весь потолок и до
/// запасного домена, ради которого он и заведён, дело не доходило бы никогда. Общего
/// потолка на всё обновление поэтому нет: бюджет режет зависание отдельного адреса,
/// а не суммарное время.
///
/// Запись профиля в реестр (`apply_updated_item`) идёт вне бюджета, поэтому принятый
/// профиль не может оборваться на середине применения.
fn address_budget(option: Option<&PrfOption>) -> std::time::Duration {
    let timeout = option.and_then(|o| o.timeout_seconds).unwrap_or(20);
    let secure = option.is_some_and(|o| o.secure.unwrap_or(false));

    let attempts_per_step = TLS_FALLBACK_ATTEMPTS * if secure { SECURE_CHANNEL_ATTEMPTS } else { 1 };
    let seconds = timeout.saturating_mul(LADDER_STEPS).saturating_mul(attempts_per_step);

    std::time::Duration::from_secs(seconds.min(BUDGET_CEILING_SECS)).saturating_add(LADDER_SLACK)
}

async fn within_budget<F>(deadline: tokio::time::Instant, work: F) -> Result<PrfItem>
where
    F: std::future::Future<Output = Result<PrfItem>>,
{
    // Бюджет уже вышел — запрос не отправляем вовсе, чтобы не дёргать панель
    // соединением, которое всё равно будет оборвано.
    if tokio::time::Instant::now() >= deadline {
        bail!("clod-sub-budget: на этот адрес подписки отведённое время уже вышло");
    }

    match tokio::time::timeout_at(deadline, Box::pin(work)).await {
        Ok(result) => result,
        Err(_) => bail!("clod-sub-budget: адрес подписки не ответил за отведённое время"),
    }
}

async fn perform_profile_update(
    uid: &String,
    url: &String,
    opt: Option<&PrfOption>,
    option: Option<&PrfOption>,
    fallback_url: Option<String>,
    fallback_domain: Option<String>,
) -> Result<bool> {
    logging!(
        info,
        Type::Config,
        "[Обновление подписки] Начинаю загрузку нового содержимого подписки"
    );
    let mut merged_opt = PrfOption::merge(opt, option);
    let budget = address_budget(merged_opt.as_ref());
    let deadline = tokio::time::Instant::now() + budget;
    let is_current = {
        let profiles = Config::profiles().await;
        profiles.latest_arc().is_current_profile_index(uid)
    };
    let profiles = Config::profiles().await;
    let profiles_arc = profiles.latest_arc();
    let profile_name = profiles_arc
        .get_name_by_uid(uid)
        .cloned()
        .unwrap_or_else(|| String::from("UnKnown Profile"));

    let mut last_err;

    match within_budget(deadline, PrfItem::from_url(url, None, None, merged_opt.as_ref())).await {
        Ok(mut item) => {
            logging!(
                info,
                Type::Config,
                "[Обновление подписки] Конфиг подписки обновлён успешно"
            );
            apply_updated_item(uid, &mut item).await?;
            return Ok(is_current);
        }
        Err(err) => {
            logging!(
                warn,
                Type::Config,
                "Warning: [Обновление подписки] Обычное обновление не удалось: {}, пробую обновить через прокси Clash",
                mask_err(&err.to_string())
            );
            last_err = err;
        }
    }

    merged_opt.get_or_insert_with(PrfOption::default).self_proxy = Some(true);
    merged_opt.get_or_insert_with(PrfOption::default).with_proxy = Some(false);

    match within_budget(deadline, PrfItem::from_url(url, None, None, merged_opt.as_ref())).await {
        Ok(mut item) => {
            logging!(
                info,
                Type::Config,
                "[Обновление подписки] Обновление конфига подписки через прокси Clash успешно"
            );
            apply_updated_item(uid, &mut item).await?;
            handle::Handle::notice_message("update_with_clash_proxy", profile_name);
            drop(last_err);
            return Ok(is_current);
        }
        Err(err) => {
            logging!(
                warn,
                Type::Config,
                "Warning: [Обновление подписки] Обновление через прокси Clash не удалось: {}, пробую обновить через системный прокси",
                mask_err(&err.to_string())
            );
            last_err = keep_the_clearer_error(last_err, err);
        }
    }

    merged_opt.get_or_insert_with(PrfOption::default).self_proxy = Some(false);
    merged_opt.get_or_insert_with(PrfOption::default).with_proxy = Some(true);

    match within_budget(deadline, PrfItem::from_url(url, None, None, merged_opt.as_ref())).await {
        Ok(mut item) => {
            logging!(
                info,
                Type::Config,
                "[Обновление подписки] Обновление конфига подписки через системный прокси успешно"
            );
            apply_updated_item(uid, &mut item).await?;
            handle::Handle::notice_message("update_with_clash_proxy", profile_name);
            drop(last_err);
            return Ok(is_current);
        }
        Err(err) => {
            logging!(
                warn,
                Type::Config,
                "Warning: [Обновление подписки] Обновление через системный прокси не удалось: {}, все попытки исчерпаны",
                mask_err(&err.to_string())
            );
            last_err = keep_the_clearer_error(last_err, err);
        }
    }

    let spare_addresses = [
        fallback_url.filter(|value| !value.trim().is_empty()),
        fallback_domain
            .filter(|value| !value.trim().is_empty())
            .and_then(|domain| sub_headers::swap_domain(url, &domain)),
    ];

    for spare in spare_addresses.into_iter().flatten() {
        logging!(
            info,
            Type::Config,
            "[Обновление подписки] [clod] primary URL failed, trying the provider spare address {}",
            mask_url(&spare)
        );

        // У запасного адреса свой бюджет: он существует ровно для того случая,
        // когда основной адрес молчит до последней секунды.
        let spare_deadline = tokio::time::Instant::now() + budget;

        match within_budget(
            spare_deadline,
            PrfItem::from_url_with_ladder(&spare, None, None, merged_opt.as_ref()),
        )
        .await
        {
            Ok(mut item) => {
                item.from_fallback = Some(true);
                apply_updated_item(uid, &mut item).await?;
                handle::Handle::notice_message("clod_sub::fallback_used", profile_name);
                drop(last_err);
                return Ok(is_current);
            }
            Err(err) => {
                logging!(
                    warn,
                    Type::Config,
                    "Warning: [Обновление подписки] [clod] spare address failed as well: {}",
                    mask_err(&err.to_string())
                );
                last_err = keep_the_clearer_error(last_err, err);
            }
        }
    }

    let last_err = mask_err(&last_err.to_string());
    bail!("{profile_name} - {last_err}")
}

/// Текст отказа, пригодный для показа пользователю.
///
/// `mask_err` прячет адреса подписки, но не трогает путей файловой системы, а в
/// сообщении ядра приезжает полный путь — вместе с именем пользователя ОС. Гоняем
/// его через ту же чистку, что и ответы команд, чтобы в тост не уезжало лишнее.
fn public_failure_text(raw: &str) -> String {
    let masked = mask_err(raw);
    let home = crate::utils::redact::home_prefix();
    String::from(crate::utils::redact::redact(&crate::utils::redact::scrub_home(
        masked.as_str(),
        home.as_deref(),
    )))
}

/// Сообщить о провале фонового обновления.
///
/// Только про текущий профиль: расписание догоняет пропущенные задания пачкой, и
/// при выключенной сети человек получил бы столько красных тостов, сколько у него
/// подписок. О фоновых провалах остальных говорит пометка на их карточках.
async fn announce_the_failure(uid: &String, status: &str, raw: &str) {
    let is_current = Config::profiles().await.latest_arc().is_current_profile_index(uid);
    if !is_current {
        return;
    }

    handle::Handle::notice_message(status, public_failure_text(raw));
}

/// Под каким видом отказа показать сообщение.
///
/// Ядро уже различает, что именно пошло не так, и на каждый вид в приложении
/// написан свой перевод с готовым советом — от «антивирус прервал проверку» до
/// «ошибка в скрипте». На пути обновления подписки этот разбор до сих пор
/// выбрасывался, и всё сводилось к одной общей фразе.
const fn failure_notice_status(result: &Result<ValidationOutcome>) -> &'static str {
    match result {
        Ok(ValidationOutcome::Invalid { kind, .. }) => {
            crate::cmd::validate::notice_key(*kind, crate::cmd::validate::ValidationNoticeTarget::Runtime)
        }
        _ => "update_failed",
    }
}

/// Вернуть на диск профиль, который работал до неудачной попытки обновления.
async fn restore_working_profile(uid: &String, snapshot: Option<crate::config::profiles::ProfileSnapshot>) {
    let Some(snapshot) = snapshot else {
        return;
    };

    match crate::config::profiles::profiles_restore_item(snapshot).await {
        Ok(()) => {
            logging!(
                info,
                Type::Config,
                "[Обновление подписки] ядро отвергло новый конфиг, на диск возвращён прежний рабочий профиль"
            );
            if let Err(err) = crate::config::profiles::profiles_mark_not_applied(uid, true).await {
                logging!(
                    warn,
                    Type::Config,
                    "Warning: [Обновление подписки] не удалось пометить профиль как непринятый: {}",
                    mask_err(&err.to_string())
                );
            }
            handle::Handle::refresh_profiles();
        }
        Err(err) => logging!(
            error,
            Type::Config,
            "[Обновление подписки] ядро отвергло новый конфиг, и вернуть прежний профиль не удалось: {}",
            mask_err(&err.to_string())
        ),
    }
}

/// Прибраться после того, как ядро приняло новый конфиг.
fn settle_after_a_successful_update(uid: &String) {
    // Пометку «скачано, но не применено» снимает сам путь применения конфига
    // (`core/manager/config.rs`) — там она снимается на всех путях сразу, включая
    // переключение профиля и ручную пересборку.
    logging!(info, Type::Config, "[Обновление подписки] Обновление успешно");
    handle::Handle::refresh_clash();
    if let Err(err) = crate::config::profiles::activate_selected_nodes() {
        logging!(
            warn,
            Type::Config,
            "Warning: [Обновление подписки] restore selection failed: {err}"
        );
    }
    crate::process::AsyncHandler::spawn(|| async {
        crate::module::sub_watcher::run_check().await;
    });
    let logo_uid = uid.clone();
    crate::process::AsyncHandler::spawn(move || async move {
        crate::module::logo_cache::sync(&logo_uid).await;
    });
}

/// Отдать ядру обновлённый профиль и разобраться с тем, что оно ответило.
async fn apply_the_updated_profile(
    uid: &String,
    snapshot: Option<crate::config::profiles::ProfileSnapshot>,
    is_mannual_trigger: bool,
) -> Result<()> {
    match CoreManager::global().update_config_with_force(is_mannual_trigger).await {
        Ok(outcome) if outcome.is_valid() => settle_after_a_successful_update(uid),
        Ok(outcome @ (ValidationOutcome::Skipped { .. } | ValidationOutcome::Busy)) if !is_mannual_trigger => {
            logging!(
                info,
                Type::Config,
                "[Обновление подписки] Обновление конфига на этот раз пропущено: {}",
                outcome
            );
        }
        result => {
            // Ядро отвергло конфиг — на диск возвращаем прежний рабочий профиль.
            // Только отвергло: `Err` от обновления конфига означает, что ядро о
            // содержимом ничего не сказало (не записался файл, не поднялась
            // служба), и выбрасывать из-за этого годную свежую подписку нельзя.
            // Разбирать, чьё содержимое виновато (подписки или пользовательских
            // цепочек merge/script/rules/groups), нельзя: ядро проверяет их слитыми
            // и сообщает об ошибке одинаково. Поэтому откат делает только самое
            // безопасное — возвращает файл, не трогая отметку времени.
            let core_rejected_the_config = matches!(result, Ok(ValidationOutcome::Invalid { .. }));
            let status = failure_notice_status(&result);
            let message = match result {
                Ok(outcome) => outcome.to_string(),
                Err(err) => err.to_string(),
            };
            let message = public_failure_text(&message);

            if core_rejected_the_config {
                restore_working_profile(uid, snapshot).await;
            }
            logging!(
                error,
                Type::Config,
                "[Обновление подписки] Обновление не удалось: {}",
                message
            );
            if !is_mannual_trigger {
                announce_the_failure(uid, status, &message).await;
            }
            bail!(message);
        }
    }

    Ok(())
}

pub async fn update_profile(
    uid: &String,
    option: Option<&PrfOption>,
    auto_refresh: bool,
    ignore_auto_update: bool,
    is_mannual_trigger: bool,
) -> Result<()> {
    logging!(
        info,
        Type::Config,
        "[Обновление подписки] Начинаю обновление подписки {}",
        uid
    );
    let url_opt = match should_update_profile(uid, ignore_auto_update).await {
        Ok(target) => target,
        Err(err) => {
            // Ручной вызов покажет ошибку сам — она вернётся ответом команды.
            if !is_mannual_trigger {
                announce_the_failure(uid, "update_failed", &err.to_string()).await;
            }
            return Err(err);
        }
    };

    // Файл профиля и `updated` меняются раньше, чем ядро успевает сказать, годится ли
    // новый конфиг. Держим слепок прежнего рабочего состояния, чтобы вернуть его, если
    // ядро откажется, — иначе после перезапуска приложения профиля бы не осталось.
    // Ядро трогает только текущий профиль, поэтому для остальных слепок не нужен.
    let snapshot = if url_opt.is_some() && Config::profiles().await.latest_arc().is_current_profile_index(uid) {
        crate::config::profiles::profiles_snapshot_item(uid).await
    } else {
        None
    };

    let should_refresh = match url_opt {
        Some(target) => {
            let outcome = Box::pin(perform_profile_update(
                uid,
                &target.url,
                target.option.as_ref(),
                option,
                target.fallback_url,
                target.fallback_domain,
            ))
            .await;
            match outcome {
                Ok(changed) => changed && auto_refresh,
                Err(err) => {
                    release_stale_panel_locks().await;
                    // Загрузка провалилась. Ручной вызов покажет ошибку сам — она
                    // уедет наверх и вернётся в интерфейс ответом команды; а вот
                    // автообновление до этой правки не сообщало о провале никак:
                    // расписание только писало в журнал.
                    if !is_mannual_trigger {
                        announce_the_failure(uid, "update_failed", &err.to_string()).await;
                    }
                    return Err(err);
                }
            }
        }
        None => auto_refresh,
    };

    if should_refresh {
        logging!(info, Type::Config, "[Обновление подписки] Обновляю конфиг ядра");
        apply_the_updated_profile(uid, snapshot, is_mannual_trigger).await?;
    }

    Ok(())
}

pub async fn enhance_profiles() -> Result<ValidationOutcome> {
    CoreManager::global().update_config_forced().await
}

const LOCK_GRACE_SECS: i64 = 72 * 60 * 60;

const LOCK_GRACE_INTERVALS: u64 = 3;

fn lock_grace_secs(item: &PrfItem) -> i64 {
    let interval_minutes = item.option.as_ref().and_then(|opt| opt.update_interval).unwrap_or(0);
    let by_interval = interval_minutes
        .saturating_mul(60)
        .saturating_mul(LOCK_GRACE_INTERVALS)
        .min(i64::MAX as u64) as i64;
    by_interval.max(LOCK_GRACE_SECS)
}

fn lock_expired(item: &PrfItem, now: i64) -> bool {
    if item.lock_mode != Some(true) {
        return false;
    }
    let Some(updated) = item.updated.filter(|value| *value > 0) else {
        return false;
    };
    now.saturating_sub(updated as i64) > lock_grace_secs(item)
}

pub async fn release_stale_panel_locks() {
    let now = chrono::Local::now().timestamp();

    let stale: Vec<String> = {
        let profiles = Config::profiles().await.latest_arc();
        profiles
            .items
            .as_deref()
            .unwrap_or_default()
            .iter()
            .filter(|item| lock_expired(item, now))
            .filter_map(|item| item.uid.clone())
            .collect()
    };

    if stale.is_empty() {
        return;
    }

    let released = Config::profiles()
        .await
        .with_data_modify(move |mut profiles| async move {
            let mut released = Vec::new();
            for item in profiles.items.as_mut().into_iter().flatten() {
                let Some(uid) = item.uid.clone() else { continue };
                if stale.contains(&uid) {
                    item.lock_mode = None;
                    released.push(uid);
                }
            }
            if !released.is_empty() {
                profiles.save_file().await?;
            }
            Ok((profiles, released))
        })
        .await;

    match released {
        Ok(released) if !released.is_empty() => {
            logging!(
                info,
                Type::Config,
                "[clod] panel lock released on {} profile(s): `clod-lock-mode` was not confirmed within the grace period",
                released.len()
            );
            for uid in released {
                handle::Handle::notify_profile_changed(&uid);
            }
            let _ = tray::Tray::global().update_menu().await;
        }
        Ok(_) => {}
        Err(err) => {
            logging!(
                warn,
                Type::Config,
                "Warning: [clod] failed to release stale lock: {err}"
            );
        }
    }
}

#[cfg(test)]
mod lock_expiry_tests {
    use super::*;

    const DAY: i64 = 24 * 60 * 60;

    fn locked_item(updated: i64, interval_minutes: Option<u64>) -> PrfItem {
        PrfItem {
            uid: Some("Rtest".into()),
            itype: Some("remote".into()),
            lock_mode: Some(true),
            updated: Some(updated as usize),
            option: interval_minutes.map(|update_interval| PrfOption {
                update_interval: Some(update_interval),
                ..PrfOption::default()
            }),
            ..PrfItem::default()
        }
    }

    #[test]
    fn fresh_lock_stays() {
        let now = 10 * DAY;
        assert!(!lock_expired(&locked_item(now - DAY, None), now));
    }

    #[test]
    fn silent_panel_releases_the_lock() {
        let now = 10 * DAY;
        assert!(lock_expired(&locked_item(now - 4 * DAY, None), now));
    }

    #[test]
    fn a_long_update_interval_stretches_the_grace() {
        let now = 100 * DAY;
        let weekly = 7 * 24 * 60;
        assert!(!lock_expired(&locked_item(now - 4 * DAY, Some(weekly)), now));
        assert!(lock_expired(&locked_item(now - 22 * DAY, Some(weekly)), now));
    }

    #[test]
    fn only_a_real_lock_expires() {
        let now = 10 * DAY;
        let mut unlocked = locked_item(now - 100 * DAY, None);
        unlocked.lock_mode = None;
        assert!(!lock_expired(&unlocked, now));

        let mut without_timestamp = locked_item(0, None);
        without_timestamp.updated = None;
        assert!(!lock_expired(&without_timestamp, now));
    }
}

#[cfg(test)]
mod failure_visibility_tests {
    use super::{failure_notice_status, public_failure_text};
    use crate::core::validate::{ValidationErrorKind, ValidationOutcome};

    #[test]
    fn each_kind_of_refusal_keeps_its_own_advice() {
        // На каждый вид отказа в приложении написан свой перевод с готовым советом,
        // и до этой правки все они на пути обновления сводились к одной общей фразе.
        assert_eq!(
            failure_notice_status(&Ok(ValidationOutcome::invalid(
                ValidationErrorKind::ProcessTerminated,
                "Процесс проверки был прерван"
            ))),
            "config_validate::process_terminated"
        );
        assert_eq!(
            failure_notice_status(&Ok(ValidationOutcome::invalid(
                ValidationErrorKind::ScriptSyntax,
                "script syntax error"
            ))),
            "config_validate::script_syntax_error"
        );
        assert_eq!(
            failure_notice_status(&Ok(ValidationOutcome::invalid(
                ValidationErrorKind::CoreRejected,
                "Parse config error"
            ))),
            "config_validate::error"
        );
    }

    #[test]
    fn a_failure_the_core_did_not_judge_stays_a_plain_update_failure() {
        assert_eq!(
            failure_notice_status(&Err(anyhow::anyhow!("не записался файл"))),
            "update_failed"
        );
        assert_eq!(failure_notice_status(&Ok(ValidationOutcome::Busy)), "update_failed");
    }

    #[test]
    fn the_notice_carries_neither_the_address_nor_a_short_token() {
        // Длинный сегмент прячет ещё `mask_err`, короткий — только `redact`:
        // без него `/s/ab12cd` уезжал бы в тост целиком.
        for raw in [
            "failed to fetch https://panel.example/sub/SECRET-TOKEN-VALUE-1234 while reading",
            "failed to fetch https://panel.example/s/ab12cd while reading",
            "request failed: authorization: Bearer ab12cd",
        ] {
            let shown = public_failure_text(raw);
            assert!(!shown.contains("ab12cd"), "{shown}");
            assert!(!shown.contains("SECRET-TOKEN-VALUE-1234"), "{shown}");
        }
    }

    #[test]
    fn the_notice_does_not_carry_the_os_user_name() {
        // `scrub_home` работает от переменных окружения, поэтому проверяем его
        // напрямую: в тесте домашний каталог тот же, что у приложения.
        let Some(home) = crate::utils::redact::home_prefix() else {
            return;
        };

        let raw = std::format!("failed to read {home}/.config/clod/profiles.yaml");
        let shown = public_failure_text(raw.as_str());

        assert!(!shown.contains(home.as_str()), "{shown}");
        assert!(shown.contains('~'), "{shown}");
    }
}

#[cfg(test)]
mod update_budget_tests {
    use super::{LADDER_SLACK, PrfOption, address_budget, keep_the_clearer_error};
    use std::time::Duration;

    fn option(timeout: Option<u64>, secure: Option<bool>) -> PrfOption {
        PrfOption {
            timeout_seconds: timeout,
            secure,
            ..PrfOption::default()
        }
    }

    #[test]
    fn the_default_ladder_fits_into_its_budget() {
        // Три ступени по 20 с, каждая с возможным повтором на запасных корнях TLS.
        assert_eq!(address_budget(None), Duration::from_secs(120) + LADDER_SLACK);
        assert_eq!(
            address_budget(Some(&option(Some(20), None))),
            Duration::from_secs(120) + LADDER_SLACK
        );
    }

    #[test]
    fn the_secure_channel_gets_its_second_attempt() {
        // Защищённый канал повторяет запрос без закрепления ключа — ступень стоит вдвое.
        assert_eq!(
            address_budget(Some(&option(Some(20), Some(true)))),
            Duration::from_secs(240) + LADDER_SLACK
        );
    }

    #[test]
    fn a_users_own_timeout_is_never_undercut() {
        // Числа здесь посчитаны руками, а не теми же константами, что и код: иначе
        // тест был бы тождественно истинным и уронённую константу не поймал бы.
        // Лестница — три ступени; каждая может быть повторена с запасными корнями
        // TLS; в защищённом канале — ещё раз без закрепления ключа прослойки.
        for (timeout, secure, honest_seconds) in [
            (1_u64, None, 6_u64),
            (5, None, 30),
            (20, None, 120),
            (60, None, 360),
            (600, None, 3600),
            (1, Some(true), 12),
            (20, Some(true), 240),
            (600, Some(true), 7200),
        ] {
            let budget = address_budget(Some(&option(Some(timeout), secure)));
            assert!(
                budget >= Duration::from_secs(honest_seconds),
                "бюджет {budget:?} короче честной лестницы {honest_seconds} с при timeout={timeout}"
            );
        }
    }

    #[test]
    fn an_absurd_timeout_does_not_panic_on_the_deadline() {
        // Именно здесь и была бы паника: `Instant + Duration::MAX`.
        for timeout in [u64::MAX, u64::MAX / 2, 1_000_000_000_000_000_000] {
            for secure in [None, Some(true)] {
                let budget = address_budget(Some(&option(Some(timeout), secure)));
                let deadline = tokio::time::Instant::now() + budget;
                assert!(deadline > tokio::time::Instant::now());
            }
        }
    }

    #[test]
    fn the_budget_never_hides_a_real_reason() {
        let real = anyhow::anyhow!("clod-sub-link-list: the panel returned a base64 link list");
        let budget = anyhow::anyhow!("clod-sub-budget: адрес подписки не ответил за отведённое время");

        assert!(
            keep_the_clearer_error(real, budget)
                .to_string()
                .contains("clod-sub-link-list")
        );
    }

    #[test]
    fn a_named_reason_is_not_lost_to_a_nameless_network_failure() {
        let named =
            anyhow::anyhow!("clod-sub-downgrade: the subscription address redirects to an insecure http address");
        let nameless = anyhow::anyhow!("failed to fetch remote profile");

        assert!(
            keep_the_clearer_error(named, nameless)
                .to_string()
                .contains("clod-sub-downgrade")
        );
    }

    #[test]
    fn a_real_reason_replaces_an_earlier_budget_failure() {
        let budget = anyhow::anyhow!("clod-sub-budget: адрес подписки не ответил за отведённое время");
        let real = anyhow::anyhow!("failed to fetch remote profile with status 403 Forbidden");

        assert!(keep_the_clearer_error(budget, real).to_string().contains("403"));
    }
}
