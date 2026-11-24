//! Launcher Custom Background Manager
//!
//! Obtains background images through a native system file dialog (never from arbitrary
//! renderer-supplied paths), validates and decodes the image payload in a blocking worker
//! with strict dimension/memory/animation limits, stores assets and `background.json`
//! under the writable per-user application data directory, and coordinates concurrent
//! style updates and resets.

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::LazyLock;
use std::time::{SystemTime, UNIX_EPOCH};

use image::codecs::gif::GifDecoder;
use image::codecs::jpeg::JpegDecoder;
use image::codecs::png::PngDecoder;
use image::codecs::webp::WebPDecoder;
use image::{AnimationDecoder, DynamicImage, ImageDecoder, ImageError, Limits};
use serde::{Deserialize, Serialize};
use tauri::Manager;
use tauri_plugin_dialog::DialogExt;
use tokio::io::AsyncReadExt;

use crate::core::fs_utils::{is_safe_single_name, write_file_atomic, FsError};
use crate::error::{AppError, DawnlandError};
use crate::logger::redact::redact_user_paths;

pub const DEFAULT_BACKGROUND_BLUR_PX: u8 = 4;
pub const MAX_BACKGROUND_BLUR_PX: u8 = 24;
pub const MAX_BACKGROUND_OVERLAY_OPACITY: u8 = 85;

pub const MAX_BACKGROUND_MB: u64 = 20;
pub const MAX_BACKGROUND_BYTES: u64 = MAX_BACKGROUND_MB * 1024 * 1024;
pub const MAX_BACKGROUND_DIMENSION: u32 = 8192;
pub const MAX_BACKGROUND_PIXELS: u64 = 36_000_000;
pub const MAX_ANIMATION_FRAMES: usize = 300;
pub const MAX_ANIMATION_TOTAL_PIXELS: u64 = 256_000_000;
const MAX_DECODE_ALLOC_BYTES: u64 = 256 * 1024 * 1024;

const BACKGROUND_FILE_PREFIX: &str = "bg-";
const SUPPORTED_EXTENSIONS: [&str; 4] = ["png", "jpg", "webp", "gif"];
const BACKGROUND_CONFIG_FILENAME: &str = "background.json";
const BACKGROUNDS_DIR_NAME: &str = "backgrounds";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum BackgroundMode {
    #[default]
    Default,
    Custom,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum BackgroundFit {
    #[default]
    Cover,
    Contain,
    Fill,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundSettings {
    #[serde(default)]
    pub mode: BackgroundMode,
    #[serde(default)]
    pub image_file: Option<String>,
    #[serde(default = "default_background_blur_px")]
    pub blur_px: u8,
    #[serde(default)]
    pub overlay_opacity: Option<u8>,
    #[serde(default)]
    pub fit: BackgroundFit,
    #[serde(default)]
    pub updated_at: u64,
}

fn default_background_blur_px() -> u8 {
    DEFAULT_BACKGROUND_BLUR_PX
}

impl Default for BackgroundSettings {
    fn default() -> Self {
        Self {
            mode: BackgroundMode::Default,
            image_file: None,
            blur_px: default_background_blur_px(),
            overlay_opacity: None,
            fit: BackgroundFit::Cover,
            updated_at: 0,
        }
    }
}

impl BackgroundSettings {
    /// Returns a copy with blur and overlay values clamped to valid UI ranges.
    pub fn sanitized(&self) -> Self {
        let mut copy = self.clone();
        copy.blur_px = copy.blur_px.min(MAX_BACKGROUND_BLUR_PX);
        copy.overlay_opacity = copy
            .overlay_opacity
            .map(|value| value.min(MAX_BACKGROUND_OVERLAY_OPACITY));
        copy
    }
}

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
    pub revision: u64,
    pub reset_epoch: u64,
}

/// Monotonic counters coordinating imports, style updates, and resets across IPC calls.
struct BackgroundCoordination {
    revision: AtomicU64,
    reset_epoch: AtomicU64,
}

impl BackgroundCoordination {
    const fn new() -> Self {
        Self {
            revision: AtomicU64::new(0),
            reset_epoch: AtomicU64::new(0),
        }
    }

    fn current(&self) -> (u64, u64) {
        (
            self.revision.load(Ordering::Acquire),
            self.reset_epoch.load(Ordering::Acquire),
        )
    }

    fn reset_epoch(&self) -> u64 {
        self.reset_epoch.load(Ordering::Acquire)
    }

    fn next_revision(&self) -> (u64, u64) {
        let rev = self.revision.fetch_add(1, Ordering::AcqRel) + 1;
        let epoch = self.reset_epoch.load(Ordering::Acquire);
        (rev, epoch)
    }

    fn advance_reset_epoch(&self) -> (u64, u64) {
        let rev = self.revision.fetch_add(1, Ordering::AcqRel) + 1;
        let epoch = self.reset_epoch.fetch_add(1, Ordering::AcqRel) + 1;
        (rev, epoch)
    }
}

static GLOBAL_COORDINATION: BackgroundCoordination = BackgroundCoordination::new();

/// Serializes all background config and file operations so concurrent commands cannot reorder.
static BACKGROUND_OP_LOCK: LazyLock<tokio::sync::Mutex<()>> =
    LazyLock::new(|| tokio::sync::Mutex::new(()));

/// Prevents opening multiple overlapping native background pickers.
static PICKER_OPEN: AtomicBool = AtomicBool::new(false);

struct PickerOpenGuard;

impl Drop for PickerOpenGuard {
    fn drop(&mut self) {
        PICKER_OPEN.store(false, Ordering::Release);
    }
}

/// Resolves the writable per-user application data directory used to store backgrounds
/// and `background.json` (safe on Windows, macOS, and read-only Linux AppImage bundles).
pub fn get_background_storage_root(app: &tauri::AppHandle) -> Result<PathBuf, DawnlandError> {
    if let Ok(dir) = app.path().app_data_dir() {
        return Ok(dir);
    }
    let identifier = app.config().identifier.as_str();
    if !identifier.is_empty() {
        if let Some(base) = dirs::data_dir() {
            return Ok(base.join(identifier));
        }
    }
    Err(DawnlandError::Unknown(
        "Failed to resolve writable user application data directory for backgrounds".to_string(),
    ))
}

pub fn backgrounds_dir_from_root(storage_root: &Path) -> PathBuf {
    storage_root.join(BACKGROUNDS_DIR_NAME)
}

pub fn background_config_path_from_root(storage_root: &Path) -> PathBuf {
    storage_root.join(BACKGROUND_CONFIG_FILENAME)
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

fn validate_non_empty_source_path(source_path: &Path) -> Result<(), DawnlandError> {
    if source_path.as_os_str().is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Source image path is empty",
        )
        .into());
    }
    if let Some(as_str) = source_path.to_str() {
        if as_str.trim().is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Source image path is empty",
            )
            .into());
        }
    }
    Ok(())
}

