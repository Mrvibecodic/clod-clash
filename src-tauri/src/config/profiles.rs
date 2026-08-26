use super::{
    PrfOption,
    prfitem::{PrfItem, PrfSelected},
};
use crate::{
    core::{handle, tray::Tray},
    utils::{
        dirs::{self, PathBufExec as _},
        help,
    },
};
use anyhow::{Context as _, Result, bail};
use clash_verge_draft::Draft;
use clash_verge_logging::{Type, logging};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_yaml_ng::Mapping;
use smartstring::alias::String;
use std::{
    collections::{HashMap, HashSet},
    path::{Component, Path},
    sync::{
        LazyLock,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tauri_plugin_mihomo::models::{Proxies, ProxyType};
use tokio::task::JoinHandle;

#[allow(clippy::unwrap_used)]
static REGEX_PROFILE_FILE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"^(?:[RLmrpg][a-zA-Z0-9]+\.yaml|s[a-zA-Z0-9]+\.js)$").unwrap());

static ACTIVATE_SELECTED_TASK: LazyLock<Mutex<Option<JoinHandle<()>>>> = LazyLock::new(|| Mutex::new(None));
static ACTIVATE_SELECTED_GENERATION: AtomicU64 = AtomicU64::new(0);

const MIHOMO_OPERATION_TIMEOUT: Duration = Duration::from_secs(10);
const SELECTED_NODES_RECHECK_DELAY: Duration = Duration::from_secs(1);

#[derive(Default, Debug, Clone, Deserialize, Serialize)]
pub struct IProfiles {
    pub current: Option<String>,

    pub items: Option<Vec<PrfItem>>,
}

pub struct IProfilePreview<'a> {
    pub uid: &'a String,
    pub name: &'a String,
    pub is_current: bool,
}

#[derive(Debug, Clone)]
pub struct CleanupResult {
    pub total_files: usize,
    pub deleted_files: usize,
    pub failed_deletions: usize,
}

