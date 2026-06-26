use crate::core::curseforge::get_cf_files_batch;
use crate::core::fabric::{InstallFabricOptions, InstallFabricTask};
use crate::core::forge::{InstallForgeOptions, InstallForgeTask};
use crate::core::modpack::{copy_overrides, extract_zip, parse_modpack_manifest, ModpackType};
use crate::core::mojang::{get_minecraft_base, InstallVanillaTask, VanillaInstallOptions};
use crate::core::task::{ExecutableTask, TaskContext, TaskError, TaskManager, TaskType};
use crate::downloader::{run_batch_download_task, DownloadTask};
use crate::error::{AppError, DawnlandError};
use futures_util::StreamExt;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

// ==========================================
// TASKS
// ==========================================

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct SubTasksInitialized;

#[derive(serde::Serialize, serde::Deserialize)]
struct ModpackResumeContext {
    mc_version: String,
    loader: String,
    tasks: Vec<DownloadTask>,
    overrides_folder: String,
    modpack_version: String,
    modpack_type: String,
}

pub struct InstallModpackOptions {
    pub zip_path: String,
    pub instance_name: String,
    pub is_update: bool,
    pub project_id: Option<String>,
}

pub struct InstallModpackTask {
    pub options: InstallModpackOptions,
}

impl InstallModpackTask {
    pub fn get_sub_tasks() -> Vec<crate::core::task::state::SubTaskState> {
        vec![
            crate::core::task::state::SubTaskState {
                key: "extract_modpack".to_string(),
                name: "解压整合包文件".to_string(),
                status: crate::core::task::state::SubTaskStatus::Pending,
                current: 0,
                total: 100,
                weight: 5,
            },
            crate::core::task::state::SubTaskState {
                key: "resolve_mods".to_string(),
                name: "解析 Mod 下载信息".to_string(),
                status: crate::core::task::state::SubTaskStatus::Pending,
                current: 0,
                total: 100,
                weight: 5,
            },
            crate::core::task::state::SubTaskState {
                key: "download_mods".to_string(),
                name: "下载 Mod".to_string(),
                status: crate::core::task::state::SubTaskStatus::Pending,
                current: 0,
                total: 100,
                weight: 50,
            },
            crate::core::task::state::SubTaskState {
                key: "apply_overrides".to_string(),
                name: "应用 Overrides".to_string(),
                status: crate::core::task::state::SubTaskStatus::Pending,
                current: 0,
                total: 100,
                weight: 5,
            },
        ]
    }
}

