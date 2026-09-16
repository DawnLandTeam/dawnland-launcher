use crate::auth::{self, Account, LoginInitResponse};
use crate::downloader::{run_batch_download, DownloadTask};
use crate::error::{AppError, DawnlandError};
use std::env::consts;
use sysinfo::System;
use tauri::AppHandle;

pub mod modpack;
pub mod task;

/// Returns a human-readable OS identifier string.
#[tauri::command]
pub fn get_system_info() -> Result<String, AppError> {
    let os = consts::OS;
    let arch = consts::ARCH;
    let family = consts::FAMILY;

    let info = format!("Operating System: {os} | Architecture: {arch} | Family: {family}");

    tracing::info!("System info requested: {}", info);
    Ok(info)
}

/// Returns the system locale (e.g. "zh-CN", "en-US")
#[tauri::command]
pub fn get_system_locale() -> Option<String> {
    sys_locale::get_locale()
}

/// Get system memory info for memory slider configuration.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemMemoryInfo {
    pub total_mb: u32,
    pub recommended_max_mb: u32,
}

#[tauri::command]
pub fn get_system_memory() -> Result<SystemMemoryInfo, AppError> {
    let mut sys = System::new_all();
    sys.refresh_all();
    let total_bytes = sys.total_memory();
    let total_mb = (total_bytes / (1024 * 1024)) as u32;
    let recommended_max_mb = (total_mb / 3).clamp(1024, 16384);

    tracing::info!(
        "System memory: {} MB, recommended max: {} MB",
        total_mb,
        recommended_max_mb
    );

    Ok(SystemMemoryInfo {
        total_mb,
        recommended_max_mb,
    })
}

/// Batch download multiple files concurrently.
#[tauri::command]
pub async fn batch_download(tasks: Vec<DownloadTask>, app: AppHandle) -> Result<(), AppError> {
    tracing::info!("Received batch download request with {} tasks", tasks.len());

    // Spawn the download tasks without blocking the command.
    let app_clone = app.clone();
    tokio::spawn(async move {
        let _ = run_batch_download(tasks, app_clone, crate::core::mojang::get_cancel_flag()).await;
    });

    // Return immediately to frontend.
    Ok(())
}

// ============ Auth Commands ============

/// Get all stored accounts.
#[tauri::command]
pub async fn get_accounts() -> Result<Vec<Account>, AppError> {
    Ok(auth::get_accounts().await?)
}

/// Add a new offline account.
#[tauri::command]
pub async fn add_offline_account(username: String) -> Result<Account, AppError> {
    tracing::info!("Adding offline account: {}", username);
    Ok(auth::add_offline_account(&username).await?)
}

