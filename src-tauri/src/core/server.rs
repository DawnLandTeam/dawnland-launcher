#![allow(dead_code)]
//! Server management commands that proxy requests to the Go web backend.

use crate::models::{FilterOptionsResponse, Server};
use serde::{Deserialize, Serialize};
use tokio::task;

fn extract_api_error(status: reqwest::StatusCode, body: &str) -> String {
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(body) {
        if let Some(err_msg) = json.get("error").and_then(|v| v.as_str()) {
            return format!("{}: {}", status, err_msg);
        }
    }
    format!("{} - An unexpected error occurred.", status)
}

/// Get the web backend URL from environment or use default.
fn get_web_backend_url() -> String {
    option_env!("VITE_WEB_BACKEND_URL")
        .unwrap_or("http://localhost:3030")
        .to_string()
}

/// Server API path prefix on the web backend.
const SERVER_API_PATH: &str = "/api/servers";

/// Build the full URL to the web backend server API with query parameters.
pub fn build_server_url(path: &str, query_params: Option<&str>) -> String {
    let backend_url = get_web_backend_url();
    let base = format!("{}{}", backend_url, SERVER_API_PATH);
    match query_params {
        Some(q) => format!("{}{}?{}", base, path, q),
        None => format!("{}{}", base, path),
    }
}

/// Response from server list API with pagination info.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerListResponse {
    pub data: Vec<Server>,
    pub total: u64,
    pub page: u32,
    pub page_size: u32,
    pub total_pages: u32,
}

/// Fetch filter options from the Go backend.
#[tauri::command]
pub async fn get_servers(
    page: u32,
    page_size: u32,
    search: String,
    version: String,
    server_type: String,
    auth_type: String,
) -> Result<ServerListResponse, String> {
    tracing::debug!(
        "Fetching servers: page={}, pageSize={}, search={}, version={}, serverType={}, authType={}",
        page,
        page_size,
        search,
        version,
        server_type,
        auth_type
    );

    // Build query parameters
    let mut query_parts = vec![format!("page={}", page), format!("pageSize={}", page_size)];
    if !search.is_empty() {
        query_parts.push(format!("search={}", urlencoding::encode(&search)));
    }
    if !version.is_empty() {
        query_parts.push(format!("version={}", urlencoding::encode(&version)));
    }
    if !server_type.is_empty() {
        query_parts.push(format!("serverType={}", urlencoding::encode(&server_type)));
    }
    if !auth_type.is_empty() {
        query_parts.push(format!("authType={}", urlencoding::encode(&auth_type)));
    }
    let query_string = query_parts.join("&");

    let url = build_server_url("", Some(&query_string));
    tracing::debug!("Server API URL: {}", url);

    let client = crate::core::utils::get_http_client().clone();
    let response = crate::core::security::secure_request(&client, reqwest::Method::GET, &url, "")
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| format!("Failed to fetch servers: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        tracing::error!("Server API error: {} - {}", status, body);
        return Err(format!("Server API error: {}", status));
    }

    let result: ServerListResponse = crate::core::utils::parse_json_response(response).await?;

    tracing::debug!(
        "Fetched {} servers (page {}/{})",
        result.data.len(),
        result.page,
        result.total_pages
    );
    Ok(result)
}

/// Fetch recommended servers from the Go backend.
#[tauri::command]
pub async fn get_recommended_servers() -> Result<Vec<Server>, String> {
    tracing::debug!("Fetching recommended servers");

    let url = build_server_url("/recommended", None);
    tracing::debug!("Server API URL: {}", url);

    let client = crate::core::utils::get_http_client().clone();
    let response = crate::core::security::secure_request(&client, reqwest::Method::GET, &url, "")
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| format!("Failed to fetch recommended servers: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        tracing::error!("Server API error: {} - {}", status, crate::logger::redact::truncate_for_log(&body, 512));
        return Err(format!("Server API error: {}", status));
    }

    let result: Vec<Server> = crate::core::utils::parse_json_response(response).await?;

    tracing::debug!("Fetched {} recommended servers", result.len());
    Ok(result)
}

