use crate::core::drop_install::get_instance_lock;
use crate::core::fs_utils::{is_safe_single_name, write_file_atomic};
use crate::core::hash::{calculate_curseforge_hash, calculate_file_hashes};
use crate::core::launcher::RunningInstances;
use crate::core::manager::MANIFEST_LOCK;
use crate::core::mod_parser::{build_mod_cache_key, ModParser};
use crate::core::modrinth::UnifiedModProject;
use crate::core::mojang::get_minecraft_base;
use crate::core::resource_naming::{
    extract_short_zh_name, render_resource_filename, resolve_filename_for_download,
    resolve_unique_filename_in_dir, sanitize_filename_component, strip_known_zh_prefixes,
    ResourceNamingContext,
};
use crate::core::settings::{
    get_launcher_settings, resolve_active_template, should_rename_category, LauncherSettings,
};
use crate::core::translation::{
    category_to_key, lookup_translation_async, resolve_online_translations, HashMappingEntry,
    OnlineResolveQueryItem, TranslationDb, TranslationEntry, TranslationLookupResult,
    TRUST_AUTHOR_LANG,
};
use crate::models::instance::{AssetCategory, AssetManifest, AssetRecord, AssetSourceType};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter, Manager};

#[derive(Debug, Clone)]
pub struct DownloadTargetInput {
    pub target_dir: PathBuf,
    pub category: AssetCategory,
    pub raw_original_filename: String,
    pub source: Option<String>,
    pub project_id: Option<String>,
    pub slug: Option<String>,
    pub mod_id: Option<String>,
    pub en_name: Option<String>,
    pub version_number: Option<String>,
    pub mc_version: Option<String>,
    pub loader: Option<String>,
    pub zh_name_override: Option<String>,
    pub existing_paths_to_ignore: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ResolvedDownloadTarget {
    pub final_filename: String,
    pub original_filename: String,
    pub display_name_zh: Option<String>,
}

/// Resolve the target filename on disk when downloading or installing a resource,
/// applying active user naming rules while returning the canonical original filename.
pub async fn resolve_download_target(input: DownloadTargetInput) -> ResolvedDownloadTarget {
    let settings = get_launcher_settings().await;
    let is_mod = input.category == AssetCategory::Mod;

    let clean_raw = sanitize_filename_component(&input.raw_original_filename, is_mod);
    let canonical_original = {
        let stripped = strip_known_zh_prefixes(&clean_raw);
        if stripped.is_empty() {
            clean_raw.clone()
        } else {
            stripped
        }
    };
    let original_filename = if canonical_original.is_empty() || !is_safe_single_name(&canonical_original) {
        input.raw_original_filename.clone()
    } else {
        canonical_original
    };

    let lookup = lookup_translation_async(
        input.category.clone(),
        input.source.clone(),
        input.project_id.clone(),
        input.slug.clone(),
        input.mod_id.clone(),
        input.en_name.clone(),
    )
    .await;

    let zh_full = lookup
        .as_ref()
        .map(|l| l.zh_name.clone())
        .or_else(|| input.zh_name_override.clone());
    let zh_short = lookup
        .as_ref()
        .map(|l| l.zh_short_name.clone())
        .or_else(|| {
            input
                .zh_name_override
                .as_deref()
                .map(extract_short_zh_name)
                .filter(|s| !s.is_empty())
        });

    let ctx = ResourceNamingContext {
        original_filename: original_filename.clone(),
        category: input.category.into(),
        zh_name: zh_short.clone(),
        zh_full_name: zh_full,
        en_name: input.en_name,
        slug: input.slug,
        version: input.version_number,
        mc_version: input.mc_version,
        loader: input.loader,
        source: input.source,
    };

    let desired = resolve_filename_for_download(&settings, &ctx);
    let target_dir = input.target_dir;
    let ignore = input.existing_paths_to_ignore;
    let desired_clone = desired.clone();

    let final_filename = tokio::task::spawn_blocking(move || {
        let ignore_refs: Vec<&str> = ignore.iter().map(|s| s.as_str()).collect();
        resolve_unique_filename_in_dir(&target_dir, &desired_clone, &ignore_refs)
    })
    .await
    .unwrap_or(desired);

    ResolvedDownloadTarget {
        final_filename,
        original_filename,
        display_name_zh: zh_short,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnrichedModProject {
    #[serde(flatten)]
    pub project: UnifiedModProject,
    pub zh_title: Option<String>,
    pub zh_summary: Option<String>,
    pub wiki_id: Option<u32>,
    pub trust_level: Option<u8>,
}

/// Enrich search/detail project items with localized Chinese titles and summaries from `translations.db`.
#[tauri::command]
pub async fn enrich_online_projects(
    category: AssetCategory,
    projects: Vec<UnifiedModProject>,
    _resolve_missing_online: Option<bool>,
) -> Result<Vec<EnrichedModProject>, String> {
    if projects.is_empty() {
        return Ok(Vec::new());
    }

    let final_lookups = lookup_projects_batch(category, &projects).await;

    Ok(projects
        .into_iter()
        .zip(final_lookups)
        .map(|(project, found)| {
            if let Some(t) = found {
                EnrichedModProject {
                    project,
                    zh_title: Some(t.zh_name),
                    zh_summary: t.zh_summary,
                    wiki_id: t.wiki_id,
                    trust_level: Some(t.trust_level),
                }
            } else {
                EnrichedModProject {
                    project,
                    zh_title: None,
                    zh_summary: None,
                    wiki_id: None,
                    trust_level: None,
                }
            }
        })
        .collect())
}

async fn lookup_projects_batch(
    category: AssetCategory,
    projects: &[UnifiedModProject],
) -> Vec<Option<TranslationLookupResult>> {
    let queries: Vec<(String, String, String, String)> = projects
        .iter()
        .map(|p| {
            (
                p.source.clone(),
                p.project_id.clone(),
                p.slug.clone(),
                p.title.clone(),
            )
        })
        .collect();

    tokio::task::spawn_blocking(move || {
        let Ok(db) = TranslationDb::open_default() else {
            return vec![None; queries.len()];
        };
        queries
            .iter()
            .map(|(source, pid, slug, title)| {
                db.lookup(
                    &category,
                    Some(source.as_str()),
                    Some(pid.as_str()),
                    Some(slug.as_str()),
                    None,
                    Some(title.as_str()),
                )
            })
            .collect()
    })
    .await
    .unwrap_or_else(|_| vec![None; projects.len()])
}

#[derive(Debug, Clone)]
struct ScannedResourceFile {
    category: AssetCategory,
    rel_path: String,
    disk_path: PathBuf,
    enabled: bool,
    base_filename: String,
    size: u64,
    mtime: u64,
}

async fn scan_instance_resource_files(instance_dir: &Path) -> Vec<ScannedResourceFile> {
    let mut results = Vec::new();

    // 1. Mods
    let mods_dir = instance_dir.join("mods");
    if tokio::fs::try_exists(&mods_dir).await.unwrap_or(false) {
        if let Ok(mut entries) = tokio::fs::read_dir(&mods_dir).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let path = entry.path();
                let Ok(meta) = tokio::fs::metadata(&path).await else {
                    continue;
                };
                if meta.is_dir() {
                    continue;
                }
                let fname = entry.file_name().to_string_lossy().to_string();
                let (enabled, base_filename) = if fname.ends_with(".jar.disable") {
                    (false, fname.trim_end_matches(".disable").to_string())
                } else if fname.ends_with(".jar") {
                    (true, fname.clone())
                } else {
                    continue;
                };
                let mtime = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                results.push(ScannedResourceFile {
                    category: AssetCategory::Mod,
                    rel_path: format!("mods/{}", fname),
                    disk_path: path,
                    enabled,
                    base_filename,
                    size: meta.len(),
                    mtime,
                });
            }
        }
    }

    // 2. Resource packs & Shader packs
    for (folder, category) in [
        ("resourcepacks", AssetCategory::ResourcePack),
        ("shaderpacks", AssetCategory::ShaderPack),
    ] {
        let dir = instance_dir.join(folder);
        if !tokio::fs::try_exists(&dir).await.unwrap_or(false) {
            continue;
        }
        if let Ok(mut entries) = tokio::fs::read_dir(&dir).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let path = entry.path();
                let Ok(meta) = tokio::fs::metadata(&path).await else {
                    continue;
                };
                if meta.is_dir() {
                    continue;
                }
                let fname = entry.file_name().to_string_lossy().to_string();
                let (enabled, base_filename) = if fname.ends_with(".disable") {
                    (false, fname.trim_end_matches(".disable").to_string())
                } else {
                    (true, fname.clone())
                };
                let mtime = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                results.push(ScannedResourceFile {
                    category: category.clone(),
                    rel_path: format!("{}/{}", folder, fname),
                    disk_path: path,
                    enabled,
                    base_filename,
                    size: meta.len(),
                    mtime,
                });
            }
        }
    }

    // 3. Datapacks inside saves/*/datapacks/*.zip
    let saves_dir = instance_dir.join("saves");
    if tokio::fs::try_exists(&saves_dir).await.unwrap_or(false) {
        if let Ok(mut worlds) = tokio::fs::read_dir(&saves_dir).await {
            while let Ok(Some(world_entry)) = worlds.next_entry().await {
                let world_name = world_entry.file_name().to_string_lossy().to_string();
                let dp_dir = world_entry.path().join("datapacks");
                if !tokio::fs::try_exists(&dp_dir).await.unwrap_or(false) {
                    continue;
                }
                if let Ok(mut entries) = tokio::fs::read_dir(&dp_dir).await {
                    while let Ok(Some(entry)) = entries.next_entry().await {
                        let path = entry.path();
                        let Ok(meta) = tokio::fs::metadata(&path).await else {
                            continue;
                        };
                        if meta.is_dir() {
                            continue;
                        }
                        let fname = entry.file_name().to_string_lossy().to_string();
                        if !fname.ends_with(".zip") && !fname.ends_with(".zip.disable") {
                            continue;
                        }
                        let (enabled, base_filename) = if fname.ends_with(".disable") {
                            (false, fname.trim_end_matches(".disable").to_string())
                        } else {
                            (true, fname.clone())
                        };
                        let mtime = meta
                            .modified()
                            .ok()
                            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                            .map(|d| d.as_secs())
                            .unwrap_or(0);
                        results.push(ScannedResourceFile {
                            category: AssetCategory::Datapack,
                            rel_path: format!("saves/{}/datapacks/{}", world_name, fname),
                            disk_path: path,
                            enabled,
                            base_filename,
                            size: meta.len(),
                            mtime,
                        });
                    }
                }
            }
        }
    }

    results
}

