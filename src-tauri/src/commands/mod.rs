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