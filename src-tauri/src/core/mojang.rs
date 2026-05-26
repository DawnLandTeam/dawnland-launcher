use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::OnceLock;
use tauri::{AppHandle, Emitter};
use tokio::fs;
use tokio::sync::Mutex;

// Re-export downloader types (from parent module)
use crate::downloader::{DownloadTask, run_batch_download};

// ============ Global State ============

/// Base directory for Minecraft files: ~/.dawnland/.minecraft
static MINECRAFT_BASE: OnceLock<PathBuf> = OnceLock::new();

/// Get the Minecraft base directory path.
pub fn get_minecraft_base() -> &'static PathBuf {
    MINECRAFT_BASE.get_or_init(|| {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        home.join(".dawnland").join(".minecraft")
    })
}

/// Global installation state for progress tracking.
static INSTALL_STATE: OnceLock<Mutex<InstallState>> = OnceLock::new();

fn get_global_install_state() -> &'static Mutex<InstallState> {
    INSTALL_STATE.get_or_init(|| Mutex::new(InstallState::default()))
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum InstallPhase {
    #[default]
    Idle,
    ResolvingVersion,
    ResolvingLibraries,
    ResolvingAssets,
    Downloading,
    Complete,
    Error,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallState {
    pub phase: InstallPhase,
    pub version_id: Option<String>,
    pub total_tasks: usize,
    pub completed_tasks: usize,
    pub current_file: Option<String>,
    pub error: Option<String>,
}

impl Default for InstallState {
    fn default() -> Self {
        Self {
            phase: InstallPhase::Idle,
            version_id: None,
            total_tasks: 0,
            completed_tasks: 0,
            current_file: None,
            error: None,
        }
    }
}

impl InstallState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_phase(&mut self, phase: InstallPhase) {
        self.phase = phase;
    }

    pub fn set_total_tasks(&mut self, total: usize) {
        self.total_tasks = total;
    }

    pub fn increment_completed(&mut self) {
        self.completed_tasks += 1;
    }

    pub fn set_current_file(&mut self, file: Option<String>) {
        self.current_file = file;
    }

    pub fn set_error(&mut self, error: String) {
        self.phase = InstallPhase::Error;
        self.error = Some(error);
    }
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
#[derive(Debug, Deserialize)]
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
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetIndex {
    pub id: String,
    pub total_size: Option<u64>,
    pub url: Option<String>, // URL might be missing in some old versions
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Downloads {
    pub client: Option<DownloadInfo>,
    pub server: Option<DownloadInfo>,
    #[serde(rename = "windows_server")]
    pub windows_server: Option<DownloadInfo>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadInfo {
    pub sha1: Option<String>, // May be missing in old versions
    pub size: Option<u64>,    // May be missing
    pub url: Option<String>,  // Critical - might be missing
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Library {
    pub downloads: Option<LibraryDownloads>,
    pub name: Option<String>, // Some libraries may not have a name
    pub url: Option<String>,  // Fabric/Forge use Maven coordinates - base URL
    pub rules: Option<Vec<Rule>>,
    pub extract: Option<ExtractRule>,
    pub natives: Option<std::collections::HashMap<String, String>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryDownloads {
    pub artifact: Option<Artifact>,
    pub classifiers: Option<std::collections::HashMap<String, Artifact>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Artifact {
    pub path: Option<String>,
    pub sha1: Option<String>,
    pub size: Option<u64>,
    pub url: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtractRule {
    pub exclude: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rule {
    pub action: Option<String>, // Some rules may not have action
    pub os: Option<RuleOs>,
    pub features: Option<std::collections::HashMap<String, bool>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleOs {
    pub name: Option<String>,
    pub arch: Option<String>,
    pub version: Option<String>,
}

#[derive(Debug, Deserialize)]
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
        let classifier = if parts.len() == 4 {
            format!("-{}", parts[3])
        } else {
            String::new()
        };
        
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
                if let Some(path) = artifact.get("path").and_then(|p| p.as_str()) {
                    return Some((url.to_string(), path.to_string()));
                }
            }
        }
    }
    
    // Fallback to Maven coordinate format (Fabric/Forge style)
    let path = maven_name_to_path(name)?;
    
    // Determine base URL from lib.url or use default
    let base_url = lib.get("url")
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

    // Process rules in order (usually just one or two).
    let mut should_allow = true;

    for rule in rules {
        let action = match &rule.action {
            Some(a) => a.as_str(),
            None => continue, // Skip rules without action
        };

        match action {
            "allow" => {
                if let Some(ref os) = rule.os {
                    if let Some(os_name) = &os.name {
                        if !matches_current_os(os_name) {
                            continue; // Rule doesn't apply to this OS
                        }
                        if let Some(ref arch) = os.arch {
                            if !matches_current_arch(arch) {
                                should_allow = false;
                                continue;
                            }
                        }
                    }
                    // If os is specified and matches, this allow rule applies
                } else {
                    // No OS restriction, allow everything
                    should_allow = true;
                }
            }
            "disallow" => {
                if let Some(ref os) = rule.os {
                    if let Some(os_name) = &os.name {
                        if !matches_current_os(os_name) {
                            continue;
                        }
                        if let Some(ref arch) = os.arch {
                            if matches_current_arch(arch) {
                                should_allow = false;
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    should_allow
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

/// Install a vanilla Minecraft version.
#[tauri::command]
pub async fn install_vanilla_version(
    version_id: String,
    version_json_url: String,
    app: AppHandle,
) -> Result<(), String> {
    tracing::info!("Starting installation of version: {}", version_id);

    // Initialize install state.
    {
        let mut state = get_global_install_state().lock().await;
        state.phase = InstallPhase::ResolvingVersion;
        state.version_id = Some(version_id.clone());
        state.total_tasks = 0;
        state.completed_tasks = 0;
        state.error = None;
    }

    // Emit initial state.
    let _ = app.emit("install-progress", serde_json::json!({
        "phase": "resolving_version",
        "versionId": version_id,
    }));

    let base_dir = get_minecraft_base();
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {e}"))?;

    // Step A: Download and save version JSON.
    tracing::info!("Downloading version JSON from: {}", version_json_url);

    let version_json_content = client
        .get(&version_json_url)
        .send()
        .await
        .map_err(|e| format!("Failed to download version JSON: {}", e))?
        .text()
        .await
        .map_err(|e| format!("Failed to read version JSON: {}", e))?;

    // Save version JSON.
    let version_dir = base_dir.join("versions").join(&version_id);
    fs::create_dir_all(&version_dir)
        .await
        .map_err(|e| format!("Failed to create version directory: {e}"))?;

    let version_json_path = version_dir.join(format!("{}.json", version_id));
    fs::write(&version_json_path, &version_json_content)
        .await
        .map_err(|e| format!("Failed to write version JSON: {e}"))?;

    tracing::info!("Saved version JSON to: {:?}", version_json_path);

    // Parse version metadata.
    let version_meta: VersionMeta = serde_json::from_str(&version_json_content)
        .map_err(|e| format!("Failed to parse version JSON: {e}"))?;

    // Update state: resolving libraries.
    {
        let mut state = get_global_install_state().lock().await;
        state.phase = InstallPhase::ResolvingLibraries;
    }
    let _ = app.emit("install-progress", serde_json::json!({
        "phase": "resolving_libraries",
    }));

    // Step B: Build libraries download queue.
    let mut tasks: Vec<DownloadTask> = Vec::new();

    // Handle Option<Vec<Library>>
    let libraries = match &version_meta.libraries {
        Some(libs) => libs,
        None => {
            tracing::warn!("No libraries found in version JSON");
            &vec![]
        }
    };

    for lib in libraries {
        let lib_name = lib.name.as_deref().unwrap_or("unknown");
        
        if !should_download_library(lib) {
            tracing::debug!("Skipping library (rules): {}", lib_name);
            continue;
        }

        if let Some(ref downloads) = lib.downloads {
            if let Some(ref artifact) = downloads.artifact {
                let path = artifact.path.as_ref()
                    .map(|p| format!("libraries/{}", p))
                    .unwrap_or_else(|| "libraries/unknown".to_string());
                let dest = base_dir.join(&path);
                
                let url = artifact.url.as_ref()
                    .cloned()
                    .unwrap_or_default();
                let hash = artifact.sha1.as_ref()
                    .cloned();
                
                if !url.is_empty() {
                    tasks.push(DownloadTask::new(
                        url,
                        dest.to_string_lossy().to_string(),
                        hash,
                    ));
                }
            }

            // Handle classifiers (native libraries).
            if let Some(ref classifiers) = downloads.classifiers {
                // Get current OS classifier key.
                let os_key = match std::env::consts::OS {
                    "windows" => "natives-windows",
                    "macos" => "natives-macos",
                    "linux" => "natives-linux",
                    _ => continue,
                };

                if let Some(ref classifier) = classifiers.get(os_key) {
                    let path = classifier.path.as_ref()
                        .map(|p| format!("libraries/{}", p))
                        .unwrap_or_else(|| "libraries/unknown".to_string());
                    let dest = base_dir.join(&path);
                    
                    let url = classifier.url.as_ref()
                        .cloned()
                        .unwrap_or_default();
                    let hash = classifier.sha1.as_ref()
                        .cloned();
                    
                    if !url.is_empty() {
                        tasks.push(DownloadTask::new(
                            url,
                            dest.to_string_lossy().to_string(),
                            hash,
                        ));
                    }
                }
            }
        }
    }

    tracing::info!("Resolved {} library files", tasks.len());

    // Step C: Add client.jar to download queue.
    let downloads = match &version_meta.downloads {
        Some(d) => d,
        None => {
            tracing::error!("No downloads section in version JSON");
            return Err("Version JSON missing downloads section".to_string());
        }
    };
    
    if let Some(ref client_download) = downloads.client {
        let url = client_download.url.as_ref()
            .cloned()
            .unwrap_or_default();
        let hash = client_download.sha1.as_ref()
            .cloned();
        
        if !url.is_empty() {
            let path = format!("versions/{}/{}.jar", version_id, version_id);
            let dest = base_dir.join(&path);
            tasks.push(DownloadTask::new(
                url,
                dest.to_string_lossy().to_string(),
                hash,
            ));
            tracing::info!("Added client.jar to download queue");
        }
    }

    // Update state: resolving assets.
    {
        let mut state = get_global_install_state().lock().await;
        state.phase = InstallPhase::ResolvingAssets;
    }
    let _ = app.emit("install-progress", serde_json::json!({
        "phase": "resolving_assets",
    }));

    // Step D: Download and parse asset index.
    let asset_index = match &version_meta.asset_index {
        Some(ai) => ai,
        None => {
            tracing::error!("No assetIndex in version JSON");
            return Err("Version JSON missing assetIndex".to_string());
        }
    };
    
    let asset_index_url = match &asset_index.url {
        Some(url) if !url.is_empty() => url.clone(),
        _ => {
            tracing::error!("No asset index URL in version JSON");
            return Err("Version JSON missing asset index URL".to_string());
        }
    };
    
    tracing::info!("Downloading asset index: {}", asset_index_url);

    let asset_index_content = client
        .get(&asset_index_url)
        .send()
        .await
        .map_err(|e| format!("Failed to download asset index: {}", e))?
        .text()
        .await
        .map_err(|e| format!("Failed to read asset index: {e}"))?;

    // Save asset index.
    let asset_index_dir = base_dir.join("assets").join("indexes");
    fs::create_dir_all(&asset_index_dir)
        .await
        .map_err(|e| format!("Failed to create asset index directory: {e}"))?;

    let asset_index_id = asset_index.id.clone();
    let asset_index_path = asset_index_dir.join(format!("{}.json", asset_index_id));
    fs::write(&asset_index_path, &asset_index_content)
        .await
        .map_err(|e| format!("Failed to write asset index: {e}"))?;

    // Parse asset index.
    let asset_index: AssetIndexMeta = serde_json::from_str(&asset_index_content)
        .map_err(|e| format!("Failed to parse asset index: {e}"))?;

    // Build assets download queue.
    let total_assets = asset_index.objects.as_ref()
        .map(|obj| obj.len())
        .unwrap_or(0);
    tracing::info!("Resolved {} asset objects", total_assets);

    if let Some(objects) = &asset_index.objects {
        for (path, obj) in objects {
            // Path format: objects/<hash_prefix>/<hash>
            let hash = obj.hash.as_ref()
                .unwrap_or(&"".to_string())
                .clone();
            
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
            ));
        }
    }

    // Update state: downloading.
    let total_tasks = tasks.len();
    {
        let mut state = get_global_install_state().lock().await;
        state.phase = InstallPhase::Downloading;
        state.set_total_tasks(total_tasks);
    }
    let _ = app.emit("install-progress", serde_json::json!({
        "phase": "downloading",
        "totalTasks": total_tasks,
    }));

tracing::info!("Starting download of {} files...", total_tasks);

    // Debug: check if tasks is empty
    if total_tasks == 0 {
        tracing::warn!("No download tasks to process!");
        let _ = app.emit("install-progress", serde_json::json!({
            "phase": "error",
            "error": "No files to download",
        }));
        return Ok(());
    }

    // Use the batch download function which properly emits download-progress events.
    // This will spawn tasks concurrently and emit progress events for each file.
    tracing::info!("Calling run_batch_download with {} tasks", total_tasks);
    let app_for_download = app.clone();
    run_batch_download(tasks, app_for_download).await;
    tracing::info!("run_batch_download completed");

    // Update final state after batch download completes.
    {
        let mut state = get_global_install_state().lock().await;
        state.phase = InstallPhase::Complete;
    }

    let _ = app.emit("install-progress", serde_json::json!({
        "phase": "complete",
        "versionId": version_id,
    }));

    tracing::info!("Installation complete!");
    Ok(())
}

/// Download a single file with async IO.
async fn download_single_file(client: &reqwest::Client, task: &DownloadTask) -> Result<(), String> {
    // Create parent directories.
    let dest_path = PathBuf::from(&task.dest_path);
    if let Some(parent) = dest_path.parent() {
        fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("Failed to create directory: {e}"))?;
    }

    // Download the file.
    let response = client
        .get(&task.url)
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("HTTP error: {}", response.status()));
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("Failed to read body: {e}"))?;

    // Write to file.
    fs::write(&dest_path, &bytes)
        .await
        .map_err(|e| format!("Failed to write file: {e}"))?;

    Ok(())
}

/// Get current installation state.
#[tauri::command]
pub async fn fetch_install_state() -> Result<InstallState, String> {
    let state = get_global_install_state().lock().await;
    Ok(state.clone())
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

    let mut dir = tokio::fs::read_dir(&versions_dir).await.map_err(|e| format!("Failed to read versions dir: {e}"))?;
    
    while let Some(entry) = dir.next_entry().await.map_err(|e| format!("Failed to read entry: {e}"))? {
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