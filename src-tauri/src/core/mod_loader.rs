use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};
use serde::{Deserialize, Serialize};
use crate::core::task::manager::TaskManager;
use crate::core::task::state::TaskType;
use crate::core::task::{ExecutableTask, TaskContext, TaskError};
use crate::error::{AppError, DawnlandError};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceLoaderInfo {
    pub instance_id: String,
    pub mc_version: String,
    pub loader_type: String,
    pub loader_version: Option<String>,
    pub is_inherited: bool,
    pub has_mods: bool,
    pub mods_count: usize,
}

/// Helper function to detect loader version from instance JSON content and ID
pub fn extract_loader_version_from_json(json: &serde_json::Value, instance_id: &str) -> Option<String> {
    // 1. Check arguments.game for explicit loader version flags
    if let Some(args) = json.get("arguments").and_then(|a| a.get("game")).and_then(|g| g.as_array()) {
        for (i, val) in args.iter().enumerate() {
            if let Some(s) = val.as_str() {
                if s == "--fml.neoForgeVersion" || s == "--fml.forgeVersion" {
                    if let Some(next_val) = args.get(i + 1).and_then(|v| v.as_str()) {
                        return Some(next_val.to_string());
                    }
                }
            }
        }
    }

    // 2. Check libraries array for explicit loader dependencies
    if let Some(libs) = json.get("libraries").and_then(|l| l.as_array()) {
        for lib in libs {
            if let Some(name) = lib.get("name").and_then(|n| n.as_str()) {
                if name.starts_with("net.neoforged:neoforge:") {
                    let parts: Vec<&str> = name.split(':').collect();
                    if parts.len() >= 3 {
                        return Some(parts[2].to_string());
                    }
                } else if name.starts_with("net.minecraftforge:forge:") {
                    let parts: Vec<&str> = name.split(':').collect();
                    if parts.len() >= 3 {
                        let ver = parts[2];
                        if let Some(idx) = ver.find('-') {
                            return Some(ver[idx + 1..].to_string());
                        } else {
                            return Some(ver.to_string());
                        }
                    }
                } else if name.starts_with("net.fabricmc:fabric-loader:")
                    || name.starts_with("org.quiltmc:quilt-loader:")
                {
                    let parts: Vec<&str> = name.split(':').collect();
                    if parts.len() >= 3 {
                        return Some(parts[2].to_string());
                    }
                }
            }
        }
    }

    // 3. Fallback to extracting from inherited IDs or instance_id
    for id_to_check in [instance_id, json.get("inheritsFrom").and_then(|v| v.as_str()).unwrap_or("")] {
        let lower = id_to_check.to_lowercase();
        if lower.starts_with("fabric-loader-") {
            let parts: Vec<&str> = id_to_check.split('-').collect();
            if parts.len() >= 3 {
                return Some(parts[2].to_string());
            }
        } else if lower.starts_with("forge-") || lower.starts_with("neoforge-") {
            let parts: Vec<&str> = id_to_check.split('-').collect();
            if parts.len() >= 3 {
                return Some(parts[2..].join("-"));
            } else if parts.len() == 2 {
                return Some(parts[1].to_string());
            }
        }
    }

    None
}

/// Helper to get an instance directory and verify it exists
pub fn get_instance_dir(instance_id: &str) -> Result<PathBuf, AppError> {
    if !crate::core::fs_utils::is_safe_single_name(instance_id) {
        return Err(DawnlandError::Unknown("Invalid instance name".to_string()).into());
    }
    let base_dir = crate::core::mojang::get_minecraft_base();
    let instance_dir = base_dir.join("versions").join(instance_id);
    if !instance_dir.exists() {
        return Err(DawnlandError::InstanceNotFound(instance_id.to_string()).into());
    }
    Ok(instance_dir)
}

/// Helper to retrieve the instance's Minecraft version
pub async fn resolve_instance_mc_version(
    instance_id: &str,
    instance_dir: &Path,
    hint_mc_version: Option<String>,
) -> Result<String, AppError> {
    if let Some(hint) = hint_mc_version {
        let trimmed = hint.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }

    let config_path = instance_dir.join("dlml.json");
    if let Ok(content) = tokio::fs::read_to_string(&config_path).await {
        if let Ok(config) = serde_json::from_str::<crate::core::launcher::InstanceConfig>(&content) {
            if let Some(ref mc) = config.mc_version {
                let trimmed = mc.trim();
                if !trimmed.is_empty() {
                    return Ok(trimmed.to_string());
                }
            }
        }
    }

    let json_path = instance_dir.join(format!("{}.json", instance_id));
    if let Ok(content) = tokio::fs::read_to_string(&json_path).await {
        let (mc_version, ..) = crate::core::manager::parse_version_json(&content, instance_id);

        let base_dir = crate::core::mojang::get_minecraft_base();
        let versions_dir = base_dir.join("versions");
        let cache_dir = crate::core::mojang::get_dawnland_cache();
        let mut current_version = mc_version.clone();
        let mut depth = 0;

        while depth < 10 {
            if current_version.is_empty() {
                break;
            }
            let mut inherited_path = versions_dir.join(&current_version).join(format!("{}.json", current_version));
            if !inherited_path.exists() {
                inherited_path = cache_dir.join(&current_version).join(format!("{}.json", current_version));
            }
            if let Ok(inherited_content) = tokio::fs::read_to_string(&inherited_path).await {
                let (real_mc, ..) = crate::core::manager::parse_version_json(&inherited_content, &current_version);
                if real_mc == current_version || real_mc.is_empty() {
                    break;
                }
                current_version = real_mc;
            } else {
                break;
            }
            depth += 1;
        }

        let resolved = if !current_version.is_empty() {
            current_version
        } else {
            mc_version
        };

        if !resolved.is_empty() {
            return Ok(resolved);
        }

        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(assets) = json.get("assetIndex").and_then(|a| a.get("id")).and_then(|v| v.as_str()) {
                if !assets.is_empty() {
                    return Ok(assets.to_string());
                }
            }
            if let Some(assets) = json.get("assets").and_then(|v| v.as_str()) {
                if !assets.is_empty() {
                    return Ok(assets.to_string());
                }
            }
        }
    }

    Err(DawnlandError::Unknown("Unable to determine Minecraft version for instance".to_string()).into())
}