/// Fetch available filter options from the Go backend.
#[tauri::command]
pub async fn get_filter_options() -> Result<FilterOptionsResponse, String> {
    tracing::debug!("Fetching server filter options");

    let url = build_server_url("/filter-options", None);
    let client = crate::core::utils::get_http_client().clone();
    let response = crate::core::security::secure_request(&client, reqwest::Method::GET, &url, "")
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| format!("Failed to fetch filter options: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        tracing::error!("Filter options API error: {} - {}", status, crate::logger::redact::truncate_for_log(&body, 512));
        return Err(format!("Filter options API error: {}", status));
    }

    let result: FilterOptionsResponse = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse filter options response: {}", e))?;

    tracing::debug!(
        "Filter options: {} versions, {} server types, {} auth types",
        result.versions.len(),
        result.server_types.len(),
        result.auth_types.len()
    );
    Ok(result)
}

/// Fetch a single server by ID from the Go backend.
#[tauri::command]
pub async fn get_server(id: String) -> Result<Server, String> {
    tracing::debug!("Fetching server {} from web backend", id);

    let url = build_server_url(&format!("/{}", id), None);
    let client = crate::core::utils::get_http_client().clone();
    let response = crate::core::security::secure_request(&client, reqwest::Method::GET, &url, "")
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| format!("Failed to fetch server: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        if status.as_u16() == 404 {
            return Err("Server not found".to_string());
        }
        return Err(format!("Server API error: {}", status));
    }

    #[derive(Deserialize)]
    struct ApiResponse<T> {
        data: T,
    }

    let result: ApiResponse<Server> = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse server response: {}", e))?;

    Ok(result.data)
}

/// Download a modpack ZIP file for a server to a local path.
#[tauri::command]
pub async fn download_pack_file(
    server_id: String,
    destination_path: String,
) -> Result<String, String> {
    tracing::info!(
        "Downloading pack file for server {} to {}",
        server_id,
        destination_path
    );

    let url = build_server_url(&format!("/{}/pack", server_id), None);

    let client = crate::core::utils::get_http_client().clone();
    let response = crate::core::security::secure_request(&client, reqwest::Method::GET, &url, "")
        .send()
        .await
        .map_err(|e| format!("Failed to download pack file: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        if status.as_u16() == 404 {
            return Err("No pack file available for this server".to_string());
        }
        return Err(format!("Failed to download pack file: {}", status));
    }

    // Get the file bytes
    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("Failed to read pack file content: {}", e))?;

    // Ensure parent directory exists
    if let Some(parent) = std::path::Path::new(&destination_path).parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("Failed to create directory: {}", e))?;
    }

    // Write to file
    tokio::fs::write(&destination_path, &bytes)
        .await
        .map_err(|e| format!("Failed to save pack file: {}", e))?;

    tracing::info!(
        "Pack file downloaded: {} ({} bytes)",
        destination_path,
        bytes.len()
    );
    Ok(destination_path)
}

