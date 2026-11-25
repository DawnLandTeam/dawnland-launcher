use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::core::fs_utils::is_safe_single_name;
use crate::core::manager::{
    asset_source_to_str, get_instance_details_in_base, get_instance_mod_install_lock,
    load_sanitized_manifest,
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

pub(crate) fn normalize_id(raw: &str) -> String {
    raw.trim().to_ascii_lowercase()
}

fn normalize_recognized_source(raw: Option<&str>) -> Option<String> {
    raw.map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| matches!(s.as_str(), "curseforge" | "modrinth"))
}

fn strip_ascii_suffix_ignore_case<'a>(s: &'a str, suffix: &str) -> Option<&'a str> {
    if s.len() >= suffix.len() {
        let split_at = s.len() - suffix.len();
        if s.is_char_boundary(split_at) && s[split_at..].eq_ignore_ascii_case(suffix) {
            return Some(&s[..split_at]);
        }
    }
    None
}

pub(crate) fn parse_mod_filename(filename: &str) -> Option<(String, bool)> {
    if let Some(stripped) = strip_ascii_suffix_ignore_case(filename, ".disable") {
        if strip_ascii_suffix_ignore_case(stripped, ".jar").is_some() {
            return Some((stripped.to_string(), false));
        }
        return None;
    }
    if strip_ascii_suffix_ignore_case(filename, ".jar").is_some() {
        return Some((filename.to_string(), true));
    }
    None
}

