use super::CmdResult;
use crate::{cmd::StringifyErr as _, config::IVerge, feat, utils::hwid};
use clash_verge_draft::SharedDraft;
use serde::Serialize;
use smartstring::alias::String;

/// Получить конфиг Verge
#[tauri::command]
pub async fn get_verge_config() -> CmdResult<SharedDraft<IVerge>> {
    feat::fetch_verge_config().await.stringify_err()
}

/// Изменить конфиг Verge
#[tauri::command]
pub async fn patch_verge_config(payload: IVerge) -> CmdResult {
    feat::patch_verge(&payload, false).await.stringify_err()
}

/// clod: what the panel sees about this device.
///
/// Exactly the values sent as `x-hwid` / `x-device-os` / `x-ver-os` /
/// `x-device-model` — the settings toggle shows them so "device
/// identification" is a checkable claim rather than a promise.
#[derive(Debug, Clone, Serialize)]
pub struct DeviceIdentity {
    /// `None` while identification is switched off — nothing is sent then.
    pub hwid: Option<String>,
    pub os: &'static str,
    pub os_version: String,
    pub model: String,
    pub user_agent: String,
}

/// Получить информацию идентификации устройства
#[tauri::command]
pub async fn get_device_identity() -> CmdResult<DeviceIdentity> {
    Ok(DeviceIdentity {
        hwid: hwid::hwid().await,
        os: hwid::device_os(),
        os_version: hwid::os_version(),
        model: hwid::device_model(),
        user_agent: hwid::user_agent(),
    })
}
