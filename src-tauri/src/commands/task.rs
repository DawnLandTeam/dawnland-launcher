use crate::core::task::{TaskManager, TaskState, TaskError};
use tauri::State;

#[tauri::command]
pub async fn get_task_history(
    task_manager: State<'_, TaskManager>,
) -> Result<Vec<TaskState>, String> {
    task_manager.load_history().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn cancel_task(
    task_id: String,
    task_manager: State<'_, TaskManager>,
) -> Result<(), String> {
    task_manager.cancel_task(&task_id).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn clear_task_history(
    task_manager: State<'_, TaskManager>,
) -> Result<(), String> {
    task_manager.clear_history().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn retry_task(
    task_id: String,
    task_manager: State<'_, TaskManager>,
    app: tauri::AppHandle,
) -> Result<String, String> {
    let task = task_manager.get_task(&task_id).await.map_err(|e| e.to_string())?
        .ok_or_else(|| "Task not found".to_string())?;

    use crate::core::task::TaskType;

    let _new_task_id = match task.task_type.clone() {
        TaskType::InstallVanilla { version_id, version_json_url, is_dependency } => {
            let base_dir = crate::core::mojang::get_minecraft_base();
            let version_dir = base_dir.join("versions").join(&version_id);
            let _ = tokio::fs::create_dir_all(&version_dir).await;
            crate::core::launcher::InstanceConfig::ensure_installing(&version_dir, is_dependency.unwrap_or(false)).await;

            let executable = crate::core::mojang::InstallVanillaTask {
                options: crate::core::mojang::VanillaInstallOptions {
                    version_id,
                    version_json_url,
                    is_dependency,
                },
            };
            task_manager.spawn_task_with_id(task_id.clone(), task.task_type, executable).await.map_err(|e| e.to_string())?
        }
        TaskType::InstallForge { mc_version, loader_version, loader_type, custom_instance_name, is_dependency } => {
            let base_dir = crate::core::mojang::get_minecraft_base();
            let instance_dir = base_dir.join("versions").join(&custom_instance_name);
            let _ = tokio::fs::create_dir_all(&instance_dir).await;
            crate::core::launcher::InstanceConfig::ensure_installing(&instance_dir, is_dependency.unwrap_or(false)).await;

            let executable = crate::core::forge::InstallForgeTask {
                options: crate::core::forge::InstallForgeOptions {
                    mc_version,
                    loader_version,
                    loader_type,
                    custom_instance_name,
                    is_dependency,
                },
            };
            task_manager.spawn_task_with_id(task_id.clone(), task.task_type, executable).await.map_err(|e| e.to_string())?
        }
        TaskType::InstallFabric { mc_version, fabric_version, custom_instance_name, is_dependency } => {
            let base_dir = crate::core::mojang::get_minecraft_base();
            let instance_dir = base_dir.join("versions").join(&custom_instance_name);
            let _ = tokio::fs::create_dir_all(&instance_dir).await;
            crate::core::launcher::InstanceConfig::ensure_installing(&instance_dir, is_dependency.unwrap_or(false)).await;

            let executable = crate::core::fabric::InstallFabricTask {
                options: crate::core::fabric::InstallFabricOptions {
                    mc_version,
                    fabric_version,
                    custom_instance_name,
                    is_dependency,
                },
            };
            task_manager.spawn_task_with_id(task_id.clone(), task.task_type, executable).await.map_err(|e| e.to_string())?
        }
        TaskType::InstallModpack { zip_path, instance_name, is_update, project_id } => {
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
            task_manager.spawn_task_with_id(task_id.clone(), task.task_type, executable).await.map_err(|e| e.to_string())?
        }
        TaskType::InstallOnlineModpack { url, instance_name, is_update, project_id } => {
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
            task_manager.spawn_task_with_id(task_id.clone(), task.task_type, executable).await.map_err(|e| e.to_string())?
        }
        TaskType::Generic { .. } => return Err("Cannot retry generic task".to_string()),
    };

    Ok(task_id)
}
