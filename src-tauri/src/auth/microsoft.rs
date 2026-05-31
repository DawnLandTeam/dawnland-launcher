use serde::{Deserialize, Serialize};
use crate::auth::{get_accounts, save_accounts, Account, AccountType};

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
    pub device_code: String,
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

fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
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
    let client = http_client();

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
        device_code: device_code.device_code,
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
    let client = http_client();

    loop {
        let payload = TokenPayload {
            client_id: CLIENT_ID,
            device_code,
            grant_type: "urn:ietf:params:oauth:grant-type:device_code",
        };

        let response = client
            .post("https://login.microsoftonline.com/consumers/oauth2/v2.0/token")
            .header("Content-Type", "application/x-www-form-urlencoded")
            .form(&payload)
            .send()
            .await
            .map_err(|e| format!("Token request failed: {e}"))?;

        let raw_text = response.text().await.map_err(|e| format!("Failed to read token response: {e}"))?;
        
        #[derive(Deserialize)]
        struct TokenResponse {
            access_token: Option<String>,
            refresh_token: Option<String>,
            error: Option<String>,
            error_description: Option<String>,
        }

        let token_resp: TokenResponse = serde_json::from_str(&raw_text)
            .map_err(|e| format!("Failed to parse token response: {e}. Raw: {}", raw_text))?;

        if let Some(access_token) = token_resp.access_token {
            let refresh_token = token_resp.refresh_token.unwrap_or_default();
            tracing::info!("Received Microsoft access token");
            return Ok((access_token, refresh_token));
        }

        if let Some(error) = &token_resp.error {
            // authorization_pending means user hasn't completed auth yet, continue polling
            if error == "authorization_pending" {
                tracing::debug!("User hasn't completed auth yet, polling...");
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                continue;
            }
            // authorization_declined means user cancelled
            if error == "authorization_declined" {
                return Err("Authorization was declined by the user".to_string());
            }
            // expired_token means the flow expired
            if error == "expired_token" {
                return Err("Device code flow expired. Please try again.".to_string());
            }
            return Err(format!("Token error: {} - {}", error, token_resp.error_description.unwrap_or_default()));
        }

        // No access_token and no error - wait and continue
        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
    }
}

/// Exchange Microsoft token for Xbox Live token.
async fn get_xbox_live_token(ms_token: &str) -> Result<String, String> {
    let client = http_client();

    #[derive(Serialize)]
    struct XboxLiveAuthRequest {
        #[serde(rename = "Properties")]
        properties: XboxLiveProperties,
        #[serde(rename = "RelyingParty")]
        relying_party: &'static str,
        #[serde(rename = "TokenType")]
        token_type: &'static str,
    }

    #[derive(Serialize)]
    struct XboxLiveProperties {
        #[serde(rename = "AuthMethod")]
        auth_method: &'static str,
        #[serde(rename = "SiteName")]
        site_name: &'static str,
        #[serde(rename = "RpsTicket")]
        rps_ticket: String,
    }

    let request = XboxLiveAuthRequest {
        properties: XboxLiveProperties {
            auth_method: "RPS",
            site_name: "user.auth.xboxlive.com",
            rps_ticket: format!("d={}", ms_token),
        },
        relying_party: "http://auth.xboxlive.com",
        token_type: "JWT",
    };

    let response = client
        .post("https://user.auth.xboxlive.com/user/authenticate")
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .json(&request)
        .send()
        .await
        .map_err(|e| format!("Xbox Live auth request failed: {e}"))?;

    let status = response.status();
    let raw_text = response.text().await.unwrap_or_default();

    if !status.is_success() {
        tracing::error!("Xbox Live auth failed: {} - {}", status, raw_text);
        return Err(format!("Xbox Live auth failed: {} - {}", status, raw_text));
    }

    #[derive(Deserialize)]
    struct XboxLiveAuthResponse {
        #[serde(rename = "Token")]
        token: Option<String>,
    }

    let xbl_resp: XboxLiveAuthResponse = serde_json::from_str(&raw_text)
        .map_err(|e| format!("Failed to parse Xbox Live response: {e}. Raw: {}", raw_text))?;

    let xbl_token = xbl_resp.token.ok_or_else(|| "No Xbox Live token in response".to_string())?;
    
    tracing::info!("Received Xbox Live token");
    Ok(xbl_token)
}