fn find_record_key(manifest: &AssetManifest, file: &ScannedResourceFile) -> Option<String> {
    if manifest.assets.contains_key(&file.rel_path) {
        return Some(file.rel_path.clone());
    }
    let prefix_dir = file
        .rel_path
        .rsplit_once('/')
        .map(|(dir, _)| dir)
        .unwrap_or("");
    let enabled_key = if prefix_dir.is_empty() {
        file.base_filename.clone()
    } else {
        format!("{}/{}", prefix_dir, file.base_filename)
    };
    if manifest.assets.contains_key(&enabled_key) {
        return Some(enabled_key);
    }
    let disabled_key = format!("{}.disable", enabled_key);
    if manifest.assets.contains_key(&disabled_key) {
        return Some(disabled_key);
    }
    None
}

/// Pure helper to detect and strip modpack project IDs accidentally stamped onto individual
/// `managed_by_modpack` records. Returns `(polluted_project_ids, manifest_changed)`.
pub fn scrub_polluted_modpack_manifest_records(
    dlml_modpack_pid: Option<&str>,
    manifest: &mut AssetManifest,
) -> (HashSet<String>, bool) {
    let mut polluted: HashSet<String> = HashSet::new();

    if let Some(mp_pid) = dlml_modpack_pid.map(str::trim).filter(|s| !s.is_empty()) {
        polluted.insert(mp_pid.to_string());
    }

    // Any project_id shared by >= 2 modpack-managed assets in the same instance cannot be an individual mod ID
    let mut counts: HashMap<String, usize> = HashMap::new();
    for rec in manifest.assets.values() {
        if rec.managed_by_modpack {
            if let Some(ref pid) = rec.project_id {
                let p = pid.trim();
                if !p.is_empty() {
                    *counts.entry(p.to_string()).or_insert(0) += 1;
                }
            }
        }
    }
    for (pid, cnt) in counts {
        if cnt >= 2 {
            polluted.insert(pid);
        }
    }

    if polluted.is_empty() {
        return (polluted, false);
    }

    let mut manifest_changed = false;
    for rec in manifest.assets.values_mut() {
        if rec.managed_by_modpack {
            if let Some(ref pid) = rec.project_id {
                if polluted.contains(pid.trim()) {
                    rec.project_id = None;
                    manifest_changed = true;
                }
            }
        }
    }

    (polluted, manifest_changed)
}

/// Detect and strip modpack project IDs accidentally stamped onto individual `managed_by_modpack`
/// records by legacy CurseForge modpack installs, and purge any poisoned `TRUST_USER` entries in `translations.db`.
pub async fn sanitize_polluted_modpack_manifest(
    instance_dir: &Path,
    manifest: &mut AssetManifest,
) -> (HashSet<String>, bool) {
    let mut dlml_mp_pid: Option<String> = None;
    let dlml_path = instance_dir.join("dlml.json");
    if let Ok(content) = tokio::fs::read_to_string(&dlml_path).await {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
            dlml_mp_pid = val
                .get("modpackProjectId")
                .or_else(|| val.get("modpack_project_id"))
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string);
        }
    }

    let (polluted, manifest_changed) =
        scrub_polluted_modpack_manifest_records(dlml_mp_pid.as_deref(), manifest);

    if polluted.is_empty() {
        return (polluted, false);
    }

    let polluted_for_db = polluted.clone();
    let _ = tokio::task::spawn_blocking(move || {
        if let Ok(db) = TranslationDb::open_default() {
            let _ = db.purge_polluted_user_entries(&polluted_for_db);
        }
    })
    .await;

    (polluted, manifest_changed)
}

