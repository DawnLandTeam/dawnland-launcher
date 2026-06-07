//! Forge & NeoForge Mod Loader Integration
//! Provides commands for fetching available Forge/NeoForge versions and installing them.
//!
//! Uses the "silent installer" approach: downloads the official installer JAR,
//! extracts the necessary files, and uses them to create a launchable instance.

use crate::core::mojang::{get_minecraft_base, InstallVanillaTask, VanillaInstallOptions};
use crate::core::task::{ExecutableTask, TaskContext, TaskError, TaskManager, TaskType};
use crate::core::utils::compare_versions;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};
use tokio::fs;
use tokio::task;

// ============ Data Types ============

/// Response to frontend: list of available Forge/NeoForge versions
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoaderVersionList {
    pub versions: Vec<LoaderVersion>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoaderVersion {
    pub version: String,
    pub mc_version: String,
    pub installer_url: String,
}

/// Forge install profile extracted from installer JAR
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForgeInstallProfile {
    pub version: String,
    pub path: Option<String>,
    pub minecraft: String,
    #[serde(default)]
    pub libraries: Vec<Library>,
    #[serde(default)]
    pub data: HashMap<String, DataEntry>,
    #[serde(default)]
    pub processors: Vec<Processor>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Library {
    pub name: Option<String>,
    pub url: Option<String>,
    pub downloads: Option<LibraryDownloads>,
    pub rules: Option<Vec<Rule>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryDownloads {
    pub artifact: Option<Artifact>,
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
pub struct Rule {
    pub action: Option<String>,
    pub os: Option<RuleOs>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleOs {
    pub name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataEntry {
    #[serde(default)]
    pub client: Option<String>,
    #[serde(default)]
    pub server: Option<String>,
}

/// Processor configuration from Forge installer
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Processor {
    #[serde(default)]
    pub jar: Option<String>,
    #[serde(default)]
    pub classpath: Vec<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub outputs: Option<HashMap<String, String>>,
    #[serde(default)]
    pub min_java_version: Option<i32>,
}

// ============ Constants ============

const FORGE_MAVEN: &str = "https://bmclapi2.bangbang93.com/maven/net/minecraftforge/forge";
const NEOFORGE_MAVEN: &str = "https://bmclapi2.bangbang93.com/maven/net/neoforged/neoforge";
const BMCLAPI_FORGE_BASE: &str = "https://bmclapi2.bangbang93.com/forge";

/// BMCLAPI response structure for Forge versions
#[derive(Debug, serde::Deserialize)]
pub struct BmclForgeVersion {
    pub version: String,
    pub build: Option<u32>,
}

/// Maven XML metadata structure for parsing version lists
#[derive(Debug, Deserialize)]
struct MavenMetadata {
    #[serde(rename = "versioning")]
    versioning: Option<MavenVersioning>,
}

#[derive(Debug, Deserialize)]
struct MavenVersioning {
    #[serde(rename = "version", default)]
    versions: Option<Vec<String>>,
    #[serde(rename = "latest", default)]
    latest: Option<String>,
    #[serde(rename = "release", default)]
    release: Option<String>,
}

// ============ Version Parsing Helpers ============

/// Parse a version string to extract MC version
/// Example: "1.20.1-47.1.0" -> mc_version = "1.20.1", loader = "47.1.0"
fn parse_forge_version(version: &str) -> Option<(String, String)> {
    // Pattern: MC_VERSION-FORGE_VERSION
    if let Some(dash_idx) = version.rfind('-') {
        let mc_version = version[..dash_idx].to_string();
        let forge_version = version[dash_idx + 1..].to_string();

        // Validate MC version format (should start with a number)
        if mc_version.starts_with('1') || mc_version.starts_with('0') {
            return Some((mc_version, forge_version));
        }
    }
    None
}

/// Get all available Forge versions from Maven metadata
async fn fetch_forge_versions(maven_base: &str) -> Result<Vec<String>, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    // Try XML metadata first (more reliable for Maven)
    let url = format!("{}/maven-metadata.xml", maven_base);

    let response = match client.get(&url).send().await {
        Ok(resp) if resp.status().is_success() => resp,
        _ => {
            // Try alternative URL format
            let alt_url = format!("{}/maven-metadata.xml", maven_base.trim_end_matches("/"));
            client
                .get(&alt_url)
                .send()
                .await
                .map_err(|e| format!("Failed to fetch version list: {}", e))?
        }
    };

    if !response.status().is_success() {
        return Err(format!("Failed to fetch versions: {}", response.status()));
    }

    let body = response
        .text()
        .await
        .map_err(|e| format!("Failed to read response: {}", e))?;

    // Parse XML metadata using serde-xml-rs
    let xml_meta: MavenMetadata = serde_xml_rs::from_str(&body)
        .map_err(|e| format!("Failed to parse XML metadata: {}", e))?;

    // Extract versions from the structure
    let versions: Vec<String> = xml_meta
        .versioning
        .and_then(|v| v.versions)
        .unwrap_or_default();

    Ok(versions)
}

/// Fetch available Forge versions, filtered by Minecraft version
/// Uses BMCLAPI for stable and fast access in China
#[tauri::command]
pub async fn get_forge_loaders(mc_version: String) -> Result<LoaderVersionList, String> {
    tracing::info!(
        "Fetching Forge loaders for Minecraft {} via BMCLAPI",
        mc_version
    );

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    // Use BMCLAPI for stable access - correct path includes /minecraft/
    let url = format!("{}/minecraft/{}", BMCLAPI_FORGE_BASE, mc_version);
    tracing::info!("Requesting Forge versions from BMCLAPI: {}", url);

    // Add User-Agent to avoid WAF blocking
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .user_agent("Dawnland-Launcher/1.0")
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    let response = match client.get(&url).send().await {
        Ok(resp) => resp,
        Err(e) => {
            tracing::warn!("BMCLAPI request failed: {}", e);
            return Ok(LoaderVersionList { versions: vec![] });
        }
    };

    if !response.status().is_success() {
        tracing::warn!(
            "BMCLAPI returned error status for Forge: {}",
            response.status()
        );
        return Ok(LoaderVersionList { versions: vec![] });
    }

    let mut versions: Vec<BmclForgeVersion> = match response.json().await {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("Failed to parse Forge response: {}", e);
            return Ok(LoaderVersionList { versions: vec![] });
        }
    };

    // Sort versions descending (newest first) using numeric comparison
    versions.sort_by(|a, b| compare_versions(&b.version, &a.version));

    // Convert to LoaderVersion format with proper installer URLs
    // BMCLAPI returns versions like "41.1.0", we need to prepend MC version for full version
    let loader_versions: Vec<LoaderVersion> = versions
        .into_iter()
        .map(|v| {
            let full_version = format!("{}-{}", mc_version, v.version);
            LoaderVersion {
                version: full_version.clone(),
                mc_version: mc_version.clone(),
                installer_url: format!(
                    "{}/{}/forge-{}-installer.jar",
                    FORGE_MAVEN, full_version, full_version
                ),
            }
        })
        .collect();

    tracing::info!(
        "Found {} Forge versions for MC {}",
        loader_versions.len(),
        mc_version
    );

    Ok(LoaderVersionList {
        versions: loader_versions,
    })
}

/// Fetch available NeoForge versions, filtered by Minecraft version
/// Uses BMCLAPI for stable access - correct endpoint: /neoforge/list/{mc_version}
#[tauri::command]
pub async fn get_neoforge_loaders(mc_version: String) -> Result<LoaderVersionList, String> {
    tracing::info!("Fetching NeoForge loaders for Minecraft {}", mc_version);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .user_agent("Dawnland-Launcher/1.0")
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    // Use correct BMCLAPI NeoForge endpoint
    let url = format!(
        "https://bmclapi2.bangbang93.com/neoforge/list/{}",
        mc_version
    );
    tracing::info!("Requesting NeoForge versions from BMCLAPI: {}", url);

    let response = match client.get(&url).send().await {
        Ok(resp) => resp,
        Err(e) => {
            tracing::warn!("BMCLAPI NeoForge request failed: {}", e);
            return Ok(LoaderVersionList { versions: vec![] });
        }
    };

    if !response.status().is_success() {
        tracing::warn!("BMCLAPI NeoForge returned status: {}", response.status());
        return Ok(LoaderVersionList { versions: vec![] });
    }

    // Dynamic JSON parsing - BMCLAPI returns array of objects with version field
    let json_data: serde_json::Value = match response.json().await {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("Failed to parse NeoForge response: {}", e);
            return Ok(LoaderVersionList { versions: vec![] });
        }
    };

    let mut versions: Vec<String> = Vec::new();

    if let Some(array) = json_data.as_array() {
        for item in array {
            if let Some(obj) = item.as_object() {
                // BMCLAPI returns: {"version": "47.1.5", "mcversion": "1.20.1", ...}
                // or newer format: {"version": "1.20.1-47.1.85", ...}
                if let Some(v) = obj.get("version").and_then(|v| v.as_str()) {
                    // If version doesn't include MC version, prepend it
                    if v.starts_with(&mc_version) {
                        versions.push(v.to_string());
                    } else {
                        versions.push(format!("{}-{}", mc_version, v));
                    }
                }
            }
        }
    }
    // Sort to have latest version first using numeric comparison
    versions.sort_by(|a, b| compare_versions(b, a));

    tracing::info!(
        "Found {} NeoForge versions for MC {}",
        versions.len(),
        mc_version
    );

    // Convert to LoaderVersion format
    let loader_versions: Vec<LoaderVersion> = versions
        .into_iter()
        .map(|v| LoaderVersion {
            version: v.clone(),
            mc_version: mc_version.clone(),
            installer_url: format!("{}/{}/forge-{}-installer.jar", NEOFORGE_MAVEN, v, v),
        })
        .collect();

    Ok(LoaderVersionList {
        versions: loader_versions,
    })
}

/// Extract files from a ZIP (JAR) archive - synchronous version to avoid Send issues
fn extract_zip_entry_sync(
    zip_path: &PathBuf,
    entry_name: &str,
    dest_path: &PathBuf,
) -> Result<(), String> {
    let file = std::fs::File::open(zip_path).map_err(|e| format!("Failed to open ZIP: {}", e))?;

    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| format!("Failed to read ZIP archive: {}", e))?;

    let mut entry = archive
        .by_name(entry_name)
        .map_err(|e| format!("Entry {} not found in ZIP: {}", entry_name, e))?;

    let mut buffer = Vec::new();
    entry
        .read_to_end(&mut buffer)
        .map_err(|e| format!("Failed to read entry: {}", e))?;

    // Create parent directories (this part is fine to do outside the blocking call)
    if let Some(parent) = dest_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create directory: {}", e))?;
    }

    std::fs::write(dest_path, &buffer)
        .map_err(|e| format!("Failed to write extracted file: {}", e))?;

    Ok(())
}

/// Extract files from a ZIP (JAR) archive - async wrapper
async fn extract_zip_entry(
    zip_path: &PathBuf,
    entry_name: &str,
    dest_path: &PathBuf,
) -> Result<(), String> {
    // Use spawn_blocking to run the synchronous ZIP operations
    let zip_path = zip_path.clone();
    let entry_name = entry_name.to_string();
    let dest_path = dest_path.clone();

    tokio::task::spawn_blocking(move || extract_zip_entry_sync(&zip_path, &entry_name, &dest_path))
        .await
        .map_err(|e| format!("Task join error: {}", e))?
}

/// Check if a library should be downloaded based on rules
fn should_download_library_json(lib: &serde_json::Value) -> bool {
    // If no rules, always download.
    let rules = match lib.get("rules").and_then(|r| r.as_array()) {
        Some(r) if r.is_empty() => return true,
        Some(r) => r,
        None => return true,
    };

    let mut should_allow = true;

    for rule in rules {
        let action = rule.get("action").and_then(|a| a.as_str());

        match action {
            Some("allow") => {
                if let Some(os) = rule.get("os") {
                    if let Some(os_name) = os.get("name").and_then(|n| n.as_str()) {
                        if matches_current_os(os_name) {
                            should_allow = true;
                        }
                    }
                } else {
                    should_allow = true;
                }
            }
            Some("disallow") => {
                if let Some(os) = rule.get("os") {
                    if let Some(os_name) = os.get("name").and_then(|n| n.as_str()) {
                        if matches_current_os(os_name) {
                            should_allow = false;
                        }
                    }
                }
            }
            _ => {}
        }
    }

    should_allow
}

fn matches_current_os(rule_os: &str) -> bool {
    let current_os = std::env::consts::OS;
    match rule_os {
        "windows" => current_os == "windows",
        "osx" | "macos" => current_os == "macos",
        "linux" => current_os == "linux",
        _ => false,
    }
}

/// Convert Maven coordinate to local file path
/// Example: "net.minecraft:client:1.20.1-patched" -> "net/minecraft/client/1.20.1/client-1.20.1-patched.jar"
fn maven_name_to_path(name: &str) -> Option<String> {
    let parts: Vec<&str> = name.split(':').collect();
    if parts.len() >= 3 {
        let group = parts[0].replace('.', "/");
        let artifact = parts[1];
        let version = parts[2];
        // Handle classifier (e.g., "patched")
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

/// Get download URL for a library from its JSON definition
fn get_library_download_info_json(lib: &serde_json::Value) -> Option<(String, String)> {
    // Try downloads.artifact first (this is the standard Mojang/Forge format)
    if let Some(downloads) = lib.get("downloads") {
        if let Some(artifact) = downloads.get("artifact") {
            if let Some(url) = artifact.get("url").and_then(|u| u.as_str()) {
                if !url.is_empty() {
                    if let Some(path) = artifact.get("path").and_then(|p| p.as_str()) {
                        // artifact.path is relative to libraries/ root, e.g. "net/minecraftforge/..."
                        // or for patched client: "net/minecraft/client/1.20.1/client-1.20.1-patched.jar"
                        let lib_path = if path.starts_with("libraries/") {
                            path.to_string()
                        } else {
                            format!("libraries/{}", path)
                        };
                        return Some((url.to_string(), lib_path));
                    }
                }
            }
        }
    }

    // Fallback to Maven coordinate format
    // Handle special cases like "net.minecraft:client:1.20.1:patched"
    let name = lib.get("name")?.as_str()?;
    let path = maven_name_to_path(name)?;

    // Get base URL - for Minecraft libraries use Mojang Maven
    let group = name.split(':').next().unwrap_or("");
    let base_url = if group == "net.minecraft" {
        "https://libraries.minecraft.net/"
    } else if group == "net.minecraftforge" {
        "https://maven.minecraftforge.net/"
    } else if group.starts_with("net.neoforged") {
        "https://maven.neoforged.net/"
    } else {
        lib.get("url")
            .and_then(|u| u.as_str())
            .unwrap_or("https://libraries.minecraft.net/")
    };

    let download_url = format!("{}{}", base_url, path);

    Some((download_url, format!("libraries/{}", path)))
}

pub struct InstallForgeOptions {
    pub mc_version: String,
    pub loader_version: String,
    pub loader_type: String, // "forge" or "neoforge"
    pub custom_instance_name: String,
    pub is_dependency: Option<bool>,
}

pub struct InstallForgeTask {
    pub options: InstallForgeOptions,
}

#[async_trait::async_trait]
impl ExecutableTask for InstallForgeTask {
    async fn execute(&self, ctx: TaskContext) -> Result<(), TaskError> {
        let mc_version = &self.options.mc_version;
        let loader_version = &self.options.loader_version;
        let loader_type = &self.options.loader_type;
        let custom_instance_name = &self.options.custom_instance_name;
        let is_dependency = self.options.is_dependency;
    tracing::info!(
        "Installing {} instance: {} (MC {} + Loader {})",
        loader_type,
        custom_instance_name,
        mc_version,
        loader_version
    );

    let base_dir = get_minecraft_base();
    let instance_dir = base_dir.join("versions").join(custom_instance_name);

    let _ = tokio::fs::create_dir_all(&instance_dir).await;
    
    let maven_base = if loader_type == "neoforge" {
        NEOFORGE_MAVEN
    } else {
        FORGE_MAVEN
    };

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| TaskError::ExecutionError(format!("Failed to create HTTP client: {}", e)))?;

    // ========== Step 1: Ensure base vanilla version is installed ==========
    let base_version_dir = base_dir.join("versions").join(&mc_version);
    let base_client_jar = base_version_dir.join(format!("{}.jar", mc_version));
    let base_version_json = base_version_dir.join(format!("{}.json", mc_version));

    ctx.manager.wait_for_instance(&mc_version, &ctx.cancel_token).await;

    if !base_client_jar.exists() || !base_version_json.exists() {
        tracing::info!(
            "Base Minecraft {} not installed, installing first...",
            mc_version
        );

        ctx.update_progress(0, 0, "Installing base Minecraft..."
            ).await;

        // Get version JSON URL from Mojang
        let manifest_url = "https://launchermeta.mojang.com/mc/game/version_manifest_v2.json";
        let manifest: serde_json::Value = client
            .get(manifest_url)
            .send()
            .await
            .map_err(|e| TaskError::ExecutionError(format!("Failed to fetch version manifest: {}", e)))?
            .json()
            .await
            .map_err(|e| TaskError::ExecutionError(format!("Failed to parse version manifest: {}", e)))?;

        // Find the version URL
        let version_url = manifest["versions"]
            .as_array()
            .and_then(|versions| {
                versions
                    .iter()
                    .find(|v| v["id"].as_str() == Some(&mc_version))
            })
            .and_then(|v| v["url"].as_str())
            .ok_or_else(|| TaskError::ExecutionError(format!("Version {} not found in manifest", mc_version)))?;

        let vanilla_task = InstallVanillaTask {
            options: VanillaInstallOptions {
                version_id: mc_version.clone(),
                version_json_url: version_url.to_string(),
                is_dependency: Some(true),
            },
        };
        vanilla_task.execute(ctx.clone()).await?;

        tracing::info!("Base vanilla {} installed successfully", mc_version);
    } else {
        tracing::info!("Base Minecraft {} already installed, skipping", mc_version);
    }

    
        if ctx.is_cancelled() {
            return Err(TaskError::ExecutionError("Installation cancelled by user".to_string()));
        }

    let is_dep = self.options.is_dependency.unwrap_or(false);

    if !is_dep {
        ctx.set_total_steps(3).await;
        // ========== Step 2: Download and extract Forge installer ==========
        ctx.next_step("Downloading Forge installer...").await;
    } else {
        ctx.update_progress(0, 0, "Downloading Forge installer...").await;
    }

    let prefix = if loader_type == "neoforge" {
        "neoforge"
    } else {
        "forge"
    };

    let full_loader_version = if loader_type == "neoforge" {
        loader_version.to_string()
    } else {
        if loader_version.starts_with(&format!("{}-", mc_version)) {
            loader_version.to_string()
        } else {
            format!("{}-{}", mc_version, loader_version)
        }
    };

    let installer_url = format!(
        "{}/{}/{}-{}-installer.jar",
        maven_base, full_loader_version, prefix, full_loader_version
    );

    // Create temp directory for installer
    let temp_dir = std::env::temp_dir().join(format!("dawnland-forge-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir)
        .await
        .map_err(|e| TaskError::ExecutionError(format!("Failed to create temp directory: {}", e)))?;

    let installer_path = temp_dir.join(format!("{}-{}-installer.jar", prefix, full_loader_version));

    // Download installer
    tracing::info!("Downloading Forge installer from: {}", installer_url);

    let response = client
        .get(&installer_url)
        .send()
        .await
        .map_err(|e| TaskError::ExecutionError(format!("Failed to download installer: {}", e)))?;

    if !response.status().is_success() {
        return Err(TaskError::ExecutionError(format!("Installer download failed with status: {}. URL: {}", response.status(), installer_url)));
    }

    let installer_bytes = response
        .bytes()
        .await
        .map_err(|e| TaskError::ExecutionError(format!("Failed to read installer bytes: {}", e)))?;

    fs::write(&installer_path, &installer_bytes)
        .await
        .map_err(|e| TaskError::ExecutionError(format!("Failed to save installer: {}", e)))?;

    tracing::info!("Installer saved to: {:?}", installer_path);

    // Extract install_profile.json and version.json from installer
    let extract_dir = temp_dir.join("extracted");
    fs::create_dir_all(&extract_dir)
        .await
        .map_err(|e| TaskError::ExecutionError(format!("Failed to create extract directory: {}", e)))?;

    let profile_path = extract_dir.join("install_profile.json");
    let version_json_path = extract_dir.join("version.json");

    // Extract files from JAR (which is a ZIP)
    extract_zip_entry(&installer_path, "install_profile.json", &profile_path).await.map_err(|e| TaskError::ExecutionError(e))?;
    extract_zip_entry(&installer_path, "version.json", &version_json_path).await.map_err(|e| TaskError::ExecutionError(e))?;

    tracing::info!("Extracted installer profile and version JSON");

    // Read and parse the install profile
    let profile_content = fs::read_to_string(&profile_path)
        .await
        .map_err(|e| TaskError::ExecutionError(format!("Failed to read install profile: {}", e)))?;

    let install_profile: ForgeInstallProfile = serde_json::from_str(&profile_content)
        .map_err(|e| TaskError::ExecutionError(format!("Failed to parse install profile: {}", e)))?;

    // Read version JSON from installer
    let version_json_content = fs::read_to_string(&version_json_path)
        .await
        .map_err(|e| TaskError::ExecutionError(format!("Failed to read version JSON: {}", e)))?;

    let mut version_json: serde_json::Value = serde_json::from_str(&version_json_content)
        .map_err(|e| TaskError::ExecutionError(format!("Failed to parse version JSON: {}", e)))?;

    let original_id = version_json
        .get("id")
        .and_then(|id| id.as_str())
        .unwrap_or("")
        .to_string();
    tracing::info!("Parsed Forge version JSON, original id: {:?}", original_id);

    
        if ctx.is_cancelled() {
            return Err(TaskError::ExecutionError("Installation cancelled by user".to_string()));
        }
        // ========== Step 3: Download Forge libraries ==========
    ctx.update_progress(0, 0, "Complete").await;

    let mut tasks: Vec<crate::downloader::DownloadTask> = Vec::new();

    // Add libraries from the version JSON
    if let Some(libraries) = version_json.get("libraries").and_then(|l| l.as_array()) {
        tracing::info!(
            "Processing {} libraries from Forge version JSON",
            libraries.len()
        );

        for lib in libraries {
            // Debug: log library name
            if let Some(name) = lib.get("name").and_then(|n| n.as_str()) {
                if name.contains("patched") || name.contains("client") {
                    tracing::debug!("Processing client/patched library: {}", name);
                }
            }

            if !should_download_library_json(lib) {
                continue;
            }

            if let Some((url, path)) = get_library_download_info_json(lib) {
                let dest = base_dir.join(&path);
                tracing::debug!("Added Forge library: {} -> {}", path, url);
                tasks.push(crate::downloader::DownloadTask::new(
                    url,
                    dest.to_string_lossy().to_string(),
                    None,
                    None,
                ));
            }
        }
    }

    let total_tasks = tasks.len();
    tracing::info!("Resolved {} Forge library files", total_tasks);



    if !tasks.is_empty() {
        if let Err(e) = crate::downloader::run_batch_download_task(tasks, ctx.clone()).await {
            tracing::warn!("Installation failed during batch download, cleaning up...");
            let version_dir = base_dir.join("versions").join(&custom_instance_name);
            let _ = tokio::fs::remove_dir_all(&version_dir).await;
            return Err(TaskError::ExecutionError(e));
        }
    }

    if ctx.is_cancelled() {
        tracing::warn!("Installation cancelled, cleaning up forge instance directory...");
        let version_dir = base_dir.join("versions").join(&custom_instance_name);
        let _ = tokio::fs::remove_dir_all(&version_dir).await;
        return Err(TaskError::ExecutionError("Installation cancelled by user".to_string()));
    }

    
        if ctx.is_cancelled() {
            return Err(TaskError::ExecutionError("Installation cancelled by user".to_string()));
        }
        // ========== Step 3.5: Run Forge Installer Processors ==========
    if !is_dep {
        ctx.next_step("Running Forge processors (this may take a while)...").await;
    } else {
        ctx.update_progress(0, 0, "Running Forge processors (this may take a while)...").await;
    }

    tracing::info!("Running Forge installer processors...");

    let mut java_exec = "java".to_string();
    if let Ok(javas) = crate::core::java::scan_local_javas().await {
        if !javas.is_empty() {
            java_exec = javas[0].path.clone();
        }
    }

    // Forge/NeoForge installer requires launcher_profiles.json to exist, otherwise it aborts
    let launcher_profiles_path = base_dir.join("launcher_profiles.json");
    if !launcher_profiles_path.exists() {
        if let Err(e) = fs::write(&launcher_profiles_path, "{ \"profiles\": {} }").await {
            tracing::warn!("Failed to create dummy launcher_profiles.json: {}", e);
        } else {
            tracing::info!("Created dummy launcher_profiles.json for the installer");
        }
    }

    let mut child = match crate::core::utils::create_hidden_command(&java_exec)
        .arg("-jar")
        .arg(&installer_path)
        .arg("--installClient")
        .arg(base_dir.to_string_lossy().to_string())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            let err_msg = format!(
                "Failed to execute Forge installer: {}. Make sure Java is installed.",
                e
            );
            tracing::warn!("{}", err_msg);
            return Err(TaskError::ExecutionError(err_msg));
        }
    };

    let stdout = child.stdout.take().expect("Failed to get stdout");
    let stderr = child.stderr.take().expect("Failed to get stderr");

    use tokio::io::AsyncBufReadExt;
    let mut stdout_reader = tokio::io::BufReader::new(stdout).lines();
    let mut stderr_reader = tokio::io::BufReader::new(stderr).lines();

    let ctx_clone1 = ctx.clone();
    let stdout_task = tokio::spawn(async move {
        while let Ok(Some(line)) = stdout_reader.next_line().await {
            tracing::debug!("Forge stdout: {}", line);
            let display_line = if line.len() > 120 {
                format!("{}...", &line[..120])
            } else {
                line
            };
            ctx_clone1.update_progress(0, 0, &display_line).await;
        }
    });

    let ctx_clone2 = ctx.clone();
    let stderr_task = tokio::spawn(async move {
        while let Ok(Some(line)) = stderr_reader.next_line().await {
            tracing::warn!("Forge stderr: {}", line);
            let display_line = if line.len() > 120 {
                format!("{}...", &line[..120])
            } else {
                line
            };
            ctx_clone2.update_progress(0, 0, &display_line).await;
        }
    });

    let status = child
        .wait()
        .await
        .map_err(|e| TaskError::ExecutionError(format!("Failed to wait on Forge installer: {}", e)))?;

    let _ = stdout_task.await;
    let _ = stderr_task.await;

    if !status.success() {
        let err_msg = format!(
            "Forge installer failed to run processors with exit code: {:?}",
            status.code()
        );
        tracing::error!("{}", err_msg);
        return Err(TaskError::ExecutionError(err_msg));
    } else {
        tracing::info!("Forge installer processors completed successfully");
    }

    
        if ctx.is_cancelled() {
            return Err(TaskError::ExecutionError("Installation cancelled by user".to_string()));
        }
        // ========== Step 4: Create the final version JSON ==========
    if !is_dep {
        ctx.next_step("Setting up Forge instance...").await;
    } else {
        ctx.update_progress(0, 0, "Setting up Forge instance...").await;
    }
    let version_dir = base_dir.join("versions").join(&custom_instance_name);

    fs::create_dir_all(&version_dir)
        .await
        .map_err(|e| TaskError::ExecutionError(format!("Failed to create version directory: {}", e)))?;

    // Modify the version JSON to use our custom instance name
    if let Some(obj) = version_json.as_object_mut() {
        obj.insert("id".to_string(), serde_json::json!(custom_instance_name));
        // Point to base version for inheritsFrom
        obj.insert("inheritsFrom".to_string(), serde_json::json!(mc_version));

        // Fix logging file reference
        if let Some(logging) = obj.get_mut("logging") {
            if let Some(log_obj) = logging.as_object_mut() {
                if let Some(client) = log_obj.get_mut("client") {
                    if let Some(c_obj) = client.as_object_mut() {
                        if let Some(file) = c_obj.get_mut("file") {
                            if let Some(file_obj) = file.as_object_mut() {
                                let log_id = format!("{}-client", mc_version);
                                file_obj.insert("id".to_string(), serde_json::json!(log_id));
                            }
                        }
                    }
                }
            }
        }
    }

    let final_version_json_path = version_dir.join(format!("{}.json", custom_instance_name));
    let final_json = serde_json::to_string_pretty(&version_json)
        .map_err(|e| TaskError::ExecutionError(format!("Failed to serialize version JSON: {}", e)))?;

    fs::write(&final_version_json_path, &final_json)
        .await
        .map_err(|e| TaskError::ExecutionError(format!("Failed to write version JSON: {}", e)))?;

    tracing::info!(
        "Saved Forge version profile to: {:?}",
        final_version_json_path
    );

    
        if ctx.is_cancelled() {
            return Err(TaskError::ExecutionError("Installation cancelled by user".to_string()));
        }
        // ========== Step 5: Create default dlml.json config ==========
    let config_path = version_dir.join("dlml.json");
    let mut config: crate::core::launcher::InstanceConfig = if config_path.exists() {
        let content = tokio::fs::read_to_string(&config_path)
            .await
            .unwrap_or_else(|_| "{}".to_string());
        serde_json::from_str(&content).unwrap_or_default()
    } else {
        crate::core::launcher::InstanceConfig {
            java_path: None,
            max_memory: None,
            jvm_args_extra: None,
            window_behavior: "keep".to_string(),
            show_game_log: false,
            hidden: false,
            server_id: None,
            pack_version_id: None,
            pack_file_name: None,
            is_installing: false,
        }
    };

    config.hidden = is_dependency.unwrap_or(false);
    config.is_installing = false;

    let config_json = serde_json::to_string_pretty(&config)
        .map_err(|e| TaskError::ExecutionError(format!("Failed to serialize instance config: {}", e)))?;

    fs::write(&config_path, config_json)
        .await
        .map_err(|e| TaskError::ExecutionError(format!("Failed to write instance config: {}", e)))?;

    tracing::info!("Created instance config at: {:?}", config_path);

    
        if ctx.is_cancelled() {
            return Err(TaskError::ExecutionError("Installation cancelled by user".to_string()));
        }
        // ========== Step 6: Cleanup temp files and installer artifacts ==========
    // Note: In production, we might want to keep the installer for potential re-installs
    let _ = fs::remove_dir_all(&temp_dir).await;

    // The Forge/NeoForge installer automatically creates a profile in `versions/<original_id>`.
    // Since we created our own custom version directory, we should delete the one generated by the installer.
    if !original_id.is_empty() && original_id != *custom_instance_name {
        let installer_generated_dir = base_dir.join("versions").join(&original_id);
        if installer_generated_dir.exists() {
            let _ = fs::remove_dir_all(&installer_generated_dir).await;
            tracing::info!(
                "Cleaned up installer-generated directory: {:?}",
                installer_generated_dir
            );
        }
    }

    // Emit complete
    ctx.update_progress(0, 0, "Complete").await;

    tracing::info!(
        "{} instance '{}' installed successfully!",
        if loader_type == "neoforge" {
            "NeoForge"
        } else {
            "Forge"
        },
        custom_instance_name
    );

    Ok(())
}
}


/// Install a Forge/NeoForge instance
#[tauri::command]
pub async fn install_forge_instance(
    mc_version: String,
    loader_version: String,
    loader_type: String, // "forge" or "neoforge"
    custom_instance_name: String,
    is_dependency: Option<bool>,
    app: AppHandle,
) -> Result<String, String> {
    let task_manager = app.state::<TaskManager>().inner().clone();

    // Pre-create instance directory and dlml.json synchronously so frontend can detect it immediately
    let base_dir = crate::core::mojang::get_minecraft_base();
    let instance_dir = base_dir.join("versions").join(&custom_instance_name);
    let _ = std::fs::create_dir_all(&instance_dir);
    let config_path = instance_dir.join("dlml.json");
    let mut pre_config = crate::core::launcher::InstanceConfig::default();
    pre_config.is_installing = true;
    pre_config.hidden = is_dependency.unwrap_or(false);
    let _ = std::fs::write(&config_path, serde_json::to_string_pretty(&pre_config).unwrap());
    
    let task = InstallForgeTask {
        options: InstallForgeOptions {
            mc_version: mc_version.clone(),
            loader_version: loader_version.clone(),
            loader_type: loader_type.clone(),
            custom_instance_name: custom_instance_name.clone(),
            is_dependency,
        },
    };
    
    let task_id = task_manager
        .spawn_task(TaskType::InstallForge { 
            mc_version: mc_version.clone(), 
            loader_version: loader_version.clone(),
            loader_type: loader_type.clone(),
            custom_instance_name: custom_instance_name.clone(),
            is_dependency,
        }, task)
        .await
        .map_err(|e| e.to_string())?;

    Ok(task_id)
}
