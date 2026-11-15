use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::LazyLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::Mutex;

use crate::core::launcher::{should_use_library, InstanceConfig};
use crate::core::mod_parser::ModParser;
use crate::core::mojang::{maven_name_to_path, AssetIndexMeta, Library, VersionMeta};
use crate::core::settings::{load_launcher_settings, CacheRetentionPolicy, LauncherSettings};
use crate::models::instance::AssetManifest;

const AUTO_GC_DEBOUNCE_SECS: u64 = 3;
const STARTUP_GC_DELAY_SECS: u64 = 5;
const TMP_ORPHAN_GRACE_SECS: u64 = 3600;
const VERSION_CACHE_TTL_SECS: u64 = 15 * 24 * 60 * 60;
const MAX_INHERITANCE_DEPTH: usize = 8;

static GC_QUEUED: AtomicBool = AtomicBool::new(false);
static GC_DEFERRED: AtomicBool = AtomicBool::new(false);
static GC_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

#[derive(Debug, Clone)]
pub struct CachePaths {
    pub dawnland_dir: PathBuf,
    pub minecraft_base: PathBuf,
}

impl CachePaths {
    pub fn current() -> Self {
        Self {
            dawnland_dir: crate::core::mojang::get_dawnland_dir().clone(),
            minecraft_base: crate::core::mojang::get_minecraft_base().clone(),
        }
    }

    pub fn pools_objects_dir(&self) -> PathBuf {
        self.dawnland_dir.join("pools").join("objects")
    }

    pub fn version_cache_dir(&self) -> PathBuf {
        self.dawnland_dir.join("cache")
    }

    pub fn versions_dir(&self) -> PathBuf {
        self.minecraft_base.join("versions")
    }

    pub fn assets_dir(&self) -> PathBuf {
        self.minecraft_base.join("assets")
    }

