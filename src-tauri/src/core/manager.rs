//! Instance Manager - Local Instance Scanning Engine
//! Provides functionality to scan and manage installed game instances.

#![allow(dead_code)]
#![allow(unused_variables)]

use crate::core::mojang::get_minecraft_base;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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
    /// Whether this instance is currently being installed
    pub is_installing: bool,
    /// Whether this instance is currently being updated
    pub is_updating: bool,
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

/// Scan all locally installed instances from the versions directory.
#[tauri::command]
pub async fn scan_installed_instances(
    task_manager: tauri::State<'_, crate::core::task::TaskManager>,
) -> Result<Vec<InstanceItem>, String> {
    tracing::info!("Scanning installed instances...");

    let base_dir = get_minecraft_base();
    let versions_dir = base_dir.join("versions");

    if !versions_dir.exists() {
        tracing::info!("Versions directory does not exist, returning empty list");
        return Ok(Vec::new());
    }

    let mut instances = Vec::new();

    let mut entries = tokio::fs::read_dir(&versions_dir)
        .await
        .map_err(|e| format!("Failed to read versions directory: {}", e))?;

    // Load all tasks from DB once to avoid locking inside loop
    let tasks = task_manager.load_history().await.unwrap_or_default();

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
                let mut is_installing = false;
                let mut is_updating = false;
                let mut server_id = None;
                let mut pack_version_id = None;
                let mut pack_file_name = None;

                if config_path.exists() {
                    if let Ok(content) = tokio::fs::read_to_string(&config_path).await {
                        if let Ok(config) =
                            serde_json::from_str::<crate::core::launcher::InstanceConfig>(&content)
                        {
                            is_hidden = config.hidden;
                            is_installing = config.is_installing;
                            is_updating = config.is_updating;
                            server_id = config.server_id;
                            pack_version_id = config.pack_version_id;
                            pack_file_name = config.pack_file_name;
                        }
                    }
                }

                let mut has_valid_task = false;
                for task in &tasks {
                    if let Some(tid) = task.task_type.instance_id() {
                        if tid == id && matches!(task.status, crate::core::task::TaskStatus::Pending | crate::core::task::TaskStatus::Running | crate::core::task::TaskStatus::Paused) {
                            has_valid_task = true;
                            if task.task_type.is_update() {
                                is_updating = true;
                            }
                            break;
                        }
                    }
                }

                // If it is installing or updating, check if its corresponding task exists and is not cancelled
                if (is_installing || is_updating) && !has_valid_task {
                    // For first time installation, always clean up zombie instances (even if they have partial data from extraction)
                    if is_installing && !is_updating {
                        tracing::info!("Cleaning up zombie installing instance: {}", id);
                        let _ = tokio::fs::remove_dir_all(&path).await;
                    } else {
                        tracing::warn!("Instance {} is marked as updating. Skipping zombie auto-cleanup and rescuing it.", id);
                        if config_path.exists() {
                            if let Ok(content) = tokio::fs::read_to_string(&config_path).await {
                                if let Ok(mut config) = serde_json::from_str::<crate::core::launcher::InstanceConfig>(&content) {
                                    config.is_installing = false;
                                    config.is_updating = false;
                                    if let Ok(json) = serde_json::to_string_pretty(&config) {
                                        let _ = tokio::fs::write(&config_path, json).await;
                                    }
                                    is_installing = false;
                                    is_updating = false;
                                }
                            }
                        }
                    }
                    if is_installing {
                        continue;
                    }
                }

                if is_hidden {
                    continue;
                }

                // An instance MUST have its basic {id}.json file unless it is currently actively installing
                if !is_installing && !json_path.exists() {
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
                        // Parse basic info from JSON
                        let (
                            mut mc_version,
                            loader_type,
                            modpack_version,
                            modpack_type,
                            modpack_project_id,
                        ) = parse_version_json(&content, &id);

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

                        instances.push(InstanceItem {
                            id: id.clone(),
                            name: id.clone(),
                            mc_version,
                            loader_type,
                            modpack_version,
                            modpack_type,
                            modpack_project_id,
                            server_id,
                            pack_version_id,
                            pack_file_name,
                            is_installing,
                            is_updating,
                        });
                    }
                    Err(e) => {
                        tracing::warn!("Failed to read version JSON for {}: {}", id, e);
                        // Add with default values
                        instances.push(InstanceItem {
                            id: id.clone(),
                            name: id.clone(),
                            mc_version: extract_mc_version_from_id(&id).unwrap_or_default(),
                            loader_type: "Vanilla".to_string(),
                            modpack_version: None,
                            modpack_type: None,
                            modpack_project_id: None,
                            server_id,
                            pack_version_id,
                            pack_file_name,
                            is_installing,
                            is_updating,
                        });
                    }
                }
            }
        }
    }

    // Sort by name
    instances.sort_by(|a, b| a.name.cmp(&b.name));

    tracing::info!("Found {} installed instances", instances.len());
    Ok(instances)
}

