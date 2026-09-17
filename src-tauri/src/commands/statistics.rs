use crate::core::statistics::{InstanceStats, StatisticsDb};
use crate::error::AppError;

#[tauri::command]
pub async fn get_instance_stats(
    instance_id: String,
    stats_db: tauri::State<'_, StatisticsDb>,
) -> Result<Option<InstanceStats>, AppError> {
    stats_db
        .get_instance_stats(instance_id)
        .await
        .map_err(|e| AppError {
            code: "DATABASE_ERROR".to_string(),
            message: e,
        })
}

#[tauri::command]
pub async fn get_all_stats(
    stats_db: tauri::State<'_, StatisticsDb>,
) -> Result<Vec<InstanceStats>, AppError> {
    stats_db
        .get_all_stats()
        .await
        .map_err(|e| AppError {
            code: "DATABASE_ERROR".to_string(),
            message: e,
        })
}

