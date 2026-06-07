use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::OnceLock;
use tauri::AppHandle;
use tokio::fs;

// Re-export downloader types (from parent module)
use crate::downloader::{run_batch_download_task, DownloadTask};
use crate::core::task::{ExecutableTask, TaskContext, TaskError, TaskManager, TaskType};

// ============ Global State ============

/// Base directory for Minecraft files: ~/.dawnland/.minecraft
static MINECRAFT_BASE: OnceLock<PathBuf> = OnceLock::new();

/// Get the Minecraft base directory path.
pub fn get_minecraft_base() -> &'static PathBuf {
    MINECRAFT_BASE.get_or_init(|| {
        let base = std::env::current_exe()
            .map(|p| p.parent().unwrap().to_path_buf())
            .unwrap_or_else(|_| PathBuf::from("."));
        base.join(".minecraft")
    })
}

// Legacy INSTALL_STATE has been removed in favor of TaskManager.
// CANCEL_FLAG is temporarily retained for compatibility with specific legacy call sites 
// (e.g., older download tasks) that have not yet been fully migrated to TaskContext cancellation tokens.

static CANCEL_FLAG: std::sync::OnceLock<std::sync::Arc<std::sync::atomic::AtomicBool>> = std::sync::OnceLock::new();
pub fn get_cancel_flag() -> std::sync::Arc<std::sync::atomic::AtomicBool> {
    CANCEL_FLAG.get_or_init(|| std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false))).clone()
}

// ============ Mojang API Types ============

/// Version manifest from Mojang (version_manifest_v2.json).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionManifest {
    pub latest: LatestVersions,
    pub versions: Vec<VersionInfo>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LatestVersions {
    pub release: Option<String>,
    pub snapshot: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionInfo {
    pub id: String,
    #[serde(rename = "type")]
    pub version_type: String,
    pub url: String,
    pub time: Option<String>,
    pub release_time: Option<String>,
}

/// Simplified version info for frontend.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VanillaVersion {
    pub id: String,
    pub version_type: String,
    pub url: String,
}

