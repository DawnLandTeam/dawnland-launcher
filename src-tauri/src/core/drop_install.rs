//! Drag-and-Drop File Inspection and Direct Installation Engine
//!
//! Provides fast archive structure sniffing (Modpack, Mod, ResourcePack, ShaderPack,
//! Datapack, World) and safe non-blocking local file installation into instances.

use crate::core::fs_utils::compute_sha1;
use crate::core::manager::{delete_asset_from_manifest, get_instance_saves, save_asset_to_manifest};
use crate::core::mojang::get_minecraft_base;
use crate::error::{AppError, DawnlandError};
use crate::models::instance::{AssetCategory, AssetRecord, AssetSourceType};
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::{Path, PathBuf};

/// Classification of a dropped file or folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DetectedFileKind {
    Modpack,
    Mod,
    ResourcePack,
    ShaderPack,
    Datapack,
    World,
    Unknown,
}

/// Metadata returned when inspecting a dropped file or directory.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectedDropFile {
    pub path: String,
    pub filename: String,
    pub kind: DetectedFileKind,
    pub display_name: Option<String>,
    pub detected_loader: Option<String>,
    pub detected_mc_version: Option<String>,
    pub size: u64,
    pub is_dir: bool,
}

/// Input item for batch importing dropped files into an instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DropImportItem {
    pub path: String,
    pub kind_override: Option<DetectedFileKind>,
}

/// Successfully imported file entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedFileResult {
    pub filename: String,
    pub kind: DetectedFileKind,
    pub target_rel_path: String,
}

/// Skipped file entry (e.g., already exists, requires world selection).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkippedFileResult {
    pub filename: String,
    pub kind: DetectedFileKind,
    pub reason: String,
}

/// Failed file entry with error details.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FailedFileResult {
    pub filename: String,
    pub error: String,
}

/// Summary report of a batch drop-import operation.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DropImportReport {
    pub succeeded: Vec<ImportedFileResult>,
    pub skipped: Vec<SkippedFileResult>,
    pub failed: Vec<FailedFileResult>,
}

/// Validate that a filename or world folder name is a single safe path component.
fn is_safe_single_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains('/')
        && !name.contains('\\')
        && !name.contains('\0')
}

/// Normalize a zip entry path to forward slashes without leading slash.
fn normalize_entry_name(raw: &str) -> String {
    raw.replace('\\', "/").trim_start_matches('/').to_string()
}

/// Inspect a `.jar` archive for Mod display name, loader type, and Minecraft version requirement.
fn inspect_jar_archive(path: &Path) -> (Option<String>, Option<String>, Option<String>) {
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return (None, None, None),
    };
    let mut archive = match zip::ZipArchive::new(file) {
        Ok(a) => a,
        Err(_) => return (None, None, None),
    };

    // 1. Check Fabric (`fabric.mod.json`)
    if let Ok(mut f) = archive.by_name("fabric.mod.json") {
        let mut content = String::new();
        if f.read_to_string(&mut content).is_ok() {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                let name = json
                    .get("name")
                    .and_then(|v| v.as_str())
                    .or_else(|| json.get("id").and_then(|v| v.as_str()))
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty());
                let mc_ver = json
                    .get("depends")
                    .and_then(|d| d.get("minecraft"))
                    .and_then(extract_json_version_req);
                return (name, Some("Fabric".to_string()), mc_ver);
            }
        }
        return (None, Some("Fabric".to_string()), None);
    }

    // 2. Check Quilt (`quilt.mod.json`)
    if let Ok(mut f) = archive.by_name("quilt.mod.json") {
        let mut content = String::new();
        if f.read_to_string(&mut content).is_ok() {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                let ql = json.get("quilt_loader");
                let name = ql
                    .and_then(|q| q.get("metadata"))
                    .and_then(|m| m.get("name"))
                    .and_then(|v| v.as_str())
                    .or_else(|| ql.and_then(|q| q.get("id")).and_then(|v| v.as_str()))
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty());
                let mc_ver = ql
                    .and_then(|q| q.get("depends"))
                    .and_then(|d| d.as_array())
                    .and_then(|arr| {
                        arr.iter().find_map(|dep| {
                            if dep.get("id").and_then(|v| v.as_str()) == Some("minecraft") {
                                dep.get("versions").and_then(extract_json_version_req)
                            } else {
                                None
                            }
                        })
                    });
                return (name, Some("Quilt".to_string()), mc_ver);
            }
        }
        return (None, Some("Quilt".to_string()), None);
    }

    // 3. Check NeoForge (`META-INF/neoforge.mods.toml`)
    if let Ok(mut f) = archive.by_name("META-INF/neoforge.mods.toml") {
        let mut content = String::new();
        if f.read_to_string(&mut content).is_ok() {
            let name = extract_toml_mod_display_name(&content);
            let mc_ver = extract_toml_mod_mc_version(&content);
            return (name, Some("NeoForge".to_string()), mc_ver);
        }
        return (None, Some("NeoForge".to_string()), None);
    }

    // 4. Check Forge (`META-INF/mods.toml`)
    if let Ok(mut f) = archive.by_name("META-INF/mods.toml") {
        let mut content = String::new();
        if f.read_to_string(&mut content).is_ok() {
            let name = extract_toml_mod_display_name(&content);
            let mc_ver = extract_toml_mod_mc_version(&content);
            return (name, Some("Forge".to_string()), mc_ver);
        }
        return (None, Some("Forge".to_string()), None);
    }

    // 5. Check Legacy Forge (`mcmod.info`)
    if let Ok(mut f) = archive.by_name("mcmod.info") {
        let mut content = String::new();
        if f.read_to_string(&mut content).is_ok() {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                let first = json
                    .as_array()
                    .and_then(|a| a.first())
                    .or_else(|| json.get("modList").and_then(|l| l.as_array()).and_then(|a| a.first()));
                let name = first
                    .and_then(|m| m.get("name").or_else(|| m.get("modid")))
                    .and_then(|v| v.as_str())
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty());
                let mc_ver = first
                    .and_then(|m| m.get("mcversion"))
                    .and_then(extract_json_version_req);
                return (name, Some("Forge".to_string()), mc_ver);
            }
        }
        return (None, Some("Forge".to_string()), None);
    }

    (None, None, None)
}

