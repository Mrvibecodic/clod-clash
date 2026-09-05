use crate::{
    config::{Config, IClashTemp, IProfiles, IVerge},
    constants,
    core::handle,
    logging,
    process::AsyncHandler,
    utils::{
        dirs::{self, PathBufExec as _},
        help,
    },
};
use anyhow::Result;
use chrono::{Local, TimeZone as _};
use clash_verge_logging::Type;
#[cfg(any(target_os = "macos", target_os = "windows"))]
use std::path::Path;
use std::{path::PathBuf, str::FromStr as _};
use tauri_plugin_shell::ShellExt as _;
use tokio::fs;
use tokio::fs::DirEntry;

#[cfg(target_os = "windows")]
async fn delete_snapshot_logs(log_dir: &Path) -> Result<()> {
    let temp_dirs = [
        log_dir.join("temp"),
        log_dir.join("service").join("temp"),
        log_dir.join("sidecar").join("temp"),
    ];

    for temp_dir in temp_dirs.iter().filter(|d| d.exists()) {
        let mut entries = fs::read_dir(temp_dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("log") {
                let _ = path.remove_if_exists().await;
                logging!(info, Type::Setup, "delete snapshot log file: {}", path.display());
            }
        }
    }

    Ok(())
}

pub async fn delete_log() -> Result<()> {
    let log_dir = dirs::app_logs_dir()?;
    let service_log_dir = dirs::service_log_dir()?;

    if !log_dir.exists() && !service_log_dir.exists() {
        return Ok(());
    }

    #[cfg(target_os = "windows")]
    if log_dir.exists() {
        delete_snapshot_logs(&log_dir).await?;
    }

    let auto_log_clean = {
        let verge = Config::verge().await;
        let verge = verge.data_arc();
        verge.auto_log_clean.unwrap_or(0)
    };

    let day = match auto_log_clean {
        1 => 1,
        2 => 7,
        3 => 30,
        4 => 90,
        _ => return Ok(()),
    };

    logging!(info, Type::Setup, "try to delete log files, day: {}", day);

    let parse_time_str = |s: &str| {
        let sa: Vec<&str> = s.split('-').collect();
        if sa.len() != 4 {
            return Err(anyhow::anyhow!("invalid time str"));
        }

        let year = i32::from_str(sa[0])?;
        let month = u32::from_str(sa[1])?;
        let day = u32::from_str(sa[2])?;
        let time = chrono::NaiveDate::from_ymd_opt(year, month, day)
            .ok_or_else(|| anyhow::anyhow!("invalid time str"))?
            .and_hms_opt(0, 0, 0)
            .ok_or_else(|| anyhow::anyhow!("invalid time str"))?;
        Ok(time)
    };

    let process_file = async move |file: DirEntry| -> Result<()> {
        let file_name = file.file_name();
        let file_name = file_name.to_str().unwrap_or_default();

        if file_name.ends_with(".log") {
            let now = Local::now();
            let created_time = parse_time_str(&file_name[0..file_name.len() - 4])?;
            let file_time = Local
                .from_local_datetime(&created_time)
                .single()
                .ok_or_else(|| anyhow::anyhow!("invalid local datetime"))?;

            let duration = now.signed_duration_since(file_time);
            if duration.num_days() > day {
                let _ = file.path().remove_if_exists().await;
                logging!(info, Type::Setup, "delete log file: {}", file_name);
            }
        }
        Ok(())
    };

    if log_dir.exists() {
        let mut log_read_dir = fs::read_dir(&log_dir).await?;
        while let Some(entry) = log_read_dir.next_entry().await? {
            std::mem::drop(process_file(entry).await);
        }
    }

    if service_log_dir.exists() {
        let mut service_log_read_dir = fs::read_dir(service_log_dir).await?;
        while let Some(entry) = service_log_read_dir.next_entry().await? {
            std::mem::drop(process_file(entry).await);
        }
    }

    Ok(())
}

#[cfg(target_os = "macos")]
async fn is_logs_dir_writable(log_dir: &Path) -> bool {
    if !log_dir.is_dir() {
        logging!(warn, Type::Setup, "macOS logs path is not a directory: {:?}", log_dir);
        return false;
    }

    let probe_path = log_dir.join(format!(
        ".clash-verge-write-test-{}-{}",
        std::process::id(),
        Local::now().timestamp_nanos_opt().unwrap_or_default()
    ));

    match fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&probe_path)
        .await
    {
        Ok(_) => {
            if let Err(e) = fs::remove_file(&probe_path).await {
                logging!(
                    warn,
                    Type::Setup,
                    "failed to remove macOS logs write probe {:?}: {}",
                    probe_path,
                    e
                );
            }
            true
        }
        Err(e) => {
            logging!(
                warn,
                Type::Setup,
                "macOS logs directory is not writable {:?}: {}",
                log_dir,
                e
            );
            false
        }
    }
}

