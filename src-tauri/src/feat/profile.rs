use crate::{
    cmd,
    config::{Config, PrfItem, PrfOption, profiles::profiles_draft_update_item_safe, sub_headers},
    core::{CoreManager, handle, tray, validate::ValidationOutcome},
    utils::help::{mask_err, mask_url},
};
use anyhow::{Result, bail};
use clash_verge_logging::{Type, logging, logging_error};
use smartstring::alias::String;

/// Toggle proxy profile
pub async fn toggle_proxy_profile(profile_index: String) {
    logging_error!(
        Type::Config,
        cmd::patch_profiles_config_by_profile_index(profile_index).await
    );
}

pub async fn switch_proxy_node(group_name: &str, proxy_name: &str) {
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

// clod:fallback begin
/// What `perform_profile_update` needs to know about a profile before it starts.
struct UpdateTarget {
    url: String,
    option: Option<PrfOption>,
    /// Full spare address from a previous `fallback-url` header.
    fallback_url: Option<String>,
    /// Spare host for the primary address, from `fallback-domain`.
    fallback_domain: Option<String>,
}
// clod:fallback end

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
            // clod: remembered from the previous fallback headers
            fallback_url: item.fallback_url.clone(),
            fallback_domain: item.fallback_domain.clone(),
        }))
    }
}

// clod:fallback begin
/// Fetch a subscription URL through the same escalation the primary path uses:
/// as configured, then through our own core, then through the system proxy.
///
/// Only used for the `fallback-url` retry; the primary path below is left
/// untouched so upstream changes to it stay easy to merge.
/// Persist a freshly fetched subscription and honour a provider migration.
///
/// A `new-url` / `new-domain` header is only applied once a probe download of
/// the candidate produced a valid mihomo config, so a typo in the panel cannot
/// strand the user on a dead URL.
async fn apply_updated_item(uid: &String, item: &mut PrfItem) -> Result<()> {
    let migrate_url = item.migrate_url.clone();
    let request_option = item.option.clone();

    profiles_draft_update_item_safe(uid, item).await?;

    let Some(candidate) = migrate_url else {
        return Ok(());
    };

    // Two panels can point at each other; stop following after a few hops.
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

    // Reuse the fresh item's option: it already references this profile's
    // merge/script/rules/proxies/groups, so the probe cannot leak new ones.
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
// clod:fallback end

async fn perform_profile_update(
    uid: &String,
    url: &String,
    opt: Option<&PrfOption>,
    option: Option<&PrfOption>,
    is_mannual_trigger: bool,
    // clod: spare addresses from previous `fallback-*` headers
    fallback_url: Option<String>,
    fallback_domain: Option<String>,
) -> Result<bool> {
    logging!(
        info,
        Type::Config,
        "[Обновление подписки] Начинаю загрузку нового содержимого подписки"
    );
    let mut merged_opt = PrfOption::merge(opt, option);
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

    match PrfItem::from_url(url, None, None, merged_opt.as_ref()).await {
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

    match PrfItem::from_url(url, None, None, merged_opt.as_ref()).await {
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
            last_err = err;
        }
    }

    merged_opt.get_or_insert_with(PrfOption::default).self_proxy = Some(false);
    merged_opt.get_or_insert_with(PrfOption::default).with_proxy = Some(true);

    match PrfItem::from_url(url, None, None, merged_opt.as_ref()).await {
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
            last_err = err;
        }
    }

    // clod:fallback begin
    // Every attempt on the primary URL failed. The provider may have handed us a
    // spare address in a previous response (`fallback-url`); use it, but keep the
    // stored `url` untouched so the primary address is retried next time.
    // `fallback-url` first, then the primary address with the spare host from
    // `fallback-domain` — the order a panel expects.
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

        match PrfItem::from_url_with_ladder(&spare, None, None, merged_opt.as_ref()).await {
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
                last_err = err;
            }
        }
    }
    // clod:fallback end

    if is_mannual_trigger {
        handle::Handle::notice_message("update_failed_even_with_clash", format!("{profile_name} - {last_err}"));
    }
    Ok(is_current)
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
    let url_opt = should_update_profile(uid, ignore_auto_update).await?;

    let should_refresh = match url_opt {
        Some(target) => {
            let outcome = perform_profile_update(
                uid,
                &target.url,
                target.option.as_ref(),
                option,
                is_mannual_trigger,
                target.fallback_url,
                target.fallback_domain,
            )
            .await;
            match outcome {
                Ok(changed) => changed && auto_refresh,
                Err(err) => {
                    // clod:lock-expiry — до панели не дозвонились ни одним из
                    // путей лестницы. Это ровно тот случай, ради которого у
                    // замка есть срок годности: проверяем его здесь, а не
                    // только на старте, иначе приложение, которое неделями не
                    // перезапускают, замок бы не отпустило.
                    release_stale_panel_locks().await;
                    return Err(err);
                }
            }
        }
        None => auto_refresh,
    };

    if should_refresh {
        logging!(info, Type::Config, "[Обновление подписки] Обновляю конфиг ядра");
        match CoreManager::global().update_config_with_force(is_mannual_trigger).await {
            Ok(outcome) if outcome.is_valid() => {
                logging!(info, Type::Config, "[Обновление подписки] Обновление успешно");
                handle::Handle::refresh_clash();
                // clod: перезагрузка конфига сбрасывает выбор узлов в ядре —
                // восстанавливаем сохранённый выбор (и избранные) сразу же,
                // иначе после каждого обновления подписки слетал сервер
                if let Err(err) = crate::config::profiles::activate_selected_nodes() {
                    logging!(
                        warn,
                        Type::Config,
                        "Warning: [Обновление подписки] restore selection failed: {err}"
                    );
                }
                // clod:F7 — fresh panel data, recompute the notification state
                crate::process::AsyncHandler::spawn(|| async {
                    crate::module::sub_watcher::run_check().await;
                });
                // clod: логотип провайдера кладём в локальный кэш — иначе он
                // грузится с чужого хоста при каждом показе экрана
                let logo_uid = uid.clone();
                crate::process::AsyncHandler::spawn(move || async move {
                    crate::module::logo_cache::sync(&logo_uid).await;
                });
            }
            Ok(outcome @ (ValidationOutcome::Skipped { .. } | ValidationOutcome::Busy)) if !is_mannual_trigger => {
                logging!(
                    info,
                    Type::Config,
                    "[Обновление подписки] Обновление конфига на этот раз пропущено: {}",
                    outcome
                );
            }
            Ok(outcome) => {
                let message = outcome.to_string();
                logging!(
                    error,
                    Type::Config,
                    "[Обновление подписки] Обновление не удалось: {}",
                    message
                );
                handle::Handle::notice_message("update_failed", message);
            }
            Err(err) => {
                logging!(
                    error,
                    Type::Config,
                    "[Обновление подписки] Обновление не удалось: {}",
                    err
                );
                handle::Handle::notice_message("update_failed", format!("{err}"));
                logging!(error, Type::Config, "{err}");
            }
        }
    }

    Ok(())
}

