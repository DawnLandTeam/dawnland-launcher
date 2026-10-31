use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex};
use tokio::sync::Mutex as TokioMutex;
use std::io::Read;

use crate::core::mojang::get_minecraft_base;
use crate::core::manager::get_instance_saves;
use crate::error::{AppError, DawnlandError};
use crate::models::instance::{AssetCategory, AssetRecord, AssetSourceType};

static INSTANCE_LOCKS: std::sync::LazyLock<StdMutex<HashMap<String, Arc<TokioMutex<()>>>>> =
    std::sync::LazyLock::new(|| StdMutex::new(HashMap::new()));

/// Get or create an asynchronous per-instance lock to serialize tasks and imports.
pub fn get_instance_lock(instance_id: &str) -> Arc<TokioMutex<()>> {
    let mut map = INSTANCE_LOCKS.lock().unwrap();
    map.entry(instance_id.to_string())
        .or_insert_with(|| Arc::new(TokioMutex::new(())))
        .clone()
}

/// Limits for resource exhaustion protection during decompression
const MAX_METADATA_ENTRY_BYTES: u64 = 2 * 1024 * 1024; // 2 MB limit for json/toml/mcmeta
const MAX_EXTRACTED_FILE_BYTES: u64 = 5 * 1024 * 1024 * 1024; // 5 GB per world file
const MAX_AGGREGATE_EXTRACTED_BYTES: u64 = 10 * 1024 * 1024 * 1024; // 10 GB total

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetectedFileKind {
    Mod,
    ResourcePack,
    ShaderPack,
    Datapack,
    World,
    Modpack,
    Unknown,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
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

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DropImportItem {
    pub path: String,
    pub kind_override: Option<DetectedFileKind>,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ImportedFileResult {
    pub filename: String,
    pub kind: DetectedFileKind,
    pub target_rel_path: String,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SkippedFileResult {
    pub filename: String,
    pub kind: DetectedFileKind,
    pub reason: String,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FailedFileResult {
    pub filename: String,
    pub error: String,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct DropImportReport {
    pub succeeded: Vec<ImportedFileResult>,
    pub skipped: Vec<SkippedFileResult>,
    pub failed: Vec<FailedFileResult>,
}

/// Validate that a filename or world folder name is a single safe path component.
pub fn is_safe_single_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains('/')
        && !name.contains('\\')
        && !name.contains('\0')
        && !name.contains("..")
}

/// Normalize a zip entry path to forward slashes without leading slash.
fn normalize_entry_name(raw: &str) -> String {
    raw.replace('\\', "/").trim_start_matches('/').to_string()
}

/// Sanitize a relative zip entry path to ensure it cannot escape a destination folder.
fn sanitize_zip_entry_rel_path(raw: &str) -> Option<String> {
    if raw.contains('\0') {
        return None;
    }
    let normalized = raw.replace('\\', "/");
    let trimmed = normalized.trim_start_matches('/');
    if trimmed.is_empty() {
        return None;
    }
    for seg in trimmed.split('/') {
        if seg == ".." {
            return None;
        }
    }
    Some(trimmed.to_string())
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
        if f.by_ref().take(MAX_METADATA_ENTRY_BYTES).read_to_string(&mut content).is_ok() {
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
        if f.by_ref().take(MAX_METADATA_ENTRY_BYTES).read_to_string(&mut content).is_ok() {
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
        if f.by_ref().take(MAX_METADATA_ENTRY_BYTES).read_to_string(&mut content).is_ok() {
            if let Ok(toml_val) = toml::from_str::<toml::Value>(&content) {
                let name = extract_forge_display_name(&toml_val);
                let mc_ver = extract_forge_mc_version(&toml_val);
                return (name, Some("NeoForge".to_string()), mc_ver);
            }
        }
        return (None, Some("NeoForge".to_string()), None);
    }

    // 4. Check Forge (`META-INF/mods.toml`)
    if let Ok(mut f) = archive.by_name("META-INF/mods.toml") {
        let mut content = String::new();
        if f.by_ref().take(MAX_METADATA_ENTRY_BYTES).read_to_string(&mut content).is_ok() {
            if let Ok(toml_val) = toml::from_str::<toml::Value>(&content) {
                let name = extract_forge_display_name(&toml_val);
                let mc_ver = extract_forge_mc_version(&toml_val);
                return (name, Some("Forge".to_string()), mc_ver);
            }
        }
        return (None, Some("Forge".to_string()), None);
    }

    // 5. Check Legacy Forge (`mcmod.info`)
    if let Ok(mut f) = archive.by_name("mcmod.info") {
        let mut content = String::new();
        if f.by_ref().take(MAX_METADATA_ENTRY_BYTES).read_to_string(&mut content).is_ok() {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                let entry = json
                    .as_array()
                    .and_then(|arr| arr.first())
                    .or_else(|| {
                        json.get("modList")
                            .and_then(|m| m.as_array())
                            .and_then(|arr| arr.first())
                    });
                if let Some(obj) = entry {
                    let name = obj
                        .get("name")
                        .and_then(|v| v.as_str())
                        .or_else(|| obj.get("modid").and_then(|v| v.as_str()))
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty());
                    let mc_ver = obj
                        .get("mcversion")
                        .and_then(|v| v.as_str())
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty());
                    return (name, Some("Forge".to_string()), mc_ver);
                }
            }
        }
        return (None, Some("Forge".to_string()), None);
    }

    (None, None, None)
}

fn extract_json_version_req(val: &serde_json::Value) -> Option<String> {
    if let Some(s) = val.as_str() {
        let trimmed = s.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    } else if let Some(arr) = val.as_array() {
        let parts: Vec<String> = arr
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.trim().to_string()))
            .filter(|s| !s.is_empty())
            .collect();
        if !parts.is_empty() {
            return Some(parts.join(" || "));
        }
    }
    None
}

