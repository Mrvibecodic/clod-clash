//! Device identity for panel-side device limits (Remnawave `x-hwid` family).
//!
//! Everything here is fork-specific (`clod`) and self-contained so upstream
//! merges never touch it. The value is derived from a stable, machine-local id
//! and hashed, so the raw machine id never leaves the device.
//!
//! Remnawave >= 2.9 validates the received hwid against
//! `^[a-zA-Z0-9=-]{10,64}$` — the 32 lowercase hex chars produced here satisfy
//! it. See `tests` below.

use crate::{
    config::{Config, IVerge},
    constants::branding,
};
use clash_verge_logging::{Type, logging};
use sha2::{Digest as _, Sha256};
use smartstring::alias::String;

/// Length of the emitted hwid in hex characters.
const HWID_HEX_LEN: usize = 32;

/// Human readable OS family, sent as `x-device-os`.
pub const fn device_os() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "Windows"
    }
    #[cfg(target_os = "macos")]
    {
        "macOS"
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        "Linux"
    }
}

/// Subscription `User-Agent`.
///
/// Must start with the literal `branding::UA_TOKEN`: panels match it with
/// case-sensitive regexes in their subscription-response rules, so it is never
/// derived from the display name at runtime.
///
/// clod: формат `ClodClash/x.y.z` — как у koala-clash (`koala-clash/1.x.x`):
/// панель Remnawave показывает User-Agent запроса с `x-hwid` в списке
/// устройств, и по чистой паре «имя/версия» видно версию клиента без мусора.
pub fn user_agent() -> String {
    format!("{}/{}", branding::UA_TOKEN, env!("CARGO_PKG_VERSION")).into()
}

/// OS version string, sent as `x-ver-os`. Best effort — never fails.
///
/// clod: определение как в koala-clash — человекочитаемая версия
/// (Windows `DisplayVersion` вроде `24H2`, macOS `productVersion`,
/// дистрибутив на Linux), а не сырой номер ядра.
pub fn os_version() -> String {
    if let Some(version) = platform_os_version() {
        return version;
    }
    sysinfo::System::os_version()
        .or_else(sysinfo::System::kernel_version)
        .map_or_else(|| String::from("unknown"), Into::into)
}

/// Device description, sent as `x-device-model`.
///
/// clod: как в koala-clash — модель/редакция системы, а НЕ имя компьютера:
/// hostname часто содержит личные данные («Ivan-PC»), которым в панели
/// провайдера делать нечего.
pub fn device_model() -> String {
    platform_device_model().unwrap_or_else(|| String::from(device_os()))
}

#[cfg(target_os = "windows")]
fn read_current_version_value(name: &str) -> Option<std::string::String> {
    use winreg::{RegKey, enums::HKEY_LOCAL_MACHINE};

    let key = RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey_with_flags(
            r"SOFTWARE\Microsoft\Windows NT\CurrentVersion",
            winreg::enums::KEY_READ | winreg::enums::KEY_WOW64_64KEY,
        )
        .ok()?;
    let value: std::string::String = key.get_value(name).ok()?;
    let value = value.trim().to_owned();
    (!value.is_empty()).then_some(value)
}

#[cfg(target_os = "windows")]
fn platform_os_version() -> Option<String> {
    // `DisplayVersion` — это «24H2»/«23H2», ровно то, что видит пользователь.
    read_current_version_value("DisplayVersion").map(Into::into)
}

#[cfg(target_os = "windows")]
fn platform_device_model() -> Option<String> {
    let product = read_current_version_value("ProductName")?;
    let build = sysinfo::System::kernel_version()
        .and_then(|v| v.rsplit('.').next().and_then(|b| b.parse::<u32>().ok()))
        .unwrap_or(0);
    Some(windows_product_name(&product, build).into())
}

#[cfg(target_os = "macos")]
fn command_output(program: &str, args: &[&str]) -> Option<std::string::String> {
    let output = std::process::Command::new(program).args(args).output().ok()?;
    let text = std::string::String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (!text.is_empty()).then_some(text)
}

#[cfg(target_os = "macos")]
fn platform_os_version() -> Option<String> {
    command_output("sw_vers", &["-productVersion"]).map(Into::into)
}