#[async_trait::async_trait]
impl ExecutableTask for InstallModpackTask {
    async fn execute(&self, ctx: TaskContext) -> Result<(), TaskError> {
        let zip_path = &self.options.zip_path;
        let instance_name = &self.options.instance_name;
        let is_update = self.options.is_update;
        let project_id = &self.options.project_id;

        tracing::info!(
            "Starting modpack installation: {} -> {}",
            zip_path,
            instance_name
        );

        let base_dir = get_minecraft_base();
        let temp_dir = base_dir
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join(".dawnland")
            .join("temp")
            .join(&ctx.id);
        let instance_dir = base_dir.join("versions").join(instance_name);

        let _ = tokio::fs::create_dir_all(&instance_dir).await;
        if !is_update {
            crate::core::launcher::InstanceConfig::ensure_installing(&instance_dir, false).await;
        }

        macro_rules! check_cancel {
            () => {
                if ctx.is_cancelled() {
                    tracing::warn!("Modpack installation cancelled, cleaning up...");
                    let _ = tokio::fs::remove_dir_all(&temp_dir).await;
                    if !is_update {
                        let _ = tokio::fs::remove_dir_all(&instance_dir).await;
                    }
                    return Err(TaskError::ExecutionError(
                        "Installation cancelled by user".to_string(),
                    ));
                }
            };
        }

        if ctx
            .get_context_data::<SubTasksInitialized>()
            .await
            .is_none()
        {
            ctx.init_sub_tasks(Self::get_sub_tasks()).await;
            ctx.set_context_data(&SubTasksInitialized).await;
        }
        let (mc_version, loader, tasks, overrides_folder, modpack_version, modpack_type_str) =
            if let Some(context) = ctx.get_context_data::<ModpackResumeContext>().await {
                tracing::info!(
                    "Found resume context for task {}, skipping extraction and resolution",
                    ctx.id
                );
                (
                    context.mc_version,
                    context.loader,
                    context.tasks,
                    context.overrides_folder,
                    context.modpack_version,
                    context.modpack_type,
                )
            } else {
                let ctx_extract = ctx.with_sub_task("extract_modpack");
                // 1. Emit phase 1: Extracting
                ctx_extract
                    .update_progress(0, 100, "Extracting modpack archive...")
                    .await;

                let zip = PathBuf::from(zip_path);
                let temp = temp_dir.clone();
                extract_zip(zip, temp)
                    .await
                    .map_err(|e| TaskError::ExecutionError(e.to_string()))?;

                ctx_extract
                    .update_progress(100, 100, "Extract complete")
                    .await;
                check_cancel!();

                // 2. Parse Manifest
                let ctx_resolve = ctx.with_sub_task("resolve_mods");
                ctx_resolve
                    .update_progress(0, 100, "Reading modpack manifest...")
                    .await;

                let modpack = parse_modpack_manifest(&temp_dir)
                    .await
                    .map_err(|e| TaskError::ExecutionError(e.to_string()))?;

                let modpack_type_str = match &modpack {
                    ModpackType::CurseForge(_) => "CurseForge",
                    ModpackType::Modrinth(_) => "Modrinth",
                };

                let (mc_version, loader, tasks, overrides_folder, modpack_version) = match modpack {
                    ModpackType::CurseForge(manifest) => {
                        ctx_resolve
                            .update_progress(0, 100, "Resolving CurseForge download links...")
                            .await;

                        let mc_version = manifest.minecraft.version.clone();
                        let loader = manifest
                            .minecraft
                            .mod_loaders
                            .first()
                            .map(|l| l.id.clone())
                            .unwrap_or_default();
                        let mp_version = manifest.version.clone();

                        let file_ids: Vec<u32> = manifest.files.iter().map(|f| f.file_id).collect();

                        // Get URLs from Proxy
                        let resolved_files = get_cf_files_batch(file_ids)
                            .await
                            .map_err(TaskError::ExecutionError)?;

                        check_cancel!();

                        let mut tasks = Vec::new();
                        for file in resolved_files {
                            let mut dest = instance_dir.join("mods").join(&file.filename);
                            let disabled_dest = instance_dir
                                .join("mods")
                                .join(format!("{}.disable", file.filename));
                            if disabled_dest.exists() {
                                dest = disabled_dest;
                            }

                            tasks.push(DownloadTask::new(
                                file.download_url,
                                dest.to_string_lossy().to_string(),
                                file.hash,
                                file.file_size,
                            ));
                        }

                        (mc_version, loader, tasks, manifest.overrides, mp_version)
                    }
                    ModpackType::Modrinth(manifest) => {
                        let mc_version = manifest.dependencies.minecraft.clone();
                        let mut loader = String::new();
                        if let Some(forge) = manifest.dependencies.forge {
                            loader = format!("forge-{}", forge);
                        } else if let Some(fabric) = manifest.dependencies.fabric_loader {
                            loader = format!("fabric-{}", fabric);
                        } else if let Some(neoforge) = manifest.dependencies.neoforge {
                            loader = format!("neoforge-{}", neoforge);
                        }
                        let mp_version = manifest.version_id.clone();

                        let mut tasks = Vec::new();
                        for file in manifest.files {
                            if let Some(url) = file.downloads.first() {
                                let mut dest = instance_dir.join(&file.path);
                                if let Some(filename) = dest.file_name().and_then(|n| n.to_str()) {
                                    let disabled_dest =
                                        dest.with_file_name(format!("{}.disable", filename));
                                    if disabled_dest.exists() {
                                        dest = disabled_dest;
                                    }
                                }

                                let hash = file.hashes.get("sha1").cloned();
                                tasks.push(DownloadTask::new(
                                    url.clone(),
                                    dest.to_string_lossy().to_string(),
                                    hash,
                                    Some(file.file_size),
                                ));
                            }
                        }

                        (
                            mc_version,
                            loader,
                            tasks,
                            "overrides".to_string(),
                            mp_version,
                        )
                    }
                }; // End of match modpack

                // Save resume context
                let context = ModpackResumeContext {
                    mc_version: mc_version.clone(),
                    loader: loader.clone(),
                    tasks: tasks.clone(),
                    overrides_folder: overrides_folder.clone(),
                    modpack_version: modpack_version.clone(),
                    modpack_type: modpack_type_str.to_string(),
                };
                ctx.set_context_data(&context).await;

                ctx_resolve
                    .update_progress(100, 100, "Manifest resolved")
                    .await;
                (
                    mc_version,
                    loader,
                    tasks,
                    overrides_folder,
                    modpack_version,
                    modpack_type_str.to_string(),
                )
            };

        // 3. Setup Instance
        let ctx_vanilla_forge = ctx.clone();
        let mc_version_clone = mc_version.clone();
        let loader_clone = loader.clone();

        let actual_loader_task = tokio::spawn(async move {
            ensure_dependencies(&mc_version_clone, &loader_clone, ctx_vanilla_forge).await
        });

        check_cancel!();

        tokio::fs::create_dir_all(&instance_dir)
            .await
            .map_err(|e| TaskError::ExecutionError(e.to_string()))?;

        // Smart Cleanup if is_update is true
        let modpack_files_path = instance_dir.join("modpack_files.json");

        let mut expected_mod_filenames = std::collections::HashSet::new();
        for task in &tasks {
            if let Some(filename) = std::path::Path::new(&task.dest_path).file_name() {
                let name = filename.to_string_lossy().to_string();
                let base_name = name.trim_end_matches(".disable").to_string();
                expected_mod_filenames.insert(base_name);
            }
        }

        if is_update
            && modpack_files_path.exists() {
                if let Ok(content) = tokio::fs::read_to_string(&modpack_files_path).await {
                    if let Ok(old_files) = serde_json::from_str::<Vec<String>>(&content) {
                        for old_file in old_files {
                            let file_path = instance_dir.join(&old_file);
                            if let Some(filename) = file_path.file_name() {
                                let name_str = filename.to_string_lossy().to_string();
                                let base_name = name_str.trim_end_matches(".disable").to_string();

                                if !expected_mod_filenames.contains(&base_name) {
                                    tracing::info!("Removing old modpack file: {}", old_file);
                                    let _ = tokio::fs::remove_file(&file_path).await;

                                    // Also try removing variants
                                    let disabled_path =
                                        file_path.with_file_name(format!("{}.disable", base_name));
                                    let _ = tokio::fs::remove_file(&disabled_path).await;

                                    let enabled_path = file_path.with_file_name(&base_name);
                                    let _ = tokio::fs::remove_file(&enabled_path).await;
                                }
                            }
                        }
                    }
                }
            }

        // Save list of expected mod files for future updates
        let mut new_modpack_files = Vec::new();
        for task in &tasks {
            if let Ok(rel_path) = std::path::Path::new(&task.dest_path).strip_prefix(&instance_dir)
            {
                let rel_str = rel_path.to_string_lossy().to_string().replace("\\\\", "/");
                let base_rel_str = rel_str.trim_end_matches(".disable").to_string();
                new_modpack_files.push(base_rel_str);
            }
        }
        let _ = tokio::fs::write(
            &modpack_files_path,
            serde_json::to_string_pretty(&new_modpack_files)
                .map_err(|e| TaskError::ExecutionError(e.to_string()))?,
        )
        .await;

        // 4. Batch Download Mods
        check_cancel!();

        let ctx_download_mods = ctx.with_sub_task("download_mods");

        let download_task = tokio::spawn(async move {
            if !tasks.is_empty() {
                if let Err(e) = run_batch_download_task(tasks, ctx_download_mods.clone()).await {
                    if ctx_download_mods.is_cancelled() {
                        tracing::warn!(
                            "Modpack installation cancelled during batch download, cleaning up..."
                        );
                        return Err(TaskError::ExecutionError(
                            "Installation cancelled by user".to_string(),
                        ));
                    }
                    tracing::warn!(
                        "Installation failed during batch download. Temp dir preserved for resume."
                    );
                    return Err(TaskError::ExecutionError(e));
                }
            }
            Ok(())
        });

        // Wait for both loader installation and mods download to complete
        let (actual_loader_res, download_res) = tokio::join!(actual_loader_task, download_task);

        check_cancel!();

        let actual_loader =
            actual_loader_res.map_err(|e| TaskError::ExecutionError(e.to_string()))??;
        download_res.map_err(|e| TaskError::ExecutionError(e.to_string()))??;

        // Setup Instance JSON based on the resolved loader
        let inherits_from = actual_loader;

        let mut version_json_map = serde_json::Map::new();
        version_json_map.insert("id".to_string(), serde_json::json!(instance_name));
        version_json_map.insert("type".to_string(), serde_json::json!("release"));
        version_json_map.insert(
            "modpackVersion".to_string(),
            serde_json::json!(modpack_version),
        );
        version_json_map.insert(
            "modpackType".to_string(),
            serde_json::json!(modpack_type_str),
        );
        version_json_map.insert(
            "modpackProjectId".to_string(),
            serde_json::json!(project_id),
        );

        let settings = crate::core::settings::get_launcher_settings_sync();
        if !settings.enable_instance_inheritance {
            crate::core::utils::flatten_instance_json_recursive(
                &inherits_from,
                &mut version_json_map,
            )
            .await
            .map_err(TaskError::ExecutionError)?;
            version_json_map.insert("clientVersion".to_string(), serde_json::json!(mc_version));
            // Copy the vanilla jar to isolated sandbox
            let dawnland_cache = crate::core::mojang::get_dawnland_cache();
            let source_jar = dawnland_cache
                .join(&mc_version)
                .join(format!("{}.jar", mc_version));
            if source_jar.exists() {
                let target_jar = instance_dir.join(format!("{}.jar", instance_name));
                let _ = tokio::fs::copy(&source_jar, &target_jar).await;
            }
        } else {
            version_json_map.insert("inheritsFrom".to_string(), serde_json::json!(inherits_from));

            // Copy from cache to versions
            let dawnland_cache = crate::core::mojang::get_dawnland_cache();
            let versions_dir = base_dir.join("versions");

            // Vanilla
            let vanilla_src = dawnland_cache.join(&mc_version);
            let vanilla_dest = versions_dir.join(&mc_version);
            if vanilla_src.exists() && !vanilla_dest.exists() {
                let _ = crate::core::utils::copy_dir_all(&vanilla_src, &vanilla_dest).await;
            }

            // Loader
            if inherits_from != mc_version {
                let loader_src = dawnland_cache.join(&inherits_from);
                let loader_dest = versions_dir.join(&inherits_from);
                if loader_src.exists() && !loader_dest.exists() {
                    let _ = crate::core::utils::copy_dir_all(&loader_src, &loader_dest).await;
                }
            }
        }

        let version_json = serde_json::Value::Object(version_json_map);

        std::fs::write(
            instance_dir.join(format!("{}.json", instance_name)),
            serde_json::to_string_pretty(&version_json).unwrap(),
        )
        .map_err(|e| TaskError::ExecutionError(e.to_string()))?;

        // 5. Apply Overrides
        let ctx_overrides = ctx.with_sub_task("apply_overrides");
        ctx_overrides
            .update_progress(0, 100, "Applying overrides...")
            .await;

        let overrides_dir = temp_dir.join(&overrides_folder);
        if overrides_dir.exists() {
            let src = temp_dir.clone();
            let dst = instance_dir.clone();
            let folder = overrides_folder.clone();
            copy_overrides(&src, &dst, &folder)
                .await
                .map_err(|e| TaskError::ExecutionError(e.to_string()))?;
        }
        ctx_overrides
            .update_progress(100, 100, "Overrides applied")
            .await;

        // 6. Cleanup Temp
        let _ = tokio::fs::remove_dir_all(&temp_dir).await;

        let config_path = instance_dir.join("dlml.json");
        if config_path.exists() {
            if let Ok(content) = tokio::fs::read_to_string(&config_path).await {
                if let Ok(mut config) =
                    serde_json::from_str::<crate::core::launcher::InstanceConfig>(&content)
                {
                    config.is_installing = false;
                    config.is_updating = false;
                    let _ = tokio::fs::write(
                        &config_path,
                        serde_json::to_string_pretty(&config).unwrap(),
                    )
                    .await;
                }
            }
        }

        Ok(())
    }
}

