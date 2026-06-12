use crate::core::task::TaskContext;
use crate::downloader::{DownloadProgress, DownloadTask};
use futures_util::StreamExt;
use sha1::{Digest, Sha1};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tokio::sync::Semaphore;
use tokio::time::Duration;

/// Maximum concurrent downloads.
const MAX_CONCURRENT: usize = 16;

/// Minimum time between progress emissions (milliseconds).
const PROGRESS_THROTTLE_MS: u64 = 500;

/// Compute SHA-1 hash of a file
pub fn compute_sha1_sync(path: &std::path::Path) -> Result<String, String> {
    use std::io::Read;

    let file =
        std::fs::File::open(path).map_err(|e| format!("Failed to open file for hashing: {}", e))?;

    let mut reader = std::io::BufReader::new(file);
    let mut hasher = Sha1::new();
    let mut buffer = [0u8; 8192];

    loop {
        let bytes_read = reader
            .read(&mut buffer)
            .map_err(|e| format!("Failed to read file: {}", e))?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }

    // Finalize and get the hash as hex string
    let result = hasher.finalize();
    Ok(format!("{:x}", result))
}

/// Downloads a single file with progress reporting.
/// Returns Ok on success, Err with message on failure.
/// If file already exists and matches expected SHA-1 hash, skips download.
async fn download_file_task(
    task: DownloadTask,
    client: &reqwest::Client,
    ctx: &TaskContext,
) -> Result<(), String> {
    tracing::debug!("Starting download: {} -> {}", task.url, task.dest_path);

    // Create parent directories if needed (always try, ignore if exists).
    let dest_path = task.dest_path_buf();
    if let Some(parent) = dest_path.parent() {
        if let Err(e) = tokio::fs::create_dir_all(parent).await {
            let err_msg = format!("Failed to create directory: {}", e);
            tracing::error!("{}: {}", err_msg, parent.display());
            return Err(err_msg);
        }
    }

    // If file exists, check hash OR size
    if dest_path.exists() {
        if let Some(expected_hash) = &task.hash {
            tracing::debug!("Checking existing file hash: {}", dest_path.display());
            let dest_path_clone = dest_path.clone();
            match tokio::task::spawn_blocking(move || compute_sha1_sync(&dest_path_clone))
                .await
                .map_err(|e| e.to_string())?
            {
                Ok(actual_hash) => {
                    if actual_hash.eq_ignore_ascii_case(expected_hash) {
                        tracing::debug!(
                            "File exists and hash matches, skipping: {}",
                            dest_path.display()
                        );
                        return Ok(());
                    } else {
                        tracing::debug!(
                            "Hash mismatch. Expected {}, got {}. Re-downloading...",
                            expected_hash,
                            actual_hash
                        );
                        let _ = tokio::fs::remove_file(&dest_path).await;
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to compute hash for existing file, re-downloading: {}",
                        e
                    );
                }
            }
        } else if let Some(expected_size) = task.expected_size {
            // No hash, but we have expected size
            if let Ok(metadata) = tokio::fs::metadata(&dest_path).await {
                if metadata.len() == expected_size {
                    tracing::debug!(
                        "File exists and size matches ({} bytes), skipping: {}",
                        expected_size,
                        dest_path.display()
                    );
                    return Ok(());
                } else {
                    tracing::debug!(
                        "Size mismatch. Expected {}, got {}. Re-downloading...",
                        expected_size,
                        metadata.len()
                    );
                    let _ = tokio::fs::remove_file(&dest_path).await;
                }
            }
        } else {
            // No hash and no size available - for safety, download anyway since we can't verify
            tracing::debug!(
                "File exists but no hash or size available, re-downloading: {}",
                dest_path.display()
            );
        }
    }

    if ctx.is_cancelled() {
        return Err("Cancelled".to_string());
    }

    let mut req = client.get(&task.url);
    if task.url.contains("forgecdn.net") {
        if let Some(key) = crate::core::curseforge::CURSE_API_KEY.get() {
            req = req.header("x-api-key", key);
        } else {
            tracing::warn!("Downloading from forgecdn.net but CURSE_API_KEY is not set!");
        }
    }

    // Make the HTTP request with timeout.
    let response = match req.send().await {
        Ok(resp) => resp,
        Err(e) => {
            return Err(format!("Request failed: {}", e));
        }
    };

    // Check HTTP status.
    if !response.status().is_success() {
        return Err(format!("HTTP error: {}", response.status()));
    }

    // Get content length.
    let total = response.content_length().unwrap_or(0);

    // Create a temporary path for the download to avoid leaving broken files
    let file_name = dest_path.file_name().unwrap_or_default().to_string_lossy();
    let tmp_path = dest_path.with_file_name(format!("{}.tmp", file_name));

    // Create the destination file at tmp path.
    let mut file = match File::create(&tmp_path).await {
        Ok(f) => f,
        Err(e) => {
            return Err(format!("Failed to create file: {}", e));
        }
    };

    // Stream the response body.
    let mut stream = response.bytes_stream();
    let mut downloaded: u64 = 0;
    let mut last_emit_time = std::time::Instant::now();
    let mut last_downloaded: u64 = 0;

    while let Some(chunk_result) = stream.next().await {
        if ctx.is_cancelled() {
            // Cancel requested: abort download and clean up temp file
            drop(file);
            let _ = tokio::fs::remove_file(&tmp_path).await;
            return Err("Cancelled".to_string());
        }

        match chunk_result {
            Ok(chunk) => {
                if let Err(e) = file.write_all(&chunk).await {
                    drop(file);
                    let _ = tokio::fs::remove_file(&tmp_path).await;
                    return Err(format!("Write failed: {}", e));
                }

                downloaded += chunk.len() as u64;

                // Throttle: only emit every PROGRESS_THROTTLE_MS or every 512KB.
                let elapsed = last_emit_time.elapsed().as_millis() as u64;
                let since_last_emit = downloaded.saturating_sub(last_downloaded);

                if elapsed >= PROGRESS_THROTTLE_MS || since_last_emit >= 512 * 1024 {
                    let speed = if elapsed > 0 {
                        let bytes_per_ms = since_last_emit.saturating_div(elapsed.max(1));
                        bytes_per_ms * 1000
                    } else {
                        0
                    };

                    // Emit progress (don't fail on emit error).
                    let mut progress =
                        DownloadProgress::progress(task.id.clone(), downloaded, total, speed);
                    progress.file_name = std::path::Path::new(&task.dest_path)
                        .file_name()
                        .map(|f| f.to_string_lossy().to_string());
                    let _ = ctx.app_handle.emit("download-progress", &progress);

                    last_emit_time = std::time::Instant::now();
                    last_downloaded = downloaded;
                }
            }
            Err(e) => {
                drop(file);
                let _ = tokio::fs::remove_file(&tmp_path).await;
                return Err(format!("Stream error: {}", e));
            }
        }
    }

    // Ensure all data is flushed to disk.
    if let Err(e) = file.flush().await {
        drop(file);
        let _ = tokio::fs::remove_file(&tmp_path).await;
        return Err(format!("Flush failed: {}", e));
    }
    
    // Rename temporary file to original destination path
    drop(file); // Ensure file is closed before renaming
    if let Err(e) = tokio::fs::rename(&tmp_path, &dest_path).await {
        let _ = tokio::fs::remove_file(&tmp_path).await;
        return Err(format!("Failed to finalize file: {}", e));
    }

    tracing::debug!("Download completed: {}", task.dest_path);
    Ok(())
}

