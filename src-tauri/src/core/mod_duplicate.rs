use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::core::fs_utils::is_safe_single_name;
use crate::core::manager::{
    asset_source_to_str, get_instance_details_in_base, load_sanitized_manifest,
};
use crate::core::mod_parser::{build_mod_cache_key, ModParser};
use crate::core::mojang::get_minecraft_base;
use crate::core::resource_naming::strip_known_zh_prefixes;
use crate::core::translation::{ModIdentityHints, TranslationDb};
use crate::models::instance::{AssetCategory, AssetRecord};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckDuplicateModsRequest {
    pub version_id: String,
    pub source: String,
    pub project_id: String,
    pub slug: String,
    pub title: String,
    #[serde(default)]
    pub target_filename: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DuplicateMatchReason {
    ExactFilename,
    ProjectId,
    ModId,
    Slug,
    ExactName,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateModMatch {
    pub filename: String,
    pub enabled: bool,
    pub mod_id: Option<String>,
    pub name: Option<String>,
    pub version: Option<String>,
    pub original_filename: Option<String>,
    pub match_reason: DuplicateMatchReason,
    pub exact_filename_conflict: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateModCheckResult {
    pub duplicates: Vec<DuplicateModMatch>,
    pub has_exact_filename_conflict: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct TargetIdentity {
    pub source_lower: String,
    pub project_id: String,
    pub target_filename_lower: Option<String>,
    pub known_platform_ids: HashSet<(String, String)>,
    pub known_ids: HashSet<String>,
    pub known_names: HashSet<String>,
    pub lazy_parse_hints: HashSet<String>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct LocalModCandidate {
    pub filename: String,
    pub original_filename: Option<String>,
    pub source: Option<String>,
    pub project_id: Option<String>,
    pub slug: Option<String>,
    pub mod_id: Option<String>,
    pub name: Option<String>,
}

struct ScannedModFile {
    path: PathBuf,
    actual_filename: String,
    enabled: bool,
    size: u64,
    mtime: u64,
}

pub(crate) fn normalize_filename(raw: &str) -> String {
    raw.trim().to_lowercase()
}

pub(crate) fn compact_id(raw: &str) -> String {
    raw.trim()
        .to_ascii_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect()
}

fn is_loader_only_segment(inner: &str) -> bool {
    let mut has_loader = false;
    let mut token = String::new();

    for ch in inner.chars() {
        if ch.is_ascii_alphanumeric() {
            token.push(ch);
        } else if matches!(ch, '/' | ',' | '&' | '+' | '-' | '|') || ch.is_whitespace() {
            if !token.is_empty() {
                if !matches!(
                    token.as_str(),
                    "fabric" | "forge" | "neoforge" | "quilt" | "liteloader" | "rift"
                ) {
                    return false;
                }
                has_loader = true;
                token.clear();
            }
        } else {
            return false;
        }
    }

    if !token.is_empty() {
        if !matches!(
            token.as_str(),
            "fabric" | "forge" | "neoforge" | "quilt" | "liteloader" | "rift"
        ) {
            return false;
        }
        has_loader = true;
    }

    has_loader
}

fn strip_loader_bracketed_segments(lower: &str) -> String {
    let mut out = String::with_capacity(lower.len());
    let mut rest = lower;

    while let Some((idx, ch)) = rest.char_indices().next() {
        let _ = idx;
        if ch == '(' || ch == '[' {
            let close = if ch == '(' { ')' } else { ']' };
            let after_open = &rest[ch.len_utf8()..];
            if let Some(close_pos) = after_open.find(close) {
                let inner = &after_open[..close_pos];
                if is_loader_only_segment(inner) {
                    out.push(' ');
                    rest = &after_open[close_pos + close.len_utf8()..];
                    continue;
                }
            }
        }
        out.push(ch);
        rest = &rest[ch.len_utf8()..];
    }

    out
}

pub(crate) fn normalize_mod_name(raw: &str) -> String {
    let lower = raw.trim().to_lowercase();
    if lower.is_empty() {
        return String::new();
    }

    let stripped = strip_loader_bracketed_segments(&lower);
    let mut normalized = String::with_capacity(stripped.len());
    for ch in stripped.chars() {
        if matches!(ch, '\'' | '\u{2018}' | '\u{2019}') {
            continue;
        }
        if ch.is_alphanumeric() {
            normalized.push(ch);
        } else {
            normalized.push(' ');
        }
    }

    normalized.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn strip_jar_extension(raw: &str) -> &str {
    let trimmed = raw.trim();
    let without_disable = if trimmed.len() >= 8 && trimmed[trimmed.len() - 8..].eq_ignore_ascii_case(".disable") {
        &trimmed[..trimmed.len() - 8]
    } else {
        trimmed
    };
    if without_disable.len() >= 4
        && without_disable[without_disable.len() - 4..].eq_ignore_ascii_case(".jar")
    {
        &without_disable[..without_disable.len() - 4]
    } else {
        without_disable
    }
}

fn is_version_or_loader_token(token_lower: &str) -> bool {
    if matches!(
        token_lower,
        "fabric" | "forge" | "neoforge" | "quilt" | "liteloader" | "rift"
    ) {
        return true;
    }
    let bytes = token_lower.as_bytes();
    if bytes.first().is_some_and(u8::is_ascii_digit) {
        return true;
    }
    if let Some(rest) = token_lower.strip_prefix("mc") {
        if rest.as_bytes().first().is_some_and(u8::is_ascii_digit) {
            return true;
        }
    }
    if let Some(rest) = token_lower.strip_prefix('v') {
        if rest.as_bytes().first().is_some_and(u8::is_ascii_digit) {
            return true;
        }
    }
    false
}

fn extract_leading_filename_hint(target_filename: &str) -> Option<String> {
    let stem = strip_jar_extension(target_filename);
    let mut collected = String::new();

    for (idx, raw_tok) in stem
        .split(['-', '_', ' ', '+'])
        .filter(|s| !s.is_empty())
        .enumerate()
    {
        let tok_lower = raw_tok.to_ascii_lowercase();
        if idx > 0 && is_version_or_loader_token(&tok_lower) {
            break;
        }
        collected.push_str(&compact_id(&tok_lower));
        if idx == 0 && is_version_or_loader_token(&tok_lower) {
            // Keep at least the leading token if it was a platform/loader word (e.g. "fabric-api"),
            // then continue scanning until a version token appears.
            continue;
        }
    }

    if collected.len() >= 2 {
        Some(collected)
    } else {
        None
    }
}

impl TargetIdentity {
    pub(crate) fn from_request(
        req: &CheckDuplicateModsRequest,
        hints: Option<&ModIdentityHints>,
    ) -> Self {
        let source_lower = req.source.trim().to_ascii_lowercase();
        let project_id = req.project_id.trim().to_string();
        let target_filename_lower = req
            .target_filename
            .as_deref()
            .map(normalize_filename)
            .filter(|s| !s.is_empty());

        let mut known_platform_ids = HashSet::new();
        if !source_lower.is_empty() && !project_id.is_empty() {
            known_platform_ids.insert((source_lower.clone(), project_id.clone()));
        }

        let mut known_ids = HashSet::new();
        let req_slug = compact_id(&req.slug);
        if !req_slug.is_empty() {
            known_ids.insert(req_slug);
        }

        let mut known_names = HashSet::new();
        let req_name = normalize_mod_name(&req.title);
        if !req_name.is_empty() {
            known_names.insert(req_name);
        }

        if let Some(h) = hints {
            if let Some(ref cf_pid) = h.cf_project_id {
                let p = cf_pid.trim();
                if !p.is_empty() {
                    known_platform_ids.insert(("curseforge".to_string(), p.to_string()));
                }
            }
            if let Some(ref mr_pid) = h.mr_project_id {
                let p = mr_pid.trim();
                if !p.is_empty() {
                    known_platform_ids.insert(("modrinth".to_string(), p.to_string()));
                }
            }
            if let Some(ref cf_slug) = h.cf_slug {
                let c = compact_id(cf_slug);
                if !c.is_empty() {
                    known_ids.insert(c);
                }
            }
            if let Some(ref mr_slug) = h.mr_slug {
                let c = compact_id(mr_slug);
                if !c.is_empty() {
                    known_ids.insert(c);
                }
            }

            // Only adopt `mod_ids` from TranslationDb when the row maps to a single unique mod ID
            // (so entries like Create's `"create;flywheel"` never match standalone Flywheel).
            let unique_mod_ids: HashSet<String> = h
                .mod_ids
                .iter()
                .map(|m| compact_id(m))
                .filter(|c| !c.is_empty())
                .collect();
            if unique_mod_ids.len() == 1 {
                known_ids.extend(unique_mod_ids);
            }

            if let Some(ref raw_name) = h.raw_name {
                let n = normalize_mod_name(raw_name);
                if !n.is_empty() {
                    known_names.insert(n);
                }
            }
        }

        let mut lazy_parse_hints: HashSet<String> = known_ids
            .iter()
            .filter(|id| id.len() >= 2)
            .cloned()
            .collect();
        let compact_title = compact_id(&req.title);
        if compact_title.len() >= 2 {
            lazy_parse_hints.insert(compact_title);
        }
        if let Some(ref tf) = req.target_filename {
            if let Some(prefix_hint) = extract_leading_filename_hint(tf) {
                lazy_parse_hints.insert(prefix_hint);
            }
        }

        Self {
            source_lower,
            project_id,
            target_filename_lower,
            known_platform_ids,
            known_ids,
            known_names,
            lazy_parse_hints,
        }
    }
}

pub(crate) fn should_lazy_parse_candidate(
    filename: &str,
    original_filename: Option<&str>,
    hints: &HashSet<String>,
) -> bool {
    if hints.is_empty() {
        return false;
    }

    let stems = [Some(filename), original_filename]
        .into_iter()
        .flatten()
        .map(strip_jar_extension)
        .filter(|s| !s.is_empty());

    for stem in stems {
        let compact_stem = compact_id(stem);
        let tokens: Vec<String> = stem
            .split(|c: char| !c.is_ascii_alphanumeric())
            .filter(|t| !t.is_empty())
            .map(str::to_ascii_lowercase)
            .collect();

        for hint in hints {
            if hint.len() >= 3 && compact_stem.contains(hint.as_str()) {
                return true;
            }
            if hint.len() == 2 && tokens.iter().any(|t| t == hint) {
                return true;
            }
        }
    }

    false
}

pub(crate) fn evaluate_duplicate_match(
    target: &TargetIdentity,
    local: &LocalModCandidate,
) -> Option<(DuplicateMatchReason, bool)> {
    let exact_filename_conflict = target.target_filename_lower.as_deref().is_some_and(|tf| {
        normalize_filename(&local.filename) == tf
            || local
                .original_filename
                .as_deref()
                .map(normalize_filename)
                .as_deref()
                == Some(tf)
    });

    let local_source_lower = local
        .source
        .as_deref()
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty());
    let local_project_id = local
        .project_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let same_platform_different_project = local_source_lower
        .as_deref()
        .is_some_and(|ls| !target.source_lower.is_empty() && ls == target.source_lower)
        && local_project_id
            .is_some_and(|lp| !target.project_id.is_empty() && lp != target.project_id);

    // Rule 1: Exact filename (current or canonical originalFilename)
    if exact_filename_conflict {
        return Some((DuplicateMatchReason::ExactFilename, true));
    }

    // Rule 2: Exact project_id match (same platform or verified cross-platform identity)
    if let Some(lp) = local_project_id {
        if (local_source_lower.is_none()
            || local_source_lower.as_deref() == Some(target.source_lower.as_str()))
            && !target.project_id.is_empty()
            && lp == target.project_id
        {
            return Some((DuplicateMatchReason::ProjectId, false));
        }

        if !same_platform_different_project {
            if let Some(ref ls) = local_source_lower {
                if target
                    .known_platform_ids
                    .contains(&(ls.clone(), lp.to_string()))
                {
                    return Some((DuplicateMatchReason::ProjectId, false));
                }
            }
        }
    }

    // Rule 3: Exact mod_id / slug match (NEVER inspects `provides`)
    if let Some(mod_id) = local
        .mod_id
        .as_deref()
        .map(compact_id)
        .filter(|s| !s.is_empty())
    {
        if target.known_ids.contains(&mod_id) {
            return Some((DuplicateMatchReason::ModId, false));
        }
    }

    if !same_platform_different_project {
        if let Some(slug) = local
            .slug
            .as_deref()
            .map(compact_id)
            .filter(|s| !s.is_empty())
        {
            if target.known_ids.contains(&slug) {
                return Some((DuplicateMatchReason::Slug, false));
            }
        }
    }

    // Rule 4: Exact normalized display name match (no substring matching)
    if !same_platform_different_project {
        if let Some(name) = local
            .name
            .as_deref()
            .map(normalize_mod_name)
            .filter(|s| !s.is_empty())
        {
            if target.known_names.contains(&name) {
                return Some((DuplicateMatchReason::ExactName, false));
            }
        }
    }

    None
}

#[tauri::command]
pub async fn check_duplicate_mods(
    request: CheckDuplicateModsRequest,
) -> Result<DuplicateModCheckResult, String> {
    if !is_safe_single_name(&request.version_id) {
        return Err("Invalid instance ID".to_string());
    }
    let _op_permit = crate::core::cache::acquire_operation_permit(None).await;
    let base_dir = get_minecraft_base();
    check_duplicate_mods_in_base(base_dir, request).await
}

pub(crate) async fn check_duplicate_mods_in_base(
    base_dir: &Path,
    request: CheckDuplicateModsRequest,
) -> Result<DuplicateModCheckResult, String> {
    check_duplicate_mods_in_base_with_db(base_dir, None, request).await
}

pub(crate) async fn check_duplicate_mods_in_base_with_db(
    base_dir: &Path,
    translations_db_path: Option<PathBuf>,
    request: CheckDuplicateModsRequest,
) -> Result<DuplicateModCheckResult, String> {
    if !is_safe_single_name(&request.version_id) {
        return Err("Invalid instance ID".to_string());
    }

    let instance_dir = base_dir.join("versions").join(&request.version_id);
    let mods_dir = instance_dir.join("mods");
    if !tokio::fs::try_exists(&mods_dir)
        .await
        .map_err(|e| format!("Failed to check mods directory: {}", e))?
    {
        return Ok(DuplicateModCheckResult {
            duplicates: Vec::new(),
            has_exact_filename_conflict: false,
        });
    }

    let loader_type = get_instance_details_in_base(base_dir, &request.version_id)
        .await
        .map(|i| i.loader_type)
        .unwrap_or_default();

    let manifest = load_sanitized_manifest(&instance_dir).await;
    let mut manifest_records: HashMap<String, AssetRecord> = HashMap::new();
    for (path_key, record) in manifest.assets {
        if record.category != AssetCategory::Mod && !path_key.starts_with("mods/") {
            continue;
        }
        if let Some(filename) = Path::new(&path_key).file_name() {
            let fname = filename.to_string_lossy();
            let base_name = fname.strip_suffix(".disable").unwrap_or(&fname).to_string();
            manifest_records.insert(base_name, record);
        }
    }

    let mut scanned_files = Vec::new();
    let mut entries = tokio::fs::read_dir(&mods_dir)
        .await
        .map_err(|e| format!("Failed to read mods directory: {}", e))?;

    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|e| format!("Failed to read directory entry: {}", e))?
    {
        let path = entry.path();
        let filename = entry.file_name().to_string_lossy().to_string();
        let metadata = tokio::fs::metadata(&path)
            .await
            .map_err(|e| format!("Failed to read mod metadata: {}", e))?;
        if !metadata.is_file() {
            continue;
        }

        let (actual_filename, enabled) = if let Some(stripped) = filename.strip_suffix(".disable") {
            if !stripped.ends_with(".jar") {
                continue;
            }
            (stripped.to_string(), false)
        } else if filename.ends_with(".jar") {
            (filename, true)
        } else {
            continue;
        };

        let mtime = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);

        scanned_files.push(ScannedModFile {
            path,
            actual_filename,
            enabled,
            size: metadata.len(),
            mtime,
        });
    }

    if scanned_files.is_empty() {
        return Ok(DuplicateModCheckResult {
            duplicates: Vec::new(),
            has_exact_filename_conflict: false,
        });
    }

    let base_dir_buf = base_dir.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let db_opt = match translations_db_path {
            Some(path) => TranslationDb::open_at(path).ok(),
            None => TranslationDb::open_default().ok(),
        };

        let hints = db_opt.as_ref().and_then(|db| {
            db.lookup_mod_identity(&request.source, &request.project_id, &request.slug)
        });
        let target = TargetIdentity::from_request(&request, hints.as_ref());

        let parser = ModParser::new(&base_dir_buf);
        let mut cache_entries = parser.load_all_cache();
        let mut duplicates = Vec::new();

        for file in scanned_files {
            let cache_key =
                build_mod_cache_key(&file.actual_filename, file.size, file.mtime, &loader_type);
            let rec_opt = manifest_records.get(&file.actual_filename);
            let original_filename = rec_opt
                .and_then(|r| r.original_filename.clone())
                .or_else(|| Some(strip_known_zh_prefixes(&file.actual_filename)));

            let hash_map = db_opt.as_ref().and_then(|db| {
                rec_opt
                    .and_then(|r| r.hash_sha1.as_deref())
                    .and_then(|sha1| db.get_hash_mapping(sha1))
            });

            let local_source = hash_map
                .as_ref()
                .map(|h| h.source.clone())
                .or_else(|| rec_opt.and_then(|r| asset_source_to_str(&r.source_type)));
            let local_project_id = hash_map
                .as_ref()
                .map(|h| h.project_id.clone())
                .or_else(|| rec_opt.and_then(|r| r.project_id.clone()));
            let local_slug = hash_map
                .as_ref()
                .map(|h| h.slug.clone())
                .filter(|s| !s.is_empty());

            let cached_meta = cache_entries.get(&cache_key).cloned();
            let mut version = cached_meta.as_ref().and_then(|m| m.version.clone());

            let mut candidate = LocalModCandidate {
                filename: file.actual_filename.clone(),
                original_filename: original_filename.clone(),
                source: local_source,
                project_id: local_project_id,
                slug: local_slug,
                mod_id: cached_meta.as_ref().and_then(|m| m.mod_id.clone()),
                name: cached_meta.as_ref().and_then(|m| m.name.clone()),
            };

            // Only lazily open uncached JARs when Phase-A cannot match and the filename hints at a match
            if cached_meta.is_none()
                && evaluate_duplicate_match(&target, &candidate).is_none()
                && should_lazy_parse_candidate(
                    &file.actual_filename,
                    original_filename.as_deref(),
                    &target.lazy_parse_hints,
                )
            {
                let loader_opt = (!loader_type.is_empty()).then_some(loader_type.as_str());
                if let Some(parsed) = parser.try_parse_mod(&file.path, &cache_key, loader_opt) {
                    parser.set_cache_entry(&cache_key, &parsed);
                    cache_entries.insert(cache_key, parsed.clone());
                    candidate.mod_id = parsed.mod_id;
                    candidate.name = parsed.name;
                    version = parsed.version;
                }
            }

            if let Some((match_reason, exact_filename_conflict)) =
                evaluate_duplicate_match(&target, &candidate)
            {
                duplicates.push(DuplicateModMatch {
                    filename: file.actual_filename,
                    enabled: file.enabled,
                    mod_id: candidate.mod_id,
                    name: candidate.name,
                    version,
                    original_filename,
                    match_reason,
                    exact_filename_conflict,
                });
            }
        }

        duplicates.sort_by(|a, b| {
            a.filename
                .cmp(&b.filename)
                .then_with(|| (!a.enabled).cmp(&!b.enabled))
        });

        let has_exact_filename_conflict =
            duplicates.iter().any(|d| d.exact_filename_conflict);

        Ok(DuplicateModCheckResult {
            duplicates,
            has_exact_filename_conflict,
        })
    })
    .await
    .map_err(|e| format!("Duplicate mod check task failed: {}", e))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::translation::{TranslationEntry, TRUST_VERIFIED};
    use crate::models::instance::{AssetManifest, AssetSourceType};
    use std::fs::{self, File};
    use std::io::Write;
    use tempfile::tempdir;
    use zip::write::SimpleFileOptions;

    fn make_req(
        source: &str,
        project_id: &str,
        slug: &str,
        title: &str,
        target_filename: Option<&str>,
    ) -> CheckDuplicateModsRequest {
        CheckDuplicateModsRequest {
            version_id: "inst-1".to_string(),
            source: source.to_string(),
            project_id: project_id.to_string(),
            slug: slug.to_string(),
            title: title.to_string(),
            target_filename: target_filename.map(str::to_string),
        }
    }

    fn make_local(
        filename: &str,
        mod_id: Option<&str>,
        name: Option<&str>,
    ) -> LocalModCandidate {
        LocalModCandidate {
            filename: filename.to_string(),
            original_filename: None,
            source: None,
            project_id: None,
            slug: None,
            mod_id: mod_id.map(str::to_string),
            name: name.map(str::to_string),
        }
    }

    fn write_test_jar(path: &Path, entries: &[(&str, &[u8])]) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let file = File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = SimpleFileOptions::default();
        for (name, content) in entries {
            zip.start_file(*name, options).unwrap();
            zip.write_all(content).unwrap();
        }
        zip.finish().unwrap();
    }

    #[test]
    fn test_normalization_helpers() {
        assert_eq!(normalize_mod_name("Litematica [Fabric]"), "litematica");
        assert_eq!(
            normalize_mod_name("Litematica (Forge / NeoForge)"),
            "litematica"
        );
        assert_eq!(
            normalize_mod_name("Litematica Printer"),
            "litematica printer"
        );
        assert_eq!(
            normalize_mod_name("Just Enough Items (JEI)"),
            "just enough items jei"
        );
        assert_eq!(
            normalize_mod_name("Tinkers' Construct"),
            "tinkers construct"
        );
        assert_eq!(
            normalize_mod_name("Create: Steam 'n' Rails"),
            "create steam n rails"
        );
        assert_eq!(compact_id("litematica-printer"), "litematicaprinter");
        assert_eq!(compact_id("litematica_printer"), "litematicaprinter");
    }

    #[test]
    fn test_anti_false_positive_addons_and_substrings() {
        // 1. Litematica vs Litematica Printer (both directions)
        let litematica_target = TargetIdentity::from_request(
            &make_req(
                "modrinth",
                "bEpr0Arc",
                "litematica",
                "Litematica",
                Some("litematica-fabric-1.20.1-0.15.4.jar"),
            ),
            None,
        );
        let printer_local = LocalModCandidate {
            filename: "litematica-printer-26.3-3.2.3.jar".to_string(),
            source: Some("modrinth".to_string()),
            project_id: Some("printerProj".to_string()),
            mod_id: Some("litematica-printer".to_string()),
            name: Some("Litematica Printer".to_string()),
            ..Default::default()
        };
        assert_eq!(
            evaluate_duplicate_match(&litematica_target, &printer_local),
            None
        );

        let printer_target = TargetIdentity::from_request(
            &make_req(
                "modrinth",
                "printerProj",
                "litematica-printer",
                "Litematica Printer",
                Some("litematica-printer-26.3-3.2.3.jar"),
            ),
            None,
        );
        let litematica_local = LocalModCandidate {
            filename: "litematica-fabric-1.20.1-0.15.4.jar".to_string(),
            source: Some("modrinth".to_string()),
            project_id: Some("bEpr0Arc".to_string()),
            mod_id: Some("litematica".to_string()),
            name: Some("Litematica".to_string()),
            ..Default::default()
        };
        assert_eq!(
            evaluate_duplicate_match(&printer_target, &litematica_local),
            None
        );

        // 2. Create vs Create: Steam 'n' Rails / Create Crafts & Additions / standalone Flywheel
        let create_hints = ModIdentityHints {
            cf_project_id: Some("328085".to_string()),
            cf_slug: Some("create".to_string()),
            mr_project_id: Some("LNytGkhc".to_string()),
            mr_slug: Some("create".to_string()),
            mod_ids: vec!["create".to_string(), "flywheel".to_string()],
            raw_name: Some("Create".to_string()),
        };
        let create_target = TargetIdentity::from_request(
            &make_req(
                "curseforge",
                "328085",
                "create",
                "Create",
                Some("create-1.20.1-0.5.1.f.jar"),
            ),
            Some(&create_hints),
        );
        assert_eq!(
            evaluate_duplicate_match(
                &create_target,
                &make_local("Steam_Rails-1.6.4.jar", Some("railways"), Some("Create: Steam 'n' Rails"))
            ),
            None
        );
        assert_eq!(
            evaluate_duplicate_match(
                &create_target,
                &make_local("createaddition-1.20.1.jar", Some("createaddition"), Some("Create Crafts & Additions"))
            ),
            None
        );
        assert_eq!(
            evaluate_duplicate_match(
                &create_target,
                &make_local("flywheel-forge-1.20.1-0.6.10.jar", Some("flywheel"), Some("Flywheel"))
            ),
            None
        );

        // 3. Sodium vs Sodium Extra / Reese's Sodium Options / Embeddium
        let sodium_target = TargetIdentity::from_request(
            &make_req(
                "modrinth",
                "AANobbMI",
                "sodium",
                "Sodium",
                Some("sodium-fabric-0.5.8+mc1.20.1.jar"),
            ),
            None,
        );
        assert_eq!(
            evaluate_duplicate_match(
                &sodium_target,
                &make_local("sodium-extra-0.5.4.jar", Some("sodium-extra"), Some("Sodium Extra"))
            ),
            None
        );
        assert_eq!(
            evaluate_duplicate_match(
                &sodium_target,
                &make_local("reeses_sodium_options-1.7.2.jar", Some("reeses-sodium-options"), Some("Reese's Sodium Options"))
            ),
            None
        );
        // Embeddium even if it declares provides=["sodium"], LocalModCandidate never passes provides
        assert_eq!(
            evaluate_duplicate_match(
                &sodium_target,
                &make_local("embeddium-0.3.18.jar", Some("embeddium"), Some("Embeddium"))
            ),
            None
        );

        // 4. Mekanism vs Mekanism Generators
        let mek_target = TargetIdentity::from_request(
            &make_req("curseforge", "268560", "mekanism", "Mekanism", None),
            None,
        );
        assert_eq!(
            evaluate_duplicate_match(
                &mek_target,
                &make_local("MekanismGenerators-1.20.1.jar", Some("mekanismgenerators"), Some("Mekanism: Generators"))
            ),
            None
        );

        // 5. JEI vs JEI Integration
        let jei_target = TargetIdentity::from_request(
            &make_req("curseforge", "238222", "jei", "Just Enough Items (JEI)", None),
            None,
        );
        assert_eq!(
            evaluate_duplicate_match(
                &jei_target,
                &make_local("jeiintegration_1.20.1.jar", Some("jeiintegration"), Some("JEI Integration"))
            ),
            None
        );

        // 6. Same platform, different project_id, same display name, different mod_id -> NOT duplicate
        let same_plat_diff_pid_target = TargetIdentity::from_request(
            &make_req("modrinth", "projA", "cool-mod-a", "Cool Mod", None),
            None,
        );
        let same_plat_diff_pid_local = LocalModCandidate {
            filename: "cool-mod-b-1.0.jar".to_string(),
            source: Some("modrinth".to_string()),
            project_id: Some("projB".to_string()),
            slug: Some("cool-mod-b".to_string()),
            mod_id: Some("cool_mod_b".to_string()),
            name: Some("Cool Mod".to_string()),
            ..Default::default()
        };
        assert_eq!(
            evaluate_duplicate_match(&same_plat_diff_pid_target, &same_plat_diff_pid_local),
            None
        );
    }

    #[test]
    fn test_true_positive_matching_rules() {
        // 1. Same platform & project_id with different filename -> ProjectId, exact_filename_conflict = false
        let target = TargetIdentity::from_request(
            &make_req(
                "modrinth",
                "bEpr0Arc",
                "litematica",
                "Litematica",
                Some("litematica-fabric-1.20.1-0.15.4.jar"),
            ),
            None,
        );
        let old_ver = LocalModCandidate {
            filename: "litematica-fabric-1.20.1-0.15.3.jar".to_string(),
            source: Some("modrinth".to_string()),
            project_id: Some("bEpr0Arc".to_string()),
            ..Default::default()
        };
        assert_eq!(
            evaluate_duplicate_match(&target, &old_ver),
            Some((DuplicateMatchReason::ProjectId, false))
        );

        // 2. Exact filename conflict (case-insensitive)
        let exact_case_diff = LocalModCandidate {
            filename: "LITEMATICA-FABRIC-1.20.1-0.15.4.JAR".to_string(),
            source: Some("modrinth".to_string()),
            project_id: Some("bEpr0Arc".to_string()),
            ..Default::default()
        };
        assert_eq!(
            evaluate_duplicate_match(&target, &exact_case_diff),
            Some((DuplicateMatchReason::ExactFilename, true))
        );

        // 3. Chinese renamed file matching via originalFilename
        let renamed_local = LocalModCandidate {
            filename: "[投影] litematica-fabric-1.20.1-0.15.4.jar".to_string(),
            original_filename: Some("litematica-fabric-1.20.1-0.15.4.jar".to_string()),
            ..Default::default()
        };
        assert_eq!(
            evaluate_duplicate_match(&target, &renamed_local),
            Some((DuplicateMatchReason::ExactFilename, true))
        );

        // 4. Loader-tagged title matches exact normalized name
        let loader_tagged_target = TargetIdentity::from_request(
            &make_req("curseforge", "308892", "litematica", "Litematica [Fabric]", None),
            None,
        );
        assert_eq!(
            evaluate_duplicate_match(
                &loader_tagged_target,
                &make_local("manual-litematica.jar", None, Some("Litematica"))
            ),
            Some((DuplicateMatchReason::ExactName, false))
        );
    }

    #[tokio::test]
    async fn test_end_to_end_lazy_parse_disabled_and_cross_platform() {
        let dir = tempdir().unwrap();
        let base_dir = dir.path();
        let inst_id = "fabric-1.20.1";
        let inst_dir = base_dir.join("versions").join(inst_id);
        let mods_dir = inst_dir.join("mods");
        fs::create_dir_all(&mods_dir).unwrap();

        // Write minimal instance config so get_instance_details_in_base detects Fabric
        fs::write(
            inst_dir.join(format!("{}.json", inst_id)),
            r#"{"id":"fabric-1.20.1","inheritsFrom":"1.20.1"}"#,
        )
        .unwrap();
        fs::write(
            inst_dir.join("dlml.json"),
            r#"{"mcVersion":"1.20.1","loaderType":"Fabric"}"#,
        )
        .unwrap();

        // Setup custom TranslationDb for cross-platform Iris matching
        let db_path = dir.path().join("translations.db");
        let db = TranslationDb::open_at(db_path.clone()).unwrap();
        db.upsert_entries_batch(&[TranslationEntry {
            category: "mod".to_string(),
            cf_project_id: Some("455508".to_string()),
            cf_slug: Some("irisshaders".to_string()),
            mr_project_id: Some("YL57xq9U".to_string()),
            mr_slug: Some("iris".to_string()),
            mod_ids: "iris".to_string(),
            raw_name: "Iris Shaders".to_string(),
            zh_name: "Iris光影".to_string(),
            zh_short_name: "Iris光影".to_string(),
            zh_aliases: String::new(),
            zh_summary: String::new(),
            wiki_id: 3864,
            trust_level: TRUST_VERIFIED,
            source_tag: "consensus".to_string(),
            updated_at: 100,
        }])
        .unwrap();

        // 1. Uncached manual old Litematica JAR (disabled!) -> should be lazy parsed & detected
        write_test_jar(
            &mods_dir.join("litematica-fabric-1.20.1-0.15.3.jar.disable"),
            &[(
                "fabric.mod.json",
                br#"{"schemaVersion":1,"id":"litematica","name":"Litematica","version":"0.15.3"}"#,
            )],
        );

        // 2. Uncached Litematica Printer JAR -> should be lazy parsed, yet NOT flagged
        write_test_jar(
            &mods_dir.join("litematica-printer-26.3-3.2.3.jar"),
            &[(
                "fabric.mod.json",
                br#"{"schemaVersion":1,"id":"litematica-printer","name":"Litematica Printer","version":"3.2.3"}"#,
            )],
        );

        // 3. Unrelated Sodium JAR -> should NOT even be parsed into mod_cache
        write_test_jar(
            &mods_dir.join("sodium-fabric-0.5.8.jar"),
            &[(
                "fabric.mod.json",
                br#"{"schemaVersion":1,"id":"sodium","name":"Sodium","version":"0.5.8"}"#,
            )],
        );

        // 4. Modrinth Iris recorded in assets.json
        write_test_jar(
            &mods_dir.join("iris-1.7.0.jar"),
            &[(
                "fabric.mod.json",
                br#"{"schemaVersion":1,"id":"iris","name":"Iris","version":"1.7.0"}"#,
            )],
        );

        let mut manifest = AssetManifest::default();
        manifest.assets.insert(
            "mods/iris-1.7.0.jar".to_string(),
            AssetRecord {
                category: AssetCategory::Mod,
                source_type: AssetSourceType::Modrinth,
                project_id: Some("YL57xq9U".to_string()),
                version_id: None,
                download_url: None,
                hash_sha1: None,
                hash_sha512: None,
                size: None,
                enabled: true,
                managed_by_modpack: false,
                original_filename: Some("iris-1.7.0.jar".to_string()),
                original_rel_path: None,
                display_name_zh: None,
            },
        );
        fs::write(
            inst_dir.join("assets.json"),
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();

        // Test A: Installing Litematica detects disabled manual Litematica, ignores Litematica Printer, skips Sodium
        let res_litematica = check_duplicate_mods_in_base_with_db(
            base_dir,
            Some(db_path.clone()),
            CheckDuplicateModsRequest {
                version_id: inst_id.to_string(),
                source: "modrinth".to_string(),
                project_id: "bEpr0Arc".to_string(),
                slug: "litematica".to_string(),
                title: "Litematica".to_string(),
                target_filename: Some("litematica-fabric-1.20.1-0.15.4.jar".to_string()),
            },
        )
        .await
        .unwrap();

        assert_eq!(res_litematica.duplicates.len(), 1);
        assert!(!res_litematica.has_exact_filename_conflict);
        let dup = &res_litematica.duplicates[0];
        assert_eq!(dup.filename, "litematica-fabric-1.20.1-0.15.3.jar");
        assert!(!dup.enabled);
        assert_eq!(dup.match_reason, DuplicateMatchReason::ModId);

        let cache = ModParser::new(base_dir).load_all_cache();
        assert!(
            cache.values().any(|m| m.mod_id.as_deref() == Some("litematica")),
            "litematica should be cached after targeted lazy parse"
        );
        assert!(
            !cache.values().any(|m| m.mod_id.as_deref() == Some("sodium")),
            "unrelated sodium.jar must NOT be parsed during Litematica check"
        );

        // Test B: Installing CurseForge Iris Shaders matches Modrinth Iris via TranslationDb cross-platform ID
        let res_iris = check_duplicate_mods_in_base_with_db(
            base_dir,
            Some(db_path),
            CheckDuplicateModsRequest {
                version_id: inst_id.to_string(),
                source: "curseforge".to_string(),
                project_id: "455508".to_string(),
                slug: "irisshaders".to_string(),
                title: "Iris Shaders".to_string(),
                target_filename: Some("iris-mc1.20.1-1.7.2.jar".to_string()),
            },
        )
        .await
        .unwrap();

        assert_eq!(res_iris.duplicates.len(), 1);
        assert_eq!(res_iris.duplicates[0].filename, "iris-1.7.0.jar");
        assert_eq!(res_iris.duplicates[0].match_reason, DuplicateMatchReason::ProjectId);
        assert!(!res_iris.has_exact_filename_conflict);
    }
}
