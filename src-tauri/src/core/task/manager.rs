use super::db::TaskDatabase;
use super::state::{TaskError, TaskState, TaskStatus, TaskType};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub struct TaskContext {
    pub id: String,
    pub app_handle: AppHandle,
    pub cancel_token: CancellationToken,
    pub manager: TaskManager,
    state: Arc<RwLock<TaskState>>,
    pub sub_task_key: Option<String>,
}

impl TaskContext {
    pub fn with_sub_task(&self, key: &str) -> Self {
        let mut new_ctx = self.clone();
        new_ctx.sub_task_key = Some(key.to_string());
        new_ctx
    }

    pub async fn init_sub_tasks(&self, tasks: Vec<crate::core::task::state::SubTaskState>) {
        let mut state = self.state.write().await;
        state.progress.sub_tasks = tasks;
        state.updated_at = chrono::Utc::now().timestamp();
        self.manager.emit_state(&state).await;
    }

    pub async fn append_sub_tasks(&self, mut tasks: Vec<crate::core::task::state::SubTaskState>) {
        let mut state = self.state.write().await;
        for task in tasks.drain(..) {
            if !state.progress.sub_tasks.iter().any(|s| s.key == task.key) {
                state.progress.sub_tasks.push(task);
            }
        }
        state.updated_at = chrono::Utc::now().timestamp();
        self.manager.emit_state(&state).await;
    }

    pub async fn update_progress(&self, current: u64, total: u64, detail: &str) {
        let mut state = self.state.write().await;

        if let Some(key) = &self.sub_task_key {
            // Update the specific sub-task
            if let Some(sub) = state.progress.sub_tasks.iter_mut().find(|s| &s.key == key) {
                if sub.status == crate::core::task::state::SubTaskStatus::Pending {
                    sub.status = crate::core::task::state::SubTaskStatus::Running;
                }
                sub.current = current;
                sub.total = total;
                if current == total && total > 0 {
                    sub.status = crate::core::task::state::SubTaskStatus::Completed;
                }
            }

            // Recalculate global progress
            let mut global_progress = 0.0;
            for s in &state.progress.sub_tasks {
                let p = if s.total > 0 {
                    s.current as f64 / s.total as f64
                } else {
                    if s.status == crate::core::task::state::SubTaskStatus::Completed {
                        1.0
                    } else {
                        0.0
                    }
                };
                global_progress += p * (s.weight as f64 / 100.0);
            }
            state.progress.current = (global_progress * 10000.0) as u64;
            state.progress.total = 10000;
            state.progress.detail = detail.to_string();
        } else {
            // Fallback for tasks without sub-tasks
            state.progress.current = current;
            state.progress.total = total;
            state.progress.detail = detail.to_string();
        }

        state.updated_at = chrono::Utc::now().timestamp();
        self.manager.emit_state(&state).await;
    }

    pub async fn update_download_metrics(&self, speed: u64, remaining_files: u32) {
        let mut state = self.state.write().await;
        state.progress.speed = speed;
        state.progress.remaining_files = remaining_files;
        state.updated_at = chrono::Utc::now().timestamp();
        self.manager.emit_state(&state).await;
    }

    pub async fn set_total_steps(&self, total_steps: u32) {
        let mut state = self.state.write().await;
        state.progress.total_steps = total_steps;
        state.updated_at = chrono::Utc::now().timestamp();
        self.manager.emit_state(&state).await;
    }

    pub async fn next_step(&self, detail: &str) {
        let mut state = self.state.write().await;
        if state.progress.step < state.progress.total_steps {
            state.progress.step += 1;
        }
        state.progress.current = 0;
        state.progress.detail = detail.to_string();
        state.updated_at = chrono::Utc::now().timestamp();
        self.manager.emit_state(&state).await;
    }

    pub async fn set_step(&self, step: u32, total_steps: u32, detail: &str) {
        let mut state = self.state.write().await;
        state.progress.step = step;
        state.progress.total_steps = total_steps;
        state.progress.current = 0;
        state.progress.detail = detail.to_string();
        state.updated_at = chrono::Utc::now().timestamp();
        self.manager.emit_state(&state).await;
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel_token.is_cancelled()
    }

    pub async fn wait_cancelled(&self) {
        self.cancel_token.cancelled().await
    }

    pub async fn get_context_data<T: serde::de::DeserializeOwned>(&self) -> Option<T> {
        let state = self.state.read().await;
        if let Some(val) = &state.context_data {
            serde_json::from_value(val.clone()).ok()
        } else {
            None
        }
    }

