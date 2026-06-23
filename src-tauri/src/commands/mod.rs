use crate::auth::{self, Account, LoginInitResponse};
use crate::downloader::{run_batch_download, DownloadTask};
use crate::error::{AppError, DawnlandError};
use std::env::consts;
use sysinfo::System;
use tauri::AppHandle;

pub mod modpack;
pub mod task;

/// Returns a human-readable OS identifier string.
#[tauri::command]
pub fn get_system_info() -> Result<String, AppError> {
    let os = consts::OS;
    let arch = consts::ARCH;
    let family = consts::FAMILY;

    let info = format!("Operating System: {os} | Architecture: {arch} | Family: {family}");

    tracing::info!("System info requested: {}", info);
    Ok(info)
}

/// Returns the system locale (e.g. "zh-CN", "en-US")
#[tauri::command]
pub fn get_system_locale() -> Option<String> {
    sys_locale::get_locale()
}

/// Get system memory info for memory slider configuration.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemMemoryInfo {
    pub total_mb: u32,
    pub recommended_max_mb: u32,
}

#[tauri::command]
pub fn get_system_memory() -> Result<SystemMemoryInfo, AppError> {
    let mut sys = System::new_all();
    sys.refresh_all();
    let total_bytes = sys.total_memory();
    let total_mb = (total_bytes / (1024 * 1024)) as u32;
    let recommended_max_mb = (total_mb / 3).clamp(1024, 16384);

    tracing::info!(
        "System memory: {} MB, recommended max: {} MB",
        total_mb,
        recommended_max_mb
    );

    Ok(SystemMemoryInfo {
        total_mb,
        recommended_max_mb,
    })
}

/// Batch download multiple files concurrently.
#[tauri::command]
pub async fn batch_download(tasks: Vec<DownloadTask>, app: AppHandle) -> Result<(), AppError> {
    tracing::info!("Received batch download request with {} tasks", tasks.len());

    // Spawn the download tasks without blocking the command.
    let app_clone = app.clone();
    tokio::spawn(async move {
        let _ = run_batch_download(tasks, app_clone, crate::core::mojang::get_cancel_flag()).await;
    });

    // Return immediately to frontend.
    Ok(())
}

// ============ Auth Commands ============

/// Get all stored accounts.
#[tauri::command]
pub async fn get_accounts() -> Result<Vec<Account>, AppError> {
    auth::get_accounts().await.map_err(AppError::from)
}

/// Add a new offline account.
#[tauri::command]
pub async fn add_offline_account(username: String) -> Result<Account, AppError> {
    tracing::info!("Adding offline account: {}", username);
    auth::add_offline_account(&username)
        .await
        .map_err(AppError::from)
}

