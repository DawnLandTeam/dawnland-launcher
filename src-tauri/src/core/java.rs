//! Java Management Module
//! Provides functionality to scan, manage, and download Java runtimes.

use crate::core::mojang::get_minecraft_base;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tauri::{AppHandle, Emitter};
use tokio::sync::Mutex;

static CACHED_JAVAS: Mutex<Option<Vec<JavaInfo>>> = Mutex::const_new(None);

/// Configuration for user-defined Java paths and download settings.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct JavaSettings {
    pub manual_paths: Vec<String>,
    pub custom_download_path: Option<String>,
}

fn get_java_config_path() -> PathBuf {
    get_minecraft_base()
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("java_config.json")
}

pub async fn load_java_config() -> JavaSettings {
    let config_path = get_java_config_path();
    if config_path.exists() {
        if let Ok(content) = tokio::fs::read_to_string(&config_path).await {
            if let Ok(config) = serde_json::from_str(&content) {
                return config;
            }
        }
    }
    JavaSettings::default()
}

pub async fn save_java_config(config: &JavaSettings) -> Result<(), String> {
    let config_path = get_java_config_path();
    let content = serde_json::to_string_pretty(config)
        .map_err(|e| format!("Failed to serialize Java config: {}", e))?;
        
    if let Some(parent) = config_path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
        
    tokio::fs::write(&config_path, content)
        .await
        .map_err(|e| format!("Failed to write Java config: {}", e))?;
    Ok(())
}

/// Represents a discovered Java installation on the system.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaInfo {
    /// Absolute path to the java executable
    pub path: String,
    /// Major version number (e.g., 8, 11, 17, 21)
    pub major_version: u32,
    /// Human-readable version string (e.g., "21.0.2")
    pub version_string: String,
    /// Vendor name (e.g., "Eclipse Temurin", "Oracle", "Adoptium")
    pub vendor: String,
    /// Whether this is a 64-bit JVM
    pub is_64bit: bool,
}

/// Scan all locally installed Java versions.
#[tauri::command]
pub async fn scan_local_javas() -> Result<Vec<JavaInfo>, String> {
    // Check cache first
    {
        let cache = CACHED_JAVAS.lock().await;
        if let Some(javas) = cache.as_ref() {
            return Ok(javas.clone());
        }
    }

    tracing::info!("Scanning local Java installations...");

    let mut javas = Vec::new();

    let config = load_java_config().await;

    // 1. Check JAVA_HOME
    if let Ok(java_home) = std::env::var("JAVA_HOME") {
        let java_path = PathBuf::from(&java_home).join("bin").join("java.exe");
        if java_path.exists() {
            if let Some(java_info) = probe_java(&java_path).await {
                tracing::info!("Found Java at JAVA_HOME: {}", java_home);
                javas.push(java_info);
            }
        }
    }

    // 2. Check manual paths
    for path in &config.manual_paths {
        let java_path = PathBuf::from(path);
        if java_path.exists() {
            // Check if we already have this Java
            let path_str = java_path.to_string_lossy().to_string();
            if !javas.iter().any(|j: &JavaInfo| j.path == path_str) {
                if let Some(java_info) = probe_java(&java_path).await {
                    tracing::info!("Found manual Java at: {}", path);
                    javas.push(java_info);
                }
            }
        }
    }

    // 3. Scan common installation directories based on OS + launcher runtimes
    let mut search_paths = get_java_search_paths();

    // Add default launcher runtimes dir
    search_paths.push(get_minecraft_base().parent().unwrap_or_else(|| std::path::Path::new(".")).join("runtimes"));

    // Add custom download path if set
    if let Some(custom_path) = &config.custom_download_path {
        search_paths.push(PathBuf::from(custom_path));
    }

    for base_path in search_paths {
        if base_path.exists() {
            if let Ok(entries) = tokio::fs::read_dir(&base_path).await {
                let mut entries = entries;
                while let Ok(Some(entry)) = entries.next_entry().await {
                    let path = entry.path();
                    if path.is_dir() {
                        // Look for java executable in common subdirectories
                        let possible_java_paths = vec![
                            path.join("bin").join("java.exe"),
                            path.join("bin").join("java"),
                            path.join("Contents").join("Home").join("bin").join("java"),
                        ];

                        for java_path in possible_java_paths {
                            if java_path.exists() {
                                // Check if we already have this Java
                                let path_str = java_path.to_string_lossy().to_string();
                                if !javas.iter().any(|j: &JavaInfo| j.path == path_str) {
                                    if let Some(java_info) = probe_java(&java_path).await {
                                        tracing::info!("Found Java at: {}", path.display());
                                        javas.push(java_info);
                                    }
                                }
                                break; // Found java in this directory
                            }
                        }
                    }
                }
            }
        }
    }

    // Sort by major version descending
    javas.sort_by(|a, b| b.major_version.cmp(&a.major_version));

    // Update cache
    *CACHED_JAVAS.lock().await = Some(javas.clone());

    tracing::info!("Found {} Java installations", javas.len());
    Ok(javas)
}

