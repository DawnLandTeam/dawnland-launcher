use chrono::NaiveDate;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{layer::SubscriberExt, reload, util::SubscriberInitExt, EnvFilter};

use crate::core::settings::{LauncherSettings, LogLevelSetting};

pub mod frontend;
pub mod redact;

pub const DEFAULT_LOG_RETENTION_DAYS: u32 = 7;
pub const MAX_LOG_DIR_BYTES: u64 = 50 * 1024 * 1024; // 50 MiB
const LOG_FILE_PREFIX: &str = "dawnland.log.";
const MAX_READ_TAIL_LINES: usize = 5000;
const DEFAULT_READ_TAIL_LINES: usize = 500;
const MAX_DIAGNOSTIC_LOG_BYTES: usize = 20 * 1024 * 1024;

type ReloadHandle = reload::Handle<EnvFilter, tracing_subscriber::Registry>;

static RELOAD_HANDLE: OnceLock<ReloadHandle> = OnceLock::new();
static FILE_GUARD: OnceLock<Mutex<Option<WorkerGuard>>> = OnceLock::new();

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogFileInfo {
    pub name: String,
    pub size_bytes: u64,
    pub modified_at: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EffectiveLogConfig {
    pub configured_level: Option<LogLevelSetting>,
    pub effective_level: LogLevelSetting,
    pub retention_days: u32,
    pub max_total_mb: u32,
    pub log_dir: String,
}

/// Returns the unified launcher log directory (`<exe_dir>/.dawnland/logs`).
pub fn log_dir() -> PathBuf {
    crate::core::mojang::get_dawnland_dir().join("logs")
}

/// Resolves the effective log level from optional user configuration.
pub fn effective_log_level(configured: Option<LogLevelSetting>) -> LogLevelSetting {
    configured.unwrap_or(if cfg!(debug_assertions) {
        LogLevelSetting::Debug
    } else {
        LogLevelSetting::Info
    })
}

/// Builds an `EnvFilter` scoped to our application crate while throttling noisy dependencies.
pub fn build_env_filter(level: LogLevelSetting) -> EnvFilter {
    let directive = format!(
        "warn,dawnland_launcher_lib={},hyper=warn,reqwest=warn,h2=warn,rustls=warn,tokio=warn,tao=warn,wry=warn",
        level.as_directive_str()
    );
    EnvFilter::try_new(&directive).unwrap_or_else(|_| EnvFilter::new("info"))
}

/// Initialize the global tracing subscriber with runtime reloadable filtering:
/// - Optional console (stdout) in debug/e2e or when `DAWNLAND_STDOUT_LOGS` is set
/// - Rolling daily file appender under `.dawnland/logs/dawnland.log`
/// - Frontend IPC broadcast layer with Span-inherited `task_id` and startup ring buffer
pub fn init() -> Result<(), String> {
    let dir = log_dir();

    // Ensure the log directory exists synchronously during pre-runtime initialization
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Failed to create log directory {}: {e}", dir.display()))?;

    let settings = crate::core::settings::get_launcher_settings_sync();
    let effective = effective_log_level(settings.log_level);
    frontend::set_broadcast_level(effective);

    let file_appender = tracing_appender::rolling::daily(&dir, "dawnland.log");
    let (non_blocking_file, guard) = tracing_appender::non_blocking(file_appender);

    let guard_slot = FILE_GUARD.get_or_init(|| Mutex::new(None));
    if let Ok(mut slot) = guard_slot.lock() {
        *slot = Some(guard);
    }

    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| build_env_filter(effective));
    let (filter_layer, reload_handle) = reload::Layer::new(env_filter);
    let _ = RELOAD_HANDLE.set(reload_handle);

    let enable_stdout = cfg!(debug_assertions)
        || cfg!(feature = "e2e")
        || std::env::var_os("DAWNLAND_STDOUT_LOGS").is_some();
    let stdout_layer = enable_stdout.then(|| {
        tracing_subscriber::fmt::layer()
            .with_writer(std::io::stdout)
            .with_target(true)
    });

    tracing_subscriber::registry()
        .with(filter_layer)
        .with(stdout_layer)
        .with(
            tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_target(true)
                .with_writer(non_blocking_file),
        )
        .with(frontend::FrontendBroadcastLayer::new())
        .init();

    tracing::info!("Dawnland Launcher core initialized.");
    tracing::info!(
        "Log directory: {} (level: {})",
        dir.display(),
        effective.as_directive_str()
    );

    Ok(())
}