macro_rules! patch {
    ($lv: expr, $rv: expr, $key: tt) => {
        if ($rv.$key).is_some() {
            $lv.$key = $rv.$key.to_owned();
        }
    };
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct PendingProfileFiles(Vec<String>);

impl PendingProfileFiles {
    fn push(&mut self, file: String) {
        self.0.push(file);
    }

    pub fn files(&self) -> &[String] {
        &self.0
    }

    pub async fn cleanup(self) {
        if self.0.is_empty() {
            return;
        }
        let Ok(dir) = dirs::app_profiles_dir() else {
            logging!(
                warn,
                Type::Config,
                "Warning: каталог подписок недоступен, файлы удалённой подписки остались на диске"
            );
            return;
        };
        for file in self.0 {
            let _ = dir.join(file.as_str()).remove_if_exists().await;
        }
    }
}

impl IProfiles {
    fn take_item_file_by_uid(items: &mut Vec<PrfItem>, target_uid: Option<&str>) -> Option<String> {
        let index = items.iter().position(|item| item.uid.as_deref() == target_uid)?;
        items.remove(index).file
    }

    pub async fn new() -> Self {
        let path = match dirs::profiles_path() {
            Ok(p) => p,
            Err(err) => {
                logging!(error, Type::Config, "{err}");
                return Self::default();
            }
        };

        match help::read_yaml::<Self>(&path).await {
            Ok(mut profiles) => {
                let items = profiles.items.get_or_insert_with(Vec::new);
                for item in items.iter_mut() {
                    if item.uid.is_none() {
                        item.uid = Some(help::get_uid("d").into());
                    }
                }
                profiles
            }
            Err(err) => {
                logging!(error, Type::Config, "{err}");
                Self::default()
            }
        }
    }

    pub async fn save_file(&self) -> Result<()> {
        help::save_yaml(&dirs::profiles_path()?, self, Some("# Profiles Config for Clash Verge")).await
    }

    pub async fn migrate_item_url(&mut self, uid: &String, new_url: String) -> Result<()> {
        let found = self.items.as_mut().is_some_and(|items| {
            items
                .iter_mut()
                .find(|each| each.uid.as_ref() == Some(uid))
                .map(|each| each.record_url_migration(new_url))
                .is_some()
        });

        if !found {
            bail!("failed to find the profile item \"uid:{uid}\"");
        }

        self.save_file().await
    }

    pub fn patch_config(&mut self, patch: &Self) {
        if self.items.is_none() {
            self.items = Some(vec![]);
        }

        if let Some(current) = &patch.current
            && let Some(items) = self.items.as_ref()
        {
            let some_uid = Some(current);
            if items.iter().any(|e| e.uid.as_ref() == some_uid) {
                self.current = some_uid.cloned();
            }
        }
    }

    pub const fn get_current(&self) -> Option<&String> {
        self.current.as_ref()
    }

    pub const fn get_items(&self) -> Option<&Vec<PrfItem>> {
        self.items.as_ref()
    }

    pub fn get_item(&self, uid: impl AsRef<str>) -> Result<&PrfItem> {
        let uid_str = uid.as_ref();

        if let Some(items) = self.items.as_ref() {
            for each in items.iter() {
                if let Some(uid_val) = &each.uid
                    && uid_val.as_str() == uid_str
                {
                    return Ok(each);
                }
            }
        }

        bail!("failed to get the profile item \"uid:{}\"", uid_str);
    }

    pub fn set_selected_node(&mut self, group: &str, node: &str) -> bool {
        let Some(current) = self.current.clone() else {
            return false;
        };
        let Some(items) = self.items.as_mut() else {
            return false;
        };
        let Some(item) = items
            .iter_mut()
            .find(|item| item.uid.as_deref() == Some(current.as_str()))
        else {
            return false;
        };

        let selected = item.selected.get_or_insert_with(Vec::new);
        if let Some(entry) = selected.iter_mut().find(|entry| entry.name.as_deref() == Some(group)) {
            if entry.now.as_deref() == Some(node) {
                return false;
            }
            entry.now = Some(node.into());
        } else {
            selected.push(PrfSelected {
                name: Some(group.into()),
                now: Some(node.into()),
            });
        }
        true
    }

    pub async fn append_item(&mut self, item: &mut PrfItem) -> Result<()> {
        let uid = &item.uid;
        if uid.is_none() {
            bail!("the uid should not be null");
        }

        if let Some(file_data) = item.file_data.take() {
            if item.file.is_none() {
                bail!("the file should not be null");
            }

            let file = item
                .file
                .clone()
                .ok_or_else(|| anyhow::anyhow!("file field is required when file_data is provided"))?;
            let path = dirs::app_profiles_dir()?.join(file.as_str());

            help::write_atomic(&path, file_data.as_bytes())
                .await
                .with_context(|| format!("failed to write to file \"{file}\""))?;
        }

        if self.current.is_none() && (item.itype == Some("remote".into()) || item.itype == Some("local".into())) {
            self.current = uid.to_owned();
        }

        if self.items.is_none() {
            self.items = Some(vec![]);
        }

        if let Some(items) = self.items.as_mut() {
            items.push(item.to_owned());
        }

        Ok(())
    }

    pub async fn reorder(&mut self, active_id: &String, over_id: &String) -> Result<()> {
        let Some(items) = self.items.as_mut() else {
            return Ok(());
        };
        let old_index = items.iter().position(|item| item.uid.as_ref() == Some(active_id));
        let new_index = items.iter().position(|item| item.uid.as_ref() == Some(over_id));
        let (Some(old_idx), Some(new_idx)) = (old_index, new_index) else {
            return Ok(());
        };
        let item = items.remove(old_idx);
        items.insert(new_idx, item);
        self.save_file().await
    }

    pub async fn patch_item(&mut self, uid: &String, item: &PrfItem) -> Result<()> {
        if let Some(file) = &item.file {
            Self::validate_profile_file(file)?;
        }

        let mut items = self.items.take().unwrap_or_default();

        for each in items.iter_mut() {
            if each.uid.as_ref() == Some(uid) {
                patch!(each, item, itype);
                patch!(each, item, name);
                patch!(each, item, desc);
                patch!(each, item, file);
                patch!(each, item, url);
                patch!(each, item, selected);
                patch!(each, item, favorites);
                patch!(each, item, extra);
                patch!(each, item, updated);
                patch!(each, item, option);
                patch!(each, item, group);
                if item.name.is_some() {
                    each.name_customized = Some(true);
                }
                patch!(each, item, promo_seen);
                patch!(each, item, name_customized);
                patch!(each, item, fallback_url);
                patch!(each, item, fallback_domain);
                patch!(each, item, interval_locked);
                patch!(each, item, notified);

                self.items = Some(items);
                return self.save_file().await;
            }
        }

        self.items = Some(items);
        bail!("failed to find the profile item \"uid:{uid}\"")
    }

    fn validate_profile_file(file: &str) -> Result<()> {
        let mut components = Path::new(file).components();
        if file.is_empty()
            || file.contains('/')
            || file.contains('\\')
            || !matches!(
                (components.next(), components.next()),
                (Some(Component::Normal(_)), None)
            )
        {
            bail!("profile file must be a single filename");
        }

        Ok(())
    }

    pub async fn update_item(&mut self, uid: &String, item: &mut PrfItem) -> Result<()> {
        if self.items.is_none() {
            self.items = Some(vec![]);
        }

        let _ = self.get_item(uid)?;

        if let Some(items) = self.items.as_mut() {
            let some_uid = Some(uid.clone());

            for each in items.iter_mut() {
                if each.uid == some_uid {
                    each.extra = item.extra;
                    each.updated = item.updated;
                    each.home = item.home.to_owned();
                    each.option = PrfOption::merge(each.option.as_ref(), item.option.as_ref());
                    each.merge_panel_meta(item);
                    if let Some(file_data) = item.file_data.take() {
                        let file = each.file.take();
                        let file =
                            file.unwrap_or_else(|| item.file.take().unwrap_or_else(|| format!("{}.yaml", uid).into()));

                        each.file = Some(file.clone());

                        let path = dirs::app_profiles_dir()?.join(file.as_str());

                        help::write_atomic(&path, file_data.as_bytes())
                            .await
                            .with_context(|| format!("failed to write to file \"{file}\""))?;
                    }

                    break;
                }
            }
        }

        self.save_file().await
    }

    pub async fn delete_item(&mut self, uid: &String) -> Result<(bool, PendingProfileFiles)> {
        let outcome = self.plan_delete_item(uid)?;
        self.save_file().await?;
        Ok(outcome)
    }

    fn plan_delete_item(&mut self, uid: &String) -> Result<(bool, PendingProfileFiles)> {
        let current = self.current.as_ref().unwrap_or(uid);
        let current = current.clone();
        let delete_uids = {
            let item = self.get_item(uid)?;
            let option = item.option.as_ref();
            option.map_or(Vec::new(), |op| {
                [
                    op.merge.clone(),
                    op.script.clone(),
                    op.rules.clone(),
                    op.proxies.clone(),
                    op.groups.clone(),
                ]
                .into_iter()
                .collect::<Vec<_>>()
            })
        };
        let mut items = self.items.take().unwrap_or_default();
        let mut pending = PendingProfileFiles::default();

        if let Some(file) = Self::take_item_file_by_uid(&mut items, Some(uid.as_str())) {
            pending.push(file);
        }

        for delete_uid in delete_uids {
            if let Some(file) = Self::take_item_file_by_uid(&mut items, delete_uid.as_deref()) {
                pending.push(file);
            }
        }

        if current == *uid {
            self.current = None;
            for item in items.iter() {
                if item.itype == Some("remote".into()) || item.itype == Some("local".into()) {
                    self.current = item.uid.clone();
                    break;
                }
            }
        }

        self.items = Some(items);
        Ok((current == *uid, pending))
    }

    pub async fn current_mapping(&self) -> Result<Mapping> {
        match (self.current.as_ref(), self.items.as_ref()) {
            (Some(current), Some(items)) => {
                if let Some(item) = items.iter().find(|e| e.uid.as_ref() == Some(current)) {
                    let file_path = match item.file.as_ref() {
                        Some(file) => dirs::app_profiles_dir()?.join(file.as_str()),
                        None => bail!("failed to get the file field"),
                    };
                    return help::read_mapping(&file_path).await;
                }
                bail!("failed to find the current profile \"uid:{current}\"");
            }
            _ => Ok(Mapping::new()),
        }
    }

    pub fn is_current_profile_index(&self, index: &String) -> bool {
        self.current.as_ref() == Some(index)
    }

    pub fn profiles_preview(&self) -> Option<Vec<IProfilePreview<'_>>> {
        self.items.as_ref().map(|items| {
            items
                .iter()
                .filter_map(|e| {
                    if let (Some(uid), Some(name)) = (e.uid.as_ref(), e.name.as_ref()) {
                        let is_current = self.is_current_profile_index(uid);
                        let preview = IProfilePreview { uid, name, is_current };
                        Some(preview)
                    } else {
                        None
                    }
                })
                .collect()
        })
    }

    pub fn get_name_by_uid(&self, uid: &String) -> Option<&String> {
        if let Some(items) = &self.items {
            for item in items {
                if item.uid.as_ref() == Some(uid) {
                    return item.name.as_ref();
                }
            }
        }
        None
    }

    pub async fn cleanup_orphaned_files(&self) -> Result<()> {
        let profiles_dir = dirs::app_profiles_dir()?;
        self.cleanup_orphaned_files_in(&profiles_dir).await
    }

    pub(super) async fn cleanup_orphaned_files_in(&self, profiles_dir: &Path) -> Result<()> {
        if !profiles_dir.exists() {
            return Ok(());
        }

        if self.items.as_ref().is_none_or(|v| v.is_empty()) {
            logging!(
                warn,
                Type::Config,
                "Элементы Profile пусты, пропускаю очистку осиротевших файлов, чтобы не удалить активные конфиги"
            );
            return Ok(());
        }

        let active_files = self.get_all_active_files();

        let protected_files = self.get_protected_global_files();

        let mut total_files = 0;
        let mut deleted_files = 0;
        let mut failed_deletions = 0;

        let mut dir_entries = tokio::fs::read_dir(&profiles_dir).await?;
        while let Some(entry) = dir_entries.next_entry().await? {
            let path = entry.path();

            if !path.is_file() {
                continue;
            }

            total_files += 1;

            if let Some(file_name) = path.file_name().and_then(|n| n.to_str())
                && Self::is_profile_file(file_name)
            {
                if protected_files.contains(file_name) {
                    logging!(
                        debug,
                        Type::Config,
                        "Защищаю глобальный расширенный конфиг: {file_name}"
                    );
                    continue;
                }

                if !active_files.contains(file_name) {
                    match path.to_path_buf().remove_if_exists().await {
                        Ok(_) => {
                            deleted_files += 1;
                            logging!(debug, Type::Config, "Удалён лишний файл: {file_name}");
                        }
                        Err(e) => {
                            failed_deletions += 1;
                            logging!(
                                warn,
                                Type::Config,
                                "Warning: Не удалось удалить файл: {file_name} - {e}"
                            );
                        }
                    }
                }
            }
        }

        let result = CleanupResult {
            total_files,
            deleted_files,
            failed_deletions,
        };

        logging!(
            info,
            Type::Config,
            "Очистка файлов Profile завершена: всего файлов={}, удалено={}, ошибок={}",
            result.total_files,
            result.deleted_files,
            result.failed_deletions
        );

        Ok(())
    }

    fn get_protected_global_files(&self) -> HashSet<String> {
        let mut protected_files = HashSet::new();

        protected_files.insert("Merge.yaml".into());
        protected_files.insert("Script.js".into());

        protected_files
    }

    fn get_all_active_files(&self) -> HashSet<&str> {
        let mut active_files: HashSet<&str> = HashSet::new();

        if let Some(items) = &self.items {
            for item in items {
                if let Some(file) = &item.file {
                    active_files.insert(file);
                }

                if let Some(itype) = &item.itype
                    && (itype == "remote" || itype == "local")
                    && let Some(option) = &item.option
                {
                    if let Some(merge_uid) = &option.merge
                        && let Ok(merge_item) = self.get_item(merge_uid)
                        && let Some(file) = &merge_item.file
                    {
                        active_files.insert(file);
                    }

                    if let Some(script_uid) = &option.script
                        && let Ok(script_item) = self.get_item(script_uid)
                        && let Some(file) = &script_item.file
                    {
                        active_files.insert(file);
                    }

                    if let Some(rules_uid) = &option.rules
                        && let Ok(rules_item) = self.get_item(rules_uid)
                        && let Some(file) = &rules_item.file
                    {
                        active_files.insert(file);
                    }

                    if let Some(proxies_uid) = &option.proxies
                        && let Ok(proxies_item) = self.get_item(proxies_uid)
                        && let Some(file) = &proxies_item.file
                    {
                        active_files.insert(file);
                    }

                    if let Some(groups_uid) = &option.groups
                        && let Ok(groups_item) = self.get_item(groups_uid)
                        && let Some(file) = &groups_item.file
                    {
                        active_files.insert(file);
                    }
                }
            }
        }

        active_files
    }

    fn is_profile_file(filename: &str) -> bool {
        REGEX_PROFILE_FILE.is_match(filename)
    }
}

