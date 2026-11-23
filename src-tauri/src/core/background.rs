//! Launcher Custom Background Manager
//!
//! Validates user-selected background images by magic bytes, copies them atomically
//! into the managed `.dawnland/backgrounds/` directory, updates settings atomically,
//! and cleans up superseded background files.

use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::Manager;
use tokio::io::AsyncReadExt;

use crate::core::fs_utils::{is_safe_single_name, write_file_atomic, FsError};
use crate::core::settings::{
    get_launcher_settings, update_background_settings, BackgroundFit, BackgroundMode,
    BackgroundSettings, DEFAULT_BACKGROUND_BLUR_PX, MAX_BACKGROUND_BLUR_PX,
    MAX_BACKGROUND_OVERLAY_OPACITY,
};
use crate::error::{AppError, DawnlandError};
use crate::logger::redact::redact_user_paths;

pub const MAX_BACKGROUND_MB: u64 = 20;
pub const MAX_BACKGROUND_BYTES: u64 = MAX_BACKGROUND_MB * 1024 * 1024;
const BACKGROUND_FILE_PREFIX: &str = "bg-";
const SUPPORTED_EXTENSIONS: [&str; 4] = ["png", "jpg", "webp", "gif"];

/// Serializes background import/reset disk operations so cleanup cannot race with imports.
static BACKGROUND_OP_LOCK: LazyLock<tokio::sync::Mutex<()>> =
    LazyLock::new(|| tokio::sync::Mutex::new(()));

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LauncherBackgroundState {
    pub mode: BackgroundMode,
    pub image_file: Option<String>,
    pub image_path: Option<String>,
    pub file_size_bytes: Option<u64>,
    pub blur_px: u8,
    pub overlay_opacity: Option<u8>,
    pub fit: BackgroundFit,
    pub updated_at: u64,
}

/// Returns the managed directory where custom launcher backgrounds are stored.
pub fn get_backgrounds_dir() -> PathBuf {
    crate::core::mojang::get_dawnland_dir().join("backgrounds")
}

/// Detects an approved raster image format strictly from file header magic bytes.
///
/// Explicitly rejects SVG, HTML, text, and unknown formats regardless of file extension.
pub fn detect_image_extension_from_magic(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        return Some("png");
    }
    if bytes.len() >= 3 && bytes[0] == 0xFF && bytes[1] == 0xD8 && bytes[2] == 0xFF {
        return Some("jpg");
    }
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Some("webp");
    }
    if bytes.len() >= 6 && (&bytes[0..6] == b"GIF87a" || &bytes[0..6] == b"GIF89a") {
        return Some("gif");
    }
    None
}

fn now_epoch_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

async fn read_and_validate_image(source_path: &Path) -> Result<(Vec<u8>, &'static str), DawnlandError> {
    if source_path.as_os_str().is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Source image path is empty",
        )
        .into());
    }

    let meta = tokio::fs::metadata(source_path).await?;
    if !meta.is_file() {
        return Err(DawnlandError::InvalidImageFormat(
            "Selected path is not a regular file".to_string(),
        ));
    }
    if meta.len() == 0 {
        return Err(DawnlandError::InvalidImageFormat(
            "Selected image file is empty".to_string(),
        ));
    }
    if meta.len() > MAX_BACKGROUND_BYTES {
        return Err(DawnlandError::ImageTooLarge {
            max_mb: MAX_BACKGROUND_MB,
        });
    }

    let file = tokio::fs::File::open(source_path).await?;
    let capacity = (meta.len() as usize).min(MAX_BACKGROUND_BYTES as usize);
    let mut bytes = Vec::with_capacity(capacity);
    let mut bounded_reader = file.take(MAX_BACKGROUND_BYTES + 1);
    bounded_reader.read_to_end(&mut bytes).await?;

    if bytes.is_empty() {
        return Err(DawnlandError::InvalidImageFormat(
            "Selected image file is empty".to_string(),
        ));
    }
    if bytes.len() as u64 > MAX_BACKGROUND_BYTES {
        return Err(DawnlandError::ImageTooLarge {
            max_mb: MAX_BACKGROUND_MB,
        });
    }

    let ext = detect_image_extension_from_magic(&bytes).ok_or_else(|| {
        DawnlandError::InvalidImageFormat(
            "Unsupported image format. Only PNG, JPEG, WebP, and GIF images are supported."
                .to_string(),
        )
    })?;

    Ok((bytes, ext))
}

