use super::{CmdResult, StringifyErr as _};
use crate::{enhance::provider_reset::SeenLine, feat};
use smartstring::alias::String;

#[tauri::command]
pub async fn get_provider_values() -> CmdResult<Option<feat::ProviderValuesView>> {
    feat::provider_values_view().await.stringify_err()
}

#[tauri::command]
pub async fn return_provider_values(uid: String, lines: Vec<SeenLine>) -> CmdResult<feat::ProviderValuesOutcome> {
    feat::return_provider_values(uid, lines).await.stringify_err()
}
