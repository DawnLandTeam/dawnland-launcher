use crate::core::fs_utils::is_safe_single_name;
use crate::core::settings::{resolve_active_template, should_rename_category, LauncherSettings};
use crate::models::instance::AssetCategory;
use serde::{Deserialize, Serialize};
use std::path::Path;

const MAX_RENDERED_FILENAME_CHARS: usize = 180;

#[derive(Debug, Clone, Default)]
pub struct ResourceNamingContext {
    pub original_filename: String,
    pub category: AssetCategoryDefaultWrapper,
    pub zh_name: Option<String>,
    pub zh_full_name: Option<String>,
    pub en_name: Option<String>,
    pub slug: Option<String>,
    pub version: Option<String>,
    pub mc_version: Option<String>,
    pub loader: Option<String>,
    pub source: Option<String>,
}

/// Wrapper so `ResourceNamingContext` can derive `Default` without changing `AssetCategory`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetCategoryDefaultWrapper(pub AssetCategory);

impl Default for AssetCategoryDefaultWrapper {
    fn default() -> Self {
        Self(AssetCategory::Mod)
    }
}

impl From<AssetCategory> for AssetCategoryDefaultWrapper {
    fn from(value: AssetCategory) -> Self {
        Self(value)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplatePreviewResult {
    pub rendered: String,
    pub fallback_rendered: String,
    pub warnings: Vec<String>,
}

/// Extract the concise canonical Chinese name for filenames by stripping parenthetical
/// English titles or trailing ` - ` annotations (e.g. `"机械动力 (Create)"` -> `"机械动力"`).
pub fn extract_short_zh_name(raw_zh: &str) -> String {
    let mut s = raw_zh.trim();
    for sep in [" (", "（", " - "] {
        if let Some(idx) = s.find(sep) {
            let prefix = s[..idx].trim();
            if !prefix.is_empty() {
                s = prefix;
            }
        }
    }
    s.trim().to_string()
}

/// Split a filename into `(stem, ext_with_dot)`.
/// Preserves common compound extensions like `.tar.gz` or single extensions like `.jar`, `.zip`.
pub fn split_stem_and_ext(filename: &str) -> (String, String) {
    let trimmed = filename.trim();
    if let Some(dot_idx) = trimmed.rfind('.') {
        if dot_idx > 0 && dot_idx < trimmed.len() - 1 {
            let stem = &trimmed[..dot_idx];
            let ext = &trimmed[dot_idx..];
            return (stem.to_string(), ext.to_string());
        }
    }
    (trimmed.to_string(), String::new())
}

/// Strip known Dawnland/PCL localization prefixes/suffixes from a legacy renamed filename
/// when `original_filename` was not previously recorded.
pub fn strip_known_zh_prefixes(filename: &str) -> String {
    let (mut stem, ext) = split_stem_and_ext(filename);
    loop {
        let trimmed = stem.trim_start();
        if let Some(rest) = trimmed.strip_prefix('[') {
            if let Some(end) = rest.find(']') {
                let inside = &rest[..end];
                let after = rest[end + 1..].trim_start();
                if !after.is_empty() && contains_cjk(inside) {
                    stem = after.to_string();
                    continue;
                }
            }
        }
        if let Some(rest) = trimmed.strip_prefix('【') {
            if let Some(end) = rest.find('】') {
                let inside = &rest[..end];
                let after = rest[end + '】'.len_utf8()..].trim_start();
                if !after.is_empty() && contains_cjk(inside) {
                    stem = after.to_string();
                    continue;
                }
            }
        }
        break;
    }
    format!("{}{}", stem, ext)
}

pub fn contains_cjk(text: &str) -> bool {
    text.chars()
        .any(|c| ('\u{4E00}'..='\u{9FFF}').contains(&c) || ('\u{3400}'..='\u{4DBF}').contains(&c))
}

/// Sanitize an individual placeholder value or full filename so it is safe across Windows/macOS/Linux
/// and avoids Java URLClassLoader hazards (`~` in Mod filenames).
pub fn sanitize_filename_component(input: &str, is_mod: bool) -> String {
    let mut out = String::with_capacity(input.len());
    for c in input.chars() {
        if c.is_control() {
            continue;
        }
        match c {
            '\\' => out.push('＼'),
            '/' => out.push('／'),
            ':' => out.push('：'),
            '*' => out.push('＊'),
            '?' => out.push('？'),
            '"' => out.push('＂'),
            '<' => out.push('＜'),
            '>' => out.push('＞'),
            '|' => out.push('｜'),
            '~' if is_mod => out.push('-'),
            _ => out.push(c),
        }
    }
    out.trim().to_string()
}

/// Collapse empty brackets and duplicate separators caused by optional empty placeholders.
fn collapse_empty_separators(raw: &str) -> String {
    let mut s = raw.to_string();
    for empty in ["[]", "【】", "()", "[ ]", "【 】", "( )"] {
        s = s.replace(empty, "");
    }
    while s.contains("  ") {
        s = s.replace("  ", " ");
    }
    while s.contains("--") {
        s = s.replace("--", "-");
    }
    while s.contains("__") {
        s = s.replace("__", "_");
    }
    s = s.replace(" - - ", " - ");
    s.trim_matches(|c: char| c == ' ' || c == '-' || c == '_')
        .to_string()
}

fn clean_rendered_stem(stem: &str, is_mod: bool) -> String {
    let sanitized = sanitize_filename_component(stem, is_mod);
    let collapsed = collapse_empty_separators(&sanitized);
    collapsed
        .trim_end_matches('.')
        .trim()
        .to_string()
}

/// Render a target filename using the provided template and metadata context.
/// Guarantees:
/// - If template asks for `{zh}` / `{zhFull}` and no Chinese name is available, falls back cleanly to original filename.
/// - Preserves original extension `{ext}` even if omitted from custom template.
/// - Produces a cross-platform safe single filename component.
pub fn render_resource_filename(template: &str, ctx: &ResourceNamingContext) -> String {
    let is_mod = ctx.category.0 == AssetCategory::Mod;
    let raw_original = sanitize_filename_component(&ctx.original_filename, is_mod);
    let safe_original = if !raw_original.is_empty() && is_safe_single_name(&raw_original) {
        raw_original
    } else {
        "resource.jar".to_string()
    };

    let (orig_stem, orig_ext) = split_stem_and_ext(&safe_original);
    let trimmed_template = template.trim();
    if trimmed_template.is_empty() || trimmed_template == "{stem}{ext}" {
        return safe_original;
    }

    let zh_short = ctx
        .zh_name
        .as_deref()
        .map(extract_short_zh_name)
        .filter(|s| !s.is_empty())
        .map(|s| sanitize_filename_component(&s, is_mod))
        .unwrap_or_default();

    let zh_full = ctx
        .zh_full_name
        .as_deref()
        .or(ctx.zh_name.as_deref())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| sanitize_filename_component(s, is_mod))
        .unwrap_or_else(|| zh_short.clone());

    let uses_zh = trimmed_template.contains("{zh}") || trimmed_template.contains("{zhFull}");
    if uses_zh && zh_short.is_empty() && zh_full.is_empty() {
        return safe_original;
    }

    let en_name = ctx
        .en_name
        .as_deref()
        .map(|s| sanitize_filename_component(s, is_mod))
        .unwrap_or_default();
    let slug = ctx
        .slug
        .as_deref()
        .map(|s| sanitize_filename_component(s, is_mod))
        .unwrap_or_default();
    let version = ctx
        .version
        .as_deref()
        .map(|s| sanitize_filename_component(s, is_mod))
        .unwrap_or_default();
    let mc_version = ctx
        .mc_version
        .as_deref()
        .map(|s| sanitize_filename_component(s, is_mod))
        .unwrap_or_default();
    let loader = ctx
        .loader
        .as_deref()
        .map(|s| sanitize_filename_component(s, is_mod))
        .unwrap_or_default();
    let source = ctx
        .source
        .as_deref()
        .map(|s| sanitize_filename_component(s, is_mod))
        .unwrap_or_default();

    let mut rendered = trimmed_template
        .replace("{zhFull}", &zh_full)
        .replace("{zh}", &zh_short)
        .replace("{en}", &en_name)
        .replace("{slug}", &slug)
        .replace("{stem}", &orig_stem)
        .replace("{ext}", &orig_ext)
        .replace("{version}", &version)
        .replace("{mcVersion}", &mc_version)
        .replace("{loader}", &loader)
        .replace("{source}", &source);

    // Strip extension temporarily if it ended up at the end so we can sanitize stem cleanly
    let mut final_stem = if !orig_ext.is_empty()
        && rendered
            .to_ascii_lowercase()
            .ends_with(&orig_ext.to_ascii_lowercase())
    {
        let cut = rendered.len() - orig_ext.len();
        rendered[..cut].to_string()
    } else {
        rendered.clone()
    };

    final_stem = clean_rendered_stem(&final_stem, is_mod);
    if final_stem.is_empty() {
        return safe_original;
    }

    let max_stem_chars = MAX_RENDERED_FILENAME_CHARS.saturating_sub(orig_ext.chars().count());
    if final_stem.chars().count() > max_stem_chars && max_stem_chars > 0 {
        final_stem = final_stem
            .chars()
            .take(max_stem_chars)
            .collect::<String>()
            .trim()
            .to_string();
    }

    rendered = format!("{}{}", final_stem, orig_ext);
    if is_safe_single_name(&rendered) {
        rendered
    } else {
        safe_original
    }
}

/// Resolve the final filename for a newly downloaded resource using current `LauncherSettings`.
pub fn resolve_filename_for_download(
    settings: &LauncherSettings,
    ctx: &ResourceNamingContext,
) -> String {
    if !should_rename_category(settings, &ctx.category.0) {
        let is_mod = ctx.category.0 == AssetCategory::Mod;
        let clean = sanitize_filename_component(&ctx.original_filename, is_mod);
        return if !clean.is_empty() && is_safe_single_name(&clean) {
            clean
        } else {
            ctx.original_filename.clone()
        };
    }

    let template = resolve_active_template(settings);
    render_resource_filename(&template, ctx)
}

/// Build a non-colliding filename inside `target_dir` if another distinct file already occupies the name.
pub fn resolve_unique_filename_in_dir(
    target_dir: &Path,
    desired_filename: &str,
    existing_paths_to_ignore: &[&str],
) -> String {
    let clean = desired_filename.to_string();
    let check_conflict = |candidate: &str| -> bool {
        if existing_paths_to_ignore.contains(&candidate) {
            return false;
        }
        let candidate_path = target_dir.join(candidate);
        let disabled_path = target_dir.join(format!("{}.disable", candidate));
        candidate_path.exists() || disabled_path.exists()
    };

    if !check_conflict(&clean) {
        return clean;
    }

    let (stem, ext) = split_stem_and_ext(&clean);
    for idx in 2..=99 {
        let candidate = format!("{} ({}){}", stem, idx, ext);
        if !check_conflict(&candidate) {
            return candidate;
        }
    }
    clean
}

/// Preview a user-supplied filename template with sample Mod metadata and validation warnings.
#[tauri::command]
pub fn preview_resource_template(template: String) -> Result<TemplatePreviewResult, String> {
    let trimmed = template.trim();
    let mut warnings = Vec::new();

    if trimmed.is_empty() {
        warnings.push("empty_template".to_string());
    } else {
        if !trimmed.contains("{ext}") {
            warnings.push("missing_ext_auto_appended".to_string());
        }
        if !trimmed.contains("{zh}")
            && !trimmed.contains("{zhFull}")
            && !trimmed.contains("{stem}")
            && !trimmed.contains("{en}")
            && !trimmed.contains("{slug}")
        {
            warnings.push("missing_identifying_placeholder".to_string());
        }
        if ['\\', '/', ':', '*', '?', '"', '<', '>', '|']
            .iter()
            .any(|ch| trimmed.contains(*ch))
        {
            warnings.push("illegal_chars_sanitized".to_string());
        }
    }

    let effective_template = if trimmed.is_empty() {
        "{stem}{ext}"
    } else {
        trimmed
    };

    let sample_with_zh = ResourceNamingContext {
        original_filename: "create-1.21.1-6.0.4.jar".to_string(),
        category: AssetCategory::Mod.into(),
        zh_name: Some("机械动力".to_string()),
        zh_full_name: Some("机械动力 (Create)".to_string()),
        en_name: Some("Create".to_string()),
        slug: Some("create".to_string()),
        version: Some("6.0.4".to_string()),
        mc_version: Some("1.21.1".to_string()),
        loader: Some("NeoForge".to_string()),
        source: Some("Modrinth".to_string()),
    };

    let sample_without_zh = ResourceNamingContext {
        zh_name: None,
        zh_full_name: None,
        ..sample_with_zh.clone()
    };

    Ok(TemplatePreviewResult {
        rendered: render_resource_filename(effective_template, &sample_with_zh),
        fallback_rendered: render_resource_filename(effective_template, &sample_without_zh),
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::settings::{FilenameFormatMode, FilenamePreset};

    fn sample_ctx() -> ResourceNamingContext {
        ResourceNamingContext {
            original_filename: "create-1.21.1-6.0.4.jar".to_string(),
            category: AssetCategory::Mod.into(),
            zh_name: Some("机械动力 (Create)".to_string()),
            zh_full_name: Some("机械动力 (Create)".to_string()),
            en_name: Some("Create".to_string()),
            slug: Some("create".to_string()),
            version: Some("6.0.4".to_string()),
            mc_version: Some("1.21.1".to_string()),
            loader: Some("NeoForge".to_string()),
            source: Some("Modrinth".to_string()),
        }
    }

    #[test]
    fn test_render_presets() {
        let ctx = sample_ctx();
        assert_eq!(
            render_resource_filename("[{zh}] {stem}{ext}", &ctx),
            "[机械动力] create-1.21.1-6.0.4.jar"
        );
        assert_eq!(
            render_resource_filename("【{zh}】{stem}{ext}", &ctx),
            "【机械动力】create-1.21.1-6.0.4.jar"
        );
        assert_eq!(
            render_resource_filename("{zh}-{stem}{ext}", &ctx),
            "机械动力-create-1.21.1-6.0.4.jar"
        );
        assert_eq!(
            render_resource_filename("{stem}-{zh}{ext}", &ctx),
            "create-1.21.1-6.0.4-机械动力.jar"
        );
        assert_eq!(
            render_resource_filename("{zh}-{version}{ext}", &ctx),
            "机械动力-6.0.4.jar"
        );
    }

    #[test]
    fn test_missing_zh_falls_back_to_original() {
        let mut ctx = sample_ctx();
        ctx.zh_name = None;
        ctx.zh_full_name = None;
        assert_eq!(
            render_resource_filename("[{zh}] {stem}{ext}", &ctx),
            "create-1.21.1-6.0.4.jar"
        );
    }

    #[test]
    fn test_custom_template_without_ext_auto_appends_ext() {
        let ctx = sample_ctx();
        assert_eq!(
            render_resource_filename("[{zh}] {en}-{mcVersion}", &ctx),
            "[机械动力] Create-1.21.1.jar"
        );
    }

    #[test]
    fn test_empty_optional_placeholders_collapse_cleanly() {
        let mut ctx = sample_ctx();
        ctx.loader = None;
        ctx.version = None;
        assert_eq!(
            render_resource_filename("[{zh}] {en} [{loader}]-{version}{ext}", &ctx),
            "[机械动力] Create.jar"
        );
    }

    #[test]
    fn test_sanitizes_illegal_chars_and_tilde_for_mods() {
        let mut ctx = sample_ctx();
        ctx.zh_name = Some("测试:Mod/A~1".to_string());
        let out = render_resource_filename("[{zh}] {stem}{ext}", &ctx);
        assert!(!out.contains(':'));
        assert!(!out.contains('/'));
        assert!(!out.contains('~'));
        assert_eq!(out, "[测试：Mod／A-1] create-1.21.1-6.0.4.jar");
    }

    #[test]
    fn test_strip_known_zh_prefixes_is_idempotent() {
        assert_eq!(
            strip_known_zh_prefixes("[机械动力] create-1.21.1-6.0.4.jar"),
            "create-1.21.1-6.0.4.jar"
        );
        assert_eq!(
            strip_known_zh_prefixes("【机械动力】create-1.21.1-6.0.4.jar"),
            "create-1.21.1-6.0.4.jar"
        );
        assert_eq!(
            strip_known_zh_prefixes("[1.21.1] create-1.21.1-6.0.4.jar"),
            "[1.21.1] create-1.21.1-6.0.4.jar"
        );
    }

    #[test]
    fn test_resolve_filename_respects_language_auto_default() {
        let ctx = sample_ctx();
        let mut settings = LauncherSettings::default();
        settings.resource_naming.filename_mode = FilenameFormatMode::Auto;

        settings.ui_language = Some("zh-CN".to_string());
        assert_eq!(
            resolve_filename_for_download(&settings, &ctx),
            "[机械动力] create-1.21.1-6.0.4.jar"
        );

        settings.ui_language = Some("en".to_string());
        assert_eq!(
            resolve_filename_for_download(&settings, &ctx),
            "create-1.21.1-6.0.4.jar"
        );

        settings.resource_naming.filename_mode = FilenameFormatMode::Preset;
        settings.resource_naming.filename_preset = FilenamePreset::CnBracketPrefix;
        assert_eq!(
            resolve_filename_for_download(&settings, &ctx),
            "【机械动力】create-1.21.1-6.0.4.jar"
        );
    }
}