/// Install a modpack from a server (download, extract, and set up instance).
#[tauri::command]
pub async fn install_server_modpack(
    server_id: String,
    instance_name: String,
) -> Result<String, String> {
    tracing::info!(
        "Installing modpack from server {} as instance '{}'",
        server_id,
        instance_name
    );

    // First get the server info to check if it has a pack file
    let server = get_server(server_id.clone()).await?;

    // Check if server has a pack file
    let pack_file_name = server
        .pack_file_name
        .as_ref()
        .ok_or_else(|| "This server does not have a modpack file".to_string())?;

    if pack_file_name.is_empty() {
        return Err("This server does not have a modpack file".to_string());
    }

    // Get the instances directory path
    let base = std::env::current_exe()
        .map(|p| p.parent().unwrap().to_path_buf())
        .unwrap_or_else(|_| std::path::PathBuf::from("."));
    let instances_dir = base.join(".dawnland").join("instances");

    // Create the instance directory
    let instance_dir = instances_dir.join(&instance_name);
    let instance_dir_clone = instance_dir.clone();

    tokio::fs::create_dir_all(&instance_dir)
        .await
        .map_err(|e| format!("Failed to create instance directory: {}", e))?;

    // Download the pack file
    let pack_zip_path = instance_dir.join("modpack.zip");
    let pack_zip_path_clone = pack_zip_path.clone();
    let server_id_clone = server_id.clone();

    download_pack_file(
        server_id_clone,
        pack_zip_path_clone.to_string_lossy().to_string(),
    )
    .await?;

    // Extract the ZIP file using spawn_blocking for blocking I/O
    let instance_dir_for_blocking = instance_dir_clone.clone();
    let pack_zip_path_for_blocking = pack_zip_path_clone.clone();

    tokio::task::spawn_blocking(move || {
        let file = std::fs::File::open(&pack_zip_path_for_blocking)
            .map_err(|e| format!("Failed to open pack file: {}", e))?;
        let mut archive =
            zip::ZipArchive::new(file).map_err(|e| format!("Failed to read ZIP archive: {}", e))?;

        // Extract all files
        for i in 0..archive.len() {
            let mut file = archive
                .by_index(i)
                .map_err(|e| format!("Failed to read ZIP entry: {}", e))?;

            let outpath = match file.enclosed_name() {
                Some(path) => instance_dir_for_blocking.join(path),
                None => continue,
            };

            if file.name().ends_with('/') {
                // Directory
                std::fs::create_dir_all(&outpath)
                    .map_err(|e| format!("Failed to create directory: {}", e))?;
            } else {
                // File - ensure parent directory exists
                if let Some(parent) = outpath.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| format!("Failed to create parent directory: {}", e))?;
                }

                let mut outfile = std::fs::File::create(&outpath)
                    .map_err(|e| format!("Failed to create file: {}", e))?;
                std::io::copy(&mut file, &mut outfile)
                    .map_err(|e| format!("Failed to extract file: {}", e))?;
            }
        }

        // Clean up the ZIP file after extraction
        let _ = std::fs::remove_file(&pack_zip_path_for_blocking);

        Ok::<(), String>(())
    })
    .await
    .map_err(|e| format!("Failed to extract pack file: {}", e))??;

    // Try to read manifest.json to get version info
    let manifest_path = instance_dir.join("manifest.json");
    let version = if manifest_path.exists() {
        let content = tokio::fs::read_to_string(&manifest_path)
            .await
            .map_err(|e| format!("Failed to read manifest: {}", e))?;

        #[derive(Deserialize)]
        struct Manifest {
            #[serde(rename = "minecraft")]
            minecraft: Option<ManifestMinecraft>,
            #[serde(rename = "manifestType")]
            manifest_type: Option<String>,
        }

        #[derive(Deserialize)]
        struct ManifestMinecraft {
            #[serde(rename = "version")]
            version: Option<String>,
        }

        if let Ok(manifest) = serde_json::from_str::<Manifest>(&content) {
            manifest
                .minecraft
                .and_then(|m| m.version)
                .unwrap_or_else(|| "unknown".to_string())
        } else {
            "unknown".to_string()
        }
    } else {
        // Use the server's version if no manifest
        server.version.clone()
    };

    // Create a basic instance config
    let config = serde_json::json!({
        "name": instance_name,
        "version": version,
        "serverAddress": format!("{}:{}", server.ip, server.port),
        "source": "server",
        "sourceServerId": server_id,
    });

    let config_path = instance_dir.join("config.json");
    tokio::fs::write(&config_path, serde_json::to_string_pretty(&config).unwrap())
        .await
        .map_err(|e| format!("Failed to save instance config: {}", e))?;

    tracing::info!(
        "Modpack installed successfully to: {}",
        instance_dir.display()
    );
    Ok(instance_dir.to_string_lossy().to_string())
}


