//! Instance Icon Cache
//!
//! Fetches and caches the icon of modpack instances installed from CurseForge or
//! Modrinth. Icons are stored inside the instance directory as `icon.<ext>` and the
//! file name is recorded in `dlml.json` (`InstanceConfig::icon`). All operations are
//! best-effort: failures are logged and never abort an installation.

use serde::Serialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter};

/// Event emitted to the frontend once an instance icon has been cached.
pub const INSTANCE_ICON_UPDATED_EVENT: &str = "instance-icon-updated";

const ICON_FILE_STEM: &str = "icon";
const ICON_TEMP_FILE: &str = "icon.download";
const SUPPORTED_EXTENSIONS: [&str; 4] = ["png", "jpg", "webp", "gif"];
const MAX_ICON_BYTES: u64 = 2 * 1024 * 1024;
const ICON_DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(20);

/// Instance IDs whose icon is currently being fetched (prevents duplicate work).
static IN_FLIGHT: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(|| Mutex::new(HashSet::new()));
/// Instance IDs already back-filled during this session (prevents retry storms on every scan).
static BACKFILL_ATTEMPTED: LazyLock<Mutex<HashSet<String>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceIconUpdatedPayload {
    pub instance_id: String,
    pub icon_path: String,
}

/// A modpack instance that is missing its cached icon.
#[derive(Debug, Clone)]
pub struct IconBackfillCandidate {
    pub instance_id: String,
    pub instance_dir: PathBuf,
    pub modpack_type: String,
    pub project_id: String,
}

/// Resolve the absolute path of an instance's cached icon, if it exists on disk.
///
/// `icon` must be a bare file name; anything containing path components is rejected
/// to prevent escaping the instance directory.
pub async fn resolve_icon_path(instance_dir: &Path, icon: Option<&str>) -> Option<PathBuf> {
    let name = icon?.trim();
    if name.is_empty() || Path::new(name).file_name().and_then(|n| n.to_str()) != Some(name) {
        return None;
    }
    let path = instance_dir.join(name);
    match tokio::fs::metadata(&path).await {
        Ok(meta) if meta.is_file() => Some(path),
        _ => None,
    }
}

/// Pick a file extension from the response `Content-Type`, falling back to the URL path.
fn detect_extension(content_type: Option<&str>, url: &str) -> Option<&'static str> {
    if let Some(ct) = content_type {
        let mime = ct.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
        match mime.as_str() {
            "image/png" => return Some("png"),
            "image/jpeg" | "image/jpg" => return Some("jpg"),
            "image/webp" => return Some("webp"),
            "image/gif" => return Some("gif"),
            _ => {}
        }
    }

    let path = url.split(['?', '#']).next().unwrap_or("");
    let ext = path.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase())?;
    match ext.as_str() {
        "png" => Some("png"),
        "jpg" | "jpeg" => Some("jpg"),
        "webp" => Some("webp"),
        "gif" => Some("gif"),
        _ => None,
    }
}

/// Query the platform API for the project's icon URL.
async fn fetch_icon_url(modpack_type: &str, project_id: &str) -> Result<Option<String>, String> {
    let project = match modpack_type.to_ascii_lowercase().as_str() {
        "curseforge" => crate::core::curseforge::get_cf_mod_details(project_id.to_string()).await?,
        "modrinth" => crate::core::modrinth::get_modrinth_mod_details(project_id.to_string()).await?,
        other => return Err(format!("Unsupported modpack source: {}", other)),
    };
    Ok(project.icon_url.filter(|u| !u.trim().is_empty()))
}

/// Download the modpack icon into `instance_dir` and record it in `dlml.json`.
///
/// Returns the absolute path of the cached icon, or `None` when the project has no icon.
pub async fn cache_modpack_icon(
    instance_dir: &Path,
    modpack_type: &str,
    project_id: &str,
) -> Result<Option<PathBuf>, String> {
    let Some(url) = fetch_icon_url(modpack_type, project_id).await? else {
        return Ok(None);
    };

    let client = crate::core::utils::get_http_client();
    let response = client
        .get(&url)
        .timeout(ICON_DOWNLOAD_TIMEOUT)
        .send()
        .await
        .map_err(|e| format!("Failed to request icon: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("Icon download failed: {}", response.status()));
    }
    if response.content_length().is_some_and(|len| len > MAX_ICON_BYTES) {
        return Err("Icon exceeds size limit".to_string());
    }

    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let ext = detect_extension(content_type.as_deref(), &url)
        .ok_or_else(|| format!("Unsupported icon format: {:?}", content_type))?;

    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("Failed to read icon body: {}", e))?;
    if bytes.is_empty() || bytes.len() as u64 > MAX_ICON_BYTES {
        return Err("Icon is empty or exceeds size limit".to_string());
    }

    // The instance may have been removed (e.g. installation cancelled) while downloading.
    if !tokio::fs::try_exists(instance_dir.join("dlml.json"))
        .await
        .unwrap_or(false)
    {
        return Err("Instance no longer exists".to_string());
    }

    let temp_path = instance_dir.join(ICON_TEMP_FILE);
    tokio::fs::write(&temp_path, &bytes)
        .await
        .map_err(|e| format!("Failed to write icon: {}", e))?;

    let file_name = format!("{}.{}", ICON_FILE_STEM, ext);
    for old_ext in SUPPORTED_EXTENSIONS.iter().filter(|e| **e != ext) {
        let _ = tokio::fs::remove_file(instance_dir.join(format!("{}.{}", ICON_FILE_STEM, old_ext))).await;
    }
    let final_path = instance_dir.join(&file_name);
    if let Err(e) = tokio::fs::rename(&temp_path, &final_path).await {
        let _ = tokio::fs::remove_file(&temp_path).await;
        return Err(format!("Failed to finalize icon: {}", e));
    }

    crate::core::launcher::update_instance_config(instance_dir, |config| {
        config.icon = Some(file_name.clone());
    })
    .await?;

    Ok(Some(final_path))
}

