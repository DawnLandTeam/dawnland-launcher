use serde::Deserialize;

use super::modrinth::{UnifiedModProject, UnifiedModFile};

/// Get the web backend URL from environment or use default.
fn get_web_backend_url() -> String {
    option_env!("WEB_BACKEND_URL")
        .unwrap_or("http://localhost:8080")
        .to_string()
}

/// CurseForge API path prefix on the web backend.
const CURSEFORGE_PROXY_PATH: &str = "/api/curseforge";

/// Game ID for Minecraft on CurseForge
const CF_GAME_ID_MINECRAFT: i32 = 432;
/// Class ID for Mods on CurseForge
const CF_CLASS_ID_MODS: i32 = 6;

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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CfFile {
    pub id: i64,
    pub display_name: String,
    pub download_url: Option<String>,
    pub game_versions: Option<Vec<String>>,
    pub loaders: Option<Vec<String>>,
    pub release_type: i32,
    pub file_date: String,
}

#[derive(Debug, Deserialize)]
pub struct CfFilesResponse {
    pub data: Vec<CfFile>,
}

// ============================================================================
// API Functions
// ============================================================================

/// Build the full URL to the web backend proxy.
fn build_proxy_url(path: &str, query: Option<&str>) -> String {
    let backend_url = get_web_backend_url();
    let base = format!("{}{}", backend_url, CURSEFORGE_PROXY_PATH);
    match query {
        Some(q) => format!("{}{}?{}", base, path, q),
        None => format!("{}{}", base, path),
    }
}

/// Search CurseForge mods via the web backend proxy.
#[tauri::command]
pub async fn search_curseforge(
    query: String,
    mc_version: String,
    loader: String,
) -> Result<Vec<UnifiedModProject>, String> {
    tracing::info!(
        "Searching CurseForge via proxy: query={}, mc_version={}, loader={}",
        query,
        mc_version,
        loader
    );

    let mod_loader_type = CfModLoader::from_str(&loader) as i32;

    // Build query string for the proxy
    let query_string = format!(
        "gameId={}&classId={}&searchFilter={}&gameVersion={}&modLoaderType={}&sortField=2&sortOrder=desc",
        CF_GAME_ID_MINECRAFT,
        CF_CLASS_ID_MODS,
        urlencoding::encode(&query),
        mc_version,
        mod_loader_type
    );

    let proxy_url = build_proxy_url("/mods/search", Some(&query_string));
    tracing::info!("Proxy URL: {}", proxy_url);

    let client = reqwest::Client::new();
    let response = client
        .get(&proxy_url)
        .header("Accept", "application/json")
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

    tracing::info!("Found {} mods on CurseForge", projects.len());
    Ok(projects)
}

/// Get all compatible mod files from CurseForge via the web backend proxy.
#[tauri::command]
pub async fn get_cf_mod_files(
    project_id: String,
    mc_version: String,
    loader: String,
) -> Result<Vec<UnifiedModFile>, String> {
    tracing::info!(
        "Getting CF mod files via proxy: project_id={}, mc_version={}, loader={}",
        project_id,
        mc_version,
        loader
    );

    let target_loader = loader.to_lowercase();
    let cf_loader_type = match target_loader.as_str() {
        "fabric" => 4,
        "forge" => 1,
        "neoforge" => 6,
        _ => 0,
    };

    // Build query string
    let query_string = if cf_loader_type != 0 {
        format!("gameVersion={}&modLoaderType={}", mc_version, cf_loader_type)
    } else {
        format!("gameVersion={}", mc_version)
    };

    let proxy_url = build_proxy_url(
        &format!("/mods/{}/files", project_id),
        Some(&query_string),
    );
    tracing::info!("Proxy URL: {}", proxy_url);

    let client = reqwest::Client::new();
    let response = client
        .get(&proxy_url)
        .header("Accept", "application/json")
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
    sorted_files.sort_by(|a, b| b.id.cmp(&a.id));

    let mut compatible_files = Vec::new();

    for file in sorted_files {
        if let Some(download_url) = file.download_url {
            let release_str = match file.release_type {
                1 => "Release",
                2 => "Beta",
                3 => "Alpha",
                _ => "Unknown",
            };

            compatible_files.push(UnifiedModFile {
                id: file.id.to_string(),
                filename: file.display_name.clone(),
                version_number: "".to_string(),
                download_url,
                release_type: release_str.to_string(),
                date: file.file_date.clone(),
            });
        } else {
            tracing::warn!("File {} matched but missing download_url!", file.id);
        }
    }

    if compatible_files.is_empty() {
        tracing::error!(
            "No compatible file found for project_id={}, target_version={}, target_loader={}",
            project_id,
            mc_version,
            target_loader
        );
        return Err("No compatible file found".to_string());
    }

    Ok(compatible_files)
}

/// Get detailed information about a specific mod via the web backend proxy.
#[tauri::command]
pub async fn get_cf_mod_details(project_id: String) -> Result<UnifiedModProject, String> {
    tracing::info!("Getting CF mod details via proxy: project_id={}", project_id);

    let proxy_url = build_proxy_url(&format!("/mods/{}", project_id), None);
    tracing::info!("Proxy URL: {}", proxy_url);

    let client = reqwest::Client::new();
    let response = client
        .get(&proxy_url)
        .header("Accept", "application/json")
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