pub struct InstallOnlineModpackOptions {
    pub url: String,
    pub instance_name: String,
    pub project_id: Option<String>,
    pub is_update: bool,
}

pub struct InstallOnlineModpackTask {
    pub options: InstallOnlineModpackOptions,
}

#[async_trait::async_trait]
impl ExecutableTask for InstallOnlineModpackTask {
    async fn execute(&self, ctx: TaskContext) -> Result<(), TaskError> {
        let url = &self.options.url;
        let instance_name = &self.options.instance_name;
        let project_id = &self.options.project_id;
        let is_update = self.options.is_update;

        tracing::info!(
            "Downloading online modpack from {} to {}",
            url,
            instance_name
        );

        let base_dir = crate::core::mojang::get_minecraft_base();
        let instance_dir = base_dir.join("versions").join(instance_name);

        let _ = tokio::fs::create_dir_all(&instance_dir).await;
        if !is_update {
            crate::core::launcher::InstanceConfig::ensure_installing(&instance_dir, false).await;
        }

        let temp_dir = base_dir
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join(".dawnland")
            .join("temp");
        let _ = tokio::fs::create_dir_all(&temp_dir).await;
        let mut sub_tasks = vec![crate::core::task::state::SubTaskState {
            key: "download_modpack_zip".to_string(),
            name: "下载整合包文件".to_string(),
            status: crate::core::task::state::SubTaskStatus::Pending,
            current: 0,
            total: 100,
            weight: 30,
        }];
        sub_tasks.extend(InstallModpackTask::get_sub_tasks());

        ctx.init_sub_tasks(sub_tasks).await;
        ctx.set_context_data(&SubTasksInitialized).await;

        let temp_zip_path = temp_dir.join(format!("{}.zip", ctx.id));

        let ctx_zip = ctx.with_sub_task("download_modpack_zip");
        ctx_zip
            .update_progress(0, 100, "Downloading modpack archive...")
            .await;

        // Get initial headers to know total_size for skip and progress bar
        let client = crate::core::utils::get_http_client().clone();
        let response = client.get(url).send().await.map_err(|e| {
            TaskError::ExecutionError(format!("Failed to start download: {}", e))
        })?;

        if !response.status().is_success() {
            return Err(TaskError::ExecutionError(format!(
                "Download failed: {}",
                response.status()
            )));
        }

        let total_size = response.content_length().unwrap_or(0);
        drop(response); // we just wanted the headers

        let mut skip_download = false;
        if total_size > 0 && temp_zip_path.exists() {
            if let Ok(metadata) = tokio::fs::metadata(&temp_zip_path).await {
                if metadata.len() == total_size {
                    tracing::info!(
                        "Found existing online modpack zip for task {}, skipping download",
                        ctx.id
                    );
                    skip_download = true;
                }
            }
        }

        if !skip_download {
            let mut task = crate::downloader::DownloadTask::new(
                url.clone(),
                temp_zip_path.to_string_lossy().to_string(),
                None,
                None,
            );
            task.expected_size = if total_size > 0 { Some(total_size) } else { None };

            let global_downloaded = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
            let monitor_ctx = ctx_zip.clone();
            let monitor_dl = global_downloaded.clone();
            
            let monitor = tokio::spawn(async move {
                let mut last_dl = 0;
                let mut last_time = tokio::time::Instant::now();
                loop {
                    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
                    if monitor_ctx.is_cancelled() { break; }
                    let dl = monitor_dl.load(std::sync::atomic::Ordering::Relaxed);
                    
                    let elapsed_sec = last_time.elapsed().as_secs_f64();
                    let speed = if elapsed_sec > 0.0 { ((dl.saturating_sub(last_dl)) as f64 / elapsed_sec) as u64 } else { 0 };
                    
                    let mb = dl as f64 / 1024.0 / 1024.0;
                    let t_mb = total_size as f64 / 1024.0 / 1024.0;
                    monitor_ctx.update_progress(dl, total_size, &format!("Downloading archive... {:.1} MB / {:.1} MB", mb, t_mb)).await;
                    monitor_ctx.update_download_metrics(speed, 1).await;
                    
                    last_dl = dl;
                    last_time = tokio::time::Instant::now();
                    
                    if dl >= total_size && total_size > 0 { break; }
                }
            });

            if let Err(e) = crate::downloader::download::download_file_task(task, client, &ctx_zip, &global_downloaded).await {
                monitor.abort();
                let _ = tokio::fs::remove_file(&temp_zip_path).await;
                if !is_update {
                    let _ = tokio::fs::remove_dir_all(&instance_dir).await;
                }
                return Err(TaskError::ExecutionError(e));
            }
            monitor.abort();
        }

        ctx_zip
            .update_progress(100, 100, "Download complete. Starting installation...")
            .await;

        // Instantiate internal modpack task
        let modpack_task = InstallModpackTask {
            options: InstallModpackOptions {
                zip_path: temp_zip_path.to_string_lossy().to_string(),
                instance_name: instance_name.clone(),
                is_update,
                project_id: project_id.clone(),
            },
        };

        let result = modpack_task.execute(ctx).await;

        if result.is_ok() {
            // Clean up the downloaded modpack zip archive to save space
            let _ = tokio::fs::remove_file(&temp_zip_path).await;
        }
        // Note: we intentionally do NOT delete the zip file on failure. If the task
        // fails during installation, retaining the fully downloaded zip allows
        // subsequent retries to skip the download phase and resume instantly.

        result
    }
}