/// Retrieve comprehensive information about the instance's current loader state
#[tauri::command]
pub async fn get_instance_loader_info(
    instance_id: String,
    mc_version: Option<String>,
) -> Result<InstanceLoaderInfo, AppError> {
    let instance_dir = get_instance_dir(&instance_id)?;
    let mc_version = resolve_instance_mc_version(&instance_id, &instance_dir, mc_version).await?;

    let json_path = instance_dir.join(format!("{}.json", instance_id));
    let config_path = instance_dir.join("dlml.json");

    let mut loader_type = "Vanilla".to_string();
    let mut loader_version = None;
    let mut is_inherited = false;

    if let Ok(content) = tokio::fs::read_to_string(&json_path).await {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(inherits) = json.get("inheritsFrom").and_then(|v| v.as_str()) {
                if !inherits.is_empty() {
                    is_inherited = true;
                }
            }
            loader_version = extract_loader_version_from_json(&json, &instance_id);
        }
        let (_, l_type, ..) = crate::core::manager::parse_version_json(&content, &instance_id);
        loader_type = l_type;
    }

    if let Ok(content) = tokio::fs::read_to_string(&config_path).await {
        if let Ok(config) = serde_json::from_str::<crate::core::launcher::InstanceConfig>(&content) {
            if let Some(ref lt) = config.loader_type {
                loader_type = format!("{:?}", lt);
            }
            if let Some(ref lv) = config.loader_version {
                if !lv.is_empty() {
                    loader_version = Some(lv.clone());
                }
            }
        }
    }

    let mods_dir = instance_dir.join("mods");
    let mut mods_count = 0;
    let mut has_mods = false;
    if mods_dir.exists() {
        if let Ok(mut entries) = tokio::fs::read_dir(&mods_dir).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let p = entry.path();
                if p.is_file() {
                    if let Some(ext) = p.extension().and_then(|e| e.to_str()) {
                        if ext.eq_ignore_ascii_case("jar") || ext.eq_ignore_ascii_case("disable") {
                            mods_count += 1;
                        }
                    }
                }
            }
        }
        has_mods = mods_count > 0;
    }

    Ok(InstanceLoaderInfo {
        instance_id,
        mc_version,
        loader_type,
        loader_version,
        is_inherited,
        has_mods,
        mods_count,
    })
}

/// Helper function to spawn loader installation or version change tasks
pub async fn spawn_loader_install_task(
    task_manager: &TaskManager,
    task_id: Option<String>,
    instance_id: String,
    mc_version: String,
    loader_type: String,
    loader_version: String,
    task_type: TaskType,
) -> Result<String, String> {
    let normalized = loader_type.to_lowercase();
    if normalized == "fabric" {
        let task = crate::core::fabric::InstallFabricTask {
            options: crate::core::fabric::InstallFabricOptions {
                mc_version,
                fabric_version: loader_version,
                custom_instance_name: instance_id,
                is_dependency: Some(false),
                is_existing_instance: Some(true),
            },
        };
        if let Some(id) = task_id {
            task_manager.spawn_task_with_id(id, task_type, task).await.map_err(|e| e.to_string())
        } else {
            task_manager.spawn_task(task_type, task).await.map_err(|e| e.to_string())
        }
    } else if normalized == "forge" || normalized == "neoforge" {
        let task = crate::core::forge::InstallForgeTask {
            options: crate::core::forge::InstallForgeOptions {
                mc_version,
                loader_version,
                loader_type: normalized,
                custom_instance_name: instance_id,
                is_dependency: Some(false),
                is_existing_instance: Some(true),
            },
        };
        if let Some(id) = task_id {
            task_manager.spawn_task_with_id(id, task_type, task).await.map_err(|e| e.to_string())
        } else {
            task_manager.spawn_task(task_type, task).await.map_err(|e| e.to_string())
        }
    } else {
        Err(format!("Unsupported mod loader type: {}", loader_type))
    }
}