/// Remove an account by ID.
#[tauri::command]
pub async fn remove_account(id: String) -> Result<(), AppError> {
    tracing::info!("Removing account: {}", id);
    Ok(auth::remove_account(&id).await?)
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn install_mod_to_instance(
    app: tauri::AppHandle,
    options: crate::core::manager::InstallModOptions,
) -> Result<String, AppError> {
    crate::core::manager::install_mod_to_instance(app, crate::core::manager::InstallModOptions {
        source: options.source,
        project_id: options.project_id,
        mod_name: options.mod_name,
        instance_id: options.instance_id,
        target_dir: options.target_dir,
        download_url: options.download_url,
        file_id: options.file_id,
        dependencies: options.dependencies,
        keep_both: options.keep_both,
    }, None)
    .await
    .map_err(|e| DawnlandError::Unknown(e).into())
}

/// Start Microsoft Device Code Flow login.
#[tauri::command]
pub async fn start_microsoft_login() -> Result<LoginInitResponse, AppError> {
    tracing::info!("Starting Microsoft login flow");
    auth::start_microsoft_login().await
}

/// Poll for Microsoft login completion.
#[tauri::command]
pub async fn poll_microsoft_token(device_code: String) -> Result<Account, AppError> {
    tracing::info!("Polling Microsoft token with device code");
    auth::poll_microsoft_token(&device_code).await
}

/// Refresh Microsoft token for an existing account.
#[tauri::command]
pub async fn refresh_microsoft_token(account_id: String) -> Result<Account, AppError> {
    tracing::info!("Refreshing Microsoft token for account: {}", account_id);
    auth::refresh_microsoft_token(&account_id).await
}

/// Start seamless Microsoft OAuth 2.0 PKCE login flow.
#[tauri::command]
pub async fn login_microsoft_oauth() -> Result<Account, AppError> {
    tracing::info!("Invoking seamless Microsoft OAuth login");
    auth::login_microsoft_oauth().await
}

/// Fetch latest account textures (skin/cape).
#[tauri::command]
pub async fn fetch_account_textures(account_id: String) -> Result<auth::AccountTextures, AppError> {
    tracing::info!("Fetching textures for account: {}", account_id);
    let accounts = auth::get_accounts().await?;
    let mut account = accounts
        .into_iter()
        .find(|a| a.id == account_id)
        .ok_or_else(|| DawnlandError::Unknown("Account not found".to_string()))?;

    match account.account_type {
        auth::AccountType::Microsoft => {
            let token = account.access_token.as_ref().ok_or_else(|| {
                DawnlandError::Unknown("No access token found for Microsoft account".to_string())
            })?;
            
            // Try fetching with current token first
            if let Ok((_, _, textures)) = auth::microsoft::get_minecraft_profile(token).await {
                if let Some(t) = textures.clone() {
                    let _lock = crate::auth::ACCOUNTS_LOCK.lock().await;
                    let mut all_accounts = auth::get_accounts().await?;
                    if let Some(a) = all_accounts.iter_mut().find(|a| a.id == account_id) {
                        a.textures = textures.clone();
                    }
                    let _ = auth::save_accounts(&all_accounts).await;
                    return Ok(t);
                }
            }

            // Token might be expired, fallback to refresh
            let updated_account = auth::refresh_microsoft_token(&account_id).await?;
            if let Some(textures) = updated_account.textures {
                return Ok(textures);
            }
        }
        auth::AccountType::Authlib => {
            let authlib_url = account.authlib_url.as_ref().ok_or_else(|| {
                DawnlandError::Unknown("No Authlib URL found".to_string())
            })?;
            // Call sessionserver profile API
            let timestamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis();
            let url = format!("{}/sessionserver/session/minecraft/profile/{}?unsigned=false&t={}", authlib_url.trim_end_matches('/'), account.id.replace("-", ""), timestamp);
            let client = reqwest::Client::new();
            let res = client.get(&url).send().await.map_err(|e| DawnlandError::Unknown(e.to_string()))?;
            if res.status().is_success() {
                #[derive(serde::Deserialize)]
                struct AuthlibProperty {
                    name: String,
                    value: String,
                }
                #[derive(serde::Deserialize)]
                struct AuthlibProfile {
                    properties: Option<Vec<AuthlibProperty>>,
                }
                
                let profile: AuthlibProfile = res.json().await.map_err(|e| DawnlandError::Unknown(e.to_string()))?;
                if let Some(props) = profile.properties {
                    if let Some(prop) = props.into_iter().find(|p| p.name == "textures") {
                        use base64::{Engine as _, engine::general_purpose};
                        let decoded = general_purpose::STANDARD.decode(prop.value).unwrap_or_default();
                        if let Ok(json_str) = String::from_utf8(decoded) {
                            #[derive(serde::Deserialize)]
                            struct TextureData { url: String }
                            #[derive(serde::Deserialize)]
                            #[serde(rename_all = "UPPERCASE")]
                            struct TexturesPayload { skin: Option<TextureData>, cape: Option<TextureData> }
                            #[derive(serde::Deserialize)]
                            struct DecodedPayload { textures: Option<TexturesPayload> }

                            if let Ok(payload) = serde_json::from_str::<DecodedPayload>(&json_str) {
                                let mut textures = auth::AccountTextures { skin_url: None, cape_url: None, variant: None };
                                if let Some(tex) = payload.textures {
                                    if let Some(skin) = tex.skin { textures.skin_url = Some(skin.url); }
                                    if let Some(cape) = tex.cape { textures.cape_url = Some(cape.url); }
                                }
                                account.textures = Some(textures.clone());
                                // save it
                                let _lock = crate::auth::ACCOUNTS_LOCK.lock().await;
                                let mut all_accounts = auth::get_accounts().await?;
                                if let Some(a) = all_accounts.iter_mut().find(|a| a.id == account_id) {
                                    a.textures = account.textures.clone();
                                }
                                auth::save_accounts(&all_accounts).await?;
                                return Ok(textures);
                            }
                        }
                    }
                }
            }
        }
        _ => {}
    }
    
    // Return empty textures by default
    Ok(auth::AccountTextures {
        skin_url: None,
        cape_url: None,
        variant: None,
    })
}

/// Upload a new skin for a Microsoft account.
#[tauri::command]
pub async fn upload_microsoft_skin(account_id: String, skin_path: String, variant: String) -> Result<(), AppError> {
    tracing::info!("Uploading skin for account: {} from {} with variant {}", account_id, skin_path, variant);
    let accounts = auth::get_accounts().await?;
    let account = accounts
        .into_iter()
        .find(|a| a.id == account_id)
        .ok_or_else(|| DawnlandError::Unknown("Account not found".to_string()))?;

    if account.account_type != auth::AccountType::Microsoft {
        return Err(DawnlandError::Unknown("Only Microsoft accounts can upload skins directly via this API".to_string()).into());
    }

    // Refresh token first to ensure valid access
    let account = auth::refresh_microsoft_token(&account_id).await?;
    let token = account.access_token.as_ref().unwrap();

    let skin_data = tokio::fs::read(&skin_path).await.map_err(|e| DawnlandError::Unknown(format!("Failed to read skin file: {}", e)))?;

    let client = reqwest::Client::new();
    
    let file_part = reqwest::multipart::Part::bytes(skin_data)
        .file_name("skin.png")
        .mime_str("image/png")
        .unwrap();

    let form = reqwest::multipart::Form::new()
        .text("variant", variant)
        .part("file", file_part);

    let res = client
        .post("https://api.minecraftservices.com/minecraft/profile/skins")
        .header("Authorization", format!("Bearer {}", token))
        .multipart(form)
        .send()
        .await
        .map_err(|e| DawnlandError::Unknown(format!("Skin upload failed: {}", e)))?;

    if !res.status().is_success() {
        let err_text = res.text().await.unwrap_or_default();
        return Err(DawnlandError::Unknown(format!("Failed to upload skin: {}", err_text)).into());
    }

    // Update textures by fetching again
    let _ = fetch_account_textures(account_id).await;

    Ok(())
}

// ============ Custom Updater Commands ============

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProgress {
    event: String,
    data: Option<ProgressData>,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressData {
    content_length: Option<u64>,
    chunk_length: Option<usize>,
}

#[tauri::command]
pub async fn update_launcher(version: String, md5: Option<String>, url: Option<String>, app: AppHandle) -> Result<(), AppError> {
    let url = if let Some(u) = url {
        u
    } else {
        let filename = if cfg!(target_os = "windows") {
            "DLML.exe"
        } else if cfg!(target_os = "linux") {
            "amd64.AppImage"
        } else {
            return Err(
                DawnlandError::Unknown("Unsupported OS for native auto-update".to_string()).into(),
            );
        };
        format!("https://dl.88880222.xyz/releases/v{}/{}", version, filename)
    };
    
    tracing::info!("Starting native update from {}", url);

    use std::io::Write;
    use tauri::Emitter;

    let mut response = reqwest::get(&url)
        .await
        .map_err(|e| format!("Failed to connect: {}", e))?;

    if !response.status().is_success() {
        return Err(DawnlandError::Unknown(format!(
            "Server returned error: {}",
            response.status()
        ))
        .into());
    }

    let content_length = response.content_length();

    app.emit(
        "portable-update-progress",
        UpdateProgress {
            event: "Started".to_string(),
            data: Some(ProgressData {
                content_length,
                chunk_length: None,
            }),
        },
    )
    .map_err(|e| e.to_string())?;

    let mut temp_file = tempfile::NamedTempFile::new().map_err(|e| e.to_string())?;

    use futures_util::StreamExt;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| format!("Download error: {}", e))?
    {
        temp_file
            .write_all(&chunk)
            .map_err(|e| format!("Write error: {}", e))?;
        app.emit(
            "portable-update-progress",
            UpdateProgress {
                event: "Progress".to_string(),
                data: Some(ProgressData {
                    content_length: None,
                    chunk_length: Some(chunk.len()),
                }),
            },
        )
        .map_err(|e| e.to_string())?;
    }

    temp_file.flush().map_err(|e| e.to_string())?;

    let temp_path = temp_file.into_temp_path();
    
    // Verify MD5 if provided
    if let Some(expected_md5) = md5 {
        tracing::info!("Verifying MD5 checksum...");
        let file_bytes = std::fs::read(&temp_path).map_err(|e| format!("Failed to read temp file for MD5 verification: {}", e))?;
        let computed_md5 = format!("{:x}", md5::compute(file_bytes));
        let expected_md5_normalized = expected_md5.trim().to_lowercase();
        
        if computed_md5 != expected_md5_normalized {
            tracing::error!("MD5 mismatch! Expected: {}, Computed: {}", expected_md5_normalized, computed_md5);
            return Err(DawnlandError::Md5Mismatch.into());
        }
        tracing::info!("MD5 verification passed.");
    }

    tracing::info!("Performing self-replace with downloaded file");

    // Set execution permissions on Linux
    #[cfg(target_family = "unix")]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(mut perms) = std::fs::metadata(&temp_path).map(|m| m.permissions()) {
            perms.set_mode(0o755);
            let _ = std::fs::set_permissions(&temp_path, perms);
        }
    }

    self_replace::self_replace(&temp_path).map_err(|e| {
        format!(
            "Self-replace failed. Make sure the file is not locked: {}",
            e
        )
    })?;
    let _ = temp_path.keep(); // Keep the file since self_replace moved it

    app.emit(
        "portable-update-progress",
        UpdateProgress {
            event: "Finished".to_string(),
            data: None,
        },
    )
    .map_err(|e| e.to_string())?;

    tracing::info!("Native update completed successfully");
    Ok(())
}

