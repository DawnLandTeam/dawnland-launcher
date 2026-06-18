use super::state::{TaskState, TaskError};
use std::path::PathBuf;
use tokio_rusqlite::Connection;

#[derive(Clone)]
pub struct TaskDatabase {
    conn: Connection,
}

impl TaskDatabase {
    pub async fn new(db_path: PathBuf) -> Result<Self, TaskError> {
        let conn = Connection::open(db_path).await.map_err(|e| TaskError::Database(e.to_string()))?;
        
        // Initialize tables
        conn.call(|conn| -> ::rusqlite::Result<()> {
            conn.execute(
                "CREATE TABLE IF NOT EXISTS tasks (
                    id TEXT PRIMARY KEY,
                    task_type TEXT NOT NULL,
                    status TEXT NOT NULL,
                    progress_current INTEGER NOT NULL,
                    progress_total INTEGER NOT NULL,
                    progress_step INTEGER NOT NULL DEFAULT 1,
                    progress_total_steps INTEGER NOT NULL DEFAULT 1,
                    progress_detail TEXT NOT NULL,
                    error TEXT,
                    created_at INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL
                )",
                [],
            )?;
            Ok(())
        }).await.map_err(|e| TaskError::Database(e.to_string()))?;
        // Migration: add columns if they don't exist
        let _ = conn.call(|conn| -> ::rusqlite::Result<()> {
            let _ = conn.execute("ALTER TABLE tasks ADD COLUMN context_data TEXT", []);
            let _ = conn.execute("ALTER TABLE tasks ADD COLUMN progress_step INTEGER NOT NULL DEFAULT 1", []);
            let _ = conn.execute("ALTER TABLE tasks ADD COLUMN progress_total_steps INTEGER NOT NULL DEFAULT 1", []);
            Ok(())
        }).await;
        // Cleanup interrupted tasks (tasks that were running when the app was closed)
        conn.call(|conn| -> ::rusqlite::Result<()> {
            conn.execute(
                "UPDATE tasks SET status = ?1, error = ?2 WHERE status IN (?3, ?4, ?5)",
                (
                    "\"Failed\"",
                    "Task interrupted by application exit",
                    "\"Pending\"",
                    "\"Running\"",
                    "\"Paused\"",
                ),
            )?;
            Ok(())
        }).await.map_err(|e| TaskError::Database(e.to_string()))?;

        Ok(Self { conn })
    }

    pub async fn save_task(&self, state: &TaskState) -> Result<(), TaskError> {
        let state_clone = state.clone();
        self.conn.call(move |conn| -> ::rusqlite::Result<()> {
            conn.execute(
                "INSERT OR REPLACE INTO tasks 
                (id, task_type, status, progress_current, progress_total, progress_step, progress_total_steps, progress_detail, error, created_at, updated_at, context_data) 
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                (
                    &state_clone.id,
                    serde_json::to_string(&state_clone.task_type).unwrap_or_default(),
                    serde_json::to_string(&state_clone.status).unwrap_or_default(),
                    state_clone.progress.current as i64,
                    state_clone.progress.total as i64,
                    state_clone.progress.step as i64,
                    state_clone.progress.total_steps as i64,
                    &state_clone.progress.detail,
                    &state_clone.error,
                    state_clone.created_at,
                    state_clone.updated_at,
                    state_clone.context_data.as_ref().map(|d| serde_json::to_string(d).unwrap_or_default()),
                ),
            )?;
            Ok(())
        }).await.map_err(|e| TaskError::Database(e.to_string()))?;
        Ok(())
    }

    pub async fn load_all_tasks(&self) -> Result<Vec<TaskState>, TaskError> {
        self.conn.call(|conn| -> ::rusqlite::Result<Vec<TaskState>> {
            let mut stmt = conn.prepare(
                "SELECT id, task_type, status, progress_current, progress_total, progress_step, progress_total_steps, progress_detail, error, created_at, updated_at, context_data FROM tasks ORDER BY created_at DESC"
            )?;
            let task_iter = stmt.query_map([], |row| {
                let id: String = row.get(0)?;
                let task_type_str: String = row.get(1)?;
                let status_str: String = row.get(2)?;
                let current: i64 = row.get(3)?;
                let total: i64 = row.get(4)?;
                let step: i64 = row.get(5)?;
                let total_steps: i64 = row.get(6)?;
                let detail: String = row.get(7)?;
                let error: Option<String> = row.get(8)?;
                let created_at: i64 = row.get(9)?;
                let updated_at: i64 = row.get(10)?;
                
                // Read context_data, it might be missing if older schema
                let context_data_str: Option<String> = row.get(11).unwrap_or(None);
                let context_data = context_data_str.and_then(|s| serde_json::from_str(&s).ok());

                let task_type = serde_json::from_str(&task_type_str).unwrap_or(super::state::TaskType::Generic { name: "Unknown".into() });
                let status = serde_json::from_str(&status_str).unwrap_or(super::state::TaskStatus::Failed);

                Ok(TaskState {
                    id,
                    task_type,
                    status,
                    progress: super::state::TaskProgress {
                        current: current as u64,
                        total: total as u64,
                        step: step as u32,
                        total_steps: total_steps as u32,
                        detail,
                        speed: 0,
                        remaining_files: 0,
                        sub_tasks: Vec::new(),
                    },
                    error,
                    created_at,
                    updated_at,
                    context_data,
                })
            })?;
            
            let mut tasks = Vec::new();
            for task in task_iter {
                tasks.push(task?);
            }
            Ok(tasks)
        }).await.map_err(|e| TaskError::Database(e.to_string()))
    }
    pub async fn get_task(&self, id: &str) -> Result<Option<TaskState>, TaskError> {
        let tasks = self.load_all_tasks().await?;
        Ok(tasks.into_iter().find(|t| t.id == id))
    }

    pub async fn clear_history(&self) -> Result<(), TaskError> {
        self.conn.call(|conn| -> ::rusqlite::Result<()> {
            conn.execute(
                "DELETE FROM tasks WHERE status IN ('\"Completed\"', '\"Failed\"', '\"Cancelled\"')",
                [],
            )?;
            Ok(())
        })
        .await
        .map_err(|e| TaskError::Database(e.to_string()))
    }

    pub async fn delete_task(&self, id: String) -> Result<(), TaskError> {
        self.conn.call(move |conn| -> ::rusqlite::Result<()> {
            conn.execute(
                "DELETE FROM tasks WHERE id = ?1",
                (id,),
            )?;
            Ok(())
        })
        .await
        .map_err(|e| TaskError::Database(e.to_string()))
    }
}
