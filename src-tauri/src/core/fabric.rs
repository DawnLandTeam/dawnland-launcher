#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(unused_variables)]
//! Fabric Mod Loader Integration
//! Provides commands for fetching available Fabric loaders and installing Fabric instances.

use crate::core::mojang::{get_minecraft_base, InstallVanillaTask, VanillaInstallOptions};
use crate::core::task::{ExecutableTask, TaskContext, TaskError, TaskManager, TaskType};
use crate::core::utils::compare_versions;
use crate::error::AppError;
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
pub async fn get_fabric_loaders(mc_version: String) -> Result<FabricLoaderList, AppError> {
    tracing::info!("Fetching Fabric loaders for Minecraft {}", mc_version);

    let client = crate::core::utils::get_http_client();

    let settings = crate::core::settings::get_launcher_settings_sync();
    let url = format!("{}/versions/loader/{}", FABRIC_META_BASE, mc_version);
    let url = crate::core::settings::replace_download_url(&url, &settings.download_source);

    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Failed to fetch Fabric loaders: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("Fabric loaders request failed: {}", response.status()).into());
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
    pub is_existing_instance: Option<bool>,
}

pub struct InstallFabricTask {
    pub options: InstallFabricOptions,
}

impl InstallFabricTask {
    pub fn get_sub_tasks() -> Vec<crate::core::task::state::SubTaskState> {
        let mut tasks = crate::core::mojang::InstallVanillaTask::get_sub_tasks();
        tasks.extend(vec![
            crate::core::task::state::SubTaskState {
                key: "resolve_loader".to_string(),
                name: "Fetch installer".to_string(),
                status: crate::core::task::state::SubTaskStatus::Pending,
                current: 0,
                total: 100,
                weight: 5,
            },
            crate::core::task::state::SubTaskState {
                key: "download_loader_libs".to_string(),
                name: "Download loader libraries".to_string(),
                status: crate::core::task::state::SubTaskStatus::Pending,
                current: 0,
                total: 100,
                weight: 45,
            },
            crate::core::task::state::SubTaskState {
                key: "install_fabric_api".to_string(),
                name: "Install Fabric API".to_string(),
                status: crate::core::task::state::SubTaskStatus::Pending,
                current: 0,
                total: 100,
                weight: 10,
            },
        ]);
        tasks
    }
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

        let is_dep = is_dependency.unwrap_or(false);

        // Define sub-tasks if running standalone
        if !is_dep && ctx.sub_task_key.is_none() {
            ctx.init_sub_tasks(Self::get_sub_tasks()).await;
        }

        let base_dir = get_minecraft_base();
        let instance_dir = if is_dependency.unwrap_or(false) {
            crate::core::mojang::get_dawnland_cache().join(custom_instance_name)
        } else {
            base_dir.join("versions").join(custom_instance_name)
        };

        let _ = tokio::fs::create_dir_all(&instance_dir).await;
        let is_existing = self.options.is_existing_instance.unwrap_or(false);
        if is_existing {
            crate::core::launcher::InstanceConfig::ensure_updating(&instance_dir)
                .await
                .map_err(|e| TaskError::ExecutionError(e.to_string()))?;
        } else {
            crate::core::launcher::InstanceConfig::ensure_installing(
                &instance_dir,
                self.options.is_dependency.unwrap_or(false),
            )
            .await
            .map_err(|e| TaskError::ExecutionError(e.to_string()))?;
        }

        let json_bak = instance_dir.join(format!("{}.json.bak", custom_instance_name));
        let config_bak = instance_dir.join("dlml.json.bak");
        if is_existing {
            let json_path = instance_dir.join(format!("{}.json", custom_instance_name));
            let config_path = instance_dir.join("dlml.json");
            if json_path.exists() {
                let _ = tokio::fs::copy(&json_path, &json_bak).await;
            }
            if config_path.exists() {
                let _ = tokio::fs::copy(&config_path, &config_bak).await;
            }
        }

        let client = crate::core::utils::get_http_client();

        // Step 1: Check if base vanilla version is installed
        let dawnland_cache = crate::core::mojang::get_dawnland_cache();
        let settings = crate::core::settings::load_launcher_settings()
            .await
            .unwrap_or_default();
        let is_dep_val = is_dependency.unwrap_or(false);
        let should_flatten = !settings.enable_instance_inheritance;
        let effective_is_dep = is_dep_val || should_flatten;