use crate::config::Config;

pub async fn profiles_append_item_with_filedata_safe(item: &PrfItem, file_data: Option<String>) -> Result<()> {
    let item = &mut PrfItem::from(item, file_data).await?;
    profiles_append_item_safe(item).await
}

pub async fn profiles_append_item_safe(item: &mut PrfItem) -> Result<()> {
    let profiles = Config::profiles().await;
    profiles_append_item_to_safe(&profiles, item).await
}

pub(super) async fn profiles_append_item_to_safe(profiles: &Draft<IProfiles>, item: &mut PrfItem) -> Result<()> {
    profiles
        .with_data_modify(|mut profiles| async move {
            profiles.append_item(item).await?;
            Ok((profiles, ()))
        })
        .await
}

pub async fn profiles_patch_item_safe(index: &String, item: &PrfItem) -> Result<()> {
    Config::profiles()
        .await
        .with_data_modify(|mut profiles| async move {
            profiles.patch_item(index, item).await?;
            Ok((profiles, ()))
        })
        .await
}

pub async fn profiles_delete_item_safe(index: &String) -> Result<(bool, PendingProfileFiles)> {
    Config::profiles()
        .await
        .with_data_modify(|mut profiles| async move {
            let deleted = profiles.delete_item(index).await?;
            Ok((profiles, deleted))
        })
        .await
}