fn extract_json_version_req(val: &serde_json::Value) -> Option<String> {
    if let Some(s) = val.as_str() {
        let trimmed = s.trim();
        if !trimmed.is_empty() && trimmed != "*" && !trimmed.contains("${") {
            return Some(trimmed.to_string());
        }
    } else if let Some(arr) = val.as_array() {
        let parts: Vec<String> = arr
            .iter()
            .filter_map(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty() && s != "*" && !s.contains("${"))
            .collect();
        if !parts.is_empty() {
            return Some(parts.join(" || "));
        }
    }
    None
}

fn extract_toml_mod_display_name(content: &str) -> Option<String> {
    let toml_val = content.parse::<toml::Value>().ok()?;
    let mods_arr = toml_val.get("mods")?.as_array()?;
    let first = mods_arr.first()?;
    first
        .get("displayName")
        .and_then(|v| v.as_str())
        .or_else(|| first.get("modId").and_then(|v| v.as_str()))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && !s.contains("${"))
}

fn extract_toml_mod_mc_version(content: &str) -> Option<String> {
    let toml_val = content.parse::<toml::Value>().ok()?;
    let deps_table = toml_val.get("dependencies")?.as_table()?;
    for (_key, dep_list) in deps_table {
        if let Some(arr) = dep_list.as_array() {
            for dep in arr {
                let mod_id = dep
                    .get("modId")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                if mod_id.eq_ignore_ascii_case("minecraft") {
                    if let Some(range) = dep.get("versionRange").and_then(|v| v.as_str()) {
                        let trimmed = range.trim();
                        if !trimmed.is_empty()
                            && trimmed != "*"
                            && trimmed != "[0,)"
                            && !trimmed.contains("${")
                        {
                            return Some(trimmed.to_string());
                        }
                    }
                }
            }
        }
    }
    None
}

/// Try to parse a Modpack manifest from a zip archive and return its display name if valid.
fn try_read_modpack_name_from_archive(
    archive: &mut zip::ZipArchive<std::fs::File>,
) -> Option<String> {
    // Check Modrinth (`modrinth.index.json`)
    if let Ok(mut f) = archive.by_name("modrinth.index.json") {
        let mut contents = String::new();
        if f.read_to_string(&mut contents).is_ok() {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&contents) {
                if json.get("formatVersion").is_some() || json.get("dependencies").is_some() {
                    let name = json
                        .get("name")
                        .and_then(|n| n.as_str())
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .unwrap_or_else(|| "Modpack".to_string());
                    return Some(name);
                }
            }
        }
    }

    // Check CurseForge / MCBBS (`manifest.json` or `mcbbs.pack.json`)
    for manifest_name in ["manifest.json", "mcbbs.pack.json"] {
        if let Ok(mut f) = archive.by_name(manifest_name) {
            let mut contents = String::new();
            if f.read_to_string(&mut contents).is_ok() {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&contents) {
                    // Verify it is a Minecraft modpack manifest (not a Bedrock pack manifest)
                    if json.get("minecraft").is_some() || json.get("files").is_some() {
                        let name = json
                            .get("name")
                            .and_then(|n| n.as_str())
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .unwrap_or_else(|| "Modpack".to_string());
                        return Some(name);
                    }
                }
            }
        }
    }

    None
}