        let base_version_dir = if effective_is_dep {
            dawnland_cache.join(mc_version)
        } else {
            base_dir.join("versions").join(mc_version)
        };
        let base_client_jar = base_version_dir.join(format!("{}.jar", mc_version));
        let base_version_json = base_version_dir.join(format!("{}.json", mc_version));

        if is_existing {
            let existing_jar = instance_dir.join(format!("{}.jar", custom_instance_name));
            let existing_json = instance_dir.join(format!("{}.json", custom_instance_name));
            let existing_json_bak = instance_dir.join(format!("{}.json.bak", custom_instance_name));
            let json_src = if existing_json.exists() {
                existing_json
            } else {
                existing_json_bak
            };
            if existing_jar.exists() && !base_client_jar.exists() {
                let _ = tokio::fs::create_dir_all(&base_version_dir).await;
                let _ = tokio::fs::copy(&existing_jar, &base_client_jar).await;
            }
            if json_src.exists() && !base_version_json.exists() {
                let _ = tokio::fs::create_dir_all(&base_version_dir).await;
                let _ = tokio::fs::copy(&json_src, &base_version_json).await;
            }
            let cache_mc_dir = dawnland_cache.join(mc_version);
            let cache_jar = cache_mc_dir.join(format!("{}.jar", mc_version));
            let cache_json = cache_mc_dir.join(format!("{}.json", mc_version));
            if existing_jar.exists() && !cache_jar.exists() {
                let _ = tokio::fs::create_dir_all(&cache_mc_dir).await;
                let _ = tokio::fs::copy(&existing_jar, &cache_jar).await;
            }
            if json_src.exists() && !cache_json.exists() {
                let _ = tokio::fs::create_dir_all(&cache_mc_dir).await;
                let _ = tokio::fs::copy(&json_src, &cache_json).await;
            }
        }

        if custom_instance_name != mc_version {
            ctx.manager
                .wait_for_instance(mc_version, Some(&ctx.id), &ctx.cancel_token)
                .await;
        }

        if !base_client_jar.exists() || !base_version_json.exists() {
            // Need to install base vanilla version first
            tracing::info!(
                "Base Minecraft {} not installed, installing first...",
                mc_version
            );

            ctx.update_progress(0, 0, "Fetching Minecraft version manifest...")
                .await;

            // Get version JSON URL from Mojang
            let manifest_url = "https://launchermeta.mojang.com/mc/game/version_manifest_v2.json";
            let settings = crate::core::settings::get_launcher_settings_sync();
            let manifest_url = crate::core::settings::replace_download_url(
                manifest_url,
                &settings.download_source,
            );

            let manifest: serde_json::Value = client
                .get(&manifest_url)
                .timeout(std::time::Duration::from_secs(15))
                .send()
                .await
                .map_err(|e| {
                    TaskError::ExecutionError(format!("Failed to download version JSON: {}", e))
                })?
                .error_for_status()
                .map_err(|e| {
                    TaskError::ExecutionError(format!("HTTP error downloading version JSON: {}", e))
                })?
                .json()
                .await
                .map_err(|e| {
                    TaskError::ExecutionError(format!("Failed to parse version manifest: {}", e))
                })?;

            // Find the version URL for requested mc_version
            let version_url = manifest["versions"]
                .as_array()
                .and_then(|versions| {
                    versions
                        .iter()
                        .find(|v| v["id"].as_str() == Some(mc_version))
                })
                .and_then(|v| v["url"].as_str())
                .ok_or_else(|| {
                    TaskError::ExecutionError(format!(
                        "Version {} not found in manifest",
                        mc_version
                    ))
                })?;

            let vanilla_task = InstallVanillaTask {
                options: VanillaInstallOptions {
                    version_id: mc_version.clone(),
                    version_json_url: version_url.to_string(),
                    custom_instance_name: None,
                    is_dependency: Some(effective_is_dep),
                },
            };
            vanilla_task.execute(ctx.clone()).await?;

            tracing::info!("Base vanilla {} installed successfully", mc_version);
        } else {
            tracing::info!("Base Minecraft {} already installed, skipping", mc_version);
            ctx.with_sub_task("download_vanilla_json")
                .update_progress(100, 100, "Skipped (Already exists)")
                .await;
            ctx.with_sub_task("download_vanilla_libs")
                .update_progress(100, 100, "Skipped (Already exists)")
                .await;
            ctx.with_sub_task("download_vanilla_assets")
                .update_progress(100, 100, "Skipped (Already exists)")
                .await;
            ctx.with_sub_task("download_vanilla_client")
                .update_progress(100, 100, "Skipped (Already exists)")
                .await;
        }

