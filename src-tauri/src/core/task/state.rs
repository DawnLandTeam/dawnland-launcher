#![allow(dead_code)]
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TaskType {
    InstallVanilla {
        version_id: String,
        version_json_url: String,
        custom_instance_name: Option<String>,
        is_dependency: Option<bool>,
    },
    InstallForge {
        mc_version: String,
        loader_version: String,
        loader_type: String,
        custom_instance_name: String,
        is_dependency: Option<bool>,
    },
    InstallFabric {
        mc_version: String,
        fabric_version: String,
        custom_instance_name: String,
        is_dependency: Option<bool>,
    },
    InstallModpack {
        zip_path: String,
        instance_name: String,
        is_update: bool,
        project_id: Option<String>,
        #[serde(default)]
        server_id: Option<String>,
        #[serde(default)]
        pack_version_id: Option<String>,
        #[serde(default)]
        pack_file_name: Option<String>,
    },
    InstallOnlineModpack {
        url: String,
        instance_name: String,
        is_update: bool,
        project_id: Option<String>,
        #[serde(default)]
        server_id: Option<String>,
        #[serde(default)]
        pack_version_id: Option<String>,
        #[serde(default)]
        pack_file_name: Option<String>,
    },
    InstallMod {
        source: String,
        project_id: String,
        mod_name: Option<String>,
        instance_id: Option<String>,
        target_dir: Option<String>,
        download_url: String,
        file_id: String,
        dependencies: Option<Vec<crate::core::modrinth::UnifiedDependency>>,
        keep_both: Option<bool>,
    },
    InstallResourcepack {
        source: String,
        project_id: String,
        pack_name: String,
        instance_id: Option<String>,
        target_dir: Option<String>,
        download_url: String,
        file_id: String,
    },
    InstallShaderpack {
        source: String,
        project_id: String,
        pack_name: String,
        instance_id: Option<String>,
        target_dir: Option<String>,
        download_url: String,
        file_id: String,
    },
    InstallDatapack {
        source: String,
        project_id: String,
        pack_name: String,
        instance_id: Option<String>,
        target_dir: Option<String>,
        download_url: String,
        file_id: String,
    },
    InstallPreset {
        preset_name: String,
        asset_type: String,
        instance_id: String,
    },

    InstallWorld {
        source: String,
        project_id: String,
        pack_name: String,
        instance_id: Option<String>,
        target_dir: Option<String>,
        download_url: String,
        file_id: String,
    },
    InstallLoaderToInstance {
        instance_id: String,
        mc_version: String,
        loader_type: String,
        loader_version: String,
    },
    SwitchInstanceLoaderVersion {
        instance_id: String,
        mc_version: String,
        loader_type: String,
        old_loader_version: Option<String>,
        new_loader_version: String,
    },
    UninstallInstanceLoader {
        instance_id: String,
        mc_version: String,
        old_loader_type: String,
        #[serde(default)]
        mod_handling_mode: Option<String>,
    },
    Generic {
        name: String,
    },
}