fn map_image_error(err: ImageError, format_name: &str) -> DawnlandError {
    match err {
        ImageError::Limits(_) => DawnlandError::ImageDimensionsTooLarge(format!(
            "{format_name} image exceeds allowed dimensions or memory budget"
        )),
        other => DawnlandError::InvalidImageFormat(format!(
            "Invalid or corrupted {format_name} image: {other}"
        )),
    }
}

fn validate_dimensions(width: u32, height: u32, format_name: &str) -> Result<(), DawnlandError> {
    if width == 0 || height == 0 {
        return Err(DawnlandError::InvalidImageFormat(format!(
            "{format_name} image has zero width or height"
        )));
    }
    let pixels = u64::from(width).saturating_mul(u64::from(height));
    if width > MAX_BACKGROUND_DIMENSION
        || height > MAX_BACKGROUND_DIMENSION
        || pixels > MAX_BACKGROUND_PIXELS
    {
        return Err(DawnlandError::ImageDimensionsTooLarge(format!(
            "{format_name} image dimensions ({width}x{height}) exceed allowed limits"
        )));
    }
    Ok(())
}

fn validate_animation_frames<'a, D>(decoder: D, format_name: &str) -> Result<(), DawnlandError>
where
    D: AnimationDecoder<'a>,
{
    let mut frame_count: usize = 0;
    let mut total_pixels: u64 = 0;

    for frame_res in decoder.into_frames() {
        if frame_count >= MAX_ANIMATION_FRAMES {
            return Err(DawnlandError::ImageDimensionsTooLarge(format!(
                "{format_name} animation exceeds maximum allowed {MAX_ANIMATION_FRAMES} frames"
            )));
        }
        let frame = frame_res.map_err(|e| map_image_error(e, format_name))?;
        let (fw, fh) = frame.buffer().dimensions();
        validate_dimensions(fw, fh, format_name)?;
        let frame_pixels = u64::from(fw).saturating_mul(u64::from(fh));
        total_pixels = total_pixels.saturating_add(frame_pixels);
        if total_pixels > MAX_ANIMATION_TOTAL_PIXELS {
            return Err(DawnlandError::ImageDimensionsTooLarge(format!(
                "{format_name} animation exceeds maximum total decoded pixels"
            )));
        }
        frame_count += 1;
    }

    if frame_count == 0 {
        return Err(DawnlandError::InvalidImageFormat(format!(
            "{format_name} animation contains no frames"
        )));
    }

    Ok(())
}

fn decode_limits() -> Limits {
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_BACKGROUND_DIMENSION);
    limits.max_image_height = Some(MAX_BACKGROUND_DIMENSION);
    limits.max_alloc = Some(MAX_DECODE_ALLOC_BYTES);
    limits
}

