use crate::cmd::CmdResult;

/// Platform-specific implementation for UWP functionality
#[cfg(windows)]
mod platform {
    use crate::cmd::CmdResult;
    use crate::cmd::StringifyErr as _;
    use crate::core::win_uwp;

    pub async fn invoke_uwp_tool() -> CmdResult {
        match tokio::task::spawn_blocking(win_uwp::invoke_uwptools).await {
            Ok(result) => result.stringify_err(),
            Err(join_error) => Err(join_error.to_string().into()),
        }
    }
}

/// Stub implementation for non-Windows platforms
#[cfg(not(windows))]
mod platform {
    use super::CmdResult;

    #[allow(clippy::unnecessary_wraps, clippy::unused_async)]
    pub async fn invoke_uwp_tool() -> CmdResult {
        Ok(())
    }
}

/// Command exposed to Tauri
#[tauri::command]
pub async fn invoke_uwp_tool() -> CmdResult {
    platform::invoke_uwp_tool().await
}
