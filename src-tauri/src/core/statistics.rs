use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio_rusqlite::Connection;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceStats {
    pub instance_id: String,
    pub launch_count: i64,
    pub play_time_seconds: i64,
    pub last_played_at: Option<i64>,
}

#[derive(Clone)]
pub struct StatisticsDb {
    conn: Connection,
}

impl StatisticsDb {
    pub async fn new(db_path: PathBuf) -> Result<Self, String> {
        let conn = Connection::open(db_path)
            .await
            .map_err(|e| e.to_string())?;

        conn.call(|conn| -> rusqlite::Result<()> {
            conn.execute_batch(
                "PRAGMA busy_timeout = 5000;
                 CREATE TABLE IF NOT EXISTS instance_stats (
                    instance_id TEXT PRIMARY KEY,
                    launch_count INTEGER NOT NULL DEFAULT 0,
                    play_time_seconds INTEGER NOT NULL DEFAULT 0,
                    last_played_at INTEGER
                 );",
            )?;
            Ok(())
        })
        .await
        .map_err(|e| e.to_string())?;

        Ok(Self { conn })
    }

    pub async fn record_launch(&self, instance_id: String) -> Result<(), String> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        self.conn
            .call(move |conn| -> rusqlite::Result<()> {
                conn.execute(
                    "INSERT INTO instance_stats (instance_id, launch_count, play_time_seconds, last_played_at)
                     VALUES (?1, 1, 0, ?2)
                     ON CONFLICT(instance_id) DO UPDATE SET
                        launch_count = launch_count + 1,
                        last_played_at = ?2",
                    rusqlite::params![instance_id, now],
                )?;
                Ok(())
            })
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn add_playtime(&self, instance_id: String, seconds: i64) -> Result<(), String> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        self.conn
            .call(move |conn| -> rusqlite::Result<()> {
                conn.execute(
                    "INSERT INTO instance_stats (instance_id, launch_count, play_time_seconds, last_played_at)
                     VALUES (?1, 0, ?2, ?3)
                     ON CONFLICT(instance_id) DO UPDATE SET
                        play_time_seconds = play_time_seconds + ?2",
                    rusqlite::params![instance_id, seconds, now],
                )?;
                Ok(())
            })
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn get_instance_stats(&self, instance_id: String) -> Result<Option<InstanceStats>, String> {
        self.conn
            .call(move |conn| -> rusqlite::Result<Option<InstanceStats>> {
                let mut stmt = conn.prepare("SELECT instance_id, launch_count, play_time_seconds, last_played_at FROM instance_stats WHERE instance_id = ?1")?;
                let mut rows = stmt.query(rusqlite::params![instance_id])?;

                if let Some(row) = rows.next()? {
                    Ok(Some(InstanceStats {
                        instance_id: row.get(0)?,
                        launch_count: row.get(1)?,
                        play_time_seconds: row.get(2)?,
                        last_played_at: row.get(3)?,
                    }))
                } else {
                    Ok(None)
                }
            })
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn delete_stats(&self, instance_id: String) -> Result<(), String> {
        self.conn
            .call(move |conn| -> rusqlite::Result<()> {
                conn.execute(
                    "DELETE FROM instance_stats WHERE instance_id = ?1",
                    rusqlite::params![instance_id],
                )?;
                Ok(())
            })
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn rename_stats(&self, old_id: String, new_id: String) -> Result<(), String> {
        if old_id == new_id {
            return Ok(());
        }
        self.conn
            .call(move |conn| -> rusqlite::Result<()> {
                let tx = conn.transaction()?;

                let old_row: Option<(i64, i64, Option<i64>)> = tx
                    .query_row(
                        "SELECT launch_count, play_time_seconds, last_played_at FROM instance_stats WHERE instance_id = ?1",
                        rusqlite::params![&old_id],
                        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                    )
                    .ok();

                if let Some((old_launches, old_playtime, old_last_played)) = old_row {
                    let new_row: Option<(i64, i64, Option<i64>)> = tx
                        .query_row(
                            "SELECT launch_count, play_time_seconds, last_played_at FROM instance_stats WHERE instance_id = ?1",
                            rusqlite::params![&new_id],
                            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                        )
                        .ok();

                    if let Some((new_launches, new_playtime, new_last_played)) = new_row {
                        let merged_launches = new_launches + old_launches;
                        let merged_playtime = new_playtime + old_playtime;
                        let merged_last_played = match (old_last_played, new_last_played) {
                            (Some(a), Some(b)) => Some(a.max(b)),
                            (a, b) => a.or(b),
                        };
                        tx.execute(
                            "UPDATE instance_stats SET launch_count = ?1, play_time_seconds = ?2, last_played_at = ?3 WHERE instance_id = ?4",
                            rusqlite::params![merged_launches, merged_playtime, merged_last_played, &new_id],
                        )?;
                        tx.execute(
                            "DELETE FROM instance_stats WHERE instance_id = ?1",
                            rusqlite::params![&old_id],
                        )?;
                    } else {
                        tx.execute(
                            "UPDATE instance_stats SET instance_id = ?1 WHERE instance_id = ?2",
                            rusqlite::params![&new_id, &old_id],
                        )?;
                    }
                }

                tx.commit()?;
                Ok(())
            })
            .await
            .map_err(|e| e.to_string())
    }



    pub async fn get_all_stats(&self) -> Result<Vec<InstanceStats>, String> {
        self.conn
            .call(move |conn| -> rusqlite::Result<Vec<InstanceStats>> {
                let mut stmt = conn.prepare("SELECT instance_id, launch_count, play_time_seconds, last_played_at FROM instance_stats ORDER BY last_played_at DESC")?;
                let mut rows = stmt.query([])?;
                let mut stats = Vec::new();

                while let Some(row) = rows.next()? {
                    stats.push(InstanceStats {
                        instance_id: row.get(0)?,
                        launch_count: row.get(1)?,
                        play_time_seconds: row.get(2)?,
                        last_played_at: row.get(3)?,
                    });
                }
                Ok(stats)
            })
            .await
            .map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_rename_stats_migrates_and_merges() {
        let dir = tempdir().unwrap();
        let db = StatisticsDb::new(dir.path().join("stats.db")).await.unwrap();

        db.record_launch("OldPack".to_string()).await.unwrap();
        db.add_playtime("OldPack".to_string(), 120).await.unwrap();

        db.rename_stats("OldPack".to_string(), "NewPack".to_string())
            .await
            .unwrap();

        assert!(db.get_instance_stats("OldPack".to_string()).await.unwrap().is_none());
        let migrated = db
            .get_instance_stats("NewPack".to_string())
            .await
            .unwrap()
            .expect("migrated stats should exist");
        assert_eq!(migrated.launch_count, 1);
        assert_eq!(migrated.play_time_seconds, 120);

        // Merging when target already has stats
        db.record_launch("SourcePack".to_string()).await.unwrap();
        db.add_playtime("SourcePack".to_string(), 30).await.unwrap();
        db.rename_stats("SourcePack".to_string(), "NewPack".to_string())
            .await
            .unwrap();

        let merged = db
            .get_instance_stats("NewPack".to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(merged.launch_count, 2);
        assert_eq!(merged.play_time_seconds, 150);
    }
}