/// Validates format magic bytes, dimensions, animation bounds, and full pixel decodability.
pub fn validate_and_decode_image_bytes(bytes: &[u8]) -> Result<&'static str, DawnlandError> {
    let ext = detect_image_extension_from_magic(bytes).ok_or_else(|| {
        DawnlandError::InvalidImageFormat(
            "Unsupported image format. Only PNG, JPEG, WebP, and GIF images are supported."
                .to_string(),
        )
    })?;

    let limits = decode_limits();

    match ext {
        "png" => {
            let decoder = PngDecoder::with_limits(Cursor::new(bytes), limits)
                .map_err(|e| map_image_error(e, "PNG"))?;
            let (w, h) = decoder.dimensions();
            validate_dimensions(w, h, "PNG")?;
            if decoder.is_apng().map_err(|e| map_image_error(e, "PNG"))? {
                let apng = decoder.apng().map_err(|e| map_image_error(e, "PNG"))?;
                validate_animation_frames(apng, "APNG")?;
            } else {
                DynamicImage::from_decoder(decoder).map_err(|e| map_image_error(e, "PNG"))?;
            }
        }
        "jpg" => {
            let mut decoder =
                JpegDecoder::new(Cursor::new(bytes)).map_err(|e| map_image_error(e, "JPEG"))?;
            let (w, h) = decoder.dimensions();
            validate_dimensions(w, h, "JPEG")?;
            decoder
                .set_limits(limits)
                .map_err(|e| map_image_error(e, "JPEG"))?;
            DynamicImage::from_decoder(decoder).map_err(|e| map_image_error(e, "JPEG"))?;
        }
        "webp" => {
            let mut decoder =
                WebPDecoder::new(Cursor::new(bytes)).map_err(|e| map_image_error(e, "WebP"))?;
            let (w, h) = decoder.dimensions();
            validate_dimensions(w, h, "WebP")?;
            decoder
                .set_limits(limits)
                .map_err(|e| map_image_error(e, "WebP"))?;
            if decoder.has_animation() {
                validate_animation_frames(decoder, "WebP")?;
            } else {
                DynamicImage::from_decoder(decoder).map_err(|e| map_image_error(e, "WebP"))?;
            }
        }
        "gif" => {
            let mut decoder =
                GifDecoder::new(Cursor::new(bytes)).map_err(|e| map_image_error(e, "GIF"))?;
            let (w, h) = decoder.dimensions();
            validate_dimensions(w, h, "GIF")?;
            decoder
                .set_limits(limits)
                .map_err(|e| map_image_error(e, "GIF"))?;
            validate_animation_frames(decoder, "GIF")?;
        }
        _ => {
            return Err(DawnlandError::InvalidImageFormat(
                "Unsupported image format".to_string(),
            ));
        }
    }

    Ok(ext)
}

async fn read_and_validate_image(source_path: &Path) -> Result<(Vec<u8>, &'static str), DawnlandError> {
    validate_non_empty_source_path(source_path)?;

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

    let (bytes, ext) = tokio::task::spawn_blocking(move || {
        let ext = validate_and_decode_image_bytes(&bytes)?;
        Ok::<_, DawnlandError>((bytes, ext))
    })
    .await
    .map_err(|e| DawnlandError::Unknown(format!("Image decode task failed: {e}")))??;

    Ok((bytes, ext))
}

async fn read_background_settings_strict(
    config_path: &Path,
) -> Result<BackgroundSettings, DawnlandError> {
    match tokio::fs::read_to_string(config_path).await {
        Ok(content) => {
            let parsed = serde_json::from_str::<BackgroundSettings>(&content)?;
            Ok(parsed.sanitized())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(BackgroundSettings::default()),
        Err(e) => Err(DawnlandError::Io(e)),
    }
}

async fn write_background_settings_atomic(
    config_path: &Path,
    settings: &BackgroundSettings,
) -> Result<(), DawnlandError> {
    let sanitized = settings.sanitized();
    let content = serde_json::to_string_pretty(&sanitized)?;
    write_file_atomic(config_path, content.as_bytes())
        .await
        .map_err(|e| match e {
            FsError::Io(io_err) => DawnlandError::Io(io_err),
            other => DawnlandError::Unknown(other.to_string()),
        })
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
    revision: u64,
    reset_epoch: u64,
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
        revision,
        reset_epoch,
    }
}

async fn load_background_state_in_dir(
    storage_root: &Path,
    coordination: &BackgroundCoordination,
) -> LauncherBackgroundState {
    let backgrounds_dir = backgrounds_dir_from_root(storage_root);
    let config_path = background_config_path_from_root(storage_root);
    let (revision, reset_epoch) = coordination.current();

    match read_background_settings_strict(&config_path).await {
        Ok(settings) => build_background_state(&backgrounds_dir, &settings, revision, reset_epoch).await,
        Err(err) => {
            tracing::warn!(
                target: "dawnland_launcher_lib::background",
                "Failed to load background config {}; falling back in-memory without overwriting disk: {}",
                redact_user_paths(&config_path.display().to_string()),
                err
            );
            build_background_state(
                &backgrounds_dir,
                &BackgroundSettings::default(),
                revision,
                reset_epoch,
            )
            .await
        }
    }
}

