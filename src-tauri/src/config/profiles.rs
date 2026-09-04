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
const SELECTED_NODES_READY_TIMEOUT: Duration = Duration::from_secs(20);
const PROXIES_POLL_INTERVAL: Duration = Duration::from_millis(500);
const PROXIES_STABLE_POLLS: u8 = 3;

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
                let panel_changed = item
                    .url
                    .as_ref()
                    .is_some_and(|fresh| points_at_another_panel(each.url.as_ref(), fresh));

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

                // Адрес подписки сменили — значит сменилась и панель, а `fallback-url`
                // с `fallback-domain` остались от прежней. Без этого сброса неудача
                // нового адреса тихо уводила обновление обратно к старому провайдеру.
                // Сброс идёт ПОСЛЕ патчей: карточка правки шлёт назад всю запись
                // целиком, вместе со старыми хвостами, и до патчей он был бы напрасен.
                // Свои хвосты новая панель пришлёт заголовками при первом же ответе.
                if panel_changed {
                    each.fallback_url = None;
                    each.fallback_domain = None;
                }

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

/// Ведёт ли новый адрес подписки к другой панели.
///
/// Сравниваем только origin — схему, хост и порт. Перевыпуск токена, лишний слэш,
/// другой регистр хоста и переставленные параметры запроса ведут к той же панели, и
/// её запасные адреса выбрасывать не за что: именно они и понадобятся, если новый
/// адрес не ответит.
fn points_at_another_panel(stored: Option<&String>, fresh: &String) -> bool {
    let Some(stored) = stored else {
        return false;
    };

    match (tauri::Url::parse(stored.as_str()), tauri::Url::parse(fresh.as_str())) {
        (Ok(stored), Ok(fresh)) => panel_key(&stored) != panel_key(&fresh),
        _ => stored.trim() != fresh.trim(),
    }
}

