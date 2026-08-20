#![cfg(target_os = "windows")]

use anyhow::{Result, bail};
use std::path::PathBuf;
use std::process::Command as StdCommand;
use std::time::Duration;

const PROBE_TIMEOUT: Duration = Duration::from_secs(15);
const CORE_FILE_NAMES: [&str; 2] = ["verge-mihomo.exe", "verge-mihomo-alpha.exe"];

fn ps_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn run_powershell(script: String) -> Result<std::process::Output> {
    use std::os::windows::process::CommandExt as _;
    Ok(StdCommand::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .creation_flags(0x08000000)
        .output()?)
}

async fn active_core_path() -> Option<PathBuf> {
    if let Some(managed) = crate::core::core_updater::managed_binary_on_disk().await {
        return Some(managed);
    }
    crate::core::service::bundled_core_path().await.ok()
}

pub async fn inbound_allowed() -> Option<bool> {
    let path = active_core_path().await?;
    let script = format!(
        "$p={};Get-NetFirewallApplicationFilter -PolicyStore ActiveStore -ErrorAction SilentlyContinue | Where-Object {{ $_.Program -eq $p }} | Get-NetFirewallRule -ErrorAction SilentlyContinue | Where-Object {{ $_.Direction -eq 'Inbound' -and $_.Enabled -eq 'True' }} | ForEach-Object {{ $_.Action.ToString() }}",
        ps_single_quote(&path.to_string_lossy())
    );
    let output = tokio::time::timeout(
        PROBE_TIMEOUT,
        tokio::task::spawn_blocking(move || run_powershell(script)),
    )
    .await
    .ok()?
    .ok()?
    .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut allowed = false;
    for line in stdout.lines() {
        match line.trim() {
            "Allow" | "AllowBypass" => allowed = true,
            "Block" => return Some(false),
            _ => {}
        }
    }
    Some(allowed)
}

pub async fn allow_inbound() -> Result<()> {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use deelevate::{PrivilegeLevel, Token};
    use runas::Command as RunasCommand;
    use std::os::windows::process::CommandExt as _;

    let mut paths: Vec<PathBuf> = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        for name in CORE_FILE_NAMES {
            let path = exe.with_file_name(name);
            if path.is_file() {
                paths.push(path);
            }
        }
    }
    if let Some(managed) = crate::core::core_updater::managed_binary_on_disk().await {
        paths.push(managed);
    }
    if paths.is_empty() {
        bail!("no core binaries found next to the app");
    }

    let mut script = String::from("$fail=0; ");
    for path in &paths {
        let file_name = path
            .file_name()
            .map_or_else(|| String::from("core"), |name| name.to_string_lossy().into_owned());
        let program = ps_single_quote(&format!("program={}", path.to_string_lossy()));
        let rule_name = ps_single_quote(&format!("name=Clod Clash core ({file_name})"));
        script.push_str(&format!(
            "netsh advfirewall firewall delete rule name=all dir=in {program} | Out-Null; \
             netsh advfirewall firewall add rule {rule_name} dir=in action=allow {program} enable=yes | Out-Null; \
             if ($LASTEXITCODE -ne 0) {{ $fail=1 }}; "
        ));
    }
    script.push_str("exit $fail");

    let encoded: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let encoded = STANDARD.encode(encoded);

    let status = tokio::task::spawn_blocking(move || -> Result<std::process::ExitStatus> {
        let args = ["-NoProfile", "-NonInteractive", "-EncodedCommand", &encoded];
        let token = Token::with_current_process()?;
        let status = match token.privilege_level()? {
            PrivilegeLevel::NotPrivileged => RunasCommand::new("powershell.exe").args(&args).show(false).status()?,
            _ => StdCommand::new("powershell.exe")
                .args(args)
                .creation_flags(0x08000000)
                .status()?,
        };
        Ok(status)
    })
    .await??;

    if !status.success() {
        bail!(
            "failed to add the firewall rule, status {}",
            status.code().unwrap_or(-1)
        );
    }
    Ok(())
}
