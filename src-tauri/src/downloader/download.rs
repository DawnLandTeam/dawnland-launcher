use crate::core::task::TaskContext;
use crate::downloader::{DownloadProgress, DownloadTask};
use futures_util::StreamExt;
use sha1::{Digest, Sha1};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Semaphore;
use tokio::time::Duration;

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

/// Core download logic shared by legacy and modern APIs.

fn emit_progress_throttled(
    app: &AppHandle,
    task_id: &str,
    file_name: &str,
    current_total: u64,
    total_size: u64,
    last_emit_time: &mut std::time::Instant,
    last_downloaded: &mut u64,
) {
    let elapsed = last_emit_time.elapsed().as_millis() as u64;
    let since_last = current_total.saturating_sub(*last_downloaded);

    if elapsed >= PROGRESS_THROTTLE_MS || since_last >= 512 * 1024 {
        let speed = if elapsed > 0 { since_last.saturating_div(elapsed.max(1)) * 1000 } else { 0 };
        let mut progress = DownloadProgress::progress(task_id.to_string(), current_total, total_size, speed);
        progress.file_name = Some(file_name.to_string());
        let _ = app.emit("download-progress", &progress);
        *last_emit_time = std::time::Instant::now();
        *last_downloaded = current_total;
    }
}

async fn download_file_core<C, G>(
    task: DownloadTask,
    client: reqwest::Client,
    app_handle: AppHandle,
    is_cancelled: C,
    add_global_downloaded: G,
) -> Result<(), String>
where
    C: Fn() -> bool + Send + Sync + 'static + Clone,
    G: Fn(u64) + Send + Sync + 'static + Clone,
{
    tracing::debug!("Starting download: {} -> {}", task.url, task.dest_path);

    let dest_path = task.dest_path_buf();
    if let Some(parent) = dest_path.parent() {
        if let Err(e) = tokio::fs::create_dir_all(parent).await {
            let err_msg = format!("Failed to create directory: {}", e);
            tracing::error!("{}: {}", err_msg, parent.display());
            return Err(err_msg);
        }
    }

    // Hash or size check for existing file
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

    if is_cancelled() {
        return Err("Cancelled".to_string());
    }

    let mut req = client.get(&task.url);
    if let Ok(parsed_url) = reqwest::Url::parse(&task.url) {
        if let Some(host) = parsed_url.host_str() {
            if host == "forgecdn.net" || host.ends_with(".forgecdn.net") {
                if let Some(key) = crate::core::curseforge::CURSE_API_KEY.get() {
                    req = req.header("x-api-key", key);
                }
            }
        }
    }

    // Step 1: Initial request to get headers
    let req_clone = match req.try_clone() {
        Some(r) => r,
        None => return Err("Request body cannot be cloned".to_string()),
    };
    let response = match req_clone.send().await {
        Ok(resp) => resp,
        Err(e) => return Err(format!("Request failed: {}", e)),
    };

    let mut response = if !response.status().is_success() {
        if task.url.contains("bmclapi2.bangbang93.com") {
            let fallbacks = get_bmclapi_fallbacks(&task.url);
            let mut fallback_success = false;
            let mut fallback_resp = response;
            for fallback_url in fallbacks {
                if let Ok(resp) = client.get(&fallback_url).send().await {
                    if resp.status().is_success() {
                        fallback_resp = resp;
                        fallback_success = true;
                        break;
                    }
                }
            }
            if !fallback_success {
                return Err(format!("HTTP error: {} (Fallbacks failed)", fallback_resp.status()));
            }
            fallback_resp
        } else {
            return Err(format!("HTTP error: {}", response.status()));
        }
    } else {
        response
    };

    let total_size = response.content_length().unwrap_or(0);
    let accept_ranges = response.headers().get(reqwest::header::ACCEPT_RANGES).map(|v| v.to_str().unwrap_or("")) == Some("bytes")
        || task.url.contains("forgecdn.net")
        || task.url.contains("modrinth.com")
        || task.url.contains("github.com")
        || task.url.contains("githubusercontent.com")
        || task.url.contains("bangbang93.com")
        || task.url.contains("mojang.com")
        || task.url.contains("minecraft.net");

    let chunk_count = if accept_ranges && total_size > 0 {
        let mb = total_size / (1024 * 1024);
        if mb <= 50 { 1 }
        else if mb <= 100 { 4 }
        else if mb <= 500 { 8 }
        else if mb <= 1024 { 16 }
        else { 32 }
    } else {
        1
    };

    let file_name = dest_path.file_name().unwrap_or_default().to_string_lossy();
    let tmp_path = dest_path.with_file_name(format!("{}.tmp", file_name));

    // Support small file resume
    let mut initial_downloaded = 0;
    if chunk_count == 1 {
        if let Ok(meta) = tokio::fs::metadata(&tmp_path).await {
            initial_downloaded = meta.len();
            if initial_downloaded > 0 && accept_ranges && initial_downloaded < total_size {
                // We drop the initial response and create a new range request
                let url = response.url().clone();
                drop(response);
                let mut range_req = client.get(url.clone()).header("Range", format!("bytes={}-", initial_downloaded));
                if let Some(host) = url.host_str() {
                    if host == "forgecdn.net" || host.ends_with(".forgecdn.net") {
                        if let Some(key) = crate::core::curseforge::CURSE_API_KEY.get() {
                            range_req = range_req.header("x-api-key", key);
                        }
                    }
                }
                let range_resp = match range_req.send().await {
                    Ok(r) => r,
                    Err(e) => return Err(format!("Range request failed: {}", e)),
                };
                if range_resp.status().is_success() {
                    response = range_resp;
                } else {
                    initial_downloaded = 0;
                    let mut fallback_req = client.get(url.clone());
                    if let Some(host) = url.host_str() {
                        if host == "forgecdn.net" || host.ends_with(".forgecdn.net") {
                            if let Some(key) = crate::core::curseforge::CURSE_API_KEY.get() {
                                fallback_req = fallback_req.header("x-api-key", key);
                            }
                        }
                    }
                    response = fallback_req.send().await.map_err(|e| e.to_string())?;
                }
            } else if initial_downloaded >= total_size {
                initial_downloaded = 0;
                let _ = tokio::fs::remove_file(&tmp_path).await;
            }
        }
    }

    if chunk_count > 1 {
        let final_url = response.url().to_string();
        drop(response); // Cancel the sequential download body stream
        let chunk_size = total_size / chunk_count;
        let mut parts = vec![];
        let mut ranges = vec![];

        for i in 0..chunk_count {
            let start = i * chunk_size;
            let end = if i == chunk_count - 1 { total_size - 1 } else { (i + 1) * chunk_size - 1 };
            parts.push(dest_path.with_file_name(format!("{}.tmp.part{}", file_name, i)));
            ranges.push((start, end));
        }

        let task_downloaded = Arc::new(AtomicU64::new(0));
        let mut spawn_handles = vec![];

        // Ensure not too many simultaneous connections globally by limiting inside chunk logic.
        // Actually, we can use an inner semaphore to avoid overwhelming the network with 32 connections per file.
        let inner_sem = Arc::new(Semaphore::new(4));

        // Pre-check parts and calculate already downloaded
        let mut initial_dl = 0;
        for i in 0..chunk_count as usize {
            if let Ok(meta) = tokio::fs::metadata(&parts[i]).await {
                let size = meta.len();
                let expected = ranges[i].1 - ranges[i].0 + 1;
                if size == expected {
                    initial_dl += expected;
                } else if size < expected {
                    initial_dl += size;
                }
            }
        }
        task_downloaded.store(initial_dl, Ordering::Relaxed);

        let last_emit_time_global = Arc::new(tokio::sync::Mutex::new(std::time::Instant::now()));
        let last_downloaded_global = Arc::new(AtomicU64::new(initial_dl));
        
        let client_clone = client.clone();
        let target_url = final_url;

        for i in 0..chunk_count as usize {
            let part_path = parts[i].clone();
            let (start, end) = ranges[i];
            let client = client_clone.clone();
            let url = target_url.clone();
            let app = app_handle.clone();
            let task_id = task.id.clone();
            let file_name_clone = file_name.to_string();
            let global_dl = task_downloaded.clone();
            let add_global_dl = add_global_downloaded.clone();
            let last_emit_time_global = last_emit_time_global.clone();
            let last_downloaded_global = last_downloaded_global.clone();
            let inner_sem = inner_sem.clone();

            let is_cancelled_clone = is_cancelled.clone();

            let h = tokio::spawn(async move {
                let _permit = inner_sem.acquire().await.unwrap();
                if is_cancelled_clone() { return Err("Cancelled".to_string()); }

                let expected_size = end - start + 1;
                let mut current_start = start;
                let mut append = false;

                if let Ok(meta) = tokio::fs::metadata(&part_path).await {
                    let size = meta.len();
                    if size == expected_size {
                        return Ok(()); // Already done
                    } else if size < expected_size {
                        current_start = start + size;
                        append = true;
                    } else {
                        let _ = tokio::fs::remove_file(&part_path).await;
                    }
                }

                let mut req = client.get(&url).header("Range", format!("bytes={}-{}", current_start, end));
                if let Ok(parsed_url) = reqwest::Url::parse(&url) {
                    if let Some(host) = parsed_url.host_str() {
                        if host == "forgecdn.net" || host.ends_with(".forgecdn.net") {
                            if let Some(key) = crate::core::curseforge::CURSE_API_KEY.get() {
                                req = req.header("x-api-key", key);
                            }
                        }
                    }
                }
                let mut resp = req.send().await.map_err(|e| e.to_string())?;
                
                // Safety check: if server ignores Range and returns 200 OK with the full file,
                // we should abort to prevent downloading the whole file N times.
                if resp.status() == reqwest::StatusCode::OK && expected_size < total_size {
                    return Err("Server ignored Range header and returned full file".to_string());
                }
                
                if !resp.status().is_success() {
                    return Err(format!("Chunk error: {}", resp.status()));
                }

                let mut file = OpenOptions::new()
                    .create(true)
                    .write(true)
                    .append(append)
                    .truncate(!append)
                    .open(&part_path)
                    .await
                    .map_err(|e| e.to_string())?;

                let mut stream = resp.bytes_stream();
                while let Some(chunk_result) = stream.next().await {
                    if is_cancelled_clone() {
                        drop(file);
                        return Err("Cancelled".to_string());
                    }
                    let chunk = chunk_result.map_err(|e| e.to_string())?;
                    file.write_all(&chunk).await.map_err(|e| e.to_string())?;
                    
                    let chunk_len = chunk.len() as u64;
                    global_dl.fetch_add(chunk_len, Ordering::Relaxed);
                    add_global_dl(chunk_len);

                    // Throttle emit
                    let mut emit_time = last_emit_time_global.lock().await;
                    let current_total = global_dl.load(Ordering::Relaxed);
                    let mut last_dl = last_downloaded_global.load(Ordering::Relaxed);
                    emit_progress_throttled(
                        &app,
                        &task_id,
                        &file_name_clone,
                        current_total,
                        total_size,
                        &mut emit_time,
                        &mut last_dl,
                    );
                    last_downloaded_global.store(last_dl, Ordering::Relaxed);
                }
                file.flush().await.map_err(|e| e.to_string())?;
                Ok(())
            });
            spawn_handles.push(h);
        }

        for h in spawn_handles {
            h.await.map_err(|e| e.to_string())??;
        }

        // Merge parts
        let mut final_file = File::create(&tmp_path).await.map_err(|e| e.to_string())?;
        for part in &parts {
            let mut part_file = File::open(part).await.map_err(|e| e.to_string())?;
            tokio::io::copy(&mut part_file, &mut final_file).await.map_err(|e| e.to_string())?;
        }
        final_file.flush().await.map_err(|e| e.to_string())?;
        drop(final_file);

        if total_size > 0 {
            let metadata = tokio::fs::metadata(&tmp_path)
                .await
                .map_err(|e| e.to_string())?;

            if metadata.len() != total_size {
                let _ = tokio::fs::remove_file(&tmp_path).await;
                return Err(format!(
                    "downloaded size mismatch: expected {}, got {}",
                    total_size,
                    metadata.len()
                ));
            }
        }

        for part in parts {
            let _ = tokio::fs::remove_file(&part).await;
        }

        if let Err(e) = tokio::fs::rename(&tmp_path, &dest_path).await {
            let _ = tokio::fs::remove_file(&tmp_path).await;
            return Err(format!("Failed to finalize file: {}", e));
        }

        tracing::debug!("Chunked download completed: {}", task.dest_path);
        return Ok(());
    }

    // Sequential fallback
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .append(initial_downloaded > 0)
        .truncate(initial_downloaded == 0)
        .open(&tmp_path)
        .await
        .map_err(|e| e.to_string())?;

    let mut stream = response.bytes_stream();
    let mut downloaded: u64 = initial_downloaded;
    let mut last_emit_time = std::time::Instant::now();
    let mut last_downloaded: u64 = initial_downloaded;

    while let Some(chunk_result) = stream.next().await {
        if is_cancelled() {
            drop(file);
            let _ = tokio::fs::remove_file(&tmp_path).await;
            return Err("Cancelled".to_string());
        }

        match chunk_result {
            Ok(chunk) => {
                file.write_all(&chunk).await.map_err(|e| e.to_string())?;
                downloaded += chunk.len() as u64;
                add_global_downloaded(chunk.len() as u64);

                emit_progress_throttled(
                    &app_handle,
                    &task.id,
                    &file_name,
                    downloaded,
                    total_size,
                    &mut last_emit_time,
                    &mut last_downloaded,
                );
            }
            Err(e) => {
                drop(file);
                let _ = tokio::fs::remove_file(&tmp_path).await;
                return Err(format!("Stream error: {}", e));
            }
        }
    }
    file.flush().await.map_err(|e| e.to_string())?;
    drop(file);
    if total_size > 0 {
        let metadata = tokio::fs::metadata(&tmp_path)
            .await
            .map_err(|e| e.to_string())?;

        if metadata.len() != total_size {
            let _ = tokio::fs::remove_file(&tmp_path).await;
            return Err(format!(
                "downloaded size mismatch: expected {}, got {}",
                total_size,
                metadata.len()
            ));
        }
    }
    if let Err(e) = tokio::fs::rename(&tmp_path, &dest_path).await {
        let _ = tokio::fs::remove_file(&tmp_path).await;
        return Err(format!("Failed to finalize file: {}", e));
    }
    tracing::debug!("Download completed: {}", task.dest_path);
    Ok(())
}

