use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio_rusqlite::Connection;

/// A persisted crash history entry with optional AI analysis results.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrashHistoryEntry {
    pub id: i64,
    pub instance_id: String,
    pub instance_name: Option<String>,
    pub exit_code: Option<i32>,
    pub crash_summary: String,
    pub ai_cause: Option<String>,
    pub ai_solution: Option<String>,
    pub ai_actions: Option<String>,
    pub created_at: i64,
}

/// Input for saving a crash history entry (frontend -> backend).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrashHistoryInput {
    pub instance_id: String,
    pub instance_name: Option<String>,
    pub exit_code: Option<i32>,
    pub crash_summary: String,
    pub ai_cause: Option<String>,
    pub ai_solution: Option<String>,
    pub ai_actions: Option<String>,
}

#[derive(Clone)]
pub struct CrashHistoryDb {
    conn: Connection,
}

impl CrashHistoryDb {
    pub async fn new(db_path: PathBuf) -> Result<Self, String> {
        let conn = Connection::open(db_path)
            .await
            .map_err(|e| e.to_string())?;

        conn.call(|conn| -> rusqlite::Result<()> {
            conn.execute_batch(
                "PRAGMA busy_timeout = 5000;
                 CREATE TABLE IF NOT EXISTS crash_history (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    instance_id TEXT NOT NULL,
                    instance_name TEXT,
                    exit_code INTEGER,
                    crash_summary TEXT NOT NULL,
                    ai_cause TEXT,
                    ai_solution TEXT,
                    ai_actions TEXT,
                    created_at INTEGER NOT NULL
                 );",
            )?;
            Ok(())
        })
        .await
        .map_err(|e| e.to_string())?;

        Ok(Self { conn })
    }

    /// Insert a new crash history entry. Truncates crash_summary to 2000 chars.
    pub async fn save(&self, input: CrashHistoryInput) -> Result<i64, String> {
        let now = chrono::Local::now().timestamp();
        let summary: String = input.crash_summary.chars().take(2000).collect();
        self.conn
            .call(move |conn| -> rusqlite::Result<i64> {
                conn.execute(
                    "INSERT INTO crash_history
                        (instance_id, instance_name, exit_code, crash_summary, ai_cause, ai_solution, ai_actions, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    (
                        &input.instance_id,
                        &input.instance_name,
                        input.exit_code,
                        &summary,
                        &input.ai_cause,
                        &input.ai_solution,
                        &input.ai_actions,
                        now,
                    ),
                )?;
                Ok(conn.last_insert_rowid())
            })
            .await
            .map_err(|e| e.to_string())
    }

    /// Load crash history, optionally filtered by instance_id. Newest first.
    pub async fn get_history(
        &self,
        instance_id: Option<String>,
    ) -> Result<Vec<CrashHistoryEntry>, String> {
        self.conn
            .call(move |conn| -> rusqlite::Result<Vec<CrashHistoryEntry>> {
                fn map_row(row: &rusqlite::Row) -> rusqlite::Result<CrashHistoryEntry> {
                    Ok(CrashHistoryEntry {
                        id: row.get(0)?,
                        instance_id: row.get(1)?,
                        instance_name: row.get(2)?,
                        exit_code: row.get(3)?,
                        crash_summary: row.get(4)?,
                        ai_cause: row.get(5)?,
                        ai_solution: row.get(6)?,
                        ai_actions: row.get(7)?,
                        created_at: row.get(8)?,
                    })
                }

                let mut entries = Vec::new();
                if let Some(iid) = instance_id {
                    let mut stmt = conn.prepare(
                        "SELECT id, instance_id, instance_name, exit_code, crash_summary,
                                ai_cause, ai_solution, ai_actions, created_at
                         FROM crash_history WHERE instance_id = ?1 ORDER BY created_at DESC",
                    )?;
                    for row in stmt.query_map([&iid], map_row)? {
                        entries.push(row?);
                    }
                } else {
                    let mut stmt = conn.prepare(
                        "SELECT id, instance_id, instance_name, exit_code, crash_summary,
                                ai_cause, ai_solution, ai_actions, created_at
                         FROM crash_history ORDER BY created_at DESC",
                    )?;
                    for row in stmt.query_map([], map_row)? {
                        entries.push(row?);
                    }
                }
                Ok(entries)
            })
            .await
            .map_err(|e| e.to_string())
    }

    /// Delete a single entry by id.
    pub async fn delete(&self, id: i64) -> Result<(), String> {
        self.conn
            .call(move |conn| -> rusqlite::Result<()> {
                conn.execute("DELETE FROM crash_history WHERE id = ?1", [id])?;
                Ok(())
            })
            .await
            .map_err(|e| e.to_string())
    }

    /// Clear all entries, or only those for a specific instance.
    pub async fn clear(&self, instance_id: Option<String>) -> Result<(), String> {
        self.conn
            .call(move |conn| -> rusqlite::Result<()> {
                if let Some(iid) = instance_id {
                    conn.execute("DELETE FROM crash_history WHERE instance_id = ?1", [iid])?;
                } else {
                    conn.execute("DELETE FROM crash_history", [])?;
                }
                Ok(())
            })
            .await
            .map_err(|e| e.to_string())
    }
}

#[tauri::command]
pub async fn save_crash_history(
    input: CrashHistoryInput,
    db: tauri::State<'_, CrashHistoryDb>,
) -> Result<i64, String> {
    tracing::info!("Saving crash history for instance: {}", input.instance_id);
    db.save(input).await
}

#[tauri::command]
pub async fn get_crash_history(
    instance_id: Option<String>,
    db: tauri::State<'_, CrashHistoryDb>,
) -> Result<Vec<CrashHistoryEntry>, String> {
    db.get_history(instance_id).await
}

#[tauri::command]
pub async fn delete_crash_history(
    id: i64,
    db: tauri::State<'_, CrashHistoryDb>,
) -> Result<(), String> {
    tracing::info!("Deleting crash history entry: {}", id);
    db.delete(id).await
}

#[tauri::command]
pub async fn clear_crash_history(
    instance_id: Option<String>,
    db: tauri::State<'_, CrashHistoryDb>,
) -> Result<(), String> {
    db.clear(instance_id).await
}
