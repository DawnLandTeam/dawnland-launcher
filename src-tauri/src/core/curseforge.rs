use serde::{Deserialize, Serialize};

use super::modrinth::{OnlineModpackVersion, UnifiedCategory, UnifiedModFile, UnifiedModProject};

pub static CURSE_API_KEY: std::sync::OnceLock<String> = std::sync::OnceLock::new();

#[tauri::command]
pub fn set_curseforge_api_key(key: String) {
    // If it's already set, this will fail, which is fine for our use case
    // since we only set it once at startup.
    let _ = CURSE_API_KEY.set(key);
}

#[tauri::command]
pub fn get_curseforge_api_key() -> Option<String> {
    CURSE_API_KEY.get().cloned()
}

/// Game ID for Minecraft on CurseForge
const CF_GAME_ID_MINECRAFT: i32 = 432;
/// Class ID for Mods on CurseForge
const CF_CLASS_ID_MODS: i32 = 6;
/// Class ID for Resource Packs on CurseForge
const CF_CLASS_ID_RESOURCE_PACKS: i32 = 12;
/// Class ID for Worlds on CurseForge
const CF_CLASS_ID_WORLDS: i32 = 17;
/// Class ID for Shaderpacks on CurseForge
const CF_CLASS_ID_SHADERS: i32 = 6552;

/// CurseForge mod loader type mapping
/// Reference: https://docs.curseforge.com/#mod-loaders
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CfModLoader {
    Any = 0,
    Forge = 1,
    // 2 = Cauldron
    // 3 = LiteLoader
    Fabric = 4,
    // 5 = Rift
    NeoForge = 6,
}

impl CfModLoader {
    pub fn from_str(loader: &str) -> Self {
        match loader.to_lowercase().as_str() {
            "forge" => CfModLoader::Forge,
            "fabric" => CfModLoader::Fabric,
            "neoforge" => CfModLoader::NeoForge,
            _ => CfModLoader::Any,
        }
    }
}

