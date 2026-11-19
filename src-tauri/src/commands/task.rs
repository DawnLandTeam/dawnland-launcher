use crate::core::task::{TaskError, TaskManager, TaskState};
use crate::error::{AppError, DawnlandError};
use tauri::State;

#[tauri::command]
pub async fn get_task_history(
    task_manager: State<'_, TaskManager>,
) -> Result<Vec<TaskState>, AppError> {
    task_manager
        .load_history()
        .await
        .map_err(|e| DawnlandError::Unknown(e.to_string()).into())
}

#[tauri::command]
pub async fn cancel_task(
    task_id: String,
    task_manager: State<'_, TaskManager>,
) -> Result<(), AppError> {
    task_manager
        .cancel_task(&task_id)
        .await
        .map_err(|e| DawnlandError::Unknown(e.to_string()).into())
}

#[tauri::command]
pub async fn force_cancel_task(
    task_id: String,
    task_manager: State<'_, TaskManager>,
) -> Result<(), AppError> {
    task_manager
        .force_cancel_task(&task_id)
        .await
        .map_err(|e| DawnlandError::Unknown(e.to_string()).into())
}

#[tauri::command]
pub async fn clear_task_history(task_manager: State<'_, TaskManager>) -> Result<(), AppError> {
    task_manager
        .clear_history()
        .await
        .map_err(|e| DawnlandError::Unknown(e.to_string()).into())
}

struct RetryInstancePrecreate {
    dir: std::path::PathBuf,
    dir_existed: bool,
    config_existed: bool,
    is_update: bool,
    _startup_guard: crate::core::task::manager::InstanceStartupGuard,
}

async fn prepare_retry_instance_dir(
    task_manager: &TaskManager,
    target_name: &str,
    is_dependency: bool,
    is_update: bool,
) -> Result<RetryInstancePrecreate, AppError> {
    if !crate::core::fs_utils::is_safe_single_name(target_name) {
        return Err(DawnlandError::Unknown("Invalid instance name".to_string()).into());
    }
    let startup_guard = task_manager.start_instance_install(target_name);
    let dir = if is_dependency {
        crate::core::mojang::get_dawnland_cache().join(target_name)
    } else {
        crate::core::mojang::get_minecraft_base()
            .join("versions")
            .join(target_name)
    };
    let dir_existed = dir.exists();
    let config_existed = dir.join("dlml.json").exists();
    let _ = tokio::fs::create_dir_all(&dir).await;
    if is_update {
        let _ = crate::core::launcher::InstanceConfig::ensure_updating(&dir).await;
    } else {
        crate::core::launcher::InstanceConfig::ensure_installing(&dir, is_dependency)
            .await
            .map_err(DawnlandError::Unknown)?;
    }
    Ok(RetryInstancePrecreate {
        dir,
        dir_existed,
        config_existed,
        is_update,
        _startup_guard: startup_guard,
    })
}

async fn spawn_retry_with_rollback<T: crate::core::task::ExecutableTask + 'static>(
    task_manager: &TaskManager,
    task_id: String,
    task_type: crate::core::task::TaskType,
    executable: T,
    prep: RetryInstancePrecreate,
) -> Result<String, AppError> {
    match task_manager
        .spawn_task_with_id(task_id, task_type, executable)
        .await
    {
        Ok(id) => Ok(id),
        Err(e) => {
            crate::core::launcher::rollback_precreated_instance_status(
                &prep.dir,
                prep.dir_existed,
                prep.config_existed,
                prep.is_update,
            )
            .await;
            Err(DawnlandError::Unknown(e.to_string()).into())
        }
    }
}