/// Resolves the absolute path and size of a managed background image file inside `backgrounds_dir`.
///
/// Rejects unsafe filenames, traversal attempts, and unapproved extensions.
pub async fn resolve_managed_background_path(
    backgrounds_dir: &Path,
    image_file: Option<&str>,
) -> Option<(PathBuf, u64)> {
    let raw = image_file?.trim();
    if raw.is_empty() {
        return None;
    }
    if !is_safe_single_name(raw) {
        tracing::warn!(
            target: "dawnland_launcher_lib::background",
            "Rejected unsafe background filename '{}'",
            redact_user_paths(raw)
        );
        return None;
    }

    let has_valid_ext = Path::new(raw)
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .is_some_and(|ext| SUPPORTED_EXTENSIONS.contains(&ext.as_str()));
    if !has_valid_ext {
        tracing::warn!(
            target: "dawnland_launcher_lib::background",
            "Rejected background filename with unsupported extension '{}'",
            redact_user_paths(raw)
        );
        return None;
    }

    let candidate = backgrounds_dir.join(raw);
    match tokio::fs::metadata(&candidate).await {
        Ok(meta) if meta.is_file() => Some((candidate, meta.len())),
        Ok(_) => {
            tracing::warn!(
                target: "dawnland_launcher_lib::background",
                "Managed background path is not a file: {}",
                redact_user_paths(&candidate.display().to_string())
            );
            None
        }
        Err(err) => {
            tracing::warn!(
                target: "dawnland_launcher_lib::background",
                "Managed background image missing on disk ({}): {}",
                redact_user_paths(&candidate.display().to_string()),
                err
            );
            None
        }
    }
}

async fn import_background_image_to_dir(
    source_path: &Path,
    backgrounds_dir: &Path,
) -> Result<(String, PathBuf, u64), DawnlandError> {
    let (bytes, ext) = read_and_validate_image(source_path).await?;
    let file_size = bytes.len() as u64;

    tokio::fs::create_dir_all(backgrounds_dir).await?;
    let file_name = format!("{}{}.{}", BACKGROUND_FILE_PREFIX, uuid::Uuid::new_v4(), ext);
    let dest_path = backgrounds_dir.join(&file_name);

    write_file_atomic(&dest_path, &bytes)
        .await
        .map_err(|e| match e {
            FsError::Io(io_err) => DawnlandError::Io(io_err),
            other => DawnlandError::Unknown(other.to_string()),
        })?;

    Ok((file_name, dest_path, file_size))
}

async fn cleanup_old_background_files(backgrounds_dir: &Path, keep_file: Option<&str>) {
    let Ok(mut entries) = tokio::fs::read_dir(backgrounds_dir).await else {
        return;
    };

    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with(BACKGROUND_FILE_PREFIX) {
            continue;
        }
        if keep_file == Some(name.as_str()) {
            continue;
        }

        if let Ok(meta) = entry.metadata().await {
            if meta.is_file() {
                if let Err(err) = tokio::fs::remove_file(entry.path()).await {
                    tracing::warn!(
                        target: "dawnland_launcher_lib::background",
                        "Failed to remove old background file {}: {}",
                        redact_user_paths(&entry.path().display().to_string()),
                        err
                    );
                }
            }
        }
    }
}

