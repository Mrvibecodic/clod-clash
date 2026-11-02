use crate::{
    config::{Config, IClashTemp, PrfItem, profiles::profiles_set_mode_choice_safe},
    core::{
        CoreManager, handle,
        manager::{CommitFailed, Delivered, Delivery, Staged},
        validate::ValidationOutcome,
    },
    enhance::{
        Sources, dns_page,
        provider_reset::{Edit, Gathered, GroupView, Reset, SeenLine, gather, without_choice_and_edits},
    },
    utils::dirs,
};
use anyhow::{Result, bail};
use clash_verge_draft::SharedDraft;
use clash_verge_logging::{Type, logging};
use serde::Serialize;
use smartstring::alias::String;
use std::path::PathBuf;

#[derive(Debug, Serialize)]
pub struct ProviderValuesView {
    uid: String,
    name: String,
    groups: Vec<GroupView>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ProviderValuesOutcome {
    Returned,
    Stale,
}

pub async fn provider_values_view() -> Result<Option<ProviderValuesView>> {
    let profiles = Config::profiles().await.data_arc();
    let verge = Config::verge().await.data_arc();
    let clash = Config::clash().await.data_arc();
    let Some(gathered) = gather(&profiles, &verge).await? else {
        return Ok(None);
    };
    Ok(Some(ProviderValuesView {
        groups: gathered.differences(&clash.0, &verge),
        uid: gathered.uid,
        name: gathered.name,
    }))
}

pub async fn return_provider_values(uid: String, seen: Vec<SeenLine>) -> Result<ProviderValuesOutcome> {
    super::refuse_while_exiting()?;
    let _clash_serialized = super::patch_clash_lock().lock().await;
    let _verge_serialized = super::patch_verge_lock().lock().await;
    let Some(turn) = CoreManager::global().claim_for_an_update().await else {
        bail!("{}", clash_verge_i18n::t!("common.exitInProgress"));
    };

    let clash = Config::clash().await;
    let verge = Config::verge().await;
    let Some(gathered) = gather(&Config::profiles().await.data_arc(), &verge.data_arc()).await? else {
        return Ok(ProviderValuesOutcome::Stale);
    };
    if gathered.uid != uid {
        return Ok(ProviderValuesOutcome::Stale);
    }
    let view = gathered.differences(&clash.data_arc().0, &verge.data_arc());
    let Some(reset) = Reset::confirmed(&view, &seen) else {
        return Ok(ProviderValuesOutcome::Stale);
    };

    let mut candidate = (**clash.data_arc()).clone();
    candidate.patch_config(&reset.clash_patch());
    let candidate: SharedDraft<IClashTemp> = SharedDraft::new(Box::new(candidate));
    let page = match gathered.dns_page.clone().filter(|_| reset.touches_the_dns_page()) {
        Some(mut page) => {
            reset.apply_to_dns_page(&mut page);
            Some((page, dirs::dns_page_path(&gathered.uid)?))
        }
        None => None,
    };

    if reset.touches_verge() {
        verge.edit_draft(|draft| reset.apply_to_verge(draft));
    }
    let sources = reset_sources(&gathered, &reset, &candidate, page.as_ref()).await;
    let staged = CoreManager::global().stage_in_turn(turn, sources).await;
    let delivered = match deliver(staged, &gathered, &reset, &candidate, page.as_ref()).await {
        Ok(delivered) => delivered,
        Err(error) => {
            if reset.touches_verge() {
                verge.discard();
            }
            if error.downcast_ref::<CommitFailed>().is_some() {
                logging!(
                    warn,
                    Type::Config,
                    "provider values: the core took the build but the settings were not written, rebuilding from them: {error:#}"
                );
                match super::enhance_profiles().await {
                    Ok(ValidationOutcome::Valid) => {}
                    Ok(outcome) => logging!(
                        error,
                        Type::Config,
                        "provider values: rebuild after a failed write: {outcome}"
                    ),
                    Err(rebuild) => logging!(
                        error,
                        Type::Config,
                        "provider values: rebuild after a failed write: {rebuild:#}"
                    ),
                }
            }
            return Err(error);
        }
    };

    let mut unsaved = Vec::new();
    if let Err(error) = candidate.save_config().await {
        unsaved.push(error);
    }
    if reset.touches_verge()
        && let Err(error) = verge.data_arc().save_file().await
    {
        unsaved.push(error);
    }
    super::profile::settle_after_delivery(delivered);
    handle::Handle::refresh_verge();
    handle::Handle::refresh_profiles();
    if let Some(mode) = reset.mode.as_deref() {
        super::the_mode_changed(mode).await;
        if verge.data_arc().auto_close_connection() {
            super::clash::close_connections_after_mode_change();
        }
    }
    if !unsaved.is_empty() {
        let reasons: Vec<_> = unsaved.iter().map(|error| format!("{error:#}")).collect();
        bail!("{}", reasons.join("; "));
    }
    Ok(ProviderValuesOutcome::Returned)
}

async fn reset_sources(
    gathered: &Gathered,
    reset: &Reset,
    candidate: &SharedDraft<IClashTemp>,
    page: Option<&(dns_page::Page, PathBuf)>,
) -> Sources {
    let uid = gathered.uid.clone();
    let mode = reset.mode.is_some();
    let edits = reset.edits.clone();
    let mut sources = Sources::default()
        .with_clash(std::sync::Arc::clone(candidate))
        .with_profiles_derived(move |accepted| {
            let mut candidate = accepted.clone();
            without_choice_and_edits(&mut candidate, &uid, mode, &edits)?;
            Ok(candidate)
        });
    if reset.touches_verge() {
        sources = sources.with_verge(Config::verge().await.latest_arc());
    }
    if let Some((page, _)) = page {
        sources = sources.with_dns_page(gathered.uid.clone(), page.clone());
    }
    sources
}

async fn deliver(
    staged: Result<Result<Staged<'_>, ValidationOutcome>>,
    gathered: &Gathered,
    reset: &Reset,
    candidate: &SharedDraft<IClashTemp>,
    page: Option<&(dns_page::Page, PathBuf)>,
) -> Result<Delivered> {
    let mode = reset.mode.is_some();
    let staged = match staged? {
        Ok(staged) => staged,
        Err(outcome) => bail!("{outcome}"),
    };

    let clash = Config::clash().await;
    let verge = Config::verge().await;
    let commit = async || {
        let uid = &gathered.uid;
        let previous_mode = match mode {
            true => Some(profiles_set_mode_choice_safe(uid, None).await?),
            false => None,
        };
        let written = match write_the_chain_templates(gathered, &reset.edits).await {
            Ok(previous) => match page {
                Some((page, path)) => match page.save(path).await {
                    Ok(()) => Ok(()),
                    Err(error) => {
                        restore_the_chains(previous).await;
                        Err(error)
                    }
                },
                None => Ok(()),
            },
            Err(error) => Err(error),
        };
        if let Err(error) = written {
            if let Some(previous) = previous_mode
                && let Err(restore) = profiles_set_mode_choice_safe(uid, previous).await
            {
                logging!(
                    error,
                    Type::Config,
                    "provider values: mode choice not restored: {restore:#}"
                );
            }
            return Err(error);
        }
        clash.replace_shared(std::sync::Arc::clone(candidate));
        if reset.touches_verge() {
            verge.apply();
        }
        Ok(())
    };
    match staged.deliver_committing(Delivery::Reload, commit).await? {
        Ok(delivered) => Ok(delivered),
        Err(outcome) => bail!("{outcome}"),
    }
}

async fn write_the_chain_templates(gathered: &Gathered, edits: &[Edit]) -> Result<Vec<(PrfItem, String)>> {
    let mut previous = Vec::new();
    for (edit, item) in gathered.chain_items.iter().filter(|(edit, _)| edits.contains(edit)) {
        let written = match item.read_file().await {
            Ok(before) => item.save_file(edit.template().into()).await.map(|()| before),
            Err(error) => Err(error),
        };
        match written {
            Ok(before) => previous.push((item.clone(), before)),
            Err(error) => {
                restore_the_chains(previous).await;
                return Err(error);
            }
        }
    }
    Ok(previous)
}

async fn restore_the_chains(previous: Vec<(PrfItem, String)>) {
    for (item, before) in previous {
        if let Err(error) = item.save_file(before).await {
            logging!(
                error,
                Type::Config,
                "chain file {:?} not restored after a failed commit: {error:#}",
                item.file
            );
        }
    }
}