    pub fn libraries_dir(&self) -> PathBuf {
        self.minecraft_base.join("libraries")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GcMode {
    Auto,
    Manual,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CacheGcReport {
    pub freed_bytes: u64,
    pub removed_pool_objects: u64,
    pub removed_tmp_files: u64,
    pub removed_mod_cache_entries: u64,
    pub removed_mod_icons: u64,
    pub removed_version_caches: u64,
    pub removed_assets: u64,
    pub removed_libraries: u64,
    pub skipped_reason: Option<String>,
}

/// Schedule a startup cache sweep after a short non-blocking delay.
pub fn trigger_startup_gc(app: AppHandle) {
    schedule_auto_gc(app, Duration::from_secs(STARTUP_GC_DELAY_SECS));
}

/// Schedule a debounced automatic unreferenced cache cleanup.
pub fn trigger_auto_gc(app: AppHandle) {
    schedule_auto_gc(app, Duration::from_secs(AUTO_GC_DEBOUNCE_SECS));
}

/// Notify the cache GC coordinator when an active background task finishes.
pub fn on_active_task_finished(app: AppHandle) {
    if !GC_DEFERRED.load(Ordering::Acquire) {
        return;
    }
    tokio::spawn(async move {
        let is_still_active = if let Some(tm) = app.try_state::<crate::core::task::TaskManager>() {
            tm.has_active_tasks().await
        } else {
            false
        };
        if !is_still_active && GC_DEFERRED.swap(false, Ordering::SeqCst) {
            trigger_auto_gc(app);
        }
    });
}

fn schedule_auto_gc(app: AppHandle, delay: Duration) {
    if GC_QUEUED.swap(true, Ordering::SeqCst) {
        return;
    }
    tokio::spawn(async move {
        tokio::time::sleep(delay).await;
        let lock = GC_LOCK.lock().await;
        GC_QUEUED.store(false, Ordering::SeqCst);

        let settings = load_launcher_settings().await.unwrap_or_default();
        if !settings.auto_clean_cache {
            GC_DEFERRED.store(false, Ordering::SeqCst);
            tracing::debug!("Auto cache GC skipped (disabled by user setting)");
            return;
        }

        let paths = CachePaths::current();
        let mut report = run_gc_with_barriers(Some(&app), &paths, GcMode::Auto, &settings).await;

        // Re-check blocker state if deferred to avoid missed wakeups when a task finishes mid-check.
        if report.skipped_reason.as_deref() == Some("active_task") {
            GC_DEFERRED.store(true, Ordering::SeqCst);
            if check_active_barriers(Some(&app), &paths).await.is_none()
                && GC_DEFERRED.swap(false, Ordering::SeqCst)
            {
                report = run_gc_with_barriers(Some(&app), &paths, GcMode::Auto, &settings).await;
            }
        } else {
            GC_DEFERRED.store(false, Ordering::SeqCst);
        }

        drop(lock);

        if report.skipped_reason.is_none() && report.freed_bytes > 0 {
            let _ = app.emit("cache-gc-completed", &report);
        }
    });
}

/// Legacy startup helper kept for direct invocations; obeys auto-GC rules safely.
pub async fn cleanup_expired_cache() {
    let settings = load_launcher_settings().await.unwrap_or_default();
    if !settings.auto_clean_cache {
        return;
    }
    let _guard = GC_LOCK.lock().await;
    let paths = CachePaths::current();
    let _ = run_gc_with_barriers(None, &paths, GcMode::Auto, &settings).await;
}

/// Manually clear safe-to-remove version cache folders under `.dawnland/cache`.
#[tauri::command]
pub async fn clean_dawnland_cache(app: AppHandle) -> Result<(), String> {
    let _guard = GC_LOCK.lock().await;
    let paths = CachePaths::current();
    if let Some(reason) = check_active_barriers(Some(&app), &paths).await {
        return Err(format!("Cannot clear cache while active operation is running ({})", reason));
    }

    let paths_clone = paths.clone();
    tokio::task::spawn_blocking(move || clean_dawnland_cache_smart(&paths_clone, 0, true))
        .await
        .map_err(|e| format!("Dawnland cache cleanup join error: {}", e))??;

    tracing::info!("Manually cleared unreferenced dawnland cache");
    Ok(())
}

/// Legacy command returning total freed bytes for existing callers.
#[tauri::command]
pub async fn clean_global_mod_cache(app: AppHandle) -> Result<u64, String> {
    let report = clean_unreferenced_cache(app).await?;
    Ok(report.freed_bytes)
}

/// Run an immediate user-initiated cache GC pass across Tier 1 (and Tier 2 if opted in).
#[tauri::command]
pub async fn clean_unreferenced_cache(app: AppHandle) -> Result<CacheGcReport, String> {
    let _guard = GC_LOCK.lock().await;
    let settings = load_launcher_settings().await.unwrap_or_default();
    let paths = CachePaths::current();
    let report = run_gc_with_barriers(Some(&app), &paths, GcMode::Manual, &settings).await;

    if report.skipped_reason.is_none() {
        let _ = app.emit("cache-gc-completed", &report);
    }

    Ok(report)
}

/// Barrier 1: verify no active tasks, active downloads, drop-install locks, or busy instances exist.
async fn check_active_barriers(app: Option<&AppHandle>, paths: &CachePaths) -> Option<String> {
    if let Some(handle) = app {
        if let Some(tm) = handle.try_state::<crate::core::task::TaskManager>() {
            if tm.has_active_tasks().await {
                return Some("active_task".to_string());
            }
        }
    }

    if crate::downloader::download::has_active_downloads()
        || crate::core::drop_install::has_busy_instance_locks()
    {
        return Some("active_task".to_string());
    }

    let paths_clone = paths.clone();
    let has_busy_instance = tokio::task::spawn_blocking(move || has_busy_instance_configs(&paths_clone))
        .await
        .unwrap_or(true);

    if has_busy_instance {
        Some("active_task".to_string())
    } else {
        None
    }
}

fn has_busy_instance_configs(paths: &CachePaths) -> bool {
    for root in [paths.versions_dir(), paths.version_cache_dir()] {
        if !root.exists() {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&root) else {
            return true;
        };
        for entry in entries {
            let Ok(entry) = entry else {
                return true;
            };
            let dir = entry.path();
            if !dir.is_dir() {
                continue;
            }
            let config_path = dir.join("dlml.json");
            if !config_path.exists() {
                continue;
            }
            let Ok(content) = std::fs::read_to_string(&config_path) else {
                return true;
            };
            let Ok(config) = serde_json::from_str::<InstanceConfig>(&content) else {
                return true;
            };
            if config.status.is_busy() {
                return true;
            }
        }
    }
    false
}

async fn run_gc_with_barriers(
    app: Option<&AppHandle>,
    paths: &CachePaths,
    mode: GcMode,
    settings: &LauncherSettings,
) -> CacheGcReport {
    if let Some(reason) = check_active_barriers(app, paths).await {
        return CacheGcReport {
            skipped_reason: Some(reason),
            ..Default::default()
        };
    }

    let paths_clone = paths.clone();
    let settings_clone = settings.clone();
    match tokio::task::spawn_blocking(move || execute_gc_blocking(&paths_clone, mode, &settings_clone))
        .await
    {
        Ok(report) => report,
        Err(err) => {
            tracing::error!("Cache GC blocking task panicked or cancelled: {}", err);
            CacheGcReport {
                skipped_reason: Some("internal_error".to_string()),
                ..Default::default()
            }
        }
    }
}

pub(crate) fn execute_gc_blocking(
    paths: &CachePaths,
    mode: GcMode,
    settings: &LauncherSettings,
) -> CacheGcReport {
    let mut report = CacheGcReport::default();

    if has_busy_instance_configs(paths) {
        report.skipped_reason = Some("active_task".to_string());
        return report;
    }

    let retention_secs = match mode {
        GcMode::Auto => settings.cache_retention_policy.retention_secs(),
        GcMode::Manual => CacheRetentionPolicy::Immediate.retention_secs(),
    };
    let tmp_grace_secs = match mode {
        GcMode::Auto => retention_secs.max(TMP_ORPHAN_GRACE_SECS),
        GcMode::Manual => 0,
    };

    // Tier 1A: Global CAS pool (.dawnland/pools/objects)
    match clean_pools_objects(paths, retention_secs, tmp_grace_secs) {
        Ok((freed, objects, tmps)) => {
            report.freed_bytes = report.freed_bytes.saturating_add(freed);
            report.removed_pool_objects = objects;
            report.removed_tmp_files = tmps;
        }
        Err(err) => {
            tracing::warn!("Tier 1A pool GC aborted safely: {}", err);
        }
    }

    // Tier 1B: Mod parser SQLite + icon PNG cache (.minecraft/.mod_cache)
    match ModParser::prune_unreferenced(&paths.minecraft_base, retention_secs) {
        Ok(stats) => {
            report.freed_bytes = report.freed_bytes.saturating_add(stats.freed_bytes);
            report.removed_mod_cache_entries = stats.removed_entries;
            report.removed_mod_icons = stats.removed_icons;
        }
        Err(err) => {
            tracing::warn!("Tier 1B mod cache GC aborted safely: {}", err);
        }
    }

    // Tier 1C: Vanilla version cache & staging (.dawnland/cache)
    match clean_dawnland_cache_smart(paths, retention_secs, mode == GcMode::Manual) {
        Ok((freed, removed)) => {
            report.freed_bytes = report.freed_bytes.saturating_add(freed);
            report.removed_version_caches = removed;
        }
        Err(err) => {
            tracing::warn!("Tier 1C version cache GC aborted safely: {}", err);
        }
    }

    // Tier 2 (Opt-in, default OFF): Shared Vanilla/Loader assets & libraries
    if settings.auto_clean_shared_assets {
        match clean_shared_minecraft_assets(paths, retention_secs, tmp_grace_secs) {
            Ok((freed, assets, libraries)) => {
                report.freed_bytes = report.freed_bytes.saturating_add(freed);
                report.removed_assets = assets;
                report.removed_libraries = libraries;
            }
            Err(err) => {
                tracing::warn!("Tier 2 shared assets/libraries GC aborted safely: {}", err);
            }
        }
    }

    if report.freed_bytes > 0 {
        tracing::info!(
            "Cache GC finished ({:?}): freed {} bytes (pools={}, tmp={}, mod_db={}, icons={}, ver_cache={}, assets={}, libs={})",
            mode,
            report.freed_bytes,
            report.removed_pool_objects,
            report.removed_tmp_files,
            report.removed_mod_cache_entries,
            report.removed_mod_icons,
            report.removed_version_caches,
            report.removed_assets,
            report.removed_libraries
        );
    }

    report
}

fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn file_age_secs(metadata: &std::fs::Metadata, now_secs: u64) -> u64 {
    metadata
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| now_secs.saturating_sub(d.as_secs()))
        .unwrap_or(0)
}

fn is_within_retention(metadata: &std::fs::Metadata, now_secs: u64, retention_secs: u64) -> bool {
    retention_secs > 0 && file_age_secs(metadata, now_secs) < retention_secs
}

fn is_safe_relative_path(rel: &str) -> bool {
    let p = Path::new(rel);
    if p.is_absolute() {
        return false;
    }
    p.components().all(|c| !matches!(c, Component::ParentDir | Component::RootDir | Component::Prefix(_)))
}

fn append_disable_suffix(path: &Path) -> Option<PathBuf> {
    path.file_name().map(|name| {
        let mut os = name.to_os_string();
        os.push(".disable");
        path.with_file_name(os)
    })
}

/// Barrier 3 for Tier 1A: inspect every instance's `assets.json` and confirm the file exists
/// (enabled or `.disable`) on disk before treating its SHA-1 as referenced.
fn collect_live_pool_hashes(versions_dir: &Path) -> Result<HashSet<String>, String> {
    let mut live_hashes = HashSet::new();
    if !versions_dir.exists() {
        return Ok(live_hashes);
    }

    let entries = std::fs::read_dir(versions_dir)
        .map_err(|e| format!("Failed to read versions directory: {}", e))?;

    for entry in entries {
        let entry = entry.map_err(|e| format!("Failed to read version entry: {}", e))?;
        let instance_dir = entry.path();
        if !instance_dir.is_dir() {
            continue;
        }
        let assets_path = instance_dir.join("assets.json");
        if !assets_path.exists() {
            continue;
        }
        let content = std::fs::read_to_string(&assets_path)
            .map_err(|e| format!("Failed to read {}: {}", assets_path.display(), e))?;
        if content.trim().is_empty() {
            continue;
        }
        let manifest: AssetManifest = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse {}: {}", assets_path.display(), e))?;

        for (rel_path, record) in manifest.assets {
            let Some(sha1) = record.hash_sha1.as_deref() else {
                continue;
            };
            let hash_lower = sha1.trim().to_ascii_lowercase();
            if hash_lower.is_empty() || !is_safe_relative_path(&rel_path) {
                continue;
            }
            let disk_path = instance_dir.join(&rel_path);
            let exists_enabled = disk_path.exists();
            let exists_disabled = append_disable_suffix(&disk_path).is_some_and(|p| p.exists());
            if exists_enabled || exists_disabled {
                live_hashes.insert(hash_lower);
            }
        }
    }

    Ok(live_hashes)
}

#[cfg(windows)]
fn file_hardlink_count(path: &Path) -> std::io::Result<u64> {
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;

    #[repr(C)]
    struct ByHandleFileInformation {
        dw_file_attributes: u32,
        ft_creation_time: [u32; 2],
        ft_last_access_time: [u32; 2],
        ft_last_write_time: [u32; 2],
        dw_volume_serial_number: u32,
        n_file_size_high: u32,
        n_file_size_low: u32,
        n_number_of_links: u32,
        n_file_index_high: u32,
        n_file_index_low: u32,
    }

    extern "system" {
        fn GetFileInformationByHandle(
            h_file: *mut std::ffi::c_void,
            lp_file_information: *mut ByHandleFileInformation,
        ) -> i32;
    }

    let file = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0x0000_0007)
        .open(path)?;
    let mut info = std::mem::MaybeUninit::<ByHandleFileInformation>::uninit();
    let ok = unsafe { GetFileInformationByHandle(file.as_raw_handle(), info.as_mut_ptr()) };
    if ok == 0 {
        return Err(std::io::Error::last_os_error());
    }
    let info = unsafe { info.assume_init() };
    Ok(u64::from(info.n_number_of_links))
}

#[cfg(unix)]
fn file_hardlink_count(path: &Path) -> std::io::Result<u64> {
    use std::os::unix::fs::MetadataExt;
    Ok(std::fs::metadata(path)?.nlink())
}

#[cfg(not(any(windows, unix)))]
fn file_hardlink_count(_path: &Path) -> std::io::Result<u64> {
    Ok(2)
}

fn clean_pools_objects(
    paths: &CachePaths,
    retention_secs: u64,
    tmp_grace_secs: u64,
) -> Result<(u64, u64, u64), String> {
    let objects_dir = paths.pools_objects_dir();
    if !objects_dir.exists() {
        return Ok((0, 0, 0));
    }

    let versions_dir = paths.versions_dir();
    let live_hashes = collect_live_pool_hashes(&versions_dir)?;
    let now_secs = now_epoch_secs();

    let mut freed_bytes = 0u64;
    let mut removed_objects = 0u64;
    let mut removed_tmps = 0u64;

    let top_entries = std::fs::read_dir(&objects_dir)
        .map_err(|e| format!("Failed to read pools/objects: {}", e))?;

    for top_entry in top_entries.flatten() {
        let top_path = top_entry.path();
        if top_path.is_file() {
            let name = top_entry.file_name().to_string_lossy().to_string();
            if name.ends_with(".tmp") || name.ends_with(".part") {
                if let Ok(meta) = top_entry.metadata() {
                    if !is_within_retention(&meta, now_secs, tmp_grace_secs) {
                        let len = meta.len();
                        if std::fs::remove_file(&top_path).is_ok() {
                            freed_bytes = freed_bytes.saturating_add(len);
                            removed_tmps += 1;
                        }
                    }
                }
            }
            continue;
        }

        if !top_path.is_dir() {
            continue;
        }

        if let Ok(files) = std::fs::read_dir(&top_path) {
            for file_entry in files.flatten() {
                let file_path = file_entry.path();
                if !file_path.is_file() {
                    continue;
                }
                let file_name = file_entry.file_name().to_string_lossy().to_string();
                let is_temp = file_name.ends_with(".tmp") || file_name.ends_with(".part");
                let base_hash = file_name
                    .trim_end_matches(".part")
                    .trim_end_matches(".tmp")
                    .to_ascii_lowercase();

                if !is_temp && live_hashes.contains(&base_hash) {
                    continue;
                }

                // Barrier 2: Non-blocking per-hash download lock prevents racing an active download.
                let Some(_dl_guard) =
                    crate::downloader::download::try_lock_download_for_gc(&base_hash)
                else {
                    continue;
                };

                let Ok(metadata) = std::fs::metadata(&file_path) else {
                    continue;
                };

                let grace = if is_temp { tmp_grace_secs } else { retention_secs };
                if is_within_retention(&metadata, now_secs, grace) {
                    continue;
                }

                if is_temp {
                    let len = metadata.len();
                    if std::fs::remove_file(&file_path).is_ok() {
                        freed_bytes = freed_bytes.saturating_add(len);
                        removed_tmps += 1;
                    }
                    continue;
                }

                // Re-verify live references under the hash lock to eliminate TOCTOU windows.
                if let Ok(rechecked) = collect_live_pool_hashes(&versions_dir) {
                    if rechecked.contains(&base_hash) {
                        continue;
                    }
                } else {
                    continue;
                }

                // Barrier 4: Protect any file with hardlink count > 1 (or unreadable link count).
                match file_hardlink_count(&file_path) {
                    Ok(nlink) if nlink <= 1 => {}
                    _ => continue,
                }

                let len = metadata.len();
                if std::fs::remove_file(&file_path).is_ok() {
                    freed_bytes = freed_bytes.saturating_add(len);
                    removed_objects += 1;
                }
            }
        }

        // Remove prefix directory if completely empty.
        let _ = std::fs::remove_dir(&top_path);
    }

    Ok((freed_bytes, removed_objects, removed_tmps))
}

#[derive(Default)]
struct VersionCacheReferences {
    inherits_targets: HashSet<String>,
    mc_versions: HashSet<String>,
}

fn collect_version_cache_references(paths: &CachePaths) -> Result<VersionCacheReferences, String> {
    let mut refs = VersionCacheReferences::default();
    let versions_dir = paths.versions_dir();
    if !versions_dir.exists() {
        return Ok(refs);
    }

    let entries = std::fs::read_dir(&versions_dir)
        .map_err(|e| format!("Failed to read versions directory: {}", e))?;

    for entry in entries {
        let entry = entry.map_err(|e| format!("Failed to read version dir entry: {}", e))?;
        let inst_dir = entry.path();
        if !inst_dir.is_dir() {
            continue;
        }
        let inst_id = entry.file_name().to_string_lossy().to_string();

        let dlml_path = inst_dir.join("dlml.json");
        if dlml_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&dlml_path) {
                if let Ok(cfg) = serde_json::from_str::<InstanceConfig>(&content) {
                    if let Some(mc_ver) = cfg.mc_version {
                        if !mc_ver.is_empty() {
                            refs.mc_versions.insert(mc_ver);
                        }
                    }
                }
            }
        }

