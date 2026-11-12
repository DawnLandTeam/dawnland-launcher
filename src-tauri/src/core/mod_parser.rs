use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use tracing::debug;

/// Bump this version whenever `ModParser` metadata extraction semantics change so
/// stale SQLite cache entries from older versions are automatically bypassed.
pub const MOD_PARSER_CACHE_VERSION: &str = "v3";
const MOD_CACHE_SCHEMA_VERSION: i32 = 3;

/// Cap metadata entry reads inside mod archives to avoid excessive memory usage on malformed JARs.
const MAX_METADATA_READ_BYTES: u64 = 2 * 1024 * 1024;
const MAX_ICON_READ_BYTES: u64 = 4 * 1024 * 1024;
const MAX_NESTED_JAR_BYTES: u64 = 32 * 1024 * 1024;
const MAX_JIJ_DEPTH: usize = 3;

/// Build a consistent SQLite cache key for a mod JAR under a specific target loader.
pub fn build_mod_cache_key(filename: &str, size: u64, mtime: u64, loader_type: &str) -> String {
    format!(
        "{}_{}_{}_{}_{}",
        MOD_PARSER_CACHE_VERSION, filename, size, mtime, loader_type
    )
}

#[derive(Debug, Clone, Default)]
pub struct ModMetadata {
    pub mod_id: Option<String>,
    pub name: Option<String>,
    pub version: Option<String>,
    pub has_icon: bool,
    /// Declared dependencies (mod ids). Used by prelaunch checks.
    pub depends: Vec<String>,
    /// Mod IDs provided by this mod (e.g. submodules in fabric API).
    pub provides: Vec<String>,
    /// Metadata source loader detected while parsing (e.g. "Forge", "NeoForge", "Fabric", "Quilt").
    pub detected_loader: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct JarManifestMarkers {
    pub is_forge_native: bool,
    pub is_til_multiversion: bool,
    pub implementation_title: Option<String>,
    pub implementation_version: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct JarArchiveFlags {
    pub has_fabric_json: bool,
    pub has_quilt_json: bool,
    pub has_forge_toml: bool,
    pub has_neoforge_toml: bool,
    pub has_mcmod_info: bool,
    pub has_forge_spi: bool,
    pub has_neoforge_spi: bool,
    pub manifest_entry_name: Option<String>,
    pub jarjar_metadata_entry: Option<String>,
    pub jarjar_paths: Vec<String>,
    pub jars_dir_paths: Vec<String>,
}

/// Unfold RFC-style JAR `MANIFEST.MF` continuation lines and extract Forge/TIL markers.
pub(crate) fn parse_jar_manifest_markers(content: &str) -> JarManifestMarkers {
    let mut markers = JarManifestMarkers::default();
    let unfolded = content.replace("\r\n ", "").replace("\n ", "");

    for raw_line in unfolded.lines() {
        let line = raw_line.trim();
        let Some((key, val)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim();
        let val = val.trim();
        if val.is_empty() {
            continue;
        }

        if key.eq_ignore_ascii_case("TILMultiversionMods")
            || key.eq_ignore_ascii_case("TILMultiversionCoreMods")
        {
            markers.is_til_multiversion = true;
            markers.is_forge_native = true;
        } else if key.eq_ignore_ascii_case("FMLModType")
            || key.eq_ignore_ascii_case("FMLCorePlugin")
            || key.eq_ignore_ascii_case("FMLCorePluginContainsFMLMod")
        {
            markers.is_forge_native = true;
        } else if key.eq_ignore_ascii_case("TweakClass") {
            let lower = val.to_lowercase();
            if lower.contains("fml") || lower.contains("forge") {
                markers.is_forge_native = true;
            }
        } else if key.eq_ignore_ascii_case("Implementation-Title") {
            markers.implementation_title = Some(val.to_string());
        } else if key.eq_ignore_ascii_case("Implementation-Version") {
            markers.implementation_version = Some(val.to_string());
        }
    }

    markers
}

/// Scan a JAR archive's central directory once to detect loader metadata and SPI markers.
pub(crate) fn scan_jar_archive_flags<R: Read + std::io::Seek>(
    archive: &zip::ZipArchive<R>,
) -> JarArchiveFlags {
    let mut flags = JarArchiveFlags::default();

    for raw_name in archive.file_names() {
        let normalized = raw_name.replace('\\', "/");
        let name = normalized.trim_start_matches('/');
        let lower = name.to_lowercase();

        if name == "fabric.mod.json" {
            flags.has_fabric_json = true;
        } else if name == "quilt.mod.json" {
            flags.has_quilt_json = true;
        } else if lower == "meta-inf/mods.toml" {
            flags.has_forge_toml = true;
        } else if lower == "meta-inf/neoforge.mods.toml" {
            flags.has_neoforge_toml = true;
        } else if name == "mcmod.info" {
            flags.has_mcmod_info = true;
        } else if lower == "meta-inf/manifest.mf" && flags.manifest_entry_name.is_none() {
            flags.manifest_entry_name = Some(raw_name.to_string());
        } else if lower == "meta-inf/jarjar/metadata.json" && flags.jarjar_metadata_entry.is_none() {
            flags.jarjar_metadata_entry = Some(raw_name.to_string());
        } else if lower == "meta-inf/coremods.json"
            || lower.starts_with("meta-inf/services/net.minecraftforge.forgespi.")
            || lower.starts_with("meta-inf/services/cpw.mods.modlauncher.serviceapi.")
        {
            flags.has_forge_spi = true;
        } else if lower.starts_with("meta-inf/services/net.neoforged.neoforgespi.") {
            flags.has_neoforge_spi = true;
        } else if lower.starts_with("meta-inf/jarjar/") && lower.ends_with(".jar") {
            flags.jarjar_paths.push(raw_name.to_string());
        } else if lower.starts_with("meta-inf/jars/") && lower.ends_with(".jar") {
            flags.jars_dir_paths.push(raw_name.to_string());
        }
    }

    flags
}

/// Read `META-INF/MANIFEST.MF` from an archive if present and parse Forge/TIL markers.
pub(crate) fn read_jar_manifest_markers(
    archive: &mut zip::ZipArchive<File>,
    manifest_entry_name: Option<&str>,
) -> JarManifestMarkers {
    let candidates = [manifest_entry_name, Some("META-INF/MANIFEST.MF")];
    for candidate in candidates.into_iter().flatten() {
        if let Ok(mut f) = archive.by_name(candidate) {
            let mut content = String::new();
            if f.by_ref()
                .take(MAX_METADATA_READ_BYTES)
                .read_to_string(&mut content)
                .is_ok()
            {
                return parse_jar_manifest_markers(&content);
            }
        }
    }
    JarManifestMarkers::default()
}

fn read_archive_entry_string<R: Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    entry_name: &str,
) -> Option<String> {
    let mut f = archive.by_name(entry_name).ok()?;
    let mut content = String::new();
    f.by_ref()
        .take(MAX_METADATA_READ_BYTES)
        .read_to_string(&mut content)
        .ok()?;
    Some(content)
}

fn read_nested_jar_bytes<R: Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    entry_name: &str,
) -> Option<Vec<u8>> {
    let mut nested_f = archive.by_name(entry_name).ok()?;
    let mut buf = Vec::new();
    nested_f
        .by_ref()
        .take(MAX_NESTED_JAR_BYTES)
        .read_to_end(&mut buf)
        .ok()?;
    (!buf.is_empty()).then_some(buf)
}

fn normalize_mcmod_dep(raw: &str, only_explicit_required: bool) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lower = trimmed.to_lowercase();
    if lower.starts_with("before:") || lower.starts_with("after:") {
        return None;
    }
    let rest = if let Some(r) = trimmed
        .strip_prefix("required-after:")
        .or_else(|| trimmed.strip_prefix("required-before:"))
    {
        r
    } else if only_explicit_required {
        return None;
    } else {
        trimmed
    };
    let mod_id = rest.split('@').next().unwrap_or(rest).trim();
    if mod_id.is_empty() {
        None
    } else {
        Some(mod_id.to_string())
    }
}

