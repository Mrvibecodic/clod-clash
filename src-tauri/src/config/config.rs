use super::{IClashTemp, IProfiles, IVerge};
use crate::{
    config::{PrfItem, profiles_append_item_to_safe, runtime::IRuntime},
    constants::{files, timing},
    core::{CoreManager, handle, validate::CoreConfigValidator},
    enhance,
    process::AsyncHandler,
    utils::{dirs, help},
};
use anyhow::{Result, anyhow};
use backon::{ExponentialBuilder, Retryable as _};
use clash_verge_draft::Draft;
use clash_verge_logging::{Type, logging, logging_error};
use serde_yaml_ng::{Mapping, Value};
use smartstring::alias::String;
use std::{collections::HashSet, path::PathBuf};
use tokio::sync::OnceCell;
use tokio::time::sleep;

/// Какие файлы настроек не удалось прочитать при запуске.
///
/// Пустой реестр в памяти после сбоя разбора — это не «у пользователя нет
/// подписок», а «мы не смогли их прочитать». Записывать такую пустоту поверх
/// настоящего файла нельзя: она сотрёт адреса панелей и привязки цепочек,
/// которых больше нигде нет. Файлы `{uid}.yaml` при этом целы, но без реестра
/// они безымянны.
pub mod load_failures {
    use clash_verge_logging::{Type, logging};
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, Ordering};

    static PROFILES: AtomicBool = AtomicBool::new(false);
    static VERGE: AtomicBool = AtomicBool::new(false);
    static CLASH: AtomicBool = AtomicBool::new(false);

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum ConfigFile {
        Profiles,
        Verge,
        Clash,
    }

    const fn flag(file: ConfigFile) -> &'static AtomicBool {
        match file {
            ConfigFile::Profiles => &PROFILES,
            ConfigFile::Verge => &VERGE,
            ConfigFile::Clash => &CLASH,
        }
    }

    pub fn mark(file: ConfigFile) {
        flag(file).store(true, Ordering::Release);
    }

    pub fn happened(file: ConfigFile) -> bool {
        flag(file).load(Ordering::Acquire)
    }

    /// Отложить в сторону файл, который не прочитался.
    ///
    /// Отказ от записи при выходе спасает только от одного, самого редкого пути:
    /// обычных мест, которые сохраняют настройки по ходу работы, много, и любое из
    /// них положило бы шаблон поверх настоящего файла в первые же секунды сессии.
    /// Поэтому копию делаем сразу при неудачном чтении — до того, как что-либо
    /// успеет записаться. Копия рядом с оригиналом: человеку её и искать, и
    /// восстанавливать проще всего.
    pub async fn keep_a_copy(path: &Path) -> Option<std::string::String> {
        let Ok(meta) = tokio::fs::metadata(path).await else {
            return None;
        };
        if !meta.is_file() || meta.len() == 0 {
            return None;
        }

        let name = path.file_name()?.to_string_lossy().into_owned();
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
        let copy = path.with_file_name(format!("{name}.unreadable-{stamp}"));

        match tokio::fs::copy(path, &copy).await {
            Ok(_) => {
                let copy_name = copy.file_name()?.to_string_lossy().into_owned();
                logging!(
                    error,
                    Type::Config,
                    "{} не прочитался; копия отложена как {}",
                    name,
                    copy_name
                );
                Some(copy_name)
            }
            Err(err) => {
                logging!(
                    error,
                    Type::Config,
                    "{} не прочитался, и отложить копию не удалось: {}",
                    name,
                    err
                );
                None
            }
        }
    }

    /// Имена не прочитанных файлов — для сообщения человеку.
    pub fn names() -> Vec<&'static str> {
        [
            (ConfigFile::Profiles, "profiles.yaml"),
            (ConfigFile::Verge, "verge.yaml"),
            (ConfigFile::Clash, "config.yaml"),
        ]
        .into_iter()
        .filter(|(file, _)| happened(*file))
        .map(|(_, name)| name)
        .collect()
    }
}

#[cfg(test)]
mod load_failure_tests {
    use super::load_failures::{ConfigFile, happened, mark, names};