        // Trace inheritance chains up to MAX_INHERITANCE_DEPTH so runtime parents survive GC.
        let mut cursor_id = Some(inst_id);
        let mut visited = HashSet::new();
        for _ in 0..MAX_INHERITANCE_DEPTH {
            let Some(current_id) = cursor_id.take() else {
                break;
            };
            if !visited.insert(current_id.clone()) {
                break;
            }
            let Some(json_val) = read_version_json_for_chain(paths, &current_id)? else {
                continue;
            };
            if let Some(cv) = json_val.get("clientVersion").and_then(|v| v.as_str()) {
                if !cv.is_empty() {
                    refs.mc_versions.insert(cv.to_string());
                }
            }
            if let Some(parent_id) = json_val.get("inheritsFrom").and_then(|v| v.as_str()) {
                if !parent_id.is_empty() {
                    refs.inherits_targets.insert(parent_id.to_string());
                    refs.mc_versions.insert(parent_id.to_string());
                    cursor_id = Some(parent_id.to_string());
                }
            }
        }
    }

    Ok(refs)
}

fn read_version_json_for_chain(
    paths: &CachePaths,
    version_id: &str,
) -> Result<Option<serde_json::Value>, String> {
    let candidates = [
        paths.versions_dir().join(version_id).join(format!("{}.json", version_id)),
        paths.version_cache_dir().join(version_id).join(format!("{}.json", version_id)),
        paths.versions_dir().join(version_id).join(format!("{}.json.bak", version_id)),
    ];
    for candidate in candidates {
        if candidate.exists() {
            let content = std::fs::read_to_string(&candidate)
                .map_err(|e| format!("Failed to read {}: {}", candidate.display(), e))?;
            let val: serde_json::Value = serde_json::from_str(&content)
                .map_err(|e| format!("Failed to parse {}: {}", candidate.display(), e))?;
            return Ok(Some(val));
        }
    }
    Ok(None)
}

