use serde::Deserialize;
use std::path::PathBuf;

use crate::core::settings::LogLevelSetting;
use crate::error::{AppError, DawnlandError};
use crate::logger::frontend::LogEvent;
use crate::logger::{EffectiveLogConfig, LogFileInfo};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrontendLogEntry {
    pub level: String,
    pub target: String,
    pub message: String,
    pub task_id: Option<String>,
}

fn sanitize_frontend_scope(raw: &str) -> String {
    let cleaned: String = raw
        .trim()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | ':' | '.'))
        .take(64)
        .collect();
    if cleaned.is_empty() {
        "App".to_string()
    } else {
        cleaned
    }
}

#[tauri::command]
pub fn get_recent_logs() -> Result<Vec<LogEvent>, AppError> {
    Ok(crate::logger::frontend::get_recent_logs())
}

#[tauri::command]
pub async fn get_log_config() -> Result<EffectiveLogConfig, AppError> {
    let settings = crate::core::settings::get_launcher_settings().await;
    Ok(crate::logger::get_effective_log_config(&settings))
}

#[tauri::command]
pub async fn set_log_level(
    level: Option<LogLevelSetting>,
    retention_days: Option<u32>,
) -> Result<EffectiveLogConfig, AppError> {
    let updated = crate::core::settings::update_log_settings(level, retention_days)
        .await
        .map_err(DawnlandError::Unknown)?;
    Ok(crate::logger::get_effective_log_config(&updated))
}

#[tauri::command]
pub fn log_frontend_batch(entries: Vec<FrontendLogEntry>) -> Result<(), AppError> {
    for entry in entries.into_iter().take(100) {
        let scope = sanitize_frontend_scope(&entry.target);
        let msg = crate::logger::redact::truncate_for_log(&entry.message, 4096)
            .replace(['\r', '\n'], " ");
        let formatted = match entry.task_id.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            Some(tid) => format!("[{scope}] [task:{tid}] {msg}"),
            None => format!("[{scope}] {msg}"),
        };

        match entry.level.trim().to_ascii_uppercase().as_str() {
            "ERROR" => {
                tracing::error!(target: "dawnland_launcher_lib::frontend", frontend_origin = true, "{}", formatted);
            }
            "WARN" => {
                tracing::warn!(target: "dawnland_launcher_lib::frontend", frontend_origin = true, "{}", formatted);
            }
            "DEBUG" => {
                tracing::debug!(target: "dawnland_launcher_lib::frontend", frontend_origin = true, "{}", formatted);
            }
            "TRACE" => {
                tracing::trace!(target: "dawnland_launcher_lib::frontend", frontend_origin = true, "{}", formatted);
            }
            _ => {
                tracing::info!(target: "dawnland_launcher_lib::frontend", frontend_origin = true, "{}", formatted);
            }
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn list_log_files() -> Result<Vec<LogFileInfo>, AppError> {
    let dir = crate::logger::log_dir();
    crate::logger::list_log_files_in_dir(&dir)
        .await
        .map_err(|e| DawnlandError::Unknown(e).into())
}

#[tauri::command]
pub async fn read_log_file(
    filename: String,
    tail_lines: Option<usize>,
) -> Result<Vec<String>, AppError> {
    let dir = crate::logger::log_dir();
    crate::logger::read_log_file_in_dir(&dir, &filename, tail_lines)
        .await
        .map_err(|e| DawnlandError::Unknown(e).into())
}

#[tauri::command]
pub async fn open_log_directory() -> Result<(), AppError> {
    let dir = crate::logger::log_dir();
    tokio::fs::create_dir_all(&dir).await?;
    open::that(&dir).map_err(|e| DawnlandError::Unknown(format!("Failed to open log directory: {e}")))?;
    tracing::debug!("Opened log directory: {}", dir.display());
    Ok(())
}

#[tauri::command]
pub async fn export_diagnostic_logs(
    dest_path: String,
    include_game_logs: Option<bool>,
    instance_id: Option<String>,
) -> Result<String, AppError> {
    let trimmed = dest_path.trim();
    if trimmed.is_empty() || trimmed.contains('\0') {
        return Err(DawnlandError::Unknown("Invalid destination path".to_string()).into());
    }
    let mut path = PathBuf::from(trimmed);
    let has_zip_ext = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("zip"));
    if !has_zip_ext {
        path.set_extension("zip");
    }

    let exported = crate::logger::export_diagnostic_bundle(
        &path,
        include_game_logs.unwrap_or(false),
        instance_id.as_deref(),
    )
    .await
    .map_err(DawnlandError::Unknown)?;

    tracing::info!("Exported diagnostic log bundle to: {}", exported);
    Ok(exported)
}