/// Схема, хост и порт — то, что делает панель панелью.
///
/// Считаем руками, а не через `Url::origin`: у нестандартных схем он непрозрачный и
/// не равен сам себе, и тогда даже неизменённый адрес считался бы сменой панели.
fn panel_key(url: &tauri::Url) -> (std::string::String, Option<std::string::String>, Option<u16>) {
    (
        url.scheme().to_owned(),
        url.host_str().map(str::to_owned),
        url.port_or_known_default(),
    )
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

/// Слепок рабочего профиля: тот YAML, который лежал на диске до попытки обновления.
///
/// Обновление подписки пишет файл ещё до того, как ядро скажет, годится ли новый
/// конфиг. Рантайм при отказе откатывается, а файл — нет, и после перезапуска
/// приложения от рабочего профиля не оставалось ничего.
///
/// В слепке намеренно только содержимое файла. Отметку `updated` откат не трогает:
/// понять, виновата ли в отказе ядра именно подписка, нельзя — ядро проверяет уже
/// слитый конфиг вместе с пользовательскими цепочками merge/script/rules/groups и
/// об одинаковой ошибке сообщает одинаково. Если бы откат возвращал и `updated`,
/// посторонняя поломка заставляла бы качать подписку заново на каждом тике.
/// Прочие поля (имя, выбранный узел, настройки) пользователь может править прямо во
/// время загрузки, и откат их тоже не трогает.
#[derive(Debug)]
pub struct ProfileSnapshot {
    uid: String,
    file: String,
    bytes: Vec<u8>,
}

pub async fn profiles_snapshot_item(uid: &String) -> Option<ProfileSnapshot> {
    let item = Config::profiles().await.data_arc().get_item(uid).ok().cloned()?;

    let file = item.file.as_ref()?;
    let path = dirs::app_profiles_dir().ok()?.join(file.as_str());
    let bytes = match tokio::fs::read(&path).await {
        Ok(bytes) => bytes,
        Err(err) => {
            logging!(
                warn,
                Type::Config,
                "Warning: [clod] не удалось снять слепок файла профиля {}: {}",
                file,
                err
            );
            return None;
        }
    };

    Some(ProfileSnapshot {
        uid: uid.clone(),
        file: file.clone(),
        bytes,
    })
}

pub async fn profiles_restore_item(snapshot: ProfileSnapshot) -> Result<()> {
    let ProfileSnapshot { uid, file, bytes } = snapshot;

    // Профиль могли удалить, пока шла загрузка: возвращать его файл на диск нельзя,
    // он остался бы сиротой.
    let still_ours = Config::profiles()
        .await
        .data_arc()
        .get_item(&uid)
        .is_ok_and(|item| item.file.as_ref() == Some(&file));
    if !still_ours {
        bail!("the profile \"uid:{uid}\" is gone, nothing to restore");
    }

    let path = dirs::app_profiles_dir()?.join(file.as_str());
    help::write_atomic(&path, &bytes)
        .await
        .with_context(|| format!("failed to restore the profile file \"{file}\""))
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

fn is_usable_replacement(candidate: &str, available_nodes: &[std::string::String]) -> bool {
    !crate::constants::policies::is_builtin(candidate) && node_is_available(available_nodes, candidate)
}

fn first_available_favorite<'a>(favorites: &'a [String], available_nodes: &[std::string::String]) -> Option<&'a str> {
    favorites
        .iter()
        .map(|favorite| favorite.as_str())
        .find(|favorite| is_usable_replacement(favorite, available_nodes))
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
                .filter(|current| is_usable_replacement(current, available_nodes))
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
                plan.selected.push(selected_item.clone());
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

        let replacement = first_available_favorite(favorites, available_nodes).or_else(|| {
            group
                .now
                .as_deref()
                .filter(|current| is_usable_replacement(current, available_nodes))
        });
        if let Some(replacement) = replacement {
            plan.repaired_count += 1;
            plan.selected.push(PrfSelected {
                name: Some(group_name.clone()),
                now: Some(replacement.into()),
            });
            if matches!(group.proxy_type, ProxyType::Selector) && group.now.as_deref() != Some(replacement) {
                plan.activations.push((group_name.clone(), replacement.into()));
            }
        } else {
            plan.selected.push(selected_item.clone());
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

fn selected_groups_are_filled(selected: &[PrfSelected], proxies: &Proxies) -> bool {
    selected.iter().all(|item| {
        let Some(group_name) = item.name.as_deref() else {
            return true;
        };
        proxies
            .proxies
            .get(group_name)
            .and_then(|group| group.all.as_deref())
            .is_some_and(|nodes| !nodes.is_empty())
    })
}

fn groups_fingerprint(proxies: &Proxies) -> Vec<(String, usize)> {
    let mut fingerprint = proxies
        .proxies
        .iter()
        .map(|(name, group)| (name.as_str().into(), group.all.as_ref().map_or(0, Vec::len)))
        .collect::<Vec<(String, usize)>>();
    fingerprint.sort();
    fingerprint
}

async fn fetch_settled_proxies(selected: &[PrfSelected], generation: u64) -> Result<(Proxies, bool)> {
    let deadline = tokio::time::Instant::now() + SELECTED_NODES_READY_TIMEOUT;
    let mut last_snapshot: Option<Proxies> = None;
    let mut last_fingerprint = Vec::new();
    let mut stable_polls = 0u8;

    loop {
        match handle::Handle::mihomo().await.get_proxies().await {
            Ok(proxies) => {
                if selected_groups_are_filled(selected, &proxies) {
                    return Ok((proxies, true));
                }

                let fingerprint = groups_fingerprint(&proxies);
                if last_snapshot.is_some() && fingerprint == last_fingerprint {
                    stable_polls += 1;
                    if stable_polls >= PROXIES_STABLE_POLLS {
                        logging!(
                            debug,
                            Type::Config,
                            "mihomo groups stopped changing without every saved group; treating the core as warmed up"
                        );
                        return Ok((proxies, true));
                    }
                } else {
                    stable_polls = 0;
                }
                last_fingerprint = fingerprint;
                last_snapshot = Some(proxies);
            }
            Err(err) => {
                logging!(debug, Type::Config, "mihomo proxies are not ready yet: {err}");
                stable_polls = 0;
            }
        }

        if !is_activation_current(generation) || tokio::time::Instant::now() >= deadline {
            break;
        }
        tokio::time::sleep(PROXIES_POLL_INTERVAL).await;
    }

    match last_snapshot {
        Some(proxies) => Ok((proxies, false)),
        None => bail!("timed out while waiting for mihomo proxies"),
    }
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
    let (first_snapshot, groups_are_settled) = fetch_settled_proxies(&selected, generation).await?;
    if !is_activation_current(generation) {
        return Ok(());
    }

    if !groups_are_settled {
        logging!(
            warn,
            Type::Config,
            "the core has not filled every saved group in time; activating what is there and keeping the records untouched"
        );
    }

    let needs_confirmation = groups_are_settled && selected_nodes_need_confirmation(&selected, &first_snapshot);
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

    if plan.repaired_count > 0 && groups_are_settled && is_activation_current(generation) {
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

    #[test]
    fn the_same_panel_is_not_a_new_panel() {
        use super::points_at_another_panel;
        let stored = String::from("https://Panel.Example/sub/abc");

        for same in [
            "https://panel.example/sub/abc",
            "https://panel.example/sub/abc/",
            "https://panel.example/sub/xyz",
            "https://panel.example:443/sub/abc?b=2&a=1",
        ] {
            assert!(
                !points_at_another_panel(Some(&stored), &String::from(same)),
                "{same} — та же панель"
            );
        }
    }

    #[test]
    fn another_host_is_another_panel() {
        use super::points_at_another_panel;
        let stored = String::from("https://panel.example/sub/abc");

        for other in [
            "https://other.example/sub/abc",
            "https://panel.example:8443/sub/abc",
            "http://panel.example/sub/abc",
        ] {
            assert!(
                points_at_another_panel(Some(&stored), &String::from(other)),
                "{other} — другая панель"
            );
        }
    }

    #[test]
    fn without_a_stored_address_nothing_is_reset() {
        use super::points_at_another_panel;
        assert!(!points_at_another_panel(
            None,
            &String::from("https://panel.example/sub")
        ));
    }
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
    fn snapshot_is_filled_only_when_every_saved_group_has_nodes() {
        let saved = vec![selected("group", "node")];

        assert!(selected_groups_are_filled(
            &saved,
            &proxies(vec![("group", &["node", "other"], Some("other"))])
        ));
        assert!(
            !selected_groups_are_filled(&saved, &proxies(vec![("group", &[], None)])),
            "an empty group means the core has not filled it yet"
        );
        assert!(
            !selected_groups_are_filled(&saved, &proxies(vec![("another", &["node"], None)])),
            "a missing group means the core has not filled it yet"
        );
    }

    #[test]
    fn fingerprint_tracks_group_names_and_sizes() {
        let one = proxies(vec![("a", &["n1"], None), ("b", &[], None)]);
        let same = proxies(vec![("b", &[], None), ("a", &["n1"], None)]);
        let grown = proxies(vec![("a", &["n1", "n2"], None), ("b", &[], None)]);

        assert_eq!(
            groups_fingerprint(&one),
            groups_fingerprint(&same),
            "the order of groups in the answer must not matter"
        );
        assert_ne!(
            groups_fingerprint(&one),
            groups_fingerprint(&grown),
            "a group that gained nodes means the core is still filling them"
        );
    }

    #[test]
    fn snapshot_is_filled_for_records_without_a_group_name() {
        let nameless = vec![PrfSelected {
            name: None,
            now: Some("node".into()),
        }];

        assert!(selected_groups_are_filled(&nameless, &proxies(vec![])));
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
    fn removes_selection_only_when_the_group_itself_is_gone() {
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

        assert_eq!(plan.selected, vec![selected("group", "missing-node")]);
        assert!(plan.activations.is_empty());
        assert_eq!(plan.repaired_count, 2);
    }

    #[test]
    fn a_dead_group_never_overwrites_the_saved_node() {
        let snapshot = proxies(vec![("group", &["REJECT"], Some("REJECT"))]);
        let saved = vec![selected("group", "my-node")];

        let plan = reconcile_selected_nodes(&saved, &[], Some(&snapshot), &snapshot);

        assert_eq!(plan.selected, saved, "a builtin policy is not a replacement");
        assert!(plan.activations.is_empty());
        assert_eq!(
            plan.repaired_count, 0,
            "nothing was repaired, so the record must not be persisted"
        );
    }

    #[test]
    fn the_saved_node_is_restored_once_it_comes_back() {
        let dead = proxies(vec![("group", &["REJECT"], Some("REJECT"))]);
        let alive = proxies(vec![("group", &["REJECT", "my-node"], Some("REJECT"))]);
        let saved = vec![selected("group", "my-node")];

        let plan = reconcile_selected_nodes(&saved, &[], Some(&dead), &alive);

        assert_eq!(plan.selected, saved);
        assert_eq!(plan.activations, vec![("group".into(), "my-node".into())]);
        assert_eq!(plan.repaired_count, 0);
    }

    #[test]
    fn favorites_that_are_builtin_policies_are_skipped() {
        let snapshot = proxies(vec![("group", &["REJECT", "fav"], Some("REJECT"))]);
        let favorites: Vec<String> = vec!["REJECT".into(), "fav".into()];

        let plan = reconcile_selected_nodes(&[selected("group", "gone")], &favorites, Some(&snapshot), &snapshot);

        assert_eq!(plan.selected, vec![selected("group", "fav")]);
        assert_eq!(plan.repaired_count, 1);
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
