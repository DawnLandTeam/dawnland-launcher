use crate::downloader::{DownloadProgress, DownloadTask};
use futures_util::StreamExt;
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

/// Downloads a single file with progress reporting.
async fn download_file(
    task: DownloadTask,
    client: &reqwest::Client,
    app: AppHandle,
    semaphore: Arc<Semaphore>,
) {
    // Acquire semaphore slot.
    let _permit = semaphore.acquire().await.expect("Semaphore closed");

    tracing::info!("Starting download: {} -> {}", task.url, task.dest_path);

    // Create parent directories if needed.
    if let Some(parent) = task.dest_path_buf().parent() {
        if let Err(e) = tokio::fs::create_dir_all(parent).await {
            let progress = DownloadProgress::failed(task.id, format!("Failed to create directory: {e}"));
            let _ = app.emit("download-progress", &progress);
            tracing::error!("Failed to create directory: {e}");
            return;
        }
    }

    // Make the HTTP request.
    let response = match client.get(&task.url).send().await {
        Ok(resp) => resp,
        Err(e) => {
            let progress = DownloadProgress::failed(task.id, format!("Request failed: {e}"));
            let _ = app.emit("download-progress", &progress);
            tracing::error!("Request failed: {e}");
            return;
        }
    };

    // Get content length.
    let total = response.content_length().unwrap_or(0);

    // Create the destination file.
    let mut file = match File::create(&task.dest_path).await {
        Ok(f) => f,
        Err(e) => {
            let progress = DownloadProgress::failed(task.id, format!("Failed to create file: {e}"));
            let _ = app.emit("download-progress", &progress);
            tracing::error!("Failed to create file: {e}");
            return;
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
                    let progress = DownloadProgress::failed(task.id, format!("Write failed: {e}"));
                    let _ = app.emit("download-progress", &progress);
                    tracing::error!("Write failed: {e}");
                    return;
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
                let progress = DownloadProgress::failed(task.id, format!("Stream error: {e}"));
                let _ = app.emit("download-progress", &progress);
                tracing::error!("Stream error: {e}");
                return;
            }
        }
    }

    // Ensure all data is flushed to disk.
    if let Err(e) = file.flush().await {
        let progress = DownloadProgress::failed(task.id, format!("Flush failed: {e}"));
        let _ = app.emit("download-progress", &progress);
        tracing::error!("Flush failed: {e}");
        return;
    }

    // Emit completion.
    let progress = DownloadProgress::completed(task.id);
    let _ = app.emit("download-progress", &progress);
    tracing::info!("Download completed: {}", task.dest_path);
}

/// Run batch download with multiple files concurrently.
pub async fn run_batch_download(tasks: Vec<DownloadTask>, app: AppHandle) {
    if tasks.is_empty() {
        tracing::warn!("batch_download called with empty task list");
        return;
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(300))
        .build()
        .expect("Failed to create HTTP client");

    let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT));

    // Spawn download tasks.
    let mut handles = Vec::with_capacity(tasks.len());

    for task in tasks {
        let client = client.clone();
        let app = app.clone();
        let semaphore = semaphore.clone();

        let handle = tokio::spawn(async move {
            download_file(task, &client, app, semaphore).await;
        });

        handles.push(handle);
    }

    // Wait for all downloads to complete.
    for handle in handles {
        if let Err(e) = handle.await {
            tracing::error!("Download task panicked: {e}");
        }
    }

    tracing::info!("All downloads finished");
}