    pub async fn set_context_data<T: serde::Serialize>(&self, data: &T) {
        let mut state = self.state.write().await;
        if let Ok(val) = serde_json::to_value(data) {
            state.context_data = Some(val);
        }
        self.manager.emit_state(&state).await;
    }
}

// A generic trait for any executable task
#[async_trait::async_trait]
pub trait ExecutableTask: Send + Sync {
    async fn execute(&self, ctx: TaskContext) -> Result<(), TaskError>;
}

pub struct TaskManager {
    db: TaskDatabase,
    app_handle: AppHandle,
    active_tasks: Arc<RwLock<HashMap<String, CancellationToken>>>,
}

impl Clone for TaskManager {
    fn clone(&self) -> Self {
        Self {
            db: self.db.clone(),
            app_handle: self.app_handle.clone(),
            active_tasks: self.active_tasks.clone(),
        }
    }
}

impl TaskManager {
    pub async fn new(app_handle: AppHandle, db: TaskDatabase) -> Self {
        Self {
            db,
            app_handle,
            active_tasks: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn load_history(&self) -> Result<Vec<TaskState>, TaskError> {
        self.db.load_all_tasks().await
    }

    pub async fn get_task(&self, id: &str) -> Result<Option<TaskState>, TaskError> {
        self.db.get_task(id).await
    }

    pub async fn clear_history(&self) -> Result<(), TaskError> {
        self.db.clear_history().await
    }

    pub async fn delete_task(&self, id: String) -> Result<(), TaskError> {
        self.db.delete_task(id.clone()).await?;
        let _ = self.app_handle.emit("task-deleted", id);
        Ok(())
    }

    pub async fn emit_state(&self, state: &TaskState) {
        // Persist to DB first
        let _ = self.db.save_task(state).await;
        // Emit to frontend
        let _ = self.app_handle.emit("task-progress-update", state);
    }

    pub async fn wait_for_instance(&self, instance_id: &str, cancel_token: &CancellationToken) {
        loop {
            if cancel_token.is_cancelled() {
                break;
            }
            let is_running = {
                let mut running = false;
                if let Ok(all_tasks) = self.db.load_all_tasks().await {
                    for task in all_tasks {
                        if (task.status == TaskStatus::Pending
                            || task.status == TaskStatus::Running
                            || task.status == TaskStatus::Paused)
                            && task.task_type.instance_id() == Some(instance_id.to_string())
                        {
                            running = true;
                            break;
                        }
                    }
                }
                running
            };
            if !is_running {
                break;
            }
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        }
    }

    pub async fn spawn_task<T: ExecutableTask + 'static>(
        &self,
        task_type: TaskType,
        executable: T,
    ) -> Result<String, TaskError> {
        // Prevent duplicate tasks by consulting the task's custom duplicate detection logic
        if let Ok(all_tasks) = self.db.load_all_tasks().await {
            for task in all_tasks {
                if task_type.conflicts_with(&task.task_type) {
                    if task.status == TaskStatus::Pending
                        || task.status == TaskStatus::Running
                        || task.status == TaskStatus::Paused
                    {
                        return Err(TaskError::Database(format!(
                            "CONFLICTING_TASK:{}",
                            task.task_type.name()
                        )));
                    } else {
                        // Delete the old inactive conflicting task (e.g. Failed/Cancelled tasks)
                        // so they don't clutter the history when superseded by this new task.
                        let _ = self.delete_task(task.id.clone()).await;
                        let _ = self.app_handle.emit("task-deleted", task.id);
                    }
                }
            }
        }

        let id = uuid::Uuid::new_v4().to_string();
        self.spawn_task_with_id(id, task_type, executable).await
    }

    pub async fn spawn_task_with_id<T: ExecutableTask + 'static>(
        &self,
        id: String,
        task_type: TaskType,
        executable: T,
    ) -> Result<String, TaskError> {
        let state = if let Ok(Some(existing_state)) = self.db.get_task(&id).await {
            // Reusing an existing task, preserve its ID and context_data
            let mut state = TaskState::new(id.clone(), task_type);
            state.context_data = existing_state.context_data;
            state
        } else {
            TaskState::new(id.clone(), task_type)
        };

        self.emit_state(&state).await;

        let cancel_token = CancellationToken::new();
        self.active_tasks
            .write()
            .await
            .insert(id.clone(), cancel_token.clone());

        let manager = self.clone();
        let id_clone = id.clone();
        let state_arc = Arc::new(RwLock::new(state));

        let ctx = TaskContext {
            id: id.clone(),
            app_handle: self.app_handle.clone(),
            cancel_token,
            manager: manager.clone(),
            state: state_arc.clone(),
            sub_task_key: None,
        };

        tokio::spawn(async move {
            {
                let mut state = state_arc.write().await;
                state.status = TaskStatus::Running;
                manager.emit_state(&state).await;
            }

            match executable.execute(ctx).await {
                Ok(_) => {
                    let mut state = state_arc.write().await;
                    state.status = TaskStatus::Completed;
                    state.progress.current = state.progress.total;
                    state.progress.detail = "Completed".to_string();
                    manager.emit_state(&state).await;
                }
                Err(e) => {
                    let mut state = state_arc.write().await;
                    // If it was cancelled by token, mark it as cancelled rather than failed
                    if manager
                        .active_tasks
                        .read()
                        .await
                        .get(&id_clone)
                        .map(|t| t.is_cancelled())
                        .unwrap_or(true)
                    {
                        state.status = TaskStatus::Cancelled;
                        state.error = Some("Task cancelled".to_string());
                    } else {
                        state.status = TaskStatus::Failed;
                        state.error = Some(e.to_string());
                    }
                    manager.emit_state(&state).await;

                    // Clean up the half-installed instance directory
                    if let Some(instance_id) = state.task_type.instance_id() {
                        let base_dir = crate::core::mojang::get_minecraft_base();
                        let instance_dir = base_dir.join("versions").join(&instance_id);
                        let config_path = instance_dir.join("dlml.json");
                        if tokio::fs::try_exists(&config_path).await.unwrap_or(false) {
                            if let Ok(content) = tokio::fs::read_to_string(&config_path).await {
                                if let Ok(config) = serde_json::from_str::<
                                    crate::core::launcher::InstanceConfig,
                                >(&content)
                                {
                                    // For first time installation, always clean up failed/cancelled instances
                                    if config.is_installing && !config.is_updating {
                                        tracing::info!(
                                            "Cleaning up failed/cancelled instance: {}",
                                            instance_id
                                        );
                                        let _ = tokio::fs::remove_dir_all(&instance_dir).await;
                                    } else if config.is_updating {
                                        tracing::warn!("Instance {} is marked as updating. Skipping auto-cleanup.", instance_id);
                                        let mut rescued_config = config.clone();
                                        rescued_config.is_installing = false;
                                        rescued_config.is_updating = false;
                                        if let Ok(json) = serde_json::to_string_pretty(&rescued_config) {
                                            let _ = tokio::fs::write(&config_path, json).await;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            manager.active_tasks.write().await.remove(&id_clone);
        });

        Ok(id)
    }

    pub async fn cancel_task(&self, id: &str) -> Result<(), TaskError> {
        let mut tasks = self.active_tasks.write().await;
        if let Some(token) = tasks.remove(id) {
            token.cancel();

            // Eagerly update the state to Cancelled to provide instant feedback
            if let Ok(Some(mut state)) = self.db.get_task(id).await {
                state.status = TaskStatus::Cancelled;
                state.error = Some("Task cancelled".to_string());
                self.emit_state(&state).await;

                // Eagerly delete the installing directory if applicable
                if let Some(instance_id) = state.task_type.instance_id() {
                    let base_dir = crate::core::mojang::get_minecraft_base();
                    let instance_dir = base_dir.join("versions").join(&instance_id);
                    // Only delete if it's currently marked as installing to avoid deleting valid updates
                    let config_path = instance_dir.join("dlml.json");
                    if tokio::fs::try_exists(&config_path).await.unwrap_or(false) {
                        if let Ok(content) = tokio::fs::read_to_string(&config_path).await {
                            if let Ok(config) = serde_json::from_str::<
                                crate::core::launcher::InstanceConfig,
                            >(&content)
                            {
                                // For first time installation, always eagerly clean up
                                if config.is_installing && !config.is_updating {
                                    tracing::info!(
                                        "Eagerly cleaning up cancelled instance: {}",
                                        instance_id
                                    );
                                    let _ = tokio::fs::remove_dir_all(&instance_dir).await;
                                } else if config.is_updating {
                                    tracing::warn!("Instance {} is marked as updating. Skipping eager auto-cleanup.", instance_id);
                                    let mut rescued_config = config.clone();
                                    rescued_config.is_installing = false;
                                    rescued_config.is_updating = false;
                                    if let Ok(json) = serde_json::to_string_pretty(&rescued_config) {
                                        let _ = tokio::fs::write(&config_path, json).await;
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Ok(())
        } else {
            Err(TaskError::NotFound)
        }
    }
}
