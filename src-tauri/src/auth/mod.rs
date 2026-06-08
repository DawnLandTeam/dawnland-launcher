#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(unused_variables)]
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;
pub mod authlib;
pub mod microsoft;

pub use authlib::{
    add_authlib_account, add_authlib_server, fetch_authlib_servers, get_authlib_meta,
    remove_authlib_server,
};
pub use microsoft::{
    login_microsoft_oauth, poll_microsoft_token, refresh_microsoft_token, start_microsoft_login,
    LoginInitResponse,
};

/// Account types supported by the launcher.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum AccountType {
    Offline,
    Microsoft,
    Authlib,
}

/// A player account stored in the launcher.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    /// Unique account ID (UUID).
    pub id: String,
    /// Player's in-game username.
    pub username: String,
    /// Type of account.
    pub account_type: AccountType,
    /// Microsoft access token (for MC services).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access_token: Option<String>,
    /// Optional refresh token for Microsoft accounts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    /// Player's texture URL (cape, skin).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub textures: Option<String>,
    /// Optional Yggdrasil API URL for Authlib accounts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authlib_url: Option<String>,
    /// Optional Yggdrasil Server Name for Authlib accounts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authlib_server_name: Option<String>,
    /// Optional Yggdrasil Client Token for Authlib accounts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_token: Option<String>,
}

/// Get the accounts file path.
fn accounts_file_path() -> Result<PathBuf, String> {
    let base = std::env::current_exe()
        .map(|p| p.parent().unwrap().to_path_buf())
        .unwrap_or_else(|_| PathBuf::from("."));
    Ok(base.join(".dawnland").join("accounts.json"))
}

/// Load all accounts from disk.
pub async fn load_accounts() -> Result<Vec<Account>, String> {
    let path = accounts_file_path()?;

    if !path.exists() {
        return Ok(Vec::new());
    }

    let content = tokio::fs::read_to_string(&path)
        .await
        .map_err(|e| format!("Failed to read accounts file: {e}"))?;

    let accounts: Vec<Account> = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse accounts file: {e}"))?;

    Ok(accounts)
}

/// Save all accounts to disk.
pub async fn save_accounts(accounts: &[Account]) -> Result<(), String> {
    let path = accounts_file_path()?;

    // Ensure parent directory exists.
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("Failed to create directory: {e}"))?;
    }

    let content = serde_json::to_string_pretty(accounts)
        .map_err(|e| format!("Failed to serialize accounts: {e}"))?;

    tokio::fs::write(&path, content)
        .await
        .map_err(|e| format!("Failed to write accounts file: {e}"))?;

    Ok(())
}

/// Generate offline player UUID v3 from username.
/// Uses "OfflinePlayer:" namespace as per Minecraft convention.
pub fn generate_offline_uuid(username: &str) -> String {
    // Namespace UUID for offline players (Mojang convention)
    let namespace_uuid =
        Uuid::parse_str("068e7e19-b9f9-4e11-a3d7-0050c06a030c").expect("Valid UUID");
    Uuid::new_v3(&namespace_uuid, username.as_bytes()).to_string()
}

/// Add a new offline account.
pub async fn add_offline_account(username: &str) -> Result<Account, String> {
    if username.trim().is_empty() {
        return Err("Username cannot be empty".to_string());
    }

    let mut accounts = load_accounts().await?;

    // Check if account already exists.
    if accounts
        .iter()
        .any(|a| a.username == username && a.account_type == AccountType::Offline)
    {
        return Err("Offline account already exists".to_string());
    }

    let account = Account {
        id: generate_offline_uuid(username),
        username: username.to_string(),
        account_type: AccountType::Offline,
        access_token: None,
        refresh_token: None,
        textures: None,
        authlib_url: None,
        authlib_server_name: None,
        client_token: None,
    };

    accounts.push(account.clone());
    save_accounts(&accounts).await?;

    Ok(account)
}

/// Get all accounts.
pub async fn get_accounts() -> Result<Vec<Account>, String> {
    load_accounts().await
}

/// Remove an account by ID.
pub async fn remove_account(id: &str) -> Result<(), String> {
    let mut accounts = load_accounts().await?;
    let original_len = accounts.len();
    accounts.retain(|a| a.id != id);

    if accounts.len() == original_len {
        return Err("Account not found".to_string());
    }

    save_accounts(&accounts).await
}

/// Update an existing account.
pub async fn update_account(account: Account) -> Result<(), String> {
    let mut accounts = load_accounts().await?;

    if let Some(existing) = accounts.iter_mut().find(|a| a.id == account.id) {
        *existing = account;
    } else {
        return Err("Account not found".to_string());
    }

    save_accounts(&accounts).await
}
