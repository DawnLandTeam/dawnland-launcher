use crate::core::mojang::get_minecraft_base;
use crate::models::instance::AssetCategory;
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[derive(Default)]
pub enum AiProviderType {
    RemoteApi,
    #[default]
    EmbeddedLlm,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiConfig {
    #[serde(default)]
    pub provider_type: AiProviderType,
    pub remote_api_key: Option<String>,
    pub remote_base_url: Option<String>,
    pub remote_model: Option<String>,
    pub active_embedded_model: Option<String>,
    #[serde(default = "default_max_ram_usage")]
    pub max_ram_usage: u32,
    #[serde(default)]
    pub unlock_context_size: bool,
}

fn default_max_ram_usage() -> u32 {
    4096
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            provider_type: AiProviderType::default(),
            remote_api_key: None,
            remote_base_url: None,
            remote_model: None,
            active_embedded_model: None,
            max_ram_usage: default_max_ram_usage(),
            unlock_context_size: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum FilenameFormatMode {
    #[default]
    Auto,
    Preset,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum FilenamePreset {
    #[default]
    BracketPrefix,
    CnBracketPrefix,
    DashPrefix,
    DashSuffix,
    ZhVersion,
    Original,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum ManagementDisplayStyle {
    #[default]
    TranslationFirst,
    FilenameFirst,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceNamingSettings {
    #[serde(default)]
    pub filename_mode: FilenameFormatMode,
    #[serde(default)]
    pub filename_preset: FilenamePreset,
    #[serde(default = "default_custom_filename_template")]
    pub custom_template: String,
    #[serde(default = "default_rename_targets")]
    pub rename_targets: Vec<AssetCategory>,
    #[serde(default)]
    pub management_display_style: ManagementDisplayStyle,
    #[serde(default = "default_true_flag")]
    pub enable_online_translation: bool,
    #[serde(default = "default_true_flag")]
    pub auto_sync_translations: bool,
}

fn default_custom_filename_template() -> String {
    "[{zh}] {stem}{ext}".to_string()
}

fn default_rename_targets() -> Vec<AssetCategory> {
    vec![
        AssetCategory::Mod,
        AssetCategory::ResourcePack,
        AssetCategory::ShaderPack,
        AssetCategory::Datapack,
    ]
}

fn default_true_flag() -> bool {
    true
}

impl Default for ResourceNamingSettings {
    fn default() -> Self {
        Self {
            filename_mode: FilenameFormatMode::Auto,
            filename_preset: FilenamePreset::BracketPrefix,
            custom_template: default_custom_filename_template(),
            rename_targets: default_rename_targets(),
            management_display_style: ManagementDisplayStyle::TranslationFirst,
            enable_online_translation: true,
            auto_sync_translations: true,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum CacheRetentionPolicy {
    #[default]
    #[serde(rename = "immediate")]
    Immediate,
    #[serde(rename = "24h")]
    Hours24,
    #[serde(rename = "7d")]
    Days7,
}

impl CacheRetentionPolicy {
    /// Returns the grace period in seconds before unreferenced cache entries are reclaimed automatically.
    pub fn retention_secs(self) -> u64 {
        match self {
            Self::Immediate => 0,
            Self::Hours24 => 24 * 3600,
            Self::Days7 => 7 * 24 * 3600,
        }
    }
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
    #[serde(default)]
    pub ai_config: AiConfig,
    #[serde(default)]
    pub enable_global_game_settings: bool,
    #[serde(default = "default_enable_global_mod_cache")]
    pub enable_global_mod_cache: bool,
    #[serde(default = "default_auto_clean_cache")]
    pub auto_clean_cache: bool,
    #[serde(default)]
    pub cache_retention_policy: CacheRetentionPolicy,
    #[serde(default)]
    pub auto_clean_shared_assets: bool,
    #[serde(default = "default_enable_jvm_tuner")]
    pub enable_jvm_tuner: bool,
    #[serde(default)]
    pub ui_language: Option<String>,
    #[serde(default)]
    pub resource_naming: ResourceNamingSettings,
}

fn default_max_concurrent_downloads() -> u32 {
    32
}

fn default_enable_global_mod_cache() -> bool {
    true
}

fn default_auto_clean_cache() -> bool {
    true
}

fn default_enable_jvm_tuner() -> bool {
    true
}

impl Default for LauncherSettings {
    fn default() -> Self {
        Self {
            enable_instance_inheritance: false,
            download_source: DownloadSource::default(),
            max_concurrent_downloads: 32,
            enable_telemetry: None,
            global_max_memory: None,
            ai_config: AiConfig::default(),
            enable_global_game_settings: false,
            enable_global_mod_cache: true,
            auto_clean_cache: true,
            cache_retention_policy: CacheRetentionPolicy::default(),
            auto_clean_shared_assets: false,
            enable_jvm_tuner: true,
            ui_language: None,
            resource_naming: ResourceNamingSettings::default(),
        }
    }
}

pub fn is_simplified_chinese(lang: &str) -> bool {
    let lower = lang.trim().to_ascii_lowercase().replace('_', "-");
    lower.starts_with("zh")
}

pub fn resolve_effective_language(settings: &LauncherSettings) -> String {
    settings
        .ui_language
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(sys_locale::get_locale)
        .unwrap_or_else(|| "zh-CN".to_string())
}

pub fn preset_to_template(preset: &FilenamePreset) -> &'static str {
    match preset {
        FilenamePreset::BracketPrefix => "[{zh}] {stem}{ext}",
        FilenamePreset::CnBracketPrefix => "【{zh}】{stem}{ext}",
        FilenamePreset::DashPrefix => "{zh}-{stem}{ext}",
        FilenamePreset::DashSuffix => "{stem}-{zh}{ext}",
        FilenamePreset::ZhVersion => "{zh}-{version}{ext}",
        FilenamePreset::Original => "{stem}{ext}",
    }
}

pub fn resolve_active_template(settings: &LauncherSettings) -> String {
    if !settings.resource_naming.enable_online_translation {
        return "{stem}{ext}".to_string();
    }
    match settings.resource_naming.filename_mode {
        FilenameFormatMode::Auto => {
            let lang = resolve_effective_language(settings);
            if is_simplified_chinese(&lang) {
                "[{zh}] {stem}{ext}".to_string()
            } else {
                "{stem}{ext}".to_string()
            }
        }
        FilenameFormatMode::Preset => {
            preset_to_template(&settings.resource_naming.filename_preset).to_string()
        }
        FilenameFormatMode::Custom => {
            let trimmed = settings.resource_naming.custom_template.trim();
            if trimmed.is_empty() {
                "{stem}{ext}".to_string()
            } else {
                trimmed.to_string()
            }
        }
    }
}

pub fn should_rename_category(settings: &LauncherSettings, category: &AssetCategory) -> bool {
    if !settings.resource_naming.enable_online_translation {
        return false;
    }
    if !settings.resource_naming.rename_targets.contains(category) {
        return false;
    }
    let template = resolve_active_template(settings);
    template.trim() != "{stem}{ext}"
}

pub fn get_launcher_settings_path() -> PathBuf {
    crate::core::mojang::get_dawnland_dir().join("launcher_settings.json")
}

static SETTINGS_CACHE: RwLock<Option<LauncherSettings>> = RwLock::new(None);
static SETTINGS_GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static SETTINGS_SAVE_LOCK: std::sync::LazyLock<tokio::sync::Mutex<()>> =
    std::sync::LazyLock::new(|| tokio::sync::Mutex::new(()));

fn read_launcher_settings_from_disk(config_path: &std::path::Path) -> (LauncherSettings, bool) {
    match std::fs::read_to_string(config_path) {
        Ok(content) => match serde_json::from_str::<LauncherSettings>(&content) {
            Ok(config) => (config, true),
            Err(e) => {
                tracing::warn!(
                    "Failed to parse launcher settings {}: {}",
                    config_path.display(),
                    e
                );
                (LauncherSettings::default(), false)
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (LauncherSettings::default(), true),
        Err(e) => {
            tracing::warn!(
                "Failed to read launcher settings {}: {}",
                config_path.display(),
                e
            );
            (LauncherSettings::default(), false)
        }
    }
}

fn finalize_launcher_settings_cache(
    settings: LauncherSettings,
    should_cache: bool,
    gen_before: u64,
    cache_lock: &RwLock<Option<LauncherSettings>>,
    generation: &std::sync::atomic::AtomicU64,
) -> LauncherSettings {
    if let Ok(mut cache) = cache_lock.write() {
        if let Some(existing) = cache.as_ref() {
            return existing.clone();
        }
        if should_cache {
            if generation.load(std::sync::atomic::Ordering::SeqCst) == gen_before {
                *cache = Some(settings.clone());
            } else if let Some(existing) = cache.as_ref() {
                return existing.clone();
            }
        }
    }

    settings
}

fn get_launcher_settings_sync_with(
    config_path: &std::path::Path,
    cache_lock: &RwLock<Option<LauncherSettings>>,
    generation: &std::sync::atomic::AtomicU64,
) -> LauncherSettings {
    if let Ok(cache) = cache_lock.read() {
        if let Some(settings) = cache.as_ref() {
            return settings.clone();
        }
    }

    let gen_before = generation.load(std::sync::atomic::Ordering::SeqCst);
    let (settings, should_cache) = read_launcher_settings_from_disk(config_path);
    finalize_launcher_settings_cache(settings, should_cache, gen_before, cache_lock, generation)
}

pub fn get_launcher_settings_sync() -> LauncherSettings {
    let config_path = get_launcher_settings_path();
    get_launcher_settings_sync_with(&config_path, &SETTINGS_CACHE, &SETTINGS_GENERATION)
}

async fn read_launcher_settings_strict_at(
    config_path: &std::path::Path,
) -> Result<LauncherSettings, String> {
    let _guard = SETTINGS_SAVE_LOCK.lock().await;
    let gen_before = SETTINGS_GENERATION.load(std::sync::atomic::Ordering::SeqCst);
    let settings = match tokio::fs::read_to_string(config_path).await {
        Ok(content) => serde_json::from_str::<LauncherSettings>(&content).map_err(|e| {
            format!(
                "Failed to parse launcher settings {}: {}",
                config_path.display(),
                e
            )
        })?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => LauncherSettings::default(),
        Err(e) => {
            return Err(format!(
                "Failed to read launcher settings {}: {}",
                config_path.display(),
                e
            ));
        }
    };

    if config_path == get_launcher_settings_path().as_path() {
        if let Ok(mut cache) = SETTINGS_CACHE.write() {
            if SETTINGS_GENERATION.load(std::sync::atomic::Ordering::SeqCst) == gen_before {
                *cache = Some(settings.clone());
            }
        }
    }

    Ok(settings)
}

/// Strictly loads launcher settings without substituting defaults when the settings file
/// exists but cannot be read or parsed.
pub async fn load_launcher_settings_strict() -> Result<LauncherSettings, String> {
    let config_path = get_launcher_settings_path();
    read_launcher_settings_strict_at(&config_path).await
}

pub async fn get_launcher_settings() -> LauncherSettings {
    let _guard = SETTINGS_SAVE_LOCK.lock().await;
    get_launcher_settings_sync()
}

#[tauri::command]
pub async fn load_launcher_settings() -> Result<LauncherSettings, String> {
    Ok(get_launcher_settings().await)
}

#[tauri::command]
pub async fn save_launcher_settings(settings: LauncherSettings) -> Result<(), String> {
    save_launcher_settings_internal(settings).await
}

async fn persist_launcher_settings_locked(settings: LauncherSettings) -> Result<(), String> {
    SETTINGS_GENERATION.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

    let config_path = get_launcher_settings_path();
    let content = serde_json::to_string_pretty(&settings)
        .map_err(|e| format!("Failed to serialize launcher settings: {}", e))?;

    crate::core::fs_utils::write_file_atomic(&config_path, content.as_bytes())
        .await
        .map_err(|e| format!("Failed to write launcher settings: {}", e))?;

    if let Ok(mut cache) = SETTINGS_CACHE.write() {
        SETTINGS_GENERATION.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        *cache = Some(settings);
    }

    Ok(())
}

pub async fn save_launcher_settings_internal(settings: LauncherSettings) -> Result<(), String> {
    let _guard = SETTINGS_SAVE_LOCK.lock().await;
    persist_launcher_settings_locked(settings).await
}

#[tauri::command]
pub async fn sync_ui_language(language: String) -> Result<(), String> {
    let trimmed = language.trim().to_string();
    if trimmed.is_empty() {
        return Ok(());
    }
    let _guard = SETTINGS_SAVE_LOCK.lock().await;
    let mut settings = get_launcher_settings_sync();
    if settings.ui_language.as_deref() == Some(trimmed.as_str()) {
        return Ok(());
    }
    settings.ui_language = Some(trimmed);
    persist_launcher_settings_locked(settings).await
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
        assert_eq!(default_settings.resource_naming.filename_mode, FilenameFormatMode::Auto);
    }

    #[test]
    fn test_resolve_active_template_auto_by_language() {
        let mut settings = LauncherSettings {
            ui_language: Some("zh-CN".to_string()),
            ..Default::default()
        };
        assert_eq!(resolve_active_template(&settings), "[{zh}] {stem}{ext}");

        settings.ui_language = Some("en".to_string());
        assert_eq!(resolve_active_template(&settings), "{stem}{ext}");

        settings.resource_naming.filename_mode = FilenameFormatMode::Preset;
        settings.resource_naming.filename_preset = FilenamePreset::CnBracketPrefix;
        assert_eq!(resolve_active_template(&settings), "【{zh}】{stem}{ext}");

        settings.resource_naming.filename_mode = FilenameFormatMode::Custom;
        settings.resource_naming.custom_template = "{zh}-{version}{ext}".to_string();
        assert_eq!(resolve_active_template(&settings), "{zh}-{version}{ext}");
    }

    #[test]
    fn test_legacy_launcher_settings_deserialize() {
        let legacy_json = r#"{"enableInstanceInheritance":true,"maxConcurrentDownloads":16}"#;
        let parsed: LauncherSettings = serde_json::from_str(legacy_json).expect("deserialize legacy");
        assert!(parsed.enable_instance_inheritance);
        assert_eq!(parsed.max_concurrent_downloads, 16);
        assert!(parsed.auto_clean_cache);
        assert_eq!(parsed.cache_retention_policy, CacheRetentionPolicy::Immediate);
        assert!(!parsed.auto_clean_shared_assets);
        assert_eq!(parsed.resource_naming.filename_mode, FilenameFormatMode::Auto);
        assert!(parsed.resource_naming.rename_targets.contains(&AssetCategory::Mod));
    }

    #[test]
    fn test_cache_gc_settings_defaults_and_roundtrip() {
        let defaults = LauncherSettings::default();
        assert!(defaults.auto_clean_cache);
        assert_eq!(defaults.cache_retention_policy, CacheRetentionPolicy::Immediate);
        assert!(!defaults.auto_clean_shared_assets);
        assert_eq!(CacheRetentionPolicy::Immediate.retention_secs(), 0);
        assert_eq!(CacheRetentionPolicy::Hours24.retention_secs(), 86_400);
        assert_eq!(CacheRetentionPolicy::Days7.retention_secs(), 604_800);

        let raw = r#"{
            "autoCleanCache": false,
            "cacheRetentionPolicy": "7d",
            "autoCleanSharedAssets": true
        }"#;
        let parsed: LauncherSettings = serde_json::from_str(raw).expect("deserialize cache settings");
        assert!(!parsed.auto_clean_cache);
        assert_eq!(parsed.cache_retention_policy, CacheRetentionPolicy::Days7);
        assert!(parsed.auto_clean_shared_assets);

        let encoded = serde_json::to_string(&parsed).expect("serialize cache settings");
        assert!(encoded.contains(r#""cacheRetentionPolicy":"7d""#));
        assert!(encoded.contains(r#""autoCleanSharedAssets":true"#));
    }

    #[tokio::test]
    async fn test_read_launcher_settings_strict_at_rejects_malformed_json() {
        let dir = tempfile::tempdir().expect("tempdir");
        let missing = dir.path().join("missing.json");
        let fallback = read_launcher_settings_strict_at(&missing)
            .await
            .expect("missing file yields defaults");
        assert!(fallback.auto_clean_cache);

        let valid_path = dir.path().join("valid.json");
        std::fs::write(&valid_path, r#"{"autoCleanCache":false,"cacheRetentionPolicy":"7d"}"#)
            .expect("write valid settings");
        let valid = read_launcher_settings_strict_at(&valid_path)
            .await
            .expect("valid settings parse");
        assert!(!valid.auto_clean_cache);
        assert_eq!(valid.cache_retention_policy, CacheRetentionPolicy::Days7);

        let corrupt_path = dir.path().join("corrupt.json");
        std::fs::write(&corrupt_path, "{ not valid json").expect("write corrupt settings");
        let err = read_launcher_settings_strict_at(&corrupt_path)
            .await
            .expect_err("malformed json must error instead of returning defaults");
        assert!(err.contains("Failed to parse launcher settings"));
    }

    #[test]
    fn test_sync_settings_falls_back_to_cache_on_corrupt_disk_and_skips_caching_defaults() {
        let dir = tempfile::tempdir().expect("tempdir");
        let corrupt_path = dir.path().join("launcher_settings.json");
        std::fs::write(&corrupt_path, "{ truncated").expect("write corrupt settings");

        // 1. Cold cache + corrupt disk returns defaults without poisoning the cache with defaults.
        let cold_cache = RwLock::new(None);
        let generation = std::sync::atomic::AtomicU64::new(0);
        let cold_res = get_launcher_settings_sync_with(&corrupt_path, &cold_cache, &generation);
        assert!(cold_res.auto_clean_cache);
        assert_eq!(cold_res.cache_retention_policy, CacheRetentionPolicy::Immediate);
        assert!(cold_cache.read().expect("read lock").is_none());

        // 2. Concurrent cache fill after read failure returns the cached settings instead of defaults.
        let custom = LauncherSettings {
            auto_clean_cache: false,
            cache_retention_policy: CacheRetentionPolicy::Days7,
            global_max_memory: Some(8192),
            ..Default::default()
        };

        let race_cache = RwLock::new(None);
        let race_gen = std::sync::atomic::AtomicU64::new(0);
        let gen_before = race_gen.load(std::sync::atomic::Ordering::SeqCst);
        let (disk_settings, should_cache) = read_launcher_settings_from_disk(&corrupt_path);
        assert!(!should_cache);
        {
            let mut guard = race_cache.write().expect("write lock");
            race_gen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            *guard = Some(custom.clone());
        }
        let raced = finalize_launcher_settings_cache(
            disk_settings,
            should_cache,
            gen_before,
            &race_cache,
            &race_gen,
        );
        assert!(!raced.auto_clean_cache);
        assert_eq!(raced.cache_retention_policy, CacheRetentionPolicy::Days7);
        assert_eq!(raced.global_max_memory, Some(8192));

        // 3. Warm cache + corrupt disk always returns the warm cache entry.
        let warm = get_launcher_settings_sync_with(&corrupt_path, &race_cache, &race_gen);
        assert!(!warm.auto_clean_cache);
        assert_eq!(warm.cache_retention_policy, CacheRetentionPolicy::Days7);
        assert_eq!(warm.global_max_memory, Some(8192));
    }
}
