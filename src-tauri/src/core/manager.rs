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
}

/// Scan all locally installed instances from the versions directory.
#[tauri::command]
pub async fn scan_installed_instances() -> Result<Vec<InstanceItem>, String> {
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

    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|e| format!("Failed to read directory entry: {}", e))?
    {
        let path = entry.path();
        if path.is_dir() {
            let id = entry.file_name().to_string_lossy().to_string();
            let json_path = path.join(format!("{}.json", id));

            if json_path.exists() {
                // Read version JSON to extract metadata
                match tokio::fs::read_to_string(&json_path).await {
                    Ok(content) => {
                        // Parse basic info from JSON
                        let (mc_version, loader_type) = parse_version_json(&content, &id);
                        
                        instances.push(InstanceItem {
                            id: id.clone(),
                            name: id.clone(),
                            mc_version,
                            loader_type,
                        });
                    }
                    Err(e) => {
                        tracing::warn!("Failed to read version JSON for {}: {}", id, e);
                        // Add with default values
                        instances.push(InstanceItem {
                            id: id.clone(),
                            name: id.clone(),
                            mc_version: "Unknown".to_string(),
                            loader_type: "Unknown".to_string(),
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
fn parse_version_json(content: &str, id: &str) -> (String, String) {
    // Try to parse as JSON
    match serde_json::from_str::<serde_json::Value>(content) {
        Ok(json) => {
            // Determine loader type based on mainClass or id
            let loader_type = if let Some(main_class) = json.get("mainClass").and_then(|v| v.as_str()) {
                if main_class.contains("fabricmc") || main_class.contains("fabric") {
                    "Fabric"
                } else if main_class.contains("forge") {
                    "Forge"
                } else {
                    "Vanilla"
                }
            } else if id.to_lowercase().contains("fabric") {
                "Fabric"
            } else if id.to_lowercase().contains("forge") {
                "Forge"
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
                    // Try to extract from id (e.g., "1.20.1" from "1.20.1" or "Fabric-1.20.1-0.15.11")
                    extract_mc_version_from_id(id)
                })
                .unwrap_or_else(|| "Unknown".to_string());

            (mc_version, loader_type.to_string())
        }
        Err(_) => {
            // Fallback: extract from id
            let mc_version = extract_mc_version_from_id(id).unwrap_or_else(|| "Unknown".to_string());
            let loader_type = if id.to_lowercase().contains("fabric") {
                "Fabric"
            } else if id.to_lowercase().contains("forge") {
                "Forge"
            } else {
                "Vanilla"
            };
            (mc_version, loader_type.to_string())
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
        if part.starts_with(|c: char| c.is_ascii_digit()) && part.contains('.') {
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

    let (mc_version, loader_type) = parse_version_json(&content, &version_id);

    Ok(InstanceItem {
        id: version_id.clone(),
        name: version_id,
        mc_version,
        loader_type,
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
        
        // Only process .jar files (enabled mods)
        if path.extension().and_then(|s| s.to_str()) == Some("jar") {
            let filename = entry.file_name().to_string_lossy().to_string();
            let metadata = tokio::fs::metadata(&path).await.map_err(|e| e.to_string())?;
            
            mods.push(LocalModItem {
                filename: filename.clone(),
                enabled: true,
                size: metadata.len(),
            });
        }
    }

    // Also check for disabled mods (.jar.disabled)
    let disabled_dir = base_dir.join("versions").join(&version_id).join("mods").join("disabled");
    if disabled_dir.exists() {
        let mut entries = tokio::fs::read_dir(&disabled_dir)
            .await
            .map_err(|e| format!("Failed to read disabled mods directory: {}", e))?;

        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|e| format!("Failed to read directory entry: {}", e))?
        {
            let path = entry.path();
            
            if path.extension().and_then(|s| s.to_str()) == Some("jar") {
                let filename = entry.file_name().to_string_lossy().to_string();
                let metadata = tokio::fs::metadata(&path).await.map_err(|e| e.to_string())?;
                
                // Remove .disabled extension for display
                let display_name = filename.trim_end_matches(".disabled");
                
                mods.push(LocalModItem {
                    filename: display_name.to_string(),
                    enabled: false,
                    size: metadata.len(),
                });
            }
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
    let disabled_dir = mods_dir.join("disabled");

    // Determine source and destination paths
    let (src_dir, dst_dir) = if enable {
        // Moving from disabled to enabled
        (&disabled_dir, &mods_dir)
    } else {
        // Moving from enabled to disabled
        (&mods_dir, &disabled_dir)
    };

    let src_file = src_dir.join(&filename);
    let dst_file = dst_dir.join(&filename);

    // If enabling, also check the main mods folder (might not be in disabled folder)
    let alt_src_file = if enable {
        mods_dir.join(&filename)
    } else {
        src_file.clone()
    };

    // Check if source exists
    let actual_src = if src_file.exists() {
        src_file
    } else if alt_src_file.exists() {
        alt_src_file
    } else {
        return Err(format!("Mod file not found: {}", filename));
    };

    // Ensure destination directory exists
    if !dst_dir.exists() {
        tokio::fs::create_dir_all(&dst_dir)
            .await
            .map_err(|e| format!("Failed to create disabled directory: {}", e))?;
    }

    // Check if destination already exists
    if dst_file.exists() {
        return Err(format!("Mod already exists in target location: {}", filename));
    }

    // Rename/move the file
    tokio::fs::rename(&actual_src, &dst_dir.join(&filename))
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
    tracing::info!(
        "Deleting mod {} from instance {}",
        filename,
        version_id
    );

    let base_dir = get_minecraft_base();
    let mods_dir = base_dir.join("versions").join(&version_id).join("mods");

    // Check in main mods directory
    let mod_file = mods_dir.join(&filename);
    
    // Check in disabled directory
    let disabled_file = mods_dir.join("disabled").join(&filename);

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
    mod_source: String,     // "modrinth" or "curseforge"
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
        return Err(format!("Download failed with status: {}", response.status()));
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