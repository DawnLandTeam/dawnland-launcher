//! Instance Icon Cache
//!
//! Fetches and caches the icon of modpack instances installed from CurseForge or
//! Modrinth. Icons are stored inside the instance directory as `icon.<ext>` and the
//! file name is recorded in `dlml.json` (`InstanceConfig::icon`). All operations are
//! best-effort: failures are logged and never abort an installation.

use futures_util::StreamExt;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter};

/// Event emitted to the frontend once an instance icon has been cached.
pub const INSTANCE_ICON_UPDATED_EVENT: &str = "instance-icon-updated";

const ICON_FILE_STEM: &str = "icon";
const SUPPORTED_EXTENSIONS: [&str; 4] = ["png", "jpg", "webp", "gif"];
const MAX_ICON_BYTES: u64 = 2 * 1024 * 1024;
const ICON_LOOKUP_TIMEOUT: Duration = Duration::from_secs(15);
const ICON_DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(20);

/// Approved public image hosts (exact or subdomain match) for modpack icons.
const ALLOWED_ICON_HOST_SUFFIXES: [&str; 4] = [
    "forgecdn.net",
    "curseforge.com",
    "modrinth.com",
    "githubusercontent.com",
];

#[derive(Debug, Clone)]
struct InstanceJobGeneration {
    generation: u64,
    modpack_type: String,
    project_id: Option<String>,
}

/// Latest generation state per instance ID so stale jobs from earlier installs/updates
/// cannot block newer jobs or overwrite a reinstalled/updated instance's icon.
static INSTANCE_GENERATIONS: LazyLock<Mutex<HashMap<String, InstanceJobGeneration>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
/// Keys `(instance_id, generation)` currently being fetched.
static IN_FLIGHT: LazyLock<Mutex<HashSet<(String, u64)>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));
/// Instance IDs already back-filled (or currently queued) during this session.
static BACKFILL_ATTEMPTED: LazyLock<Mutex<HashSet<String>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

static NEXT_GENERATION_COUNTER: AtomicU64 = AtomicU64::new(1);
static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(1);

/// HTTP client configured strictly for public icon downloads (validates every redirect).
static ICON_HTTP_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    let policy = reqwest::redirect::Policy::custom(|attempt| {
        if attempt.previous().len() >= 5 {
            return attempt.error("Too many redirects while fetching icon");
        }
        if let Err(reason) = validate_icon_url(attempt.url()) {
            return attempt.error(reason);
        }
        attempt.follow()
    });
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(ICON_DOWNLOAD_TIMEOUT)
        .redirect(policy)
        .user_agent("Dawnland-Launcher/1.0")
        .build()
        .unwrap_or_default()
});

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceIconUpdatedPayload {
    pub instance_id: String,
    pub icon_path: String,
    pub updated_at: u64,
}

/// A modpack instance whose icon should be fetched and cached.
#[derive(Debug, Clone)]
pub struct IconBackfillCandidate {
    pub instance_id: String,
    pub instance_dir: PathBuf,
    pub modpack_type: String,
    pub project_id: String,
}

/// Record a new install/update generation for `instance_id`, invalidating any
/// older in-flight icon jobs for the same instance.
pub fn advance_instance_icon_generation(
    instance_id: &str,
    modpack_type: Option<&str>,
    project_id: Option<&str>,
) -> u64 {
    let generation = NEXT_GENERATION_COUNTER.fetch_add(1, Ordering::Relaxed);
    if let Ok(mut map) = INSTANCE_GENERATIONS.lock() {
        map.insert(
            instance_id.to_string(),
            InstanceJobGeneration {
                generation,
                modpack_type: modpack_type.unwrap_or_default().to_ascii_lowercase(),
                project_id: project_id
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned),
            },
        );
    }
    if let Ok(mut attempted) = BACKFILL_ATTEMPTED.lock() {
        attempted.remove(instance_id);
    }
    generation
}

