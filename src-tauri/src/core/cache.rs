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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

static GC_QUEUED: AtomicBool = AtomicBool::new(false);
static GC_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

pub fn trigger_auto_gc(app: AppHandle) {
    if GC_QUEUED.swap(true, Ordering::SeqCst) {
        return;
    }
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        
        let lock = GC_LOCK.get_or_init(|| tokio::sync::Mutex::new(()));
        let _guard = lock.lock().await;
        
        GC_QUEUED.store(false, Ordering::SeqCst);
        
        tracing::debug!("Running debounced auto global mod cache cleanup...");
        if let Err(e) = clean_global_mod_cache(app).await {
            tracing::warn!("Auto cleanup of global mod cache failed: {}", e);
        }
    });
}

/// Clean unreferenced global mod cache files
#[tauri::command]
pub async fn clean_global_mod_cache(_app: AppHandle) -> Result<u64, String> {
    let lock = GC_LOCK.get_or_init(|| tokio::sync::Mutex::new(()));
    let _guard = lock.lock().await;

    let base_dir = crate::core::mojang::get_minecraft_base();
    let versions_dir = base_dir.join("versions");

    let alive_hashes = tokio::task::spawn_blocking(move || {
        let mut hashes = std::collections::HashSet::new();
        let entries = match std::fs::read_dir(&versions_dir) {
            Ok(e) => e,
            Err(_) => return hashes,
        };

        for entry in entries.flatten() {
            let instance_dir = entry.path();
            if !instance_dir.is_dir() {
                continue;
            }

            let assets_path = instance_dir.join("assets.json");
            if let Ok(content) = std::fs::read_to_string(assets_path) {
                if let Ok(manifest) = serde_json::from_str::<crate::models::instance::AssetManifest>(&content) {
                    for (_path_key, record) in manifest.assets {
                        if let Some(hash) = record.hash_sha1 {
                            hashes.insert(hash.to_lowercase());
                        }
                    }
                }
            }
        }
        hashes
    })
    .await
    .map_err(|e| e.to_string())?;

    let mut freed_bytes = 0;
    let pools_dir = crate::core::mojang::get_dawnland_dir().join("pools");
    let target_dir = pools_dir.join("objects");

    if !target_dir.exists() {
        tracing::info!("Cleaned global mod cache, freed 0 bytes");
        return Ok(0);
    }

    let mut target_empty = true;
    if let Ok(mut entries) = tokio::fs::read_dir(&target_dir).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            let meta = match entry.metadata().await {
                Ok(m) => m,
                Err(_) => {
                    target_empty = false;
                    continue;
                }
            };

            if meta.is_dir() {
                let mut sub_empty = true;
                if let Ok(mut sub_entries) = tokio::fs::read_dir(entry.path()).await {
                    while let Ok(Some(sub_entry)) = sub_entries.next_entry().await {
                        let sub_meta = match sub_entry.metadata().await {
                            Ok(m) => m,
                            Err(_) => {
                                sub_empty = false;
                                continue;
                            }
                        };

                        if !sub_meta.is_file() {
                            sub_empty = false;
                            continue;
                        }

                        let file_name = sub_entry.file_name().to_string_lossy().to_string();
                        let is_alive = alive_hashes.contains(&file_name);

                        let mut nlink = 1;
                        #[cfg(unix)]
                        {
                            use std::os::unix::fs::MetadataExt;
                            nlink = sub_meta.nlink();
                        }
                        #[cfg(windows)]
                        {
                            use std::os::windows::io::AsRawHandle;
                            use std::ffi::c_void;

                            #[repr(C)]
                            struct Filetime {
                                dw_low_date_time: u32,
                                dw_high_date_time: u32,
                            }

                            #[repr(C)]
                            struct ByHandleFileInformation {
                                dw_file_attributes: u32,
                                ft_creation_time: Filetime,
                                ft_last_access_time: Filetime,
                                ft_last_write_time: Filetime,
                                dw_volume_serial_number: u32,
                                n_file_size_high: u32,
                                n_file_size_low: u32,
                                n_number_of_links: u32,
                                n_file_index_high: u32,
                                n_file_index_low: u32,
                            }

                            extern "system" {
                                fn GetFileInformationByHandle(
                                    h_file: *mut c_void,
                                    lp_file_information: *mut ByHandleFileInformation,
                                ) -> i32;
                            }

                            if let Ok(file) = std::fs::File::open(sub_entry.path()) {
                                let handle = file.as_raw_handle();
                                let mut info: ByHandleFileInformation = unsafe { std::mem::zeroed() };
                                
                                let result = unsafe { GetFileInformationByHandle(handle, &mut info) };
                                if result != 0 {
                                    nlink = info.n_number_of_links as u64;
                                }
                            }
                        }

                        let mut recent = false;
                        if let Ok(modified) = sub_meta.modified() {
                            match std::time::SystemTime::now().duration_since(modified) {
                                Ok(duration) => {
                                    if duration.as_secs() < 86400 {
                                        recent = true;
                                    }
                                }
                                Err(_) => recent = true, // mtime is in the future
                            }
                        }

                        if is_alive || recent {
                            sub_empty = false;
                        } else {
                            let hash_for_lock = file_name.strip_suffix(".tmp").unwrap_or(&file_name);
                            let _hash_lock = crate::downloader::download::get_download_lock(hash_for_lock).await.lock_owned().await;
                            
                            // Re-check mtime inside the lock in case a concurrent download task just updated it
                            let mut still_recent = false;
                            if let Ok(new_meta) = tokio::fs::metadata(sub_entry.path()).await {
                                if let Ok(modified) = new_meta.modified() {
                                    match std::time::SystemTime::now().duration_since(modified) {
                                        Ok(duration) => {
                                            if duration.as_secs() < 86400 {
                                                still_recent = true;
                                            }
                                        }
                                        Err(_) => still_recent = true, // mtime is in the future
                                    }
                                }
                            } else {
                                still_recent = true; // file might be gone or inaccessible
                            }

                            if still_recent || tokio::fs::remove_file(sub_entry.path()).await.is_err() {
                                sub_empty = false;
                            } else {
                                if nlink <= 1 {
                                    freed_bytes += sub_meta.len();
                                }
                            }
                        }
                    }
                } else {
                    sub_empty = false;
                }

                if sub_empty {
                    let _ = tokio::fs::remove_dir(entry.path()).await;
                } else {
                    target_empty = false;
                }
            } else {
                // Unexpected file in objects/
                let mut recent = false;
                if let Ok(modified) = meta.modified() {
                    match std::time::SystemTime::now().duration_since(modified) {
                        Ok(duration) => {
                            if duration.as_secs() < 3600 {
                                recent = true;
                            }
                        }
                        Err(_) => recent = true, // mtime is in the future
                    }
                }
                
                let file_name = entry.file_name().to_string_lossy().to_string();
                if file_name.ends_with(".tmp") || recent || tokio::fs::remove_file(entry.path()).await.is_err() {
                    target_empty = false;
                } else {
                    freed_bytes += meta.len();
                }
            }
        }
    }

    if target_empty {
        let _ = tokio::fs::remove_dir(&target_dir).await;
    }

    tracing::info!("Cleaned global mod cache, freed {} bytes", freed_bytes);
    Ok(freed_bytes)
}