/// Parse version JSON content to extract Minecraft version and loader type.
fn parse_version_json(
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

    let (mut mc_version, loader_type, modpack_version, modpack_type, modpack_project_id) =
        parse_version_json(&content, &version_id);

    // Resolve actual MC version
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

    // Read dlml.json for bindings
    let config_path = base_dir
        .join("versions")
        .join(&version_id)
        .join("dlml.json");
    let mut server_id = None;
    let mut pack_version_id = None;
    let mut pack_file_name = None;
    let mut is_installing = false;

    if config_path.exists() {
        if let Ok(content) = tokio::fs::read_to_string(&config_path).await {
            if let Ok(config) =
                serde_json::from_str::<crate::core::launcher::InstanceConfig>(&content)
            {
                server_id = config.server_id;
                pack_version_id = config.pack_version_id;
                pack_file_name = config.pack_file_name;
                is_installing = config.is_installing;
            }
        }
    }

    Ok(InstanceItem {
        id: version_id.clone(),
        name: version_id,
        mc_version,
        loader_type,
        modpack_version,
        modpack_type,
        modpack_project_id,
        server_id,
        pack_version_id,
        pack_file_name,
        is_installing,
        is_updating: false,
    })
}

/// Open the instance folder in the system file manager.
#[tauri::command]
pub async fn open_instance_folder(version_id: String) -> Result<(), String> {
    let base_dir = get_minecraft_base();
    let instance_dir = base_dir.join("versions").join(&version_id);

    // Ensure directory exists — create it if missing so the user sees an empty folder
    if !instance_dir.exists() {
        tokio::fs::create_dir_all(&instance_dir)
            .await
            .map_err(|e| format!("Failed to create instance directory: {}", e))?;
    }

    // Open in system file manager (Explorer / Finder / xdg-open)
    open::that(&instance_dir).map_err(|e| format!("Failed to open folder: {}", e))?;

    tracing::info!("Opened instance folder: {}", instance_dir.display());
    Ok(())
}

