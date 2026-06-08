#![allow(dead_code)]
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
pub fn extract_zip<P: AsRef<Path>>(zip_path: P, extract_dir: P) -> Result<(), String> {
    let zip_file =
        std::fs::File::open(zip_path).map_err(|e| format!("Failed to open zip: {}", e))?;
    let mut archive =
        zip::ZipArchive::new(zip_file).map_err(|e| format!("Failed to read zip archive: {}", e))?;

    std::fs::create_dir_all(&extract_dir)
        .map_err(|e| format!("Failed to create extract directory: {}", e))?;

    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| format!("Failed to access file in zip: {}", e))?;
        let outpath = match file.enclosed_name() {
            Some(path) => extract_dir.as_ref().join(path),
            None => continue,
        };

        if file.name().ends_with('/') {
            std::fs::create_dir_all(&outpath)
                .map_err(|e| format!("Failed to create directory from zip: {}", e))?;
        } else {
            if let Some(p) = outpath.parent() {
                if !p.exists() {
                    std::fs::create_dir_all(&p)
                        .map_err(|e| format!("Failed to create parent directory: {}", e))?;
                }
            }
            let mut outfile = std::fs::File::create(&outpath)
                .map_err(|e| format!("Failed to create output file: {}", e))?;
            std::io::copy(&mut file, &mut outfile)
                .map_err(|e| format!("Failed to extract file: {}", e))?;
        }
    }

    Ok(())
}

/// Parses the modpack manifest from an extracted directory.
pub fn parse_modpack_manifest(extract_dir: &PathBuf) -> Result<ModpackType, String> {
    let cf_manifest_path = extract_dir.join("manifest.json");
    if cf_manifest_path.exists() {
        tracing::info!("Found CurseForge manifest.json");
        let content = std::fs::read_to_string(&cf_manifest_path).map_err(|e| e.to_string())?;
        let manifest: CfManifest =
            serde_json::from_str(&content).map_err(|e| format!("Invalid CF manifest: {}", e))?;
        return Ok(ModpackType::CurseForge(manifest));
    }

    let mr_manifest_path = extract_dir.join("modrinth.index.json");
    if mr_manifest_path.exists() {
        tracing::info!("Found Modrinth modrinth.index.json");
        let content = std::fs::read_to_string(&mr_manifest_path).map_err(|e| e.to_string())?;
        let manifest: ModrinthManifest =
            serde_json::from_str(&content).map_err(|e| format!("Invalid Modrinth index: {}", e))?;
        return Ok(ModpackType::Modrinth(manifest));
    }

    Err("Unknown modpack format: Neither manifest.json nor modrinth.index.json found.".into())
}

/// Copies the overrides folder from the extracted modpack to the instance root.
pub fn copy_overrides(
    extract_dir: &PathBuf,
    instance_dir: &PathBuf,
    overrides_folder: &str,
) -> Result<(), String> {
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

    for entry in WalkDir::new(&overrides_path)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();

        if path.is_file() {
            let relative_path = path.strip_prefix(&overrides_path).unwrap();
            let dest_path = instance_dir.join(relative_path);

            if let Some(parent) = dest_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }

            std::fs::copy(path, &dest_path)
                .map_err(|e| format!("Failed to copy override file {:?}: {}", path, e))?;
        }
    }

    Ok(())
}