#[cfg(target_os = "macos")]
fn available_legacy_path(parent: &Path, prefix: &str) -> Result<PathBuf> {
    let timestamp = Local::now().format("%Y%m%d%H%M%S");
    let base_name = format!("{prefix}-{timestamp}");
    let candidate = parent.join(&base_name);

    if !candidate.exists() {
        return Ok(candidate);
    }

    for index in 1..100 {
        let candidate = parent.join(format!("{base_name}-{index}"));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }

    Err(anyhow::anyhow!(
        "failed to allocate legacy path under {:?} with prefix {}",
        parent,
        prefix
    ))
}

#[cfg(target_os = "macos")]
async fn migrate_legacy_macos_service_logs(log_dir: &Path) -> Result<()> {
    let legacy_service_dir = log_dir.join("service");
    if !legacy_service_dir.exists() {
        return Ok(());
    }

    let service_logs_root = dirs::service_logs_root_dir()?;
    fs::create_dir_all(&service_logs_root)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to create service logs root {:?}: {}", service_logs_root, e))?;

    let archived_service_dir = available_legacy_path(&service_logs_root, "service.legacy")?;
    fs::rename(&legacy_service_dir, &archived_service_dir)
        .await
        .map_err(|e| {
            anyhow::anyhow!(
                "Failed to archive legacy macOS service logs {:?} to {:?}: {}",
                legacy_service_dir,
                archived_service_dir,
                e
            )
        })?;

    logging!(
        info,
        Type::Setup,
        "Archived legacy macOS service logs: {:?} -> {:?}",
        legacy_service_dir,
        archived_service_dir
    );

    Ok(())
}

#[cfg(target_os = "macos")]
async fn migrate_legacy_macos_logs() -> Result<()> {
    let log_dir = dirs::app_logs_dir()?;

    if !log_dir.exists() {
        return Ok(());
    }

    if is_logs_dir_writable(&log_dir).await {
        if let Err(e) = migrate_legacy_macos_service_logs(&log_dir).await {
            logging!(warn, Type::Setup, "Failed to migrate legacy macOS service logs: {}", e);
        }
        return Ok(());
    }

    let app_home = dirs::app_home_dir()?;
    let archived_log_dir = available_legacy_path(&app_home, "logs.legacy-root")?;
    fs::rename(&log_dir, &archived_log_dir).await.map_err(|e| {
        anyhow::anyhow!(
            "Failed to archive unwritable macOS logs directory {:?} to {:?}: {}",
            log_dir,
            archived_log_dir,
            e
        )
    })?;

    logging!(
        warn,
        Type::Setup,
        "Archived unwritable macOS logs directory: {:?} -> {:?}",
        log_dir,
        archived_log_dir
    );

    fs::create_dir_all(&log_dir)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to recreate macOS logs directory {:?}: {}", log_dir, e))?;
    logging!(info, Type::Setup, "Recreated macOS logs directory: {:?}", log_dir);

    Ok(())
}

fn default_dns_config() -> serde_yaml_ng::Mapping {
    use serde_yaml_ng::Value;

    serde_yaml_ng::Mapping::from_iter([
        ("enable".into(), Value::Bool(true)),
        ("ipv6".into(), Value::Bool(true)),
        ("listen".into(), Value::String(":53".into())),
        ("enhanced-mode".into(), Value::String("fake-ip".into())),
        ("fake-ip-range".into(), Value::String("198.18.0.1/16".into())),
        ("fake-ip-range6".into(), Value::String("2001:2::0/64".into())),
        ("fake-ip-filter-mode".into(), Value::String("blacklist".into())),
        ("prefer-h3".into(), Value::Bool(false)),
        ("respect-rules".into(), Value::Bool(false)),
        ("use-hosts".into(), Value::Bool(false)),
        ("use-system-hosts".into(), Value::Bool(false)),
        (
            "fake-ip-filter".into(),
            Value::Sequence(vec![
                Value::String("*.lan".into()),
                Value::String("*.local".into()),
                Value::String("*.arpa".into()),
                Value::String("time.*.com".into()),
                Value::String("ntp.*.com".into()),
                Value::String("time.*.com".into()),
                Value::String("+.market.xiaomi.com".into()),
                Value::String("localhost.ptlogin2.qq.com".into()),
                Value::String("*.msftncsi.com".into()),
                Value::String("www.msftconnecttest.com".into()),
            ]),
        ),
        (
            "default-nameserver".into(),
            Value::Sequence(vec![
                Value::String("system".into()),
                Value::String("223.6.6.6".into()),
                Value::String("8.8.8.8".into()),
                Value::String("2400:3200::1".into()),
                Value::String("2001:4860:4860::8888".into()),
            ]),
        ),
        (
            "nameserver".into(),
            Value::Sequence(vec![
                Value::String("8.8.8.8".into()),
                Value::String("https://doh.pub/dns-query".into()),
                Value::String("https://dns.alidns.com/dns-query".into()),
            ]),
        ),
        (
            "nameserver-policy".into(),
            Value::Mapping(serde_yaml_ng::Mapping::new()),
        ),
        (
            "proxy-server-nameserver".into(),
            Value::Sequence(vec![
                Value::String("https://doh.pub/dns-query".into()),
                Value::String("https://dns.alidns.com/dns-query".into()),
                Value::String("tls://223.5.5.5".into()),
            ]),
        ),
        ("direct-nameserver".into(), Value::Sequence(vec![])),
        ("direct-nameserver-follow-policy".into(), Value::Bool(false)),
    ])
}