/// Get platform-specific search paths for Java installations.
fn get_java_search_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    #[cfg(target_os = "windows")]
    {
        // Common Java installation directories on Windows
        paths.push(PathBuf::from("C:\\Program Files\\Java"));
        paths.push(PathBuf::from("C:\\Program Files\\Eclipse Adoptium"));
        paths.push(PathBuf::from("C:\\Program Files\\Eclipse Foundation"));
        paths.push(PathBuf::from("C:\\Program Files\\Amazon Corretto"));
        paths.push(PathBuf::from("C:\\Program Files\\Microsoft"));
        paths.push(PathBuf::from("C:\\Program Files (x86)\\Eclipse Adoptium"));
        paths.push(PathBuf::from("C:\\Program Files (x86)\\Amazon Corretto"));
    }

    #[cfg(target_os = "macos")]
    {
        paths.push(PathBuf::from("/Library/Java/JavaVirtualMachines"));
        paths.push(PathBuf::from("/System/Library/Java/JavaVirtualMachines"));
    }

    #[cfg(target_os = "linux")]
    {
        paths.push(PathBuf::from("/usr/lib/jvm"));
        paths.push(PathBuf::from("/opt/java"));
        paths.push(PathBuf::from("/opt/jdk"));
    }

    // Also check user-specific paths
    if let Ok(home) = std::env::var("HOME") {
        #[cfg(target_os = "windows")]
        {
            paths.push(
                PathBuf::from(&home)
                    .join("AppData")
                    .join("Local")
                    .join("Programs")
                    .join("Java"),
            );
            paths.push(
                PathBuf::from(&home)
                    .join("AppData")
                    .join("Local")
                    .join("Programs")
                    .join("Eclipse Adoptium"),
            );
        }
        #[cfg(target_os = "macos")]
        {
            paths.push(
                PathBuf::from(&home)
                    .join("Library")
                    .join("Java")
                    .join("JavaVirtualMachines"),
            );
        }
        #[cfg(target_os = "linux")]
        {
            paths.push(PathBuf::from(&home).join(".jdks"));
            paths.push(PathBuf::from(&home).join("jdk"));
        }
    }

    paths
}

/// Probe a Java executable to get its version information.
async fn probe_java(java_path: &PathBuf) -> Option<JavaInfo> {
    let output = crate::core::utils::create_hidden_command(java_path)
        .arg("-version")
        .output()
        .await
        .ok()?;

    if !output.status.success() {
        return None;
    }

    // Parse stderr (Java prints version to stderr)
    let stderr = String::from_utf8_lossy(&output.stderr);
    let version_string = extract_version_string(&stderr);
    let major_version = extract_major_version(&stderr);
    let vendor = extract_vendor(&stderr);
    let is_64bit = stderr.contains("64-Bit");

    Some(JavaInfo {
        path: java_path.to_string_lossy().to_string(),
        major_version,
        version_string,
        vendor,
        is_64bit,
    })
}

/// Extract version string from java -version output.
fn extract_version_string(output: &str) -> String {
    // Format: java version "1.8.0_392"
    // Or: openjdk version "21.0.2" ...
    for line in output.lines() {
        if line.contains("version") {
            if let Some(start) = line.find('"') {
                if let Some(end) = line[start + 1..].find('"') {
                    return line[start + 1..start + 1 + end].to_string();
                }
            }
        }
    }
    "Unknown".to_string()
}

/// Extract major version number.
pub fn extract_major_version(output: &str) -> u32 {
    for line in output.lines() {
        if line.contains("version") {
            // Check for "1.8.0" format (Java 8)
            if line.contains("\"1.8") || line.contains("\"1.7") {
                return 8;
            }
            // Check for "21.0.2" format (Java 9+)
            if let Some(start) = line.find("\"") {
                if let Some(end) = line[start + 1..].find('"') {
                    let version = &line[start + 1..start + 1 + end];
                    if let Some(dot_pos) = version.find('.') {
                        if let Ok(major) = version[..dot_pos].parse::<u32>() {
                            return major;
                        }
                    }
                }
            }
        }
    }
    0
}