async fn check_instance_busy(
    version_id: &str,
    instance_dir: &std::path::Path,
    task_manager: &crate::core::task::TaskManager,
) -> bool {
    let config_path = instance_dir.join("dlml.json");
    if config_path.exists() {
        if let Ok(content) = tokio::fs::read_to_string(&config_path).await {
            if let Ok(config) = serde_json::from_str::<crate::core::launcher::InstanceConfig>(&content) {
                if config.is_installing || config.is_updating {
                    return true;
                }
            }
        }
    }
    
    let tasks = task_manager.load_history().await.unwrap_or_default();
    for task in &tasks {
        if let Some(tid) = task.task_type.instance_id() {
            if tid == version_id && matches!(task.status, crate::core::task::TaskStatus::Pending | crate::core::task::TaskStatus::Running | crate::core::task::TaskStatus::Paused) {
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
    task_manager: tauri::State<'_, crate::core::task::TaskManager>,
    running_instances: tauri::State<'_, crate::core::launcher::RunningInstances>,
) -> Result<(), String> {
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

    // Use blocking remove_dir_all since the recursive directory removal
    // doesn't work well with async
    tokio::task::spawn_blocking(move || {
        std::fs::remove_dir_all(&instance_dir)
            .map_err(|e| format!("Failed to delete instance directory: {}", e))
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))??;

    tracing::info!("Deleted instance: {}", version_id);
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
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct LocalDatapackItem {
    pub filename: String,
    pub is_dir: bool,
    pub size: u64,
}

#[tauri::command]
pub async fn get_installed_datapacks(version_id: String, world_name: String) -> Result<Vec<LocalDatapackItem>, String> {
    let base_dir = get_minecraft_base();
    let datapacks_dir = base_dir.join("versions").join(&version_id).join("saves").join(&world_name).join("datapacks");

    if !tokio::fs::try_exists(&datapacks_dir).await.unwrap_or(false) {
        return Ok(Vec::new());
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

        datapacks.push(LocalDatapackItem {
            filename,
            is_dir,
            size: metadata.len(),
        });
    }

    Ok(datapacks)
}

#[tauri::command]
pub async fn delete_local_datapack(version_id: String, world_name: String, filename: String) -> Result<(), String> {
    if filename.contains('/') || filename.contains('\\') || filename == ".." || filename == "." {
        return Err("Invalid filename".to_string());
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
    
    Ok(())
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalAssetItem {
    pub filename: String,
    pub is_dir: bool,
    pub size: u64,
}

async fn get_assets_in_dir(dir: std::path::PathBuf) -> Result<Vec<LocalAssetItem>, String> {
    if !tokio::fs::try_exists(&dir).await.unwrap_or(false) {
        return Ok(Vec::new());
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

        assets.push(LocalAssetItem {
            filename,
            is_dir: metadata.is_dir(),
            size,
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
    get_assets_in_dir(dir).await
}

#[tauri::command]
pub async fn delete_local_resourcepack(version_id: String, filename: String) -> Result<(), String> {
    let dir = get_minecraft_base().join("versions").join(&version_id).join("resourcepacks");
    delete_asset_in_dir(dir, filename).await
}

#[tauri::command]
pub async fn get_installed_shaders(version_id: String) -> Result<Vec<LocalAssetItem>, String> {
    let dir = get_minecraft_base().join("versions").join(&version_id).join("shaderpacks");
    get_assets_in_dir(dir).await
}

#[tauri::command]
pub async fn delete_local_shader(version_id: String, filename: String) -> Result<(), String> {
    let dir = get_minecraft_base().join("versions").join(&version_id).join("shaderpacks");
    delete_asset_in_dir(dir, filename).await
}

#[tauri::command]
pub async fn get_installed_worlds(version_id: String) -> Result<Vec<LocalAssetItem>, String> {
    let dir = get_minecraft_base().join("versions").join(&version_id).join("saves");
    get_assets_in_dir(dir).await
}

#[tauri::command]
pub async fn delete_local_world(version_id: String, world_name: String) -> Result<(), String> {
    let dir = get_minecraft_base().join("versions").join(&version_id).join("saves");
    delete_asset_in_dir(dir, world_name).await
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
        let path = entry.path();
        let is_valid = match asset_type.as_str() {
            "mod_groups" => path.extension().is_some_and(|e| e == "json"),
            "resourcepacks" | "shaderpacks" => path.extension().is_some_and(|e| e == "zip"),
            _ => false,
        };

        if path.is_file() && is_valid {
            let filename = entry.file_name().to_string_lossy().to_string();
            
            let metadata = entry.metadata().await.ok();
            let size_bytes = metadata.map(|m| m.len()).unwrap_or(0);
            
            results.push(LocalAssetItem {
                filename,
                is_dir: false,
                size: size_bytes,
            });
        }
    }
    
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
pub async fn resolve_preset_for_instance(preset_name: String, asset_type: String, version_id: String) -> Result<ResolvePresetResult, String> {
    if !["mod_groups", "shaderpacks", "resourcepacks"].contains(&asset_type.as_str()) {
        return Err("Invalid asset type".to_string());
    }
    
    // Get instance details
    let instance = get_instance_details(version_id.clone()).await?;
    let mc_version = instance.mc_version.clone();
    let loader = instance.loader_type.to_lowercase();
    
    let clean_name = preset_name.replace("/", "").replace("\\", "").replace(".json", "");
    let file_path = get_minecraft_base().join("global_assets").join(&asset_type).join(format!("{}.json", clean_name));
    
    let preset_content = tokio::fs::read_to_string(&file_path).await.map_err(|e| e.to_string())?;
    let preset: OnlinePreset = serde_json::from_str(&preset_content).map_err(|e| e.to_string())?;
    
    let mut resolved_mods = Vec::new();
    let mut failed_mods = Vec::new();
    let loaders = vec![loader.clone()];

    for pm in preset.mods {
        let files_result = if pm.source == "modrinth" {
            crate::core::modrinth::get_modrinth_mod_files(pm.project_id.clone(), mc_version.clone(), loaders.clone()).await
        } else if pm.source == "curseforge" {
            crate::core::curseforge::get_cf_mod_files(pm.project_id.clone(), mc_version.clone(), loaders.clone()).await
        } else {
            Err("Unknown source".to_string())
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


async fn resolve_mod_metadata(
    path: &std::path::Path,
    cache_key: &str,
    cache_entries: &mut std::collections::HashMap<String, crate::core::mod_parser::ModMetadata>,
    parser: &crate::core::mod_parser::ModParser,
    base_dir: &std::path::Path,
    skip_parsing: bool,
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
    let m = tokio::task::spawn_blocking(move || {
        let p = crate::core::mod_parser::ModParser::new(&base_dir_clone);
        p.parse_mod(&path_clone, &key_clone)
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

    let base_dir = get_minecraft_base();
    let mods_dir = base_dir.join("versions").join(&version_id).join("mods");

    if !mods_dir.exists() {
        tracing::info!("Mods directory does not exist for {}", version_id);
        return Ok(Vec::new());
    }

    let mut mods = Vec::new();

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
        let cache_key = format!("{}_{}_{}", actual_filename, metadata.len(), mtime);

        let meta = resolve_mod_metadata(&path, &cache_key, &mut cache_entries, &parser, &base_dir, skip_parsing.unwrap_or(false)).await;
        let icon_url = get_mod_icon_url(&meta, &parser, &cache_key);

        mods.push(LocalModItem {
            filename: actual_filename,
            enabled,
            size: metadata.len(),
            mod_id: meta.mod_id,
            name: meta.name,
            version: meta.version,
            icon_url,
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

    let base_dir = get_minecraft_base();
    let mods_dir = base_dir.join("versions").join(&version_id).join("mods");
    let enabled_file = mods_dir.join(&filename);
    let disabled_file = mods_dir.join(format!("{}.disable", filename));

    let (src_file, dst_file) = if enable {
        (disabled_file, enabled_file)
    } else {
        (enabled_file, disabled_file)
    };

    if !src_file.exists() {
        if dst_file.exists() {
            // Already in desired state
            return Ok(());
        }
        return Err(format!("Mod file not found: {}", src_file.display()));
    }

    if dst_file.exists() {
        return Err(format!(
            "Mod already exists in target location: {}",
            dst_file.display()
        ));
    }

    tokio::fs::rename(&src_file, &dst_file)
        .await
        .map_err(|e| format!("Failed to move mod file: {}", e))?;

    tracing::info!(
        "Toggled mod {} to enabled={} for instance {}",
        filename,
        enable,
        version_id
    );
    Ok(())
}

/// Delete a local mod file
#[tauri::command]
pub async fn delete_local_mod(
    version_id: String,
    filename: String,
    is_enabled: Option<bool>,
) -> Result<(), String> {
    tracing::info!("Deleting mod {} from instance {}", filename, version_id);

    let base_dir = get_minecraft_base();
    let mods_dir = base_dir.join("versions").join(&version_id).join("mods");

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

    // Only remove from dlml.json if both the enabled and disabled variants are gone
    if !mod_file.exists() && !disabled_file.exists() {
        let mut installed_map = load_installed_mods_json(&version_id).await;
        let initial_len = installed_map.len();
        installed_map.retain(|_, v| v != &filename);
        if installed_map.len() != initial_len {
            save_installed_mods_json(&version_id, &installed_map).await;
        }
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

    if decoded.is_empty() {
        format!("{}.jar", project_id)
    } else if !decoded.contains('.') {
        format!("{}.jar", decoded)
    } else {
        decoded
    }
}

/// Helper function to load and save installed mods directly into `dlml.json`
async fn load_installed_mods_json(instance_id: &str) -> std::collections::HashMap<String, String> {
    let base_dir = get_minecraft_base();
    let config_path = base_dir
        .join("versions")
        .join(instance_id)
        .join("dlml.json");
    if config_path.exists() {
        if let Ok(content) = tokio::fs::read_to_string(&config_path).await {
            if let Ok(config) =
                serde_json::from_str::<crate::core::launcher::InstanceConfig>(&content)
            {
                return config.installed_mods;
            }
        }
    }
    std::collections::HashMap::new()
}

async fn save_installed_mods_json(
    instance_id: &str,
    map: &std::collections::HashMap<String, String>,
) {
    let base_dir = get_minecraft_base();
    let config_path = base_dir
        .join("versions")
        .join(instance_id)
        .join("dlml.json");

    // Read existing config first to preserve other fields
    let mut config: crate::core::launcher::InstanceConfig = if config_path.exists() {
        tokio::fs::read_to_string(&config_path)
            .await
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    } else {
        crate::core::launcher::InstanceConfig::default()
    };

    config.installed_mods = map.clone();

    if let Ok(content) = serde_json::to_string_pretty(&config) {
        let _ = tokio::fs::write(&config_path, content).await;
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

    let client = reqwest::Client::new();

    let filename = extract_filename_from_url(&download_url, &project_id);
    let target_path = mods_dir.join(&filename);

    let mut installed_map = load_installed_mods_json(&version_id).await;
    let mod_key = format!("{}_{}", mod_source, project_id);

    // Remove old version if it exists and filename differs
    let keep_old = keep_both.unwrap_or(false);
    if !keep_old {
        if let Some(old_filename) = installed_map.get(&mod_key) {
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
    installed_map.retain(|_, v| v != &filename);
    installed_map.insert(mod_key, filename.clone());
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
                                            if let Some(old_fname) = installed_map.get(&dep_key) {
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
                                            installed_map.retain(|_, v| v != fname);
                                            installed_map.insert(dep_key, fname.to_string());
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

                                if let Some(old_fname) = installed_map.get(&dep_key) {
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
                                installed_map.retain(|_, v| v != fname);
                                installed_map.insert(dep_key, fname.to_string());
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

                            if let Some(old_fname) = installed_map.get(&dep_key) {
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
                            installed_map.retain(|_, v| v != fname);
                            installed_map.insert(dep_key, fname.to_string());
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

    let config_path = instance_dir.join("dlml.json");

    let mut config = if config_path.exists() {
        let content = tokio::fs::read_to_string(&config_path)
            .await
            .unwrap_or_else(|_| "{}".to_string());
        serde_json::from_str::<crate::core::launcher::InstanceConfig>(&content).unwrap_or_default()
    } else {
        crate::core::launcher::InstanceConfig::default()
    };

    config.server_id = Some(server_id);
    config.pack_version_id = pack_version_id;
    config.pack_file_name = pack_file_name;

    let json_str = serde_json::to_string_pretty(&config)
        .map_err(|e| format!("Failed to serialize dlml.json: {}", e))?;

    tokio::fs::write(&config_path, json_str)
        .await
        .map_err(|e| format!("Failed to write dlml.json: {}", e))?;

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
}

async fn download_mod_file_task(
    ctx: &crate::core::task::TaskContext,
    client: &reqwest::Client,
    url: &str,
    target_path: &std::path::Path,
) -> Result<(), String> {
    use futures_util::StreamExt;
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

    let content_length = response.content_length().unwrap_or(0);
    ctx.update_progress(0, content_length, "Starting download...")
        .await;

    let mut file = tokio::fs::File::create(target_path)
        .await
        .map_err(|e| format!("Failed to create mod file: {}", e))?;

    let mut downloaded = 0;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| format!("Chunk error: {}", e))?
    {
        if ctx.is_cancelled() {
            let _ = file.flush().await;
            let _ = tokio::fs::remove_file(target_path).await;
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
    let final_len = if content_length == 0 {
        100
    } else {
        content_length
    };
    ctx.update_progress(final_len, final_len, "Download finished")
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
        let app = self.app.clone();

        let results = futures::stream::iter(mods)
            .map(|m| {
                let app = app.clone();
                let ctx = ctx.with_sub_task(&m.project_id);
                let instance_id = instance_id.clone();
                async move {
                    ctx.update_progress(0, 100, "Starting download...").await;

                    if ctx.is_cancelled() {
                        return Err(crate::core::task::TaskError::ExecutionError("Cancelled".to_string()));
                    }

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

impl InstallModTask {
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

        let req_deps: Vec<_> = options.dependencies.clone().unwrap_or_default().into_iter().filter(|d| d.required).collect();
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
                if let Some(old_filename) = installed_map.get(&mod_key) {
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
            installed_map.retain(|_, v| v != &filename);
            installed_map.insert(mod_key, filename);
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
        let req_deps: Vec<_> = self.options.dependencies.clone().unwrap_or_default().into_iter().filter(|d| d.required).collect();
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

            let result = self.download_single_dependency(&dep, &dep_ctx, client, target_dir_path, version_id_opt, &mc_version, &loader, &mut installed_map).await;
            if let Err(e) = result { tracing::warn!("Failed to resolve dependency {}: {:?}", dep.project_id, e); }
        }
        Ok(())
    }

    async fn download_single_dependency(
        &self,
        dep: &crate::core::modrinth::UnifiedDependency,
        dep_ctx: &crate::core::task::TaskContext,
        client: &reqwest::Client,
        target_dir_path: &std::path::Path,
        version_id_opt: &Option<String>,
        mc_version: &str,
        loader: &str,
        installed_map: &mut std::collections::HashMap<String, String>,
    ) -> Result<(), crate::core::task::TaskError> {
        let dep_key = format!("{}_{}", self.options.source, dep.project_id);
        
        let (url, fname) = if self.options.source == "modrinth" {
            self.resolve_modrinth_dep(dep, client, version_id_opt, mc_version, loader).await?
        } else if self.options.source == "curseforge" {
            self.resolve_curseforge_dep(dep, client, version_id_opt, mc_version, loader).await?
        } else {
            return Err(crate::core::task::TaskError::ExecutionError("Unknown source".to_string()));
        };

        if let Some(v_id) = version_id_opt {
            if let Some(old_fname) = installed_map.get(&dep_key) {
                if old_fname != &fname {
                    let _ = tokio::fs::remove_file(target_dir_path.join(old_fname)).await;
                    let _ = tokio::fs::remove_file(target_dir_path.join(format!("{}.disable", old_fname))).await;
                }
            }
        }

        let dep_path = target_dir_path.join(&fname);
        if !dep_path.exists() {
            download_mod_file_task(dep_ctx, client, &url, &dep_path).await.map_err(crate::core::task::TaskError::ExecutionError)?;
        } else {
            dep_ctx.update_progress(100, 100, "File exists").await;
        }

        if let Some(v_id) = version_id_opt {
            installed_map.retain(|_, v| v != &fname);
            installed_map.insert(dep_key, fname);
            save_installed_mods_json(v_id, installed_map).await;
        }
        Ok(())
    }

    async fn resolve_modrinth_dep(
        &self,
        dep: &crate::core::modrinth::UnifiedDependency,
        client: &reqwest::Client,
        version_id_opt: &Option<String>,
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
            if version_id_opt.is_some() && !mc_version.is_empty() {
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
        version_id_opt: &Option<String>,
        mc_version: &str,
        loader: &str,
    ) -> Result<(String, String), crate::core::task::TaskError> {
        if version_id_opt.is_some() && !mc_version.is_empty() {
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
        let client = reqwest::Client::new();
        let (target_dir_path, version_id_opt) = self.get_target_dir().await?;

        self.download_main_mod(&ctx.with_sub_task("main"), &client, &target_dir_path, &version_id_opt).await?;
        self.download_dependencies(&ctx, &client, &target_dir_path, &version_id_opt).await?;

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
        let client = reqwest::Client::new();

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
        download_mod_file_task(&sub_ctx, &client, &options.download_url, &target_path)
            .await
            .map_err(crate::core::task::TaskError::ExecutionError)?;

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

        let client = reqwest::Client::new();

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
            download_mod_file_task(&ctx, &client, &options.download_url, &target_path)
                .await
                .map_err(crate::core::task::TaskError::ExecutionError)?;
        } else {
            ctx.update_progress(100, 100, "File already exists").await;
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

        let client = reqwest::Client::new();

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
            download_mod_file_task(&ctx, &client, &options.download_url, &target_path)
                .await
                .map_err(crate::core::task::TaskError::ExecutionError)?;
        } else {
            ctx.update_progress(100, 100, "File already exists").await;
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
        let client = reqwest::Client::new();
        
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
        download_mod_file_task(&download_ctx, &client, &options.download_url, &temp_zip_path)
            .await.map_err(crate::core::task::TaskError::ExecutionError)?;

        let extract_ctx = ctx.with_sub_task("extract");
        extract_ctx.update_progress(0, 100, "Extracting world...").await;
        
        extract_world_zip(temp_zip_path.clone(), target_dir_path).await.map_err(crate::core::task::TaskError::ExecutionError)?;

        tokio::fs::remove_dir_all(temp_dir).await.ok();
        extract_ctx.update_progress(100, 100, "World extracted").await;
        ctx.update_progress(100, 100, "World installed successfully").await;

        Ok(())
    }
}

#[tauri::command]
pub async fn get_instance_mod_mapping(
    version_id: String,
) -> Result<std::collections::HashMap<String, String>, String> {
    Ok(std::collections::HashMap::new())
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