    // Флаги живут на весь процесс и снятия не предусматривают, поэтому тест
    // проверяет только то, что не зависит от порядка и от чужих отметок.
    #[test]
    fn a_file_that_did_not_load_is_marked_and_named() {
        mark(ConfigFile::Profiles);

        assert!(happened(ConfigFile::Profiles));
        assert!(names().contains(&"profiles.yaml"));

        // Отметка не снимается: перезаписывать непрочитанный файл нельзя до
        // перезапуска, даже если позже что-то удалось прочитать.
        mark(ConfigFile::Profiles);
        assert!(happened(ConfigFile::Profiles));
    }
}

pub struct Config {
    clash_config: Draft<IClashTemp>,
    verge_config: Draft<IVerge>,
    profiles_config: Draft<IProfiles>,
    runtime_config: Draft<IRuntime>,
}

impl Config {
    pub async fn global() -> &'static Self {
        static CONFIG: OnceCell<Config> = OnceCell::const_new();
        CONFIG
            .get_or_init(|| async {
                Self {
                    clash_config: Draft::new(IClashTemp::new().await),
                    verge_config: Draft::new(IVerge::new().await),
                    profiles_config: Draft::new(IProfiles::new().await),
                    runtime_config: Draft::new(IRuntime::new()),
                }
            })
            .await
    }

    pub async fn clash() -> Draft<IClashTemp> {
        Self::global().await.clash_config.clone()
    }

    pub async fn verge() -> Draft<IVerge> {
        Self::global().await.verge_config.clone()
    }

    pub async fn profiles() -> Draft<IProfiles> {
        Self::global().await.profiles_config.clone()
    }

    pub async fn runtime() -> Draft<IRuntime> {
        Self::global().await.runtime_config.clone()
    }

    /// Инициализация подписки
    pub async fn init_config() -> Result<()> {
        Self::init_config_before_window().await?;
        Self::init_runtime_config().await
    }

    pub async fn init_config_before_window() -> Result<()> {
        Self::ensure_default_profile_items().await?;

        let verge = Self::verge().await.latest_arc();
        clash_verge_i18n::sync_locale(verge.language.as_deref());

        // clod:tun-ready — раньше здесь стоял «init Tun mode»: одна проверка
        // прав ДО того, как поднят менеджер службы, и запись
        // `enable_tun_mode: false` прямо в verge.yaml. На автозапуске служба
        // отвечает позже приложения, так что выбор пользователя стирался
        // навсегда. Готовность TUN теперь считается после старта ядра
        // (`feat::tun`), а недоступность подавляется только на сессию.

        Ok(())
    }

    pub async fn init_runtime_config() -> Result<()> {
        let validation_result = Self::generate_and_validate().await?;

        if let Some((msg_type, msg_content)) = validation_result {
            sleep(timing::STARTUP_ERROR_DELAY).await;
            handle::Handle::notice_message(msg_type, msg_content);
        }

        {
            let profiles = Self::profiles().await.data_arc();
            // Logging error internally
            let _ = profiles.cleanup_orphaned_files().await;
        }

        Ok(())
    }

    // Ensure "Merge" and "Script" profile items exist, adding them if missing.
    async fn ensure_default_profile_items() -> Result<()> {
        let profiles = Self::profiles().await;
        Self::ensure_default_profile_items_for(&profiles).await
    }

    async fn ensure_default_profile_items_for(profiles: &Draft<IProfiles>) -> Result<()> {
        if profiles.latest_arc().get_items().is_none() {
            logging!(
                warn,
                Type::Config,
                "Не удалось загрузить элементы Profile, пропускаю инициализацию элементов по умолчанию, чтобы сохранить текущие конфиги"
            );
            return Ok(());
        }

        if profiles.latest_arc().get_item("Merge").is_err() {
            let merge_item = &mut PrfItem::from_merge(Some("Merge".into()))?;
            profiles_append_item_to_safe(profiles, merge_item).await?;
        }
        if profiles.latest_arc().get_item("Script").is_err() {
            let script_item = &mut PrfItem::from_script(Some("Script".into()))?;
            profiles_append_item_to_safe(profiles, script_item).await?;
        }
        Ok(())
    }

    async fn generate_and_validate() -> Result<Option<(&'static str, String)>> {
        // Генерируем runtime-конфиг
        if let Err(err) = Self::generate().await {
            let error_msg: String = err.to_string().into();
            logging!(
                error,
                Type::Config,
                "Не удалось сгенерировать runtime-конфиг: {}",
                error_msg
            );
            CoreManager::global()
                .use_default_config("config_validate::boot_error", &error_msg)
                .await?;
            return Ok(Some(("config_validate::boot_error", error_msg)));
        }
        logging!(info, Type::Config, "Runtime-конфиг сгенерирован успешно");

        // Генерируем файл runtime-конфига и проверяем его
        let config_result = Self::generate_file(ConfigType::Run).await;

        if config_result.is_ok() {
            // Проверяем конфиг-файл
            logging!(info, Type::Config, "Начинаю проверку конфига");

            match CoreConfigValidator::global().validate_config_outcome().await {
                Ok(outcome) if outcome.is_valid() => {
                    logging!(info, Type::Config, "Проверка конфига успешна");
                    // Фронтенду не нужно знать об успешной проверке, событие не требуется
                    // Some(("config_validate::success", String::new()))
                    Ok(None)
                }
                Ok(outcome) => {
                    let error_msg: String = outcome.to_string().into();
                    logging!(
                        warn,
                        Type::Config,
                        "[Первый запуск] Проверка конфига не пройдена, запускаю с минимальным конфигом по умолчанию: {}",
                        error_msg
                    );
                    CoreManager::global()
                        .use_default_config("config_validate::boot_error", &error_msg)
                        .await?;
                    Ok(Some(("config_validate::boot_error", error_msg)))
                }
                Err(err) => {
                    logging!(warn, Type::Config, "Не удалось выполнить проверку: {}", err);
                    CoreManager::global()
                        .use_default_config("config_validate::process_terminated", "")
                        .await?;
                    Ok(Some(("config_validate::process_terminated", String::new())))
                }
            }
        } else {
            logging!(
                warn,
                Type::Config,
                "Не удалось сгенерировать конфиг, использую конфиг по умолчанию"
            );
            CoreManager::global()
                .use_default_config("config_validate::error", "")
                .await?;
            Ok(Some(("config_validate::error", String::new())))
        }
    }

    pub async fn dns_page_check_config(page: &Mapping) -> Option<Mapping> {
        let runtime = Self::runtime().await;
        let runtime_latest = runtime.latest_arc();
        let runtime_data = runtime.data_arc();
        let working = runtime_latest.config.as_ref().or(runtime_data.config.as_ref())?;

        Some(check_config_with_dns_page(working, page))
    }

    pub async fn generate_file(typ: ConfigType) -> Result<PathBuf> {
        let path = match typ {
            ConfigType::Run => dirs::app_home_dir()?.join(files::RUNTIME_CONFIG),
            ConfigType::Check => dirs::app_home_dir()?.join(files::CHECK_CONFIG),
        };

        let runtime = Self::runtime().await;
        let runtime_lastest = runtime.latest_arc();
        // Fall back to committed config if runtime config is missing
        let runtime_data = runtime.data_arc();
        let config = runtime_lastest
            .config
            .as_ref()
            .or_else(|| runtime_data.config.as_ref())
            .ok_or_else(|| anyhow!("failed to generate runtime config, might need to restart application"))?;

        match typ {
            ConfigType::Run => help::save_yaml(&path, config, Some("# Generated by Clash Verge")).await?,
            ConfigType::Check => {
                let config = without_fake_ip_store(config);
                help::save_yaml(&path, &config, Some("# Generated by Clash Verge")).await?;
            }
        }
        Ok(path)
    }

    pub async fn generate() -> Result<()> {
        let (mut config, exists_keys, logs, sentinel_report) = enhance::enhance().await?;

        sanitize_tunnels_proxy(&mut config);

        Self::runtime().await.edit_draft(|d| {
            *d = IRuntime {
                config: Some(config),
                exists_keys,
                chain_logs: logs,
                sentinel_report,
            }
        });

        Ok(())
    }

    pub async fn verify_config_initialization() {
        let backoff = ExponentialBuilder::default()
            .with_min_delay(std::time::Duration::from_millis(100))
            .with_max_delay(std::time::Duration::from_secs(2))
            .with_factor(2.0)
            .with_max_times(10);

        if let Err(e) = (|| async {
            if Self::runtime().await.latest_arc().config.is_some() {
                return Ok::<(), anyhow::Error>(());
            }
            Self::generate().await
        })
        .retry(backoff)
        .await
        {
            logging!(error, Type::Setup, "Config init verification failed: {}", e);
        }
    }

    // Переводит черновик в основные данные и записывает в файл. Избегает потери действий пользователя.
    // Используется только при событиях выхода из приложения, перезапуска, выключения системы
    pub async fn apply_all_and_save_file() {
        logging!(info, Type::Config, "save all draft data");
        // Файл, который не прочитался при запуске, перезаписывать нельзя: в памяти
        // у него пусто, и сохранение стёрло бы настоящее содержимое.
        let save_clash_task = AsyncHandler::spawn(|| async {
            if load_failures::happened(load_failures::ConfigFile::Clash) {
                logging!(
                    error,
                    Type::Config,
                    "config.yaml не прочитался при запуске — не перезаписываем его пустым"
                );
                return;
            }
            let clash = Self::clash().await;
            clash.apply();
            logging_error!(Type::Config, clash.data_arc().save_config().await);
        });

        let save_verge_task = AsyncHandler::spawn(|| async {
            if load_failures::happened(load_failures::ConfigFile::Verge) {
                logging!(
                    error,
                    Type::Config,
                    "verge.yaml не прочитался при запуске — не перезаписываем его пустым"
                );
                return;
            }
            let verge = Self::verge().await;
            verge.apply();
            logging_error!(Type::Config, verge.data_arc().save_file().await);
        });

        let save_profiles_task = AsyncHandler::spawn(|| async {
            if load_failures::happened(load_failures::ConfigFile::Profiles) {
                logging!(
                    error,
                    Type::Config,
                    "profiles.yaml не прочитался при запуске — не перезаписываем его пустым"
                );
                return;
            }
            let profiles = Self::profiles().await;
            // clod:Э10-05 — черновик профилей здесь не коммитим: он живёт только на
            // время переключения подписки, и незавершённое переключение при выходе
            // фиксировать нечего. Слепое `apply()` вдобавок затирало результат
            // параллельного обновления, которое пишет committed напрямую.
            profiles.discard();
            logging_error!(Type::Config, profiles.data_arc().save_file().await);
        });

        let _ = tokio::join!(save_clash_task, save_verge_task, save_profiles_task);
        logging!(info, Type::Config, "save all draft data finished");
    }
}