fn compute_path_size_sync(path: &Path) -> u64 {
    if path.is_file() {
        return std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    }
    let mut total = 0u64;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            total = total.saturating_add(compute_path_size_sync(&entry.path()));
        }
    }
    total
}

fn clean_dawnland_cache_smart(
    paths: &CachePaths,
    retention_secs: u64,
    force_manual_ttl: bool,
) -> Result<(u64, u64), String> {
    let cache_dir = paths.version_cache_dir();
    if !cache_dir.exists() {
        return Ok((0, 0));
    }

    let refs = collect_version_cache_references(paths)?;
    let now_secs = now_epoch_secs();
    let mut freed_bytes = 0u64;
    let mut removed_count = 0u64;

    let entries = std::fs::read_dir(&cache_dir)
        .map_err(|e| format!("Failed to read dawnland cache dir: {}", e))?;

    for entry in entries.flatten() {
        let entry_path = entry.path();
        let entry_name = entry.file_name().to_string_lossy().to_string();

        if refs.inherits_targets.contains(&entry_name) {
            continue;
        }

        if entry_path.is_dir() {
            let dlml_path = entry_path.join("dlml.json");
            if dlml_path.exists() {
                if let Ok(content) = std::fs::read_to_string(&dlml_path) {
                    if let Ok(cfg) = serde_json::from_str::<InstanceConfig>(&content) {
                        if cfg.status.is_busy() {
                            continue;
                        }
                    }
                }
            }
        }

        let Ok(metadata) = entry.metadata() else {
            continue;
        };

        // Keep speed-cache entries for active flattened instances up to 15 days in Auto mode;
        // completely unreferenced version caches use the configured retention policy.
        let required_age = if force_manual_ttl {
            0
        } else if refs.mc_versions.contains(&entry_name) {
            retention_secs.max(VERSION_CACHE_TTL_SECS)
        } else {
            retention_secs
        };

        if required_age > 0 && file_age_secs(&metadata, now_secs) < required_age {
            continue;
        }

        let size = compute_path_size_sync(&entry_path);
        let remove_res = if entry_path.is_dir() {
            std::fs::remove_dir_all(&entry_path)
        } else {
            std::fs::remove_file(&entry_path)
        };
        if remove_res.is_ok() {
            freed_bytes = freed_bytes.saturating_add(size);
            removed_count += 1;
        }
    }

    Ok((freed_bytes, removed_count))
}

#[derive(Default)]
struct SharedAssetReferences {
    asset_index_ids: HashSet<String>,
    asset_hashes: HashSet<String>,
    log_configs: HashSet<String>,
    library_relpaths: HashSet<String>,
    mc_versions: HashSet<String>,
}

fn normalize_rel_path(rel: &str) -> String {
    let slash = rel.replace('\\', "/");
    let trimmed = slash.trim_start_matches('/');
    #[cfg(windows)]
    {
        trimmed.to_ascii_lowercase()
    }
    #[cfg(not(windows))]
    {
        trimmed.to_string()
    }
}

fn resolve_maven_path_extended(name: &str) -> Option<String> {
    if let Some(at_idx) = name.find('@') {
        let base = &name[..at_idx];
        let ext = &name[at_idx + 1..];
        let std_path = maven_name_to_path(base)?;
        return Some(format!("{}.{}", std_path.trim_end_matches(".jar"), ext));
    }
    maven_name_to_path(name)
}

fn collect_arg_library_paths(args_val: Option<&serde_json::Value>, out: &mut HashSet<String>) {
    let Some(items) = args_val.and_then(|v| v.as_array()) else {
        return;
    };
    let mut strings: Vec<&str> = Vec::new();
    for item in items {
        if let Some(s) = item.as_str() {
            strings.push(s);
        } else if let Some(val) = item.get("value") {
            if let Some(s) = val.as_str() {
                strings.push(s);
            } else if let Some(arr) = val.as_array() {
                for sub in arr {
                    if let Some(s) = sub.as_str() {
                        strings.push(s);
                    }
                }
            }
        }
    }

    const MARKER: &str = "${library_directory}";
    for raw in strings {
        let mut cursor = raw;
        while let Some(pos) = cursor.find(MARKER) {
            let after = &cursor[pos + MARKER.len()..];
            let segment_end = after
                .find(|c: char| c == ';' || c == ':' || c.is_whitespace())
                .unwrap_or(after.len());
            let mut rel = &after[..segment_end];
            if let Some(stripped) = rel.strip_prefix("${classpath_separator}") {
                rel = stripped;
            }
            let rel_clean = rel.trim_start_matches(['/', '\\']);
            if !rel_clean.is_empty() && !rel_clean.contains('$') {
                out.insert(normalize_rel_path(rel_clean));
            }
            cursor = &after[segment_end..];
        }
    }
}

fn collect_library_entries(lib: &Library, os: &str, arch: &str, out: &mut HashSet<String>) {
    if !should_use_library(lib, os, arch) {
        return;
    }

    if let Some(downloads) = &lib.downloads {
        if let Some(artifact) = &downloads.artifact {
            if let Some(path) = &artifact.path {
                out.insert(normalize_rel_path(path));
            }
        }
        if let Some(classifiers) = &downloads.classifiers {
            let mut target_keys: HashSet<&str> = HashSet::new();
            if let Some(natives) = &lib.natives {
                for os_key in match os {
                    "windows" => ["windows", "natives-windows"].as_slice(),
                    "macos" => ["osx", "macos", "osx-arm64", "natives-osx", "natives-macos"].as_slice(),
                    "linux" => ["linux", "linux-x86", "natives-linux"].as_slice(),
                    _ => [].as_slice(),
                } {
                    if let Some(v) = natives.get(*os_key) {
                        target_keys.insert(v.as_str());
                    }
                }
            }
            let default_prefix = match os {
                "windows" => "natives-windows",
                "macos" => "natives-osx",
                "linux" => "natives-linux",
                _ => "",
            };
            for (key, artifact) in classifiers {
                let matches = target_keys.contains(key.as_str())
                    || (!default_prefix.is_empty() && key.starts_with(default_prefix))
                    || (os == "macos" && key.starts_with("natives-macos"));
                if matches {
                    if let Some(path) = &artifact.path {
                        out.insert(normalize_rel_path(path));
                    }
                }
            }
        }
    }

    if let Some(name) = &lib.name {
        if let Some(path) = resolve_maven_path_extended(name) {
            out.insert(normalize_rel_path(&path));
        }
        if let Some(natives) = &lib.natives {
            let base_no_ext = name.split('@').next().unwrap_or(name);
            for os_key in match os {
                "windows" => ["windows", "natives-windows"].as_slice(),
                "macos" => ["osx", "macos", "osx-arm64", "natives-osx", "natives-macos"].as_slice(),
                "linux" => ["linux", "linux-x86", "natives-linux"].as_slice(),
                _ => [].as_slice(),
            } {
                if let Some(classifier) = natives.get(*os_key) {
                    let expanded = classifier.replace("${arch}", if arch == "x86" { "32" } else { "64" });
                    let coord = format!("{}:{}", base_no_ext, expanded);
                    if let Some(path) = resolve_maven_path_extended(&coord) {
                        out.insert(normalize_rel_path(&path));
                    }
                }
            }
        }
    }
}

