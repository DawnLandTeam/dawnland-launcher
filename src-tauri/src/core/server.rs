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
    tracing::info!(
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

    tracing::info!(
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
    tracing::info!("Fetching recommended servers");

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
        tracing::error!("Server API error: {} - {}", status, body);
        return Err(format!("Server API error: {}", status));
    }

    let result: Vec<Server> = crate::core::utils::parse_json_response(response).await?;

    tracing::info!("Fetched {} recommended servers", result.len());
    Ok(result)
}

/// Fetch available filter options from the Go backend.
#[tauri::command]
pub async fn get_filter_options() -> Result<FilterOptionsResponse, String> {
    tracing::info!("Fetching server filter options");

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
        tracing::error!("Filter options API error: {} - {}", status, body);
        return Err(format!("Filter options API error: {}", status));
    }

    let result: FilterOptionsResponse = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse filter options response: {}", e))?;

    tracing::info!(
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
    tracing::info!("Fetching server {} from web backend", id);

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
