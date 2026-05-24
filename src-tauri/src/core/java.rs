//! Java Management Module
//! Provides functionality to scan, manage, and download Java runtimes.

use crate::core::mojang::get_minecraft_base;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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
    tracing::info!("Scanning local Java installations...");

    let mut javas = Vec::new();

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

    // 2. Scan common installation directories based on OS
    let search_paths = get_java_search_paths();

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
            paths.push(PathBuf::from(&home).join("AppData").join("Local").join("Programs").join("Java"));
            paths.push(PathBuf::from(&home).join("AppData").join("Local").join("Programs").join("Eclipse Adoptium"));
        }
        #[cfg(target_os = "macos")]
        {
            paths.push(PathBuf::from(&home).join("Library").join("Java").join("JavaVirtualMachines"));
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
    let output = tokio::process::Command::new(java_path)
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
fn extract_major_version(output: &str) -> u32 {
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

    let extension = if cfg!(target_os = "windows") { "zip" } else { "tar.gz" };

    // Construct Adoptium API URL
    let url = format!(
        "https://api.adoptium.net/v3/binary/latest/{}/ga/{}/{}/jdk/hotspot/normal/eclipse",
        major_version, os, arch
    );

    tracing::info!("Download URL: {}", url);

    // Create runtimes directory
    let base_dir = get_minecraft_base();
    let runtimes_dir = base_dir.join("runtimes");
    tokio::fs::create_dir_all(&runtimes_dir)
        .await
        .map_err(|e| format!("Failed to create runtimes directory: {}", e))?;

    // Download the file
    let response = reqwest::get(&url)
        .await
        .map_err(|e| format!("Failed to download Java: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("Failed to download Java: HTTP {}", response.status()));
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
        tokio::process::Command::new("powershell")
            .args(["-Command", &ps_command])
            .output()
            .await
            .map_err(|e| format!("Failed to extract archive: {}", e))?;
    } else {
        // Use tar on macOS/Linux
        let output = tokio::process::Command::new("tar")
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
    if version_parts.len() >= 2 {
        let major: u32 = version_parts[1].parse().unwrap_or(0);
        
        if major >= 20 {
            // 1.20.x - Java 21 recommended
            return 21;
        } else if major >= 17 {
            // 1.17.x - 1.19.x - Java 17 recommended
            return 17;
        } else if major >= 8 {
            // 1.8.x - 1.16.x - Java 8 or 11 recommended
            return 8;
        }
    }
    // Default to Java 8 for older versions
    8
}