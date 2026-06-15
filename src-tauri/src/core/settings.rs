use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use crate::core::mojang::get_minecraft_base;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LauncherSettings {
    #[serde(default)]
    pub enable_instance_inheritance: bool,
}

impl Default for LauncherSettings {
    fn default() -> Self {
        Self {
            enable_instance_inheritance: false,
        }
    }
}

pub fn get_launcher_settings_path() -> PathBuf {
    crate::core::mojang::get_dawnland_dir().join("launcher_settings.json")
}

#[tauri::command]
pub async fn load_launcher_settings() -> Result<LauncherSettings, String> {
    let config_path = get_launcher_settings_path();
    if config_path.exists() {
        if let Ok(content) = tokio::fs::read_to_string(&config_path).await {
            if let Ok(config) = serde_json::from_str(&content) {
                return Ok(config);
            }
        }
    }
    Ok(LauncherSettings::default())
}

#[tauri::command]
pub async fn save_launcher_settings(settings: LauncherSettings) -> Result<(), String> {
    let config_path = get_launcher_settings_path();
    let content = serde_json::to_string_pretty(&settings)
        .map_err(|e| format!("Failed to serialize launcher settings: {}", e))?;
        
    if let Some(parent) = config_path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
        
    tokio::fs::write(&config_path, content)
        .await
        .map_err(|e| format!("Failed to write launcher settings: {}", e))
}