fn try_mark_in_flight(instance_id: &str) -> bool {
    match IN_FLIGHT.lock() {
        Ok(mut set) => set.insert(instance_id.to_string()),
        Err(_) => false,
    }
}

fn clear_in_flight(instance_id: &str) {
    if let Ok(mut set) = IN_FLIGHT.lock() {
        set.remove(instance_id);
    }
}

/// Fetch one icon, emit the update event on success and log failures.
async fn run_icon_job(app: &AppHandle, candidate: &IconBackfillCandidate) {
    if !try_mark_in_flight(&candidate.instance_id) {
        return;
    }

    match cache_modpack_icon(
        &candidate.instance_dir,
        &candidate.modpack_type,
        &candidate.project_id,
    )
    .await
    {
        Ok(Some(path)) => {
            tracing::info!("Cached icon for instance {}", candidate.instance_id);
            let _ = app.emit(
                INSTANCE_ICON_UPDATED_EVENT,
                InstanceIconUpdatedPayload {
                    instance_id: candidate.instance_id.clone(),
                    icon_path: path.to_string_lossy().to_string(),
                },
            );
        }
        Ok(None) => {
            tracing::info!("Modpack of instance {} has no icon", candidate.instance_id);
        }
        Err(e) => {
            tracing::warn!("Failed to cache icon for instance {}: {}", candidate.instance_id, e);
        }
    }

    clear_in_flight(&candidate.instance_id);
}

/// Cache the icon in the background (used during modpack installation / update).
pub fn spawn_cache_modpack_icon(app: AppHandle, candidate: IconBackfillCandidate) {
    tauri::async_runtime::spawn(async move {
        run_icon_job(&app, &candidate).await;
    });
}

/// Back-fill icons for existing modpack instances, sequentially in one background task.
/// Each instance is attempted at most once per session.
pub fn schedule_icon_backfill(app: AppHandle, candidates: Vec<IconBackfillCandidate>) {
    let pending: Vec<IconBackfillCandidate> = match BACKFILL_ATTEMPTED.lock() {
        Ok(mut attempted) => candidates
            .into_iter()
            .filter(|c| attempted.insert(c.instance_id.clone()))
            .collect(),
        Err(_) => return,
    };

    if pending.is_empty() {
        return;
    }

    tracing::info!("Back-filling icons for {} modpack instance(s)", pending.len());
    tauri::async_runtime::spawn(async move {
        for candidate in &pending {
            run_icon_job(&app, candidate).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_extension_from_content_type() {
        assert_eq!(detect_extension(Some("image/png"), "https://x/a"), Some("png"));
        assert_eq!(detect_extension(Some("image/jpeg; charset=binary"), "https://x/a"), Some("jpg"));
        assert_eq!(detect_extension(Some("IMAGE/WEBP"), "https://x/a"), Some("webp"));
        assert_eq!(detect_extension(Some("image/gif"), "https://x/a"), Some("gif"));
    }

    #[test]
    fn falls_back_to_url_extension() {
        assert_eq!(
            detect_extension(Some("application/octet-stream"), "https://cdn/x/icon.JPEG?v=1"),
            Some("jpg")
        );
        assert_eq!(detect_extension(None, "https://cdn/x/icon.webp#frag"), Some("webp"));
    }

    #[test]
    fn rejects_unsupported_formats() {
        assert_eq!(detect_extension(Some("image/svg+xml"), "https://cdn/x/icon.svg"), None);
        assert_eq!(detect_extension(None, "https://cdn/x/icon"), None);
    }

    #[tokio::test]
    async fn resolve_icon_path_rejects_traversal_and_missing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let icon = dir.path().join("icon.png");
        tokio::fs::write(&icon, b"x").await.expect("write icon");

        assert_eq!(resolve_icon_path(dir.path(), Some("icon.png")).await, Some(icon));
        assert_eq!(resolve_icon_path(dir.path(), Some("../icon.png")).await, None);
        assert_eq!(resolve_icon_path(dir.path(), Some("missing.png")).await, None);
        assert_eq!(resolve_icon_path(dir.path(), Some("")).await, None);
        assert_eq!(resolve_icon_path(dir.path(), None).await, None);
    }
}