/// Расширенный конфиг
pub async fn enhance_profiles() -> Result<ValidationOutcome> {
    CoreManager::global().update_config_forced().await
}

/// clod:lock-expiry — минимальный срок годности замка провайдера.
///
/// Замок (`clod-lock-mode`) держится, пока панель его подтверждает: каждое
/// успешное обновление подписки приносит заголовок заново, а исчезнувший
/// заголовок замок снимает (`merge_panel_meta`). Дыра была в третьем случае —
/// панель не отвечает вовсе. Домен забанили, провайдер закрылся, срок вышел —
/// подтверждать замок стало некому, и он оставался на устройстве навсегда,
/// причём выход («удалить подписку») нигде не объяснён.
const LOCK_GRACE_SECS: i64 = 72 * 60 * 60;

/// Во сколько раз срок годности замка длиннее интервала обновления подписки.
///
/// Абсолютного порога мало: панель с интервалом в неделю штатно молчит дольше
/// трёх суток, и фиксированный срок снимал бы замок на живой подписке. Три
/// пропущенных обновления подряд — это уже не «сеть моргнула».
const LOCK_GRACE_INTERVALS: u64 = 3;

/// Сколько замок живёт без подтверждения для конкретной подписки, в секундах.
fn lock_grace_secs(item: &PrfItem) -> i64 {
    let interval_minutes = item.option.as_ref().and_then(|opt| opt.update_interval).unwrap_or(0);
    let by_interval = interval_minutes
        .saturating_mul(60)
        .saturating_mul(LOCK_GRACE_INTERVALS)
        .min(i64::MAX as u64) as i64;
    by_interval.max(LOCK_GRACE_SECS)
}

/// Протух ли замок на этой подписке к моменту `now` (unix-секунды).
fn lock_expired(item: &PrfItem, now: i64) -> bool {
    if item.lock_mode != Some(true) {
        return false;
    }
    let Some(updated) = item.updated.filter(|value| *value > 0) else {
        // Замок есть, а отметки об удачном обновлении нет — штатно такого
        // профиля не бывает (`from_url` ставит `updated` вместе с
        // заголовками). Снимать замок по неизвестному возрасту нельзя: это
        // подарок тому, кто подчистит поле в profiles.yaml.
        return false;
    };
    now.saturating_sub(updated as i64) > lock_grace_secs(item)
}

/// clod:lock-expiry — снять замки, которые панель давно не подтверждала.
///
/// Чистим ПОЛЕ в профиле, а не заводим второе понятие «замок, но протухший»:
/// потребителей у `lock_mode` четверо (переключение режима, трей, экран
/// настроек, страница прокси), и второй источник истины разошёлся бы с первым
/// на первой же правке. Панель ожила — ближайшее удачное обновление принесёт
/// заголовок обратно, и замок вернётся.
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
        // Недельный интервал: четыре дня молчания для такой подписки — норма,
        // а три пропущенных круга подряд — уже нет.
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
