use reqwest::Client;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::{get_accounts, save_accounts, Account, AccountType};

#[derive(Serialize)]
struct Agent {
    name: String,
    version: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AuthenticateRequest {
    agent: Agent,
    username: String,
    password: String,
    client_token: String,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct YggdrasilProfile {
    pub id: String,
    pub name: String,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct AuthenticateResponse {
    access_token: String,
    client_token: String,
    available_profiles: Option<Vec<YggdrasilProfile>>,
    selected_profile: Option<YggdrasilProfile>,
}

#[derive(Deserialize, Debug)]
struct YggdrasilError {
    error: Option<String>,
    #[serde(rename = "errorMessage")]
    error_message: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthlibAuthResult {
    pub access_token: String,
    pub client_token: String,
    pub available_profiles: Vec<YggdrasilProfile>,
    pub authlib_server_name: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct YggdrasilMetaLinks {
    pub homepage: Option<String>,
    pub register: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct YggdrasilMeta {
    pub server_name: Option<String>,
    pub links: Option<YggdrasilMetaLinks>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct YggdrasilRootResponse {
    pub meta: Option<YggdrasilMeta>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AuthlibServer {
    pub url: String,
    pub name: String,
}

pub async fn get_authlib_servers() -> Result<Vec<AuthlibServer>, String> {
    let mut config_path = std::env::current_exe()
        .map(|p| p.parent().unwrap().to_path_buf())
        .unwrap_or_else(|_| std::path::PathBuf::from("."));
    config_path.push(".dawnland");
    std::fs::create_dir_all(&config_path).map_err(|e| e.to_string())?;
    config_path.push("authlib_servers.json");

    if !config_path.exists() {
        return Ok(Vec::new());
    }

    let contents = tokio::fs::read_to_string(&config_path)
        .await
        .map_err(|e| e.to_string())?;
    let servers: Vec<AuthlibServer> = serde_json::from_str(&contents).unwrap_or_default();
    Ok(servers)
}

#[tauri::command]
pub async fn fetch_authlib_servers() -> Result<Vec<AuthlibServer>, String> {
    get_authlib_servers().await
}

#[tauri::command]
pub async fn add_authlib_server(url: String) -> Result<AuthlibServer, String> {
    let meta = get_authlib_meta(url.clone()).await?;
    let name = meta
        .meta
        .and_then(|m| m.server_name)
        .unwrap_or_else(|| "Unknown Server".to_string());
    let server = AuthlibServer {
        url: url.clone(),
        name,
    };

    let mut servers = get_authlib_servers().await?;
    servers.retain(|s| s.url != url);
    servers.push(server.clone());

    let mut config_path = std::env::current_exe()
        .map(|p| p.parent().unwrap().to_path_buf())
        .unwrap_or_else(|_| std::path::PathBuf::from("."));
    config_path.push(".dawnland");
    std::fs::create_dir_all(&config_path).map_err(|e| e.to_string())?;
    config_path.push("authlib_servers.json");

    let json = serde_json::to_string_pretty(&servers).map_err(|e| e.to_string())?;
    tokio::fs::write(&config_path, json)
        .await
        .map_err(|e| e.to_string())?;

    Ok(server)
}

#[tauri::command]
pub async fn remove_authlib_server(url: String) -> Result<(), String> {
    let mut servers = get_authlib_servers().await?;
    servers.retain(|s| s.url != url);

    let mut config_path = std::env::current_exe()
        .map(|p| p.parent().unwrap().to_path_buf())
        .unwrap_or_else(|_| std::path::PathBuf::from("."));
    config_path.push(".dawnland");
    std::fs::create_dir_all(&config_path).map_err(|e| e.to_string())?;
    config_path.push("authlib_servers.json");

    let json = serde_json::to_string_pretty(&servers).map_err(|e| e.to_string())?;
    tokio::fs::write(&config_path, json)
        .await
        .map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
pub async fn get_authlib_meta(url: String) -> Result<YggdrasilRootResponse, String> {
    let client = Client::new();
    let res = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    if !res.status().is_success() {
        return Err(format!("Server returned status {}", res.status()));
    }

    let meta_res: YggdrasilRootResponse = res
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    Ok(meta_res)
}

#[tauri::command]
pub async fn authenticate_authlib_user(
    url: String,
    username: String,
    password: String,
) -> Result<AuthlibAuthResult, String> {
    let client_token = Uuid::new_v4().to_string();
    let client = Client::new();

    let auth_url = if url.ends_with('/') {
        format!("{}authserver/authenticate", url)
    } else {
        format!("{}/authserver/authenticate", url)
    };

    let req_body = AuthenticateRequest {
        agent: Agent {
            name: "Minecraft".to_string(),
            version: 1,
        },
        username: username.clone(),
        password,
        client_token: client_token.clone(),
    };

    let res = client
        .post(&auth_url)
        .json(&req_body)
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    if !res.status().is_success() {
        let err_body = res.text().await.unwrap_or_default();
        if let Ok(ygg_err) = serde_json::from_str::<YggdrasilError>(&err_body) {
            return Err(ygg_err.error_message.unwrap_or_else(|| {
                ygg_err
                    .error
                    .unwrap_or_else(|| "Unknown authentication error".to_string())
            }));
        }
        return Err("Authentication failed".to_string());
    }

    let auth_res: AuthenticateResponse = res
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    let mut profiles = auth_res.available_profiles.unwrap_or_default();
    if profiles.is_empty() {
        if let Some(sp) = auth_res.selected_profile {
            profiles.push(sp);
        }
    }

    if profiles.is_empty() {
        return Err("No profile available for this account".to_string());
    }

    let authlib_server_name = get_authlib_meta(url)
        .await
        .ok()
        .and_then(|r| r.meta.and_then(|m| m.server_name));

    Ok(AuthlibAuthResult {
        access_token: auth_res.access_token,
        client_token: auth_res.client_token,
        available_profiles: profiles,
        authlib_server_name,
    })
}

#[tauri::command]
pub async fn save_authlib_accounts(
    url: String,
    selected_profiles: Vec<YggdrasilProfile>,
    access_token: String,
    client_token: String,
    authlib_server_name: Option<String>,
) -> Result<Vec<Account>, String> {
    if selected_profiles.is_empty() {
        return Err("No profiles selected".to_string());
    }

    let mut accounts = get_accounts().await?;
    let mut added_accounts = Vec::new();

    for profile in selected_profiles {
        let uuid_str = profile.id;
        let uuid_with_hyphens = if uuid_str.len() == 32 {
            format!(
                "{}-{}-{}-{}-{}",
                &uuid_str[0..8],
                &uuid_str[8..12],
                &uuid_str[12..16],
                &uuid_str[16..20],
                &uuid_str[20..32]
            )
        } else {
            uuid_str
        };

        let account = Account {
            id: uuid_with_hyphens.clone(),
            username: profile.name,
            account_type: AccountType::Authlib,
            access_token: Some(access_token.clone()),
            refresh_token: None, // Authlib doesn't use standard oauth refresh tokens usually
            textures: None,
            authlib_url: Some(url.clone()),
            authlib_server_name: authlib_server_name.clone(),
            client_token: Some(client_token.clone()),
        };

        // Remove existing Authlib account with same UUID if exists.
        accounts.retain(|a| {
            let same_type = a.account_type == AccountType::Authlib;
            let same_id =
                a.id.replace("-", "")
                    .eq_ignore_ascii_case(&uuid_with_hyphens.replace("-", ""));
            !(same_type && same_id)
        });

        accounts.push(account.clone());
        added_accounts.push(account);
    }

    save_accounts(&accounts).await?;

    Ok(added_accounts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Server;
    use tokio::sync::Mutex;
    use std::sync::LazyLock;

    static TEST_MUTEX: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    async fn clear_authlib_files() {
        let mut config_path = std::env::current_exe()
            .map(|p| p.parent().unwrap().to_path_buf())
            .unwrap_or_else(|_| std::path::PathBuf::from("."));
        config_path.push(".dawnland");
        config_path.push("authlib_servers.json");
        let _ = tokio::fs::remove_file(config_path).await;
        
        let _ = crate::auth::save_accounts(&[]).await;
    }

    #[tokio::test]
    async fn test_get_authlib_meta() {
        let mut server = Server::new_async().await;
        let mock = server.mock("GET", "/")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{
                "meta": {
                    "serverName": "Test Authlib Server",
                    "links": {
                        "homepage": "https://test.com",
                        "register": "https://test.com/register"
                    }
                }
            }"#)
            .create_async().await;

        let result = get_authlib_meta(server.url()).await;
        assert!(result.is_ok());
        let meta = result.unwrap();
        assert_eq!(meta.meta.unwrap().server_name.unwrap(), "Test Authlib Server");
        
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_add_remove_authlib_server() {
        let _guard = TEST_MUTEX.lock().await;
        clear_authlib_files().await;

        let mut server = Server::new_async().await;
        let mock = server.mock("GET", "/")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"meta": {"serverName": "Mock Server"}}"#)
            .create_async().await;

        let url = server.url();

        let add_res = add_authlib_server(url.clone()).await;
        assert!(add_res.is_ok());
        assert_eq!(add_res.unwrap().name, "Mock Server");

        let servers = fetch_authlib_servers().await.unwrap();
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].url, url);

        let remove_res = remove_authlib_server(url.clone()).await;
        assert!(remove_res.is_ok());

        let servers_after = fetch_authlib_servers().await.unwrap();
        assert_eq!(servers_after.len(), 0);

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_authenticate_and_save_authlib_account() {
        let _guard = TEST_MUTEX.lock().await;
        clear_authlib_files().await;

        let mut server = Server::new_async().await;
        
        // Mock root meta
        let mock_meta = server.mock("GET", "/")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"meta": {"serverName": "My Authlib"}}"#)
            .create_async().await;

        // Mock authenticate
        let mock_auth = server.mock("POST", "/authserver/authenticate")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{
                "accessToken": "mock_access_token",
                "clientToken": "mock_client_token",
                "availableProfiles": [
                    {
                        "id": "1234567890abcdef1234567890abcdef",
                        "name": "AuthlibPlayer1"
                    },
                    {
                        "id": "abcdef1234567890abcdef1234567890",
                        "name": "AuthlibPlayer2"
                    }
                ]
            }"#)
            .create_async().await;

        let result = authenticate_authlib_user(server.url(), "user".to_string(), "pass".to_string()).await;
        assert!(result.is_ok(), "Failed: {:?}", result.err());
        let auth_res = result.unwrap();

        assert_eq!(auth_res.available_profiles.len(), 2);
        assert_eq!(auth_res.available_profiles[0].name, "AuthlibPlayer1");
        assert_eq!(auth_res.authlib_server_name, Some("My Authlib".to_string()));

        let save_result = save_authlib_accounts(
            server.url(),
            auth_res.available_profiles,
            auth_res.access_token,
            auth_res.client_token,
            auth_res.authlib_server_name,
        ).await;

        assert!(save_result.is_ok());
        let added_accounts = save_result.unwrap();
        assert_eq!(added_accounts.len(), 2);
        assert_eq!(added_accounts[0].username, "AuthlibPlayer1");
        assert_eq!(added_accounts[0].id, "12345678-90ab-cdef-1234-567890abcdef");
        assert_eq!(added_accounts[1].username, "AuthlibPlayer2");

        mock_meta.assert_async().await;
        mock_auth.assert_async().await;
    }
}