/// Exchange Xbox Live token for XSTS token.
async fn get_xsts_token(xbl_token: &str) -> Result<(String, String), String> {
    let client = http_client();

    #[derive(Serialize)]
    #[serde(rename_all = "PascalCase")]
    pub struct XstsRequest {
        pub properties: XstsProperties,
        pub relying_party: String,
        pub token_type: String,
    }

    #[derive(Serialize)]
    #[serde(rename_all = "PascalCase")]
    pub struct XstsProperties {
        pub sandbox_id: String,
        pub user_tokens: Vec<String>,
    }

    let request = XstsRequest {
        properties: XstsProperties {
            sandbox_id: "RETAIL".to_string(),
            user_tokens: vec![xbl_token.to_string()],
        },
        relying_party: "rp://api.minecraftservices.com/".to_string(),
        token_type: "JWT".to_string(),
    };

    // Debug: print exact JSON being sent
    let request_json = serde_json::to_string(&request).unwrap_or_default();
    tracing::info!("XSTS request JSON: {}", request_json);

    let response = client
        .post("https://xsts.auth.xboxlive.com/xsts/authorize")
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .json(&request)
        .send()
        .await
        .map_err(|e| format!("XSTS auth request failed: {e}"))?;

    let status = response.status();
    let raw_text = response.text().await.unwrap_or_default();

    // Log full response details
    tracing::info!("XSTS response status: {}, body: {}", status, raw_text);

    if !status.is_success() {
        tracing::error!("XSTS auth failed ({}): body='{}'", status, raw_text);
        // Try to parse the error as XErr for more details
        #[derive(Deserialize)]
        struct XErrResponse {
            #[serde(rename = "XErr")]
            xerr: Option<i64>,
            #[serde(rename = "Message")]
            message: Option<String>,
        }
        
        if let Ok(xerr_resp) = serde_json::from_str::<XErrResponse>(&raw_text) {
            if let Some(xerr) = xerr_resp.xerr {
                let detailed_error = match xerr {
                    2148916233 => "This account does not have Xbox Live. Please subscribe to Xbox Live.",
                    2148916235 => "This is a child account without adult approval. Please use an adult account.",
                    2148916236 => "This account is a child and needs parental verification.",
                    2148916237 => "This account requires parental consent for Xbox Live.",
                    _ => xerr_resp.message.as_deref().unwrap_or("Unknown XSTS error"),
                };
                return Err(format!("XSTS error ({}): {}", xerr, detailed_error));
            }
        }
        return Err(format!("XSTS auth failed: {} - {}", status, raw_text));
    }

    #[derive(Deserialize)]
    struct XSTSAuthResponse {
        #[serde(rename = "Token")]
        token: Option<String>,
        #[serde(rename = "DisplayClaims")]
        display_claims: Option<XSTSDisplayClaims>,
    }

    #[derive(Deserialize)]
    struct XSTSDisplayClaims {
        #[serde(rename = "xui")]
        xui: Option<Vec<XSTSUserClaim>>,
    }

    #[derive(Deserialize)]
    struct XSTSUserClaim {
        #[serde(rename = "uhs")]
        uhs: Option<String>,
    }

    let xsts_resp: XSTSAuthResponse = serde_json::from_str(&raw_text)
        .map_err(|e| format!("Failed to parse XSTS response: {e}. Raw: {}", raw_text))?;

    let token = xsts_resp.token.ok_or_else(|| "No XSTS token in response".to_string())?;

    // Get UHS from display claims.
    let uhs = xsts_resp
        .display_claims
        .and_then(|dc| dc.xui)
        .and_then(|mut xui| xui.pop())
        .and_then(|u| u.uhs)
        .unwrap_or_default();

    tracing::info!("Received XSTS token");
    Ok((token, uhs))
}

/// Exchange XSTS token for Minecraft access token.
async fn get_minecraft_token(xsts_token: &str, uhs: &str) -> Result<String, String> {
    let client = http_client();

    #[derive(Serialize)]
    struct MCAuthRequest {
        #[serde(rename = "identityToken")]
        identity_token: String,
    }

    let request = MCAuthRequest {
        identity_token: format!("XBL3.0 x={};{}", uhs, xsts_token),
    };

    let response = client
        .post("https://api.minecraftservices.com/authentication/login_with_xbox")
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .json(&request)
        .send()
        .await
        .map_err(|e| format!("Minecraft auth request failed: {e}"))?;

    let status = response.status();
    let raw_text = response.text().await.unwrap_or_default();

    if !status.is_success() {
        tracing::error!("Minecraft auth failed: {} - {}", status, raw_text);
        return Err(format!("Minecraft auth failed: {} - {}", status, raw_text));
    }

    #[derive(Deserialize)]
    struct MCAuthResponse {
        #[serde(rename = "access_token")]
        access_token: Option<String>,
        #[serde(rename = "error")]
        error: Option<String>,
        #[serde(rename = "errorMessage")]
        error_message: Option<String>,
    }

    let mc_resp: MCAuthResponse = serde_json::from_str(&raw_text)
        .map_err(|e| format!("Failed to parse Minecraft auth response: {e}. Raw: {}", raw_text))?;

    if let Some(error) = mc_resp.error {
        return Err(format!("Minecraft auth error: {} - {}", error, mc_resp.error_message.unwrap_or_default()));
    }

    let token = mc_resp.access_token.ok_or_else(|| "No Minecraft access token in response".to_string())?;
    
    tracing::info!("Received Minecraft access token");
    Ok(token)
}

