use crate::{
    process::AsyncHandler,
    utils::dirs::{self, PathBufExec as _},
};
use anyhow::Error;
use smartstring::alias::String;
use std::{env::consts::OS, io::Write as _, path::PathBuf};
use tokio::fs;
use zip::write::SimpleFileOptions;

pub async fn create_backup() -> Result<(String, PathBuf), Error> {
    let now = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S").to_string();
    let zip_file_name: String = format!("{OS}-backup-{now}.zip").into();
    let zip_path = dirs::app_home_dir()?.join(zip_file_name.as_str());
    if let Err(err) = write_backup(zip_path.clone()).await {
        let _ = zip_path.remove_if_exists().await;
        return Err(err);
    }
    Ok((zip_file_name, zip_path))
}

async fn write_backup(zip_path: PathBuf) -> Result<(), Error> {
    let value = zip_path.clone();
    let file = AsyncHandler::spawn_blocking(move || std::fs::File::create(&value)).await??;
    let mut zip = zip::ZipWriter::new(file);
    zip.add_directory("profiles/", SimpleFileOptions::default())?;
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);

    if let Ok(mut entries) = fs::read_dir(dirs::app_profiles_dir()?).await {
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.is_file() {
                let file_name_os = entry.file_name();
                let file_name = file_name_os
                    .to_str()
                    .ok_or_else(|| anyhow::Error::msg("Invalid file name encoding"))?;
                if kept_out_of_backup(file_name) {
                    continue;
                }
                let backup_path = format!("profiles/{}", file_name);
                zip.start_file(backup_path, options)?;
                let file_content = fs::read(&path).await?;
                zip.write_all(&file_content)?;
            }
        }
    }
    zip.start_file(dirs::CLASH_CONFIG, options)?;
    zip.write_all(fs::read(dirs::clash_path()?).await?.as_slice())?;

    let verge_text = fs::read_to_string(dirs::verge_path()?).await?;
    let mut verge_config: serde_json::Value = serde_yaml_ng::from_str(&verge_text)?;
    if let Some(obj) = verge_config.as_object_mut() {
        crate::feat::strip_machine_local(obj);
    }
    zip.start_file(dirs::VERGE_CONFIG, options)?;
    zip.write_all(serde_yaml_ng::to_string(&verge_config)?.as_bytes())?;

    zip.start_file(dirs::PROFILE_YAML, options)?;
    zip.write_all(fs::read(dirs::profiles_path()?).await?.as_slice())?;
    zip.finish()?;
    Ok(())
}

/// Файлы подписок, которые в копию не идут. Запасная копия прежней подписки и
/// кандидат приёма — не данные человека: в копии им нечего делать (у прежней
/// подписки — прежние ключи). Замеры отчёта — этого компьютера, недописанный
/// файл — не файл.
fn kept_out_of_backup(file_name: &str) -> bool {
    file_name.ends_with(".prev")
        || file_name.ends_with(".new")
        || dirs::is_report_file(file_name)
        || crate::utils::help::is_staging_leftover(file_name)
}

#[cfg(test)]
mod tests {
    use super::kept_out_of_backup;

    #[test]
    fn report_measurements_and_leftovers_stay_out_of_the_backup() {
        for name in [
            "report-Rmc1x0.json",
            "Rmc1x0.yaml.prev",
            "Rmc1x0.yaml.new",
            ".Rmc1x0.yaml.aB3dE9xZ.tmp",
        ] {
            assert!(kept_out_of_backup(name), "{name}");
        }
        for name in ["Rmc1x0.yaml", "freeze-Rmc1x0.json", "profiles.yaml"] {
            assert!(!kept_out_of_backup(name), "{name}");
        }
    }
}