async fn import_and_commit_background_in_dir<F>(
    storage_root: &Path,
    source_path: &Path,
    coordination: &BackgroundCoordination,
    authorize_dir: F,
) -> Result<LauncherBackgroundState, DawnlandError>
where
    F: FnOnce(&Path) -> Result<(), String>,
{
    let backgrounds_dir = backgrounds_dir_from_root(storage_root);
    let config_path = background_config_path_from_root(storage_root);

    let (bytes, ext) = read_and_validate_image(source_path).await?;
    tokio::fs::create_dir_all(&backgrounds_dir).await?;

    let file_name = format!("{}{}.{}", BACKGROUND_FILE_PREFIX, uuid::Uuid::new_v4(), ext);
    let dest_path = backgrounds_dir.join(&file_name);

    write_file_atomic(&dest_path, &bytes)
        .await
        .map_err(|e| match e {
            FsError::Io(io_err) => DawnlandError::Io(io_err),
            other => DawnlandError::Unknown(other.to_string()),
        })?;

    if let Err(err) = authorize_dir(&backgrounds_dir) {
        let _ = tokio::fs::remove_file(&dest_path).await;
        return Err(DawnlandError::Unknown(format!(
            "Failed to authorize background asset access: {err}"
        )));
    }

    let mut settings = match read_background_settings_strict(&config_path).await {
        Ok(s) => s,
        Err(err) => {
            let _ = tokio::fs::remove_file(&dest_path).await;
            return Err(err);
        }
    };

    settings.mode = BackgroundMode::Custom;
    settings.image_file = Some(file_name.clone());
    settings.updated_at = now_epoch_millis();

    if let Err(err) = write_background_settings_atomic(&config_path, &settings).await {
        let _ = tokio::fs::remove_file(&dest_path).await;
        return Err(err);
    }

    let (revision, reset_epoch) = coordination.next_revision();
    cleanup_old_background_files(&backgrounds_dir, Some(&file_name)).await;

    tracing::info!(
        target: "dawnland_launcher_lib::background",
        "Custom launcher background updated (file: {})",
        file_name
    );

    Ok(build_background_state(&backgrounds_dir, &settings, revision, reset_epoch).await)
}

async fn update_background_style_in_dir(
    storage_root: &Path,
    blur_px: u8,
    overlay_opacity: Option<u8>,
    fit: BackgroundFit,
    expected_reset_epoch: Option<u64>,
    coordination: &BackgroundCoordination,
) -> Result<LauncherBackgroundState, DawnlandError> {
    let backgrounds_dir = backgrounds_dir_from_root(storage_root);
    let config_path = background_config_path_from_root(storage_root);

    if let Some(expected) = expected_reset_epoch {
        if expected != coordination.reset_epoch() {
            let (revision, reset_epoch) = coordination.current();
            let current = read_background_settings_strict(&config_path).await?;
            return Ok(build_background_state(&backgrounds_dir, &current, revision, reset_epoch).await);
        }
    }

    let mut settings = read_background_settings_strict(&config_path).await?;
    settings.blur_px = blur_px.min(MAX_BACKGROUND_BLUR_PX);
    settings.overlay_opacity = overlay_opacity.map(|value| value.min(MAX_BACKGROUND_OVERLAY_OPACITY));
    settings.fit = fit;
    settings.updated_at = now_epoch_millis();

    write_background_settings_atomic(&config_path, &settings).await?;
    let (revision, reset_epoch) = coordination.next_revision();

    Ok(build_background_state(&backgrounds_dir, &settings, revision, reset_epoch).await)
}

async fn reset_background_in_dir(
    storage_root: &Path,
    reset_style: bool,
    coordination: &BackgroundCoordination,
) -> Result<LauncherBackgroundState, DawnlandError> {
    let backgrounds_dir = backgrounds_dir_from_root(storage_root);
    let config_path = background_config_path_from_root(storage_root);
    let now_ms = now_epoch_millis();

    let mut settings = if reset_style {
        BackgroundSettings::default()
    } else {
        read_background_settings_strict(&config_path).await?
    };
    settings.mode = BackgroundMode::Default;
    settings.image_file = None;
    settings.updated_at = now_ms;

    write_background_settings_atomic(&config_path, &settings).await?;
    let (revision, reset_epoch) = coordination.advance_reset_epoch();
    cleanup_old_background_files(&backgrounds_dir, None).await;

    tracing::info!(
        target: "dawnland_launcher_lib::background",
        "Launcher background reset to default (reset_style={})",
        reset_style
    );

    Ok(build_background_state(&backgrounds_dir, &settings, revision, reset_epoch).await)
}

#[tauri::command]
pub async fn get_launcher_background(app: tauri::AppHandle) -> Result<LauncherBackgroundState, AppError> {
    let _op_guard = BACKGROUND_OP_LOCK.lock().await;
    let storage_root = get_background_storage_root(&app)?;
    Ok(load_background_state_in_dir(&storage_root, &GLOBAL_COORDINATION).await)
}