async fn build_background_state(
    backgrounds_dir: &Path,
    bg: &BackgroundSettings,
) -> LauncherBackgroundState {
    let sanitized = bg.sanitized();
    let (image_path, file_size_bytes) = if sanitized.mode == BackgroundMode::Custom {
        match resolve_managed_background_path(backgrounds_dir, sanitized.image_file.as_deref()).await
        {
            Some((path, size)) => (Some(path.to_string_lossy().to_string()), Some(size)),
            None => (None, None),
        }
    } else {
        (None, None)
    };

    LauncherBackgroundState {
        mode: sanitized.mode,
        image_file: sanitized.image_file,
        image_path,
        file_size_bytes,
        blur_px: sanitized.blur_px,
        overlay_opacity: sanitized.overlay_opacity,
        fit: sanitized.fit,
        updated_at: sanitized.updated_at,
    }
}

#[tauri::command]
pub async fn get_launcher_background() -> Result<LauncherBackgroundState, AppError> {
    let settings = get_launcher_settings().await;
    let backgrounds_dir = get_backgrounds_dir();
    Ok(build_background_state(&backgrounds_dir, &settings.background).await)
}

#[tauri::command]
pub async fn set_launcher_background(
    app: tauri::AppHandle,
    source_path: String,
) -> Result<LauncherBackgroundState, AppError> {
    let _op_guard = BACKGROUND_OP_LOCK.lock().await;
    let backgrounds_dir = get_backgrounds_dir();

    let (file_name, dest_path, _file_size) =
        import_background_image_to_dir(Path::new(source_path.trim()), &backgrounds_dir).await?;

    let now_ms = now_epoch_millis();
    let updated_settings = match update_background_settings(|bg| {
        bg.mode = BackgroundMode::Custom;
        bg.image_file = Some(file_name.clone());
        bg.updated_at = now_ms;
    })
    .await
    {
        Ok(s) => s,
        Err(err) => {
            let _ = tokio::fs::remove_file(&dest_path).await;
            return Err(AppError::from(err));
        }
    };

    if let Err(err) = app
        .asset_protocol_scope()
        .allow_directory(&backgrounds_dir, false)
    {
        tracing::warn!(
            target: "dawnland_launcher_lib::background",
            "Failed to allow asset protocol scope for backgrounds directory {}: {}",
            redact_user_paths(&backgrounds_dir.display().to_string()),
            err
        );
    }

    cleanup_old_background_files(&backgrounds_dir, Some(&file_name)).await;

    tracing::info!(
        target: "dawnland_launcher_lib::background",
        "Custom launcher background updated (file: {})",
        file_name
    );

    Ok(build_background_state(&backgrounds_dir, &updated_settings.background).await)
}

#[tauri::command]
pub async fn update_launcher_background_style(
    blur_px: u8,
    overlay_opacity: Option<u8>,
    fit: BackgroundFit,
) -> Result<LauncherBackgroundState, AppError> {
    let clamped_blur = blur_px.min(MAX_BACKGROUND_BLUR_PX);
    let clamped_overlay = overlay_opacity.map(|value| value.min(MAX_BACKGROUND_OVERLAY_OPACITY));
    let now_ms = now_epoch_millis();

    let updated_settings = update_background_settings(|bg| {
        bg.blur_px = clamped_blur;
        bg.overlay_opacity = clamped_overlay;
        bg.fit = fit;
        bg.updated_at = now_ms;
    })
    .await?;

    let backgrounds_dir = get_backgrounds_dir();
    Ok(build_background_state(&backgrounds_dir, &updated_settings.background).await)
}