fn sanitize_tunnels_proxy(config: &mut Mapping) {
    // Проверяем наличие tunnels
    if !config
        .get("tunnels")
        .and_then(|v| v.as_sequence())
        .is_some_and(|t| tunnels_need_validation(t))
    {
        return;
    }

    // При необходимости собираем доступные цели (proxies + proxy-groups + встроенные)
    let mut valid: HashSet<String> = HashSet::with_capacity(64);
    collect_names(config, "proxies", &mut valid);
    collect_names(config, "proxy-groups", &mut valid);

    valid.insert("DIRECT".into());
    valid.insert("REJECT".into());

    let Some(tunnels) = config.get_mut("tunnels").and_then(|v| v.as_sequence_mut()) else {
        return;
    };

    // Изменяем tunnels: удаляем недействительный proxy
    for item in tunnels {
        let Some(tunnel) = item.as_mapping_mut() else { continue };

        let Some(proxy_name) = tunnel.get("proxy").and_then(|v| v.as_str()) else {
            continue;
        };

        if proxy_name == "DIRECT" || proxy_name == "REJECT" {
            continue;
        }

        if !valid.contains(proxy_name) {
            tunnel.remove("proxy");
        }
    }
}

// Возвращает true, если tunnels существуют и хотя бы у одной tunnel proxy требует проверки
fn tunnels_need_validation(tunnels: &[Value]) -> bool {
    tunnels.iter().any(|item| {
        item.as_mapping()
            .and_then(|t| t.get("proxy"))
            .and_then(|p| p.as_str())
            .is_some_and(|name| name != "DIRECT" && name != "REJECT")
    })
}