const DNS_CONFIG_HEADER: &str = "# Clash Verge DNS Config";

fn dns_config_problem(raw: &str) -> Option<std::string::String> {
    let parsed = match serde_yaml_ng::from_str::<serde_yaml_ng::Value>(raw) {
        Ok(parsed) => parsed,
        Err(err) => return Some(format!("the YAML in it does not parse: {err}")),
    };

    let Some(mapping) = parsed.as_mapping().filter(|mapping| !mapping.is_empty()) else {
        return Some("the YAML in it is empty or is not a mapping".into());
    };

    match mapping.get("dns") {
        Some(dns) => match dns.as_mapping() {
            Some(dns) if !dns.is_empty() => None,
            _ => Some("the YAML `dns` block in it is empty or is not a mapping".into()),
        },
        None => None,
    }
}

pub(crate) async fn ensure_dns_config_file() -> Result<()> {
    let dns_path = dirs::app_home_dir()?.join(constants::files::DNS_CONFIG);

    if fs::try_exists(&dns_path).await? {
        let raw = fs::read_to_string(&dns_path).await?;
        return match dns_config_problem(&raw) {
            Some(problem) => Err(anyhow::anyhow!("DNS config file {:?} is unusable: {problem}", dns_path)),
            None => Ok(()),
        };
    }

    let runtime = Config::runtime().await;
    let runtime = runtime.latest_arc();
    let runtime_dns = runtime
        .config
        .as_ref()
        .and_then(|config| config.get("dns"))
        .and_then(serde_yaml_ng::Value::as_mapping)
        .filter(|dns| !dns.is_empty())
        .cloned();

    logging!(
        info,
        Type::Setup,
        "Creating DNS config file from {}",
        if runtime_dns.is_some() {
            "the working config"
        } else {
            "the built-in defaults"
        }
    );

    let dns_config = seeded_dns_block(runtime_dns.unwrap_or_else(default_dns_config));
    let file_config = serde_yaml_ng::Mapping::from_iter([("dns".into(), serde_yaml_ng::Value::Mapping(dns_config))]);

    help::save_yaml(&dns_path, &file_config, Some(DNS_CONFIG_HEADER)).await
}

fn seeded_dns_block(mut dns: serde_yaml_ng::Mapping) -> serde_yaml_ng::Mapping {
    use serde_yaml_ng::Value;

    dns.insert("use-hosts".into(), Value::Bool(false));
    dns.insert("use-system-hosts".into(), Value::Bool(false));
    dns
}

fn legacy_fallback_filter() -> serde_yaml_ng::Mapping {
    use serde_yaml_ng::Value;

    serde_yaml_ng::Mapping::from_iter([
        ("geoip".into(), Value::Bool(true)),
        ("geoip-code".into(), Value::String("CN".into())),
        (
            "ipcidr".into(),
            Value::Sequence(vec![
                Value::String("240.0.0.0/4".into()),
                Value::String("0.0.0.0/32".into()),
            ]),
        ),
        (
            "domain".into(),
            Value::Sequence(vec![
                Value::String("+.google.com".into()),
                Value::String("+.facebook.com".into()),
                Value::String("+.youtube.com".into()),
            ]),
        ),
    ])
}

fn has_untouched_legacy_fallback(dns: &serde_yaml_ng::Mapping) -> bool {
    let fallback_untouched = dns
        .get("fallback")
        .and_then(serde_yaml_ng::Value::as_sequence)
        .is_some_and(|fallback| fallback.is_empty());

    let filter_untouched = dns
        .get("fallback-filter")
        .and_then(serde_yaml_ng::Value::as_mapping)
        .is_some_and(|filter| *filter == legacy_fallback_filter());

    fallback_untouched && filter_untouched
}