/// Extract vendor name.
fn extract_vendor(output: &str) -> String {
    for line in output.lines() {
        if line.contains("version") {
            if line.contains("Temurin") || line.contains("Adoptium") {
                return "Eclipse Temurin".to_string();
            }
            if line.contains("Oracle") {
                return "Oracle".to_string();
            }
            if line.contains("Amazon") {
                return "Amazon Corretto".to_string();
            }
            if line.contains("Microsoft") {
                return "Microsoft".to_string();
            }
            if line.contains("OpenJDK") {
                return "OpenJDK".to_string();
            }
            if line.contains("Zulu") {
                return "Azul Zulu".to_string();
            }
        }
    }
    "Unknown".to_string()
}

/// Download and install a specific Java version from Adoptium.
#[tauri::command]
pub async fn download_java(major_version: u32) -> Result<JavaInfo, String> {
    tracing::info!("Downloading Java {} from Adoptium...", major_version);

    let arch = if cfg!(target_arch = "x86_64") {
        "x64"
    } else if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else {
        return Err("Unsupported architecture".to_string());
    };

    let os = if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "mac"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        return Err("Unsupported OS".to_string());
    };

    let extension = if cfg!(target_os = "windows") {
        "zip"
    } else {
        "tar.gz"
    };

    let url = format!(
        "https://api.adoptium.net/v3/binary/latest/{}/ga/{}/{}/jdk/hotspot/normal/eclipse",
        major_version, os, arch
    );

    tracing::info!("Resolving Download URL: {}", url);

    // Resolve runtimes directory
    let config = load_java_config().await;
    let runtimes_dir = config
        .custom_download_path
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            get_minecraft_base()
                .parent()
                .unwrap_or_else(|| std::path::Path::new("."))
                .join("runtimes")
        });

    tokio::fs::create_dir_all(&runtimes_dir)
        .await
        .map_err(|e| format!("Failed to create runtimes directory: {}", e))?;

    // Create a client that does NOT follow redirects automatically
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    let redirect_res = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Failed to resolve Adoptium API: {}", e))?;

    let mut final_url = url.clone();

    // If it's a redirect, get the Location header
    if redirect_res.status().is_redirection() {
        if let Some(loc) = redirect_res.headers().get(reqwest::header::LOCATION) {
            if let Ok(loc_str) = loc.to_str() {
                // Apply ghproxy to GitHub release URLs for better connectivity in China
                final_url = loc_str.replace("github.com", "mirror.ghproxy.com/github.com");
                tracing::info!("Redirected and proxied to: {}", final_url);
            }
        }
    } else if redirect_res.status().is_success() {
        tracing::info!("No redirect needed.");
    } else {
        return Err(format!(
            "Adoptium API returned error: {}",
            redirect_res.status()
        ));
    }

    // Download the file from the final URL
    let download_client = reqwest::Client::new();
    let response = download_client
        .get(&final_url)
        .send()
        .await
        .map_err(|e| format!("Failed to download Java: {}", e))?;

    if !response.status().is_success() {
        return Err(format!(
            "Failed to download Java: HTTP {}",
            response.status()
        ));
    }

    let filename = format!("jdk-{}.{}", major_version, extension);
    let download_path = runtimes_dir.join(&filename);

    // Download to file
    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("Failed to read response: {}", e))?;

    tokio::fs::write(&download_path, &bytes)
        .await
        .map_err(|e| format!("Failed to write file: {}", e))?;

    tracing::info!("Downloaded Java to: {}", download_path.display());

    // Extract the archive
    let extract_dir = runtimes_dir.join(format!("jdk-{}", major_version));

    if cfg!(target_os = "windows") {
        // Use PowerShell to extract zip on Windows
        let ps_command = format!(
            "Expand-Archive -Path '{}' -DestinationPath '{}' -Force",
            download_path.display(),
            runtimes_dir.display()
        );
        crate::core::utils::create_hidden_command("powershell")
            .args(["-Command", &ps_command])
            .output()
            .await
            .map_err(|e| format!("Failed to extract archive: {}", e))?;
    } else {
        // Use tar on macOS/Linux
        let output = crate::core::utils::create_hidden_command("tar")
            .args(["-xzf", &download_path.to_string_lossy()])
            .current_dir(&runtimes_dir)
            .output()
            .await
            .map_err(|e| format!("Failed to extract archive: {}", e))?;

        if !output.status.success() {
            return Err("Failed to extract Java archive".to_string());
        }
    }

    // Find the extracted java executable
    let java_path = find_extracted_java(&runtimes_dir, major_version)
        .await
        .ok_or_else(|| "Failed to find extracted Java".to_string())?;

    // Clean up the archive
    let _ = tokio::fs::remove_file(&download_path).await;

    // Probe and return the Java info
    probe_java(&java_path)
        .await
        .ok_or_else(|| "Failed to probe downloaded Java".to_string())
}

