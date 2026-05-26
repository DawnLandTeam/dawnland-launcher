//! Minecraft Launcher Engine
//! Handles JVM process spawning, natives extraction, and classpath construction.

use crate::auth::Account;
use crate::core::mojang::{get_minecraft_base, maven_name_to_path, Library, Rule, VersionMeta};
use crate::downloader::{DownloadTask, run_batch_download};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use sysinfo::System;
use tauri::{AppHandle, Emitter, Manager};
use tokio::fs;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

// ============ Constants ============

#[cfg(target_os = "windows")]
const CLASSPATH_SEPARATOR: &str = ";";
#[cfg(not(target_os = "windows"))]
const CLASSPATH_SEPARATOR: &str = ":";

const LAUNCHER_NAME: &str = "Dawnland";
const LAUNCHER_VERSION: &str = "1.0.0";

// ============ Helper Functions ============

/// Get system total memory in MB.
fn get_system_memory_mb() -> u32 {
    let mut sys = System::new_all();
    sys.refresh_all();
    let total_bytes = sys.total_memory();
    // Convert bytes to MB, be conservative (floor)
    (total_bytes / (1024 * 1024)) as u32
}

/// Get recommended max memory based on system RAM.
/// Returns 1/3 of system memory, capped between 1024MB and 8192MB.
fn get_recommended_max_memory() -> u32 {
    let system_memory = get_system_memory_mb();
    let recommended = system_memory / 3;
    
    // Clamp to reasonable bounds
    recommended.max(1024).min(8192)
}

/// Normalize OS name from Rust to JSON format
/// Rust: "macos" -> JSON: "osx"
fn normalize_os_name(os: &str) -> String {
    match os {
        "macos" => "osx".to_string(),
        "windows" => "windows".to_string(),
        "linux" => "linux".to_string(),
        "freebsd" => "freebsd".to_string(),
        _ => os.to_string(),
    }
}

/// Normalize architecture from Rust to JSON format
/// Rust: "x86_64" -> JSON: "x64", "aarch64" -> JSON: "arm64"
fn normalize_arch(arch: &str) -> String {
    match arch {
        "x86_64" => "x64".to_string(),
        "aarch64" => "arm64".to_string(),
        "x86" => "x86".to_string(),
        _ => arch.to_string(),
    }
}

/// Check if a rule's OS condition matches the current system
fn matches_os_condition(os_name: Option<&str>, os_arch: Option<&str>) -> bool {
    let current_os = normalize_os_name(std::env::consts::OS);
    let current_arch = normalize_arch(std::env::consts::ARCH);

    // Check OS name
    if let Some(name) = os_name {
        if name != current_os {
            return false;
        }
    }

    // Check OS arch (only if specified)
    if let Some(arch) = os_arch {
        // Handle common patterns
        let arch_matches = match arch {
            "x64" | "64" => current_arch == "x64" || current_arch == "x86_64",
            "x86" | "32" => current_arch == "x86",
            "arm64" => current_arch == "arm64" || current_arch == "aarch64",
            _ => arch == current_arch.as_str(),
        };
        if !arch_matches {
            return false;
        }
    }

    true
}

// ============ Launch Configuration ============

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchConfig {
    pub version_id: String,
    pub account: Account,
    pub game_directory: PathBuf,
    pub assets_directory: PathBuf,
    pub natives_directory: PathBuf,
    pub classpath: Vec<PathBuf>,
    pub main_class: String,
    pub jvm_args: Vec<String>,
    pub game_args: Vec<String>,
}

/// Instance-specific configuration stored in versions/<id>/dlml.json
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct InstanceConfig {
    /// Custom Java executable path. If None, falls back to system default.
    pub java_path: Option<String>,
    /// Maximum heap memory in MB (e.g., 4096 for 4GB). If None, uses default.
    pub max_memory: Option<u32>,
    /// JVM arguments to append (advanced)
    pub jvm_args_extra: Option<Vec<String>>,
    /// Window behavior when game starts: "hide" | "minimize" | "keep"
    #[serde(default = "default_window_behavior")]
    pub window_behavior: String,
    /// Whether to show the game log window automatically on launch
    #[serde(default = "default_show_game_log")]
    pub show_game_log: bool,
}

fn default_window_behavior() -> String {
    "keep".to_string()
}

fn default_show_game_log() -> bool {
    false
}

// ============ Instance Config Commands ============

const CONFIG_FILENAME: &str = "dlml.json";

/// Get instance configuration
#[tauri::command]
pub async fn get_instance_config(version_id: String) -> Result<InstanceConfig, String> {
    let base_dir = get_minecraft_base();
    let config_path = base_dir
        .join("versions")
        .join(&version_id)
        .join(CONFIG_FILENAME);

    if !config_path.exists() {
        tracing::info!("No config found for {}, using defaults", version_id);
        return Ok(InstanceConfig::default());
    }

    let content = tokio::fs::read_to_string(&config_path)
        .await
        .map_err(|e| format!("Failed to read config: {}", e))?;

    let config: InstanceConfig = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse config: {}", e))?;

    tracing::info!("Loaded config for {}: {:?}", version_id, config);
    Ok(config)
}

/// Save instance configuration
#[tauri::command]
pub async fn save_instance_config(
    version_id: String, 
    config: InstanceConfig
) -> Result<(), String> {
    let base_dir = get_minecraft_base();
    let version_dir = base_dir.join("versions").join(&version_id);
    let config_path = version_dir.join(CONFIG_FILENAME);

    // Create version directory if it doesn't exist
    if !version_dir.exists() {
        tokio::fs::create_dir_all(&version_dir)
            .await
            .map_err(|e| format!("Failed to create version directory: {}", e))?;
    }

    let content = serde_json::to_string_pretty(&config)
        .map_err(|e| format!("Failed to serialize config: {}", e))?;

    tokio::fs::write(&config_path, content)
        .await
        .map_err(|e| format!("Failed to write config: {}", e))?;

    tracing::info!("Saved config for {}: {:?}", version_id, config);
    Ok(())
}

