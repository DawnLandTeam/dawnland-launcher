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
    #[serde(default)]
    pub id: Option<i64>,
    pub instance_id: String,
    #[serde(default)]
    pub instance_name: Option<String>,
    #[serde(default)]
    pub exit_code: Option<i32>,
    pub crash_summary: String,
    #[serde(default)]
    pub ai_cause: Option<String>,
    #[serde(default)]
    pub ai_solution: Option<String>,
    #[serde(default)]
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

    /// Insert or update a crash history entry. Truncates crash_summary to 2000 chars.
    /// Also enforces a retention policy of keeping at most 50 entries per instance.
    pub async fn save(&self, input: CrashHistoryInput) -> Result<i64, String> {
        let now = chrono::Local::now().timestamp();
        let summary: String = input.crash_summary.chars().take(2000).collect();
        self.conn
            .call(move |conn| -> rusqlite::Result<i64> {
                // 1. If an explicit ID is supplied, update that entry with any newly provided fields
                if let Some(target_id) = input.id {
                    let mut stmt = conn.prepare(
                        "UPDATE crash_history
                         SET instance_name = COALESCE(?1, instance_name),
                             exit_code = COALESCE(?2, exit_code),
                             crash_summary = CASE WHEN ?3 != '' THEN ?3 ELSE crash_summary END,
                             ai_cause = COALESCE(?4, ai_cause),
                             ai_solution = COALESCE(?5, ai_solution),
                             ai_actions = COALESCE(?6, ai_actions)
                         WHERE id = ?7",
                    )?;
                    let rows = stmt.execute(rusqlite::params![
                        &input.instance_name,
                        input.exit_code,
                        &summary,
                        &input.ai_cause,
                        &input.ai_solution,
                        &input.ai_actions,
                        target_id,
                    ])?;
                    if rows > 0 {
                        return Ok(target_id);
                    }
                }

                // 2. Deduplication fallback: Check if this same crash for this instance was already
                // auto-recorded recently (within 120s). If so, update the existing entry with AI details.
                let recent_cutoff = now - 120;
                let existing_id: Option<i64> = conn
                    .query_row(
                        "SELECT id FROM crash_history
                         WHERE instance_id = ?1 AND crash_summary = ?2 AND created_at >= ?3
                         ORDER BY id DESC LIMIT 1",
                        rusqlite::params![&input.instance_id, &summary, recent_cutoff],
                        |row| row.get(0),
                    )
                    .ok();

                if let Some(existing_id) = existing_id {
                    conn.execute(
                        "UPDATE crash_history
                         SET ai_cause = COALESCE(?1, ai_cause),
                             ai_solution = COALESCE(?2, ai_solution),
                             ai_actions = COALESCE(?3, ai_actions)
                         WHERE id = ?4",
                        rusqlite::params![
                            &input.ai_cause,
                            &input.ai_solution,
                            &input.ai_actions,
                            existing_id,
                        ],
                    )?;
                    return Ok(existing_id);
                }

                // 3. Otherwise, insert a new crash record
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
                let row_id = conn.last_insert_rowid();

                // 4. Retention policy: Keep only the newest 50 entries per instance
                conn.execute(
                    "DELETE FROM crash_history
                     WHERE instance_id = ?1
                       AND id NOT IN (
                           SELECT id FROM crash_history
                           WHERE instance_id = ?1
                           ORDER BY created_at DESC, id DESC
                           LIMIT 50
                       )",
                    rusqlite::params![&input.instance_id],
                )?;

                Ok(row_id)
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