        if ctx.is_cancelled() {
            return Err(TaskError::ExecutionError(
                "Installation cancelled by user".to_string(),
            ));
        }

        let ctx_resolve = ctx.with_sub_task("resolve_loader");

        if !is_dep {
            ctx.set_total_steps(2).await;
            // Step 2: Install Fabric profile
            ctx_resolve
                .update_progress(0, 100, "Fetching Fabric profile...")
                .await;
        } else {
            ctx_resolve
                .update_progress(0, 100, "Fetching Fabric profile...")
                .await;
        }

        // Fetch Fabric profile JSON
        let fabric_url = format!(
            "{}/versions/loader/{}/{}/profile/json",
            FABRIC_META_BASE, mc_version, fabric_version
        );
        let settings = crate::core::settings::get_launcher_settings_sync();
        let fabric_url =
            crate::core::settings::replace_download_url(&fabric_url, &settings.download_source);

        tracing::info!("Fetching Fabric profile from: {}", fabric_url);

        let profile_json = client
            .get(&fabric_url)
            .send()
            .await
            .map_err(|e| {
                TaskError::ExecutionError(format!("Failed to download Fabric profile: {}", e))
            })?
            .error_for_status()
            .map_err(|e| {
                TaskError::ExecutionError(format!("HTTP error downloading Fabric profile: {}", e))
            })?
            .text()
            .await
            .map_err(|e| {
                TaskError::ExecutionError(format!("Failed to read Fabric profile: {}", e))
            })?;

        // Parse and modify the profile
        let mut profile: serde_json::Value = serde_json::from_str(&profile_json).map_err(|e| {
            TaskError::ExecutionError(format!("Failed to parse Fabric profile JSON: {}", e))
        })?;

        // Modify id and set inheritsFrom
        if let Some(obj) = profile.as_object_mut() {
            obj.insert("id".to_string(), serde_json::json!(custom_instance_name));

            let settings = crate::core::settings::load_launcher_settings()
                .await
                .unwrap_or_default();
            if !settings.enable_instance_inheritance || custom_instance_name == mc_version {
                crate::core::utils::flatten_instance_json_recursive(mc_version, obj)
                    .await
                    .map_err(TaskError::ExecutionError)?;
                obj.remove("inheritsFrom");
                obj.insert("clientVersion".to_string(), serde_json::json!(mc_version));

                // Any failures are logged but do not fail the overall task.
                let parent_jar = base_version_dir.join(format!("{}.jar", mc_version));
                let dest_jar = instance_dir.join(format!("{}.jar", custom_instance_name));
                crate::core::utils::copy_jar_if_exists_with_logging(&parent_jar, &dest_jar).await;
            } else {
                obj.insert("inheritsFrom".to_string(), serde_json::json!(mc_version));
            }

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
        let version_dir = instance_dir.clone();

        tokio::fs::create_dir_all(&version_dir).await.map_err(|e| {
            TaskError::ExecutionError(format!("Failed to create version directory: {}", e))
        })?;

        let version_json_path = version_dir.join(format!("{}.json", custom_instance_name));
        let updated_json = serde_json::to_string_pretty(&profile).map_err(|e| {
            TaskError::ExecutionError(format!("Failed to serialize Fabric profile: {}", e))
        })?;

        tokio::fs::write(&version_json_path, &updated_json)
            .await
            .map_err(|e| {
                TaskError::ExecutionError(format!("Failed to write Fabric profile: {}", e))
            })?;

        tracing::info!("Saved Fabric profile to: {:?}", version_json_path);

        if ctx.is_cancelled() {
            return Err(TaskError::ExecutionError(
                "Installation cancelled by user".to_string(),
            ));
        }

        ctx_resolve
            .update_progress(100, 100, "Fabric profile resolved")
            .await;

        // Step 3: Download Fabric libraries
        let ctx_libs = ctx.with_sub_task("download_loader_libs");

        ctx_libs
            .update_progress(0, 0, "Resolving Fabric libraries...")
            .await;

        let libraries: &[serde_json::Value] =
            match profile.get("libraries").and_then(|l| l.as_array()) {
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
                let replaced_url = crate::core::settings::replace_download_url(
                    &download_url,
                    &settings.download_source,
                );
                tracing::debug!(
                    "Added Fabric library: {} -> {}",
                    relative_path,
                    replaced_url
                );
                tasks.push(crate::downloader::DownloadTask::new(
                    replaced_url,
                    dest.to_string_lossy().to_string(),
                    None, // SHA1 not available for Maven coordinates
                    None,
                ));
            }
        }