pub async fn profiles_set_selected_node_safe(group: &str, node: &str) -> Result<()> {
    let changed = Config::profiles()
        .await
        .with_data_modify(|mut profiles| async move {
            let changed = profiles.set_selected_node(group, node);
            Ok((profiles, changed))
        })
        .await?;
    if changed {
        profiles_save_file_safe().await?;
    }
    Ok(())
}

pub async fn profiles_restore_snapshot_safe(snapshot: IProfiles) -> Result<()> {
    Config::profiles()
        .await
        .with_data_modify(|_current| async move {
            snapshot.save_file().await?;
            Ok((snapshot, ()))
        })
        .await
}

pub async fn profiles_reorder_safe(active_id: &String, over_id: &String) -> Result<()> {
    Config::profiles()
        .await
        .with_data_modify(|mut profiles| async move {
            profiles.reorder(active_id, over_id).await?;
            Ok((profiles, ()))
        })
        .await
}

pub async fn profiles_migrate_url_safe(index: &String, new_url: String) -> Result<()> {
    Config::profiles()
        .await
        .with_data_modify(|mut profiles| async move {
            profiles.migrate_item_url(index, new_url).await?;
            Ok((profiles, ()))
        })
        .await
}

pub async fn profiles_save_file_safe() -> Result<()> {
    Config::profiles()
        .await
        .with_data_modify(|profiles| async move {
            profiles.save_file().await?;
            Ok((profiles, ()))
        })
        .await
}

pub async fn profiles_draft_update_item_safe(index: &String, item: &mut PrfItem) -> Result<()> {
    Config::profiles()
        .await
        .with_data_modify(|mut profiles| async move {
            profiles.update_item(index, item).await?;
            Ok((profiles, ()))
        })
        .await
}

#[derive(Debug, PartialEq, Eq)]
struct SelectedNodesPlan {
    selected: Vec<PrfSelected>,
    activations: Vec<(String, String)>,
    repaired_count: usize,
}

fn node_is_available(available_nodes: &[std::string::String], node: &str) -> bool {
    available_nodes.iter().any(|available| available == node)
}

fn selected_nodes_need_confirmation(selected: &[PrfSelected], proxies: &Proxies) -> bool {
    selected.iter().any(|selected_item| {
        let (Some(group_name), Some(node)) = (&selected_item.name, &selected_item.now) else {
            return false;
        };
        let Some(group) = proxies.proxies.get(group_name.as_str()) else {
            return true;
        };
        let Some(available_nodes) = group.all.as_deref().filter(|nodes| !nodes.is_empty()) else {
            return true;
        };
        !node_is_available(available_nodes, node)
    })
}

fn first_available_favorite<'a>(favorites: &'a [String], available_nodes: &[std::string::String]) -> Option<&'a str> {
    favorites
        .iter()
        .map(|favorite| favorite.as_str())
        .find(|favorite| node_is_available(available_nodes, favorite))
}

