use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tracing::debug;

static MOD_CACHE_WRITE_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

#[derive(Debug, Clone, Default)]
pub struct ModCachePruneStats {
    pub freed_bytes: u64,
    pub db_freed_bytes: u64,
    pub removed_entries: u64,
    pub removed_icons: u64,
}

fn open_db_with_timeout(db_path: &Path) -> rusqlite::Result<rusqlite::Connection> {
    let conn = rusqlite::Connection::open(db_path)?;
    conn.busy_timeout(Duration::from_secs(3))?;
    Ok(conn)
}

fn sanitize_icon_stem(raw: &str) -> String {
    let sanitized: String = raw
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
    if safe_name.is_empty() {
        "unknown_mod".to_string()
    } else {
        safe_name.to_string()
    }
}

/// Bump this version whenever `ModParser` metadata extraction semantics change so
/// stale SQLite cache entries from older versions are automatically bypassed.
pub const MOD_PARSER_CACHE_VERSION: &str = "v4";
const MOD_CACHE_SCHEMA_VERSION: i32 = 4;

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
    /// Chinese mod name declared inside the JAR's `assets/<mod_id>/lang/zh_cn.json` if present.
    pub zh_name_embedded: Option<String>,
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

fn ensure_mod_cache_schema(conn: &rusqlite::Connection) -> rusqlite::Result<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS mod_cache (
            cache_key TEXT PRIMARY KEY,
            mod_id TEXT,
            name TEXT,
            version TEXT,
            has_icon INTEGER,
            depends TEXT DEFAULT '[]',
            provides TEXT DEFAULT '[]',
            detected_loader TEXT,
            zh_name_embedded TEXT,
            unreferenced_since INTEGER
        )",
        [],
    )?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS mod_icon_retention (
            file_name TEXT PRIMARY KEY,
            unreferenced_since INTEGER NOT NULL
        )",
        [],
    )?;

    for alter_sql in [
        "ALTER TABLE mod_cache ADD COLUMN depends TEXT DEFAULT '[]'",
        "ALTER TABLE mod_cache ADD COLUMN provides TEXT DEFAULT '[]'",
        "ALTER TABLE mod_cache ADD COLUMN detected_loader TEXT",
        "ALTER TABLE mod_cache ADD COLUMN zh_name_embedded TEXT",
        "ALTER TABLE mod_cache ADD COLUMN unreferenced_since INTEGER",
    ] {
        if let Err(e) = conn.execute(alter_sql, []) {
            if !e.to_string().contains("duplicate column name") {
                tracing::warn!("Migration error on mod_cache ({}): {}", alter_sql, e);
            }
        }
    }

    Ok(())
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
        let _lock = MOD_CACHE_WRITE_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let conn = open_db_with_timeout(&self.db_path)?;
        ensure_mod_cache_schema(&conn)?;

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
        if let Ok(conn) = open_db_with_timeout(&self.db_path) {
            if let Ok(mut stmt) = conn.prepare(
                "SELECT cache_key, mod_id, name, version, has_icon, depends, provides, detected_loader, zh_name_embedded FROM mod_cache",
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
                                    zh_name_embedded: row.get(8).ok(),
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
        let _lock = MOD_CACHE_WRITE_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Ok(conn) = open_db_with_timeout(&self.db_path) {
            let depends_str =
                serde_json::to_string(&meta.depends).unwrap_or_else(|_| "[]".to_string());
            let provides_str =
                serde_json::to_string(&meta.provides).unwrap_or_else(|_| "[]".to_string());
            if let Err(e) = conn.execute(
                "INSERT OR REPLACE INTO mod_cache (cache_key, mod_id, name, version, has_icon, depends, provides, detected_loader, zh_name_embedded) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
                rusqlite::params![
                    key,
                    meta.mod_id,
                    meta.name,
                    meta.version,
                    if meta.has_icon { 1 } else { 0 },
                    depends_str,
                    provides_str,
                    meta.detected_loader,
                    meta.zh_name_embedded
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
        self.icons_dir
            .join(format!("{}.png", sanitize_icon_stem(cache_key)))
    }

    /// Collect active `mod_cache` key prefixes (`v4_<filename>_<size>_<mtime>_`) from all instances.
    fn collect_live_mod_key_prefixes(base_dir: &Path) -> Result<HashSet<String>, String> {
        let mut prefixes = HashSet::new();
        let versions_dir = base_dir.join("versions");
        if !versions_dir.exists() {
            return Ok(prefixes);
        }

        let version_entries = fs::read_dir(&versions_dir)
            .map_err(|e| format!("Failed to read versions directory for mod cache GC: {}", e))?;

        for ver_entry in version_entries {
            let ver_entry = ver_entry.map_err(|e| format!("Failed to inspect version entry: {}", e))?;
            let ver_path = ver_entry.path();
            if !ver_path.is_dir() {
                continue;
            }
            let mods_dir = ver_path.join("mods");
            if !mods_dir.exists() {
                continue;
            }

            let mod_entries = fs::read_dir(&mods_dir)
                .map_err(|e| format!("Failed to read mods directory {}: {}", mods_dir.display(), e))?;
            for mod_entry in mod_entries {
                let mod_entry = mod_entry.map_err(|e| format!("Failed to inspect mod entry: {}", e))?;
                let path = mod_entry.path();
                if !path.is_file() {
                    continue;
                }
                let filename = mod_entry.file_name().to_string_lossy().to_string();
                let actual_filename = if let Some(stripped) = filename.strip_suffix(".disable") {
                    if !stripped.ends_with(".jar") {
                        continue;
                    }
                    stripped
                } else if filename.ends_with(".jar") {
                    filename.as_str()
                } else {
                    continue;
                };

                let metadata = mod_entry.metadata().map_err(|e| {
                    format!("Failed to read mod metadata {}: {}", path.display(), e)
                })?;
                let mtime = metadata
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                prefixes.insert(format!(
                    "{}_{}_{}_{}_",
                    MOD_PARSER_CACHE_VERSION,
                    actual_filename,
                    metadata.len(),
                    mtime
                ));
            }
        }

        Ok(prefixes)
    }

    /// Prune unreferenced `cache.db` entries and orphaned icon PNGs under `<base_dir>/.mod_cache`.
    /// Prune unreferenced `cache.db` entries and orphaned icon PNGs under `<base_dir>/.mod_cache`.
    pub fn prune_unreferenced(
        base_dir: &Path,
        retention_secs: u64,
    ) -> Result<ModCachePruneStats, String> {
        let now_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        Self::prune_unreferenced_at(base_dir, retention_secs, now_secs)
    }

    fn prune_unreferenced_at(
        base_dir: &Path,
        retention_secs: u64,
        now_secs: u64,
    ) -> Result<ModCachePruneStats, String> {
        let cache_dir = base_dir.join(".mod_cache");
        let cache_meta = match fs::symlink_metadata(&cache_dir) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(ModCachePruneStats::default());
            }
            Err(e) => {
                return Err(format!(
                    "Failed to inspect mod cache directory {}: {}",
                    cache_dir.display(),
                    e
                ));
            }
        };
        if cache_meta.file_type().is_symlink() || !cache_meta.file_type().is_dir() {
            tracing::warn!(
                "Skipping mod cache GC because {} is a symlink or not a directory",
                cache_dir.display()
            );
            return Ok(ModCachePruneStats::default());
        }

        let live_prefixes = Self::collect_live_mod_key_prefixes(base_dir)?;
        let sanitized_prefixes: Vec<String> =
            live_prefixes.iter().map(|p| sanitize_icon_stem(p)).collect();

        let mut stats = ModCachePruneStats::default();
        let db_path = cache_dir.join("cache.db");
        let icons_dir = cache_dir.join("icons");
        let mut live_icon_names: HashSet<String> = HashSet::new();

        let _lock = MOD_CACHE_WRITE_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let db_meta_opt = match fs::symlink_metadata(&db_path) {
            Ok(m) => {
                if m.file_type().is_symlink() || !m.file_type().is_file() {
                    tracing::warn!(
                        "Skipping mod cache GC because {} is a symlink or not a regular file",
                        db_path.display()
                    );
                    return Ok(ModCachePruneStats::default());
                }
                Some(m)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => {
                return Err(format!(
                    "Failed to inspect mod cache DB {}: {}",
                    db_path.display(),
                    e
                ));
            }
        };

        let icons_dir_valid = match fs::symlink_metadata(&icons_dir) {
            Ok(m) => {
                if m.file_type().is_symlink() || !m.file_type().is_dir() {
                    tracing::warn!(
                        "Skipping mod icon GC because {} is a symlink or not a directory",
                        icons_dir.display()
                    );
                    false
                } else {
                    true
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
            Err(e) => {
                return Err(format!(
                    "Failed to inspect mod icon directory {}: {}",
                    icons_dir.display(),
                    e
                ));
            }
        };

        let mut conn_opt = if let Some(db_meta) = db_meta_opt {
            let size_before = db_meta.len();
            let mut conn = open_db_with_timeout(&db_path)
                .map_err(|e| format!("Failed to open mod_cache DB {}: {}", db_path.display(), e))?;
            ensure_mod_cache_schema(&conn)
                .map_err(|e| format!("Failed to ensure mod_cache schema: {}", e))?;

            let rows: Vec<(String, Option<String>, Option<i64>)> = {
                let mut stmt = conn
                    .prepare("SELECT cache_key, mod_id, unreferenced_since FROM mod_cache")
                    .map_err(|e| format!("Failed to query mod_cache rows: {}", e))?;
                let mapped = stmt
                    .query_map([], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, Option<String>>(1)?,
                            row.get::<_, Option<i64>>(2)?,
                        ))
                    })
                    .map_err(|e| format!("Failed to read mod_cache rows: {}", e))?;
                mapped
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| format!("Failed to collect mod_cache rows: {}", e))?
            };

            let mut reactivated_keys = Vec::new();
            let mut newly_dead_keys = Vec::new();
            let mut delete_keys = Vec::new();

            for (key, mod_id_opt, unref_opt) in rows {
                let is_alive = live_prefixes.iter().any(|prefix| key.starts_with(prefix));
                if is_alive {
                    live_icon_names.insert(format!("{}.png", sanitize_icon_stem(&key)));
                    if let Some(mod_id) =
                        mod_id_opt.as_deref().map(str::trim).filter(|s| !s.is_empty())
                    {
                        live_icon_names.insert(format!("{}.png", sanitize_icon_stem(mod_id)));
                    }
                    if unref_opt.is_some() {
                        reactivated_keys.push(key);
                    }
                } else if retention_secs == 0 {
                    delete_keys.push(key);
                } else {
                    match unref_opt {
                        None => newly_dead_keys.push(key),
                        Some(since_secs) => {
                            let since = u64::try_from(since_secs).unwrap_or(now_secs);
                            if now_secs.saturating_sub(since) >= retention_secs {
                                delete_keys.push(key);
                            }
                        }
                    }
                }
            }

            if !reactivated_keys.is_empty()
                || !newly_dead_keys.is_empty()
                || !delete_keys.is_empty()
            {
                let tx = conn
                    .transaction()
                    .map_err(|e| format!("Failed to start mod_cache GC transaction: {}", e))?;
                if !reactivated_keys.is_empty() {
                    let mut reactivate_stmt = tx
                        .prepare("UPDATE mod_cache SET unreferenced_since = NULL WHERE cache_key = ?")
                        .map_err(|e| format!("Failed to prepare mod_cache reactivate: {}", e))?;
                    for key in &reactivated_keys {
                        reactivate_stmt.execute([key]).map_err(|e| {
                            format!("Failed to clear mod_cache unreferenced_since: {}", e)
                        })?;
                    }
                }
                if !newly_dead_keys.is_empty() {
                    let now_i64 = i64::try_from(now_secs).unwrap_or(i64::MAX);
                    let mut mark_stmt = tx
                        .prepare("UPDATE mod_cache SET unreferenced_since = ? WHERE cache_key = ? AND unreferenced_since IS NULL")
                        .map_err(|e| format!("Failed to prepare mod_cache retention mark: {}", e))?;
                    for key in &newly_dead_keys {
                        mark_stmt
                            .execute(rusqlite::params![now_i64, key])
                            .map_err(|e| {
                                format!("Failed to mark mod_cache unreferenced_since: {}", e)
                            })?;
                    }
                }
                if !delete_keys.is_empty() {
                    let mut del_stmt = tx
                        .prepare("DELETE FROM mod_cache WHERE cache_key = ?")
                        .map_err(|e| format!("Failed to prepare mod_cache delete: {}", e))?;
                    for key in &delete_keys {
                        if del_stmt.execute([key]).is_ok() {
                            stats.removed_entries += 1;
                        }
                    }
                }
                tx.commit()
                    .map_err(|e| format!("Failed to commit mod_cache GC transaction: {}", e))?;

                if stats.removed_entries > 0 {
                    let _ = conn.execute_batch("VACUUM;");
                }
            }

            let size_after = fs::symlink_metadata(&db_path)
                .map(|m| m.len())
                .unwrap_or(size_before);
            stats.db_freed_bytes = size_before.saturating_sub(size_after);
            stats.freed_bytes = stats.freed_bytes.saturating_add(stats.db_freed_bytes);
            Some(conn)
        } else {
            None
        };

        if icons_dir_valid {
            let entries = fs::read_dir(&icons_dir)
                .map_err(|e| format!("Failed to read icons dir {}: {}", icons_dir.display(), e))?;

            let mut candidate_orphans: Vec<(PathBuf, String, u64)> = Vec::new();
            let mut live_files_on_disk: Vec<String> = Vec::new();

            for entry in entries.flatten() {
                let Ok(ft) = entry.file_type() else {
                    continue;
                };
                if ft.is_symlink() || !ft.is_file() {
                    continue;
                }
                let path = entry.path();
                let Ok(metadata) = fs::symlink_metadata(&path) else {
                    continue;
                };
                if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
                    continue;
                }
                let file_name = entry.file_name().to_string_lossy().to_string();
                let stem = Path::new(&file_name)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("");
                if live_icon_names.contains(&file_name)
                    || sanitized_prefixes.iter().any(|p| stem.starts_with(p))
                {
                    live_files_on_disk.push(file_name);
                } else {
                    candidate_orphans.push((path, file_name, metadata.len()));
                }
            }

            if retention_secs > 0 && !candidate_orphans.is_empty() && conn_opt.is_none() {
                if let Ok(conn) = open_db_with_timeout(&db_path) {
                    if ensure_mod_cache_schema(&conn).is_ok() {
                        conn_opt = Some(conn);
                    }
                }
            }

            let existing_icon_retention: HashMap<String, u64> =
                if let Some(conn) = conn_opt.as_ref() {
                    conn.prepare("SELECT file_name, unreferenced_since FROM mod_icon_retention")
                        .and_then(|mut stmt| {
                            let mapped = stmt.query_map([], |row| {
                                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
                            })?;
                            Ok(mapped
                                .filter_map(Result::ok)
                                .map(|(name, ts)| (name, u64::try_from(ts).unwrap_or(now_secs)))
                                .collect())
                        })
                        .unwrap_or_default()
                } else {
                    HashMap::new()
                };

            let mut icon_retention_marks: Vec<String> = Vec::new();
            let mut icon_retention_clears: Vec<String> = live_files_on_disk;
            let mut surviving_orphans: HashSet<String> = HashSet::new();

            for (path, file_name, len) in candidate_orphans {
                let should_delete = if retention_secs == 0 {
                    true
                } else if let Some(&since) = existing_icon_retention.get(&file_name) {
                    now_secs.saturating_sub(since) >= retention_secs
                } else {
                    icon_retention_marks.push(file_name.clone());
                    false
                };

                if should_delete {
                    match fs::remove_file(&path) {
                        Ok(()) => {
                            stats.removed_icons += 1;
                            stats.freed_bytes = stats.freed_bytes.saturating_add(len);
                            icon_retention_clears.push(file_name);
                        }
                        Err(_) => {
                            surviving_orphans.insert(file_name);
                        }
                    }
                } else {
                    surviving_orphans.insert(file_name);
                }
            }

            for tracked_name in existing_icon_retention.keys() {
                if !surviving_orphans.contains(tracked_name)
                    && !icon_retention_clears.contains(tracked_name)
                {
                    icon_retention_clears.push(tracked_name.clone());
                }
            }

            if let Some(conn) = conn_opt.as_mut() {
                if !icon_retention_marks.is_empty() || !icon_retention_clears.is_empty() {
                    let tx = conn.transaction().map_err(|e| {
                        format!("Failed to start mod_icon_retention transaction: {}", e)
                    })?;
                    if !icon_retention_marks.is_empty() {
                        let now_i64 = i64::try_from(now_secs).unwrap_or(i64::MAX);
                        let mut ins_stmt = tx
                            .prepare("INSERT OR IGNORE INTO mod_icon_retention (file_name, unreferenced_since) VALUES (?, ?)")
                            .map_err(|e| format!("Failed to prepare mod_icon_retention insert: {}", e))?;
                        for name in &icon_retention_marks {
                            ins_stmt
                                .execute(rusqlite::params![name, now_i64])
                                .map_err(|e| format!("Failed to insert mod_icon_retention: {}", e))?;
                        }
                    }
                    if !icon_retention_clears.is_empty() {
                        let mut del_stmt = tx
                            .prepare("DELETE FROM mod_icon_retention WHERE file_name = ?")
                            .map_err(|e| format!("Failed to prepare mod_icon_retention delete: {}", e))?;
                        for name in &icon_retention_clears {
                            del_stmt
                                .execute([name])
                                .map_err(|e| format!("Failed to delete mod_icon_retention: {}", e))?;
                        }
                    }
                    tx.commit().map_err(|e| {
                        format!("Failed to commit mod_icon_retention transaction: {}", e)
                    })?;
                }
            }
        }

        Ok(stats)
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

        if let Some(ref mid) = meta.mod_id {
            meta.zh_name_embedded = extract_embedded_zh_name(&mut archive, mid);
        }

        meta
    }
}