// ==========================================
// HELPERS
// ==========================================

async fn ensure_dependencies(
    mc_version: &str,
    loader: &str,
    ctx: TaskContext,
) -> Result<String, TaskError> {
    let base_dir = crate::core::mojang::get_minecraft_base();

    // Check if loader is empty -> means only vanilla
    if loader.is_empty() {
        ctx.manager
            .wait_for_instance(mc_version, &ctx.cancel_token)
            .await;

        let dawnland_cache = crate::core::mojang::get_dawnland_cache();
        let vanilla_json = dawnland_cache
            .join(mc_version)
            .join(format!("{}.json", mc_version));
        if !vanilla_json.exists() {
            ctx.update_progress(0, 0, &format!("Installing Minecraft {}...", mc_version))
                .await;

            let versions = crate::core::mojang::get_vanilla_versions()
                .await
                .map_err(TaskError::ExecutionError)?;
            let version_info = versions
                .into_iter()
                .find(|v| v.id == mc_version)
                .ok_or_else(|| {
                    TaskError::ExecutionError(format!(
                        "Minecraft version {} not found in Mojang API",
                        mc_version
                    ))
                })?;

            let vanilla_task = InstallVanillaTask {
                options: VanillaInstallOptions {
                    version_id: mc_version.to_string(),
                    version_json_url: version_info.url.clone(),
                    custom_instance_name: None,
                    is_dependency: Some(true),
                },
            };
            ctx.append_sub_tasks(crate::core::mojang::InstallVanillaTask::get_sub_tasks())
                .await;
            vanilla_task.execute(ctx.clone()).await?;
        } else {
            ctx.append_sub_tasks(crate::core::mojang::InstallVanillaTask::get_sub_tasks())
                .await;
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

        return Ok(mc_version.to_string());
    }

    let base_loader = loader
        .strip_suffix(&format!("-{}", mc_version))
        .unwrap_or(loader);
    let custom_instance_name = format!("{}-{}", base_loader, mc_version);

    // Loader is present
    ctx.manager
        .wait_for_instance(&custom_instance_name, &ctx.cancel_token)
        .await;

    let dawnland_cache = crate::core::mojang::get_dawnland_cache();
    let loader_json = dawnland_cache
        .join(&custom_instance_name)
        .join(format!("{}.json", custom_instance_name));
    if !loader_json.exists() {
        ctx.update_progress(
            0,
            0,
            &format!("Installing dependency {}...", custom_instance_name),
        )
        .await;

        if loader.starts_with("fabric-") {
            ctx.append_sub_tasks(crate::core::fabric::InstallFabricTask::get_sub_tasks())
                .await;
            let loader_version = loader.strip_prefix("fabric-").unwrap().to_string();
            let fabric_task = InstallFabricTask {
                options: InstallFabricOptions {
                    mc_version: mc_version.to_string(),
                    fabric_version: loader_version,
                    custom_instance_name: custom_instance_name.clone(),
                    is_dependency: Some(true),
                },
            };
            fabric_task.execute(ctx.clone()).await?;
        } else if loader.starts_with("forge-") {
            ctx.append_sub_tasks(crate::core::forge::InstallForgeTask::get_sub_tasks())
                .await;
            let loader_version = loader.strip_prefix("forge-").unwrap().to_string();
            let forge_task = InstallForgeTask {
                options: InstallForgeOptions {
                    mc_version: mc_version.to_string(),
                    loader_version,
                    loader_type: "forge".to_string(),
                    custom_instance_name: custom_instance_name.clone(),
                    is_dependency: Some(true),
                },
            };
            forge_task.execute(ctx.clone()).await?;
        } else if loader.starts_with("neoforge-") {
            ctx.append_sub_tasks(crate::core::forge::InstallForgeTask::get_sub_tasks())
                .await;
            let loader_version = loader.strip_prefix("neoforge-").unwrap().to_string();
            let forge_task = InstallForgeTask {
                options: InstallForgeOptions {
                    mc_version: mc_version.to_string(),
                    loader_version,
                    loader_type: "neoforge".to_string(),
                    custom_instance_name: custom_instance_name.clone(),
                    is_dependency: Some(true),
                },
            };
            forge_task.execute(ctx.clone()).await?;
        } else {
            return Err(TaskError::ExecutionError(format!(
                "Unsupported loader type: {}",
                loader
            )));
        }
    } else {
        if loader.starts_with("fabric-") {
            ctx.append_sub_tasks(crate::core::fabric::InstallFabricTask::get_sub_tasks())
                .await;
        } else if loader.starts_with("forge-") || loader.starts_with("neoforge-") {
            ctx.append_sub_tasks(crate::core::forge::InstallForgeTask::get_sub_tasks())
                .await;
        }

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

        ctx.with_sub_task("resolve_loader")
            .update_progress(100, 100, "Skipped (Already exists)")
            .await;
        ctx.with_sub_task("download_loader_libs")
            .update_progress(100, 100, "Skipped (Already exists)")
            .await;

        if loader.starts_with("forge-") || loader.starts_with("neoforge-") {
            ctx.with_sub_task("install_loader")
                .update_progress(100, 100, "Skipped (Already exists)")
                .await;
        }
    }

    Ok(custom_instance_name)
}

// ==========================================
// TAURI COMMANDS
// ==========================================

#[tauri::command]
pub async fn install_modpack(
    zip_path: String,
    instance_name: String,
    is_update: bool,
    project_id: Option<String>,
    app: AppHandle,
) -> Result<String, AppError> {
    let task_manager = app.state::<TaskManager>().inner().clone();

    // Pre-create instance directory and dlml.json synchronously so frontend can detect it immediately
    let base_dir = crate::core::mojang::get_minecraft_base();
    let instance_dir = base_dir.join("versions").join(&instance_name);
    let _ = tokio::fs::create_dir_all(&instance_dir).await;
    let config_path = instance_dir.join("dlml.json");
    if !is_update {
        let pre_config = crate::core::launcher::InstanceConfig {
            is_installing: true,
            ..Default::default()
        };
        let _ = tokio::fs::write(&config_path, serde_json::to_string_pretty(&pre_config)?).await;
    } else {
        if let Ok(content) = tokio::fs::read_to_string(&config_path).await {
            if let Ok(mut config) = serde_json::from_str::<crate::core::launcher::InstanceConfig>(&content) {
                config.is_updating = true;
                let _ = tokio::fs::write(&config_path, serde_json::to_string_pretty(&config)?).await;
            }
        }
    }

    let task = InstallModpackTask {
        options: InstallModpackOptions {
            zip_path: zip_path.clone(),
            instance_name: instance_name.clone(),
            is_update,
            project_id: project_id.clone(),
        },
    };

    let task_id = task_manager
        .spawn_task(
            TaskType::InstallModpack {
                zip_path: zip_path.clone(),
                instance_name: instance_name.clone(),
                is_update,
                project_id,
            },
            task,
        )
        .await
        .map_err(|e| DawnlandError::Unknown(e.to_string()))?;

    Ok(task_id)
}

#[tauri::command]
pub async fn download_and_install_online_modpack(
    url: String,
    instance_name: String,
    project_id: Option<String>,
    is_update: bool,
    app: AppHandle,
) -> Result<String, AppError> {
    let task_manager = app.state::<TaskManager>().inner().clone();

    // Pre-create instance directory and dlml.json synchronously so frontend can detect it immediately
    let base_dir = crate::core::mojang::get_minecraft_base();
    let instance_dir = base_dir.join("versions").join(&instance_name);
    let _ = tokio::fs::create_dir_all(&instance_dir).await;
    let config_path = instance_dir.join("dlml.json");
    if !is_update {
        let pre_config = crate::core::launcher::InstanceConfig {
            is_installing: true,
            ..Default::default()
        };
        let _ = tokio::fs::write(&config_path, serde_json::to_string_pretty(&pre_config)?).await;
    } else {
        if let Ok(content) = tokio::fs::read_to_string(&config_path).await {
            if let Ok(mut config) = serde_json::from_str::<crate::core::launcher::InstanceConfig>(&content) {
                config.is_updating = true;
                let _ = tokio::fs::write(&config_path, serde_json::to_string_pretty(&config)?).await;
            }
        }
    }

    let task = InstallOnlineModpackTask {
        options: InstallOnlineModpackOptions {
            url: url.clone(),
            instance_name: instance_name.clone(),
            project_id: project_id.clone(),
            is_update,
        },
    };

    let task_id = task_manager
        .spawn_task(
            TaskType::InstallOnlineModpack {
                url: url.clone(),
                instance_name: instance_name.clone(),
                is_update,
                project_id: project_id.clone(),
            },
            task,
        )
        .await
        .map_err(|e| DawnlandError::Unknown(e.to_string()))?;

    Ok(task_id)
}

#[tauri::command]
pub async fn get_modpack_name(zip_path: String) -> Result<String, AppError> {
    let name = tokio::task::spawn_blocking(move || -> Result<String, DawnlandError> {
        let file = std::fs::File::open(&zip_path)
            .map_err(|e| DawnlandError::Unknown(format!("Failed to open zip: {}", e)))?;
        let mut archive = zip::ZipArchive::new(file)
            .map_err(|e| DawnlandError::Unknown(format!("Failed to read zip: {}", e)))?;

        if let Ok(mut manifest_file) = archive.by_name("manifest.json") {
            let mut contents = String::new();
            use std::io::Read;
            if manifest_file.read_to_string(&mut contents).is_ok() {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&contents) {
                    if let Some(name) = json.get("name").and_then(|n| n.as_str()) {
                        return Ok(name.to_string());
                    }
                }
            }
        }

        if let Ok(mut manifest_file) = archive.by_name("modrinth.index.json") {
            let mut contents = String::new();
            use std::io::Read;
            if manifest_file.read_to_string(&mut contents).is_ok() {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&contents) {
                    if let Some(name) = json.get("name").and_then(|n| n.as_str()) {
                        return Ok(name.to_string());
                    }
                }
            }
        }

        Err(DawnlandError::Unknown(
            "Could not find manifest.json or modrinth.index.json with a valid name".to_string(),
        ))
    })
    .await
    .map_err(|e| DawnlandError::ProcessError(format!("Task join error: {}", e)))??;

    Ok(name)
}