fn collect_names(config: &Mapping, list_key: &str, out: &mut HashSet<String>) {
    let Some(Value::Sequence(seq)) = config.get(list_key) else {
        return;
    };

    for item in seq {
        let Value::Mapping(map) = item else {
            continue;
        };
        if let Some(Value::String(n)) = map.get("name")
            && !n.is_empty()
        {
            out.insert(n.into());
        }
    }
}

fn without_fake_ip_store(config: &Mapping) -> Mapping {
    let mut config = config.clone();
    let key = Value::from("profile");
    if let Some(Value::Mapping(profile)) = config.get_mut(&key) {
        profile.remove(Value::from("store-fake-ip"));
    }
    config
}

pub(crate) fn check_config_with_dns_page(working: &Mapping, page: &Mapping) -> Mapping {
    let mut config = without_fake_ip_store(working);

    if let Some(hosts) = page.get("hosts").filter(|value| value.is_mapping()) {
        config.insert(Value::from("hosts"), hosts.clone());
    }

    match page.get("dns") {
        Some(dns) => {
            if dns.is_mapping() {
                config.insert(Value::from("dns"), dns.clone());
            }
        }
        None => {
            config.insert(Value::from("dns"), Value::Mapping(page.clone()));
        }
    }

    config
}

#[derive(Debug)]
pub enum ConfigType {
    Run,
    Check,
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::mem;