/// Install a mod loader onto an existing vanilla instance
#[tauri::command]
pub async fn install_instance_loader(
    instance_id: String,
    loader_type: String,
    loader_version: String,
    mc_version: Option<String>,
    app: AppHandle,
) -> Result<String, AppError> {
    let instance_dir = get_instance_dir(&instance_id)?;
    let mc_version = resolve_instance_mc_version(&instance_id, &instance_dir, mc_version).await?;

    let task_manager = app.state::<TaskManager>().inner().clone();
    if task_manager.is_instance_busy(&instance_id).await {
        return Err(DawnlandError::Unknown(format!("Instance '{}' is currently busy", instance_id)).into());
    }

    let task_type = TaskType::InstallLoaderToInstance {
        instance_id: instance_id.clone(),
        mc_version: mc_version.clone(),
        loader_type: loader_type.clone(),
        loader_version: loader_version.clone(),
    };

    spawn_loader_install_task(
        &task_manager,
        None,
        instance_id,
        mc_version,
        loader_type,
        loader_version,
        task_type,
    )
    .await
    .map_err(|e| DawnlandError::Unknown(e).into())
}

/// Switch loader version (upgrade or downgrade) on an existing loader instance
#[tauri::command]
pub async fn switch_instance_loader_version(
    instance_id: String,
    loader_type: String,
    loader_version: String,
    mc_version: Option<String>,
    app: AppHandle,
) -> Result<String, AppError> {
    let instance_dir = get_instance_dir(&instance_id)?;
    let mc_version = resolve_instance_mc_version(&instance_id, &instance_dir, mc_version).await?;

    let task_manager = app.state::<TaskManager>().inner().clone();
    if task_manager.is_instance_busy(&instance_id).await {
        return Err(DawnlandError::Unknown(format!("Instance '{}' is currently busy", instance_id)).into());
    }

    let info = get_instance_loader_info(instance_id.clone(), Some(mc_version.clone())).await.ok();
    let old_version = info.and_then(|i| i.loader_version);

    let task_type = TaskType::SwitchInstanceLoaderVersion {
        instance_id: instance_id.clone(),
        mc_version: mc_version.clone(),
        loader_type: loader_type.clone(),
        old_loader_version: old_version,
        new_loader_version: loader_version.clone(),
    };

    spawn_loader_install_task(
        &task_manager,
        None,
        instance_id,
        mc_version,
        loader_type,
        loader_version,
        task_type,
    )
    .await
    .map_err(|e| DawnlandError::Unknown(e).into())
}

/// Uninstall mod loader from an instance and revert to pure vanilla
#[tauri::command]
pub async fn uninstall_instance_loader(
    instance_id: String,
    mod_handling_mode: String,
    mc_version: Option<String>,
    app: AppHandle,
) -> Result<String, AppError> {
    let instance_dir = get_instance_dir(&instance_id)?;
    let mc_version = resolve_instance_mc_version(&instance_id, &instance_dir, mc_version).await?;

    let task_manager = app.state::<TaskManager>().inner().clone();
    if task_manager.is_instance_busy(&instance_id).await {
        return Err(DawnlandError::Unknown(format!("Instance '{}' is currently busy", instance_id)).into());
    }

    let info = get_instance_loader_info(instance_id.clone(), Some(mc_version.clone())).await?;
    let old_loader = info.loader_type;

    let task = UninstallInstanceLoaderTask {
        options: UninstallInstanceLoaderOptions {
            instance_id: instance_id.clone(),
            mod_handling_mode: mod_handling_mode.clone(),
            mc_version: Some(mc_version.clone()),
        },
    };

    let task_type = TaskType::UninstallInstanceLoader {
        instance_id,
        mc_version,
        old_loader_type: old_loader,
        mod_handling_mode: Some(mod_handling_mode),
    };

    task_manager
        .spawn_task(task_type, task)
        .await
        .map_err(|e| DawnlandError::Unknown(e.to_string()).into())
}

/// Validate whether a JSON string is genuinely pure vanilla Minecraft (not Fabric, Forge, NeoForge, Quilt).
pub fn is_pure_vanilla_json(content: &str, expected_mc_version: &str) -> bool {
    let (_, loader_type, ..) = crate::core::manager::parse_version_json(content, expected_mc_version);
    if loader_type != "Vanilla" {
        return false;
    }
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(content) {
        if let Some(main_class) = val.get("mainClass").and_then(|v| v.as_str()) {
            let mc_lower = main_class.to_lowercase();
            if mc_lower.contains("fabric")
                || mc_lower.contains("knot")
                || mc_lower.contains("forge")
                || mc_lower.contains("bootstraplauncher")
            {
                return false;
            }
        }
        if let Some(inherits) = val.get("inheritsFrom").and_then(|v| v.as_str()) {
            let inh_lower = inherits.to_lowercase();
            if inh_lower.contains("fabric") || inh_lower.contains("forge") || inh_lower.contains("neoforge") {
                return false;
            }
        }
        return val.get("downloads").is_some() || val.get("assetIndex").is_some() || val.get("mainClass").is_some();
    }
    false
}