/// Inspect a `.zip` archive by examining its central directory and key metadata files.
fn inspect_zip_archive(
    path: &Path,
    file_stem: &str,
) -> (DetectedFileKind, Option<String>, Option<String>) {
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return (DetectedFileKind::Unknown, None, None),
    };
    let mut archive = match zip::ZipArchive::new(file) {
        Ok(a) => a,
        Err(_) => return (DetectedFileKind::Unknown, None, None),
    };

    // 1. Check if it is a Modpack
    if let Some(pack_name) = try_read_modpack_name_from_archive(&mut archive) {
        return (DetectedFileKind::Modpack, Some(pack_name), None);
    }

    let mut has_root_level_dat = false;
    let mut nested_world_folder: Option<String> = None;
    let mut has_shaders_dir = false;
    let mut has_pack_mcmeta = false;
    let mut has_assets_dir = false;
    let mut has_data_dir = false;
    let mut has_fabric_mod = false;
    let mut has_forge_mod = false;

    for raw_name in archive.file_names() {
        let name = normalize_entry_name(raw_name);
        if name.is_empty() {
            continue;
        }

        // World detection (`level.dat`)
        if name.eq_ignore_ascii_case("level.dat") {
            has_root_level_dat = true;
        } else if let Some(prefix) = name.strip_suffix("/level.dat") {
            let parts: Vec<&str> = prefix.split('/').filter(|p| !p.is_empty()).collect();
            if parts.len() == 1 {
                nested_world_folder = Some(parts[0].to_string());
            } else if parts.len() == 2 && parts[0].eq_ignore_ascii_case("saves") {
                nested_world_folder = Some(parts[1].to_string());
            }
        }

        // Strip at most one top-level wrapper folder for pack/shader structure checks
        let relative = match name.split_once('/') {
            Some((first, rest))
                if !first.eq_ignore_ascii_case("assets")
                    && !first.eq_ignore_ascii_case("data")
                    && !first.eq_ignore_ascii_case("shaders")
                    && !first.eq_ignore_ascii_case("META-INF") =>
            {
                rest
            }
            _ => name.as_str(),
        };

        // ShaderPack detection (`shaders/` with shader files or properties)
        for candidate in [name.as_str(), relative] {
            if let Some(after_shaders) = candidate.strip_prefix("shaders/") {
                let lower = after_shaders.to_lowercase();
                if lower.ends_with(".vsh")
                    || lower.ends_with(".fsh")
                    || lower.ends_with(".gsh")
                    || lower.ends_with(".csh")
                    || lower.ends_with(".glsl")
                    || lower == "shaders.properties"
                    || lower == "block.properties"
                    || !after_shaders.is_empty()
                {
                    has_shaders_dir = true;
                }
            }
        }

        // ResourcePack / Datapack detection (`pack.mcmeta`, `assets/`, `data/`)
        if name.eq_ignore_ascii_case("pack.mcmeta") || relative.eq_ignore_ascii_case("pack.mcmeta")
        {
            has_pack_mcmeta = true;
        }
        if name.starts_with("assets/") || relative.starts_with("assets/") {
            has_assets_dir = true;
        }
        if name.starts_with("data/") || relative.starts_with("data/") {
            has_data_dir = true;
        }

        // Legacy Mod in `.zip`
        if name == "fabric.mod.json" || name == "quilt.mod.json" {
            has_fabric_mod = true;
        }
        if name == "META-INF/mods.toml"
            || name == "META-INF/neoforge.mods.toml"
            || name == "mcmod.info"
        {
            has_forge_mod = true;
        }
    }

    if has_root_level_dat {
        return (
            DetectedFileKind::World,
            Some(file_stem.to_string()),
            None,
        );
    }
    if let Some(world_folder) = nested_world_folder {
        return (DetectedFileKind::World, Some(world_folder), None);
    }
    if has_shaders_dir && !has_pack_mcmeta {
        return (
            DetectedFileKind::ShaderPack,
            Some(file_stem.to_string()),
            None,
        );
    }
    if has_pack_mcmeta {
        if has_assets_dir {
            return (
                DetectedFileKind::ResourcePack,
                Some(file_stem.to_string()),
                None,
            );
        }
        if has_data_dir {
            return (
                DetectedFileKind::Datapack,
                Some(file_stem.to_string()),
                None,
            );
        }
        // Fallback when only pack.mcmeta is present
        return (
            DetectedFileKind::ResourcePack,
            Some(file_stem.to_string()),
            None,
        );
    }
    if has_shaders_dir {
        return (
            DetectedFileKind::ShaderPack,
            Some(file_stem.to_string()),
            None,
        );
    }
    if has_fabric_mod {
        return (
            DetectedFileKind::Mod,
            Some(file_stem.to_string()),
            Some("Fabric".to_string()),
        );
    }
    if has_forge_mod {
        return (
            DetectedFileKind::Mod,
            Some(file_stem.to_string()),
            Some("Forge".to_string()),
        );
    }

    (DetectedFileKind::Unknown, None, None)
}

/// Synchronously inspect a single file or directory path.
pub fn inspect_single_path_sync(path: &Path) -> InspectedDropFile {
    let path_str = path.to_string_lossy().to_string();
    let filename = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_string();

    let metadata = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(_) => {
            return InspectedDropFile {
                path: path_str,
                filename,
                kind: DetectedFileKind::Unknown,
                display_name: None,
                detected_loader: None,
                detected_mc_version: None,
                size: 0,
                is_dir: false,
            };
        }
    };

    if metadata.is_dir() {
        let kind = if path.join("level.dat").is_file() {
            DetectedFileKind::World
        } else if path.join("shaders").is_dir() {
            DetectedFileKind::ShaderPack
        } else if path.join("pack.mcmeta").is_file() && path.join("assets").is_dir() {
            DetectedFileKind::ResourcePack
        } else if path.join("pack.mcmeta").is_file() && path.join("data").is_dir() {
            DetectedFileKind::Datapack
        } else {
            DetectedFileKind::Unknown
        };

        return InspectedDropFile {
            path: path_str,
            display_name: Some(filename.clone()),
            filename,
            kind,
            detected_loader: None,
            detected_mc_version: None,
            size: 0,
            is_dir: true,
        };
    }

    let size = metadata.len();
    let lower_name = filename.to_lowercase();
    let file_stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(&filename)
        .to_string();

    if lower_name.ends_with(".mrpack") {
        let display_name = std::fs::File::open(path)
            .ok()
            .and_then(|f| zip::ZipArchive::new(f).ok())
            .and_then(|mut a| try_read_modpack_name_from_archive(&mut a))
            .or_else(|| Some(file_stem.clone()));

        return InspectedDropFile {
            path: path_str,
            filename,
            kind: DetectedFileKind::Modpack,
            display_name,
            detected_loader: None,
            detected_mc_version: None,
            size,
            is_dir: false,
        };
    }

    if lower_name.ends_with(".jar")
        || lower_name.ends_with(".jar.disable")
        || lower_name.ends_with(".litemod")
    {
        let (display_name, detected_loader, detected_mc_version) = inspect_jar_archive(path);
        return InspectedDropFile {
            path: path_str,
            filename,
            kind: DetectedFileKind::Mod,
            display_name: display_name.or(Some(file_stem)),
            detected_loader,
            detected_mc_version,
            size,
            is_dir: false,
        };
    }

    if lower_name.ends_with(".zip") {
        let (kind, display_name, detected_loader) = inspect_zip_archive(path, &file_stem);
        return InspectedDropFile {
            path: path_str,
            filename,
            kind,
            display_name,
            detected_loader,
            detected_mc_version: None,
            size,
            is_dir: false,
        };
    }

    InspectedDropFile {
        path: path_str,
        filename,
        kind: DetectedFileKind::Unknown,
        display_name: None,
        detected_loader: None,
        detected_mc_version: None,
        size,
        is_dir: false,
    }
}