/// Run batch download with multiple files concurrently.
pub async fn run_batch_download_task(tasks: Vec<DownloadTask>, ctx: TaskContext) -> Result<(), String> {
    if tasks.is_empty() {
        tracing::warn!("batch_download called with empty task list");
        return Ok(());
    }

    let total_tasks = tasks.len();
    tracing::info!("Starting batch download of {} files", total_tasks);

    // Create a single shared HTTP client with connection pooling.
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(300))
        .pool_max_idle_per_host(16) // Keep connections alive
        .http2_adaptive_window(true)
        .build()
        .expect("Failed to create HTTP client");

    let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT));
    let completed_files = Arc::new(AtomicUsize::new(0));

    // Spawn download tasks with error isolation.
    let handles: Vec<_> = tasks
        .into_iter()
        .map(|task| {
            let task_id = task.id.clone();
            let dest_path = task.dest_path.clone(); // Clone for error logging
            let client = client.clone();
            let ctx = ctx.clone();
            let semaphore = semaphore.clone();
            let completed_files = completed_files.clone();

            tokio::spawn(async move {
                if ctx.is_cancelled() {
                    return Err("Cancelled".to_string());
                }

                // Acquire semaphore slot for concurrency control.
                let _permit = semaphore.acquire().await.expect("Semaphore closed");

                if ctx.is_cancelled() {
                    return Err("Cancelled".to_string());
                }

                tracing::info!("Downloading: {}", dest_path);

                let mut attempts = 0;
                let max_attempts = 3;
                let mut last_err = String::new();

                while attempts < max_attempts {
                    if ctx.is_cancelled() {
                        return Err("Cancelled".to_string());
                    }

                    // Execute download and handle errors gracefully.
                    match download_file_task(task.clone(), &client, &ctx).await {
                        Ok(()) => {
                            // Emit completion.
                            tracing::info!("Emitting completed for task: {}", task_id);
                            let mut progress = DownloadProgress::completed(task_id);
                            progress.file_name = std::path::Path::new(&dest_path)
                                .file_name()
                                .map(|f| f.to_string_lossy().to_string());
                            let _ = ctx.app_handle.emit("download-progress", &progress);
                            
                            // Update overall task progress
                            let count = completed_files.fetch_add(1, Ordering::SeqCst) + 1;
                            let file_str = std::path::Path::new(&dest_path).file_name().unwrap_or_default().to_string_lossy();
                            ctx.update_progress(count as u64, total_tasks as u64, &format!("Downloaded {}", file_str)).await;
                            return Ok(());
                        }
                        Err(err) => {
                            last_err = err;
                            attempts += 1;
                            if attempts < max_attempts {
                                tracing::warn!("Download failed for {}, retrying {}/{}: {}", dest_path, attempts, max_attempts, last_err);
                                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                            }
                        }
                    }
                }

                // Log error but don't propagate - single failure shouldn't crash queue.
                tracing::error!("Download failed after {} attempts: {} - {}", max_attempts, dest_path, last_err);

                // Emit error progress so frontend knows.
                let progress = DownloadProgress::failed(task_id, last_err.clone());
                let _ = ctx.app_handle.emit("download-progress", &progress);
                
                Err(last_err)
            })
        })
        .collect();

    // Wait for all downloads to complete.
    let mut error_count = 0;
    for handle in handles {
        match handle.await {
            Ok(Ok(())) => {},
            Ok(Err(e)) => {
                if e == "Cancelled" {
                    return Err("Cancelled".to_string());
                }
                error_count += 1;
                tracing::error!("Download task failed: {}", e);
            },
            Err(e) => {
                error_count += 1;
                tracing::error!("Download task panicked: {}", e);
            }
        }
    }

    if error_count > 0 {
        tracing::warn!("Batch download finished with {} errors", error_count);
        return Err(format!("{} downloads failed", error_count));
    } else {
        tracing::info!("Batch download completed successfully");
    }

    // Emit final completion event.
    let _ = ctx.app_handle.emit(
        "download-batch-complete",
        serde_json::json!({
            "total": total_tasks,
            "errors": error_count,
        }),
    );
    Ok(())
}