/// Applies a new log level at runtime without restarting the application.
pub fn apply_log_level(configured: Option<LogLevelSetting>) {
    let effective = effective_log_level(configured);
    frontend::set_broadcast_level(effective);

    if let Some(handle) = RELOAD_HANDLE.get() {
        let next_filter = build_env_filter(effective);
        if let Err(e) = handle.reload(next_filter) {
            tracing::warn!("Failed to reload runtime log filter: {}", e);
        } else {
            tracing::info!(
                "Runtime log level updated to '{}' (configured={:?})",
                effective.as_directive_str(),
                configured
            );
        }
    }
}

/// Applies the log level from a `LauncherSettings` instance.
pub fn apply_log_level_from_settings(settings: &LauncherSettings) {
    apply_log_level(settings.log_level);
}

/// Builds the current effective log configuration snapshot for the frontend UI.
pub fn get_effective_log_config(settings: &LauncherSettings) -> EffectiveLogConfig {
    let retention_days = settings.log_retention_days.clamp(1, 90);
    EffectiveLogConfig {
        configured_level: settings.log_level,
        effective_level: effective_log_level(settings.log_level),
        retention_days,
        max_total_mb: (MAX_LOG_DIR_BYTES / (1024 * 1024)) as u32,
        log_dir: log_dir().to_string_lossy().to_string(),
    }
}

/// Flushes and releases the non-blocking file appender worker guard on application exit.
pub fn shutdown() {
    if let Some(lock) = FILE_GUARD.get() {
        if let Ok(mut guard) = lock.lock() {
            let _ = guard.take();
        }
    }
}

/// Cleans up expired or oversized log files in the launcher log directory.
pub async fn cleanup_old_logs(retention_days: u32, max_total_bytes: u64) {
    let dir = log_dir();
    // `tracing_appender::rolling::daily` names files using UTC dates, so compare against UTC midnight.
    let today_utc = chrono::Utc::now().date_naive();
    cleanup_old_logs_in_dir(&dir, retention_days, max_total_bytes, today_utc).await;
}

fn parse_log_date_from_name(name: &str) -> Option<NaiveDate> {
    let suffix = name.strip_prefix(LOG_FILE_PREFIX)?;
    NaiveDate::parse_from_str(suffix, "%Y-%m-%d").ok()
}

#[derive(Clone)]
struct LogDiskCandidate {
    path: PathBuf,
    name: String,
    parsed_date: Option<NaiveDate>,
    modified_secs: u64,
    size: u64,
    is_today: bool,
    is_expired: bool,
}