/// Remove an account by ID.
#[tauri::command]
pub async fn remove_account(id: String) -> Result<(), AppError> {
    tracing::info!("Removing account: {}", id);
    auth::remove_account(&id).await.map_err(AppError::from)
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn install_mod_to_instance(
    app: tauri::AppHandle,
    version_id: String,
    mod_source: String,
    project_id: String,
    file_id: String,
    download_url: String,
    dependencies: Option<Vec<crate::core::modrinth::UnifiedDependency>>,
    keep_both: Option<bool>,
) -> Result<String, String> {
    crate::core::manager::install_mod_to_instance(app, crate::core::manager::InstallModOptions {
        source: mod_source,
        project_id,
        mod_name: None,
        instance_id: Some(version_id),
        target_dir: None,
        download_url,
        file_id,
        dependencies,
        keep_both,
    }).await
}

/// Start Microsoft Device Code Flow login.
#[tauri::command]
pub async fn start_microsoft_login() -> Result<LoginInitResponse, AppError> {
    tracing::info!("Starting Microsoft login flow");
    auth::start_microsoft_login().await
}

/// Poll for Microsoft login completion.
#[tauri::command]
pub async fn poll_microsoft_token(device_code: String) -> Result<Account, AppError> {
    tracing::info!("Polling Microsoft token with device code");
    auth::poll_microsoft_token(&device_code).await
}

/// Refresh Microsoft token for an existing account.
#[tauri::command]
pub async fn refresh_microsoft_token(account_id: String) -> Result<Account, AppError> {
    tracing::info!("Refreshing Microsoft token for account: {}", account_id);
    auth::refresh_microsoft_token(&account_id).await
}

/// Start seamless Microsoft OAuth 2.0 PKCE login flow.
#[tauri::command]
pub async fn login_microsoft_oauth() -> Result<Account, AppError> {
    tracing::info!("Invoking seamless Microsoft OAuth login");
    auth::login_microsoft_oauth().await
}

// ============ Custom Updater Commands ============

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProgress {
    event: String,
    data: Option<ProgressData>,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressData {
    content_length: Option<u64>,
    chunk_length: Option<usize>,
}

#[tauri::command]
pub async fn update_launcher(version: String, app: AppHandle) -> Result<(), AppError> {
    let filename = if cfg!(target_os = "windows") {
        "DLML.exe"
    } else if cfg!(target_os = "linux") {
        "amd64.AppImage"
    } else {
        return Err(
            DawnlandError::Unknown("Unsupported OS for native auto-update".to_string()).into(),
        );
    };

    let url = format!("https://dl.88880222.xyz/releases/v{}/{}", version, filename);
    tracing::info!("Starting native update from {}", url);

    use std::io::Write;
    use tauri::Emitter;

    let mut response = reqwest::get(&url)
        .await
        .map_err(|e| format!("Failed to connect: {}", e))?;

    if !response.status().is_success() {
        return Err(DawnlandError::Unknown(format!(
            "Server returned error: {}",
            response.status()
        ))
        .into());
    }

    let content_length = response.content_length();

    app.emit(
        "portable-update-progress",
        UpdateProgress {
            event: "Started".to_string(),
            data: Some(ProgressData {
                content_length,
                chunk_length: None,
            }),
        },
    )
    .map_err(|e| e.to_string())?;

    let mut temp_file = tempfile::NamedTempFile::new().map_err(|e| e.to_string())?;

    use futures_util::StreamExt;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| format!("Download error: {}", e))?
    {
        temp_file
            .write_all(&chunk)
            .map_err(|e| format!("Write error: {}", e))?;
        app.emit(
            "portable-update-progress",
            UpdateProgress {
                event: "Progress".to_string(),
                data: Some(ProgressData {
                    content_length: None,
                    chunk_length: Some(chunk.len()),
                }),
            },
        )
        .map_err(|e| e.to_string())?;
    }

    temp_file.flush().map_err(|e| e.to_string())?;

    let temp_path = temp_file.into_temp_path();
    tracing::info!("Performing self-replace with downloaded file");

    // Set execution permissions on Linux
    #[cfg(target_family = "unix")]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(mut perms) = std::fs::metadata(&temp_path).map(|m| m.permissions()) {
            perms.set_mode(0o755);
            let _ = std::fs::set_permissions(&temp_path, perms);
        }
    }

    self_replace::self_replace(&temp_path).map_err(|e| {
        format!(
            "Self-replace failed. Make sure the file is not locked: {}",
            e
        )
    })?;
    let _ = temp_path.keep(); // Keep the file since self_replace moved it

    app.emit(
        "portable-update-progress",
        UpdateProgress {
            event: "Finished".to_string(),
            data: None,
        },
    )
    .map_err(|e| e.to_string())?;

    tracing::info!("Native update completed successfully");
    Ok(())
}

#[tauri::command]
pub async fn app_track_event(
    app: tauri::AppHandle,
    name: String,
    props: Option<serde_json::Value>,
) -> Result<(), AppError> {
    let aptabase_key = option_env!("APTABASE_KEY")
        .map(String::from)
        .or_else(|| std::env::var("APTABASE_KEY").ok())
        .filter(|k| !k.trim().is_empty());

    if aptabase_key.is_none() {
        tracing::debug!(
            "Aptabase disabled. Dropping event: {} (Props: {:?})",
            name,
            props
        );
        return Ok(());
    }
    use tauri_plugin_aptabase::EventTracker;
    tracing::debug!("Tracking event: {} (Props: {:?})", name, props);
    app.track_event(&name, props);
    // Removed app.flush_events_blocking() to prevent blocking the async runtime
    Ok(())
}