        let total_tasks = tasks.len();
        tracing::info!("Resolved {} Fabric library files", total_tasks);

        if !tasks.is_empty() {
            if let Err(e) = crate::downloader::run_batch_download_task(tasks, ctx_libs).await {
                tracing::warn!("Installation failed during batch download, cleaning up...");
                let version_dir = instance_dir.clone();
                if is_existing {
                    if json_bak.exists() {
                        let _ = tokio::fs::copy(&json_bak, version_dir.join(format!("{}.json", custom_instance_name))).await;
                        let _ = tokio::fs::remove_file(&json_bak).await;
                    }
                    if config_bak.exists() {
                        let _ = tokio::fs::copy(&config_bak, version_dir.join("dlml.json")).await;
                        let _ = tokio::fs::remove_file(&config_bak).await;
                    }
                    let _ = crate::core::launcher::update_instance_config(&version_dir, |c| {
                        c.set_status(crate::core::launcher::InstanceStatus::Ready);
                    }).await;
                } else {
                    let _ = tokio::fs::remove_dir_all(&version_dir).await;
                }
                return Err(TaskError::ExecutionError(e));
            }
        }

        if ctx.is_cancelled() {
            tracing::warn!("Installation cancelled, cleaning up fabric instance directory...");
            let version_dir = instance_dir.clone();
            if is_existing {
                if json_bak.exists() {
                    let _ = tokio::fs::copy(&json_bak, version_dir.join(format!("{}.json", custom_instance_name))).await;
                    let _ = tokio::fs::remove_file(&json_bak).await;
                }
                if config_bak.exists() {
                    let _ = tokio::fs::copy(&config_bak, version_dir.join("dlml.json")).await;
                    let _ = tokio::fs::remove_file(&config_bak).await;
                }
                let _ = crate::core::launcher::update_instance_config(&version_dir, |c| {
                    c.set_status(crate::core::launcher::InstanceStatus::Ready);
                }).await;
            } else {
                let _ = tokio::fs::remove_dir_all(&version_dir).await;
            }
            return Err(TaskError::ExecutionError(
                "Installation cancelled by user".to_string(),
            ));
        }

        // Step 4: Update dlml.json config atomically
        crate::core::launcher::update_instance_config(&version_dir, |config| {
            config.hidden = is_dependency.unwrap_or(false);
            config.loader_type = Some(crate::models::instance::LoaderType::Fabric);
            config.loader_version = Some(fabric_version.clone());
            config.mc_version = Some(mc_version.clone());
            config.set_status(crate::core::launcher::InstanceStatus::Ready);
        })
        .await
        .map_err(TaskError::ExecutionError)?;

        if is_existing {
            let _ = tokio::fs::remove_file(&json_bak).await;
            let _ = tokio::fs::remove_file(&config_bak).await;
        }

        tracing::info!("Updated instance config in: {:?}", version_dir);

        if !is_dependency.unwrap_or(false) {
            crate::core::launcher::apply_global_game_settings(&version_dir).await;

            // Step 5: Auto-install Fabric API
            auto_install_fabric_api(
                &version_dir,
                mc_version,
                custom_instance_name,
                ctx.with_sub_task("install_fabric_api"),
            )
            .await;
        } else {
            ctx.with_sub_task("install_fabric_api")
                .update_progress(100, 100, "Skipped (Dependency)")
                .await;
        }

        if ctx.is_cancelled() {
            return Err(TaskError::ExecutionError(
                "Installation cancelled by user".to_string(),
            ));
        }

