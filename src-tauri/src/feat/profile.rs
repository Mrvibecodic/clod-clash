use crate::{
    cmd,
    config::{Config, PrfItem, PrfOption, profiles::profiles_draft_update_item_safe, sub_headers},
    core::{CoreManager, handle, tray, validate::ValidationOutcome},
    utils::help::{mask_err, mask_url},
};
use anyhow::{Result, bail};
use clash_verge_logging::{Type, logging, logging_error};
use smartstring::alias::String;
use tauri::Emitter as _;

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
            logging!(info, Type::Tray, "切换代理成功: {} -> {}", group_name, proxy_name);
            let _ = handle::Handle::app_handle().emit("verge://refresh-proxy-config", ());
            let _ = tray::Tray::global().update_menu().await;
            return;
        }
        Err(err) => {
            logging!(
                error,
                Type::Tray,
                "切换代理失败: {} -> {}, 错误: {:?}",
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
            logging!(info, Type::Tray, "代理切换回退成功: {} -> {}", group_name, proxy_name);
            let _ = tray::Tray::global().update_menu().await;
        }
        Err(err) => {
            logging!(
                error,
                Type::Tray,
                "代理切换最终失败: {} -> {}, 错误: {:?}",
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
        logging!(info, Type::Config, "[订阅更新] {uid} 不是远程订阅，跳过更新");
        Ok(None)
    } else if item.url.is_none() {
        logging!(warn, Type::Config, "Warning: [订阅更新] {uid} 缺少URL，无法更新");
        bail!("failed to get the profile item url");
    } else if !ignore_auto_update && !item.option.as_ref().and_then(|o| o.allow_auto_update).unwrap_or(true) {
        logging!(info, Type::Config, "[订阅更新] {} 禁止自动更新，跳过更新", uid);
        Ok(None)
    } else {
        logging!(
            info,
            Type::Config,
            "[订阅更新] {} 是远程订阅，URL: {}",
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
async fn fetch_with_proxy_ladder(url: &String, base: Option<&PrfOption>) -> Result<PrfItem> {
    let mut attempt = base.cloned();
    let mut last_err = match PrfItem::from_url(url, None, None, attempt.as_ref()).await {
        Ok(item) => return Ok(item),
        Err(err) => err,
    };

    for (self_proxy, with_proxy) in [(true, false), (false, true)] {
        let opt = attempt.get_or_insert_with(PrfOption::default);
        opt.self_proxy = Some(self_proxy);
        opt.with_proxy = Some(with_proxy);

        match PrfItem::from_url(url, None, None, attempt.as_ref()).await {
            Ok(item) => return Ok(item),
            Err(err) => last_err = err,
        }
    }

    Err(last_err)
}

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
            "Warning: [订阅更新] [clod] ignoring migration to {}: {} consecutive hops already followed",
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
                    "[订阅更新] [clod] provider migrated the subscription URL to {}",
                    mask_url(&candidate)
                );
                handle::Handle::notice_message("clod_sub::url_migrated", mask_url(&candidate));
            }
            Err(err) => logging!(
                warn,
                Type::Config,
                "Warning: [订阅更新] [clod] failed to persist the migrated subscription URL: {}",
                mask_err(&err.to_string())
            ),
        },
        Err(err) => logging!(
            warn,
            Type::Config,
            "Warning: [订阅更新] [clod] candidate URL {} failed verification, keeping the current one: {}",
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
    logging!(info, Type::Config, "[订阅更新] 开始下载新的订阅内容");
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
            logging!(info, Type::Config, "[订阅更新] 更新订阅配置成功");
            apply_updated_item(uid, &mut item).await?;
            return Ok(is_current);
        }
        Err(err) => {
            logging!(
                warn,
                Type::Config,
                "Warning: [订阅更新] 正常更新失败: {}，尝试使用Clash代理更新",
                mask_err(&err.to_string())
            );
            last_err = err;
        }
    }

    merged_opt.get_or_insert_with(PrfOption::default).self_proxy = Some(true);
    merged_opt.get_or_insert_with(PrfOption::default).with_proxy = Some(false);

    match PrfItem::from_url(url, None, None, merged_opt.as_ref()).await {
        Ok(mut item) => {
            logging!(info, Type::Config, "[订阅更新] 使用 Clash代理 更新订阅配置成功");
            apply_updated_item(uid, &mut item).await?;
            handle::Handle::notice_message("update_with_clash_proxy", profile_name);
            drop(last_err);
            return Ok(is_current);
        }
        Err(err) => {
            logging!(
                warn,
                Type::Config,
                "Warning: [订阅更新] Clash代理更新失败: {}，尝试使用系统代理更新",
                mask_err(&err.to_string())
            );
            last_err = err;
        }
    }

    merged_opt.get_or_insert_with(PrfOption::default).self_proxy = Some(false);
    merged_opt.get_or_insert_with(PrfOption::default).with_proxy = Some(true);

    match PrfItem::from_url(url, None, None, merged_opt.as_ref()).await {
        Ok(mut item) => {
            logging!(info, Type::Config, "[订阅更新] 使用 系统代理 更新订阅配置成功");
            apply_updated_item(uid, &mut item).await?;
            handle::Handle::notice_message("update_with_clash_proxy", profile_name);
            drop(last_err);
            return Ok(is_current);
        }
        Err(err) => {
            logging!(
                warn,
                Type::Config,
                "Warning: [订阅更新] 系统代理更新失败: {}，所有重试均已失败",
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
            "[订阅更新] [clod] primary URL failed, trying the provider spare address {}",
            mask_url(&spare)
        );

        match fetch_with_proxy_ladder(&spare, merged_opt.as_ref()).await {
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
                    "Warning: [订阅更新] [clod] spare address failed as well: {}",
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
    logging!(info, Type::Config, "[订阅更新] 开始更新订阅 {}", uid);
    let url_opt = should_update_profile(uid, ignore_auto_update).await?;

    let should_refresh = match url_opt {
        Some(target) => {
            perform_profile_update(
                uid,
                &target.url,
                target.option.as_ref(),
                option,
                is_mannual_trigger,
                target.fallback_url,
                target.fallback_domain,
            )
            .await?
                && auto_refresh
        }
        None => auto_refresh,
    };

    if should_refresh {
        logging!(info, Type::Config, "[订阅更新] 更新内核配置");
        match CoreManager::global().update_config_with_force(is_mannual_trigger).await {
            Ok(outcome) if outcome.is_valid() => {
                logging!(info, Type::Config, "[订阅更新] 更新成功");
                handle::Handle::refresh_clash();
                // clod:F7 — fresh panel data, recompute the notification state
                crate::process::AsyncHandler::spawn(|| async {
                    crate::module::sub_watcher::run_check().await;
                });
            }
            Ok(outcome @ (ValidationOutcome::Skipped { .. } | ValidationOutcome::Busy)) if !is_mannual_trigger => {
                logging!(info, Type::Config, "[订阅更新] 本次配置刷新已跳过: {}", outcome);
            }
            Ok(outcome) => {
                let message = outcome.to_string();
                logging!(error, Type::Config, "[订阅更新] 更新失败: {}", message);
                handle::Handle::notice_message("update_failed", message);
            }
            Err(err) => {
                logging!(error, Type::Config, "[订阅更新] 更新失败: {}", err);
                handle::Handle::notice_message("update_failed", format!("{err}"));
                logging!(error, Type::Config, "{err}");
            }
        }
    }

    Ok(())
}

/// 增强配置
pub async fn enhance_profiles() -> Result<ValidationOutcome> {
    CoreManager::global().update_config_forced().await
}