#[tauri::command]
pub async fn retry_task(
    task_id: String,
    task_manager: State<'_, TaskManager>,
    app: tauri::AppHandle,
) -> Result<String, AppError> {
    let task = task_manager
        .get_task(&task_id)
        .await
        .map_err(|e| DawnlandError::Unknown(e.to_string()))?
        .ok_or_else(|| DawnlandError::Unknown("Task not found".to_string()))?;

    // Block retrying if the task is currently active (e.g. Cancelling or Running)
    if task.status.is_active() {
        return Err(DawnlandError::Unknown(format!(
            "Cannot retry task while it is still active (status: {:?})",
            task.status
        )).into());
    }

    use crate::core::task::TaskType;

    let _new_task_id = match task.task_type.clone() {
        TaskType::InstallVanilla {
            version_id,
            version_json_url,
            custom_instance_name,
            is_dependency,
        } => {
            let is_dep = is_dependency.unwrap_or(false);
            let target_name = if is_dep {
                version_id.as_str()
            } else {
                custom_instance_name
                    .as_deref()
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or(&version_id)
            };
            let prep = prepare_retry_instance_dir(&task_manager, target_name, is_dep, false).await?;

            let executable = crate::core::mojang::InstallVanillaTask {
                options: crate::core::mojang::VanillaInstallOptions {
                    version_id: version_id.clone(),
                    version_json_url: version_json_url.clone(),
                    custom_instance_name: custom_instance_name.clone(),
                    is_dependency,
                },
            };
            spawn_retry_with_rollback(
                &task_manager,
                task_id.clone(),
                task.task_type,
                executable,
                prep,
            )
            .await?
        }
        TaskType::InstallForge {
            mc_version,
            loader_version,
            loader_type,
            custom_instance_name,
            is_dependency,
        } => {
            let is_dep = is_dependency.unwrap_or(false);
            let prep =
                prepare_retry_instance_dir(&task_manager, &custom_instance_name, is_dep, false)
                    .await?;

            let executable = crate::core::forge::InstallForgeTask {
                options: crate::core::forge::InstallForgeOptions {
                    mc_version,
                    loader_version,
                    loader_type,
                    custom_instance_name,
                    is_dependency,
                    is_existing_instance: None,
                },
            };
            spawn_retry_with_rollback(
                &task_manager,
                task_id.clone(),
                task.task_type,
                executable,
                prep,
            )
            .await?
        }
        TaskType::InstallFabric {
            mc_version,
            fabric_version,
            custom_instance_name,
            is_dependency,
        } => {
            let is_dep = is_dependency.unwrap_or(false);
            let prep =
                prepare_retry_instance_dir(&task_manager, &custom_instance_name, is_dep, false)
                    .await?;

            let executable = crate::core::fabric::InstallFabricTask {
                options: crate::core::fabric::InstallFabricOptions {
                    mc_version,
                    fabric_version,
                    custom_instance_name,
                    is_dependency,
                    is_existing_instance: None,
                },
            };
            spawn_retry_with_rollback(
                &task_manager,
                task_id.clone(),
                task.task_type,
                executable,
                prep,
            )
            .await?
        }
        TaskType::InstallModpack {
            zip_path,
            instance_name,
            is_update,
            project_id,
            server_id,
            pack_version_id,
            pack_file_name,
        } => {
            let prep =
                prepare_retry_instance_dir(&task_manager, &instance_name, false, is_update)
                    .await?;

            let executable = crate::commands::modpack::InstallModpackTask {
                options: crate::commands::modpack::InstallModpackOptions {
                    zip_path,
                    instance_name,
                    is_update,
                    project_id,
                    server_id,
                    pack_version_id,
                    pack_file_name,
                },
            };
            spawn_retry_with_rollback(
                &task_manager,
                task_id.clone(),
                task.task_type,
                executable,
                prep,
            )
            .await?
        }
        TaskType::InstallMod {
            source,
            project_id,
            mod_name,
            instance_id,
            target_dir,
            download_url,
            file_id,
            dependencies,
            keep_both,
            slug,
            original_filename,
            version_number,
        } => {
            let executable = crate::core::manager::InstallModTask {
                options: crate::core::manager::InstallModOptions {
                    source,
                    project_id,
                    mod_name,
                    instance_id,
                    target_dir,
                    download_url,
                    file_id,
                    dependencies,
                    keep_both,
                    slug,
                    original_filename,
                    version_number,
                },
            };
            task_manager
                .spawn_task_with_id(task_id.clone(), task.task_type, executable)
                .await
                .map_err(|e| e.to_string())?
        }
        TaskType::InstallResourcepack {
            source,
            project_id,
            pack_name,
            instance_id,
            target_dir,
            download_url,
            file_id,
            slug,
            original_filename,
            version_number,
        } => {
            let executable = crate::core::manager::InstallResourcepackTask {
                options: crate::core::manager::InstallResourcepackOptions {
                    source,
                    project_id,
                    pack_name,
                    instance_id,
                    target_dir,
                    download_url,
                    file_id,
                    slug,
                    original_filename,
                    version_number,
                },
            };
            task_manager
                .spawn_task_with_id(task_id.clone(), task.task_type, executable)
                .await
                .map_err(|e| e.to_string())?
        }
        TaskType::InstallOnlineModpack {
            url,
            instance_name,
            is_update,
            project_id,
            server_id,
            pack_version_id,
            pack_file_name,
        } => {
            let prep =
                prepare_retry_instance_dir(&task_manager, &instance_name, false, is_update)
                    .await?;

            let executable = crate::commands::modpack::InstallOnlineModpackTask {
                options: crate::commands::modpack::InstallOnlineModpackOptions {
                    url,
                    instance_name,
                    project_id,
                    is_update,
                    server_id,
                    pack_version_id,
                    pack_file_name,
                },
            };
            spawn_retry_with_rollback(
                &task_manager,
                task_id.clone(),
                task.task_type,
                executable,
                prep,
            )
            .await?
        }
        TaskType::InstallShaderpack {
            source,
            project_id,
            pack_name,
            instance_id,
            target_dir,
            download_url,
            file_id,
            slug,
            original_filename,
            version_number,
        } => {
            let executable = crate::core::manager::InstallShaderpackTask {
                options: crate::core::manager::InstallShaderpackOptions {
                    source,
                    project_id,
                    pack_name,
                    instance_id,
                    target_dir,
                    download_url,
                    file_id,
                    slug,
                    original_filename,
                    version_number,
                },
            };
            task_manager
                .spawn_task_with_id(task_id.clone(), task.task_type, executable)
                .await
                .map_err(|e| e.to_string())?
        }
        TaskType::InstallWorld {
            source,
            project_id,
            pack_name,
            instance_id,
            target_dir,
            download_url,
            file_id,
        } => {
            let executable = crate::core::manager::InstallWorldTask {
                options: crate::core::manager::InstallWorldOptions {
                    source,
                    project_id,
                    pack_name,
                    instance_id,
                    target_dir,
                    download_url,
                    file_id,
                },
            };
            task_manager
                .spawn_task_with_id(task_id.clone(), task.task_type, executable)
                .await
                .map_err(|e| e.to_string())?
        }
        TaskType::InstallDatapack {
            source,
            project_id,
            pack_name,
            instance_id,
            target_dir,
            download_url,
            file_id,
            slug,
            original_filename,
            version_number,
        } => {
            let executable = crate::core::manager::InstallDatapackTask {
                options: crate::core::manager::InstallDatapackOptions {
                    source,
                    project_id,
                    pack_name,
                    instance_id,
                    target_dir,
                    download_url,
                    file_id,
                    slug,
                    original_filename,
                    version_number,
                },
            };
            task_manager
                .spawn_task_with_id(task_id.clone(), task.task_type, executable)
                .await
                .map_err(|e| e.to_string())?
        }
        TaskType::InstallLoaderToInstance {
            instance_id,
            mc_version,
            loader_type,
            loader_version,
        } => {
            crate::core::mod_loader::spawn_loader_install_task(
                &task_manager,
                Some(task_id.clone()),
                instance_id,
                mc_version,
                loader_type,
                loader_version,
                task.task_type,
            )
            .await
            .map_err(|e| e.to_string())?
        }
        TaskType::SwitchInstanceLoaderVersion {
            instance_id,
            mc_version,
            loader_type,
            new_loader_version,
            ..
        } => {
            crate::core::mod_loader::spawn_loader_install_task(
                &task_manager,
                Some(task_id.clone()),
                instance_id,
                mc_version,
                loader_type,
                new_loader_version,
                task.task_type,
            )
            .await
            .map_err(|e| e.to_string())?
        }
        TaskType::UninstallInstanceLoader {
            instance_id,
            mc_version,
            mod_handling_mode,
            ..
        } => {
            let executable = crate::core::mod_loader::UninstallInstanceLoaderTask {
                options: crate::core::mod_loader::UninstallInstanceLoaderOptions {
                    instance_id,
                    mod_handling_mode: mod_handling_mode.unwrap_or_else(|| "keep".to_string()),
                    mc_version: Some(mc_version),
                },
            };
            task_manager
                .spawn_task_with_id(task_id.clone(), task.task_type, executable)
                .await
                .map_err(|e| e.to_string())?
        }
        TaskType::InstallPreset { .. } => {
            return Err(DawnlandError::Unknown("Cannot retry preset install task".to_string()).into())
        }
        TaskType::Generic { .. } => {
            return Err(DawnlandError::Unknown("Cannot retry generic task".to_string()).into())
        }
    };

    Ok(task_id)
}