        // Emit complete
        ctx.update_progress(100, 100, "Complete").await;

        tracing::info!(
            "Fabric instance '{}' installed successfully!",
            custom_instance_name
        );
        Ok(())
    }
}

/// Check if Fabric API is already installed in the instance
async fn is_fabric_api_installed(instance_dir: &std::path::Path) -> bool {
    let mods_dir = instance_dir.join("mods");
    if let Ok(mut entries) = tokio::fs::read_dir(&mods_dir).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            let name = entry.file_name().to_string_lossy().to_lowercase();
            if (name.starts_with("fabric-api") || name.starts_with("fabric_api"))
                && (name.ends_with(".jar") || name.ends_with(".disable"))
            {
                return true;
            }
        }
    }

    let assets_path = instance_dir.join("assets.json");
    if let Ok(content) = tokio::fs::read_to_string(&assets_path).await {
        if let Ok(manifest) = serde_json::from_str::<crate::models::instance::AssetManifest>(&content) {
            for (path_key, record) in &manifest.assets {
                if let Some(pid) = &record.project_id {
                    let pid_lower = pid.to_lowercase();
                    if pid_lower == "fabric-api" || pid == "p7dr8msh" || pid == "P7dR8mSH" || pid == "306612" {
                        let full_path = instance_dir.join(path_key);
                        if full_path.exists() {
                            return true;
                        }
                    }
                }
            }
        }
    }

    false
}

/// Automatically resolve, download, and register Fabric API for a Fabric instance
async fn auto_install_fabric_api(
    instance_dir: &std::path::Path,
    mc_version: &str,
    instance_name: &str,
    ctx_api: TaskContext,
) {
    if ctx_api.is_cancelled() {
        return;
    }

    ctx_api
        .update_progress(0, 100, "Checking Fabric API...")
        .await;

    let mods_dir = instance_dir.join("mods");
    if let Err(e) = tokio::fs::create_dir_all(&mods_dir).await {
        tracing::warn!("Failed to create mods directory: {}", e);
        ctx_api
            .update_progress(100, 100, "Skipped (Failed to create mods directory)")
            .await;
        return;
    }

    if is_fabric_api_installed(instance_dir).await {
        tracing::info!(
            "Fabric API already installed in instance {}, skipping auto-install",
            instance_name
        );
        ctx_api
            .update_progress(100, 100, "Fabric API already installed")
            .await;
        return;
    }

    if ctx_api.is_cancelled() {
        return;
    }

    ctx_api
        .update_progress(10, 100, "Searching Fabric API...")
        .await;

    // 1. Search Modrinth
    let mod_file = match crate::core::modrinth::get_modrinth_mod_files(
        "fabric-api".to_string(),
        mc_version.to_string(),
        vec!["fabric".to_string()],
    )
    .await
    {
        Ok(files) if !files.is_empty() => {
            let file = files
                .iter()
                .find(|f| f.release_type == "release")
                .unwrap_or(&files[0])
                .clone();
            Some((
                file,
                crate::models::instance::AssetSourceType::Modrinth,
                "P7dR8mSH".to_string(),
            ))
        }
        _ => None,
    };

    // 2. Search CurseForge as fallback
    let mod_file = if let Some(m) = mod_file {
        Some(m)
    } else {
        match crate::core::curseforge::get_cf_mod_files(
            "306612".to_string(),
            mc_version.to_string(),
            vec!["fabric".to_string()],
        )
        .await
        {
            Ok(files) if !files.is_empty() => {
                let file = files
                    .iter()
                    .find(|f| f.release_type == "release")
                    .unwrap_or(&files[0])
                    .clone();
                Some((
                    file,
                    crate::models::instance::AssetSourceType::CurseForge,
                    "306612".to_string(),
                ))
            }
            _ => None,
        }
    };

    let (file, source_type, project_id) = match mod_file {
        Some(info) => info,
        None => {
            tracing::warn!(
                "Could not find compatible Fabric API for Minecraft version {}. Skipping Fabric API installation.",
                mc_version
            );
            ctx_api
                .update_progress(100, 100, "Fabric API not found (Skipped)")
                .await;
            return;
        }
    };

    if ctx_api.is_cancelled() {
        return;
    }

    let safe_filename = std::path::Path::new(&file.filename)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(&file.filename);
    let dest = mods_dir.join(safe_filename);
    let rel_path_str = format!("mods/{}", safe_filename);

    if !dest.starts_with(&mods_dir) {
        tracing::warn!("Target path escapes mods directory: {:?}", dest);
        return;
    }

    ctx_api
        .update_progress(20, 100, &format!("Downloading {}...", safe_filename))
        .await;

    let dl_task = crate::downloader::DownloadTask::new(
        file.download_url.clone(),
        dest.to_string_lossy().to_string(),
        file.hash.clone(),
        file.file_size,
    );

    if let Err(e) = crate::downloader::run_batch_download_task(vec![dl_task], ctx_api.clone()).await {
        if ctx_api.is_cancelled() {
            tracing::info!("Fabric API download cancelled by user");
            return;
        }
        tracing::warn!("Failed to download Fabric API: {}. Proceeding without it.", e);
        ctx_api
            .update_progress(100, 100, "Download failed (Skipped)")
            .await;
        return;
    }

    // Register into assets.json
    let record = crate::models::instance::AssetRecord {
        category: crate::models::instance::AssetCategory::Mod,
        source_type,
        project_id: Some(project_id),
        version_id: Some(file.id),
        download_url: Some(file.download_url),
        hash_sha1: file.hash,
        hash_sha512: None,
        size: file.file_size,
        enabled: true,
        managed_by_modpack: false,
    };

    if let Err(e) = crate::core::manager::save_asset_to_manifest(instance_name, rel_path_str, record).await {
        tracing::warn!("Failed to save Fabric API to manifest: {}", e);
    }

    ctx_api
        .update_progress(100, 100, "Fabric API installed")
        .await;
    tracing::info!(
        "Fabric API ({}) installed successfully for instance {}",
        safe_filename,
        instance_name
    );
}

