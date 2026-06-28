use serde::{Deserialize, Serialize};

/// Server entity representing a multiplayer Minecraft server.
/// Mirrors the Go backend model in web-backend/models/server.go
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Server {
    pub id: u32,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub name: String,
    pub ip: String,
    pub port: i32,
    pub motd: Option<String>,
    pub version: String,
    pub server_type: String, // "vanilla", "modded", "custom"
    pub auth_type: String,   // "offline", "online", "authlib"
    pub authlib_api: Option<String>,
    pub pack_file_name: Option<String>,  // Modpack ZIP file name
    pub pack_file_size: Option<i64>,     // Modpack file size in bytes
    pub pack_project_id: Option<String>, // CurseForge or Modrinth Project ID
    pub pack_version_id: Option<String>, // CurseForge or Modrinth Version/File ID
    pub pack_source: Option<String>,     // "curseforge" or "modrinth"
    pub icon_url: Option<String>,
    pub email: Option<String>,
    pub is_active: bool,
    pub tags: Option<String>,
    pub description: Option<String>,
    pub contact_group: Option<String>,
    pub contact_owner: Option<String>,
}

/// Response from filter options API.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilterOptionsResponse {
    pub versions: Vec<String>,
    pub server_types: Vec<String>,
    pub auth_types: Vec<String>,
}
