use super::db::TaskDatabase;
use super::state::{TaskError, TaskState, TaskStatus, TaskType};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
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
    last_emit_time: Arc<RwLock<tokio::time::Instant>>,
}

fn should_skip_progress_update_state(is_cancelled: bool, state: &TaskState) -> bool {
    is_cancelled || state.status.is_cancelling() || state.status.is_terminal()
}

fn apply_cancel_to_state(state: &mut TaskState) -> bool {
    if state.status != TaskStatus::Cancelling
        && state.status.can_transition_to(TaskStatus::Cancelling)
    {
        let _ = state.transition_to(TaskStatus::Cancelling);
        state.error = Some("Task cancelled".to_string());
        true
    } else {
        false
    }
}

fn apply_force_cancel_to_state(state: &mut TaskState) -> bool {
    if state.status != TaskStatus::Cancelled {
        let _ = state.transition_to(TaskStatus::Cancelled);
        state.status = TaskStatus::Cancelled;
        state.error = Some("Task force cancelled by user".to_string());
        true
    } else {
        false
    }
}

impl TaskContext {
    fn should_skip_progress_update(&self, state: &TaskState) -> bool {
        should_skip_progress_update_state(self.is_cancelled(), state)
    }

    pub fn with_sub_task(&self, key: &str) -> Self {
        let mut new_ctx = self.clone();
        new_ctx.sub_task_key = Some(key.to_string());
        new_ctx
    }

    pub async fn init_sub_tasks(&self, tasks: Vec<crate::core::task::state::SubTaskState>) {
        if self.is_cancelled() {
            return;
        }
        let mut state = self.state.write().await;
        if self.should_skip_progress_update(&state) {
            return;
        }
        state.progress.sub_tasks = tasks;
        state.updated_at = chrono::Utc::now().timestamp();
        *self.last_emit_time.write().await = tokio::time::Instant::now();
        self.manager.emit_state(&state).await;
    }

    pub async fn append_sub_tasks(&self, mut tasks: Vec<crate::core::task::state::SubTaskState>) {
        if self.is_cancelled() {
            return;
        }
        let mut state = self.state.write().await;
        if self.should_skip_progress_update(&state) {
            return;
        }
        for task in tasks.drain(..) {
            if !state.progress.sub_tasks.iter().any(|s| s.key == task.key) {
                state.progress.sub_tasks.push(task);
            }
        }
        state.updated_at = chrono::Utc::now().timestamp();
        *self.last_emit_time.write().await = tokio::time::Instant::now();
        self.manager.emit_state(&state).await;
    }

    pub async fn update_progress(&self, current: u64, total: u64, detail: &str) {
        if self.is_cancelled() {
            return;
        }
        let mut state = self.state.write().await;
        if self.should_skip_progress_update(&state) {
            return;
        }

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
        
        let mut should_emit = false;
        let now = tokio::time::Instant::now();
        {
            let mut last_emit = self.last_emit_time.write().await;
            if current == total || now.duration_since(*last_emit).as_millis() >= 200 {
                should_emit = true;
                *last_emit = now;
            }
        }

        if should_emit {
            self.manager.emit_state(&state).await;
        }
    }

    pub async fn update_download_metrics(&self, speed: u64, remaining_files: u32) {
        if self.is_cancelled() {
            return;
        }
        let mut state = self.state.write().await;
        if self.should_skip_progress_update(&state) {
            return;
        }
        state.progress.speed = speed;
        state.progress.remaining_files = remaining_files;
        state.updated_at = chrono::Utc::now().timestamp();
        
        let mut should_emit = false;
        let now = tokio::time::Instant::now();
        {
            let mut last_emit = self.last_emit_time.write().await;
            if remaining_files == 0 || now.duration_since(*last_emit).as_millis() >= 400 {
                should_emit = true;
                *last_emit = now;
            }
        }

        if should_emit {
            self.manager.emit_state(&state).await;
        }
    }

    pub async fn set_total_steps(&self, total_steps: u32) {
        if self.is_cancelled() {
            return;
        }
        let mut state = self.state.write().await;
        if self.should_skip_progress_update(&state) {
            return;
        }
        state.progress.total_steps = total_steps;
        state.updated_at = chrono::Utc::now().timestamp();
        *self.last_emit_time.write().await = tokio::time::Instant::now();
        self.manager.emit_state(&state).await;
    }

    pub async fn next_step(&self, detail: &str) {
        if self.is_cancelled() {
            return;
        }
        let mut state = self.state.write().await;
        if self.should_skip_progress_update(&state) {
            return;
        }
        if state.progress.step < state.progress.total_steps {
            state.progress.step += 1;
        }
        state.progress.current = 0;
        state.progress.detail = detail.to_string();
        state.updated_at = chrono::Utc::now().timestamp();
        *self.last_emit_time.write().await = tokio::time::Instant::now();
        self.manager.emit_state(&state).await;
    }

    pub async fn set_step(&self, step: u32, total_steps: u32, detail: &str) {
        if self.is_cancelled() {
            return;
        }
        let mut state = self.state.write().await;
        if self.should_skip_progress_update(&state) {
            return;
        }
        state.progress.step = step;
        state.progress.total_steps = total_steps;
        state.progress.current = 0;
        state.progress.detail = detail.to_string();
        state.updated_at = chrono::Utc::now().timestamp();
        *self.last_emit_time.write().await = tokio::time::Instant::now();
        self.manager.emit_state(&state).await;
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel_token.is_cancelled()
    }

    pub async fn wait_cancelled(&self) {
        self.cancel_token.cancelled().await
    }

    /// Races an async operation against the task's cancellation token so
    /// in-flight network requests abort promptly when the user cancels.
    pub async fn cancellable<F, T>(&self, fut: F) -> Result<T, TaskError>
    where
        F: std::future::Future<Output = T>,
    {
        if self.is_cancelled() {
            return Err(TaskError::ExecutionError("Task cancelled".to_string()));
        }
        tokio::select! {
            biased;
            _ = self.cancel_token.cancelled() => {
                Err(TaskError::ExecutionError("Task cancelled".to_string()))
            }
            res = fut => Ok(res),
        }
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
        if state.status.is_terminal() {
            return;
        }
        if let Ok(val) = serde_json::to_value(data) {
            state.context_data = Some(val);
        }
        if self.is_cancelled() || state.status.is_cancelling() {
            let _ = self.manager.db.save_task(&state).await;
            return;
        }
        *self.last_emit_time.write().await = tokio::time::Instant::now();
        self.manager.emit_state(&state).await;
    }
}

// A generic trait for any executable task
#[allow(clippy::double_must_use)]
#[async_trait::async_trait]
pub trait ExecutableTask: Send + Sync {
    #[allow(clippy::double_must_use)]
    async fn execute(&self, ctx: TaskContext) -> Result<(), TaskError>;
}

pub struct InstanceStartupGuard {
    instance_id: String,
    starting_instances: Arc<std::sync::RwLock<HashSet<String>>>,
}

impl Drop for InstanceStartupGuard {
    fn drop(&mut self) {
        if let Ok(mut set) = self.starting_instances.write() {
            set.remove(&self.instance_id);
        }
    }
}

#[derive(Clone)]
pub struct TaskWorkerHandle {
    pub cancel_token: Arc<CancellationToken>,
    pub done_rx: tokio::sync::watch::Receiver<bool>,
    pub instance_id: Option<String>,
    pub task_type: TaskType,
    pub state: Arc<RwLock<TaskState>>,
}

