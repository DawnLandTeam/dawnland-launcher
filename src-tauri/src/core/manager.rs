//! Instance Manager - Local Instance Scanning Engine
//! Provides functionality to scan and manage installed game instances.

#![allow(dead_code)]
#![allow(unused_variables)]

use crate::core::mojang::get_minecraft_base;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::LazyLock;
use tokio::sync::Mutex;
use tauri::Manager;

pub static MANIFEST_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

static PATH_LOCKS: LazyLock<std::sync::Mutex<std::collections::HashMap<PathBuf, std::sync::Arc<tokio::sync::Mutex<()>>>>> =
    LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

/// Get an asynchronous lock for a specific filesystem destination to serialize concurrent writes.
pub fn get_path_lock(path: &std::path::Path) -> std::sync::Arc<tokio::sync::Mutex<()>> {
    let mut map = PATH_LOCKS.lock().unwrap_or_else(|e| e.into_inner());
    map.entry(path.to_path_buf())
        .or_insert_with(|| std::sync::Arc::new(tokio::sync::Mutex::new(())))
        .clone()
}

const ALLOWED_PRESET_DOWNLOAD_HOST_SUFFIXES: [&str; 3] = [
    "forgecdn.net",
    "curseforge.com",
    "modrinth.com",
];

/// Validates that a preset download URL is strictly HTTPS and targets approved provider hosts.
pub fn validate_preset_download_url(url: &reqwest::Url) -> Result<(), String> {
    if url.scheme() != "https" {
        return Err(format!("Unsupported download scheme: {}", url.scheme()));
    }
    let Some(raw_host) = url.host_str() else {
        return Err("Download URL is missing a host".to_string());
    };
    let host_unbracketed = raw_host
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(raw_host);

    if let Ok(ip) = host_unbracketed.parse::<std::net::IpAddr>() {
        match ip {
            std::net::IpAddr::V4(v4) if crate::core::instance_icon::is_disallowed_ipv4(v4) => {
                return Err("Download URL points to a disallowed IPv4 address".to_string());
            }
            std::net::IpAddr::V6(v6) if crate::core::instance_icon::is_disallowed_ipv6(v6) => {
                return Err("Download URL points to a disallowed IPv6 address".to_string());
            }
            _ => return Err("Direct IP download URLs are not allowed for presets".to_string()),
        }
    }

    let d = host_unbracketed.trim_end_matches('.').to_ascii_lowercase();
    if d == "localhost"
        || d.ends_with(".localhost")
        || d.ends_with(".local")
        || d.ends_with(".internal")
    {
        return Err("Download URL points to a local domain".to_string());
    }

    let allowed = ALLOWED_PRESET_DOWNLOAD_HOST_SUFFIXES
        .iter()
        .any(|suffix| d == *suffix || d.ends_with(&format!(".{}", suffix)));
    if !allowed {
        return Err(format!("Download host '{}' is not in approved provider list", d));
    }
    Ok(())
}

/// HTTP client configured for preset downloads that validates host/scheme on each redirect.
pub static PRESET_DOWNLOAD_HTTP_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    let policy = reqwest::redirect::Policy::custom(|attempt| {
        if attempt.previous().len() >= 5 {
            return attempt.error("Too many redirects while downloading preset asset");
        }
        if let Err(reason) = validate_preset_download_url(attempt.url()) {
            return attempt.error(reason);
        }
        attempt.follow()
    });
    crate::core::utils::http_client_builder()
        .connect_timeout(std::time::Duration::from_secs(15))
        .redirect(policy)
        .build()
        .unwrap_or_else(|e| {
            tracing::warn!("Failed to build preset download HTTP client: {}", e);
            crate::core::utils::get_http_client().clone()
        })
});

/// Represents a scanned installed instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceItem {
    /// Unique identifier (folder name)
    pub id: String,
    /// Display name (same as id by default)
    pub name: String,
    /// Minecraft version (e.g., "1.20.1")
    pub mc_version: String,
    /// Loader type: "Vanilla" or "Fabric"
    pub loader_type: String,
    /// Loader version (if any, e.g., "0.16.9")
    pub loader_version: Option<String>,
    /// Modpack Version (if any)
    pub modpack_version: Option<String>,
    /// Modpack Type (e.g., "CurseForge" or "Modrinth")
    pub modpack_type: Option<String>,
    pub modpack_project_id: Option<String>,
    /// Server ID this instance is bound to (optional)
    pub server_id: Option<String>,
    /// Modpack Version ID for online modpacks (optional)
    pub pack_version_id: Option<String>,
    /// Modpack File Name for local zips (optional)
    pub pack_file_name: Option<String>,
    /// State machine lifecycle status of the instance
    pub status: crate::models::instance::InstanceStatus,
    /// Absolute path of the cached instance icon (optional)
    pub icon_path: Option<String>,
}

#[tauri::command]
pub async fn get_instance_saves(instance_id: String) -> Result<Vec<String>, String> {
    let base_dir = get_minecraft_base();
    let instance_dir = base_dir.join("versions").join(&instance_id);
    let saves_dir = instance_dir.join("saves");

    if !tokio::fs::try_exists(&saves_dir).await.unwrap_or(false) {
        return Ok(Vec::new());
    }

    let mut saves = Vec::new();
    let mut entries = tokio::fs::read_dir(&saves_dir)
        .await
        .map_err(|e| format!("Failed to read saves directory: {}", e))?;

    loop {
        match entries.next_entry().await {
            Ok(Some(entry)) => {
                let path = entry.path();
                let is_dir = tokio::fs::metadata(&path).await.map(|m| m.is_dir()).unwrap_or(false);
                if is_dir {
                    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                        saves.push(name.to_string());
                    }
                }
            }
            Ok(None) => break,
            Err(e) => {
                return Err(format!("Failed to read entry in saves directory: {}", e));
            }
        }
    }

    Ok(saves)
}

#[tauri::command]
pub async fn get_instance_datapack_dir(instance_id: String, world_name: String) -> Result<String, String> {
    if !crate::core::fs_utils::is_safe_single_name(&instance_id)
        || !crate::core::fs_utils::is_safe_single_name(&world_name)
    {
        return Err("Invalid instance ID or world name".to_string());
    }
    let base_dir = get_minecraft_base();
    let datapack_dir = base_dir
        .join("versions")
        .join(&instance_id)
        .join("saves")
        .join(&world_name)
        .join("datapacks");

    if !tokio::fs::try_exists(&datapack_dir).await.unwrap_or(false) {
        tokio::fs::create_dir_all(&datapack_dir)
            .await
            .map_err(|e| format!("Failed to create datapack directory: {}", e))?;
    }

    Ok(datapack_dir.to_string_lossy().to_string())
}

pub struct InstanceTaskContext<'a> {
    pub starting_instances: Option<&'a std::sync::RwLock<std::collections::HashSet<String>>>,
    pub active_tasks: Option<&'a tokio::sync::RwLock<std::collections::HashMap<String, std::sync::Arc<crate::core::task::manager::TaskWorkerHandle>>>>,
    pub tasks: &'a [crate::core::task::TaskState],
    pub running_instances: Option<&'a std::collections::HashSet<String>>,
}

impl<'a> InstanceTaskContext<'a> {
    pub fn is_starting(&self, id: &str) -> bool {
        self.starting_instances
            .and_then(|s| s.read().ok())
            .map(|set| set.contains(id))
            .unwrap_or(false)
    }

    pub fn is_running(&self, id: &str) -> bool {
        let id_lower = id.to_lowercase();
        self.running_instances
            .map(|set| set.iter().any(|k| k.to_lowercase() == id_lower))
            .unwrap_or(false)
    }

    pub async fn is_busy(&self, id: &str) -> bool {
        if let Some(active) = self.active_tasks {
            let active = active.read().await;
            active.values().any(|h| h.instance_id.as_deref() == Some(id))
        } else {
            false
        }
    }

    pub async fn is_task_worker_active(&self, task_id: &str, instance_id: &str) -> bool {
        if let Some(active) = self.active_tasks {
            let active = active.read().await;
            active.get(task_id).is_some_and(|h| {
                h.instance_id.as_deref() == Some(instance_id)
                    && (h.task_type.is_core_installation() || h.task_type.is_update())
            })
        } else {
            false
        }
    }
}

pub async fn scan_installed_instances_internal(
    versions_dir: &std::path::Path,
    task_ctx: &InstanceTaskContext<'_>,
) -> Result<(Vec<InstanceItem>, Vec<crate::core::instance_icon::IconBackfillCandidate>), String> {
    let mut instances = Vec::new();
    let mut icon_backfill = Vec::new();

    let mut entries = tokio::fs::read_dir(versions_dir)
        .await
        .map_err(|e| format!("Failed to read versions directory: {}", e))?;

    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|e| format!("Failed to read directory entry: {}", e))?
    {
        let path = entry.path();
        if path.is_dir() {
            let id = entry.file_name().to_string_lossy().to_string();
            let json_path = path.join(format!("{}.json", id));
            let config_path = path.join("dlml.json");

            if json_path.exists() || config_path.exists() {
                // Check if instance is hidden via dlml.json
                let mut is_hidden = false;
                let mut status = crate::models::instance::InstanceStatus::Ready;
                let mut server_id = None;
                let mut pack_version_id = None;
                let mut pack_file_name = None;
                let mut icon_name = None;
                let mut config_opt: Option<crate::core::launcher::InstanceConfig> = None;

                if config_path.exists() {
                    if let Ok(content) = tokio::fs::read_to_string(&config_path).await {
                        let has_legacy_fields = crate::core::launcher::InstanceConfig::has_legacy_status_in_json(&content);
                        if let Ok(mut config) =
                            serde_json::from_str::<crate::core::launcher::InstanceConfig>(&content)
                        {
                            if has_legacy_fields {
                                if let Ok(cleaned) = crate::core::launcher::update_instance_config(&path, |_| {}).await {
                                    config = cleaned;
                                }
                            }
                            is_hidden = config.hidden;
                            status = config.effective_status();
                            server_id = config.server_id.clone();
                            pack_version_id = config.pack_version_id.clone();
                            pack_file_name = config.pack_file_name.clone();
                            icon_name = config.icon.clone();
                            config_opt = Some(config);
                        }
                    }
                }

                let icon_path =
                    crate::core::instance_icon::resolve_icon_path(&path, icon_name.as_deref()).await;

                let is_busy = task_ctx.is_busy(&id).await;
                let is_starting = task_ctx.is_starting(&id);
                let mut has_valid_task = is_starting || is_busy;

                for task in task_ctx.tasks {
                    if task.task_type.instance_id().as_deref() == Some(&id) && task.status.is_active() {
                        has_valid_task = true;
                    }
                }

                // Filter core installation and update tasks targeting this instance
                let mut core_tasks: Vec<&crate::core::task::TaskState> = task_ctx
                    .tasks
                    .iter()
                    .filter(|t| {
                        t.task_type.instance_id().as_deref() == Some(&id)
                            && (t.task_type.is_core_installation() || t.task_type.is_update())
                    })
                    .collect();

                // Sort by latest (updated_at, then created_at) descending
                core_tasks.sort_by_key(|t| std::cmp::Reverse((t.updated_at, t.created_at)));

                // Derive status from the most relevant (latest active) core task
                let mut matched_core_task = None;
                for t in &core_tasks {
                    if t.status.is_active() {
                        matched_core_task = Some(*t);
                        break;
                    }
                }

                if let Some(task) = matched_core_task {
                    let is_active_task = task.status.is_active();
                    if task.task_type.is_update() {
                        if task.status == crate::core::task::TaskStatus::Cancelling {
                            status = crate::models::instance::InstanceStatus::CancellingUpdate;
                        } else if status == crate::models::instance::InstanceStatus::Ready
                            && is_active_task
                        {
                            status = crate::models::instance::InstanceStatus::Updating;
                        }
                    } else if task.status == crate::core::task::TaskStatus::Cancelling {
                        status = crate::models::instance::InstanceStatus::CancellingInstall;
                    } else if status == crate::models::instance::InstanceStatus::Ready
                        && is_active_task
                    {
                        status = crate::models::instance::InstanceStatus::Installing;
                    }
                }

                if status == crate::models::instance::InstanceStatus::Ready && task_ctx.is_running(&id) {
                    status = crate::models::instance::InstanceStatus::Running;
                }

                // If it is installing, updating, or cancelling, check if its corresponding task exists and is not cancelled
                if status.is_busy() && !has_valid_task {
                    // Check if installation startup or modification is recent (under 60s)
                    let is_recently_modified = if let Ok(meta) = tokio::fs::metadata(&config_path).await {
                        meta.modified().ok().and_then(|m| m.elapsed().ok()).map(|d| d.as_secs() < 60).unwrap_or(false)
                    } else if let Ok(meta) = tokio::fs::metadata(&path).await {
                        meta.modified().ok().and_then(|m| m.elapsed().ok()).map(|d| d.as_secs() < 60).unwrap_or(false)
                    } else {
                        false
                    };

                    if is_recently_modified {
                        tracing::info!("Instance {} was created or modified recently; skipping zombie cleanup as installation startup or cancellation may be in progress", id);
                    } else {
                        // For first time installation, clean up stale zombie instances (> 60s without active task)
                        if status == crate::models::instance::InstanceStatus::Installing {
                            tracing::info!("Cleaning up zombie installing instance: {}", id);
                            let _ = tokio::fs::remove_dir_all(&path).await;
                            continue;
                        } else {
                            tracing::warn!("Instance {} is marked as {:?} without active task. Skipping zombie auto-cleanup and resetting status.", id, status);
                            if config_path.exists()
                                && crate::core::launcher::update_instance_config(&path, |config| {
                                    config.set_status(crate::models::instance::InstanceStatus::Ready);
                                })
                                .await
                                .is_ok()
                            {
                                status = crate::models::instance::InstanceStatus::Ready;
                            }
                        }
                    }
                }

                if is_hidden {
                    continue;
                }

                // An instance MUST have its basic {id}.json file unless it is currently actively installing, updating, or cancelling
                if !status.is_busy() && !json_path.exists() {
                    tracing::warn!(
                        "Skipping invalid/empty instance directory {}: missing {}.json",
                        id,
                        id
                    );
                    continue;
                }

                // Read version JSON to extract metadata
                match tokio::fs::read_to_string(&json_path).await {
                    Ok(content) => {
                        let parsed_json_opt: Option<serde_json::Value> = serde_json::from_str(&content).ok();
                        // Parse basic info from JSON
                        let (
                            mut mc_version,
                            mut loader_type,
                            mut modpack_version,
                            mut modpack_type,
                            mut modpack_project_id,
                        ) = parse_version_json(&content, &id);

                        let mut loader_version = None;

                        if let Some(ref config) = config_opt {
                            if let Some(ref mc) = config.mc_version {
                                if !mc.is_empty() { mc_version = mc.clone(); }
                            }
                            if let Some(ref ld) = config.loader_type {
                                loader_type = format!("{:?}", ld);
                            }
                            if let Some(ref lv) = config.loader_version {
                                if !lv.is_empty() { loader_version = Some(lv.clone()); }
                            }
                            if config.modpack_version.is_some() {
                                modpack_version = config.modpack_version.clone();
                            }
                            if config.modpack_type.is_some() {
                                modpack_type = config.modpack_type.clone();
                            }
                            if config.modpack_project_id.is_some() {
                                modpack_project_id = config.modpack_project_id.clone();
                            }
                        }

                        if loader_version.is_none() && loader_type != "Vanilla" {
                            if let Some(ref json) = parsed_json_opt {
                                loader_version = crate::core::mod_loader::extract_loader_version_from_json(json, &id);
                            }
                        }

                        // Resolve actual MC version if it's pointing to a loader instance
                        if !mc_version.starts_with("1.") {
                            let mut current_version = mc_version.clone();
                            let mut depth = 0;
                            let mut reached_root = false;
                            while !current_version.starts_with("1.") && depth < 5 {
                                let inherited_path = versions_dir
                                    .join(&current_version)
                                    .join(format!("{}.json", current_version));
                                if let Ok(inherited_content) =
                                    tokio::fs::read_to_string(&inherited_path).await
                                {
                                    let (real_mc, ..) =
                                        parse_version_json(&inherited_content, &current_version);
                                    if real_mc == current_version || real_mc.is_empty() {
                                        reached_root = true;
                                        break;
                                    }
                                    current_version = real_mc;
                                } else {
                                    break;
                                }
                                depth += 1;
                            }

                            if current_version.starts_with("1.") || reached_root {
                                mc_version = current_version;
                            } else if let Some(extracted) =
                                extract_mc_version_from_id(&current_version)
                            {
                                mc_version = extracted;
                            } else if let Some(extracted) = extract_mc_version_from_id(&mc_version)
                            {
                                mc_version = extracted;
                            }
                        }

                        // Queue a background icon back-fill for modpack instances missing one
                        if icon_path.is_none() && !status.is_busy() {
                            if let (Some(mp_type), Some(pid)) = (&modpack_type, &modpack_project_id) {
                                if !pid.trim().is_empty() {
                                    icon_backfill.push(crate::core::instance_icon::IconBackfillCandidate {
                                        instance_id: id.clone(),
                                        instance_dir: path.clone(),
                                        modpack_type: mp_type.clone(),
                                        project_id: pid.clone(),
                                    });
                                }
                            }
                        }

                        instances.push(InstanceItem {
                            id: id.clone(),
                            name: id.clone(),
                            mc_version,
                            loader_type,
                            loader_version,
                            modpack_version,
                            modpack_type,
                            modpack_project_id,
                            server_id,
                            pack_version_id,
                            pack_file_name,
                            status,
                            icon_path: icon_path.map(|p| p.to_string_lossy().to_string()),
                        });
                    }
                    Err(e) => {
                        tracing::warn!("Failed to read version JSON for {}: {}", id, e);
                        let mut mc_version = extract_mc_version_from_id(&id).unwrap_or_default();
                        let mut loader_type = "Vanilla".to_string();
                        let mut loader_version = None;
                        let mut modpack_version = None;
                        let mut modpack_type = None;
                        let mut modpack_project_id = None;

                        if let Some(ref config) = config_opt {
                            if let Some(ref mc) = config.mc_version {
                                if !mc.is_empty() { mc_version = mc.clone(); }
                            }
                            if let Some(ref ld) = config.loader_type {
                                loader_type = format!("{:?}", ld);
                            }
                            if let Some(ref lv) = config.loader_version {
                                if !lv.is_empty() { loader_version = Some(lv.clone()); }
                            }
                            if config.modpack_version.is_some() {
                                modpack_version = config.modpack_version.clone();
                            }
                            if config.modpack_type.is_some() {
                                modpack_type = config.modpack_type.clone();
                            }
                            if config.modpack_project_id.is_some() {
                                modpack_project_id = config.modpack_project_id.clone();
                            }
                        }

                        // Add with default/parsed values
                        instances.push(InstanceItem {
                            id: id.clone(),
                            name: id.clone(),
                            mc_version,
                            loader_type,
                            loader_version,
                            modpack_version,
                            modpack_type,
                            modpack_project_id,
                            server_id,
                            pack_version_id,
                            pack_file_name,
                            status,
                            icon_path: icon_path.map(|p| p.to_string_lossy().to_string()),
                        });
                    }
                }
            }
        }
    }

    // Sort by name
    instances.sort_by(|a, b| a.name.cmp(&b.name));

    Ok((instances, icon_backfill))
}

struct ScanSharedState {
    in_flight_tx: Option<tokio::sync::broadcast::Sender<Result<Vec<InstanceItem>, String>>>,
    last_completed_at: Option<std::time::Instant>,
    last_result: Option<Vec<InstanceItem>>,
}

static SCAN_STATE: LazyLock<Mutex<ScanSharedState>> = LazyLock::new(|| {
    Mutex::new(ScanSharedState {
        in_flight_tx: None,
        last_completed_at: None,
        last_result: None,
    })
});

/// Invalidate the short-lived instance scan cache after mutating instance metadata or folders.
pub async fn invalidate_instance_scan_cache() {
    let mut state = SCAN_STATE.lock().await;
    state.last_completed_at = None;
    state.last_result = None;
}

/// Scan all locally installed instances from the versions directory.
/// Protected by Singleflight coalescing and a 200ms throttle cache against concurrent avalanche.
#[tauri::command]
pub async fn scan_installed_instances(
    app: tauri::AppHandle,
    task_manager: tauri::State<'_, crate::core::task::TaskManager>,
) -> Result<Vec<InstanceItem>, String> {
    let mut rx_opt = None;
    {
        let mut state = SCAN_STATE.lock().await;
        // 1. Throttle / Debounce cache: if completed within last 200ms, return cached
        if let (Some(completed_at), Some(ref res)) = (state.last_completed_at, &state.last_result) {
            if completed_at.elapsed() < std::time::Duration::from_millis(200) {
                return Ok(res.clone());
            }
        }
        // 2. In-flight coalescing: join existing scan if in progress
        if let Some(ref tx) = state.in_flight_tx {
            rx_opt = Some(tx.subscribe());
        } else {
            let (tx, _) = tokio::sync::broadcast::channel(1);
            state.in_flight_tx = Some(tx);
        }
    }

    if let Some(mut rx) = rx_opt {
        match rx.recv().await {
            Ok(res) => return res,
            Err(_) => {
                // Sender was dropped or lagged, continue to fresh scan fallback
            }
        }
    }

    // Execute actual scan
    let res = scan_installed_instances_impl(&app, &task_manager).await;

    // Broadcast result and update cache
    {
        let mut state = SCAN_STATE.lock().await;
        if let Some(tx) = state.in_flight_tx.take() {
            let _ = tx.send(res.clone());
        }
        if let Ok(ref instances) = res {
            state.last_completed_at = Some(std::time::Instant::now());
            state.last_result = Some(instances.clone());
        }
    }

    res
}

async fn scan_installed_instances_impl(
    app: &tauri::AppHandle,
    task_manager: &crate::core::task::TaskManager,
) -> Result<Vec<InstanceItem>, String> {
    tracing::info!("Scanning installed instances...");

    let base_dir = get_minecraft_base();
    let versions_dir = base_dir.join("versions");

    if !versions_dir.exists() {
        tracing::info!("Versions directory does not exist, returning empty list");
        return Ok(Vec::new());
    }

    // Load running instances from launcher state
    let running_instances_set: std::collections::HashSet<String> =
        if let Some(state) = app.try_state::<crate::core::launcher::RunningInstances>() {
            state.get_running_instances().await
        } else {
            std::collections::HashSet::new()
        };

    // Load all tasks from DB once to avoid locking inside loop
    let tasks = task_manager.load_history().await.unwrap_or_default();
    let mut task_ctx = task_manager.task_context(&tasks);
    task_ctx.running_instances = Some(&running_instances_set);

    let (instances, icon_backfill) =
        scan_installed_instances_internal(&versions_dir, &task_ctx).await?;

    crate::core::instance_icon::schedule_icon_backfill(app.clone(), icon_backfill);

    tracing::info!("Found {} installed instances", instances.len());
    Ok(instances)
}