#[tauri::command]
pub async fn task_create(
    task_type: String,
    payload: serde_json::Value,
    task_manager: State<'_, TaskManager>,
    app: tauri::AppHandle,
) -> Result<String, AppError> {
    use crate::core::task::TaskType;

    tracing::info!("task_create: type={task_type}");

    let task_id = match task_type.as_str() {
        "install-mod" => {
            let options: crate::core::manager::InstallModOptions = serde_json::from_value(payload)
                .map_err(|e| DawnlandError::Unknown(format!("Invalid payload: {}", e)))?;

            let task_type_enum = TaskType::InstallMod {
                source: options.source.clone(),
                project_id: options.project_id.clone(),
                mod_name: options.mod_name.clone(),
                instance_id: options.instance_id.clone(),
                target_dir: options.target_dir.clone(),
                download_url: options.download_url.clone(),
                file_id: options.file_id.clone(),
                dependencies: options.dependencies.clone(),
                keep_both: options.keep_both,
                slug: options.slug.clone(),
                original_filename: options.original_filename.clone(),
                version_number: options.version_number.clone(),
            };

            let executable = crate::core::manager::InstallModTask { options };
            task_manager
                .spawn_task(task_type_enum, executable)
                .await
                .map_err(|e| e.to_string())?
        }
        "install-resourcepack" => {
            let options: crate::core::manager::InstallResourcepackOptions =
                serde_json::from_value(payload)
                    .map_err(|e| DawnlandError::Unknown(format!("Invalid payload: {}", e)))?;

            let task_type_enum = TaskType::InstallResourcepack {
                source: options.source.clone(),
                project_id: options.project_id.clone(),
                pack_name: options.pack_name.clone(),
                instance_id: options.instance_id.clone(),
                target_dir: options.target_dir.clone(),
                download_url: options.download_url.clone(),
                file_id: options.file_id.clone(),
                slug: options.slug.clone(),
                original_filename: options.original_filename.clone(),
                version_number: options.version_number.clone(),
            };

            let executable = crate::core::manager::InstallResourcepackTask { options };
            task_manager
                .spawn_task(task_type_enum, executable)
                .await
                .map_err(|e| e.to_string())?
        }
        "install-datapack" => {
            let options: crate::core::manager::InstallDatapackOptions =
                serde_json::from_value(payload)
                    .map_err(|e| DawnlandError::Unknown(format!("Invalid payload: {}", e)))?;

            let task_type_enum = TaskType::InstallDatapack {
                source: options.source.clone(),
                project_id: options.project_id.clone(),
                pack_name: options.pack_name.clone(),
                instance_id: options.instance_id.clone(),
                target_dir: options.target_dir.clone(),
                download_url: options.download_url.clone(),
                file_id: options.file_id.clone(),
                slug: options.slug.clone(),
                original_filename: options.original_filename.clone(),
                version_number: options.version_number.clone(),
            };

            let executable = crate::core::manager::InstallDatapackTask { options };
            task_manager
                .spawn_task(task_type_enum, executable)
                .await
                .map_err(|e| e.to_string())?
        }
        "install-shaderpack" => {
            let options: crate::core::manager::InstallShaderpackOptions =
                serde_json::from_value(payload)
                    .map_err(|e| DawnlandError::Unknown(format!("Invalid payload: {}", e)))?;

            let task_type_enum = TaskType::InstallShaderpack {
                source: options.source.clone(),
                project_id: options.project_id.clone(),
                pack_name: options.pack_name.clone(),
                instance_id: options.instance_id.clone(),
                target_dir: options.target_dir.clone(),
                download_url: options.download_url.clone(),
                file_id: options.file_id.clone(),
                slug: options.slug.clone(),
                original_filename: options.original_filename.clone(),
                version_number: options.version_number.clone(),
            };

            let executable = crate::core::manager::InstallShaderpackTask { options };
            task_manager
                .spawn_task(task_type_enum, executable)
                .await
                .map_err(|e| e.to_string())?
        }
        "install-world" => {
            let options: crate::core::manager::InstallWorldOptions =
                serde_json::from_value(payload)
                    .map_err(|e| DawnlandError::Unknown(format!("Invalid payload: {}", e)))?;

            let task_type_enum = TaskType::InstallWorld {
                source: options.source.clone(),
                project_id: options.project_id.clone(),
                pack_name: options.pack_name.clone(),
                instance_id: options.instance_id.clone(),
                target_dir: options.target_dir.clone(),
                download_url: options.download_url.clone(),
                file_id: options.file_id.clone(),
            };

            let executable = crate::core::manager::InstallWorldTask { options };
            task_manager
                .spawn_task(task_type_enum, executable)
                .await
                .map_err(|e| e.to_string())?
        }
        "install-preset" => {
            let options: crate::core::manager::InstallPresetOptions =
                serde_json::from_value(payload)
                    .map_err(|e| DawnlandError::Unknown(format!("Invalid payload: {}", e)))?;

            let task_type_enum = TaskType::InstallPreset {
                preset_name: options.preset_name.clone(),
                asset_type: options.asset_type.clone(),
                instance_id: options.instance_id.clone(),
            };

            let executable = crate::core::manager::InstallPresetTask { options, app };
            task_manager
                .spawn_task(task_type_enum, executable)
                .await
                .map_err(|e| e.to_string())?
        }
        _ => {
            return Err(
                DawnlandError::Unknown(format!("Unsupported task type: {}", task_type)).into(),
            );
        }
    };

    Ok(task_id)
}

#[tauri::command]
pub async fn delete_task(
    task_id: String,
    task_manager: State<'_, TaskManager>,
) -> Result<(), AppError> {
    task_manager
        .delete_task(task_id)
        .await
        .map_err(|e| DawnlandError::Unknown(e.to_string()).into())
}
