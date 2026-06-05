//! Fabric Mod Loader Integration
//! Provides commands for fetching available Fabric loaders and installing Fabric instances.

use crate::core::mojang::get_minecraft_base;
use crate::core::utils::compare_versions;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

// ============ Fabric API Types ============

/// Root response from Fabric Meta API for loader versions.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FabricLoaderResponse {
    loader: FabricLoaderData,
}

/// The actual loader data from the API response.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FabricLoaderData {
    version: String,
    build: Option<i32>,
    maven: Option<String>,
    #[serde(default)]
    stable: bool,
}

/// Response to frontend: separated stable and unstable versions
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FabricLoaderList {
    pub stable: Vec<String>,
    pub unstable: Vec<String>,
}

// ============ Constants ============

const FABRIC_META_BASE: &str = "https://meta.fabricmc.net/v2";

// ============ Tauri Commands ============

/// Get available Fabric Loader versions for a given Minecraft version.
/// Returns stable versions first, then unstable versions.
#[tauri::command]
pub async fn get_fabric_loaders(mc_version: String) -> Result<FabricLoaderList, String> {
    tracing::info!("Fetching Fabric loaders for Minecraft {}", mc_version);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {e}"))?;

    let url = format!("{}/versions/loader/{}", FABRIC_META_BASE, mc_version);

    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Failed to fetch Fabric loaders: {}", e))?;

    if !response.status().is_success() {
        return Err(format!(
            "Fabric loader API request failed: {}",
            response.status()
        ));
    }

    let loaders: Vec<FabricLoaderResponse> = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse Fabric loader response: {}", e))?;

    // Separate stable and unstable versions
    let mut stable_versions: Vec<String> = Vec::new();
    let mut unstable_versions: Vec<String> = Vec::new();

    for loader in &loaders {
        if loader.loader.stable {
            stable_versions.push(loader.loader.version.clone());
        } else {
            unstable_versions.push(loader.loader.version.clone());
        }
    }

    // Sort both lists: numerically descending (newest first)
    stable_versions.sort_by(|a, b| compare_versions(b, a));
    unstable_versions.sort_by(|a, b| compare_versions(b, a));

    tracing::info!(
        "Found {} stable + {} unstable = {} total Fabric loader versions for MC {}",
        stable_versions.len(),
        unstable_versions.len(),
        stable_versions.len() + unstable_versions.len(),
        mc_version
    );

    Ok(FabricLoaderList {
        stable: stable_versions,
        unstable: unstable_versions,
    })
}