/// Parse version JSON content to extract Minecraft version and loader type.
pub fn parse_version_json(
    content: &str,
    id: &str,
) -> (
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
) {
    // Try to parse as JSON
    match serde_json::from_str::<serde_json::Value>(content) {
        Ok(json) => {
            // Determine loader type based on id first, then inheritsFrom, then mainClass
            let id_lower = id.to_lowercase();
            let inherits_from = json
                .get("inheritsFrom")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let inherits_lower = inherits_from.to_lowercase();

            let loader_type =
                if id_lower.contains("neoforge") || inherits_lower.contains("neoforge") {
                    "NeoForge"
                } else if id_lower.contains("forge") || inherits_lower.contains("forge") {
                    "Forge"
                } else if id_lower.contains("fabric") || inherits_lower.contains("fabric") {
                    "Fabric"
                } else {
                    let main_class = json.get("mainClass").and_then(|v| v.as_str()).unwrap_or("");
                    let mc_args = json
                        .get("minecraftArguments")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");

                    let mut check_forge = false;
                    let mut check_fabric = false;
                    let mut check_neoforge = false;

                    let mut check_str = |s: &str| {
                        let lower = s.to_lowercase();
                        if lower.contains("fabric") {
                            check_fabric = true;
                        }
                        if lower.contains("neoforge") {
                            check_neoforge = true;
                        }
                        if lower.contains("forge")
                            || lower.contains("fml")
                            || lower.contains("bootstraplauncher")
                        {
                            check_forge = true;
                        }
                    };

                    check_str(main_class);
                    check_str(mc_args);

                    if let Some(args) = json.get("arguments") {
                        if let Some(game) = args.get("game") {
                            if let Some(arr) = game.as_array() {
                                for item in arr {
                                    if let Some(s) = item.as_str() {
                                        check_str(s);
                                    } else if let Some(obj) = item.as_object() {
                                        if let Some(value) = obj.get("value") {
                                            match value {
                                                serde_json::Value::String(s) => check_str(s),
                                                serde_json::Value::Array(values) => {
                                                    for v in values {
                                                        if let Some(s) = v.as_str() {
                                                            check_str(s);
                                                        }
                                                    }
                                                }
                                                _ => {}
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    if check_fabric {
                        "Fabric"
                    } else if check_neoforge {
                        "NeoForge"
                    } else if check_forge {
                        "Forge"
                    } else {
                        "Vanilla"
                    }
                };

            // Extract Minecraft version
            // For Fabric/Forge, inheritsFrom contains the base version
            let mc_version = json
                .get("inheritsFrom")
                .and_then(|v| v.as_str())
                .map(String::from)
                .or_else(|| {
                    json.get("clientVersion")
                        .and_then(|v| v.as_str())
                        .map(String::from)
                })
                .or_else(|| json.get("id").and_then(|v| v.as_str()).map(String::from))
                .unwrap_or_else(|| {
                    // Fallback to folder name if nothing else works
                    extract_mc_version_from_id(id).unwrap_or_default()
                });

            // Extract Modpack Info
            let modpack_version = json
                .get("modpackVersion")
                .and_then(|v| v.as_str())
                .map(String::from);
            let modpack_type = json
                .get("modpackType")
                .and_then(|v| v.as_str())
                .map(String::from);
            let modpack_project_id = json
                .get("modpackProjectId")
                .and_then(|v| v.as_str())
                .map(String::from);

            (
                mc_version,
                loader_type.to_string(),
                modpack_version,
                modpack_type,
                modpack_project_id,
            )
        }
        Err(_) => {
            // Fallback: extract from id
            let loader_type = if id.to_lowercase().contains("fabric") {
                "Fabric"
            } else if id.to_lowercase().contains("forge") {
                "Forge"
            } else if id.to_lowercase().contains("neoforge") {
                "NeoForge"
            } else {
                "Vanilla"
            };
            (
                extract_mc_version_from_id(id).unwrap_or_default(),
                loader_type.to_string(),
                None,
                None,
                None,
            )
        }
    }
}

/// Extract Minecraft version from instance ID string.
fn extract_mc_version_from_id(id: &str) -> Option<String> {
    // Common patterns: "1.20.1", "1.20.1-Fabric", "Fabric-1.20.1-0.15.11"
    // Match version pattern like "1.20.1" or "1.20"
    let parts: Vec<&str> = id.split(['-', '_']).collect();

    for part in parts {
        // Check if it looks like a version (starts with digit, contains dots)
        // Ensure it starts with "1." to filter out Forge/NeoForge version numbers like 26.1.2 or 47.1.0
        if part.starts_with("1.") && part.contains('.') {
            // Basic validation: should have at least major.minor
            let dots = part.matches('.').count();
            if (1..=3).contains(&dots) {
                return Some(part.to_string());
            }
        }
    }

    None
}

/// Get detailed information about a specific instance.
#[tauri::command]
pub async fn get_instance_details(version_id: String) -> Result<InstanceItem, String> {
    let base_dir = get_minecraft_base();
    let json_path = base_dir
        .join("versions")
        .join(&version_id)
        .join(format!("{}.json", version_id));

    if !json_path.exists() {
        return Err(format!("Instance {} not found", version_id));
    }

    let content = tokio::fs::read_to_string(&json_path)
        .await
        .map_err(|e| format!("Failed to read version JSON: {}", e))?;

    let (mut mc_version, mut loader_type, mut modpack_version, mut modpack_type, mut modpack_project_id) =
        parse_version_json(&content, &version_id);

    // Read dlml.json for core metadata and bindings
    let config_path = base_dir
        .join("versions")
        .join(&version_id)
        .join("dlml.json");
    let mut server_id = None;
    let mut pack_version_id = None;
    let mut pack_file_name = None;
    let mut status = crate::models::instance::InstanceStatus::Ready;
    let mut icon_name = None;
    let mut loader_version = None;

    if config_path.exists() {
        if let Ok(content) = tokio::fs::read_to_string(&config_path).await {
            let has_legacy_fields = crate::core::launcher::InstanceConfig::has_legacy_status_in_json(&content);
            if let Ok(mut config) =
                serde_json::from_str::<crate::core::launcher::InstanceConfig>(&content)
            {
                if has_legacy_fields {
                    let inst_dir = get_instance_dir(&version_id);
                    if let Ok(cleaned) = crate::core::launcher::update_instance_config(&inst_dir, |_| {}).await {
                        config = cleaned;
                    }
                }
                status = config.effective_status();
                server_id = config.server_id.clone();
                pack_version_id = config.pack_version_id.clone();
                pack_file_name = config.pack_file_name.clone();
                icon_name = config.icon.clone();
                
                // Override metadata with dlml.json if available
                if let Some(mc) = &config.mc_version {
                    if !mc.is_empty() { mc_version = mc.clone(); }
                }
                if let Some(ld) = &config.loader_type {
                    loader_type = format!("{:?}", ld);
                }
                if config.modpack_version.is_some() {
                    modpack_version = config.modpack_version.clone();
                }
                if config.modpack_type.is_some() {
                    modpack_type = config.modpack_type.clone();
                }
                if config.modpack_project_id.is_some() {
                    modpack_project_id = config.modpack_project_id.clone();
                }
                if let Some(lv) = &config.loader_version {
                    if !lv.is_empty() {
                        loader_version = Some(lv.clone());
                    }
                }
            }
        }
    }

    // Resolve actual MC version if still missing or invalid
    if !mc_version.starts_with("1.") {
        let mut current_version = mc_version.clone();
        let mut depth = 0;
        while !current_version.starts_with("1.") && depth < 5 {
            let inherited_path = base_dir
                .join("versions")
                .join(&current_version)
                .join(format!("{}.json", current_version));
            if let Ok(inherited_content) = tokio::fs::read_to_string(&inherited_path).await {
                let (real_mc, ..) = parse_version_json(&inherited_content, &current_version);
                if real_mc == current_version || real_mc.is_empty() {
                    break;
                }
                current_version = real_mc;
            } else {
                break;
            }
            depth += 1;
        }

        if current_version.starts_with("1.") {
            mc_version = current_version;
        } else if let Some(extracted) = extract_mc_version_from_id(&current_version) {
            mc_version = extracted;
        } else if let Some(extracted) = extract_mc_version_from_id(&mc_version) {
            mc_version = extracted;
        }
    }

    let instance_dir = base_dir.join("versions").join(&version_id);
    let icon_path = crate::core::instance_icon::resolve_icon_path(&instance_dir, icon_name.as_deref())
        .await
        .map(|p| p.to_string_lossy().to_string());

    if loader_version.is_none() && loader_type != "Vanilla" {
        if let Ok(c) = tokio::fs::read_to_string(&json_path).await {
            if let Ok(pj) = serde_json::from_str::<serde_json::Value>(&c) {
                loader_version = crate::core::mod_loader::extract_loader_version_from_json(&pj, &version_id);
            }
        }
    }

    Ok(InstanceItem {
        id: version_id.clone(),
        name: version_id,
        mc_version,
        loader_type,
        loader_version,
        modpack_version,
        modpack_type,
        modpack_project_id,
        server_id,
        pack_version_id,
        pack_file_name,
        status,
        icon_path,
    })
}

/// Compute the base directory for a specific instance/version.
///
/// Centralizes the instance layout so commands stay in sync if the structure changes.
fn get_instance_dir(version_id: &str) -> std::path::PathBuf {
    get_minecraft_base().join("versions").join(version_id)
}

/// Open the instance folder in the system file manager.
#[tauri::command]
pub async fn open_instance_folder(version_id: String) -> Result<(), String> {
    if !crate::core::fs_utils::is_safe_single_name(&version_id) {
        return Err("Invalid instance ID".to_string());
    }
    let instance_dir = get_instance_dir(&version_id);

    // Ensure directory exists — create it if missing so the user sees an empty folder
    tokio::fs::create_dir_all(&instance_dir)
        .await
        .map_err(|e| format!("Failed to create instance directory: {}", e))?;

    // Open in system file manager (Explorer / Finder / xdg-open)
    open::that(&instance_dir).map_err(|e| format!("Failed to open folder: {}", e))?;

    tracing::info!("Opened instance folder: {}", instance_dir.display());
    Ok(())
}

/// Open a specific subfolder of an instance in the system file manager.
#[tauri::command]
pub async fn open_instance_subfolder(version_id: String, subfolder: String) -> Result<(), String> {
    if !crate::core::fs_utils::is_safe_single_name(&version_id) {
        return Err("Invalid instance ID".to_string());
    }
    let mut target_dir = get_instance_dir(&version_id);

    // Prevent path traversal
    if subfolder.contains("..") || subfolder.contains("/") || subfolder.contains("\\") {
        return Err("Invalid subfolder name".to_string());
    }

    target_dir = target_dir.join(subfolder);

    // Ensure directory exists (async, idempotent)
    tokio::fs::create_dir_all(&target_dir)
        .await
        .map_err(|e| format!("Failed to create directory: {}", e))?;

    open::that(&target_dir).map_err(|e| format!("Failed to open folder: {}", e))?;

    tracing::info!("Opened instance subfolder: {}", target_dir.display());
    Ok(())
}

async fn check_instance_busy(
    version_id: &str,
    instance_dir: &std::path::Path,
    task_manager: &crate::core::task::TaskManager,
) -> bool {
    if task_manager.is_instance_starting(version_id) || task_manager.is_instance_busy(version_id).await {
        return true;
    }

    let config_path = instance_dir.join("dlml.json");
    if config_path.exists() {
        if let Ok(content) = tokio::fs::read_to_string(&config_path).await {
            if let Ok(config) = serde_json::from_str::<crate::core::launcher::InstanceConfig>(&content) {
                if config.effective_status().is_busy() {
                    return true;
                }
            }
        }
    }
    
    let tasks = task_manager.load_history().await.unwrap_or_default();
    for task in &tasks {
        if let Some(tid) = task.task_type.instance_id() {
            if tid == version_id && task.status.is_active() {
                return true;
            }
        }
    }
    false
}

/// Delete an instance (removes version directory).
#[tauri::command]
pub async fn delete_instance(
    version_id: String,
    app: tauri::AppHandle,
    task_manager: tauri::State<'_, crate::core::task::TaskManager>,
    running_instances: tauri::State<'_, crate::core::launcher::RunningInstances>,
) -> Result<(), String> {
    if !crate::core::fs_utils::is_safe_single_name(&version_id) {
        return Err(format!("Invalid instance name: {}", version_id));
    }
    let base_dir = get_minecraft_base();
    let instance_dir = base_dir.join("versions").join(&version_id);

    if !instance_dir.exists() {
        return Err(format!("Instance {} not found", version_id));
    }

    {
        let map = running_instances.0.lock().await;
        if map.contains_key(&version_id) {
            return Err(format!("Cannot delete instance {} because it is currently running", version_id));
        }
    }

    if check_instance_busy(&version_id, &instance_dir, &task_manager).await {
        return Err(format!("Cannot delete instance {} while it is installing or updating", version_id));
    }

    // Coordinate instance deletion with asset applications via per-instance lock
    let instance_lock = crate::core::drop_install::get_instance_lock(&version_id);
    let _instance_guard = instance_lock.lock().await;

    if check_instance_busy(&version_id, &instance_dir, &task_manager).await {
        return Err(format!("Cannot delete instance {} while it is installing or updating", version_id));
    }

    if !instance_dir.exists() {
        return Ok(());
    }

    // Use blocking remove_dir_all since the recursive directory removal
    // doesn't work well with async
    tokio::task::spawn_blocking(move || {
        std::fs::remove_dir_all(&instance_dir)
            .map_err(|e| format!("Failed to delete instance directory: {}", e))
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))??;

    if let Some(stats_db) = app.try_state::<crate::core::statistics::StatisticsDb>() {
        if let Err(e) = stats_db.delete_stats(version_id.clone()).await {
            tracing::warn!("Failed to delete statistics for instance {}: {}", version_id, e);
        }
    }

    tracing::info!("Deleted instance: {}", version_id);

    // Automatically trigger debounced garbage collection of the global mod cache
    crate::core::cache::trigger_auto_gc(app);

    Ok(())
}

async fn rename_file_case_safe(
    old_path: &std::path::Path,
    new_path: &std::path::Path,
    is_case_only: bool,
) -> Result<bool, String> {
    if !tokio::fs::try_exists(old_path)
        .await
        .map_err(|e| format!("Failed to check {:?}: {}", old_path, e))?
    {
        return Ok(false);
    }
    if is_case_only {
        let tmp_path = old_path.with_file_name(format!(
            ".dlml_rename_tmp_{}",
            uuid::Uuid::new_v4().simple()
        ));
        tokio::fs::rename(old_path, &tmp_path)
            .await
            .map_err(|e| format!("Failed to stage rename for {:?}: {}", old_path, e))?;
        if let Err(e) = tokio::fs::rename(&tmp_path, new_path).await {
            let _ = tokio::fs::rename(&tmp_path, old_path).await;
            return Err(format!("Failed to finalize rename to {:?}: {}", new_path, e));
        }
    } else {
        tokio::fs::rename(old_path, new_path)
            .await
            .map_err(|e| format!("Failed to rename {:?} to {:?}: {}", old_path, new_path, e))?;
    }
    Ok(true)
}

async fn rename_dir_case_safe(
    old_dir: &std::path::Path,
    new_dir: &std::path::Path,
    is_case_only: bool,
) -> Result<(), String> {
    if is_case_only {
        let parent = old_dir
            .parent()
            .ok_or_else(|| "Instance directory has no parent".to_string())?;
        let temp_dir = parent.join(format!(".rename_tmp_{}", uuid::Uuid::new_v4().simple()));
        tokio::fs::rename(old_dir, &temp_dir)
            .await
            .map_err(|e| format!("Failed to stage instance directory rename: {}", e))?;
        if let Err(e) = tokio::fs::rename(&temp_dir, new_dir).await {
            let _ = tokio::fs::rename(&temp_dir, old_dir).await;
            return Err(format!("Failed to finalize instance directory rename: {}", e));
        }
    } else {
        tokio::fs::rename(old_dir, new_dir)
            .await
            .map_err(|e| format!("Failed to rename instance directory: {}", e))?;
    }
    Ok(())
}

fn rebase_task_dest_path(
    dest_path: &str,
    old_dir: &std::path::Path,
    new_dir: &std::path::Path,
) -> Option<String> {
    if let Ok(rel) = std::path::Path::new(dest_path).strip_prefix(old_dir) {
        return Some(new_dir.join(rel).to_string_lossy().to_string());
    }
    let norm_dest = dest_path.replace('\\', "/");
    let norm_old = old_dir.to_string_lossy().replace('\\', "/");
    let norm_old_trimmed = norm_old.trim_end_matches('/');
    if let Some(rest) = norm_dest.strip_prefix(norm_old_trimmed) {
        if rest.is_empty() || rest.starts_with('/') {
            let rel_trimmed = rest.trim_start_matches('/');
            let mut rebased = new_dir.to_path_buf();
            for part in rel_trimmed.split('/').filter(|p| !p.is_empty()) {
                rebased.push(part);
            }
            return Some(rebased.to_string_lossy().to_string());
        }
    }
    None
}

struct DependentInstanceJson {
    id: String,
    json_path: std::path::PathBuf,
    orig_bytes: Vec<u8>,
    json_val: serde_json::Value,
}

async fn collect_dependent_instance_jsons(
    versions_dir: &std::path::Path,
    target_id: &str,
) -> Result<Vec<DependentInstanceJson>, String> {
    let mut dependents = Vec::new();
    let mut entries = tokio::fs::read_dir(versions_dir)
        .await
        .map_err(|e| format!("Failed to read versions directory: {}", e))?;
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|e| format!("Failed to read versions entry: {}", e))?
    {
        let entry_path = entry.path();
        let is_dir = match tokio::fs::metadata(&entry_path).await {
            Ok(m) => m.is_dir(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                return Err(format!(
                    "Failed to inspect instance directory {:?}: {}",
                    entry_path, e
                ));
            }
        };
        if !is_dir {
            continue;
        }
        let other_id = entry.file_name().to_string_lossy().to_string();
        if other_id == target_id {
            continue;
        }
        let other_json_path = entry_path.join(format!("{}.json", other_id));
        if !tokio::fs::try_exists(&other_json_path).await.map_err(|e| {
            format!(
                "Failed to check dependent instance JSON {:?}: {}",
                other_json_path, e
            )
        })? {
            continue;
        }
        let orig_bytes = tokio::fs::read(&other_json_path).await.map_err(|e| {
            format!(
                "Failed to read dependent instance JSON {:?}: {}",
                other_json_path, e
            )
        })?;
        let json_val = match serde_json::from_slice::<serde_json::Value>(&orig_bytes) {
            Ok(val) => val,
            Err(e) => {
                tracing::warn!(
                    "Skipping malformed version JSON {:?} while scanning dependents of {}: {}",
                    other_json_path,
                    target_id,
                    e
                );
                continue;
            }
        };
        let inherits_matches = json_val
            .get("inheritsFrom")
            .and_then(|v| v.as_str())
            .is_some_and(|v| v == target_id);
        if inherits_matches {
            dependents.push(DependentInstanceJson {
                id: other_id,
                json_path: other_json_path,
                orig_bytes,
                json_val,
            });
        }
    }
    Ok(dependents)
}

async fn find_dependent_instances(
    versions_dir: &std::path::Path,
    target_id: &str,
) -> Result<Vec<String>, String> {
    let dependents = collect_dependent_instance_jsons(versions_dir, target_id).await?;
    Ok(dependents.into_iter().map(|d| d.id).collect())
}

enum VersionJsonPlan {
    None,
    RenameRaw,
    Update {
        orig_bytes: Vec<u8>,
        updated_bytes: Vec<u8>,
    },
}

struct DependentJsonUpdate {
    json_path: std::path::PathBuf,
    orig_bytes: Vec<u8>,
    updated_bytes: Vec<u8>,
}

struct RenameCommitPlan {
    version_json: VersionJsonPlan,
    dlml_orig_bytes: Option<Vec<u8>>,
    tasks_update: Option<(Vec<u8>, Vec<u8>)>,
    dependent_updates: Vec<DependentJsonUpdate>,
}

enum RollbackStep {
    RenameDir {
        from: std::path::PathBuf,
        to: std::path::PathBuf,
        is_case_only: bool,
    },
    RenameFile {
        from: std::path::PathBuf,
        to: std::path::PathBuf,
        is_case_only: bool,
    },
    RestoreVersionJsonFromBackup {
        new_json_path: std::path::PathBuf,
        old_json_path: std::path::PathBuf,
        backup_json: std::path::PathBuf,
        orig_bytes: Vec<u8>,
    },
    RestoreFile {
        path: std::path::PathBuf,
        orig_bytes: Vec<u8>,
    },
}

/// Read, validate, and prepare all metadata transformations before mutating the filesystem.
async fn prepare_rename_commit_plan(
    versions_dir: &std::path::Path,
    old_dir: &std::path::Path,
    new_dir: &std::path::Path,
    old_id: &str,
    new_id: &str,
) -> Result<RenameCommitPlan, String> {
    // 1. Validate dlml.json if present so malformed configs fail before any directory rename
    let old_config_path = old_dir.join("dlml.json");
    let (dlml_orig_bytes, dlml_mc) = if tokio::fs::try_exists(&old_config_path)
        .await
        .map_err(|e| format!("Failed to check dlml.json: {}", e))?
    {
        let orig_bytes = tokio::fs::read(&old_config_path)
            .await
            .map_err(|e| format!("Failed to read instance config: {}", e))?;
        let config = serde_json::from_slice::<crate::core::launcher::InstanceConfig>(&orig_bytes)
            .map_err(|e| format!("Failed to parse instance config: {}", e))?;
        let mc_version = config.mc_version.filter(|s| !s.trim().is_empty());
        (Some(orig_bytes), mc_version)
    } else {
        (None, None)
    };

    // 2. Read and prepare <old_id>.json -> <new_id>.json
    let old_json_path = old_dir.join(format!("{}.json", old_id));
    let version_json = if tokio::fs::try_exists(&old_json_path)
        .await
        .map_err(|e| format!("Failed to check version JSON: {}", e))?
    {
        let orig_bytes = tokio::fs::read(&old_json_path)
            .await
            .map_err(|e| format!("Failed to read version JSON: {}", e))?;
        if let Ok(content) = std::str::from_utf8(&orig_bytes) {
            if let Ok(mut json_val) = serde_json::from_str::<serde_json::Value>(content) {
                if let Some(obj) = json_val.as_object_mut() {
                    let has_inherits = obj
                        .get("inheritsFrom")
                        .and_then(|v| v.as_str())
                        .is_some_and(|s| !s.is_empty());
                    let has_client_version = obj
                        .get("clientVersion")
                        .and_then(|v| v.as_str())
                        .is_some_and(|s| !s.is_empty());
                    if !has_inherits && !has_client_version {
                        let (parsed_mc, ..) = parse_version_json(content, old_id);
                        let preserved_mc = dlml_mc.unwrap_or(parsed_mc);
                        if !preserved_mc.is_empty() {
                            obj.insert(
                                "clientVersion".to_string(),
                                serde_json::json!(preserved_mc),
                            );
                        }
                    }
                    obj.insert("id".to_string(), serde_json::json!(new_id));
                }
                let updated = serde_json::to_string_pretty(&json_val)
                    .map_err(|e| format!("Failed to serialize version JSON: {}", e))?;
                VersionJsonPlan::Update {
                    orig_bytes,
                    updated_bytes: updated.into_bytes(),
                }
            } else {
                VersionJsonPlan::RenameRaw
            }
        } else {
            VersionJsonPlan::RenameRaw
        }
    } else {
        VersionJsonPlan::None
    };

    // 3. Validate and prepare modpack_tasks.json destination path rebasing
    let old_tasks_path = old_dir.join("modpack_tasks.json");
    let tasks_update = if tokio::fs::try_exists(&old_tasks_path)
        .await
        .map_err(|e| format!("Failed to check modpack_tasks.json: {}", e))?
    {
        let orig_bytes = tokio::fs::read(&old_tasks_path)
            .await
            .map_err(|e| format!("Failed to read modpack_tasks.json: {}", e))?;
        let mut tasks = serde_json::from_slice::<Vec<crate::downloader::DownloadTask>>(&orig_bytes)
            .map_err(|e| format!("Failed to parse modpack_tasks.json: {}", e))?;
        let mut changed = false;
        for task in &mut tasks {
            if let Some(updated_dest) = rebase_task_dest_path(&task.dest_path, old_dir, new_dir) {
                task.dest_path = updated_dest;
                changed = true;
            }
        }
        if changed {
            let serialized = serde_json::to_string_pretty(&tasks)
                .map_err(|e| format!("Failed to serialize modpack_tasks.json: {}", e))?;
            Some((orig_bytes, serialized.into_bytes()))
        } else {
            None
        }
    } else {
        None
    };

    // 4. Collect and prepare dependent instances' inheritsFrom updates
    let dependents = collect_dependent_instance_jsons(versions_dir, old_id).await?;
    let mut dependent_updates = Vec::with_capacity(dependents.len());
    for mut dep in dependents {
        if let Some(obj) = dep.json_val.as_object_mut() {
            obj.insert("inheritsFrom".to_string(), serde_json::json!(new_id));
        }
        let updated = serde_json::to_string_pretty(&dep.json_val).map_err(|e| {
            format!(
                "Failed to serialize dependent instance JSON for {}: {}",
                dep.id, e
            )
        })?;
        dependent_updates.push(DependentJsonUpdate {
            json_path: dep.json_path,
            orig_bytes: dep.orig_bytes,
            updated_bytes: updated.into_bytes(),
        });
    }

    Ok(RenameCommitPlan {
        version_json,
        dlml_orig_bytes,
        tasks_update,
        dependent_updates,
    })
}

async fn swap_version_json_with_backup(
    old_json_path: &std::path::Path,
    new_json_path: &std::path::Path,
    updated_bytes: &[u8],
) -> Result<std::path::PathBuf, String> {
    let parent = old_json_path
        .parent()
        .ok_or_else(|| "Version JSON path has no parent directory".to_string())?;
    let unique = uuid::Uuid::new_v4().simple();
    let temp_json = parent.join(format!(".dlml_rename_tmp_{}", unique));
    let backup_json = parent.join(format!(".dlml_rename_bak_{}", unique));

    crate::core::fs_utils::write_file_atomic(&temp_json, updated_bytes)
        .await
        .map_err(|e| format!("Failed to write temporary version JSON: {}", e))?;

    if let Err(e) = tokio::fs::rename(old_json_path, &backup_json).await {
        let _ = tokio::fs::remove_file(&temp_json).await;
        return Err(format!("Failed to backup old version JSON: {}", e));
    }
    if let Err(e) = tokio::fs::rename(&temp_json, new_json_path).await {
        let _ = tokio::fs::rename(&backup_json, old_json_path).await;
        let _ = tokio::fs::remove_file(&temp_json).await;
        return Err(format!("Failed to finalize version JSON rename: {}", e));
    }

    Ok(backup_json)
}

async fn restore_version_json_from_backup(
    new_json_path: &std::path::Path,
    old_json_path: &std::path::Path,
    backup_json: &std::path::Path,
    orig_bytes: &[u8],
) -> Result<(), String> {
    let mut remove_err: Option<String> = None;
    if tokio::fs::try_exists(new_json_path).await.unwrap_or(false) {
        if let Err(e) = tokio::fs::remove_file(new_json_path).await {
            remove_err = Some(format!("Failed to remove {:?}: {}", new_json_path, e));
        }
    }

    if tokio::fs::try_exists(backup_json).await.unwrap_or(false) {
        match tokio::fs::rename(backup_json, old_json_path).await {
            Ok(()) => return remove_err.map_or(Ok(()), Err),
            Err(rename_err) => {
                crate::core::fs_utils::write_file_atomic(old_json_path, orig_bytes)
                    .await
                    .map_err(|write_err| {
                        format!(
                            "Failed to restore {:?} from backup ({}) or fallback write ({})",
                            old_json_path, rename_err, write_err
                        )
                    })?;
                let _ = tokio::fs::remove_file(backup_json).await;
                return remove_err.map_or(Ok(()), Err);
            }
        }
    }

    crate::core::fs_utils::write_file_atomic(old_json_path, orig_bytes)
        .await
        .map_err(|e| format!("Failed to restore {:?}: {}", old_json_path, e))?;
    remove_err.map_or(Ok(()), Err)
}

async fn execute_rollback(steps: Vec<RollbackStep>) -> Vec<String> {
    let mut errors = Vec::new();
    for step in steps.into_iter().rev() {
        let res = match step {
            RollbackStep::RenameDir {
                from,
                to,
                is_case_only,
            } => rename_dir_case_safe(&from, &to, is_case_only).await,
            RollbackStep::RenameFile {
                from,
                to,
                is_case_only,
            } => rename_file_case_safe(&from, &to, is_case_only)
                .await
                .map(|_| ()),
            RollbackStep::RestoreVersionJsonFromBackup {
                new_json_path,
                old_json_path,
                backup_json,
                orig_bytes,
            } => {
                restore_version_json_from_backup(
                    &new_json_path,
                    &old_json_path,
                    &backup_json,
                    &orig_bytes,
                )
                .await
            }
            RollbackStep::RestoreFile { path, orig_bytes } => {
                crate::core::fs_utils::write_file_atomic(&path, &orig_bytes)
                    .await
                    .map_err(|e| format!("Failed to restore {:?}: {}", path, e))
            }
        };
        if let Err(e) = res {
            tracing::error!("Instance rename rollback step failed: {}", e);
            errors.push(e);
        }
    }
    errors
}

struct RenameTarget<'a> {
    old_dir: &'a std::path::Path,
    new_dir: &'a std::path::Path,
    old_id: &'a str,
    new_id: &'a str,
    is_case_only: bool,
}

#[derive(Default)]
struct RenameTxState {
    rollback: Vec<RollbackStep>,
    cleanup_files: Vec<std::path::PathBuf>,
}

async fn apply_rename_plan_steps(
    target: &RenameTarget<'_>,
    plan: RenameCommitPlan,
    tx: &mut RenameTxState,
) -> Result<(), String> {
    let old_dir = target.old_dir;
    let new_dir = target.new_dir;
    let old_id = target.old_id;
    let new_id = target.new_id;
    let is_case_only = target.is_case_only;

    // 1. Rename instance directory
    rename_dir_case_safe(old_dir, new_dir, is_case_only).await?;
    tx.rollback.push(RollbackStep::RenameDir {
        from: new_dir.to_path_buf(),
        to: old_dir.to_path_buf(),
        is_case_only,
    });

    // 2. Rename <old_id>.jar -> <new_id>.jar and <old_id>.json.bak -> <new_id>.json.bak
    let old_jar_path = new_dir.join(format!("{}.jar", old_id));
    let new_jar_path = new_dir.join(format!("{}.jar", new_id));
    if rename_file_case_safe(&old_jar_path, &new_jar_path, is_case_only).await? {
        tx.rollback.push(RollbackStep::RenameFile {
            from: new_jar_path,
            to: old_jar_path,
            is_case_only,
        });
    }

    let old_bak_path = new_dir.join(format!("{}.json.bak", old_id));
    let new_bak_path = new_dir.join(format!("{}.json.bak", new_id));
    if rename_file_case_safe(&old_bak_path, &new_bak_path, is_case_only).await? {
        tx.rollback.push(RollbackStep::RenameFile {
            from: new_bak_path,
            to: old_bak_path,
            is_case_only,
        });
    }

    // 3. Swap and update <old_id>.json -> <new_id>.json (keep backup until transaction commits)
    let old_json_path = new_dir.join(format!("{}.json", old_id));
    let new_json_path = new_dir.join(format!("{}.json", new_id));
    match plan.version_json {
        VersionJsonPlan::None => {}
        VersionJsonPlan::RenameRaw => {
            if rename_file_case_safe(&old_json_path, &new_json_path, is_case_only).await? {
                tx.rollback.push(RollbackStep::RenameFile {
                    from: new_json_path,
                    to: old_json_path,
                    is_case_only,
                });
            }
        }
        VersionJsonPlan::Update {
            orig_bytes,
            updated_bytes,
        } => {
            let backup_json =
                swap_version_json_with_backup(&old_json_path, &new_json_path, &updated_bytes)
                    .await?;
            tx.cleanup_files.push(backup_json.clone());
            tx.rollback.push(RollbackStep::RestoreVersionJsonFromBackup {
                new_json_path,
                old_json_path,
                backup_json,
                orig_bytes,
            });
        }
    }

    // 4. Update dlml.json if present
    if let Some(orig_bytes) = plan.dlml_orig_bytes {
        crate::core::launcher::update_instance_config(new_dir, |config| {
            if config.name.is_some() {
                config.name = Some(new_id.to_string());
            }
        })
        .await?;
        tx.rollback.push(RollbackStep::RestoreFile {
            path: new_dir.join("dlml.json"),
            orig_bytes,
        });
    }

    // 5. Update modpack_tasks.json destination paths if rebased
    if let Some((orig_bytes, updated_bytes)) = plan.tasks_update {
        let tasks_path = new_dir.join("modpack_tasks.json");
        crate::core::fs_utils::write_file_atomic(&tasks_path, &updated_bytes)
            .await
            .map_err(|e| format!("Failed to update modpack_tasks.json: {}", e))?;
        tx.rollback.push(RollbackStep::RestoreFile {
            path: tasks_path,
            orig_bytes,
        });
    }

    // 6. Update any child instances under versions_dir whose inheritsFrom points to old_id
    for dep in plan.dependent_updates {
        crate::core::fs_utils::write_file_atomic(&dep.json_path, &dep.updated_bytes)
            .await
            .map_err(|e| {
                format!(
                    "Failed to update inheritsFrom in dependent instance {:?}: {}",
                    dep.json_path, e
                )
            })?;
        tx.rollback.push(RollbackStep::RestoreFile {
            path: dep.json_path,
            orig_bytes: dep.orig_bytes,
        });
    }

    Ok(())
}

async fn commit_rename_plan(
    old_dir: &std::path::Path,
    new_dir: &std::path::Path,
    old_id: &str,
    new_id: &str,
    is_case_only: bool,
    plan: RenameCommitPlan,
) -> Result<(), String> {
    let target = RenameTarget {
        old_dir,
        new_dir,
        old_id,
        new_id,
        is_case_only,
    };
    let mut tx = RenameTxState::default();

    if let Err(err) = apply_rename_plan_steps(&target, plan, &mut tx).await {
        let rollback_errors = execute_rollback(tx.rollback).await;
        if rollback_errors.is_empty() {
            return Err(err);
        }
        return Err(format!(
            "{} (rollback errors: {})",
            err,
            rollback_errors.join("; ")
        ));
    }

    for path in tx.cleanup_files {
        if let Err(e) = tokio::fs::remove_file(&path).await {
            tracing::warn!(
                "Failed to remove temporary rename backup {:?}: {}",
                path,
                e
            );
        }
    }

    Ok(())
}

/// Core filesystem logic for renaming an instance directory and updating its internal descriptors.
pub async fn rename_instance_in_dir(
    versions_dir: &std::path::Path,
    old_id: &str,
    new_id: &str,
) -> Result<(), String> {
    let new_id = new_id.trim();
    if old_id.is_empty() || new_id.is_empty() {
        return Err("Instance name cannot be empty".to_string());
    }
    if old_id == new_id {
        return Ok(());
    }

    if !crate::core::fs_utils::is_safe_or_existing_instance_id(old_id, versions_dir).await {
        return Err(format!("Invalid instance name: {}", old_id));
    }
    if !crate::core::fs_utils::is_safe_single_name(new_id) {
        return Err(format!("Invalid instance name: {}", new_id));
    }

    let old_dir = versions_dir.join(old_id);
    let old_meta = tokio::fs::metadata(&old_dir)
        .await
        .map_err(|_| format!("Instance {} not found", old_id))?;
    if !old_meta.is_dir() {
        return Err(format!("Instance {} not found", old_id));
    }

    let new_dir = versions_dir.join(new_id);
    let is_case_only = old_id.eq_ignore_ascii_case(new_id);

    // Check case-insensitive name collisions against other entries in versions_dir
    let mut entries = tokio::fs::read_dir(versions_dir)
        .await
        .map_err(|e| format!("Failed to read versions directory: {}", e))?;
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|e| format!("Failed to read versions entry: {}", e))?
    {
        let existing_name = entry.file_name().to_string_lossy().to_string();
        if existing_name != old_id && existing_name.eq_ignore_ascii_case(new_id) {
            return Err(format!("Instance with name '{}' already exists", new_id));
        }
    }
    if !is_case_only
        && tokio::fs::try_exists(&new_dir)
            .await
            .map_err(|e| format!("Failed to check target instance directory: {}", e))?
    {
        return Err(format!("Instance with name '{}' already exists", new_id));
    }

    let plan = prepare_rename_commit_plan(versions_dir, &old_dir, &new_dir, old_id, new_id).await?;
    commit_rename_plan(&old_dir, &new_dir, old_id, new_id, is_case_only, plan).await
}

/// Rename an existing instance (directory, core metadata files, statistics, and crash history).
#[tauri::command]
pub async fn rename_instance(
    old_id: String,
    new_id: String,
    app: tauri::AppHandle,
    task_manager: tauri::State<'_, crate::core::task::TaskManager>,
    running_instances: tauri::State<'_, crate::core::launcher::RunningInstances>,
) -> Result<(), String> {
    let new_id = new_id.trim().to_string();
    if old_id.is_empty() || new_id.is_empty() {
        return Err("Instance name cannot be empty".to_string());
    }
    if old_id == new_id {
        return Ok(());
    }

    let base_dir = get_minecraft_base();
    let versions_dir = base_dir.join("versions");

    if !crate::core::fs_utils::is_safe_or_existing_instance_id(&old_id, &versions_dir).await {
        return Err(format!("Invalid instance name: {}", old_id));
    }
    if !crate::core::fs_utils::is_safe_single_name(&new_id) {
        return Err(format!("Invalid instance name: {}", new_id));
    }

    let old_dir = versions_dir.join(&old_id);
    let new_dir = versions_dir.join(&new_id);
    if !tokio::fs::try_exists(&old_dir).await.unwrap_or(false) {
        return Err(format!("Instance {} not found", old_id));
    }

    if running_instances.is_instance_running(&old_id).await {
        return Err(format!(
            "Cannot rename instance {} because it is currently running",
            old_id
        ));
    }
    if !old_id.eq_ignore_ascii_case(&new_id) && running_instances.is_instance_running(&new_id).await {
        return Err(format!(
            "Cannot rename to {} because an instance with that name is currently running",
            new_id
        ));
    }

    if check_instance_busy(&old_id, &old_dir, &task_manager).await {
        return Err(format!(
            "Cannot rename instance {} while it is installing or updating",
            old_id
        ));
    }
    if check_instance_busy(&new_id, &new_dir, &task_manager).await {
        return Err(format!(
            "Cannot rename to {} while another task for that name is active",
            new_id
        ));
    }

    // Check dependent instances if any are currently running
    for dep_id in find_dependent_instances(&versions_dir, &old_id).await? {
        if running_instances.is_instance_running(&dep_id).await {
            return Err(format!(
                "Cannot rename instance {} while dependent instance {} is running",
                old_id, dep_id
            ));
        }
    }

    // Acquire per-instance locks in deterministic lexicographical order to prevent deadlocks
    let (first_key, second_key) = if old_id < new_id {
        (&old_id, &new_id)
    } else {
        (&new_id, &old_id)
    };
    let first_lock = crate::core::drop_install::get_instance_lock(first_key);
    let second_lock = crate::core::drop_install::get_instance_lock(second_key);
    let _first_guard = first_lock.lock().await;
    let _second_guard = second_lock.lock().await;

    // Re-verify runtime & task state after acquiring locks
    if running_instances.is_instance_running(&old_id).await {
        return Err(format!(
            "Cannot rename instance {} because it is currently running",
            old_id
        ));
    }
    if check_instance_busy(&old_id, &old_dir, &task_manager).await {
        return Err(format!(
            "Cannot rename instance {} while it is installing or updating",
            old_id
        ));
    }
    if check_instance_busy(&new_id, &new_dir, &task_manager).await {
        return Err(format!(
            "Cannot rename to {} while another task for that name is active",
            new_id
        ));
    }

    rename_instance_in_dir(&versions_dir, &old_id, &new_id).await?;

    invalidate_instance_scan_cache().await;
    crate::core::instance_icon::handle_instance_renamed(&old_id, &new_id);

    if let Some(stats_db) = app.try_state::<crate::core::statistics::StatisticsDb>() {
        if let Err(e) = stats_db.rename_stats(old_id.clone(), new_id.clone()).await {
            tracing::warn!(
                "Failed to migrate statistics during rename ({} -> {}): {}",
                old_id,
                new_id,
                e
            );
        }
    }

    if let Some(crash_db) = app.try_state::<crate::core::crash_history::CrashHistoryDb>() {
        if let Err(e) = crash_db.rename_instance(old_id.clone(), new_id.clone()).await {
            tracing::warn!(
                "Failed to migrate crash history during rename ({} -> {}): {}",
                old_id,
                new_id,
                e
            );
        }
    }

    tracing::info!("Renamed instance: {} -> {}", old_id, new_id);
    Ok(())
}

pub async fn instance_has_data_async(instance_dir: &std::path::Path) -> bool {
    let data_folders = [
        "saves",
        "resourcepacks",
        "shaderpacks",
        "screenshots",
        "servers.dat",
    ];
    for folder in &data_folders {
        if tokio::fs::try_exists(instance_dir.join(folder)).await.unwrap_or(false) {
            return true;
        }
    }
    false
}

#[tauri::command]
pub async fn check_instance_data(version_id: String) -> Result<bool, String> {
    let base_dir = get_minecraft_base();
    let instance_dir = base_dir.join("versions").join(&version_id);
    Ok(instance_has_data_async(&instance_dir).await)
}

// ============================================================================
// Local Mod Management
// ============================================================================

/// Represents a local mod file
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalModItem {
    /// Filename of the mod
    pub filename: String,
    /// Whether the mod is enabled (not .jar.disabled)
    pub enabled: bool,
    /// File size in bytes
    pub size: u64,
    pub mod_id: Option<String>,
    pub name: Option<String>,
    pub version: Option<String>,
    pub icon_url: Option<String>,
    pub managed_by_modpack: bool,
    pub provides: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct LocalDatapackItem {
    pub filename: String,
    pub is_dir: bool,
    pub size: u64,
    pub managed_by_modpack: bool,
}

#[tauri::command]
pub async fn get_installed_datapacks(version_id: String, world_name: String) -> Result<Vec<LocalDatapackItem>, String> {
    let base_dir = get_minecraft_base();
    let datapacks_dir = base_dir.join("versions").join(&version_id).join("saves").join(&world_name).join("datapacks");

    if !tokio::fs::try_exists(&datapacks_dir).await.unwrap_or(false) {
        return Ok(Vec::new());
    }
    
    let mut managed_files = std::collections::HashMap::new();
    let assets_path = base_dir.join("versions").join(&version_id).join("assets.json");
    let prefix = format!("saves/{}/datapacks/", world_name);
    if let Ok(content) = tokio::fs::read_to_string(&assets_path).await {
        if let Ok(manifest) = serde_json::from_str::<crate::models::instance::AssetManifest>(&content) {
            for (path_key, record) in manifest.assets {
                if path_key.starts_with(&prefix) {
                    if let Some(filename) = std::path::Path::new(&path_key).file_name() {
                        let base_name = filename.to_string_lossy().to_string();
                        managed_files.insert(base_name, record.managed_by_modpack);
                    }
                }
            }
        }
    }

    let mut datapacks = Vec::new();
    let mut entries = tokio::fs::read_dir(&datapacks_dir)
        .await
        .map_err(|e| format!("Failed to read datapacks directory: {}", e))?;

    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|e| format!("Failed to read directory entry: {}", e))?
    {
        let path = entry.path();
        let filename = entry.file_name().to_string_lossy().to_string();
        let metadata = tokio::fs::metadata(&path)
            .await
            .map_err(|e| e.to_string())?;

        let is_dir = metadata.is_dir();
        
        // Basic filtering
        if !is_dir && !filename.ends_with(".zip") {
            continue;
        }

        let managed = managed_files.get(&filename).copied().unwrap_or(false);

        datapacks.push(LocalDatapackItem {
            filename,
            is_dir,
            size: metadata.len(),
            managed_by_modpack: managed,
        });
    }

    Ok(datapacks)
}

#[tauri::command]
pub async fn delete_local_datapack(version_id: String, world_name: String, filename: String) -> Result<(), String> {
    if !crate::core::fs_utils::is_safe_single_name(&version_id)
        || !crate::core::fs_utils::is_safe_single_name(&world_name)
        || !crate::core::fs_utils::is_safe_single_name(&filename)
    {
        return Err("Invalid instance, world, or datapack filename".to_string());
    }

    let base_dir = get_minecraft_base();
    let datapacks_dir = base_dir.join("versions").join(&version_id).join("saves").join(&world_name).join("datapacks");
    let target = datapacks_dir.join(&filename);

    if tokio::fs::try_exists(&target).await.unwrap_or(false) {
        let metadata = tokio::fs::metadata(&target).await.map_err(|e| e.to_string())?;
        if metadata.is_dir() {
            tokio::fs::remove_dir_all(&target).await.map_err(|e| e.to_string())?;
        } else {
            tokio::fs::remove_file(&target).await.map_err(|e| e.to_string())?;
        }
    }
    
    delete_asset_from_manifest(&version_id, &format!("saves/{}/datapacks/{}", world_name, filename)).await;
    // Fallback for ghost records created with backslashes due to a path serialization bug
    delete_asset_from_manifest(&version_id, &format!("saves\\{}\\datapacks\\{}", world_name, filename)).await;
    
    Ok(())
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalAssetItem {
    pub filename: String,
    pub is_dir: bool,
    pub size: u64,
    pub managed_by_modpack: bool,
}

async fn get_assets_in_dir(dir: std::path::PathBuf, version_id: &str, prefix: &str) -> Result<Vec<LocalAssetItem>, String> {
    if !tokio::fs::try_exists(&dir).await.unwrap_or(false) {
        return Ok(Vec::new());
    }

    let mut managed_files = std::collections::HashMap::new();
    let base_dir = get_minecraft_base();
    let assets_path = base_dir.join("versions").join(version_id).join("assets.json");
    if let Ok(content) = tokio::fs::read_to_string(&assets_path).await {
        if let Ok(manifest) = serde_json::from_str::<crate::models::instance::AssetManifest>(&content) {
            for (path_key, record) in manifest.assets {
                if path_key.starts_with(prefix) {
                    if let Some(filename) = std::path::Path::new(&path_key).file_name() {
                        let base_name = filename.to_string_lossy().to_string();
                        managed_files.insert(base_name, record.managed_by_modpack);
                    }
                }
            }
        }
    }

    let mut assets = Vec::new();
    let mut entries = tokio::fs::read_dir(&dir).await.map_err(|e| e.to_string())?;
    while let Some(entry) = entries.next_entry().await.map_err(|e| e.to_string())? {
        let path = entry.path();
        let filename = entry.file_name().to_string_lossy().to_string();
        let metadata = tokio::fs::metadata(&path).await.map_err(|e| e.to_string())?;
        
        let mut size = metadata.len();
        if metadata.is_dir() {
            // Very naive size calculation for directories, could be slow for huge worlds
            // For now, we skip deep recursion to avoid blocking, returning 0
            size = 0;
        }

        let managed = managed_files.get(&filename).copied().unwrap_or(false);

        assets.push(LocalAssetItem {
            filename,
            is_dir: metadata.is_dir(),
            size,
            managed_by_modpack: managed,
        });
    }
    Ok(assets)
}

async fn delete_asset_in_dir(dir: std::path::PathBuf, filename: String) -> Result<(), String> {
    if filename.contains('/') || filename.contains('\\') || filename == ".." || filename == "." {
        return Err("Invalid filename".to_string());
    }
    let target = dir.join(&filename);
    if tokio::fs::try_exists(&target).await.unwrap_or(false) {
        let metadata = tokio::fs::metadata(&target).await.map_err(|e| e.to_string())?;
        if metadata.is_dir() {
            tokio::fs::remove_dir_all(&target).await.map_err(|e| e.to_string())?;
        } else {
            tokio::fs::remove_file(&target).await.map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn get_installed_resourcepacks(version_id: String) -> Result<Vec<LocalAssetItem>, String> {
    let dir = get_minecraft_base().join("versions").join(&version_id).join("resourcepacks");
    get_assets_in_dir(dir, &version_id, "resourcepacks/").await
}

#[tauri::command]
pub async fn delete_local_resourcepack(version_id: String, filename: String) -> Result<(), String> {
    let dir = get_minecraft_base().join("versions").join(&version_id).join("resourcepacks");
    delete_asset_in_dir(dir, filename.clone()).await?;
    delete_asset_from_manifest(&version_id, &format!("resourcepacks/{}", filename)).await;
    Ok(())
}

#[tauri::command]
pub async fn get_installed_shaders(version_id: String) -> Result<Vec<LocalAssetItem>, String> {
    let dir = get_minecraft_base().join("versions").join(&version_id).join("shaderpacks");
    get_assets_in_dir(dir, &version_id, "shaderpacks/").await
}

#[tauri::command]
pub async fn delete_local_shader(version_id: String, filename: String) -> Result<(), String> {
    let dir = get_minecraft_base().join("versions").join(&version_id).join("shaderpacks");
    delete_asset_in_dir(dir, filename.clone()).await?;
    delete_asset_from_manifest(&version_id, &format!("shaderpacks/{}", filename)).await;
    // Fallback cleanup for the previous bug where shaders were incorrectly registered as resourcepacks
    delete_asset_from_manifest(&version_id, &format!("resourcepacks/{}", filename)).await;
    Ok(())
}

#[tauri::command]
pub async fn get_installed_worlds(version_id: String) -> Result<Vec<LocalAssetItem>, String> {
    let dir = get_minecraft_base().join("versions").join(&version_id).join("saves");
    get_assets_in_dir(dir, &version_id, "saves/").await
}

#[tauri::command]
pub async fn delete_local_world(version_id: String, world_name: String) -> Result<(), String> {
    let dir = get_minecraft_base().join("versions").join(&version_id).join("saves");
    delete_asset_in_dir(dir, world_name.clone()).await?;
    delete_asset_from_manifest(&version_id, &format!("saves/{}", world_name)).await;
    Ok(())
}

// Global custom assets logic
#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct OnlinePreset {
    pub name: String,
    pub mods: Vec<PresetMod>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct PresetMod {
    pub source: String, // "modrinth" or "curseforge"
    pub project_id: String,
    pub name: String,
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct ResolvedPresetMod {
    pub source: String,
    pub project_id: String,
    pub project_name: String,
    pub file_id: String,
    pub filename: String,
    pub download_url: String,
    #[serde(default)]
    pub hash: Option<String>,
    #[serde(default)]
    pub file_size: Option<u64>,
    pub dependencies: Option<Vec<crate::core::modrinth::UnifiedDependency>>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct ResolvePresetResult {
    pub resolved_mods: Vec<ResolvedPresetMod>,
    pub failed_mods: Vec<PresetMod>,
}

#[tauri::command]
pub async fn get_custom_assets(asset_type: String) -> Result<Vec<LocalAssetItem>, String> {
    if !["mod_groups", "shaderpacks", "resourcepacks"].contains(&asset_type.as_str()) {
        return Err("Invalid asset type".to_string());
    }
    let dir = get_minecraft_base().join("global_assets").join(&asset_type);
    tokio::fs::create_dir_all(&dir).await.map_err(|e| e.to_string())?;

    let mut results = Vec::new();
    let mut entries = tokio::fs::read_dir(&dir).await.map_err(|e| e.to_string())?;
    
    while let Ok(Some(entry)) = entries.next_entry().await {
        let file_type = match entry.file_type().await {
            Ok(ft) => ft,
            Err(_) => continue,
        };
        let filename = entry.file_name().to_string_lossy().to_string();
        if filename.starts_with('.') {
            continue;
        }

        let is_dir = file_type.is_dir();
        let path = entry.path();
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
        let is_valid = if is_dir {
            // Directories ending in .json are not valid presets or asset folders
            if ext == "json" {
                false
            } else {
                matches!(asset_type.as_str(), "shaderpacks" | "resourcepacks")
            }
        } else {
            match asset_type.as_str() {
                "mod_groups" => ext == "json" || ext == "jar",
                "shaderpacks" | "resourcepacks" => ext == "json" || ext == "zip",
                _ => false,
            }
        };

        if is_valid {
            let size_bytes = if is_dir {
                0
            } else {
                entry.metadata().await.map(|m| m.len()).unwrap_or(0)
            };

            results.push(LocalAssetItem {
                filename,
                is_dir,
                size: size_bytes,
                managed_by_modpack: false,
            });
        }
    }

    results.sort_by(|a, b| {
        let a_is_json = a.filename.ends_with(".json");
        let b_is_json = b.filename.ends_with(".json");
        match (a_is_json, b_is_json) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.filename.to_lowercase().cmp(&b.filename.to_lowercase()),
        }
    });
    
    Ok(results)
}

#[tauri::command]
pub async fn get_asset_presets(asset_type: String) -> Result<Vec<LocalAssetItem>, String> {
    if !["mod_groups", "shaderpacks", "resourcepacks"].contains(&asset_type.as_str()) {
        return Err("Invalid asset type".to_string());
    }
    let dir = get_minecraft_base().join("global_assets").join(&asset_type);
    tokio::fs::create_dir_all(&dir).await.map_err(|e| e.to_string())?;

    let mut results = Vec::new();
    let mut entries = tokio::fs::read_dir(&dir).await.map_err(|e| e.to_string())?;
    
    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if path.is_file() && path.extension().is_some_and(|e| e == "json") {
            let filename = entry.file_name().to_string_lossy().to_string();
            let metadata = entry.metadata().await.ok();
            let size_bytes = metadata.map(|m| m.len()).unwrap_or(0);
            
            results.push(LocalAssetItem {
                filename,
                is_dir: false,
                size: size_bytes,
                managed_by_modpack: false,
            });
        }
    }
    
    Ok(results)
}

#[tauri::command]
pub async fn delete_custom_asset(asset_type: String, filename: String) -> Result<(), String> {
    if !["mod_groups", "shaderpacks", "resourcepacks"].contains(&asset_type.as_str()) {
        return Err("Invalid asset type".to_string());
    }
    let dir = get_minecraft_base().join("global_assets").join(&asset_type);
    delete_asset_in_dir(dir, filename).await
}

#[tauri::command]
pub async fn open_custom_asset_folder(asset_type: String) -> Result<(), String> {
    if !["mod_groups", "shaderpacks", "resourcepacks"].contains(&asset_type.as_str()) {
        return Err("Invalid asset type".to_string());
    }
    let dir = get_minecraft_base().join("global_assets").join(&asset_type);
    tokio::fs::create_dir_all(&dir).await.map_err(|e| e.to_string())?;
    
    if let Err(e) = open::that(dir) {
        tracing::error!("Failed to open global asset folder: {}", e);
        return Err(e.to_string());
    }
    Ok(())
}

#[tauri::command]
pub async fn apply_custom_asset_to_instance(
    version_id: String,
    asset_type: String,
    filename: String,
) -> Result<(), String> {
    if !["mod_groups", "shaderpacks", "resourcepacks"].contains(&asset_type.as_str()) {
        return Err("Invalid asset type".to_string());
    }
    if !crate::core::fs_utils::is_safe_single_name(&version_id) || !crate::core::fs_utils::is_safe_single_name(&filename) {
        return Err("Invalid instance ID or filename".to_string());
    }

    let base_dir = get_minecraft_base();
    let src_path = base_dir.join("global_assets").join(&asset_type).join(&filename);
    if !src_path.exists() {
        return Err(format!("Source asset '{}' not found in global_assets/{}", filename, asset_type));
    }

    let is_dir = tokio::fs::metadata(&src_path).await.map(|m| m.is_dir()).unwrap_or(false);
    let ext = std::path::Path::new(&filename).extension().and_then(|e| e.to_str()).unwrap_or("");
    if !is_dir && ext.eq_ignore_ascii_case("json") {
        return Err("Preset configuration files cannot be applied as raw assets; use resolve_preset_for_instance".to_string());
    }

    // Coordinate apply operations with instance deletion via per-instance lock
    let instance_lock = crate::core::drop_install::get_instance_lock(&version_id);
    let _instance_guard = instance_lock.lock().await;

    let instance_dir = base_dir.join("versions").join(&version_id);
    if !instance_dir.exists() {
        return Err(format!("Instance '{}' not found", version_id));
    }

    let (folder_name, category) = match asset_type.as_str() {
        "shaderpacks" => ("shaderpacks", crate::models::instance::AssetCategory::ShaderPack),
        "resourcepacks" => ("resourcepacks", crate::models::instance::AssetCategory::ResourcePack),
        "mod_groups" => ("mods", crate::models::instance::AssetCategory::Mod),
        _ => return Err("Unsupported asset type".to_string()),
    };

    let target_dir = instance_dir.join(folder_name);
    tokio::fs::create_dir_all(&target_dir).await.map_err(|e| e.to_string())?;
    let target_path = target_dir.join(&filename);

    if !target_path.starts_with(&target_dir) {
        return Err("Target path escapes asset directory".to_string());
    }

    // Reject destination symlinks
    if let Ok(meta) = tokio::fs::symlink_metadata(&target_path).await {
        if meta.file_type().is_symlink() {
            return Err(format!("Destination '{}' is a symlink and cannot be overwritten", filename));
        }
    }

    // Serialize concurrent writes to target_path across local applies and installs
    let path_lock = get_path_lock(&target_path);
    let _path_guard = path_lock.lock().await;

    if is_dir {
        let temp_dir = target_dir.join(format!(".dir_tmp_{}_{}", filename, uuid::Uuid::new_v4().simple()));
        crate::core::utils::copy_dir_all(&src_path, &temp_dir).await
            .map_err(|e| format!("Failed to copy directory: {}", e))?;
        if target_path.exists() {
            if let Err(e) = tokio::fs::remove_dir_all(&target_path).await {
                let _ = tokio::fs::remove_dir_all(&temp_dir).await;
                return Err(format!("Failed to clean existing directory before replace: {}", e));
            }
        }
        if let Err(e) = tokio::fs::rename(&temp_dir, &target_path).await {
            let _ = tokio::fs::remove_dir_all(&temp_dir).await;
            return Err(format!("Failed to commit replaced directory: {}", e));
        }
    } else {
        if target_path.is_dir() {
            tokio::fs::remove_dir_all(&target_path).await
                .map_err(|e| format!("Failed to remove directory conflicting with target file: {}", e))?;
        }
        let temp_file = target_dir.join(format!(".file_tmp_{}_{}", filename, uuid::Uuid::new_v4().simple()));
        tokio::fs::copy(&src_path, &temp_file).await
            .map_err(|e| format!("Failed to copy file: {}", e))?;
        if target_path.exists() {
            let _ = tokio::fs::remove_file(&target_path).await;
        }
        if let Err(e) = tokio::fs::rename(&temp_file, &target_path).await {
            let _ = tokio::fs::remove_file(&temp_file).await;
            return Err(format!("Failed to commit copied file: {}", e));
        }
    }

    // Register into assets.json
    let rel_key = format!("{}/{}", folder_name, filename);
    let size = if !is_dir {
        tokio::fs::metadata(&target_path).await.map(|m| m.len()).ok()
    } else {
        Some(0)
    };
    let hash_sha1 = if !is_dir {
        let p = target_path.clone();
        tokio::task::spawn_blocking(move || crate::downloader::download::compute_sha1_sync(&p))
            .await
            .ok()
            .and_then(|r| r.ok())
    } else {
        None
    };

    save_asset_to_manifest(&version_id, rel_key, crate::models::instance::AssetRecord {
        category,
        source_type: crate::models::instance::AssetSourceType::LocalImport,
        project_id: None,
        version_id: None,
        download_url: None,
        hash_sha1,
        hash_sha512: None,
        size,
        enabled: true,
        managed_by_modpack: false,
    }).await?;

    Ok(())
}

#[tauri::command]
pub async fn resolve_preset_for_instance(preset_name: String, asset_type: String, version_id: String) -> Result<ResolvePresetResult, String> {
    if !["mod_groups", "shaderpacks", "resourcepacks"].contains(&asset_type.as_str()) {
        return Err("Invalid asset type".to_string());
    }
    
    // Get instance details
    let instance = get_instance_details(version_id.clone()).await?;
    let mc_version = instance.mc_version.clone();
    let loader = instance.loader_type.to_lowercase();
    
    let clean_name = preset_name.replace("/", "").replace("\\", "");
    let clean_name = if clean_name.to_lowercase().ends_with(".json") {
        &clean_name[..clean_name.len() - 5]
    } else {
        &clean_name
    };
    if !crate::core::fs_utils::is_safe_single_name(clean_name) {
        return Err("Invalid preset name".to_string());
    }
    let file_path = get_minecraft_base().join("global_assets").join(&asset_type).join(format!("{}.json", clean_name));
    
    let meta = tokio::fs::metadata(&file_path).await.map_err(|e| e.to_string())?;
    if meta.is_dir() {
        return Err("Preset target is a directory, not a preset JSON file".to_string());
    }

    let preset_content = tokio::fs::read_to_string(&file_path).await.map_err(|e| e.to_string())?;
    let preset: OnlinePreset = serde_json::from_str(&preset_content).map_err(|e| e.to_string())?;
    
    let mut resolved_mods = Vec::new();
    let mut failed_mods = Vec::new();
    let loaders = if asset_type == "mod_groups" {
        vec![loader.clone()]
    } else {
        vec![]
    };

    for pm in preset.mods {
        let files_result = if pm.source == "modrinth" {
            let res = crate::core::modrinth::get_modrinth_mod_files(pm.project_id.clone(), mc_version.clone(), loaders.clone()).await;
            match res {
                Ok(files) if files.is_empty() && asset_type != "mod_groups" => {
                    crate::core::modrinth::get_modrinth_mod_files(pm.project_id.clone(), "".to_string(), vec![]).await
                }
                other => other,
            }
        } else if pm.source == "curseforge" {
            let res = crate::core::curseforge::get_cf_mod_files(pm.project_id.clone(), mc_version.clone(), loaders.clone()).await;
            match res {
                Ok(files) if files.is_empty() && asset_type != "mod_groups" => {
                    crate::core::curseforge::get_cf_mod_files(pm.project_id.clone(), "".to_string(), vec![]).await
                }
                other => other,
            }
        } else {
            tracing::warn!("Unknown source '{}' for mod '{}', skipping", pm.source, pm.name);
            continue;
        };

        match files_result {
            Ok(files) => {
                if let Some(best) = files.into_iter().next() {
                    resolved_mods.push(ResolvedPresetMod {
                        source: pm.source.clone(),
                        project_id: pm.project_id.clone(),
                        project_name: pm.name.clone(),
                        file_id: best.id,
                        filename: best.filename,
                        download_url: best.download_url,
                        hash: best.hash,
                        file_size: best.file_size,
                        dependencies: Some(best.dependencies),
                    });
                } else {
                    failed_mods.push(pm);
                }
            }
            Err(_) => failed_mods.push(pm),
        }
    }
    
    Ok(ResolvePresetResult { resolved_mods, failed_mods })
}

#[tauri::command]
pub async fn download_resolved_preset(
    app: tauri::AppHandle,
    version_id: String,
    resolved_mods: Vec<ResolvedPresetMod>
) -> Result<(), String> {
    for rm in resolved_mods {
        crate::commands::install_mod_to_instance(
            app.clone(),
            crate::core::manager::InstallModOptions {
                source: rm.source,
                project_id: rm.project_id,
                mod_name: None,
                instance_id: Some(version_id.clone()),
                target_dir: None,
                download_url: rm.download_url,
                file_id: rm.file_id,
                dependencies: rm.dependencies,
                keep_both: Some(false),
            }
        ).await.map_err(|e| format!("{:?}", e))?;
    }
    Ok(())
}

#[tauri::command]
pub async fn add_mod_to_preset(preset_name: String, asset_type: String, source: String, project_id: String, project_name: String) -> Result<(), String> {
    if !["mod_groups", "shaderpacks", "resourcepacks"].contains(&asset_type.as_str()) {
        return Err("Invalid asset type".to_string());
    }
    
    let clean_name = preset_name.replace("/", "").replace("\\", "").replace(".json", "");
    if !crate::core::fs_utils::is_safe_single_name(&clean_name) {
        return Err("Invalid preset name".to_string());
    }
    let file_path = get_minecraft_base().join("global_assets").join(&asset_type).join(format!("{}.json", clean_name));
    
    let mut preset = if tokio::fs::try_exists(&file_path).await.unwrap_or(false) {
        let content = tokio::fs::read_to_string(&file_path).await.map_err(|e| e.to_string())?;
        serde_json::from_str::<OnlinePreset>(&content).unwrap_or(OnlinePreset {
            name: preset_name.clone(),
            mods: vec![],
        })
    } else {
        OnlinePreset {
            name: preset_name.clone(),
            mods: vec![],
        }
    };

    if !preset.mods.iter().any(|m| m.project_id == project_id && m.source == source) {
        preset.mods.push(PresetMod { source, project_id, name: project_name });
    }

    let json_content = serde_json::to_string_pretty(&preset).map_err(|e| e.to_string())?;
    tokio::fs::create_dir_all(file_path.parent().unwrap()).await.map_err(|e| e.to_string())?;
    tokio::fs::write(&file_path, json_content).await.map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
pub async fn get_preset_details(asset_type: String, preset_name: String) -> Result<OnlinePreset, String> {
    if !["mod_groups", "shaderpacks", "resourcepacks"].contains(&asset_type.as_str()) {
        return Err("Invalid asset type".to_string());
    }
    
    let clean_name = preset_name.replace("/", "").replace("\\", "").replace(".json", "");
    if !crate::core::fs_utils::is_safe_single_name(&clean_name) {
        return Err("Invalid preset name".to_string());
    }
    let file_path = get_minecraft_base().join("global_assets").join(&asset_type).join(format!("{}.json", clean_name));
    
    match tokio::fs::try_exists(&file_path).await {
        Ok(true) => {},
        Ok(false) => return Err("Preset not found".to_string()),
        Err(e) => return Err(format!("Failed to check if preset exists: {}", e)),
    }
    
    let content = tokio::fs::read_to_string(&file_path).await.map_err(|e| e.to_string())?;
    let preset: OnlinePreset = serde_json::from_str(&content).map_err(|e| e.to_string())?;
    
    Ok(preset)
}

#[tauri::command]
pub async fn update_preset(asset_type: String, preset_name: String, preset: OnlinePreset) -> Result<(), String> {
    if !["mod_groups", "shaderpacks", "resourcepacks"].contains(&asset_type.as_str()) {
        return Err("Invalid asset type".to_string());
    }
    
    let clean_name = preset_name.replace("/", "").replace("\\", "").replace(".json", "");
    if !crate::core::fs_utils::is_safe_single_name(&clean_name) {
        return Err("Invalid preset name".to_string());
    }
    let file_path = get_minecraft_base().join("global_assets").join(&asset_type).join(format!("{}.json", clean_name));
    
    let json_content = serde_json::to_string_pretty(&preset).map_err(|e| e.to_string())?;
    tokio::fs::create_dir_all(file_path.parent().unwrap()).await.map_err(|e| e.to_string())?;
    tokio::fs::write(&file_path, json_content).await.map_err(|e| e.to_string())?;
    
    Ok(())
}



async fn resolve_mod_metadata(
    path: &std::path::Path,
    cache_key: &str,
    cache_entries: &mut std::collections::HashMap<String, crate::core::mod_parser::ModMetadata>,
    parser: &crate::core::mod_parser::ModParser,
    base_dir: &std::path::Path,
    skip_parsing: bool,
    loader_type: Option<String>
) -> crate::core::mod_parser::ModMetadata {
    if let Some(m) = cache_entries.get(cache_key) {
        return m.clone();
    }
    if skip_parsing {
        return crate::core::mod_parser::ModMetadata::default();
    }
    let path_clone = path.to_path_buf();
    let key_clone = cache_key.to_string();
    let base_dir_clone = base_dir.to_path_buf();
    let loader_type_clone = loader_type.clone();
    let m = tokio::task::spawn_blocking(move || {
        let p = crate::core::mod_parser::ModParser::new(&base_dir_clone);
        p.parse_mod(&path_clone, &key_clone, loader_type_clone.as_deref())
    })
    .await
    .unwrap_or_default();

    cache_entries.insert(cache_key.to_string(), m.clone());
    parser.set_cache_entry(cache_key, &m);
    m
}

fn get_mod_icon_url(
    meta: &crate::core::mod_parser::ModMetadata,
    parser: &crate::core::mod_parser::ModParser,
    cache_key: &str,
) -> Option<String> {
    if !meta.has_icon {
        return None;
    }
    let icon_name = meta.mod_id.as_deref().unwrap_or(cache_key);
    let mut p = parser.get_icon_path(icon_name);
    if !p.exists() {
        p = parser.get_icon_path(cache_key);
    }
    Some(p.to_string_lossy().to_string())
}

/// Get list of installed mods for a specific instance
#[tauri::command]
pub async fn get_installed_mods(version_id: String, skip_parsing: Option<bool>) -> Result<Vec<LocalModItem>, String> {
    tracing::info!("Getting installed mods for instance: {}", version_id);

    if !crate::core::fs_utils::is_safe_single_name(&version_id) {
        return Err("Invalid instance ID".to_string());
    }
    let base_dir = get_minecraft_base();
    let mods_dir = base_dir.join("versions").join(&version_id).join("mods");

    if !mods_dir.exists() {
        tracing::info!("Mods directory does not exist for {}", version_id);
        return Ok(Vec::new());
    }

    let instance = match crate::core::manager::get_instance_details(version_id.clone()).await {
        Ok(i) => i,
        Err(_) => return Ok(Vec::new()),
    };
    let loader_type = instance.loader_type.clone();

    let mut mods = Vec::new();
    let mut managed_files = std::collections::HashMap::new();
    let assets_path = base_dir.join("versions").join(&version_id).join("assets.json");
    if let Ok(content) = tokio::fs::read_to_string(&assets_path).await {
        if let Ok(manifest) = serde_json::from_str::<crate::models::instance::AssetManifest>(&content) {
            for (path_key, record) in manifest.assets {
                if let Some(filename) = std::path::Path::new(&path_key).file_name() {
                    let base_name = filename.to_string_lossy().to_string().trim_end_matches(".disable").to_string();
                    managed_files.insert(base_name, record.managed_by_modpack);
                }
            }
        }
    }

    let mut entries = tokio::fs::read_dir(&mods_dir)
        .await
        .map_err(|e| format!("Failed to read mods directory: {}", e))?;

    let parser = crate::core::mod_parser::ModParser::new(base_dir);
    let mut cache_entries = parser.load_all_cache();

    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|e| format!("Failed to read directory entry: {}", e))?
    {
        let path = entry.path();
        let filename = entry.file_name().to_string_lossy().to_string();

        let metadata = tokio::fs::metadata(&path)
            .await
            .map_err(|e| e.to_string())?;

        let mut actual_filename = filename.clone();
        let mut enabled = true;

        if filename.ends_with(".jar.disable") {
            actual_filename = filename.trim_end_matches(".disable").to_string();
            enabled = false;
        } else if !filename.ends_with(".jar") {
            continue;
        }

        let mtime = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let cache_key = format!("{}_{}_{}_{}", actual_filename, metadata.len(), mtime, loader_type);

        let meta = resolve_mod_metadata(&path, &cache_key, &mut cache_entries, &parser, base_dir, skip_parsing.unwrap_or(false), Some(loader_type.clone())).await;
        let icon_url = get_mod_icon_url(&meta, &parser, &cache_key);

        let is_managed = managed_files.get(&actual_filename).copied().unwrap_or(false);

        mods.push(LocalModItem {
            filename: actual_filename,
            enabled,
            size: metadata.len(),
            mod_id: meta.mod_id,
            name: meta.name,
            version: meta.version,
            icon_url,
            managed_by_modpack: is_managed,
            provides: meta.provides,
        });
    }

    // Sort by filename
    mods.sort_by(|a, b| a.filename.cmp(&b.filename));

    tracing::info!("Found {} mods for instance {}", mods.len(), version_id);
    Ok(mods)
}

/// Toggle mod enabled/disabled status
#[tauri::command]
pub async fn toggle_mod_status(
    version_id: String,
    filename: String,
    enable: bool,
) -> Result<(), String> {
    tracing::info!(
        "Toggling mod {} for instance {} to enabled={}",
        filename,
        version_id,
        enable
    );

    if !crate::core::fs_utils::is_safe_single_name(&version_id) {
        return Err("Invalid instance ID".to_string());
    }
    let base_dir = get_minecraft_base();
    let mods_dir = base_dir.join("versions").join(&version_id).join("mods");
    if !crate::core::fs_utils::is_safe_single_name(&filename) {
        return Err("Invalid mod filename".to_string());
    }
    let enabled_file = mods_dir.join(&filename);
    let disabled_file = mods_dir.join(format!("{}.disable", filename));

    let (src_file, dst_file) = if enable {
        (disabled_file, enabled_file)
    } else {
        (enabled_file, disabled_file)
    };

    if !src_file.exists() {
        if dst_file.exists() {
            // Already in desired state, but we should still ensure assets.json is updated below
        } else {
            return Err(format!("Mod file not found: {}", src_file.display()));
        }
    } else {
        if dst_file.exists() {
            return Err(format!(
                "Mod already exists in target location: {}",
                dst_file.display()
            ));
        }

        tokio::fs::rename(&src_file, &dst_file)
            .await
            .map_err(|e| format!("Failed to move mod file: {}", e))?;
    }

    // Also update the enabled status in assets.json
    let assets_path = base_dir.join("versions").join(&version_id).join("assets.json");
    if assets_path.exists() {
        let _lock = MANIFEST_LOCK.lock().await;
        let content = tokio::fs::read_to_string(&assets_path)
            .await
            .map_err(|e| format!("Failed to read assets.json: {}", e))?;
        let mut manifest = serde_json::from_str::<crate::models::instance::AssetManifest>(&content)
            .map_err(|e| format!("Failed to parse assets.json: {}", e))?;
        let path_key = format!("mods/{}", filename);
        if let Some(record) = manifest.assets.get_mut(&path_key) {
            record.enabled = enable;
            let json = serde_json::to_string_pretty(&manifest)
                .map_err(|e| format!("Failed to serialize assets.json: {}", e))?;
            let temp_assets_path = base_dir
                .join("versions")
                .join(&version_id)
                .join(format!(".assets_{}.json.tmp", uuid::Uuid::new_v4().simple()));
            tokio::fs::write(&temp_assets_path, json)
                .await
                .map_err(|e| format!("Failed to write assets.json temp: {}", e))?;
            tokio::fs::rename(&temp_assets_path, &assets_path)
                .await
                .map_err(|e| format!("Failed to commit assets.json: {}", e))?;
        }
    }

    tracing::info!(
        "Toggled mod {} to enabled={} for instance {}",
        filename,
        enable,
        version_id
    );
    Ok(())
}

/// Disable all enabled mods whose name/id/filename fuzzy-matches the given mod_name.
/// Returns the display names of disabled mods. Used by the AI crash analysis one-click fix.
#[tauri::command]
pub async fn disable_mod_by_name(
    version_id: String,
    mod_name: String,
) -> Result<Vec<String>, String> {
    tracing::info!("Disabling mods matching '{}' in instance {}", mod_name, version_id);

    let mods = get_installed_mods(version_id.clone(), None).await?;
    let needle = mod_name.to_lowercase();
    let mut disabled = Vec::new();

    for m in &mods {
        if !m.enabled {
            continue;
        }

        let name_match = m.name.as_ref().is_some_and(|n| n.to_lowercase().contains(&needle));
        let id_match = m.mod_id.as_ref().is_some_and(|i| i.to_lowercase().contains(&needle));
        let file_match = m.filename.to_lowercase().contains(&needle);

        if name_match || id_match || file_match {
            match toggle_mod_status(version_id.clone(), m.filename.clone(), false).await {
                Ok(_) => {
                    let mod_display = m.name.clone().unwrap_or_else(|| m.filename.clone());
                    tracing::info!("Disabled mod '{}' (matched '{}')", mod_display, mod_name);
                    disabled.push(mod_display);
                }
                Err(e) => {
                    tracing::warn!("Failed to disable mod '{}': {}", m.filename, e);
                }
            }
        }
    }

    if disabled.is_empty() {
        return Err(format!(
            "No enabled mod matching '{}' was found in instance {}",
            mod_name, version_id
        ));
    }

    Ok(disabled)
}

/// A single warning from a prelaunch mod dependency check.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrelaunchWarning {
    pub kind: String,
    pub dependency_id: String,
    pub required_by: String,
}

/// Result of a prelaunch check: list of warnings and total mod count.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrelaunchCheckResult {
    pub warnings: Vec<PrelaunchWarning>,
    pub mod_count: usize,
}

/// Scan installed mods and detect missing dependencies before launching.
#[tauri::command]
pub async fn prelaunch_check(version_id: String) -> Result<PrelaunchCheckResult, String> {
    tracing::info!("Running prelaunch check for instance: {}", version_id);

    if !crate::core::fs_utils::is_safe_single_name(&version_id) {
        return Err("Invalid instance ID".to_string());
    }
    let base_dir = get_minecraft_base();
    let mods_dir = base_dir.join("versions").join(&version_id).join("mods");

    if !mods_dir.exists() {
        return Ok(PrelaunchCheckResult { warnings: vec![], mod_count: 0 });
    }

    // Collect enabled .jar file paths based on installed mod metadata
    let installed_mods = get_installed_mods(version_id.clone(), Some(true))
        .await
        .map_err(|e| format!("Failed to get installed mods: {}", e))?;

    let mut jar_entries = Vec::new();
    for mod_entry in installed_mods.into_iter().filter(|m| m.enabled) {
        jar_entries.push((mod_entry.filename.clone(), mods_dir.join(&mod_entry.filename)));
    }

    if jar_entries.is_empty() {
        return Ok(PrelaunchCheckResult { warnings: vec![], mod_count: 0 });
    }

    let instance = match get_instance_details(version_id.clone()).await {
        Ok(i) => i,
        Err(_) => return Ok(PrelaunchCheckResult { warnings: vec![], mod_count: 0 }),
    };
    let loader_type = instance.loader_type.clone();

    // Parse all mod metadata in a blocking thread (parse_mod uses sync IO)
    let base_dir_for_parser = base_dir.clone();
    let metas = tokio::task::spawn_blocking(move || {
        let parser = crate::core::mod_parser::ModParser::new(&base_dir_for_parser);
        let mut cache = parser.load_all_cache();
        jar_entries.iter().map(|(filename, path)| {
            let (size, mtime) = if let Ok(metadata) = std::fs::metadata(path) {
                let mt = metadata.modified().ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                (metadata.len(), mt)
            } else {
                (0, 0)
            };
            let cache_key = format!("{}_{}_{}_{}", filename, size, mtime, loader_type);

            if let Some(cached_meta) = cache.get(&cache_key) {
                cached_meta.clone()
            } else {
                let parsed_meta = parser.parse_mod(path, &cache_key, Some(&loader_type));
                parser.set_cache_entry(&cache_key, &parsed_meta);
                cache.insert(cache_key, parsed_meta.clone());
                parsed_meta
            }
        }).collect::<Vec<_>>()
    })
    .await
    .map_err(|e| format!("Blocking task failed: {}", e))?;

    // Collect installed mod ids (lowercase for case-insensitive matching)
    let mut installed_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
    for meta in &metas {
        if let Some(ref id) = meta.mod_id {
            installed_ids.insert(id.to_lowercase());
        }
        for provided in &meta.provides {
            installed_ids.insert(provided.to_lowercase());
        }
    }

    // Platform-level dependencies that are always present
    const PLATFORM_DEPS: &[&str] = &[
        "minecraft", "java", "fabricloader", "quilt_loader", "quiltloader",
        "forge", "neoforge", "minecraftforge",
    ];

    let mut warnings = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    for meta in &metas {
        let display_name = meta.name.clone()
            .or_else(|| meta.mod_id.clone())
            .unwrap_or_default();

        for dep in &meta.depends {
            let dep_lower = dep.to_lowercase();
            if PLATFORM_DEPS.contains(&dep_lower.as_str()) {
                continue;
            }
            if !installed_ids.contains(&dep_lower) {
                let key = format!("{}|{}", dep_lower, display_name);
                if seen.insert(key) {
                    warnings.push(PrelaunchWarning {
                        kind: "missing_dependency".to_string(),
                        dependency_id: dep.clone(),
                        required_by: display_name.clone(),
                    });
                }
            }
        }
    }

    tracing::info!("Prelaunch check: {} mods, {} warnings", metas.len(), warnings.len());
    Ok(PrelaunchCheckResult { warnings, mod_count: metas.len() })
}

/// Delete a local mod file
#[tauri::command]
pub async fn delete_local_mod(
    version_id: String,
    filename: String,
    is_enabled: Option<bool>,
) -> Result<(), String> {
    tracing::info!("Deleting mod {} from instance {}", filename, version_id);

    if !crate::core::fs_utils::is_safe_single_name(&version_id) {
        return Err("Invalid instance ID".to_string());
    }
    let base_dir = get_minecraft_base();
    let mods_dir = base_dir.join("versions").join(&version_id).join("mods");

    if !crate::core::fs_utils::is_safe_single_name(&filename) {
        return Err("Invalid mod filename".to_string());
    }
    let mod_file = mods_dir.join(&filename);
    let disabled_file = mods_dir.join(format!("{}.disable", filename));

    let (primary, fallback) = match is_enabled {
        Some(true) => (&mod_file, &disabled_file),
        Some(false) => (&disabled_file, &mod_file),
        None => (&mod_file, &disabled_file),
    };

    if primary.exists() {
        tokio::fs::remove_file(primary)
            .await
            .map_err(|e| format!("Failed to delete mod file {}: {}", primary.display(), e))?;
    } else if fallback.exists() {
        tokio::fs::remove_file(fallback)
            .await
            .map_err(|e| format!("Failed to delete mod file {}: {}", fallback.display(), e))?;
    } else {
        return Err(format!(
            "Mod file not found for '{}' in instance '{}'",
            filename, version_id
        ));
    }

    // Only remove from assets.json if both the enabled and disabled variants are gone
    if !mod_file.exists() && !disabled_file.exists() {
        delete_asset_from_manifest(&version_id, &format!("mods/{}", filename)).await;
    }

    tracing::info!("Deleted mod {} from instance {}", filename, version_id);
    Ok(())
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModInstallProgress {
    event: String,
    filename: String,
    content_length: Option<u64>,
    chunk_length: Option<usize>,
}

async fn download_mod_file_stream(
    app: &tauri::AppHandle,
    client: &reqwest::Client,
    url: &str,
    target_path: &std::path::Path,
    filename: &str,
) -> Result<(), String> {
    use futures_util::StreamExt;
    use tauri::Emitter;
    use tokio::io::AsyncWriteExt;

    let mut response = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("Failed to download mod: {}", e))?;

    if !response.status().is_success() {
        return Err(format!(
            "Download failed with status: {}",
            response.status()
        ));
    }

    let content_length = response.content_length();

    let _ = app.emit(
        "mod-install-progress",
        ModInstallProgress {
            event: "Started".to_string(),
            filename: filename.to_string(),
            content_length,
            chunk_length: None,
        },
    );

    let mut file = tokio::fs::File::create(target_path)
        .await
        .map_err(|e| format!("Failed to create mod file: {}", e))?;

    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| format!("Chunk error: {}", e))?
    {
        file.write_all(&chunk)
            .await
            .map_err(|e| format!("Write error: {}", e))?;

        let _ = app.emit(
            "mod-install-progress",
            ModInstallProgress {
                event: "Progress".to_string(),
                filename: filename.to_string(),
                content_length: None,
                chunk_length: Some(chunk.len()),
            },
        );
    }

    file.flush()
        .await
        .map_err(|e| format!("Flush error: {}", e))?;

    let _ = app.emit(
        "mod-install-progress",
        ModInstallProgress {
            event: "Finished".to_string(),
            filename: filename.to_string(),
            content_length: None,
            chunk_length: None,
        },
    );

    Ok(())
}

fn extract_filename_from_url(url: &str, project_id: &str) -> String {
    let url_filename = url
        .split('/')
        .next_back()
        .unwrap_or_default()
        .split('?')
        .next()
        .unwrap_or_default()
        .to_string();

    let decoded = urlencoding::decode(&url_filename)
        .map(|c| c.into_owned())
        .unwrap_or(url_filename);

    let leaf = decoded
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .trim();

    let safe_project_id: String = project_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    let fallback_stem = if safe_project_id.is_empty() {
        "mod"
    } else {
        safe_project_id.as_str()
    };

    if leaf.is_empty() || !crate::core::fs_utils::is_safe_single_name(leaf) {
        format!("{}.jar", fallback_stem)
    } else if !leaf.contains('.') {
        format!("{}.jar", leaf)
    } else {
        leaf.to_string()
    }
}


pub async fn register_manual_asset_with_metadata(
    instance_id: &str,
    path_key: &str,
    category: crate::models::instance::AssetCategory,
    source_str: &str,
    project_id: &str,
    download_url: &str,
) -> Result<(), String> {
    register_manual_asset_full(instance_id, path_key, category, source_str, project_id, None, download_url).await
}

pub async fn register_manual_asset_full(
    instance_id: &str,
    path_key: &str,
    category: crate::models::instance::AssetCategory,
    source_str: &str,
    project_id: &str,
    version_id: Option<&str>,
    download_url: &str,
) -> Result<(), String> {
    let base_dir = crate::core::mojang::get_minecraft_base();
    let full_path = base_dir.join("versions").join(instance_id).join(path_key);
    
    let size = tokio::fs::metadata(&full_path).await.map(|m| m.len()).ok();
    let hash_sha1 = if full_path.exists() {
        let p = full_path.clone();
        tokio::task::spawn_blocking(move || crate::downloader::download::compute_sha1_sync(&p))
            .await
            .ok()
            .and_then(|r| r.ok())
    } else {
        None
    };

    let source_type = match source_str {
        "modrinth" => crate::models::instance::AssetSourceType::Modrinth,
        "curseforge" => crate::models::instance::AssetSourceType::CurseForge,
        _ => crate::models::instance::AssetSourceType::CustomUrl,
    };
    
    let filename = std::path::Path::new(path_key).file_name().unwrap_or_default().to_string_lossy().to_string();

    save_asset_to_manifest(instance_id, path_key.to_string(), crate::models::instance::AssetRecord {
        category,
        source_type,
        project_id: Some(project_id.to_string()),
        version_id: version_id.map(|v| v.to_string()),
        download_url: Some(download_url.to_string()),
        hash_sha1,
        hash_sha512: None,
        size,
        enabled: !filename.ends_with(".disable"),
        managed_by_modpack: false,
    }).await
}

pub async fn save_asset_to_manifest(instance_id: &str, path_key: String, record: crate::models::instance::AssetRecord) -> Result<(), String> {
    let _lock = MANIFEST_LOCK.lock().await;
    let base_dir = get_minecraft_base();
    let instance_dir = base_dir.join("versions").join(instance_id);
    let assets_path = instance_dir.join("assets.json");
    let mut manifest = if assets_path.exists() {
        let content = tokio::fs::read_to_string(&assets_path)
            .await
            .map_err(|e| format!("Failed to read assets.json: {}", e))?;
        serde_json::from_str::<crate::models::instance::AssetManifest>(&content)
            .map_err(|e| format!("Failed to parse assets.json: {}", e))?
    } else {
        crate::models::instance::AssetManifest::default()
    };
    manifest.assets.insert(path_key, record);
    let json = serde_json::to_string_pretty(&manifest)
        .map_err(|e| format!("Failed to serialize assets.json: {}", e))?;
    let temp_assets_path = instance_dir.join(format!(".assets_{}.json.tmp", uuid::Uuid::new_v4().simple()));
    if let Err(e) = tokio::fs::write(&temp_assets_path, &json).await {
        let _ = tokio::fs::remove_file(&temp_assets_path).await;
        return Err(format!("Failed to write temp assets.json: {}", e));
    }
    if let Err(e) = tokio::fs::rename(&temp_assets_path, &assets_path).await {
        let _ = tokio::fs::remove_file(&temp_assets_path).await;
        return Err(format!("Failed to commit assets.json: {}", e));
    }
    Ok(())
}

pub async fn delete_asset_from_manifest(instance_id: &str, path_key: &str) -> Result<(), String> {
    let _lock = MANIFEST_LOCK.lock().await;
    let base_dir = get_minecraft_base();
    let instance_dir = base_dir.join("versions").join(instance_id);
    let assets_path = instance_dir.join("assets.json");
    if let Ok(content) = tokio::fs::read_to_string(&assets_path).await {
        if let Ok(mut manifest) = serde_json::from_str::<crate::models::instance::AssetManifest>(&content) {
            if manifest.assets.remove(path_key).is_some() {
                if let Ok(json) = serde_json::to_string_pretty(&manifest) {
                    let temp_assets_path = instance_dir.join(format!(".assets_{}.json.tmp", uuid::Uuid::new_v4().simple()));
                    if tokio::fs::write(&temp_assets_path, &json).await.is_ok() {
                        let _ = tokio::fs::rename(&temp_assets_path, &assets_path).await;
                    }
                }
            }
        }
    }
    Ok(())
}

/// Helper function to load installed mods mapping from `assets.json`
async fn load_installed_mods_json(instance_id: &str) -> std::collections::HashMap<String, (String, Option<String>)> {
    let _lock = MANIFEST_LOCK.lock().await;
    let base_dir = get_minecraft_base();
    let assets_path = base_dir.join("versions").join(instance_id).join("assets.json");
    let mut map = std::collections::HashMap::new();
    if let Ok(content) = tokio::fs::read_to_string(&assets_path).await {
        if let Ok(manifest) = serde_json::from_str::<crate::models::instance::AssetManifest>(&content) {
            for (path_key, record) in manifest.assets {
                if let Some(pid) = record.project_id {
                    let source_prefix = match record.source_type {
                        crate::models::instance::AssetSourceType::Modrinth => "modrinth",
                        crate::models::instance::AssetSourceType::CurseForge => "curseforge",
                        _ => "unknown"
                    };
                    let mod_key = format!("{}_{}", source_prefix, pid);
                    if let Some(filename) = std::path::Path::new(&path_key).file_name() {
                        let name_str = filename.to_string_lossy().to_string();
                        map.insert(mod_key, (name_str.trim_end_matches(".disable").to_string(), record.download_url));
                    }
                }
            }
        }
    }
    map
}

async fn save_installed_mods_json(
    instance_id: &str,
    map: &std::collections::HashMap<String, (String, Option<String>)>,
) {
    let _lock = MANIFEST_LOCK.lock().await;
    let base_dir = get_minecraft_base();
    let instance_dir = base_dir.join("versions").join(instance_id);
    let assets_path = instance_dir.join("assets.json");
    let mut manifest = if assets_path.exists() {
        if let Ok(content) = tokio::fs::read_to_string(&assets_path).await {
            serde_json::from_str::<crate::models::instance::AssetManifest>(&content).unwrap_or_default()
        } else {
            crate::models::instance::AssetManifest::default()
        }
    } else {
        crate::models::instance::AssetManifest::default()
    };

    // Remove old versions or uninstalled mods that were tracked by project_id
    let mut to_remove = Vec::new();
    for (path_key, record) in &manifest.assets {
        if record.category == crate::models::instance::AssetCategory::Mod && !record.managed_by_modpack {
            if let Some(pid) = &record.project_id {
                let source_str = match record.source_type {
                    crate::models::instance::AssetSourceType::Modrinth => "modrinth",
                    crate::models::instance::AssetSourceType::CurseForge => "curseforge",
                    _ => "unknown"
                };
                let expected_mod_key = format!("{}_{}", source_str, pid);
            
            if let Some((expected_filename, _)) = map.get(&expected_mod_key) {
                let expected_path_key = format!("mods/{}", expected_filename);
                if path_key != &expected_path_key {
                    to_remove.push(path_key.clone());
                }
            } else {
                to_remove.push(path_key.clone());
            }
            }
        }
    }
    for k in to_remove {
        manifest.assets.remove(&k);
    }

    // Insert or update entries from the map
    for (mod_key, (filename, download_url)) in map {
        let parts: Vec<&str> = mod_key.splitn(2, '_').collect();
        if parts.len() == 2 {
            let source_str = parts[0];
            let pid = parts[1];
            let source_type = match source_str {
                "modrinth" => crate::models::instance::AssetSourceType::Modrinth,
                "curseforge" => crate::models::instance::AssetSourceType::CurseForge,
                _ => crate::models::instance::AssetSourceType::CustomUrl,
            };
            
            let path_key = format!("mods/{}", filename);
            
            let full_path = base_dir.join("versions").join(instance_id).join(&path_key);
            let size = tokio::fs::metadata(&full_path).await.map(|m| m.len()).ok();
            let hash_sha1 = if full_path.exists() {
                let p = full_path.clone();
                tokio::task::spawn_blocking(move || crate::downloader::download::compute_sha1_sync(&p))
                    .await
                    .ok()
                    .and_then(|r| r.ok())
            } else {
                None
            };

            if let Some(existing) = manifest.assets.get_mut(&path_key) {
                existing.project_id = Some(pid.to_string());
                existing.source_type = source_type;
                existing.managed_by_modpack = false;
                if size.is_some() { existing.size = size; }
                if download_url.is_some() { existing.download_url = download_url.clone(); }
                if hash_sha1.is_some() { existing.hash_sha1 = hash_sha1; }
            } else {
                manifest.assets.insert(path_key.clone(), crate::models::instance::AssetRecord {
                    category: crate::models::instance::AssetCategory::Mod,
                    source_type,
                    project_id: Some(pid.to_string()),
                    version_id: None,
                    download_url: download_url.clone(),
                    hash_sha1,
                    hash_sha512: None,
                    size,
                    enabled: !filename.ends_with(".disable"),
                    managed_by_modpack: false,
                });
            }
        }
    }
    
    if let Ok(json) = serde_json::to_string_pretty(&manifest) {
        let temp_assets_path = instance_dir.join(format!(".assets_{}.json.tmp", uuid::Uuid::new_v4().simple()));
        if tokio::fs::write(&temp_assets_path, &json).await.is_ok() {
            let _ = tokio::fs::rename(&temp_assets_path, &assets_path).await;
        }
    }
}

/// Install a mod to a specific instance (download + save)
pub async fn install_mod_to_instance(
    app: tauri::AppHandle,
    options: InstallModOptions,
    ctx: Option<crate::core::task::TaskContext>,
) -> Result<String, String> {
    let version_id = options
        .instance_id
        .ok_or_else(|| "instance_id is required".to_string())?;
    let mod_source = options.source;
    let project_id = options.project_id;
    let file_id = options.file_id;
    let download_url = options.download_url;
    let dependencies = options.dependencies;
    let keep_both = options.keep_both;

    tracing::info!(
        "Installing mod {} from {} to instance {}",
        project_id,
        mod_source,
        version_id
    );

    if !crate::core::fs_utils::is_safe_single_name(&version_id) {
        return Err("Invalid instance ID".to_string());
    }
    let base_dir = get_minecraft_base();
    let mods_dir = base_dir.join("versions").join(&version_id).join("mods");

    // Ensure mods directory exists
    if !mods_dir.exists() {
        tokio::fs::create_dir_all(&mods_dir)
            .await
            .map_err(|e| format!("Failed to create mods directory: {}", e))?;
    }

    let item = crate::core::manager::get_instance_details(version_id.clone())
        .await
        .map_err(|e| format!("Failed to load instance details: {}", e))?;
    let mc_version = item.mc_version;
    let loader = item.loader_type;

    let client = crate::core::utils::get_http_client().clone();

    let raw_filename = extract_filename_from_url(&download_url, &project_id);
    let filename = std::path::Path::new(&raw_filename)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(&raw_filename)
        .to_string();
    if !crate::core::fs_utils::is_safe_single_name(&filename) {
        return Err("Invalid mod filename".to_string());
    }
    let target_path = mods_dir.join(&filename);
    if !target_path.starts_with(&mods_dir) {
        return Err("Target path escapes mods directory".to_string());
    }

    if let Ok(meta) = tokio::fs::symlink_metadata(&target_path).await {
        if meta.file_type().is_symlink() {
            return Err(format!("Destination '{}' is a symlink and cannot be overwritten", filename));
        }
    }

    let path_lock = get_path_lock(&target_path);
    let _path_guard = path_lock.lock().await;

    let mut installed_map = load_installed_mods_json(&version_id).await;
    let mod_key = format!("{}_{}", mod_source, project_id);

    // Remove old version if it exists and filename differs
    let keep_old = keep_both.unwrap_or(false);
    if !keep_old {
        if let Some((old_filename, _)) = installed_map.get(&mod_key) {
            if old_filename != &filename {
                let old_path = mods_dir.join(old_filename);
                if old_path.exists() {
                    let _ = tokio::fs::remove_file(old_path).await;
                }
            }
            let old_path_disabled = mods_dir.join(format!("{}.disable", old_filename));
            if old_path_disabled.exists() {
                let _ = tokio::fs::remove_file(old_path_disabled).await;
            }
        }
    }

    if !target_path.exists() {
        if let Some(ref c) = ctx {
            download_mod_file_task(c, &client, &download_url, &target_path).await?;
        } else {
            download_mod_file_stream(&app, &client, &download_url, &target_path, &filename).await?;
        }
    }

    tracing::info!(
        "Installed mod {} to instance {} (file: {})",
        project_id,
        version_id,
        filename
    );

    // Enforce 1-to-1 mapping: Remove any existing keys that map to this exact filename
    installed_map.retain(|_, (v, _)| v != &filename);
    installed_map.insert(mod_key, (filename.clone(), Some(download_url.clone())));
    save_installed_mods_json(&version_id, &installed_map).await;

    // Await dependency resolution and downloading sequentially
    if let Some(deps) = dependencies {
        let req_deps: Vec<_> = deps.into_iter().filter(|d| d.required).collect();
        if !req_deps.is_empty() {
            tracing::info!(
                "Found {} required dependencies for mod {}. Initiating download.",
                req_deps.len(),
                project_id
            );
            for dep in req_deps {
                let dep_key = format!("{}_{}", mod_source, dep.project_id);

                if mod_source == "modrinth" {
                    if let Some(vid) = dep.version_id {
                        tracing::info!("Fetching Modrinth dependency version: {}", vid);
                        if let Ok(res) = client
                            .get(format!("https://api.modrinth.com/v2/version/{}", vid))
                            .send()
                            .await
                        {
                            if let Ok(json) = res.json::<serde_json::Value>().await {
                                if let Some(files) = json.get("files").and_then(|f| f.as_array()) {
                                    if let Some(primary) = files.first() {
                                        if let (Some(url), Some(fname)) = (
                                            primary.get("url").and_then(|u| u.as_str()),
                                            primary.get("filename").and_then(|f| f.as_str()),
                                        ) {
                                            if let Some((old_fname, _)) = installed_map.get(&dep_key) {
                                                if old_fname != fname {
                                                    let _ = tokio::fs::remove_file(
                                                        mods_dir.join(old_fname),
                                                    )
                                                    .await;
                                                }
                                                let _ = tokio::fs::remove_file(
                                                    mods_dir.join(format!("{}.disable", old_fname)),
                                                )
                                                .await;
                                            }
                                            let dep_path = mods_dir.join(fname);
                                            if !dep_path.exists() {
                                                if let Some(ref c) = ctx {
                                                    let _ = download_mod_file_task(c, &client, url, &dep_path).await;
                                                } else {
                                                    let _ = download_mod_file_stream(
                                                        &app, &client, url, &dep_path, fname,
                                                    )
                                                    .await;
                                                }
                                            }
                                            // Enforce 1-to-1 mapping
                                            installed_map.retain(|_, (v, _)| v != fname);
                                            installed_map.insert(dep_key, (fname.to_string(), Some(url.to_string())));
                                            save_installed_mods_json(&version_id, &installed_map)
                                                .await;
                                        }
                                    }
                                }
                            }
                        }
                    } else {
                        let pid = &dep.project_id;
                        tracing::info!(
                            "Fetching Modrinth dependency project (latest compatible): {}",
                            pid
                        );
                        if let Ok(files) = crate::core::modrinth::get_modrinth_mod_files(
                            pid.clone(),
                            mc_version.clone(),
                            vec![loader.clone()],
                        )
                        .await
                        {
                            if let Some(primary) = files.first() {
                                let fname = &primary.filename;
                                let url = &primary.download_url;

                                if let Some((old_fname, _)) = installed_map.get(&dep_key) {
                                    if old_fname != fname {
                                        let _ =
                                            tokio::fs::remove_file(mods_dir.join(old_fname)).await;
                                    }
                                    let _ = tokio::fs::remove_file(
                                        mods_dir.join(format!("{}.disable", old_fname)),
                                    )
                                    .await;
                                }
                                let dep_path = mods_dir.join(fname);
                                if !dep_path.exists() {
                                    if let Some(ref c) = ctx {
                                        let _ = download_mod_file_task(c, &client, url, &dep_path).await;
                                    } else {
                                        let _ = download_mod_file_stream(
                                            &app, &client, url, &dep_path, fname,
                                        )
                                        .await;
                                    }
                                }
                                // Enforce 1-to-1 mapping
                                installed_map.retain(|_, (v, _)| v != fname);
                                installed_map.insert(dep_key, (fname.to_string(), Some(url.to_string())));
                                save_installed_mods_json(&version_id, &installed_map).await;
                            }
                        }
                    }
                } else if mod_source == "curseforge" {
                    tracing::info!("Fetching CurseForge dependency project: {}", dep.project_id);
                    if let Ok(files) = crate::core::curseforge::get_cf_mod_files(
                        dep.project_id.clone(),
                        mc_version.clone(),
                        vec![loader.clone()],
                    )
                    .await
                    {
                        if let Some(primary) = files.first() {
                            let fname = &primary.filename;
                            let url = &primary.download_url;

                            if let Some((old_fname, _)) = installed_map.get(&dep_key) {
                                if old_fname != fname {
                                    let _ = tokio::fs::remove_file(mods_dir.join(old_fname)).await;
                                }
                                let _ = tokio::fs::remove_file(
                                    mods_dir.join(format!("{}.disable", old_fname)),
                                )
                                .await;
                            }
                            let dep_path = mods_dir.join(fname);
                            if !dep_path.exists() {
                                let _ =
                                    download_mod_file_stream(&app, &client, url, &dep_path, fname)
                                        .await;
                            }
                            // Enforce 1-to-1 mapping
                            installed_map.retain(|_, (v, _)| v != fname);
                            installed_map.insert(dep_key, (fname.to_string(), Some(url.to_string())));
                            save_installed_mods_json(&version_id, &installed_map).await;
                        }
                    }
                }
            }
        }
    }

    Ok(filename)
}

/// Import a local mod file into an instance
#[tauri::command]
pub async fn import_local_mod_to_instance(
    version_id: String,
    file_path: String,
) -> Result<String, String> {
    if !crate::core::fs_utils::is_safe_single_name(&version_id) {
        return Err("Invalid instance ID".to_string());
    }
    let base_dir = get_minecraft_base();
    let mods_dir = base_dir.join("versions").join(&version_id).join("mods");

    if !mods_dir.exists() {
        tokio::fs::create_dir_all(&mods_dir)
            .await
            .map_err(|e| format!("Failed to create mods directory: {}", e))?;
    }

    let path = std::path::Path::new(&file_path);
    if !path.exists() || !path.is_file() {
        return Err(format!("File does not exist: {}", file_path));
    }

    let filename = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown.jar")
        .to_string();

    let target_path = mods_dir.join(&filename);

    tokio::fs::copy(&path, &target_path)
        .await
        .map_err(|e| format!("Failed to copy mod file: {}", e))?;

    tracing::info!("Imported local mod {} to instance {}", filename, version_id);
    Ok(filename)
}

/// Bind an installed instance to a specific server for updates
#[tauri::command]
pub async fn bind_instance_to_server(
    instance_id: String,
    server_id: String,
    pack_version_id: Option<String>,
    pack_file_name: Option<String>,
) -> Result<(), String> {
    tracing::info!(
        "Binding instance {} to server {} (packVersionId: {:?}, packFileName: {:?})",
        instance_id,
        server_id,
        pack_version_id,
        pack_file_name
    );

    let base_dir = get_minecraft_base();
    let instance_dir = base_dir.join("versions").join(&instance_id);

    if !instance_dir.exists() {
        return Err(format!(
            "Instance directory does not exist: {}",
            instance_id
        ));
    }

    crate::core::launcher::update_instance_config(&instance_dir, |config| {
        config.server_id = Some(server_id);
        config.pack_version_id = pack_version_id;
        config.pack_file_name = pack_file_name;
    })
    .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_mc_version_from_id() {
        assert_eq!(
            extract_mc_version_from_id("1.20.1"),
            Some("1.20.1".to_string())
        );
        assert_eq!(
            extract_mc_version_from_id("1.19.4-Fabric-0.15.7"),
            Some("1.19.4".to_string())
        );
        assert_eq!(
            extract_mc_version_from_id("Fabric-1.18.2-0.14.21"),
            Some("1.18.2".to_string())
        );
        assert_eq!(
            extract_mc_version_from_id("NeoForge-1.20.4-20.4.80-beta"),
            Some("1.20.4".to_string())
        );
        assert_eq!(
            extract_mc_version_from_id("1.8.9-forge1.8.9-11.15.1.2318-1.8.9"),
            Some("1.8.9".to_string())
        );
        assert_eq!(extract_mc_version_from_id("invalid-version"), None);
        assert_eq!(extract_mc_version_from_id("47.1.0"), None); // Forge version shouldn't be extracted
    }

    #[test]
    fn test_parse_version_json() {
        let vanilla_json = r#"{
            "id": "1.20.1",
            "type": "release"
        }"#;
        let (mc, loader, mv, _mt, _mpid) = parse_version_json(vanilla_json, "1.20.1");
        assert_eq!(mc, "1.20.1");
        assert_eq!(loader, "Vanilla");
        assert_eq!(mv, None);

        let fabric_json = r#"{
            "id": "fabric-loader-0.15.7-1.20.1",
            "inheritsFrom": "1.20.1",
            "mainClass": "net.fabricmc.loader.impl.launch.knot.KnotClient",
            "modpackVersion": "1.0.0",
            "modpackType": "CurseForge"
        }"#;
        let (mc, loader, mv, mt, _mpid) =
            parse_version_json(fabric_json, "fabric-loader-0.15.7-1.20.1");
        assert_eq!(mc, "1.20.1");
        assert_eq!(loader, "Fabric");
        assert_eq!(mv, Some("1.0.0".to_string()));
        assert_eq!(mt, Some("CurseForge".to_string()));

        let forge_json = r#"{
            "id": "1.20.1-forge-47.2.20",
            "inheritsFrom": "1.20.1",
            "mainClass": "cpw.mods.bootstraplauncher.BootstrapLauncher"
        }"#;
        let (mc, loader, _mv, _mt, _mpid) = parse_version_json(forge_json, "1.20.1-forge-47.2.20");
        assert_eq!(mc, "1.20.1");
        assert_eq!(loader, "Forge");

        let neoforge_json = r#"{
            "id": "1.20.4-neoforge-20.4.80",
            "inheritsFrom": "1.20.4"
        }"#;
        let (mc, loader, _mv, _mt, _mpid) =
            parse_version_json(neoforge_json, "1.20.4-neoforge-20.4.80");
        assert_eq!(mc, "1.20.4");
        assert_eq!(loader, "NeoForge");
    }

    #[tokio::test]
    async fn test_scan_installed_instances_keeps_updating_instance_after_task_cancellation() {
        use tempfile::tempdir;

        let temp = tempdir().expect("tempdir");
        let versions_dir = temp.path().join("versions");
        let inst_id = "DawnlandPack";
        let inst_dir = versions_dir.join(inst_id);
        tokio::fs::create_dir_all(&inst_dir).await.expect("create dir");

        let version_json = r#"{
            "id": "DawnlandPack",
            "inheritsFrom": "26.1.2"
        }"#;
        tokio::fs::write(inst_dir.join(format!("{}.json", inst_id)), version_json)
            .await
            .expect("write version json");

        let dlml_json = r#"{
            "icon": "icon.png",
            "mcVersion": "26.1.2",
            "loaderType": "NeoForge",
            "modpackVersion": "0.13.0-beta",
            "modpackType": "CurseForge",
            "modpackProjectId": "1148445",
            "isInstalling": false,
            "isUpdating": true
        }"#;
        tokio::fs::write(inst_dir.join("dlml.json"), dlml_json)
            .await
            .expect("write dlml.json");

        let mut task = crate::core::task::TaskState::new(
            "task_1".to_string(),
            crate::core::task::TaskType::InstallModpack {
                zip_path: "test.zip".to_string(),
                instance_name: inst_id.to_string(),
                is_update: true,
                project_id: Some("1148445".to_string()),
                server_id: Some("33".to_string()),
                pack_version_id: Some("9343763".to_string()),
                pack_file_name: None,
            },
        );
        task.status = crate::core::task::TaskStatus::Cancelled;
        task.error = Some("Task cancelled".to_string());

        let tasks = vec![task];
        let task_ctx = InstanceTaskContext {
            starting_instances: None,
            active_tasks: None,
            tasks: &tasks,
            running_instances: None,
        };

        let (instances, _) = scan_installed_instances_internal(&versions_dir, &task_ctx)
            .await
            .expect("scan instances");

        assert_eq!(instances.len(), 1);
        let item = &instances[0];
        assert_eq!(item.id, inst_id);
        assert_eq!(item.name, inst_id);
        assert_eq!(item.mc_version, "26.1.2");
        assert_eq!(item.loader_type, "NeoForge");
        assert_eq!(item.modpack_version.as_deref(), Some("0.13.0-beta"));
        assert_eq!(item.modpack_type.as_deref(), Some("CurseForge"));
        assert_eq!(item.modpack_project_id.as_deref(), Some("1148445"));
        assert_eq!(item.status, crate::models::instance::InstanceStatus::Updating);

        // Verify dlml.json on disk has been migrated: old fields removed, new status written
        let updated_dlml = tokio::fs::read_to_string(inst_dir.join("dlml.json"))
            .await
            .expect("read updated dlml.json");
        assert!(updated_dlml.contains("\"status\": \"updating\""));
        assert!(!updated_dlml.contains("isUpdating"));
        assert!(!updated_dlml.contains("isInstalling"));
    }

    #[tokio::test]
    async fn test_scan_installed_instances_marks_cancelling_when_dlml_is_cancelling() {
        use tempfile::tempdir;

        let temp = tempdir().expect("tempdir");
        let versions_dir = temp.path().join("versions");
        let inst_id = "DawnlandPack";
        let inst_dir = versions_dir.join(inst_id);
        tokio::fs::create_dir_all(&inst_dir).await.expect("create dir");

        let version_json = r#"{
            "id": "DawnlandPack",
            "inheritsFrom": "26.1.2"
        }"#;
        tokio::fs::write(inst_dir.join(format!("{}.json", inst_id)), version_json)
            .await
            .expect("write version json");

        let dlml_json = r#"{
            "icon": "icon.png",
            "mcVersion": "26.1.2",
            "loaderType": "NeoForge",
            "modpackVersion": "0.13.0-beta",
            "modpackType": "CurseForge",
            "modpackProjectId": "1148445",
            "isUpdating": true,
            "isCancelling": true
        }"#;
        tokio::fs::write(inst_dir.join("dlml.json"), dlml_json)
            .await
            .expect("write dlml.json");

        let tasks = vec![];
        let task_ctx = InstanceTaskContext {
            starting_instances: None,
            active_tasks: None,
            tasks: &tasks,
            running_instances: None,
        };

        let (instances, _) = scan_installed_instances_internal(&versions_dir, &task_ctx)
            .await
            .expect("scan instances");

        assert_eq!(instances.len(), 1);
        let item = &instances[0];
        assert_eq!(item.id, inst_id);
        assert_eq!(item.status, crate::models::instance::InstanceStatus::CancellingUpdate);

        let updated_dlml = tokio::fs::read_to_string(inst_dir.join("dlml.json"))
            .await
            .expect("read updated dlml.json");
        assert!(updated_dlml.contains("\"status\": \"cancellingUpdate\""));
        assert!(!updated_dlml.contains("isUpdating"));
        assert!(!updated_dlml.contains("isCancelling"));
    }

    #[tokio::test]
    async fn test_scan_installed_instances_detects_running_status() {
        use std::collections::HashSet;
        use tempfile::tempdir;

        let temp = tempdir().expect("tempdir");
        let versions_dir = temp.path().join("versions");
        let inst_id = "RunningPack";
        let inst_dir = versions_dir.join(inst_id);
        tokio::fs::create_dir_all(&inst_dir).await.expect("create dir");

        let version_json = r#"{
            "id": "RunningPack",
            "inheritsFrom": "1.20.1"
        }"#;
        tokio::fs::write(inst_dir.join(format!("{}.json", inst_id)), version_json)
            .await
            .expect("write version json");

        let dlml_json = r#"{
            "mcVersion": "1.20.1",
            "loaderType": "Vanilla",
            "status": "ready"
        }"#;
        tokio::fs::write(inst_dir.join("dlml.json"), dlml_json)
            .await
            .expect("write dlml.json");

        let mut running_set = HashSet::new();
        running_set.insert(inst_id.to_string());

        let tasks = vec![];
        let task_ctx = InstanceTaskContext {
            starting_instances: None,
            active_tasks: None,
            tasks: &tasks,
            running_instances: Some(&running_set),
        };

        let (instances, _) = scan_installed_instances_internal(&versions_dir, &task_ctx)
            .await
            .expect("scan instances");

        assert_eq!(instances.len(), 1);
        assert_eq!(instances[0].status, crate::models::instance::InstanceStatus::Running);
    }

    #[tokio::test]
    async fn test_scan_installed_instances_completed_update_task_does_not_promote_ready() {
        use tempfile::tempdir;

        let temp = tempdir().expect("tempdir");
        let versions_dir = temp.path().join("versions");
        let inst_id = "ReadyPackWithDoneUpdate";
        let inst_dir = versions_dir.join(inst_id);
        tokio::fs::create_dir_all(&inst_dir).await.expect("create dir");

        let version_json = r#"{
            "id": "ReadyPackWithDoneUpdate",
            "inheritsFrom": "1.20.1"
        }"#;
        tokio::fs::write(inst_dir.join(format!("{}.json", inst_id)), version_json)
            .await
            .expect("write version json");

        let dlml_json = r#"{
            "status": "ready"
        }"#;
        tokio::fs::write(inst_dir.join("dlml.json"), dlml_json)
            .await
            .expect("write dlml.json");

        let mut completed_task = crate::core::task::TaskState::new(
            "task_update_done".to_string(),
            crate::core::task::TaskType::InstallModpack {
                zip_path: "test.zip".to_string(),
                instance_name: inst_id.to_string(),
                is_update: true,
                project_id: None,
                server_id: None,
                pack_version_id: None,
                pack_file_name: None,
            },
        );
        completed_task.status = crate::core::task::TaskStatus::Completed;

        let tasks = vec![completed_task];
        let task_ctx = InstanceTaskContext {
            starting_instances: None,
            active_tasks: None,
            tasks: &tasks,
            running_instances: None,
        };

        let (instances, _) = scan_installed_instances_internal(&versions_dir, &task_ctx)
            .await
            .expect("scan instances");

        assert_eq!(instances.len(), 1);
        assert_eq!(instances[0].status, crate::models::instance::InstanceStatus::Ready);
    }

    #[tokio::test]
    async fn test_scan_installed_instances_active_update_task_promotes_ready() {
        use tempfile::tempdir;

        let temp = tempdir().expect("tempdir");
        let versions_dir = temp.path().join("versions");
        let inst_id = "ReadyPackWithRunningUpdate";
        let inst_dir = versions_dir.join(inst_id);
        tokio::fs::create_dir_all(&inst_dir).await.expect("create dir");

        let version_json = r#"{
            "id": "ReadyPackWithRunningUpdate",
            "inheritsFrom": "1.20.1"
        }"#;
        tokio::fs::write(inst_dir.join(format!("{}.json", inst_id)), version_json)
            .await
            .expect("write version json");

        let dlml_json = r#"{
            "status": "ready"
        }"#;
        tokio::fs::write(inst_dir.join("dlml.json"), dlml_json)
            .await
            .expect("write dlml.json");

        let mut running_task = crate::core::task::TaskState::new(
            "task_update_running".to_string(),
            crate::core::task::TaskType::InstallModpack {
                zip_path: "test.zip".to_string(),
                instance_name: inst_id.to_string(),
                is_update: true,
                project_id: None,
                server_id: None,
                pack_version_id: None,
                pack_file_name: None,
            },
        );
        running_task.status = crate::core::task::TaskStatus::Running;

        let tasks = vec![running_task];
        let task_ctx = InstanceTaskContext {
            starting_instances: None,
            active_tasks: None,
            tasks: &tasks,
            running_instances: None,
        };

        let (instances, _) = scan_installed_instances_internal(&versions_dir, &task_ctx)
            .await
            .expect("scan instances");

        assert_eq!(instances.len(), 1);
        assert_eq!(instances[0].status, crate::models::instance::InstanceStatus::Updating);
    }

    #[tokio::test]
    async fn test_scan_installed_instances_migrates_legacy_idle_to_ready() {
        use tempfile::tempdir;

        let temp = tempdir().expect("tempdir");
        let versions_dir = temp.path().join("versions");
        let inst_id = "IdlePack";
        let inst_dir = versions_dir.join(inst_id);
        tokio::fs::create_dir_all(&inst_dir).await.expect("create dir");

        let version_json = r#"{
            "id": "IdlePack",
            "inheritsFrom": "1.20.1"
        }"#;
        tokio::fs::write(inst_dir.join(format!("{}.json", inst_id)), version_json)
            .await
            .expect("write version json");

        let dlml_json = r#"{
            "mcVersion": "1.20.1",
            "loaderType": "Vanilla",
            "status": "idle"
        }"#;
        tokio::fs::write(inst_dir.join("dlml.json"), dlml_json)
            .await
            .expect("write dlml.json");

        let tasks = vec![];
        let task_ctx = InstanceTaskContext {
            starting_instances: None,
            active_tasks: None,
            tasks: &tasks,
            running_instances: None,
        };

        let (instances, _) = scan_installed_instances_internal(&versions_dir, &task_ctx)
            .await
            .expect("scan instances");

        assert_eq!(instances.len(), 1);
        assert_eq!(instances[0].status, crate::models::instance::InstanceStatus::Ready);

        // Verify dlml.json on disk was rewritten from "idle" to "ready"
        let updated_dlml = tokio::fs::read_to_string(inst_dir.join("dlml.json"))
            .await
            .expect("read updated dlml.json");
        assert!(updated_dlml.contains("\"status\": \"ready\""));
        assert!(!updated_dlml.contains("\"status\": \"idle\""));
        assert!(!updated_dlml.contains("\"idle\""));
    }

    #[tokio::test]
    async fn test_scan_installed_instances_mod_download_task_does_not_promote_ready() {
        use tempfile::tempdir;

        let temp = tempdir().expect("tempdir");
        let versions_dir = temp.path().join("versions");
        let inst_id = "ReadyPackWithModDownload";
        let inst_dir = versions_dir.join(inst_id);
        tokio::fs::create_dir_all(&inst_dir).await.expect("create dir");

        let version_json = r#"{
            "id": "ReadyPackWithModDownload",
            "inheritsFrom": "1.20.1"
        }"#;
        tokio::fs::write(inst_dir.join(format!("{}.json", inst_id)), version_json)
            .await
            .expect("write version json");

        let dlml_json = r#"{ "status": "ready" }"#;
        tokio::fs::write(inst_dir.join("dlml.json"), dlml_json)
            .await
            .expect("write dlml.json");

        let mut mod_task = crate::core::task::TaskState::new(
            "task_mod_running".to_string(),
            crate::core::task::TaskType::InstallMod {
                source: "modrinth".into(),
                project_id: "mod1".into(),
                file_id: "f1".into(),
                mod_name: Some("testmod".into()),
                download_url: "http://example.com/mod.jar".into(),
                instance_id: Some(inst_id.to_string()),
                target_dir: None,
                dependencies: None,
                keep_both: None,
            },
        );
        mod_task.status = crate::core::task::TaskStatus::Running;

        let tasks = vec![mod_task];
        let task_ctx = InstanceTaskContext {
            starting_instances: None,
            active_tasks: None,
            tasks: &tasks,
            running_instances: None,
        };

        let (instances, _) = scan_installed_instances_internal(&versions_dir, &task_ctx)
            .await
            .expect("scan instances");

        assert_eq!(instances.len(), 1);
        // Status must NOT be promoted to Installing!
        assert_eq!(instances[0].status, crate::models::instance::InstanceStatus::Ready);
    }

    #[tokio::test]
    async fn test_scan_installed_instances_prefers_latest_active_task_over_old_history() {
        use tempfile::tempdir;

        let temp = tempdir().expect("tempdir");
        let versions_dir = temp.path().join("versions");
        let inst_id = "PackWithHistoryAndActive";
        let inst_dir = versions_dir.join(inst_id);
        tokio::fs::create_dir_all(&inst_dir).await.expect("create dir");

        let version_json = r#"{
            "id": "PackWithHistoryAndActive",
            "inheritsFrom": "1.20.1"
        }"#;
        tokio::fs::write(inst_dir.join(format!("{}.json", inst_id)), version_json)
            .await
            .expect("write version json");

        let dlml_json = r#"{ "status": "ready" }"#;
        tokio::fs::write(inst_dir.join("dlml.json"), dlml_json)
            .await
            .expect("write dlml.json");

        // Old completed installation task appearing FIRST in the slice
        let mut old_task = crate::core::task::TaskState::new(
            "old_install".to_string(),
            crate::core::task::TaskType::InstallVanilla {
                version_id: "1.20.1".into(),
                version_json_url: "".into(),
                custom_instance_name: Some(inst_id.to_string()),
                is_dependency: None,
            },
        );
        old_task.status = crate::core::task::TaskStatus::Completed;
        old_task.created_at = 100;
        old_task.updated_at = 150;

        // Newer active update task appearing SECOND in the slice
        let mut new_update_task = crate::core::task::TaskState::new(
            "new_update".to_string(),
            crate::core::task::TaskType::InstallModpack {
                zip_path: "p.zip".into(),
                instance_name: inst_id.to_string(),
                is_update: true,
                project_id: None,
                server_id: None,
                pack_version_id: None,
                pack_file_name: None,
            },
        );
        new_update_task.status = crate::core::task::TaskStatus::Running;
        new_update_task.created_at = 200;
        new_update_task.updated_at = 250;

        let tasks = vec![old_task, new_update_task];
        let task_ctx = InstanceTaskContext {
            starting_instances: None,
            active_tasks: None,
            tasks: &tasks,
            running_instances: None,
        };

        let (instances, _) = scan_installed_instances_internal(&versions_dir, &task_ctx)
            .await
            .expect("scan instances");

        assert_eq!(instances.len(), 1);
        // Must derive status from new active update task (Updating), not stopping at old completed task
        assert_eq!(instances[0].status, crate::models::instance::InstanceStatus::Updating);
    }

    #[tokio::test]
    async fn test_scan_installed_instances_cancelled_update_with_running_mod_stays_ready() {
        use std::collections::HashMap;
        use std::sync::Arc;
        use tempfile::tempdir;
        use tokio::sync::RwLock;
        use tokio_util::sync::CancellationToken;

        let temp = tempdir().expect("tempdir");
        let versions_dir = temp.path().join("versions");
        let inst_id = "ReadyPackCancelledUpdateRunningMod";
        let inst_dir = versions_dir.join(inst_id);
        tokio::fs::create_dir_all(&inst_dir).await.expect("create dir");

        let version_json = r#"{
            "id": "ReadyPackCancelledUpdateRunningMod",
            "inheritsFrom": "1.20.1"
        }"#;
        tokio::fs::write(inst_dir.join(format!("{}.json", inst_id)), version_json)
            .await
            .expect("write version json");

        let dlml_json = r#"{ "status": "ready" }"#;
        tokio::fs::write(inst_dir.join("dlml.json"), dlml_json)
            .await
            .expect("write dlml.json");

        // Old cancelled update task in DB history
        let mut cancelled_update = crate::core::task::TaskState::new(
            "old_cancelled_update".to_string(),
            crate::core::task::TaskType::InstallModpack {
                zip_path: "p.zip".into(),
                instance_name: inst_id.to_string(),
                is_update: true,
                project_id: None,
                server_id: None,
                pack_version_id: None,
                pack_file_name: None,
            },
        );
        cancelled_update.status = crate::core::task::TaskStatus::Cancelled;

        // Currently running mod download task on the same instance
        let mut running_mod = crate::core::task::TaskState::new(
            "running_mod".to_string(),
            crate::core::task::TaskType::InstallMod {
                source: "modrinth".into(),
                project_id: "mod1".into(),
                file_id: "f1".into(),
                mod_name: Some("testmod".into()),
                download_url: "http://example.com/mod.jar".into(),
                instance_id: Some(inst_id.to_string()),
                target_dir: None,
                dependencies: None,
                keep_both: None,
            },
        );
        running_mod.status = crate::core::task::TaskStatus::Running;

        let (_, done_rx) = tokio::sync::watch::channel(false);
        let running_mod_type = running_mod.task_type.clone();
        let active_tasks_map = HashMap::from([(
            "running_mod".to_string(),
            Arc::new(crate::core::task::manager::TaskWorkerHandle {
                cancel_token: Arc::new(CancellationToken::new()),
                done_rx,
                instance_id: Some(inst_id.to_string()),
                task_type: running_mod_type,
                state: Arc::new(RwLock::new(running_mod.clone())),
            }),
        )]);
        let active_tasks_lock = RwLock::new(active_tasks_map);

        let tasks = vec![cancelled_update, running_mod];
        let task_ctx = InstanceTaskContext {
            starting_instances: None,
            active_tasks: Some(&active_tasks_lock),
            tasks: &tasks,
            running_instances: None,
        };

        let (instances, _) = scan_installed_instances_internal(&versions_dir, &task_ctx)
            .await
            .expect("scan instances");

        assert_eq!(instances.len(), 1);
        // Reported status must stay Ready, NOT CancellingUpdate!
        assert_eq!(instances[0].status, crate::models::instance::InstanceStatus::Ready);
    }

    #[test]
    fn test_validate_preset_download_url() {
        let ok_mr = reqwest::Url::parse("https://cdn.modrinth.com/data/P7dR8mSH/versions/1.0/mod.jar").unwrap();
        let ok_cf = reqwest::Url::parse("https://edge.forgecdn.net/files/123/456/mod.jar").unwrap();
        let ok_cf2 = reqwest::Url::parse("https://media.forgecdn.net/files/123/456/mod.jar").unwrap();
        let ok_cf3 = reqwest::Url::parse("https://curseforge.com/api/v1/mods/123").unwrap();

        assert!(validate_preset_download_url(&ok_mr).is_ok());
        assert!(validate_preset_download_url(&ok_cf).is_ok());
        assert!(validate_preset_download_url(&ok_cf2).is_ok());
        assert!(validate_preset_download_url(&ok_cf3).is_ok());

        let bad_scheme = reqwest::Url::parse("http://cdn.modrinth.com/data/mod.jar").unwrap();
        assert!(validate_preset_download_url(&bad_scheme).is_err());

        let bad_host = reqwest::Url::parse("https://evil.com/mod.jar").unwrap();
        assert!(validate_preset_download_url(&bad_host).is_err());

        let bad_local = reqwest::Url::parse("https://localhost/mod.jar").unwrap();
        assert!(validate_preset_download_url(&bad_local).is_err());

        let bad_ip = reqwest::Url::parse("https://127.0.0.1/mod.jar").unwrap();
        assert!(validate_preset_download_url(&bad_ip).is_err());

        let bad_private = reqwest::Url::parse("https://192.168.1.1/mod.jar").unwrap();
        assert!(validate_preset_download_url(&bad_private).is_err());
    }

    #[tokio::test]
    async fn test_rename_instance_in_dir_updates_files_and_metadata() {
        use tempfile::tempdir;

        let temp = tempdir().expect("tempdir");
        let versions_dir = temp.path().join("versions");
        let old_dir = versions_dir.join("1.20.1");
        tokio::fs::create_dir_all(old_dir.join("mods"))
            .await
            .expect("create old dir");

        let version_json = r#"{
            "id": "1.20.1",
            "type": "release"
        }"#;
        tokio::fs::write(old_dir.join("1.20.1.json"), version_json)
            .await
            .expect("write version json");
        tokio::fs::write(old_dir.join("1.20.1.jar"), b"fake-jar")
            .await
            .expect("write jar");

        let dlml_json = r#"{
            "name": "1.20.1",
            "status": "ready"
        }"#;
        tokio::fs::write(old_dir.join("dlml.json"), dlml_json)
            .await
            .expect("write dlml.json");

        let mod_dest = old_dir.join("mods").join("sodium.jar");
        let tasks = vec![crate::downloader::DownloadTask::new(
            "https://example.com/sodium.jar".to_string(),
            mod_dest.to_string_lossy().to_string(),
            None,
            None,
        )];
        tokio::fs::write(
            old_dir.join("modpack_tasks.json"),
            serde_json::to_string_pretty(&tasks).unwrap(),
        )
        .await
        .expect("write modpack_tasks.json");

        // Also create a child instance inheriting from 1.20.1
        let child_dir = versions_dir.join("ChildPack");
        tokio::fs::create_dir_all(&child_dir).await.unwrap();
        tokio::fs::write(
            child_dir.join("ChildPack.json"),
            r#"{ "id": "ChildPack", "inheritsFrom": "1.20.1" }"#,
        )
        .await
        .unwrap();

        rename_instance_in_dir(&versions_dir, "1.20.1", "SurvivalPack")
            .await
            .expect("rename should succeed");

        assert!(!old_dir.exists());
        let new_dir = versions_dir.join("SurvivalPack");
        assert!(new_dir.exists());
        assert!(!new_dir.join("1.20.1.json").exists());
        assert!(!new_dir.join("1.20.1.jar").exists());
        assert!(new_dir.join("SurvivalPack.jar").exists());

        let new_json_content = tokio::fs::read_to_string(new_dir.join("SurvivalPack.json"))
            .await
            .expect("read new json");
        let parsed: serde_json::Value = serde_json::from_str(&new_json_content).unwrap();
        assert_eq!(parsed.get("id").and_then(|v| v.as_str()), Some("SurvivalPack"));
        assert_eq!(parsed.get("clientVersion").and_then(|v| v.as_str()), Some("1.20.1"));

        let new_dlml = tokio::fs::read_to_string(new_dir.join("dlml.json"))
            .await
            .expect("read dlml.json");
        let parsed_dlml: crate::core::launcher::InstanceConfig =
            serde_json::from_str(&new_dlml).unwrap();
        assert_eq!(parsed_dlml.name.as_deref(), Some("SurvivalPack"));

        let updated_tasks_str = tokio::fs::read_to_string(new_dir.join("modpack_tasks.json"))
            .await
            .expect("read modpack_tasks.json");
        let updated_tasks: Vec<crate::downloader::DownloadTask> =
            serde_json::from_str(&updated_tasks_str).unwrap();
        assert_eq!(
            updated_tasks[0].dest_path,
            new_dir.join("mods").join("sodium.jar").to_string_lossy().to_string()
        );

        let child_json = tokio::fs::read_to_string(child_dir.join("ChildPack.json"))
            .await
            .unwrap();
        let parsed_child: serde_json::Value = serde_json::from_str(&child_json).unwrap();
        assert_eq!(
            parsed_child.get("inheritsFrom").and_then(|v| v.as_str()),
            Some("SurvivalPack")
        );
    }

    #[tokio::test]
    async fn test_rename_instance_in_dir_case_only_and_conflicts() {
        use tempfile::tempdir;

        let temp = tempdir().expect("tempdir");
        let versions_dir = temp.path().join("versions");
        let inst_a = versions_dir.join("TestPack");
        let inst_b = versions_dir.join("OtherPack");
        tokio::fs::create_dir_all(&inst_a).await.unwrap();
        tokio::fs::create_dir_all(&inst_b).await.unwrap();

        tokio::fs::write(
            inst_a.join("TestPack.json"),
            r#"{ "id": "TestPack", "clientVersion": "1.20.1" }"#,
        )
        .await
        .unwrap();
        tokio::fs::write(inst_a.join("TestPack.jar"), b"jar").await.unwrap();
        tokio::fs::write(
            inst_b.join("OtherPack.json"),
            r#"{ "id": "OtherPack", "clientVersion": "1.20.1" }"#,
        )
        .await
        .unwrap();

        // Case-only rename on same instance should succeed
        rename_instance_in_dir(&versions_dir, "TestPack", "testpack")
            .await
            .expect("case-only rename should succeed");

        let entries: Vec<String> = std::fs::read_dir(&versions_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert!(entries.contains(&"testpack".to_string()));
        assert!(!entries.contains(&"TestPack".to_string()));

        let renamed_dir = versions_dir.join("testpack");
        let renamed_files: Vec<String> = std::fs::read_dir(&renamed_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert!(renamed_files.contains(&"testpack.json".to_string()));
        assert!(renamed_files.contains(&"testpack.jar".to_string()));
        assert!(!renamed_files.contains(&"TestPack.json".to_string()));
        assert!(!renamed_files.contains(&"TestPack.jar".to_string()));

        let json_str = tokio::fs::read_to_string(renamed_dir.join("testpack.json"))
            .await
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        assert_eq!(parsed.get("id").and_then(|v| v.as_str()), Some("testpack"));

        // Case-insensitive conflict against another instance should fail
        let conflict_err = rename_instance_in_dir(&versions_dir, "testpack", "otherpack")
            .await
            .expect_err("case-insensitive collision with another instance must fail");
        assert!(conflict_err.contains("already exists"));

        // Invalid path characters and reserved names must fail
        assert!(rename_instance_in_dir(&versions_dir, "testpack", "bad/name")
            .await
            .is_err());
        assert!(rename_instance_in_dir(&versions_dir, "testpack", "CON")
            .await
            .is_err());
        assert!(rename_instance_in_dir(&versions_dir, "testpack", "trailing.")
            .await
            .is_err());
    }

    #[tokio::test]
    async fn test_rename_instance_in_dir_malformed_dlml_preserves_old_instance() {
        use tempfile::tempdir;

        let temp = tempdir().expect("tempdir");
        let versions_dir = temp.path().join("versions");
        let old_dir = versions_dir.join("1.20.1");
        tokio::fs::create_dir_all(&old_dir).await.unwrap();

        let orig_version_json = r#"{ "id": "1.20.1", "type": "release" }"#;
        tokio::fs::write(old_dir.join("1.20.1.json"), orig_version_json)
            .await
            .unwrap();
        tokio::fs::write(old_dir.join("1.20.1.jar"), b"jar-bytes")
            .await
            .unwrap();
        tokio::fs::write(old_dir.join("1.20.1.json.bak"), b"bak-bytes")
            .await
            .unwrap();
        tokio::fs::write(old_dir.join("dlml.json"), "{ malformed json")
            .await
            .unwrap();

        let child_dir = versions_dir.join("ChildPack");
        tokio::fs::create_dir_all(&child_dir).await.unwrap();
        tokio::fs::write(
            child_dir.join("ChildPack.json"),
            r#"{ "id": "ChildPack", "inheritsFrom": "1.20.1" }"#,
        )
        .await
        .unwrap();

        let err = rename_instance_in_dir(&versions_dir, "1.20.1", "SurvivalPack")
            .await
            .expect_err("malformed dlml.json must fail rename");
        assert!(err.contains("parse instance config"));

        assert!(old_dir.exists());
        assert!(!versions_dir.join("SurvivalPack").exists());
        assert_eq!(
            tokio::fs::read_to_string(old_dir.join("1.20.1.json"))
                .await
                .unwrap(),
            orig_version_json
        );
        assert!(old_dir.join("1.20.1.jar").exists());
        assert!(old_dir.join("1.20.1.json.bak").exists());

        let child_json: serde_json::Value = serde_json::from_str(
            &tokio::fs::read_to_string(child_dir.join("ChildPack.json"))
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            child_json.get("inheritsFrom").and_then(|v| v.as_str()),
            Some("1.20.1")
        );

        let task_ctx = InstanceTaskContext {
            starting_instances: None,
            active_tasks: None,
            tasks: &[],
            running_instances: None,
        };
        let (instances, _) = scan_installed_instances_internal(&versions_dir, &task_ctx)
            .await
            .unwrap();
        assert!(instances.iter().any(|item| item.id == "1.20.1"));
    }

    #[tokio::test]
    async fn test_rename_instance_in_dir_malformed_modpack_tasks_preserves_old_instance() {
        use tempfile::tempdir;

        let temp = tempdir().expect("tempdir");
        let versions_dir = temp.path().join("versions");
        let old_dir = versions_dir.join("1.20.1");
        tokio::fs::create_dir_all(&old_dir).await.unwrap();

        tokio::fs::write(
            old_dir.join("1.20.1.json"),
            r#"{ "id": "1.20.1", "clientVersion": "1.20.1" }"#,
        )
        .await
        .unwrap();
        tokio::fs::write(old_dir.join("1.20.1.jar"), b"jar")
            .await
            .unwrap();
        tokio::fs::write(
            old_dir.join("dlml.json"),
            r#"{ "name": "1.20.1", "status": "ready" }"#,
        )
        .await
        .unwrap();
        tokio::fs::write(old_dir.join("modpack_tasks.json"), "{ invalid tasks")
            .await
            .unwrap();

        let err = rename_instance_in_dir(&versions_dir, "1.20.1", "SurvivalPack")
            .await
            .expect_err("malformed modpack_tasks.json must fail rename");
        assert!(err.contains("modpack_tasks.json"));

        assert!(old_dir.exists());
        assert!(!versions_dir.join("SurvivalPack").exists());
        assert!(old_dir.join("1.20.1.json").exists());
        assert!(old_dir.join("1.20.1.jar").exists());
    }

    #[tokio::test]
    async fn test_rename_instance_in_dir_blocked_jar_rolls_back_directory() {
        use tempfile::tempdir;

        let temp = tempdir().expect("tempdir");
        let versions_dir = temp.path().join("versions");
        let old_dir = versions_dir.join("1.20.1");
        tokio::fs::create_dir_all(&old_dir).await.unwrap();

        let orig_json = r#"{ "id": "1.20.1", "type": "release" }"#;
        tokio::fs::write(old_dir.join("1.20.1.json"), orig_json)
            .await
            .unwrap();
        tokio::fs::write(old_dir.join("1.20.1.jar"), b"jar-bytes")
            .await
            .unwrap();
        tokio::fs::write(old_dir.join("1.20.1.json.bak"), b"bak-bytes")
            .await
            .unwrap();

        // Create a non-empty directory at the target jar name so renaming 1.20.1.jar fails
        let blocking_dir = old_dir.join("SurvivalPack.jar");
        tokio::fs::create_dir_all(&blocking_dir).await.unwrap();
        tokio::fs::write(blocking_dir.join("lock"), b"x")
            .await
            .unwrap();

        let err = rename_instance_in_dir(&versions_dir, "1.20.1", "SurvivalPack")
            .await
            .expect_err("blocked jar rename must fail");
        assert!(!err.is_empty());

        assert!(old_dir.exists());
        assert!(!versions_dir.join("SurvivalPack").exists());
        assert_eq!(
            tokio::fs::read_to_string(old_dir.join("1.20.1.json"))
                .await
                .unwrap(),
            orig_json
        );
        assert!(old_dir.join("1.20.1.jar").exists());
        assert!(old_dir.join("1.20.1.json.bak").exists());
    }

    #[tokio::test]
    async fn test_commit_rename_plan_rolls_back_all_mutated_files_on_dependent_failure() {
        use tempfile::tempdir;

        let temp = tempdir().expect("tempdir");
        let versions_dir = temp.path().join("versions");
        let old_dir = versions_dir.join("1.20.1");
        let new_dir = versions_dir.join("SurvivalPack");
        tokio::fs::create_dir_all(old_dir.join("mods"))
            .await
            .unwrap();

        let orig_version_json = r#"{ "id": "1.20.1", "type": "release" }"#;
        tokio::fs::write(old_dir.join("1.20.1.json"), orig_version_json)
            .await
            .unwrap();
        tokio::fs::write(old_dir.join("1.20.1.jar"), b"jar-bytes")
            .await
            .unwrap();
        tokio::fs::write(old_dir.join("1.20.1.json.bak"), b"bak-bytes")
            .await
            .unwrap();

        let orig_dlml = r#"{ "name": "1.20.1", "status": "ready" }"#;
        tokio::fs::write(old_dir.join("dlml.json"), orig_dlml)
            .await
            .unwrap();

        let tasks = vec![crate::downloader::DownloadTask::new(
            "https://example.com/sodium.jar".to_string(),
            old_dir
                .join("mods")
                .join("sodium.jar")
                .to_string_lossy()
                .to_string(),
            None,
            None,
        )];
        let orig_tasks = serde_json::to_string_pretty(&tasks).unwrap();
        tokio::fs::write(old_dir.join("modpack_tasks.json"), &orig_tasks)
            .await
            .unwrap();

        let child_a_dir = versions_dir.join("ChildA");
        tokio::fs::create_dir_all(&child_a_dir).await.unwrap();
        let orig_child_a = r#"{ "id": "ChildA", "inheritsFrom": "1.20.1" }"#;
        tokio::fs::write(child_a_dir.join("ChildA.json"), orig_child_a)
            .await
            .unwrap();

        let mut plan = prepare_rename_commit_plan(
            &versions_dir,
            &old_dir,
            &new_dir,
            "1.20.1",
            "SurvivalPack",
        )
        .await
        .expect("prepare plan");

        // Inject a failing second dependent update where an intermediate path component is a regular file
        let blocker_file = versions_dir.join("write_blocker_file");
        tokio::fs::write(&blocker_file, b"not-a-dir")
            .await
            .unwrap();
        plan.dependent_updates.push(DependentJsonUpdate {
            json_path: blocker_file.join("ChildB.json"),
            orig_bytes: Vec::new(),
            updated_bytes: br#"{"id":"ChildB","inheritsFrom":"SurvivalPack"}"#.to_vec(),
        });

        let err = commit_rename_plan(&old_dir, &new_dir, "1.20.1", "SurvivalPack", false, plan)
            .await
            .expect_err("second dependent write failure must fail commit");
        assert!(err.contains("inheritsFrom"));

        // Verify full rollback of directory, internal files, configs, tasks, and ChildA
        assert!(old_dir.exists());
        assert!(!new_dir.exists());
        assert_eq!(
            tokio::fs::read_to_string(old_dir.join("1.20.1.json"))
                .await
                .unwrap(),
            orig_version_json
        );
        assert!(!old_dir.join("SurvivalPack.json").exists());
        assert!(old_dir.join("1.20.1.jar").exists());
        assert!(!old_dir.join("SurvivalPack.jar").exists());
        assert!(old_dir.join("1.20.1.json.bak").exists());
        assert!(!old_dir.join("SurvivalPack.json.bak").exists());
        assert_eq!(
            tokio::fs::read_to_string(old_dir.join("dlml.json"))
                .await
                .unwrap(),
            orig_dlml
        );
        assert_eq!(
            tokio::fs::read_to_string(old_dir.join("modpack_tasks.json"))
                .await
                .unwrap(),
            orig_tasks
        );
        assert_eq!(
            tokio::fs::read_to_string(child_a_dir.join("ChildA.json"))
                .await
                .unwrap(),
            orig_child_a
        );

        // No leftover temporary backup/staging files
        let mut dir_entries = tokio::fs::read_dir(&old_dir).await.unwrap();
        while let Some(entry) = dir_entries.next_entry().await.unwrap() {
            let name = entry.file_name().to_string_lossy().to_string();
            assert!(
                !name.starts_with(".dlml_rename_"),
                "unexpected leftover temp file: {}",
                name
            );
        }

        // Instance remains scannable under old name
        let task_ctx = InstanceTaskContext {
            starting_instances: None,
            active_tasks: None,
            tasks: &[],
            running_instances: None,
        };
        let (instances, _) = scan_installed_instances_internal(&versions_dir, &task_ctx)
            .await
            .unwrap();
        assert!(instances.iter().any(|item| item.id == "1.20.1"));
        assert!(!instances.iter().any(|item| item.id == "SurvivalPack"));
    }

    #[tokio::test]
    async fn test_commit_rename_plan_case_only_rolls_back_casing_and_contents() {
        use tempfile::tempdir;

        let temp = tempdir().expect("tempdir");
        let versions_dir = temp.path().join("versions");
        let old_dir = versions_dir.join("TestPack");
        let new_dir = versions_dir.join("testpack");
        tokio::fs::create_dir_all(&old_dir).await.unwrap();

        let orig_version_json = r#"{ "id": "TestPack", "clientVersion": "1.20.1" }"#;
        tokio::fs::write(old_dir.join("TestPack.json"), orig_version_json)
            .await
            .unwrap();
        tokio::fs::write(old_dir.join("TestPack.jar"), b"jar")
            .await
            .unwrap();
        tokio::fs::write(old_dir.join("TestPack.json.bak"), b"bak")
            .await
            .unwrap();

        let child_a_dir = versions_dir.join("ChildA");
        tokio::fs::create_dir_all(&child_a_dir).await.unwrap();
        let orig_child_a = r#"{ "id": "ChildA", "inheritsFrom": "TestPack" }"#;
        tokio::fs::write(child_a_dir.join("ChildA.json"), orig_child_a)
            .await
            .unwrap();

        let mut plan =
            prepare_rename_commit_plan(&versions_dir, &old_dir, &new_dir, "TestPack", "testpack")
                .await
                .expect("prepare case-only plan");

        let blocker_file = versions_dir.join("write_blocker_file");
        tokio::fs::write(&blocker_file, b"not-a-dir")
            .await
            .unwrap();
        plan.dependent_updates.push(DependentJsonUpdate {
            json_path: blocker_file.join("ChildB.json"),
            orig_bytes: Vec::new(),
            updated_bytes: br#"{"id":"ChildB","inheritsFrom":"testpack"}"#.to_vec(),
        });

        let err = commit_rename_plan(&old_dir, &new_dir, "TestPack", "testpack", true, plan)
            .await
            .expect_err("failing dependent write must fail case-only commit");
        assert!(err.contains("inheritsFrom"));

        // Verify exact casing of directory and internal files is restored
        let mut version_entries = Vec::new();
        let mut rd = tokio::fs::read_dir(&versions_dir).await.unwrap();
        while let Some(e) = rd.next_entry().await.unwrap() {
            version_entries.push(e.file_name().to_string_lossy().to_string());
        }
        assert!(version_entries.contains(&"TestPack".to_string()));
        assert!(!version_entries.contains(&"testpack".to_string()));

        let mut inst_entries = Vec::new();
        let mut inst_rd = tokio::fs::read_dir(&old_dir).await.unwrap();
        while let Some(e) = inst_rd.next_entry().await.unwrap() {
            inst_entries.push(e.file_name().to_string_lossy().to_string());
        }
        assert!(inst_entries.contains(&"TestPack.json".to_string()));
        assert!(inst_entries.contains(&"TestPack.jar".to_string()));
        assert!(inst_entries.contains(&"TestPack.json.bak".to_string()));
        assert!(!inst_entries.contains(&"testpack.json".to_string()));
        assert!(!inst_entries.contains(&"testpack.jar".to_string()));
        assert!(!inst_entries.contains(&"testpack.json.bak".to_string()));
        assert!(!inst_entries.iter().any(|n| n.starts_with(".dlml_rename_")));

        assert_eq!(
            tokio::fs::read_to_string(old_dir.join("TestPack.json"))
                .await
                .unwrap(),
            orig_version_json
        );
        assert_eq!(
            tokio::fs::read_to_string(child_a_dir.join("ChildA.json"))
                .await
                .unwrap(),
            orig_child_a
        );
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn test_rename_instance_in_dir_readonly_modpack_tasks_and_dependent_propagate_and_rollback(
    ) {
        use tempfile::tempdir;

        let temp = tempdir().expect("tempdir");
        let versions_dir = temp.path().join("versions");
        let old_dir = versions_dir.join("1.20.1");
        tokio::fs::create_dir_all(old_dir.join("mods"))
            .await
            .unwrap();

        let orig_version_json = r#"{ "id": "1.20.1", "type": "release" }"#;
        tokio::fs::write(old_dir.join("1.20.1.json"), orig_version_json)
            .await
            .unwrap();
        tokio::fs::write(old_dir.join("1.20.1.jar"), b"jar")
            .await
            .unwrap();
        let orig_dlml = r#"{ "name": "1.20.1", "status": "ready" }"#;
        tokio::fs::write(old_dir.join("dlml.json"), orig_dlml)
            .await
            .unwrap();

        let tasks = vec![crate::downloader::DownloadTask::new(
            "https://example.com/sodium.jar".to_string(),
            old_dir
                .join("mods")
                .join("sodium.jar")
                .to_string_lossy()
                .to_string(),
            None,
            None,
        )];
        let orig_tasks = serde_json::to_string_pretty(&tasks).unwrap();
        let tasks_path = old_dir.join("modpack_tasks.json");
        tokio::fs::write(&tasks_path, &orig_tasks).await.unwrap();

        let mut tasks_perms = tokio::fs::metadata(&tasks_path).await.unwrap().permissions();
        tasks_perms.set_readonly(true);
        tokio::fs::set_permissions(&tasks_path, tasks_perms)
            .await
            .unwrap();

        let tasks_err = rename_instance_in_dir(&versions_dir, "1.20.1", "SurvivalPack")
            .await
            .expect_err("read-only modpack_tasks.json write failure must propagate");
        assert!(tasks_err.contains("modpack_tasks.json"));

        // Clear read-only before asserting / next phase
        let mut tasks_perms_rw = tokio::fs::metadata(&tasks_path).await.unwrap().permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        tasks_perms_rw.set_readonly(false);
        tokio::fs::set_permissions(&tasks_path, tasks_perms_rw)
            .await
            .unwrap();

        assert!(old_dir.exists());
        assert!(!versions_dir.join("SurvivalPack").exists());
        assert_eq!(
            tokio::fs::read_to_string(old_dir.join("1.20.1.json"))
                .await
                .unwrap(),
            orig_version_json
        );
        assert_eq!(
            tokio::fs::read_to_string(old_dir.join("dlml.json"))
                .await
                .unwrap(),
            orig_dlml
        );
        assert_eq!(
            tokio::fs::read_to_string(&tasks_path).await.unwrap(),
            orig_tasks
        );

        // Now verify read-only dependent instance JSON failure also propagates and rolls back everything
        let child_a_dir = versions_dir.join("ChildA");
        let child_b_dir = versions_dir.join("ChildB");
        tokio::fs::create_dir_all(&child_a_dir).await.unwrap();
        tokio::fs::create_dir_all(&child_b_dir).await.unwrap();
        let orig_child_a = r#"{ "id": "ChildA", "inheritsFrom": "1.20.1" }"#;
        let orig_child_b = r#"{ "id": "ChildB", "inheritsFrom": "1.20.1" }"#;
        tokio::fs::write(child_a_dir.join("ChildA.json"), orig_child_a)
            .await
            .unwrap();
        let child_b_json = child_b_dir.join("ChildB.json");
        tokio::fs::write(&child_b_json, orig_child_b).await.unwrap();

        let mut child_b_perms = tokio::fs::metadata(&child_b_json)
            .await
            .unwrap()
            .permissions();
        child_b_perms.set_readonly(true);
        tokio::fs::set_permissions(&child_b_json, child_b_perms)
            .await
            .unwrap();

        let dep_err = rename_instance_in_dir(&versions_dir, "1.20.1", "SurvivalPack")
            .await
            .expect_err("read-only dependent JSON write failure must propagate");
        assert!(dep_err.contains("inheritsFrom"));

        let mut child_b_perms_rw = tokio::fs::metadata(&child_b_json)
            .await
            .unwrap()
            .permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        child_b_perms_rw.set_readonly(false);
        tokio::fs::set_permissions(&child_b_json, child_b_perms_rw)
            .await
            .unwrap();

        assert!(old_dir.exists());
        assert!(!versions_dir.join("SurvivalPack").exists());
        assert_eq!(
            tokio::fs::read_to_string(old_dir.join("1.20.1.json"))
                .await
                .unwrap(),
            orig_version_json
        );
        assert_eq!(
            tokio::fs::read_to_string(old_dir.join("dlml.json"))
                .await
                .unwrap(),
            orig_dlml
        );
        assert_eq!(
            tokio::fs::read_to_string(&tasks_path).await.unwrap(),
            orig_tasks
        );
        assert_eq!(
            tokio::fs::read_to_string(child_a_dir.join("ChildA.json"))
                .await
                .unwrap(),
            orig_child_a
        );
        assert_eq!(
            tokio::fs::read_to_string(&child_b_json).await.unwrap(),
            orig_child_b
        );
    }
}

async fn download_mod_file_task(
    ctx: &crate::core::task::TaskContext,
    client: &reqwest::Client,
    url: &str,
    target_path: &std::path::Path,
) -> Result<(), String> {
    use futures_util::StreamExt;
    use tokio::io::AsyncWriteExt;

    // Validate URL scheme and host
    let parsed_url = reqwest::Url::parse(url).map_err(|e| format!("Invalid download URL: {}", e))?;
    validate_preset_download_url(&parsed_url)?;

    let mut response = client
        .get(parsed_url)
        .send()
        .await
        .map_err(|e| format!("Failed to download file: {}", e))?;

    // Validate URL after any redirects
    validate_preset_download_url(response.url())?;

    if !response.status().is_success() {
        return Err(format!(
            "Download failed with status: {}",
            response.status()
        ));
    }

    let content_length = response.content_length().unwrap_or(0);
    ctx.update_progress(0, content_length, "Starting download...")
        .await;

    let target_dir = target_path.parent().ok_or_else(|| "Invalid target path parent".to_string())?;
    tokio::fs::create_dir_all(target_dir).await.map_err(|e| e.to_string())?;

    let file_stem = target_path.file_name().unwrap_or_default().to_string_lossy();
    let temp_path = target_dir.join(format!(".{}_{}.tmp", file_stem, uuid::Uuid::new_v4().simple()));

    let mut file = match tokio::fs::File::create(&temp_path).await {
        Ok(f) => f,
        Err(e) => return Err(format!("Failed to create temporary download file: {}", e)),
    };

    let mut downloaded = 0;
    let write_res: Result<(), String> = async {
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|e| format!("Chunk error: {}", e))?
        {
            if ctx.is_cancelled() {
                return Err("Cancelled".to_string());
            }
            file.write_all(&chunk)
                .await
                .map_err(|e| format!("Write error: {}", e))?;

            downloaded += chunk.len() as u64;
            ctx.update_progress(downloaded, content_length, "Downloading...")
                .await;
        }

        file.flush()
            .await
            .map_err(|e| format!("Flush error: {}", e))?;

        drop(file);
        Ok(())
    }.await;

    if let Err(e) = write_res {
        let _ = tokio::fs::remove_file(&temp_path).await;
        return Err(e);
    }

    if content_length > 0 && downloaded != content_length {
        let _ = tokio::fs::remove_file(&temp_path).await;
        return Err(format!("Download incomplete: expected {} bytes, got {}", content_length, downloaded));
    }

    if target_path.is_dir() {
        let _ = tokio::fs::remove_dir_all(target_path).await;
    } else if target_path.exists() {
        let _ = tokio::fs::remove_file(target_path).await;
    }

    if let Err(e) = tokio::fs::rename(&temp_path, target_path).await {
        let _ = tokio::fs::remove_file(&temp_path).await;
        return Err(format!("Failed to finalize download file: {}", e));
    }

    let final_len = if content_length == 0 {
        100
    } else {
        content_length
    };
    ctx.update_progress(final_len, final_len, "Download completed")
        .await;
    Ok(())
}

pub struct InstallPresetTask {
    pub options: InstallPresetOptions,
    pub app: tauri::AppHandle,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct InstallPresetOptions {
    pub preset_name: String,
    pub asset_type: String,
    pub instance_id: String,
    pub mods: Vec<crate::core::manager::ResolvedPresetMod>,
}

#[async_trait::async_trait]
impl crate::core::task::ExecutableTask for InstallPresetTask {
    async fn execute(
        &self,
        ctx: crate::core::task::TaskContext,
    ) -> Result<(), crate::core::task::TaskError> {
        let options = &self.options;
        if !crate::core::fs_utils::is_safe_single_name(&options.instance_id) {
            return Err(crate::core::task::TaskError::ExecutionError("Invalid instance ID".to_string()));
        }

        let mut sub_tasks = Vec::new();
        for m in &options.mods {
            let name = if !m.project_name.is_empty() { m.project_name.clone() } else { m.project_id.clone() };
            sub_tasks.push(crate::core::task::state::SubTaskState {
                key: m.project_id.clone(),
                name,
                status: crate::core::task::state::SubTaskStatus::Pending,
                current: 0,
                total: 100,
                weight: 10,
            });
        }
        ctx.init_sub_tasks(sub_tasks).await;

        use futures::StreamExt;
        
        let mods = options.mods.clone();
        let instance_id = options.instance_id.clone();
        let asset_type = options.asset_type.clone();
        let app = self.app.clone();

        let results = futures::stream::iter(mods)
            .map(|m| {
                let app = app.clone();
                let ctx = ctx.with_sub_task(&m.project_id);
                let instance_id = instance_id.clone();
                let asset_type = asset_type.clone();
                async move {
                    ctx.update_progress(0, 100, "Starting download...").await;

                    if ctx.is_cancelled() {
                        return Err(crate::core::task::TaskError::ExecutionError("Cancelled".to_string()));
                    }

                    match asset_type.as_str() {
                        "shaderpacks" | "resourcepacks" => {
                            let folder_name = if asset_type == "shaderpacks" { "shaderpacks" } else { "resourcepacks" };
                            let category = if asset_type == "shaderpacks" {
                                crate::models::instance::AssetCategory::ShaderPack
                            } else {
                                crate::models::instance::AssetCategory::ResourcePack
                            };
                            let base_dir = crate::core::mojang::get_minecraft_base();
                            let target_dir = base_dir.join("versions").join(&instance_id).join(folder_name);
                            if !target_dir.exists() {
                                tokio::fs::create_dir_all(&target_dir).await.map_err(|e| crate::core::task::TaskError::ExecutionError(e.to_string()))?;
                            }
                            let raw_filename = if !m.filename.is_empty() {
                                m.filename.clone()
                            } else {
                                extract_filename_from_url(&m.download_url, &m.project_id)
                            };
                            let clean_filename = std::path::Path::new(&raw_filename)
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or("");
                            if clean_filename.is_empty() || !crate::core::fs_utils::is_safe_single_name(clean_filename) {
                                return Err(crate::core::task::TaskError::ExecutionError(format!("Invalid filename '{}'", raw_filename)));
                            }
                            let target_path = target_dir.join(clean_filename);
                            if !target_path.starts_with(&target_dir) {
                                return Err(crate::core::task::TaskError::ExecutionError("Target path escapes asset directory".to_string()));
                            }

                            if let Ok(meta) = tokio::fs::symlink_metadata(&target_path).await {
                                if meta.file_type().is_symlink() {
                                    return Err(crate::core::task::TaskError::ExecutionError(format!("Destination '{}' is a symlink and cannot be overwritten", clean_filename)));
                                }
                            }

                            let path_lock = get_path_lock(&target_path);
                            let _path_guard = path_lock.lock().await;

                            let parsed_download_url = reqwest::Url::parse(&m.download_url)
                                .map_err(|e| crate::core::task::TaskError::ExecutionError(format!("Invalid download URL: {}", e)))?;
                            validate_preset_download_url(&parsed_download_url)
                                .map_err(crate::core::task::TaskError::ExecutionError)?;

                            let mut matches_existing = false;
                            if target_path.exists() {
                                let rel_key = format!("{}/{}", folder_name, clean_filename);
                                let manifest_record = {
                                    let _manifest_guard = MANIFEST_LOCK.lock().await;
                                    let manifest_path = base_dir.join("versions").join(&instance_id).join("assets.json");
                                    if manifest_path.exists() {
                                        tokio::fs::read_to_string(&manifest_path)
                                            .await
                                            .ok()
                                            .and_then(|content| serde_json::from_str::<crate::models::instance::AssetManifest>(&content).ok())
                                            .and_then(|manifest| manifest.assets.get(&rel_key).cloned())
                                    } else {
                                        None
                                    }
                                };

                                let (project_matches, record_size, record_hash) = if let Some(ref record) = manifest_record {
                                    let p_match = record.project_id.as_deref() == Some(&m.project_id);
                                    let u_match = record.download_url.as_deref() == Some(&m.download_url);
                                    let v_match = !m.file_id.is_empty() && record.version_id.as_deref() == Some(&m.file_id);
                                    (p_match && (u_match || v_match), record.size, record.hash_sha1.clone())
                                } else {
                                    (false, None, None)
                                };

                                let has_verification_criterion = m.file_size.is_some() || record_size.is_some() || m.hash.is_some() || record_hash.is_some();

                                if has_verification_criterion {
                                    let mut verified = manifest_record.is_none() || project_matches;

                                    if verified {
                                        let expected_size = m.file_size.or(record_size);
                                        if let Some(expected_size) = expected_size {
                                            if let Ok(meta) = tokio::fs::metadata(&target_path).await {
                                                if meta.len() != expected_size {
                                                    verified = false;
                                                }
                                            } else {
                                                verified = false;
                                            }
                                        }
                                    }

                                    if verified {
                                        let expected_hash = m.hash.as_ref().or(record_hash.as_ref());
                                        if let Some(expected_hash) = expected_hash {
                                            let p = target_path.clone();
                                            match tokio::task::spawn_blocking(move || crate::downloader::download::compute_sha1_sync(&p)).await {
                                                Ok(Ok(actual_hash)) => {
                                                    if !actual_hash.eq_ignore_ascii_case(expected_hash) {
                                                        verified = false;
                                                    }
                                                }
                                                _ => {
                                                    verified = false;
                                                }
                                            }
                                        }
                                    }

                                    matches_existing = verified;
                                }
                            }

                            if !matches_existing {
                                download_mod_file_task(&ctx, &PRESET_DOWNLOAD_HTTP_CLIENT, &m.download_url, &target_path)
                                    .await
                                    .map_err(crate::core::task::TaskError::ExecutionError)?;
                            } else {
                                ctx.update_progress(100, 100, "File already exists and matches").await;
                            }

                            register_manual_asset_full(
                                &instance_id,
                                &format!("{}/{}", folder_name, clean_filename),
                                category,
                                &m.source,
                                &m.project_id,
                                if m.file_id.is_empty() { None } else { Some(&m.file_id) },
                                &m.download_url,
                            ).await.map_err(crate::core::task::TaskError::ExecutionError)?;
                        }
                        _ => {
                            let parsed_download_url = reqwest::Url::parse(&m.download_url)
                                .map_err(|e| crate::core::task::TaskError::ExecutionError(format!("Invalid download URL: {}", e)))?;
                            validate_preset_download_url(&parsed_download_url)
                                .map_err(crate::core::task::TaskError::ExecutionError)?;

                            let result = install_mod_to_instance(
                                app,
                                InstallModOptions {
                                    source: m.source.clone(),
                                    project_id: m.project_id.clone(),
                                    mod_name: Some(m.project_name.clone()),
                                    instance_id: Some(instance_id),
                                    target_dir: None,
                                    download_url: m.download_url.clone(),
                                    file_id: m.file_id.clone(),
                                    dependencies: m.dependencies.clone(),
                                    keep_both: Some(false),
                                },
                                Some(ctx.clone()),
                            ).await;

                            if let Err(e) = result {
                                tracing::error!("Failed to install preset mod {}: {}", m.project_id, e);
                                return Err(crate::core::task::TaskError::ExecutionError(format!("Failed to install {}: {}", m.project_id, e)));
                            }
                        }
                    }

                    ctx.update_progress(100, 100, "Completed").await;
                    Ok(())
                }
            })
            .buffered(1) // Execute sequentially to prevent instance config race conditions
            .collect::<Vec<Result<(), crate::core::task::TaskError>>>()
            .await;

        for res in results {
            res?;
        }

        Ok(())
    }
}

pub struct InstallModTask {
    pub options: InstallModOptions,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct InstallModOptions {
    pub source: String,
    pub project_id: String,
    pub mod_name: Option<String>,
    pub instance_id: Option<String>,
    pub target_dir: Option<String>,
    pub download_url: String,
    pub file_id: String,
    pub dependencies: Option<Vec<crate::core::modrinth::UnifiedDependency>>,
    pub keep_both: Option<bool>,
}

pub struct DownloadDependencyContext<'a> {
    pub dep: &'a crate::core::modrinth::UnifiedDependency,
    pub dep_ctx: &'a crate::core::task::TaskContext,
    pub client: &'a reqwest::Client,
    pub target_dir_path: &'a std::path::Path,
    pub version_id_opt: &'a Option<String>,
    pub mc_version: &'a str,
    pub loader: &'a str,
}

impl InstallModTask {
    fn get_required_dependencies(&self) -> Vec<crate::core::modrinth::UnifiedDependency> {
        self.options.dependencies.clone().unwrap_or_default().into_iter().filter(|d| d.required).collect()
    }

    fn prepare_sub_tasks(&self) -> Vec<crate::core::task::state::SubTaskState> {
        let options = &self.options;
        let mut sub_tasks = vec![crate::core::task::state::SubTaskState {
            key: "main".to_string(),
            name: format!("{} (Main)", options.mod_name.clone().unwrap_or_else(|| options.project_id.clone())),
            status: crate::core::task::state::SubTaskStatus::Pending,
            current: 0,
            total: 100,
            weight: 50,
        }];

        let req_deps = self.get_required_dependencies();
        let dep_weight = if req_deps.is_empty() { 0 } else { 50 / req_deps.len() as u32 };

        for dep in req_deps {
            sub_tasks.push(crate::core::task::state::SubTaskState {
                key: dep.project_id.clone(),
                name: format!("Dependency: {}", dep.name.clone().unwrap_or_else(|| dep.project_id.clone())),
                status: crate::core::task::state::SubTaskStatus::Pending,
                current: 0,
                total: 100,
                weight: dep_weight,
            });
        }
        sub_tasks
    }

    async fn get_target_dir(&self) -> Result<(std::path::PathBuf, Option<String>), crate::core::task::TaskError> {
        if let Some(vid) = &self.options.instance_id {
            let base_dir = crate::core::mojang::get_minecraft_base();
            let mods_dir = base_dir.join("versions").join(vid).join("mods");
            if !mods_dir.exists() { tokio::fs::create_dir_all(&mods_dir).await.ok(); }
            Ok((mods_dir, Some(vid.clone())))
        } else if let Some(td) = &self.options.target_dir {
            let p = std::path::PathBuf::from(td);
            if !p.exists() { tokio::fs::create_dir_all(&p).await.ok(); }
            Ok((p, None))
        } else {
            Err(crate::core::task::TaskError::ExecutionError("No target directory or instance specified".to_string()))
        }
    }

    async fn download_main_mod(
        &self,
        main_ctx: &crate::core::task::TaskContext,
        client: &reqwest::Client,
        target_dir_path: &std::path::Path,
        version_id_opt: &Option<String>,
    ) -> Result<(), crate::core::task::TaskError> {
        let options = &self.options;
        let filename = extract_filename_from_url(&options.download_url, &options.project_id);
        let target_path = target_dir_path.join(&filename);
        let mut installed_map = if let Some(vid) = version_id_opt { load_installed_mods_json(vid).await } else { std::collections::HashMap::new() };
        let mod_key = format!("{}_{}", options.source, options.project_id);

        if let Some(vid) = version_id_opt {
            if !options.keep_both.unwrap_or(false) {
                if let Some((old_filename, _)) = installed_map.get(&mod_key) {
                    if old_filename != &filename {
                        let _ = tokio::fs::remove_file(target_dir_path.join(old_filename)).await;
                        let _ = tokio::fs::remove_file(target_dir_path.join(format!("{}.disable", old_filename))).await;
                    }
                }
            }
        }

        if !target_path.exists() {
            download_mod_file_task(main_ctx, client, &options.download_url, &target_path).await.map_err(crate::core::task::TaskError::ExecutionError)?;
        } else {
            main_ctx.update_progress(100, 100, "File already exists").await;
        }

        if let Some(vid) = version_id_opt {
            installed_map.retain(|_, (v, _)| v != &filename);
            installed_map.insert(mod_key, (filename, Some(options.download_url.clone())));
            save_installed_mods_json(vid, &installed_map).await;
        }
        Ok(())
    }

    async fn download_dependencies(
        &self,
        ctx: &crate::core::task::TaskContext,
        client: &reqwest::Client,
        target_dir_path: &std::path::Path,
        version_id_opt: &Option<String>,
    ) -> Result<(), crate::core::task::TaskError> {
        let req_deps = self.get_required_dependencies();
        if req_deps.is_empty() { return Ok(()); }

        let (mc_version, loader) = if let Some(vid) = version_id_opt {
            if let Ok(item) = get_instance_details(vid.clone()).await {
                (item.mc_version, item.loader_type)
            } else { ("".to_string(), "".to_string()) }
        } else { ("".to_string(), "".to_string()) };

        let mut installed_map = if let Some(vid) = version_id_opt { load_installed_mods_json(vid).await } else { std::collections::HashMap::new() };

        for dep in req_deps {
            if ctx.is_cancelled() { return Err(crate::core::task::TaskError::ExecutionError("Cancelled".to_string())); }
            let dep_ctx = ctx.with_sub_task(&dep.project_id);
            dep_ctx.update_progress(0, 100, "Resolving dependency...").await;

            let ctx_info = DownloadDependencyContext {
                dep: &dep,
                dep_ctx: &dep_ctx,
                client,
                target_dir_path,
                version_id_opt,
                mc_version: &mc_version,
                loader: &loader,
            };
            let result = self.download_single_dependency(ctx_info, &mut installed_map).await;
            if let Err(e) = result { tracing::warn!("Failed to resolve dependency {}: {:?}", dep.project_id, e); }
        }
        Ok(())
    }

    async fn download_single_dependency(
        &self,
        ctx: DownloadDependencyContext<'_>,
        installed_map: &mut std::collections::HashMap<String, (String, Option<String>)>,
    ) -> Result<(), crate::core::task::TaskError> {
        let dep_key = format!("{}_{}", self.options.source, ctx.dep.project_id);
        let has_instance = ctx.version_id_opt.is_some();
        
        let (url, fname) = if self.options.source == "modrinth" {
            self.resolve_modrinth_dep(ctx.dep, ctx.client, has_instance, ctx.mc_version, ctx.loader).await?
        } else if self.options.source == "curseforge" {
            self.resolve_curseforge_dep(ctx.dep, ctx.client, has_instance, ctx.mc_version, ctx.loader).await?
        } else {
            return Err(crate::core::task::TaskError::ExecutionError("Unknown source".to_string()));
        };
        if !crate::core::fs_utils::is_safe_single_name(&fname) {
            return Err(crate::core::task::TaskError::ExecutionError(format!(
                "Unsafe dependency filename rejected: {}",
                fname
            )));
        }

        if let Some(v_id) = ctx.version_id_opt {
            if let Some((old_fname, _)) = installed_map.get(&dep_key) {
                if old_fname != &fname {
                    let _ = tokio::fs::remove_file(ctx.target_dir_path.join(old_fname)).await;
                    let _ = tokio::fs::remove_file(ctx.target_dir_path.join(format!("{}.disable", old_fname))).await;
                }
            }
        }

        let dep_path = ctx.target_dir_path.join(&fname);
        if !dep_path.exists() {
            download_mod_file_task(ctx.dep_ctx, ctx.client, &url, &dep_path).await.map_err(crate::core::task::TaskError::ExecutionError)?;
        } else {
            ctx.dep_ctx.update_progress(100, 100, "File exists").await;
        }

        if let Some(v_id) = ctx.version_id_opt {
            installed_map.retain(|_, (v, _)| v != &fname);
            installed_map.insert(dep_key, (fname, Some(url.to_string())));
            save_installed_mods_json(v_id, installed_map).await;
        }
        Ok(())
    }

    async fn resolve_modrinth_dep(
        &self,
        dep: &crate::core::modrinth::UnifiedDependency,
        client: &reqwest::Client,
        has_instance: bool,
        mc_version: &str,
        loader: &str,
    ) -> Result<(String, String), crate::core::task::TaskError> {
        if let Some(vid) = &dep.version_id {
            if let Ok(res) = client.get(format!("https://api.modrinth.com/v2/version/{}", vid)).send().await {
                if let Ok(json) = res.json::<serde_json::Value>().await {
                    if let Some(files) = json.get("files").and_then(|f| f.as_array()) {
                        if let Some(primary) = files.first() {
                            if let (Some(url), Some(fname)) = (primary.get("url").and_then(|u| u.as_str()), primary.get("filename").and_then(|f| f.as_str())) {
                                return Ok((url.to_string(), fname.to_string()));
                            }
                        }
                    }
                }
            }
            Err(crate::core::task::TaskError::ExecutionError("Modrinth dep version files not found".to_string()))
        } else {
            let pid = &dep.project_id;
            if has_instance && !mc_version.is_empty() {
                if let Ok(files) = crate::core::modrinth::get_modrinth_mod_files(pid.clone(), mc_version.to_string(), vec![loader.to_string()]).await {
                    if let Some(primary) = files.first() {
                        return Ok((primary.download_url.clone(), primary.filename.clone()));
                    }
                }
            } else {
                if let Ok(res) = client.get(format!("https://api.modrinth.com/v2/project/{}/version", pid)).send().await {
                    if let Ok(versions) = res.json::<Vec<serde_json::Value>>().await {
                        if let Some(latest_ver) = versions.first() {
                            if let Some(files) = latest_ver.get("files").and_then(|f| f.as_array()) {
                                if let Some(primary) = files.first() {
                                    if let (Some(url), Some(fname)) = (primary.get("url").and_then(|u| u.as_str()), primary.get("filename").and_then(|f| f.as_str())) {
                                        return Ok((url.to_string(), fname.to_string()));
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Err(crate::core::task::TaskError::ExecutionError("Modrinth dep mod files not found".to_string()))
        }
    }

    async fn resolve_curseforge_dep(
        &self,
        dep: &crate::core::modrinth::UnifiedDependency,
        client: &reqwest::Client,
        has_instance: bool,
        mc_version: &str,
        loader: &str,
    ) -> Result<(String, String), crate::core::task::TaskError> {
        if has_instance && !mc_version.is_empty() {
            if let Ok(files) = crate::core::curseforge::get_cf_mod_files(dep.project_id.clone(), mc_version.to_string(), vec![loader.to_string()]).await {
                if let Some(primary) = files.first() {
                    return Ok((primary.download_url.clone(), primary.filename.clone()));
                }
            }
            Err(crate::core::task::TaskError::ExecutionError("CF dep files not found".to_string()))
        } else {
            let cf_url = crate::core::curseforge::build_cf_url(&format!("/mods/{}/files", dep.project_id), None);
            if let Ok(req) = crate::core::curseforge::cf_request(client, reqwest::Method::GET, &cf_url) {
                if let Ok(res) = req.send().await {
                    if let Ok(files_res) = res.json::<crate::core::curseforge::CfFilesResponse>().await {
                        let mut sorted_files = files_res.data;
                        sorted_files.retain(|f| f.parent_project_file_id.unwrap_or(0) == 0 && !f.is_server_pack.unwrap_or(false));
                        sorted_files.sort_by_key(|b| std::cmp::Reverse(b.id));
                        if let Some(primary) = sorted_files.first() {
                            let fname = primary.file_name.clone();
                            let url = primary.download_url.clone().unwrap_or_else(|| crate::core::curseforge::get_fallback_download_url(primary.id, &fname));
                            return Ok((url, fname));
                        }
                    }
                }
            }
            Err(crate::core::task::TaskError::ExecutionError("CF dep files empty".to_string()))
        }
    }
}

#[async_trait::async_trait]
impl crate::core::task::ExecutableTask for InstallModTask {
    async fn execute(
        &self,
        ctx: crate::core::task::TaskContext,
    ) -> Result<(), crate::core::task::TaskError> {
        ctx.init_sub_tasks(self.prepare_sub_tasks()).await;
        let client = crate::core::utils::get_http_client();
        let (target_dir_path, version_id_opt) = self.get_target_dir().await?;

        self.download_main_mod(&ctx.with_sub_task("main"), client, &target_dir_path, &version_id_opt).await?;
        self.download_dependencies(&ctx, client, &target_dir_path, &version_id_opt).await?;

        Ok(())
    }
}

pub struct InstallDatapackTask {
    pub options: InstallDatapackOptions,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct InstallDatapackOptions {
    pub source: String,
    pub project_id: String,
    pub pack_name: String,
    pub instance_id: Option<String>,
    pub target_dir: Option<String>,
    pub download_url: String,
    pub file_id: String,
}

#[async_trait::async_trait]
impl crate::core::task::ExecutableTask for InstallDatapackTask {
    async fn execute(
        &self,
        ctx: crate::core::task::TaskContext,
    ) -> Result<(), crate::core::task::TaskError> {
        let options = &self.options;
        let client = crate::core::utils::get_http_client();

        let target_dir_path = if let Some(td) = &options.target_dir {
            let p = std::path::PathBuf::from(td);
            if !tokio::fs::try_exists(&p).await.unwrap_or(false) {
                tokio::fs::create_dir_all(&p).await.ok();
            }
            p
        } else {
            return Err(crate::core::task::TaskError::ExecutionError(
                "No target directory specified for datapack install".to_string(),
            ));
        };

        let filename = extract_filename_from_url(&options.download_url, &options.project_id);
        let target_path = target_dir_path.join(&filename);

        ctx.init_sub_tasks(vec![crate::core::task::state::SubTaskState {
            key: "download".to_string(),
            name: "Downloading Datapack".to_string(),
            status: crate::core::task::state::SubTaskStatus::Pending,
            current: 0,
            total: 100,
            weight: 100,
        }])
        .await;

        let sub_ctx = ctx.with_sub_task("download");
        download_mod_file_task(&sub_ctx, client, &options.download_url, &target_path)
            .await
            .map_err(crate::core::task::TaskError::ExecutionError)?;

        let mut components = target_dir_path.components();
        let mut found_versions = false;
        let mut instance_id_opt = None;
        
        while let Some(comp) = components.next() {
            if !found_versions && comp.as_os_str() == "versions" {
                found_versions = true;
                if let Some(std::path::Component::Normal(vid_os)) = components.next() {
                    instance_id_opt = Some(vid_os.to_string_lossy().to_string());
                    break;
                }
            }
        }
        
        if let Some(vid) = instance_id_opt {
            let rest: std::path::PathBuf = components.collect();
            let path_key = rest.join(&filename).to_string_lossy().replace("\\", "/");
            register_manual_asset_with_metadata(&vid, &path_key, crate::models::instance::AssetCategory::Datapack, &options.source, &options.project_id, &options.download_url).await;
        }

        Ok(())
    }
}

pub struct InstallResourcepackTask {
    pub options: InstallResourcepackOptions,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct InstallResourcepackOptions {
    pub source: String,
    pub project_id: String,
    pub pack_name: String,
    pub instance_id: Option<String>,
    pub target_dir: Option<String>,
    pub download_url: String,
    pub file_id: String,
}

#[async_trait::async_trait]
impl crate::core::task::ExecutableTask for InstallResourcepackTask {
    async fn execute(
        &self,
        ctx: crate::core::task::TaskContext,
    ) -> Result<(), crate::core::task::TaskError> {
        let options = &self.options;

        let client = crate::core::utils::get_http_client();

        let target_dir_path = if let Some(vid) = &options.instance_id {
            let base_dir = crate::core::mojang::get_minecraft_base();
            let rp_dir = base_dir.join("versions").join(vid).join("resourcepacks");
            if !rp_dir.exists() {
                tokio::fs::create_dir_all(&rp_dir).await.ok();
            }
            rp_dir
        } else if let Some(td) = &options.target_dir {
            let p = std::path::PathBuf::from(td);
            if !p.exists() {
                tokio::fs::create_dir_all(&p).await.ok();
            }
            p
        } else {
            return Err(crate::core::task::TaskError::ExecutionError(
                "No target directory or instance specified".to_string(),
            ));
        };

        let filename = extract_filename_from_url(&options.download_url, &options.project_id);
        let target_path = target_dir_path.join(&filename);

        if !target_path.exists() {
            download_mod_file_task(&ctx, client, &options.download_url, &target_path)
                .await
                .map_err(crate::core::task::TaskError::ExecutionError)?;
        } else {
            ctx.update_progress(100, 100, "File already exists").await;
        }

        if let Some(vid) = &options.instance_id {
            register_manual_asset_with_metadata(vid, &format!("resourcepacks/{}", filename), crate::models::instance::AssetCategory::ResourcePack, &options.source, &options.project_id, &options.download_url).await;
        }

        Ok(())
    }
}

pub struct InstallShaderpackTask {
    pub options: InstallShaderpackOptions,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct InstallShaderpackOptions {
    pub source: String,
    pub project_id: String,
    pub pack_name: String,
    pub instance_id: Option<String>,
    pub target_dir: Option<String>,
    pub download_url: String,
    pub file_id: String,
}

#[async_trait::async_trait]
impl crate::core::task::ExecutableTask for InstallShaderpackTask {
    async fn execute(
        &self,
        ctx: crate::core::task::TaskContext,
    ) -> Result<(), crate::core::task::TaskError> {
        let options = &self.options;

        let client = crate::core::utils::get_http_client();

        let target_dir_path = if let Some(vid) = &options.instance_id {
            let base_dir = crate::core::mojang::get_minecraft_base();
            let rp_dir = base_dir.join("versions").join(vid).join("shaderpacks");
            if !rp_dir.exists() {
                tokio::fs::create_dir_all(&rp_dir).await.ok();
            }
            rp_dir
        } else if let Some(td) = &options.target_dir {
            let p = std::path::PathBuf::from(td);
            if !p.exists() {
                tokio::fs::create_dir_all(&p).await.ok();
            }
            p
        } else {
            return Err(crate::core::task::TaskError::ExecutionError(
                "No target directory or instance specified".to_string(),
            ));
        };

        let filename = extract_filename_from_url(&options.download_url, &options.project_id);
        let target_path = target_dir_path.join(&filename);

        if !target_path.exists() {
            download_mod_file_task(&ctx, client, &options.download_url, &target_path)
                .await
                .map_err(crate::core::task::TaskError::ExecutionError)?;
        } else {
            ctx.update_progress(100, 100, "File already exists").await;
        }

        if let Some(vid) = &options.instance_id {
            register_manual_asset_with_metadata(vid, &format!("shaderpacks/{}", filename), crate::models::instance::AssetCategory::ShaderPack, &options.source, &options.project_id, &options.download_url).await;
        }

        Ok(())
    }
}

pub struct InstallWorldTask {
    pub options: InstallWorldOptions,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct InstallWorldOptions {
    pub source: String,
    pub project_id: String,
    pub pack_name: String,
    pub instance_id: Option<String>,
    pub target_dir: Option<String>,
    pub download_url: String,
    pub file_id: String,
}

async fn extract_world_zip(
    zip_path: std::path::PathBuf,
    target_dir_path: std::path::PathBuf,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let file = std::fs::File::open(&zip_path).map_err(|e| e.to_string())?;
        let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;

        for i in 0..archive.len() {
            let mut file = archive.by_index(i).map_err(|e| e.to_string())?;
            let outpath = match file.enclosed_name() {
                Some(path) => target_dir_path.join(path),
                None => continue,
            };

            if file.name().ends_with('/') {
                std::fs::create_dir_all(&outpath).map_err(|e| e.to_string())?;
            } else {
                if let Some(p) = outpath.parent() {
                    if !p.exists() {
                        std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
                    }
                }
                let mut outfile = std::fs::File::create(&outpath).map_err(|e| e.to_string())?;
                std::io::copy(&mut file, &mut outfile).map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }).await.map_err(|e| e.to_string())?
}

#[async_trait::async_trait]
impl crate::core::task::ExecutableTask for InstallWorldTask {
    async fn execute(
        &self,
        ctx: crate::core::task::TaskContext,
    ) -> Result<(), crate::core::task::TaskError> {
        let options = &self.options;
        let client = crate::core::utils::get_http_client();
        
        let target_dir_path = if let Some(vid) = &options.instance_id {
            let base_dir = crate::core::mojang::get_minecraft_base();
            let saves_dir = base_dir.join("versions").join(vid).join("saves");
            if !saves_dir.exists() { tokio::fs::create_dir_all(&saves_dir).await.ok(); }
            saves_dir
        } else if let Some(td) = &options.target_dir {
            let p = std::path::PathBuf::from(td);
            if !p.exists() { tokio::fs::create_dir_all(&p).await.ok(); }
            p
        } else {
            return Err(crate::core::task::TaskError::ExecutionError("No target directory or instance specified".to_string()));
        };

        let filename = extract_filename_from_url(&options.download_url, &options.project_id);
        let temp_dir = std::env::temp_dir().join(format!("dawnland_world_{}", options.file_id));
        if !temp_dir.exists() { tokio::fs::create_dir_all(&temp_dir).await.ok(); }
        let temp_zip_path = temp_dir.join(&filename);

        ctx.init_sub_tasks(vec![
            crate::core::task::state::SubTaskState {
                key: "download".to_string(), name: "Downloading World".to_string(),
                status: crate::core::task::state::SubTaskStatus::Pending, current: 0, total: 100, weight: 50,
            },
            crate::core::task::state::SubTaskState {
                key: "extract".to_string(), name: "Extracting World".to_string(),
                status: crate::core::task::state::SubTaskStatus::Pending, current: 0, total: 100, weight: 50,
            },
        ]).await;

        let download_ctx = ctx.with_sub_task("download");
        download_ctx.update_progress(0, 100, "Downloading world file...").await;
        download_mod_file_task(&download_ctx, client, &options.download_url, &temp_zip_path)
            .await.map_err(crate::core::task::TaskError::ExecutionError)?;

        let extract_ctx = ctx.with_sub_task("extract");
        extract_ctx.update_progress(0, 100, "Extracting world...").await;
        
        extract_world_zip(temp_zip_path.clone(), target_dir_path).await.map_err(crate::core::task::TaskError::ExecutionError)?;

        tokio::fs::remove_dir_all(temp_dir).await.ok();
        extract_ctx.update_progress(100, 100, "World extracted").await;

        if let Some(vid) = &options.instance_id {
            let folder_name = filename.trim_end_matches(".zip");
            register_manual_asset_with_metadata(vid, &format!("saves/{}", folder_name), crate::models::instance::AssetCategory::Save, &options.source, &options.project_id, &options.download_url).await;
        }
        ctx.update_progress(100, 100, "World installed successfully").await;

        Ok(())
    }
}

#[tauri::command]
pub async fn get_instance_mod_mapping(
    version_id: String,
) -> Result<std::collections::HashMap<String, String>, String> {
    let map = load_installed_mods_json(&version_id).await;
    let mut result = std::collections::HashMap::new();
    for (k, v) in map {
        result.insert(k, v.0);
    }
    
    if let Ok(mods) = get_installed_mods(version_id, Some(false)).await {
        for m in mods {
            if m.enabled {
                if let Some(id) = &m.mod_id {
                    result.insert(format!("modid_{}", id), m.filename.clone());
                }
                for pid in &m.provides {
                    result.insert(format!("modid_{}", pid), m.filename.clone());
                }
            }
        }
    }
    
    Ok(result)
}

#[tauri::command]
pub async fn download_mod_to_directory(
    mod_project: crate::core::modrinth::UnifiedModProject,
    file_id: String,
    target_dir: String,
    dependencies: Vec<crate::core::modrinth::UnifiedDependency>,
) -> Result<(), String> {
    Ok(())
}