pub fn get_fallback_download_url(file_id: i64, file_name: &str) -> String {
    let part1 = file_id / 1000;
    let part2 = file_id % 1000;
    let encoded_name = urlencoding::encode(file_name);
    format!(
        "https://edge.forgecdn.net/files/{}/{:03}/{}",
        part1, part2, encoded_name
    )
}
// ============================================================================
// CurseForge API Response Types
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct CfSearchResponse {
    pub data: Vec<CfModProject>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CfModProject {
    pub id: i64,
    pub name: String,
    pub summary: String,
    pub logo: Option<CfLogo>,
    pub download_count: f64,
    pub authors: Vec<CfAuthor>,
    pub latest_files: Option<Vec<CfFile>>,
    pub latest_files_indexes: Option<Vec<CfFileIndex>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CfFileIndex {
    pub game_version: String,
    pub file_id: i64,
    pub filename: String,
    pub release_type: i32,
    pub game_version_type_id: Option<i32>,
    pub mod_loader: Option<i32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CfLogo {
    pub thumbnail_url: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CfAuthor {
    pub name: String,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CfFileHash {
    pub value: String,
    pub algo: i32,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CfDependency {
    pub mod_id: i64,
    pub relation_type: i32, // 3 = requiredDependency
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CfFile {
    pub id: i64,
    pub file_name: String,
    pub display_name: String,
    pub download_url: Option<String>,
    pub file_length: Option<u64>,
    pub hashes: Option<Vec<CfFileHash>>,
    pub game_versions: Option<Vec<String>>,
    pub loaders: Option<Vec<String>>,
    pub release_type: i32,
    pub file_date: String,
    pub dependencies: Option<Vec<CfDependency>>,
    pub parent_project_file_id: Option<i64>,
    pub is_server_pack: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct CfFilesResponse {
    pub data: Vec<CfFile>,
}

// ============================================================================
// API Functions
// ============================================================================

/// Build the full URL to the CurseForge Core API.
pub fn build_cf_url(path: &str, query: Option<&str>) -> String {
    let base = "https://api.curseforge.com/v1";
    match query {
        Some(q) => format!("{}{}?{}", base, path, q),
        None => format!("{}{}", base, path),
    }
}

/// Helper to create a request builder with the API key.
pub fn cf_request(
    client: &reqwest::Client,
    method: reqwest::Method,
    url: &str,
) -> Result<reqwest::RequestBuilder, String> {
    let mut req = client
        .request(method, url)
        .header("Accept", "application/json");
    if let Some(key) = CURSE_API_KEY.get() {
        req = req.header("x-api-key", key);
        Ok(req)
    } else {
        tracing::error!("CURSE_API_KEY is not set! Cannot make CurseForge API request.");
        Err("CURSE_API_KEY is not set! Cannot make CurseForge API request.".to_string())
    }
}

/// Search CurseForge mods via the web backend proxy.
#[tauri::command]
pub async fn search_curseforge(
    query: String,
    mc_versions: Vec<String>,
    loaders: Vec<String>,
    categories: Vec<String>,
    offset: Option<i32>,
    limit: Option<i32>,
) -> Result<Vec<UnifiedModProject>, String> {
    search_curseforge_internal(
        query,
        mc_versions,
        loaders,
        categories,
        offset,
        limit,
        CF_CLASS_ID_MODS,
    )
    .await
}

#[tauri::command]
pub async fn search_curseforge_resourcepacks(
    query: String,
    mc_versions: Vec<String>,
    categories: Vec<String>,
    offset: Option<i32>,
    limit: Option<i32>,
) -> Result<Vec<UnifiedModProject>, String> {
    search_curseforge_internal(
        query,
        mc_versions,
        vec![],
        categories,
        offset,
        limit,
        CF_CLASS_ID_RESOURCE_PACKS,
    )
    .await
}

#[tauri::command]
pub async fn search_curseforge_shaderpacks(
    query: String,
    mc_versions: Vec<String>,
    categories: Vec<String>,
    offset: Option<i32>,
    limit: Option<i32>,
) -> Result<Vec<UnifiedModProject>, String> {
    search_curseforge_internal(
        query,
        mc_versions,
        vec![],
        categories,
        offset,
        limit,
        CF_CLASS_ID_SHADERS,
    )
    .await
}

#[tauri::command]
pub async fn search_curseforge_worlds(
    query: String,
    mc_versions: Vec<String>,
    categories: Vec<String>,
    offset: Option<i32>,
    limit: Option<i32>,
) -> Result<Vec<UnifiedModProject>, String> {
    search_curseforge_internal(
        query,
        mc_versions,
        vec![],
        categories,
        offset,
        limit,
        CF_CLASS_ID_WORLDS,
    )
    .await
}

async fn search_curseforge_internal(
    query: String,
    mc_versions: Vec<String>,
    loaders: Vec<String>,
    categories: Vec<String>,
    offset: Option<i32>,
    limit: Option<i32>,
    class_id: i32,
) -> Result<Vec<UnifiedModProject>, String> {
    tracing::info!(
        "Searching CurseForge via proxy: query={}, mc_versions={:?}, loaders={:?}",
        query,
        mc_versions,
        loaders
    );

    let idx = offset.unwrap_or(0);
    let ps = limit.unwrap_or(20);

    let sort_field = if query.trim().is_empty() { 6 } else { 1 }; // 6=TotalDownloads, 1=Featured or Relevance

    let mut query_params = vec![
        format!("gameId={}", CF_GAME_ID_MINECRAFT),
        format!("classId={}", class_id),
        format!("searchFilter={}", urlencoding::encode(&query)),
        format!("index={}", idx),
        format!("pageSize={}", ps),
        format!("sortField={}", sort_field),
        "sortOrder=desc".to_string(),
    ];

    if !mc_versions.is_empty() {
        let v_arr = mc_versions
            .iter()
            .map(|v| format!("\"{}\"", v))
            .collect::<Vec<_>>()
            .join(",");
        query_params.push(format!(
            "gameVersions={}",
            urlencoding::encode(&format!("[{}]", v_arr))
        ));
    }

    let mut has_loaders = false;
    if !loaders.is_empty() {
        let l_arr = loaders
            .iter()
            .filter_map(|l| {
                let loader_type = CfModLoader::from_str(l) as i32;
                if loader_type != 0 {
                    Some(loader_type.to_string())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join(",");
        if !l_arr.is_empty() {
            query_params.push(format!(
                "modLoaderTypes={}",
                urlencoding::encode(&format!("[{}]", l_arr))
            ));
            has_loaders = true;
        }
    }

    // CF API strict requirement: If gameVersions is passed, modLoaderTypes must be passed for mods (classId=6).
    // If no specific loader is selected, pass "Any" loader which is 0.
    // For resource packs (classId=12), passing modLoaderTypes causes the search to fail/return empty.
    if !mc_versions.is_empty() && !has_loaders && class_id == CF_CLASS_ID_MODS {
        query_params.push(format!("modLoaderTypes={}", urlencoding::encode("[0]")));
    }

    if !categories.is_empty() {
        let c_arr = categories
            .iter()
            .filter_map(|c| c.parse::<i32>().ok())
            .map(|c| c.to_string())
            .collect::<Vec<_>>()
            .join(",");
        if !c_arr.is_empty() {
            query_params.push(format!(
                "categoryIds={}",
                urlencoding::encode(&format!("[{}]", c_arr))
            ));
        }
    }

    let query_string = query_params.join("&");

    let cf_url = build_cf_url("/mods/search", Some(&query_string));
    tracing::info!("CF API URL: {}", cf_url);

    let client = crate::core::utils::get_http_client().clone();
    let response = cf_request(&client, reqwest::Method::GET, &cf_url)?
        .send()
        .await
        .map_err(|e| format!("Network Error: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        tracing::error!("CurseForge API error: {} - {}", status, body);
        return Err(format!("CurseForge API error: {}", status));
    }

    let raw_text = response.text().await.map_err(|e| e.to_string())?;

    let search_result: CfSearchResponse = match serde_json::from_str(&raw_text) {
        Ok(data) => data,
        Err(e) => {
            let sample = raw_text.chars().take(1000).collect::<String>();
            tracing::error!("Failed to parse CF JSON: {}. Raw data: {}", e, sample);
            return Err(format!("JSON Parse Error: {}", e));
        }
    };

    let projects: Vec<UnifiedModProject> = search_result
        .data
        .into_iter()
        .map(|m| {
            let mc_versions = m
                .latest_files_indexes
                .as_ref()
                .map(|indexes| {
                    let mut versions: Vec<String> =
                        indexes.iter().map(|idx| idx.game_version.clone()).collect();
                    versions.sort();
                    versions.dedup();
                    // exclude loaders
                    let excluded = vec![
                        "forge",
                        "fabric",
                        "quilt",
                        "neoforge",
                        "liteloader",
                        "rift",
                        "vanilla",
                        "client",
                        "server",
                    ];
                    versions.retain(|v| {
                        let lower = v.to_lowercase();
                        !lower.starts_with("java") && !excluded.iter().any(|ex| lower.contains(ex))
                    });
                    versions.sort_by(|a, b| b.cmp(a)); // Sort descending
                    versions
                })
                .unwrap_or_else(|| {
                    m.latest_files
                        .as_ref()
                        .and_then(|f| f.first())
                        .and_then(|f| f.game_versions.as_ref())
                        .cloned()
                        .unwrap_or_default()
                });

            let loaders = m
                .latest_files
                .as_ref()
                .and_then(|f| f.first())
                .and_then(|f| f.loaders.as_ref())
                .cloned()
                .unwrap_or_default();

            UnifiedModProject {
                source: "curseforge".to_string(),
                project_id: m.id.to_string(),
                title: m.name,
                description: m.summary,
                icon_url: m.logo.and_then(|l| l.thumbnail_url),
                downloads: m.download_count as u64,
                author: m
                    .authors
                    .first()
                    .map(|a| a.name.clone())
                    .unwrap_or_else(|| "Unknown".to_string()),
                mc_versions,
                loaders,
                download_url: None,
                file_id: None,
            }
        })
        .collect();

    tracing::info!("Found {} mods on CurseForge", projects.len());
    Ok(projects)
}

/// Get all compatible mod files from CurseForge via the web backend proxy.
#[tauri::command]
pub async fn get_cf_mod_files(
    project_id: String,
    mc_version: String,
    loaders: Vec<String>,
) -> Result<Vec<UnifiedModFile>, String> {
    tracing::info!(
        "Getting CF mod files via proxy: project_id={}, mc_version={}, loaders={:?}",
        project_id,
        mc_version,
        loaders
    );

    // Build query string
    let mut query_params = vec![];
    if !mc_version.is_empty() && mc_version != "Other" {
        query_params.push(format!("gameVersion={}", urlencoding::encode(&mc_version)));
    }
    // We omit modLoaderType so CF returns all loaders for this mc_version!
    let query_string = query_params.join("&");

    let query_opt = if query_string.is_empty() {
        None
    } else {
        Some(query_string.as_str())
    };
    let cf_url = build_cf_url(&format!("/mods/{}/files", project_id), query_opt);
    tracing::info!("CF API URL: {}", cf_url);

    let client = crate::core::utils::get_http_client().clone();
    let response = cf_request(&client, reqwest::Method::GET, &cf_url)?
        .send()
        .await
        .map_err(|e| format!("Failed to send request: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        return Err(format!("CurseForge API error: {}", status));
    }

    let files_result: CfFilesResponse = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    // Sort by file ID descending (higher ID = newer)
    let mut sorted_files = files_result.data;
    sorted_files.retain(|f| {
        f.parent_project_file_id.unwrap_or(0) == 0 && !f.is_server_pack.unwrap_or(false)
    });
    sorted_files.sort_by(|a, b| b.id.cmp(&a.id));

    let mut compatible_files = Vec::new();

    for file in sorted_files {
        // Filter by loaders locally
        let file_loaders = file.game_versions.clone().unwrap_or_default();
        let has_loader = loaders.is_empty()
            || file_loaders.iter().any(|fl| {
                loaders
                    .iter()
                    .any(|target_loader| fl.to_lowercase() == target_loader.to_lowercase())
            });

        if !has_loader {
            continue;
        }

        let download_url = file
            .download_url
            .unwrap_or_else(|| get_fallback_download_url(file.id, &file.file_name));
        let release_str = match file.release_type {
            1 => "Release",
            2 => "Beta",
            3 => "Alpha",
            _ => "Unknown",
        };

        let mut hash = None;
        if let Some(hashes) = &file.hashes {
            if let Some(h) = hashes.iter().find(|h| h.algo == 1) {
                hash = Some(h.value.clone());
            }
        }

        let deps = file
            .dependencies
            .as_ref()
            .map(|d_list| {
                d_list
                    .iter()
                    .map(|d| super::modrinth::UnifiedDependency {
                        project_id: d.mod_id.to_string(),
                        version_id: None,
                        required: d.relation_type == 3,
                        name: None,
                    })
                    .collect()
            })
            .unwrap_or_default();

        compatible_files.push(UnifiedModFile {
            id: file.id.to_string(),
            filename: file.file_name.clone(),
            version_number: "".to_string(),
            download_url,
            release_type: release_str.to_string(),
            date: file.file_date.clone(),
            file_size: file.file_length,
            hash,
            dependencies: deps,
            mc_versions: file.game_versions.clone().unwrap_or_default(),
        });
    }

    if compatible_files.is_empty() {
        tracing::error!(
            "No compatible file found for project_id={}, target_version={}, target_loaders={:?}",
            project_id,
            mc_version,
            loaders
        );
        return Err("No compatible file found".to_string());
    }

    Ok(compatible_files)
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct CfBatchFilesRequest {
    file_ids: Vec<u32>,
}

/// Get multiple mod files from CurseForge via the web backend proxy using a batch request.
#[tauri::command]
pub async fn get_cf_files_batch(file_ids: Vec<u32>) -> Result<Vec<UnifiedModFile>, String> {
    if file_ids.is_empty() {
        tracing::debug!("get_cf_files_batch called with empty file_ids, returning early");
        return Ok(Vec::new());
    }

    tracing::info!(
        "Getting CF mod files batch via proxy for {} files",
        file_ids.len()
    );

    let cf_url = build_cf_url("/mods/files", None);

    let request_body = CfBatchFilesRequest { file_ids };

    let client = crate::core::utils::get_http_client().clone();
    let response = cf_request(&client, reqwest::Method::POST, &cf_url)?
        .json(&request_body)
        .send()
        .await
        .map_err(|e| format!("Failed to send request: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        tracing::error!("CurseForge API error: {} - {}", status, body);
        return Err(format!("CurseForge API error: {}", status));
    }

    let files_result: CfFilesResponse = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    let mut compatible_files = Vec::new();

    for file in files_result.data {
        let download_url = file
            .download_url
            .unwrap_or_else(|| get_fallback_download_url(file.id, &file.file_name));
        let release_str = match file.release_type {
            1 => "Release",
            2 => "Beta",
            3 => "Alpha",
            _ => "Unknown",
        };

        let mut hash = None;
        if let Some(hashes) = file.hashes {
            if let Some(h) = hashes.into_iter().find(|h| h.algo == 1) {
                hash = Some(h.value);
            }
        }

        let deps = file
            .dependencies
            .as_ref()
            .map(|d_list| {
                d_list
                    .iter()
                    .map(|d| super::modrinth::UnifiedDependency {
                        project_id: d.mod_id.to_string(),
                        version_id: None,
                        required: d.relation_type == 3,
                        name: None,
                    })
                    .collect()
            })
            .unwrap_or_default();

        compatible_files.push(UnifiedModFile {
            id: file.id.to_string(),
            filename: file.file_name.clone(),
            version_number: "".to_string(),
            download_url,
            release_type: release_str.to_string(),
            date: file.file_date.clone(),
            file_size: file.file_length,
            hash,
            dependencies: deps,
            mc_versions: file.game_versions.clone().unwrap_or_default(),
        });
    }

    Ok(compatible_files)
}

/// Get detailed information about a specific mod via the web backend proxy.
#[tauri::command]
pub async fn get_cf_mod_details(project_id: String) -> Result<UnifiedModProject, String> {
    tracing::info!(
        "Getting CF mod details via proxy: project_id={}",
        project_id
    );

    let cf_url = build_cf_url(&format!("/mods/{}", project_id), None);
    tracing::info!("CF API URL: {}", cf_url);

    let client = crate::core::utils::get_http_client().clone();
    let response = cf_request(&client, reqwest::Method::GET, &cf_url)?
        .send()
        .await
        .map_err(|e| format!("Failed to send request: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        return Err(format!("CurseForge API error: {}", status));
    }

    #[derive(Deserialize)]
    struct CfModDetailsResponse {
        data: CfModProject,
    }

    let details: CfModDetailsResponse = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    let m = details.data;

    let mc_versions = m
        .latest_files
        .as_ref()
        .and_then(|f| f.first())
        .and_then(|f| f.game_versions.as_ref())
        .cloned()
        .unwrap_or_default();

    let loaders = m
        .latest_files
        .as_ref()
        .and_then(|f| f.first())
        .and_then(|f| f.loaders.as_ref())
        .cloned()
        .unwrap_or_default();

    Ok(UnifiedModProject {
        source: "curseforge".to_string(),
        project_id: m.id.to_string(),
        title: m.name,
        description: m.summary,
        icon_url: m.logo.and_then(|l| l.thumbnail_url),
        downloads: m.download_count as u64,
        author: m
            .authors
            .first()
            .map(|a| a.name.clone())
            .unwrap_or_else(|| "Unknown".to_string()),
        mc_versions,
        loaders,
        download_url: None,
        file_id: None,
    })
}

#[tauri::command]
pub async fn search_curseforge_modpacks(query: String) -> Result<Vec<UnifiedModProject>, String> {
    tracing::info!("Searching CurseForge modpacks: query={}", query);

    // Build search query parameters with classId=4471 (Modpacks)
    let query_string = format!(
        "gameId=432&classId=4471&searchFilter={}&sortField=2&sortOrder=desc",
        urlencoding::encode(&query)
    );

    let cf_url = build_cf_url("/mods/search", Some(&query_string));

    let client = crate::core::utils::get_http_client().clone();
    let response = cf_request(&client, reqwest::Method::GET, &cf_url)?
        .send()
        .await
        .map_err(|e| format!("Failed to send request: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("CurseForge API error: {} - {}", status, body));
    }

    let raw_text = response.text().await.map_err(|e| e.to_string())?;

    #[derive(Deserialize)]
    struct CfSearchResponse {
        data: Vec<CfModProject>,
    }

    let search_result: CfSearchResponse = match serde_json::from_str(&raw_text) {
        Ok(res) => res,
        Err(e) => {
            tracing::error!("Failed to parse CurseForge API response: {}", e);
            tracing::error!("Raw Response: {}", raw_text);
            return Err(format!("Failed to parse JSON: {}", e));
        }
    };

    let projects: Vec<UnifiedModProject> = search_result
        .data
        .into_iter()
        .map(|m| {
            let mc_versions = m
                .latest_files
                .as_ref()
                .and_then(|f| f.first())
                .and_then(|f| f.game_versions.as_ref())
                .cloned()
                .unwrap_or_default();

            let loaders = m
                .latest_files
                .as_ref()
                .and_then(|f| f.first())
                .and_then(|f| f.loaders.as_ref())
                .cloned()
                .unwrap_or_default();

            UnifiedModProject {
                source: "curseforge".to_string(),
                project_id: m.id.to_string(),
                title: m.name,
                description: m.summary,
                icon_url: m.logo.and_then(|l| l.thumbnail_url),
                downloads: m.download_count as u64,
                author: m
                    .authors
                    .first()
                    .map(|a| a.name.clone())
                    .unwrap_or_else(|| "Unknown".to_string()),
                mc_versions,
                loaders,
                download_url: None,
                file_id: None,
            }
        })
        .collect();

    tracing::info!("Found {} CurseForge modpacks", projects.len());

    Ok(projects)
}

#[tauri::command]
pub async fn get_curseforge_modpack_versions(
    project_id: String,
) -> Result<Vec<OnlineModpackVersion>, String> {
    tracing::info!(
        "Getting CurseForge modpack versions: project_id={}",
        project_id
    );

    let query_string = format!("modId={}", project_id);
    // Use /mods/{modId}/files to get versions. The proxy might just expect /mods/files or something?
    // Wait, the API endpoint is /v1/mods/{modId}/files. Let's see how `get_cf_files_batch` or others use proxy.
    // I'll assume `build_proxy_url(&format!("/mods/{}/files", project_id), None)`
    let cf_url = build_cf_url(&format!("/mods/{}/files", project_id), None);

    let client = crate::core::utils::get_http_client().clone();
    let response = cf_request(&client, reqwest::Method::GET, &cf_url)?
        .send()
        .await
        .map_err(|e| format!("Failed to send request: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        return Err(format!("CurseForge API error: {}", status));
    }

    #[derive(Deserialize)]
    struct CfFilesResponse {
        data: Vec<CfFile>,
    }

    let files: CfFilesResponse = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    let mut result = Vec::new();
    for file in files.data {
        let mut mc_versions = Vec::new();
        let mut loaders = file.loaders.clone().unwrap_or_default();

        if let Some(gv) = &file.game_versions {
            for v in gv {
                let lower = v.to_lowercase();
                if ["forge", "fabric", "quilt", "neoforge", "liteloader"].contains(&lower.as_str())
                {
                    if !loaders.contains(v) {
                        loaders.push(v.clone());
                    }
                } else if lower != "standard" && !lower.starts_with("java ") && lower != "modpack" {
                    mc_versions.push(v.clone());
                }
            }
        }

        result.push(OnlineModpackVersion {
            id: file.id.to_string(),
            name: file.display_name,
            mc_version: mc_versions.join(", "),
            loaders,
            download_url: file
                .download_url
                .unwrap_or_else(|| get_fallback_download_url(file.id, &file.file_name)),
            date: file.file_date,
        });
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_cf_search_response() {
        let json = r#"{
            "data": [
                {
                    "id": 238222,
                    "name": "Just Enough Items (JEI)",
                    "summary": "JEI is an item and recipe viewing mod for Minecraft",
                    "downloadCount": 265000000.0,
                    "authors": [
                        { "name": "mezz" }
                    ],
                    "logo": {
                        "thumbnailUrl": "https://media.forgecdn.net/avatars/thumbnail/1.png"
                    },
                    "latestFiles": [
                        {
                            "id": 123456,
                            "fileName": "jei-1.20.1.jar",
                            "displayName": "JEI 1.20.1",
                            "releaseType": 1,
                            "fileDate": "2023-01-01T00:00:00Z",
                            "gameVersions": ["1.20.1"],
                            "loaders": ["Forge", "Fabric"]
                        }
                    ]
                }
            ]
        }"#;

        let response: CfSearchResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.data.len(), 1);
        let project = &response.data[0];
        assert_eq!(project.id, 238222);
        assert_eq!(project.name, "Just Enough Items (JEI)");
        assert_eq!(project.authors[0].name, "mezz");
        assert_eq!(project.latest_files.as_ref().unwrap().len(), 1);
        assert_eq!(
            project.latest_files.as_ref().unwrap()[0].file_name,
            "jei-1.20.1.jar"
        );
        assert_eq!(
            project.latest_files.as_ref().unwrap()[0]
                .game_versions
                .as_ref()
                .unwrap()[0],
            "1.20.1"
        );
    }

    #[test]
    fn test_parse_cf_files_response() {
        let json = r#"{
            "data": [
                {
                    "id": 123456,
                    "fileName": "jei-1.20.1.jar",
                    "displayName": "JEI 1.20.1",
                    "downloadUrl": "https://edge.forgecdn.net/files/123/456/jei-1.20.1.jar",
                    "fileLength": 1024000,
                    "releaseType": 1,
                    "fileDate": "2023-01-01T00:00:00Z",
                    "hashes": [
                        { "value": "1234567890abcdef", "algo": 1 }
                    ],
                    "gameVersions": ["1.20.1", "Forge"],
                    "loaders": ["Forge"]
                }
            ]
        }"#;

        let response: CfFilesResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.data.len(), 1);
        let file = &response.data[0];
        assert_eq!(file.id, 123456);
        assert_eq!(
            file.download_url.as_ref().unwrap(),
            "https://edge.forgecdn.net/files/123/456/jei-1.20.1.jar"
        );
        assert_eq!(file.hashes.as_ref().unwrap()[0].algo, 1);
        assert_eq!(file.hashes.as_ref().unwrap()[0].value, "1234567890abcdef");
    }

    #[test]
    fn test_cf_mod_loader_parsing() {
        assert_eq!(CfModLoader::from_str("forge"), CfModLoader::Forge);
        assert_eq!(CfModLoader::from_str("fabric"), CfModLoader::Fabric);
        assert_eq!(CfModLoader::from_str("neoforge"), CfModLoader::NeoForge);
        assert_eq!(CfModLoader::from_str("unknown"), CfModLoader::Any);
    }
}

#[derive(Deserialize)]
struct CfCategory {
    id: i32,
    name: String,
    #[serde(rename = "iconUrl")]
    icon_url: String,
}

#[derive(Deserialize)]
struct CfCategoriesResponse {
    data: Vec<CfCategory>,
}

#[tauri::command]
pub async fn get_curseforge_categories() -> Result<Vec<UnifiedCategory>, String> {
    let cf_url = build_cf_url(
        &format!(
            "/categories?gameId={}&classId={}",
            CF_GAME_ID_MINECRAFT, CF_CLASS_ID_MODS
        ),
        None,
    );
    let client = crate::core::utils::get_http_client().clone();
    let res = cf_request(&client, reqwest::Method::GET, &cf_url)?
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !res.status().is_success() {
        return Err(format!("CF API Error: {}", res.status()));
    }

    let json: CfCategoriesResponse = res.json().await.map_err(|e| e.to_string())?;
    let cats = json
        .data
        .into_iter()
        .map(|c| UnifiedCategory {
            id: c.id.to_string(),
            name: c.name,
            icon: c.icon_url,
        })
        .collect();

    Ok(cats)
}

#[tauri::command]
pub async fn get_curseforge_resourcepack_categories() -> Result<Vec<UnifiedCategory>, String> {
    let cf_url = build_cf_url(
        &format!(
            "/categories?gameId={}&classId={}",
            CF_GAME_ID_MINECRAFT, CF_CLASS_ID_RESOURCE_PACKS
        ),
        None,
    );
    let client = crate::core::utils::get_http_client().clone();
    let res = cf_request(&client, reqwest::Method::GET, &cf_url)?
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !res.status().is_success() {
        return Err(format!("CF API Error: {}", res.status()));
    }

    let json: CfCategoriesResponse = res.json().await.map_err(|e| e.to_string())?;
    let cats = json
        .data
        .into_iter()
        .map(|c| UnifiedCategory {
            id: c.id.to_string(),
            name: c.name,
            icon: c.icon_url,
        })
        .collect();

    Ok(cats)
}

#[tauri::command]
pub async fn get_curseforge_shaderpack_categories() -> Result<Vec<UnifiedCategory>, String> {
    let cf_url = build_cf_url(
        &format!(
            "/categories?gameId={}&classId={}",
            CF_GAME_ID_MINECRAFT, CF_CLASS_ID_SHADERS
        ),
        None,
    );
    let client = crate::core::utils::get_http_client().clone();
    let res = cf_request(&client, reqwest::Method::GET, &cf_url)?
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !res.status().is_success() {
        return Err(format!("CF API Error: {}", res.status()));
    }

    let json: CfCategoriesResponse = res.json().await.map_err(|e| e.to_string())?;
    let cats = json
        .data
        .into_iter()
        .map(|c| UnifiedCategory {
            id: c.id.to_string(),
            name: c.name,
            icon: c.icon_url,
        })
        .collect();

    Ok(cats)
}

#[tauri::command]
pub async fn get_curseforge_world_categories() -> Result<Vec<UnifiedCategory>, String> {
    let cf_url = build_cf_url(
        &format!(
            "/categories?gameId={}&classId={}",
            CF_GAME_ID_MINECRAFT, CF_CLASS_ID_WORLDS
        ),
        None,
    );
    let client = crate::core::utils::get_http_client().clone();
    let res = cf_request(&client, reqwest::Method::GET, &cf_url)?
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !res.status().is_success() {
        return Err(format!("CF API Error: {}", res.status()));
    }

    let json: CfCategoriesResponse = res.json().await.map_err(|e| e.to_string())?;
    let cats = json
        .data
        .into_iter()
        .map(|c| UnifiedCategory {
            id: c.id.to_string(),
            name: c.name,
            icon: c.icon_url,
        })
        .collect();

    Ok(cats)
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CFGameVersionData {
    pub id: i32,
    #[serde(rename = "gameVersionId")]
    pub game_version_id: i32,
    #[serde(rename = "versionString")]
    pub version_string: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CFGameVersionResponse {
    pub data: Vec<CFGameVersionData>,
}

#[tauri::command]
pub async fn get_curseforge_game_versions() -> Result<Vec<String>, String> {
    let cf_url = build_cf_url("/minecraft/version", None);
    let client = crate::core::utils::get_http_client().clone();
    let req = cf_request(&client, reqwest::Method::GET, &cf_url)?;

    let res: CFGameVersionResponse = req
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| format!("JSON parse error: {}", e))?;

    // Filter out snapshots and betas
    let mut releases: Vec<String> = res
        .data
        .into_iter()
        .filter(|v| {
            let lower = v.version_string.to_lowercase();
            !lower.contains("snapshot")
                && !lower.contains("beta")
                && !lower.contains("alpha")
                && !lower.contains("rc")
                && !lower.contains("pre")
        })
        .map(|v| v.version_string)
        .collect();

    // Sort descending (e.g. 1.20.1 > 1.20 > 1.19.4)
    releases.sort_by(|a, b| {
        let a_parts: Vec<u32> = a.split('.').filter_map(|s| s.parse().ok()).collect();
        let b_parts: Vec<u32> = b.split('.').filter_map(|s| s.parse().ok()).collect();
        b_parts.cmp(&a_parts)
    });

    // Remove duplicates
    releases.dedup();

    Ok(releases)
}

#[tauri::command]
pub async fn get_curseforge_loaders() -> Result<Vec<String>, String> {
    // CurseForge API doesn't have an endpoint for generic loader types (it returns thousands of specific loaders instead)
    // They are standardized by the ModLoaderType enum.
    Ok(vec![
        "Forge".to_string(),
        "Fabric".to_string(),
        "Quilt".to_string(),
        "NeoForge".to_string(),
        "LiteLoader".to_string(),
        "Cauldron".to_string(),
    ])
}