/// Install a Fabric instance - automatically installs base vanilla first, then Fabric.
#[tauri::command]
pub async fn install_fabric_instance(
    mc_version: String,
    fabric_version: String,
    custom_instance_name: String,
    is_dependency: Option<bool>,
    app: AppHandle,
) -> Result<String, AppError> {
    if !crate::core::fs_utils::is_safe_single_name(&custom_instance_name)
        || !crate::core::fs_utils::is_safe_single_name(&mc_version)
    {
        return Err(crate::error::DawnlandError::Unknown(
            "Invalid instance name or Minecraft version".to_string(),
        )
        .into());
    }
    let task_manager = app.state::<TaskManager>().inner().clone();

    // Pre-create instance directory and dlml.json synchronously so frontend can detect it immediately
    let base_dir = crate::core::mojang::get_minecraft_base();
    let instance_dir = if is_dependency.unwrap_or(false) {
        crate::core::mojang::get_dawnland_cache().join(&custom_instance_name)
    } else {
        let dir = base_dir.join("versions").join(&custom_instance_name);
        if dir.exists() {
            return Err(crate::error::DawnlandError::Unknown(format!("Instance with name '{}' already exists", custom_instance_name)).into());
        }
        dir
    };
    let _startup_guard = task_manager.start_instance_install(&custom_instance_name);
    let _ = tokio::fs::create_dir_all(&instance_dir).await;
    let config_path = instance_dir.join("dlml.json");
    let mut pre_config = crate::core::launcher::InstanceConfig {
        hidden: is_dependency.unwrap_or(false),
        ..Default::default()
    };
    pre_config.set_status(crate::core::launcher::InstanceStatus::Installing);
    crate::core::launcher::write_instance_config_atomic(&config_path, &pre_config)
        .await
        .map_err(crate::error::DawnlandError::Unknown)?;

    let task = InstallFabricTask {
        options: InstallFabricOptions {
            mc_version: mc_version.clone(),
            fabric_version: fabric_version.clone(),
            custom_instance_name: custom_instance_name.clone(),
            is_dependency,
            is_existing_instance: None,
        },
    };

    let task_id = task_manager
        .spawn_task(
            TaskType::InstallFabric {
                mc_version: mc_version.clone(),
                fabric_version: fabric_version.clone(),
                custom_instance_name: custom_instance_name.clone(),
                is_dependency,
            },
            task,
        )
        .await
        .map_err(|e| format!("Failed to spawn task: {}", e))?;

    Ok(task_id)
}