/// Ensure a generation entry exists for `candidate` (used by backfill when no install ran
/// in the current session) and return its active generation if it matches `candidate`.
fn ensure_or_match_generation(candidate: &IconBackfillCandidate) -> Option<u64> {
    let mut map = INSTANCE_GENERATIONS.lock().ok()?;
    let norm_type = candidate.modpack_type.to_ascii_lowercase();
    let norm_pid = candidate.project_id.trim().to_string();
    match map.get(&candidate.instance_id) {
        Some(existing) => {
            if existing.modpack_type == norm_type
                && existing.project_id.as_deref() == Some(norm_pid.as_str())
            {
                Some(existing.generation)
            } else {
                None
            }
        }
        None => {
            let generation = NEXT_GENERATION_COUNTER.fetch_add(1, Ordering::Relaxed);
            map.insert(
                candidate.instance_id.clone(),
                InstanceJobGeneration {
                    generation,
                    modpack_type: norm_type,
                    project_id: Some(norm_pid),
                },
            );
            Some(generation)
        }
    }
}

fn is_generation_current(
    instance_id: &str,
    generation: u64,
    modpack_type: &str,
    project_id: &str,
) -> bool {
    let Ok(map) = INSTANCE_GENERATIONS.lock() else {
        return false;
    };
    let Some(curr) = map.get(instance_id) else {
        return false;
    };
    curr.generation == generation
        && curr.modpack_type.eq_ignore_ascii_case(modpack_type)
        && curr.project_id.as_deref() == Some(project_id.trim())
}

/// Validate that a URL points to an approved public HTTPS host and never a private,
/// loopback, link-local, or unspecified address.
fn validate_icon_url(url: &reqwest::Url) -> Result<(), String> {
    if url.scheme() != "https" {
        return Err(format!("Unsupported icon URL scheme: {}", url.scheme()));
    }
    let Some(raw_host) = url.host_str() else {
        return Err("Icon URL is missing a host".to_string());
    };
    let host_unbracketed = raw_host
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(raw_host);

    if let Ok(ip) = host_unbracketed.parse::<IpAddr>() {
        match ip {
            IpAddr::V4(v4) if is_disallowed_ipv4(v4) => {
                return Err("Icon URL points to a disallowed IPv4 address".to_string());
            }
            IpAddr::V6(v6) if is_disallowed_ipv6(v6) => {
                return Err("Icon URL points to a disallowed IPv6 address".to_string());
            }
            _ => return Err("Direct IP icon URLs are not allowed".to_string()),
        }
    }

    let d = host_unbracketed.trim_end_matches('.').to_ascii_lowercase();
    if d == "localhost"
        || d.ends_with(".localhost")
        || d.ends_with(".local")
        || d.ends_with(".internal")
    {
        return Err("Icon URL points to a local domain".to_string());
    }

    let allowed = ALLOWED_ICON_HOST_SUFFIXES
        .iter()
        .any(|suffix| d == *suffix || d.ends_with(&format!(".{}", suffix)));
    if !allowed {
        return Err(format!("Icon host '{}' is not in the approved list", d));
    }
    Ok(())
}

fn is_disallowed_ipv4(ip: Ipv4Addr) -> bool {
    ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_documentation()
        || ip.is_unspecified()
        || ip.octets()[0] == 100 && (ip.octets()[1] & 0b1100_0000) == 64 // CGNAT 100.64.0.0/10
}

