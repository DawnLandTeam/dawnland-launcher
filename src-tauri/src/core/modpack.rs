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

/// Copies the overrides folder from the extracted modpack to the instance root.
pub async fn copy_overrides(
    extract_dir: &std::path::Path,
    instance_dir: &std::path::Path,
    overrides_folder: &str,
) -> Result<(), DawnlandError> {
    let overrides_path = extract_dir.join(overrides_folder);
    if !overrides_path.exists() || !overrides_path.is_dir() {
        tracing::warn!(
            "Overrides folder '{}' not found or is not a directory. Skipping.",
            overrides_folder
        );
        return Ok(());
    }

    tracing::info!(
        "Copying overrides from {:?} to {:?}",
        overrides_path,
        instance_dir
    );

    let overrides_path_clone = overrides_path.to_path_buf();
    let instance_dir_clone = instance_dir.to_path_buf();
    tokio::task::spawn_blocking(move || {
        for entry in WalkDir::new(&overrides_path_clone)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();

            if path.is_file() {
                let relative_path = path.strip_prefix(&overrides_path_clone).unwrap();
                let dest_path = instance_dir_clone.join(relative_path);

                if let Some(parent) = dest_path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }

                std::fs::copy(path, &dest_path).map_err(|e| {
                    DawnlandError::Unknown(format!(
                        "Failed to copy override file {:?}: {}",
                        path, e
                    ))
                })?;
            }
        }
        Ok::<(), DawnlandError>(())
    })
    .await
    .map_err(|e| DawnlandError::ProcessError(format!("Task join error: {}", e)))??;

    Ok(())
}