    #[test]
    #[allow(unused_variables)]
    #[allow(clippy::expect_used)]
    fn test_prfitem_from_merge_size() {
        let merge_item = PrfItem::from_merge(Some("Merge".into())).expect("Failed to create merge item in test");
        let prfitem_size = mem::size_of_val(&merge_item);
        // Boxed version
        let boxed_merge_item = Box::new(merge_item);
        let box_prfitem_size = mem::size_of_val(&boxed_merge_item);
        // The size of Box<T> is always pointer-sized (usually 8 bytes on 64-bit)
        // assert_eq!(box_prfitem_size, mem::size_of::<Box<PrfItem>>());
        assert!(box_prfitem_size < prfitem_size);
    }

    #[test]
    #[allow(unused_variables)]
    fn test_draft_size_non_boxed() {
        let draft = Draft::new(IRuntime::new());
        let iruntime_size = std::mem::size_of_val(&draft);
        assert_eq!(iruntime_size, std::mem::size_of::<Draft<IRuntime>>());
    }

    #[test]
    #[allow(unused_variables)]
    fn test_draft_size_boxed() {
        let draft = Draft::new(Box::new(IRuntime::new()));
        let box_iruntime_size = std::mem::size_of_val(&draft);
        assert_eq!(box_iruntime_size, std::mem::size_of::<Draft<Box<IRuntime>>>());
    }

    #[test]
    #[allow(clippy::expect_used)]
    fn the_check_copy_drops_the_fake_ip_store_and_keeps_the_rest() {
        let mut profile = Mapping::new();
        profile.insert(Value::from("store-selected"), Value::from(true));
        profile.insert(Value::from("store-fake-ip"), Value::from(true));
        let mut config = Mapping::new();
        config.insert(Value::from("profile"), Value::from(profile));
        config.insert(Value::from("mode"), Value::from("rule"));

        let checked = without_fake_ip_store(&config);
        let checked_profile = checked
            .get(Value::from("profile"))
            .and_then(Value::as_mapping)
            .expect("profile survives the check copy");

        assert!(!checked_profile.contains_key(Value::from("store-fake-ip")));
        assert_eq!(
            checked_profile.get(Value::from("store-selected")),
            Some(&Value::from(true))
        );
        assert_eq!(checked.get(Value::from("mode")), Some(&Value::from("rule")));
        assert_eq!(
            config
                .get(Value::from("profile"))
                .and_then(Value::as_mapping)
                .and_then(|profile| profile.get(Value::from("store-fake-ip"))),
            Some(&Value::from(true)),
            "the working config keeps the key"
        );
    }