/// Downloads a single file with progress reporting (Legacy).
async fn download_file(
    task: DownloadTask,
    client: &reqwest::Client,
    app: &AppHandle,
    cancel_flag: Arc<AtomicBool>,
) -> Result<(), String> {
    tracing::debug!("Starting download: {} -> {}", task.url, task.dest_path);

    let dest_path = task.dest_path_buf();
    if let Some(parent) = dest_path.parent() {
        if let Err(e) = tokio::fs::create_dir_all(parent).await {
            return Err(format!("Failed to create directory: {}", e));
        }
    }

    if dest_path.exists() {
        if let Some(expected_hash) = &task.hash {
            let dest_path_clone = dest_path.clone();
            match tokio::task::spawn_blocking(move || compute_sha1_sync(&dest_path_clone))
                .await
                .map_err(|e| e.to_string())?
            {
                Ok(actual_hash) => {
                    if actual_hash.eq_ignore_ascii_case(expected_hash) {
                        return Ok(());
                    } else {
                        let _ = tokio::fs::remove_file(&dest_path).await;
                    }
                }
                Err(_) => {}
            }
        } else if let Some(expected_size) = task.expected_size {
            if let Ok(metadata) = tokio::fs::metadata(&dest_path).await {
                if metadata.len() == expected_size {
                    return Ok(());
                } else {
                    let _ = tokio::fs::remove_file(&dest_path).await;
                }
            }
        }
    }

    let response = match client.get(&task.url).send().await {
        Ok(resp) => resp,
        Err(e) => return Err(format!("Request failed: {}", e)),
    };

    if !response.status().is_success() {
        return Err(format!("HTTP error: {}", response.status()));
    }

    let total = response.content_length().unwrap_or(0);
    let file_name = dest_path.file_name().unwrap_or_default().to_string_lossy();
    let tmp_path = dest_path.with_file_name(format!("{}.tmp", file_name));

    let mut file = match File::create(&tmp_path).await {
        Ok(f) => f,
        Err(e) => return Err(format!("Failed to create file: {}", e)),
    };

    let mut stream = response.bytes_stream();
    let mut downloaded: u64 = 0;
    let mut last_emit_time = std::time::Instant::now();
    let mut last_downloaded: u64 = 0;

    while let Some(chunk_result) = stream.next().await {
        if cancel_flag.load(Ordering::Relaxed) {
            drop(file);
            let _ = tokio::fs::remove_file(&tmp_path).await;
            return Err("Cancelled".to_string());
        }

        match chunk_result {
            Ok(chunk) => {
                if let Err(e) = file.write_all(&chunk).await {
                    drop(file);
                    let _ = tokio::fs::remove_file(&tmp_path).await;
                    return Err(format!("Write failed: {}", e));
                }

                downloaded += chunk.len() as u64;
                let elapsed = last_emit_time.elapsed().as_millis() as u64;
                let since_last_emit = downloaded.saturating_sub(last_downloaded);

                if elapsed >= PROGRESS_THROTTLE_MS || since_last_emit >= 512 * 1024 {
                    let speed = if elapsed > 0 {
                        (since_last_emit.saturating_div(elapsed.max(1))) * 1000
                    } else {
                        0
                    };

                    let mut progress = DownloadProgress::progress(task.id.clone(), downloaded, total, speed);
                    progress.file_name = std::path::Path::new(&task.dest_path).file_name().map(|f| f.to_string_lossy().to_string());
                    let _ = app.emit("download-progress", &progress);

                    last_emit_time = std::time::Instant::now();
                    last_downloaded = downloaded;
                }
            }
            Err(e) => {
                drop(file);
                let _ = tokio::fs::remove_file(&tmp_path).await;
                return Err(format!("Stream error: {}", e));
            }
        }
    }

    if let Err(e) = file.flush().await {
        drop(file);
        let _ = tokio::fs::remove_file(&tmp_path).await;
        return Err(format!("Flush failed: {}", e));
    }
    
    drop(file);
    if let Err(e) = tokio::fs::rename(&tmp_path, &dest_path).await {
        let _ = tokio::fs::remove_file(&tmp_path).await;
        return Err(format!("Failed to finalize file: {}", e));
    }

    Ok(())
}

