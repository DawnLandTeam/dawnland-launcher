//! Fabric Mod Loader Integration
//! Provides commands for fetching available Fabric loaders and installing Fabric instances.

use crate::core::mojang::{get_minecraft_base, InstallVanillaTask, VanillaInstallOptions};
use crate::core::task::{ExecutableTask, TaskContext, TaskError, TaskManager, TaskType};
use crate::core::utils::compare_versions;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

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

pub struct InstallFabricOptions {
    pub mc_version: String,
    pub fabric_version: String,
    pub custom_instance_name: String,
    pub is_dependency: Option<bool>,
}

pub struct InstallFabricTask {
    pub options: InstallFabricOptions,
}

#[async_trait::async_trait]
impl ExecutableTask for InstallFabricTask {
    async fn execute(&self, ctx: TaskContext) -> Result<(), TaskError> {
        let mc_version = &self.options.mc_version;
        let fabric_version = &self.options.fabric_version;
        let custom_instance_name = &self.options.custom_instance_name;
        let is_dependency = self.options.is_dependency;
    tracing::info!(
        "Installing Fabric instance: {} (MC {} + Loader {})",
        custom_instance_name,
        mc_version,
        fabric_version
    );

    let base_dir = get_minecraft_base();
    let instance_dir = base_dir.join("versions").join(custom_instance_name);

    let _ = tokio::fs::create_dir_all(&instance_dir).await;
    crate::core::launcher::InstanceConfig::ensure_installing(&instance_dir, is_dependency.unwrap_or(false)).await;
    
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| TaskError::ExecutionError(format!("Failed to create HTTP client: {e}")))?;

    // Step 1: Check if base vanilla version is installed
    let base_version_dir = base_dir.join("versions").join(&mc_version);
    let base_client_jar = base_version_dir.join(format!("{}.jar", mc_version));
    let base_version_json = base_version_dir.join(format!("{}.json", mc_version));

    ctx.manager.wait_for_instance(&mc_version, &ctx.cancel_token).await;

    if !base_client_jar.exists() || !base_version_json.exists() {
        // Need to install base vanilla version first
        tracing::info!(
            "Base Minecraft {} not installed, installing first...",
            mc_version
        );

        ctx.update_progress(0, 0, "Fetching Minecraft version manifest..."
            ).await;

        // Get version JSON URL from Mojang
        let manifest_url = "https://launchermeta.mojang.com/mc/game/version_manifest_v2.json";
        let manifest: serde_json::Value = client
            .get(manifest_url)
            .send()
            .await
            .map_err(|e| TaskError::ExecutionError(format!("Failed to fetch version manifest: {}", e)))?
            .json()
            .await
            .map_err(|e| TaskError::ExecutionError(format!("Failed to parse version manifest: {}", e)))?;

        // Find the version URL for requested mc_version
        let version_url = manifest["versions"]
            .as_array()
            .and_then(|versions| {
                versions
                    .iter()
                    .find(|v| v["id"].as_str() == Some(&mc_version))
            })
            .and_then(|v| v["url"].as_str())
            .ok_or_else(|| TaskError::ExecutionError(format!("Version {} not found in manifest", mc_version)))?;

        let vanilla_task = InstallVanillaTask {
            options: VanillaInstallOptions {
                version_id: mc_version.clone(),
                version_json_url: version_url.to_string(),
                is_dependency: Some(true),
            },
        };
        vanilla_task.execute(ctx.clone()).await?;

        tracing::info!("Base vanilla {} installed successfully", mc_version);
    } else {
        tracing::info!("Base Minecraft {} already installed, skipping", mc_version);
    }

    
        if ctx.is_cancelled() {
            return Err(TaskError::ExecutionError("Installation cancelled by user".to_string()));
        }

    let is_dep = self.options.is_dependency.unwrap_or(false);

    if !is_dep {
        ctx.set_total_steps(2).await;
        // Step 2: Install Fabric profile
        ctx.next_step("Fetching Fabric profile...").await;
    } else {
        ctx.update_progress(0, 0, "Fetching Fabric profile...").await;
    }

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
        .map_err(|e| TaskError::ExecutionError(format!("Failed to fetch Fabric profile: {}", e)))?
        .text()
        .await
        .map_err(|e| TaskError::ExecutionError(format!("Failed to read Fabric profile: {}", e)))?;