fn reconcile_selected_nodes(
    selected: &[PrfSelected],
    favorites: &[String],
    previous: Option<&Proxies>,
    proxies: &Proxies,
) -> SelectedNodesPlan {
    let mut plan = SelectedNodesPlan {
        selected: Vec::with_capacity(selected.len()),
        activations: Vec::new(),
        repaired_count: 0,
    };
    let mut seen_groups = HashSet::new();
    let mut unique_selected = selected
        .iter()
        .rev()
        .filter(|item| item.name.as_ref().is_some_and(|name| seen_groups.insert(name.clone())))
        .collect::<Vec<_>>();
    unique_selected.reverse();
    plan.repaired_count += selected.len() - unique_selected.len();

    for selected_item in unique_selected {
        let (Some(group_name), Some(node)) = (&selected_item.name, &selected_item.now) else {
            plan.repaired_count += 1;
            continue;
        };
        let Some(group) = proxies.proxies.get(group_name.as_str()) else {
            if previous.is_some_and(|snapshot| !snapshot.proxies.contains_key(group_name.as_str())) {
                plan.repaired_count += 1;
            } else {
                plan.selected.push(selected_item.clone());
            }
            continue;
        };
        let Some(available_nodes) = group.all.as_deref().filter(|nodes| !nodes.is_empty()) else {
            plan.selected.push(selected_item.clone());
            continue;
        };
        let is_selectable_group = matches!(
            &group.proxy_type,
            ProxyType::Selector | ProxyType::URLTest | ProxyType::Fallback | ProxyType::LoadBalance
        );
        if !is_selectable_group {
            let preferred_node = group
                .now
                .as_deref()
                .filter(|current| node_is_available(available_nodes, current))
                .or_else(|| node_is_available(available_nodes, node).then_some(node.as_str()));
            if let Some(preferred_node) = preferred_node {
                if preferred_node != node.as_str() {
                    plan.repaired_count += 1;
                }
                plan.selected.push(PrfSelected {
                    name: Some(group_name.clone()),
                    now: Some(preferred_node.into()),
                });
            } else {
                plan.repaired_count += 1;
            }
            continue;
        }

        if node_is_available(available_nodes, node) {
            plan.selected.push(selected_item.clone());
            if matches!(group.proxy_type, ProxyType::Selector) && group.now.as_deref() != Some(node.as_str()) {
                plan.activations.push((group_name.clone(), node.clone()));
            }
            continue;
        }

        let missing_was_confirmed = previous
            .and_then(|snapshot| snapshot.proxies.get(group_name.as_str()))
            .and_then(|group| group.all.as_deref())
            .filter(|nodes| !nodes.is_empty())
            .is_some_and(|nodes| !node_is_available(nodes, node));
        if !missing_was_confirmed {
            plan.selected.push(selected_item.clone());
            continue;
        }

        plan.repaired_count += 1;
        let replacement = first_available_favorite(favorites, available_nodes).or_else(|| {
            group
                .now
                .as_deref()
                .filter(|current| node_is_available(available_nodes, current))
        });
        if let Some(replacement) = replacement {
            plan.selected.push(PrfSelected {
                name: Some(group_name.clone()),
                now: Some(replacement.into()),
            });
            if matches!(group.proxy_type, ProxyType::Selector) && group.now.as_deref() != Some(replacement) {
                plan.activations.push((group_name.clone(), replacement.into()));
            }
        }
    }

    plan
}

fn is_activation_current(generation: u64) -> bool {
    ACTIVATE_SELECTED_GENERATION.load(Ordering::Acquire) == generation
}

