use serde::{Deserialize, Serialize};
use crate::auth::{get_accounts, Account, AccountType};

/// Device code flow response from Microsoft.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct DeviceCodeResponse {
    pub user_code: String,
    pub device_code: String,
    pub verification_uri: String,
    pub expires_in: i64,
    pub interval: Option<i64>,
    pub message: Option<String>,
}

/// Response for frontend to initiate Microsoft login.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginInitResponse {
    pub user_code: String,
    pub verification_uri: String,
    pub message: String,
}

/// Error types for Microsoft auth.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthError {
    pub code: String,
    pub message: String,
}

impl AuthError {
    pub fn new(code: &str, message: &str) -> Self {
        Self {
            code: code.to_string(),
            message: message.to_string(),
        }
    }
}

/// Device code request payload.
#[derive(Serialize)]
struct DeviceCodePayload {
    client_id: &'static str,
    scope: &'static str,
}

/// Client ID for the launcher (newly registered application).
const CLIENT_ID: &str = "780ab3ca-a1a0-4830-ac98-92a595e85a13";
const SCOPE: &str = "XboxLive.signin offline_access";

/// Initiate Microsoft Device Code Flow.
pub async fn start_microsoft_login() -> Result<LoginInitResponse, String> {
    let client = reqwest::Client::new();

    let payload = DeviceCodePayload {
        client_id: CLIENT_ID,
        scope: SCOPE,
    };

    let response = client
        .post("https://login.microsoftonline.com/consumers/oauth2/v2.0/devicecode")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .form(&payload)
        .send()
        .await
        .map_err(|e| format!("Failed to request device code: {e}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("Device code request failed: {} - {}", status, body));
    }

    // Debug: print raw response body
    let raw_text = response.text().await.map_err(|e| format!("Failed to read response: {e}"))?;
    tracing::info!("Raw Device Code Response: {}", raw_text);

    let device_code: DeviceCodeResponse = serde_json::from_str(&raw_text)
        .map_err(|e| format!("Failed to parse device code response: {e}. Raw: {}", raw_text))?;

    Ok(LoginInitResponse {
        user_code: device_code.user_code,
        verification_uri: device_code.verification_uri,
        message: device_code.message.unwrap_or_else(|| "Please enter the code on the website.".to_string()),
    })
}

/// Token request payload.
#[derive(Serialize)]
struct TokenPayload<'a> {
    client_id: &'a str,
    device_code: &'a str,
    grant_type: &'a str,
}

/// Poll for token - returns (access_token, refresh_token).
async fn poll_for_token(device_code: &str) -> Result<(String, String), String> {
    let client = reqwest::Client::new();

    loop {
        let payload = TokenPayload {
            client_id: CLIENT_ID,
            device_code,
            grant_type: "urn:ietf:params:oauth:grant-type:device_code",
        };

        let response = client
            .post("https://login.microsoftonline.com/consumers/oauth2/v2.0/token")
            .form(&payload)
            .send()
            .await
            .map_err(|e| format!("Token request failed: {e}))"))?;

        #[derive(Deserialize)]
        struct TokenResponse {
            access_token: Option<String>,
            refresh_token: Option<String>,
            error: Option<String>,
            error_description: Option<String>,
        }

        let token_resp: TokenResponse = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse token response: {e}"))?;

        if let Some(access_token) = token_resp.access_token {
            let refresh_token = token_resp.refresh_token.unwrap_or_default();
            return Ok((access_token, refresh_token));
        }

        if let Some(error) = token_resp.error {
            return Err(format!("Token error: {} - {}", error, token_resp.error_description.unwrap_or_default()));
        }

        // Wait before next poll.
        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
    }
}

/// Xbox Live authentication request.
#[derive(Serialize)]
struct XboxLiveAuthRequest {
    Properties: XboxLiveProperties,
    RelyingParty: &'static str,
    TokenType: &'static str,
}

#[derive(Serialize)]
struct XboxLiveProperties {
    AuthMethod: &'static str,
    SiteName: &'static str,
    RpsTicket: String,
}

#[derive(Deserialize)]
struct XboxLiveAuthResponse {
    Token: Option<String>,
    #[serde(rename = "DisplayClaims")]
    DisplayClaims: Option<XboxDisplayClaims>,
}

#[derive(Deserialize)]
struct XboxDisplayClaims {
    xui: Option<Vec<XboxUserClaim>>,
}

#[derive(Deserialize)]
struct XboxUserClaim {
    uhs: Option<String>,
}

/// XSTS authentication request.
#[derive(Serialize)]
struct XSTSAuthRequest {
    Properties: XSTSProperties,
    RelyingParty: &'static str,
}

#[derive(Serialize)]
struct XSTSProperties {
    SandboxId: &'static str,
    UserTokens: Vec<String>,
}

#[derive(Deserialize)]
struct XSTSAuthResponse {
    Token: Option<String>,
    #[serde(rename = "DisplayClaims")]
    DisplayClaims: Option<XSTSDisplayClaims>,
    #[serde(rename = "Status")]
    Status: Option<XSTSStatus>,
}

#[derive(Deserialize)]
struct XSTSDisplayClaims {
    xui: Option<Vec<XSTSUserClaim>>,
}

#[derive(Deserialize)]
struct XSTSUserClaim {
    xid: Option<String>,
    uhs: Option<String>,
}

#[derive(Deserialize)]
struct XSTSStatus {
    code: Option<String>,
    message: Option<String>,
}

