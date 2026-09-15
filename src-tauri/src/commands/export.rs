use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::fs;
use tauri::Emitter;
use futures::stream::StreamExt;

use crate::core::hash::{calculate_curseforge_hash, calculate_file_hashes};
use crate::core::manager::get_installed_mods;
use crate::core::mojang::get_minecraft_base;
use crate::core::utils::{copy_dir_all, copy_jar_if_exists_with_logging, get_http_client};


#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ExportRequest {
    pub version_id: String,
    pub name: String,
    pub version: String,
    pub author: String,
    pub format: String, // "modrinth" or "curseforge"
    pub include_saves: bool,
    pub output_path: String,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ExportProgress {
    pub step: String,
    pub translation_key: String,
    pub current: Option<usize>,
    pub total_items: Option<usize>,
    pub progress: u32,
    pub total: u32,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ExportAnalysisResult {
    pub matched_mods: Vec<MatchedMod>,
    pub unmatched_mods: Vec<UnmatchedMod>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MatchedMod {
    pub filename: String,
    pub mod_id: Option<String>,
    pub mr_json: Option<serde_json::Value>,
    pub cf_project_id: Option<u32>,
    pub cf_file_id: Option<u32>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UnmatchedMod {
    pub filename: String,
    pub mod_id: Option<String>,
    pub name: Option<String>,
    pub size: u64,
}

// Modrinth API Responses
#[derive(Deserialize, Debug)]
pub struct MrVersionFileResponse {
    pub files: Vec<MrFile>,
}

#[derive(Deserialize, Debug)]
pub struct MrFile {
    pub hashes: HashMap<String, String>,
    pub url: String,
    pub size: u64,
}

#[derive(Deserialize, Debug)]
pub struct MrFallbackVersion {
    pub id: String,
    pub name: String,
    pub version_number: Option<String>,
    pub game_versions: Vec<String>,
    pub loaders: Vec<String>,
    pub files: Vec<MrFile>,
}

// CurseForge API Responses
#[derive(Serialize)]
pub struct CfFingerprintsRequest {
    pub fingerprints: Vec<u32>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CfFingerprintsResponse {
    pub data: CfFingerprintsData,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CfFingerprintsData {
    pub exact_matches: Vec<CfExactMatch>,
    pub exact_fingerprints: Vec<u32>,
}

#[derive(Deserialize, Debug)]
pub struct CfExactMatch {
    pub id: u32,
    pub file: CfFileMatch,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CfFileMatch {
    pub id: u32,
    pub mod_id: u32,
}


async fn get_instance_env(instance_dir: &std::path::Path, version_id: &str) -> Result<(String, String, String), String> {
    let details = crate::core::manager::get_instance_details(version_id.to_string()).await?;
    
    let mc_version = details.mc_version.clone();
    let mut loader_name = details.loader_type.to_lowercase();
    if loader_name == "vanilla" || loader_name.is_empty() {
        loader_name = "fabric".to_string(); // fallback
    }
    
    let mut loader_version = "latest".to_string();
    let base_dir = get_minecraft_base();
    
    let mut current_id = version_id.to_string();
    for _ in 0..5 {
        let json_path = base_dir.join("versions").join(&current_id).join(format!("{}.json", current_id));
        if let Ok(content) = tokio::fs::read_to_string(&json_path).await {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                // 1. Check arguments.game for explicit loader version flags
                if loader_version == "latest" {
                    if let Some(args) = json.get("arguments").and_then(|a| a.get("game")).and_then(|g| g.as_array()) {
                        for (i, val) in args.iter().enumerate() {
                            if let Some(s) = val.as_str() {
                                if s == "--fml.neoForgeVersion" || s == "--fml.forgeVersion" {
                                    if let Some(next_val) = args.get(i + 1).and_then(|v| v.as_str()) {
                                        loader_version = next_val.to_string();
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }

                // 2. Check libraries array for explicit loader dependencies
                if loader_version == "latest" {
                    if let Some(libs) = json.get("libraries").and_then(|l| l.as_array()) {
                        for lib in libs {
                            if let Some(name) = lib.get("name").and_then(|n| n.as_str()) {
                                if name.starts_with("net.neoforged:neoforge:") {
                                    let parts: Vec<&str> = name.split(':').collect();
                                    if parts.len() >= 3 { loader_version = parts[2].to_string(); break; }
                                } else if name.starts_with("net.minecraftforge:forge:") {
                                    let parts: Vec<&str> = name.split(':').collect();
                                    if parts.len() >= 3 {
                                        let ver = parts[2];
                                        if let Some(idx) = ver.find('-') {
                                            loader_version = ver[idx+1..].to_string();
                                        } else {
                                            loader_version = ver.to_string();
                                        }
                                        break;
                                    }
                                } else if name.starts_with("net.fabricmc:fabric-loader:") {
                                    let parts: Vec<&str> = name.split(':').collect();
                                    if parts.len() >= 3 { loader_version = parts[2].to_string(); break; }
                                }
                            }
                        }
                    }
                }

                // 3. Fallback to extracting from inherited IDs
                if loader_version == "latest" {
                    let lower = current_id.to_lowercase();
                    if lower.starts_with("fabric-loader-") {
                        let parts: Vec<&str> = current_id.split('-').collect();
                        if parts.len() >= 3 {
                            loader_version = parts[2].to_string();
                        }
                    } else if lower.starts_with("forge-") || lower.starts_with("neoforge-") {
                        let parts: Vec<&str> = current_id.split('-').collect();
                        if parts.len() >= 2 {
                            // Some IDs are forge-<loader>-<mc>, some are forge-<mc>-<loader>.
                            // We can check if parts[1] looks like MC version (e.g. starts with "1.")
                            if parts[1].starts_with("1.") && parts.len() >= 3 {
                                loader_version = parts[2].to_string();
                            } else {
                                loader_version = parts[1].to_string();
                            }
                        }
                    }
                }
                
                if loader_version != "latest" {
                    break;
                }

                let inherits_from = json.get("inheritsFrom").and_then(|v| v.as_str()).unwrap_or("");
                if inherits_from.is_empty() {
                    break;
                }
                current_id = inherits_from.to_string();
            } else {
                break;
            }
        } else {
            break;
        }
    }
    
    if loader_version == "latest" {
        if loader_name == "neoforge" {
            if let Ok(list) = crate::core::forge::get_neoforge_loaders(mc_version.clone()).await {
                if let Some(v) = list.versions.first() {
                    loader_version = v.version.clone();
                }
            }
        } else if loader_name == "forge" {
            if let Ok(list) = crate::core::forge::get_forge_loaders(mc_version.clone()).await {
                if let Some(v) = list.versions.first() {
                    loader_version = v.version.clone();
                }
            }
        } else if loader_name == "fabric" {
            if let Ok(list) = crate::core::fabric::get_fabric_loaders(mc_version.clone()).await {
                if let Some(v) = list.stable.first().or(list.unstable.first()) {
                    loader_version = v.clone();
                }
            }
        }
    }
    
    Ok((mc_version, loader_name, loader_version))
}

#[tauri::command]
pub async fn analyze_export_instance(
    window: tauri::Window,
    version_id: String,
    format: String,
) -> Result<ExportAnalysisResult, String> {
    tracing::info!("Analyzing export instance: {} ({})", version_id, format);

    let base_dir = get_minecraft_base();
    let instance_dir = base_dir.join("versions").join(&version_id);
    if !instance_dir.exists() {
        return Err(format!("Instance directory not found: {:?}", instance_dir));
    }

    let _ = window.emit("export-progress", ExportProgress {
        step: "Gathering mods".to_string(),
        translation_key: "instances.export.progress.gathering".to_string(),
        current: None,
        total_items: None,
        progress: 0,
        total: 100,
    });

    let mods = get_installed_mods(version_id.clone(), None).await?;
    let mods_dir = instance_dir.join("mods");

    let mut manifest = crate::models::instance::AssetManifest::default();
    let assets_path = instance_dir.join("assets.json");
    if let Ok(content) = tokio::fs::read_to_string(&assets_path).await {
        if let Ok(m) = serde_json::from_str::<crate::models::instance::AssetManifest>(&content) {
            manifest = m;
        }
    }

    let (mc_version, loader_name, _loader_version) =
        get_instance_env(&instance_dir, &version_id).await?;

    if format == "modrinth" {
        analyze_modrinth(&window, &mods, &mods_dir, &mc_version, &loader_name, manifest).await
    } else {
        analyze_curseforge(&window, &mods, &mods_dir, manifest).await
    }
}

async fn analyze_modrinth(
    window: &tauri::Window,
    mods: &[crate::core::manager::LocalModItem],
    mods_dir: &std::path::Path,
    mc_version: &str,
    loader_name: &str,
    manifest: crate::models::instance::AssetManifest,
) -> Result<ExportAnalysisResult, String> {
    let client = get_http_client();
    
    let active_items: Vec<ActiveModItem> = mods.iter()
        .filter(|m| m.enabled)
        .map(|m| (m.filename.clone(), m.mod_id.clone(), m.name.clone(), m.version.clone(), m.size))
        .collect();
        
    let total_mods = active_items.len();
    let completed = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));

    let results = futures::stream::iter(active_items)
        .map(|(m_filename, m_mod_id, m_name, m_version, m_size)| {
            let window_clone = window.clone();
            let completed_clone = completed.clone();
            let client_clone = client.clone();
            let mod_path = mods_dir.join(&m_filename);
            let mc_version = mc_version.to_string();
            let loader_name = loader_name.to_string();
            
            // Get asset record from manifest
            let asset_key = format!("mods/{}", m_filename);
            let asset_record = manifest.assets.get(&asset_key).cloned();
            
            async move {
                let current = completed_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                let _ = window_clone.emit("export-progress", ExportProgress {
                    step: format!("Hashing mod {}/{}", current, total_mods),
                    translation_key: "instances.export.progress.hashing".to_string(),
                    current: Some(current),
                    total_items: Some(total_mods),
                    progress: ((current as f32 / total_mods as f32) * 80.0) as u32,
                    total: 100,
                });

                if !mod_path.exists() { 
                    return Ok::<Option<Result<MatchedMod, UnmatchedMod>>, String>(None);
                }

                // 1. FAST PATH: If we have full metadata from assets.json, build mr_json instantly!
                if let Some(ref record) = asset_record {
                    if let (Some(sha1), Some(sha512), Some(url)) = (record.hash_sha1.clone(), record.hash_sha512.clone(), record.download_url.clone()) {
                        let mr_json = serde_json::json!({
                            "path": format!("mods/{}", m_filename),
                            "hashes": {
                                "sha1": sha1,
                                "sha512": sha512
                            },
                            "env": {
                                "client": "required",
                                "server": "required"
                            },
                            "downloads": [url],
                            "fileSize": record.size.unwrap_or(m_size)
                        });
                        
                        return Ok(Some(Ok(MatchedMod {
                            filename: m_filename,
                            mod_id: m_mod_id,
                            mr_json: Some(mr_json),
                            cf_project_id: None,
                            cf_file_id: None,
                        })));
                    }
                }

                // 2. PARTIAL PATH: If we only have hashes in assets.json, use them instead of reading the file
                let (sha1, sha512) = if let Some(ref record) = asset_record {
                    if let (Some(s1), Some(s512)) = (record.hash_sha1.clone(), record.hash_sha512.clone()) {
                        (s1, s512)
                    } else {
                        match calculate_file_hashes(&mod_path).await {
                            Ok(h) => h,
                            Err(_) => return Ok(None),
                        }
                    }
                } else {
                    match calculate_file_hashes(&mod_path).await {
                        Ok(h) => h,
                        Err(_) => return Ok(None),
                    }
                };
                
                // Try to query Modrinth API by SHA1
                let url = format!("https://api.modrinth.com/v2/version_file/{}?algorithm=sha1", sha1);
                if let Ok(resp) = client_clone.get(&url).send().await {
                    if resp.status().is_success() {
                        if let Ok(file_resp) = resp.json::<MrVersionFileResponse>().await {
                            if let Some(target_file) = file_resp.files.iter().find(|f| f.hashes.get("sha1").map(|s| s.as_str()) == Some(&sha1)) {
                                let mr_json = serde_json::json!({
                                    "path": format!("mods/{}", m_filename),
                                    "hashes": {
                                        "sha1": sha1,
                                        "sha512": sha512
                                    },
                                    "env": {
                                        "client": "required",
                                        "server": "required"
                                    },
                                    "downloads": [target_file.url.clone()],
                                    "fileSize": target_file.size
                                });
                                return Ok(Some(Ok(MatchedMod {
                                    filename: m_filename,
                                    mod_id: m_mod_id,
                                    mr_json: Some(mr_json),
                                    cf_project_id: None,
                                    cf_file_id: None,
                                })));
                            }
                        }
                    }
                }

                // Fallback: Query by mod_id (slug) if SHA1 failed
                if let Some(slug) = &m_mod_id {
                    let fallback_url = format!(
                        "https://api.modrinth.com/v2/project/{}/version?loaders=[%22{}%22]&game_versions=[%22{}%22]",
                        slug, loader_name, mc_version
                    );
                    if let Ok(resp) = client_clone.get(&fallback_url).send().await {
                        if resp.status().is_success() {
                            if let Ok(versions) = resp.json::<Vec<MrFallbackVersion>>().await {
                                if let Some(latest) = versions.first() {
                                    if let Some(target_file) = latest.files.first() {
                                        let mr_sha1 = target_file.hashes.get("sha1").cloned().unwrap_or_default();
                                        let mr_sha512 = target_file.hashes.get("sha512").cloned().unwrap_or_default();
                                        if !mr_sha1.is_empty() {
                                            let mr_json = serde_json::json!({
                                                "path": format!("mods/{}", m_filename),
                                                "hashes": {
                                                    "sha1": mr_sha1,
                                                    "sha512": mr_sha512
                                                },
                                                "env": {
                                                    "client": "required",
                                                    "server": "required"
                                                },
                                                "downloads": [target_file.url.clone()],
                                                "fileSize": target_file.size
                                            });
                                            return Ok(Some(Ok(MatchedMod {
                                                filename: m_filename,
                                                mod_id: m_mod_id.clone(),
                                                mr_json: Some(mr_json),
                                                cf_project_id: None,
                                                cf_file_id: None,
                                            })));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                // Unmatched
                Ok(Some(Err(UnmatchedMod {
                    filename: m_filename,
                    mod_id: m_mod_id,
                    name: m_name,
                    size: m_size,
                })))
            }
        })
        .buffer_unordered(32)
        .collect::<Vec<_>>()
        .await;

    let mut matched_mods = Vec::new();
    let mut unmatched_mods = Vec::new();

    for r in results {
        if let Ok(Some(res)) = r {
            match res {
                Ok(matched) => matched_mods.push(matched),
                Err(unmatched) => unmatched_mods.push(unmatched),
            }
        }
    }

    Ok(ExportAnalysisResult { matched_mods, unmatched_mods })
}

async fn analyze_curseforge(
    window: &tauri::Window,
    mods: &[crate::core::manager::LocalModItem],
    mods_dir: &std::path::Path,
    manifest: crate::models::instance::AssetManifest,
) -> Result<ExportAnalysisResult, String> {
    let client = get_http_client();
    
    let active_items: Vec<ActiveModItem> = mods.iter()
        .filter(|m| m.enabled)
        .map(|m| (m.filename.clone(), m.mod_id.clone(), m.name.clone(), m.version.clone(), m.size))
        .collect();
        
    let total_mods = active_items.len();
    
    // We will separate items into ones we already have matched via manifest, and ones we need to hash
    let mut pre_matched: Vec<MatchedMod> = Vec::new();
    let mut needs_hashing = Vec::new();
    
    for (m_filename, m_mod_id, m_name, m_version, m_size) in active_items {
        let asset_key = format!("mods/{}", m_filename);
        let mut matched = false;
        
        if let Some(record) = manifest.assets.get(&asset_key) {
            if record.source_type == crate::models::instance::AssetSourceType::CurseForge {
                if let (Some(pid_str), Some(vid_str)) = (&record.project_id, &record.version_id) {
                    if let (Ok(pid), Ok(vid)) = (pid_str.parse::<u32>(), vid_str.parse::<u32>()) {
                        pre_matched.push(MatchedMod {
                            filename: m_filename.clone(),
                            mod_id: m_mod_id.clone(),
                            mr_json: None,
                            cf_project_id: Some(pid),
                            cf_file_id: Some(vid),
                        });
                        matched = true;
                    }
                }
            }
        }
        
        if !matched {
            needs_hashing.push((m_filename, m_mod_id, m_name, m_version, m_size));
        }
    }
    
    let total_hash_needed = needs_hashing.len();
    let completed = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut file_map = HashMap::new(); // hash -> UnmatchedMod

    let results = futures::stream::iter(needs_hashing)
        .map(|(m_filename, m_mod_id, m_name, m_version, m_size)| {
            let window_clone = window.clone();
            let completed_clone = completed.clone();
            let mod_path = mods_dir.join(&m_filename);
            async move {
                let current = completed_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                let _ = window_clone.emit("export-progress", ExportProgress {
                    step: format!("Hashing mod {}/{}", current, total_hash_needed),
                    translation_key: "instances.export.progress.hashing".to_string(),
                    current: Some(current),
                    total_items: Some(total_hash_needed),
                    progress: ((current as f32 / total_hash_needed as f32) * 40.0) as u32,
                    total: 100,
                });

                if !mod_path.exists() { return None::<(u32, UnmatchedMod)>; }

                let hash = match calculate_curseforge_hash(&mod_path).await {
                    Ok(h) => h,
                    Err(_) => return None,
                };
                
                let unmatched = UnmatchedMod {
                    filename: m_filename,
                    mod_id: m_mod_id,
                    name: m_name,
                    size: m_size,
                };
                
                Some((hash, unmatched))
            }
        })
        .buffer_unordered(32)
        .collect::<Vec<_>>()
        .await;

    for r in results.into_iter().flatten() {
        file_map.insert(r.0, r.1);
    }

    let _ = window.emit("export-progress", ExportProgress {
        step: "Querying CurseForge...".to_string(),
        translation_key: "instances.export.progress.querying".to_string(),
        current: None,
        total_items: None,
        progress: 60,
        total: 100,
    });

    let hashes: Vec<u32> = file_map.keys().cloned().collect();
    let mut matched_mods = pre_matched;
    
    if !hashes.is_empty() {
        let req_body = CfFingerprintsRequest { fingerprints: hashes };
        
        let cf_url = crate::core::curseforge::build_cf_url("/fingerprints", None);
        if let Ok(req) = crate::core::curseforge::cf_request(client, reqwest::Method::POST, &cf_url) {
            let res = req.json(&req_body).send().await;
            if let Ok(resp) = res {
                if resp.status().is_success() {
                    if let Ok(match_data) = resp.json::<CfFingerprintsResponse>().await {
                        for (exact, fp) in match_data.data.exact_matches.into_iter().zip(match_data.data.exact_fingerprints) {
                            if let Some(unmatched) = file_map.remove(&fp) {
                                matched_mods.push(MatchedMod {
                                    filename: unmatched.filename,
                                    mod_id: unmatched.mod_id,
                                    mr_json: None,
                                    cf_project_id: Some(exact.file.mod_id),
                                    cf_file_id: Some(exact.file.id),
                                });
                            }
                        }
                    }
                }
            }
        }
    }
    
    let mut unmatched_mods = Vec::new();
    for (_, unmatched) in file_map {
        unmatched_mods.push(unmatched);
    }
    
    Ok(ExportAnalysisResult { matched_mods, unmatched_mods })
}

#[tauri::command]
pub async fn build_export_instance(
    window: tauri::Window,
    request: ExportRequest,
    analysis_result: ExportAnalysisResult,
) -> Result<String, String> {
    tracing::info!("Building export instance: {:?}", request);

    let base_dir = get_minecraft_base();
    let instance_dir = base_dir.join("versions").join(&request.version_id);
    if !instance_dir.exists() {
        return Err(format!("Instance directory not found: {:?}", instance_dir));
    }

    let temp_dir = std::env::temp_dir().join(format!("dawnland_export_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir)
        .await
        .map_err(|e| e.to_string())?;
        
    struct TempGuard(std::path::PathBuf);
    impl Drop for TempGuard {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let _guard = TempGuard(temp_dir.clone());

    let overrides_dir = temp_dir.join("overrides");
    fs::create_dir_all(&overrides_dir)
        .await
        .map_err(|e| e.to_string())?;

    let _ = window.emit("export-progress", ExportProgress {
        step: "Gathering overrides".to_string(),
        translation_key: "instances.export.progress.gathering".to_string(),
        current: None,
        total_items: None,
        progress: 70,
        total: 100,
    });

    // Copy configs, resourcepacks, shaderpacks, etc.
    let folders_to_copy = vec!["config", "resourcepacks", "shaderpacks", "defaultconfigs"];
    for folder in folders_to_copy {
        let src = instance_dir.join(folder);
        if src.exists() {
            let dst = overrides_dir.join(folder);
            if let Err(e) = copy_dir_all(&src, &dst).await {
                return Err(format!("Failed to copy {}: {}", folder, e));
            }
        }
    }
    
    if request.include_saves {
        let src = instance_dir.join("saves");
        if src.exists() {
            let dst = overrides_dir.join("saves");
            copy_dir_all(&src, &dst).await.map_err(|e| format!("Failed to copy saves: {}", e))?;
        }
    }

    let mods_dir = instance_dir.join("mods");
    let dst_mod_dir = overrides_dir.join("mods");
    if !analysis_result.unmatched_mods.is_empty() {
        let _ = fs::create_dir_all(&dst_mod_dir).await;
        for unmatched in analysis_result.unmatched_mods {
            copy_jar_if_exists_with_logging(&mods_dir.join(&unmatched.filename), &dst_mod_dir.join(&unmatched.filename)).await;
        }
    }

    // Determine game version and loader
    let (mc_version, loader_name, loader_version) =
        get_instance_env(&instance_dir, &request.version_id).await?;

    let output_path = PathBuf::from(&request.output_path);

    if request.format == "modrinth" {
        let mut mr_files = Vec::new();
        for m in analysis_result.matched_mods {
            if let Some(json) = m.mr_json {
                mr_files.push(json);
            }
        }
        
        let mr_loader_key = match loader_name.as_str() {
            "fabric" => "fabric-loader",
            "quilt" => "quilt-loader",
            other => other,
        };

        let index = serde_json::json!({
            "formatVersion": 1,
            "game": "minecraft",
            "versionId": request.version,
            "name": request.name,
            "summary": "Exported from Dawnland Launcher",
            "files": mr_files,
            "dependencies": {
                "minecraft": mc_version,
                mr_loader_key: loader_version
            }
        });
    
        let index_str = serde_json::to_string_pretty(&index).unwrap();
        fs::write(temp_dir.join("modrinth.index.json"), index_str).await.map_err(|e| e.to_string())?;
    } else {
        let mut cf_files = Vec::new();
        for m in analysis_result.matched_mods {
            if let (Some(pid), Some(fid)) = (m.cf_project_id, m.cf_file_id) {
                cf_files.push(serde_json::json!({
                    "projectID": pid,
                    "fileID": fid,
                    "required": true
                }));
            }
        }
        
        let forge_version_str = format!("{}-{}", loader_name, loader_version);
    
        let manifest = serde_json::json!({
            "minecraft": {
                "version": mc_version,
                "modLoaders": [
                    {
                        "id": forge_version_str,
                        "primary": true
                    }
                ]
            },
            "manifestType": "minecraftModpack",
            "manifestVersion": 1,
            "name": request.name,
            "version": request.version,
            "author": request.author,
            "files": cf_files,
            "overrides": "overrides"
        });
    
        let manifest_str = serde_json::to_string_pretty(&manifest).unwrap();
        fs::write(temp_dir.join("manifest.json"), manifest_str).await.map_err(|e| e.to_string())?;
    }

    // Zip everything
    let _ = window.emit("export-progress", ExportProgress {
        step: "Zipping archive".to_string(),
        translation_key: "instances.export.progress.zipping".to_string(),
        current: None,
        total_items: None,
        progress: 90,
        total: 100,
    });

    let zip_file = std::fs::File::create(&output_path).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipWriter::new(zip_file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    for entry in walkdir::WalkDir::new(&temp_dir) {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        let name = path
            .strip_prefix(&temp_dir)
            .unwrap()
            .to_str()
            .unwrap()
            .replace("\\", "/");

        if path.is_file() {
            zip.start_file(name, options).map_err(|e| e.to_string())?;
            let mut f = std::fs::File::open(path).map_err(|e| e.to_string())?;
            std::io::copy(&mut f, &mut zip).map_err(|e| e.to_string())?;
        } else if !name.is_empty() {
            zip.add_directory(name, options).map_err(|e| e.to_string())?;
        }
    }

    zip.finish().map_err(|e| e.to_string())?;

    let _ = window.emit("export-progress", ExportProgress {
        step: "Finished".to_string(),
        translation_key: "instances.export.progress.finished".to_string(),
        current: None,
        total_items: None,
        progress: 100,
        total: 100,
    });

    Ok(request.output_path)
}

type ActiveModItem = (String, Option<String>, Option<String>, Option<String>, u64);

#[derive(serde::Serialize)]
#[serde(tag = "status")]
pub enum ResolveResponse {
    Matched { matched_mod: MatchedMod },
    Choices { choices: Vec<ResolveChoice> },
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveChoice {
    pub version_id: String,
    pub version_name: String,
    pub game_versions: Vec<String>,
    pub loaders: Vec<String>,
    pub matched_mod: MatchedMod,
}
#[tauri::command]
pub async fn resolve_manual_match(
    version_id: String,
    format: String,
    filename: String,
    project_id: String, // Slug or ID
) -> Result<ResolveResponse, String> {
    let client = get_http_client();
    let base_dir = get_minecraft_base();
    let instance_dir = base_dir.join("versions").join(&version_id);
    let (mc_version, loader_name, _) = get_instance_env(&instance_dir, &version_id).await?;

    if format == "modrinth" {
        let slug = project_id.trim().to_lowercase().replace(" ", "-");
        // Try strict match first
        let strict_url = format!(
            "https://api.modrinth.com/v2/project/{}/version?loaders=[%22{}%22]&game_versions=[%22{}%22]",
            slug, loader_name, mc_version
        );
        let mut versions = Vec::new();
        
        if let Ok(resp) = client.get(&strict_url).send().await {
            if resp.status().is_success() {
                if let Ok(text) = resp.text().await {
                    if let Ok(v) = serde_json::from_str::<Vec<MrFallbackVersion>>(&text) {
                        versions = v;
                    }
                }
            }
        }
        
        let mut target_slug = slug.clone();
        
        // If strict fails, try to use Modrinth Search API to find the real slug
        if versions.is_empty() {
            let search_query = urlencoding::encode(project_id.trim());
            let search_url = format!("https://api.modrinth.com/v2/search?query={}&limit=1", search_query);
            if let Ok(resp) = client.get(&search_url).send().await {
                if let Ok(json) = resp.json::<serde_json::Value>().await {
                    if let Some(hits) = json.get("hits").and_then(|h| h.as_array()) {
                        if let Some(first_hit) = hits.first() {
                            if let Some(hit_slug) = first_hit.get("slug").and_then(|s| s.as_str()) {
                                target_slug = hit_slug.to_string();
                                
                                // Retry strict match with new target_slug
                                let retry_strict_url = format!(
                                    "https://api.modrinth.com/v2/project/{}/version?loaders=[%22{}%22]&game_versions=[%22{}%22]",
                                    target_slug, loader_name, mc_version
                                );
                                if let Ok(retry_resp) = client.get(&retry_strict_url).send().await {
                                    if retry_resp.status().is_success() {
                                        if let Ok(text) = retry_resp.text().await {
                                            if let Ok(v) = serde_json::from_str::<Vec<MrFallbackVersion>>(&text) {
                                                versions = v;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        
        // If it's STILL empty (strict match failed even with correct slug),
        // fetch all versions for the target_slug so the user can manually pick.
        if versions.is_empty() {
            let all_url = format!("https://api.modrinth.com/v2/project/{}/version", target_slug);
            if let Ok(resp) = client.get(&all_url).send().await {
                if resp.status().is_success() {
                    if let Ok(text) = resp.text().await {
                        if let Ok(v) = serde_json::from_str::<Vec<MrFallbackVersion>>(&text) {
                            versions = v;
                        }
                    }
                } else if resp.status() == 404 {
                    return Err("instances.export.error.mr404".into());
                }
            }
        }
        
        if versions.is_empty() {
            return Err("instances.export.error.mrNoVersions".into());
        }
        
        // We could not find a hash match on Modrinth.
        // We will return all compatible versions as choices so the user can manually pick the closest match,
        // rather than silently assuming the first version is correct.
        
        // Build choices
        let mut choices = Vec::new();
        for v in versions.into_iter() {
            // Filter by loader to prevent Fabric/Forge mismatch
            let loaders = v.loaders.clone();
            let is_compatible = loaders.contains(&loader_name) 
                || (loader_name == "neoforge" && loaders.contains(&"forge".to_string()))
                || (loader_name == "forge" && loaders.contains(&"neoforge".to_string()));
            
            if !is_compatible {
                continue;
            }
            
            if let Some(target_file) = v.files.first() {
                let mr_sha1 = target_file.hashes.get("sha1").cloned().unwrap_or_default();
                let mr_sha512 = target_file.hashes.get("sha512").cloned().unwrap_or_default();
                if !mr_sha1.is_empty() {
                    let mr_json = serde_json::json!({
                        "path": format!("mods/{}", filename),
                        "hashes": {
                            "sha1": mr_sha1,
                            "sha512": mr_sha512
                        },
                        "env": {
                            "client": "required",
                            "server": "required"
                        },
                        "downloads": [target_file.url.clone()],
                        "fileSize": target_file.size
                    });
                    choices.push(ResolveChoice {
                        version_id: v.id.clone(),
                        version_name: format!("{} ({})", v.name, v.version_number.as_deref().unwrap_or("Unknown")),
                        game_versions: v.game_versions.clone(),
                        loaders: v.loaders.clone(),
                        matched_mod: MatchedMod {
                            filename: filename.clone(),
                            mod_id: Some(project_id.clone()),
                            mr_json: Some(mr_json),
                            cf_project_id: None,
                            cf_file_id: None,
                        }
                    });
                }
            }
            if choices.len() >= 20 { break; }
        }
        
        if choices.is_empty() {
            return Err("instances.export.error.invalidJson".into());
        }
        
        Ok(ResolveResponse::Choices { choices })

    } else {
        // CurseForge
        let mut cf_project_id: u32 = 0;
        if let Ok(id) = project_id.parse::<u32>() {
            cf_project_id = id;
        } else {
            // Try Search API
            let search_query = urlencoding::encode(project_id.trim());
            let search_url = crate::core::curseforge::build_cf_url(
                "/mods/search",
                Some(&format!("gameId=432&searchFilter={}&pageSize=1", search_query))
            );
            if let Ok(req) = crate::core::curseforge::cf_request(client, reqwest::Method::GET, &search_url) {
                if let Ok(resp) = req.send().await {
                    if resp.status().is_success() {
                        if let Ok(json) = resp.json::<serde_json::Value>().await {
                            if let Some(data_arr) = json.get("data").and_then(|d| d.as_array()) {
                                if let Some(first) = data_arr.first() {
                                    if let Some(id) = first.get("id").and_then(|i| i.as_u64()) {
                                        cf_project_id = id as u32;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        
        if cf_project_id == 0 {
            return Err("instances.export.error.cf404".into());
        }

        let loader_type = crate::core::curseforge::CfModLoader::from_str(&loader_name) as i32;
        let strict_url = crate::core::curseforge::build_cf_url(
            &format!("/mods/{}/files", cf_project_id), 
            Some(&format!("gameVersion={}&modLoaderType={}", mc_version, loader_type))
        );
        
        let mut data = Vec::new();
        if let Ok(req) = crate::core::curseforge::cf_request(client, reqwest::Method::GET, &strict_url) {
            if let Ok(resp) = req.send().await {
                if resp.status().is_success() {
                    if let Ok(json) = resp.json::<serde_json::Value>().await {
                        if let Some(arr) = json.get("data").and_then(|d| d.as_array()) {
                            data = arr.clone();
                        }
                    }
                }
            }
        }
        
        if data.is_empty() {
            let all_url = crate::core::curseforge::build_cf_url(&format!("/mods/{}/files", cf_project_id), None);
            if let Ok(req) = crate::core::curseforge::cf_request(client, reqwest::Method::GET, &all_url) {
                if let Ok(resp) = req.send().await {
                    if resp.status().is_success() {
                        if let Ok(json) = resp.json::<serde_json::Value>().await {
                            if let Some(arr) = json.get("data").and_then(|d| d.as_array()) {
                                data = arr.clone();
                            }
                        }
                    } else if resp.status() == 404 {
                        return Err("instances.export.error.cf404".into());
                    }
                }
            }
        }
        
        if data.is_empty() {
            return Err("instances.export.error.cfNoFiles".into());
        }
        
        if data.len() == 1 {
            if let Some(first_file) = data.first() {
                if let Some(file_id) = first_file.get("id").and_then(|i| i.as_u64()) {
                    return Ok(ResolveResponse::Matched {
                        matched_mod: MatchedMod {
                            filename,
                            mod_id: Some(project_id),
                            mr_json: None,
                            cf_project_id: Some(cf_project_id),
                            cf_file_id: Some(file_id as u32),
                        }
                    });
                }
            }
        }
        
        let mut choices = Vec::new();
        for f in data.into_iter() {
            let gvs: Vec<String> = f.get("gameVersions").and_then(|a| a.as_array()).map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect()).unwrap_or_default();
            
            // Filter by loader
            let is_compatible = gvs.iter().any(|s| {
                let lower = s.to_lowercase();
                lower == loader_name || (loader_name == "neoforge" && lower == "forge") || (loader_name == "forge" && lower == "neoforge")
            });
            
            if !is_compatible && !gvs.is_empty() {
                // If it has gameVersions but none match our loader, skip it
                // (some mods might have empty gameVersions, we could be lenient, but usually CF populates it)
                continue;
            }
            
            if let Some(file_id) = f.get("id").and_then(|i| i.as_u64()) {
                let v_name = f.get("displayName").and_then(|s| s.as_str()).unwrap_or("Unknown Version").to_string();
                choices.push(ResolveChoice {
                    version_id: file_id.to_string(),
                    version_name: v_name,
                    game_versions: gvs, // CF mixes MC versions and Loaders in gameVersions
                    loaders: vec![],
                    matched_mod: MatchedMod {
                        filename: filename.clone(),
                        mod_id: Some(project_id.clone()),
                        mr_json: None,
                        cf_project_id: Some(cf_project_id),
                        cf_file_id: Some(file_id as u32),
                    }
                });
            }
            if choices.len() >= 20 { break; }
        }
        
        if choices.is_empty() {
            return Err("instances.export.error.invalidJson".into());
        }
        
        Ok(ResolveResponse::Choices { choices })
    }
}










