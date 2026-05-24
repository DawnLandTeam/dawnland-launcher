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