async fn fetch_proxies_with_timeout() -> Result<Proxies> {
    tokio::time::timeout(MIHOMO_OPERATION_TIMEOUT, async {
        loop {
            match handle::Handle::mihomo().await.get_proxies().await {
                Ok(proxies) => return proxies,
                Err(err) => {
                    logging!(debug, Type::Config, "mihomo proxies are not ready yet: {err}");
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
            }
        }
    })
    .await
    .context("timed out while waiting for mihomo proxies")
}

async fn select_node_with_timeout(group_name: &String, node: &String) -> Result<()> {
    tokio::time::timeout(MIHOMO_OPERATION_TIMEOUT, async {
        handle::Handle::mihomo()
            .await
            .select_node_for_group(group_name, node)
            .await
    })
    .await
    .with_context(|| format!("timed out while selecting node [{node}] for group [{group_name}]"))?
    .with_context(|| format!("failed to select node [{node}] for group [{group_name}]"))
}

fn remaining_activations(
    activations: &[(String, String)],
    completed: &HashMap<String, String>,
) -> Vec<(String, String)> {
    activations
        .iter()
        .filter(|(group_name, node)| completed.get(group_name) != Some(node))
        .cloned()
        .collect()
}

async fn apply_activations(
    activations: &[(String, String)],
    completed: &mut HashMap<String, String>,
    generation: u64,
) -> Option<usize> {
    let mut activated_count = 0;
    for (group_name, node) in remaining_activations(activations, completed) {
        if !is_activation_current(generation) {
            return None;
        }
        match select_node_with_timeout(&group_name, &node).await {
            Ok(()) => {
                if !is_activation_current(generation) {
                    return None;
                }
                logging!(
                    info,
                    Type::Config,
                    "Selected node for proxy: {group_name}, node: {node}"
                );
                completed.insert(group_name, node);
                activated_count += 1;
            }
            Err(err) => logging!(error, Type::Config, "{err:#}"),
        }
        if !is_activation_current(generation) {
            return None;
        }
    }
    Some(activated_count)
}

async fn update_tray_after_activation(generation: u64) {
    if !is_activation_current(generation) {
        return;
    }
    if let Err(err) = Tray::global().update_tooltip().await {
        logging!(
            warn,
            Type::Config,
            "failed to update tray tooltip after profile switch: {err:#}"
        );
    }

    if !is_activation_current(generation) {
        return;
    }
    if let Err(err) = Tray::global().update_menu().await {
        logging!(
            warn,
            Type::Config,
            "failed to update tray menu after profile switch: {err:#}"
        );
    }
}

async fn persist_reconciled_selected(
    profile_uid: &String,
    original_selected: &[PrfSelected],
    selected: Vec<PrfSelected>,
    generation: u64,
) -> Result<()> {
    if !is_activation_current(generation) {
        return Ok(());
    }

    let profiles = Config::profiles().await;
    let profile_uid = profile_uid.clone();
    let original_selected = original_selected.to_vec();
    let updated = profiles
        .with_data_modify(move |mut profiles| async move {
            if !is_activation_current(generation) || profiles.current.as_ref() != Some(&profile_uid) {
                return Ok((profiles, false));
            }

            let profile = profiles
                .items
                .as_mut()
                .and_then(|items| items.iter_mut().find(|item| item.uid.as_ref() == Some(&profile_uid)))
                .with_context(|| format!("failed to find the profile item \"uid:{profile_uid}\""))?;
            if profile.selected.as_deref().unwrap_or(&[]) != original_selected.as_slice() {
                return Ok((profiles, false));
            }

            profile.selected = (!selected.is_empty()).then_some(selected);
            profiles.save_file().await?;
            Ok((profiles, true))
        })
        .await?;

    if updated {
        handle::Handle::refresh_profiles();
    }
    Ok(())
}

async fn activate_selected_nodes_worker(
    profile_uid: String,
    selected: Vec<PrfSelected>,
    favorites: Vec<String>,
    generation: u64,
) -> Result<()> {
    let first_snapshot = fetch_proxies_with_timeout().await?;
    if !is_activation_current(generation) {
        return Ok(());
    }

    let needs_confirmation = selected_nodes_need_confirmation(&selected, &first_snapshot);
    let immediate_plan = reconcile_selected_nodes(&selected, &favorites, None, &first_snapshot);
    logging!(
        debug,
        Type::Config,
        "immediate selected nodes activation plan: {immediate_plan:?}"
    );

    let mut completed_activations = HashMap::new();
    if apply_activations(&immediate_plan.activations, &mut completed_activations, generation)
        .await
        .is_none()
    {
        return Ok(());
    }

    if is_activation_current(generation) {
        handle::Handle::refresh_clash();
    }

    let plan = if needs_confirmation {
        tokio::time::sleep(SELECTED_NODES_RECHECK_DELAY).await;
        if !is_activation_current(generation) {
            return Ok(());
        }
        let second_snapshot = fetch_proxies_with_timeout().await?;
        if !is_activation_current(generation) {
            return Ok(());
        }
        let confirmed_plan = reconcile_selected_nodes(&selected, &favorites, Some(&first_snapshot), &second_snapshot);
        logging!(
            debug,
            Type::Config,
            "confirmed selected nodes activation plan: {confirmed_plan:?}"
        );
        let Some(confirmed_activated_count) =
            apply_activations(&confirmed_plan.activations, &mut completed_activations, generation).await
        else {
            return Ok(());
        };
        if confirmed_activated_count > 0 && is_activation_current(generation) {
            handle::Handle::refresh_clash();
        }
        confirmed_plan
    } else {
        immediate_plan
    };
    if !is_activation_current(generation) {
        return Ok(());
    }

    if plan.repaired_count > 0 && is_activation_current(generation) {
        logging!(
            info,
            Type::Config,
            "repairing {} invalid selected node record(s) for profile {profile_uid}",
            plan.repaired_count
        );
        persist_reconciled_selected(&profile_uid, &selected, plan.selected, generation).await?;
    }

    Ok(())
}

pub fn activate_selected_nodes() -> Result<()> {
    logging!(info, Type::Config, "starting activating selected nodes");
    let mut active_task = ACTIVATE_SELECTED_TASK.lock();
    let generation = ACTIVATE_SELECTED_GENERATION.fetch_add(1, Ordering::AcqRel) + 1;
    let previous_task = active_task.take();

    let handle = tokio::spawn(async move {
        if let Some(previous_task) = previous_task {
            let _ = previous_task.await;
        }
        if !is_activation_current(generation) {
            return;
        }

        let result = async {
            let profiles = Config::profiles().await.latest_arc();
            let current = profiles.get_current().context("no current profile running")?.clone();
            let item = profiles.get_item(&current).context("failed to get current profile")?;
            let selected = item.selected.clone().unwrap_or_default();
            let favorites = item.favorites.clone().unwrap_or_default();

            if selected.is_empty() {
                if is_activation_current(generation) {
                    handle::Handle::refresh_clash();
                }
                return Ok(());
            }
            activate_selected_nodes_worker(current, selected, favorites, generation).await
        }
        .await;

        if is_activation_current(generation) {
            if let Err(err) = result {
                logging!(error, Type::Config, "failed to activate selected nodes: {err:#}");
                handle::Handle::refresh_clash();
            }
            update_tray_after_activation(generation).await;
            logging!(info, Type::Config, "activating selected nodes done!");
        }
    });
    *active_task = Some(handle);
    drop(active_task);
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use tauri_plugin_mihomo::models::Proxy;

    fn selected(group: &str, node: &str) -> PrfSelected {
        PrfSelected {
            name: Some(group.into()),
            now: Some(node.into()),
        }
    }

    fn proxies(groups: Vec<(&str, &[&str], Option<&str>)>) -> Proxies {
        Proxies {
            proxies: groups
                .into_iter()
                .map(|(name, all, now)| {
                    (
                        name.to_owned(),
                        Proxy {
                            name: name.to_owned(),
                            all: Some(all.iter().map(|node| (*node).to_owned()).collect()),
                            now: now.map(str::to_owned),
                            proxy_type: ProxyType::Selector,
                            ..Proxy::default()
                        },
                    )
                })
                .collect::<HashMap<_, _>>(),
        }
    }

    #[test]
    fn keeps_valid_selection_and_activates_when_needed() {
        let saved = vec![selected("group", "saved")];
        let plan = reconcile_selected_nodes(
            &saved,
            &[],
            None,
            &proxies(vec![("group", &["current", "saved"], Some("current"))]),
        );

        assert_eq!(plan.selected, saved);
        assert_eq!(plan.activations, vec![("group".into(), "saved".into())]);
        assert_eq!(plan.repaired_count, 0);
    }

    #[test]
    fn replaces_missing_node_with_valid_current_node() {
        let snapshot = proxies(vec![("group", &["current"], Some("current"))]);
        let plan = reconcile_selected_nodes(&[selected("group", "renamed-node")], &[], Some(&snapshot), &snapshot);

        assert_eq!(plan.selected, vec![selected("group", "current")]);
        assert!(plan.activations.is_empty());
        assert_eq!(plan.repaired_count, 1);
    }

    #[test]
    fn validates_membership_in_group_not_global_existence() {
        let snapshot = proxies(vec![
            ("group", &["current"], Some("current")),
            ("other-node", &[], None),
        ]);
        let plan = reconcile_selected_nodes(&[selected("group", "other-node")], &[], Some(&snapshot), &snapshot);

        assert_eq!(plan.selected, vec![selected("group", "current")]);
        assert!(plan.activations.is_empty());
        assert_eq!(plan.repaired_count, 1);
    }

    #[test]
    fn does_not_activate_non_selectable_groups() {
        let snapshot = Proxies {
            proxies: HashMap::from([(
                "group".to_owned(),
                Proxy {
                    name: "group".to_owned(),
                    all: Some(vec!["current".to_owned(), "saved".to_owned()]),
                    now: Some("current".to_owned()),
                    proxy_type: ProxyType::Direct,
                    ..Proxy::default()
                },
            )]),
        };

        let plan = reconcile_selected_nodes(&[selected("group", "saved")], &[], None, &snapshot);

        assert_eq!(plan.selected, vec![selected("group", "current")]);
        assert!(plan.activations.is_empty());
        assert_eq!(plan.repaired_count, 1);
    }

    #[test]
    fn does_not_impose_a_saved_node_on_automatic_groups() {
        for (proxy_type, label) in [
            (ProxyType::URLTest, "url-test"),
            (ProxyType::Fallback, "fallback"),
            (ProxyType::LoadBalance, "load-balance"),
        ] {
            let snapshot = Proxies {
                proxies: HashMap::from([(
                    "group".to_owned(),
                    Proxy {
                        name: "group".to_owned(),
                        all: Some(vec!["current".to_owned(), "saved".to_owned()]),
                        now: Some("current".to_owned()),
                        proxy_type,
                        ..Proxy::default()
                    },
                )]),
            };

            let saved = vec![selected("group", "saved")];
            let plan = reconcile_selected_nodes(&saved, &[], None, &snapshot);

            assert_eq!(plan.selected, saved);
            assert!(plan.activations.is_empty(), "{label} must pick for itself");
            assert_eq!(plan.repaired_count, 0);
        }
    }

    #[test]
    fn removes_selection_when_group_or_fallback_is_invalid() {
        let snapshot = proxies(vec![("group", &["valid"], Some("invalid-current"))]);
        let plan = reconcile_selected_nodes(
            &[
                selected("missing-group", "node"),
                selected("group", "missing-node"),
                PrfSelected::default(),
            ],
            &[],
            Some(&snapshot),
            &snapshot,
        );

        assert!(plan.selected.is_empty());
        assert!(plan.activations.is_empty());
        assert_eq!(plan.repaired_count, 3);
    }

    #[test]
    fn preserves_selection_until_missing_node_is_confirmed() {
        let saved = vec![selected("group", "saved")];
        let incomplete = proxies(vec![("group", &[], None)]);
        let complete = proxies(vec![("group", &["current"], Some("current"))]);

        let incomplete_plan = reconcile_selected_nodes(&saved, &[], None, &incomplete);
        let one_snapshot_plan = reconcile_selected_nodes(&saved, &[], None, &complete);

        assert_eq!(incomplete_plan.selected, saved);
        assert_eq!(incomplete_plan.repaired_count, 0);
        assert_eq!(one_snapshot_plan.selected, saved);
        assert_eq!(one_snapshot_plan.repaired_count, 0);
    }

    #[test]
    fn recovers_when_group_appears_in_second_snapshot() {
        let saved = vec![selected("group", "saved")];
        let incomplete = Proxies::default();
        let complete = proxies(vec![("group", &["current", "saved"], Some("current"))]);

        let plan = reconcile_selected_nodes(&saved, &[], Some(&incomplete), &complete);

        assert_eq!(plan.selected, saved);
        assert_eq!(plan.activations, vec![("group".into(), "saved".into())]);
        assert_eq!(plan.repaired_count, 0);
    }

    #[test]
    fn keeps_last_selection_for_duplicate_group_entries() {
        let saved = vec![selected("group", "old"), selected("group", "new")];
        let snapshot = proxies(vec![("group", &["old", "new"], Some("old"))]);

        let plan = reconcile_selected_nodes(&saved, &[], None, &snapshot);

        assert_eq!(plan.selected, vec![selected("group", "new")]);
        assert_eq!(plan.activations, vec![("group".into(), "new".into())]);
        assert_eq!(plan.repaired_count, 1);
    }

    #[test]
    fn activates_valid_nodes_before_confirming_invalid_records() {
        let saved = vec![selected("valid-group", "saved"), selected("stale-group", "missing")];
        let first_snapshot = proxies(vec![
            ("valid-group", &["current", "saved"], Some("current")),
            ("stale-group", &["fallback"], Some("fallback")),
        ]);

        assert!(selected_nodes_need_confirmation(&saved, &first_snapshot));
        let immediate_plan = reconcile_selected_nodes(&saved, &[], None, &first_snapshot);

        assert_eq!(immediate_plan.selected, saved);
        assert_eq!(immediate_plan.activations, vec![("valid-group".into(), "saved".into())]);
        assert_eq!(immediate_plan.repaired_count, 0);
    }

    #[test]
    fn favorites_replace_a_confirmed_missing_node() {
        let snapshot = proxies(vec![("group", &["other", "fav-b", "fav-a"], Some("other"))]);
        let favorites: Vec<String> = vec!["fav-a".into(), "fav-b".into()];

        let plan = reconcile_selected_nodes(&[selected("group", "gone")], &favorites, Some(&snapshot), &snapshot);

        assert_eq!(plan.selected, vec![selected("group", "fav-a")]);
        assert_eq!(plan.activations, vec![("group".into(), "fav-a".into())]);
        assert_eq!(plan.repaired_count, 1);
    }

    #[test]
    fn favorites_do_not_override_a_saved_selection() {
        let snapshot = proxies(vec![("group", &["manual", "fav"], Some("fav"))]);
        let favorites: Vec<String> = vec!["fav".into()];

        let plan = reconcile_selected_nodes(&[selected("group", "manual")], &favorites, None, &snapshot);

        assert_eq!(plan.selected, vec![selected("group", "manual")]);
        assert_eq!(plan.activations, vec![("group".into(), "manual".into())]);
        assert_eq!(plan.repaired_count, 0);
    }

    #[test]
    fn favorites_do_not_claim_groups_without_a_saved_selection() {
        let snapshot = proxies(vec![
            ("picked", &["manual", "fav"], Some("manual")),
            ("untouched", &["node", "fav"], Some("node")),
        ]);
        let favorites: Vec<String> = vec!["fav".into()];

        let plan = reconcile_selected_nodes(&[selected("picked", "manual")], &favorites, None, &snapshot);

        assert_eq!(plan.selected, vec![selected("picked", "manual")]);
        assert!(plan.activations.is_empty());
        assert_eq!(plan.repaired_count, 0);
    }

    #[test]
    fn skips_only_activations_that_already_succeeded() {
        let activations = vec![
            ("first-group".into(), "saved".into()),
            ("second-group".into(), "new".into()),
            ("first-group".into(), "replacement".into()),
        ];
        let completed = HashMap::from([("first-group".into(), "saved".into())]);

        assert_eq!(
            remaining_activations(&activations, &completed),
            vec![
                ("second-group".into(), "new".into()),
                ("first-group".into(), "replacement".into()),
            ]
        );
    }

    fn item(uid: &str, itype: &str, file: &str) -> PrfItem {
        PrfItem {
            uid: Some(uid.into()),
            itype: Some(itype.into()),
            file: Some(file.into()),
            ..PrfItem::default()
        }
    }

    fn profiles_with(items: Vec<PrfItem>, current: &str) -> IProfiles {
        IProfiles {
            current: Some(current.into()),
            items: Some(items),
        }
    }

    #[test]
    fn delete_plan_collects_the_profile_and_its_attachments() {
        let attachments = PrfOption {
            merge: Some("merge-uid".into()),
            script: Some("script-uid".into()),
            ..PrfOption::default()
        };

        let mut main = item("victim", "remote", "victim.yaml");
        main.option = Some(attachments);

        let mut profiles = profiles_with(
            vec![
                main,
                item("merge-uid", "merge", "merge.yaml"),
                item("script-uid", "script", "script.js"),
                item("keeper", "remote", "keeper.yaml"),
            ],
            "keeper",
        );

        let (was_current, pending) = profiles.plan_delete_item(&"victim".into()).unwrap();

        assert!(!was_current, "удалили не текущую подписку");
        assert_eq!(pending.files(), ["victim.yaml", "merge.yaml", "script.js"]);
        assert_eq!(
            profiles.items.as_ref().map(Vec::len),
            Some(1),
            "в списке остаётся только уцелевшая подписка"
        );
        assert_eq!(profiles.current.as_deref(), Some("keeper"));
    }

    #[test]
    fn deleting_the_current_profile_moves_the_pointer_to_a_survivor() {
        let mut profiles = profiles_with(
            vec![
                item("victim", "remote", "victim.yaml"),
                item("merge-uid", "merge", "merge.yaml"),
                item("keeper", "local", "keeper.yaml"),
            ],
            "victim",
        );

        let (was_current, pending) = profiles.plan_delete_item(&"victim".into()).unwrap();

        assert!(was_current, "удалили текущую подписку — конфиг надо пересобрать");
        assert_eq!(pending.files(), ["victim.yaml"]);
        assert_eq!(
            profiles.current.as_deref(),
            Some("keeper"),
            "указатель переезжает на подписку, а не на вложение"
        );
    }

    #[test]
    fn unknown_uid_changes_nothing() {
        let mut profiles = profiles_with(vec![item("keeper", "remote", "keeper.yaml")], "keeper");

        assert!(profiles.plan_delete_item(&"ghost".into()).is_err());
        let remaining: Vec<_> = profiles
            .items
            .iter()
            .flatten()
            .filter_map(|item| item.uid.clone())
            .collect();
        assert_eq!(remaining, ["keeper"], "неизвестный uid не трогает список");
        assert_eq!(profiles.current.as_deref(), Some("keeper"));
    }

    #[test]
    fn selecting_a_node_adds_and_replaces_only_its_group() {
        let mut profiles = profiles_with(vec![item("current", "remote", "current.yaml")], "current");

        assert!(profiles.set_selected_node("Германия", "de-1"));
        assert!(profiles.set_selected_node("Нидерланды", "nl-1"));
        assert!(profiles.set_selected_node("Германия", "de-2"));

        let selected = profiles
            .get_item("current")
            .unwrap()
            .selected
            .clone()
            .unwrap_or_default();
        let pairs: Vec<_> = selected
            .iter()
            .map(|entry| {
                (
                    entry.name.clone().unwrap_or_default(),
                    entry.now.clone().unwrap_or_default(),
                )
            })
            .collect();

        assert_eq!(
            pairs,
            vec![("Германия".into(), "de-2".into()), ("Нидерланды".into(), "nl-1".into())],
            "вторая группа не пострадала, первая обновилась на месте"
        );
    }

    #[test]
    fn selecting_the_same_node_twice_changes_nothing() {
        let mut profiles = profiles_with(vec![item("current", "remote", "current.yaml")], "current");

        assert!(profiles.set_selected_node("Германия", "de-1"));
        assert!(
            !profiles.set_selected_node("Германия", "de-1"),
            "повтор не считается изменением — лишней записи файла быть не должно"
        );
    }

    #[test]
    fn selection_without_a_current_profile_is_ignored() {
        let mut profiles = IProfiles {
            current: None,
            items: Some(vec![item("orphan", "remote", "orphan.yaml")]),
        };

        assert!(!profiles.set_selected_node("Германия", "de-1"));
        assert!(
            profiles.items.iter().flatten().all(|item| item.selected.is_none()),
            "без текущей подписки писать выбор некуда"
        );
    }

    #[tokio::test]
    async fn empty_plan_needs_no_directory() {
        PendingProfileFiles::default().cleanup().await;
    }
}