// ============ Natives Extraction ============

/// Extract native libraries for the current platform.
/// Returns the path to the natives directory.
pub async fn extract_natives(
    version_id: &str,
    libraries: &[Library],
) -> Result<PathBuf, String> {
    let base_dir = get_minecraft_base();
    let natives_dir = base_dir
        .join("versions")
        .join(version_id)
        .join("natives");

    // Create natives directory
    fs::create_dir_all(&natives_dir)
        .await
        .map_err(|e| format!("Failed to create natives directory: {e}"))?;

    // Get current platform info
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;

    // Find libraries with natives for current platform
    for lib in libraries {
        // Check if library applies to current platform
        if !should_use_library(lib, os, arch) {
            continue;
        }

        // Check if this library has natives
        let Some(natives) = &lib.natives else {
            continue;
        };

        // Get the native classifier for current platform
        let classifier = match (os, arch) {
            ("windows", "x86_64") => natives.get("windows").or(natives.get("natives-windows")),
            ("windows", "x86") => natives.get("windows-x86").or(natives.get("natives-windows")),
            ("macos", "x86_64") => natives.get("osx").or(natives.get("natives-osx")),
            ("macos", "aarch64") => natives.get("osx-arm64").or(natives.get("natives-osx")),
            ("linux", "x86_64") => natives.get("linux").or(natives.get("natives-linux")),
            ("linux", "x86") => natives.get("linux-x86").or(natives.get("natives-linux")),
            _ => None,
        };

        let Some(_classifier) = classifier else {
            continue;
        };

        // Get the library JAR path from classifiers
        let classifiers = match lib.downloads.as_ref().and_then(|d| d.classifiers.as_ref()) {
            Some(c) => c,
            None => continue,
        };

        // Find the classifier key (e.g., "natives-windows")
        let classifier_key = match (os, arch) {
            ("windows", "x86_64") => "natives-windows",
            ("windows", "x86") => "natives-windows-x86",
            ("macos", "x86_64") => "natives-osx",
            ("macos", "aarch64") => "natives-osx-arm64",
            ("linux", "x86_64") => "natives-linux",
            ("linux", "x86") => "natives-linux-x86",
            _ => continue,
        };

        let artifact = match classifiers.get(classifier_key) {
            Some(a) => a,
            None => continue,
        };

        let Some(path) = &artifact.path else {
            continue;
        };

        let jar_path = base_dir.join("libraries").join(path);

        // Skip if file doesn't exist (might not have been downloaded)
        if !jar_path.exists() {
            tracing::warn!("Native library not found: {:?}", jar_path);
            continue;
        }

        // Extract the JAR
        tracing::info!("Extracting natives from: {:?}", jar_path);
        if let Err(e) = extract_jar(&jar_path, &natives_dir, lib).await {
            tracing::error!("Failed to extract natives from {:?}: {}", jar_path, e);
            // Continue with other libraries - don't fail the whole process
        }
    }

    tracing::info!("Natives extracted to: {:?}", natives_dir);
    Ok(natives_dir)
}

/// Check if a library should be used on the current platform.
fn should_use_library(lib: &Library, os: &str, arch: &str) -> bool {
    let Some(rules) = &lib.rules else {
        return true; // No rules = apply to all
    };

    if rules.is_empty() {
        return true;
    }

    let mut allowed = true;
    let mut has_explicit_disallow = false;

    for rule in rules {
        let action = rule.action.as_deref().unwrap_or("allow");
        let applies = rule_applies_to_platform(rule, os, arch);

        if applies {
            if action == "disallow" {
                allowed = false;
                has_explicit_disallow = true;
            } else {
                allowed = true;
            }
        }
    }

    // If there was an explicit disallow rule, respect it
    if has_explicit_disallow {
        return allowed;
    }

    // Otherwise allow if no rules blocked it
    allowed
}

/// Check if a rule applies to the current platform.
fn rule_applies_to_platform(rule: &Rule, os: &str, arch: &str) -> bool {
    // If no OS or features specified, rule applies unconditionally
    if rule.os.is_none() && rule.features.is_none() {
        return true;
    }

    // Check OS condition
    if let Some(rule_os) = &rule.os {
        let os_match = rule_os.name.as_ref().map_or(true, |name| {
            match name.as_str() {
                "windows" => os == "windows",
                "osx" => os == "macos",
                "linux" => os == "linux",
                "freebsd" => os == "freebsd",
                _ => false,
            }
        });

        let arch_match = rule_os.arch.as_ref().map_or(true, |arch_rule| {
            // arch can be "x86", "x64", "arm64", etc.
            if arch_rule.contains("64") || arch_rule == "x64" {
                arch == "x86_64" || arch == "aarch64"
            } else if arch_rule == "x86" {
                arch == "x86"
            } else if arch_rule == "arm64" {
                arch == "aarch64"
            } else {
                true
            }
        });

        // If OS is specified but doesn't match, rule doesn't apply
        if rule_os.name.is_some() && !os_match {
            return false;
        }
        if rule_os.arch.is_some() && !arch_match {
            return false;
        }
    }

    true
}