fn drop_legacy_fallback(file_config: &mut serde_yaml_ng::Mapping) -> bool {
    let Some(dns) = file_config
        .get_mut("dns")
        .and_then(serde_yaml_ng::Value::as_mapping_mut)
        .filter(|dns| has_untouched_legacy_fallback(dns))
    else {
        return false;
    };

    dns.remove("fallback");
    dns.remove("fallback-filter");
    true
}

fn drop_empty_hosts(file_config: &mut serde_yaml_ng::Mapping) -> bool {
    if !file_config
        .get("hosts")
        .and_then(serde_yaml_ng::Value::as_mapping)
        .is_some_and(serde_yaml_ng::Mapping::is_empty)
    {
        return false;
    }

    file_config.remove("hosts");
    true
}

fn has_user_comments(raw: &str) -> bool {
    raw.lines()
        .map(str::trim)
        .filter(|line| line.starts_with('#'))
        .any(|line| line != DNS_CONFIG_HEADER)
}

fn drop_legacy_dns_keys(file_config: &mut serde_yaml_ng::Mapping) -> bool {
    let dropped_fallback = drop_legacy_fallback(file_config);
    let dropped_hosts = drop_empty_hosts(file_config);

    dropped_fallback || dropped_hosts
}

async fn drop_legacy_dns_fallback() -> Result<()> {
    let dns_path = dirs::app_home_dir()?.join(constants::files::DNS_CONFIG);

    if !fs::try_exists(&dns_path).await? {
        return Ok(());
    }

    let raw = fs::read_to_string(&dns_path).await?;
    let Ok(mut file_config) = serde_yaml_ng::from_str::<serde_yaml_ng::Mapping>(&raw) else {
        return Ok(());
    };

    if !drop_legacy_dns_keys(&mut file_config) {
        return Ok(());
    }

    if has_user_comments(&raw) {
        logging!(
            info,
            Type::Setup,
            "Kept the legacy DNS keys in {:?}: the file carries comments of its own",
            dns_path
        );
        return Ok(());
    }

    let yaml_str = crate::utils::yaml_emitter::to_mihomo_config_string(&file_config)?;
    let yaml_str = format!("{DNS_CONFIG_HEADER}\n\n{yaml_str}");
    help::write_atomic(&dns_path, yaml_str.as_bytes()).await?;
    logging!(info, Type::Setup, "Removed the legacy DNS keys from {:?}", dns_path);

    Ok(())
}

async fn drop_leftover_dns_check_file() -> Result<()> {
    let leftover = dirs::app_home_dir()?.join(constants::files::DNS_CHECK_CONFIG);

    if fs::try_exists(&leftover).await? {
        fs::remove_file(&leftover).await?;
        logging!(info, Type::Setup, "Removed leftover DNS check file: {:?}", leftover);
    }

    Ok(())
}

pub(super) async fn init_dns_config() -> Result<()> {
    let leftover = drop_leftover_dns_check_file().await;
    if let Err(err) = &leftover {
        logging!(warn, Type::Setup, "Failed to remove the DNS check file: {}", err);
    }

    let migration = drop_legacy_dns_fallback().await;
    if let Err(err) = &migration {
        logging!(warn, Type::Setup, "Failed to migrate the DNS config file: {}", err);
    }

    leftover.and(migration)
}

async fn ensure_directories() -> Result<()> {
    let directories = [
        ("app_home", dirs::app_home_dir()?),
        ("app_profiles", dirs::app_profiles_dir()?),
        ("app_logs", dirs::app_logs_dir()?),
        ("service_logs", dirs::service_log_dir()?),
    ];

    for (name, dir) in directories {
        if !dir.exists() {
            fs::create_dir_all(&dir)
                .await
                .map_err(|e| anyhow::anyhow!("Failed to create {} directory {:?}: {}", name, dir, e))?;
            logging!(info, Type::Setup, "Created {} directory: {:?}", name, dir);
        }
    }

    Ok(())
}