/// Exchange Microsoft token for Xbox Live token.
async fn get_xbox_live_token(ms_token: &str) -> Result<String, String> {
    let client = reqwest::Client::new();

    let request = XboxLiveAuthRequest {
        Properties: XboxLiveProperties {
            AuthMethod: "RPS",
            SiteName: "user.auth.xboxlive.com",
            RpsTicket: format!("d={}", ms_token),
        },
        RelyingParty: "http://auth.xboxlive.com",
        TokenType: "JWT",
    };

    let response = client
        .post("https://user.auth.xboxlive.com/user/authenticate")
        .json(&request)
        .send()
        .await
        .map_err(|e| format!("Xbox Live auth request failed: {e}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("Xbox Live auth failed: {} - {}", status, body));
    }

    let xbl_resp: XboxLiveAuthResponse = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse Xbox Live response: {e}"))?;

    xbl_resp.Token.ok_or_else(|| "No Xbox Live token in response".to_string())
}

/// Exchange Xbox Live token for XSTS token.
async fn get_xsts_token(xbl_token: &str) -> Result<(String, String), String> {
    let client = reqwest::Client::new();

    let request = XSTSAuthRequest {
        Properties: XSTSProperties {
            SandboxId: "RETAIL",
            UserTokens: vec![xbl_token.to_string()],
        },
        RelyingParty: "rp://api.minecraftservices.com/",
    };

    let response = client
        .post("https://xsts.auth.xboxlive.com/xsts/authorize")
        .json(&request)
        .send()
        .await
        .map_err(|e| format!("XSTS auth request failed: {e}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("XSTS auth failed: {} - {}", status, body));
    }

    let xsts_resp: XSTSAuthResponse = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse XSTS response: {e}"))?;

    // Check for errors (e.g., not an Xbox member).
    if let Some(status) = xsts_resp.Status {
        if let Some(code) = status.code {
            if code != "OK" {
                return Err(format!("XSTS error: {} - {}", code, status.message.unwrap_or_default()));
            }
        }
    }

    let token = xsts_resp.Token.ok_or_else(|| "No XSTS token in response".to_string())?;

    // Get UHS from display claims.
    let uhs = xsts_resp
        .DisplayClaims
        .and_then(|dc| dc.xui)
        .and_then(|mut xui| xui.pop())
        .and_then(|u| u.uhs)
        .unwrap_or_default();

    Ok((token, uhs))
}

/// Exchange XSTS token for Minecraft access token.
async fn get_minecraft_token(xsts_token: &str, uhs: &str) -> Result<String, String> {
    let client = reqwest::Client::new();

    #[derive(Serialize)]
    struct MCAuthRequest {
        identityToken: String,
    }

    let request = MCAuthRequest {
        identityToken: format!("XBL3.0 x={};{}", uhs, xsts_token),
    };

    let response = client
        .post("https://api.minecraftservices.com/authentication/login_with_xbox")
        .json(&request)
        .send()
        .await
        .map_err(|e| format!("Minecraft auth request failed: {e}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("Minecraft auth failed: {} - {}", status, body));
    }

    #[derive(Deserialize)]
    struct MCAuthResponse {
        access_token: Option<String>,
    }

    let mc_resp: MCAuthResponse = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse Minecraft auth response: {e}"))?;

    mc_resp.access_token.ok_or_else(|| "No Minecraft access token in response".to_string())
}

/// Get Minecraft profile (UUID and username).
async fn get_minecraft_profile(mc_token: &str) -> Result<(String, String), String> {
    let client = reqwest::Client::new();

    let response = client
        .get("https://api.minecraftservices.com/minecraft/profile")
        .bearer_auth(mc_token)
        .send()
        .await
        .map_err(|e| format!("Minecraft profile request failed: {e}"))?;

    if !response.status().is_success() {
        let status = response.status();
        if status.as_u16() == 404 {
            return Err("Account does not own Minecraft Java Edition".to_string());
        }
        let body = response.text().await.unwrap_or_default();
        return Err(format!("Minecraft profile request failed: {} - {}", status, body));
    }

    #[derive(Deserialize)]
    struct MCProfileResponse {
        id: Option<String>,
        name: Option<String>,
    }

    let profile: MCProfileResponse = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse Minecraft profile: {e}"))?;

    let uuid = profile.id.ok_or_else(|| "No UUID in profile response".to_string())?;
    let username = profile.name.ok_or_else(|| "No name in profile response".to_string())?;

    Ok((uuid, username))
}

/// Poll for Microsoft login and complete the full authentication chain.
pub async fn poll_microsoft_token(device_code: &str) -> Result<Account, String> {
    tracing::info!("Polling for Microsoft token...");

    // Step 1: Poll for Microsoft access token.
    let (ms_token, refresh_token) = poll_for_token(device_code).await?;
    tracing::info!("Received Microsoft access token");

    // Step 2: Get Xbox Live token.
    let xbl_token = get_xbox_live_token(&ms_token).await?;
    tracing::info!("Received Xbox Live token");

    // Step 3: Get XSTS token.
    let (xsts_token, uhs) = get_xsts_token(&xbl_token).await?;
    tracing::info!("Received XSTS token");

    // Step 4: Get Minecraft access token.
    let mc_token = get_minecraft_token(&xsts_token, &uhs).await?;
    tracing::info!("Received Minecraft access token");

    // Step 5: Get Minecraft profile.
    let (uuid, username) = get_minecraft_profile(&mc_token).await?;
    tracing::info!("Received Minecraft profile: {} ({})", username, uuid);

    // Create account.
    let account = Account {
        id: uuid,
        username,
        account_type: AccountType::Microsoft,
        access_token: Some(mc_token),
        refresh_token: Some(refresh_token),
        textures: None,
    };

    // Load existing accounts and add new one.
    let mut accounts = get_accounts().await?;

    // Remove existing Microsoft account with same UUID if exists.
    accounts.retain(|a| !(a.account_type == AccountType::Microsoft && a.id == account.id));

    accounts.push(account.clone());
    super::save_accounts(&accounts).await?;

    Ok(account)
}