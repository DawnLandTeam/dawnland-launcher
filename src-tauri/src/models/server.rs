use serde::{Deserialize, Serialize};

/// Server entity representing a multiplayer Minecraft server.
/// Mirrors the Go backend model in web-backend/models/server.go
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Server {
    pub id: u32,
    pub created_at: String,
    pub updated_at: String,
    pub name: String,
    pub ip: String,
    pub port: i32,
    pub motd: String,
    pub version: String,
    pub loader_type: String,
    pub server_type: String, // "vanilla", "modded", "custom"
    pub auth_type: String,   // "offline", "online", "authlib"
    pub authlib_api: Option<String>,
    pub pack_file_name: Option<String>,  // Modpack ZIP file name
    pub pack_file_size: Option<i64>,     // Modpack file size in bytes
    pub pack_project_id: Option<String>, // CurseForge or Modrinth Project ID
    pub pack_version_id: Option<String>, // CurseForge or Modrinth Version/File ID
    pub pack_source: Option<String>,     // "curseforge" or "modrinth"
    pub icon_url: String,
    pub email: String,
    pub is_active: bool,
    pub tags: Option<String>,
    pub description: Option<String>,
    pub contact_group: Option<String>,
    pub contact_owner: Option<String>,
}

/// Input for creating a new server.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateServerInput {
    pub name: String,
    pub ip: String,
    pub port: i32,
    pub motd: String,
    pub version: String,
    pub loader_type: String,
    pub server_type: String, // "vanilla", "modded", "custom"
    pub auth_type: String,   // "offline", "online", "authlib"
    pub authlib_api: Option<String>,
    pub pack_file_name: Option<String>, // Set after pack is uploaded
    pub pack_project_id: Option<String>,
    pub pack_version_id: Option<String>,
    pub pack_source: Option<String>,
    pub icon_url: String,
    pub email: String,
    pub tags: Option<String>,
    pub description: Option<String>,
    pub contact_group: Option<String>,
    pub contact_owner: Option<String>,
}

/// Input for updating a server (all fields optional).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateServerInput {
    pub name: Option<String>,
    pub ip: Option<String>,
    pub port: Option<i32>,
    pub motd: Option<String>,
    pub version: Option<String>,
    pub loader_type: Option<String>,
    pub server_type: Option<String>,
    pub auth_type: Option<String>,
    pub authlib_api: Option<String>,
    pub pack_file_name: Option<String>,
    pub pack_project_id: Option<String>,
    pub pack_version_id: Option<String>,
    pub pack_source: Option<String>,
    pub icon_url: Option<String>,
    pub email: Option<String>,
    pub is_active: Option<bool>,
    pub tags: Option<String>,
    pub description: Option<String>,
    pub contact_group: Option<String>,
    pub contact_owner: Option<String>,
}

/// Response from pack file upload
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackFileResponse {
    pub message: String,
    pub pack_file_name: String,
    pub pack_file_size: i64,
}

/// Response from filter options API.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilterOptionsResponse {
    pub versions: Vec<String>,
    pub server_types: Vec<String>,
    pub auth_types: Vec<String>,
}