impl TaskType {
    pub fn name(&self) -> String {
        match self {
            TaskType::InstallVanilla { version_id, custom_instance_name, .. } => {
                let id = custom_instance_name.clone().filter(|s| !s.trim().is_empty()).unwrap_or(version_id.clone());
                format!("Installing Vanilla {}", id)
            }
            TaskType::InstallForge {
                mc_version,
                loader_version,
                ..
            } => format!("Installing Forge {} for {}", loader_version, mc_version),
            TaskType::InstallFabric {
                mc_version,
                fabric_version,
                ..
            } => format!("Installing Fabric {} for {}", fabric_version, mc_version),
            TaskType::InstallModpack { instance_name, .. } => {
                format!("Installing Modpack {}", instance_name)
            }
            TaskType::InstallOnlineModpack { instance_name, .. } => {
                format!("Downloading Modpack {}", instance_name)
            }
            TaskType::InstallMod {
                project_id,
                mod_name,
                instance_id,
                ..
            } => {
                let name = mod_name.as_deref().unwrap_or(project_id);
                if let Some(i) = instance_id {
                    format!("Installing Mod {} to {}", name, i)
                } else {
                    format!("Downloading Mod {}", name)
                }
            }
            TaskType::InstallResourcepack {
                pack_name,
                instance_id,
                ..
            } => {
                if let Some(i) = instance_id {
                    format!("Installing Resource Pack {} to {}", pack_name, i)
                } else {
                    format!("Downloading Resource Pack {}", pack_name)
                }
            }
            TaskType::InstallShaderpack {
                pack_name,
                instance_id,
                ..
            } => {
                if let Some(i) = instance_id {
                    format!("Installing Shaderpack {} to {}", pack_name, i)
                } else {
                    format!("Downloading Shaderpack {}", pack_name)
                }
            }
            TaskType::InstallWorld {
                pack_name,
                instance_id,
                ..
            } => {
                if let Some(i) = instance_id {
                    format!("Installing World {} to {}", pack_name, i)
                } else {
                    format!("Downloading World {}", pack_name)
                }
            }
            TaskType::InstallDatapack {
                pack_name,
                instance_id,
                ..
            } => {
                if let Some(i) = instance_id {
                    format!("Installing Datapack {} to {}", pack_name, i)
                } else {
                    format!("Downloading Datapack {}", pack_name)
                }
            }
            TaskType::InstallPreset {
                preset_name,
                instance_id,
                ..
            } => format!("Installing Preset {} to {}", preset_name, instance_id),
            TaskType::InstallLoaderToInstance { instance_id, loader_type, loader_version, .. } => {
                format!("Installing {} {} to {}", loader_type, loader_version, instance_id)
            }
            TaskType::SwitchInstanceLoaderVersion { instance_id, loader_type, new_loader_version, .. } => {
                format!("Switching {} in {} to {}", loader_type, instance_id, new_loader_version)
            }
            TaskType::UninstallInstanceLoader { instance_id, old_loader_type, .. } => {
                format!("Uninstalling {} from {}", old_loader_type, instance_id)
            }
            TaskType::Generic { name } => name.clone(),
        }
    }

    pub fn instance_id(&self) -> Option<String> {
        match self {
            TaskType::InstallVanilla { version_id, custom_instance_name, .. } => {
                Some(custom_instance_name.clone().filter(|s| !s.trim().is_empty()).unwrap_or(version_id.clone()))
            }
            TaskType::InstallForge {
                custom_instance_name,
                ..
            } => Some(custom_instance_name.clone()),
            TaskType::InstallFabric {
                custom_instance_name,
                ..
            } => Some(custom_instance_name.clone()),
            TaskType::InstallModpack { instance_name, .. } => Some(instance_name.clone()),
            TaskType::InstallOnlineModpack { instance_name, .. } => Some(instance_name.clone()),
            TaskType::InstallMod { instance_id, .. } => instance_id.clone(),
            TaskType::InstallResourcepack { instance_id, .. } => instance_id.clone(),
            TaskType::InstallShaderpack { instance_id, .. } => instance_id.clone(),
            TaskType::InstallWorld { instance_id, .. } => instance_id.clone(),
            TaskType::InstallDatapack { instance_id, .. } => instance_id.clone(),
            TaskType::InstallPreset { instance_id, .. } => Some(instance_id.clone()),
            TaskType::InstallLoaderToInstance { instance_id, .. } => Some(instance_id.clone()),
            TaskType::SwitchInstanceLoaderVersion { instance_id, .. } => Some(instance_id.clone()),
            TaskType::UninstallInstanceLoader { instance_id, .. } => Some(instance_id.clone()),
            TaskType::Generic { .. } => None,
        }
    }

    pub fn is_update(&self) -> bool {
        match self {
            TaskType::InstallModpack { is_update, .. } => *is_update,
            TaskType::InstallOnlineModpack { is_update, .. } => *is_update,
            TaskType::InstallLoaderToInstance { .. }
            | TaskType::SwitchInstanceLoaderVersion { .. }
            | TaskType::UninstallInstanceLoader { .. } => true,
            _ => false,
        }
    }

    pub fn is_dependency(&self) -> bool {
        match self {
            TaskType::InstallVanilla { is_dependency, .. } => is_dependency.unwrap_or(false),
            TaskType::InstallForge { is_dependency, .. } => is_dependency.unwrap_or(false),
            TaskType::InstallFabric { is_dependency, .. } => is_dependency.unwrap_or(false),
            _ => false,
        }
    }

