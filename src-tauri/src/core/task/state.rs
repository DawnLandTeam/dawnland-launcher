use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TaskType {
    InstallVanilla { version_id: String, version_json_url: String, is_dependency: Option<bool> },
    InstallForge { mc_version: String, loader_version: String, loader_type: String, custom_instance_name: String, is_dependency: Option<bool> },
    InstallFabric { mc_version: String, fabric_version: String, custom_instance_name: String, is_dependency: Option<bool> },
    InstallModpack { zip_path: String, instance_name: String, is_update: bool, project_id: Option<String> },
    InstallOnlineModpack { url: String, instance_name: String, is_update: bool, project_id: Option<String> },
    Generic { name: String },
}

impl TaskType {
    pub fn name(&self) -> String {
        match self {
            TaskType::InstallVanilla { version_id, .. } => format!("Downloading Vanilla {}", version_id),
            TaskType::InstallForge { mc_version, loader_version, .. } => format!("Installing Forge {} for {}", loader_version, mc_version),
            TaskType::InstallFabric { mc_version, fabric_version, .. } => format!("Installing Fabric {} for {}", fabric_version, mc_version),
            TaskType::InstallModpack { instance_name, .. } => format!("Installing Modpack {}", instance_name),
            TaskType::InstallOnlineModpack { instance_name, .. } => format!("Downloading Modpack {}", instance_name),
            TaskType::Generic { name } => name.clone(),
        }
    }

    pub fn instance_id(&self) -> Option<String> {
        match self {
            TaskType::InstallVanilla { version_id, .. } => Some(version_id.clone()),
            TaskType::InstallForge { custom_instance_name, .. } => Some(custom_instance_name.clone()),
            TaskType::InstallFabric { custom_instance_name, .. } => Some(custom_instance_name.clone()),
            TaskType::InstallModpack { instance_name, .. } => Some(instance_name.clone()),
            TaskType::InstallOnlineModpack { instance_name, .. } => Some(instance_name.clone()),
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskProgress {
    pub current: u64,
    pub total: u64,
    pub step: u32,
    pub total_steps: u32,
    pub detail: String,
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
    pub context_data: Option<serde_json::Value>,
}

impl TaskState {
    pub fn new(id: String, task_type: TaskType) -> Self {
        let now = chrono::Utc::now().timestamp();
        Self {
            id,
            task_type,
            status: TaskStatus::Pending,
            progress: TaskProgress {
                current: 0,
                total: 100,
                step: 1,
                total_steps: 1,
                detail: "Pending...".to_string(),
            },
            error: None,
            created_at: now,
            updated_at: now,
            context_data: None,
        }
    }

    pub fn can_transition_to(&self, new_status: &TaskStatus) -> bool {
        use TaskStatus::*;
        match (&self.status, new_status) {
            (Pending, Running) => true,
            (Pending, Cancelled) => true,
            (Running, Paused) => true,
            (Running, Completed) => true,
            (Running, Failed) => true,
            (Running, Cancelled) => true,
            (Paused, Running) => true,
            (Paused, Cancelled) => true,
            _ => false,
        }
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