/// Extract a JAR file, excluding META-INF and specified patterns.
async fn extract_jar(
    jar_path: &Path,
    dest_dir: &Path,
    lib: &Library,
) -> Result<(), String> {
    let jar_path_owned = jar_path.to_path_buf();
    let dest_dir_owned = dest_dir.to_path_buf();
    let exclude_clone = lib.extract.as_ref()
        .and_then(|e| e.exclude.clone())
        .unwrap_or_default();

    // Use blocking file operations since zip crate needs std::io::Read
    tokio::task::spawn_blocking(move || {
        use std::fs::File;

        let file = File::open(&jar_path_owned)
            .map_err(|e| format!("Failed to open JAR: {e}"))?;

        let mut archive = zip::ZipArchive::new(file)
            .map_err(|e| format!("Failed to read ZIP: {e}"))?;

        // Get exclude patterns
        let mut exclude_patterns = vec!["META-INF/".to_string()];
        for pat in &exclude_clone {
            if !pat.ends_with('/') && !pat.ends_with('*') {
                exclude_patterns.push(format!("{}/", pat));
                exclude_patterns.push(format!("{}/*", pat));
            } else {
                exclude_patterns.push(pat.clone());
            }
        }

        for i in 0..archive.len() {
            let mut file = archive
                .by_index(i)
                .map_err(|e| format!("Failed to read ZIP entry: {e}"))?;

            let outpath = dest_dir_owned.join(file.name());

            let should_exclude = exclude_patterns.iter().any(|pat| {
                if pat.ends_with("/*") {
                    let dir = pat.trim_end_matches("/*");
                    file.name().starts_with(dir)
                } else {
                    file.name() == pat || file.name().starts_with(pat)
                }
            });

            if should_exclude {
                continue;
            }

            if let Some(parent) = outpath.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("Failed to create directory: {e}"))?;
            }

            if file.is_dir() {
                std::fs::create_dir_all(&outpath)
                    .map_err(|e| format!("Failed to create directory: {e}"))?;
            } else {
                let mut outfile = std::fs::File::create(&outpath)
                    .map_err(|e| format!("Failed to create file: {e}"))?;
                std::io::copy(&mut file, &mut outfile)
                    .map_err(|e| format!("Failed to write file: {e}"))?;
            }
        }

        Ok(())
    }).await.map_err(|e| format!("Task join error: {e}"))?
}

// ============ Classpath Builder ============

/// Build the classpath from libraries and client.jar.
/// For Fabric/Forge with inheritsFrom, uses the parent version's jar.
pub fn build_classpath(
    version_id: &str,
    version_meta: &VersionMeta,
    libraries: &[Library],
) -> Result<Vec<PathBuf>, String> {
    use crate::core::mojang::maven_name_to_path;
    
    let base_dir = get_minecraft_base();
    let mut classpath = Vec::new();

    // Track if we found the patched client JAR
    let mut found_patched = false;

    // Add all applicable libraries
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;

    for lib in libraries {
        // Skip libraries not applicable to current platform
        if !should_use_library(lib, os, arch) {
            continue;
        }

        // Try standard Mojang format first
        if let Some(downloads) = &lib.downloads {
            if let Some(artifact) = &downloads.artifact {
                if let Some(path) = &artifact.path {
                    let lib_path = base_dir.join("libraries").join(path);
                    if lib_path.exists() {
                        classpath.push(lib_path.clone());
                        
                        // Check if this is the patched client JAR
                        let path_str = path.to_string();
                        if path_str.contains("client") && path_str.contains("patched") {
                            tracing::debug!("Found patched client JAR: {:?}", lib_path);
                            found_patched = true;
                        }
                    } else {
                        tracing::warn!("Library not found: {:?}", lib_path);
                    }
                    continue; // Done with this library
                }
            }
        }
        
        // Fallback: use Maven coordinates (Fabric/Forge style)
        if let Some(name) = &lib.name {
            // Check for patched client JAR via Maven coordinates
            // Format: "net.minecraft:client:1.20.1:patched"
            if name.contains("net.minecraft") && name.contains("client") && name.contains("patched") {
                if let Some(maven_path) = maven_name_to_path(name) {
                    let lib_path = base_dir.join("libraries").join(&maven_path);
                    if lib_path.exists() {
                        classpath.push(lib_path.clone());
                        tracing::debug!("Found patched client JAR via Maven: {:?}", lib_path);
                        found_patched = true;
                        continue;
                    } else {
                        tracing::warn!("Patched client JAR not found via Maven: {:?}", lib_path);
                    }
                }
            }
            
            // Regular library fallback
            if let Some(maven_path) = maven_name_to_path(name) {
                let lib_path = base_dir.join("libraries").join(&maven_path);
                if lib_path.exists() {
                    classpath.push(lib_path);
                    tracing::debug!("Added library via Maven coords: {}", name);
                } else {
                    tracing::warn!("Library not found (Maven): {:?}", lib_path);
                }
            }
        }
    }

    // Add client.jar
    // For Fabric/Forge with inheritsFrom, use the parent version's jar
    let jar_version = version_meta.inherits_from.as_deref().unwrap_or(version_id);
    let client_jar = base_dir
        .join("versions")
        .join(jar_version)
        .join(format!("{}.jar", jar_version));
    
    if client_jar.exists() {
        classpath.push(client_jar);
    } else {
        // Try to find patched client JAR in libraries directory as fallback
        let mc_version = jar_version;
        let possible_patched_paths = [
            base_dir.join("libraries").join("net/minecraft/client").join(mc_version).join(format!("client-{}-patched.jar", mc_version)),
            base_dir.join("libraries").join("net/minecraft/client").join(mc_version).join(format!("client-{}-20230612.114412-patched.jar", mc_version)),
        ];
        
        let mut found_alternative = false;
        for patched_path in &possible_patched_paths {
            if patched_path.exists() {
                classpath.push(patched_path.clone());
                tracing::info!("Using patched client JAR from libraries: {:?}", patched_path);
                found_alternative = true;
                break;
            }
        }
        
        if !found_alternative {
            return Err(format!("client.jar not found at {:?} (version: {})", client_jar, jar_version));
        }
    }

    if !found_patched {
        // Log warning if patched JAR wasn't found - this could cause Forge to fail
        tracing::warn!("Patched client JAR not found in classpath! Forge may fail to launch.");
    }

    tracing::info!("Built classpath with {} entries", classpath.len());
    
    // Log classpath entries for debugging
    for (i, entry) in classpath.iter().enumerate() {
        let name = entry.file_name().and_then(|n| n.to_str()).unwrap_or("unknown");
        if name.contains("patched") || name.contains("client") {
            tracing::debug!("Classpath[{}]: {:?}", i, entry);
        }
    }
    
    Ok(classpath)
}