pub async fn download_file_task(
    task: DownloadTask,
    client: reqwest::Client,
    ctx: &TaskContext,
    global_downloaded: &Arc<AtomicU64>,
) -> Result<(), String> {
    let cancel_token = ctx.cancel_token.clone();
    let is_cancelled = move || cancel_token.is_cancelled();
    let global_dl = global_downloaded.clone();
    let add_global = move |bytes: u64| {
        global_dl.fetch_add(bytes, Ordering::Relaxed);
    };
    download_file_core(task, client.clone(), ctx.app_handle.clone(), is_cancelled, add_global).await
}

async fn download_file(
    task: DownloadTask,
    client: reqwest::Client,
    app: &AppHandle,
    cancel_flag: Arc<AtomicBool>,
) -> Result<(), String> {
    let cancel_flag_clone = cancel_flag.clone();
    let is_cancelled = move || cancel_flag_clone.load(Ordering::Relaxed);
    let add_global = |_| {};
    download_file_core(task, client.clone(), app.clone(), is_cancelled, add_global).await
}

pub async fn run_batch_download_task(
    tasks: Vec<DownloadTask>,
    ctx: TaskContext,
) -> Result<(), String> {
    if tasks.is_empty() {
        tracing::warn!("batch_download called with empty task list");
        return Ok(());
    }

    let total_tasks = tasks.len();
    tracing::info!("Starting batch download of {} files", total_tasks);

    // Create a single shared HTTP client with connection pooling.
    let client = crate::core::utils::get_http_client().clone();

    let settings = crate::core::settings::get_launcher_settings_sync();
    let max_concurrent = settings.max_concurrent_downloads as usize;
    let semaphore = Arc::new(Semaphore::new(max_concurrent.max(1)));
    let completed_files = Arc::new(AtomicUsize::new(0));

    // Global speed tracking
    let global_downloaded = Arc::new(AtomicU64::new(0));
    let global_dl_clone = global_downloaded.clone();
    let ctx_monitor = ctx.clone();
    let total_files = total_tasks as u32;
    let completed_files_clone = completed_files.clone();

    let monitor_handle = tokio::spawn(async move {
        let mut last_bytes = 0;
        let mut last_time = tokio::time::Instant::now();
        loop {
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            if ctx_monitor.is_cancelled() {
                break;
            }
            let current_bytes = global_dl_clone.load(Ordering::Relaxed);
            let elapsed = last_time.elapsed().as_secs_f64();
            if elapsed > 0.0 {
                let speed = ((current_bytes - last_bytes) as f64 / elapsed) as u64;
                let remaining =
                    total_files.saturating_sub(completed_files_clone.load(Ordering::SeqCst) as u32);
                ctx_monitor.update_download_metrics(speed, remaining).await;
            }
            last_bytes = current_bytes;
            last_time = tokio::time::Instant::now();

            if completed_files_clone.load(Ordering::SeqCst) >= total_tasks {
                break;
            }
        }
        // Reset speed to 0 when done
        ctx_monitor.update_download_metrics(0, 0).await;
    });

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
            let global_downloaded_clone = global_downloaded.clone();

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
                    match download_file_task(task.clone(), client.clone(), &ctx, &global_downloaded_clone)
                        .await
                    {
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
                            let file_str = std::path::Path::new(&dest_path)
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy();
                            ctx.update_progress(
                                count as u64,
                                total_tasks as u64,
                                &format!("Downloaded {}", file_str),
                            )
                            .await;
                            return Ok(());
                        }
                        Err(err) => {
                            last_err = err;
                            attempts += 1;
                            if attempts < max_attempts {
                                tracing::warn!(
                                    "Download failed for {}, retrying {}/{}: {}",
                                    dest_path,
                                    attempts,
                                    max_attempts,
                                    last_err
                                );
                                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                            }
                        }
                    }
                }

                // Log error but don't propagate - single failure shouldn't crash queue.
                tracing::error!(
                    "Download failed after {} attempts: {} - {}",
                    max_attempts,
                    dest_path,
                    last_err
                );

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
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                if e == "Cancelled" {
                    monitor_handle.abort();
                    return Err("Cancelled".to_string());
                }
                error_count += 1;
                tracing::error!("Download task failed: {}", e);
            }
            Err(e) => {
                error_count += 1;
                tracing::error!("Download task panicked: {}", e);
            }
        }
    }

    if error_count > 0 {
        monitor_handle.abort();
        tracing::warn!("Batch download finished with {} errors", error_count);
        return Err(format!("{} downloads failed", error_count));
    } else {
        let _ = monitor_handle.await;
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
pub async fn run_batch_download(
    tasks: Vec<DownloadTask>,
    app: AppHandle,
    cancel_flag: Arc<AtomicBool>,
) -> Result<(), String> {
    if tasks.is_empty() {
        return Ok(());
    }

    let total_tasks = tasks.len();
    let client = crate::core::utils::get_http_client().clone();

    let settings = crate::core::settings::get_launcher_settings_sync();
    let max_concurrent = settings.max_concurrent_downloads as usize;
    let semaphore = Arc::new(Semaphore::new(max_concurrent.max(1)));
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

                    match download_file(task.clone(), client.clone(), &app, cancel_flag.clone()).await {
                        Ok(()) => {
                            let mut progress = DownloadProgress::completed(task_id);
                            progress.file_name = std::path::Path::new(&dest_path)
                                .file_name()
                                .map(|f| f.to_string_lossy().to_string());
                            let _ = app.emit("download-progress", &progress);
                            return Ok(());
                        }
                        Err(err) => {
                            last_err = err;
                            attempts += 1;
                            if attempts < max_attempts {
                                tracing::warn!(
                                    "Download failed for {}, retrying {}/{}: {}",
                                    dest_path,
                                    attempts,
                                    max_attempts,
                                    last_err
                                );
                                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                            }
                        }
                    }
                }

                tracing::error!(
                    "Download failed after {} attempts: {} - {}",
                    max_attempts,
                    dest_path,
                    last_err
                );
                let progress = DownloadProgress::failed(task_id, last_err.clone());
                let _ = app.emit("download-progress", &progress);
                Err(last_err)
            })
        })
        .collect();

    let mut error_count = 0;
    for handle in handles {
        match handle.await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                error_count += 1;
                tracing::error!("Download task failed: {}", e);
            }
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