#[tauri::command]
pub async fn reset_launcher_background(
    reset_style: Option<bool>,
) -> Result<LauncherBackgroundState, AppError> {
    let _op_guard = BACKGROUND_OP_LOCK.lock().await;
    let should_reset_style = reset_style.unwrap_or(true);
    let now_ms = now_epoch_millis();

    let updated_settings = update_background_settings(|bg| {
        bg.mode = BackgroundMode::Default;
        bg.image_file = None;
        bg.updated_at = now_ms;
        if should_reset_style {
            bg.blur_px = DEFAULT_BACKGROUND_BLUR_PX;
            bg.overlay_opacity = None;
            bg.fit = BackgroundFit::Cover;
        }
    })
    .await?;

    let backgrounds_dir = get_backgrounds_dir();
    cleanup_old_background_files(&backgrounds_dir, None).await;

    tracing::info!(
        target: "dawnland_launcher_lib::background",
        "Launcher background reset to default (reset_style={})",
        should_reset_style
    );

    Ok(build_background_state(&backgrounds_dir, &updated_settings.background).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG_HEADER: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

    #[test]
    fn test_detect_image_extension_from_magic() {
        assert_eq!(detect_image_extension_from_magic(&PNG_HEADER), Some("png"));
        assert_eq!(
            detect_image_extension_from_magic(&[0xFF, 0xD8, 0xFF, 0xE0]),
            Some("jpg")
        );
        assert_eq!(
            detect_image_extension_from_magic(b"RIFF\x24\x00\x00\x00WEBPextra"),
            Some("webp")
        );
        assert_eq!(
            detect_image_extension_from_magic(b"GIF87a\x01\x00"),
            Some("gif")
        );
        assert_eq!(
            detect_image_extension_from_magic(b"GIF89a\x01\x00"),
            Some("gif")
        );

        assert_eq!(
            detect_image_extension_from_magic(b"<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>"),
            None
        );
        assert_eq!(detect_image_extension_from_magic(b"plain text"), None);
        assert_eq!(detect_image_extension_from_magic(b""), None);
        assert_eq!(detect_image_extension_from_magic(&[0xFF, 0xD8]), None);
    }

    #[tokio::test]
    async fn test_read_and_validate_image_accepts_valid_and_rejects_invalid() {
        let dir = tempfile::tempdir().expect("tempdir");

        // Valid PNG even if filename has uppercase extension
        let valid_png = dir.path().join("wallpaper.PNG");
        let mut png_bytes = PNG_HEADER.to_vec();
        png_bytes.extend_from_slice(b"payload");
        tokio::fs::write(&valid_png, &png_bytes)
            .await
            .expect("write valid png");
        let (bytes, ext) = read_and_validate_image(&valid_png)
            .await
            .expect("valid png accepted");
        assert_eq!(ext, "png");
        assert_eq!(bytes, png_bytes);

        // Spoofed extension (.png containing SVG/text)
        let spoofed = dir.path().join("fake.png");
        tokio::fs::write(&spoofed, b"<svg><script>alert(1)</script></svg>")
            .await
            .expect("write spoofed");
        let err = read_and_validate_image(&spoofed)
            .await
            .expect_err("spoofed png must fail");
        assert!(matches!(err, DawnlandError::InvalidImageFormat(_)));

        // Empty file
        let empty = dir.path().join("empty.png");
        tokio::fs::write(&empty, b"").await.expect("write empty");
        let err = read_and_validate_image(&empty)
            .await
            .expect_err("empty file must fail");
        assert!(matches!(err, DawnlandError::InvalidImageFormat(_)));

        // Directory instead of file
        let subdir = dir.path().join("subdir");
        tokio::fs::create_dir_all(&subdir)
            .await
            .expect("create subdir");
        let err = read_and_validate_image(&subdir)
            .await
            .expect_err("directory must fail");
        assert!(matches!(err, DawnlandError::InvalidImageFormat(_)));

        // Missing file
        let missing = dir.path().join("missing.png");
        let err = read_and_validate_image(&missing)
            .await
            .expect_err("missing file must fail");
        assert!(matches!(err, DawnlandError::Io(_)));

        // Oversized file (> 20 MiB)
        let oversized = dir.path().join("huge.png");
        let file = tokio::fs::File::create(&oversized)
            .await
            .expect("create huge file");
        file.set_len(MAX_BACKGROUND_BYTES + 1)
            .await
            .expect("set_len oversized");
        let err = read_and_validate_image(&oversized)
            .await
            .expect_err("oversized file must fail");
        assert!(matches!(
            err,
            DawnlandError::ImageTooLarge {
                max_mb: MAX_BACKGROUND_MB
            }
        ));
    }

    #[tokio::test]
    async fn test_resolve_managed_background_path_safety() {
        let dir = tempfile::tempdir().expect("tempdir");
        let valid_file = dir.path().join("bg-123.webp");
        tokio::fs::write(&valid_file, b"RIFF\x00\x00\x00\x00WEBP")
            .await
            .expect("write webp");

        let svg_file = dir.path().join("bg-bad.svg");
        tokio::fs::write(&svg_file, b"<svg/>")
            .await
            .expect("write svg");

        assert_eq!(
            resolve_managed_background_path(dir.path(), Some("bg-123.webp"))
                .await
                .map(|(p, _)| p),
            Some(valid_file)
        );
        assert_eq!(
            resolve_managed_background_path(dir.path(), Some("../bg-123.webp")).await,
            None
        );
        assert_eq!(
            resolve_managed_background_path(dir.path(), Some("sub/bg-123.webp")).await,
            None
        );
        assert_eq!(
            resolve_managed_background_path(dir.path(), Some("bg-bad.svg")).await,
            None
        );
        assert_eq!(
            resolve_managed_background_path(dir.path(), Some("missing.png")).await,
            None
        );
        assert_eq!(resolve_managed_background_path(dir.path(), None).await, None);
    }

    #[tokio::test]
    async fn test_import_and_cleanup_old_backgrounds() {
        let dir = tempfile::tempdir().expect("tempdir");
        let src_dir = dir.path().join("src");
        let managed_dir = dir.path().join("backgrounds");
        tokio::fs::create_dir_all(&src_dir)
            .await
            .expect("create src dir");

        let img1 = src_dir.join("first.png");
        let mut png_bytes = PNG_HEADER.to_vec();
        png_bytes.extend_from_slice(b"first");
        tokio::fs::write(&img1, &png_bytes)
            .await
            .expect("write img1");

        let img2 = src_dir.join("second.jpg");
        tokio::fs::write(&img2, &[0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10])
            .await
            .expect("write img2");

        // Import first image
        let (name1, path1, size1) = import_background_image_to_dir(&img1, &managed_dir)
            .await
            .expect("import img1");
        assert!(name1.starts_with("bg-") && name1.ends_with(".png"));
        assert!(path1.exists());
        assert_eq!(size1, png_bytes.len() as u64);

        // Place an unrelated non-bg file to confirm cleanup does not touch non-bg files
        let unrelated = managed_dir.join("notes.txt");
        tokio::fs::write(&unrelated, b"keep")
            .await
            .expect("write unrelated");

        // Import second image and clean up previous bg-*
        let (name2, path2, _) = import_background_image_to_dir(&img2, &managed_dir)
            .await
            .expect("import img2");
        assert!(name2.starts_with("bg-") && name2.ends_with(".jpg"));
        cleanup_old_background_files(&managed_dir, Some(&name2)).await;

        assert!(!path1.exists());
        assert!(path2.exists());
        assert!(unrelated.exists());

        // Build state verifies Custom resolves path2 while Default returns None
        let custom_cfg = BackgroundSettings {
            mode: BackgroundMode::Custom,
            image_file: Some(name2.clone()),
            blur_px: 10,
            overlay_opacity: Some(40),
            fit: BackgroundFit::Contain,
            updated_at: 99,
        };
        let state = build_background_state(&managed_dir, &custom_cfg).await;
        assert_eq!(state.image_path, Some(path2.to_string_lossy().to_string()));
        assert_eq!(state.blur_px, 10);
        assert_eq!(state.overlay_opacity, Some(40));
        assert_eq!(state.fit, BackgroundFit::Contain);

        // Reset cleanup deletes remaining bg-* files
        cleanup_old_background_files(&managed_dir, None).await;
        assert!(!path2.exists());
        assert!(unrelated.exists());
    }
}
