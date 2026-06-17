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
                            server_id = config.server_id;
                            pack_version_id = config.pack_version_id;
                            pack_file_name = config.pack_file_name;
                        }
                    }
                }

                // If it is installing, check if its corresponding task exists and is not cancelled
                if is_installing {
                    let mut has_valid_task = false;
                    for task in &tasks {
                        if let Some(tid) = task.task_type.instance_id() {
                            if tid == id && task.status != crate::core::task::TaskStatus::Cancelled {
                                has_valid_task = true;
                                break;
                            }
                        }
                    }

                    if !has_valid_task {
                        tracing::info!("Cleaning up zombie installing instance: {}", id);
                        let _ = tokio::fs::remove_dir_all(&path).await;
                        continue;
                    }
                }

                if is_hidden {
                    continue;
                }

                // An instance MUST have its basic {id}.json file unless it is currently actively installing
                if !is_installing && !json_path.exists() {
                    tracing::warn!("Skipping invalid/empty instance directory {}: missing {}.json", id, id);
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
                        });
                    }
                    Err(e) => {
                        tracing::warn!("Failed to read version JSON for {}: {}", id, e);
                        // Add with default values
                        instances.push(InstanceItem {
                            id: id.clone(),
                            name: id.clone(),
                            mc_version: extract_mc_version_from_id(&id)
                                .unwrap_or_else(|| id.clone()),
                            loader_type: "Vanilla".to_string(),
                            modpack_version: None,
                            modpack_type: None,
                            modpack_project_id: None,
                            server_id,
                            pack_version_id,
                            pack_file_name,
                            is_installing,
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
                } else if let Some(main_class) = json.get("mainClass").and_then(|v| v.as_str()) {
                    let mc_lower = main_class.to_lowercase();
                    if mc_lower.contains("fabric") {
                        "Fabric"
                    } else if mc_lower.contains("neoforge") {
                        "NeoForge"
                    } else if mc_lower.contains("forge")
                        || mc_lower.contains("fml")
                        || mc_lower.contains("bootstraplauncher")
                    {
                        "Forge"
                    } else {
                        "Vanilla"
                    }
                } else {
                    "Vanilla"
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
                    extract_mc_version_from_id(id).unwrap_or_else(|| id.to_string())
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
                extract_mc_version_from_id(id).unwrap_or_else(|| id.to_string()),
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
    let parts: Vec<&str> = id.split(|c: char| c == '-' || c == '_').collect();

    for part in parts {
        // Check if it looks like a version (starts with digit, contains dots)
        // Ensure it starts with "1." to filter out Forge/NeoForge version numbers like 26.1.2 or 47.1.0
        if part.starts_with("1.") && part.contains('.') {
            // Basic validation: should have at least major.minor
            let dots = part.matches('.').count();
            if dots >= 1 && dots <= 3 {
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

/// Delete an instance (removes version directory).
#[tauri::command]
pub async fn delete_instance(version_id: String) -> Result<(), String> {
    let base_dir = get_minecraft_base();
    let instance_dir = base_dir.join("versions").join(&version_id);

    if !instance_dir.exists() {
        return Err(format!("Instance {} not found", version_id));
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

/// Get list of installed mods for a specific instance
#[tauri::command]
pub async fn get_installed_mods(version_id: String) -> Result<Vec<LocalModItem>, String> {
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

    let parser = crate::core::mod_parser::ModParser::new(&base_dir);
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

        let meta = if let Some(m) = cache_entries.get(&cache_key) {
            m.clone()
        } else {
            let path_clone = path.clone();
            let key_clone = cache_key.clone();
            let base_dir_clone = base_dir.clone();
            let m = tokio::task::spawn_blocking(move || {
                let p = crate::core::mod_parser::ModParser::new(&base_dir_clone);
                p.parse_mod(&path_clone, &key_clone)
            })
            .await
            .unwrap_or_default();

            cache_entries.insert(cache_key.clone(), m.clone());
            parser.set_cache_entry(&cache_key, &m);
            m
        };

        let icon_url = if meta.has_icon {
            let icon_name = meta.mod_id.as_deref().unwrap_or(&cache_key);
            let mut p = parser.get_icon_path(icon_name);
            if !p.exists() {
                p = parser.get_icon_path(&cache_key);
            }
            Some(p.to_string_lossy().to_string())
        } else {
            None
        };

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
        return Err(format!("Mod already exists in target location: {}", dst_file.display()));
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
pub async fn delete_local_mod(version_id: String, filename: String, is_enabled: Option<bool>) -> Result<(), String> {
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
        return Err(format!("Mod file not found for '{}' in instance '{}'", filename, version_id));
    }

    // Only remove from dlml.json if both the enabled and disabled variants are gone
    if !mod_file.exists() && !disabled_file.exists() {
        let mut installed_map = load_installed_mods_json(&version_id).await;
        let mut keys_to_remove = Vec::new();
        for (k, v) in installed_map.iter() {
            if v == &filename {
                keys_to_remove.push(k.clone());
            }
        }
        if !keys_to_remove.is_empty() {
            for k in keys_to_remove {
                installed_map.remove(&k);
            }
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
    use tokio::io::AsyncWriteExt;
    use tauri::Emitter;

    let mut response = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("Failed to download mod: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("Download failed with status: {}", response.status()));
    }

    let content_length = response.content_length();
    
    let _ = app.emit("mod-install-progress", ModInstallProgress {
        event: "Started".to_string(),
        filename: filename.to_string(),
        content_length,
        chunk_length: None,
    });

    let mut file = tokio::fs::File::create(target_path)
        .await
        .map_err(|e| format!("Failed to create mod file: {}", e))?;

    while let Some(chunk) = response.chunk().await.map_err(|e| format!("Chunk error: {}", e))? {
        file.write_all(&chunk)
            .await
            .map_err(|e| format!("Write error: {}", e))?;
        
        let _ = app.emit("mod-install-progress", ModInstallProgress {
            event: "Progress".to_string(),
            filename: filename.to_string(),
            content_length: None,
            chunk_length: Some(chunk.len()),
        });
    }

    file.flush().await.map_err(|e| format!("Flush error: {}", e))?;

    let _ = app.emit("mod-install-progress", ModInstallProgress {
        event: "Finished".to_string(),
        filename: filename.to_string(),
        content_length: None,
        chunk_length: None,
    });

    Ok(())
}

fn extract_filename_from_url(url: &str, project_id: &str) -> String {
    let url_filename = url
        .split('/')
        .last()
        .unwrap_or_default()
        .split('?')
        .next()
        .unwrap_or_default()
        .to_string();

    if url_filename.is_empty() || !url_filename.ends_with(".jar") {
        format!("{}.jar", project_id)
    } else {
        url_filename
    }
}

/// Helper function to load and save installed mods directly into `dlml.json`
async fn load_installed_mods_json(instance_id: &str) -> std::collections::HashMap<String, String> {
    let base_dir = get_minecraft_base();
    let config_path = base_dir.join("versions").join(instance_id).join("dlml.json");
    if config_path.exists() {
        if let Ok(content) = tokio::fs::read_to_string(&config_path).await {
            if let Ok(config) = serde_json::from_str::<crate::core::launcher::InstanceConfig>(&content) {
                return config.installed_mods;
            }
        }
    }
    std::collections::HashMap::new()
}

async fn save_installed_mods_json(instance_id: &str, map: &std::collections::HashMap<String, String>) {
    let base_dir = get_minecraft_base();
    let config_path = base_dir.join("versions").join(instance_id).join("dlml.json");
    
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
#[tauri::command]
pub async fn install_mod_to_instance(
    app: tauri::AppHandle,
    version_id: String,
    mod_source: String, // "modrinth" or "curseforge"
    project_id: String,
    file_id: String,
    download_url: String,
    dependencies: Option<Vec<crate::core::modrinth::UnifiedDependency>>,
    keep_both: Option<bool>,
) -> Result<String, String> {
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
        download_mod_file_stream(&app, &client, &download_url, &target_path, &filename).await?;
    }

    tracing::info!("Installed mod {} to instance {} (file: {})", project_id, version_id, filename);

    // Enforce 1-to-1 mapping: Remove any existing keys that map to this exact filename
    installed_map.retain(|_, v| v != &filename);
    installed_map.insert(mod_key, filename.clone());
    save_installed_mods_json(&version_id, &installed_map).await;

    // Await dependency resolution and downloading sequentially
    if let Some(deps) = dependencies {
        let req_deps: Vec<_> = deps.into_iter().filter(|d| d.required).collect();
        if !req_deps.is_empty() {
            tracing::info!("Found {} required dependencies for mod {}. Initiating download.", req_deps.len(), project_id);
            for dep in req_deps {
                let dep_key = format!("{}_{}", mod_source, dep.project_id);

                if mod_source == "modrinth" {
                    if let Some(vid) = dep.version_id {
                        tracing::info!("Fetching Modrinth dependency version: {}", vid);
                        if let Ok(res) = client.get(&format!("https://api.modrinth.com/v2/version/{}", vid)).send().await {
                            if let Ok(json) = res.json::<serde_json::Value>().await {
                                if let Some(files) = json.get("files").and_then(|f| f.as_array()) {
                                    if let Some(primary) = files.first() {
                                        if let (Some(url), Some(fname)) = (primary.get("url").and_then(|u| u.as_str()), primary.get("filename").and_then(|f| f.as_str())) {
                                            if let Some(old_fname) = installed_map.get(&dep_key) {
                                                if old_fname != fname {
                                                    let _ = tokio::fs::remove_file(mods_dir.join(old_fname)).await;
                                                }
                                                let _ = tokio::fs::remove_file(mods_dir.join(format!("{}.disable", old_fname))).await;
                                            }
                                            let dep_path = mods_dir.join(fname);
                                            if !dep_path.exists() {
                                                let _ = download_mod_file_stream(&app, &client, url, &dep_path, fname).await;
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
                    } else {
                        let pid = &dep.project_id;
                        tracing::info!("Fetching Modrinth dependency project (latest compatible): {}", pid);
                        if let Ok(files) = crate::core::modrinth::get_modrinth_mod_files(pid.clone(), mc_version.clone(), loader.clone()).await {
                            if let Some(primary) = files.first() {
                                let fname = &primary.filename;
                                let url = &primary.download_url;

                                if let Some(old_fname) = installed_map.get(&dep_key) {
                                    if old_fname != fname {
                                        let _ = tokio::fs::remove_file(mods_dir.join(old_fname)).await;
                                    }
                                    let _ = tokio::fs::remove_file(mods_dir.join(format!("{}.disable", old_fname))).await;
                                }
                                let dep_path = mods_dir.join(fname);
                                if !dep_path.exists() {
                                    let _ = download_mod_file_stream(&app, &client, url, &dep_path, fname).await;
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
                    if let Ok(files) = crate::core::curseforge::get_cf_mod_files(dep.project_id.clone(), mc_version.clone(), loader.clone()).await {
                        if let Some(primary) = files.first() {
                            let fname = &primary.filename;
                            let url = &primary.download_url;

                            if let Some(old_fname) = installed_map.get(&dep_key) {
                                if old_fname != fname {
                                    let _ = tokio::fs::remove_file(mods_dir.join(old_fname)).await;
                                }
                                let _ = tokio::fs::remove_file(mods_dir.join(format!("{}.disable", old_fname))).await;
                            }
                            let dep_path = mods_dir.join(fname);
                            if !dep_path.exists() {
                                let _ = download_mod_file_stream(&app, &client, url, &dep_path, fname).await;
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

    let filename = path.file_name()
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
        assert_eq!(extract_mc_version_from_id("1.20.1"), Some("1.20.1".to_string()));
        assert_eq!(extract_mc_version_from_id("1.19.4-Fabric-0.15.7"), Some("1.19.4".to_string()));
        assert_eq!(extract_mc_version_from_id("Fabric-1.18.2-0.14.21"), Some("1.18.2".to_string()));
        assert_eq!(extract_mc_version_from_id("NeoForge-1.20.4-20.4.80-beta"), Some("1.20.4".to_string()));
        assert_eq!(extract_mc_version_from_id("1.8.9-forge1.8.9-11.15.1.2318-1.8.9"), Some("1.8.9".to_string()));
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
        let (mc, loader, mv, mt, _mpid) = parse_version_json(fabric_json, "fabric-loader-0.15.7-1.20.1");
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
        let (mc, loader, _mv, _mt, _mpid) = parse_version_json(neoforge_json, "1.20.4-neoforge-20.4.80");
        assert_eq!(mc, "1.20.4");
        assert_eq!(loader, "NeoForge");
    }
}