fn extract_embedded_zh_name(archive: &mut zip::ZipArchive<File>, mod_id: &str) -> Option<String> {
    let candidate_paths = [
        format!("assets/{}/lang/zh_cn.json", mod_id),
        format!("assets/{}/lang/zh_cn.json", mod_id.to_ascii_lowercase()),
    ];
    for path in &candidate_paths {
        if let Some(content) = read_archive_entry_string(archive, path) {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(obj) = json.as_object() {
                    let keys = [
                        format!("modmenu.nameTranslation.{}", mod_id),
                        format!("modmenu.nameTranslation.{}", mod_id.to_ascii_lowercase()),
                        format!("fml.menu.mods.info.displayName.{}", mod_id),
                    ];
                    for k in &keys {
                        if let Some(val) = obj.get(k).and_then(|v| v.as_str()) {
                            let trimmed = val.trim();
                            if !trimmed.is_empty()
                                && !trimmed.contains("${")
                                && crate::core::resource_naming::contains_cjk(trimmed)
                            {
                                return Some(trimmed.to_string());
                            }
                        }
                    }
                }
            }
        }
    }
    None
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
            zh_name_embedded: None,
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

    #[test]
    fn test_prune_unreferenced_skips_when_mod_cache_absent() {
        let dir = tempdir().unwrap();
        let stats = ModParser::prune_unreferenced(dir.path(), 0).unwrap();
        assert_eq!(stats.removed_entries, 0);
        assert_eq!(stats.removed_icons, 0);
        assert!(!dir.path().join(".mod_cache").exists());
    }

    #[test]
    fn test_prune_unreferenced_preserves_enabled_and_disabled_mods_and_cleans_orphans() {
        let dir = tempdir().unwrap();
        let mods_dir = dir.path().join("versions").join("inst-a").join("mods");
        fs::create_dir_all(&mods_dir).unwrap();

        let enabled_jar = mods_dir.join("sodium.jar");
        let disabled_jar = mods_dir.join("iris.jar.disable");
        fs::write(&enabled_jar, b"sodium-bytes").unwrap();
        fs::write(&disabled_jar, b"iris-bytes-123").unwrap();

        let mtime_enabled = fs::metadata(&enabled_jar)
            .unwrap()
            .modified()
            .unwrap()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let mtime_disabled = fs::metadata(&disabled_jar)
            .unwrap()
            .modified()
            .unwrap()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let parser = ModParser::new(dir.path());
        let key_enabled = build_mod_cache_key("sodium.jar", 12, mtime_enabled, "Fabric");
        let key_disabled = build_mod_cache_key("iris.jar", 14, mtime_disabled, "Fabric");
        let key_orphan = build_mod_cache_key("deleted.jar", 99, 1000, "Fabric");

        let meta = ModMetadata {
            mod_id: Some("test".to_string()),
            name: Some("Test".to_string()),
            version: Some("1.0".to_string()),
            has_icon: true,
            ..Default::default()
        };
        parser.set_cache_entry(&key_enabled, &meta);
        parser.set_cache_entry(&key_disabled, &meta);
        parser.set_cache_entry(&key_orphan, &meta);

        let icon_enabled = parser.get_icon_path(&key_enabled);
        let icon_disabled = parser.get_icon_path(&key_disabled);
        let icon_orphan = parser.get_icon_path(&key_orphan);
        let icon_legacy = dir.path().join(".mod_cache").join("icons").join("v2_old.png");
        fs::write(&icon_enabled, b"png1").unwrap();
        fs::write(&icon_disabled, b"png2").unwrap();
        fs::write(&icon_orphan, b"png3").unwrap();
        fs::write(&icon_legacy, b"png4").unwrap();

        // With 24h retention, fresh orphan icons and DB rows remain untouched.
        let deferred = ModParser::prune_unreferenced(dir.path(), 86_400).unwrap();
        assert_eq!(deferred.removed_entries, 0);
        assert_eq!(deferred.removed_icons, 0);
        assert!(icon_orphan.exists());

        // With immediate retention (0s), only orphan DB entry + orphan/legacy icons are removed.
        let stats = ModParser::prune_unreferenced(dir.path(), 0).unwrap();
        assert_eq!(stats.removed_entries, 1);
        assert_eq!(stats.removed_icons, 2);

        let remaining = parser.load_all_cache();
        assert!(remaining.contains_key(&key_enabled));
        assert!(remaining.contains_key(&key_disabled));
        assert!(!remaining.contains_key(&key_orphan));

        assert!(icon_enabled.exists());
        assert!(icon_disabled.exists());
        assert!(!icon_orphan.exists());
        assert!(!icon_legacy.exists());
    }

    #[test]
    fn test_prune_unreferenced_protects_mod_id_icons_and_tracks_unreferenced_grace_window() {
        let dir = tempdir().unwrap();
        let mods_dir = dir.path().join("versions").join("inst-a").join("mods");
        fs::create_dir_all(&mods_dir).unwrap();

        let live_jar = mods_dir.join("sodium.jar");
        let temp_jar = mods_dir.join("tempmod.jar");
        fs::write(&live_jar, b"sodium-bytes").unwrap();
        fs::write(&temp_jar, b"temp-bytes-123").unwrap();

        let mtime_live = fs::metadata(&live_jar)
            .unwrap()
            .modified()
            .unwrap()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let mtime_temp = fs::metadata(&temp_jar)
            .unwrap()
            .modified()
            .unwrap()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let parser = ModParser::new(dir.path());
        let key_live = build_mod_cache_key("sodium.jar", 12, mtime_live, "Fabric");
        let key_temp = build_mod_cache_key("tempmod.jar", 14, mtime_temp, "Fabric");

        parser.set_cache_entry(
            &key_live,
            &ModMetadata {
                mod_id: Some("sodium".to_string()),
                name: Some("Sodium".to_string()),
                version: Some("0.5".to_string()),
                has_icon: true,
                ..Default::default()
            },
        );
        parser.set_cache_entry(
            &key_temp,
            &ModMetadata {
                mod_id: Some("temp_mod_id".to_string()),
                name: Some("TempMod".to_string()),
                version: Some("1.0".to_string()),
                has_icon: true,
                ..Default::default()
            },
        );

        let live_mod_id_icon = parser.get_icon_path("sodium");
        let temp_mod_id_icon = parser.get_icon_path("temp_mod_id");
        let old_orphan_icon = parser.get_icon_path("old_orphan_icon");
        fs::write(&live_mod_id_icon, b"live-icon").unwrap();
        fs::write(&temp_mod_id_icon, b"temp-icon").unwrap();
        fs::write(&old_orphan_icon, b"orphan-icon").unwrap();

        // Backdate files & DB to 30 days ago to verify grace window does NOT use file mtime.
        let thirty_days_ago = SystemTime::now() - Duration::from_secs(30 * 86_400);
        for p in [
            &live_mod_id_icon,
            &temp_mod_id_icon,
            &old_orphan_icon,
            &parser.db_path,
        ] {
            let f = fs::OpenOptions::new().write(true).open(p).unwrap();
            let _ = f.set_modified(thirty_days_ago);
        }

        // Remove tempmod.jar so key_temp & temp_mod_id_icon become unreferenced now.
        fs::remove_file(&temp_jar).unwrap();

        let t0 = 2_000_000_u64;
        // Pass 1 at t0: records unreferenced_since = t0, removes nothing during 24h grace window.
        let s1 = ModParser::prune_unreferenced_at(dir.path(), 86_400, t0).unwrap();
        assert_eq!(s1.removed_entries, 0);
        assert_eq!(s1.removed_icons, 0);
        assert!(live_mod_id_icon.exists());
        assert!(temp_mod_id_icon.exists());
        assert!(old_orphan_icon.exists());

        // Pass 2 at t0 + 86_399: still inside grace window.
        let s2 = ModParser::prune_unreferenced_at(dir.path(), 86_400, t0 + 86_399).unwrap();
        assert_eq!(s2.removed_entries, 0);
        assert_eq!(s2.removed_icons, 0);
        assert!(live_mod_id_icon.exists());
        assert!(temp_mod_id_icon.exists());

        // Pass 3 at t0 + 86_400: grace window elapsed -> temp row + orphan icons removed; live mod_id icon preserved.
        let s3 = ModParser::prune_unreferenced_at(dir.path(), 86_400, t0 + 86_400).unwrap();
        assert_eq!(s3.removed_entries, 1);
        assert_eq!(s3.removed_icons, 2);
        assert!(live_mod_id_icon.exists());
        assert!(!temp_mod_id_icon.exists());
        assert!(!old_orphan_icon.exists());

        // Immediate prune (0s) also preserves live mod_id icon.
        let s4 = ModParser::prune_unreferenced_at(dir.path(), 0, t0 + 86_500).unwrap();
        assert_eq!(s4.removed_entries, 0);
        assert_eq!(s4.removed_icons, 0);
        assert!(live_mod_id_icon.exists());
    }

    #[test]
    fn test_prune_unreferenced_skips_symlinked_icons_dir() {
        let dir = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let victim = outside.path().join("do_not_delete.png");
        fs::write(&victim, b"important").unwrap();

        let _parser = ModParser::new(dir.path());
        let icons_dir = dir.path().join(".mod_cache").join("icons");
        fs::remove_dir_all(&icons_dir).unwrap();

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(outside.path(), &icons_dir).unwrap();
        }
        #[cfg(windows)]
        {
            let status = std::process::Command::new("cmd")
                .args([
                    "/C",
                    "mklink",
                    "/J",
                    &icons_dir.to_string_lossy(),
                    &outside.path().to_string_lossy(),
                ])
                .status();
            if status.map(|s| !s.success()).unwrap_or(true) {
                return;
            }
        }

        let stats = ModParser::prune_unreferenced_at(dir.path(), 0, 2_000_000).unwrap();
        assert_eq!(stats.removed_icons, 0);
        assert!(victim.exists());
    }
}