/// Find the extracted Java executable in the runtimes directory.
async fn find_extracted_java(runtimes_dir: &PathBuf, major_version: u32) -> Option<PathBuf> {
    let mut entries = tokio::fs::read_dir(runtimes_dir).await.ok()?;
    let mut entries = entries;

    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if path.is_dir() {
            let dir_name = path.file_name()?.to_string_lossy();
            if dir_name.contains(&format!("{}", major_version)) {
                let java_path = path.join("bin").join("java.exe");
                if java_path.exists() {
                    return Some(java_path);
                }
                // macOS/Linux
                let java_path_alt = path.join("bin").join("java");
                if java_path_alt.exists() {
                    return Some(java_path_alt);
                }
            }
        }
    }
    None
}

/// Get the recommended Java version for a given Minecraft version.
#[tauri::command]
pub fn get_recommended_java(mc_version: &str) -> u32 {
    // MC 1.17+ requires Java 17+
    // MC 1.20.5+ requires Java 21
    let version_parts: Vec<&str> = mc_version.split('.').collect();

    // Determine if the version starts with "1." (classic format) or uses a new format (e.g. "26.1.2")
    let is_classic = version_parts.get(0) == Some(&"1");

    let major: u32 = if is_classic {
        version_parts
            .get(1)
            .and_then(|v| v.parse().ok())
            .unwrap_or(0)
    } else {
        version_parts
            .get(0)
            .and_then(|v| v.parse().ok())
            .unwrap_or(0)
    };

    let minor: u32 = if is_classic {
        version_parts
            .get(2)
            .and_then(|v| v.parse().ok())
            .unwrap_or(0)
    } else {
        version_parts
            .get(1)
            .and_then(|v| v.parse().ok())
            .unwrap_or(0)
    };

    if major > 20 || (major == 20 && minor >= 5) {
        // 1.20.5+ (or 21+) requires Java 21
        return 21;
    } else if major >= 17 {
        // 1.17.x - 1.20.4 requires Java 17
        return 17;
    } else if major >= 8 {
        // 1.8.x - 1.16.x requires Java 8
        return 8;
    }

    // Default to Java 8 for older versions
    8
}

#[tauri::command]
pub async fn add_manual_java(path: String) -> Result<JavaInfo, String> {
    *CACHED_JAVAS.lock().await = None;
    let java_path = PathBuf::from(&path);
    if !java_path.exists() {
        return Err("Java path does not exist".to_string());
    }

    let mut java_info = probe_java(&java_path)
        .await
        .ok_or_else(|| "Failed to probe Java. Is this a valid Java executable?".to_string())?;

    let mut config = load_java_config().await;
    // Don't add duplicate
    if !config.manual_paths.contains(&java_info.path) {
        config.manual_paths.push(java_info.path.clone());
        save_java_config(&config).await?;
    }

    Ok(java_info)
}

#[tauri::command]
pub async fn remove_java(path: String) -> Result<(), String> {
    *CACHED_JAVAS.lock().await = None;
    let mut config = load_java_config().await;

    // Check if it's in manual paths
    if let Some(pos) = config.manual_paths.iter().position(|p| p == &path) {
        config.manual_paths.remove(pos);
        save_java_config(&config).await?;
        tracing::info!("Removed manual Java path: {}", path);
        return Ok(());
    }

    // Check if it's a downloaded Java in runtimes or custom download path
    let is_managed = {
        let runtimes_dir = get_minecraft_base()
            .join("runtimes")
            .to_string_lossy()
            .to_string();
        let custom_dir = config
            .custom_download_path
            .as_deref()
            .unwrap_or(&runtimes_dir);
        path.starts_with(&runtimes_dir) || path.starts_with(custom_dir)
    };

    if is_managed {
        let java_path = PathBuf::from(&path);
        // We want to delete the whole jdk directory, not just java.exe
        // Usually it's in `jdk-17/bin/java.exe`, so we go up 2 levels
        if let Some(bin_dir) = java_path.parent() {
            if let Some(jdk_dir) = bin_dir.parent() {
                if jdk_dir.exists() {
                    tokio::fs::remove_dir_all(jdk_dir)
                        .await
                        .map_err(|e| format!("Failed to delete Java directory: {}", e))?;
                    tracing::info!("Deleted managed Java at: {}", jdk_dir.display());
                    return Ok(());
                }
            }
        }
        return Err("Could not determine JDK root directory for deletion".to_string());
    }

    Err(
        "Cannot remove this Java. It is neither manually added nor managed by the launcher."
            .to_string(),
    )
}