pub struct ModParser {
    cache_dir: PathBuf,
    db_path: PathBuf,
    icons_dir: PathBuf,
}

impl ModParser {
    pub fn new(base_dir: &Path) -> Self {
        let cache_dir = base_dir.join(".mod_cache");
        let icons_dir = cache_dir.join("icons");
        let db_path = cache_dir.join("cache.db");

        if !icons_dir.exists() {
            let _ = fs::create_dir_all(&icons_dir);
        }

        let parser = Self {
            cache_dir,
            db_path,
            icons_dir,
        };

        // Initialize DB and run migration
        let _ = parser.init_db();

        parser
    }

    fn init_db(&self) -> rusqlite::Result<()> {
        let conn = rusqlite::Connection::open(&self.db_path)?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS mod_cache (
                cache_key TEXT PRIMARY KEY,
                mod_id TEXT,
                name TEXT,
                version TEXT,
                has_icon INTEGER,
                depends TEXT DEFAULT '[]',
                provides TEXT DEFAULT '[]',
                detected_loader TEXT
            )",
            [],
        )?;

        for alter_sql in [
            "ALTER TABLE mod_cache ADD COLUMN depends TEXT DEFAULT '[]'",
            "ALTER TABLE mod_cache ADD COLUMN provides TEXT DEFAULT '[]'",
            "ALTER TABLE mod_cache ADD COLUMN detected_loader TEXT",
        ] {
            if let Err(e) = conn.execute(alter_sql, []) {
                if !e.to_string().contains("duplicate column name") {
                    tracing::warn!("Migration error on mod_cache ({}): {}", alter_sql, e);
                }
            }
        }

        // Invalidate stale cache entries whenever parser metadata schema changes
        let user_version: i32 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap_or(0);
        if user_version < MOD_CACHE_SCHEMA_VERSION {
            let _ = conn.execute("DELETE FROM mod_cache", []);
            let _ = conn.execute(&format!("PRAGMA user_version = {}", MOD_CACHE_SCHEMA_VERSION), []);
        }

