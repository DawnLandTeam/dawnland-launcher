use crate::auth::{self, Account, LoginInitResponse};
use crate::core::{self, InstallState, VanillaVersion};
use crate::downloader::{run_batch_download, DownloadTask};
use std::env::consts;
use tauri::AppHandle;

/// Returns a human-readable OS identifier string.
#[tauri::command]
pub fn get_system_info() -> Result<String, String> {
    let os = consts::OS;
    let arch = consts::ARCH;
    let family = consts::FAMILY;

    let info = format!(
        "Operating System: {os} | Architecture: {arch} | Family: {family}"
    );

    tracing::info!("System info requested: {info}");
    Ok(info)
}

/// Batch download multiple files concurrently.
#[tauri::command]
pub async fn batch_download(tasks: Vec<DownloadTask>, app: AppHandle) -> Result<(), String> {
    tracing::info!("Received batch download request with {} tasks", tasks.len());

    // Spawn the download tasks without blocking the command.
    let app_clone = app.clone();
    tokio::spawn(async move {
        run_batch_download(tasks, app_clone).await;
    });

    // Return immediately to frontend.
    Ok(())
}

// ============ Auth Commands ============

/// Get all stored accounts.
#[tauri::command]
pub async fn get_accounts() -> Result<Vec<Account>, String> {
    auth::get_accounts().await
}

/// Add a new offline account.
#[tauri::command]
pub async fn add_offline_account(username: String) -> Result<Account, String> {
    tracing::info!("Adding offline account: {}", username);
    auth::add_offline_account(&username).await
}

/// Remove an account by ID.
#[tauri::command]
pub async fn remove_account(id: String) -> Result<(), String> {
    tracing::info!("Removing account: {}", id);
    auth::remove_account(&id).await
}

/// Start Microsoft Device Code Flow login.
#[tauri::command]
pub async fn start_microsoft_login() -> Result<LoginInitResponse, String> {
    tracing::info!("Starting Microsoft login flow");
    auth::start_microsoft_login().await
}

/// Poll for Microsoft login completion.
#[tauri::command]
pub async fn poll_microsoft_token(device_code: String) -> Result<Account, String> {
    tracing::info!("Polling Microsoft token with device code");
    auth::poll_microsoft_token(&device_code).await
}