/// Cleans up log files older than `retention_days` and trims oldest non-active logs when total size exceeds `max_total_bytes`.
///
/// Both `today_utc` logs and the newest non-expired log file present in `dir` are protected so the currently open
/// appender file is never deleted across timezone/UTC rollover boundaries.
pub async fn cleanup_old_logs_in_dir(
    dir: &Path,
    retention_days: u32,
    max_total_bytes: u64,
    today_utc: NaiveDate,
) {
    let mut entries = match tokio::fs::read_dir(dir).await {
        Ok(rd) => rd,
        Err(_) => return,
    };

    let retention_days = retention_days.max(1) as i64;
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut candidates: Vec<LogDiskCandidate> = Vec::new();

    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with(LOG_FILE_PREFIX) {
            continue;
        }

        let path = entry.path();
        let Ok(meta) = tokio::fs::symlink_metadata(&path).await else {
            continue;
        };
        if meta.file_type().is_symlink() || !meta.is_file() {
            continue;
        }

        let parsed_date = parse_log_date_from_name(&name);
        let modified_secs = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let size = meta.len();
        let is_today = parsed_date == Some(today_utc);

        let is_expired = if let Some(file_date) = parsed_date {
            (today_utc - file_date).num_days() >= retention_days
        } else if modified_secs > 0 {
            now_secs.saturating_sub(modified_secs) >= (retention_days as u64) * 86_400
        } else {
            false
        };

        candidates.push(LogDiskCandidate {
            path,
            name,
            parsed_date,
            modified_secs,
            size,
            is_today,
            is_expired,
        });
    }

    // Protect the newest non-expired file in addition to `today_utc` in case UTC midnight just passed
    // and the rolling appender still holds the previous day's file open.
    let newest_active_name = candidates
        .iter()
        .filter(|c| c.is_today || !c.is_expired)
        .max_by(|a, b| {
            a.parsed_date
                .cmp(&b.parsed_date)
                .then_with(|| a.modified_secs.cmp(&b.modified_secs))
                .then_with(|| a.name.cmp(&b.name))
        })
        .map(|c| c.name.clone());

    let is_protected = |c: &LogDiskCandidate| {
        c.is_today || newest_active_name.as_deref() == Some(c.name.as_str())
    };

    let mut kept_files: Vec<LogDiskCandidate> = Vec::new();
    let mut deleted_count = 0usize;
    let mut freed_bytes = 0u64;

    for candidate in candidates {
        if !is_protected(&candidate) && candidate.is_expired {
            if tokio::fs::remove_file(&candidate.path).await.is_ok() {
                deleted_count += 1;
                freed_bytes = freed_bytes.saturating_add(candidate.size);
            }
            continue;
        }
        kept_files.push(candidate);
    }

    let mut total_bytes: u64 = kept_files.iter().map(|f| f.size).sum();
    if total_bytes > max_total_bytes {
        kept_files.sort_by(|a, b| {
            a.parsed_date
                .cmp(&b.parsed_date)
                .then_with(|| a.modified_secs.cmp(&b.modified_secs))
                .then_with(|| a.name.cmp(&b.name))
        });

        for candidate in kept_files {
            if total_bytes <= max_total_bytes {
                break;
            }
            if is_protected(&candidate) {
                continue;
            }
            if tokio::fs::remove_file(&candidate.path).await.is_ok() {
                total_bytes = total_bytes.saturating_sub(candidate.size);
                deleted_count += 1;
                freed_bytes = freed_bytes.saturating_add(candidate.size);
            }
        }
    }

    if deleted_count > 0 {
        tracing::info!(
            "Cleaned up {} old log file(s), freed {} bytes",
            deleted_count,
            freed_bytes
        );
    }
}

/// Lists available launcher log files under `dir`, sorted newest-first.
pub async fn list_log_files_in_dir(dir: &Path) -> Result<Vec<LogFileInfo>, String> {
    let mut entries = match tokio::fs::read_dir(dir).await {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("Failed to read log directory: {e}")),
    };

    let mut files = Vec::new();
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with("dawnland.log") || name.ends_with(".zip") {
            continue;
        }
        let path = entry.path();
        let Ok(meta) = tokio::fs::symlink_metadata(&path).await else {
            continue;
        };
        if meta.file_type().is_symlink() || !meta.is_file() {
            continue;
        }
        let modified_at = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        files.push(LogFileInfo {
            name,
            size_bytes: meta.len(),
            modified_at,
        });
    }

    files.sort_by(|a, b| {
        b.modified_at
            .cmp(&a.modified_at)
            .then_with(|| b.name.cmp(&a.name))
    });
    Ok(files)
}

/// Reads the trailing lines of a named launcher log file safely (preventing path traversal).
pub async fn read_log_file_in_dir(
    dir: &Path,
    filename: &str,
    tail_lines: Option<usize>,
) -> Result<Vec<String>, String> {
    if !crate::core::fs_utils::is_safe_single_name(filename) || !filename.starts_with("dawnland.log")
    {
        return Err("Invalid log filename".to_string());
    }

    let path = dir.join(filename);
    let meta = tokio::fs::symlink_metadata(&path)
        .await
        .map_err(|e| format!("Failed to inspect log file: {e}"))?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err("Invalid log file target".to_string());
    }

    let content = tokio::fs::read_to_string(&path)
        .await
        .map_err(|e| format!("Failed to read log file: {e}"))?;
    let limit = tail_lines
        .unwrap_or(DEFAULT_READ_TAIL_LINES)
        .clamp(1, MAX_READ_TAIL_LINES);

    let all_lines: Vec<&str> = content.lines().collect();
    let start = all_lines.len().saturating_sub(limit);
    Ok(all_lines[start..]
        .iter()
        .map(|line| redact::redact_sensitive(line))
        .collect())
}

