//! Instance Manager - Local Instance Scanning Engine
//! Provides functionality to scan and manage installed game instances.

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

        if filename.ends_with(".jar") {
            mods.push(LocalModItem {
                filename: filename.clone(),
                enabled: true,
                size: metadata.len(),
            });
        } else if filename.ends_with(".jar.disable") {
            let display_name = filename.trim_end_matches(".disable").to_string();
            mods.push(LocalModItem {
                filename: display_name,
                enabled: false,
                size: metadata.len(),
            });
        }
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
pub async fn delete_local_mod(version_id: String, filename: String) -> Result<(), String> {
    tracing::info!("Deleting mod {} from instance {}", filename, version_id);

    let base_dir = get_minecraft_base();
    let mods_dir = base_dir.join("versions").join(&version_id).join("mods");

    // Check in main mods directory
    let mod_file = mods_dir.join(&filename);

    // Check in disabled directory
    let disabled_file = mods_dir.join(format!("{}.disable", filename));

    let file_to_delete = if mod_file.exists() {
        mod_file
    } else if disabled_file.exists() {
        disabled_file
    } else {
        return Err(format!("Mod file not found: {}", filename));
    };

    tokio::fs::remove_file(&file_to_delete)
        .await
        .map_err(|e| format!("Failed to delete mod file: {}", e))?;

    tracing::info!("Deleted mod {} from instance {}", filename, version_id);
    Ok(())
}

/// Install a mod to a specific instance (download + save)
#[tauri::command]
pub async fn install_mod_to_instance(
    version_id: String,
    mod_source: String, // "modrinth" or "curseforge"
    project_id: String,
    file_id: String,
    download_url: String,
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

    // Download the mod file
    let client = reqwest::Client::new();
    let response = client
        .get(&download_url)
        .send()
        .await
        .map_err(|e| format!("Failed to download mod: {}", e))?;

    if !response.status().is_success() {
        return Err(format!(
            "Download failed with status: {}",
            response.status()
        ));
    }

    // Extract filename from URL or use project_id
    let url_filename = download_url
        .split('/')
        .last()
        .unwrap_or_default()
        .split('?')
        .next()
        .unwrap_or_default()
        .to_string();

    let filename = if url_filename.is_empty() || !url_filename.ends_with(".jar") {
        format!("{}.jar", project_id)
    } else {
        url_filename
    };

    let target_path = mods_dir.join(&filename);

    // Save the file
    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("Failed to read download content: {}", e))?;

    tokio::fs::write(&target_path, &bytes)
        .await
        .map_err(|e| format!("Failed to save mod file: {}", e))?;

    tracing::info!(
        "Installed mod {} to instance {} (file: {})",
        project_id,
        version_id,
        filename
    );

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