/// Inspect a list of dropped file/folder paths and classify their Minecraft asset types.
#[tauri::command]
pub async fn inspect_dropped_files(
    paths: Vec<String>,
) -> Result<Vec<InspectedDropFile>, AppError> {
    let results = tokio::task::spawn_blocking(move || {
        paths
            .into_iter()
            .map(|p| inspect_single_path_sync(Path::new(&p)))
            .collect::<Vec<_>>()
    })
    .await
    .map_err(|e| DawnlandError::ProcessError(format!("Task join error: {}", e)))?;

    Ok(results)
}

/// Copy a directory recursively in a blocking context.
fn copy_dir_recursive_sync(src: &Path, dst: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dst).map_err(|e| format!("Failed to create directory: {}", e))?;
    for entry_res in walkdir::WalkDir::new(src) {
        let entry = entry_res.map_err(|e| format!("WalkDir error: {}", e))?;
        let rel = entry
            .path()
            .strip_prefix(src)
            .map_err(|e| format!("Strip prefix error: {}", e))?;
        if rel.as_os_str().is_empty() {
            continue;
        }
        let target = dst.join(rel);
        if entry.file_type().is_dir() {
            std::fs::create_dir_all(&target)
                .map_err(|e| format!("Failed to create subdirectory: {}", e))?;
        } else if entry.file_type().is_file() {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("Failed to create parent directory: {}", e))?;
            }
            std::fs::copy(entry.path(), &target)
                .map_err(|e| format!("Failed to copy file: {}", e))?;
        }
    }
    Ok(())
}

/// Extract a world `.zip` archive into `saves/<world_folder>/`, automatically handling both
/// root-level `level.dat` and nested `<folder>/level.dat` structures.
fn extract_world_archive_sync(
    zip_path: &Path,
    saves_dir: &Path,
    fallback_stem: &str,
    overwrite: bool,
) -> Result<String, String> {
    let file = std::fs::File::open(zip_path).map_err(|e| format!("Failed to open zip: {}", e))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| format!("Failed to read zip archive: {}", e))?;

    // Determine prefix inside zip where `level.dat` lives
    let mut strip_prefix_str = String::new();
    let mut target_world_name = fallback_stem.to_string();

    for i in 0..archive.len() {
        if let Ok(entry) = archive.by_index(i) {
            let norm = normalize_entry_name(entry.name());
            if norm.eq_ignore_ascii_case("level.dat") {
                strip_prefix_str.clear();
                target_world_name = fallback_stem.to_string();
                break;
            } else if let Some(prefix) = norm.strip_suffix("/level.dat") {
                let parts: Vec<&str> = prefix.split('/').filter(|p| !p.is_empty()).collect();
                if let Some(last) = parts.last() {
                    if is_safe_single_name(last) {
                        target_world_name = (*last).to_string();
                        strip_prefix_str = format!("{}/", prefix);
                        break;
                    }
                }
            }
        }
    }

    if !is_safe_single_name(&target_world_name) {
        return Err("Invalid world folder name".to_string());
    }

    let world_dest_dir = saves_dir.join(&target_world_name);
    if world_dest_dir.exists() && !overwrite {
        return Err("FILE_EXISTS".to_string());
    }

    std::fs::create_dir_all(&world_dest_dir)
        .map_err(|e| format!("Failed to create world directory: {}", e))?;

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("Failed to read zip entry: {}", e))?;
        let enclosed = match entry.enclosed_name() {
            Some(p) => p.to_path_buf(),
            None => continue,
        };
        let norm = normalize_entry_name(&enclosed.to_string_lossy());
        let rel_str = if !strip_prefix_str.is_empty() {
            match norm.strip_prefix(&strip_prefix_str) {
                Some(r) => r,
                None => continue,
            }
        } else {
            norm.as_str()
        };

        if rel_str.is_empty() {
            continue;
        }

        let outpath = world_dest_dir.join(rel_str);
        if entry.name().ends_with('/') {
            std::fs::create_dir_all(&outpath)
                .map_err(|e| format!("Failed to create directory: {}", e))?;
        } else {
            if let Some(parent) = outpath.parent() {
                if !parent.exists() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| format!("Failed to create parent directory: {}", e))?;
                }
            }
            let mut outfile = std::fs::File::create(&outpath)
                .map_err(|e| format!("Failed to create file: {}", e))?;
            std::io::copy(&mut entry, &mut outfile)
                .map_err(|e| format!("Failed to extract file: {}", e))?;
        }
    }

    Ok(target_world_name)
}