/// Pre-flight check: verify merged libraries and client.jar exist.
/// Returns a vector of (local_path, download_url) for missing files.
pub async fn verify_and_collect_missing_files(
    libraries: &[Library],
    client_jar_version: &str,
) -> Result<Vec<DownloadTask>, String> {
    use crate::core::mojang::get_library_download_info_from_json;
    
    let base_dir = get_minecraft_base();
    let mut missing_tasks: Vec<DownloadTask> = Vec::new();

    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;

    for lib in libraries {
        // Skip libraries not applicable to current platform
        if !should_use_library(lib, os, arch) {
            continue;
        }

        // Try standard Mojang format first
        if let Some(downloads) = &lib.downloads {
            if let Some(artifact) = &downloads.artifact {
                if let Some(path) = &artifact.path {
                    let lib_path = base_dir.join("libraries").join(path);
                    if !lib_path.exists() {
                        let url = artifact.url.clone().unwrap_or_default();
                        if !url.is_empty() {
                            tracing::info!("Missing library (Mojang): {:?} - will download from {}", lib_path, url);
                            missing_tasks.push(DownloadTask::new(
                                url,
                                lib_path.to_string_lossy().to_string(),
                                artifact.sha1.clone(),
                            ));
                        }
                    }
                    continue;
                }
            }
        }
        
        // Fallback: use Maven coordinates (Fabric/Forge style)
        if let Some(name) = &lib.name {
            if let Some(maven_path) = maven_name_to_path(name) {
                let lib_path = base_dir.join("libraries").join(&maven_path);
                if !lib_path.exists() {
                    // Get download URL from lib.url or default
                    let base_url = lib.url.as_deref().unwrap_or("https://libraries.minecraft.net/");
                    let download_url = format!("{}{}", base_url, maven_path);
                    tracing::info!("Missing library (Maven): {:?} - will download from {}", lib_path, download_url);
                    missing_tasks.push(DownloadTask::new(
                        download_url,
                        lib_path.to_string_lossy().to_string(),
                        None, // SHA1 not available for Maven coords
                    ));
                }
            }
        }
    }

    // Check client.jar
    let client_jar = base_dir
        .join("versions")
        .join(client_jar_version)
        .join(format!("{}.jar", client_jar_version));
    
    if !client_jar.exists() {
        // Need to download client.jar - fetch from version JSON or use known URL pattern
        let version_json_path = base_dir
            .join("versions")
            .join(client_jar_version)
            .join(format!("{}.json", client_jar_version));
        
        if version_json_path.exists() {
            let content = tokio::fs::read_to_string(&version_json_path).await
                .map_err(|e| format!("Failed to read version JSON: {}", e))?;
            let version_meta: serde_json::Value = serde_json::from_str(&content)
                .map_err(|e| format!("Failed to parse version JSON: {}", e))?;
            
            if let Some(downloads) = version_meta.get("downloads") {
                if let Some(client) = downloads.get("client") {
                    if let Some(url) = client.get("url").and_then(|u| u.as_str()) {
                        tracing::info!("Missing client.jar: {:?} - will download from {}", client_jar, url);
                        missing_tasks.push(DownloadTask::new(
                            url.to_string(),
                            client_jar.to_string_lossy().to_string(),
                            client.get("sha1").and_then(|s| s.as_str()).map(String::from),
                        ));
                    }
                }
            }
        }
    }

    tracing::info!("Pre-flight check: {} missing files to download", missing_tasks.len());
    Ok(missing_tasks)
}

// ============ Argument Parsing ============

/// Parse JVM arguments from version metadata, applying rules and substitutions.
pub fn parse_jvm_arguments(
    version_meta: &VersionMeta,
    config: &LaunchConfig,
) -> Result<Vec<String>, String> {
    let mut args = Vec::new();

    // Handle old format (minecraftArguments - deprecated)
    if let Some(mc_args) = &version_meta.minecraft_arguments {
        // Old format: just a string with all args
        args.extend(mc_args.split_whitespace().map(String::from));
    }

    // Handle new format (arguments.jvm)
    if let Some(arguments) = &version_meta.arguments {
        if let Some(jvm) = &arguments.jvm {
            parse_argument_list(jvm, &mut args)?;
        }
    }

    // Apply template substitutions
    let game_dir = config.game_directory.to_string_lossy().to_string();
    let natives_dir = config.natives_directory.to_string_lossy().to_string();
    let assets_dir = config.assets_directory.to_string_lossy().to_string();

    // Build classpath string
    let classpath_str = config
        .classpath
        .iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join(CLASSPATH_SEPARATOR);

    let libraries_dir = get_minecraft_base().join("libraries").to_string_lossy().to_string();

    for arg in &mut args {
        *arg = arg
            .replace("${auth_player_name}", &config.account.username)
            .replace("${version_name}", &config.version_id)
            .replace("${game_directory}", &game_dir)
            .replace("${assets_root}", &assets_dir)
            .replace("${assets_index_name}", version_meta.assets.as_deref().unwrap_or("1.0"))
            .replace("${auth_uuid}", &config.account.id)
            .replace("${auth_access_token}", config.account.access_token.as_deref().unwrap_or("0"))
            .replace("${user_type}", if config.account.account_type == crate::auth::AccountType::Microsoft { "msa" } else { "mojang" })
            .replace("${version_type}", version_meta.version_type.as_deref().unwrap_or("release"))
            .replace("${natives_directory}", &natives_dir)
            .replace("${launcher_name}", LAUNCHER_NAME)
            .replace("${launcher_version}", LAUNCHER_VERSION)
            .replace("${classpath}", &classpath_str)
            .replace("${library_directory}", &libraries_dir)
            .replace("${classpath_separator}", CLASSPATH_SEPARATOR)
            .replace("${ignore_list}", "")
            .replace("${ignoreList}", "");
    }

    // Add default JVM args if not present
    if !args.iter().any(|a| a.contains("-Xmx")) {
        args.push("-Xmx2G".to_string()); // Default 2GB heap
    }
    if !args.iter().any(|a| a.contains("-Xms")) {
        args.push("-Xms512M".to_string()); // Default 512MB initial
    }

    // Add Djava.net.preferIPv4Stack=true for better compatibility
    if !args.iter().any(|a| a.contains("preferIPv4Stack")) {
        args.push("-Djava.net.preferIPv4Stack=true".to_string());
    }

    tracing::debug!("Parsed JVM args: {:?}", args);
    Ok(args)
}