fn extract_forge_display_name(toml_val: &toml::Value) -> Option<String> {
    toml_val
        .get("mods")
        .and_then(|m| m.as_array())
        .and_then(|arr| arr.first())
        .and_then(|entry| {
            entry
                .get("displayName")
                .and_then(|v| v.as_str())
                .or_else(|| entry.get("modId").and_then(|v| v.as_str()))
        })
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn extract_forge_mc_version(toml_val: &toml::Value) -> Option<String> {
    if let Some(deps_table) = toml_val.get("dependencies").and_then(|d| d.as_table()) {
        for (_mod_id, dep_val) in deps_table {
            if let Some(dep_arr) = dep_val.as_array() {
                for dep in dep_arr {
                    let is_mc = dep
                        .get("modId")
                        .and_then(|v| v.as_str())
                        .map(|id| id.eq_ignore_ascii_case("minecraft"))
                        .unwrap_or(false);
                    if is_mc {
                        if let Some(ver) = dep.get("versionRange").and_then(|v| v.as_str()) {
                            let trimmed = ver.trim();
                            if !trimmed.is_empty() {
                                return Some(trimmed.to_string());
                            }
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
        if f.by_ref().take(MAX_METADATA_ENTRY_BYTES).read_to_string(&mut contents).is_ok() {
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
            if f.by_ref().take(MAX_METADATA_ENTRY_BYTES).read_to_string(&mut contents).is_ok() {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&contents) {
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
            }
        }
    };

    if metadata.is_dir() {
        let has_level_dat = path.join("level.dat").exists();
        let has_pack_mcmeta = path.join("pack.mcmeta").exists();
        let has_shaders_dir = path.join("shaders").is_dir();
        let has_assets_dir = path.join("assets").is_dir();
        let has_data_dir = path.join("data").is_dir();

        let kind = if has_level_dat {
            DetectedFileKind::World
        } else if has_shaders_dir && !has_pack_mcmeta {
            DetectedFileKind::ShaderPack
        } else if has_pack_mcmeta && has_data_dir && !has_assets_dir {
            DetectedFileKind::Datapack
        } else if has_pack_mcmeta {
            DetectedFileKind::ResourcePack
        } else if has_shaders_dir {
            DetectedFileKind::ShaderPack
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

/// Copy a directory recursively into a temporary destination and atomically commit it.
/// Returns Ok(Some(backup_path)) if an existing directory was backed up, or Ok(None) if new.
/// The caller is responsible for deleting the backup after downstream manifest saves succeed,
/// or restoring the backup if downstream operations fail.
fn copy_dir_recursive_sync(src: &Path, dst: &Path) -> Result<Option<PathBuf>, String> {
    let parent = dst
        .parent()
        .ok_or_else(|| "Target path has no parent directory".to_string())?;
    std::fs::create_dir_all(parent)
        .map_err(|e| format!("Failed to create parent directory: {}", e))?;

    let file_stem = dst
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("dir");
    let temp_dir = parent.join(format!(
        ".tmp_copy_{}_{}",
        file_stem,
        uuid::Uuid::new_v4().simple()
    ));

    if temp_dir.exists() {
        let _ = std::fs::remove_dir_all(&temp_dir);
    }
    std::fs::create_dir_all(&temp_dir)
        .map_err(|e| format!("Failed to create temp directory: {}", e))?;

    let copy_res = (|| -> Result<(), String> {
        for entry_res in walkdir::WalkDir::new(src) {
            let entry = entry_res.map_err(|e| format!("WalkDir error: {}", e))?;
            let rel = entry
                .path()
                .strip_prefix(src)
                .map_err(|e| format!("Strip prefix error: {}", e))?;
            if rel.as_os_str().is_empty() {
                continue;
            }
            let target = temp_dir.join(rel);
            if !target.starts_with(&temp_dir) {
                return Err("Directory traversal detected in source".to_string());
            }
            if entry.file_type().is_dir() {
                std::fs::create_dir_all(&target)
                    .map_err(|e| format!("Failed to create subdirectory: {}", e))?;
            } else if entry.file_type().is_file() {
                if let Some(p) = target.parent() {
                    if !p.exists() {
                        std::fs::create_dir_all(p)
                            .map_err(|e| format!("Failed to create parent directory: {}", e))?;
                    }
                }
                std::fs::copy(entry.path(), &target)
                    .map_err(|e| format!("Failed to copy file: {}", e))?;
            }
        }
        Ok(())
    })();

    if let Err(e) = copy_res {
        let _ = std::fs::remove_dir_all(&temp_dir);
        return Err(e);
    }

    let backup_dir = if dst.exists() {
        let backup_name = format!(
            ".backup_{}_{}",
            uuid::Uuid::new_v4().simple(),
            dst.file_name().unwrap_or_default().to_string_lossy()
        );
        let b = parent.join(backup_name);
        std::fs::rename(dst, &b)
            .map_err(|e| format!("Failed to backup existing directory before overwrite: {}", e))?;
        Some(b)
    } else {
        None
    };

    if let Err(e) = std::fs::rename(&temp_dir, dst) {
        let _ = std::fs::remove_dir_all(&temp_dir);
        if let Some(ref b) = backup_dir {
            let _ = std::fs::rename(b, dst);
        }
        return Err(format!("Failed to commit directory: {}", e));
    }

    Ok(backup_dir)
}

/// Extract a world `.zip` archive into `saves/<world_folder>/`, handling both root-level
/// and wrapped `<folder>/level.dat` structures safely.
fn extract_world_archive_sync(
    zip_path: &Path,
    saves_dir: &Path,
    fallback_stem: &str,
    overwrite: bool,
) -> Result<(String, Option<PathBuf>), String> {
    let file = std::fs::File::open(zip_path).map_err(|e| format!("Failed to open zip: {}", e))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| format!("Failed to read zip archive: {}", e))?;

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

    let temp_world_dir = saves_dir.join(format!(
        ".tmp_world_{}_{}",
        target_world_name,
        uuid::Uuid::new_v4().simple()
    ));

    if temp_world_dir.exists() {
        let _ = std::fs::remove_dir_all(&temp_world_dir);
    }
    std::fs::create_dir_all(&temp_world_dir)
        .map_err(|e| format!("Failed to create temp world directory: {}", e))?;

    let mut total_extracted_bytes: u64 = 0;

    let extract_res = (|| -> Result<(), String> {
        for i in 0..archive.len() {
            let mut entry = archive
                .by_index(i)
                .map_err(|e| format!("Failed to read zip entry: {}", e))?;

            let norm = match sanitize_zip_entry_rel_path(entry.name()) {
                Some(p) => p,
                None => continue,
            };

            let rel_str = if !strip_prefix_str.is_empty() {
                match norm.strip_prefix(&strip_prefix_str) {
                    Some(r) => r,
                    None => continue,
                }
            } else {
                norm.as_str()
            };

            if rel_str.is_empty() || rel_str.contains("..") {
                continue;
            }

            let outpath = temp_world_dir.join(rel_str);
            if !outpath.starts_with(&temp_world_dir) {
                continue;
            }

            if entry.name().ends_with('/') || entry.is_dir() {
                std::fs::create_dir_all(&outpath)
                    .map_err(|e| format!("Failed to create directory: {}", e))?;
            } else {
                if let Some(parent) = outpath.parent() {
                    if !parent.exists() {
                        std::fs::create_dir_all(parent)
                            .map_err(|e| format!("Failed to create parent directory: {}", e))?;
                    }
                }
                let declared_size = entry.size();
                if declared_size > MAX_EXTRACTED_FILE_BYTES {
                    return Err(format!(
                        "World archive entry '{}' declared size exceeds limit ({} > {} bytes)",
                        entry.name(),
                        declared_size,
                        MAX_EXTRACTED_FILE_BYTES
                    ));
                }
                let mut outfile = std::fs::File::create(&outpath)
                    .map_err(|e| format!("Failed to create file: {}", e))?;
                let mut limited = (&mut entry).take(MAX_EXTRACTED_FILE_BYTES + 1);
                let copy_res = std::io::copy(&mut limited, &mut outfile);
                match copy_res {
                    Ok(copied) => {
                        if copied > MAX_EXTRACTED_FILE_BYTES {
                            drop(outfile);
                            let _ = std::fs::remove_file(&outpath);
                            return Err(format!(
                                "World archive entry '{}' decompressed size exceeded limit",
                                entry.name()
                            ));
                        }
                        total_extracted_bytes += copied;
                        if total_extracted_bytes > MAX_AGGREGATE_EXTRACTED_BYTES {
                            drop(outfile);
                            let _ = std::fs::remove_file(&outpath);
                            return Err("Archive exceeded aggregate decompressed size limit".to_string());
                        }
                    }
                    Err(e) => {
                        drop(outfile);
                        let _ = std::fs::remove_file(&outpath);
                        return Err(format!("Failed to extract file: {}", e));
                    }
                }
            }
        }
        Ok(())
    })();

    if let Err(e) = extract_res {
        let _ = std::fs::remove_dir_all(&temp_world_dir);
        return Err(e);
    }

    let backup_dir = if world_dest_dir.exists() {
        let backup_name = format!(
            ".backup_{}_{}",
            uuid::Uuid::new_v4().simple(),
            world_dest_dir.file_name().unwrap_or_default().to_string_lossy()
        );
        let b = saves_dir.join(backup_name);
        std::fs::rename(&world_dest_dir, &b)
            .map_err(|e| format!("Failed to backup existing world before overwrite: {}", e))?;
        Some(b)
    } else {
        None
    };

    if let Err(e) = std::fs::rename(&temp_world_dir, &world_dest_dir) {
        let _ = std::fs::remove_dir_all(&temp_world_dir);
        if let Some(ref b) = backup_dir {
            let _ = std::fs::rename(b, &world_dest_dir);
        }
        return Err(format!("Failed to commit world directory: {}", e));
    }

    Ok((target_world_name, backup_dir))
}

/// Detect if a resourcepack, shaderpack, or datapack zip has all contents wrapped inside a top-level directory.
fn detect_zip_wrapper_prefix(zip_path: &Path) -> Option<String> {
    let file = std::fs::File::open(zip_path).ok()?;
    let mut archive = zip::ZipArchive::new(file).ok()?;

    let mut has_root_key_file = false;
    let mut potential_prefix: Option<String> = None;

    for raw_name in archive.file_names() {
        let norm = normalize_entry_name(raw_name);
        if norm.is_empty() {
            continue;
        }
        if norm.eq_ignore_ascii_case("pack.mcmeta")
            || norm.starts_with("shaders/")
            || norm.starts_with("assets/")
            || norm.starts_with("data/")
        {
            has_root_key_file = true;
            break;
        }
        if let Some((first, rest)) = norm.split_once('/') {
            if potential_prefix.is_none()
                && (rest.eq_ignore_ascii_case("pack.mcmeta")
                    || rest.starts_with("shaders/")
                    || rest.starts_with("assets/")
                    || rest.starts_with("data/"))
            {
                potential_prefix = Some(format!("{}/", first));
            }
        }
    }

    if has_root_key_file {
        return None;
    }
    potential_prefix
}

/// Copy a zip file or unwrap top-level wrapper folder so pack metadata is at root.
/// Returns Ok(Some(backup_path)) if an existing file was backed up, or Ok(None) if newly installed.
fn install_zip_stripping_wrapper_sync(
    src: &Path,
    dst: &Path,
    wrapper: Option<&str>,
) -> Result<Option<PathBuf>, String> {
    let dst_parent = dst
        .parent()
        .ok_or_else(|| "Target path has no parent directory".to_string())?;
    let temp_dst = dst_parent.join(format!(
        ".tmp_pack_{}_{}",
        uuid::Uuid::new_v4().simple(),
        dst.file_name().and_then(|n| n.to_str()).unwrap_or("pack")
    ));

    match wrapper {
        Some(prefix) if !prefix.is_empty() => {
            let src_file = std::fs::File::open(src).map_err(|e| format!("Failed to open zip: {}", e))?;
            let mut archive =
                zip::ZipArchive::new(src_file).map_err(|e| format!("Failed to read zip: {}", e))?;

            let dst_file =
                std::fs::File::create(&temp_dst).map_err(|e| format!("Failed to create temp zip: {}", e))?;
            let mut writer = zip::ZipWriter::new(dst_file);

            let mut total_decompressed_bytes: u64 = 0;
            let repack_res = (|| -> Result<(), String> {
                for i in 0..archive.len() {
                    let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
                    let entry_name = normalize_entry_name(entry.name());
                    let new_name = match entry_name.strip_prefix(prefix) {
                        Some(stripped) => stripped.to_string(),
                        None => continue,
                    };
                    if new_name.is_empty() || new_name.contains("..") {
                        continue;
                    }
                    if entry.is_dir() {
                        writer
                            .add_directory(&new_name, zip::write::SimpleFileOptions::default())
                            .map_err(|e| e.to_string())?;
                    } else {
                        let declared_size = entry.size();
                        if declared_size > MAX_EXTRACTED_FILE_BYTES {
                            return Err(format!(
                                "Entry '{}' declared size exceeds limit ({} > {} bytes)",
                                entry.name(),
                                declared_size,
                                MAX_EXTRACTED_FILE_BYTES
                            ));
                        }
                        writer
                            .start_file(&new_name, zip::write::SimpleFileOptions::default())
                            .map_err(|e| e.to_string())?;

                        let mut limited = (&mut entry).take(MAX_EXTRACTED_FILE_BYTES + 1);
                        let copied = std::io::copy(&mut limited, &mut writer).map_err(|e| e.to_string())?;
                        if copied > MAX_EXTRACTED_FILE_BYTES {
                            return Err(format!(
                                "Entry '{}' decompressed size exceeded limit",
                                entry.name()
                            ));
                        }
                        total_decompressed_bytes += copied;
                        if total_decompressed_bytes > MAX_AGGREGATE_EXTRACTED_BYTES {
                            return Err("Archive exceeded aggregate decompressed size limit".to_string());
                        }
                    }
                }
                writer.finish().map_err(|e| e.to_string())?;
                Ok(())
            })();

            if let Err(e) = repack_res {
                let _ = std::fs::remove_file(&temp_dst);
                return Err(format!("Failed to unwrap pack: {}", e));
            }
        }
        _ => {
            if let Err(e) = std::fs::copy(src, &temp_dst) {
                let _ = std::fs::remove_file(&temp_dst);
                return Err(format!("Failed to copy file to temp: {}", e));
            }
        }
    }

    let backup_file = if dst.exists() {
        let backup_name = format!(
            ".backup_{}_{}",
            uuid::Uuid::new_v4().simple(),
            dst.file_name().unwrap_or_default().to_string_lossy()
        );
        let b = dst_parent.join(backup_name);
        std::fs::rename(dst, &b)
            .map_err(|e| format!("Failed to backup existing file before overwrite: {}", e))?;
        Some(b)
    } else {
        None
    };

    if let Err(e) = std::fs::rename(&temp_dst, dst) {
        let _ = std::fs::remove_file(&temp_dst);
        if let Some(ref b) = backup_file {
            let _ = std::fs::rename(b, dst);
        }
        return Err(format!("Failed to commit pack: {}", e));
    }

    Ok(backup_file)
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

/// Compute SHA1 hex digest of a file asynchronously by streaming chunks.
async fn compute_sha1(path: &Path) -> Result<String, String> {
    use sha1::Digest;
    use tokio::io::AsyncReadExt;

    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|e| format!("Failed to open file for hash: {}", e))?;

    let mut hasher = sha1::Sha1::new();
    let mut buffer = [0u8; 64 * 1024];

    loop {
        let bytes_read = file
            .read(&mut buffer)
            .await
            .map_err(|e| format!("Failed to read file chunk for hash: {}", e))?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }

    Ok(format!("{:x}", hasher.finalize()))
}

/// Import a list of inspected drop files into an instance's appropriate subdirectories.
/// Verify instance is not currently locked by an active installation, update, or task.
async fn verify_instance_idle(
    instance_dir: &Path,
    instance_id: &str,
    task_manager: &crate::core::task::TaskManager,
) -> Result<(), DawnlandError> {
    let config_path = instance_dir.join("dlml.json");
    if config_path.exists() {
        let content = tokio::fs::read_to_string(&config_path)
            .await
            .map_err(|e| DawnlandError::Unknown(format!("Failed to read instance config: {}", e)))?;
        let cfg = serde_json::from_str::<crate::core::launcher::InstanceConfig>(&content)
            .map_err(|e| DawnlandError::Unknown(format!("Malformed instance config: {}", e)))?;
        if cfg.is_installing || cfg.is_updating {
            return Err(DawnlandError::Unknown(
                "Instance is currently installing or updating".to_string(),
            ));
        }
    }

    let tasks = task_manager
        .load_history()
        .await
        .map_err(|e| DawnlandError::Unknown(format!("Failed to verify instance tasks: {}", e)))?;
    for t in &tasks {
        if t.task_type.instance_id().as_deref() == Some(instance_id)
            && matches!(
                t.status,
                crate::core::task::TaskStatus::Pending
                    | crate::core::task::TaskStatus::Running
                    | crate::core::task::TaskStatus::Paused
            )
        {
            return Err(DawnlandError::Unknown(
                "Instance has an active background task".to_string(),
            ));
        }
    }

    Ok(())
}

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

    let instance_lock = get_instance_lock(&instance_id);
    let _instance_guard = instance_lock.lock().await;

    let base_dir = get_minecraft_base();
    let instance_dir = base_dir.join("versions").join(&instance_id);
    if !instance_dir.exists() {
        return Err(DawnlandError::InstanceNotFound(format!(
            "Instance '{}' not found",
            instance_id
        ))
        .into());
    }

    verify_instance_idle(&instance_dir, &instance_id, &task_manager).await?;

    let mut report = DropImportReport::default();

    for item in items {
        verify_instance_idle(&instance_dir, &instance_id, &task_manager).await?;
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

                // If a mod is dropped with .zip extension, install it as .jar so mod loaders load it
                let installed_filename = if filename.to_lowercase().ends_with(".zip") {
                    let stem = filename
                        .strip_suffix(".zip")
                        .or_else(|| filename.strip_suffix(".ZIP"))
                        .unwrap_or(&filename);
                    format!("{}.jar", stem)
                } else {
                    filename.clone()
                };

                let target_path = mods_dir.join(&installed_filename);
                if is_same_file_path(&src_path, &target_path) {
                    let rel_key = format!("mods/{}", installed_filename);
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

                // Re-verify instance idle immediately before committing file
                if let Err(e) = verify_instance_idle(&instance_dir, &instance_id, &task_manager).await {
                    report.failed.push(FailedFileResult {
                        filename,
                        error: format!("Instance is busy: {}", e),
                    });
                    continue;
                }

                // Copy to unique temp file first to avoid leaving partial/corrupt jar
                let temp_mod_path = mods_dir.join(format!(
                    ".tmp_mod_{}_{}",
                    uuid::Uuid::new_v4().simple(),
                    installed_filename
                ));
                if let Err(e) = tokio::fs::copy(&src_path, &temp_mod_path).await {
                    let _ = tokio::fs::remove_file(&temp_mod_path).await;
                    report.failed.push(FailedFileResult {
                        filename,
                        error: format!("Failed to copy mod file: {}", e),
                    });
                    continue;
                }

                // Backup existing target mod if overwriting
                let backup_mod_path = if target_path.exists() {
                    let b = mods_dir.join(format!(
                        ".backup_mod_{}_{}",
                        uuid::Uuid::new_v4().simple(),
                        installed_filename
                    ));
                    if let Err(e) = tokio::fs::rename(&target_path, &b).await {
                        let _ = tokio::fs::remove_file(&temp_mod_path).await;
                        report.failed.push(FailedFileResult {
                            filename,
                            error: format!("Failed to backup existing mod file: {}", e),
                        });
                        continue;
                    }
                    Some(b)
                } else {
                    None
                };

                // Backup conflicting counterpart (e.g. .disable or enabled variant) if overwriting
                let counterpart_name = if let Some(base) = installed_filename.strip_suffix(".disable") {
                    base.to_string()
                } else {
                    format!("{}.disable", installed_filename)
                };
                let counterpart_path = mods_dir.join(&counterpart_name);
                let counterpart_backup = if overwrite && counterpart_path.exists() {
                    let b = mods_dir.join(format!(
                        ".backup_mod_{}_{}",
                        uuid::Uuid::new_v4().simple(),
                        counterpart_name
                    ));
                    if tokio::fs::rename(&counterpart_path, &b).await.is_ok() {
                        Some((b, counterpart_path.clone(), counterpart_name.clone()))
                    } else {
                        None
                    }
                } else {
                    None
                };

                // Commit temp file to target_path
                if let Err(e) = tokio::fs::rename(&temp_mod_path, &target_path).await {
                    let _ = tokio::fs::remove_file(&temp_mod_path).await;
                    if let Some(ref b) = backup_mod_path {
                        let _ = tokio::fs::rename(b, &target_path).await;
                    }
                    if let Some((ref b, ref orig, _)) = counterpart_backup {
                        let _ = tokio::fs::rename(b, orig).await;
                    }
                    report.failed.push(FailedFileResult {
                        filename,
                        error: format!("Failed to commit mod file: {}", e),
                    });
                    continue;
                }

                let hash_sha1 = compute_sha1(&target_path).await.ok();
                let size = tokio::fs::metadata(&target_path)
                    .await
                    .map(|m| m.len())
                    .ok();

                // Store disabled mods under canonical name mods/<name>.jar matching toggle and delete logic
                let canonical_filename = installed_filename
                    .strip_suffix(".disable")
                    .unwrap_or(&installed_filename);
                let rel_key = format!("mods/{}", canonical_filename);

                let save_res = save_asset_to_manifest(
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
                        enabled: !installed_filename.ends_with(".disable"),
                        managed_by_modpack: false,
                    },
                )
                .await;

                if let Err(e) = save_res {
                    let _ = tokio::fs::remove_file(&target_path).await;
                    if let Some(ref b) = backup_mod_path {
                        let _ = tokio::fs::rename(b, &target_path).await;
                    }
                    if let Some((ref b, ref orig, _)) = counterpart_backup {
                        let _ = tokio::fs::rename(b, orig).await;
                    }
                    report.failed.push(FailedFileResult {
                        filename,
                        error: format!("Failed to update assets manifest: {}", e),
                    });
                    continue;
                }

                if let Some((ref b, _, ref name)) = counterpart_backup {
                    let _ = delete_asset_from_manifest(&instance_id, &format!("mods/{}", name)).await;
                    let _ = tokio::fs::remove_file(b).await;
                }
                if let Some(ref b) = backup_mod_path {
                    let _ = tokio::fs::remove_file(b).await;
                }

                tracing::info!(
                    "Drag-and-drop imported mod '{}' (as '{}') into '{}'",
                    filename,
                    installed_filename,
                    instance_id
                );
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

                // Re-verify instance idle immediately before committing file
                if let Err(e) = verify_instance_idle(&instance_dir, &instance_id, &task_manager).await {
                    report.failed.push(FailedFileResult {
                        filename,
                        error: format!("Instance is busy: {}", e),
                    });
                    continue;
                }

                let backup_opt = if inspected.is_dir {
                    let s = src_path.clone();
                    let d = target_path.clone();
                    let copy_res =
                        tokio::task::spawn_blocking(move || copy_dir_recursive_sync(&s, &d))
                            .await
                            .map_err(|e| format!("Task join error: {}", e))
                            .and_then(|r| r);
                    match copy_res {
                        Ok(b) => b,
                        Err(e) => {
                            report.failed.push(FailedFileResult {
                                filename,
                                error: e,
                            });
                            continue;
                        }
                    }
                } else {
                    let s_inspect = src_path.clone();
                    let wrapper = tokio::task::spawn_blocking(move || {
                        detect_zip_wrapper_prefix(&s_inspect)
                    })
                    .await
                    .unwrap_or(None);

                    let s = src_path.clone();
                    let d = target_path.clone();
                    let copy_res = tokio::task::spawn_blocking(move || {
                        install_zip_stripping_wrapper_sync(&s, &d, wrapper.as_deref())
                    })
                    .await
                    .map_err(|e| format!("Task join error: {}", e))
                    .and_then(|r| r);

                    match copy_res {
                        Ok(b) => b,
                        Err(e) => {
                            report.failed.push(FailedFileResult {
                                filename,
                                error: e,
                            });
                            continue;
                        }
                    }
                };

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

                let save_res = save_asset_to_manifest(
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

                if let Err(e) = save_res {
                    if inspected.is_dir {
                        let _ = tokio::fs::remove_dir_all(&target_path).await;
                        if let Some(ref b) = backup_opt {
                            let _ = tokio::fs::rename(b, &target_path).await;
                        }
                    } else {
                        let _ = tokio::fs::remove_file(&target_path).await;
                        if let Some(ref b) = backup_opt {
                            let _ = tokio::fs::rename(b, &target_path).await;
                        }
                    }
                    report.failed.push(FailedFileResult {
                        filename,
                        error: format!("Failed to update assets manifest: {}", e),
                    });
                    continue;
                }

                if let Some(ref b) = backup_opt {
                    if inspected.is_dir {
                        let _ = tokio::fs::remove_dir_all(b).await;
                    } else {
                        let _ = tokio::fs::remove_file(b).await;
                    }
                }

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

                let world_dir = instance_dir.join("saves").join(&resolved_world);
                if !world_dir.is_dir() {
                    report.failed.push(FailedFileResult {
                        filename,
                        error: format!(
                            "Target world '{}' does not exist in instance '{}'",
                            resolved_world, instance_id
                        ),
                    });
                    continue;
                }

                let datapacks_dir = world_dir.join("datapacks");
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

                // Re-verify instance idle immediately before committing file
                if let Err(e) = verify_instance_idle(&instance_dir, &instance_id, &task_manager).await {
                    report.failed.push(FailedFileResult {
                        filename,
                        error: format!("Instance is busy: {}", e),
                    });
                    continue;
                }

                let backup_opt = if inspected.is_dir {
                    let s = src_path.clone();
                    let d = target_path.clone();
                    let copy_res =
                        tokio::task::spawn_blocking(move || copy_dir_recursive_sync(&s, &d))
                            .await
                            .map_err(|e| format!("Task join error: {}", e))
                            .and_then(|r| r);
                    match copy_res {
                        Ok(b) => b,
                        Err(e) => {
                            report.failed.push(FailedFileResult {
                                filename,
                                error: e,
                            });
                            continue;
                        }
                    }
                } else {
                    let s_inspect = src_path.clone();
                    let wrapper = tokio::task::spawn_blocking(move || {
                        detect_zip_wrapper_prefix(&s_inspect)
                    })
                    .await
                    .unwrap_or(None);

                    let s = src_path.clone();
                    let d = target_path.clone();
                    let copy_res = tokio::task::spawn_blocking(move || {
                        install_zip_stripping_wrapper_sync(&s, &d, wrapper.as_deref())
                    })
                    .await
                    .map_err(|e| format!("Task join error: {}", e))
                    .and_then(|r| r);

                    match copy_res {
                        Ok(b) => b,
                        Err(e) => {
                            report.failed.push(FailedFileResult {
                                filename,
                                error: e,
                            });
                            continue;
                        }
                    }
                };

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

                let save_res = save_asset_to_manifest(
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

                if let Err(e) = save_res {
                    if inspected.is_dir {
                        let _ = tokio::fs::remove_dir_all(&target_path).await;
                        if let Some(ref b) = backup_opt {
                            let _ = tokio::fs::rename(b, &target_path).await;
                        }
                    } else {
                        let _ = tokio::fs::remove_file(&target_path).await;
                        if let Some(ref b) = backup_opt {
                            let _ = tokio::fs::rename(b, &target_path).await;
                        }
                    }
                    report.failed.push(FailedFileResult {
                        filename,
                        error: format!("Failed to update assets manifest: {}", e),
                    });
                    continue;
                }

                if let Some(ref b) = backup_opt {
                    if inspected.is_dir {
                        let _ = tokio::fs::remove_dir_all(b).await;
                    } else {
                        let _ = tokio::fs::remove_file(b).await;
                    }
                }

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

                    // Re-verify instance idle immediately before committing directory
                    if let Err(e) = verify_instance_idle(&instance_dir, &instance_id, &task_manager).await {
                        report.failed.push(FailedFileResult {
                            filename,
                            error: format!("Instance is busy: {}", e),
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
                    let backup_dir = match copy_res {
                        Ok(b) => b,
                        Err(e) => {
                            report.failed.push(FailedFileResult {
                                filename,
                                error: e,
                            });
                            continue;
                        }
                    };

                    let rel_key = format!("saves/{}", filename);
                    let save_res = save_asset_to_manifest(
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

                    if let Err(e) = save_res {
                        let _ = tokio::fs::remove_dir_all(&target_path).await;
                        if let Some(ref b) = backup_dir {
                            let _ = tokio::fs::rename(b, &target_path).await;
                        }
                        report.failed.push(FailedFileResult {
                            filename,
                            error: format!("Failed to update assets manifest: {}", e),
                        });
                        continue;
                    }

                    if let Some(ref b) = backup_dir {
                        let _ = tokio::fs::remove_dir_all(b).await;
                    }

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

                    // Re-verify instance idle immediately before extracting archive
                    if let Err(e) = verify_instance_idle(&instance_dir, &instance_id, &task_manager).await {
                        report.failed.push(FailedFileResult {
                            filename,
                            error: format!("Instance is busy: {}", e),
                        });
                        continue;
                    }

                    let extract_res = tokio::task::spawn_blocking(move || {
                        extract_world_archive_sync(&zp, &sd, &stem, overwrite)
                    })
                    .await
                    .map_err(|e| format!("Task join error: {}", e))
                    .and_then(|r| r);

                    match extract_res {
                        Ok((world_folder, backup_dir)) => {
                            let rel_key = format!("saves/{}", world_folder);
                            let save_res = save_asset_to_manifest(
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

                            if let Err(e) = save_res {
                                let committed_dir = saves_dir.join(&world_folder);
                                let _ = tokio::fs::remove_dir_all(&committed_dir).await;
                                if let Some(ref b) = backup_dir {
                                    let _ = tokio::fs::rename(b, &committed_dir).await;
                                }
                                report.failed.push(FailedFileResult {
                                    filename,
                                    error: format!("Failed to update assets manifest: {}", e),
                                });
                                continue;
                            }

                            if let Some(ref b) = backup_dir {
                                let _ = tokio::fs::remove_dir_all(b).await;
                            }

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
            }        }
    }

    Ok(report)
}

/// Helper to save or update an asset in `assets.json` atomically with shared serialization lock.
async fn save_asset_to_manifest(
    instance_id: &str,
    rel_key: String,
    record: AssetRecord,
) -> Result<(), String> {
    let _guard = crate::core::manager::MANIFEST_LOCK.lock().await;

    let base_dir = get_minecraft_base();
    let instance_dir = base_dir.join("versions").join(instance_id);
    let assets_path = instance_dir.join("assets.json");

    let mut manifest = if assets_path.exists() {
        let content = tokio::fs::read_to_string(&assets_path)
            .await
            .map_err(|e| format!("Failed to read assets.json: {}", e))?;
        serde_json::from_str::<crate::models::instance::AssetManifest>(&content)
            .map_err(|e| format!("Failed to parse assets.json: {}", e))?
    } else {
        crate::models::instance::AssetManifest::default()
    };

    manifest.assets.insert(rel_key, record);

    let json_str = serde_json::to_string_pretty(&manifest)
        .map_err(|e| format!("Failed to serialize assets.json: {}", e))?;

    let temp_assets_path = instance_dir.join(format!(".assets_{}.json.tmp", uuid::Uuid::new_v4().simple()));
    if let Err(e) = tokio::fs::write(&temp_assets_path, &json_str).await {
        let _ = tokio::fs::remove_file(&temp_assets_path).await;
        return Err(format!("Failed to write temp assets.json: {}", e));
    }

    if let Err(e) = tokio::fs::rename(&temp_assets_path, &assets_path).await {
        let _ = tokio::fs::remove_file(&temp_assets_path).await;
        return Err(format!("Failed to commit assets.json: {}", e));
    }

    Ok(())
}

/// Helper to remove an asset entry from `assets.json` atomically with shared serialization lock.
async fn delete_asset_from_manifest(instance_id: &str, rel_key: &str) -> Result<(), String> {
    let _guard = crate::core::manager::MANIFEST_LOCK.lock().await;

    let base_dir = get_minecraft_base();
    let instance_dir = base_dir.join("versions").join(instance_id);
    let assets_path = instance_dir.join("assets.json");

    if !assets_path.exists() {
        return Ok(());
    }

    let content = tokio::fs::read_to_string(&assets_path)
        .await
        .map_err(|e| format!("Failed to read assets.json: {}", e))?;
    let mut manifest = serde_json::from_str::<crate::models::instance::AssetManifest>(&content)
        .map_err(|e| format!("Failed to parse assets.json: {}", e))?;

    if manifest.assets.remove(rel_key).is_some() {
        let json_str = serde_json::to_string_pretty(&manifest)
            .map_err(|e| format!("Failed to serialize assets.json: {}", e))?;

        let temp_assets_path = instance_dir.join(format!(".assets_{}.json.tmp", uuid::Uuid::new_v4().simple()));
        if let Err(e) = tokio::fs::write(&temp_assets_path, &json_str).await {
            let _ = tokio::fs::remove_file(&temp_assets_path).await;
            return Err(format!("Failed to write temp assets.json: {}", e));
        }

        if let Err(e) = tokio::fs::rename(&temp_assets_path, &assets_path).await {
            let _ = tokio::fs::remove_file(&temp_assets_path).await;
            return Err(format!("Failed to commit assets.json: {}", e));
        }
    }

    Ok(())
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
        let (extracted_name, backup_opt) =
            extract_world_archive_sync(&world_zip, &saves_dir, "skyblock_archive", true)
                .expect("extract world");
        if let Some(ref b) = backup_opt {
            let _ = std::fs::remove_dir_all(b);
        }
        assert_eq!(extracted_name, "SkyBlockWorld");
        assert!(saves_dir.join("SkyBlockWorld").join("level.dat").exists());
        assert!(saves_dir
            .join("SkyBlockWorld")
            .join("region")
            .join("r.0.0.mca")
            .exists());

        // 6. Path traversal in world zip rejection test
        let evil_zip = temp.join("evil_world.zip");
        create_test_zip(
            &evil_zip,
            &[
                ("level.dat", b"nbt"),
                ("../evil.txt", b"escaped"),
                ("subdir/../../evil2.txt", b"escaped2"),
            ],
        );
        let evil_saves_dir = temp.join("evil_saves");
        let (extracted_evil, backup_evil) =
            extract_world_archive_sync(&evil_zip, &evil_saves_dir, "evil_world", true)
                .expect("extract evil");
        if let Some(ref b) = backup_evil {
            let _ = std::fs::remove_dir_all(b);
        }
        assert_eq!(extracted_evil, "evil_world");
        assert!(evil_saves_dir.join("evil_world").join("level.dat").exists());
        assert!(!evil_saves_dir.join("evil.txt").exists());
        assert!(!temp.join("evil.txt").exists());
        assert!(!temp.join("evil2.txt").exists());

        let _ = std::fs::remove_dir_all(&temp);
    }

    #[test]
    fn test_world_overwrite_backup_retention_and_restore() {
        let temp = std::env::temp_dir().join(format!("dlml_drop_backup_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp).expect("mkdir");

        let saves_dir = temp.join("saves");
        let initial_world = saves_dir.join("MyWorld");
        std::fs::create_dir_all(&initial_world).expect("create initial world");
        std::fs::write(initial_world.join("level.dat"), b"initial_content").expect("write initial level.dat");

        let new_world_zip = temp.join("new_world.zip");
        create_test_zip(
            &new_world_zip,
            &[("level.dat", b"new_content")],
        );

        let (target_world, backup_dir_opt) =
            extract_world_archive_sync(&new_world_zip, &saves_dir, "MyWorld", true)
                .expect("extract world overwrite");

        assert_eq!(target_world, "MyWorld");
        let backup_dir = backup_dir_opt.expect("backup_dir must be Some when overwriting");
        assert!(backup_dir.exists(), "backup directory must exist");
        assert_eq!(
            std::fs::read(backup_dir.join("level.dat")).unwrap(),
            b"initial_content"
        );
        assert_eq!(
            std::fs::read(initial_world.join("level.dat")).unwrap(),
            b"new_content"
        );

        // Simulate failure in manifest save: rollback world from backup
        std::fs::remove_dir_all(&initial_world).expect("remove corrupt world");
        std::fs::rename(&backup_dir, &initial_world).expect("restore from backup");
        assert_eq!(
            std::fs::read(initial_world.join("level.dat")).unwrap(),
            b"initial_content"
        );

        let _ = std::fs::remove_dir_all(&temp);
    }

    #[test]
    fn test_install_zip_stripping_wrapper_sync_and_backup() {
        let temp = std::env::temp_dir().join(format!("dlml_repack_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp).expect("mkdir");

        let wrapped_zip = temp.join("wrapped_pack.zip");
        create_test_zip(
            &wrapped_zip,
            &[
                ("TopFolder/pack.mcmeta", br#"{"pack":{"pack_format":15}}"#),
                ("TopFolder/assets/minecraft/textures/block/stone.png", b"stone_texture"),
            ],
        );

        let detected = detect_zip_wrapper_prefix(&wrapped_zip);
        assert_eq!(detected.as_deref(), Some("TopFolder/"));

        let packs_dir = temp.join("resourcepacks");
        std::fs::create_dir_all(&packs_dir).expect("create packs dir");
        let target_pack = packs_dir.join("unwrapped.zip");

        // First install: target does not exist -> backup is None
        let b1 = install_zip_stripping_wrapper_sync(&wrapped_zip, &target_pack, detected.as_deref())
            .expect("install pack");
        assert!(b1.is_none());
        assert!(target_pack.exists());

        // Verify pack was unwrapped: pack.mcmeta is now at root
        let target_file = std::fs::File::open(&target_pack).expect("open target");
        let mut target_archive = zip::ZipArchive::new(target_file).expect("read archive");
        assert!(target_archive.by_name("pack.mcmeta").is_ok());

        // Second install with overwrite: target exists -> backup is Some
        let b2 = install_zip_stripping_wrapper_sync(&wrapped_zip, &target_pack, detected.as_deref())
            .expect("overwrite pack");
        let backup = b2.expect("backup must be Some on overwrite");
        assert!(backup.exists());
        let _ = std::fs::remove_file(backup);

        let _ = std::fs::remove_dir_all(&temp);
    }
}
