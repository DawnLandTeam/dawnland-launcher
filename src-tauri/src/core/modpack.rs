#![allow(dead_code)]
use crate::error::DawnlandError;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

// ============================================================================
// CurseForge Modpack Structs
// ============================================================================

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CfManifest {
    pub minecraft: CfMinecraftInfo,
    pub name: String,
    pub version: String,
    pub files: Vec<CfModFile>,
    #[serde(default = "default_overrides")]
    pub overrides: String, // Typically "overrides"
}

fn default_overrides() -> String {
    "overrides".to_string()
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CfMinecraftInfo {
    pub version: String,
    pub mod_loaders: Vec<CfModLoader>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CfModLoader {
    pub id: String, // e.g., "forge-47.2.0" or "fabric-0.14.22"
    pub primary: bool,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CfModFile {
    #[serde(rename = "projectID")]
    pub project_id: u32,
    #[serde(rename = "fileID")]
    pub file_id: u32,
    pub required: bool,
}

// ============================================================================
// Modrinth Modpack Structs
// ============================================================================

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ModrinthManifest {
    pub format_version: u32,
    pub game: String,
    pub version_id: String,
    pub name: String,
    pub dependencies: ModrinthDependencies,
    pub files: Vec<ModrinthModFile>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "kebab-case")]
pub struct ModrinthDependencies {
    pub minecraft: String,
    pub forge: Option<String>,
    pub fabric_loader: Option<String>,
    pub quilt_loader: Option<String>,
    pub neoforge: Option<String>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ModrinthModFile {
    pub path: String, // e.g., "mods/sodium-1.2.jar"
    pub hashes: std::collections::HashMap<String, String>,
    pub env: Option<ModrinthEnv>,
    pub downloads: Vec<String>,
    pub file_size: u64,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ModrinthEnv {
    pub client: String, // "required", "optional", "unsupported"
    pub server: String,
}

// ============================================================================
// Unified Parsing Entry Point
// ============================================================================

pub enum ModpackType {
    CurseForge(CfManifest),
    Modrinth(ModrinthManifest),
}

/// Helper function to extract a ZIP file to a target directory using `zip-rs`.
pub async fn extract_zip<P: AsRef<Path>>(zip_path: P, extract_dir: P) -> Result<(), DawnlandError> {
    let zip_path_buf = zip_path.as_ref().to_path_buf();
    let extract_dir_buf = extract_dir.as_ref().to_path_buf();

    tokio::task::spawn_blocking(move || {
        let zip_file = std::fs::File::open(&zip_path_buf)
            .map_err(|e| DawnlandError::Unknown(format!("Failed to open zip: {}", e)))?;
        let mut archive = zip::ZipArchive::new(zip_file)
            .map_err(|e| DawnlandError::Unknown(format!("Failed to read zip archive: {}", e)))?;

        std::fs::create_dir_all(&extract_dir_buf).map_err(|e| {
            DawnlandError::Unknown(format!("Failed to create extract directory: {}", e))
        })?;

        for i in 0..archive.len() {
            let mut file = archive.by_index(i).map_err(|e| {
                DawnlandError::Unknown(format!("Failed to access file in zip: {}", e))
            })?;
            let outpath = match file.enclosed_name() {
                Some(path) => extract_dir_buf.join(path),
                None => continue,
            };

            if file.name().ends_with('/') {
                std::fs::create_dir_all(&outpath).map_err(|e| {
                    DawnlandError::Unknown(format!("Failed to create directory from zip: {}", e))
                })?;
            } else {
                if let Some(p) = outpath.parent() {
                    if !p.exists() {
                        std::fs::create_dir_all(p).map_err(|e| {
                            DawnlandError::Unknown(format!(
                                "Failed to create parent directory: {}",
                                e
                            ))
                        })?;
                    }
                }
                let mut outfile = std::fs::File::create(&outpath).map_err(|e| {
                    DawnlandError::Unknown(format!("Failed to create output file: {}", e))
                })?;
                std::io::copy(&mut file, &mut outfile).map_err(|e| {
                    DawnlandError::Unknown(format!("Failed to extract file: {}", e))
                })?;
            }
        }
        Ok::<(), DawnlandError>(())
    })
    .await
    .map_err(|e| DawnlandError::ProcessError(format!("Task join error: {}", e)))??;

    Ok(())
}

/// Helper function to parse CurseForge-like manifests
async fn parse_cf_format_manifest(
    path: &PathBuf,
    format_name: &str,
) -> Result<ModpackType, DawnlandError> {
    tracing::info!(
        "Found {}",
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(format_name)
    );
    let content = tokio::fs::read_to_string(path).await?;
    let manifest: CfManifest = serde_json::from_str(&content)
        .map_err(|e| DawnlandError::Unknown(format!("Invalid {} manifest: {}", format_name, e)))?;
    Ok(ModpackType::CurseForge(manifest))
}

/// Parses the modpack manifest from an extracted directory.
pub async fn parse_modpack_manifest(extract_dir: &std::path::Path) -> Result<ModpackType, DawnlandError> {
    let cf_manifest_path = extract_dir.join("manifest.json");
    if cf_manifest_path.exists() {
        return parse_cf_format_manifest(&cf_manifest_path, "CF").await;
    }

    let mcbbs_manifest_path = extract_dir.join("mcbbs.pack.json");
    if mcbbs_manifest_path.exists() {
        // MCBBS format is structurally compatible with CurseForge manifest
        return parse_cf_format_manifest(&mcbbs_manifest_path, "MCBBS").await;
    }

    let mr_manifest_path = extract_dir.join("modrinth.index.json");
    if mr_manifest_path.exists() {
        tracing::info!("Found Modrinth modrinth.index.json");
        let content = tokio::fs::read_to_string(&mr_manifest_path).await?;
        let manifest: ModrinthManifest = serde_json::from_str(&content)
            .map_err(|e| DawnlandError::Unknown(format!("Invalid Modrinth index: {}", e)))?;
        return Ok(ModpackType::Modrinth(manifest));
    }

    Err(DawnlandError::Unknown("Unknown modpack format: None of manifest.json, mcbbs.pack.json, or modrinth.index.json found.".into()))
}

/// Recursively removes read-only attributes from all files and directories, then removes the directory.
fn force_remove_dir_all(dir: &std::path::Path) -> std::io::Result<()> {
    for entry_result in WalkDir::new(dir) {
        let entry = match entry_result {
            Ok(e) => e,
            Err(err) => {
                tracing::warn!("Failed to read directory entry during permission reset: {}", err);
                continue;
            }
        };
        let path = entry.path();
        if let Ok(metadata) = std::fs::symlink_metadata(path) {
            if metadata.file_type().is_symlink() {
                continue;
            }
            let mut perms = metadata.permissions();
            if perms.readonly() {
                #[allow(clippy::permissions_set_readonly_false)]
                perms.set_readonly(false);
                let _ = std::fs::set_permissions(path, perms);
            }
        }
    }
    std::fs::remove_dir_all(dir)
}

/// Copies the overrides folder from the extracted modpack to the instance root.
pub async fn copy_overrides(
    extract_dir: &std::path::Path,
    instance_dir: &std::path::Path,
    overrides_folder: &str,
) -> Result<(), DawnlandError> {
    copy_overrides_with_progress(extract_dir, instance_dir, overrides_folder, None, None)
        .await
        .map(|_| ())
}

/// Copies the overrides folder from the extracted modpack to the instance root with progress reporting and cancellation support.
pub async fn copy_overrides_with_progress(
    extract_dir: &std::path::Path,
    instance_dir: &std::path::Path,
    overrides_folder: &str,
    cancel_token: Option<tokio_util::sync::CancellationToken>,
    progress_tx: Option<tokio::sync::mpsc::Sender<(u64, u64, String)>>,
) -> Result<usize, DawnlandError> {
    if !crate::core::fs_utils::is_safe_relative_path(overrides_folder) {
        tracing::warn!(
            "Unsafe overrides folder '{}' rejected.",
            overrides_folder
        );
        return Ok(0);
    }
    let overrides_path = extract_dir.join(overrides_folder);
    if !overrides_path.exists() || !overrides_path.is_dir() {
        tracing::warn!(
            "Overrides folder '{}' not found or is not a directory. Skipping.",
            overrides_folder
        );
        return Ok(0);
    }

    tracing::info!(
        "Scanning overrides from {:?} to prepare copying to {:?}",
        overrides_path,
        instance_dir
    );

    let overrides_path_clone = overrides_path.to_path_buf();
    let instance_dir_clone = instance_dir.to_path_buf();
    let copied_count = tokio::task::spawn_blocking(move || -> Result<usize, DawnlandError> {
        // Collect valid files first for accurate total progress reporting
        let mut files_to_copy = Vec::new();
        for entry_result in WalkDir::new(&overrides_path_clone) {
            let entry = match entry_result {
                Ok(e) => e,
                Err(err) => {
                    tracing::warn!("Failed to read directory entry while traversing overrides: {}", err);
                    continue;
                }
            };

            // Skip symlinks to prevent path traversal or leaking sensitive files
            if entry.path_is_symlink() || entry.file_type().is_symlink() {
                tracing::warn!("Skipping symlink in overrides: {:?}", entry.path());
                continue;
            }

            let path = entry.path();
            if path.is_file() {
                let relative_path = match path.strip_prefix(&overrides_path_clone) {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::warn!("Failed to strip prefix for {:?}: {}", path, e);
                        continue;
                    }
                };
                let rel_str = relative_path.to_string_lossy();
                if !crate::core::fs_utils::is_safe_relative_path(&rel_str) {
                    tracing::warn!("Skipping unsafe relative path in overrides: {}", rel_str);
                    continue;
                }
                files_to_copy.push((path.to_path_buf(), relative_path.to_path_buf()));
            }
        }

        let total_files = files_to_copy.len();
        tracing::info!(
            "Found {} override files to copy from {:?} to {:?}",
            total_files,
            overrides_path_clone,
            instance_dir_clone
        );

        // Cache for verified safe parent directories to avoid redundant, expensive symlink checks and directory creation
        let mut verified_safe_dirs = std::collections::HashSet::new();
        verified_safe_dirs.insert(instance_dir_clone.clone());

        let mut copied = 0u64;
        let mut last_emit = std::time::Instant::now();

        for (src_path, relative_path) in files_to_copy {
            if let Some(ref token) = cancel_token {
                if token.is_cancelled() {
                    return Err(DawnlandError::ProcessError("Overrides copying cancelled by user".to_string()));
                }
            }

            let dest_path = instance_dir_clone.join(&relative_path);

            if let Some(parent) = dest_path.parent() {
                let mut check = parent;
                let mut unverified = Vec::new();
                while check.starts_with(&instance_dir_clone) && check != instance_dir_clone {
                    if verified_safe_dirs.contains(check) {
                        break;
                    }
                    unverified.push(check);
                    match check.parent() {
                        Some(p) => check = p,
                        None => break,
                    }
                }

                // Verify unverified paths top-down
                let mut has_symlink = false;
                for dir in unverified.iter().rev() {
                    if let Ok(meta) = std::fs::symlink_metadata(dir) {
                        if meta.file_type().is_symlink() {
                            has_symlink = true;
                            break;
                        }
                    }
                    verified_safe_dirs.insert(dir.to_path_buf());
                }

                if has_symlink {
                    tracing::warn!("Skipping override file because parent path is a symlink: {:?}", dest_path);
                    continue;
                }

                if !parent.exists() {
                    std::fs::create_dir_all(parent).map_err(|e| {
                        DawnlandError::Unknown(format!(
                            "Failed to create parent directory for override file {:?}: {}",
                            dest_path, e
                        ))
                    })?;
                }
            }

            if let Ok(dest_metadata) = std::fs::symlink_metadata(&dest_path) {
                if dest_metadata.file_type().is_symlink() {
                    #[cfg(windows)]
                    {
                        use std::os::windows::fs::FileTypeExt;
                        if dest_metadata.file_type().is_symlink_dir() {
                            std::fs::remove_dir(&dest_path).map_err(|e| {
                                DawnlandError::Unknown(format!(
                                    "Failed to remove existing directory symlink {:?}: {}",
                                    dest_path, e
                                ))
                            })?;
                        } else {
                            std::fs::remove_file(&dest_path).map_err(|e| {
                                DawnlandError::Unknown(format!(
                                    "Failed to remove existing file symlink {:?}: {}",
                                    dest_path, e
                                ))
                            })?;
                        }
                    }
                    #[cfg(not(windows))]
                    {
                        std::fs::remove_file(&dest_path).map_err(|e| {
                            DawnlandError::Unknown(format!(
                                "Failed to remove existing symlink {:?}: {}",
                                dest_path, e
                            ))
                        })?;
                    }
                } else if dest_metadata.file_type().is_dir() {
                    force_remove_dir_all(&dest_path).map_err(|e| {
                        DawnlandError::Unknown(format!(
                            "Failed to remove existing directory {:?}: {}",
                            dest_path, e
                        ))
                    })?;
                } else {
                    let mut perms = dest_metadata.permissions();
                    if perms.readonly() {
                        #[allow(clippy::permissions_set_readonly_false)]
                        perms.set_readonly(false);
                        let _ = std::fs::set_permissions(&dest_path, perms);
                    }
                    std::fs::remove_file(&dest_path).map_err(|e| {
                        DawnlandError::Unknown(format!(
                            "Failed to remove existing file {:?}: {}",
                            dest_path, e
                        ))
                    })?;
                }
            }

            if let Ok(final_check) = std::fs::symlink_metadata(&dest_path) {
                if final_check.file_type().is_symlink() {
                    return Err(DawnlandError::Unknown(format!(
                        "Refusing to copy override file to symlink destination: {:?}",
                        dest_path
                    )));
                }
            }

            std::fs::copy(&src_path, &dest_path).map_err(|e| {
                DawnlandError::Unknown(format!(
                    "Failed to copy override file {:?} to {:?}: {}",
                    src_path, dest_path, e
                ))
            })?;

            copied += 1;

            if let Some(ref tx) = progress_tx {
                let now = std::time::Instant::now();
                if copied == total_files as u64 || now.duration_since(last_emit).as_millis() >= 100 {
                    last_emit = now;
                    let file_name = relative_path.to_string_lossy().to_string();
                    let _ = tx.blocking_send((copied, total_files as u64, file_name));
                }
            }
        }

        tracing::info!(
            "Successfully copied {}/{} override files to {:?}",
            copied,
            total_files,
            instance_dir_clone
        );

        Ok(copied as usize)
    })
    .await
    .map_err(|e| DawnlandError::ProcessError(format!("Task join error: {}", e)))??;

    Ok(copied_count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_copy_overrides_removes_file_symlink() {
        let temp = tempdir().expect("tempdir");
        let extract_dir = temp.path().join("extract");
        let instance_dir = temp.path().join("instance");
        let overrides = extract_dir.join("overrides");
        std::fs::create_dir_all(&overrides).expect("create overrides");
        std::fs::create_dir_all(&instance_dir).expect("create instance");

        let override_file = overrides.join("config.txt");
        std::fs::write(&override_file, "new content").expect("write override");

        let outside_dir = temp.path().join("outside");
        std::fs::create_dir_all(&outside_dir).expect("create outside");
        let outside_file = outside_dir.join("target.txt");
        std::fs::write(&outside_file, "original content").expect("write outside");

        let symlink_dest = instance_dir.join("config.txt");
        #[cfg(windows)]
        {
            if std::os::windows::fs::symlink_file(&outside_file, &symlink_dest).is_ok() {
                let res = copy_overrides(&extract_dir, &instance_dir, "overrides").await;
                assert!(res.is_ok());
                assert_eq!(std::fs::read_to_string(&outside_file).unwrap(), "original content");
                assert_eq!(std::fs::read_to_string(&symlink_dest).unwrap(), "new content");
                let meta = std::fs::symlink_metadata(&symlink_dest).unwrap();
                assert!(!meta.file_type().is_symlink());
            }
        }
        #[cfg(unix)]
        {
            if std::os::unix::fs::symlink(&outside_file, &symlink_dest).is_ok() {
                let res = copy_overrides(&extract_dir, &instance_dir, "overrides").await;
                assert!(res.is_ok());
                assert_eq!(std::fs::read_to_string(&outside_file).unwrap(), "original content");
                assert_eq!(std::fs::read_to_string(&symlink_dest).unwrap(), "new content");
                let meta = std::fs::symlink_metadata(&symlink_dest).unwrap();
                assert!(!meta.file_type().is_symlink());
            }
        }
    }

    #[tokio::test]
    async fn test_copy_overrides_removes_directory_symlink() {
        let temp = tempdir().expect("tempdir");
        let extract_dir = temp.path().join("extract");
        let instance_dir = temp.path().join("instance");
        let overrides = extract_dir.join("overrides");
        std::fs::create_dir_all(&overrides).expect("create overrides");
        std::fs::create_dir_all(&instance_dir).expect("create instance");

        let override_file = overrides.join("data_link");
        std::fs::write(&override_file, "override data").expect("write override");

        let outside_dir = temp.path().join("outside_dir");
        std::fs::create_dir_all(&outside_dir).expect("create outside dir");
        let canary_file = outside_dir.join("canary.txt");
        std::fs::write(&canary_file, "canary").expect("write canary");

        let symlink_dest = instance_dir.join("data_link");
        #[cfg(windows)]
        {
            if std::os::windows::fs::symlink_dir(&outside_dir, &symlink_dest).is_ok() {
                let res = copy_overrides(&extract_dir, &instance_dir, "overrides").await;
                assert!(res.is_ok());
                assert_eq!(std::fs::read_to_string(&canary_file).unwrap(), "canary");
                assert_eq!(std::fs::read_to_string(&symlink_dest).unwrap(), "override data");
                let meta = std::fs::symlink_metadata(&symlink_dest).unwrap();
                assert!(!meta.file_type().is_symlink());
            }
        }
        #[cfg(unix)]
        {
            if std::os::unix::fs::symlink(&outside_dir, &symlink_dest).is_ok() {
                let res = copy_overrides(&extract_dir, &instance_dir, "overrides").await;
                assert!(res.is_ok());
                assert_eq!(std::fs::read_to_string(&canary_file).unwrap(), "canary");
                assert_eq!(std::fs::read_to_string(&symlink_dest).unwrap(), "override data");
                let meta = std::fs::symlink_metadata(&symlink_dest).unwrap();
                assert!(!meta.file_type().is_symlink());
            }
        }
    }
}