/// Parse game arguments from version metadata.
pub fn parse_game_arguments(
    version_meta: &VersionMeta,
    config: &LaunchConfig,
) -> Result<Vec<String>, String> {
    let mut args = Vec::new();

    // Handle old format (minecraftArguments)
    if let Some(mc_args) = &version_meta.minecraft_arguments {
        args.extend(mc_args.split_whitespace().map(String::from));
    }

    // Handle new format (arguments.game)
    if let Some(arguments) = &version_meta.arguments {
        if let Some(game) = &arguments.game {
            parse_argument_list(game, &mut args)?;
        }
    }

    // Apply template substitutions
    let game_dir = config.game_directory.to_string_lossy().to_string();
    let assets_dir = config.assets_directory.to_string_lossy().to_string();
    let libraries_dir = get_minecraft_base().join("libraries").to_string_lossy().to_string();

    for arg in &mut args {
        *arg = arg
            .replace("${auth_player_name}", &config.account.username)
            .replace("${version_name}", &config.version_id)
            .replace("${game_directory}", &game_dir)
            .replace("${assets_root}", &assets_dir)
            .replace("${assets_index_name}", version_meta.assets.as_deref().unwrap_or("1.0"))
            .replace("${auth_uuid}", &config.account.id)
            .replace("${auth_access_token}", config.account.access_token.as_deref().unwrap_or("0"))
            .replace("${user_type}", if config.account.account_type == crate::auth::AccountType::Microsoft { "msa" } else { "mojang" })
            .replace("${version_type}", version_meta.version_type.as_deref().unwrap_or("release"))
            .replace("${launcher_name}", LAUNCHER_NAME)
            .replace("${launcher_version}", LAUNCHER_VERSION)
            .replace("${library_directory}", &libraries_dir)
            .replace("${classpath_separator}", CLASSPATH_SEPARATOR)
            .replace("${ignore_list}", "")
            .replace("${ignoreList}", "");
    }

    tracing::debug!("Parsed game args: {:?}", args);
    Ok(args)
}

/// Parse the argument list which can contain strings or objects with rules.
fn parse_argument_list(value: &serde_json::Value, args: &mut Vec<String>) -> Result<(), String> {
    match value {
        serde_json::Value::Array(arr) => {
            for item in arr {
                parse_argument_value(item, args)?;
            }
        }
        serde_json::Value::Object(obj) => {
            // This is a rule object: { "rules": [...], "value": [...] }
            if obj.contains_key("rules") {
                if !apply_rules(obj)? {
                    return Ok(()); // Rules don't apply, skip this
                }
                // Rules apply, get the value
                if let Some(value) = obj.get("value") {
                    parse_argument_value(value, args)?;
                }
            } else {
                // Object as string value
                if let Some(s) = obj.get("value") {
                    if let Some(s) = s.as_str() {
                        args.push(s.to_string());
                    }
                }
            }
        }
        serde_json::Value::String(s) => {
            args.push(s.clone());
        }
        _ => {}
    }
    Ok(())
}

/// Parse a single argument value (can be string or array).
fn parse_argument_value(value: &serde_json::Value, args: &mut Vec<String>) -> Result<(), String> {
    match value {
        serde_json::Value::String(s) => {
            args.push(s.clone());
        }
        serde_json::Value::Array(arr) => {
            for item in arr {
                if let serde_json::Value::String(s) = item {
                    args.push(s.clone());
                }
            }
        }
        serde_json::Value::Object(obj) => {
            // Object with rules
            if obj.contains_key("rules") {
                if !apply_rules(obj)? {
                    return Ok(());
                }
                if let Some(value) = obj.get("value") {
                    parse_argument_value(value, args)?;
                }
            }
        }
        _ => {}
    }
    Ok(())
}