async fn initialize_config_files() -> Result<()> {
    if let Ok(path) = dirs::clash_path()
        && !path.exists()
    {
        let template = IClashTemp::template().0;
        help::save_yaml(&path, &template, Some("# Clash Verge"))
            .await
            .map_err(|e| anyhow::anyhow!("Failed to create clash config: {}", e))?;
        logging!(info, Type::Setup, "Created clash config at {:?}", path);
    }

    if let Ok(path) = dirs::verge_path()
        && !path.exists()
    {
        let template = IVerge::template();
        help::save_yaml(&path, &template, Some("# Clash Verge"))
            .await
            .map_err(|e| anyhow::anyhow!("Failed to create verge config: {}", e))?;
        logging!(info, Type::Setup, "Created verge config at {:?}", path);
    }

    if let Ok(path) = dirs::profiles_path()
        && !path.exists()
    {
        let template = IProfiles::default();
        help::save_yaml(&path, &template, Some("# Clash Verge"))
            .await
            .map_err(|e| anyhow::anyhow!("Failed to create profiles config: {}", e))?;
        logging!(info, Type::Setup, "Created profiles config at {:?}", path);
    }

    IVerge::validate_and_fix_config()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to validate verge config: {}", e))?;

    if let Err(e) = unpin_core_log_keys().await {
        logging!(warn, Type::Setup, "Failed to unpin the core log keys: {}", e);
    }

    Ok(())
}

async fn unpin_core_log_keys() -> Result<()> {
    use serde_yaml_ng::Value;

    let verge_path = dirs::verge_path()?;
    let mut verge = help::read_mapping(&verge_path).await?;
    let unpinned_key = Value::from("core_log_keys_unpinned");
    if verge.get(&unpinned_key).and_then(Value::as_bool) == Some(true) {
        return Ok(());
    }
    let clash_path = dirs::clash_path()?;
    let mut clash = help::read_mapping(&clash_path).await?;
    if IClashTemp::unpin_legacy_defaults(&mut clash) {
        help::save_yaml(&clash_path, &clash, Some("# Generated by Clash Verge")).await?;
        logging!(
            info,
            Type::Setup,
            "log-level and unified-delay now follow the subscription; the stock values were dropped from {:?}",
            clash_path
        );
    }
    verge.insert(unpinned_key, Value::from(true));
    help::save_yaml(&verge_path, &verge, Some("# Clash Verge Config")).await?;
    Ok(())
}

pub async fn init_config() -> Result<()> {
    #[cfg(target_os = "macos")]
    migrate_legacy_macos_logs().await?;

    ensure_directories().await?;

    initialize_config_files().await?;

    AsyncHandler::spawn(|| async {
        for dir in [dirs::app_home_dir(), dirs::app_profiles_dir()].into_iter().flatten() {
            let removed = crate::utils::help::sweep_staging_leftovers(&dir).await;
            if removed > 0 {
                logging!(
                    info,
                    Type::Setup,
                    "removed {} stale config drafts from {:?}",
                    removed,
                    dir
                );
            }
        }
        if let Err(e) = delete_log().await {
            logging!(warn, Type::Setup, "Failed to clean old logs: {}", e);
        }
        logging!(info, Type::Setup, "Фоновая задача очистки логов завершена");
    });

    Ok(())
}

const GEO_ASSET_MARKER: &str = ".geo-assets.json";

type AssetStamp = (u64, u64);

async fn asset_stamp(path: &PathBuf) -> Option<AssetStamp> {
    let meta = fs::metadata(path).await.ok()?;
    let modified = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    Some((meta.len(), modified))
}

fn should_copy_bundled_asset(
    src: Option<AssetStamp>,
    dest: Option<AssetStamp>,
    delivered: Option<AssetStamp>,
) -> bool {
    let Some(src) = src else {
        return false;
    };
    let Some(dest) = dest else {
        return true;
    };
    if delivered.is_some_and(|delivered| delivered != dest) {
        return false;
    }
    src.1 > dest.1
}

