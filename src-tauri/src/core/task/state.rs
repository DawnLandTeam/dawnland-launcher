#![allow(dead_code)]
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TaskType {
    InstallVanilla {
        version_id: String,
        version_json_url: String,
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
    },
    InstallOnlineModpack {
        url: String,
        instance_name: String,
        is_update: bool,
        project_id: Option<String>,
    },
    InstallMod {
        source: String,
        project_id: String,
        mod_name: String,
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
    InstallWorld {
        source: String,
        project_id: String,
        pack_name: String,
        instance_id: Option<String>,
        target_dir: Option<String>,
        download_url: String,
        file_id: String,
    },
    Generic {
        name: String,
    },
}

impl TaskType {
    pub fn name(&self) -> String {
        match self {
            TaskType::InstallVanilla { version_id, .. } => {
                format!("Downloading Vanilla {}", version_id)
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
                mod_name,
                instance_id,
                ..
            } => {
                if let Some(i) = instance_id {
                    format!("Installing Mod {} to {}", mod_name, i)
                } else {
                    format!("Downloading Mod {}", mod_name)
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
            TaskType::Generic { name } => name.clone(),
        }
    }

    pub fn instance_id(&self) -> Option<String> {
        match self {
            TaskType::InstallVanilla { version_id, .. } => Some(version_id.clone()),
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
            TaskType::Generic { .. } => None,
        }
    }

    /// Determines if this task conflicts with another task, preventing them from running concurrently.
    /// This acts as the "duplicate detection method" for various task implementations.
    pub fn conflicts_with(&self, other: &TaskType) -> bool {
        // Default rule: If both tasks target the same instance_id, they conflict.
        if let (Some(id1), Some(id2)) = (self.instance_id(), other.instance_id()) {
            if id1 == id2 {
                return true;
            }
        }

        // Custom conflict rules for specific task types can be added here.
        // For example, Generic tasks conflict if they have the exact same name.
        match (self, other) {
            (TaskType::Generic { name: n1 }, TaskType::Generic { name: n2 }) => n1 == n2,
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TaskStatus {
    Pending,
    Running,
    Paused,
    Completed,
    Failed,
    Cancelled,
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
        use TaskStatus::*;
        matches!(
            (&self.status, new_status),
            (Pending, Running)
                | (Pending, Cancelled)
                | (Running, Paused)
                | (Running, Completed)
                | (Running, Failed)
                | (Running, Cancelled)
                | (Paused, Running)
                | (Paused, Cancelled)
        )
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_type_conflicts() {
        let vanilla_1 = TaskType::InstallVanilla {
            version_id: "1.20.1".to_string(),
            version_json_url: "".to_string(),
            is_dependency: None,
        };
        let vanilla_2 = TaskType::InstallVanilla {
            version_id: "1.20.1".to_string(),
            version_json_url: "".to_string(),
            is_dependency: None,
        };
        let vanilla_3 = TaskType::InstallVanilla {
            version_id: "1.19.4".to_string(),
            version_json_url: "".to_string(),
            is_dependency: None,
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
    fn test_task_state_transitions() {
        use TaskStatus::*;

        let task = TaskState::new(
            "test-id".to_string(),
            TaskType::Generic {
                name: "test".to_string(),
            },
        );
        assert_eq!(task.status, Pending);

        // Pending -> Running, Cancelled (valid)
        assert!(task.can_transition_to(&Running));
        assert!(task.can_transition_to(&Cancelled));

        // Pending -> Completed, Failed, Paused (invalid)
        assert!(!task.can_transition_to(&Completed));
        assert!(!task.can_transition_to(&Failed));
        assert!(!task.can_transition_to(&Paused));

        let mut running_task = task.clone();
        running_task.status = Running;
        assert!(running_task.can_transition_to(&Paused));
        assert!(running_task.can_transition_to(&Completed));
        assert!(running_task.can_transition_to(&Failed));
        assert!(running_task.can_transition_to(&Cancelled));
        assert!(!running_task.can_transition_to(&Pending));

        let mut paused_task = task.clone();
        paused_task.status = Paused;
        assert!(paused_task.can_transition_to(&Running));
        assert!(paused_task.can_transition_to(&Cancelled));
        assert!(!paused_task.can_transition_to(&Completed));

        let mut completed_task = task.clone();
        completed_task.status = Completed;
        assert!(!completed_task.can_transition_to(&Running));
        assert!(!completed_task.can_transition_to(&Pending));
    }
}