#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ClientConfigResponse {
    pub curseforge_api_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LauncherAnnouncementResponse {
    pub announcements: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawUpdatePlatform {
    url: Option<String>,
    md5: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawLauncherUpdateResponse {
    version: Option<String>,
    notes: Option<String>,
    platforms: Option<std::collections::HashMap<String, RawUpdatePlatform>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LauncherUpdateInfo {
    pub version: String,
    pub body: String,
    pub md5: Option<String>,
    pub url: Option<String>,
}

fn get_update_target_os() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows-standalone"
    } else {
        "linux-standalone"
    }
}

async fn fetch_client_config_with_base(
    client: &reqwest::Client,
    base_url: &str,
) -> Result<ClientConfigResponse, String> {
    let url = format!("{}/api/client-config", base_url.trim_end_matches('/'));
    let response = crate::core::security::secure_request(client, reqwest::Method::GET, &url, "")
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| format!("Failed to fetch client config: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(extract_api_error(status, &body));
    }

    crate::core::utils::parse_json_response(response).await
}

/// Fetch public client configuration (such as CurseForge API key) from the Go web backend.
#[tauri::command]
pub async fn fetch_client_config() -> Result<ClientConfigResponse, String> {
    fetch_client_config_with_base(crate::core::utils::get_http_client(), &get_web_backend_url())
        .await
}

async fn fetch_launcher_announcement_with_base(
    client: &reqwest::Client,
    base_url: &str,
) -> Result<LauncherAnnouncementResponse, String> {
    let url = format!(
        "{}/api/launcher/announcement",
        base_url.trim_end_matches('/')
    );
    let response = crate::core::security::secure_request(client, reqwest::Method::GET, &url, "")
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| format!("Failed to fetch launcher announcement: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(extract_api_error(status, &body));
    }

    crate::core::utils::parse_json_response(response).await
}

/// Fetch active launcher announcements from the Go web backend.
#[tauri::command]
pub async fn fetch_launcher_announcement() -> Result<LauncherAnnouncementResponse, String> {
    fetch_launcher_announcement_with_base(
        crate::core::utils::get_http_client(),
        &get_web_backend_url(),
    )
    .await
}

/// Parse a launcher version string into strict SemVer 2.0.0 after trimming whitespace
/// and stripping at most one leading `v` or `V`.
pub fn parse_launcher_semver(raw: &str) -> Option<semver::Version> {
    let trimmed = raw.trim();
    let normalized = trimmed
        .strip_prefix('v')
        .or_else(|| trimmed.strip_prefix('V'))
        .unwrap_or(trimmed);
    if normalized.is_empty() {
        return None;
    }
    semver::Version::parse(normalized).ok()
}

/// Check whether upgrading from `current_version` to `candidate_version` is permitted
/// under the selected update `channel` (`"prerelease"` or stable).
pub fn is_launcher_update_allowed(
    current_version: &str,
    candidate_version: &str,
    channel: Option<&str>,
) -> bool {
    let Some(current) = parse_launcher_semver(current_version) else {
        return false;
    };
    let Some(candidate) = parse_launcher_semver(candidate_version) else {
        return false;
    };
    if channel != Some("prerelease") && !candidate.pre.is_empty() {
        return false;
    }
    candidate.cmp_precedence(&current) == std::cmp::Ordering::Greater
}

async fn check_launcher_update_with_base(
    client: &reqwest::Client,
    base_url: &str,
    target_os: &str,
    current_version: &str,
    channel: Option<&str>,
) -> Result<Option<LauncherUpdateInfo>, String> {
    let query = if channel == Some("prerelease") {
        "?channel=prerelease"
    } else {
        ""
    };
    let url = format!(
        "{}/api/launcher/update/{}/{}{}",
        base_url.trim_end_matches('/'),
        target_os,
        current_version,
        query
    );

    let response = client
        .get(&url)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| format!("Failed to check launcher update: {}", e))?;

    let status = response.status();
    if status == reqwest::StatusCode::NO_CONTENT {
        return Ok(None);
    }

    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(extract_api_error(status, &body));
    }

    let raw: RawLauncherUpdateResponse = crate::core::utils::parse_json_response(response).await?;
    let Some(version) = raw.version.filter(|v| !v.is_empty()) else {
        return Ok(None);
    };

    if !is_launcher_update_allowed(current_version, &version, channel) {
        tracing::warn!(
            target: "dawnland_launcher_lib::updater",
            current_version = %current_version,
            candidate_version = %version,
            channel = ?channel,
            "Ignoring launcher update candidate rejected by SemVer/channel policy"
        );
        return Ok(None);
    }

    let Some(platform_data) = raw.platforms.and_then(|mut p| p.remove(target_os)) else {
        tracing::warn!("No update asset found for target OS: {}", target_os);
        return Ok(None);
    };

    Ok(Some(LauncherUpdateInfo {
        version,
        body: raw.notes.unwrap_or_default(),
        md5: platform_data.md5,
        url: platform_data.url,
    }))
}

