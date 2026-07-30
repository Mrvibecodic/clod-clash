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

/// Lowercase OS slug used inside the `User-Agent`.
pub const fn os_slug() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "windows"
    }
    #[cfg(target_os = "macos")]
    {
        "macos"
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        "linux"
    }
}

/// Subscription `User-Agent`.
///
/// Must start with the literal `branding::UA_TOKEN`: panels match it with
/// case-sensitive regexes in their subscription-response rules, so it is never
/// derived from the display name at runtime.
pub fn user_agent() -> String {
    format!(
        "{}/{} (Mihomo; {})",
        branding::UA_TOKEN,
        env!("CARGO_PKG_VERSION"),
        os_slug()
    )
    .into()
}

/// OS version string, sent as `x-ver-os`. Best effort — never fails.
pub fn os_version() -> String {
    sysinfo::System::os_version()
        .or_else(sysinfo::System::kernel_version)
        .map_or_else(|| String::from("unknown"), Into::into)
}

/// Machine name, sent as `x-device-model`.
pub fn device_model() -> String {
    gethostname::gethostname().to_string_lossy().trim().into()
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
        HWID_HEX_LEN, compute_hwid, digest_machine_id, is_valid_hwid, os_slug, parse_io_platform_uuid, user_agent,
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
    fn user_agent_starts_with_the_panel_token() {
        let ua = user_agent();
        assert!(ua.starts_with("ClodClash/"), "unexpected UA: {ua}");
        assert!(ua.contains("(Mihomo; "), "unexpected UA: {ua}");
        assert!(ua.ends_with(&format!("{})", os_slug())), "unexpected UA: {ua}");
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