/// Acquire pure vanilla Minecraft version JSON content.
/// Tries dawnland_cache, then versions/{mc_version} (if not current instance), and finally Mojang manifest.
pub async fn acquire_pure_vanilla_json(
    mc_version: &str,
    current_instance_id: &str,
) -> Result<String, String> {
    let dawnland_cache = crate::core::mojang::get_dawnland_cache();
    let base_dir = crate::core::mojang::get_minecraft_base();

    // 1. Try dawnland_cache/{mc_version}/{mc_version}.json
    let cache_json = dawnland_cache.join(mc_version).join(format!("{}.json", mc_version));
    if cache_json.exists() {
        if let Ok(content) = tokio::fs::read_to_string(&cache_json).await {
            if is_pure_vanilla_json(&content, mc_version) {
                tracing::info!("Found pure vanilla JSON for {} in dawnland_cache", mc_version);
                return Ok(content);
            }
        }
    }

    // 2. Try versions/{mc_version}/{mc_version}.json ONLY if mc_version != current_instance_id
    if current_instance_id != mc_version {
        let base_json = base_dir.join("versions").join(mc_version).join(format!("{}.json", mc_version));
        if base_json.exists() {
            if let Ok(content) = tokio::fs::read_to_string(&base_json).await {
                if is_pure_vanilla_json(&content, mc_version) {
                    tracing::info!("Found pure vanilla JSON for {} in versions base directory", mc_version);
                    return Ok(content);
                }
            }
        }
    }

    // 3. Fallback: Download official pure vanilla JSON from Mojang version manifest!
    tracing::info!("No local pure vanilla JSON found for {}. Fetching from Mojang manifest...", mc_version);
    let vanilla_versions = crate::core::mojang::get_vanilla_versions().await
        .map_err(|e| format!("Failed to fetch vanilla versions: {}", e))?;

    let v = vanilla_versions
        .iter()
        .find(|v| v.id == mc_version)
        .ok_or_else(|| format!("Minecraft version '{}' not found in Mojang version manifest", mc_version))?;

    let client = crate::core::utils::get_http_client().clone();
    let settings = crate::core::settings::get_launcher_settings_sync();
    let json_url = crate::core::settings::replace_download_url(&v.url, &settings.download_source);

    let content = client
        .get(&json_url)
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| format!("Failed to download vanilla JSON from Mojang: {}", e))?
        .error_for_status()
        .map_err(|e| format!("HTTP error downloading vanilla JSON: {}", e))?
        .text()
        .await
        .map_err(|e| format!("Failed to read vanilla JSON content: {}", e))?;

    // Cache it to dawnland_cache for future use
    let cache_dir = dawnland_cache.join(mc_version);
    let _ = tokio::fs::create_dir_all(&cache_dir).await;
    let _ = tokio::fs::write(cache_dir.join(format!("{}.json", mc_version)), &content).await;

    Ok(content)
}

pub struct UninstallInstanceLoaderOptions {
    pub instance_id: String,
    pub mod_handling_mode: String,
    pub mc_version: Option<String>,
}

pub struct UninstallInstanceLoaderTask {
    pub options: UninstallInstanceLoaderOptions,
}