/// Version metadata (from version's JSON file).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionMeta {
    pub id: String,
    // Inherits from another version (used by Fabric, Forge, etc.)
    #[serde(rename = "inheritsFrom")]
    pub inherits_from: Option<String>,
    // Time fields - optional as some old versions may lack them
    pub time: Option<String>,
    pub release_time: Option<String>,
    // Type - optional because old snapshots may not have it
    #[serde(rename = "type")]
    pub version_type: Option<String>,
    // Main class - some versions may use different entry points
    #[serde(rename = "mainClass")]
    pub main_class: Option<String>,
    // Arguments - old versions use minecraft_arguments, new versions use arguments
    pub minecraft_arguments: Option<String>,
    pub arguments: Option<Arguments>,
    #[serde(rename = "minimumLauncherVersion")]
    pub minimum_launcher_version: Option<u32>,
    // Assets - critical for running the game
    pub assets: Option<String>,
    #[serde(rename = "assetIndex")]
    pub asset_index: Option<AssetIndex>,
    // Downloads - critical for getting client.jar
    pub downloads: Option<Downloads>,
    // Libraries - critical for game runtime
    pub libraries: Option<Vec<Library>>,
    // Java version requested by this profile
    #[serde(rename = "javaVersion")]
    pub java_version: Option<JavaVersion>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaVersion {
    pub component: String,
    #[serde(rename = "majorVersion")]
    pub major_version: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetIndex {
    pub id: String,
    pub total_size: Option<u64>,
    pub url: Option<String>, // URL might be missing in some old versions
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Downloads {
    pub client: Option<DownloadInfo>,
    pub server: Option<DownloadInfo>,
    #[serde(rename = "windows_server")]
    pub windows_server: Option<DownloadInfo>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadInfo {
    pub sha1: Option<String>, // May be missing in old versions
    pub size: Option<u64>,    // May be missing
    pub url: Option<String>,  // Critical - might be missing
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Library {
    pub downloads: Option<LibraryDownloads>,
    pub name: Option<String>, // Some libraries may not have a name
    pub url: Option<String>,  // Fabric/Forge use Maven coordinates - base URL
    pub rules: Option<Vec<Rule>>,
    pub extract: Option<ExtractRule>,
    pub natives: Option<std::collections::HashMap<String, String>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryDownloads {
    pub artifact: Option<Artifact>,
    pub classifiers: Option<std::collections::HashMap<String, Artifact>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Artifact {
    pub path: Option<String>,
    pub sha1: Option<String>,
    pub size: Option<u64>,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtractRule {
    pub exclude: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rule {
    pub action: Option<String>, // Some rules may not have action
    pub os: Option<RuleOs>,
    pub features: Option<std::collections::HashMap<String, bool>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleOs {
    pub name: Option<String>,
    pub arch: Option<String>,
    pub version: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Arguments {
    pub game: Option<serde_json::Value>,
    pub jvm: Option<serde_json::Value>,
}

/// Asset index from Mojang (objects map).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetIndexMeta {
    pub objects: Option<std::collections::HashMap<String, AssetObject>>,
}

// ============ Maven Coordinate Parser ============

/// Convert Maven coordinate to local file path
/// Example: "net.fabricmc:fabric-loader:0.14.22" -> "net/fabricmc/fabric-loader/0.14.22/fabric-loader-0.14.22.jar"
pub fn maven_name_to_path(name: &str) -> Option<String> {
    let parts: Vec<&str> = name.split(':').collect();
    if parts.len() >= 3 {
        let group = parts[0].replace('.', "/");
        let artifact = parts[1];
        let version = parts[2];
        let mut classifier = if parts.len() == 4 {
            format!("-{}", parts[3])
        } else {
            String::new()
        };

        if group == "net/minecraftforge" && artifact == "forge" && classifier.is_empty() {
            classifier = "-universal".to_string();
        }

        Some(format!(
            "{}/{}/{}/{}-{}{}.jar",
            group, artifact, version, artifact, version, classifier
        ))
    } else {
        None
    }
}

/// Get download URL for a library using Maven coordinates (from JSON value)
pub fn get_library_download_info_from_json(lib: &serde_json::Value) -> Option<(String, String)> {
    // Get the library name
    let name = lib.get("name")?.as_str()?;

    // Try standard downloads.artifact first
    if let Some(downloads) = lib.get("downloads") {
        if let Some(artifact) = downloads.get("artifact") {
            if let Some(url) = artifact.get("url").and_then(|u| u.as_str()) {
                if !url.is_empty() {
                    if let Some(path) = artifact.get("path").and_then(|p| p.as_str()) {
                        return Some((url.to_string(), path.to_string()));
                    }
                }
            }
        }
    }

    // Fallback to Maven coordinate format (Fabric/Forge style)
    let path = maven_name_to_path(name)?;

    // Determine base URL from lib.url or use default
    let base_url = lib
        .get("url")
        .and_then(|u| u.as_str())
        .unwrap_or("https://libraries.minecraft.net/");

    let download_url = format!("{}{}", base_url, path);

    Some((download_url, format!("libraries/{}", path)))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetObject {
    pub hash: Option<String>,
    pub size: Option<u64>,
}

// ============ Rules Parser ============

/// Check if a library should be downloaded for the current system.
pub fn should_download_library(lib: &Library) -> bool {
    // If no name, skip it (shouldn't happen, but safety first)
    if lib.name.is_none() {
        return false;
    }

    // If no rules, always download.
    let rules = match &lib.rules {
        Some(r) if r.is_empty() => return true,
        Some(r) => r,
        None => return true,
    };

    let mut allowed = false;
    if let Some(first) = rules.first() {
        let first_action = first.action.as_deref().unwrap_or("allow");
        if first_action == "allow" {
            allowed = false;
        } else if first_action == "disallow" {
            allowed = true;
        }
    }

    for rule in rules {
        let action = rule.action.as_deref().unwrap_or("allow");
        let mut applies = true;

        if let Some(ref os) = rule.os {
            if let Some(os_name) = &os.name {
                if !matches_current_os(os_name) {
                    applies = false;
                }
            }
            if applies {
                if let Some(ref arch) = os.arch {
                    if !matches_current_arch(arch) {
                        applies = false;
                    }
                }
            }
        }

        if applies {
            if action == "allow" {
                allowed = true;
            } else if action == "disallow" {
                allowed = false;
            }
        }
    }

    allowed
}

/// Check if current OS matches the rule.
fn matches_current_os(rule_os: &str) -> bool {
    let current_os = std::env::consts::OS;
    match rule_os {
        "windows" => current_os == "windows",
        "osx" | "macos" => current_os == "macos",
        "linux" => current_os == "linux",
        _ => false,
    }
}

/// Check if current architecture matches the rule.
fn matches_current_arch(rule_arch: &str) -> bool {
    let current_arch = std::env::consts::ARCH;
    match rule_arch {
        "x86" => current_arch == "x86",
        "x64" | "amd64" => current_arch == "x86_64",
        "arm64" | "aarch64" => current_arch == "aarch64",
        "arm" => current_arch == "arm",
        _ => false,
    }
}

// ============ Tauri Commands ============

/// Fetch all vanilla Minecraft versions from Mojang API.
#[tauri::command]
pub async fn get_vanilla_versions() -> Result<Vec<VanillaVersion>, String> {
    tracing::info!("Fetching vanilla versions from Mojang...");

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {e}"))?;

    let response = client
        .get("https://launchermeta.mojang.com/mc/game/version_manifest_v2.json")
        .send()
        .await
        .map_err(|e| format!("Failed to fetch version manifest: {e}"))?;

    if !response.status().is_success() {
        return Err(format!(
            "Version manifest request failed: {}",
            response.status()
        ));
    }

    let manifest: VersionManifest = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse version manifest: {e}"))?;

    let versions: Vec<VanillaVersion> = manifest
        .versions
        .into_iter()
        .map(|v| VanillaVersion {
            id: v.id,
            version_type: v.version_type,
            url: v.url,
        })
        .collect();

    tracing::info!("Fetched {} versions", versions.len());
    Ok(versions)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VanillaInstallOptions {
    pub version_id: String,
    pub version_json_url: String,
    pub is_dependency: Option<bool>,
}

pub struct InstallVanillaTask {
    pub options: VanillaInstallOptions,
}

#[async_trait::async_trait]
impl ExecutableTask for InstallVanillaTask {
    async fn execute(&self, ctx: TaskContext) -> Result<(), TaskError> {
        let version_id = &self.options.version_id;
        let version_json_url = &self.options.version_json_url;
        let is_dependency = self.options.is_dependency;

        let is_dep = is_dependency.unwrap_or(false);
        if !is_dep {
            ctx.set_total_steps(1).await;
            ctx.next_step(&format!("Resolving version: {}", version_id)).await;
        } else {
            ctx.update_progress(0, 0, &format!("Resolving version: {}", version_id)).await;
        }

        let base_dir = get_minecraft_base();
        let version_dir = base_dir.join("versions").join(version_id);

        let _ = tokio::fs::create_dir_all(&version_dir).await;

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .map_err(|e| TaskError::ExecutionError(format!("Failed to create HTTP client: {e}")))?;

        // Step A: Download and save version JSON.
        tracing::info!("Downloading version JSON from: {}", version_json_url);
        ctx.update_progress(0, 0, "Downloading version JSON").await;

        let version_json_content = client
            .get(version_json_url)
            .send()
            .await
            .map_err(|e| TaskError::ExecutionError(format!("Failed to download version JSON: {}", e)))?
            .text()
            .await
            .map_err(|e| TaskError::ExecutionError(format!("Failed to read version JSON: {}", e)))?;

        // Save version JSON.
        let version_json_path = version_dir.join(format!("{}.json", version_id));
        fs::write(&version_json_path, &version_json_content)
            .await
            .map_err(|e| TaskError::ExecutionError(format!("Failed to write version JSON: {e}")))?;

        // Parse version metadata.
        let version_meta: VersionMeta = serde_json::from_str(&version_json_content)
            .map_err(|e| TaskError::ExecutionError(format!("Failed to parse version JSON: {e}")))?;

        if ctx.is_cancelled() {
            return Err(TaskError::ExecutionError("Installation cancelled by user".to_string()));
        }

        // Step B: Build libraries download queue.
        ctx.update_progress(0, 0, "Resolving libraries").await;
        let mut tasks: Vec<DownloadTask> = Vec::new();

        let libraries = match &version_meta.libraries {
            Some(libs) => libs,
            None => &vec![],
        };

        for lib in libraries {
            if !should_download_library(lib) {
                continue;
            }

            if let Some(ref downloads) = lib.downloads {
                if let Some(ref artifact) = downloads.artifact {
                    let path = artifact
                        .path
                        .as_ref()
                        .map(|p| format!("libraries/{}", p))
                        .unwrap_or_else(|| "libraries/unknown".to_string());
                    let dest = base_dir.join(&path);

                    let url = artifact.url.as_ref().cloned().unwrap_or_default();
                    let hash = artifact.sha1.as_ref().cloned();

                    if !url.is_empty() {
                        tasks.push(DownloadTask::new(
                            url,
                            dest.to_string_lossy().to_string(),
                            hash,
                            artifact.size,
                        ));
                    }
                }

                if let Some(ref classifiers) = downloads.classifiers {
                    let os_key = match std::env::consts::OS {
                        "windows" => "natives-windows",
                        "macos" => "natives-macos",
                        "linux" => "natives-linux",
                        _ => continue,
                    };

                    if let Some(ref classifier) = classifiers.get(os_key) {
                        let path = classifier
                            .path
                            .as_ref()
                            .map(|p| format!("libraries/{}", p))
                            .unwrap_or_else(|| "libraries/unknown".to_string());
                        let dest = base_dir.join(&path);

                        let url = classifier.url.as_ref().cloned().unwrap_or_default();
                        let hash = classifier.sha1.as_ref().cloned();

                        if !url.is_empty() {
                            tasks.push(DownloadTask::new(
                                url,
                                dest.to_string_lossy().to_string(),
                                hash,
                                classifier.size,
                            ));
                        }
                    }
                }
            }
        }

        // Step C: Add client.jar to download queue.
        let downloads = match &version_meta.downloads {
            Some(d) => d,
            None => {
                return Err(TaskError::ExecutionError("Version JSON missing downloads section".to_string()));
            }
        };

        if let Some(ref client_download) = downloads.client {
            let url = client_download.url.as_ref().cloned().unwrap_or_default();
            let hash = client_download.sha1.as_ref().cloned();

            if !url.is_empty() {
                let path = format!("versions/{}/{}.jar", version_id, version_id);
                let dest = base_dir.join(&path);
                tasks.push(DownloadTask::new(
                    url,
                    dest.to_string_lossy().to_string(),
                    hash,
                    client_download.size,
                ));
            }
        }

        if ctx.is_cancelled() {
            return Err(TaskError::ExecutionError("Installation cancelled by user".to_string()));
        }

        // Step D: Download and parse asset index.
        ctx.update_progress(0, 0, "Resolving assets").await;
        let asset_index = match &version_meta.asset_index {
            Some(ai) => ai,
            None => {
                return Err(TaskError::ExecutionError("Version JSON missing assetIndex".to_string()));
            }
        };

        let asset_index_url = match &asset_index.url {
            Some(url) if !url.is_empty() => url.clone(),
            _ => {
                return Err(TaskError::ExecutionError("Version JSON missing asset index URL".to_string()));
            }
        };

        let asset_index_content = client
            .get(&asset_index_url)
            .send()
            .await
            .map_err(|e| TaskError::ExecutionError(format!("Failed to download asset index: {}", e)))?
            .text()
            .await
            .map_err(|e| TaskError::ExecutionError(format!("Failed to read asset index: {e}")))?;

        let asset_index_dir = base_dir.join("assets").join("indexes");
        fs::create_dir_all(&asset_index_dir)
            .await
            .map_err(|e| TaskError::ExecutionError(format!("Failed to create asset index directory: {e}")))?;

        let asset_index_id = asset_index.id.clone();
        let asset_index_path = asset_index_dir.join(format!("{}.json", asset_index_id));
        fs::write(&asset_index_path, &asset_index_content)
            .await
            .map_err(|e| TaskError::ExecutionError(format!("Failed to write asset index: {e}")))?;

        let asset_index: AssetIndexMeta = serde_json::from_str(&asset_index_content)
            .map_err(|e| TaskError::ExecutionError(format!("Failed to parse asset index: {e}")))?;

        if let Some(objects) = &asset_index.objects {
            for (_, obj) in objects {
                let hash = obj.hash.as_ref().unwrap_or(&"".to_string()).clone();
                if hash.is_empty() {
                    continue;
                }

                let hash_prefix = &hash[..2];
                let url = format!(
                    "https://resources.download.minecraft.net/{}/{}",
                    hash_prefix, hash
                );
                let dest_path = format!("assets/objects/{}/{}", hash_prefix, hash);
                let dest = base_dir.join(&dest_path);

                tasks.push(DownloadTask::new(
                    url,
                    dest.to_string_lossy().to_string(),
                    Some(hash),
                    obj.size,
                ));
            }
        }

        // Step E: Run batch download.
        let total_tasks = tasks.len();
        ctx.update_progress(0, total_tasks as u64, &format!("Downloading {} files", total_tasks)).await;
        
        if total_tasks == 0 {
            return Err(TaskError::ExecutionError("No files to download".to_string()));
        }

        run_batch_download_task(tasks, ctx.clone()).await.map_err(|e| TaskError::ExecutionError(e))?;

        if ctx.is_cancelled() {
            let version_dir = base_dir.join("versions").join(version_id);
            let _ = tokio::fs::remove_dir_all(&version_dir).await;
            return Err(TaskError::ExecutionError("Installation cancelled by user".to_string()));
        }

        ctx.update_progress(total_tasks as u64, total_tasks as u64, "Complete").await;

        let version_dir = base_dir.join("versions").join(version_id);
        let config_path = version_dir.join("dlml.json");
        let mut config: crate::core::launcher::InstanceConfig = if config_path.exists() {
            let content = tokio::fs::read_to_string(&config_path)
                .await
                .unwrap_or_else(|_| "{}".to_string());
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            Default::default()
        };

        config.hidden = is_dependency.unwrap_or(false);
        config.is_installing = false;
        
        let config_json = serde_json::to_string_pretty(&config)
            .map_err(|e| TaskError::ExecutionError(format!("Failed to serialize dlml.json: {e}")))?;
        let _ = tokio::fs::write(&config_path, config_json).await;

        Ok(())
    }
}

/// Install a vanilla Minecraft version.
#[tauri::command]
pub async fn install_vanilla_version(
    version_id: String,
    version_json_url: String,
    is_dependency: Option<bool>,
    app: AppHandle,
) -> Result<String, String> {
    use tauri::Manager;
    let task_manager = app.state::<TaskManager>().inner().clone();

    tracing::info!("Starting installation of version: {}", version_id);

    // Pre-create instance directory and dlml.json synchronously so frontend can detect it immediately
    let base_dir = crate::core::mojang::get_minecraft_base();
    let version_dir = base_dir.join("versions").join(&version_id);
    let _ = std::fs::create_dir_all(&version_dir);
    let config_path = version_dir.join("dlml.json");
    let mut pre_config = crate::core::launcher::InstanceConfig::default();
    pre_config.is_installing = true;
    pre_config.hidden = is_dependency.unwrap_or(false);
    let _ = std::fs::write(&config_path, serde_json::to_string_pretty(&pre_config).unwrap());

    let task = InstallVanillaTask {
        options: VanillaInstallOptions {
            version_id: version_id.clone(),
            version_json_url: version_json_url.clone(),
            is_dependency,
        },
    };

    let task_id = task_manager
        .spawn_task(TaskType::InstallVanilla { 
            version_id: version_id.clone(),
            version_json_url: version_json_url.clone(),
            is_dependency,
        }, task)
        .await
        .map_err(|e| e.to_string())?;

    Ok(task_id)
}


/// Get list of installed versions.
#[tauri::command]
pub async fn get_installed_versions() -> Result<Vec<String>, String> {
    let base_dir = get_minecraft_base();
    let versions_dir = base_dir.join("versions");

    if !versions_dir.exists() {
        return Ok(Vec::new());
    }

    let mut versions = Vec::new();

    let entries = fs::read_dir(&versions_dir)
        .await
        .map_err(|e| format!("Failed to read versions directory: {e}"))?;

    let mut dir = tokio::fs::read_dir(&versions_dir)
        .await
        .map_err(|e| format!("Failed to read versions dir: {e}"))?;

    while let Some(entry) = dir
        .next_entry()
        .await
        .map_err(|e| format!("Failed to read entry: {e}"))?
    {
        let path = entry.path();
        if path.is_dir() {
            if let Some(name) = path.file_name() {
                let name_str = name.to_string_lossy().to_string();
                // Check if version JSON exists
                let json_path = path.join(format!("{}.json", name_str));
                if json_path.exists() {
                    versions.push(name_str);
                }
            }
        }
    }

    use crate::core::utils::compare_versions;
    // Sort descending (newest versions first)
    versions.sort_by(|a, b| compare_versions(b, a));
    tracing::info!("Found {} installed versions", versions.len());
    Ok(versions)
}
