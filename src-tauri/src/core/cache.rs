use std::time::{Duration, SystemTime};
use tauri::AppHandle;

/// Cleanup the dawnland cache by removing folders older than 15 days
pub async fn cleanup_expired_cache() {
    let cache_dir = crate::core::mojang::get_dawnland_cache();
    if !cache_dir.exists() {
        return;
    }

    let threshold = SystemTime::now() - Duration::from_secs(15 * 24 * 60 * 60);

    if let Ok(mut entries) = tokio::fs::read_dir(&cache_dir).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            if let Ok(metadata) = entry.metadata().await {
                if let Ok(modified) = metadata.modified() {
                    if modified < threshold {
                        let _ = tokio::fs::remove_dir_all(entry.path()).await;
                        tracing::info!("Cleaned up expired cache folder: {:?}", entry.path());
                    }
                }
            }
        }
    }
}

/// Force cleanup the dawnland cache
#[tauri::command]
pub async fn clean_dawnland_cache(_app: AppHandle) -> Result<(), String> {
    let cache_dir = crate::core::mojang::get_dawnland_cache();
    if cache_dir.exists() {
        tokio::fs::remove_dir_all(&cache_dir)
            .await
            .map_err(|e| e.to_string())?;
        tracing::info!("Manually cleared dawnland cache");
    }
    Ok(())
}