/// Check for launcher updates via the Go web backend.
#[tauri::command]
pub async fn check_launcher_update(
    app: tauri::AppHandle,
    channel: Option<String>,
) -> Result<Option<LauncherUpdateInfo>, String> {
    let current_version = app.package_info().version.to_string();
    check_launcher_update_with_base(
        crate::core::utils::get_http_client(),
        &get_web_backend_url(),
        get_update_target_os(),
        &current_version,
        channel.as_deref(),
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::utils::{get_http_client, APP_USER_AGENT};
    use mockito::{Matcher, Server};

    #[tokio::test]
    async fn test_fetch_client_config_sends_unified_user_agent_and_signature() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("GET", "/api/client-config")
            .match_header("user-agent", APP_USER_AGENT)
            .match_header("x-launcher-time", Matcher::Any)
            .match_header("x-launcher-signature", Matcher::Any)
            .with_status(200)
            .with_body(r#"{"curseforgeApiKey":"cf-test-key"}"#)
            .create_async()
            .await;

        let res = fetch_client_config_with_base(get_http_client(), &server.url())
            .await
            .expect("fetch_client_config should succeed");
        assert_eq!(res.curseforge_api_key.as_deref(), Some("cf-test-key"));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_fetch_launcher_announcement_sends_unified_user_agent() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("GET", "/api/launcher/announcement")
            .match_header("user-agent", APP_USER_AGENT)
            .match_header("x-launcher-time", Matcher::Any)
            .match_header("x-launcher-signature", Matcher::Any)
            .with_status(200)
            .with_body(r#"{"announcements":["Hello DLML"]}"#)
            .create_async()
            .await;

        let res = fetch_launcher_announcement_with_base(get_http_client(), &server.url())
            .await
            .expect("fetch_launcher_announcement should succeed");
        assert_eq!(res.announcements, vec!["Hello DLML".to_string()]);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_check_launcher_update_handles_200_and_204() {
        let mut server = Server::new_async().await;
        let mock_ok = server
            .mock(
                "GET",
                "/api/launcher/update/windows-standalone/0.0.1?channel=prerelease",
            )
            .match_header("user-agent", APP_USER_AGENT)
            .with_status(200)
            .with_body(
                r#"{
                    "version": "1.2.3",
                    "notes": "Release notes",
                    "platforms": {
                        "windows-standalone": {
                            "url": "https://dl.dlml.app/releases/v1.2.3/DLML.exe",
                            "md5": "abc123"
                        }
                    }
                }"#,
            )
            .create_async()
            .await;

        let update = check_launcher_update_with_base(
            get_http_client(),
            &server.url(),
            "windows-standalone",
            "0.0.1",
            Some("prerelease"),
        )
        .await
        .expect("check_launcher_update 200 should succeed");
        assert_eq!(
            update,
            Some(LauncherUpdateInfo {
                version: "1.2.3".to_string(),
                body: "Release notes".to_string(),
                md5: Some("abc123".to_string()),
                url: Some("https://dl.dlml.app/releases/v1.2.3/DLML.exe".to_string()),
            })
        );
        mock_ok.assert_async().await;

        let mock_no_content = server
            .mock("GET", "/api/launcher/update/linux-standalone/1.2.3")
            .match_header("user-agent", APP_USER_AGENT)
            .with_status(204)
            .create_async()
            .await;

        let up_to_date = check_launcher_update_with_base(
            get_http_client(),
            &server.url(),
            "linux-standalone",
            "1.2.3",
            Some("stable"),
        )
        .await
        .expect("check_launcher_update 204 should succeed");
        assert_eq!(up_to_date, None);
        mock_no_content.assert_async().await;
    }

    #[test]
    fn test_is_launcher_update_allowed_matrix() {
        // Block prerelease -> older stable downgrade
        assert!(!is_launcher_update_allowed(
            "0.1.6-rc.4",
            "0.1.5",
            Some("stable")
        ));
        assert!(!is_launcher_update_allowed(
            "0.1.6-rc.4",
            "0.1.5",
            Some("prerelease")
        ));
        assert!(!is_launcher_update_allowed("0.1.6-rc.4", "0.1.5", None));

        // Allow 0.1.6-rc.3 -> 0.1.6-rc.4 -> 0.1.6
        assert!(is_launcher_update_allowed(
            "0.1.6-rc.3",
            "0.1.6-rc.4",
            Some("prerelease")
        ));
        assert!(!is_launcher_update_allowed(
            "0.1.6-rc.3",
            "0.1.6-rc.4",
            Some("stable")
        ));
        assert!(is_launcher_update_allowed(
            "0.1.6-rc.4",
            "0.1.6",
            Some("stable")
        ));
        assert!(is_launcher_update_allowed(
            "0.1.6-rc.4",
            "0.1.6",
            Some("prerelease")
        ));

        // Block downgrades and same version
        assert!(!is_launcher_update_allowed(
            "0.1.6-rc.4",
            "0.1.6-rc.3",
            Some("prerelease")
        ));
        assert!(!is_launcher_update_allowed(
            "0.1.6",
            "0.1.6-rc.4",
            Some("prerelease")
        ));
        assert!(!is_launcher_update_allowed(
            "0.1.6",
            "0.1.6-rc.4",
            Some("stable")
        ));
        assert!(!is_launcher_update_allowed(
            "0.1.6-rc.4",
            "0.1.6-rc.4",
            Some("prerelease")
        ));
        assert!(!is_launcher_update_allowed(
            "0.1.6-rc.4",
            "v0.1.6-rc.4",
            Some("prerelease")
        ));

        // SemVer 2.0.0 prerelease ordering and build metadata
        assert!(is_launcher_update_allowed(
            "0.1.6-rc.9",
            "0.1.6-rc.10",
            Some("prerelease")
        ));
        assert!(is_launcher_update_allowed(
            "0.1.6-rc",
            "0.1.6-rc.1",
            Some("prerelease")
        ));
        assert!(is_launcher_update_allowed(
            " V0.1.6-rc.3 ",
            " V0.1.6-rc.4 ",
            Some("prerelease")
        ));
        assert!(!is_launcher_update_allowed(
            "0.1.6+a",
            "0.1.6+b",
            Some("stable")
        ));

        // Reject invalid versions (fail-closed)
        assert!(!is_launcher_update_allowed(
            "invalid",
            "0.1.6",
            Some("stable")
        ));
        assert!(!is_launcher_update_allowed(
            "0.1.5",
            "vv0.1.6",
            Some("stable")
        ));
        assert!(!is_launcher_update_allowed("", "0.1.6", Some("stable")));
    }

    #[tokio::test]
    async fn test_check_launcher_update_blocks_downgrade_and_channel_mismatch() {
        let mut server = Server::new_async().await;

        // Server erroneously returns 0.1.5 (200 OK) for 0.1.6-rc.4 on stable channel -> client must reject
        let mock_downgrade = server
            .mock("GET", "/api/launcher/update/windows-standalone/0.1.6-rc.4")
            .with_status(200)
            .with_body(
                r#"{
                    "version": "0.1.5",
                    "notes": "Old stable",
                    "platforms": {
                        "windows-standalone": {
                            "url": "https://dl.dlml.app/releases/v0.1.5/DLML.exe",
                            "md5": "oldmd5"
                        }
                    }
                }"#,
            )
            .create_async()
            .await;

        let downgraded = check_launcher_update_with_base(
            get_http_client(),
            &server.url(),
            "windows-standalone",
            "0.1.6-rc.4",
            Some("stable"),
        )
        .await
        .expect("check should succeed and ignore downgrade");
        assert_eq!(downgraded, None);
        mock_downgrade.assert_async().await;

        // Server erroneously returns 0.1.6-rc.5 on stable channel -> client must reject
        let mock_channel_leak = server
            .mock("GET", "/api/launcher/update/windows-standalone/0.1.6-rc.4")
            .with_status(200)
            .with_body(
                r#"{
                    "version": "0.1.6-rc.5",
                    "notes": "Leaked RC",
                    "platforms": {
                        "windows-standalone": {
                            "url": "https://dl.dlml.app/releases/v0.1.6-rc.5/DLML.exe",
                            "md5": "rc5md5"
                        }
                    }
                }"#,
            )
            .create_async()
            .await;

        let leaked = check_launcher_update_with_base(
            get_http_client(),
            &server.url(),
            "windows-standalone",
            "0.1.6-rc.4",
            Some("stable"),
        )
        .await
        .expect("check should succeed and ignore prerelease on stable channel");
        assert_eq!(leaked, None);
        mock_channel_leak.assert_async().await;

        // Server returns 0.1.6 for 0.1.6-rc.4 on stable channel -> client accepts
        let mock_upgrade_stable = server
            .mock("GET", "/api/launcher/update/windows-standalone/0.1.6-rc.4")
            .with_status(200)
            .with_body(
                r#"{
                    "version": "0.1.6",
                    "notes": "Stable 0.1.6",
                    "platforms": {
                        "windows-standalone": {
                            "url": "https://dl.dlml.app/releases/v0.1.6/DLML.exe",
                            "md5": "stable016"
                        }
                    }
                }"#,
            )
            .create_async()
            .await;

        let upgraded_stable = check_launcher_update_with_base(
            get_http_client(),
            &server.url(),
            "windows-standalone",
            "0.1.6-rc.4",
            Some("stable"),
        )
        .await
        .expect("check should allow rc.4 -> 0.1.6 on stable channel");
        assert_eq!(
            upgraded_stable.as_ref().map(|u| u.version.as_str()),
            Some("0.1.6")
        );
        mock_upgrade_stable.assert_async().await;

        // Server returns 0.1.6-rc.4 for 0.1.6-rc.3 on prerelease channel -> client accepts
        let mock_upgrade_rc = server
            .mock(
                "GET",
                "/api/launcher/update/windows-standalone/0.1.6-rc.3?channel=prerelease",
            )
            .with_status(200)
            .with_body(
                r#"{
                    "version": "0.1.6-rc.4",
                    "notes": "Preview 0.1.6-rc.4",
                    "platforms": {
                        "windows-standalone": {
                            "url": "https://dl.dlml.app/releases/v0.1.6-rc.4/DLML.exe",
                            "md5": "rc4md5"
                        }
                    }
                }"#,
            )
            .create_async()
            .await;

        let upgraded_rc = check_launcher_update_with_base(
            get_http_client(),
            &server.url(),
            "windows-standalone",
            "0.1.6-rc.3",
            Some("prerelease"),
        )
        .await
        .expect("check should allow rc.3 -> rc.4 on prerelease channel");
        assert_eq!(
            upgraded_rc.as_ref().map(|u| u.version.as_str()),
            Some("0.1.6-rc.4")
        );
        mock_upgrade_rc.assert_async().await;
    }
}