    // Parse and modify the profile
    let mut profile: serde_json::Value = serde_json::from_str(&profile_json)
        .map_err(|e| TaskError::ExecutionError(format!("Failed to parse Fabric profile JSON: {}", e)))?;

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
        .map_err(|e| TaskError::ExecutionError(format!("Failed to create version directory: {}", e)))?;

    let version_json_path = version_dir.join(format!("{}.json", custom_instance_name));
    let updated_json = serde_json::to_string_pretty(&profile)
        .map_err(|e| TaskError::ExecutionError(format!("Failed to serialize Fabric profile: {}", e)))?;

    tokio::fs::write(&version_json_path, &updated_json)
        .await
        .map_err(|e| TaskError::ExecutionError(format!("Failed to write Fabric profile: {}", e)))?;

    tracing::info!("Saved Fabric profile to: {:?}", version_json_path);

    
        if ctx.is_cancelled() {
            return Err(TaskError::ExecutionError("Installation cancelled by user".to_string()));
        }
    // Step 3: Download Fabric libraries
    if !is_dep {
        ctx.next_step("Resolving Fabric libraries...").await;
    } else {
        ctx.update_progress(0, 0, "Resolving Fabric libraries...").await;
    }

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

    

    if !tasks.is_empty() {
        if let Err(e) = crate::downloader::run_batch_download_task(tasks, ctx.clone()).await {
            tracing::warn!("Installation failed during batch download, cleaning up...");
            let version_dir = base_dir.join("versions").join(&custom_instance_name);
            let _ = tokio::fs::remove_dir_all(&version_dir).await;
            return Err(TaskError::ExecutionError(e));
        }
    }

    if ctx.is_cancelled() {
        tracing::warn!("Installation cancelled, cleaning up fabric instance directory...");
        let version_dir = base_dir.join("versions").join(&custom_instance_name);
        let _ = tokio::fs::remove_dir_all(&version_dir).await;
        return Err(TaskError::ExecutionError("Installation cancelled by user".to_string()));
    }

    
        if ctx.is_cancelled() {
            return Err(TaskError::ExecutionError("Installation cancelled by user".to_string()));
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
            is_installing: false,
            extra: std::collections::HashMap::new(),
        }
    };

    config.hidden = is_dependency.unwrap_or(false);
    config.is_installing = false;
    let config_json = serde_json::to_string_pretty(&config)
        .map_err(|e| TaskError::ExecutionError(format!("Failed to serialize instance config: {}", e)))?;

    tokio::fs::write(&config_path, config_json)
        .await
        .map_err(|e| TaskError::ExecutionError(format!("Failed to write instance config: {}", e)))?;

    tracing::info!("Created instance config at: {:?}", config_path);

    // Emit complete
    ctx.update_progress(0, 0, "Complete").await;

    tracing::info!(
        "Fabric instance '{}' installed successfully!",
        custom_instance_name
    );
    Ok(())
}
}

/// Install a Fabric instance - automatically installs base vanilla first, then Fabric.
#[tauri::command]
pub async fn install_fabric_instance(
    mc_version: String,
    fabric_version: String,
    custom_instance_name: String,
    is_dependency: Option<bool>,
    app: AppHandle,
) -> Result<String, String> {
    let task_manager = app.state::<TaskManager>().inner().clone();
    
    // Pre-create instance directory and dlml.json synchronously so frontend can detect it immediately
    let base_dir = crate::core::mojang::get_minecraft_base();
    let instance_dir = base_dir.join("versions").join(&custom_instance_name);
    let _ = std::fs::create_dir_all(&instance_dir);
    let config_path = instance_dir.join("dlml.json");
    let mut pre_config = crate::core::launcher::InstanceConfig::default();
    pre_config.is_installing = true;
    pre_config.hidden = is_dependency.unwrap_or(false);
    let _ = std::fs::write(&config_path, serde_json::to_string_pretty(&pre_config).unwrap());
    
    let task = InstallFabricTask {
        options: InstallFabricOptions {
            mc_version: mc_version.clone(),
            fabric_version: fabric_version.clone(),
            custom_instance_name: custom_instance_name.clone(),
            is_dependency,
        },
    };
    
    let task_id = task_manager
        .spawn_task(TaskType::InstallFabric { 
            mc_version: mc_version.clone(), 
            fabric_version: fabric_version.clone(),
            custom_instance_name: custom_instance_name.clone(),
            is_dependency,
        }, task)
        .await
        .map_err(|e| e.to_string())?;

    Ok(task_id)
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