    #[test]
    fn the_check_copy_leaves_a_config_without_a_profile_block_alone() {
        let config = Mapping::from_iter([(Value::from("mode"), Value::from("rule"))]);
        assert_eq!(without_fake_ip_store(&config), config);
    }

    fn working_config() -> Mapping {
        let mut config = Mapping::new();
        config.insert(
            Value::from("rule-providers"),
            Value::from(Mapping::from_iter([(Value::from("ru"), Value::from("stub"))])),
        );
        config.insert(
            Value::from("profile"),
            Value::from(Mapping::from_iter([(Value::from("store-fake-ip"), Value::from(true))])),
        );
        config.insert(
            Value::from("dns"),
            Value::from(Mapping::from_iter([(Value::from("ipv6"), Value::from(false))])),
        );
        config.insert(
            Value::from("hosts"),
            Value::from(Mapping::from_iter([(Value::from("a.test"), Value::from("1.2.3.4"))])),
        );
        config
    }

    #[test]
    #[allow(clippy::expect_used)]
    fn the_dns_page_is_checked_against_the_working_config() {
        let page = Mapping::from_iter([(
            Value::from("dns"),
            Value::from(Mapping::from_iter([(
                Value::from("nameserver-policy"),
                Value::from(Mapping::from_iter([(
                    Value::from("+.test"),
                    Value::from("rule-set:ru"),
                )])),
            )])),
        )]);

        let checked = check_config_with_dns_page(&working_config(), &page);

        assert!(
            checked.contains_key(Value::from("rule-providers")),
            "the rule sets the policy points at have to be in the checked file"
        );
        assert_eq!(checked.get(Value::from("dns")), page.get("dns"));
        assert!(
            !checked
                .get(Value::from("profile"))
                .and_then(Value::as_mapping)
                .expect("profile")
                .contains_key(Value::from("store-fake-ip")),
            "the check copy still drops the fake-ip store"
        );
    }

    #[test]
    fn a_page_without_hosts_leaves_the_profile_hosts_alone() {
        let page = Mapping::from_iter([(Value::from("dns"), Value::from(Mapping::new()))]);
        let checked = check_config_with_dns_page(&working_config(), &page);

        assert_eq!(
            checked.get(Value::from("hosts")),
            working_config().get(Value::from("hosts"))
        );
    }

    #[test]
    fn a_page_with_hosts_overrides_the_profile_hosts() {
        let hosts = Value::from(Mapping::from_iter([(Value::from("b.test"), Value::from("9.9.9.9"))]));
        let page = Mapping::from_iter([(Value::from("hosts"), hosts.clone())]);
        let checked = check_config_with_dns_page(&working_config(), &page);

        assert_eq!(checked.get(Value::from("hosts")), Some(&hosts));
    }

    #[test]
    fn a_page_without_a_dns_key_is_taken_as_the_dns_block_itself() {
        let page = Mapping::from_iter([(Value::from("ipv6"), Value::from(true))]);
        let checked = check_config_with_dns_page(&working_config(), &page);

        assert_eq!(checked.get(Value::from("dns")), Some(&Value::from(page)));
    }

    #[tokio::test]
    async fn failed_profile_index_survives_startup_without_cleanup() -> Result<()> {
        let profiles = Draft::new(IProfiles::default());
        let profiles_dir = std::env::temp_dir().join(format!("clash-verge-profile-cleanup-{}", nanoid::nanoid!()));
        tokio::fs::create_dir_all(&profiles_dir).await?;
        let active_profile = profiles_dir.join("Ractive.yaml");
        tokio::fs::write(&active_profile, "proxies: []").await?;

        Config::ensure_default_profile_items_for(&profiles).await?;
        profiles.data_arc().cleanup_orphaned_files_in(&profiles_dir).await?;

        let profile_was_preserved = tokio::fs::try_exists(&active_profile).await?;
        tokio::fs::remove_dir_all(&profiles_dir).await?;

        assert!(
            profile_was_preserved,
            "startup must not delete profiles when profiles.yaml could not be loaded"
        );
        assert!(
            profiles.data_arc().get_items().is_none(),
            "startup must not replace an unreadable profile index with defaults"
        );
        Ok(())
    }
}