/// Check if two paths refer to the same underlying file on disk.
fn is_same_file_path(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(ca), Ok(cb)) => ca == cb,
        _ => false,
    }
}

/// Import a batch of dropped files/directories into a target instance.
#[tauri::command]
pub async fn import_dropped_files_to_instance(
    instance_id: String,
    items: Vec<DropImportItem>,
    target_world: Option<String>,
    overwrite: bool,
    task_manager: tauri::State<'_, crate::core::task::TaskManager>,
) -> Result<DropImportReport, AppError> {
    if !is_safe_single_name(&instance_id) {
        return Err(DawnlandError::Unknown("Invalid instance ID".to_string()).into());
    }

    let base_dir = get_minecraft_base();
    let instance_dir = base_dir.join("versions").join(&instance_id);
    if !instance_dir.exists() {
        return Err(DawnlandError::InstanceNotFound(format!(
            "Instance '{}' not found",
            instance_id
        ))
        .into());
    }

    // Verify instance is not currently locked by installation or update
    let config_path = instance_dir.join("dlml.json");
    if config_path.exists() {
        if let Ok(content) = tokio::fs::read_to_string(&config_path).await {
            if let Ok(cfg) =
                serde_json::from_str::<crate::core::launcher::InstanceConfig>(&content)
            {
                if cfg.is_installing || cfg.is_updating {
                    return Err(DawnlandError::Unknown(
                        "Instance is currently installing or updating".to_string(),
                    )
                    .into());
                }
            }
        }
    }

    let tasks = task_manager.load_history().await.unwrap_or_default();
    for t in &tasks {
        if t.task_type.instance_id().as_deref() == Some(instance_id.as_str())
            && matches!(
                t.status,
                crate::core::task::TaskStatus::Pending
                    | crate::core::task::TaskStatus::Running
                    | crate::core::task::TaskStatus::Paused
            )
        {
            return Err(DawnlandError::Unknown(
                "Instance has an active background task".to_string(),
            )
            .into());
        }
    }

    let mut report = DropImportReport::default();

    for item in items {
        let src_path = PathBuf::from(&item.path);
        let inspected = {
            let p = src_path.clone();
            tokio::task::spawn_blocking(move || inspect_single_path_sync(&p))
                .await
                .map_err(|e| DawnlandError::ProcessError(format!("Task join error: {}", e)))?
        };

        let filename = inspected.filename.clone();
        if !is_safe_single_name(&filename) {
            report.failed.push(FailedFileResult {
                filename,
                error: "Invalid filename".to_string(),
            });
            continue;
        }

        if !src_path.exists() {
            report.failed.push(FailedFileResult {
                filename,
                error: "Source file does not exist".to_string(),
            });
            continue;
        }

        let effective_kind = item
            .kind_override
            .filter(|k| *k != DetectedFileKind::Unknown)
            .unwrap_or(inspected.kind);

        match effective_kind {
            DetectedFileKind::Mod => {
                if inspected.is_dir {
                    report.failed.push(FailedFileResult {
                        filename,
                        error: "Mod cannot be a directory".to_string(),
                    });
                    continue;
                }

                let mods_dir = instance_dir.join("mods");
                if let Err(e) = tokio::fs::create_dir_all(&mods_dir).await {
                    report.failed.push(FailedFileResult {
                        filename,
                        error: format!("Failed to create mods directory: {}", e),
                    });
                    continue;
                }

                let target_path = mods_dir.join(&filename);
                if is_same_file_path(&src_path, &target_path) {
                    let rel_key = format!("mods/{}", filename);
                    report.succeeded.push(ImportedFileResult {
                        filename,
                        kind: DetectedFileKind::Mod,
                        target_rel_path: rel_key,
                    });
                    continue;
                }

                if target_path.exists() && !overwrite {
                    report.skipped.push(SkippedFileResult {
                        filename,
                        kind: DetectedFileKind::Mod,
                        reason: "FILE_EXISTS".to_string(),
                    });
                    continue;
                }

                // Clean up conflicting enabled/disabled counterpart when overwriting
                if overwrite {
                    let counterpart = if let Some(base) = filename.strip_suffix(".disable") {
                        base.to_string()
                    } else {
                        format!("{}.disable", filename)
                    };
                    let counterpart_path = mods_dir.join(&counterpart);
                    if counterpart_path.exists() {
                        let _ = tokio::fs::remove_file(&counterpart_path).await;
                        delete_asset_from_manifest(&instance_id, &format!("mods/{}", counterpart))
                            .await;
                    }
                }

                if let Err(e) = tokio::fs::copy(&src_path, &target_path).await {
                    report.failed.push(FailedFileResult {
                        filename,
                        error: format!("Failed to copy mod file: {}", e),
                    });
                    continue;
                }

                let hash_sha1 = compute_sha1(&target_path).await.ok();
                let size = tokio::fs::metadata(&target_path)
                    .await
                    .map(|m| m.len())
                    .ok();
                let rel_key = format!("mods/{}", filename);

                save_asset_to_manifest(
                    &instance_id,
                    rel_key.clone(),
                    AssetRecord {
                        category: AssetCategory::Mod,
                        source_type: AssetSourceType::LocalImport,
                        project_id: None,
                        version_id: None,
                        download_url: None,
                        hash_sha1,
                        hash_sha512: None,
                        size,
                        enabled: !filename.ends_with(".disable"),
                        managed_by_modpack: false,
                    },
                )
                .await;

                tracing::info!("Drag-and-drop imported mod '{}' into '{}'", filename, instance_id);
                report.succeeded.push(ImportedFileResult {
                    filename,
                    kind: DetectedFileKind::Mod,
                    target_rel_path: rel_key,
                });
            }

            DetectedFileKind::ResourcePack | DetectedFileKind::ShaderPack => {
                let (folder_name, category) =
                    if effective_kind == DetectedFileKind::ResourcePack {
                        ("resourcepacks", AssetCategory::ResourcePack)
                    } else {
                        ("shaderpacks", AssetCategory::ShaderPack)
                    };

                let target_dir = instance_dir.join(folder_name);
                if let Err(e) = tokio::fs::create_dir_all(&target_dir).await {
                    report.failed.push(FailedFileResult {
                        filename,
                        error: format!("Failed to create {} directory: {}", folder_name, e),
                    });
                    continue;
                }

                let target_path = target_dir.join(&filename);
                if is_same_file_path(&src_path, &target_path) {
                    let rel_key = format!("{}/{}", folder_name, filename);
                    report.succeeded.push(ImportedFileResult {
                        filename,
                        kind: effective_kind,
                        target_rel_path: rel_key,
                    });
                    continue;
                }

                if target_path.exists() && !overwrite {
                    report.skipped.push(SkippedFileResult {
                        filename,
                        kind: effective_kind,
                        reason: "FILE_EXISTS".to_string(),
                    });
                    continue;
                }

                if inspected.is_dir {
                    let s = src_path.clone();
                    let d = target_path.clone();
                    let copy_res =
                        tokio::task::spawn_blocking(move || copy_dir_recursive_sync(&s, &d))
                            .await
                            .map_err(|e| format!("Task join error: {}", e))
                            .and_then(|r| r);
                    if let Err(e) = copy_res {
                        report.failed.push(FailedFileResult {
                            filename,
                            error: e,
                        });
                        continue;
                    }
                } else if let Err(e) = tokio::fs::copy(&src_path, &target_path).await {
                    report.failed.push(FailedFileResult {
                        filename,
                        error: format!("Failed to copy file: {}", e),
                    });
                    continue;
                }

                let hash_sha1 = if !inspected.is_dir {
                    compute_sha1(&target_path).await.ok()
                } else {
                    None
                };
                let size = if !inspected.is_dir {
                    tokio::fs::metadata(&target_path)
                        .await
                        .map(|m| m.len())
                        .ok()
                } else {
                    Some(0)
                };
                let rel_key = format!("{}/{}", folder_name, filename);

                save_asset_to_manifest(
                    &instance_id,
                    rel_key.clone(),
                    AssetRecord {
                        category,
                        source_type: AssetSourceType::LocalImport,
                        project_id: None,
                        version_id: None,
                        download_url: None,
                        hash_sha1,
                        hash_sha512: None,
                        size,
                        enabled: true,
                        managed_by_modpack: false,
                    },
                )
                .await;

                tracing::info!(
                    "Drag-and-drop imported {:?} '{}' into '{}'",
                    effective_kind,
                    filename,
                    instance_id
                );
                report.succeeded.push(ImportedFileResult {
                    filename,
                    kind: effective_kind,
                    target_rel_path: rel_key,
                });
            }

            DetectedFileKind::Datapack => {
                // Determine target world
                let resolved_world = match target_world
                    .as_ref()
                    .map(|w| w.trim().to_string())
                    .filter(|w| !w.is_empty())
                {
                    Some(w) => {
                        if !is_safe_single_name(&w) {
                            report.failed.push(FailedFileResult {
                                filename,
                                error: "Invalid target world name".to_string(),
                            });
                            continue;
                        }
                        w
                    }
                    None => {
                        let saves = get_instance_saves(instance_id.clone())
                            .await
                            .unwrap_or_default();
                        if saves.is_empty() {
                            report.skipped.push(SkippedFileResult {
                                filename,
                                kind: DetectedFileKind::Datapack,
                                reason: "NO_WORLD_FOUND".to_string(),
                            });
                            continue;
                        } else if saves.len() == 1 {
                            saves[0].clone()
                        } else {
                            report.skipped.push(SkippedFileResult {
                                filename,
                                kind: DetectedFileKind::Datapack,
                                reason: "WORLD_SELECTION_REQUIRED".to_string(),
                            });
                            continue;
                        }
                    }
                };

                let datapacks_dir = instance_dir
                    .join("saves")
                    .join(&resolved_world)
                    .join("datapacks");
                if let Err(e) = tokio::fs::create_dir_all(&datapacks_dir).await {
                    report.failed.push(FailedFileResult {
                        filename,
                        error: format!("Failed to create datapacks directory: {}", e),
                    });
                    continue;
                }

                let target_path = datapacks_dir.join(&filename);
                if is_same_file_path(&src_path, &target_path) {
                    let rel_key = format!("saves/{}/datapacks/{}", resolved_world, filename);
                    report.succeeded.push(ImportedFileResult {
                        filename,
                        kind: DetectedFileKind::Datapack,
                        target_rel_path: rel_key,
                    });
                    continue;
                }

                if target_path.exists() && !overwrite {
                    report.skipped.push(SkippedFileResult {
                        filename,
                        kind: DetectedFileKind::Datapack,
                        reason: "FILE_EXISTS".to_string(),
                    });
                    continue;
                }

                if inspected.is_dir {
                    let s = src_path.clone();
                    let d = target_path.clone();
                    let copy_res =
                        tokio::task::spawn_blocking(move || copy_dir_recursive_sync(&s, &d))
                            .await
                            .map_err(|e| format!("Task join error: {}", e))
                            .and_then(|r| r);
                    if let Err(e) = copy_res {
                        report.failed.push(FailedFileResult {
                            filename,
                            error: e,
                        });
                        continue;
                    }
                } else if let Err(e) = tokio::fs::copy(&src_path, &target_path).await {
                    report.failed.push(FailedFileResult {
                        filename,
                        error: format!("Failed to copy datapack file: {}", e),
                    });
                    continue;
                }

                let hash_sha1 = if !inspected.is_dir {
                    compute_sha1(&target_path).await.ok()
                } else {
                    None
                };
                let size = if !inspected.is_dir {
                    tokio::fs::metadata(&target_path)
                        .await
                        .map(|m| m.len())
                        .ok()
                } else {
                    Some(0)
                };
                let rel_key = format!("saves/{}/datapacks/{}", resolved_world, filename);

                save_asset_to_manifest(
                    &instance_id,
                    rel_key.clone(),
                    AssetRecord {
                        category: AssetCategory::Datapack,
                        source_type: AssetSourceType::LocalImport,
                        project_id: None,
                        version_id: None,
                        download_url: None,
                        hash_sha1,
                        hash_sha512: None,
                        size,
                        enabled: true,
                        managed_by_modpack: false,
                    },
                )
                .await;

                tracing::info!(
                    "Drag-and-drop imported datapack '{}' into '{}/{}'",
                    filename,
                    instance_id,
                    resolved_world
                );
                report.succeeded.push(ImportedFileResult {
                    filename,
                    kind: DetectedFileKind::Datapack,
                    target_rel_path: rel_key,
                });
            }

            DetectedFileKind::World => {
                let saves_dir = instance_dir.join("saves");
                if let Err(e) = tokio::fs::create_dir_all(&saves_dir).await {
                    report.failed.push(FailedFileResult {
                        filename,
                        error: format!("Failed to create saves directory: {}", e),
                    });
                    continue;
                }

                if inspected.is_dir {
                    let target_path = saves_dir.join(&filename);
                    if is_same_file_path(&src_path, &target_path) {
                        let rel_key = format!("saves/{}", filename);
                        report.succeeded.push(ImportedFileResult {
                            filename,
                            kind: DetectedFileKind::World,
                            target_rel_path: rel_key,
                        });
                        continue;
                    }
                    if target_path.exists() && !overwrite {
                        report.skipped.push(SkippedFileResult {
                            filename,
                            kind: DetectedFileKind::World,
                            reason: "FILE_EXISTS".to_string(),
                        });
                        continue;
                    }
                    let s = src_path.clone();
                    let d = target_path.clone();
                    let copy_res =
                        tokio::task::spawn_blocking(move || copy_dir_recursive_sync(&s, &d))
                            .await
                            .map_err(|e| format!("Task join error: {}", e))
                            .and_then(|r| r);
                    if let Err(e) = copy_res {
                        report.failed.push(FailedFileResult {
                            filename,
                            error: e,
                        });
                        continue;
                    }

                    let rel_key = format!("saves/{}", filename);
                    save_asset_to_manifest(
                        &instance_id,
                        rel_key.clone(),
                        AssetRecord {
                            category: AssetCategory::Save,
                            source_type: AssetSourceType::LocalImport,
                            project_id: None,
                            version_id: None,
                            download_url: None,
                            hash_sha1: None,
                            hash_sha512: None,
                            size: Some(0),
                            enabled: true,
                            managed_by_modpack: false,
                        },
                    )
                    .await;

                    report.succeeded.push(ImportedFileResult {
                        filename,
                        kind: DetectedFileKind::World,
                        target_rel_path: rel_key,
                    });
                } else {
                    let stem = src_path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("World")
                        .to_string();
                    let zp = src_path.clone();
                    let sd = saves_dir.clone();
                    let extract_res = tokio::task::spawn_blocking(move || {
                        extract_world_archive_sync(&zp, &sd, &stem, overwrite)
                    })
                    .await
                    .map_err(|e| format!("Task join error: {}", e))
                    .and_then(|r| r);

                    match extract_res {
                        Ok(world_folder) => {
                            let rel_key = format!("saves/{}", world_folder);
                            save_asset_to_manifest(
                                &instance_id,
                                rel_key.clone(),
                                AssetRecord {
                                    category: AssetCategory::Save,
                                    source_type: AssetSourceType::LocalImport,
                                    project_id: None,
                                    version_id: None,
                                    download_url: None,
                                    hash_sha1: None,
                                    hash_sha512: None,
                                    size: Some(0),
                                    enabled: true,
                                    managed_by_modpack: false,
                                },
                            )
                            .await;

                            tracing::info!(
                                "Drag-and-drop extracted world '{}' into '{}/saves/{}'",
                                filename,
                                instance_id,
                                world_folder
                            );
                            report.succeeded.push(ImportedFileResult {
                                filename,
                                kind: DetectedFileKind::World,
                                target_rel_path: rel_key,
                            });
                        }
                        Err(reason) if reason == "FILE_EXISTS" => {
                            report.skipped.push(SkippedFileResult {
                                filename,
                                kind: DetectedFileKind::World,
                                reason,
                            });
                        }
                        Err(e) => {
                            report.failed.push(FailedFileResult {
                                filename,
                                error: e,
                            });
                        }
                    }
                }
            }

            DetectedFileKind::Modpack => {
                report.skipped.push(SkippedFileResult {
                    filename,
                    kind: DetectedFileKind::Modpack,
                    reason: "MODPACK_REQUIRES_INSTALL_FLOW".to_string(),
                });
            }

            DetectedFileKind::Unknown => {
                report.skipped.push(SkippedFileResult {
                    filename,
                    kind: DetectedFileKind::Unknown,
                    reason: "UNSUPPORTED_FILE_TYPE".to_string(),
                });
            }
        }
    }

    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    fn create_test_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let file = std::fs::File::create(path).expect("create zip");
        let mut zip = zip::ZipWriter::new(file);
        let options = SimpleFileOptions::default();
        for (name, content) in entries {
            zip.start_file(*name, options).expect("start file");
            zip.write_all(content).expect("write content");
        }
        zip.finish().expect("finish zip");
    }

    #[test]
    fn test_inspect_mod_jar_fabric_and_forge() {
        let temp = std::env::temp_dir().join(format!("dlml_drop_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp).expect("mkdir");

        let fabric_jar = temp.join("sodium-fabric.jar");
        create_test_zip(
            &fabric_jar,
            &[(
                "fabric.mod.json",
                br#"{"schemaVersion": 1, "id": "sodium", "name": "Sodium"}"#,
            )],
        );
        let res = inspect_single_path_sync(&fabric_jar);
        assert_eq!(res.kind, DetectedFileKind::Mod);
        assert_eq!(res.display_name.as_deref(), Some("Sodium"));
        assert_eq!(res.detected_loader.as_deref(), Some("Fabric"));

        let forge_jar = temp.join("jei-forge.jar");
        create_test_zip(
            &forge_jar,
            &[(
                "META-INF/mods.toml",
                br#"[[mods]]
modId="jei"
displayName="Just Enough Items"
"#,
            )],
        );
        let res_forge = inspect_single_path_sync(&forge_jar);
        assert_eq!(res_forge.kind, DetectedFileKind::Mod);
        assert_eq!(res_forge.display_name.as_deref(), Some("Just Enough Items"));
        assert_eq!(res_forge.detected_loader.as_deref(), Some("Forge"));

        let _ = std::fs::remove_dir_all(&temp);
    }

    #[test]
    fn test_inspect_modpack_resourcepack_datapack_shader_world() {
        let temp = std::env::temp_dir().join(format!("dlml_drop_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp).expect("mkdir");

        // 1. CurseForge Modpack .zip
        let cf_pack = temp.join("dawncraft.zip");
        create_test_zip(
            &cf_pack,
            &[(
                "manifest.json",
                br#"{"minecraft": {"version": "1.18.2"}, "name": "DawnCraft", "version": "1.0"}"#,
            )],
        );
        let res_cf = inspect_single_path_sync(&cf_pack);
        assert_eq!(res_cf.kind, DetectedFileKind::Modpack);
        assert_eq!(res_cf.display_name.as_deref(), Some("DawnCraft"));

        // 2. ResourcePack .zip
        let rp = temp.join("faithful.zip");
        create_test_zip(
            &rp,
            &[
                ("pack.mcmeta", br#"{"pack":{"pack_format":15}}"#),
                ("assets/minecraft/textures/block/stone.png", b"png"),
            ],
        );
        let res_rp = inspect_single_path_sync(&rp);
        assert_eq!(res_rp.kind, DetectedFileKind::ResourcePack);

        // 3. Datapack .zip
        let dp = temp.join("terralith.zip");
        create_test_zip(
            &dp,
            &[
                ("pack.mcmeta", br#"{"pack":{"pack_format":15}}"#),
                ("data/terralith/worldgen/biome/alpha.json", b"{}"),
            ],
        );
        let res_dp = inspect_single_path_sync(&dp);
        assert_eq!(res_dp.kind, DetectedFileKind::Datapack);

        // 4. ShaderPack .zip
        let sp = temp.join("complementary.zip");
        create_test_zip(
            &sp,
            &[
                ("shaders/shaders.properties", b"profile.low=false"),
                ("shaders/gbuffers_terrain.vsh", b"void main() {}"),
            ],
        );
        let res_sp = inspect_single_path_sync(&sp);
        assert_eq!(res_sp.kind, DetectedFileKind::ShaderPack);

        // 5. World .zip (nested folder) with extraction verification
        let world_zip = temp.join("skyblock_archive.zip");
        create_test_zip(
            &world_zip,
            &[
                ("SkyBlockWorld/level.dat", b"nbt"),
                ("SkyBlockWorld/region/r.0.0.mca", b"mca"),
            ],
        );
        let res_world = inspect_single_path_sync(&world_zip);
        assert_eq!(res_world.kind, DetectedFileKind::World);
        assert_eq!(res_world.display_name.as_deref(), Some("SkyBlockWorld"));

        let saves_dir = temp.join("saves");
        let extracted_name =
            extract_world_archive_sync(&world_zip, &saves_dir, "skyblock_archive", true)
                .expect("extract world");
        assert_eq!(extracted_name, "SkyBlockWorld");
        assert!(saves_dir.join("SkyBlockWorld").join("level.dat").exists());
        assert!(saves_dir
            .join("SkyBlockWorld")
            .join("region")
            .join("r.0.0.mca")
            .exists());

        let _ = std::fs::remove_dir_all(&temp);
    }
}