/// Check if a base Minecraft version is installed (has client.jar).
#[tauri::command]
pub async fn check_vanilla_installed(mc_version: String) -> Result<bool, AppError> {
    let base_dir = get_minecraft_base();
    let client_jar = base_dir
        .join("versions")
        .join(&mc_version)
        .join(format!("{}.jar", mc_version));

    let installed = client_jar.exists();
    tracing::info!("Vanilla {} installed: {}", mc_version, installed);
    Ok(installed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_fabric_loader_response() {
        let json = r#"[
            {
                "loader": {
                    "separator": ".",
                    "build": 1,
                    "maven": "net.fabricmc:fabric-loader:0.15.7",
                    "version": "0.15.7",
                    "stable": true
                }
            },
            {
                "loader": {
                    "separator": ".",
                    "build": 2,
                    "maven": "net.fabricmc:fabric-loader:0.15.8-beta.1",
                    "version": "0.15.8-beta.1",
                    "stable": false
                }
            }
        ]"#;

        let responses: Vec<FabricLoaderResponse> = serde_json::from_str(json).unwrap();
        assert_eq!(responses.len(), 2);

        let stable = &responses[0].loader;
        assert_eq!(stable.version, "0.15.7");
        assert!(stable.stable);
        assert_eq!(stable.build, Some(1));
        assert_eq!(
            stable.maven.as_ref().unwrap(),
            "net.fabricmc:fabric-loader:0.15.7"
        );

        let unstable = &responses[1].loader;
        assert_eq!(unstable.version, "0.15.8-beta.1");
        assert!(!unstable.stable);
    }

    #[test]
    fn test_fabric_subtasks_contain_fabric_api() {
        let subtasks = InstallFabricTask::get_sub_tasks();
        let api_task = subtasks.iter().find(|t| t.key == "install_fabric_api");
        assert!(api_task.is_some(), "install_fabric_api subtask should exist");
        let api_task = api_task.unwrap();
        assert_eq!(api_task.name, "Install Fabric API");
    }

    #[tokio::test]
    async fn test_is_fabric_api_installed() {
        let temp_dir = tempfile::tempdir().unwrap();
        let instance_dir = temp_dir.path();

        // 1. Initially false
        assert!(!is_fabric_api_installed(instance_dir).await);

        // 2. Detected when fabric-api jar in mods/
        let mods_dir = instance_dir.join("mods");
        tokio::fs::create_dir_all(&mods_dir).await.unwrap();
        let jar_path = mods_dir.join("fabric-api-0.92.2+1.20.1.jar");
        tokio::fs::write(&jar_path, b"dummy jar").await.unwrap();
        assert!(is_fabric_api_installed(instance_dir).await);

        // 3. Detected when fabric-api is disabled
        tokio::fs::remove_file(&jar_path).await.unwrap();
        let disabled_path = mods_dir.join("fabric-api-0.92.2+1.20.1.jar.disable");
        tokio::fs::write(&disabled_path, b"dummy jar").await.unwrap();
        assert!(is_fabric_api_installed(instance_dir).await);

        // 4. Detected via assets.json
        tokio::fs::remove_file(&disabled_path).await.unwrap();
        assert!(!is_fabric_api_installed(instance_dir).await);

        let custom_mod_path = mods_dir.join("my-custom-name.jar");
        tokio::fs::write(&custom_mod_path, b"dummy jar").await.unwrap();

        let mut manifest = crate::models::instance::AssetManifest::default();
        manifest.assets.insert(
            "mods/my-custom-name.jar".to_string(),
            crate::models::instance::AssetRecord {
                category: crate::models::instance::AssetCategory::Mod,
                source_type: crate::models::instance::AssetSourceType::Modrinth,
                project_id: Some("P7dR8mSH".to_string()),
                version_id: None,
                download_url: None,
                hash_sha1: None,
                hash_sha512: None,
                size: None,
                enabled: true,
                managed_by_modpack: false,
            },
        );
        let manifest_content = serde_json::to_string(&manifest).unwrap();
        tokio::fs::write(instance_dir.join("assets.json"), manifest_content).await.unwrap();
        assert!(is_fabric_api_installed(instance_dir).await);
    }
}