/// Install a Fabric instance - automatically installs base vanilla first, then Fabric.
#[tauri::command]
pub async fn install_fabric_instance(
    mc_version: String,
    fabric_version: String,
    custom_instance_name: String,
    is_dependency: Option<bool>,
    app: AppHandle,
) -> Result<(), String> {
    tracing::info!(
        "Installing Fabric instance: {} (MC {} + Loader {})",
        custom_instance_name,
        mc_version,
        fabric_version
    );

    let base_dir = get_minecraft_base();
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {e}"))?;

    // Step 1: Check if base vanilla version is installed
    let base_version_dir = base_dir.join("versions").join(&mc_version);
    let base_client_jar = base_version_dir.join(format!("{}.jar", mc_version));
    let base_version_json = base_version_dir.join(format!("{}.json", mc_version));

    if !base_client_jar.exists() || !base_version_json.exists() {
        // Need to install base vanilla version first
        tracing::info!(
            "Base Minecraft {} not installed, installing first...",
            mc_version
        );

        let _ = app.emit(
            "install-progress",
            serde_json::json!({
                "phase": "resolving_version",
                "versionId": mc_version,
                "currentFile": "Fetching Minecraft version manifest..."
            }),
        );

        // Get version JSON URL from Mojang
        let manifest_url = "https://launchermeta.mojang.com/mc/game/version_manifest_v2.json";
        let manifest: serde_json::Value = client
            .get(manifest_url)
            .send()
            .await
            .map_err(|e| format!("Failed to fetch version manifest: {}", e))?
            .json()
            .await
            .map_err(|e| format!("Failed to parse version manifest: {}", e))?;

        // Find the version URL for requested mc_version
        let version_url = manifest["versions"]
            .as_array()
            .and_then(|versions| {
                versions
                    .iter()
                    .find(|v| v["id"].as_str() == Some(&mc_version))
            })
            .and_then(|v| v["url"].as_str())
            .ok_or_else(|| format!("Version {} not found in manifest", mc_version))?;

        crate::core::mojang::install_vanilla_version(
            mc_version.clone(),
            version_url.to_string(),
            Some(true),
            app.clone(),
        )
        .await?;

        tracing::info!("Base vanilla {} installed successfully", mc_version);
    } else {
        tracing::info!("Base Minecraft {} already installed, skipping", mc_version);
    }

    // Step 2: Install Fabric profile
    let _ = app.emit(
        "install-progress",
        serde_json::json!({
            "phase": "resolving_version",
            "versionId": custom_instance_name,
        }),
    );

    // Fetch Fabric profile JSON
    let fabric_url = format!(
        "{}/versions/loader/{}/{}/profile/json",
        FABRIC_META_BASE, mc_version, fabric_version
    );

    tracing::info!("Fetching Fabric profile from: {}", fabric_url);

    let profile_json = client
        .get(&fabric_url)
        .send()
        .await
        .map_err(|e| format!("Failed to fetch Fabric profile: {}", e))?
        .text()
        .await
        .map_err(|e| format!("Failed to read Fabric profile: {}", e))?;

    // Parse and modify the profile
    let mut profile: serde_json::Value = serde_json::from_str(&profile_json)
        .map_err(|e| format!("Failed to parse Fabric profile JSON: {}", e))?;

    // Modify id and set inheritsFrom
    if let Some(obj) = profile.as_object_mut() {
        obj.insert("id".to_string(), serde_json::json!(custom_instance_name));
        obj.insert("inheritsFrom".to_string(), serde_json::json!(mc_version));

        // Update logging file reference
        if let Some(logging) = obj.get_mut("logging") {
            if let Some(log_obj) = logging.as_object_mut() {
                if let Some(client) = log_obj.get_mut("client") {
                    if let Some(c_obj) = client.as_object_mut() {
                        if let Some(file) = c_obj.get_mut("file") {
                            if let Some(file_obj) = file.as_object_mut() {
                                let log_id = format!("{}-client", mc_version);
                                file_obj.insert("id".to_string(), serde_json::json!(log_id));
                            }
                        }
                    }
                }
            }
        }
    }

    // Save Fabric profile
    let version_dir = base_dir.join("versions").join(&custom_instance_name);

    tokio::fs::create_dir_all(&version_dir)
        .await
        .map_err(|e| format!("Failed to create version directory: {}", e))?;

    let version_json_path = version_dir.join(format!("{}.json", custom_instance_name));
    let updated_json = serde_json::to_string_pretty(&profile)
        .map_err(|e| format!("Failed to serialize Fabric profile: {}", e))?;

    tokio::fs::write(&version_json_path, &updated_json)
        .await
        .map_err(|e| format!("Failed to write Fabric profile: {}", e))?;

    tracing::info!("Saved Fabric profile to: {:?}", version_json_path);

    // Step 3: Download Fabric libraries
    let _ = app.emit(
        "install-progress",
        serde_json::json!({
            "phase": "resolving_libraries",
            "versionId": custom_instance_name,
        }),
    );

    let libraries: &[serde_json::Value] = match profile.get("libraries").and_then(|l| l.as_array())
    {
        Some(libs) => libs,
        None => &[],
    };

    let mut tasks: Vec<crate::downloader::DownloadTask> = Vec::new();

    for lib in libraries {
        // Use the helper function that handles both Mojang format and Maven coordinates
        if let Some((download_url, relative_path)) =
            crate::core::mojang::get_library_download_info_from_json(lib)
        {
            let dest = base_dir.join("libraries").join(&relative_path);
            tracing::debug!(
                "Added Fabric library: {} -> {}",
                relative_path,
                download_url
            );
            tasks.push(crate::downloader::DownloadTask::new(
                download_url,
                dest.to_string_lossy().to_string(),
                None, // SHA1 not available for Maven coordinates
                None,
            ));
        }
    }

    let total_tasks = tasks.len();
    tracing::info!("Resolved {} Fabric library files", total_tasks);

    let _ = app.emit(
        "install-progress",
        serde_json::json!({
            "phase": "downloading",
            "versionId": custom_instance_name,
            "totalTasks": total_tasks,
        }),
    );

    if !tasks.is_empty() {
        let app_clone = app.clone();
        crate::downloader::run_batch_download(tasks, app_clone, crate::core::mojang::get_cancel_flag()).await;
    }

    if crate::core::mojang::get_cancel_flag().load(std::sync::atomic::Ordering::Relaxed) {
        tracing::warn!("Installation cancelled, cleaning up fabric instance directory...");
        let version_dir = base_dir.join("versions").join(&custom_instance_name);
        let _ = tokio::fs::remove_dir_all(&version_dir).await;
        
        let _ = app.emit(
            "install-progress",
            serde_json::json!({
                "phase": "error",
                "error": "Installation cancelled by user",
            }),
        );
        return Err("Installation cancelled by user".to_string());
    }

    // Step 4: Create default dlml.json config
    let config_path = version_dir.join("dlml.json");
    let mut config: crate::core::launcher::InstanceConfig = if config_path.exists() {
        let content = tokio::fs::read_to_string(&config_path)
            .await
            .unwrap_or_else(|_| "{}".to_string());
        serde_json::from_str(&content).unwrap_or_default()
    } else {
        crate::core::launcher::InstanceConfig {
            java_path: None,
            max_memory: None,
            jvm_args_extra: None,
            window_behavior: "keep".to_string(),
            show_game_log: false,
            hidden: false,
            server_id: None,
            pack_version_id: None,
            pack_file_name: None,
        }
    };

    config.hidden = is_dependency.unwrap_or(false);

    let config_json = serde_json::to_string_pretty(&config)
        .map_err(|e| format!("Failed to serialize instance config: {}", e))?;

    tokio::fs::write(&config_path, config_json)
        .await
        .map_err(|e| format!("Failed to write instance config: {}", e))?;

    tracing::info!("Created instance config at: {:?}", config_path);

    // Emit complete
    let _ = app.emit(
        "install-progress",
        serde_json::json!({
            "phase": "complete",
            "versionId": custom_instance_name,
        }),
    );

    tracing::info!(
        "Fabric instance '{}' installed successfully!",
        custom_instance_name
    );
    Ok(())
}

/// Check if a base Minecraft version is installed (has client.jar).
#[tauri::command]
pub async fn check_vanilla_installed(mc_version: String) -> Result<bool, String> {
    let base_dir = get_minecraft_base();
    let client_jar = base_dir
        .join("versions")
        .join(&mc_version)
        .join(format!("{}.jar", mc_version));

    let installed = client_jar.exists();
    tracing::info!("Vanilla {} installed: {}", mc_version, installed);
    Ok(installed)
}