async fn read_delivered_assets(marker: &PathBuf) -> std::collections::HashMap<String, AssetStamp> {
    let raw = match fs::read_to_string(marker).await {
        Ok(raw) => raw,
        Err(_) => return std::collections::HashMap::new(),
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

async fn write_delivered_assets(marker: &PathBuf, delivered: &std::collections::HashMap<String, AssetStamp>) {
    match serde_json::to_string(delivered) {
        Ok(raw) => {
            if let Err(err) = fs::write(marker, raw).await {
                logging!(debug, Type::Setup, "failed to record delivered geo assets: {}", err);
            }
        }
        Err(err) => {
            logging!(debug, Type::Setup, "failed to encode delivered geo assets: {}", err);
        }
    }
}

pub async fn init_resources() -> Result<()> {
    let app_dir = dirs::app_home_dir()?;
    let res_dir = dirs::app_resources_dir()?;

    if !app_dir.exists() {
        std::mem::drop(fs::create_dir_all(&app_dir).await);
    }
    if !res_dir.exists() {
        std::mem::drop(fs::create_dir_all(&res_dir).await);
    }

    let file_list = ["Country.mmdb", "geoip.dat", "geosite.dat"];
    let marker = app_dir.join(GEO_ASSET_MARKER);
    let mut delivered = read_delivered_assets(&marker).await;
    let mut delivered_changed = false;

    for file in file_list.iter() {
        let src_path = res_dir.join(file);
        let dest_path = app_dir.join(file);

        let src = asset_stamp(&src_path).await;
        let dest = asset_stamp(&dest_path).await;

        if should_copy_bundled_asset(src, dest, delivered.get(*file).copied()) {
            handle_copy(&src_path, &dest_path, file).await;
            if let Some(stamp) = asset_stamp(&dest_path).await {
                delivered.insert((*file).to_string(), stamp);
                delivered_changed = true;
            }
        } else if let Some(stamp) = dest
            && !delivered.contains_key(*file)
        {
            delivered.insert((*file).to_string(), stamp);
            delivered_changed = true;
        }
    }

    if delivered_changed {
        write_delivered_assets(&marker, &delivered).await;
    }

    Ok(())
}

#[cfg(target_os = "windows")]
pub fn init_scheme() -> Result<()> {
    use tauri::utils::platform::current_exe;
    use winreg::{RegKey, enums::HKEY_CURRENT_USER};

    let app_exe = current_exe()?;
    let app_exe = dunce::canonicalize(app_exe)?;
    let app_exe = app_exe.to_string_lossy().into_owned();

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (clash, _) = hkcu.create_subkey("Software\\Classes\\Clash")?;
    clash.set_value("", &crate::constants::branding::APP_NAME)?;
    clash.set_value(
        "URL Protocol",
        &format!("{} URL Scheme Protocol", crate::constants::branding::APP_NAME),
    )?;
    let (default_icon, _) = hkcu.create_subkey("Software\\Classes\\Clash\\DefaultIcon")?;
    default_icon.set_value("", &app_exe)?;
    let (command, _) = hkcu.create_subkey("Software\\Classes\\Clash\\Shell\\Open\\Command")?;
    command.set_value("", &format!("{app_exe} \"%1\""))?;

    Ok(())
}
#[cfg(target_os = "linux")]
pub fn init_scheme() -> Result<()> {
    let desktop_file = format!("{}.desktop", crate::constants::branding::APP_SLUG);
    let desktop_file = desktop_file.as_str();

    for scheme in DEEP_LINK_SCHEMES {
        let handler = format!("x-scheme-handler/{scheme}");
        let output = std::process::Command::new("xdg-mime")
            .arg("default")
            .arg(desktop_file)
            .arg(&handler)
            .output()?;
        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "failed to set {handler}, {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
    }

    crate::utils::linux::mime::ensure_mimeapps_entries(desktop_file, DEEP_LINK_SCHEMES)?;
    Ok(())
}
#[cfg(target_os = "macos")]
pub const fn init_scheme() -> Result<()> {
    Ok(())
}

#[cfg(target_os = "linux")]
const DEEP_LINK_SCHEMES: &[&str] = &["clash", "clash-verge", "clodclash"];

pub async fn startup_script() -> Result<()> {
    let app_handle = handle::Handle::app_handle();
    let script_path = {
        let verge = Config::verge().await;
        let verge = verge.data_arc();
        verge.startup_script.clone().unwrap_or_else(|| "".into())
    };

    if script_path.is_empty() {
        return Ok(());
    }

    let (shell_type, args): (&str, Vec<std::string::String>) = if script_path.ends_with(".sh") {
        ("bash", vec![script_path.to_string()])
    } else if script_path.ends_with(".ps1") {
        (
            "powershell",
            vec![
                "-NoProfile".to_string(),
                "-ExecutionPolicy".to_string(),
                "Bypass".to_string(),
                "-File".to_string(),
                script_path.to_string(),
            ],
        )
    } else if script_path.ends_with(".bat") {
        ("cmd", vec!["/C".to_string(), script_path.to_string()])
    } else {
        return Err(anyhow::anyhow!("unsupported script extension: {}", script_path));
    };

    let script_dir = PathBuf::from(script_path.as_str());
    if !script_dir.exists() {
        return Err(anyhow::anyhow!("script not found: {}", script_path));
    }

    let parent_dir = script_dir.parent();
    let working_dir = parent_dir.unwrap_or_else(|| script_dir.as_ref());

    let output = app_handle
        .shell()
        .command(shell_type)
        .current_dir(working_dir)
        .args(args)
        .output()
        .await?;

    if !output.status.success() {
        logging!(
            warn,
            Type::Setup,
            "startup script exited with {:?}: {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    Ok(())
}

async fn handle_copy(src: &PathBuf, dest: &PathBuf, file: &str) {
    match fs::copy(src, dest).await {
        Ok(_) => {
            logging!(debug, Type::Setup, "resources copied '{}'", file);
        }
        Err(err) => {
            logging!(
                error,
                Type::Setup,
                "failed to copy resources '{}' to '{:?}', {}",
                file,
                dest,
                err
            );
        }
    };
}

#[cfg(test)]
mod tests {
    use super::{
        DNS_CONFIG_HEADER, default_dns_config, dns_config_problem, drop_legacy_dns_keys, has_untouched_legacy_fallback,
        has_user_comments, legacy_fallback_filter, seeded_dns_block, should_copy_bundled_asset,
    };
    use serde_yaml_ng::{Mapping, Value};

    #[test]
    fn a_missing_geo_asset_is_delivered() {
        assert!(should_copy_bundled_asset(Some((10, 100)), None, None));
    }

    #[test]
    fn a_newer_bundled_geo_asset_replaces_the_one_we_delivered() {
        assert!(should_copy_bundled_asset(Some((10, 200)), Some((10, 100)), Some((10, 100))));
        assert!(!should_copy_bundled_asset(Some((10, 50)), Some((10, 100)), Some((10, 100))));
    }

    #[test]
    fn a_geo_asset_updated_by_the_core_is_left_alone() {
        assert!(!should_copy_bundled_asset(Some((10, 200)), Some((12, 150)), Some((10, 100))));
    }

    #[test]
    fn without_a_marker_the_previous_rule_applies() {
        assert!(should_copy_bundled_asset(Some((10, 200)), Some((12, 150)), None));
        assert!(!should_copy_bundled_asset(None, Some((12, 150)), None));
    }

    fn dns_with_legacy_fallback() -> Mapping {
        Mapping::from_iter([
            ("fallback".into(), Value::Sequence(vec![])),
            ("fallback-filter".into(), Value::Mapping(legacy_fallback_filter())),
        ])
    }

    #[test]
    fn factory_fallback_pair_is_recognized() {
        assert!(has_untouched_legacy_fallback(&dns_with_legacy_fallback()));
    }

    #[test]
    fn edited_fallback_filter_is_kept() {
        let mut dns = dns_with_legacy_fallback();
        let mut filter = legacy_fallback_filter();
        filter.insert("geoip-code".into(), Value::String("RU".into()));
        dns.insert("fallback-filter".into(), Value::Mapping(filter));

        assert!(!has_untouched_legacy_fallback(&dns));
    }

    #[test]
    fn non_empty_fallback_is_kept() {
        let mut dns = dns_with_legacy_fallback();
        dns.insert(
            "fallback".into(),
            Value::Sequence(vec![Value::String("1.0.0.1".into())]),
        );

        assert!(!has_untouched_legacy_fallback(&dns));
    }

    #[test]
    fn missing_keys_are_kept() {
        let mut dns = dns_with_legacy_fallback();
        dns.remove("fallback");
        assert!(!has_untouched_legacy_fallback(&dns));

        let mut dns = dns_with_legacy_fallback();
        dns.remove("fallback-filter");
        assert!(!has_untouched_legacy_fallback(&dns));
    }

    #[test]
    fn seeding_switches_the_hosts_keys_off() {
        let dns = Mapping::from_iter([
            ("use-hosts".into(), Value::Bool(true)),
            ("use-system-hosts".into(), Value::Bool(true)),
            ("enhanced-mode".into(), Value::String("fake-ip".into())),
        ]);

        let seeded = seeded_dns_block(dns);

        assert_eq!(seeded.get("use-hosts"), Some(&Value::Bool(false)));
        assert_eq!(seeded.get("use-system-hosts"), Some(&Value::Bool(false)));
        assert_eq!(
            seeded.get("enhanced-mode"),
            Some(&Value::String("fake-ip".into())),
            "the rest of the block arrives untouched"
        );
    }

    #[test]
    fn seeding_adds_the_hosts_keys_when_the_working_config_omits_them() {
        let seeded = seeded_dns_block(Mapping::from_iter([("ipv6".into(), Value::Bool(true))]));

        assert_eq!(seeded.get("use-hosts"), Some(&Value::Bool(false)));
        assert_eq!(seeded.get("use-system-hosts"), Some(&Value::Bool(false)));
        assert_eq!(seeded.get("ipv6"), Some(&Value::Bool(true)));
    }

    #[test]
    fn seeding_leaves_the_built_in_defaults_as_they_are() {
        assert_eq!(seeded_dns_block(default_dns_config()), default_dns_config());
    }

    #[test]
    fn a_healthy_file_has_no_problem() {
        assert_eq!(dns_config_problem("dns:\n  ipv6: true\n"), None);
        assert_eq!(dns_config_problem("ipv6: true\n"), None, "the legacy flat layout");
    }

    #[test]
    fn an_empty_file_is_a_problem() {
        for raw in ["", "\n", "# Clash Verge DNS Config\n", "{}\n"] {
            assert!(dns_config_problem(raw).is_some(), "empty source {raw:?}");
        }
    }

    #[test]
    fn a_broken_file_is_a_problem() {
        for raw in [
            "dns:\n  ipv6: true\n ipv6: false\n",
            "- one\n- two\n",
            "just a string\n",
        ] {
            assert!(dns_config_problem(raw).is_some(), "broken source {raw:?}");
        }
    }

    #[test]
    fn an_empty_dns_block_is_a_problem() {
        assert!(dns_config_problem("dns: {}\nhosts: {}\n").is_some());
        assert!(dns_config_problem("dns: nonsense\n").is_some());
    }

    #[test]
    fn every_problem_names_yaml_so_the_frontend_can_explain_it() {
        for raw in ["", "- one\n", "dns: {}\n", "dns:\n  a: 1\n b: 2\n"] {
            let problem = dns_config_problem(raw).unwrap_or_default();
            assert!(
                problem.to_lowercase().contains("yaml"),
                "problem for {raw:?}: {problem}"
            );
        }
    }

    #[test]
    fn our_own_factory_file_carries_no_user_comments() {
        assert!(!has_user_comments(""));
        assert!(!has_user_comments("dns:\n  ipv6: true\n"));
        assert!(!has_user_comments(&format!(
            "{DNS_CONFIG_HEADER}\n\ndns:\n  ipv6: true\n"
        )));
    }

    #[test]
    fn a_comment_the_user_added_is_seen() {
        for raw in [
            "# my own note\ndns:\n  ipv6: true\n",
            "dns:\n  # keep this one\n  ipv6: true\n",
            "dns:\n  ipv6: true\n# trailing note\n",
        ] {
            assert!(has_user_comments(raw), "user comment in {raw:?}");
            assert!(
                has_user_comments(&format!("{DNS_CONFIG_HEADER}\n\n{raw}")),
                "user comment next to the factory header in {raw:?}"
            );
        }
    }

    #[test]
    fn a_hash_inside_a_value_is_not_a_comment_line() {
        assert!(!has_user_comments(
            "dns:\n  nameserver:\n    - https://dns.example/dns-query#frag\n"
        ));
    }

    #[test]
    fn the_migration_drops_our_own_empty_hosts_map() {
        let mut file_config = Mapping::from_iter([
            ("dns".into(), Value::Mapping(Mapping::new())),
            ("hosts".into(), Value::Mapping(Mapping::new())),
        ]);

        assert!(drop_legacy_dns_keys(&mut file_config));
        assert!(!file_config.contains_key("hosts"));
    }

    #[test]
    fn the_migration_keeps_a_hosts_map_with_entries() {
        let hosts = Mapping::from_iter([("a.test".into(), Value::String("1.2.3.4".into()))]);
        let mut file_config = Mapping::from_iter([
            ("dns".into(), Value::Mapping(dns_with_legacy_fallback())),
            ("hosts".into(), Value::Mapping(hosts.clone())),
        ]);

        assert!(drop_legacy_dns_keys(&mut file_config));
        assert_eq!(file_config.get("hosts"), Some(&Value::Mapping(hosts)));
    }

    #[test]
    fn the_migration_reports_nothing_to_do_for_a_current_file() {
        let mut file_config = Mapping::from_iter([(
            "dns".into(),
            Value::Mapping(Mapping::from_iter([("ipv6".into(), Value::Bool(true))])),
        )]);

        assert!(!drop_legacy_dns_keys(&mut file_config));
    }

    #[test]
    fn the_migration_drops_the_fallback_pair_and_the_empty_hosts_together() {
        let mut file_config = Mapping::from_iter([
            ("dns".into(), Value::Mapping(dns_with_legacy_fallback())),
            ("hosts".into(), Value::Mapping(Mapping::new())),
        ]);

        assert!(drop_legacy_dns_keys(&mut file_config));
        assert!(!file_config.contains_key("hosts"));
        let dns = file_config.get("dns").and_then(Value::as_mapping);
        assert_eq!(dns.map(|dns| dns.contains_key("fallback")), Some(false));
        assert_eq!(dns.map(|dns| dns.contains_key("fallback-filter")), Some(false));
    }
}