pub struct TaskManager {
    db: TaskDatabase,
    app_handle: AppHandle,
    active_tasks: Arc<RwLock<HashMap<String, Arc<TaskWorkerHandle>>>>,
    starting_instances: Arc<std::sync::RwLock<HashSet<String>>>,
    instance_locks: Arc<tokio::sync::Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>>,
    task_locks: Arc<tokio::sync::Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>>,
}

impl Clone for TaskManager {
    fn clone(&self) -> Self {
        Self {
            db: self.db.clone(),
            app_handle: self.app_handle.clone(),
            active_tasks: self.active_tasks.clone(),
            starting_instances: self.starting_instances.clone(),
            instance_locks: self.instance_locks.clone(),
            task_locks: self.task_locks.clone(),
        }
    }
}

impl TaskManager {
    pub async fn new(app_handle: AppHandle, db: TaskDatabase) -> Self {
        let manager = Self {
            db,
            app_handle,
            active_tasks: Arc::new(RwLock::new(HashMap::new())),
            starting_instances: Arc::new(std::sync::RwLock::new(HashSet::new())),
            instance_locks: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            task_locks: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        };
        manager.recover_stale_instances().await;
        manager
    }

    pub async fn get_instance_lock(&self, instance_id: &str) -> Arc<tokio::sync::Mutex<()>> {
        crate::core::drop_install::get_instance_lock(instance_id)
    }

    pub async fn is_instance_busy(&self, instance_id: &str) -> bool {
        let active = self.active_tasks.read().await;
        active
            .values()
            .any(|h| h.instance_id.as_deref() == Some(instance_id))
    }

    pub async fn get_task_lock(&self, id: &str) -> Arc<tokio::sync::Mutex<()>> {
        let mut locks = self.task_locks.lock().await;
        locks
            .entry(id.to_string())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    }

    pub async fn recover_stale_instances(&self) {
        let base_dir = crate::core::mojang::get_minecraft_base();
        let versions_dir = base_dir.join("versions");
        recover_stale_instances_internal(&versions_dir, &self.db, &self.active_tasks).await;
    }

    pub fn start_instance_install(&self, instance_id: &str) -> InstanceStartupGuard {
        if let Ok(mut set) = self.starting_instances.write() {
            set.insert(instance_id.to_string());
        }
        InstanceStartupGuard {
            instance_id: instance_id.to_string(),
            starting_instances: self.starting_instances.clone(),
        }
    }

    pub fn is_instance_starting(&self, instance_id: &str) -> bool {
        self.starting_instances
            .read()
            .map(|set| set.contains(instance_id))
            .unwrap_or(false)
    }