/// Apply rules from a rule object.
/// Returns true if the argument should be included (allowed).
/// 
/// Algorithm:
/// 1. If no rules array, default is ALLOW (for backward compatibility)
/// 2. If rules exist, default is DISALLOW
/// 3. For each rule, check if it applies to current OS:
///    - If OS matches, use its action to set the final result
///    - If OS doesn't match, skip that rule
fn apply_rules(rule_obj: &serde_json::Map<String, serde_json::Value>) -> Result<bool, String> {
    let rules = match rule_obj.get("rules").and_then(|v| v.as_array()) {
        Some(r) if !r.is_empty() => r,
        _ => return Ok(true), // No rules = allow everything
    };

    // When rules exist, default is to DISALLOW
    let mut allowed = false;
    let mut has_applied_rule = false;

    for rule in rules {
        let action = rule
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("allow");
        
        let os_rule = rule.get("os").and_then(|v| v.as_object());
        let features = rule.get("features").and_then(|v| v.as_object());

        // Check if this rule applies to current OS
        let os_name = os_rule.and_then(|o| o.get("name").and_then(|v| v.as_str()));
        let os_arch = os_rule.and_then(|o| o.get("arch").and_then(|v| v.as_str()));
        let os_matches = matches_os_condition(os_name, os_arch);

        // Check feature conditions
        // By default, all special features are disabled (normal launch, not quick play, not demo)
        let features_ok = if let Some(feats) = features {
            // Our launcher default states
            let is_demo = false;
            let is_applet = false;
            let is_quick_play_singleplayer = false;
            let is_quick_play_multiplayer = false;
            let is_quick_play_realms = false;
            let has_custom_resolution = false;
            
            // Check each feature requirement
            for (feature_name, required_state) in feats {
                let actual_state = match feature_name.as_str() {
                    "demo" => is_demo,
                    "applet" => is_applet,
                    "is_quick_play_singleplayer" => is_quick_play_singleplayer,
                    "is_quick_play_multiplayer" => is_quick_play_multiplayer,
                    "is_quick_play_realms" => is_quick_play_realms,
                    "has_custom_resolution" => has_custom_resolution,
                    _ => false, // Unknown features default to false (disabled)
                };
                
                // If the required state doesn't match our actual state, this rule doesn't apply
                if actual_state != *required_state {
                    break; // Will set features_ok = false
                }
            }
            
            // Verify all feature requirements are satisfied
            let mut all_satisfied = true;
            for (feature_name, required_state) in feats {
                let actual_state = match feature_name.as_str() {
                    "demo" => is_demo,
                    "applet" => is_applet,
                    "is_quick_play_singleplayer" => is_quick_play_singleplayer,
                    "is_quick_play_multiplayer" => is_quick_play_multiplayer,
                    "is_quick_play_realms" => is_quick_play_realms,
                    "has_custom_resolution" => has_custom_resolution,
                    _ => false,
                };
                if actual_state != *required_state {
                    all_satisfied = false;
                    break;
                }
            }
            all_satisfied
        } else {
            true // No feature requirements = always applies
        };

        // Only update allowed if the rule actually applies to current platform
        if os_matches && features_ok {
            has_applied_rule = true;
            allowed = action == "allow";
        }
    }

    // If no rule matched current OS, default to allowing (for backward compatibility)
    if !has_applied_rule {
        allowed = false;
    }

    Ok(allowed)
}

// ============ Process Launching ============