fn collect_shared_asset_references(paths: &CachePaths) -> Result<SharedAssetReferences, String> {
    let mut refs = SharedAssetReferences::default();
    let versions_dir = paths.versions_dir();
    if !versions_dir.exists() {
        return Ok(refs);
    }

    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;

    let version_entries = std::fs::read_dir(&versions_dir)
        .map_err(|e| format!("Failed to read versions directory: {}", e))?;

    for entry in version_entries {
        let entry = entry.map_err(|e| format!("Failed to inspect version dir: {}", e))?;
        let inst_dir = entry.path();
        if !inst_dir.is_dir() {
            continue;
        }
        let inst_id = entry.file_name().to_string_lossy().to_string();

        // Always treat any subdirectory inside versions/ as potentially valid;
        // fail-safe abort Tier 2 if its primary version JSON is missing or corrupted.
        let primary_json_path = inst_dir.join(format!("{}.json", inst_id));
        if !primary_json_path.exists() {
            return Err(format!(
                "Missing primary version JSON {} in versions/{}; aborting shared GC for safety",
                primary_json_path.display(),
                inst_id
            ));
        }

        if let Ok(cfg_str) = std::fs::read_to_string(inst_dir.join("dlml.json")) {
            if let Ok(cfg) = serde_json::from_str::<InstanceConfig>(&cfg_str) {
                if let Some(mc_ver) = cfg.mc_version {
                    if !mc_ver.is_empty() {
                        refs.mc_versions.insert(mc_ver);
                    }
                }
            }
        }

        let mut cursor = Some(inst_id.clone());
        let mut visited = HashSet::new();
        for depth in 0..MAX_INHERITANCE_DEPTH {
            let Some(current_id) = cursor.take() else {
                break;
            };
            if !visited.insert(current_id.clone()) {
                break;
            }
            let Some(raw_val) = read_version_json_for_chain(paths, &current_id)? else {
                return Err(format!(
                    "Unresolved inheritance parent '{}' while scanning '{}'",
                    current_id, inst_id
                ));
            };
            let meta: VersionMeta = serde_json::from_value(raw_val.clone()).map_err(|e| {
                format!("Failed to parse VersionMeta for '{}': {}", current_id, e)
            })?;

            if let Some(idx_id) = meta.asset_index.as_ref().map(|a| a.id.trim()).filter(|s| !s.is_empty()) {
                refs.asset_index_ids.insert(idx_id.to_string());
            }
            if let Some(assets_name) = meta.assets.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                refs.asset_index_ids.insert(assets_name.to_string());
            }

            if let Some(log_id) = raw_val
                .pointer("/logging/client/file/id")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
            {
                refs.log_configs.insert(log_id.to_string());
            }

            if let Some(cv) = raw_val
                .get("clientVersion")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
            {
                refs.mc_versions.insert(cv.to_string());
            }

            if let Some(libs) = &meta.libraries {
                for lib in libs {
                    collect_library_entries(lib, os, arch, &mut refs.library_relpaths);
                }
            }

            if let Some(args) = &meta.arguments {
                collect_arg_library_paths(args.jvm.as_ref(), &mut refs.library_relpaths);
                collect_arg_library_paths(args.game.as_ref(), &mut refs.library_relpaths);
            }

            if let Some(parent_id) = meta.inherits_from.as_deref().filter(|s| !s.is_empty()) {
                refs.mc_versions.insert(parent_id.to_string());
                if depth + 1 == MAX_INHERITANCE_DEPTH {
                    return Err(format!(
                        "Inheritance chain exceeded max depth for '{}'",
                        inst_id
                    ));
                }
                cursor = Some(parent_id.to_string());
            } else {
                // Leaf profile (either vanilla instance or pre-flattened).
                refs.mc_versions.insert(current_id);
            }
        }
    }

    // Resolve asset object hashes for every referenced asset index; abort if any index is missing/corrupt.
    let indexes_dir = paths.assets_dir().join("indexes");
    for index_id in &refs.asset_index_ids {
        let index_path = indexes_dir.join(format!("{}.json", index_id));
        if !index_path.exists() {
            return Err(format!(
                "Referenced asset index {} is missing; aborting shared assets GC",
                index_path.display()
            ));
        }
        let content = std::fs::read_to_string(&index_path)
            .map_err(|e| format!("Failed to read {}: {}", index_path.display(), e))?;
        let parsed: AssetIndexMeta = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse {}: {}", index_path.display(), e))?;
        if let Some(objects) = parsed.objects {
            for obj in objects.values() {
                if let Some(hash) = obj.hash.as_deref() {
                    if !hash.is_empty() {
                        refs.asset_hashes.insert(hash.to_ascii_lowercase());
                    }
                }
            }
        }
    }

    Ok(refs)
}

fn is_protected_fml_processor_lib(rel_norm: &str, mc_versions: &HashSet<String>) -> bool {
    const PREFIXES: [&str; 3] = [
        "net/minecraft/client/",
        "de/oceanlabs/mcp/mcp_config/",
        "net/neoforged/neoform/",
    ];
    for prefix in PREFIXES {
        if let Some(rest) = rel_norm.strip_prefix(prefix) {
            if let Some(version_segment) = rest.split('/').next() {
                let is_match = mc_versions.iter().any(|mc| {
                    let mc_norm = normalize_rel_path(mc);
                    version_segment == mc_norm
                        || version_segment.starts_with(&format!("{}-", mc_norm))
                });
                if is_match {
                    return true;
                }
            }
        }
    }
    false
}

fn clean_shared_minecraft_assets(
    paths: &CachePaths,
    retention_secs: u64,
    tmp_grace_secs: u64,
) -> Result<(u64, u64, u64), String> {
    let refs = collect_shared_asset_references(paths)?;
    let now_secs = now_epoch_secs();

    let mut freed_bytes = 0u64;
    let mut removed_assets = 0u64;
    let mut removed_libraries = 0u64;

    let assets_dir = paths.assets_dir();
    if assets_dir.exists() {
        // 1. assets/objects/<prefix>/<hash>
        let objects_dir = assets_dir.join("objects");
        if objects_dir.exists() {
            if let Ok(subdirs) = std::fs::read_dir(&objects_dir) {
                for sub_entry in subdirs.flatten() {
                    let sub_path = sub_entry.path();
                    if sub_path.is_file() {
                        let name = sub_entry.file_name().to_string_lossy().to_string();
                        if name.ends_with(".tmp") {
                            if let Ok(meta) = sub_entry.metadata() {
                                if !is_within_retention(&meta, now_secs, tmp_grace_secs) {
                                    let len = meta.len();
                                    if std::fs::remove_file(&sub_path).is_ok() {
                                        freed_bytes = freed_bytes.saturating_add(len);
                                        removed_assets += 1;
                                    }
                                }
                            }
                        }
                        continue;
                    }
                    if !sub_path.is_dir() {
                        continue;
                    }
                    if let Ok(files) = std::fs::read_dir(&sub_path) {
                        for file_entry in files.flatten() {
                            let file_path = file_entry.path();
                            if !file_path.is_file() {
                                continue;
                            }
                            let name = file_entry.file_name().to_string_lossy().to_string();
                            let is_temp = name.ends_with(".tmp") || name.ends_with(".part");
                            let hash_lower = name.to_ascii_lowercase();
                            if !is_temp && refs.asset_hashes.contains(&hash_lower) {
                                continue;
                            }
                            let Ok(meta) = file_entry.metadata() else {
                                continue;
                            };
                            let grace = if is_temp { tmp_grace_secs } else { retention_secs };
                            if is_within_retention(&meta, now_secs, grace) {
                                continue;
                            }
                            let len = meta.len();
                            if std::fs::remove_file(&file_path).is_ok() {
                                freed_bytes = freed_bytes.saturating_add(len);
                                removed_assets += 1;
                            }
                        }
                    }
                    let _ = std::fs::remove_dir(&sub_path);
                }
            }
        }

        // 2. assets/indexes/<id>.json
        let indexes_dir = assets_dir.join("indexes");
        if indexes_dir.exists() {
            if let Ok(entries) = std::fs::read_dir(&indexes_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if !path.is_file() {
                        continue;
                    }
                    let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                        continue;
                    };
                    if path.extension().and_then(|e| e.to_str()) != Some("json") {
                        continue;
                    }
                    if refs.asset_index_ids.contains(stem) {
                        continue;
                    }
                    if let Ok(meta) = entry.metadata() {
                        if is_within_retention(&meta, now_secs, retention_secs) {
                            continue;
                        }
                        let len = meta.len();
                        if std::fs::remove_file(&path).is_ok() {
                            freed_bytes = freed_bytes.saturating_add(len);
                            removed_assets += 1;
                        }
                    }
                }
            }
        }

        // 3. assets/log_configs/<id>
        let log_configs_dir = assets_dir.join("log_configs");
        if log_configs_dir.exists() {
            if let Ok(entries) = std::fs::read_dir(&log_configs_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if !path.is_file() {
                        continue;
                    }
                    let name = entry.file_name().to_string_lossy().to_string();
                    if refs.log_configs.contains(&name) {
                        continue;
                    }
                    if let Ok(meta) = entry.metadata() {
                        if is_within_retention(&meta, now_secs, retention_secs) {
                            continue;
                        }
                        let len = meta.len();
                        if std::fs::remove_file(&path).is_ok() {
                            freed_bytes = freed_bytes.saturating_add(len);
                            removed_assets += 1;
                        }
                    }
                }
            }
        }
    }

    // 4. libraries/**
    let libs_dir = paths.libraries_dir();
    if libs_dir.exists() {
        clean_libraries_recursive(
            &libs_dir,
            &libs_dir,
            &refs,
            now_secs,
            retention_secs,
            tmp_grace_secs,
            &mut freed_bytes,
            &mut removed_libraries,
        );
    }

    Ok((freed_bytes, removed_assets, removed_libraries))
}