/// Generates a list of fallback URLs for a given BMCLAPI URL.
fn get_bmclapi_fallbacks(url: &str) -> Vec<String> {
    let mut fallbacks = vec![];
    if url.contains("/assets/") {
        fallbacks.push(url.replace("https://bmclapi2.bangbang93.com/assets", "https://resources.download.minecraft.net"));
    } else if url.contains("/fabric-meta/") {
        fallbacks.push(url.replace("https://bmclapi2.bangbang93.com/fabric-meta", "https://meta.fabricmc.net"));
    } else if url.contains("/maven/") {
        if url.contains("/maven/net/minecraftforge/") || url.contains("/maven/de/oceanlabs/") {
            fallbacks.push(url.replace("https://bmclapi2.bangbang93.com/maven", "https://maven.minecraftforge.net"));
        } else if url.contains("/maven/net/fabricmc/") {
            fallbacks.push(url.replace("https://bmclapi2.bangbang93.com/maven", "https://maven.fabricmc.net"));
        } else if url.contains("/maven/net/neoforged/") {
            fallbacks.push(url.replace("https://bmclapi2.bangbang93.com/maven", "https://maven.neoforged.net/releases"));
        } else {
            // Default to mojang libraries
            fallbacks.push(url.replace("https://bmclapi2.bangbang93.com/maven", "https://libraries.minecraft.net"));
        }
        
        // Add all other common maven repos as a safety net (e.g. for org.ow2.asm)
        let other_mavens = [
            "https://libraries.minecraft.net",
            "https://maven.fabricmc.net",
            "https://maven.minecraftforge.net",
            "https://maven.neoforged.net/releases"
        ];
        for repo in other_mavens.iter() {
            let fallback_url = url.replace("https://bmclapi2.bangbang93.com/maven", repo);
            if !fallbacks.contains(&fallback_url) {
                fallbacks.push(fallback_url);
            }
        }
    } else {
        fallbacks.push(url.replace("https://bmclapi2.bangbang93.com", "https://piston-meta.mojang.com"));
        fallbacks.push(url.replace("https://bmclapi2.bangbang93.com", "https://launchermeta.mojang.com"));
    }
    fallbacks
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