/// Get Minecraft profile (UUID and username).
async fn get_minecraft_profile(mc_token: &str) -> Result<(String, String), String> {
    let client = http_client();

    let response = client
        .get("https://api.minecraftservices.com/minecraft/profile")
        .header("Authorization", format!("Bearer {}", mc_token))
        .send()
        .await
        .map_err(|e| format!("Minecraft profile request failed: {e}"))?;

    let status = response.status();
    let raw_text = response.text().await.unwrap_or_default();

    if !status.is_success() {
        if status.as_u16() == 404 {
            tracing::error!("Account does not own Minecraft Java Edition");
            return Err("Account does not own Minecraft Java Edition. Please purchase the game first.".to_string());
        }
        tracing::error!("Minecraft profile request failed: {} - {}", status, raw_text);
        return Err(format!("Minecraft profile request failed: {} - {}", status, raw_text));
    }

    #[derive(Deserialize)]
    struct MCProfileResponse {
        #[serde(rename = "id")]
        id: Option<String>,
        #[serde(rename = "name")]
        name: Option<String>,
    }

    let profile: MCProfileResponse = serde_json::from_str(&raw_text)
        .map_err(|e| format!("Failed to parse Minecraft profile: {e}. Raw: {}", raw_text))?;

    let uuid = profile.id.ok_or_else(|| "No UUID in profile response".to_string())?;
    let username = profile.name.ok_or_else(|| "No name in profile response".to_string())?;

    tracing::info!("Received Minecraft profile: {} ({})", username, uuid);
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

/// Refresh Microsoft token for a given account.
/// Returns updated account if successful, Err if refresh fails.
pub async fn refresh_microsoft_token(account_id: &str) -> Result<Account, String> {
    tracing::info!("Refreshing Microsoft token for account: {}", account_id);

    // Load all accounts to find the Microsoft account
    let mut accounts = get_accounts().await?;

    // Find the account
    let account_pos = accounts.iter().position(|a| a.id == account_id)
        .ok_or_else(|| "Account not found".to_string())?;

    let account = &mut accounts[account_pos];

    // Must be a Microsoft account with refresh token
    if account.account_type != AccountType::Microsoft {
        return Err("Account is not a Microsoft account".to_string());
    }

    let refresh_token = account.refresh_token.as_ref()
        .ok_or_else(|| "No refresh token available for this account".to_string())?;

    // Step 1: Refresh Microsoft access token
    let client = http_client();

    #[derive(Serialize)]
    struct RefreshPayload<'a> {
        client_id: &'a str,
        refresh_token: &'a str,
        grant_type: &'a str,
    }

    let payload = RefreshPayload {
        client_id: CLIENT_ID,
        refresh_token,
        grant_type: "refresh_token",
    };

    let response = client
        .post("https://login.microsoftonline.com/consumers/oauth2/v2.0/token")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .form(&payload)
        .send()
        .await
        .map_err(|e| format!("Token refresh request failed: {}", e))?;

    let raw_text = response.text().await.map_err(|e| format!("Failed to read refresh response: {e}"))?;

    #[derive(Deserialize)]
    struct RefreshResponse {
        access_token: Option<String>,
        refresh_token: Option<String>,
        error: Option<String>,
        error_description: Option<String>,
    }

    let refresh_resp: RefreshResponse = serde_json::from_str(&raw_text)
        .map_err(|e| format!("Failed to parse refresh response: {e}. Raw: {}", raw_text))?;

    if let Some(error) = &refresh_resp.error {
        let error_desc = refresh_resp.error_description.clone().unwrap_or_default();
        tracing::error!("Token refresh failed: {} - {}", error, error_desc);
        
        // If refresh token is invalid/expired, user needs to re-authenticate
        if error == "invalid_grant" || error == "refresh_token_expired" || error == "invalid_request" {
            return Err("REAUTH_REQUIRED".to_string());
        }
        
        return Err(format!("Token refresh error: {} - {}", error, error_desc));
    }

    let new_ms_token = refresh_resp.access_token
        .ok_or_else(|| "No access token in refresh response".to_string())?;

    let new_refresh_token = refresh_resp.refresh_token
        .unwrap_or_else(|| refresh_token.to_string()); // Use old one if not returned

    tracing::info!("Microsoft token refreshed successfully");

    // Step 2: Get new Xbox Live token
    let xbl_token = get_xbox_live_token(&new_ms_token).await?;
    tracing::info!("Xbox Live token refreshed");

    // Step 3: Get new XSTS token
    let (xsts_token, uhs) = get_xsts_token(&xbl_token).await?;
    tracing::info!("XSTS token refreshed");

    // Step 4: Get new Minecraft access token
    let mc_token = get_minecraft_token(&xsts_token, &uhs).await?;
    tracing::info!("Minecraft access token refreshed");

    // Update account with new tokens and collect the updated account
    let updated_account = {
        let account = &mut accounts[account_pos];
        account.access_token = Some(mc_token.clone());
        account.refresh_token = Some(new_refresh_token.clone());
        account.clone()
    };

    // Save updated accounts
    save_accounts(&accounts).await?;

    tracing::info!(target: "auth", "Account token refreshed for: {}", account_id);
    Ok(updated_account)
}