    pub fn is_core_installation(&self) -> bool {
        matches!(
            self,
            TaskType::InstallVanilla { .. }
                | TaskType::InstallForge { .. }
                | TaskType::InstallFabric { .. }
                | TaskType::InstallModpack { .. }
                | TaskType::InstallOnlineModpack { .. }
                | TaskType::InstallLoaderToInstance { .. }
                | TaskType::SwitchInstanceLoaderVersion { .. }
                | TaskType::UninstallInstanceLoader { .. }
        )
    }

    /// Returns the identity of asset-install-like tasks as a `(kind, project_id, instance_id)` tuple.
    /// Non-asset tasks return `None`.
    pub fn asset_identity(&self) -> Option<(&'static str, &String, &Option<String>)> {
        match self {
            TaskType::InstallMod { project_id, instance_id, .. } => Some(("mod", project_id, instance_id)),
            TaskType::InstallResourcepack { project_id, instance_id, .. } => Some(("resourcepack", project_id, instance_id)),
            TaskType::InstallShaderpack { project_id, instance_id, .. } => Some(("shaderpack", project_id, instance_id)),
            TaskType::InstallWorld { project_id, instance_id, .. } => Some(("world", project_id, instance_id)),
            TaskType::InstallPreset { preset_name, .. } => None, // Let presets run without strict asset locking, or could add custom preset locking if needed.
            _ => None,
        }
    }

    /// Determines the instance-level locking requirement for this task.
    pub fn mutex_type(&self) -> TaskMutex {
        match self {
            // Core tasks that mutate the base instance structure require an exclusive lock.
            TaskType::InstallVanilla { .. }
            | TaskType::InstallForge { .. }
            | TaskType::InstallFabric { .. }
            | TaskType::InstallModpack { .. }
            | TaskType::InstallOnlineModpack { .. }
            | TaskType::InstallLoaderToInstance { .. }
            | TaskType::SwitchInstanceLoaderVersion { .. }
            | TaskType::UninstallInstanceLoader { .. } => TaskMutex::Exclusive,

            // Asset downloads safely drop files into specific subdirectories (mods, resourcepacks, etc.).
            // They only require a shared lock, meaning they can run concurrently with other shared tasks
            // on the same instance, provided they aren't downloading the exact same asset.
            TaskType::InstallMod { .. }
            | TaskType::InstallResourcepack { .. }
            | TaskType::InstallShaderpack { .. }
            | TaskType::InstallWorld { .. }
            | TaskType::InstallPreset { .. }
            | TaskType::InstallDatapack { .. } => TaskMutex::Shared,

            // Generic tasks do not target instances in a standardized way.
            TaskType::Generic { .. } => TaskMutex::None,
        }
    }