/// Run batch download with multiple files concurrently (Legacy).
pub async fn run_batch_download(tasks: Vec<DownloadTask>, app: AppHandle, cancel_flag: Arc<AtomicBool>) -> Result<(), String> {
    if tasks.is_empty() {
        return Ok(());
    }

    let total_tasks = tasks.len();
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(300))
        .pool_max_idle_per_host(16)
        .http2_adaptive_window(true)
        .build()
        .expect("Failed to create HTTP client");

    let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT));
    let handles: Vec<_> = tasks
        .into_iter()
        .map(|task| {
            let task_id = task.id.clone();
            let dest_path = task.dest_path.clone();
            let client = client.clone();
            let app = app.clone();
            let semaphore = semaphore.clone();
            let cancel_flag = cancel_flag.clone();

            tokio::spawn(async move {
                let _permit = semaphore.acquire().await.expect("Semaphore closed");
                let mut attempts = 0;
                let max_attempts = 3;
                let mut last_err = String::new();

                while attempts < max_attempts {
                    if cancel_flag.load(Ordering::SeqCst) {
                        return Err("Cancelled".to_string());
                    }

                    match download_file(task.clone(), &client, &app, cancel_flag.clone()).await {
                        Ok(()) => {
                            let mut progress = DownloadProgress::completed(task_id);
                            progress.file_name = std::path::Path::new(&dest_path).file_name().map(|f| f.to_string_lossy().to_string());
                            let _ = app.emit("download-progress", &progress);
                            return Ok(());
                        }
                        Err(err) => {
                            last_err = err;
                            attempts += 1;
                            if attempts < max_attempts {
                                tracing::warn!("Download failed for {}, retrying {}/{}: {}", dest_path, attempts, max_attempts, last_err);
                                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                            }
                        }
                    }
                }

                tracing::error!("Download failed after {} attempts: {} - {}", max_attempts, dest_path, last_err);
                let progress = DownloadProgress::failed(task_id, last_err.clone());
                let _ = app.emit("download-progress", &progress);
                Err(last_err)
            })
        })
        .collect();

    let mut error_count = 0;
    for handle in handles {
        match handle.await {
            Ok(Ok(())) => {},
            Ok(Err(e)) => {
                error_count += 1;
                tracing::error!("Download task failed: {}", e);
            },
            Err(e) => {
                error_count += 1;
                tracing::error!("Download task panicked: {}", e);
            }
        }
    }

    if error_count > 0 {
        tracing::warn!("Batch download finished with {} errors", error_count);
        return Err(format!("{} downloads failed", error_count));
    }

    let _ = app.emit(
        "download-batch-complete",
        serde_json::json!({
            "total": total_tasks,
            "errors": error_count,
        }),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_compute_sha1_sync() {
        // Create a temporary file
        let mut temp_file = NamedTempFile::new().unwrap();
        
        // Write some known data
        let data = b"hello world";
        temp_file.write_all(data).unwrap();
        temp_file.flush().unwrap();

        // sha1 of "hello world" is 2aae6c35c94fcfb415dbe95f408b9ce91ee846ed
        let expected_hash = "2aae6c35c94fcfb415dbe95f408b9ce91ee846ed";
        let actual_hash = compute_sha1_sync(temp_file.path()).unwrap();
        
        assert_eq!(actual_hash, expected_hash);
    }
}