fn is_disallowed_ipv6(ip: Ipv6Addr) -> bool {
    ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_multicast()
        || (ip.segments()[0] & 0xfe00) == 0xfc00 // Unique local (fc00::/7)
        || (ip.segments()[0] & 0xffc0) == 0xfe80 // Link-local (fe80::/10)
        || ip.to_ipv4_mapped().is_some_and(is_disallowed_ipv4)
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

/// Query the platform API for the project's icon URL with an overall timeout.
async fn fetch_icon_url(modpack_type: &str, project_id: &str) -> Result<Option<String>, String> {
    let lookup = async {
        match modpack_type.to_ascii_lowercase().as_str() {
            "curseforge" => crate::core::curseforge::get_cf_mod_details(project_id.to_string()).await,
            "modrinth" => crate::core::modrinth::get_modrinth_mod_details(project_id.to_string()).await,
            other => Err(format!("Unsupported modpack source: {}", other)),
        }
    };

    let project = tokio::time::timeout(ICON_LOOKUP_TIMEOUT, lookup)
        .await
        .map_err(|_| "Timed out while fetching modpack details for icon".to_string())??;

    Ok(project.icon_url.filter(|u| !u.trim().is_empty()))
}

/// Stream a response body up to `max_bytes`, aborting immediately if exceeded.
async fn read_body_bounded(
    response: reqwest::Response,
    max_bytes: u64,
) -> Result<Vec<u8>, String> {
    if response.content_length().is_some_and(|len| len > max_bytes) {
        return Err("Icon exceeds size limit".to_string());
    }

    let capacity = response
        .content_length()
        .map(|len| (len as usize).min(max_bytes as usize))
        .unwrap_or(16 * 1024);
    let mut buf = Vec::with_capacity(capacity);
    let mut stream = response.bytes_stream();

    while let Some(chunk_res) = stream.next().await {
        let chunk = chunk_res.map_err(|e| format!("Failed to read icon stream: {}", e))?;
        if (buf.len() as u64).saturating_add(chunk.len() as u64) > max_bytes {
            return Err("Icon exceeds size limit".to_string());
        }
        buf.extend_from_slice(&chunk);
    }

    if buf.is_empty() {
        return Err("Icon response body is empty".to_string());
    }
    Ok(buf)
}

/// Replace `final_path` with `temp_path` safely on all platforms (including Windows
/// when `final_path` already exists), preserving any existing file if replacement or
/// `commit` fails.
async fn commit_icon_file<F, Fut>(
    instance_dir: &Path,
    temp_path: &Path,
    final_path: &Path,
    new_ext: &str,
    commit: F,
) -> Result<(), String>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<(), String>>,
{
    let nonce = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let backup_path = instance_dir.join(format!("{}.backup.{}.{}", ICON_FILE_STEM, nonce, new_ext));
    let had_existing_same_ext = tokio::fs::try_exists(final_path).await.unwrap_or(false);

    if had_existing_same_ext {
        if let Err(e) = tokio::fs::rename(final_path, &backup_path).await {
            let _ = tokio::fs::remove_file(temp_path).await;
            return Err(format!("Failed to stage existing icon backup: {}", e));
        }
    }

    if let Err(e) = tokio::fs::rename(temp_path, final_path).await {
        let _ = tokio::fs::remove_file(temp_path).await;
        if had_existing_same_ext {
            let _ = tokio::fs::rename(&backup_path, final_path).await;
        }
        return Err(format!("Failed to finalize icon: {}", e));
    }

    if let Err(e) = commit().await {
        let _ = tokio::fs::remove_file(final_path).await;
        if had_existing_same_ext {
            let _ = tokio::fs::rename(&backup_path, final_path).await;
        }
        return Err(e);
    }

    if had_existing_same_ext {
        let _ = tokio::fs::remove_file(&backup_path).await;
    }

    // Only remove icons of other extensions after the new file and config are committed.
    for old_ext in SUPPORTED_EXTENSIONS.iter().filter(|e| **e != new_ext) {
        let _ = tokio::fs::remove_file(instance_dir.join(format!("{}.{}", ICON_FILE_STEM, old_ext)))
            .await;
    }

    Ok(())
}

/// Download the modpack icon into `instance_dir` and record it in `dlml.json`.
///
/// Returns the absolute path of the cached icon, or `None` when the project has no icon.
pub async fn cache_modpack_icon(
    instance_id: &str,
    instance_dir: &Path,
    modpack_type: &str,
    project_id: &str,
    generation: u64,
) -> Result<Option<PathBuf>, String> {
    let Some(raw_url) = fetch_icon_url(modpack_type, project_id).await? else {
        return Ok(None);
    };

    let parsed_url =
        reqwest::Url::parse(&raw_url).map_err(|e| format!("Invalid icon URL: {}", e))?;
    validate_icon_url(&parsed_url)?;

    let response = ICON_HTTP_CLIENT
        .get(parsed_url)
        .send()
        .await
        .map_err(|e| format!("Failed to request icon: {}", e))?;

    validate_icon_url(response.url())?;

    if !response.status().is_success() {
        return Err(format!("Icon download failed: {}", response.status()));
    }

    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let final_url_str = response.url().as_str().to_owned();
    let ext = detect_extension(content_type.as_deref(), &final_url_str)
        .ok_or_else(|| format!("Unsupported icon format: {:?}", content_type))?;

    let bytes = tokio::time::timeout(
        ICON_DOWNLOAD_TIMEOUT,
        read_body_bounded(response, MAX_ICON_BYTES),
    )
    .await
    .map_err(|_| "Timed out while reading icon body".to_string())??;

    if !is_generation_current(instance_id, generation, modpack_type, project_id) {
        return Err("Icon job superseded by a newer instance operation".to_string());
    }

    if !tokio::fs::try_exists(instance_dir.join("dlml.json"))
        .await
        .unwrap_or(false)
    {
        return Err("Instance no longer exists".to_string());
    }

    let nonce = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let temp_path = instance_dir.join(format!("{}.download.{}", ICON_FILE_STEM, nonce));
    tokio::fs::write(&temp_path, &bytes)
        .await
        .map_err(|e| format!("Failed to write icon: {}", e))?;

    let file_name = format!("{}.{}", ICON_FILE_STEM, ext);
    let final_path = instance_dir.join(&file_name);

    let expected_type = modpack_type.to_string();
    let expected_pid = project_id.trim().to_string();
    let file_name_for_cfg = file_name.clone();
    let instance_id_owned = instance_id.to_string();

    commit_icon_file(instance_dir, &temp_path, &final_path, ext, || async move {
        if !is_generation_current(&instance_id_owned, generation, &expected_type, &expected_pid) {
            return Err("Icon job superseded before config commit".to_string());
        }
        crate::core::launcher::update_instance_config_if(
            instance_dir,
            |config| {
                if !is_generation_current(
                    &instance_id_owned,
                    generation,
                    &expected_type,
                    &expected_pid,
                ) {
                    return false;
                }
                let type_matches = config
                    .modpack_type
                    .as_deref()
                    .is_some_and(|t| t.eq_ignore_ascii_case(&expected_type));
                let pid_matches = config
                    .modpack_project_id
                    .as_deref()
                    .map(str::trim)
                    == Some(expected_pid.as_str());
                type_matches && pid_matches
            },
            |config| {
                config.icon = Some(file_name_for_cfg);
            },
        )
        .await
        .map(|_| ())
    })
    .await?;

    Ok(Some(final_path))
}

fn try_mark_in_flight(instance_id: &str, generation: u64) -> bool {
    match IN_FLIGHT.lock() {
        Ok(mut set) => set.insert((instance_id.to_string(), generation)),
        Err(_) => false,
    }
}

fn clear_in_flight(instance_id: &str, generation: u64) {
    if let Ok(mut set) = IN_FLIGHT.lock() {
        set.remove(&(instance_id.to_string(), generation));
    }
}

fn unmark_backfill_attempted(instance_id: &str) {
    if let Ok(mut attempted) = BACKFILL_ATTEMPTED.lock() {
        attempted.remove(instance_id);
    }
}

/// Fetch one icon, emit the update event on success, and return whether it succeeded.
async fn run_icon_job(app: &AppHandle, candidate: &IconBackfillCandidate, generation: u64) -> bool {
    if !is_generation_current(
        &candidate.instance_id,
        generation,
        &candidate.modpack_type,
        &candidate.project_id,
    ) {
        return false;
    }

    if !try_mark_in_flight(&candidate.instance_id, generation) {
        return true;
    }

    let outcome = cache_modpack_icon(
        &candidate.instance_id,
        &candidate.instance_dir,
        &candidate.modpack_type,
        &candidate.project_id,
        generation,
    )
    .await;

    clear_in_flight(&candidate.instance_id, generation);

    match outcome {
        Ok(Some(path)) => {
            tracing::info!("Cached icon for instance {}", candidate.instance_id);
            let updated_at = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(generation);
            let _ = app.emit(
                INSTANCE_ICON_UPDATED_EVENT,
                InstanceIconUpdatedPayload {
                    instance_id: candidate.instance_id.clone(),
                    icon_path: path.to_string_lossy().to_string(),
                    updated_at,
                },
            );
            true
        }
        Ok(None) => {
            tracing::info!("Modpack of instance {} has no icon", candidate.instance_id);
            true
        }
        Err(e) => {
            tracing::warn!(
                "Failed to cache icon for instance {}: {}",
                candidate.instance_id,
                e
            );
            false
        }
    }
}

/// Cache the icon in the background (used during modpack installation / update).
pub fn spawn_cache_modpack_icon(
    app: AppHandle,
    candidate: IconBackfillCandidate,
    generation: u64,
) {
    tauri::async_runtime::spawn(async move {
        if !run_icon_job(&app, &candidate, generation).await {
            unmark_backfill_attempted(&candidate.instance_id);
        }
    });
}

/// Back-fill icons for existing modpack instances, sequentially in one background task.
/// Failed instances are removed from `BACKFILL_ATTEMPTED` so a subsequent scan can retry.
pub fn schedule_icon_backfill(app: AppHandle, candidates: Vec<IconBackfillCandidate>) {
    let pending: Vec<(IconBackfillCandidate, u64)> = match BACKFILL_ATTEMPTED.lock() {
        Ok(mut attempted) => candidates
            .into_iter()
            .filter_map(|c| {
                let gen = ensure_or_match_generation(&c)?;
                if attempted.insert(c.instance_id.clone()) {
                    Some((c, gen))
                } else {
                    None
                }
            })
            .collect(),
        Err(_) => return,
    };

    if pending.is_empty() {
        return;
    }

    tracing::info!("Back-filling icons for {} modpack instance(s)", pending.len());
    tauri::async_runtime::spawn(async move {
        for (candidate, generation) in &pending {
            if !run_icon_job(&app, candidate, *generation).await {
                unmark_backfill_attempted(&candidate.instance_id);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_extension_from_content_type() {
        assert_eq!(detect_extension(Some("image/png"), "https://x/a"), Some("png"));
        assert_eq!(
            detect_extension(Some("image/jpeg; charset=binary"), "https://x/a"),
            Some("jpg")
        );
        assert_eq!(detect_extension(Some("IMAGE/WEBP"), "https://x/a"), Some("webp"));
        assert_eq!(detect_extension(Some("image/gif"), "https://x/a"), Some("gif"));
    }

    #[test]
    fn falls_back_to_url_extension() {
        assert_eq!(
            detect_extension(
                Some("application/octet-stream"),
                "https://cdn.modrinth.com/x/icon.JPEG?v=1"
            ),
            Some("jpg")
        );
        assert_eq!(
            detect_extension(None, "https://cdn.modrinth.com/x/icon.webp#frag"),
            Some("webp")
        );
    }

    #[test]
    fn rejects_unsupported_formats() {
        assert_eq!(
            detect_extension(Some("image/svg+xml"), "https://cdn.modrinth.com/x/icon.svg"),
            None
        );
        assert_eq!(detect_extension(None, "https://cdn.modrinth.com/x/icon"), None);
    }

    #[test]
    fn validates_icon_urls_against_ssrf() {
        let ok_mr = reqwest::Url::parse("https://cdn.modrinth.com/data/P7dR8mSH/icon.png").unwrap();
        let ok_cf =
            reqwest::Url::parse("https://media.forgecdn.net/avatars/thumbnail/1.png").unwrap();
        assert!(validate_icon_url(&ok_mr).is_ok());
        assert!(validate_icon_url(&ok_cf).is_ok());

        for bad in [
            "http://cdn.modrinth.com/data/P7dR8mSH/icon.png",
            "https://127.0.0.1/icon.png",
            "https://localhost/icon.png",
            "https://192.168.1.1/icon.png",
            "https://10.0.0.1/icon.png",
            "https://169.254.169.254/latest/meta-data",
            "https://[::1]/icon.png",
            "https://evil.example.com/icon.png",
        ] {
            let u = reqwest::Url::parse(bad).unwrap();
            assert!(validate_icon_url(&u).is_err(), "expected {bad} to be rejected");
        }
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

    #[tokio::test]
    async fn commit_icon_file_overwrites_same_ext_and_rolls_back_on_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let final_path = dir.path().join("icon.png");
        tokio::fs::write(&final_path, b"old-bytes").await.unwrap();

        // Successful replacement of same extension (Windows-safe)
        let temp_ok = dir.path().join("icon.download.1");
        tokio::fs::write(&temp_ok, b"new-bytes").await.unwrap();
        commit_icon_file(dir.path(), &temp_ok, &final_path, "png", || async { Ok(()) })
            .await
            .expect("commit succeeds");
        assert_eq!(tokio::fs::read(&final_path).await.unwrap(), b"new-bytes");

        // Failed commit rolls back to the previous icon
        let temp_fail = dir.path().join("icon.download.2");
        tokio::fs::write(&temp_fail, b"bad-bytes").await.unwrap();
        let res = commit_icon_file(dir.path(), &temp_fail, &final_path, "png", || async {
            Err("simulated config failure".to_string())
        })
        .await;
        assert!(res.is_err());
        assert_eq!(tokio::fs::read(&final_path).await.unwrap(), b"new-bytes");
    }
}