    pub fn task_context<'a>(&'a self, tasks: &'a [TaskState]) -> crate::core::manager::InstanceTaskContext<'a> {
        crate::core::manager::InstanceTaskContext {
            starting_instances: Some(&self.starting_instances),
            active_tasks: Some(&self.active_tasks),
            tasks,
            running_instances: None,
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

    pub async fn wait_for_instance(
        &self,
        instance_id: &str,
        exclude_task_id: Option<&str>,
        cancel_token: &CancellationToken,
    ) {
        loop {
            if cancel_token.is_cancelled() {
                break;
            }
            let is_running = {
                let mut running = false;
                if let Ok(all_tasks) = self.db.load_all_tasks().await {
                    for task in all_tasks {
                        if let Some(exclude) = exclude_task_id {
                            if task.id == exclude {
                                continue;
                            }
                        }
                        if task.status.is_active()
                            && task.task_type.instance_id().as_deref() == Some(instance_id)
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
            tokio::select! {
                _ = tokio::time::sleep(tokio::time::Duration::from_secs(1)) => {},
                _ = cancel_token.cancelled() => break,
            }
        }
    }

    pub async fn spawn_task<T: ExecutableTask + 'static>(
        &self,
        task_type: TaskType,
        executable: T,
    ) -> Result<String, TaskError> {
        // Prevent duplicate tasks by consulting memory active_tasks first (including tombstones)
        {
            let active = self.active_tasks.read().await;
            for handle in active.values() {
                if task_type.conflicts_with(&handle.task_type) {
                    return Err(TaskError::Database(format!(
                        "CONFLICTING_TASK:{}",
                        handle.task_type.name()
                    )));
                }
            }
        }

        // Consult database task history
        if let Ok(all_tasks) = self.db.load_all_tasks().await {
            for task in all_tasks {
                if task_type.conflicts_with(&task.task_type) {
                    if task.status.is_active() {
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
        let task_lock = self.get_task_lock(&id).await;
        let _task_guard = task_lock.lock().await;

        // If a previous worker for this task ID is still active, signal cancellation
        // and await its full completion before spawning the replacement worker.
        let existing_handle = self.active_tasks.read().await.get(&id).cloned();
        if let Some(handle) = existing_handle {
            tracing::info!(
                "Awaiting completion of previous worker for task {} before spawning replacement",
                id
            );
            handle.cancel_token.cancel();
            let mut rx = handle.done_rx.clone();
            let timeout_duration = std::time::Duration::from_secs(15);
            let wait_res = tokio::time::timeout(timeout_duration, async {
                while !*rx.borrow() {
                    if rx.changed().await.is_err() {
                        break;
                    }
                }
            })
            .await;

            if wait_res.is_err() {
                tracing::warn!(
                    "Timed out waiting for previous worker for task {} to stop after cancellation",
                    id
                );
                return Err(TaskError::ExecutionError(format!(
                    "Timed out waiting for previous task worker {} to stop. Please try again.",
                    id
                )));
            }
        }

        let instance_id = task_type.instance_id();

        // Ensure any in-progress cleanup for the target instance is completed before spawning
        if let Some(ref inst_id) = instance_id {
            let instance_lock = self.get_instance_lock(inst_id).await;
            let _guard = instance_lock.lock().await;
        }

        let state = if let Ok(Some(existing_state)) = self.db.get_task(&id).await {
            // Reusing an existing task, preserve its ID and context_data
            let mut state = TaskState::new(id.clone(), task_type);
            state.context_data = existing_state.context_data;
            state
        } else {
            TaskState::new(id.clone(), task_type)
        };

        self.emit_state(&state).await;

        let task_type_for_handle = state.task_type.clone();
        let state_arc = Arc::new(RwLock::new(state));
        let cancel_token = CancellationToken::new();
        let task_token = Arc::new(cancel_token.clone());
        let (done_tx, done_rx) = tokio::sync::watch::channel(false);
        let worker_handle = Arc::new(TaskWorkerHandle {
            cancel_token: task_token.clone(),
            done_rx,
            instance_id,
            task_type: task_type_for_handle,
            state: state_arc.clone(),
        });

        self.active_tasks
            .write()
            .await
            .insert(id.clone(), worker_handle);

        let manager = self.clone();
        let id_clone = id.clone();

        let ctx = TaskContext {
            id: id.clone(),
            app_handle: self.app_handle.clone(),
            cancel_token,
            manager: manager.clone(),
            state: state_arc.clone(),
            sub_task_key: None,
            last_emit_time: Arc::new(RwLock::new(tokio::time::Instant::now())),
        };

        use futures_util::FutureExt;
        use tracing::Instrument;
        let span = tracing::info_span!("task", task_id = %id_clone);

        tokio::spawn(async move {
            struct TaskSpawnGuard {
                done_tx: Option<tokio::sync::watch::Sender<bool>>,
                task_id: String,
                active_tasks: Arc<RwLock<HashMap<String, Arc<TaskWorkerHandle>>>>,
                task_token: Arc<CancellationToken>,
                finished: bool,
            }
            impl TaskSpawnGuard {
                fn mark_done(&mut self) {
                    if let Some(tx) = self.done_tx.take() {
                        let _ = tx.send(true);
                    }
                }
            }
            impl Drop for TaskSpawnGuard {
                fn drop(&mut self) {
                    self.mark_done();
                    if !self.finished {
                        let active_tasks = self.active_tasks.clone();
                        let task_id = self.task_id.clone();
                        let task_token = self.task_token.clone();
                        tokio::spawn(async move {
                            let mut active = active_tasks.write().await;
                            if active.get(&task_id).is_some_and(|h| Arc::ptr_eq(&h.cancel_token, &task_token)) {
                                active.remove(&task_id);
                            }
                        });
                    }
                }
            }
            let mut spawn_guard = TaskSpawnGuard {
                done_tx: Some(done_tx),
                task_id: id_clone.clone(),
                active_tasks: manager.active_tasks.clone(),
                task_token: task_token.clone(),
                finished: false,
            };

            let start_cancelled = {
                let mut state = state_arc.write().await;
                if task_token.is_cancelled() || state.status != TaskStatus::Pending {
                    true
                } else {
                    let _ = state.transition_to(TaskStatus::Running);
                    manager.emit_state(&state).await;
                    false
                }
            };

            let exec_res = if start_cancelled {
                Ok(Err(TaskError::ExecutionError("Task cancelled".to_string())))
            } else {
                std::panic::AssertUnwindSafe(executable.execute(ctx))
                    .catch_unwind()
                    .await
            };

            let res: Result<(), TaskError> = match exec_res {
                Ok(r) => r,
                Err(panic_err) => {
                    let msg = if let Some(s) = panic_err.downcast_ref::<&str>() {
                        s.to_string()
                    } else if let Some(s) = panic_err.downcast_ref::<String>() {
                        s.clone()
                    } else {
                        "Fatal: Task worker panicked unexpectedly".to_string()
                    };
                    tracing::error!("Task {} panicked: {}", id_clone, msg);
                    Err(TaskError::Fatal(msg))
                }
            };

            match res {
                Ok(_) => {
                    let mut completed_while_cancelling = false;
                    {
                        let mut state = state_arc.write().await;
                        if state.status == TaskStatus::Cancelled {
                            tracing::info!(
                                "Task {} finished after force-cancellation; preserving Cancelled state",
                                id_clone
                            );
                        } else if state.status == TaskStatus::Cancelling || task_token.is_cancelled() {
                            tracing::info!(
                                "Task {} completed while cancellation was requested; finalizing Cancelled state",
                                id_clone
                            );
                            let _ = state.transition_to(TaskStatus::Cancelled);
                            state.status = TaskStatus::Cancelled;
                            manager.emit_state(&state).await;
                            completed_while_cancelling = true;
                        } else {
                            tracing::info!("Task {} finished successfully (task_type={:?})", id_clone, state.task_type);
                            let _ = state.transition_to(TaskStatus::Completed);
                            state.progress.current = state.progress.total;
                            state.progress.detail = "Completed".to_string();
                            manager.emit_state(&state).await;
                        }
                    }
                    if completed_while_cancelling {
                        let task_type = state_arc.read().await.task_type.clone();
                        if let Some(instance_id) = task_type.instance_id() {
                            if !task_type.is_dependency() && crate::core::fs_utils::is_safe_single_name(&instance_id) {
                                let instance_dir = crate::core::mojang::get_minecraft_base()
                                    .join("versions")
                                    .join(&instance_id);
                                let config_path = instance_dir.join("dlml.json");
                                if tokio::fs::try_exists(&config_path).await.unwrap_or(false) {
                                    let _ = crate::core::launcher::update_instance_config_if(
                                        &instance_dir,
                                        |c| c.effective_status().is_busy(),
                                        |c| c.set_status(crate::core::launcher::InstanceStatus::Ready),
                                    ).await;
                                }
                            }
                        }
                    }
                    spawn_guard.mark_done();
                }
                Err(e) => {
                    let is_replaced_by_retry = manager
                        .active_tasks
                        .read()
                        .await
                        .get(&id_clone)
                        .is_some_and(|h| !Arc::ptr_eq(&h.cancel_token, &task_token) && !h.cancel_token.is_cancelled());

                    if is_replaced_by_retry {
                        spawn_guard.finished = true;
                        return;
                    }

                    if task_token.is_cancelled() {
                        {
                            let mut state = state_arc.write().await;
                            if state.status != TaskStatus::Cancelled && state.status != TaskStatus::Cancelling {
                                // Mark task as Cancelling while cleanup is proceeding
                                let _ = state.transition_to(TaskStatus::Cancelling);
                                state.error = Some("Task cancelled".to_string());
                                manager.emit_state(&state).await;
                            }
                        }

                        let task_type = state_arc.read().await.task_type.clone();

                        // Clean up the cancelled instance directory safely after task execution has terminated
                        if let Some(instance_id) = task_type.instance_id() {
                            if crate::core::fs_utils::is_safe_single_name(&instance_id) {
                                let is_update_task = task_type.is_update();
                                let instance_dir = if task_type.is_dependency() {
                                    crate::core::mojang::get_dawnland_cache().join(&instance_id)
                                } else {
                                    crate::core::mojang::get_minecraft_base().join("versions").join(&instance_id)
                                };
                                let config_path = instance_dir.join("dlml.json");

                                // Acquire exclusive per-instance lock and startup guard before cleanup
                                let _startup_guard = manager.start_instance_install(&instance_id);
                                let instance_lock = manager.get_instance_lock(&instance_id).await;
                                let _instance_guard = instance_lock.lock().await;

                                // Confirm this worker's token is still the exact same token in active_tasks
                                let is_still_current = manager
                                    .active_tasks
                                    .read()
                                    .await
                                    .get(&id_clone)
                                    .is_some_and(|h| Arc::ptr_eq(&h.cancel_token, &task_token));

                                // Verify that no other active non-cancelled worker is operating on the same instance
                                let has_other_active_worker = manager
                                    .active_tasks
                                    .read()
                                    .await
                                    .iter()
                                    .any(|(tid, h)| {
                                        tid != &id_clone
                                            && h.instance_id.as_deref() == Some(&instance_id)
                                            && !h.cancel_token.is_cancelled()
                                    });

                                let should_cleanup_instance = if !is_still_current {
                                    tracing::warn!(
                                        "Cancelled worker for task {} was replaced or removed before cleanup. Skipping instance cleanup.",
                                        id_clone
                                    );
                                    false
                                } else if has_other_active_worker {
                                    tracing::warn!(
                                        "Another active non-cancelled task is operating on instance {}. Skipping cleanup for task {}.",
                                        instance_id,
                                        id_clone
                                    );
                                    false
                                } else {
                                    true
                                };

                                if should_cleanup_instance {
                                    // If it is an update task, it is an existing instance and MUST NOT be deleted!
                                    if is_update_task {
                                        tracing::warn!(
                                            "Instance {} is an existing instance being updated. Resetting status to Ready after cancellation.",
                                            instance_id
                                        );
                                        if tokio::fs::try_exists(&config_path).await.unwrap_or(false) {
                                            let _ = crate::core::launcher::update_instance_config(&instance_dir, |config| {
                                                config.set_status(crate::core::launcher::InstanceStatus::Ready);
                                            }).await;
                                            let _ = manager.app_handle.emit("instances-changed", ());
                                        }
                                    } else if task_type.is_core_installation() {
                                        tracing::info!(
                                            "Cleaning up cancelled fresh installation instance: {}",
                                            instance_id
                                        );
                                        if let Err(err) = tokio::fs::remove_dir_all(&instance_dir).await {
                                            if err.kind() != std::io::ErrorKind::NotFound {
                                                tracing::error!(
                                                    "Failed to remove cancelled instance directory {:?}: {}",
                                                    instance_dir,
                                                    err
                                                );
                                                let mut state = state_arc.write().await;
                                                state.error = Some(format!(
                                                    "Task cancelled (failed to clean up instance directory: {})",
                                                    err
                                                ));
                                                manager.emit_state(&state).await;

                                                // If removal failed and dlml.json still exists, ensure status is reset to Ready
                                                if tokio::fs::try_exists(&config_path).await.unwrap_or(false) {
                                                    let _ = crate::core::launcher::update_instance_config(&instance_dir, |config| {
                                                        config.set_status(crate::core::launcher::InstanceStatus::Ready);
                                                    }).await;
                                                    let _ = manager.app_handle.emit("instances-changed", ());
                                                }
                                            }
                                        } else {
                                            let _ = manager.app_handle.emit("instances-changed", ());
                                        }
                                    }
                                }
                            }
                        }

                        // Complete cancellation transition: Cancelling -> Cancelled
                        {
                            let mut state = state_arc.write().await;
                            if state.status != TaskStatus::Cancelled {
                                let _ = state.transition_to(TaskStatus::Cancelled);
                                state.status = TaskStatus::Cancelled;
                                manager.emit_state(&state).await;
                            }
                        }
                        spawn_guard.mark_done();
                    } else {
                        let error_msg = e.to_string();
                        let (instance_id_opt, task_type) = {
                            let mut state = state_arc.write().await;
                            let _ = state.transition_to(TaskStatus::Failed);
                            state.error = Some(error_msg);
                            manager.emit_state(&state).await;
                            (state.task_type.instance_id(), state.task_type.clone())
                        };

                        // On failure, do not delete the entire instance directory to allow retry/resume.
                        // Instead, reset the installing/updating flags back to Ready.
                        if let Some(instance_id) = instance_id_opt {
                            if crate::core::fs_utils::is_safe_single_name(&instance_id) {
                                let has_other_active_worker = manager
                                    .active_tasks
                                    .read()
                                    .await
                                    .iter()
                                    .any(|(tid, h)| {
                                        tid != &id_clone
                                            && h.instance_id.as_deref() == Some(&instance_id)
                                            && !h.cancel_token.is_cancelled()
                                    });

                                if !has_other_active_worker {
                                    spawn_guard.mark_done();
                                    let instance_dir = if task_type.is_dependency() {
                                        crate::core::mojang::get_dawnland_cache().join(&instance_id)
                                    } else {
                                        crate::core::mojang::get_minecraft_base().join("versions").join(&instance_id)
                                    };
                                    let config_path = instance_dir.join("dlml.json");
                                    if tokio::fs::try_exists(&config_path).await.unwrap_or(false) {
                                        let _ = crate::core::launcher::update_instance_config(&instance_dir, |config| {
                                            config.set_status(crate::core::launcher::InstanceStatus::Ready);
                                        }).await;
                                        let _ = manager.app_handle.emit("instances-changed", ());
                                    }
                                }
                            }
                        }
                    }
                }
            }

            spawn_guard.mark_done();
            spawn_guard.finished = true;

            let mut active = manager.active_tasks.write().await;
            if active
                .get(&id_clone)
                .is_some_and(|h| Arc::ptr_eq(&h.cancel_token, &task_token))
            {
                active.remove(&id_clone);
            }
            drop(active);

            let is_instance_mutating = {
                let state = state_arc.read().await;
                state.task_type.is_core_installation() || state.task_type.is_update()
            };
            if is_instance_mutating {
                let _ = manager.app_handle.emit("instances-changed", ());
            }
        }.instrument(span));

        Ok(id)
    }

    pub async fn cancel_task(&self, id: &str) -> Result<(), TaskError> {
        let handle = {
            let tasks = self.active_tasks.read().await;
            tasks.get(id).cloned()
        };
        if let Some(handle) = handle {
            handle.cancel_token.cancel();

            let instance_id_opt = handle.instance_id.clone();
            let task_type = handle.task_type.clone();

            // Transition shared in-memory state to Cancelling, persist to DB, and emit to frontend.
            // Terminal Cancelled state is emitted ONLY after the task worker finishes cleaning up!
            {
                let mut state = handle.state.write().await;
                if apply_cancel_to_state(&mut state) {
                    self.emit_state(&state).await;
                }
            }

            // Skip cancelling-status write for dependency tasks, which live in cache
            if task_type.is_dependency() {
                return Ok(());
            }

            if let Some(ref inst_id) = instance_id_opt {
                let is_install_or_update = task_type.is_core_installation() || task_type.is_update();
                let is_update = task_type.is_update();

                if is_install_or_update && crate::core::fs_utils::is_safe_single_name(inst_id) {
                    let mut done_rx = handle.done_rx.clone();
                    // Re-check whether the worker is still executing before persisting cancelling status
                    if !*done_rx.borrow() {
                        let base_dir = crate::core::mojang::get_minecraft_base();
                        let inst_dir = base_dir.join("versions").join(inst_id);
                        let config_path = inst_dir.join("dlml.json");
                        if tokio::fs::try_exists(&config_path).await.unwrap_or(false) {
                            let res = crate::core::launcher::update_instance_config_if(
                                &inst_dir,
                                |config| {
                                    // Re-check done_rx under the instance config lock:
                                    // if worker has finished, abort the status write.
                                    // Also abort if the instance config is already Ready or already Cancelling!
                                    if *done_rx.borrow() {
                                        return false;
                                    }
                                    let current_status = config.effective_status();
                                    current_status != crate::core::launcher::InstanceStatus::Ready
                                        && current_status != crate::core::launcher::InstanceStatus::CancellingUpdate
                                        && current_status != crate::core::launcher::InstanceStatus::CancellingInstall
                                },
                                |config| {
                                    let current_status = config.effective_status();
                                    if is_update || current_status == crate::core::launcher::InstanceStatus::Updating {
                                        config.set_status(crate::core::launcher::InstanceStatus::CancellingUpdate);
                                    } else {
                                        config.set_status(crate::core::launcher::InstanceStatus::CancellingInstall);
                                    }
                                },
                            ).await;
                            if res.is_ok() {
                                let _ = self.app_handle.emit("instances-changed", ());
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

    pub async fn force_cancel_task(&self, id: &str) -> Result<(), TaskError> {
        let handle = {
            let tasks = self.active_tasks.read().await;
            tasks.get(id).cloned()
        };

        if let Some(ref handle) = handle {
            // Signal cancellation while leaving tombstone in active_tasks for conflict checks
            handle.cancel_token.cancel();
            let mut state = handle.state.write().await;
            if apply_force_cancel_to_state(&mut state) {
                self.emit_state(&state).await;
            }
        } else if let Ok(Some(mut state)) = self.db.get_task(id).await {
            // Persist Cancelled status in DB and emit to frontend immediately
            if apply_force_cancel_to_state(&mut state) {
                self.emit_state(&state).await;
            }
        }

        if let Some(handle) = handle {
            let mut done_rx = handle.done_rx.clone();
            let grace_period = tokio::time::Duration::from_secs(5);

            // Wait for worker exit signal or until bounded grace period expires
            let wait_res = tokio::time::timeout(grace_period, async {
                while !*done_rx.borrow() {
                    if done_rx.changed().await.is_err() {
                        break;
                    }
                }
            })
            .await;

            if wait_res.is_err() {
                tracing::warn!(
                    "Grace period elapsed waiting for force-cancelled task {} worker to stop",
                    id
                );
            }

            // Remove tombstone from active_tasks after worker finished or grace period elapsed
            {
                let mut tasks = self.active_tasks.write().await;
                if tasks.get(id).is_some_and(|h| Arc::ptr_eq(&h.cancel_token, &handle.cancel_token)) {
                    tasks.remove(id);
                }
            }

            // Reset instance config to Ready if instance exists and config exists
            if let Some(ref inst_id) = handle.instance_id {
                if !handle.task_type.is_dependency() && crate::core::fs_utils::is_safe_single_name(inst_id) {
                    let instance_lock = self.get_instance_lock(inst_id).await;
                    let _instance_guard = instance_lock.lock().await;

                    let base_dir = crate::core::mojang::get_minecraft_base();
                    let inst_dir = base_dir.join("versions").join(inst_id);
                    let config_path = inst_dir.join("dlml.json");
                    if tokio::fs::try_exists(&config_path).await.unwrap_or(false) {
                        let _ = crate::core::launcher::update_instance_config(&inst_dir, |config| {
                            config.set_status(crate::core::launcher::InstanceStatus::Ready);
                        }).await;
                        let _ = self.app_handle.emit("instances-changed", ());
                    }
                }
            }
        } else {
            // No active handle in memory, check if DB state had an instance needing reset
            if let Ok(Some(state)) = self.db.get_task(id).await {
                if let Some(ref inst_id) = state.task_type.instance_id() {
                    if !state.task_type.is_dependency() && crate::core::fs_utils::is_safe_single_name(inst_id) {
                        let instance_lock = self.get_instance_lock(inst_id).await;
                        let _instance_guard = instance_lock.lock().await;

                        let base_dir = crate::core::mojang::get_minecraft_base();
                        let inst_dir = base_dir.join("versions").join(inst_id);
                        let config_path = inst_dir.join("dlml.json");
                        if tokio::fs::try_exists(&config_path).await.unwrap_or(false) {
                            let _ = crate::core::launcher::update_instance_config(&inst_dir, |config| {
                                config.set_status(crate::core::launcher::InstanceStatus::Ready);
                            }).await;
                            let _ = self.app_handle.emit("instances-changed", ());
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

pub async fn recover_stale_instances_internal(
    versions_dir: &std::path::Path,
    db: &TaskDatabase,
    active_tasks: &RwLock<HashMap<String, Arc<TaskWorkerHandle>>>,
) {
    if !tokio::fs::try_exists(versions_dir).await.unwrap_or(false) {
        return;
    }

    let mut entries = match tokio::fs::read_dir(versions_dir).await {
        Ok(entries) => entries,
        Err(e) => {
            tracing::warn!("Failed to read versions directory during instance recovery: {}", e);
            return;
        }
    };

    let all_tasks = db.load_all_tasks().await.unwrap_or_default();
    let active_tasks_guard = active_tasks.read().await;

    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if !entry.file_type().await.map(|ft| ft.is_dir()).unwrap_or(false) {
            continue;
        }

        let instance_id = entry.file_name().to_string_lossy().to_string();
        if !crate::core::fs_utils::is_safe_single_name(&instance_id) {
            continue;
        }

        let config_path = path.join("dlml.json");
        if !tokio::fs::try_exists(&config_path).await.unwrap_or(false) {
            continue;
        }

        let content = match tokio::fs::read_to_string(&config_path).await {
            Ok(c) => c,
            Err(_) => continue,
        };

        let config: crate::core::launcher::InstanceConfig = match serde_json::from_str(&content) {
            Ok(cfg) => cfg,
            Err(_) => continue,
        };

        if !config.effective_status().is_busy() {
            continue;
        }

        // Check if there is an active in-memory task targeting this instance
        let has_active_memory_task = active_tasks_guard.iter().any(|(_task_id, handle)| {
            !handle.cancel_token.is_cancelled()
                && handle.instance_id.as_deref() == Some(&instance_id)
        });

        if has_active_memory_task {
            continue;
        }

        // Find all tasks related to this instance in DB
        let mut related_tasks: Vec<&TaskState> = all_tasks
            .iter()
            .filter(|t| t.task_type.instance_id().as_deref() == Some(&instance_id))
            .collect();

        // Sort by updated_at descending to inspect the most recent task
        related_tasks.sort_by_key(|a| std::cmp::Reverse(a.updated_at));

        let latest_task = related_tasks.first().copied();

        let is_cancelled_fresh_install = latest_task.is_some_and(|t| {
            (t.status == TaskStatus::Cancelled || t.status == TaskStatus::Cancelling)
                && t.task_type.is_core_installation()
                && !t.task_type.is_update()
        });

        if is_cancelled_fresh_install {
            tracing::info!(
                "Startup recovery: removing leftover directory of cancelled fresh installation: {}",
                instance_id
            );
            if let Err(err) = tokio::fs::remove_dir_all(&path).await {
                tracing::warn!(
                    "Startup recovery: failed to remove cancelled instance dir {}: {}",
                    path.display(),
                    err
                );
            }
        } else {
            tracing::info!(
                "Startup recovery: resetting stale status for instance: {}",
                instance_id
            );
            let _ = crate::core::launcher::update_instance_config(&path, |cfg| {
                cfg.set_status(crate::core::launcher::InstanceStatus::Ready);
            })
            .await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_instance_startup_guard_lifecycle() {
        let starting = Arc::new(std::sync::RwLock::new(HashSet::new()));
        let id = "test_instance";
        {
            if let Ok(mut set) = starting.write() {
                set.insert(id.to_string());
            }
            let _guard = InstanceStartupGuard {
                instance_id: id.to_string(),
                starting_instances: starting.clone(),
            };
            assert!(starting.read().unwrap().contains(id));
        }
        assert!(!starting.read().unwrap().contains(id));
    }

    #[tokio::test]
    async fn test_recover_stale_instances_cleans_cancelled_fresh_install() {
        let temp = tempdir().expect("tempdir");
        let versions_dir = temp.path().join("versions");
        let inst_dir = versions_dir.join("test_fresh_cancelled");
        tokio::fs::create_dir_all(&inst_dir).await.expect("create inst dir");

        let config = crate::core::launcher::InstanceConfig {
            status: crate::core::launcher::InstanceStatus::Installing,
            ..Default::default()
        };
        let json = serde_json::to_string_pretty(&config).unwrap();
        tokio::fs::write(inst_dir.join("dlml.json"), json).await.expect("write dlml.json");

        let db = TaskDatabase::new(temp.path().join("tasks.db")).await.expect("db");
        let mut task = TaskState::new(
            "task_1".to_string(),
            TaskType::InstallVanilla {
                version_id: "1.20.1".to_string(),
                version_json_url: "http://example.com".to_string(),
                custom_instance_name: Some("test_fresh_cancelled".to_string()),
                is_dependency: None,
            },
        );
        task.status = TaskStatus::Cancelled;
        db.save_task(&task).await.expect("save task");

        let active_tasks = RwLock::new(HashMap::new());
        recover_stale_instances_internal(&versions_dir, &db, &active_tasks).await;

        assert!(!tokio::fs::try_exists(&inst_dir).await.unwrap_or(false));
    }

    #[tokio::test]
    async fn test_recover_stale_instances_resets_flags_for_update() {
        let temp = tempdir().expect("tempdir");
        let versions_dir = temp.path().join("versions");
        let inst_dir = versions_dir.join("test_update_cancelled");
        tokio::fs::create_dir_all(&inst_dir).await.expect("create inst dir");

        let config = crate::core::launcher::InstanceConfig {
            status: crate::core::launcher::InstanceStatus::Updating,
            ..Default::default()
        };
        let json = serde_json::to_string_pretty(&config).unwrap();
        tokio::fs::write(inst_dir.join("dlml.json"), json).await.expect("write dlml.json");

        let db = TaskDatabase::new(temp.path().join("tasks.db")).await.expect("db");
        let mut task = TaskState::new(
            "task_2".to_string(),
            TaskType::InstallModpack {
                zip_path: "test.zip".to_string(),
                instance_name: "test_update_cancelled".to_string(),
                is_update: true,
                project_id: None,
                server_id: None,
                pack_version_id: None,
                pack_file_name: None,
            },
        );
        task.status = TaskStatus::Cancelled;
        db.save_task(&task).await.expect("save task");

        let active_tasks = RwLock::new(HashMap::new());
        recover_stale_instances_internal(&versions_dir, &db, &active_tasks).await;

        // Directory must still exist!
        assert!(tokio::fs::try_exists(&inst_dir).await.unwrap_or(false));

        // Config must have status reset to Ready
        let content = tokio::fs::read_to_string(inst_dir.join("dlml.json")).await.unwrap();
        let loaded: crate::core::launcher::InstanceConfig = serde_json::from_str(&content).unwrap();
        assert_eq!(loaded.status, crate::core::launcher::InstanceStatus::Ready);
    }

    #[tokio::test]
    async fn test_recover_stale_instances_resets_flags_for_crashed_task() {
        let temp = tempdir().expect("tempdir");
        let versions_dir = temp.path().join("versions");
        let inst_dir = versions_dir.join("test_crashed");
        tokio::fs::create_dir_all(&inst_dir).await.expect("create inst dir");

        let config = crate::core::launcher::InstanceConfig {
            status: crate::core::launcher::InstanceStatus::Installing,
            ..Default::default()
        };
        let json = serde_json::to_string_pretty(&config).unwrap();
        tokio::fs::write(inst_dir.join("dlml.json"), json).await.expect("write dlml.json");

        let db = TaskDatabase::new(temp.path().join("tasks.db")).await.expect("db");
        let mut task = TaskState::new(
            "task_3".to_string(),
            TaskType::InstallVanilla {
                version_id: "1.20.1".to_string(),
                version_json_url: "http://example.com".to_string(),
                custom_instance_name: Some("test_crashed".to_string()),
                is_dependency: None,
            },
        );
        task.status = TaskStatus::Failed;
        db.save_task(&task).await.expect("save task");

        let active_tasks = RwLock::new(HashMap::new());
        recover_stale_instances_internal(&versions_dir, &db, &active_tasks).await;

        // Directory must still exist for retry/resume!
        assert!(tokio::fs::try_exists(&inst_dir).await.unwrap_or(false));

        let content = tokio::fs::read_to_string(inst_dir.join("dlml.json")).await.unwrap();
        let loaded: crate::core::launcher::InstanceConfig = serde_json::from_str(&content).unwrap();
        assert_eq!(loaded.status, crate::core::launcher::InstanceStatus::Ready);
    }

    #[tokio::test]
    async fn test_cancellation_status_transition_rules() {
        use tempfile::tempdir;

        let temp = tempdir().expect("tempdir");
        let inst_dir = temp.path().join("test_cancel_instance");
        tokio::fs::create_dir_all(&inst_dir).await.expect("create inst dir");

        // Case 1: Initial status is Updating -> cancel updates it to CancellingUpdate
        let mut config = crate::core::launcher::InstanceConfig {
            status: crate::core::launcher::InstanceStatus::Updating,
            ..Default::default()
        };
        crate::core::launcher::write_instance_config_atomic(&inst_dir.join("dlml.json"), &config).await.unwrap();

        // Simulate cancel logic for update task
        let is_update = true;
        let _ = crate::core::launcher::update_instance_config(&inst_dir, |cfg| {
            let current = cfg.effective_status();
            if current == crate::core::launcher::InstanceStatus::CancellingUpdate
                || current == crate::core::launcher::InstanceStatus::CancellingInstall
            {
                return;
            }
            if is_update || current == crate::core::launcher::InstanceStatus::Updating {
                cfg.set_status(crate::core::launcher::InstanceStatus::CancellingUpdate);
            } else {
                cfg.set_status(crate::core::launcher::InstanceStatus::CancellingInstall);
            }
        }).await.unwrap();

        let loaded = crate::core::launcher::update_instance_config(&inst_dir, |_| {}).await.unwrap();
        assert_eq!(loaded.status, crate::core::launcher::InstanceStatus::CancellingUpdate);

        // Case 2: Repeated cancellation call must PRESERVE CancellingUpdate and NOT overwrite to CancellingInstall
        let is_update = false;
        let _ = crate::core::launcher::update_instance_config(&inst_dir, |cfg| {
            let current = cfg.effective_status();
            if current == crate::core::launcher::InstanceStatus::CancellingUpdate
                || current == crate::core::launcher::InstanceStatus::CancellingInstall
            {
                return;
            }
            if is_update || current == crate::core::launcher::InstanceStatus::Updating {
                cfg.set_status(crate::core::launcher::InstanceStatus::CancellingUpdate);
            } else {
                cfg.set_status(crate::core::launcher::InstanceStatus::CancellingInstall);
            }
        }).await.unwrap();

        let loaded_again = crate::core::launcher::update_instance_config(&inst_dir, |_| {}).await.unwrap();
        assert_eq!(loaded_again.status, crate::core::launcher::InstanceStatus::CancellingUpdate);
    }

    #[tokio::test]
    async fn test_cancellation_worker_terminal_write_wins_over_cancelling_status() {
        use tempfile::tempdir;

        let temp = tempdir().expect("tempdir");
        let inst_dir = temp.path().join("test_terminal_wins");
        tokio::fs::create_dir_all(&inst_dir).await.expect("create inst dir");

        // The worker has finished and its terminal write set status to Ready
        let mut config = crate::core::launcher::InstanceConfig {
            status: crate::core::launcher::InstanceStatus::Ready,
            ..Default::default()
        };
        crate::core::launcher::write_instance_config_atomic(&inst_dir.join("dlml.json"), &config).await.unwrap();

        // Channel simulating finished worker
        let (done_tx, done_rx) = tokio::sync::watch::channel(false);
        done_tx.send(true).unwrap(); // worker is done!

        // If cancel_task attempts to update status when worker is done:
        let is_update = true;
        if !*done_rx.borrow() {
            let _ = crate::core::launcher::update_instance_config(&inst_dir, |cfg| {
                if *done_rx.borrow() {
                    return;
                }
                cfg.set_status(crate::core::launcher::InstanceStatus::CancellingUpdate);
            }).await;
        }

        let loaded = crate::core::launcher::update_instance_config(&inst_dir, |_| {}).await.unwrap();
        // Worker's terminal Ready status MUST be preserved!
        assert_eq!(loaded.status, crate::core::launcher::InstanceStatus::Ready);
    }

    #[test]
    fn test_task_types_eligible_for_instance_cancellation() {
        let vanilla = TaskType::InstallVanilla {
            version_id: "1.20.1".into(),
            version_json_url: "".into(),
            custom_instance_name: None,
            is_dependency: None,
        };
        assert!(vanilla.is_core_installation() || vanilla.is_update());

        let modpack_update = TaskType::InstallModpack {
            zip_path: "p.zip".into(),
            instance_name: "inst".into(),
            is_update: true,
            project_id: None,
            server_id: None,
            pack_version_id: None,
            pack_file_name: None,
        };
        assert!(modpack_update.is_core_installation() || modpack_update.is_update());
        assert!(modpack_update.is_update());

        let mod_download = TaskType::InstallMod {
            source: "modrinth".into(),
            project_id: "proj".into(),
            file_id: "file1".into(),
            mod_name: Some("testmod".into()),
            download_url: "url".into(),
            instance_id: Some("inst".into()),
            target_dir: None,
            dependencies: None,
            keep_both: None,
        };
        // Mod task has an instance_id, but must NOT be treated as core installation or update
        assert!(!(mod_download.is_core_installation() || mod_download.is_update()));
    }

    #[test]
    fn test_task_type_is_dependency() {
        let vanilla_dep = TaskType::InstallVanilla {
            version_id: "1.20.1".into(),
            version_json_url: "".into(),
            custom_instance_name: None,
            is_dependency: Some(true),
        };
        assert!(vanilla_dep.is_dependency());

        let vanilla_user = TaskType::InstallVanilla {
            version_id: "1.20.1".into(),
            version_json_url: "".into(),
            custom_instance_name: None,
            is_dependency: Some(false),
        };
        assert!(!vanilla_user.is_dependency());

        let vanilla_default = TaskType::InstallVanilla {
            version_id: "1.20.1".into(),
            version_json_url: "".into(),
            custom_instance_name: None,
            is_dependency: None,
        };
        assert!(!vanilla_default.is_dependency());

        let forge_dep = TaskType::InstallForge {
            mc_version: "1.20.1".into(),
            loader_version: "47.1.0".into(),
            loader_type: "forge".into(),
            custom_instance_name: "test".into(),
            is_dependency: Some(true),
        };
        assert!(forge_dep.is_dependency());

        let fabric_dep = TaskType::InstallFabric {
            mc_version: "1.20.1".into(),
            fabric_version: "0.14.21".into(),
            custom_instance_name: "test".into(),
            is_dependency: Some(true),
        };
        assert!(fabric_dep.is_dependency());
    }

    #[tokio::test]
    async fn test_cancellation_aborts_status_write_if_worker_done_under_lock() {
        use tempfile::tempdir;

        let temp = tempdir().expect("tempdir");
        let inst_dir = temp.path().join("test_abort_under_lock");
        tokio::fs::create_dir_all(&inst_dir).await.expect("create inst dir");

        let config = crate::core::launcher::InstanceConfig {
            status: crate::core::launcher::InstanceStatus::Ready,
            ..Default::default()
        };
        crate::core::launcher::write_instance_config_atomic(&inst_dir.join("dlml.json"), &config).await.unwrap();

        let (done_tx, done_rx) = tokio::sync::watch::channel(false);
        // Worker marks done before lock check
        let _ = done_tx.send(true);

        let res = crate::core::launcher::update_instance_config_if(
            &inst_dir,
            |_| !*done_rx.borrow(),
            |cfg| {
                cfg.set_status(crate::core::launcher::InstanceStatus::CancellingInstall);
            },
        ).await;

        assert!(res.is_err(), "Predicate must reject status update when worker is already done");
        let loaded = crate::core::launcher::update_instance_config(&inst_dir, |_| {}).await.unwrap();
        assert_eq!(loaded.status, crate::core::launcher::InstanceStatus::Ready);
    }

    #[tokio::test]
    async fn test_cancellation_aborts_status_write_if_instance_is_ready() {
        use tempfile::tempdir;

        let temp = tempdir().expect("tempdir");
        let inst_dir = temp.path().join("test_ready_preserved");
        tokio::fs::create_dir_all(&inst_dir).await.expect("create inst dir");

        let config = crate::core::launcher::InstanceConfig {
            status: crate::core::launcher::InstanceStatus::Ready,
            ..Default::default()
        };
        crate::core::launcher::write_instance_config_atomic(&inst_dir.join("dlml.json"), &config).await.unwrap();

        let (_done_tx, done_rx) = tokio::sync::watch::channel(false);

        let res = crate::core::launcher::update_instance_config_if(
            &inst_dir,
            |config| {
                if *done_rx.borrow() {
                    return false;
                }
                let current_status = config.effective_status();
                current_status != crate::core::launcher::InstanceStatus::Ready
                    && current_status != crate::core::launcher::InstanceStatus::CancellingUpdate
                    && current_status != crate::core::launcher::InstanceStatus::CancellingInstall
            },
            |cfg| {
                cfg.set_status(crate::core::launcher::InstanceStatus::CancellingUpdate);
            },
        ).await;

        assert!(res.is_err(), "Predicate must reject cancelling write when instance is already Ready");
        let loaded = crate::core::launcher::update_instance_config(&inst_dir, |_| {}).await.unwrap();
        assert_eq!(loaded.status, crate::core::launcher::InstanceStatus::Ready);
    }

    #[tokio::test]
    async fn test_conflicting_task_detection_includes_cancelling() {
        let update_task1 = TaskType::InstallModpack {
            zip_path: "pack1.zip".into(),
            instance_name: "my_instance".into(),
            is_update: true,
            project_id: None,
            server_id: None,
            pack_version_id: None,
            pack_file_name: None,
        };
        let update_task2 = TaskType::InstallModpack {
            zip_path: "pack2.zip".into(),
            instance_name: "my_instance".into(),
            is_update: true,
            project_id: None,
            server_id: None,
            pack_version_id: None,
            pack_file_name: None,
        };

        // They conflict
        assert!(update_task1.conflicts_with(&update_task2));

        // When previous task is Cancelling, it MUST be recognized as active
        let cancelling_status = TaskStatus::Cancelling;
        assert!(cancelling_status.is_active());
        assert!(cancelling_status.is_cancelling());
        assert!(!cancelling_status.is_terminal());
    }

    #[tokio::test]
    async fn test_cancellation_full_lifecycle_and_escape() {
        use tempfile::tempdir;

        let temp = tempdir().expect("tempdir");
        let inst_dir = temp.path().join("test_lifecycle_inst");
        tokio::fs::create_dir_all(&inst_dir).await.expect("create inst dir");

        let mut config = crate::core::launcher::InstanceConfig {
            status: crate::core::launcher::InstanceStatus::Updating,
            ..Default::default()
        };
        crate::core::launcher::write_instance_config_atomic(&inst_dir.join("dlml.json"), &config).await.unwrap();

        // 1. Task begins cancelling -> Instance transitions to CancellingUpdate
        let _ = crate::core::launcher::update_instance_config(&inst_dir, |cfg| {
            cfg.set_status(crate::core::launcher::InstanceStatus::CancellingUpdate);
        }).await.unwrap();

        let loaded = crate::core::launcher::update_instance_config(&inst_dir, |_| {}).await.unwrap();
        assert_eq!(loaded.status, crate::core::launcher::InstanceStatus::CancellingUpdate);

        // 2. Cancellation finishes (worker cleanup complete) -> Instance transitions to Ready
        let _ = crate::core::launcher::update_instance_config(&inst_dir, |cfg| {
            cfg.set_status(crate::core::launcher::InstanceStatus::Ready);
        }).await.unwrap();

        let loaded_final = crate::core::launcher::update_instance_config(&inst_dir, |_| {}).await.unwrap();
        assert_eq!(loaded_final.status, crate::core::launcher::InstanceStatus::Ready);
    }

    #[tokio::test]
    async fn test_cancelled_worker_transitions_to_cancelled_when_token_stale() {
        let mut state = TaskState::new(
            "test_stale_task".to_string(),
            TaskType::InstallVanilla {
                version_id: "1.20.1".into(),
                version_json_url: "".into(),
                custom_instance_name: Some("test_inst".into()),
                is_dependency: None,
            },
        );
        state.status = TaskStatus::Running;

        // When token is cancelled, transition_to(TaskStatus::Cancelling) first
        assert!(state.transition_to(TaskStatus::Cancelling).is_ok());
        assert_eq!(state.status, TaskStatus::Cancelling);

        // Even when token is stale or replaced (is_still_current = false),
        // the worker MUST complete the terminal transition to Cancelled
        assert!(state.transition_to(TaskStatus::Cancelled).is_ok());
        assert_eq!(state.status, TaskStatus::Cancelled);
        assert!(state.status.is_terminal());
    }

    #[tokio::test]
    async fn test_force_cancelled_worker_does_not_repersist_cancelling() {
        use tempfile::tempdir;

        let temp = tempdir().expect("tempdir");
        let db = TaskDatabase::new(temp.path().join("tasks.db")).await.expect("db");

        let mut state = TaskState::new(
            "task_force_cancelled".to_string(),
            TaskType::InstallVanilla {
                version_id: "1.20.1".into(),
                version_json_url: "".into(),
                custom_instance_name: Some("test_inst".into()),
                is_dependency: None,
            },
        );
        // Force-cancel already set status to Cancelled in DB
        state.status = TaskStatus::Cancelled;
        db.save_task(&state).await.expect("save task");

        // Worker checks DB status
        let is_already_cancelled = match db.get_task("task_force_cancelled").await {
            Ok(Some(db_state)) => db_state.status == TaskStatus::Cancelled,
            _ => false,
        };
        assert!(is_already_cancelled, "Worker must detect that task was already force-cancelled");

        // Worker skips transitioning to Cancelling
        if !is_already_cancelled {
            let _ = state.transition_to(TaskStatus::Cancelling);
        } else {
            let _ = state.transition_to(TaskStatus::Cancelled);
        }

        assert_eq!(state.status, TaskStatus::Cancelled);
    }

    #[tokio::test]
    async fn test_force_cancel_tombstone_fences_active_tasks() {
        let task_token = Arc::new(CancellationToken::new());
        let (_done_tx, done_rx) = tokio::sync::watch::channel(false);

        let task_type = TaskType::InstallVanilla {
            version_id: "1.20.1".into(),
            version_json_url: "".into(),
            custom_instance_name: Some("busy_instance".into()),
            is_dependency: None,
        };
        let handle = Arc::new(TaskWorkerHandle {
            cancel_token: task_token.clone(),
            done_rx,
            instance_id: Some("busy_instance".to_string()),
            task_type: task_type.clone(),
            state: Arc::new(RwLock::new(TaskState::new("busy_task".to_string(), task_type))),
        });

        let active_tasks: Arc<RwLock<HashMap<String, Arc<TaskWorkerHandle>>>> =
            Arc::new(RwLock::new(HashMap::new()));
        active_tasks
            .write()
            .await
            .insert("busy_task".to_string(), handle.clone());

        // Cancel the token (tombstone)
        handle.cancel_token.cancel();
        assert!(handle.cancel_token.is_cancelled());

        // While tombstone is in active_tasks, instance is busy
        let is_busy = active_tasks
            .read()
            .await
            .values()
            .any(|h| h.instance_id.as_deref() == Some("busy_instance"));
        assert!(is_busy, "Tombstone must fence off instance conflict checks");

        // Conflicting task must conflict with the tombstone
        let new_task_type = TaskType::InstallVanilla {
            version_id: "1.20.1".into(),
            version_json_url: "".into(),
            custom_instance_name: Some("busy_instance".into()),
            is_dependency: None,
        };
        let conflicts = active_tasks
            .read()
            .await
            .values()
            .any(|h| new_task_type.conflicts_with(&h.task_type));
        assert!(conflicts, "New task must conflict with tombstoned active worker");

        // Once worker finishes and tombstone is removed:
        active_tasks.write().await.remove("busy_task");
        let is_busy_after = active_tasks
            .read()
            .await
            .values()
            .any(|h| h.instance_id.as_deref() == Some("busy_instance"));
        assert!(!is_busy_after, "Instance must no longer be busy after tombstone removal");
    }

    #[tokio::test]
    async fn test_shared_handle_state_prevents_status_regression_after_cancel() {
        let task_type = TaskType::InstallFabric {
            mc_version: "1.20.1".into(),
            fabric_version: "0.15.11".into(),
            custom_instance_name: "fabric-test".into(),
            is_dependency: None,
        };
        let mut initial_state = TaskState::new("task-fabric-cancel".to_string(), task_type.clone());
        assert!(initial_state.transition_to(TaskStatus::Running).is_ok());

        let state_arc = Arc::new(RwLock::new(initial_state));
        let cancel_token = Arc::new(CancellationToken::new());
        let (_done_tx, done_rx) = tokio::sync::watch::channel(false);

        let handle = Arc::new(TaskWorkerHandle {
            cancel_token: cancel_token.clone(),
            done_rx,
            instance_id: Some("fabric-test".to_string()),
            task_type,
            state: state_arc.clone(),
        });

        // Before cancel: worker progress updates are allowed
        {
            let guard = state_arc.read().await;
            assert!(!should_skip_progress_update_state(cancel_token.is_cancelled(), &guard));
        }

        // Simulate cancel_task: cancel token + mutate shared handle.state
        handle.cancel_token.cancel();
        {
            let mut guard = handle.state.write().await;
            assert!(apply_cancel_to_state(&mut guard));
            // Repeated cancel must be idempotent
            assert!(!apply_cancel_to_state(&mut guard));
        }

        // Worker's TaskContext sees Cancelling immediately and blocks subsequent progress emits
        {
            let guard = state_arc.read().await;
            assert_eq!(guard.status, TaskStatus::Cancelling);
            assert!(should_skip_progress_update_state(cancel_token.is_cancelled(), &guard));
            assert!(should_skip_progress_update_state(false, &guard));
        }

        // Force-cancel transitions shared state directly to Cancelled and keeps blocking emits
        {
            let mut guard = handle.state.write().await;
            assert!(apply_force_cancel_to_state(&mut guard));
            assert_eq!(guard.status, TaskStatus::Cancelled);
            assert!(should_skip_progress_update_state(false, &guard));
        }
    }
}