#[tauri::command]
pub async fn get_java_download_path() -> Result<Option<String>, String> {
    let config = load_java_config().await;
    Ok(config.custom_download_path)
}

#[tauri::command]
pub async fn set_java_download_path(path: Option<String>) -> Result<(), String> {
    let mut config = load_java_config().await;
    config.custom_download_path = path;
    save_java_config(&config).await?;
    Ok(())
}

#[tauri::command]
pub async fn scan_full_disk(app: tauri::AppHandle) -> Result<(), String> {
    *CACHED_JAVAS.lock().await = None;
    tracing::info!("Starting full disk scan for Java...");

    // Spawn blocking so we don't hang the async executor
    tokio::task::spawn_blocking(move || {
        let drives = if cfg!(target_os = "windows") {
            vec!["C:\\", "D:\\", "E:\\", "F:\\"]
        } else {
            vec!["/"]
        };

        let mut found_paths = Vec::new();

        for drive in drives {
            let root = PathBuf::from(drive);
            if !root.exists() {
                continue;
            }

            // Limit depth to 5 to avoid infinite/long loops in deeply nested dirs,
            // and filter out some obvious system directories that are huge and won't contain user-installed Java.
            let walker = walkdir::WalkDir::new(&root)
                .max_depth(5)
                .into_iter()
                .filter_entry(|e| {
                    let name = e.file_name().to_string_lossy().to_lowercase();
                    // Skip common heavy directories that likely don't have Java
                    !name.contains("windows")
                        && !name.contains("system32")
                        && !name.contains("node_modules")
                        && !name.contains(".git")
                });

            for entry in walker.filter_map(|e| e.ok()) {
                let path = entry.path();
                if path.is_file() {
                    if let Some(file_name) = path.file_name() {
                        let name_str = file_name.to_string_lossy().to_lowercase();
                        if name_str == "java.exe" || name_str == "java" {
                            // Let the UI know we are scanning
                            let _ = app.emit(
                                "java-scan-progress",
                                serde_json::json!({
                                    "status": "scanning",
                                    "currentPath": path.to_string_lossy()
                                }),
                            );

                            found_paths.push(path.to_string_lossy().to_string());
                        }
                    }
                }
            }
        }

        // Add discovered paths to config
        // Since we are in a blocking thread, we can block_on to run async functions
        let runtime = tokio::runtime::Handle::current();
        runtime.block_on(async {
            let mut config = load_java_config().await;
            let mut updated = false;

            for path in found_paths {
                if !config.manual_paths.contains(&path) {
                    // Quick probe
                    let java_path = PathBuf::from(&path);
                    if let Some(_info) = probe_java(&java_path).await {
                        config.manual_paths.push(path);
                        updated = true;
                    }
                }
            }

            if updated {
                let _ = save_java_config(&config).await;
            }
            
            // Clear the cache AFTER the scan so the frontend will fetch the newly found Javas
            *CACHED_JAVAS.lock().await = None;
        });

        let _ = app.emit(
            "java-scan-progress",
            serde_json::json!({
                "status": "complete"
            }),
        );
    });

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_major_version() {
        assert_eq!(extract_major_version("java version \"1.8.0_392\""), 8);
        assert_eq!(extract_major_version("openjdk version \"17.0.2\""), 17);
        assert_eq!(extract_major_version("openjdk version \"21.0.2\""), 21);
    }

    #[test]
    fn test_extract_version_string() {
        assert_eq!(
            extract_version_string("java version \"1.8.0_392\""),
            "1.8.0_392"
        );
        assert_eq!(
            extract_version_string("openjdk version \"17.0.2\""),
            "17.0.2"
        );
    }

    #[test]
    fn test_get_recommended_java() {
        assert_eq!(get_recommended_java("1.20.5"), 21);
        assert_eq!(get_recommended_java("1.19.4"), 17);
        assert_eq!(get_recommended_java("1.16.5"), 8);
        assert_eq!(get_recommended_java("1.7.10"), 8);
    }
}

#[tauri::command]
pub async fn clear_java_cache() -> Result<(), String> {
    *CACHED_JAVAS.lock().await = None;
    Ok(())
}