/// Launch a Minecraft instance.
#[tauri::command]
pub async fn launch_instance(
    app: AppHandle,
    version_id: String,
    account_uuid: String,
) -> Result<(), String> {
    tracing::info!("Launching instance {} with account {}", version_id, account_uuid);

    let base_dir = get_minecraft_base();
    
    // Version isolation: Use instance-specific directory for game data (saves, resourcepacks, options.txt)
    // This ensures each instance has its own isolated game directory
    let instance_dir = base_dir.join("versions").join(&version_id);
    let game_dir = instance_dir.clone();
    let assets_dir = base_dir.join("assets");

    // Load account
    let accounts = crate::auth::load_accounts().await?;
    let account = accounts
        .iter()
        .find(|a| a.id == account_uuid)
        .ok_or_else(|| "Account not found".to_string())?
        .clone();

    // Load version metadata
    let version_json_path = base_dir
        .join("versions")
        .join(&version_id)
        .join(format!("{}.json", version_id));
    
    let version_json_content = fs::read_to_string(&version_json_path)
        .await
        .map_err(|e| format!("Failed to read version JSON: {e}"))?;
    
    let mut version_meta: VersionMeta = serde_json::from_str(&version_json_content)
        .map_err(|e| format!("Failed to parse version JSON: {e}"))?;

    // Handle inheritsFrom (used by Fabric, Forge, etc.)
    // If the version has inheritsFrom, we need to load the parent version's data
    if let Some(ref parent_version) = version_meta.inherits_from {
        tracing::info!("Version {} inherits from {}, loading parent...", version_id, parent_version);
        
        let parent_json_path = base_dir
            .join("versions")
            .join(parent_version)
            .join(format!("{}.json", parent_version));
        
        let parent_content = fs::read_to_string(&parent_json_path)
            .await
            .map_err(|e| format!("Failed to read parent version JSON: {}", e))?;
        
        let mut parent_meta: VersionMeta = serde_json::from_str(&parent_content)
            .map_err(|e| format!("Failed to parse parent version JSON: {e}"))?;
        
        // Merge parent data into version_meta
        // Use parent's data if current version doesn't have it
        if version_meta.main_class.is_none() {
            version_meta.main_class = parent_meta.main_class;
        }
        if version_meta.minecraft_arguments.is_none() {
            version_meta.minecraft_arguments = parent_meta.minecraft_arguments;
        }
        
// Deep merge arguments: parent first, then child appends
        if let Some(parent_args) = parent_meta.arguments.take() {
            if let Some(child_args) = version_meta.arguments.take() {
                // Both have arguments - deep merge
                let mut merged = parent_args;
                
                // Merge game args: parent first, then child
                // Use merged.game since merged now holds parent_args
                if let Some(parent_game) = merged.game.take() {
                    let mut game_list = parent_game.clone();
                    if let Some(child_game) = child_args.game {
                        // Extend game args - child appends after parent
                        match (game_list.clone(), child_game) {
                            (serde_json::Value::Array(mut parent_arr), serde_json::Value::Array(child_arr)) => {
                                parent_arr.extend(child_arr);
                                game_list = serde_json::Value::Array(parent_arr);
                            }
                            (serde_json::Value::Array(mut parent_arr), other) => {
                                parent_arr.push(other);
                                game_list = serde_json::Value::Array(parent_arr);
                            }
                            (other, serde_json::Value::Array(child_arr)) => {
                                let mut combined = vec![other];
                                combined.extend(child_arr);
                                game_list = serde_json::Value::Array(combined);
                            }
                            _ => {}
                        }
                    }
                    merged.game = Some(game_list);
                } else if let Some(child_game) = child_args.game {
                    merged.game = Some(child_game);
                }
                
                // Merge JVM args: parent first, then child
                // Use merged.jvm since merged now holds parent_args
                if let Some(parent_jvm) = merged.jvm.take() {
                    let mut jvm_list = parent_jvm.clone();
                    if let Some(child_jvm) = child_args.jvm {
                        match (jvm_list.clone(), child_jvm) {
                            (serde_json::Value::Array(mut parent_arr), serde_json::Value::Array(child_arr)) => {
                                parent_arr.extend(child_arr);
                                jvm_list = serde_json::Value::Array(parent_arr);
                            }
                            (serde_json::Value::Array(mut parent_arr), other) => {
                                parent_arr.push(other);
                                jvm_list = serde_json::Value::Array(parent_arr);
                            }
                            (other, serde_json::Value::Array(child_arr)) => {
                                let mut combined = vec![other];
                                combined.extend(child_arr);
                                jvm_list = serde_json::Value::Array(combined);
                            }
                            _ => {}
                        }
                    }
                    merged.jvm = Some(jvm_list);
                } else if let Some(child_jvm) = child_args.jvm {
                    merged.jvm = Some(child_jvm);
                }
                
                version_meta.arguments = Some(merged);
            } else {
                // Child has no arguments, use parent's
                version_meta.arguments = Some(parent_args);
            }
        } else if version_meta.arguments.is_none() {
            // Neither has arguments
            version_meta.arguments = parent_meta.arguments;
        }
        
        if version_meta.assets.is_none() {
            version_meta.assets = parent_meta.assets;
        }
        if version_meta.asset_index.is_none() {
            version_meta.asset_index = parent_meta.asset_index;
        }
        if version_meta.downloads.is_none() {
            version_meta.downloads = parent_meta.downloads;
        }
        
        // Merge libraries: parent libraries + current libraries
        let mut merged_libs = parent_meta.libraries.unwrap_or_default();
        if let Some(current_libs) = version_meta.libraries.take() {
            merged_libs.extend(current_libs);
        }
        version_meta.libraries = Some(merged_libs);
        
        tracing::info!("Successfully merged parent version {} into {}", parent_version, version_id);
    }

    // Get main class
    let main_class = version_meta
        .main_class
        .clone()
        .unwrap_or_else(|| "net.minecraft.client.main.Main".to_string());

    // Get libraries
    let libraries = version_meta.libraries.as_deref().unwrap_or(&[]);

    // ========== Pre-flight Check: Verify and Auto-repair Missing Files ==========
    let jar_version = version_meta.inherits_from.as_deref().unwrap_or(&version_id);
    
    let missing_files = verify_and_collect_missing_files(libraries, jar_version).await?;
    
    if !missing_files.is_empty() {
        tracing::info!("Detected {} missing libraries. Starting auto-repair...", missing_files.len());
        
        // Notify frontend: entering repairing state
        let _ = app.emit("instance-state-changed", serde_json::json!({
            "versionId": version_id,
            "status": "repairing",
            "missingCount": missing_files.len()
        }));

        // Download missing files
        let app_for_download = app.clone();
        run_batch_download(missing_files, app_for_download).await;

        // Notify frontend: repairing complete
        let _ = app.emit("instance-state-changed", serde_json::json!({
            "versionId": version_id,
            "status": "repairing_complete"
        }));
        
        tracing::info!("Auto-repair complete. Proceeding with launch...");
    }

    // Extract natives
    let natives_dir = extract_natives(&version_id, libraries).await?;
    tracing::info!("Natives extracted to: {:?}", natives_dir);

    // Build classpath
    tracing::info!("Building classpath for version: {}", version_id);
    let classpath = build_classpath(&version_id, &version_meta, libraries)?;
    tracing::info!("Classpath built with {} entries", classpath.len());

    // Create launch config
    let config = LaunchConfig {
        version_id: version_id.clone(),
        account,
        game_directory: game_dir.clone(),
        assets_directory: assets_dir.clone(),
        natives_directory: natives_dir.clone(),
        classpath: classpath.clone(),
        main_class: main_class.clone(),
        jvm_args: Vec::new(),
        game_args: Vec::new(),
    };

    // Load instance configuration (for custom Java path)
    let instance_config = get_instance_config(version_id.clone()).await?;

    // Determine Java executable path: instance config > system default
    let java_executable = match &instance_config.java_path {
        Some(path) if !path.is_empty() => path.clone(),
        _ => find_java().ok_or("Java not found. Please ensure Java is installed and in PATH.")?,
    };

    tracing::info!("Using Java executable: {}", java_executable);

    // ========== Pre-flight Check: Validate Java & Detect Version ==========
    let java_check = tokio::process::Command::new(&java_executable)
        .arg("-version")
        .output()
        .await;

    let mut java_major_version: u32 = 8; // Default to Java 8 for safety

    match java_check {
        Ok(output) if output.status.success() => {
            // Parse Java version from stderr (Java prints version to stderr)
            let stderr_output = String::from_utf8_lossy(&output.stderr);
            tracing::info!("Java version check output: {}", stderr_output.lines().next().unwrap_or("unknown"));
            
            // Try to extract major version from output like "openjdk version \"21.0.8\""
            if let Some(line) = stderr_output.lines().next() {
                if let Some(version_str) = line.split('"').nth(1) {
                    if let Some(major_str) = version_str.split('.').next() {
                        if let Ok(major) = major_str.parse::<u32>() {
                            java_major_version = major;
                            tracing::info!("Detected Java Major Version: {}", java_major_version);
                        }
                    }
                }
            }
        }
        Ok(output) => {
            let stderr_output = String::from_utf8_lossy(&output.stderr);
            return Err(format!(
                "Java executable '{}' failed to run.\nError: {}\nMC 1.20.5+ requires Java 21.",
                java_executable, stderr_output
            ));
        }
        Err(e) => {
            return Err(format!(
                "Failed to execute Java at '{}': {}\nPlease ensure Java is installed and the path is valid. Minecraft 1.20.5+ requires Java 21.",
                java_executable, e
            ));
        }
    }

    // Parse JVM args
    let mut jvm_args = parse_jvm_arguments(&version_meta, &config)?;

    // ========== Dynamic JVM Args Filtering ==========
    // For Java < 23, filter out unsupported parameters (Java 23+ only)
    if java_major_version < 23 {
        let original_count = jvm_args.len();
        jvm_args.retain(|arg| {
            // Filter out --sun-misc-unsafe-memory-access and related parameters
            if arg.starts_with("--sun-misc-unsafe-memory-access") {
                tracing::warn!("Filtering unsupported JVM arg for Java {}: {}", java_major_version, arg);
                return false;
            }
            true
        });
        
        if jvm_args.len() < original_count {
            tracing::info!("Filtered {} JVM args not supported by Java {}", original_count - jvm_args.len(), java_major_version);
        }
    }

    // Apply custom max memory if configured, otherwise use recommended default
    let max_mem = instance_config.max_memory
        .map(|m| m.max(512)) // At least 512MB
        .unwrap_or_else(get_recommended_max_memory);
    let min_mem = max_mem / 2;

    // Remove any existing -Xmx/-Xms flags and insert our custom ones at the front
    jvm_args.retain(|arg| !arg.starts_with("-Xmx") && !arg.starts_with("-Xms"));
    jvm_args.insert(0, format!("-Xmx{}M", max_mem));
    jvm_args.insert(0, format!("-Xms{}M", min_mem));
    
    tracing::info!("Memory allocation: -Xms{}M -Xmx{}M (system recommended: {}MB)", 
        min_mem, max_mem, get_recommended_max_memory());

    // Apply extra JVM arguments if configured
    if let Some(extra_args) = &instance_config.jvm_args_extra {
        for arg in extra_args {
            if !jvm_args.contains(arg) {
                jvm_args.push(arg.clone());
            }
        }
    }

    // Parse game args
    let game_args = parse_game_arguments(&version_meta, &config)?;

    tracing::info!("Starting Minecraft with main class: {}", main_class);
    tracing::info!("Full JVM args: {:?}", jvm_args);
    tracing::debug!("Game args: {:?}", game_args);

    // Get the main window for behavior control
    let window = app.get_webview_window("main")
        .ok_or("Failed to get main window")?;

    // Determine window behavior after game starts
    let window_behavior = instance_config.window_behavior.clone();
    let should_hide = window_behavior == "hide";
    let should_minimize = window_behavior == "minimize";

    // Apply window behavior before spawning
    if should_hide {
        let _ = window.hide();
    } else if should_minimize {
        let _ = window.minimize();
    }

    // Spawn the process using the resolved Java executable
    tracing::info!("Spawning process: {} with main class: {}", java_executable, main_class);
    tracing::info!("Game directory: {:?}", game_dir);
    tracing::info!("JVM args count: {}", jvm_args.len());
    tracing::info!("Game args count: {}", game_args.len());
    tracing::info!("Classpath entries: {}", classpath.len());
    
    let mut child = Command::new(&java_executable)
        .current_dir(&game_dir)
        .args(&jvm_args)
        .arg(&main_class)
        .args(&game_args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to start JVM: {e}"))?;

    let pid = child.id().unwrap_or(0);
    tracing::info!("Minecraft process spawned with PID: {}", pid);

    // Emit "running" state to frontend
    let _ = app.emit("instance-state-changed", serde_json::json!({
        "versionId": version_id,
        "status": "running"
    }));

    // Get stdout and stderr
    let stdout = child.stdout.take().ok_or("Failed to capture stdout")?;
    let stderr = child.stderr.take().ok_or("Failed to capture stderr")?;

    // Spawn tasks to read output
    let app_clone = app.clone();
    tokio::spawn(async move {
        let mut reader = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            let _ = app_clone.emit("game-log", serde_json::json!({
                "type": "stdout",
                "line": line
            }));
        }
    });

    let app_clone2 = app.clone();
    tokio::spawn(async move {
        let mut reader = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            let _ = app_clone2.emit("game-log", serde_json::json!({
                "type": "stderr",
                "line": line
            }));
        }
    });

    // Lifecycle Guardian: Async wait for process exit
    let app_handle_clone = app.clone();
    let version_id_clone = version_id.clone();
    let window_clone = window.clone();
    
    tokio::spawn(async move {
        // Wait for process to exit
        match child.wait().await {
            Ok(status) => {
                let exit_code = status.code().unwrap_or(-1);
                tracing::info!("Game exited with code: {}", exit_code);

                // Restore window visibility when game exits
                let _ = window_clone.show();

                // Notify frontend game has exited with exit code
                let _ = app_handle_clone.emit("instance-state-changed", serde_json::json!({
                    "versionId": version_id_clone,
                    "status": "exited",
                    "exitCode": exit_code
                }));
            }
            Err(e) => {
                tracing::error!("Failed to wait for game process: {}", e);
                // Restore window on error too
                let _ = window_clone.show();
                
                let _ = app_handle_clone.emit("instance-state-changed", serde_json::json!({
                    "versionId": version_id_clone,
                    "status": "exited",
                    "exitCode": -1
                }));
            }
        }
    });

    Ok(())
}

/// Find Java executable in PATH or common locations.
fn find_java() -> Option<String> {
    // First try java from PATH
    if which::which("java").is_ok() {
        return Some("java".to_string());
    }

    // On Windows, check common Java installation paths
    #[cfg(target_os = "windows")]
    {
        let program_files = std::env::var("ProgramFiles").ok();
        if let Some(pf) = program_files {
            // Check Java 17+ locations
            for ver in &["21", "17", "11"] {
                let java_exe = format!("{}\\Java\\jdk-{}\\bin\\java.exe", pf, ver);
                if std::path::Path::new(&java_exe).exists() {
                    return Some(java_exe);
                }
            }
        }
    }

    None
}