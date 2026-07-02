use crate::core::mojang::get_minecraft_base;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[derive(Default)]
pub enum DownloadSource {
    Official,
    #[default]
    Bmclapi,
}


#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LauncherSettings {
    #[serde(default)]
    pub enable_instance_inheritance: bool,
    #[serde(default)]
    pub download_source: DownloadSource,
    #[serde(default = "default_max_concurrent_downloads")]
    pub max_concurrent_downloads: u32,
    #[serde(default)]
    pub enable_telemetry: Option<bool>,
    #[serde(default)]
    pub global_max_memory: Option<u32>,
}

fn default_max_concurrent_downloads() -> u32 {
    32
}

impl Default for LauncherSettings {
    fn default() -> Self {
        Self {
            enable_instance_inheritance: false,
            download_source: DownloadSource::default(),
            max_concurrent_downloads: 32,
            enable_telemetry: None,
            global_max_memory: None,
        }
    }
}

pub fn get_launcher_settings_path() -> PathBuf {
    crate::core::mojang::get_dawnland_dir().join("launcher_settings.json")
}

static SETTINGS_CACHE: RwLock<Option<LauncherSettings>> = RwLock::new(None);

pub fn get_launcher_settings_sync() -> LauncherSettings {
    if let Ok(cache) = SETTINGS_CACHE.read() {
        if let Some(settings) = cache.as_ref() {
            return settings.clone();
        }
    }

    let config_path = get_launcher_settings_path();
    let mut settings = LauncherSettings::default();
    if config_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&config_path) {
            if let Ok(config) = serde_json::from_str(&content) {
                settings = config;
            }
        }
    }

    if let Ok(mut cache) = SETTINGS_CACHE.write() {
        *cache = Some(settings.clone());
    }

    settings
}

#[tauri::command]
pub async fn load_launcher_settings() -> Result<LauncherSettings, String> {
    Ok(get_launcher_settings_sync())
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
        .map_err(|e| format!("Failed to write launcher settings: {}", e))?;

    if let Ok(mut cache) = SETTINGS_CACHE.write() {
        *cache = Some(settings);
    }
    
    Ok(())
}

pub fn replace_download_url(url: &str, source: &DownloadSource) -> String {
    match source {
        DownloadSource::Official => url.to_string(),
        DownloadSource::Bmclapi => {
            
            url
                .replace(
                    "https://launchermeta.mojang.com",
                    "https://bmclapi2.bangbang93.com",
                )
                .replace(
                    "https://piston-meta.mojang.com",
                    "https://bmclapi2.bangbang93.com",
                )
                .replace(
                    "https://libraries.minecraft.net",
                    "https://bmclapi2.bangbang93.com/maven",
                )
                .replace(
                    "https://resources.download.minecraft.net",
                    "https://bmclapi2.bangbang93.com/assets",
                )
                .replace(
                    "https://meta.fabricmc.net",
                    "https://bmclapi2.bangbang93.com/fabric-meta",
                )
                .replace(
                    "https://maven.fabricmc.net",
                    "https://bmclapi2.bangbang93.com/maven",
                )
                .replace(
                    "https://maven.minecraftforge.net",
                    "https://bmclapi2.bangbang93.com/maven",
                )
                .replace(
                    "https://files.minecraftforge.net/maven",
                    "https://bmclapi2.bangbang93.com/maven",
                )
                .replace(
                    "https://maven.neoforged.net/releases",
                    "https://bmclapi2.bangbang93.com/maven",
                )
                .replace(
                    "https://maven.neoforged.net",
                    "https://bmclapi2.bangbang93.com/maven",
                )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_replace_download_url_official() {
        let url = "https://libraries.minecraft.net/com/mojang/test.jar";
        let replaced = replace_download_url(url, &DownloadSource::Official);
        assert_eq!(replaced, "https://libraries.minecraft.net/com/mojang/test.jar");
    }

    #[test]
    fn test_replace_download_url_bmclapi() {
        let url = "https://libraries.minecraft.net/com/mojang/test.jar";
        let replaced = replace_download_url(url, &DownloadSource::Bmclapi);
        assert_eq!(replaced, "https://bmclapi2.bangbang93.com/maven/com/mojang/test.jar");

        let launchermeta = "https://launchermeta.mojang.com/v1/packages/test.json";
        assert_eq!(
            replace_download_url(launchermeta, &DownloadSource::Bmclapi),
            "https://bmclapi2.bangbang93.com/v1/packages/test.json"
        );
        
        let fabric_meta = "https://meta.fabricmc.net/v2/versions";
        assert_eq!(
            replace_download_url(fabric_meta, &DownloadSource::Bmclapi),
            "https://bmclapi2.bangbang93.com/fabric-meta/v2/versions"
        );
    }
    
    #[test]
    fn test_launcher_settings_default() {
        let default_settings = LauncherSettings::default();
        assert_eq!(default_settings.max_concurrent_downloads, 32);
        assert_eq!(default_settings.download_source, DownloadSource::Bmclapi);
        assert!(!default_settings.enable_instance_inheritance);
    }
}