#[tauri::command]
pub async fn pick_and_set_launcher_background(
    app: tauri::AppHandle,
    dialog_title: Option<String>,
    filter_name: Option<String>,
) -> Result<Option<LauncherBackgroundState>, AppError> {
    if PICKER_OPEN
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Ok(None);
    }
    let _picker_guard = PickerOpenGuard;

    let filter_label = filter_name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("Images");

    let (tx, rx) = tokio::sync::oneshot::channel();
    let mut dialog = app
        .dialog()
        .file()
        .add_filter(filter_label, &["png", "jpg", "jpeg", "webp", "gif"]);

    if let Some(title) = dialog_title
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        dialog = dialog.set_title(title);
    }

    if let Some(window) = app.get_webview_window("main") {
        dialog = dialog.set_parent(&window);
    }

    dialog.pick_file(move |file_path| {
        let _ = tx.send(file_path);
    });

    let picked = rx.await.map_err(|_| {
        DawnlandError::Unknown("File picker channel closed unexpectedly".to_string())
    })?;
    let Some(file_path) = picked else {
        return Ok(None);
    };

    let source_path = file_path.into_path().map_err(|e| {
        DawnlandError::InvalidImageFormat(format!("Selected file path is not accessible: {e}"))
    })?;

    let _op_guard = BACKGROUND_OP_LOCK.lock().await;
    let storage_root = get_background_storage_root(&app)?;
    let state = import_and_commit_background_in_dir(
        &storage_root,
        &source_path,
        &GLOBAL_COORDINATION,
        |dir| {
            app.asset_protocol_scope()
                .allow_directory(dir, false)
                .map_err(|e| e.to_string())
        },
    )
    .await?;

    Ok(Some(state))
}

#[tauri::command]
pub async fn update_launcher_background_style(
    app: tauri::AppHandle,
    blur_px: u8,
    overlay_opacity: Option<u8>,
    fit: BackgroundFit,
    expected_reset_epoch: Option<u64>,
) -> Result<LauncherBackgroundState, AppError> {
    let _op_guard = BACKGROUND_OP_LOCK.lock().await;
    let storage_root = get_background_storage_root(&app)?;
    let state = update_background_style_in_dir(
        &storage_root,
        blur_px,
        overlay_opacity,
        fit,
        expected_reset_epoch,
        &GLOBAL_COORDINATION,
    )
    .await?;
    Ok(state)
}