#[async_trait::async_trait]
impl ExecutableTask for UninstallInstanceLoaderTask {
    async fn execute(&self, ctx: TaskContext) -> Result<(), TaskError> {
        let instance_id = &self.options.instance_id;
        let mod_handling = &self.options.mod_handling_mode;

        ctx.update_progress(0, 100, "Starting uninstallation...").await;

        let base_dir = crate::core::mojang::get_minecraft_base();
        let instance_dir = base_dir.join("versions").join(instance_id);
        if !instance_dir.exists() {
            return Err(TaskError::ExecutionError(format!("Instance '{}' does not exist", instance_id)));
        }

        let json_path = instance_dir.join(format!("{}.json", instance_id));
        let config_path = instance_dir.join("dlml.json");

        // Determine Minecraft version
        let mc_version = resolve_instance_mc_version(instance_id, &instance_dir, self.options.mc_version.clone())
            .await
            .map_err(|e| TaskError::ExecutionError(e.message))?;

        // 1. Create backups
        let json_bak = instance_dir.join(format!("{}.json.bak", instance_id));
        let config_bak = instance_dir.join("dlml.json.bak");
        if json_path.exists() {
            let _ = crate::core::fs_utils::copy_file_atomic(&json_path, &json_bak).await;
        }
        if config_path.exists() {
            let _ = crate::core::fs_utils::copy_file_atomic(&config_path, &config_bak).await;
        }

        let rollback = || async {
            if json_bak.exists() {
                let _ = crate::core::fs_utils::copy_file_atomic(&json_bak, &json_path).await;
                let _ = tokio::fs::remove_file(&json_bak).await;
            }
            if config_bak.exists() {
                let _ = crate::core::fs_utils::copy_file_atomic(&config_bak, &config_path).await;
                let _ = tokio::fs::remove_file(&config_bak).await;
            }
            let _ = crate::core::launcher::update_instance_config(&instance_dir, |c| {
                c.set_status(crate::core::launcher::InstanceStatus::Ready);
            }).await;
        };

        // Mark instance as Updating
        let _ = crate::core::launcher::InstanceConfig::ensure_updating(&instance_dir).await;

        // 2. Reconstruct vanilla version profile JSON
        ctx.update_progress(20, 100, "Restoring vanilla profile...").await;

        let pure_vanilla_content = match acquire_pure_vanilla_json(&mc_version, instance_id).await {
            Ok(c) => c,
            Err(e) => {
                rollback().await;
                return Err(TaskError::ExecutionError(e));
            }
        };

        if ctx.is_cancelled() {
            rollback().await;
            return Err(TaskError::ExecutionError("Uninstallation cancelled by user".to_string()));
        }

        let mut parsed: serde_json::Value = match serde_json::from_str(&pure_vanilla_content) {
            Ok(v) => v,
            Err(e) => {
                rollback().await;
                return Err(TaskError::ExecutionError(format!("Failed to parse pure vanilla JSON: {}", e)));
            }
        };

        if let Some(obj) = parsed.as_object_mut() {
            obj.insert("id".to_string(), serde_json::json!(instance_id));
            obj.remove("inheritsFrom");
        }

        let serialized = match serde_json::to_string_pretty(&parsed) {
            Ok(s) => s,
            Err(e) => {
                rollback().await;
                return Err(TaskError::ExecutionError(format!("Failed to serialize restored vanilla JSON: {}", e)));
            }
        };

        if let Err(e) = crate::core::fs_utils::write_file_atomic(&json_path, serialized.as_bytes()).await {
            rollback().await;
            return Err(TaskError::ExecutionError(format!("Failed to write restored vanilla JSON: {}", e)));
        }

        // 3. Restore / ensure vanilla client jar
        ctx.update_progress(50, 100, "Ensuring vanilla client jar...").await;
        let dest_jar = instance_dir.join(format!("{}.jar", instance_id));
        let dawnland_cache = crate::core::mojang::get_dawnland_cache();
        let cached_jar = dawnland_cache.join(&mc_version).join(format!("{}.jar", mc_version));
        let base_jar = base_dir.join("versions").join(&mc_version).join(format!("{}.jar", mc_version));

        let mut jar_restored = false;
        if cached_jar.exists() {
            if !dest_jar.exists() || dest_jar != cached_jar {
                let _ = tokio::fs::copy(&cached_jar, &dest_jar).await;
            }
            jar_restored = true;
        } else if instance_id != &mc_version && base_jar.exists() {
            if !dest_jar.exists() || dest_jar != base_jar {
                let _ = tokio::fs::copy(&base_jar, &dest_jar).await;
            }
            jar_restored = true;
        }

        if !jar_restored && !dest_jar.exists() {
            if let Some(client_dl) = parsed.get("downloads").and_then(|d| d.get("client")) {
                if let Some(url) = client_dl.get("url").and_then(|u| u.as_str()) {
                    let sha1 = client_dl.get("sha1").and_then(|s| s.as_str()).map(|s| s.to_string());
                    let size = client_dl.get("size").and_then(|s| s.as_u64());
                    let settings = crate::core::settings::get_launcher_settings_sync();
                    let replaced_url = crate::core::settings::replace_download_url(url, &settings.download_source);
                    let dl_task = crate::downloader::DownloadTask::new(
                        replaced_url,
                        dest_jar.to_string_lossy().to_string(),
                        sha1,
                        size,
                    );
                    if let Err(e) = crate::downloader::run_batch_download_task(vec![dl_task], ctx.clone()).await {
                        rollback().await;
                        return Err(TaskError::ExecutionError(format!("Failed to download vanilla client jar: {}", e)));
                    }
                }
            }
        }

        if ctx.is_cancelled() {
            rollback().await;
            return Err(TaskError::ExecutionError("Uninstallation cancelled by user".to_string()));
        }

        // 4. Remove loader temporary artifacts
        let artifacts = [
            instance_dir.join(".fabric"),
            instance_dir.join(".fabric-installer"),
            instance_dir.join(".forge"),
            instance_dir.join(".neoforge"),
        ];
        for dir in &artifacts {
            if dir.exists() {
                let _ = tokio::fs::remove_dir_all(dir).await;
            }
        }

        // 5. Handle mods directory after vanilla profile and jar have been safely acquired
        ctx.update_progress(75, 100, "Handling mods folder...").await;
        let mods_dir = instance_dir.join("mods");
        let mods_trash = instance_dir.join(".mods_trash_tmp");
        let assets_path = instance_dir.join("assets.json");
        let mut backup_dir_opt: Option<std::path::PathBuf> = None;
        let mut moved_to_trash = false;
        let mut disabled_renames: Vec<(std::path::PathBuf, std::path::PathBuf)> = Vec::new();
        let mut original_assets_content: Option<String> = None;
        let mut previously_enabled_asset_keys: Vec<String> = Vec::new();

        if mods_dir.exists() {
            match mod_handling.as_str() {
                "disable" => {
                    if let Ok(mut entries) = tokio::fs::read_dir(&mods_dir).await {
                        while let Ok(Some(entry)) = entries.next_entry().await {
                            let p = entry.path();
                            if p.is_file() {
                                if let Some(ext) = p.extension().and_then(|e| e.to_str()) {
                                    if ext.eq_ignore_ascii_case("jar") {
                                        let file_name = p.file_name().unwrap().to_string_lossy();
                                        let disabled_name = format!("{}.disable", file_name);
                                        let disabled_path = mods_dir.join(disabled_name);
                                        if tokio::fs::rename(&p, &disabled_path).await.is_ok() {
                                            disabled_renames.push((disabled_path, p));
                                        }
                                    }
                                }
                            }
                        }
                    }
                    if assets_path.exists() {
                        if let Ok(content) = tokio::fs::read_to_string(&assets_path).await {
                            original_assets_content = Some(content.clone());
                            if let Ok(mut manifest) = serde_json::from_str::<crate::models::instance::AssetManifest>(&content) {
                                for (key, record) in manifest.assets.iter_mut() {
                                    if record.category == crate::models::instance::AssetCategory::Mod
                                        && record.enabled
                                    {
                                        previously_enabled_asset_keys.push(key.clone());
                                        record.enabled = false;
                                    }
                                }
                                if !previously_enabled_asset_keys.is_empty() {
                                    if let Ok(serialized) = serde_json::to_string_pretty(&manifest) {
                                        let _ = crate::core::fs_utils::write_file_atomic(&assets_path, serialized.as_bytes()).await;
                                    }
                                }
                            }
                        }
                    }
                }
                "backup" => {
                    let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
                    let b_dir = instance_dir.join(format!("mods_backup_{}", timestamp));
                    let mut renamed = false;
                    for _ in 0..3 {
                        if tokio::fs::rename(&mods_dir, &b_dir).await.is_ok() {
                            renamed = true;
                            break;
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    }
                    if renamed {
                        backup_dir_opt = Some(b_dir);
                    } else {
                        rollback().await;
                        return Err(TaskError::ExecutionError(
                            "Failed to backup mods directory (files may be in use)".to_string(),
                        ));
                    }
                }
                "delete" => {
                    // Move to temporary trash directory during uninstallation to allow rollback
                    if mods_trash.exists() {
                        let _ = tokio::fs::remove_dir_all(&mods_trash).await;
                    }
                    let mut renamed = false;
                    let mut rename_err = String::new();
                    for _ in 0..3 {
                        match tokio::fs::rename(&mods_dir, &mods_trash).await {
                            Ok(_) => {
                                renamed = true;
                                break;
                            }
                            Err(e) => {
                                rename_err = e.to_string();
                                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                            }
                        }
                    }
                    if renamed {
                        moved_to_trash = true;
                    } else {
                        rollback().await;
                        return Err(TaskError::ExecutionError(format!(
                            "Failed to move mods folder to trash for deletion (files may be in use): {}",
                            rename_err
                        )));
                    }
                    if assets_path.exists() {
                        if let Ok(content) = tokio::fs::read_to_string(&assets_path).await {
                            original_assets_content = Some(content.clone());
                            if let Ok(mut manifest) = serde_json::from_str::<crate::models::instance::AssetManifest>(&content) {
                                manifest.assets.retain(|_, r| r.category != crate::models::instance::AssetCategory::Mod);
                                if let Ok(serialized) = serde_json::to_string_pretty(&manifest) {
                                    let _ = crate::core::fs_utils::write_file_atomic(&assets_path, serialized.as_bytes()).await;
                                }
                            }
                        }
                    }
                }
                _ => {
                    // "keep": do nothing to files
                }
            }
        }

        let rollback_mods = || async {
            if moved_to_trash && mods_trash.exists() {
                let _ = tokio::fs::rename(&mods_trash, &mods_dir).await;
            }
            if let Some(ref b_dir) = backup_dir_opt {
                if b_dir.exists() && !mods_dir.exists() {
                    let _ = tokio::fs::rename(b_dir, &mods_dir).await;
                }
            }
            for (disabled_path, orig_path) in &disabled_renames {
                if disabled_path.exists() && !orig_path.exists() {
                    let _ = tokio::fs::rename(disabled_path, orig_path).await;
                }
            }
            if let Some(ref orig_content) = original_assets_content {
                let _ = crate::core::fs_utils::write_file_atomic(&assets_path, orig_content.as_bytes()).await;
            } else if !previously_enabled_asset_keys.is_empty() && assets_path.exists() {
                if let Ok(content) = tokio::fs::read_to_string(&assets_path).await {
                    if let Ok(mut manifest) = serde_json::from_str::<crate::models::instance::AssetManifest>(&content) {
                        for key in &previously_enabled_asset_keys {
                            if let Some(record) = manifest.assets.get_mut(key) {
                                record.enabled = true;
                            }
                        }
                        if let Ok(serialized) = serde_json::to_string_pretty(&manifest) {
                            let _ = crate::core::fs_utils::write_file_atomic(&assets_path, serialized.as_bytes()).await;
                        }
                    }
                }
            }
        };

        if ctx.is_cancelled() {
            rollback_mods().await;
            rollback().await;
            return Err(TaskError::ExecutionError("Uninstallation cancelled by user".to_string()));
        }

        // 6. Update dlml.json
        ctx.update_progress(90, 100, "Updating instance configuration...").await;
        let update_res = crate::core::launcher::update_instance_config(&instance_dir, |c| {
            c.loader_type = Some(crate::models::instance::LoaderType::Vanilla);
            c.loader_version = None;
            c.mc_version = Some(mc_version.clone());
            c.set_status(crate::core::launcher::InstanceStatus::Ready);
        }).await;

        if let Err(e) = update_res {
            rollback_mods().await;
            rollback().await;
            return Err(TaskError::ExecutionError(e));
        }

        // Clean up backup and temporary trash files on success
        let _ = tokio::fs::remove_file(&json_bak).await;
        let _ = tokio::fs::remove_file(&config_bak).await;
        if moved_to_trash && mods_trash.exists() {
            let _ = tokio::fs::remove_dir_all(&mods_trash).await;
        }

        ctx.update_progress(100, 100, "Uninstallation complete").await;
        tracing::info!("Successfully uninstalled mod loader from instance '{}'", instance_id);
        Ok(())
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_loader_version_fabric_lib() {
        let json = serde_json::json!({
            "libraries": [
                { "name": "net.fabricmc:sponge-mixin:0.12.5+mixin.0.8.5" },
                { "name": "net.fabricmc:fabric-loader:0.16.9" }
            ]
        });
        assert_eq!(extract_loader_version_from_json(&json, "inst1"), Some("0.16.9".to_string()));
    }

    #[test]
    fn test_extract_loader_version_forge_args() {
        let json = serde_json::json!({
            "arguments": {
                "game": [
                    "--fml.forgeVersion",
                    "47.3.0"
                ]
            }
        });
        assert_eq!(extract_loader_version_from_json(&json, "inst2"), Some("47.3.0".to_string()));
    }

    #[test]
    fn test_extract_loader_version_neoforge_args() {
        let json = serde_json::json!({
            "arguments": {
                "game": [
                    "--fml.neoForgeVersion",
                    "20.4.80-beta"
                ]
            }
        });
        assert_eq!(extract_loader_version_from_json(&json, "inst3"), Some("20.4.80-beta".to_string()));
    }

    #[test]
    fn test_extract_loader_version_inherited_id() {
        let json = serde_json::json!({
            "inheritsFrom": "fabric-loader-0.15.11-1.20.1"
        });
        assert_eq!(extract_loader_version_from_json(&json, "inst4"), Some("0.15.11".to_string()));
    }

    #[test]
    fn test_extract_loader_version_vanilla() {
        let json = serde_json::json!({
            "id": "1.20.1",
            "mainClass": "net.minecraft.client.main.Main"
        });
        assert_eq!(extract_loader_version_from_json(&json, "vanilla-inst"), None);
    }

    #[test]
    fn test_extract_loader_version_forge_26_3() {
        let json = serde_json::json!({
            "inheritsFrom": "forge-26.3-66.1.5"
        });
        assert_eq!(extract_loader_version_from_json(&json, "26.3"), Some("66.1.5".to_string()));
    }

    #[test]
    fn test_is_pure_vanilla_json() {
        let vanilla_json = serde_json::json!({
            "id": "1.20.1",
            "mainClass": "net.minecraft.client.main.Main",
            "downloads": {
                "client": {
                    "sha1": "test",
                    "url": "https://example.com"
                }
            }
        }).to_string();
        assert!(is_pure_vanilla_json(&vanilla_json, "1.20.1"));

        let fabric_json = serde_json::json!({
            "id": "fabric-loader-0.16.0-1.20.1",
            "mainClass": "net.fabricmc.loader.impl.launch.knot.KnotClient",
            "libraries": [
                { "name": "net.fabricmc:fabric-loader:0.16.0" }
            ]
        }).to_string();
        assert!(!is_pure_vanilla_json(&fabric_json, "1.20.1"));

        let forge_json = serde_json::json!({
            "id": "forge-1.20.1-47.3.0",
            "mainClass": "cpw.mods.bootstraplauncher.BootstrapLauncher"
        }).to_string();
        assert!(!is_pure_vanilla_json(&forge_json, "1.20.1"));

        let inherited_fabric = serde_json::json!({
            "id": "26.3",
            "inheritsFrom": "fabric-loader-0.20.1-26.3"
        }).to_string();
        assert!(!is_pure_vanilla_json(&inherited_fabric, "26.3"));
    }

    #[test]
    fn test_instance_loader_info_camel_case_serialization() {
        let info = InstanceLoaderInfo {
            instance_id: "test-instance".to_string(),
            mc_version: "1.20.1".to_string(),
            loader_type: "Fabric".to_string(),
            loader_version: Some("0.16.0".to_string()),
            is_inherited: false,
            has_mods: true,
            mods_count: 5,
        };
        let val = serde_json::to_value(&info).unwrap();
        assert_eq!(val["modsCount"], 5);
        assert_eq!(val["hasMods"], true);
        assert_eq!(val["loaderType"], "Fabric");
        assert_eq!(val["loaderVersion"], "0.16.0");
        assert!(val.get("mods_count").is_none());
    }

    #[tokio::test]
    async fn test_uninstall_disable_mods_rollback() {
        let temp_dir = std::env::temp_dir().join(format!("dawnland-test-rollback-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&temp_dir).await.unwrap();
        let mods_dir = temp_dir.join("mods");
        tokio::fs::create_dir_all(&mods_dir).await.unwrap();

        let mod1_path = mods_dir.join("mod1.jar");
        let mod2_path = mods_dir.join("mod2.jar");
        let already_disabled_path = mods_dir.join("already.jar.disable");

        tokio::fs::write(&mod1_path, "dummy mod 1").await.unwrap();
        tokio::fs::write(&mod2_path, "dummy mod 2").await.unwrap();
        tokio::fs::write(&already_disabled_path, "dummy disabled").await.unwrap();

        let assets_path = temp_dir.join("assets.json");
        let mut manifest = crate::models::instance::AssetManifest::default();
        let mod1_record: crate::models::instance::AssetRecord = serde_json::from_value(serde_json::json!({
            "category": "Mod",
            "sourceType": "LocalImport",
            "enabled": true
        })).unwrap();
        let disabled_record: crate::models::instance::AssetRecord = serde_json::from_value(serde_json::json!({
            "category": "Mod",
            "sourceType": "LocalImport",
            "enabled": false
        })).unwrap();
        manifest.assets.insert("mods/mod1.jar".to_string(), mod1_record);
        manifest.assets.insert("mods/already.jar.disable".to_string(), disabled_record);
        tokio::fs::write(&assets_path, serde_json::to_string_pretty(&manifest).unwrap()).await.unwrap();

        // 1. Simulate disable mode
        let mut disabled_renames: Vec<(std::path::PathBuf, std::path::PathBuf)> = Vec::new();
        let mut original_assets_content: Option<String> = None;
        let mut previously_enabled_asset_keys: Vec<String> = Vec::new();

        if let Ok(mut entries) = tokio::fs::read_dir(&mods_dir).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let p = entry.path();
                if p.is_file() {
                    if let Some(ext) = p.extension().and_then(|e| e.to_str()) {
                        if ext.eq_ignore_ascii_case("jar") {
                            let file_name = p.file_name().unwrap().to_string_lossy();
                            let disabled_name = format!("{}.disable", file_name);
                            let disabled_path = mods_dir.join(disabled_name);
                            if tokio::fs::rename(&p, &disabled_path).await.is_ok() {
                                disabled_renames.push((disabled_path, p));
                            }
                        }
                    }
                }
            }
        }

        if assets_path.exists() {
            if let Ok(content) = tokio::fs::read_to_string(&assets_path).await {
                original_assets_content = Some(content.clone());
                if let Ok(mut m) = serde_json::from_str::<crate::models::instance::AssetManifest>(&content) {
                    for (key, record) in m.assets.iter_mut() {
                        if record.category == crate::models::instance::AssetCategory::Mod && record.enabled {
                            previously_enabled_asset_keys.push(key.clone());
                            record.enabled = false;
                        }
                    }
                    if !previously_enabled_asset_keys.is_empty() {
                        let serialized = serde_json::to_string_pretty(&m).unwrap();
                        tokio::fs::write(&assets_path, serialized).await.unwrap();
                    }
                }
            }
        }

        assert_eq!(disabled_renames.len(), 2);
        assert!(!mod1_path.exists());
        assert!(!mod2_path.exists());
        assert!(mods_dir.join("mod1.jar.disable").exists());
        assert!(mods_dir.join("mod2.jar.disable").exists());
        assert!(already_disabled_path.exists());

        let modified_assets: crate::models::instance::AssetManifest =
            serde_json::from_str(&tokio::fs::read_to_string(&assets_path).await.unwrap()).unwrap();
        assert!(!modified_assets.assets["mods/mod1.jar"].enabled);

        // 2. Simulate rollback_mods
        for (disabled_path, orig_path) in &disabled_renames {
            if disabled_path.exists() && !orig_path.exists() {
                let _ = tokio::fs::rename(disabled_path, orig_path).await;
            }
        }
        if let Some(ref orig_content) = original_assets_content {
            let _ = tokio::fs::write(&assets_path, orig_content).await;
        }

        assert!(mod1_path.exists());
        assert!(mod2_path.exists());
        assert!(!mods_dir.join("mod1.jar.disable").exists());
        assert!(!mods_dir.join("mod2.jar.disable").exists());
        assert!(already_disabled_path.exists());

        let restored_assets: crate::models::instance::AssetManifest =
            serde_json::from_str(&tokio::fs::read_to_string(&assets_path).await.unwrap()).unwrap();
        assert!(restored_assets.assets["mods/mod1.jar"].enabled);
        assert!(!restored_assets.assets["mods/already.jar.disable"].enabled);

        let _ = tokio::fs::remove_dir_all(&temp_dir).await;
    }
}
