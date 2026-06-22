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
pub async fn clear_task_history(task_manager: State<'_, TaskManager>) -> Result<(), AppError> {
    task_manager
        .clear_history()
        .await
        .map_err(|e| DawnlandError::Unknown(e.to_string()).into())
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

    use crate::core::task::TaskType;

    let _new_task_id = match task.task_type.clone() {
        TaskType::InstallVanilla {
            version_id,
            version_json_url,
            is_dependency,
        } => {
            let base_dir = crate::core::mojang::get_minecraft_base();
            let version_dir = base_dir.join("versions").join(&version_id);
            let _ = tokio::fs::create_dir_all(&version_dir).await;
            crate::core::launcher::InstanceConfig::ensure_installing(
                &version_dir,
                is_dependency.unwrap_or(false),
            )
            .await;

            let executable = crate::core::mojang::InstallVanillaTask {
                options: crate::core::mojang::VanillaInstallOptions {
                    version_id,
                    version_json_url,
                    is_dependency,
                },
            };
            task_manager
                .spawn_task_with_id(task_id.clone(), task.task_type, executable)
                .await
                .map_err(|e| e.to_string())?
        }
        TaskType::InstallForge {
            mc_version,
            loader_version,
            loader_type,
            custom_instance_name,
            is_dependency,
        } => {
            let base_dir = crate::core::mojang::get_minecraft_base();
            let instance_dir = base_dir.join("versions").join(&custom_instance_name);
            let _ = tokio::fs::create_dir_all(&instance_dir).await;
            crate::core::launcher::InstanceConfig::ensure_installing(
                &instance_dir,
                is_dependency.unwrap_or(false),
            )
            .await;

            let executable = crate::core::forge::InstallForgeTask {
                options: crate::core::forge::InstallForgeOptions {
                    mc_version,
                    loader_version,
                    loader_type,
                    custom_instance_name,
                    is_dependency,
                },
            };
            task_manager
                .spawn_task_with_id(task_id.clone(), task.task_type, executable)
                .await
                .map_err(|e| e.to_string())?
        }
        TaskType::InstallFabric {
            mc_version,
            fabric_version,
            custom_instance_name,
            is_dependency,
        } => {
            let base_dir = crate::core::mojang::get_minecraft_base();
            let instance_dir = base_dir.join("versions").join(&custom_instance_name);
            let _ = tokio::fs::create_dir_all(&instance_dir).await;
            crate::core::launcher::InstanceConfig::ensure_installing(
                &instance_dir,
                is_dependency.unwrap_or(false),
            )
            .await;

            let executable = crate::core::fabric::InstallFabricTask {
                options: crate::core::fabric::InstallFabricOptions {
                    mc_version,
                    fabric_version,
                    custom_instance_name,
                    is_dependency,
                },
            };
            task_manager
                .spawn_task_with_id(task_id.clone(), task.task_type, executable)
                .await
                .map_err(|e| e.to_string())?
        }
        TaskType::InstallModpack {
            zip_path,
            instance_name,
            is_update,
            project_id,
        } => {
            let base_dir = crate::core::mojang::get_minecraft_base();
            let instance_dir = base_dir.join("versions").join(&instance_name);
            let _ = tokio::fs::create_dir_all(&instance_dir).await;
            crate::core::launcher::InstanceConfig::ensure_installing(&instance_dir, false).await;

            let executable = crate::commands::modpack::InstallModpackTask {
                options: crate::commands::modpack::InstallModpackOptions {
                    zip_path,
                    instance_name,
                    is_update,
                    project_id,
                },
            };
            task_manager
                .spawn_task_with_id(task_id.clone(), task.task_type, executable)
                .await
                .map_err(|e| e.to_string())?
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
        } => {
            let base_dir = crate::core::mojang::get_minecraft_base();
            let instance_dir = base_dir.join("versions").join(&instance_name);
            let _ = tokio::fs::create_dir_all(&instance_dir).await;
            crate::core::launcher::InstanceConfig::ensure_installing(&instance_dir, false).await;

            let executable = crate::commands::modpack::InstallOnlineModpackTask {
                options: crate::commands::modpack::InstallOnlineModpackOptions {
                    url,
                    instance_name,
                    project_id,
                    is_update,
                },
            };
            task_manager
                .spawn_task_with_id(task_id.clone(), task.task_type, executable)
                .await
                .map_err(|e| e.to_string())?
        }
        TaskType::InstallShaderpack {
            source,
            project_id,
            pack_name,
            instance_id,
            target_dir,
            download_url,
            file_id,
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
        TaskType::Generic { .. } => {
            return Err(DawnlandError::Unknown("Cannot retry generic task".to_string()).into())
        }
    };

    Ok(task_id)
}

#[tauri::command]
pub async fn task_create(
    taskType: String,
    payload: serde_json::Value,
    task_manager: State<'_, TaskManager>,
    app: tauri::AppHandle,
) -> Result<String, AppError> {
    use crate::core::task::TaskType;

    let task_id = match taskType.as_str() {
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
            };

            let executable = crate::core::manager::InstallResourcepackTask { options };
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
        _ => {
            return Err(
                DawnlandError::Unknown(format!("Unsupported task type: {}", taskType)).into(),
            )
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