#[cfg(target_os = "macos")]
fn platform_device_model() -> Option<String> {
    let model = command_output("sysctl", &["-n", "hw.model"])?;
    let chip =
        command_output("sysctl", &["-n", "machdep.cpu.brand_string"]).and_then(|brand| apple_chip_from_brand(&brand));
    Some(match chip {
        Some(chip) => format!("{model} ({chip})").into(),
        None => model.into(),
    })
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn read_os_release() -> Option<std::string::String> {
    std::fs::read_to_string("/etc/os-release").ok()
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn platform_os_version() -> Option<String> {
    let content = read_os_release()?;
    linux_os_version_from_release(&content).map(Into::into)
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn platform_device_model() -> Option<String> {
    let content = read_os_release()?;
    os_release_field(&content, "PRETTY_NAME")
        .or_else(|| os_release_field(&content, "NAME"))
        .map(Into::into)
}

/// Build ≥ 22000 is Windows 11, but the registry `ProductName` still says
/// «Windows 10 …» there — the same promotion koala-clash does.
#[cfg(any(target_os = "windows", test))]
fn windows_product_name(product: &str, build: u32) -> std::string::String {
    if build >= 22000 && product.contains("Windows 10") {
        product.replace("Windows 10", "Windows 11")
    } else {
        product.to_owned()
    }
}

/// «Apple M2 Pro» out of the CPU brand string, when present.
#[cfg(any(target_os = "macos", test))]
fn apple_chip_from_brand(brand: &str) -> Option<std::string::String> {
    let rest = brand.trim().strip_prefix("Apple ")?;
    let chip = rest.split_whitespace().take(2).collect::<Vec<_>>().join(" ");
    chip.starts_with('M').then_some(chip)
}

/// A single `KEY="value"` field out of `/etc/os-release`.
#[cfg(any(not(any(target_os = "windows", target_os = "macos")), test))]
fn os_release_field(content: &str, field: &str) -> Option<std::string::String> {
    content.lines().find_map(|line| {
        let value = line.strip_prefix(field)?.strip_prefix('=')?;
        let value = value.trim().trim_matches('"').trim();
        (!value.is_empty()).then(|| value.to_owned())
    })
}

/// `NAME VERSION_ID` («Ubuntu 24.04»), or just the name.
#[cfg(any(not(any(target_os = "windows", target_os = "macos")), test))]
fn linux_os_version_from_release(content: &str) -> Option<std::string::String> {
    let name = os_release_field(content, "NAME")?;
    Some(match os_release_field(content, "VERSION_ID") {
        Some(version) => format!("{name} {version}"),
        None => name,
    })
}

/// Hash a raw machine id into the value we are allowed to transmit.
fn digest_machine_id(raw: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(raw.as_bytes());
    hasher.update(branding::HWID_SALT.as_bytes());
    let full = hex::encode(hasher.finalize());
    full[..HWID_HEX_LEN].into()
}

/// Read the platform specific, stable machine id.
fn raw_machine_id() -> Option<std::string::String> {
    #[cfg(target_os = "windows")]
    {
        use winreg::{RegKey, enums::HKEY_LOCAL_MACHINE};

        let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
        let key = hklm
            .open_subkey_with_flags(
                r"SOFTWARE\Microsoft\Cryptography",
                winreg::enums::KEY_READ | winreg::enums::KEY_WOW64_64KEY,
            )
            .ok()?;
        let guid: std::string::String = key.get_value("MachineGuid").ok()?;
        let guid = guid.trim().to_owned();
        (!guid.is_empty()).then_some(guid)
    }

    #[cfg(target_os = "macos")]
    {
        let output = std::process::Command::new("ioreg")
            .args(["-rd1", "-c", "IOPlatformExpertDevice"])
            .output()
            .ok()?;
        let text = std::string::String::from_utf8_lossy(&output.stdout);
        parse_io_platform_uuid(&text)
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        for path in ["/etc/machine-id", "/var/lib/dbus/machine-id"] {
            if let Ok(content) = std::fs::read_to_string(path) {
                let trimmed = content.trim().to_owned();
                if !trimmed.is_empty() {
                    return Some(trimmed);
                }
            }
        }
        None
    }
}

/// Extract `IOPlatformUUID` out of `ioreg` output.
#[cfg(any(target_os = "macos", test))]
fn parse_io_platform_uuid(text: &str) -> Option<std::string::String> {
    for line in text.lines() {
        if !line.contains("IOPlatformUUID") {
            continue;
        }
        let mut parts = line.split('=');
        parts.next()?;
        let value = parts.next()?.trim().trim_matches('"').trim().to_owned();
        if !value.is_empty() {
            return Some(value);
        }
    }
    None
}

/// Compute the hwid without touching persisted config.
///
/// Falls back to a random id when no stable source is readable; the caller is
/// expected to persist the result so the value stays stable afterwards.
fn compute_hwid() -> String {
    match raw_machine_id() {
        Some(raw) => digest_machine_id(&raw),
        None => {
            logging!(
                warn,
                Type::System,
                "Warning: [hwid] стабильный machine-id недоступен, генерируем случайный"
            );
            digest_machine_id(&nanoid::nanoid!(32))
        }
    }
}

/// Get the device id, computing and persisting it on first use.
///
/// Returns `None` when the user disabled device identification.
pub async fn hwid() -> Option<String> {
    let verge = Config::verge().await;

    let (enabled, cached) = {
        let data = verge.data_arc();
        (
            data.enable_hwid.unwrap_or(IVerge::DEFAULT_ENABLE_HWID),
            data.hwid.clone(),
        )
    };

    if !enabled {
        return None;
    }

    if let Some(cached) = cached
        && is_valid_hwid(&cached)
    {
        return Some(cached);
    }

    let fresh = compute_hwid();

    verge.edit_draft(|draft| {
        draft.hwid = Some(fresh.clone());
    });
    verge.apply();
    if let Err(err) = verge.data_arc().save_file().await {
        logging!(warn, Type::System, "Warning: [hwid] не удалось сохранить hwid: {err}");
    }

    Some(fresh)
}

/// Remnawave >= 2.9 server side validation.
pub fn is_valid_hwid(value: &str) -> bool {
    let len = value.chars().count();
    (10..=64).contains(&len) && value.chars().all(|c| c.is_ascii_alphanumeric() || c == '=' || c == '-')
}

#[cfg(test)]
mod tests {
    use super::{
        HWID_HEX_LEN, apple_chip_from_brand, compute_hwid, digest_machine_id, is_valid_hwid,
        linux_os_version_from_release, os_release_field, parse_io_platform_uuid, user_agent, windows_product_name,
    };

    #[test]
    fn hwid_is_32_lowercase_hex_chars() {
        let hwid = digest_machine_id("11111111-2222-3333-4444-555555555555");
        assert_eq!(hwid.len(), HWID_HEX_LEN);
        assert!(hwid.chars().all(|c| c.is_ascii_hexdigit() && !c.is_uppercase()));
    }

    #[test]
    fn hwid_matches_remnawave_regex() {
        // ^[a-zA-Z0-9=-]{10,64}$
        assert!(is_valid_hwid(&digest_machine_id("abc")));
        assert!(is_valid_hwid(&compute_hwid()));

        assert!(!is_valid_hwid("short"));
        assert!(!is_valid_hwid("has space inside the value"));
        assert!(!is_valid_hwid(&"x".repeat(65)));
        assert!(is_valid_hwid(&"x".repeat(64)));
    }

    #[test]
    fn hwid_is_stable_for_the_same_machine_id() {
        let a = digest_machine_id("stable-machine-id");
        let b = digest_machine_id("stable-machine-id");
        assert_eq!(a, b);

        assert_ne!(a, digest_machine_id("another-machine-id"));
    }

    #[test]
    fn hwid_does_not_leak_the_raw_id() {
        let raw = "11111111-2222-3333-4444-555555555555";
        assert!(!digest_machine_id(raw).contains("1111"));
    }

    #[test]
    fn user_agent_is_the_panel_token_and_version_only() {
        // koala-style `name/version`: панель показывает UA в списке устройств,
        // и версия клиента должна читаться без дополнительного мусора.
        let ua = user_agent();
        assert!(ua.starts_with("ClodClash/"), "unexpected UA: {ua}");
        assert_eq!(ua.as_str(), format!("ClodClash/{}", env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn windows_product_name_promotes_win10_to_win11_on_new_builds() {
        assert_eq!(windows_product_name("Windows 10 Pro", 22631), "Windows 11 Pro");
        assert_eq!(windows_product_name("Windows 10 Pro", 19045), "Windows 10 Pro");
        assert_eq!(windows_product_name("Windows 11 Home", 26100), "Windows 11 Home");
    }

    #[test]
    fn apple_chip_is_extracted_from_the_brand_string() {
        assert_eq!(apple_chip_from_brand("Apple M2 Pro").as_deref(), Some("M2 Pro"));
        assert_eq!(apple_chip_from_brand("Apple M1").as_deref(), Some("M1"));
        assert_eq!(apple_chip_from_brand("Intel(R) Core(TM) i7"), None);
    }

    #[test]
    fn os_release_fields_are_parsed() {
        let sample = "NAME=\"Ubuntu\"\nVERSION_ID=\"24.04\"\nPRETTY_NAME=\"Ubuntu 24.04.1 LTS\"\n";
        assert_eq!(
            os_release_field(sample, "PRETTY_NAME").as_deref(),
            Some("Ubuntu 24.04.1 LTS")
        );
        assert_eq!(linux_os_version_from_release(sample).as_deref(), Some("Ubuntu 24.04"));
        assert_eq!(linux_os_version_from_release("VERSION_ID=1"), None);
    }

    #[test]
    fn parses_io_platform_uuid_from_ioreg_output() {
        let sample = r#"
    +-o Root  <class IORegistryEntry, id 0x100000100, retain 15>
        "IOPlatformSerialNumber" = "C02XXXXXXXXX"
        "IOPlatformUUID" = "7A1B2C3D-4E5F-6071-8293-A4B5C6D7E8F9"
"#;
        assert_eq!(
            parse_io_platform_uuid(sample).as_deref(),
            Some("7A1B2C3D-4E5F-6071-8293-A4B5C6D7E8F9")
        );
        assert_eq!(parse_io_platform_uuid("nothing here"), None);
    }
}
