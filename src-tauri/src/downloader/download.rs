use crate::downloader::{DownloadProgress, DownloadTask};
use futures_util::StreamExt;
use sha1::{Digest, Sha1};
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
fn compute_sha1_sync(path: &std::path::Path) -> Result<String, String> {
    use std::io::Read;
    
    let file = std::fs::File::open(path)
        .map_err(|e| format!("Failed to open file for hashing: {}", e))?;
    
    let mut reader = std::io::BufReader::new(file);
    let mut hasher = Sha1::new();
    let mut buffer = [0u8; 8192];
    
    loop {
        let bytes_read = reader.read(&mut buffer)
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
async fn download_file(
    task: DownloadTask,
    client: &reqwest::Client,
    app: &AppHandle,
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

    // Check if file already exists and verify integrity with hash
    if dest_path.exists() {
        if let Some(expected_hash) = &task.hash {
            // Hash is available - compute existing file's hash for verification
            let dest_path_clone = dest_path.clone();
            let existing_hash = tokio::task::spawn_blocking(move || {
                compute_sha1_sync(&dest_path_clone)
            }).await
            .map_err(|e| format!("Task join error: {}", e))?;
            
            match existing_hash {
                Ok(actual_hash) => {
                    if actual_hash == *expected_hash {
                        tracing::debug!("File exists and hash matches, skipping: {}", task.dest_path);
                        return Ok(());
                    } else {
                        tracing::debug!("File exists but hash mismatch, re-downloading: {} (expected: {}, got: {})", 
                            task.dest_path, expected_hash, actual_hash);
                        // Hash mismatch - delete the corrupted file and re-download
                        tokio::fs::remove_file(&dest_path).await
                            .map_err(|e| format!("Failed to remove corrupted file: {}", e))?;
                    }
                }
                Err(e) => {
                    // Failed to compute hash - delete and re-download
                    tracing::warn!("Failed to compute hash for existing file: {}, re-downloading", e);
                    let _ = tokio::fs::remove_file(&dest_path).await;
                }
            }
        } else {
            // No hash available - for safety, download anyway since we can't verify
            tracing::debug!("File exists but no hash available, re-downloading: {}", task.dest_path);
        }
    }

    // Make the HTTP request with timeout.
    let response = match client.get(&task.url).send().await {
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

    // Create the destination file.
    let mut file = match File::create(&dest_path).await {
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
        match chunk_result {
            Ok(chunk) => {
                if let Err(e) = file.write_all(&chunk).await {
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
                    let progress = DownloadProgress::progress(
                        task.id.clone(),
                        downloaded,
                        total,
                        speed,
                    );
                    let _ = app.emit("download-progress", &progress);

                    last_emit_time = std::time::Instant::now();
                    last_downloaded = downloaded;
                }
            }
            Err(e) => {
                return Err(format!("Stream error: {}", e));
            }
        }
    }

    // Ensure all data is flushed to disk.
    if let Err(e) = file.flush().await {
        return Err(format!("Flush failed: {}", e));
    }

    tracing::debug!("Download completed: {}", task.dest_path);
    Ok(())
}

/// Run batch download with multiple files concurrently.
pub async fn run_batch_download(tasks: Vec<DownloadTask>, app: AppHandle) {
    if tasks.is_empty() {
        tracing::warn!("batch_download called with empty task list");
        return;
    }

    let total_tasks = tasks.len();
    tracing::info!("Starting batch download of {} files", total_tasks);

    // Create a single shared HTTP client with connection pooling.
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(300))
        .pool_max_idle_per_host(16)  // Keep connections alive
        .http2_adaptive_window(true)
        .build()
        .expect("Failed to create HTTP client");

    let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT));

    // Spawn download tasks with error isolation.
    let handles: Vec<_> = tasks
        .into_iter()
        .map(|task| {
            let task_id = task.id.clone();
            let dest_path = task.dest_path.clone(); // Clone for error logging
            let client = client.clone();
            let app = app.clone();
            let semaphore = semaphore.clone();

            tokio::spawn(async move {
                // Acquire semaphore slot for concurrency control.
                let _permit = semaphore.acquire().await.expect("Semaphore closed");

                tracing::info!("Downloading: {}", dest_path);
                
                // Execute download and handle errors gracefully.
                match download_file(task, &client, &app).await {
                    Ok(()) => {
                        // Emit completion.
                        tracing::info!("Emitting completed for task: {}", task_id);
                        let progress = DownloadProgress::completed(task_id);
                        let _ = app.emit("download-progress", &progress);
                    }
                    Err(err) => {
                        // Log error but don't propagate - single failure shouldn't crash queue.
                        tracing::error!("Download failed: {} - {}", dest_path, err);
                        
                        // Emit error progress so frontend knows.
                        let progress = DownloadProgress::failed(task_id, err);
                        let _ = app.emit("download-progress", &progress);
                    }
                }
            })
        })
        .collect();

    // Wait for all downloads to complete (errors already handled internally).
    let mut error_count = 0;
    for handle in handles {
        if let Err(e) = handle.await {
            error_count += 1;
            tracing::error!("Download task panicked: {}", e);
        }
    }

    if error_count > 0 {
        tracing::warn!("Batch download finished with {} task panics", error_count);
    } else {
        tracing::info!("Batch download completed successfully");
    }

    // Emit final completion event.
    let _ = app.emit("download-batch-complete", serde_json::json!({
        "total": total_tasks,
        "errors": error_count,
    }));
}