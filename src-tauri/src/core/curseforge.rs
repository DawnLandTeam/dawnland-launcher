use serde::{Deserialize, Serialize};

/// CurseForge API configuration
/// In production, this should be loaded from environment or config file
const CURSEFORGE_API_KEY: &str = "YOUR_CURSEFORGE_API_KEY_HERE";
const CURSEFORGE_BASE_URL: &str = "https://api.curseforge.com/v1";

/// Game ID for Minecraft on CurseForge
const CF_GAME_ID_MINECRAFT: i32 = 432;
/// Class ID for Mods on CurseForge
const CF_CLASS_ID_MODS: i32 = 6;

/// Unified mod project structure for both Modrinth and CurseForge
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedModProject {
    /// Source platform: "modrinth" or "curseforge"
    pub source: String,
    /// Project ID (modId on CurseForge, project_id on Modrinth)
    pub project_id: String,
    /// Project title/name
    pub title: String,
    /// Project description
    pub description: String,
    /// Icon URL (cover image)
    pub icon_url: Option<String>,
    /// Download count
    pub downloads: u64,
    /// Author/Owner name
    pub author: String,
    /// Minecraft versions this mod supports
    pub mc_versions: Vec<String>,
    /// Mod loader types (fabric, forge, neoforge)
    pub loaders: Vec<String>,
    /// Direct download URL (if available)
    pub download_url: Option<String>,
    /// File ID (CurseForge specific)
    pub file_id: Option<String>,
}

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
struct CfSearchResponse {
    data: Vec<CfMod>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CfMod {
    id: i64,
    name: String,
    summary: String,
    logo: Option<CfLogo>,
    download_count: u64,
    authors: Vec<CfAuthor>,
    latest_files: Option<Vec<CfFile>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CfLogo {
    url: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CfAuthor {
    name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CfFile {
    id: i64,
    display_name: String,
    download_url: Option<String>,
    game_versions: Option<Vec<String>>,
    loaders: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct CfFilesResponse {
    data: Vec<CfFile>,
}

// ============================================================================
// API Functions
// ============================================================================

/// Search CurseForge mods
#[tauri::command]
pub async fn search_curseforge(
    query: String,
    mc_version: String,
    loader: String,
) -> Result<Vec<UnifiedModProject>, String> {
    tracing::info!(
        "Searching CurseForge: query={}, mc_version={}, loader={}",
        query,
        mc_version,
        loader
    );

    let mod_loader_type = CfModLoader::from_str(&loader) as i32;

    // Build the search URL
    let search_url = format!(
        "{}/mods/search?gameId={}&classId={}&searchFilter={}&gameVersion={}&modLoaderType={}&sortField=2&sortOrder=desc",
        CURSEFORGE_BASE_URL,
        CF_GAME_ID_MINECRAFT,
        CF_CLASS_ID_MODS,
        urlencoding::encode(&query),
        mc_version,
        mod_loader_type
    );

    let client = reqwest::Client::new();
    let response = client
        .get(&search_url)
        .header("x-api-key", CURSEFORGE_API_KEY)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| format!("Failed to send request: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        tracing::error!("CurseForge API error: {} - {}", status, body);
        return Err(format!("CurseForge API error: {}", status));
    }

    let search_result: CfSearchResponse = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

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
                icon_url: m.logo.and_then(|l| l.url),
                downloads: m.download_count,
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

/// Get mod download URL from CurseForge
#[tauri::command]
pub async fn get_cf_mod_download_url(
    project_id: String,
    mc_version: String,
    loader: String,
) -> Result<(String, String), String> {
    tracing::info!(
        "Getting CF download URL: project_id={}, mc_version={}, loader={}",
        project_id,
        mc_version,
        loader
    );

    let mod_id: i64 = project_id
        .parse()
        .map_err(|_| "Invalid project ID")?;

    // Fetch files for this mod
    let files_url = format!("{}/mods/{}/files", CURSEFORGE_BASE_URL, mod_id);

    let client = reqwest::Client::new();
    let response = client
        .get(&files_url)
        .header("x-api-key", CURSEFORGE_API_KEY)
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

    // Find the latest compatible file
    let target_loader = loader.to_lowercase();
    let target_version = format!(" {}", mc_version); // Match version format

    // Sort by file ID descending (higher ID = newer)
    let mut sorted_files = files_result.data;
    sorted_files.sort_by(|a, b| b.id.cmp(&a.id));

    for file in sorted_files {
        // Check game version compatibility
        let game_versions = file.game_versions.as_ref();
        let has_mc_version = game_versions
            .map(|v| v.iter().any(|gv| gv.contains(&mc_version)))
            .unwrap_or(false);

        // Check loader compatibility
        let loaders = file.loaders.as_ref();
        let has_loader = loaders
            .map(|l| {
                l.iter()
                    .any(|loader| loader.to_lowercase().contains(&target_loader))
            })
            .unwrap_or(false);

        if has_mc_version && has_loader {
            if let Some(download_url) = file.download_url {
                tracing::info!(
                    "Found compatible file: id={}, name={}",
                    file.id,
                    file.display_name
                );
                return Ok((download_url, file.id.to_string()));
            }
        }
    }

    Err("No compatible file found".to_string())
}

/// Get detailed information about a specific mod
#[tauri::command]
pub async fn get_cf_mod_details(project_id: String) -> Result<UnifiedModProject, String> {
    tracing::info!("Getting CF mod details: project_id={}", project_id);

    let mod_id: i64 = project_id
        .parse()
        .map_err(|_| "Invalid project ID")?;

    let mod_url = format!("{}/mods/{}", CURSEFORGE_BASE_URL, mod_id);

    let client = reqwest::Client::new();
    let response = client
        .get(&mod_url)
        .header("x-api-key", CURSEFORGE_API_KEY)
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
        data: CfMod,
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
        icon_url: m.logo.and_then(|l| l.url),
        downloads: m.download_count,
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