/// Reads up to the trailing `max_bytes` of a log file and redacts sensitive fields/paths without loading oversized files into memory.
async fn read_tail_redacted_bytes(path: &Path, max_bytes: usize) -> Option<Vec<u8>> {
    use tokio::io::{AsyncReadExt, AsyncSeekExt};

    if max_bytes == 0 {
        return Some(Vec::new());
    }

    let mut file = tokio::fs::File::open(path).await.ok()?;
    let file_len = file.metadata().await.ok()?.len();
    let truncated_head = file_len > max_bytes as u64;
    if truncated_head {
        let start = file_len - max_bytes as u64;
        file.seek(std::io::SeekFrom::Start(start)).await.ok()?;
    }

    let mut buf = Vec::with_capacity(file_len.min(max_bytes as u64) as usize);
    file.take(max_bytes as u64).read_to_end(&mut buf).await.ok()?;

    let raw_slice: &[u8] = if truncated_head {
        // Advance past the first partial line (or partial UTF-8 lead bytes) after a mid-file seek.
        if let Some(pos) = buf.iter().position(|&b| b == b'\n') {
            &buf[pos + 1..]
        } else {
            let mut offset = 0;
            while offset < buf.len() && offset < 3 && (buf[offset] & 0b1100_0000) == 0b1000_0000 {
                offset += 1;
            }
            &buf[offset..]
        }
    } else {
        &buf[..]
    };

    let raw_text = String::from_utf8_lossy(raw_slice);
    let redacted = redact::redact_user_paths(&redact::redact_sensitive(&raw_text));
    let mut bytes = redacted.into_bytes();
    if bytes.len() > max_bytes {
        let mut start = bytes.len() - max_bytes;
        while start < bytes.len() && (bytes[start] & 0b1100_0000) == 0b1000_0000 {
            start += 1;
        }
        bytes.drain(..start);
    }
    Some(bytes)
}

/// Builds a sanitized diagnostic summary JSON string from `LauncherSettings`.
fn build_sanitized_summary_json(mut settings: LauncherSettings) -> String {
    if settings.ai_config.remote_api_key.is_some() {
        settings.ai_config.remote_api_key = Some("***REDACTED***".to_string());
    }
    if let Some(url) = settings.ai_config.remote_base_url.as_mut() {
        *url = redact::redact_user_paths(&redact::redact_sensitive(url));
    }

    let mut summary = serde_json::json!({
        "generatedAt": chrono::Local::now().to_rfc3339(),
        "appVersion": env!("CARGO_PKG_VERSION"),
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "family": std::env::consts::FAMILY,
        "effectiveLogLevel": effective_log_level(settings.log_level),
        "settings": settings,
    });
    redact::sanitize_json_value(&mut summary);

    let serialized = serde_json::to_string_pretty(&summary).unwrap_or_else(|_| "{}".to_string());
    redact::redact_user_paths(&redact::redact_sensitive(&serialized))
}