    /// Determines if this task conflicts with another task, preventing them from running concurrently.
    /// This acts as the "duplicate detection method" for various task implementations.
    pub fn conflicts_with(&self, other: &TaskType) -> bool {
        // Handle instance-level concurrency based on TaskMutex.
        if let (Some(id1), Some(id2)) = (self.instance_id(), other.instance_id()) {
            if id1 == id2 {
                match (self.mutex_type(), other.mutex_type()) {
                    // If either task requires exclusive access to the instance, they conflict.
                    (TaskMutex::Exclusive, _) | (_, TaskMutex::Exclusive) => return true,
                    
                    // If both are shared, they only conflict if they are mutating the exact same asset.
                    (TaskMutex::Shared, TaskMutex::Shared) => {
                        if let (Some(asset1), Some(asset2)) = (self.asset_identity(), other.asset_identity()) {
                            if asset1 == asset2 {
                                return true;
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        // Generic tasks conflict if they have the exact same name.
        match (self, other) {
            (TaskType::Generic { name: n1 }, TaskType::Generic { name: n2 }) => n1 == n2,
            _ => false,
        }
    }
}

pub enum TaskMutex {
    Exclusive,
    Shared,
    None,
}


#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum TaskStatus {
    Pending,
    Running,
    Paused,
    Cancelling,
    Completed,
    Failed,
    Cancelled,
}

impl TaskStatus {
    pub fn is_active(&self) -> bool {
        matches!(
            self,
            Self::Pending | Self::Running | Self::Paused | Self::Cancelling
        )
    }

    pub fn is_cancelling(&self) -> bool {
        matches!(self, Self::Cancelling)
    }

    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }

    pub fn can_transition_to(&self, next: TaskStatus) -> bool {
        match (*self, next) {
            (a, b) if a == b => true,
            // Pending -> Running, Cancelling, Cancelled
            (Self::Pending, Self::Running | Self::Cancelling | Self::Cancelled) => true,
            // Running -> Paused, Cancelling, Cancelled, Completed, Failed
            (Self::Running, Self::Paused | Self::Cancelling | Self::Cancelled | Self::Completed | Self::Failed) => true,
            // Paused -> Running, Cancelling, Cancelled, Failed
            (Self::Paused, Self::Running | Self::Cancelling | Self::Cancelled | Self::Failed) => true,
            // Cancelling -> Cancelled (terminal on completion) or Failed (on secondary fatal error)
            (Self::Cancelling, Self::Cancelled | Self::Failed) => true,
            // Terminal states can transition to Pending / Running on retry/re-queue
            (Self::Completed | Self::Failed | Self::Cancelled, Self::Pending | Self::Running) => true,
            _ => false,
        }
    }

    pub fn transition_to(&mut self, next: TaskStatus) -> Result<(), TaskError> {
        if self.can_transition_to(next) {
            *self = next;
            Ok(())
        } else {
            Err(TaskError::InvalidStateTransition)
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SubTaskStatus {
    Pending,
    Running,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubTaskState {
    pub key: String,
    pub name: String,
    pub status: SubTaskStatus,
    pub current: u64,
    pub total: u64,
    pub weight: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskProgress {
    pub current: u64,
    pub total: u64,
    pub step: u32,
    pub total_steps: u32,
    pub detail: String,
    #[serde(default)]
    pub speed: u64,
    #[serde(default)]
    pub remaining_files: u32,
    #[serde(default)]
    pub sub_tasks: Vec<SubTaskState>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskState {
    pub id: String,
    pub task_type: TaskType,
    pub status: TaskStatus,
    pub progress: TaskProgress,
    pub error: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    #[serde(default)]
    pub auto_clear: bool,
    pub context_data: Option<serde_json::Value>,
}

impl TaskState {
    pub fn new(id: String, task_type: TaskType) -> Self {
        let now = chrono::Utc::now().timestamp();
        let auto_clear = matches!(
            task_type,
            TaskType::InstallMod { .. }
                | TaskType::InstallResourcepack { .. }
                | TaskType::InstallShaderpack { .. }
                | TaskType::InstallWorld { .. }
        );
        Self {
            id,
            task_type,
            status: TaskStatus::Pending,
            progress: TaskProgress {
                current: 0,
                total: 10000,
                step: 1,
                total_steps: 1,
                detail: "Pending...".to_string(),
                speed: 0,
                remaining_files: 0,
                sub_tasks: Vec::new(),
            },
            error: None,
            auto_clear,
            created_at: now,
            updated_at: now,
            context_data: None,
        }
    }

    pub fn can_transition_to(&self, new_status: &TaskStatus) -> bool {
        self.status.can_transition_to(*new_status)
    }

    pub fn transition_to(&mut self, new_status: TaskStatus) -> Result<(), TaskError> {
        self.status.transition_to(new_status)?;
        self.updated_at = chrono::Utc::now().timestamp();
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TaskError {
    #[error("Task not found")]
    NotFound,
    #[error("Invalid state transition")]
    InvalidStateTransition,
    #[error("Database error: {0}")]
    Database(String),
    #[error("Task failed: {0}")]
    ExecutionError(String),
    #[error("Internal error: {0}")]
    Internal(String),
    #[error("Fatal task error: {0}")]
    Fatal(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_type_conflicts() {
        let vanilla_1 = TaskType::InstallVanilla {
            version_id: "1.20.1".to_string(),
            version_json_url: "url".to_string(),
            custom_instance_name: None,
            is_dependency: Some(false),
        };
        let vanilla_2 = TaskType::InstallVanilla {
            version_id: "1.20.1".to_string(),
            version_json_url: "url".to_string(),
            custom_instance_name: None,
            is_dependency: Some(false),
        };
        let vanilla_3 = TaskType::InstallVanilla {
            version_id: "1.18.2".to_string(),
            version_json_url: "url".to_string(),
            custom_instance_name: None,
            is_dependency: Some(false),
        };

        let forge_1 = TaskType::InstallForge {
            mc_version: "1.20.1".to_string(),
            loader_version: "47.1.0".to_string(),
            loader_type: "forge".to_string(),
            custom_instance_name: "1.20.1".to_string(),
            is_dependency: None,
        };
        let forge_2 = TaskType::InstallForge {
            mc_version: "1.20.1".to_string(),
            loader_version: "47.1.0".to_string(),
            loader_type: "forge".to_string(),
            custom_instance_name: "forge-1.20.1".to_string(),
            is_dependency: None,
        };

        let generic_1 = TaskType::Generic {
            name: "test".to_string(),
        };
        let generic_2 = TaskType::Generic {
            name: "test".to_string(),
        };
        let generic_3 = TaskType::Generic {
            name: "other".to_string(),
        };

        assert!(vanilla_1.conflicts_with(&vanilla_2));
        assert!(!vanilla_1.conflicts_with(&vanilla_3));

        // They both target "1.20.1" instance folder
        assert!(vanilla_1.conflicts_with(&forge_1));
        assert!(!vanilla_1.conflicts_with(&forge_2));

        assert!(generic_1.conflicts_with(&generic_2));
        assert!(!generic_1.conflicts_with(&generic_3));
        assert!(!vanilla_1.conflicts_with(&generic_1));
    }

    #[test]
    fn test_vanilla_custom_instance_id_resolution() {
        let vanilla_default = TaskType::InstallVanilla {
            version_id: "1.20.1".to_string(),
            version_json_url: "url".to_string(),
            custom_instance_name: None,
            is_dependency: None,
        };
        assert_eq!(vanilla_default.instance_id(), Some("1.20.1".to_string()));

        let vanilla_custom = TaskType::InstallVanilla {
            version_id: "1.20.1".to_string(),
            version_json_url: "url".to_string(),
            custom_instance_name: Some("MyCustomVanilla".to_string()),
            is_dependency: None,
        };
        assert_eq!(vanilla_custom.instance_id(), Some("MyCustomVanilla".to_string()));

        let vanilla_empty = TaskType::InstallVanilla {
            version_id: "1.20.1".to_string(),
            version_json_url: "url".to_string(),
            custom_instance_name: Some("   ".to_string()),
            is_dependency: None,
        };
        assert_eq!(vanilla_empty.instance_id(), Some("1.20.1".to_string()));
    }

    #[test]
    fn test_task_state_transitions() {
        use TaskStatus::*;

        let mut task = TaskState::new(
            "test-id".to_string(),
            TaskType::Generic {
                name: "test".to_string(),
            },
        );
        assert_eq!(task.status, Pending);
        assert!(task.status.is_active());
        assert!(!task.status.is_terminal());

        // Pending -> Running, Cancelling, Cancelled (valid)
        assert!(task.can_transition_to(&Running));
        assert!(task.can_transition_to(&Cancelling));
        assert!(task.can_transition_to(&Cancelled));

        // Pending -> Completed, Failed, Paused (invalid)
        assert!(!task.can_transition_to(&Completed));
        assert!(!task.can_transition_to(&Failed));
        assert!(!task.can_transition_to(&Paused));

        // Test transition_to
        assert!(task.transition_to(Running).is_ok());
        assert_eq!(task.status, Running);
        assert!(task.status.is_active());

        // Running -> Paused, Cancelling, Cancelled, Completed, Failed
        assert!(task.can_transition_to(&Paused));
        assert!(task.can_transition_to(&Cancelling));
        assert!(task.can_transition_to(&Cancelled));
        assert!(task.can_transition_to(&Completed));
        assert!(task.can_transition_to(&Failed));
        assert!(!task.can_transition_to(&Pending));

        // Running -> Cancelling
        assert!(task.transition_to(Cancelling).is_ok());
        assert_eq!(task.status, Cancelling);
        assert!(task.status.is_active());
        assert!(task.status.is_cancelling());
        assert!(!task.status.is_terminal());

        // Cancelling can only transition to Cancelled (normal) or Failed (fatal cleanup error)
        assert!(task.can_transition_to(&Cancelled));
        assert!(task.can_transition_to(&Failed));
        assert!(!task.can_transition_to(&Running));
        assert!(!task.can_transition_to(&Pending));
        assert!(!task.can_transition_to(&Completed));

        // Cancelling -> Cancelled
        assert!(task.transition_to(Cancelled).is_ok());
        assert_eq!(task.status, Cancelled);
        assert!(!task.status.is_active());
        assert!(task.status.is_terminal());

        // Retrying terminal task -> Pending
        assert!(task.can_transition_to(&Pending));
        assert!(task.transition_to(Pending).is_ok());
        assert_eq!(task.status, Pending);
    }
}