        // Migration from old cache.json
        let json_path = self.cache_dir.join("cache.json");
        if json_path.exists() {
            if let Ok(content) = fs::read_to_string(&json_path) {
                #[derive(serde::Deserialize)]
                struct OldModMetadata {
                    mod_id: Option<String>,
                    name: Option<String>,
                    version: Option<String>,
                    has_icon: bool,
                }
                #[derive(serde::Deserialize)]
                struct OldModCache {
                    entries: std::collections::HashMap<String, OldModMetadata>,
                }

                if let Ok(cache) = serde_json::from_str::<OldModCache>(&content) {
                    let mut stmt = conn.prepare("INSERT OR IGNORE INTO mod_cache (cache_key, mod_id, name, version, has_icon) VALUES (?, ?, ?, ?, ?)")?;
                    for (k, v) in cache.entries {
                        let _ = stmt.execute(rusqlite::params![
                            k,
                            v.mod_id,
                            v.name,
                            v.version,
                            if v.has_icon { 1 } else { 0 }
                        ]);
                    }
                }
            }
            let _ = fs::remove_file(json_path);
        }

        Ok(())
    }

    pub fn load_all_cache(&self) -> HashMap<String, ModMetadata> {
        let mut map = HashMap::new();
        if let Ok(conn) = rusqlite::Connection::open(&self.db_path) {
            if let Ok(mut stmt) = conn.prepare(
                "SELECT cache_key, mod_id, name, version, has_icon, depends, provides, detected_loader FROM mod_cache",
            ) {
                if let Ok(mut rows) = stmt.query([]) {
                    while let Ok(Some(row)) = rows.next() {
                        if let Ok(key) = row.get::<_, String>(0) {
                            let depends_str: String =
                                row.get(5).unwrap_or_else(|_| "[]".to_string());
                            let provides_str: String =
                                row.get(6).unwrap_or_else(|_| "[]".to_string());
                            map.insert(
                                key,
                                ModMetadata {
                                    mod_id: row.get(1).ok(),
                                    name: row.get(2).ok(),
                                    version: row.get(3).ok(),
                                    has_icon: row.get::<_, i32>(4).unwrap_or(0) == 1,
                                    depends: serde_json::from_str(&depends_str).unwrap_or_default(),
                                    provides: serde_json::from_str(&provides_str)
                                        .unwrap_or_default(),
                                    detected_loader: row.get(7).ok(),
                                },
                            );
                        }
                    }
                }
            }
        }
        map
    }

    pub fn set_cache_entry(&self, key: &str, meta: &ModMetadata) {
        if let Ok(conn) = rusqlite::Connection::open(&self.db_path) {
            let depends_str =
                serde_json::to_string(&meta.depends).unwrap_or_else(|_| "[]".to_string());
            let provides_str =
                serde_json::to_string(&meta.provides).unwrap_or_else(|_| "[]".to_string());
            if let Err(e) = conn.execute(
                "INSERT OR REPLACE INTO mod_cache (cache_key, mod_id, name, version, has_icon, depends, provides, detected_loader) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                rusqlite::params![
                    key,
                    meta.mod_id,
                    meta.name,
                    meta.version,
                    if meta.has_icon { 1 } else { 0 },
                    depends_str,
                    provides_str,
                    meta.detected_loader
                ],
            ) {
                tracing::warn!("Failed to insert cache entry for {}: {}", key, e);

                // Fallback for missing columns if migration failed
                let _ = conn.execute(
                    "INSERT OR REPLACE INTO mod_cache (cache_key, mod_id, name, version, has_icon) VALUES (?, ?, ?, ?, ?)",
                    rusqlite::params![
                        key,
                        meta.mod_id,
                        meta.name,
                        meta.version,
                        if meta.has_icon { 1 } else { 0 }
                    ],
                );
            }
        }
    }

    pub fn get_icon_path(&self, cache_key: &str) -> PathBuf {
        let sanitized: String = cache_key
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let safe_name = sanitized.trim_matches('.');
        let final_name = if safe_name.is_empty() {
            "unknown_mod"
        } else {
            safe_name
        };
        self.icons_dir.join(format!("{}.png", final_name))
    }

    fn extract_icon_from_archive(
        &self,
        archive: &mut zip::ZipArchive<File>,
        icon_path: &str,
        icon_name: &str,
    ) -> bool {
        let clean_path = icon_path.trim().trim_start_matches('/');
        if clean_path.is_empty() {
            return false;
        }
        let dest_path = self.get_icon_path(icon_name);
        if dest_path.exists() {
            return true;
        }
        if let Ok(mut icon_file) = archive.by_name(clean_path) {
            let mut buffer = Vec::new();
            if icon_file
                .by_ref()
                .take(MAX_ICON_READ_BYTES)
                .read_to_end(&mut buffer)
                .is_ok()
                && !buffer.is_empty()
                && fs::write(dest_path, &buffer).is_ok()
            {
                return true;
            }
        }
        false
    }

    fn parse_forge_toml_entries(
        &self,
        archive: &mut zip::ZipArchive<File>,
        meta: &mut ModMetadata,
        cache_key: &str,
        prefer_neoforge: bool,
    ) -> Option<&'static str> {
        let candidates: [(&str, &str); 2] = if prefer_neoforge {
            [
                ("META-INF/neoforge.mods.toml", "NeoForge"),
                ("META-INF/mods.toml", "Forge"),
            ]
        } else {
            [
                ("META-INF/mods.toml", "Forge"),
                ("META-INF/neoforge.mods.toml", "NeoForge"),
            ]
        };

        for (entry_name, matched_loader) in candidates {
            let Some(content) = read_archive_entry_string(archive, entry_name) else {
                continue;
            };

            if !content.is_empty() {
                if let Ok(toml_val) = content.parse::<toml::Value>() {
                    if let Some(mods_arr) = toml_val.get("mods").and_then(|m| m.as_array()) {
                        if let Some(first_mod) = mods_arr.first() {
                            meta.mod_id = first_mod
                                .get("modId")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string());
                            meta.name = first_mod
                                .get("displayName")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string());
                            meta.version = first_mod
                                .get("version")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string());

                            if let Some(logo_path) =
                                first_mod.get("logoFile").and_then(|v| v.as_str())
                            {
                                let icon_name = meta.mod_id.as_deref().unwrap_or(cache_key);
                                if self.extract_icon_from_archive(archive, logo_path, icon_name) {
                                    meta.has_icon = true;
                                }
                            }
                        }

                        for m in mods_arr.iter().skip(1) {
                            if let Some(id) = m.get("modId").and_then(|v| v.as_str()) {
                                meta.provides.push(id.to_string());
                            }
                        }
                    }

                    if let Some(deps) = toml_val.get("dependencies").and_then(|d| d.as_table()) {
                        for (_, dep_list) in deps {
                            if let Some(arr) = dep_list.as_array() {
                                for dep in arr {
                                    let is_required = dep
                                        .get("mandatory")
                                        .and_then(|m| m.as_bool())
                                        .unwrap_or_else(|| {
                                            dep.get("type")
                                                .and_then(|t| t.as_str())
                                                .is_none_or(|t| t == "required")
                                        });
                                    if is_required {
                                        if let Some(dep_id) =
                                            dep.get("modId").and_then(|m| m.as_str())
                                        {
                                            meta.depends.push(dep_id.to_string());
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            return Some(matched_loader);
        }

        None
    }

    fn resolve_jij_nested_paths<R: Read + std::io::Seek>(
        archive: &mut zip::ZipArchive<R>,
        flags: &JarArchiveFlags,
    ) -> Vec<String> {
        let mut paths = Vec::new();
        let mut seen = std::collections::HashSet::new();

        let mut push_unique = |p: String| {
            let trimmed = p.trim().trim_start_matches('/').replace('\\', "/");
            if !trimmed.is_empty() && seen.insert(trimmed.to_lowercase()) {
                paths.push(trimmed);
            }
        };

        if let Some(ref meta_entry) = flags.jarjar_metadata_entry {
            if let Some(meta_str) = read_archive_entry_string(archive, meta_entry) {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&meta_str) {
                    if let Some(jars) = json.get("jars").and_then(|v| v.as_array()) {
                        for j in jars {
                            if let Some(p) = j.get("path").and_then(|v| v.as_str()) {
                                push_unique(p.to_string());
                            }
                        }
                    }
                }
            }
        }

        for p in &flags.jarjar_paths {
            push_unique(p.clone());
        }

        // Only scan `META-INF/jars/*.jar` directly when `metadata.json` is absent
        // (some universal JARs place Fabric-only nested JARs in `META-INF/jars/` alongside Forge ones
        // and rely on `metadata.json` to select the Forge-specific artifacts).
        if flags.jarjar_metadata_entry.is_none() {
            for p in &flags.jars_dir_paths {
                push_unique(p.clone());
            }
        }

        paths
    }

    fn collect_jarjar_provides<R: Read + std::io::Seek>(
        archive: &mut zip::ZipArchive<R>,
        flags: &JarArchiveFlags,
        meta: &mut ModMetadata,
        prefer_neoforge: bool,
        depth: usize,
    ) {
        if depth >= MAX_JIJ_DEPTH {
            return;
        }

        let inner_candidates = if prefer_neoforge {
            ["META-INF/neoforge.mods.toml", "META-INF/mods.toml"]
        } else {
            ["META-INF/mods.toml", "META-INF/neoforge.mods.toml"]
        };

        let nested_paths = Self::resolve_jij_nested_paths(archive, flags);
        for path in nested_paths {
            let Some(buf) = read_nested_jar_bytes(archive, &path) else {
                continue;
            };
            if let Ok(mut nested_archive) = zip::ZipArchive::new(std::io::Cursor::new(buf)) {
                let mut inner_s = None;
                for inner_name in inner_candidates {
                    if let Some(s) = read_archive_entry_string(&mut nested_archive, inner_name) {
                        inner_s = Some(s);
                        break;
                    }
                }
                if let Some(s) = inner_s.filter(|s| !s.is_empty()) {
                    if let Ok(inner_toml) = s.parse::<toml::Value>() {
                        if let Some(inner_mods) =
                            inner_toml.get("mods").and_then(|m| m.as_array())
                        {
                            for inner_mod in inner_mods {
                                if let Some(id) =
                                    inner_mod.get("modId").and_then(|v| v.as_str())
                                {
                                    meta.provides.push(id.to_string());
                                }
                            }
                        }
                    }
                }

                let inner_flags = scan_jar_archive_flags(&nested_archive);
                if inner_flags.jarjar_metadata_entry.is_some()
                    || !inner_flags.jarjar_paths.is_empty()
                    || !inner_flags.jars_dir_paths.is_empty()
                {
                    Self::collect_jarjar_provides(
                        &mut nested_archive,
                        &inner_flags,
                        meta,
                        prefer_neoforge,
                        depth + 1,
                    );
                }
            }
        }
    }

    fn parse_mcmod_info(
        &self,
        archive: &mut zip::ZipArchive<File>,
        meta: &mut ModMetadata,
        cache_key: &str,
    ) -> bool {
        let Some(content) = read_archive_entry_string(archive, "mcmod.info") else {
            return false;
        };
        if content.trim().is_empty() {
            return false;
        }
        let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) else {
            return true;
        };

        let entries_opt = json
            .as_array()
            .or_else(|| json.get("modList").and_then(|m| m.as_array()));
        let Some(entries) = entries_opt else {
            return true;
        };

        if let Some(first) = entries.first() {
            meta.mod_id = first
                .get("modid")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            meta.name = first
                .get("name")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            meta.version = first
                .get("version")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            if let Some(req_mods) = first.get("requiredMods").and_then(|v| v.as_array()) {
                for req in req_mods {
                    if let Some(raw) = req.as_str() {
                        if let Some(dep_id) = normalize_mcmod_dep(raw, false) {
                            meta.depends.push(dep_id);
                        }
                    }
                }
            }

            if let Some(deps) = first.get("dependencies").and_then(|v| v.as_array()) {
                for dep in deps {
                    if let Some(raw) = dep.as_str() {
                        if let Some(dep_id) = normalize_mcmod_dep(raw, true) {
                            meta.depends.push(dep_id);
                        }
                    }
                }
            }

            if let Some(logo_path) = first.get("logoFile").and_then(|v| v.as_str()) {
                let icon_name = meta.mod_id.as_deref().unwrap_or(cache_key);
                if self.extract_icon_from_archive(archive, logo_path, icon_name) {
                    meta.has_icon = true;
                }
            }
        }

        for extra in entries.iter().skip(1) {
            if let Some(id) = extra.get("modid").and_then(|v| v.as_str()) {
                meta.provides.push(id.to_string());
            }
        }

        true
    }

    fn parse_forge_family_meta(
        &self,
        archive: &mut zip::ZipArchive<File>,
        flags: &JarArchiveFlags,
        meta: &mut ModMetadata,
        cache_key: &str,
        prefer_neoforge: bool,
    ) -> bool {
        let matched_toml = self.parse_forge_toml_entries(archive, meta, cache_key, prefer_neoforge);

        if flags.jarjar_metadata_entry.is_some()
            || !flags.jarjar_paths.is_empty()
            || !flags.jars_dir_paths.is_empty()
        {
            Self::collect_jarjar_provides(archive, flags, meta, prefer_neoforge, 0);
        }

        let parsed_mcmod = if matched_toml.is_none() && flags.has_mcmod_info {
            self.parse_mcmod_info(archive, meta, cache_key)
        } else {
            false
        };

        let markers = if flags.manifest_entry_name.is_some() {
            read_jar_manifest_markers(archive, flags.manifest_entry_name.as_deref())
        } else {
            JarManifestMarkers::default()
        };

        let has_forge_evidence = matched_toml.is_some()
            || flags.jarjar_metadata_entry.is_some()
            || !flags.jarjar_paths.is_empty()
            || parsed_mcmod
            || flags.has_forge_spi
            || flags.has_neoforge_spi
            || markers.is_forge_native;

        if !has_forge_evidence {
            return false;
        }

        if meta.name.is_none() {
            meta.name = markers.implementation_title;
        }
        if meta.version.is_none() {
            meta.version = markers.implementation_version;
        }

        let detected = if let Some(toml_loader) = matched_toml {
            toml_loader
        } else if prefer_neoforge && flags.has_neoforge_spi && !flags.has_forge_spi {
            "NeoForge"
        } else {
            "Forge"
        };
        meta.detected_loader = Some(detected.to_string());

        true
    }

    fn parse_fabric_family_meta(
        &self,
        archive: &mut zip::ZipArchive<File>,
        meta: &mut ModMetadata,
        cache_key: &str,
        prefer_quilt: bool,
    ) -> bool {
        let candidates: [(&str, &str); 2] = if prefer_quilt {
            [("quilt.mod.json", "Quilt"), ("fabric.mod.json", "Fabric")]
        } else {
            [("fabric.mod.json", "Fabric"), ("quilt.mod.json", "Quilt")]
        };

        let mut loaded: Option<(String, &'static str)> = None;
        for (entry_name, loader_name) in candidates {
            if let Some(content) = read_archive_entry_string(archive, entry_name) {
                if !content.is_empty() {
                    loaded = Some((content, loader_name));
                    break;
                }
            }
        }

        let Some((content, detected_loader)) = loaded else {
            return false;
        };

        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
            meta.mod_id = json
                .get("id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            meta.name = json
                .get("name")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            meta.version = json
                .get("version")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            if let Some(depends) = json.get("depends").and_then(|v| v.as_object()) {
                meta.depends = depends.keys().cloned().collect();
            }

            if let Some(provides) = json.get("provides").and_then(|v| v.as_array()) {
                meta.provides = provides
                    .iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect();
            }

            if let Some(jars) = json.get("jars").and_then(|v| v.as_array()) {
                for j in jars {
                    if let Some(file_path) = j.get("file").and_then(|v| v.as_str()) {
                        let Some(buf) = read_nested_jar_bytes(archive, file_path) else {
                            continue;
                        };
                        if let Ok(mut nested_archive) =
                            zip::ZipArchive::new(std::io::Cursor::new(buf))
                        {
                            if let Some(inner_s) =
                                read_archive_entry_string(&mut nested_archive, "fabric.mod.json")
                            {
                                if let Ok(inner_json) =
                                    serde_json::from_str::<serde_json::Value>(&inner_s)
                                {
                                    if let Some(id) =
                                        inner_json.get("id").and_then(|v| v.as_str())
                                    {
                                        meta.provides.push(id.to_string());
                                    }
                                }
                            }
                        }
                    }
                }
            }

            if meta.mod_id.is_none() {
                if let Some(ql) = json.get("quilt_loader").and_then(|v| v.as_object()) {
                    meta.mod_id = ql
                        .get("id")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());

                    if let Some(depends) = ql.get("depends").and_then(|v| v.as_array()) {
                        for d in depends {
                            if d.as_object()
                                .and_then(|o| o.get("optional"))
                                .and_then(|v| v.as_bool())
                                == Some(true)
                            {
                                continue;
                            }
                            if let Some(id) = d
                                .as_object()
                                .and_then(|o| o.get("id"))
                                .and_then(|v| v.as_str())
                            {
                                meta.depends.push(id.to_string());
                            } else if let Some(s) = d.as_str() {
                                meta.depends.push(s.to_string());
                            }
                        }
                    }

                    if let Some(provides) = ql.get("provides").and_then(|v| v.as_array()) {
                        for p in provides {
                            if let Some(id) = p
                                .as_object()
                                .and_then(|o| o.get("id"))
                                .and_then(|v| v.as_str())
                            {
                                meta.provides.push(id.to_string());
                            } else if let Some(s) = p.as_str() {
                                meta.provides.push(s.to_string());
                            }
                        }
                    }

                    if let Some(metadata) = ql.get("metadata").and_then(|v| v.as_object()) {
                        meta.name = metadata
                            .get("name")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string());
                        meta.version = ql
                            .get("version")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string());
                    }
                }
            }

            if let Some(icon_path) = json.get("icon").and_then(|v| v.as_str()) {
                let icon_name = meta.mod_id.as_deref().unwrap_or(cache_key);
                if self.extract_icon_from_archive(archive, icon_path, icon_name) {
                    meta.has_icon = true;
                }
            }
        }

        meta.detected_loader = Some(detected_loader.to_string());
        true
    }

    pub fn parse_mod(
        &self,
        file_path: &Path,
        cache_key: &str,
        loader_type: Option<&str>,
    ) -> ModMetadata {
        let mut meta = ModMetadata::default();

        let file = match File::open(file_path) {
            Ok(f) => f,
            Err(e) => {
                debug!("Failed to open mod file {:?}: {}", file_path, e);
                return meta;
            }
        };

        let mut archive = match zip::ZipArchive::new(file) {
            Ok(a) => a,
            Err(e) => {
                debug!("Failed to read zip archive {:?}: {}", file_path, e);
                return meta;
            }
        };

        let flags = scan_jar_archive_flags(&archive);
        let is_neoforge = loader_type.is_some_and(|l| l.eq_ignore_ascii_case("neoforge"));
        let is_forge_family =
            is_neoforge || loader_type.is_some_and(|l| l.eq_ignore_ascii_case("forge"));
        let is_quilt = loader_type.is_some_and(|l| l.eq_ignore_ascii_case("quilt"));

        if is_forge_family {
            if !self.parse_forge_family_meta(&mut archive, &flags, &mut meta, cache_key, is_neoforge)
            {
                self.parse_fabric_family_meta(&mut archive, &mut meta, cache_key, is_quilt);
            }
        } else if !self.parse_fabric_family_meta(&mut archive, &mut meta, cache_key, is_quilt) {
            self.parse_forge_family_meta(&mut archive, &flags, &mut meta, cache_key, is_neoforge);
        }

        meta
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;
    use zip::write::SimpleFileOptions;

    fn write_test_jar(path: &Path, entries: &[(&str, &[u8])]) {
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
    fn test_mod_parser_cache_db() {
        let dir = tempdir().unwrap();
        let parser = ModParser::new(dir.path());

        let meta = ModMetadata {
            mod_id: Some("test_mod".to_string()),
            name: Some("Test Mod".to_string()),
            version: Some("1.0.0".to_string()),
            has_icon: true,
            depends: vec!["dep_a".to_string()],
            provides: vec!["sub_a".to_string()],
            detected_loader: Some("Forge".to_string()),
        };

        parser.set_cache_entry("hash123", &meta);

        let cache = parser.load_all_cache();
        assert_eq!(cache.len(), 1);
        let loaded = cache.get("hash123").unwrap();

        assert_eq!(loaded.mod_id, Some("test_mod".to_string()));
        assert_eq!(loaded.name, Some("Test Mod".to_string()));
        assert_eq!(loaded.version, Some("1.0.0".to_string()));
        assert!(loaded.has_icon);
        assert_eq!(loaded.depends, vec!["dep_a".to_string()]);
        assert_eq!(loaded.provides, vec!["sub_a".to_string()]);
        assert_eq!(loaded.detected_loader.as_deref(), Some("Forge"));
    }

    #[test]
    fn test_parse_universal_til_jar_under_forge_does_not_fallback_to_fabric() {
        let dir = tempdir().unwrap();
        let parser = ModParser::new(dir.path());
        let jar_path = dir.path().join("theimpossiblelibrary-0.4.7.jar");

        write_test_jar(
            &jar_path,
            &[
                (
                    "fabric.mod.json",
                    br#"{
                        "id": "theimpossibleloader",
                        "name": "The Impossible Loader",
                        "version": "0.4.7",
                        "depends": { "fabricloader": ">=0.14.0", "fabric": "*", "minecraft": ">=1.16.5" }
                    }"#,
                ),
                (
                    "mcmod.info",
                    br#"[{
                        "modid": "theimpossiblelibrary",
                        "name": "The Impossible Library",
                        "version": "0.4.7",
                        "mcversion": "1.12.2",
                        "dependencies": []
                    }]"#,
                ),
                (
                    "META-INF/services/net.minecraftforge.forgespi.locating.IModLocator",
                    b"mods.thecomputerizer.theimpossiblelibrary.forge.core.MultiVersionModLocator\n",
                ),
                (
                    "META-INF/MANIFEST.MF",
                    b"Manifest-Version: 1.0\r\nImplementation-Title: The Impossible Library\r\nImplementation-Version: 0.4.7\r\nTILMultiversionMods: mods.thecomputerizer.theimpossiblelibrary.api.commo\r\n n.TILCommonEntryPoint\r\n",
                ),
            ],
        );

        let forge_meta = parser.parse_mod(&jar_path, "til_forge", Some("Forge"));
        assert_eq!(forge_meta.mod_id.as_deref(), Some("theimpossiblelibrary"));
        assert_eq!(forge_meta.name.as_deref(), Some("The Impossible Library"));
        assert_eq!(forge_meta.version.as_deref(), Some("0.4.7"));
        assert!(forge_meta.depends.is_empty());
        assert_eq!(forge_meta.detected_loader.as_deref(), Some("Forge"));

        let fabric_meta = parser.parse_mod(&jar_path, "til_fabric", Some("Fabric"));
        assert_eq!(fabric_meta.mod_id.as_deref(), Some("theimpossibleloader"));
        assert_eq!(fabric_meta.name.as_deref(), Some("The Impossible Loader"));
        assert!(fabric_meta.depends.contains(&"fabric".to_string()));
        assert_eq!(fabric_meta.detected_loader.as_deref(), Some("Fabric"));
    }

    #[test]
    fn test_parse_til_dependent_manifest_only_jar_under_forge() {
        let dir = tempdir().unwrap();
        let parser = ModParser::new(dir.path());
        let jar_path = dir.path().join("musictriggers-7.0.4.jar");

        write_test_jar(
            &jar_path,
            &[(
                "META-INF/MANIFEST.MF",
                b"Manifest-Version: 1.0\r\nImplementation-Title: Music Triggers\r\nImplementation-Version: 7.0.4\r\nTILMultiversionMods: mods.thecomputerizer.musictriggers.api.common.MTCom\r\n monEntryPoint\r\n",
            )],
        );

        let meta = parser.parse_mod(&jar_path, "mt_forge", Some("forge"));
        assert_eq!(meta.name.as_deref(), Some("Music Triggers"));
        assert_eq!(meta.version.as_deref(), Some("7.0.4"));
        assert!(meta.depends.is_empty());
        assert_eq!(meta.detected_loader.as_deref(), Some("Forge"));
    }

    #[test]
    fn test_parse_dual_toml_respects_forge_vs_neoforge() {
        let dir = tempdir().unwrap();
        let parser = ModParser::new(dir.path());
        let jar_path = dir.path().join("dual-mod.jar");

        write_test_jar(
            &jar_path,
            &[
                (
                    "META-INF/mods.toml",
                    br#"[[mods]]
modId="dualmod"
displayName="Dual Mod Forge"
version="1.0.0"
[[dependencies.dualmod]]
modId="forge_only_dep"
mandatory=true
"#,
                ),
                (
                    "META-INF/neoforge.mods.toml",
                    br#"[[mods]]
modId="dualmod"
displayName="Dual Mod NeoForge"
version="1.0.0"
[[dependencies.dualmod]]
modId="neoforge_only_dep"
type="required"
"#,
                ),
            ],
        );

        let forge_meta = parser.parse_mod(&jar_path, "dual_forge", Some("Forge"));
        assert_eq!(forge_meta.name.as_deref(), Some("Dual Mod Forge"));
        assert_eq!(forge_meta.depends, vec!["forge_only_dep".to_string()]);
        assert_eq!(forge_meta.detected_loader.as_deref(), Some("Forge"));

        let neo_meta = parser.parse_mod(&jar_path, "dual_neo", Some("NeoForge"));
        assert_eq!(neo_meta.name.as_deref(), Some("Dual Mod NeoForge"));
        assert_eq!(neo_meta.depends, vec!["neoforge_only_dep".to_string()]);
        assert_eq!(neo_meta.detected_loader.as_deref(), Some("NeoForge"));
    }

    #[test]
    fn test_mcmod_info_dependency_filtering() {
        let dir = tempdir().unwrap();
        let parser = ModParser::new(dir.path());
        let jar_path = dir.path().join("legacy-1.12.2.jar");

        write_test_jar(
            &jar_path,
            &[(
                "mcmod.info",
                br#"[{
                    "modid": "legacy_mod",
                    "name": "Legacy Mod",
                    "version": "1.2.3",
                    "requiredMods": ["forge@[14.23.5,)", "baubles@[1.5.2,)", "after:jei"],
                    "dependencies": ["soft_hint", "required-after:cofhcore@[4.6,)"]
                }]"#,
            )],
        );

        let meta = parser.parse_mod(&jar_path, "legacy", Some("Forge"));
        assert_eq!(meta.mod_id.as_deref(), Some("legacy_mod"));
        assert_eq!(
            meta.depends,
            vec![
                "forge".to_string(),
                "baubles".to_string(),
                "cofhcore".to_string()
            ]
        );
    }

    fn build_jar_bytes(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut cursor = std::io::Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut cursor);
            let options = SimpleFileOptions::default();
            for (name, content) in entries {
                zip.start_file(*name, options).unwrap();
                zip.write_all(content).unwrap();
            }
            zip.finish().unwrap();
        }
        cursor.into_inner()
    }

    #[test]
    fn test_jij_metadata_json_and_recursive_nested_jars() {
        let dir = tempdir().unwrap();
        let parser = ModParser::new(dir.path());

        let leaf_jar_bytes = build_jar_bytes(&[(
            "META-INF/mods.toml",
            br#"[[mods]]
modId="leaf_submodule"
displayName="Leaf Submodule"
version="1.0.0"
"#,
        )]);

        let palladium_jar_bytes = build_jar_bytes(&[
            (
                "META-INF/mods.toml",
                br#"[[mods]]
modId="palladiumcore"
displayName="PalladiumCore"
version="1.20-2.0.0.0"
"#,
            ),
            (
                "META-INF/jarjar/metadata.json",
                br#"{"jars":[{"path":"META-INF/jars/leaf.jar"}]}"#,
            ),
            ("META-INF/jars/leaf.jar", &leaf_jar_bytes),
        ]);

        let unreferenced_jar_bytes = build_jar_bytes(&[(
            "META-INF/mods.toml",
            br#"[[mods]]
modId="unreferenced_mod"
version="1.0.0"
"#,
        )]);

        let gliders_jar = dir.path().join("gliders-forge-1.1.7.jar");
        write_test_jar(
            &gliders_jar,
            &[
                (
                    "META-INF/mods.toml",
                    br#"[[mods]]
modId="vc_gliders"
displayName="Gliders"
version="1.1.7"
[[dependencies.vc_gliders]]
modId="palladiumcore"
mandatory=true
"#,
                ),
                (
                    "META-INF/jarjar/metadata.json",
                    br#"{"jars":[{"path":"META-INF/jars/PalladiumCore-forge-1.20-2.0.0.0-forge.jar"}]}"#,
                ),
                (
                    "META-INF/jars/PalladiumCore-forge-1.20-2.0.0.0-forge.jar",
                    &palladium_jar_bytes,
                ),
                (
                    "META-INF/jars/unreferenced.jar",
                    &unreferenced_jar_bytes,
                ),
            ],
        );

        let meta = parser.parse_mod(&gliders_jar, "gliders_v3", Some("Forge"));
        assert_eq!(meta.mod_id.as_deref(), Some("vc_gliders"));
        assert_eq!(meta.name.as_deref(), Some("Gliders"));
        assert_eq!(meta.depends, vec!["palladiumcore".to_string()]);
        assert!(meta.provides.contains(&"palladiumcore".to_string()));
        assert!(meta.provides.contains(&"leaf_submodule".to_string()));
        assert!(!meta.provides.contains(&"unreferenced_mod".to_string()));
    }

    #[test]
    fn test_cache_db_invalidates_pre_v3_entries() {
        let dir = tempdir().unwrap();
        let cache_dir = dir.path().join(".mod_cache");
        fs::create_dir_all(&cache_dir).unwrap();
        let db_path = cache_dir.join("cache.db");

        {
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            conn.execute(
                "CREATE TABLE mod_cache (cache_key TEXT PRIMARY KEY, mod_id TEXT, name TEXT, version TEXT, has_icon INTEGER, depends TEXT DEFAULT '[]', provides TEXT DEFAULT '[]', detected_loader TEXT)",
                [],
            ).unwrap();
            conn.execute("PRAGMA user_version = 2", []).unwrap();
            conn.execute(
                "INSERT INTO mod_cache (cache_key, mod_id, name, version, has_icon, depends, provides, detected_loader) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                rusqlite::params![
                    "v2_gliders.jar_100_1_Forge",
                    "vc_gliders",
                    "Gliders",
                    "1.1.7",
                    0,
                    "[\"palladiumcore\"]",
                    "[]",
                    "Forge"
                ],
            ).unwrap();
        }

        let parser = ModParser::new(dir.path());
        assert!(parser.load_all_cache().is_empty());
    }
}