async fn read_instance_mc_and_loader(instance_dir: &Path, instance_id: &str) -> (String, String) {
    let dlml_path = instance_dir.join("dlml.json");
    if let Ok(content) = tokio::fs::read_to_string(&dlml_path).await {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
            let mc = val
                .get("mcVersion")
                .or_else(|| val.get("mc_version"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let loader = val
                .get("loaderType")
                .or_else(|| val.get("loader_type"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if !mc.is_empty() || !loader.is_empty() {
                return (mc, loader);
            }
        }
    }
    if let Ok(detail) = crate::core::manager::get_instance_details(instance_id.to_string()).await {
        return (detail.mc_version, detail.loader_type);
    }
    (String::new(), String::new())
}

/// Event-driven background identification of instance resources via SHA-1 / CurseForge MurmurHash2,
/// local JAR `zh_cn.json` extraction, `translations.db` matching, and optional backend resolution.
/// Emits `"instance-translations-updated"` when any new metadata or translation is written.
#[tauri::command]
pub async fn resolve_instance_translations(
    app: AppHandle,
    instance_id: String,
    force_online: Option<bool>,
) -> Result<usize, String> {
    if !is_safe_single_name(&instance_id) {
        return Err("Invalid instance ID".to_string());
    }

    let base_dir = get_minecraft_base();
    let instance_dir = base_dir.join("versions").join(&instance_id);
    if !tokio::fs::try_exists(&instance_dir).await.unwrap_or(false) {
        return Err(format!("Instance directory not found: {}", instance_id));
    }

    let scanned = scan_instance_resource_files(&instance_dir).await;
    if scanned.is_empty() {
        return Ok(0);
    }

    let assets_path = instance_dir.join("assets.json");
    let mut manifest = {
        let _lock = MANIFEST_LOCK.lock().await;
        if let Ok(content) = tokio::fs::read_to_string(&assets_path).await {
            serde_json::from_str::<AssetManifest>(&content).unwrap_or_default()
        } else {
            AssetManifest::default()
        }
    };
    let (_, sanitized_dirty) = sanitize_polluted_modpack_manifest(&instance_dir, &mut manifest).await;
    if sanitized_dirty {
        let _lock = MANIFEST_LOCK.lock().await;
        if let Ok(json_str) = serde_json::to_string_pretty(&manifest) {
            let _ = write_file_atomic(&assets_path, json_str.as_bytes()).await;
        }
    }

    let (_, loader_type) = read_instance_mc_and_loader(&instance_dir, &instance_id).await;

    // Step 1: Compute missing SHA-1 hashes & Murmur2 hashes, plus check cached file_hash_map
    struct FileWorkItem {
        file: ScannedResourceFile,
        manifest_key: String,
        sha1: String,
        sha512: Option<String>,
        murmur2: u32,
    }

    let mut work_items = Vec::with_capacity(scanned.len());
    for file in scanned {
        let existing_key = find_record_key(&manifest, &file);
        let existing_rec = existing_key.as_ref().and_then(|k| manifest.assets.get(k));

        let (sha1, sha512) = if let Some(h) = existing_rec.and_then(|r| r.hash_sha1.clone()) {
            (h.to_ascii_lowercase(), existing_rec.and_then(|r| r.hash_sha512.clone()))
        } else {
            match calculate_file_hashes(&file.disk_path).await {
                Ok((s1, s512)) => (s1, Some(s512)),
                Err(_) => continue,
            }
        };

        let murmur2 = calculate_curseforge_hash(&file.disk_path).await.unwrap_or(0);
        let manifest_key = existing_key.unwrap_or_else(|| {
            if file.enabled {
                file.rel_path.clone()
            } else {
                file.rel_path.trim_end_matches(".disable").to_string()
            }
        });

        work_items.push(FileWorkItem {
            file,
            manifest_key,
            sha1,
            sha512,
            murmur2,
        });
    }

    // Step 2: Check local `file_hash_map` for existing hash->project mappings
    let sha1s: Vec<String> = work_items.iter().map(|w| w.sha1.clone()).collect();
    let cached_hash_map = tokio::task::spawn_blocking(move || {
        let mut map = HashMap::new();
        if let Ok(db) = TranslationDb::open_default() {
            for s in sha1s {
                if let Some(m) = db.get_hash_mapping(&s) {
                    map.insert(s, m);
                }
            }
        }
        map
    })
    .await
    .unwrap_or_default();

    let mut need_mr_sha1 = Vec::new();
    let mut need_cf_murmur = Vec::new();

    for w in &work_items {
        let has_proj = manifest
            .assets
            .get(&w.manifest_key)
            .and_then(|r| r.project_id.as_ref())
            .is_some();
        if !has_proj && !cached_hash_map.contains_key(&w.sha1) {
            need_mr_sha1.push(w.sha1.clone());
            if w.murmur2 != 0 {
                need_cf_murmur.push(w.murmur2);
            }
        }
    }

    // Step 3: Batch Modrinth hash recognition
    let mut new_mappings: Vec<HashMappingEntry> = Vec::new();
    let mut newly_resolved_sha1: HashSet<String> = HashSet::new();

    if !need_mr_sha1.is_empty() {
        if let Ok(mr_matches) = crate::core::modrinth::get_modrinth_version_files_by_hashes(need_mr_sha1).await {
            let pids: Vec<String> = mr_matches
                .iter()
                .map(|m| m.project_id.clone())
                .collect::<HashSet<_>>()
                .into_iter()
                .collect();
            let mr_projects = crate::core::modrinth::get_modrinth_projects_batch(pids)
                .await
                .unwrap_or_default();
            let proj_map: HashMap<String, UnifiedModProject> = mr_projects
                .into_iter()
                .map(|p| (p.project_id.clone(), p))
                .collect();

            for m in mr_matches {
                let sha_lower = m.sha1.to_ascii_lowercase();
                let proj = proj_map.get(&m.project_id);
                let cat_str = work_items
                    .iter()
                    .find(|w| w.sha1 == sha_lower)
                    .map(|w| category_to_key(&w.file.category).to_string())
                    .unwrap_or_else(|| "mod".to_string());
                let murmur = work_items
                    .iter()
                    .find(|w| w.sha1 == sha_lower)
                    .map(|w| w.murmur2);
                new_mappings.push(HashMappingEntry {
                    sha1: sha_lower.clone(),
                    cf_murmur2: murmur.filter(|v| *v != 0),
                    source: "modrinth".to_string(),
                    project_id: m.project_id.clone(),
                    slug: proj.map(|p| p.slug.clone()).unwrap_or_default(),
                    version_id: Some(m.version_id),
                    category: cat_str,
                    raw_name: proj.map(|p| p.title.clone()),
                });
                newly_resolved_sha1.insert(sha_lower);
            }
        }
    }

    // Step 4: Batch CurseForge Murmur2 recognition for anything Modrinth missed
    need_cf_murmur.retain(|m| {
        work_items
            .iter()
            .any(|w| w.murmur2 == *m && !newly_resolved_sha1.contains(&w.sha1))
    });
    if !need_cf_murmur.is_empty() {
        if let Ok(cf_matches) = crate::core::curseforge::get_cf_files_by_fingerprints(need_cf_murmur).await {
            let mod_ids: Vec<i64> = cf_matches
                .iter()
                .filter_map(|m| m.project_id.parse::<i64>().ok())
                .collect::<HashSet<_>>()
                .into_iter()
                .collect();
            let cf_projects = crate::core::curseforge::get_cf_projects_batch(mod_ids)
                .await
                .unwrap_or_default();
            let proj_map: HashMap<String, UnifiedModProject> = cf_projects
                .into_iter()
                .map(|p| (p.project_id.clone(), p))
                .collect();

            for m in cf_matches {
                if let Some(w) = work_items.iter().find(|w| w.murmur2 == m.fingerprint) {
                    if newly_resolved_sha1.contains(&w.sha1) {
                        continue;
                    }
                    let proj = proj_map.get(&m.project_id);
                    new_mappings.push(HashMappingEntry {
                        sha1: w.sha1.clone(),
                        cf_murmur2: Some(m.fingerprint),
                        source: "curseforge".to_string(),
                        project_id: m.project_id.clone(),
                        slug: proj.map(|p| p.slug.clone()).unwrap_or_default(),
                        version_id: Some(m.file_id),
                        category: category_to_key(&w.file.category).to_string(),
                        raw_name: proj.map(|p| p.title.clone()),
                    });
                    newly_resolved_sha1.insert(w.sha1.clone());
                }
            }
        }
    }

    if !new_mappings.is_empty() {
        let mappings_clone = new_mappings.clone();
        let _ = tokio::task::spawn_blocking(move || {
            if let Ok(db) = TranslationDb::open_default() {
                let _ = db.save_hash_mappings(&mappings_clone);
            }
        })
        .await;
    }

    // Merge all hash mappings
    let mut all_hash_map = cached_hash_map;
    for m in new_mappings {
        all_hash_map.insert(m.sha1.clone(), m);
    }

    // Step 5: Extract mod metadata (mod_id, name, embedded zh_cn.json) and upsert Tier-2 author translations
    let mod_meta_map: HashMap<String, crate::core::mod_parser::ModMetadata> = {
        let base_dir_clone = base_dir.clone();
        let mod_files: Vec<(String, PathBuf, String)> = work_items
            .iter()
            .filter(|w| w.file.category == AssetCategory::Mod)
            .map(|w| {
                let key = build_mod_cache_key(&w.file.base_filename, w.file.size, w.file.mtime, &loader_type);
                (w.manifest_key.clone(), w.file.disk_path.clone(), key)
            })
            .collect();
        let loader_clone = loader_type.clone();
        tokio::task::spawn_blocking(move || {
            let parser = ModParser::new(&base_dir_clone);
            let mut cached = parser.load_all_cache();
            let mut out = HashMap::new();
            let mut author_entries = Vec::new();
            for (manifest_key, disk_path, cache_key) in mod_files {
                let meta = if let Some(m) = cached.get(&cache_key) {
                    m.clone()
                } else {
                    let parsed = parser.parse_mod(&disk_path, &cache_key, Some(loader_clone.as_str()));
                    cached.insert(cache_key.clone(), parsed.clone());
                    parser.set_cache_entry(&cache_key, &parsed);
                    parsed
                };
                if let Some(ref zh_emb) = meta.zh_name_embedded {
                    let clean_zh = zh_emb.trim();
                    if !clean_zh.is_empty() {
                        author_entries.push(TranslationEntry {
                            category: "mod".to_string(),
                            cf_project_id: None,
                            cf_slug: None,
                            mr_project_id: None,
                            mr_slug: None,
                            mod_ids: meta.mod_id.clone().unwrap_or_default(),
                            raw_name: meta.name.clone().unwrap_or_default(),
                            zh_name: clean_zh.to_string(),
                            zh_short_name: extract_short_zh_name(clean_zh),
                            zh_aliases: String::new(),
                            zh_summary: String::new(),
                            wiki_id: 0,
                            trust_level: TRUST_AUTHOR_LANG,
                            source_tag: "author_lang".to_string(),
                            updated_at: 0,
                        });
                    }
                }
                out.insert(manifest_key, meta);
            }
            if !author_entries.is_empty() {
                if let Ok(db) = TranslationDb::open_default() {
                    let _ = db.upsert_entries_batch(&author_entries);
                }
            }
            out
        })
        .await
        .unwrap_or_default()
    };

    // Step 6: Lookup translations locally; collect unresolved items for optional backend resolution
    struct ItemLookupSpec {
        manifest_key: String,
        category: AssetCategory,
        source: Option<String>,
        project_id: Option<String>,
        slug: Option<String>,
        mod_id: Option<String>,
        raw_name: Option<String>,
    }

    let mut specs = Vec::with_capacity(work_items.len());
    for w in &work_items {
        let rec_opt = manifest.assets.get(&w.manifest_key);
        let hmap = all_hash_map.get(&w.sha1);
        let mod_meta = mod_meta_map.get(&w.manifest_key);

        let source = hmap
            .map(|h| h.source.clone())
            .or_else(|| {
                rec_opt.and_then(|r| match r.source_type {
                    AssetSourceType::Modrinth => Some("modrinth".to_string()),
                    AssetSourceType::CurseForge => Some("curseforge".to_string()),
                    _ => None,
                })
            });
        let project_id = hmap
            .map(|h| h.project_id.clone())
            .or_else(|| rec_opt.and_then(|r| r.project_id.clone()));
        let slug = hmap
            .map(|h| h.slug.clone())
            .filter(|s| !s.is_empty());
        let mod_id = mod_meta.and_then(|m| m.mod_id.clone());
        let raw_name = hmap
            .and_then(|h| h.raw_name.clone())
            .or_else(|| mod_meta.and_then(|m| m.name.clone()));

        specs.push(ItemLookupSpec {
            manifest_key: w.manifest_key.clone(),
            category: w.file.category.clone(),
            source,
            project_id,
            slug,
            mod_id,
            raw_name,
        });
    }

    let lookup_inputs: Vec<_> = specs
        .iter()
        .map(|s| {
            (
                s.category.clone(),
                s.source.clone(),
                s.project_id.clone(),
                s.slug.clone(),
                s.mod_id.clone(),
                s.raw_name.clone(),
            )
        })
        .collect();

    let mut local_results = tokio::task::spawn_blocking(move || {
        let Ok(db) = TranslationDb::open_default() else {
            return vec![None; lookup_inputs.len()];
        };
        lookup_inputs
            .iter()
            .map(|(cat, src, pid, slug, mid, rname)| {
                db.lookup(
                    cat,
                    src.as_deref(),
                    pid.as_deref(),
                    slug.as_deref(),
                    mid.as_deref(),
                    rname.as_deref(),
                )
            })
            .collect()
    })
    .await
    .unwrap_or_else(|_| vec![None; specs.len()]);

    // Fast-path: local dictionary lookup only.
    // Online /resolve queries have been completely removed in favor of local consensus dictionary.

    // Step 7: Persist discovered project IDs, hashes, and localized names into assets.json
    let mut updated_count = 0usize;
    for ((w, spec), found) in work_items
        .iter()
        .zip(specs.iter())
        .zip(local_results)
    {
        let hmap = all_hash_map.get(&w.sha1);
        let mod_meta = mod_meta_map.get(&w.manifest_key);
        let zh_short = found
            .map(|f| f.zh_short_name)
            .or_else(|| mod_meta.and_then(|m| m.zh_name_embedded.as_deref().map(extract_short_zh_name)))
            .filter(|s| !s.is_empty());

        let record = manifest
            .assets
            .entry(w.manifest_key.clone())
            .or_insert_with(|| AssetRecord {
                category: w.file.category.clone(),
                source_type: AssetSourceType::LocalImport,
                project_id: None,
                version_id: None,
                download_url: None,
                hash_sha1: Some(w.sha1.clone()),
                hash_sha512: w.sha512.clone(),
                size: Some(w.file.size),
                enabled: w.file.enabled,
                managed_by_modpack: false,
                original_filename: Some(strip_known_zh_prefixes(&w.file.base_filename)),
                original_rel_path: None,
                display_name_zh: None,
            });

        let mut changed = false;
        if record.hash_sha1.is_none() {
            record.hash_sha1 = Some(w.sha1.clone());
            changed = true;
        }
        if record.hash_sha512.is_none() && w.sha512.is_some() {
            record.hash_sha512 = w.sha512.clone();
            changed = true;
        }
        if record.project_id.is_none() {
            if let Some(pid) = &spec.project_id {
                record.project_id = Some(pid.clone());
                if let Some(h) = hmap {
                    record.source_type = if h.source.eq_ignore_ascii_case("modrinth") {
                        AssetSourceType::Modrinth
                    } else if h.source.eq_ignore_ascii_case("curseforge") {
                        AssetSourceType::CurseForge
                    } else {
                        record.source_type.clone()
                    };
                    if record.version_id.is_none() {
                        record.version_id = h.version_id.clone();
                    }
                }
                changed = true;
            }
        }
        if record.original_filename.is_none() {
            record.original_filename = Some(strip_known_zh_prefixes(&w.file.base_filename));
            changed = true;
        }
        if zh_short.is_some() && record.display_name_zh != zh_short {
            record.display_name_zh = zh_short;
            changed = true;
        }
        if changed {
            updated_count += 1;
        }
    }

    if updated_count > 0 {
        let _lock = MANIFEST_LOCK.lock().await;
        if let Ok(json_str) = serde_json::to_string_pretty(&manifest) {
            let _ = write_file_atomic(&assets_path, json_str.as_bytes()).await;
        }
        let _ = app.emit(
            "instance-translations-updated",
            serde_json::json!({
                "instanceId": instance_id,
                "updatedCount": updated_count,
            }),
        );
    }

    Ok(updated_count)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenamePreviewItem {
    pub category: AssetCategory,
    pub current_rel_path: String,
    pub target_rel_path: String,
    pub current_filename: String,
    pub target_filename: String,
    pub original_filename: String,
    pub zh_name: Option<String>,
    pub will_change: bool,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameApplyResult {
    pub renamed_count: usize,
    pub skipped_count: usize,
    pub failed: Vec<String>,
}

/// Pure planning helper parameterized over `instance_dir`, `settings`, and `TranslationDb`
/// so both Tauri commands and unit tests execute the exact same algorithm.
pub async fn build_rename_plan_for_instance(
    instance_dir: &Path,
    instance_id: &str,
    settings: &LauncherSettings,
    db: &TranslationDb,
    restore_original: bool,
    categories_filter: Option<&HashSet<AssetCategory>>,
) -> Result<Vec<RenamePreviewItem>, String> {
    let scanned = scan_instance_resource_files(instance_dir).await;
    if scanned.is_empty() {
        return Ok(Vec::new());
    }

    let assets_path = instance_dir.join("assets.json");
    let mut manifest: AssetManifest = if let Ok(content) = tokio::fs::read_to_string(&assets_path).await {
        serde_json::from_str(&content).unwrap_or_default()
    } else {
        AssetManifest::default()
    };
    let (_, manifest_dirty) = sanitize_polluted_modpack_manifest(instance_dir, &mut manifest).await;
    if manifest_dirty {
        if let Ok(json) = serde_json::to_string_pretty(&manifest) {
            let _ = tokio::fs::write(&assets_path, json).await;
        }
    }

    let (mc_version, loader) = read_instance_mc_and_loader(instance_dir, instance_id).await;

    // Load cached mod metadata if present
    let mod_cache = {
        let base_dir_opt = instance_dir
            .parent()
            .and_then(|p| p.parent())
            .map(Path::to_path_buf);
        if let Some(base_dir) = base_dir_opt {
            tokio::task::spawn_blocking(move || ModParser::new(&base_dir).load_all_cache())
                .await
                .unwrap_or_default()
        } else {
            HashMap::new()
        }
    };

    let active_template = if restore_original {
        "{stem}{ext}".to_string()
    } else {
        resolve_active_template(settings)
    };

    // Track simulated target filenames per parent directory to avoid batch collisions
    let mut occupied_by_dir: HashMap<PathBuf, HashSet<String>> = HashMap::new();
    for f in &scanned {
        if let Some(parent) = f.disk_path.parent() {
            occupied_by_dir
                .entry(parent.to_path_buf())
                .or_default()
                .insert(f.base_filename.clone());
        }
    }

    let mut plan = Vec::new();
    for file in scanned {
        if let Some(filter) = categories_filter {
            if !filter.contains(&file.category) {
                continue;
            }
        }

        if !restore_original && !should_rename_category(settings, &file.category) {
            continue;
        }

        let manifest_key = find_record_key(&manifest, &file);
        let record_opt = manifest_key.as_ref().and_then(|k| manifest.assets.get(k));

        let is_mod = file.category == AssetCategory::Mod;
        let original_enabled_name = record_opt
            .and_then(|r| r.original_filename.clone())
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| strip_known_zh_prefixes(&file.base_filename));
        let clean_original = {
            let s = sanitize_filename_component(&original_enabled_name, is_mod);
            if s.is_empty() {
                file.base_filename.clone()
            } else {
                s
            }
        };

        let current_disk_name = file
            .disk_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&file.base_filename)
            .to_string();

        if restore_original {
            let desired_base = clean_original.clone();
            let desired_disk_name = if file.enabled {
                desired_base.clone()
            } else {
                format!("{}.disable", desired_base)
            };
            let will_change = desired_disk_name != current_disk_name;
            let parent_dir = file.rel_path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
            let target_rel_path = if parent_dir.is_empty() {
                desired_disk_name.clone()
            } else {
                format!("{}/{}", parent_dir, desired_disk_name)
            };
            plan.push(RenamePreviewItem {
                category: file.category,
                current_rel_path: file.rel_path,
                target_rel_path,
                current_filename: current_disk_name,
                target_filename: desired_disk_name,
                original_filename: clean_original,
                zh_name: record_opt.and_then(|r| r.display_name_zh.clone()),
                will_change,
                reason: if will_change {
                    "restore_original".to_string()
                } else {
                    "unchanged".to_string()
                },
            });
            continue;
        }

        let hash_mapping = record_opt
            .and_then(|r| r.hash_sha1.as_deref())
            .and_then(|sha| db.get_hash_mapping(sha));

        let cache_key = build_mod_cache_key(&file.base_filename, file.size, file.mtime, &loader);
        let mod_meta = mod_cache.get(&cache_key);

        let source_str = hash_mapping
            .as_ref()
            .map(|h| h.source.as_str())
            .or_else(|| {
                record_opt.and_then(|r| match r.source_type {
                    AssetSourceType::Modrinth => Some("modrinth"),
                    AssetSourceType::CurseForge => Some("curseforge"),
                    _ => None,
                })
            });
        let pid_str = hash_mapping
            .as_ref()
            .map(|h| h.project_id.as_str())
            .or_else(|| record_opt.and_then(|r| r.project_id.as_deref()));
        let slug_str = hash_mapping
            .as_ref()
            .map(|h| h.slug.as_str())
            .filter(|s| !s.is_empty());
        let mod_id_str = mod_meta.and_then(|m| m.mod_id.as_deref());
        let raw_name_str = hash_mapping
            .as_ref()
            .and_then(|h| h.raw_name.as_deref())
            .or_else(|| mod_meta.and_then(|m| m.name.as_deref()));

        let lookup = db.lookup(
            &file.category,
            source_str,
            pid_str,
            slug_str,
            mod_id_str,
            raw_name_str,
        );

        let zh_full = lookup
            .as_ref()
            .map(|l| l.zh_name.clone())
            .or_else(|| record_opt.and_then(|r| r.display_name_zh.clone()))
            .or_else(|| mod_meta.and_then(|m| m.zh_name_embedded.clone()));
        let zh_short = lookup
            .as_ref()
            .map(|l| l.zh_short_name.clone())
            .or_else(|| zh_full.as_deref().map(extract_short_zh_name).filter(|s| !s.is_empty()));

        let ctx = ResourceNamingContext {
            original_filename: clean_original.clone(),
            category: file.category.clone().into(),
            zh_name: zh_short.clone(),
            zh_full_name: zh_full,
            en_name: raw_name_str.map(str::to_string),
            slug: slug_str.map(str::to_string),
            version: mod_meta.and_then(|m| m.version.clone()),
            mc_version: if mc_version.is_empty() {
                None
            } else {
                Some(mc_version.clone())
            },
            loader: if loader.is_empty() {
                None
            } else {
                Some(loader.clone())
            },
            source: source_str.map(str::to_string),
        };

        let mut desired_base = render_resource_filename(&active_template, &ctx);

        // Collision resolution excluding current file's own base name
        if desired_base != file.base_filename {
            if let Some(parent_path) = file.disk_path.parent() {
                let occ = occupied_by_dir.entry(parent_path.to_path_buf()).or_default();
                occ.remove(&file.base_filename);
                if occ.contains(&desired_base) {
                    desired_base = resolve_unique_filename_in_dir(
                        parent_path,
                        &desired_base,
                        &[&file.base_filename],
                    );
                    while occ.contains(&desired_base) {
                        let (st, ex) = crate::core::resource_naming::split_stem_and_ext(&desired_base);
                        desired_base = format!("{} (2){}", st, ex);
                    }
                }
                occ.insert(desired_base.clone());
            }
        }

        let desired_disk_name = if file.enabled {
            desired_base.clone()
        } else {
            format!("{}.disable", desired_base)
        };

        let will_change = desired_disk_name != current_disk_name;
        let reason = if !will_change {
            if zh_short.is_none()
                && (active_template.contains("{zh}") || active_template.contains("{zhFull}"))
            {
                "missing_zh_fallback".to_string()
            } else {
                "unchanged".to_string()
            }
        } else {
            "apply_template".to_string()
        };

        let parent_dir = file.rel_path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
        let target_rel_path = if parent_dir.is_empty() {
            desired_disk_name.clone()
        } else {
            format!("{}/{}", parent_dir, desired_disk_name)
        };

        plan.push(RenamePreviewItem {
            category: file.category,
            current_rel_path: file.rel_path,
            target_rel_path,
            current_filename: current_disk_name,
            target_filename: desired_disk_name,
            original_filename: clean_original,
            zh_name: zh_short,
            will_change,
            reason,
        });
    }

    Ok(plan)
}

#[tauri::command]
pub async fn preview_resource_renames(
    instance_id: String,
    restore_original: Option<bool>,
    categories: Option<Vec<AssetCategory>>,
) -> Result<Vec<RenamePreviewItem>, String> {
    if !is_safe_single_name(&instance_id) {
        return Err("Invalid instance ID".to_string());
    }
    let instance_dir = get_minecraft_base().join("versions").join(&instance_id);
    if !tokio::fs::try_exists(&instance_dir).await.unwrap_or(false) {
        return Err(format!("Instance not found: {}", instance_id));
    }

    let settings = get_launcher_settings().await;
    let restore = restore_original.unwrap_or(false);
    if !settings.resource_naming.enable_online_translation && !restore {
        return Ok(Vec::new());
    }
    let filter_set = categories.map(|v| v.into_iter().collect::<HashSet<_>>());
    let iid = instance_id.clone();

    let db = tokio::task::spawn_blocking(TranslationDb::open_default)
        .await
        .map_err(|e| e.to_string())??;

    build_rename_plan_for_instance(
        &instance_dir,
        &iid,
        &settings,
        &db,
        restore,
        filter_set.as_ref(),
    )
    .await
}

/// Execute atomic file renames and update `assets.json` keys while preserving `original_filename` & `original_rel_path`.
pub async fn execute_rename_plan_in_instance(
    instance_dir: &Path,
    plan: &[RenamePreviewItem],
    selected_rel_paths: Option<&HashSet<String>>,
) -> Result<RenameApplyResult, String> {
    let assets_path = instance_dir.join("assets.json");
    let mut manifest: AssetManifest = if tokio::fs::try_exists(&assets_path).await.unwrap_or(false) {
        let content = tokio::fs::read_to_string(&assets_path)
            .await
            .map_err(|e| format!("Failed to read assets.json: {}", e))?;
        serde_json::from_str(&content).unwrap_or_default()
    } else {
        AssetManifest::default()
    };

    let mut completed_moves: Vec<(PathBuf, PathBuf)> = Vec::new();
    let mut skipped_count = 0usize;
    let mut failed = Vec::new();
    let mut applied_items: Vec<&RenamePreviewItem> = Vec::new();

    for item in plan {
        if let Some(sel) = selected_rel_paths {
            if !sel.contains(&item.current_rel_path) {
                skipped_count += 1;
                continue;
            }
        }
        if !item.will_change {
            skipped_count += 1;
            continue;
        }

        let src = instance_dir.join(&item.current_rel_path);
        let dst = instance_dir.join(&item.target_rel_path);
        if !src.starts_with(instance_dir) || !dst.starts_with(instance_dir) {
            failed.push(format!("{}: unsafe path", item.current_rel_path));
            continue;
        }
        if !tokio::fs::try_exists(&src).await.unwrap_or(false) {
            failed.push(format!("{}: source file missing", item.current_rel_path));
            continue;
        }
        if tokio::fs::try_exists(&dst).await.unwrap_or(false) {
            failed.push(format!("{}: destination already exists", item.target_rel_path));
            continue;
        }

        if let Err(e) = tokio::fs::rename(&src, &dst).await {
            // Rollback previously completed moves in reverse order to avoid split state
            for (moved_src, moved_dst) in completed_moves.iter().rev() {
                let _ = tokio::fs::rename(moved_dst, moved_src).await;
            }
            return Err(format!(
                "Failed renaming '{}' -> '{}': {} (rolled back {} file(s))",
                item.current_rel_path,
                item.target_rel_path,
                e,
                completed_moves.len()
            ));
        }

        completed_moves.push((src, dst));
        applied_items.push(item);
    }

    // Migrate keys in assets.json
    for item in &applied_items {
        let old_enabled_key = item.current_rel_path.trim_end_matches(".disable").to_string();
        let new_enabled_key = item.target_rel_path.trim_end_matches(".disable").to_string();

        let existing = manifest
            .assets
            .remove(&item.current_rel_path)
            .or_else(|| manifest.assets.remove(&old_enabled_key));

        let is_enabled = !item.target_rel_path.ends_with(".disable");
        let updated_record = if let Some(mut rec) = existing {
            if rec.original_filename.is_none() {
                rec.original_filename = Some(item.original_filename.clone());
            }
            if rec.original_rel_path.is_none() {
                rec.original_rel_path = Some(old_enabled_key.clone());
            }
            if item.zh_name.is_some() {
                rec.display_name_zh = item.zh_name.clone();
            }
            rec.enabled = is_enabled;
            rec
        } else {
            AssetRecord {
                category: item.category.clone(),
                source_type: AssetSourceType::LocalImport,
                project_id: None,
                version_id: None,
                download_url: None,
                hash_sha1: None,
                hash_sha512: None,
                size: None,
                enabled: is_enabled,
                managed_by_modpack: false,
                original_filename: Some(item.original_filename.clone()),
                original_rel_path: Some(old_enabled_key),
                display_name_zh: item.zh_name.clone(),
            }
        };

        manifest.assets.insert(new_enabled_key, updated_record);
    }

    if !applied_items.is_empty() {
        let json_str = serde_json::to_string_pretty(&manifest)
            .map_err(|e| format!("Failed to serialize assets.json: {}", e))?;
        if let Err(e) = write_file_atomic(&assets_path, json_str.as_bytes()).await {
            for (moved_src, moved_dst) in completed_moves.iter().rev() {
                let _ = tokio::fs::rename(moved_dst, moved_src).await;
            }
            return Err(format!("Failed writing assets.json (rolled back moves): {}", e));
        }
    }

    Ok(RenameApplyResult {
        renamed_count: applied_items.len(),
        skipped_count,
        failed,
    })
}

#[tauri::command]
pub async fn apply_resource_renames(
    app: AppHandle,
    instance_id: String,
    restore_original: Option<bool>,
    categories: Option<Vec<AssetCategory>>,
    items: Option<Vec<String>>,
) -> Result<RenameApplyResult, String> {
    if !is_safe_single_name(&instance_id) {
        return Err("Invalid instance ID".to_string());
    }

    let running_instances = app.state::<RunningInstances>();
    if running_instances.is_instance_running(&instance_id).await {
        return Err("Instance is currently running; stop the game before batch renaming".to_string());
    }

    let instance_dir = get_minecraft_base().join("versions").join(&instance_id);
    if !tokio::fs::try_exists(&instance_dir).await.unwrap_or(false) {
        return Err(format!("Instance not found: {}", instance_id));
    }

    let inst_lock = get_instance_lock(&instance_id);
    let _inst_guard = inst_lock.lock().await;
    let _manifest_guard = MANIFEST_LOCK.lock().await;

    let settings = get_launcher_settings().await;
    let restore = restore_original.unwrap_or(false);
    if !settings.resource_naming.enable_online_translation && !restore {
        return Ok(RenameApplyResult {
            renamed_count: 0,
            skipped_count: 0,
            failed: Vec::new(),
        });
    }
    let filter_set = categories.map(|v| v.into_iter().collect::<HashSet<_>>());
    let iid = instance_id.clone();

    let db = tokio::task::spawn_blocking(TranslationDb::open_default)
        .await
        .map_err(|e| e.to_string())??;

    let plan = build_rename_plan_for_instance(
        &instance_dir,
        &iid,
        &settings,
        &db,
        restore,
        filter_set.as_ref(),
    )
    .await?;

    let selected_set = items.map(|v| v.into_iter().collect::<HashSet<_>>());
    let result = execute_rename_plan_in_instance(&instance_dir, &plan, selected_set.as_ref()).await?;

    if result.renamed_count > 0 {
        let _ = app.emit(
            "instance-translations-updated",
            serde_json::json!({
                "instanceId": instance_id,
                "updatedCount": result.renamed_count,
            }),
        );
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::settings::{FilenameFormatMode, FilenamePreset, ResourceNamingSettings};
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_batch_rename_apply_and_restore_preserves_original_and_disable_state() {
        let dir = tempdir().unwrap();
        let inst_dir = dir.path().join("versions").join("test_inst");
        let mods_dir = inst_dir.join("mods");
        tokio::fs::create_dir_all(&mods_dir).await.unwrap();

        tokio::fs::write(mods_dir.join("create-1.21.1-6.0.4.jar"), b"dummy-create")
            .await
            .unwrap();
        tokio::fs::write(mods_dir.join("jei-1.21.1-19.21.0.jar.disable"), b"dummy-jei")
            .await
            .unwrap();

        let mut manifest = AssetManifest::default();
        manifest.assets.insert(
            "mods/create-1.21.1-6.0.4.jar".to_string(),
            AssetRecord {
                category: AssetCategory::Mod,
                source_type: AssetSourceType::Modrinth,
                project_id: Some("LNytGkhc".to_string()),
                version_id: None,
                download_url: None,
                hash_sha1: None,
                hash_sha512: None,
                size: Some(12),
                enabled: true,
                managed_by_modpack: true,
                original_filename: None,
                original_rel_path: None,
                display_name_zh: None,
            },
        );
        manifest.assets.insert(
            "mods/jei-1.21.1-19.21.0.jar".to_string(),
            AssetRecord {
                category: AssetCategory::Mod,
                source_type: AssetSourceType::CurseForge,
                project_id: Some("238222".to_string()),
                version_id: None,
                download_url: None,
                hash_sha1: None,
                hash_sha512: None,
                size: Some(9),
                enabled: false,
                managed_by_modpack: false,
                original_filename: None,
                original_rel_path: None,
                display_name_zh: None,
            },
        );
        tokio::fs::write(
            inst_dir.join("assets.json"),
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .await
        .unwrap();

        let db = TranslationDb::open_at(dir.path().join("translations.db")).unwrap();
        db.seed_builtin_consensus().unwrap();
        db.upsert_entries_batch(&[
            TranslationEntry {
                category: "mod".to_string(),
                cf_project_id: Some("328085".to_string()),
                cf_slug: Some("create".to_string()),
                mr_project_id: Some("LNytGkhc".to_string()),
                mr_slug: Some("create".to_string()),
                mod_ids: "create".to_string(),
                raw_name: "Create".to_string(),
                zh_name: "机械动力 (Create)".to_string(),
                zh_short_name: "机械动力".to_string(),
                zh_aliases: String::new(),
                zh_summary: String::new(),
                wiki_id: 2973,
                trust_level: crate::core::translation::TRUST_VERIFIED,
                source_tag: "consensus".to_string(),
                updated_at: 1,
            },
            TranslationEntry {
                category: "mod".to_string(),
                cf_project_id: Some("238222".to_string()),
                cf_slug: Some("jei".to_string()),
                mr_project_id: Some("u6dRKJwZ".to_string()),
                mr_slug: Some("jei".to_string()),
                mod_ids: "jei".to_string(),
                raw_name: "Just Enough Items (JEI)".to_string(),
                zh_name: "JEI物品管理器".to_string(),
                zh_short_name: "JEI物品管理器".to_string(),
                zh_aliases: String::new(),
                zh_summary: String::new(),
                wiki_id: 459,
                trust_level: crate::core::translation::TRUST_VERIFIED,
                source_tag: "consensus".to_string(),
                updated_at: 1,
            },
        ])
        .unwrap();

        let settings = LauncherSettings {
            ui_language: Some("zh-CN".to_string()),
            resource_naming: ResourceNamingSettings {
                filename_mode: FilenameFormatMode::Preset,
                filename_preset: FilenamePreset::BracketPrefix,
                ..Default::default()
            },
            ..Default::default()
        };

        let plan = build_rename_plan_for_instance(&inst_dir, "test_inst", &settings, &db, false, None)
            .await
            .unwrap();
        assert_eq!(plan.len(), 2);
        let apply_res = execute_rename_plan_in_instance(&inst_dir, &plan, None)
            .await
            .unwrap();
        assert_eq!(apply_res.renamed_count, 2);

        assert!(mods_dir.join("[机械动力] create-1.21.1-6.0.4.jar").exists());
        assert!(mods_dir.join("[JEI物品管理器] jei-1.21.1-19.21.0.jar.disable").exists());

        // Restore originals
        let restore_plan = build_rename_plan_for_instance(&inst_dir, "test_inst", &settings, &db, true, None)
            .await
            .unwrap();
        let restore_res = execute_rename_plan_in_instance(&inst_dir, &restore_plan, None)
            .await
            .unwrap();
        assert_eq!(restore_res.renamed_count, 2);
        assert!(mods_dir.join("create-1.21.1-6.0.4.jar").exists());
        assert!(mods_dir.join("jei-1.21.1-19.21.0.jar.disable").exists());

        let saved_manifest: AssetManifest = serde_json::from_str(
            &tokio::fs::read_to_string(inst_dir.join("assets.json"))
                .await
                .unwrap(),
        )
        .unwrap();
        let create_rec = saved_manifest
            .assets
            .get("mods/create-1.21.1-6.0.4.jar")
            .unwrap();
        assert_eq!(
            create_rec.original_filename.as_deref(),
            Some("create-1.21.1-6.0.4.jar")
        );
        assert!(create_rec.managed_by_modpack);
    }

    #[test]
    fn test_polluted_modpack_project_id_is_sanitized_and_isolated() {
        let dir = tempdir().unwrap();
        let db = TranslationDb::open_at(dir.path().join("translations.db")).unwrap();

        // Simulate legacy CurseForge modpack install stamping modpack project_id ("686977") on every mod
        let mut manifest = AssetManifest::default();
        for fname in ["mod-a-1.0.jar", "mod-b-2.0.jar"] {
            manifest.assets.insert(
                format!("mods/{}", fname),
                AssetRecord {
                    category: AssetCategory::Mod,
                    source_type: AssetSourceType::CurseForge,
                    project_id: Some("686977".to_string()),
                    version_id: None,
                    download_url: None,
                    hash_sha1: None,
                    hash_sha512: None,
                    size: Some(100),
                    enabled: true,
                    managed_by_modpack: true,
                    original_filename: None,
                    original_rel_path: None,
                    display_name_zh: None,
                },
            );
        }
        // Individual non-modpack or unique-ID mod should remain untouched
        manifest.assets.insert(
            "mods/create-1.20.1.jar".to_string(),
            AssetRecord {
                category: AssetCategory::Mod,
                source_type: AssetSourceType::CurseForge,
                project_id: Some("328085".to_string()),
                version_id: None,
                download_url: None,
                hash_sha1: None,
                hash_sha512: None,
                size: Some(200),
                enabled: true,
                managed_by_modpack: false,
                original_filename: None,
                original_rel_path: None,
                display_name_zh: None,
            },
        );

        // Simulate poisoned TRUST_USER row written under modpack project_id + a real mod TRUST_USER row
        db.upsert_entries_batch(&[
            TranslationEntry {
                category: "mod".to_string(),
                cf_project_id: Some("686977".to_string()),
                cf_slug: None,
                mr_project_id: None,
                mr_slug: None,
                mod_ids: String::new(),
                raw_name: "Mod A".to_string(),
                zh_name: "机械动力 (Create)".to_string(),
                zh_short_name: "机械动力".to_string(),
                zh_aliases: String::new(),
                zh_summary: String::new(),
                wiki_id: 0,
                trust_level: crate::core::translation::TRUST_USER,
                source_tag: "user_custom".to_string(),
                updated_at: 0,
            },
            TranslationEntry {
                category: "mod".to_string(),
                cf_project_id: Some("328085".to_string()),
                cf_slug: Some("create".to_string()),
                mr_project_id: None,
                mr_slug: None,
                mod_ids: "create".to_string(),
                raw_name: "Create".to_string(),
                zh_name: "机械动力 (Create)".to_string(),
                zh_short_name: "机械动力".to_string(),
                zh_aliases: String::new(),
                zh_summary: String::new(),
                wiki_id: 0,
                trust_level: crate::core::translation::TRUST_USER,
                source_tag: "user_custom".to_string(),
                updated_at: 0,
            },
        ])
        .unwrap();

        let (polluted, changed) =
            scrub_polluted_modpack_manifest_records(Some("686977"), &mut manifest);
        assert!(changed);
        assert!(polluted.contains("686977"));
        assert_eq!(manifest.assets["mods/mod-a-1.0.jar"].project_id, None);
        assert_eq!(manifest.assets["mods/mod-b-2.0.jar"].project_id, None);
        assert_eq!(
            manifest.assets["mods/create-1.20.1.jar"].project_id.as_deref(),
            Some("328085")
        );

        let deleted = db.purge_polluted_user_entries(&polluted).unwrap();
        assert_eq!(deleted, 1);

        // Polluted modpack ID no longer resolves any translation for unrelated mods
        assert!(db
            .lookup(
                &AssetCategory::Mod,
                Some("CurseForge"),
                Some("686977"),
                None,
                None,
                Some("Mod B"),
            )
            .is_none());
        // Legitimate individual mod translation stays intact
        let legitimate = db
            .lookup(
                &AssetCategory::Mod,
                Some("CurseForge"),
                Some("328085"),
                Some("create"),
                Some("create"),
                Some("Create"),
            )
            .unwrap();
        assert_eq!(legitimate.zh_short_name, "机械动力");
    }
}