#[allow(clippy::too_many_arguments)]
fn clean_libraries_recursive(
    root: &Path,
    current: &Path,
    refs: &SharedAssetReferences,
    now_secs: u64,
    retention_secs: u64,
    tmp_grace_secs: u64,
    freed_bytes: &mut u64,
    removed_libraries: &mut u64,
) {
    let Ok(entries) = std::fs::read_dir(current) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            clean_libraries_recursive(
                root,
                &path,
                refs,
                now_secs,
                retention_secs,
                tmp_grace_secs,
                freed_bytes,
                removed_libraries,
            );
            let _ = std::fs::remove_dir(&path);
            continue;
        }
        if !path.is_file() {
            continue;
        }
        let Ok(rel) = path.strip_prefix(root) else {
            continue;
        };
        let rel_norm = normalize_rel_path(&rel.to_string_lossy());
        let name = entry.file_name().to_string_lossy().to_string();
        let is_temp = name.ends_with(".tmp") || name.ends_with(".part");

        if !is_temp
            && (refs.library_relpaths.contains(&rel_norm)
                || is_protected_fml_processor_lib(&rel_norm, &refs.mc_versions))
        {
            continue;
        }

        let Ok(meta) = entry.metadata() else {
            continue;
        };
        let grace = if is_temp { tmp_grace_secs } else { retention_secs };
        if is_within_retention(&meta, now_secs, grace) {
            continue;
        }

        let len = meta.len();
        if std::fs::remove_file(&path).is_ok() {
            *freed_bytes = freed_bytes.saturating_add(len);
            *removed_libraries += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::launcher::InstanceStatus;
    use crate::models::instance::{AssetCategory, AssetRecord, AssetSourceType};
    use tempfile::tempdir;

    fn build_test_paths(root: &Path) -> CachePaths {
        CachePaths {
            dawnland_dir: root.join(".dawnland"),
            minecraft_base: root.join(".minecraft"),
        }
    }

    fn make_record(sha1: &str) -> AssetRecord {
        AssetRecord {
            project_id: None,
            version_id: None,
            source_type: AssetSourceType::LocalImport,
            download_url: None,
            hash_sha1: Some(sha1.to_string()),
            hash_sha512: None,
            size: None,
            enabled: true,
            category: AssetCategory::Mod,
            managed_by_modpack: false,
            display_name_zh: None,
            original_filename: None,
            original_rel_path: None,
        }
    }

    fn write_manifest(inst_dir: &Path, entries: Vec<(&str, AssetRecord)>) {
        let mut manifest = AssetManifest::default();
        for (rel, rec) in entries {
            manifest.assets.insert(rel.to_string(), rec);
        }
        std::fs::write(
            inst_dir.join("assets.json"),
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();
    }

    fn write_dlml_config(inst_dir: &Path, status: InstanceStatus, mc_version: Option<&str>) {
        let cfg = InstanceConfig {
            status,
            mc_version: mc_version.map(str::to_string),
            ..Default::default()
        };
        std::fs::write(
            inst_dir.join("dlml.json"),
            serde_json::to_string_pretty(&cfg).unwrap(),
        )
        .unwrap();
    }

    fn backdate_path(path: &Path, age_secs: u64) {
        let target_time = SystemTime::now() - Duration::from_secs(age_secs);
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            if path.is_dir() {
                // FILE_FLAG_BACKUP_SEMANTICS (0x02000000) is required on Windows to open a directory handle.
                opts.custom_flags(0x0200_0000);
            }
        }
        let file = opts.open(path).unwrap();
        file.set_modified(target_time).unwrap();
    }

    #[test]
    fn test_tier1_pool_barriers_preserve_valid_links_and_clean_orphans_and_ghosts() {
        let dir = tempdir().unwrap();
        let paths = build_test_paths(dir.path());

        let inst_dir = paths.versions_dir().join("inst-a");
        let mods_dir = inst_dir.join("mods");
        std::fs::create_dir_all(&mods_dir).unwrap();
        write_dlml_config(&inst_dir, InstanceStatus::Ready, Some("1.20.1"));

        let h_enabled = "aa11111111111111111111111111111111111111";
        let h_disabled = "bb22222222222222222222222222222222222222";
        let h_crossdisk = "cc33333333333333333333333333333333333333";
        let h_ghost = "dd44444444444444444444444444444444444444";
        let h_orphan = "ee55555555555555555555555555555555555555";
        let h_ext_hardlink = "ff66666666666666666666666666666666666666";

        for (hash, data) in [
            (h_enabled, b"enabled-mod".as_slice()),
            (h_disabled, b"disabled-mod".as_slice()),
            (h_crossdisk, b"crossdisk-mod".as_slice()),
            (h_ghost, b"ghost-mod-bytes".as_slice()),
            (h_orphan, b"orphan-mod-bytes".as_slice()),
            (h_ext_hardlink, b"ext-hardlink".as_slice()),
        ] {
            let obj_dir = paths.pools_objects_dir().join(&hash[0..2]);
            std::fs::create_dir_all(&obj_dir).unwrap();
            std::fs::write(obj_dir.join(hash), data).unwrap();
        }

        // Hardlink enabled and .disable mods; simulate cross-disk copy (nlink == 1); leave ghost absent on disk.
        std::fs::hard_link(
            paths.pools_objects_dir().join(&h_enabled[0..2]).join(h_enabled),
            mods_dir.join("enabled.jar"),
        )
        .unwrap();
        std::fs::hard_link(
            paths.pools_objects_dir().join(&h_disabled[0..2]).join(h_disabled),
            mods_dir.join("disabled.jar.disable"),
        )
        .unwrap();
        std::fs::write(mods_dir.join("crossdisk.jar"), b"crossdisk-mod").unwrap();

        // Simulate third-party external hardlink not tracked in assets.json -> protected by Barrier 4!
        let external_file = dir.path().join("external_untracked.jar");
        std::fs::hard_link(
            paths.pools_objects_dir().join(&h_ext_hardlink[0..2]).join(h_ext_hardlink),
            &external_file,
        )
        .unwrap();

        write_manifest(
            &inst_dir,
            vec![
                ("mods/enabled.jar", make_record(h_enabled)),
                ("mods/disabled.jar", make_record(h_disabled)),
                ("mods/crossdisk.jar", make_record(h_crossdisk)),
                ("mods/ghost_deleted_externally.jar", make_record(h_ghost)),
            ],
        );

        let settings = LauncherSettings::default(); // Auto + Immediate retention + shared OFF
        let report = execute_gc_blocking(&paths, GcMode::Auto, &settings);

        assert_eq!(report.skipped_reason, None);
        assert_eq!(report.removed_pool_objects, 2); // h_ghost + h_orphan
        assert_eq!(
            report.freed_bytes,
            (b"ghost-mod-bytes".len() + b"orphan-mod-bytes".len()) as u64
        );

        assert!(paths.pools_objects_dir().join(&h_enabled[0..2]).join(h_enabled).exists());
        assert!(paths.pools_objects_dir().join(&h_disabled[0..2]).join(h_disabled).exists());
        assert!(paths.pools_objects_dir().join(&h_crossdisk[0..2]).join(h_crossdisk).exists());
        assert!(paths.pools_objects_dir().join(&h_ext_hardlink[0..2]).join(h_ext_hardlink).exists());
        assert!(!paths.pools_objects_dir().join(&h_ghost[0..2]).join(h_ghost).exists());
        assert!(!paths.pools_objects_dir().join(&h_orphan[0..2]).join(h_orphan).exists());
    }

    #[test]
    fn test_corrupt_assets_json_aborts_pool_gc_safely() {
        let dir = tempdir().unwrap();
        let paths = build_test_paths(dir.path());
        let inst_dir = paths.versions_dir().join("inst-corrupt");
        std::fs::create_dir_all(&inst_dir).unwrap();
        std::fs::write(inst_dir.join("assets.json"), b"{invalid json").unwrap();

        let h_orphan = "aa99999999999999999999999999999999999999";
        let obj_dir = paths.pools_objects_dir().join("aa");
        std::fs::create_dir_all(&obj_dir).unwrap();
        std::fs::write(obj_dir.join(h_orphan), b"must-not-delete").unwrap();

        let report = execute_gc_blocking(&paths, GcMode::Manual, &LauncherSettings::default());
        assert_eq!(report.removed_pool_objects, 0);
        assert!(obj_dir.join(h_orphan).exists());
    }

    #[test]
    fn test_busy_instance_barrier_and_retention_policies() {
        let dir = tempdir().unwrap();
        let paths = build_test_paths(dir.path());
        let inst_dir = paths.versions_dir().join("inst-busy");
        std::fs::create_dir_all(&inst_dir).unwrap();
        write_dlml_config(&inst_dir, InstanceStatus::Installing, Some("1.20.1"));

        let h_orphan = "1122334455667788990011223344556677889900";
        let obj_dir = paths.pools_objects_dir().join("11");
        std::fs::create_dir_all(&obj_dir).unwrap();
        let obj_path = obj_dir.join(h_orphan);
        let tmp_path = obj_dir.join(format!("{}.tmp", h_orphan));
        std::fs::write(&obj_path, b"orphan").unwrap();
        std::fs::write(&tmp_path, b"partial").unwrap();

        // Busy instance aborts GC regardless of Manual/Auto.
        let busy_report = execute_gc_blocking(&paths, GcMode::Manual, &LauncherSettings::default());
        assert_eq!(busy_report.skipped_reason.as_deref(), Some("active_task"));
        assert!(obj_path.exists());

        // Set instance Ready and switch policy to 24h retention -> fresh files stay.
        write_dlml_config(&inst_dir, InstanceStatus::Ready, Some("1.20.1"));
        let settings_24h = LauncherSettings {
            cache_retention_policy: CacheRetentionPolicy::Hours24,
            ..Default::default()
        };

        let kept_report = execute_gc_blocking(&paths, GcMode::Auto, &settings_24h);
        assert_eq!(kept_report.skipped_reason, None);
        assert_eq!(kept_report.removed_pool_objects, 0);
        assert!(obj_path.exists());
        assert!(tmp_path.exists());

        // Immediate retention in Auto mode deletes completed orphan immediately, preserves <1h .tmp.
        let immediate_auto = execute_gc_blocking(&paths, GcMode::Auto, &LauncherSettings::default());
        assert_eq!(immediate_auto.removed_pool_objects, 1);
        assert_eq!(immediate_auto.removed_tmp_files, 0);
        assert!(!obj_path.exists());
        assert!(tmp_path.exists());

        // Backdate .tmp > 1h -> Auto cleans .tmp too.
        backdate_path(&tmp_path, TMP_ORPHAN_GRACE_SECS + 10);
        let tmp_cleaned = execute_gc_blocking(&paths, GcMode::Auto, &LauncherSettings::default());
        assert_eq!(tmp_cleaned.removed_tmp_files, 1);
        assert!(!tmp_path.exists());
    }

    #[tokio::test]
    async fn test_barrier2_download_lock_skips_locked_pool_object() {
        let dir = tempdir().unwrap();
        let paths = build_test_paths(dir.path());
        let h_locked = "ab12121212121212121212121212121212121212";
        let obj_dir = paths.pools_objects_dir().join("ab");
        std::fs::create_dir_all(&obj_dir).unwrap();
        let obj_path = obj_dir.join(h_locked);
        std::fs::write(&obj_path, b"downloading-bytes").unwrap();

        let dl_mutex = crate::downloader::download::get_download_lock(h_locked).await;
        let dl_guard = dl_mutex.lock().await;

        let skipped = execute_gc_blocking(&paths, GcMode::Manual, &LauncherSettings::default());
        assert_eq!(skipped.removed_pool_objects, 0);
        assert!(obj_path.exists());

        drop(dl_guard);
        let cleaned = execute_gc_blocking(&paths, GcMode::Manual, &LauncherSettings::default());
        assert_eq!(cleaned.removed_pool_objects, 1);
        assert!(!obj_path.exists());
    }

    #[test]
    fn test_tier1c_dawnland_cache_preserves_inherits_from_over_15_days() {
        let dir = tempdir().unwrap();
        let paths = build_test_paths(dir.path());

        let inst_dir = paths.versions_dir().join("fabric-inst");
        std::fs::create_dir_all(&inst_dir).unwrap();
        write_dlml_config(&inst_dir, InstanceStatus::Ready, Some("1.20.1"));
        std::fs::write(
            inst_dir.join("fabric-inst.json"),
            r#"{"id":"fabric-inst","inheritsFrom":"1.20.1"}"#,
        )
        .unwrap();

        let flat_inst = paths.versions_dir().join("vanilla-flat");
        std::fs::create_dir_all(&flat_inst).unwrap();
        write_dlml_config(&flat_inst, InstanceStatus::Ready, Some("1.18.2"));
        std::fs::write(flat_inst.join("vanilla-flat.json"), r#"{"id":"vanilla-flat"}"#).unwrap();

        let flat_old = paths.versions_dir().join("vanilla-old");
        std::fs::create_dir_all(&flat_old).unwrap();
        write_dlml_config(&flat_old, InstanceStatus::Ready, Some("1.17.1"));
        std::fs::write(flat_old.join("vanilla-old.json"), r#"{"id":"vanilla-old"}"#).unwrap();

        let inherited_cache = paths.version_cache_dir().join("1.20.1");
        let fresh_speed_cache = paths.version_cache_dir().join("1.18.2");
        let expired_speed_cache = paths.version_cache_dir().join("1.17.1");
        let orphan_cache = paths.version_cache_dir().join("1.19.4");

        for (d, id) in [
            (&inherited_cache, "1.20.1"),
            (&fresh_speed_cache, "1.18.2"),
            (&expired_speed_cache, "1.17.1"),
            (&orphan_cache, "1.19.4"),
        ] {
            std::fs::create_dir_all(d).unwrap();
            std::fs::write(d.join(format!("{}.json", id)), format!(r#"{{"id":"{}"}}"#, id)).unwrap();
        }

        let inherited_json = inherited_cache.join("1.20.1.json");
        backdate_path(&inherited_cache, 30 * 24 * 3600);
        backdate_path(&expired_speed_cache, 20 * 24 * 3600);

        let report = execute_gc_blocking(&paths, GcMode::Auto, &LauncherSettings::default());
        // orphan_cache (1.19.4, unreferenced) + expired_speed_cache (1.17.1, >15d flattened speed cache)
        assert_eq!(report.removed_version_caches, 2);
        assert!(inherited_json.exists());
        assert!(fresh_speed_cache.exists());
        assert!(!expired_speed_cache.exists());
        assert!(!orphan_cache.exists());
    }

    #[test]
    fn test_tier2_shared_assets_and_libraries_opt_in_and_forge_neoforge_protection() {
        let dir = tempdir().unwrap();
        let paths = build_test_paths(dir.path());

        // Instance: Forge/NeoForge-style profile inheriting 1.21.1 + library_directory JVM arg
        let inst_dir = paths.versions_dir().join("neoforge-1.21.1");
        std::fs::create_dir_all(&inst_dir).unwrap();
        write_dlml_config(&inst_dir, InstanceStatus::Ready, Some("1.21.1"));

        let child_json = serde_json::json!({
            "id": "neoforge-1.21.1",
            "inheritsFrom": "1.21.1",
            "libraries": [
                {
                    "downloads": {
                        "artifact": { "path": "net/neoforged/fancymodloader/loader/4.0.0/loader-4.0.0.jar" }
                    }
                },
                {
                    "name": "com.example:winonly:1.0",
                    "rules": [{ "action": "allow", "os": { "name": "windows" } }]
                },
                {
                    "name": "com.example:linuxonly:1.0",
                    "rules": [{ "action": "allow", "os": { "name": "linux" } }]
                }
            ],
            "arguments": {
                "jvm": ["-p", "${library_directory}/cpw/mods/bootstraplauncher/2.0.2/bootstraplauncher-2.0.2.jar"]
            }
        });
        std::fs::write(
            inst_dir.join("neoforge-1.21.1.json"),
            serde_json::to_string_pretty(&child_json).unwrap(),
        )
        .unwrap();

        let parent_dir = paths.version_cache_dir().join("1.21.1");
        std::fs::create_dir_all(&parent_dir).unwrap();
        let parent_json = serde_json::json!({
            "id": "1.21.1",
            "assetIndex": { "id": "17" },
            "assets": "17",
            "logging": { "client": { "file": { "id": "client-1.12.xml" } } },
            "libraries": [
                { "downloads": { "artifact": { "path": "com/mojang/brigadier/1.2.9/brigadier-1.2.9.jar" } } }
            ]
        });
        std::fs::write(
            parent_dir.join("1.21.1.json"),
            serde_json::to_string_pretty(&parent_json).unwrap(),
        )
        .unwrap();

        // Assets setup: 17.json (active) + 5.json (orphan)
        let indexes_dir = paths.assets_dir().join("indexes");
        std::fs::create_dir_all(&indexes_dir).unwrap();
        std::fs::write(
            indexes_dir.join("17.json"),
            r#"{"objects":{"minecraft/sounds/click.ogg":{"hash":"aa10101010101010101010101010101010101010","size":5}}}"#,
        )
        .unwrap();
        std::fs::write(indexes_dir.join("5.json"), r#"{"objects":{}}"#).unwrap();

        let obj_alive = paths.assets_dir().join("objects").join("aa").join("aa10101010101010101010101010101010101010");
        let obj_orphan = paths.assets_dir().join("objects").join("bb").join("bb20202020202020202020202020202020202020");
        std::fs::create_dir_all(obj_alive.parent().unwrap()).unwrap();
        std::fs::create_dir_all(obj_orphan.parent().unwrap()).unwrap();
        std::fs::write(&obj_alive, b"sound").unwrap();
        std::fs::write(&obj_orphan, b"dead-sound").unwrap();

        let log_alive = paths.assets_dir().join("log_configs").join("client-1.12.xml");
        let log_orphan = paths.assets_dir().join("log_configs").join("client-1.7.xml");
        std::fs::create_dir_all(log_alive.parent().unwrap()).unwrap();
        std::fs::write(&log_alive, b"<Configuration/>").unwrap();
        std::fs::write(&log_orphan, b"<Old/>").unwrap();

        // Libraries setup: standard, module-path arg, Forge/NeoForge patched + neoform, plus orphans
        let lib_paths = [
            ("net/neoforged/fancymodloader/loader/4.0.0/loader-4.0.0.jar", true),
            ("cpw/mods/bootstraplauncher/2.0.2/bootstraplauncher-2.0.2.jar", true),
            ("com/mojang/brigadier/1.2.9/brigadier-1.2.9.jar", true),
            ("net/minecraft/client/1.21.1-20240613/client-1.21.1-20240613-srg.jar", true),
            ("net/neoforged/neoform/1.21.1-20240613/neoform-1.21.1-20240613.zip", true),
            ("net/minecraft/client/1.19.2-patched/client-1.19.2-patched.jar", false),
            ("com/unused/orphan/1.0/orphan-1.0.jar", false),
        ];
        for (rel, _) in &lib_paths {
            let p = paths.libraries_dir().join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, b"jar").unwrap();
        }

        // First: default settings (autoCleanSharedAssets = false) must NOT touch Tier 2.
        let off_report = execute_gc_blocking(&paths, GcMode::Auto, &LauncherSettings::default());
        assert_eq!(off_report.removed_assets, 0);
        assert_eq!(off_report.removed_libraries, 0);
        assert!(obj_orphan.exists());

        // Now enable autoCleanSharedAssets = true.
        let on_settings = LauncherSettings {
            auto_clean_shared_assets: true,
            ..Default::default()
        };
        let on_report = execute_gc_blocking(&paths, GcMode::Auto, &on_settings);
        assert_eq!(on_report.removed_assets, 3); // obj_orphan + 5.json + client-1.7.xml
        assert_eq!(on_report.removed_libraries, 2); // 1.19.2-patched + orphan-1.0

        assert!(obj_alive.exists());
        assert!(indexes_dir.join("17.json").exists());
        assert!(log_alive.exists());
        assert!(!obj_orphan.exists());
        assert!(!indexes_dir.join("5.json").exists());
        assert!(!log_orphan.exists());

        for (rel, should_keep) in &lib_paths {
            let exists = paths.libraries_dir().join(rel).exists();
            assert_eq!(exists, *should_keep, "unexpected state for library {}", rel);
        }

        // Corrupt 17.json -> fail-safe aborts Tier 2 cleanly.
        std::fs::write(indexes_dir.join("17.json"), b"{corrupt").unwrap();
        let late_orphan = paths.libraries_dir().join("com/unused/new/1.0/new-1.0.jar");
        std::fs::create_dir_all(late_orphan.parent().unwrap()).unwrap();
        std::fs::write(&late_orphan, b"keep-on-abort").unwrap();
        let aborted = execute_gc_blocking(&paths, GcMode::Auto, &on_settings);
        assert_eq!(aborted.removed_libraries, 0);
        assert!(late_orphan.exists());
    }
}