#[tauri::command]
pub async fn app_track_event(
    app: tauri::AppHandle,
    name: String,
    props: Option<serde_json::Value>,
) -> Result<(), AppError> {
    let settings = crate::core::settings::get_launcher_settings_sync();
    if settings.enable_telemetry != Some(true) {
        tracing::debug!("Telemetry disabled or unconfirmed. Dropping event: {}", name);
        return Ok(());
    }

    let aptabase_key = option_env!("APTABASE_KEY")
        .map(String::from)
        .or_else(|| std::env::var("APTABASE_KEY").ok())
        .filter(|k| !k.trim().is_empty());

    if aptabase_key.is_none() {
        tracing::debug!(
            "Aptabase disabled. Dropping event: {} (Props: {:?})",
            name,
            props
        );
        return Ok(());
    }
    use tauri_plugin_aptabase::EventTracker;
    tracing::debug!("Tracking event: {} (Props: {:?})", name, props);
    app.track_event(&name, props);
    // Removed app.flush_events_blocking() to prevent blocking the async runtime
    Ok(())
}

#[tauri::command]
pub async fn proxy_image_base64(url: String) -> Result<String, AppError> {
    use base64::{Engine as _, engine::general_purpose};
    use std::net::IpAddr;
    
    // SSRF Protection: Parse URL and validate host via DNS resolution
    let parsed = reqwest::Url::parse(&url).map_err(|e| DawnlandError::Unknown(format!("Invalid URL: {e}")))?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return Err(DawnlandError::Unknown("Only HTTP/HTTPS allowed".to_string()).into());
    }
    
    let host = parsed.host_str().ok_or_else(|| DawnlandError::Unknown("Invalid host".to_string()))?;
    let port = parsed.port_or_known_default().unwrap_or(80);
    
    // Resolve DNS
    let addrs = tokio::net::lookup_host(format!("{}:{}", host, port))
        .await
        .map_err(|e| DawnlandError::Unknown(format!("DNS resolution failed: {e}")))?;
        
    let mut safe_addr = None;
    for addr in addrs {
        let ip = addr.ip();
        let mut is_private = false;
        
        if ip.is_loopback() || ip.is_unspecified() || ip.is_multicast() {
            is_private = true;
        } else {
            match ip {
                IpAddr::V4(ipv4) => {
                    if ipv4.is_private() || ipv4.is_link_local() {
                        is_private = true;
                    }
                }
                IpAddr::V6(ipv6) => {
                    let segments = ipv6.segments();
                    if (segments[0] & 0xfe00) == 0xfc00 { // Unique local fc00::/7
                        is_private = true;
                    }
                    if (segments[0] & 0xffc0) == 0xfe80 { // Link local fe80::/10
                        is_private = true;
                    }
                }
            }
        }
        
        if !is_private {
            safe_addr = Some(addr);
            break;
        }
    }
    
    let safe_addr = safe_addr.ok_or_else(|| DawnlandError::Unknown("Host resolves to private IP".to_string()))?;

    // Create client that forces the resolved safe IP to prevent DNS rebinding
    let client = reqwest::Client::builder()
        .resolve(host, safe_addr)
        .build()
        .map_err(|e| DawnlandError::Unknown(e.to_string()))?;
        
    let res = client.get(parsed).send().await.map_err(|e| DawnlandError::Unknown(e.to_string()))?;
    
    // Detect MIME type
    let content_type = res.headers().get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("image/png")
        .to_string();
        
    let bytes = res.bytes().await.map_err(|e| DawnlandError::Unknown(e.to_string()))?;
    let base64_str = general_purpose::STANDARD.encode(bytes);
    Ok(format!("data:{};base64,{}", content_type, base64_str))
}

pub mod export;