#[tauri::command]
pub async fn reset_launcher_background(
    app: tauri::AppHandle,
    reset_style: Option<bool>,
) -> Result<LauncherBackgroundState, AppError> {
    let _op_guard = BACKGROUND_OP_LOCK.lock().await;
    let storage_root = get_background_storage_root(&app)?;
    let should_reset_style = reset_style.unwrap_or(true);
    let state =
        reset_background_in_dir(&storage_root, should_reset_style, &GLOBAL_COORDINATION).await?;
    Ok(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::codecs::gif::GifEncoder;
    use image::codecs::jpeg::JpegEncoder;
    use image::codecs::png::PngEncoder;
    use image::codecs::webp::WebPEncoder;
    use image::{ColorType, Delay, Frame, ImageEncoder, Rgba, RgbaImage};

    const PNG_HEADER: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

    fn make_valid_png_bytes(width: u32, height: u32) -> Vec<u8> {
        let img = RgbaImage::from_pixel(width, height, Rgba([20, 120, 220, 255]));
        let mut out = Vec::new();
        PngEncoder::new(&mut out)
            .write_image(img.as_raw(), width, height, ColorType::Rgba8.into())
            .expect("encode png");
        out
    }

    fn make_valid_jpeg_bytes(width: u32, height: u32) -> Vec<u8> {
        let img = image::RgbImage::from_pixel(width, height, image::Rgb([220, 120, 40]));
        let mut out = Vec::new();
        JpegEncoder::new_with_quality(&mut out, 85)
            .write_image(img.as_raw(), width, height, ColorType::Rgb8.into())
            .expect("encode jpeg");
        out
    }

    fn make_valid_webp_bytes(width: u32, height: u32) -> Vec<u8> {
        let img = RgbaImage::from_pixel(width, height, Rgba([80, 200, 120, 255]));
        let mut out = Vec::new();
        WebPEncoder::new_lossless(&mut out)
            .write_image(img.as_raw(), width, height, ColorType::Rgba8.into())
            .expect("encode webp");
        out
    }

    fn make_gif_bytes(frame_count: usize) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut encoder = GifEncoder::new(&mut out);
            let frames = (0..frame_count).map(|i| {
                let c = (i % 255) as u8;
                let buf = RgbaImage::from_pixel(2, 2, Rgba([c, 100, 150, 255]));
                Frame::from_parts(buf, 0, 0, Delay::from_numer_denom_ms(50, 1))
            });
            encoder.encode_frames(frames).expect("encode gif frames");
        }
        out
    }

    fn crc32_ieee(data: &[u8]) -> u32 {
        let mut crc: u32 = 0xFFFF_FFFF;
        for &byte in data {
            crc ^= u32::from(byte);
            for _ in 0..8 {
                let mask = (crc & 1).wrapping_neg();
                crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
            }
        }
        !crc
    }

    fn make_png_with_spoofed_ihdr_dimensions(width: u32, height: u32) -> Vec<u8> {
        let mut png = make_valid_png_bytes(2, 2);
        // PNG structure: 8-byte signature, 4-byte length (13), 4-byte "IHDR", then 4-byte width & 4-byte height.
        png[16..20].copy_from_slice(&width.to_be_bytes());
        png[20..24].copy_from_slice(&height.to_be_bytes());
        let crc = crc32_ieee(&png[12..29]);
        png[29..33].copy_from_slice(&crc.to_be_bytes());
        png
    }

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
    async fn test_read_and_validate_image_decodes_valid_and_rejects_corrupt_or_oversized() {
        let dir = tempfile::tempdir().expect("tempdir");

        // Valid PNG / JPEG / WebP / GIF
        for (name, data, expected_ext) in [
            ("wall.PNG", make_valid_png_bytes(4, 4), "png"),
            ("wall.jpg", make_valid_jpeg_bytes(4, 4), "jpg"),
            ("wall.webp", make_valid_webp_bytes(4, 4), "webp"),
            ("anim.gif", make_gif_bytes(3), "gif"),
        ] {
            let p = dir.path().join(name);
            tokio::fs::write(&p, &data).await.expect("write valid");
            let (read_bytes, ext) = read_and_validate_image(&p)
                .await
                .unwrap_or_else(|e| panic!("expected {name} to succeed, got {e}"));
            assert_eq!(ext, expected_ext);
            assert_eq!(read_bytes, data);
        }

        // Corrupted/truncated image with valid PNG signature must fail full decode
        let truncated = dir.path().join("truncated.png");
        let mut bad_png = PNG_HEADER.to_vec();
        bad_png.extend_from_slice(b"not-a-valid-ihdr-or-idat-stream");
        tokio::fs::write(&truncated, &bad_png)
            .await
            .expect("write truncated png");
        let err = read_and_validate_image(&truncated)
            .await
            .expect_err("truncated png must fail decode");
        assert!(matches!(err, DawnlandError::InvalidImageFormat(_)));

        // Valid PNG truncated halfway through pixel data must also fail
        let valid_png = make_valid_png_bytes(16, 16);
        let half_png = dir.path().join("half.png");
        tokio::fs::write(&half_png, &valid_png[..valid_png.len() / 2])
            .await
            .expect("write half png");
        let err = read_and_validate_image(&half_png)
            .await
            .expect_err("half-truncated png must fail decode");
        assert!(matches!(err, DawnlandError::InvalidImageFormat(_)));

        // Excessive dimensions (> 8192) must fail with ImageDimensionsTooLarge
        let huge_dims = dir.path().join("huge_dims.png");
        tokio::fs::write(&huge_dims, &make_png_with_spoofed_ihdr_dimensions(9000, 100))
            .await
            .expect("write huge dims png");
        let err = read_and_validate_image(&huge_dims)
            .await
            .expect_err("excessive dimensions must fail");
        assert!(matches!(err, DawnlandError::ImageDimensionsTooLarge(_)));

        // Excessive pixel count within max single axis (7000 x 7000 = 49M > 36M)
        let huge_pixels = dir.path().join("huge_pixels.png");
        tokio::fs::write(&huge_pixels, &make_png_with_spoofed_ihdr_dimensions(7000, 7000))
            .await
            .expect("write huge pixels png");
        let err = read_and_validate_image(&huge_pixels)
            .await
            .expect_err("excessive pixel count must fail");
        assert!(matches!(err, DawnlandError::ImageDimensionsTooLarge(_)));

        // Excessive animation frame count (> 300 frames)
        let huge_gif = dir.path().join("too_many_frames.gif");
        tokio::fs::write(&huge_gif, &make_gif_bytes(MAX_ANIMATION_FRAMES + 1))
            .await
            .expect("write oversized animation gif");
        let err = read_and_validate_image(&huge_gif)
            .await
            .expect_err("excessive animation frames must fail");
        assert!(matches!(err, DawnlandError::ImageDimensionsTooLarge(_)));

        // Spoofed SVG disguised as PNG
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

    #[test]
    fn test_validate_non_empty_source_path_preserves_trailing_whitespace() {
        assert!(validate_non_empty_source_path(Path::new("")).is_err());
        assert!(validate_non_empty_source_path(Path::new("   \t  ")).is_err());
        assert!(validate_non_empty_source_path(Path::new("/tmp/wallpaper.png ")).is_ok());
    }

    #[tokio::test]
    async fn test_resolve_managed_background_path_safety() {
        let dir = tempfile::tempdir().expect("tempdir");
        let valid_file = dir.path().join("bg-123.webp");
        tokio::fs::write(&valid_file, &make_valid_webp_bytes(2, 2))
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
    async fn test_command_orchestration_persists_reloads_and_cleans_up() {
        let dir = tempfile::tempdir().expect("tempdir");
        let src_dir = dir.path().join("src");
        let storage_root = dir.path().join("app_data");
        tokio::fs::create_dir_all(&src_dir)
            .await
            .expect("create src dir");

        let coord = BackgroundCoordination::new();

        let img1 = src_dir.join("first.png");
        tokio::fs::write(&img1, &make_valid_png_bytes(4, 4))
            .await
            .expect("write img1");

        let img2 = src_dir.join("second.jpg");
        tokio::fs::write(&img2, &make_valid_jpeg_bytes(4, 4))
            .await
            .expect("write img2");

        // 1. Import first image through full orchestration
        let state1 = import_and_commit_background_in_dir(&storage_root, &img1, &coord, |_| Ok(()))
            .await
            .expect("import img1");
        let file1 = state1.image_file.clone().expect("file1 set");
        let path1 = PathBuf::from(state1.image_path.clone().expect("path1 set"));
        assert!(path1.exists());
        assert_eq!(state1.revision, 1);

        // Reload fresh from disk to assert background.json actually persisted
        let config_path = background_config_path_from_root(&storage_root);
        let reloaded1 = read_background_settings_strict(&config_path)
            .await
            .expect("reload config1");
        assert_eq!(reloaded1.mode, BackgroundMode::Custom);
        assert_eq!(reloaded1.image_file.as_deref(), Some(file1.as_str()));

        // Place an unrelated file in backgrounds dir to confirm cleanup ignores it
        let backgrounds_dir = backgrounds_dir_from_root(&storage_root);
        let unrelated = backgrounds_dir.join("notes.txt");
        tokio::fs::write(&unrelated, b"keep")
            .await
            .expect("write unrelated");

        // 2. Import second image via orchestration -> automatically cleans up first bg-* file
        let state2 = import_and_commit_background_in_dir(&storage_root, &img2, &coord, |_| Ok(()))
            .await
            .expect("import img2");
        let file2 = state2.image_file.clone().expect("file2 set");
        let path2 = PathBuf::from(state2.image_path.clone().expect("path2 set"));
        assert_ne!(file1, file2);
        assert!(!path1.exists(), "superseded bg-* file must be deleted by orchestration");
        assert!(path2.exists());
        assert!(unrelated.exists());
        assert_eq!(state2.revision, 2);

        // 3. Asset scope authorization failure rolls back newly copied file without losing active config
        let img3 = src_dir.join("third.webp");
        tokio::fs::write(&img3, &make_valid_webp_bytes(4, 4))
            .await
            .expect("write img3");
        let err = import_and_commit_background_in_dir(&storage_root, &img3, &coord, |_| {
            Err("scope denied".to_string())
        })
        .await
        .expect_err("scope denial must fail import");
        assert!(err.to_string().contains("scope denied"));
        assert!(path2.exists(), "previous custom background must stay intact");
        let mut bg_files = 0;
        let mut entries = tokio::fs::read_dir(&backgrounds_dir).await.unwrap();
        while let Some(entry) = entries.next_entry().await.unwrap() {
            if entry.file_name().to_string_lossy().starts_with(BACKGROUND_FILE_PREFIX) {
                bg_files += 1;
            }
        }
        assert_eq!(bg_files, 1, "failed import file must be rolled back");
        let reloaded2 = read_background_settings_strict(&config_path)
            .await
            .expect("reload config2");
        assert_eq!(reloaded2.image_file.as_deref(), Some(file2.as_str()));

        // 4. Reset via orchestration cleans up bg-* and persists Default state on disk
        let reset_state = reset_background_in_dir(&storage_root, true, &coord)
            .await
            .expect("reset background");
        assert_eq!(reset_state.mode, BackgroundMode::Default);
        assert_eq!(reset_state.image_file, None);
        assert_eq!(reset_state.reset_epoch, 1);
        assert!(!path2.exists());
        assert!(unrelated.exists());

        let reloaded_after_reset = read_background_settings_strict(&config_path)
            .await
            .expect("reload config after reset");
        assert_eq!(reloaded_after_reset.mode, BackgroundMode::Default);
        assert_eq!(reloaded_after_reset.image_file, None);
    }

    #[tokio::test]
    async fn test_strict_config_and_stale_style_after_reset() {
        let dir = tempfile::tempdir().expect("tempdir");
        let storage_root = dir.path().join("app_data");
        tokio::fs::create_dir_all(&storage_root)
            .await
            .expect("create root");
        let config_path = background_config_path_from_root(&storage_root);
        let coord = BackgroundCoordination::new();

        // Style update with matching reset_epoch succeeds and persists to disk
        let styled = update_background_style_in_dir(
            &storage_root,
            16,
            Some(55),
            BackgroundFit::Contain,
            Some(0),
            &coord,
        )
        .await
        .expect("style update succeeds");
        assert_eq!(styled.blur_px, 16);
        assert_eq!(styled.overlay_opacity, Some(55));
        assert_eq!(styled.fit, BackgroundFit::Contain);

        // Reset bumps reset_epoch to 1 and restores defaults
        let reset = reset_background_in_dir(&storage_root, true, &coord)
            .await
            .expect("reset succeeds");
        assert_eq!(reset.reset_epoch, 1);
        assert_eq!(reset.blur_px, DEFAULT_BACKGROUND_BLUR_PX);

        // Stale style update arriving with old reset_epoch=0 must NOT overwrite disk
        let stale_resp = update_background_style_in_dir(
            &storage_root,
            22,
            Some(80),
            BackgroundFit::Fill,
            Some(0),
            &coord,
        )
        .await
        .expect("stale style returns current state without failing");
        assert_eq!(stale_resp.blur_px, DEFAULT_BACKGROUND_BLUR_PX);
        assert_eq!(stale_resp.overlay_opacity, None);
        assert_eq!(stale_resp.fit, BackgroundFit::Cover);

        let on_disk = read_background_settings_strict(&config_path)
            .await
            .expect("read config from disk");
        assert_eq!(on_disk.blur_px, DEFAULT_BACKGROUND_BLUR_PX);
        assert_eq!(on_disk.overlay_opacity, None);
        assert_eq!(on_disk.fit, BackgroundFit::Cover);

        // Corrupt background.json must error strictly and never be overwritten with defaults
        let corrupt_payload = "{ not valid json";
        tokio::fs::write(&config_path, corrupt_payload)
            .await
            .expect("write corrupt config");
        let err = update_background_style_in_dir(
            &storage_root,
            10,
            Some(40),
            BackgroundFit::Cover,
            None,
            &coord,
        )
        .await
        .expect_err("corrupt config must error instead of resetting");
        assert!(matches!(err, DawnlandError::JsonError(_)));
        assert_eq!(
            tokio::fs::read_to_string(&config_path)
                .await
                .expect("read corrupt"),
            corrupt_payload
        );

        // Corrupt background.json during import must also fail without leaving staged files behind
        let img = dir.path().join("valid.png");
        tokio::fs::write(&img, &make_valid_png_bytes(4, 4))
            .await
            .expect("write valid png");
        let err = import_and_commit_background_in_dir(&storage_root, &img, &coord, |_| Ok(()))
            .await
            .expect_err("import must fail on corrupt background.json");
        assert!(matches!(err, DawnlandError::JsonError(_)));
        assert_eq!(
            tokio::fs::read_to_string(&config_path)
                .await
                .expect("read corrupt after import"),
            corrupt_payload
        );
    }

    #[tokio::test]
    async fn test_background_storage_isolated_from_legacy_launcher_settings() {
        let dir = tempfile::tempdir().expect("tempdir");
        let storage_root = dir.path().join("app_data");
        let src_dir = dir.path().join("src");
        tokio::fs::create_dir_all(&src_dir).await.unwrap();

        let img = src_dir.join("wall.png");
        tokio::fs::write(&img, &make_valid_png_bytes(4, 4))
            .await
            .unwrap();

        let coord = BackgroundCoordination::new();
        let state = import_and_commit_background_in_dir(&storage_root, &img, &coord, |_| Ok(()))
            .await
            .expect("import background");

        // Simulating an older launcher writing launcher_settings.json (without any background field)
        let launcher_settings_path = dir.path().join("launcher_settings.json");
        let legacy_json = serde_json::to_string_pretty(&crate::core::settings::LauncherSettings::default())
            .expect("serialize launcher settings");
        assert!(!legacy_json.contains("background"));
        tokio::fs::write(&launcher_settings_path, legacy_json.as_bytes())
            .await
            .unwrap();

        // Reload background state from dedicated background.json -> unaffected
        let reloaded = load_background_state_in_dir(&storage_root, &coord).await;
        assert_eq!(reloaded.mode, BackgroundMode::Custom);
        assert_eq!(reloaded.image_file, state.image_file);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_import_preserves_trailing_whitespace_path_on_unix() {
        let dir = tempfile::tempdir().expect("tempdir");
        let storage_root = dir.path().join("app_data");
        let spaced_file = dir.path().join("wallpaper.png ");
        tokio::fs::write(&spaced_file, &make_valid_png_bytes(4, 4))
            .await
            .expect("write trailing-space file");

        let coord = BackgroundCoordination::new();
        let state =
            import_and_commit_background_in_dir(&storage_root, &spaced_file, &coord, |_| Ok(()))
                .await
                .expect("import trailing-space file");
        assert_eq!(state.mode, BackgroundMode::Custom);
    }

    #[test]
    fn test_background_settings_sanitization() {
        let unclamped = BackgroundSettings {
            mode: BackgroundMode::Custom,
            image_file: Some("bg-1.png".to_string()),
            blur_px: 99,
            overlay_opacity: Some(100),
            fit: BackgroundFit::Fill,
            updated_at: 42,
        };
        let clamped = unclamped.sanitized();
        assert_eq!(clamped.blur_px, MAX_BACKGROUND_BLUR_PX);
        assert_eq!(clamped.overlay_opacity, Some(MAX_BACKGROUND_OVERLAY_OPACITY));
    }
}
