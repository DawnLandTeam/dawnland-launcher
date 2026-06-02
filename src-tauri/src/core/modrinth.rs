use serde::{Deserialize, Serialize};

/// Modrinth API base URL
const MODRINTH_BASE_URL: &str = "https://api.modrinth.com/v2";

/// Unified mod project structure
/// Re-use the same structure as curseforge for unified handling
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

/// Unified mod version file representing a downloadable mod file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedModFile {
    pub id: String,
    pub filename: String,
    pub version_number: String,
    pub download_url: String,
    pub release_type: String, // "release", "beta", "alpha"
    pub date: String,
    pub file_size: Option<u64>,
    pub hash: Option<String>,
}

/// Online Modpack Version representing a modpack file to download
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnlineModpackVersion {
    pub id: String, // Version ID or File ID
    pub name: String, // E.g., "1.19.2 - v1.0.0"
    pub mc_version: String,
    pub loaders: Vec<String>,
    pub download_url: String,
    pub date: String,
}

// ============================================================================
// Modrinth API Response Types
// ============================================================================

#[derive(Debug, Deserialize)]
struct ModrinthSearchResult {
    hits: Vec<ModrinthProject>,
    #[serde(rename = "total_hits")]
    total_hits: usize,
}

#[derive(Debug, Deserialize)]
struct ModrinthProject {
    #[serde(rename = "project_id")]
    project_id: String,
    title: String,
    description: String,
    #[serde(rename = "icon_url")]
    icon_url: Option<String>,
    downloads: u64,
    author: String,
    #[serde(default)]
    game_versions: Option<Vec<String>>,
    #[serde(default)]
    loaders: Option<Vec<String>>,
    categories: Option<Vec<String>>,
    versions: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct ModrinthProjectDetails {
    #[serde(rename = "project_id")]
    project_id: String,
    title: String,
    description: String,
    #[serde(rename = "icon_url")]
    icon_url: Option<String>,
    downloads: u64,
    author: String,
    body: Option<String>,
    #[serde(rename = "game_versions")]
    game_versions: Vec<String>,
    loaders: Vec<String>,
    categories: Vec<String>,
    version_groups: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ModrinthVersion {
    id: String,
    #[serde(rename = "project_id")]
    project_id: String,
    version_number: String,
    #[serde(default)]
    game_versions: Vec<String>,
    #[serde(default)]
    loaders: Vec<String>,
    version_type: String, // "release", "beta", "alpha"
    date_published: String,
    files: Vec<ModrinthFile>,
}

#[derive(Debug, Deserialize)]
struct ModrinthFile {
    filename: String,
    url: String,
    size: u64,
    #[serde(rename = "file_type")]
    file_type: Option<String>,
    #[serde(default)]
    hashes: std::collections::HashMap<String, String>,
}

// ============================================================================
// API Functions
// ============================================================================

/// Search Modrinth mods
#[tauri::command]
pub async fn search_modrinth(
    query: String,
    mc_version: String,
    loader: String,
) -> Result<Vec<UnifiedModProject>, String> {
    tracing::info!(
        "Searching Modrinth: query={}, mc_version={}, loader={}",
        query,
        mc_version,
        loader
    );

    // Build search query parameters - keep it simple first
    let search_url = format!(
        "{}/search?query={}&limit=20",
        MODRINTH_BASE_URL,
        urlencoding::encode(&query)
    );

    let client = reqwest::Client::new();
    let response = client
        .get(&search_url)
        .header("Accept", "application/json")
        .header("User-Agent", "DawnlandLauncher/1.0")
        .send()
        .await
        .map_err(|e| format!("Failed to send request: {}", e))?;

    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    
    if !status.is_success() {
        tracing::error!("Modrinth API error: {} - {}", status, body);
        return Err(format!("Modrinth API error: {} - {}", status, body));
    }

    // Debug: print the response body
    tracing::debug!("Modrinth response: {}", &body[..body.len().min(500)]);

    let search_result: ModrinthSearchResult = serde_json::from_str(&body)
        .map_err(|e| format!("Failed to parse response: {} - body: {}", e, &body[..body.len().min(200)]))?;

    let mut projects: Vec<UnifiedModProject> = search_result
        .hits
        .into_iter()
        .filter(|p| {
            // Filter by MC version if specified
            if !mc_version.is_empty() {
                if let Some(ref versions) = p.game_versions {
                    if !versions.contains(&mc_version) {
                        return false;
                    }
                } else {
                    return false;
                }
            }
            // Filter by loader if specified
            if !loader.is_empty() {
                let loader_lower = loader.to_lowercase();
                if let Some(ref loaders) = p.loaders {
                    if !loaders.iter().any(|l| l.to_lowercase() == loader_lower) {
                        return false;
                    }
                } else {
                    return false;
                }
            }
            true
        })
        .map(|p| {
            UnifiedModProject {
                source: "modrinth".to_string(),
                project_id: p.project_id,
                title: p.title,
                description: p.description,
                icon_url: p.icon_url,
                downloads: p.downloads,
                author: p.author,
                mc_versions: p.game_versions.unwrap_or_default(),
                loaders: p.loaders.unwrap_or_default(),
                download_url: None,
                file_id: None,
            }
        })
        .collect();

    // Limit results
    projects.truncate(20);

    tracing::info!(
        "Found {} mods on Modrinth (filtered from {})",
        projects.len(),
        search_result.total_hits
    );
    Ok(projects)
}

/// Get all compatible mod files from Modrinth
#[tauri::command]
pub async fn get_modrinth_mod_files(
    project_id: String,
    mc_version: String,
    loader: String,
) -> Result<Vec<UnifiedModFile>, String> {
    tracing::info!(
        "Getting Modrinth mod files: project_id={}, mc_version={}, loader={}",
        project_id,
        mc_version,
        loader
    );

    // Get all versions for this project
    let versions_url = format!("{}/project/{}/version", MODRINTH_BASE_URL, project_id);
    tracing::info!("Fetching Modrinth files from URL: {}", versions_url);

    let client = reqwest::Client::new();
    let response = client
        .get(&versions_url)
        .header("Accept", "application/json")
        .header("User-Agent", "DawnlandLauncher/1.0")
        .send()
        .await
        .map_err(|e| format!("Failed to send request: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        return Err(format!("Modrinth API error: {}", status));
    }

    let versions: Vec<ModrinthVersion> = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    // Sort by date_published (descending)
    let mut sorted_versions = versions;
    sorted_versions.sort_by(|a, b| b.date_published.cmp(&a.date_published));

    let target_loader = loader.to_lowercase();
    let mut compatible_files = Vec::new();

    for version in sorted_versions {
        // Check game version compatibility
        let has_mc_version = version
            .game_versions
            .iter()
            .any(|gv| gv.contains(&mc_version));

        // Check loader compatibility
        let has_loader = version
            .loaders
            .iter()
            .any(|l| l.to_lowercase() == target_loader || target_loader.is_empty());

        if has_mc_version && has_loader {
            // Get the primary file (first one or primary file)
            if let Some(file) = version.files.first() {
                compatible_files.push(UnifiedModFile {
                    id: version.id.clone(),
                    filename: file.filename.clone(),
                    version_number: version.version_number.clone(),
                    download_url: file.url.clone(),
                    release_type: version.version_type.clone(),
                    date: version.date_published.clone(),
                    file_size: Some(file.size),
                    hash: file.hashes.get("sha1").cloned(),
                });
            }
        }
    }

    if compatible_files.is_empty() {
        tracing::error!("No compatible version found for project_id={}, target_version={}, target_loader={}", project_id, mc_version, target_loader);
        return Err("No compatible version found".to_string());
    }

    Ok(compatible_files)
}

/// Get detailed information about a specific mod from Modrinth
#[tauri::command]
pub async fn get_modrinth_mod_details(project_id: String) -> Result<UnifiedModProject, String> {
    tracing::info!(
        "Getting Modrinth mod details: project_id={}",
        project_id
    );

    let project_url = format!("{}/project/{}", MODRINTH_BASE_URL, project_id);

    let client = reqwest::Client::new();
    let response = client
        .get(&project_url)
        .header("Accept", "application/json")
        .header("User-Agent", "DawnlandLauncher/1.0")
        .send()
        .await
        .map_err(|e| format!("Failed to send request: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        return Err(format!("Modrinth API error: {}", status));
    }

    let details: ModrinthProjectDetails = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    Ok(UnifiedModProject {
        source: "modrinth".to_string(),
        project_id: details.project_id,
        title: details.title,
        description: details.description,
        icon_url: details.icon_url,
        downloads: details.downloads,
        author: details.author,
        mc_versions: details.game_versions,
        loaders: details.loaders,
        download_url: None,
        file_id: None,
    })
}

/// Get all available versions for a Modrinth project
#[tauri::command]
pub async fn get_modrinth_mod_versions(project_id: String) -> Result<Vec<String>, String> {
    tracing::info!(
        "Getting Modrinth mod versions: project_id={}",
        project_id
    );

    let versions_url = format!("{}/project/{}/version", MODRINTH_BASE_URL, project_id);

    let client = reqwest::Client::new();
    let response = client
        .get(&versions_url)
        .header("Accept", "application/json")
        .header("User-Agent", "DawnlandLauncher/1.0")
        .send()
        .await
        .map_err(|e| format!("Failed to send request: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        return Err(format!("Modrinth API error: {}", status));
    }

    #[derive(Deserialize)]
    struct VersionInfo {
        id: String,
        version_number: String,
        game_versions: Vec<String>,
        loaders: Vec<String>,
    }

    let versions: Vec<VersionInfo> = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    // Return version info as JSON string
    let version_list: Vec<String> = versions
        .iter()
        .map(|v| format!("{} ({} | {:?})", v.version_number, v.game_versions.join(", "), v.loaders))
        .collect();

    Ok(version_list)
}

#[tauri::command]
pub async fn search_modrinth_modpacks(query: String) -> Result<Vec<UnifiedModProject>, String> {
    tracing::info!("Searching Modrinth modpacks: query={}", query);

    // Build search query parameters with project_type:modpack
    let facets = "[[\"project_type:modpack\"]]";
    let search_url = format!(
        "{}/search?query={}&limit=20&facets={}",
        MODRINTH_BASE_URL,
        urlencoding::encode(&query),
        urlencoding::encode(facets)
    );

    let client = reqwest::Client::new();
    let response = client
        .get(&search_url)
        .header("Accept", "application/json")
        .header("User-Agent", "DawnlandLauncher/1.0")
        .send()
        .await
        .map_err(|e| format!("Failed to send request: {:?}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        return Err(format!("Modrinth API error: {}", status));
    }

    let body = response.text().await.unwrap_or_default();
    let search_result: ModrinthSearchResult = serde_json::from_str(&body)
        .map_err(|e| format!("Failed to parse response: {:?}", e))?;

    let projects: Vec<UnifiedModProject> = search_result
        .hits
        .into_iter()
        .map(|p| UnifiedModProject {
            source: "modrinth".to_string(),
            project_id: p.project_id,
            title: p.title,
            description: p.description,
            icon_url: p.icon_url,
            downloads: p.downloads,
            author: p.author,
            mc_versions: p.game_versions.unwrap_or_default(),
            loaders: p.categories.unwrap_or_default(),
            download_url: None,
            file_id: None,
        })
        .collect();

    tracing::info!(
        "Found {} Modrinth modpacks (total: {})",
        projects.len(),
        search_result.total_hits
    );

    Ok(projects)
}

#[tauri::command]
pub async fn get_modrinth_modpack_versions(project_id: String) -> Result<Vec<OnlineModpackVersion>, String> {
    tracing::info!("Getting Modrinth modpack versions: project_id={}", project_id);

    let versions_url = format!("{}/project/{}/version", MODRINTH_BASE_URL, project_id);

    let client = reqwest::Client::new();
    let response = client
        .get(&versions_url)
        .header("Accept", "application/json")
        .header("User-Agent", "DawnlandLauncher/1.0")
        .send()
        .await
        .map_err(|e| format!("Failed to send request: {:?}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        return Err(format!("Modrinth API error: {}", status));
    }

    let versions: Vec<ModrinthVersion> = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {:?}", e))?;

    let mut result = Vec::new();
    for version in versions {
        if let Some(file) = version.files.first() {
            result.push(OnlineModpackVersion {
                id: version.id,
                name: version.version_number.clone(),
                mc_version: version.game_versions.join(", "),
                loaders: version.loaders,
                download_url: file.url.clone(),
                date: version.date_published,
            });
        }
    }

    Ok(result)
}