fn compute_file_and_path_fingerprint(path: &Path) -> Result<String, String> {
    let canonical = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let mut path_hasher = Sha256::new();
    path_hasher.update(canonical.to_string_lossy().as_bytes());
    let path_hex = hex::encode(path_hasher.finalize());

    let mut file = File::open(path)
        .map_err(|e| format!("Failed to open mod file {}: {}", path.display(), e))?;
    let mut content_hasher = Sha256::new();
    let mut buf = [0u8; 8192];
    loop {
        let read_bytes = file
            .read(&mut buf)
            .map_err(|e| format!("Failed to read mod file {}: {}", path.display(), e))?;
        if read_bytes == 0 {
            break;
        }
        content_hasher.update(&buf[..read_bytes]);
    }
    let content_hex = hex::encode(content_hasher.finalize());

    Ok(format!("{}_{}", &path_hex[..16], &content_hex[..32]))
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
        if let Some(recognized_source) = normalize_recognized_source(Some(&source_lower)) {
            if !project_id.is_empty() {
                known_platform_ids.insert((recognized_source, project_id.clone()));
            }
        }

        let mut known_ids = HashSet::new();
        let req_slug = normalize_id(&req.slug);
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
                let c = normalize_id(cf_slug);
                if !c.is_empty() {
                    known_ids.insert(c);
                }
            }
            if let Some(ref mr_slug) = h.mr_slug {
                let c = normalize_id(mr_slug);
                if !c.is_empty() {
                    known_ids.insert(c);
                }
            }

            // Only adopt `mod_ids` from TranslationDb when the row maps to a single unique mod ID
            // (so entries like Create's `"create;flywheel"` never match standalone Flywheel).
            let unique_mod_ids: HashSet<String> = h
                .mod_ids
                .iter()
                .map(|m| normalize_id(m))
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

        Self {
            source_lower,
            project_id,
            target_filename_lower,
            known_platform_ids,
            known_ids,
            known_names,
        }
    }
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

    let target_source_recognized = normalize_recognized_source(Some(&target.source_lower));
    let local_source_recognized = normalize_recognized_source(local.source.as_deref());
    let local_project_id = local
        .project_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let same_platform_different_project = local_source_recognized
        .as_deref()
        .is_some_and(|ls| target_source_recognized.as_deref() == Some(ls))
        && local_project_id
            .is_some_and(|lp| !target.project_id.is_empty() && lp != target.project_id);

    // Rule 1: Exact filename (current or canonical originalFilename)
    if exact_filename_conflict {
        return Some((DuplicateMatchReason::ExactFilename, true));
    }

    // Rule 2: Exact project_id match requiring a recognized provider source
    if let (Some(ref ls), Some(lp)) = (&local_source_recognized, local_project_id) {
        if target_source_recognized.as_deref() == Some(ls.as_str())
            && !target.project_id.is_empty()
            && lp == target.project_id
        {
            return Some((DuplicateMatchReason::ProjectId, false));
        }

        if !same_platform_different_project
            && target
                .known_platform_ids
                .contains(&(ls.clone(), lp.to_string()))
        {
            return Some((DuplicateMatchReason::ProjectId, false));
        }
    }

    // Rule 3: Exact mod_id / slug match (NEVER inspects `provides`)
    if let Some(mod_id) = local
        .mod_id
        .as_deref()
        .map(normalize_id)
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
            .map(normalize_id)
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
    let instance_lock = get_instance_mod_install_lock(&request.version_id).await;
    let _instance_guard = instance_lock.lock().await;
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
            let base_name = strip_ascii_suffix_ignore_case(&fname, ".disable").unwrap_or(&fname);
            manifest_records.insert(normalize_filename(base_name), record);
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

        let Some((actual_filename, enabled)) = parse_mod_filename(&filename) else {
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
        let db = match translations_db_path {
            Some(path) => TranslationDb::open_at(path)
                .map_err(|e| format!("Failed to open translation database: {}", e))?,
            None => TranslationDb::open_default()
                .map_err(|e| format!("Failed to open translation database: {}", e))?,
        };

        let hints = db.lookup_mod_identity(&request.source, &request.project_id, &request.slug)?;
        let target = TargetIdentity::from_request(&request, hints.as_ref());

        let parser = ModParser::new(&base_dir_buf);
        let mut cache_entries = parser.load_all_cache();
        let mut duplicates = Vec::new();

        for file in scanned_files {
            let fingerprint = compute_file_and_path_fingerprint(&file.path)?;
            let cache_key = format!(
                "{}_{}",
                build_mod_cache_key(&file.actual_filename, file.size, file.mtime, &loader_type),
                fingerprint
            );
            let rec_opt = manifest_records.get(&normalize_filename(&file.actual_filename));
            let original_filename = rec_opt
                .and_then(|r| r.original_filename.clone())
                .or_else(|| Some(strip_known_zh_prefixes(&file.actual_filename)));

            let hash_map = rec_opt
                .and_then(|r| r.hash_sha1.as_deref())
                .and_then(|sha1| db.get_hash_mapping(sha1))
                .filter(|h| h.category.trim().eq_ignore_ascii_case("mod"));

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

            // Parse every uncached JAR whose Phase-A metadata did not already match,
            // regardless of filename, so manually renamed duplicates are never missed.
            if cached_meta.is_none() && evaluate_duplicate_match(&target, &candidate).is_none() {
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
    use crate::core::translation::{HashMappingEntry, TranslationEntry, TRUST_VERIFIED};
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
        let options = SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        for (name, content) in entries {
            zip.start_file(*name, options).unwrap();
            zip.write_all(content).unwrap();
        }
        zip.finish().unwrap();
    }

    fn setup_minimal_instance(base_dir: &Path, inst_id: &str) -> PathBuf {
        let inst_dir = base_dir.join("versions").join(inst_id);
        let mods_dir = inst_dir.join("mods");
        fs::create_dir_all(&mods_dir).unwrap();
        fs::write(
            inst_dir.join(format!("{}.json", inst_id)),
            format!(r#"{{"id":"{}","inheritsFrom":"1.20.1"}}"#, inst_id),
        )
        .unwrap();
        fs::write(
            inst_dir.join("dlml.json"),
            r#"{"mcVersion":"1.20.1","loaderType":"Fabric"}"#,
        )
        .unwrap();
        mods_dir
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

        // Punctuation must remain distinct in ID normalization
        assert_ne!(normalize_id("foo-bar"), normalize_id("foobar"));
        assert_ne!(normalize_id("foo.bar"), normalize_id("foobar"));
        assert_ne!(normalize_id("foo-bar"), normalize_id("foo_bar"));
        assert_eq!(normalize_id("  Litematica-Printer "), "litematica-printer");

        // Case-insensitive JAR and .disable parsing
        assert_eq!(
            parse_mod_filename("LITEMATICA.JAR"),
            Some(("LITEMATICA.JAR".to_string(), true))
        );
        assert_eq!(
            parse_mod_filename("Litematica-0.15.3.Jar.DISABLE"),
            Some(("Litematica-0.15.3.Jar".to_string(), false))
        );
        assert_eq!(parse_mod_filename("notes.txt"), None);
        assert_eq!(parse_mod_filename("readme.txt.disable"), None);
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
                &make_local(
                    "Steam_Rails-1.6.4.jar",
                    Some("railways"),
                    Some("Create: Steam 'n' Rails")
                )
            ),
            None
        );
        assert_eq!(
            evaluate_duplicate_match(
                &create_target,
                &make_local(
                    "createaddition-1.20.1.jar",
                    Some("createaddition"),
                    Some("Create Crafts & Additions")
                )
            ),
            None
        );
        assert_eq!(
            evaluate_duplicate_match(
                &create_target,
                &make_local(
                    "flywheel-forge-1.20.1-0.6.10.jar",
                    Some("flywheel"),
                    Some("Flywheel")
                )
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
                &make_local(
                    "sodium-extra-0.5.4.jar",
                    Some("sodium-extra"),
                    Some("Sodium Extra")
                )
            ),
            None
        );
        assert_eq!(
            evaluate_duplicate_match(
                &sodium_target,
                &make_local(
                    "reeses_sodium_options-1.7.2.jar",
                    Some("reeses-sodium-options"),
                    Some("Reese's Sodium Options")
                )
            ),
            None
        );
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
                &make_local(
                    "MekanismGenerators-1.20.1.jar",
                    Some("mekanismgenerators"),
                    Some("Mekanism: Generators")
                )
            ),
            None
        );

        // 5. JEI vs JEI Integration
        let jei_target = TargetIdentity::from_request(
            &make_req(
                "curseforge",
                "238222",
                "jei",
                "Just Enough Items (JEI)",
                None,
            ),
            None,
        );
        assert_eq!(
            evaluate_duplicate_match(
                &jei_target,
                &make_local(
                    "jeiintegration_1.20.1.jar",
                    Some("jeiintegration"),
                    Some("JEI Integration")
                )
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

        // 7. Distinct punctuation in mod_id must NOT collide
        let punct_target = TargetIdentity::from_request(
            &make_req("modrinth", "projPunct", "foo-bar", "Foo-Bar", None),
            None,
        );
        assert_eq!(
            evaluate_duplicate_match(
                &punct_target,
                &make_local("foobar.jar", Some("foobar"), Some("Foobar"))
            ),
            None
        );
        assert_eq!(
            evaluate_duplicate_match(
                &punct_target,
                &make_local("foo_bar.jar", Some("foo_bar"), Some("Foo Underscore Bar"))
            ),
            None
        );

        // 8. Missing or unrecognized source must NOT match by project_id
        assert_eq!(
            evaluate_duplicate_match(
                &litematica_target,
                &LocalModCandidate {
                    filename: "unrelated.jar".to_string(),
                    source: None,
                    project_id: Some("bEpr0Arc".to_string()),
                    ..Default::default()
                }
            ),
            None
        );
        assert_eq!(
            evaluate_duplicate_match(
                &litematica_target,
                &LocalModCandidate {
                    filename: "unrelated.jar".to_string(),
                    source: Some("custom".to_string()),
                    project_id: Some("bEpr0Arc".to_string()),
                    ..Default::default()
                }
            ),
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
        let mods_dir = setup_minimal_instance(base_dir, inst_id);

        // Setup custom TranslationDb for cross-platform Iris matching + non-mod hash guard
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

        // 1. Uncached manual old Litematica JAR with uppercase extension & renamed unrelated filename
        write_test_jar(
            &mods_dir.join("Litematica-Fabric-1.20.1-0.15.3.Jar.DISABLE"),
            &[(
                "fabric.mod.json",
                br#"{"schemaVersion":1,"id":"litematica","name":"Litematica","version":"0.15.3"}"#,
            )],
        );
        write_test_jar(
            &mods_dir.join("renamed_manual_999.JAR"),
            &[(
                "fabric.mod.json",
                br#"{"schemaVersion":1,"id":"litematica","name":"Litematica","version":"0.15.2"}"#,
            )],
        );

        // 2. Uncached Litematica Printer JAR -> MUST be parsed & cached yet NOT flagged
        write_test_jar(
            &mods_dir.join("litematica-printer-26.3-3.2.3.jar"),
            &[(
                "fabric.mod.json",
                br#"{"schemaVersion":1,"id":"litematica-printer","name":"Litematica Printer","version":"3.2.3"}"#,
            )],
        );

        // 3. Sodium JAR whose hash in file_hash_map belongs to a non-mod (`resourcepack`) row -> must be ignored
        let sodium_path = mods_dir.join("sodium-fabric-0.5.8.jar");
        write_test_jar(
            &sodium_path,
            &[(
                "fabric.mod.json",
                br#"{"schemaVersion":1,"id":"sodium","name":"Sodium","version":"0.5.8"}"#,
            )],
        );
        let sodium_sha1 = crate::downloader::download::compute_sha1_sync(&sodium_path).unwrap();
        db.save_hash_mappings(&[HashMappingEntry {
            sha1: sodium_sha1.clone(),
            cf_murmur2: None,
            source: "modrinth".to_string(),
            project_id: "bEpr0Arc".to_string(),
            slug: "litematica".to_string(),
            version_id: None,
            category: "resourcepack".to_string(),
            raw_name: Some("Litematica".to_string()),
        }])
        .unwrap();

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
        manifest.assets.insert(
            "mods/sodium-fabric-0.5.8.jar".to_string(),
            AssetRecord {
                category: AssetCategory::Mod,
                source_type: AssetSourceType::LocalImport,
                project_id: None,
                version_id: None,
                download_url: None,
                hash_sha1: Some(sodium_sha1),
                hash_sha512: None,
                size: None,
                enabled: true,
                managed_by_modpack: false,
                original_filename: Some("sodium-fabric-0.5.8.jar".to_string()),
                original_rel_path: None,
                display_name_zh: None,
            },
        );
        fs::write(
            inst_dir.join("assets.json"),
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();

        // Test A: Installing Litematica detects BOTH disabled uppercase Litematica and unrelated-name manual JAR,
        // caches Printer (with mod_id="litematica-printer") without flagging it, and ignores non-mod hash mapping.
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

        assert_eq!(res_litematica.duplicates.len(), 2);
        assert!(!res_litematica.has_exact_filename_conflict);
        assert_eq!(
            res_litematica.duplicates[0].filename,
            "Litematica-Fabric-1.20.1-0.15.3.Jar"
        );
        assert!(!res_litematica.duplicates[0].enabled);
        assert_eq!(
            res_litematica.duplicates[0].match_reason,
            DuplicateMatchReason::ModId
        );
        assert_eq!(res_litematica.duplicates[1].filename, "renamed_manual_999.JAR");
        assert!(res_litematica.duplicates[1].enabled);
        assert_eq!(
            res_litematica.duplicates[1].match_reason,
            DuplicateMatchReason::ModId
        );

        let cache = ModParser::new(base_dir).load_all_cache();
        assert!(
            cache.values().any(|m| m.mod_id.as_deref() == Some("litematica")),
            "litematica should be cached after parse"
        );
        assert!(
            cache
                .values()
                .any(|m| m.mod_id.as_deref() == Some("litematica-printer")),
            "litematica-printer MUST be parsed & cached while remaining absent from duplicates"
        );

        // Test B: Installing CurseForge Iris Shaders matches Modrinth Iris via TranslationDb cross-platform ID
        let res_iris = check_duplicate_mods_in_base_with_db(
            base_dir,
            Some(db_path.clone()),
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
        assert_eq!(
            res_iris.duplicates[0].match_reason,
            DuplicateMatchReason::ProjectId
        );
        assert!(!res_iris.has_exact_filename_conflict);

        // Test C: Translation DB open error propagates cleanly instead of silently succeeding
        let bad_db_dir = dir.path().join("db_is_a_directory");
        fs::create_dir_all(&bad_db_dir).unwrap();
        let db_err = check_duplicate_mods_in_base_with_db(
            base_dir,
            Some(bad_db_dir),
            CheckDuplicateModsRequest {
                version_id: inst_id.to_string(),
                source: "modrinth".to_string(),
                project_id: "bEpr0Arc".to_string(),
                slug: "litematica".to_string(),
                title: "Litematica".to_string(),
                target_filename: None,
            },
        )
        .await
        .expect_err("directory as translation db path must fail duplicate check");
        assert!(db_err.contains("translation database"));
    }

    #[tokio::test]
    async fn test_cache_key_distinguishes_replaced_same_size_same_mtime_and_other_instance_jars() {
        let dir = tempdir().unwrap();
        let base_dir = dir.path();
        let mods_a = setup_minimal_instance(base_dir, "inst-a");
        let mods_b = setup_minimal_instance(base_dir, "inst-b");

        let db_path = dir.path().join("translations.db");
        let _db = TranslationDb::open_at(db_path.clone()).unwrap();

        let jar_a = mods_a.join("mod.jar");
        let jar_b = mods_b.join("mod.jar");

        // Pad JSON strings so both JARs have identical uncompressed/compressed byte length
        let content_other = br#"{"schemaVersion":1,"id":"alpha_mod","name":"Alpha Mod","version":"1.0.0"}"#;
        let content_target = br#"{"schemaVersion":1,"id":"litematic","name":"Litematic","version":"1.0.0"}"#;
        assert_eq!(content_other.len(), content_target.len());

        write_test_jar(&jar_a, &[("fabric.mod.json", content_other)]);
        write_test_jar(&jar_b, &[("fabric.mod.json", content_target)]);

        let size_a = fs::metadata(&jar_a).unwrap().len();
        let size_b = fs::metadata(&jar_b).unwrap().len();
        assert_eq!(size_a, size_b);

        // Populate cache for inst-a (alpha_mod) and verify inst-b (litematic) does NOT reuse inst-a's cache
        let res_a_1 = check_duplicate_mods_in_base_with_db(
            base_dir,
            Some(db_path.clone()),
            CheckDuplicateModsRequest {
                version_id: "inst-a".to_string(),
                source: "modrinth".to_string(),
                project_id: "pTarget".to_string(),
                slug: "litematic".to_string(),
                title: "Litematic".to_string(),
                target_filename: None,
            },
        )
        .await
        .unwrap();
        assert!(res_a_1.duplicates.is_empty());

        let res_b = check_duplicate_mods_in_base_with_db(
            base_dir,
            Some(db_path.clone()),
            CheckDuplicateModsRequest {
                version_id: "inst-b".to_string(),
                source: "modrinth".to_string(),
                project_id: "pTarget".to_string(),
                slug: "litematic".to_string(),
                title: "Litematic".to_string(),
                target_filename: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(res_b.duplicates.len(), 1);
        assert_eq!(res_b.duplicates[0].match_reason, DuplicateMatchReason::ModId);

        // Now replace jar_a in place preserving the exact same byte length & original mtime
        let orig_modified = fs::metadata(&jar_a).unwrap().modified().unwrap();
        write_test_jar(&jar_a, &[("fabric.mod.json", content_target)]);
        File::options()
            .write(true)
            .open(&jar_a)
            .unwrap()
            .set_modified(orig_modified)
            .unwrap();
        assert_eq!(fs::metadata(&jar_a).unwrap().len(), size_a);

        let res_a_2 = check_duplicate_mods_in_base_with_db(
            base_dir,
            Some(db_path),
            CheckDuplicateModsRequest {
                version_id: "inst-a".to_string(),
                source: "modrinth".to_string(),
                project_id: "pTarget".to_string(),
                slug: "litematic".to_string(),
                title: "Litematic".to_string(),
                target_filename: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(res_a_2.duplicates.len(), 1);
        assert_eq!(res_a_2.duplicates[0].match_reason, DuplicateMatchReason::ModId);
    }
}