/// Exports recent redacted launcher logs, sanitized settings/system summary, and bounded game latest.log tail to a zip file.
pub async fn export_diagnostic_bundle(
    dest_path: &Path,
    include_game_logs: bool,
    instance_id: Option<&str>,
) -> Result<String, String> {
    let dir = log_dir();
    let files = list_log_files_in_dir(&dir).await?;

    let mut payloads: Vec<(String, Vec<u8>)> = Vec::new();
    let mut accumulated = 0usize;

    for info in files.into_iter().take(3) {
        let remaining = MAX_DIAGNOSTIC_LOG_BYTES.saturating_sub(accumulated);
        if remaining == 0 {
            break;
        }
        let path = dir.join(&info.name);
        if let Some(take_bytes) = read_tail_redacted_bytes(&path, remaining).await {
            if !take_bytes.is_empty() {
                accumulated += take_bytes.len();
                payloads.push((format!("launcher/{}", info.name), take_bytes));
            }
        }
    }

    let settings = crate::core::settings::get_launcher_settings().await;
    let summary_str = build_sanitized_summary_json(settings);
    payloads.push(("system_summary.json".to_string(), summary_str.into_bytes()));

    if include_game_logs {
        if let Some(inst_id) = instance_id.map(str::trim).filter(|s| !s.is_empty()) {
            if !crate::core::fs_utils::is_safe_single_name(inst_id) {
                return Err("Invalid instance ID".to_string());
            }
            let game_log_path = crate::core::mojang::get_minecraft_base()
                .join("versions")
                .join(inst_id)
                .join("logs")
                .join("latest.log");
            if let Ok(meta) = tokio::fs::symlink_metadata(&game_log_path).await {
                if !meta.file_type().is_symlink() && meta.is_file() {
                    if let Some(bytes) =
                        read_tail_redacted_bytes(&game_log_path, MAX_DIAGNOSTIC_LOG_BYTES).await
                    {
                        payloads.push((format!("game/{}/latest.log", inst_id), bytes));
                    }
                }
            }
        }
    }

    if let Some(parent) = dest_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("Failed to create export directory: {e}"))?;
    }

    let output_path = dest_path.to_path_buf();
    let output_display = output_path.to_string_lossy().to_string();

    tokio::task::spawn_blocking(move || -> Result<(), String> {
        use std::io::Write;
        let zip_file = std::fs::File::create(&output_path)
            .map_err(|e| format!("Failed to create zip file: {e}"))?;
        let mut zip = zip::ZipWriter::new(zip_file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);

        for (entry_name, bytes) in payloads {
            zip.start_file(&entry_name, options)
                .map_err(|e| format!("Failed to write zip entry {entry_name}: {e}"))?;
            zip.write_all(&bytes)
                .map_err(|e| format!("Failed to write zip payload {entry_name}: {e}"))?;
        }

        zip.finish()
            .map_err(|e| format!("Failed to finalize diagnostic zip: {e}"))?;
        Ok(())
    })
    .await
    .map_err(|e| format!("Diagnostic export task failed: {e}"))??;

    Ok(output_display)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_cleanup_old_logs_enforces_retention_days_and_size_cap() {
        let dir = tempdir().expect("tempdir");
        let today = NaiveDate::from_ymd_opt(2026, 11, 16).expect("valid date");

        let today_file = dir.path().join("dawnland.log.2026-11-16");
        let recent_ok = dir.path().join("dawnland.log.2026-11-15");
        let mid_file = dir.path().join("dawnland.log.2026-11-12");
        let expired_file = dir.path().join("dawnland.log.2026-11-08");
        let unrelated_file = dir.path().join("notes.txt");

        tokio::fs::write(&today_file, vec![b'a'; 60])
            .await
            .expect("write today");
        tokio::fs::write(&recent_ok, vec![b'b'; 60])
            .await
            .expect("write recent");
        tokio::fs::write(&mid_file, vec![b'c'; 60])
            .await
            .expect("write mid");
        tokio::fs::write(&expired_file, vec![b'd'; 60])
            .await
            .expect("write expired");
        tokio::fs::write(&unrelated_file, b"keep me")
            .await
            .expect("write unrelated");

        // Retain 7 days, max 130 bytes -> expired (8 days old) removed first, then mid_file (11-12) removed to fit <= 130 bytes
        cleanup_old_logs_in_dir(dir.path(), 7, 130, today).await;

        assert!(today_file.exists(), "today's active log must be preserved");
        assert!(recent_ok.exists(), "newest prior log should fit in 130 bytes");
        assert!(!mid_file.exists(), "older file should be evicted to respect size cap");
        assert!(!expired_file.exists(), "expired file (>= 7 days) must be removed");
        assert!(unrelated_file.exists(), "non-log files must never be deleted");
    }

    #[tokio::test]
    async fn test_read_log_file_rejects_traversal_and_reads_tail() {
        let dir = tempdir().expect("tempdir");
        let file = dir.path().join("dawnland.log.2026-11-16");
        tokio::fs::write(&file, "line1\nline2\nBearer secret_tok_123456\n")
            .await
            .expect("write log");

        assert!(read_log_file_in_dir(dir.path(), "../secret.txt", None)
            .await
            .is_err());
        assert!(read_log_file_in_dir(dir.path(), "other.txt", None)
            .await
            .is_err());

        let lines = read_log_file_in_dir(dir.path(), "dawnland.log.2026-11-16", Some(2))
            .await
            .expect("read tail");
        assert_eq!(lines, vec!["line2", "Bearer ***"]);
    }

    #[tokio::test]
    async fn test_cleanup_old_logs_preserves_newest_active_file_across_utc_rollover() {
        let dir = tempdir().expect("tempdir");
        let today_utc = NaiveDate::from_ymd_opt(2026, 11, 17).expect("valid date");

        let active_prev_utc = dir.path().join("dawnland.log.2026-11-16");
        let older_file = dir.path().join("dawnland.log.2026-11-15");

        tokio::fs::write(&active_prev_utc, vec![b'x'; 80])
            .await
            .expect("write active");
        tokio::fs::write(&older_file, vec![b'y'; 80])
            .await
            .expect("write older");

        // Size cap smaller than even active_prev_utc alone: older file is evicted, newest active file must never be unlinked
        cleanup_old_logs_in_dir(dir.path(), 7, 50, today_utc).await;

        assert!(
            active_prev_utc.exists(),
            "newest active rolling log must never be deleted across midnight/timezone boundaries"
        );
        assert!(!older_file.exists(), "older file should be evicted first");
    }

    #[test]
    fn test_build_sanitized_summary_redacts_secrets_urls_and_paths() {
        let mut settings = LauncherSettings::default();
        settings.ai_config.remote_api_key = Some("sk-proj-secret123456".to_string());
        settings.ai_config.remote_base_url =
            Some("https://alice:topsecret@api.example.com/v1?api_key=query_secret".to_string());
        settings.resource_naming.custom_template = r"C:\Users\Alice\Desktop\{stem}".to_string();

        let summary_json = build_sanitized_summary_json(settings);
        assert!(!summary_json.contains("sk-proj-secret123456"));
        assert!(!summary_json.contains("topsecret"));
        assert!(!summary_json.contains("query_secret"));
        assert!(!summary_json.contains("Alice"));
        let parsed: serde_json::Value =
            serde_json::from_str(&summary_json).expect("sanitized summary must remain valid JSON");
        assert_eq!(parsed["settings"]["aiConfig"]["remoteApiKey"], "***");
        assert_eq!(
            parsed["settings"]["aiConfig"]["remoteBaseUrl"],
            "https://***@api.example.com/v1?api_key=***"
        );
        assert!(parsed["settings"]["resourceNaming"]["customTemplate"]
            .as_str()
            .expect("customTemplate string")
            .contains("<USER>"));
    }

    #[tokio::test]
    async fn test_read_tail_redacted_bytes_caps_huge_log_files() {
        let dir = tempdir().expect("tempdir");
        let log_path = dir.path().join("latest.log");

        let mut content = String::from("EARLY_LINE Bearer early_tok_999999\n");
        for i in 0..4000 {
            content.push_str(&format!("[{i}] Normal modded output line padding text\n"));
        }
        content.push_str("FINAL_LINE Bearer tail_tok_123456 C:\\Users\\Bob\\instance\n");
        tokio::fs::write(&log_path, &content)
            .await
            .expect("write huge log");

        let bytes = read_tail_redacted_bytes(&log_path, 1024)
            .await
            .expect("read tail bytes");
        assert!(bytes.len() <= 1024);

        let tail = String::from_utf8(bytes).expect("valid utf8");
        assert!(!tail.contains("EARLY_LINE"));
        assert!(tail.contains("FINAL_LINE"));
        assert!(!tail.contains("tail_tok_123456"));
        assert!(!tail.contains("Bob"));
        assert!(tail.contains("Bearer ***"));
        assert!(tail.contains("<USER>"));
    }
}
