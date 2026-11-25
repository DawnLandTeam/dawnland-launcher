use crate::core::mojang::get_dawnland_dir;
use crate::core::resource_naming::extract_short_zh_name;
use crate::core::security::secure_request;
use crate::core::utils::get_http_client;
use crate::models::instance::AssetCategory;
use flate2::read::GzDecoder;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

pub const TRUST_AI_DRAFT: u8 = 1;
pub const TRUST_AUTHOR_LANG: u8 = 2;
pub const TRUST_VERIFIED: u8 = 3;
pub const TRUST_USER: u8 = 4;

const STARTER_SEED_JSON: &str = include_str!("starter_consensus_seed.json");

static SEED_INITIALIZED: OnceLock<()> = OnceLock::new();

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationEntry {
    pub category: String,
    #[serde(default)]
    pub cf_project_id: Option<String>,
    #[serde(default)]
    pub cf_slug: Option<String>,
    #[serde(default)]
    pub mr_project_id: Option<String>,
    #[serde(default)]
    pub mr_slug: Option<String>,
    #[serde(default)]
    pub mod_ids: String,
    #[serde(default)]
    pub raw_name: String,
    pub zh_name: String,
    #[serde(default)]
    pub zh_short_name: String,
    #[serde(default)]
    pub zh_aliases: String,
    #[serde(default)]
    pub zh_summary: String,
    #[serde(default)]
    pub wiki_id: u32,
    #[serde(default = "default_verified_trust")]
    pub trust_level: u8,
    #[serde(default = "default_source_tag")]
    pub source_tag: String,
    #[serde(default)]
    pub updated_at: i64,
}

fn default_verified_trust() -> u8 {
    TRUST_VERIFIED
}

fn default_source_tag() -> String {
    "consensus".to_string()
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModIdentityHints {
    pub cf_project_id: Option<String>,
    pub cf_slug: Option<String>,
    pub mr_project_id: Option<String>,
    pub mr_slug: Option<String>,
    pub mod_ids: Vec<String>,
    pub raw_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationLookupResult {
    pub zh_name: String,
    pub zh_short_name: String,
    pub zh_summary: Option<String>,
    pub wiki_id: Option<u32>,
    pub trust_level: u8,
    pub source_tag: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationSearchCandidate {
    pub category: String,
    pub cf_project_id: Option<String>,
    pub cf_slug: Option<String>,
    pub mr_project_id: Option<String>,
    pub mr_slug: Option<String>,
    pub raw_name: String,
    pub zh_name: String,
    pub zh_short_name: String,
    pub wiki_id: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HashMappingEntry {
    pub sha1: String,
    pub cf_murmur2: Option<u32>,
    pub source: String,
    pub project_id: String,
    pub slug: String,
    pub version_id: Option<String>,
    pub category: String,
    pub raw_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationDbStats {
    pub total_entries: usize,
    pub verified_entries: usize,
    pub user_entries: usize,
    pub ai_entries: usize,
    pub snapshot_version: String,
    pub last_sync_at: i64,
}

pub fn category_to_key(category: &AssetCategory) -> &'static str {
    match category {
        AssetCategory::Mod => "mod",
        AssetCategory::ResourcePack => "resourcepack",
        AssetCategory::ShaderPack => "shaderpack",
        AssetCategory::Datapack => "datapack",
        AssetCategory::Save => "save",
    }
}

pub fn get_translations_db_path() -> PathBuf {
    get_dawnland_dir().join("translations.db")
}

fn now_unix_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn get_web_backend_url() -> String {
    option_env!("VITE_WEB_BACKEND_URL")
        .unwrap_or("http://localhost:3030")
        .trim_end_matches('/')
        .to_string()
}

pub struct TranslationDb {
    db_path: PathBuf,
}

impl TranslationDb {
    pub fn open_default() -> Result<Self, String> {
        let path = get_translations_db_path();
        let db = Self { db_path: path };
        db.init_and_seed_once()?;
        Ok(db)
    }

    pub fn open_at(db_path: PathBuf) -> Result<Self, String> {
        let db = Self { db_path };
        db.init_schema()?;
        Ok(db)
    }

    fn connect(&self) -> Result<Connection, String> {
        if let Some(parent) = self.db_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let conn = Connection::open(&self.db_path).map_err(|e| e.to_string())?;
        let _ = conn.busy_timeout(std::time::Duration::from_secs(5));
        let _ = conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;");
        Ok(conn)
    }

    fn init_schema(&self) -> Result<(), String> {
        let conn = self.connect()?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS resource_translations (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                category TEXT NOT NULL,
                cf_project_id TEXT,
                cf_slug TEXT,
                mr_project_id TEXT,
                mr_slug TEXT,
                mod_ids TEXT NOT NULL DEFAULT '',
                raw_name TEXT NOT NULL DEFAULT '',
                zh_name TEXT NOT NULL,
                zh_short_name TEXT NOT NULL,
                zh_aliases TEXT NOT NULL DEFAULT '',
                zh_summary TEXT NOT NULL DEFAULT '',
                wiki_id INTEGER NOT NULL DEFAULT 0,
                trust_level INTEGER NOT NULL,
                source_tag TEXT NOT NULL,
                updated_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_rt_cf_pid ON resource_translations (category, cf_project_id);
            CREATE INDEX IF NOT EXISTS idx_rt_cf_slug ON resource_translations (category, cf_slug);
            CREATE INDEX IF NOT EXISTS idx_rt_cf_slug_lower ON resource_translations (category, LOWER(cf_slug));
            CREATE INDEX IF NOT EXISTS idx_rt_mr_pid ON resource_translations (category, mr_project_id);
            CREATE INDEX IF NOT EXISTS idx_rt_mr_slug ON resource_translations (category, mr_slug);
            CREATE INDEX IF NOT EXISTS idx_rt_mr_slug_lower ON resource_translations (category, LOWER(mr_slug));
            CREATE INDEX IF NOT EXISTS idx_rt_raw_name ON resource_translations (category, raw_name);

            CREATE TABLE IF NOT EXISTS file_hash_map (
                sha1 TEXT PRIMARY KEY,
                cf_murmur2 INTEGER,
                source TEXT NOT NULL,
                project_id TEXT NOT NULL,
                slug TEXT NOT NULL,
                version_id TEXT,
                category TEXT NOT NULL,
                raw_name TEXT,
                updated_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_fhm_murmur ON file_hash_map (cf_murmur2);

            CREATE TABLE IF NOT EXISTS translation_meta (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );",
        )
        .map_err(|e| format!("Failed to initialize translations.db schema: {}", e))?;
        Ok(())
    }

    fn init_and_seed_once(&self) -> Result<(), String> {
        self.init_schema()?;
        if SEED_INITIALIZED.get().is_none() {
            self.seed_builtin_consensus()?;
            let _ = SEED_INITIALIZED.set(());
        }
        Ok(())
    }

    pub fn seed_builtin_consensus(&self) -> Result<(), String> {
        let entries: Vec<TranslationEntry> = serde_json::from_str(STARTER_SEED_JSON)
            .map_err(|e| format!("Failed to parse starter consensus seed: {}", e))?;
        self.upsert_entries_batch(&entries)
    }

    /// Upsert translation entries enforcing monotonic trust-level protection:
    /// an entry with a lower `trust_level` NEVER overwrites a higher-trust entry.
    pub fn upsert_entries_batch(&self, entries: &[TranslationEntry]) -> Result<(), String> {
        if entries.is_empty() {
            return Ok(());
        }
        let mut conn = self.connect()?;
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        let now = now_unix_secs();

        for item in entries {
            let cat = item.category.trim().to_ascii_lowercase();
            let zh_name = item.zh_name.trim();
            if cat.is_empty() || zh_name.is_empty() {
                continue;
            }
            let short = if item.zh_short_name.trim().is_empty() {
                extract_short_zh_name(zh_name)
            } else {
                item.zh_short_name.trim().to_string()
            };
            let cf_pid = item.cf_project_id.as_deref().map(str::trim).filter(|s| !s.is_empty());
            let cf_slug = item.cf_slug.as_deref().map(str::trim).filter(|s| !s.is_empty());
            let mr_pid = item.mr_project_id.as_deref().map(str::trim).filter(|s| !s.is_empty());
            let mr_slug = item.mr_slug.as_deref().map(str::trim).filter(|s| !s.is_empty());
            let raw_name = item.raw_name.trim();
            let updated_at = if item.updated_at > 0 { item.updated_at } else { now };

            let existing = find_existing_row(&tx, &cat, cf_pid, cf_slug, mr_pid, mr_slug, raw_name)?;
            match existing {
                Some((row_id, existing_trust)) => {
                    if item.trust_level < existing_trust {
                        continue;
                    }
                    tx.execute(
                        "UPDATE resource_translations SET
                            cf_project_id = COALESCE(?1, cf_project_id),
                            cf_slug = COALESCE(?2, cf_slug),
                            mr_project_id = COALESCE(?3, mr_project_id),
                            mr_slug = COALESCE(?4, mr_slug),
                            mod_ids = CASE WHEN ?5 != '' THEN ?5 ELSE mod_ids END,
                            raw_name = CASE WHEN ?6 != '' THEN ?6 ELSE raw_name END,
                            zh_name = ?7,
                            zh_short_name = ?8,
                            zh_aliases = CASE WHEN ?9 != '' THEN ?9 ELSE zh_aliases END,
                            zh_summary = CASE WHEN ?10 != '' THEN ?10 ELSE zh_summary END,
                            wiki_id = CASE WHEN ?11 > 0 THEN ?11 ELSE wiki_id END,
                            trust_level = ?12,
                            source_tag = ?13,
                            updated_at = ?14
                         WHERE id = ?15",
                        params![
                            cf_pid,
                            cf_slug,
                            mr_pid,
                            mr_slug,
                            item.mod_ids.trim(),
                            raw_name,
                            zh_name,
                            short,
                            item.zh_aliases.trim(),
                            item.zh_summary.trim(),
                            item.wiki_id as i64,
                            item.trust_level as i64,
                            item.source_tag.trim(),
                            updated_at,
                            row_id
                        ],
                    )
                    .map_err(|e| e.to_string())?;
                }
                None => {
                    tx.execute(
                        "INSERT INTO resource_translations (
                            category, cf_project_id, cf_slug, mr_project_id, mr_slug,
                            mod_ids, raw_name, zh_name, zh_short_name, zh_aliases,
                            zh_summary, wiki_id, trust_level, source_tag, updated_at
                        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
                        params![
                            cat,
                            cf_pid,
                            cf_slug,
                            mr_pid,
                            mr_slug,
                            item.mod_ids.trim(),
                            raw_name,
                            zh_name,
                            short,
                            item.zh_aliases.trim(),
                            item.zh_summary.trim(),
                            item.wiki_id as i64,
                            item.trust_level as i64,
                            item.source_tag.trim(),
                            updated_at
                        ],
                    )
                    .map_err(|e| e.to_string())?;
                }
            }
        }

        tx.commit().map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn lookup(
        &self,
        category: &AssetCategory,
        source: Option<&str>,
        project_id: Option<&str>,
        slug: Option<&str>,
        mod_id: Option<&str>,
        raw_name: Option<&str>,
    ) -> Option<TranslationLookupResult> {
        let conn = self.connect().ok()?;
        let cat = category_to_key(category);
        let src_lower = source.unwrap_or("").trim().to_ascii_lowercase();
        let pid = project_id.map(str::trim).filter(|s| !s.is_empty());
        let sl = slug.map(str::trim).filter(|s| !s.is_empty());
        let mid = mod_id.map(str::trim).filter(|s| !s.is_empty());
        let rname = raw_name.map(str::trim).filter(|s| !s.is_empty());

        // 1. Exact project_id match
        if let Some(p) = pid {
            let query = match src_lower.as_str() {
                "curseforge" => "SELECT zh_name, zh_short_name, zh_summary, wiki_id, trust_level, source_tag FROM resource_translations WHERE category = ?1 AND cf_project_id = ?2 ORDER BY trust_level DESC LIMIT 1",
                "modrinth" => "SELECT zh_name, zh_short_name, zh_summary, wiki_id, trust_level, source_tag FROM resource_translations WHERE category = ?1 AND mr_project_id = ?2 ORDER BY trust_level DESC LIMIT 1",
                _ => "SELECT zh_name, zh_short_name, zh_summary, wiki_id, trust_level, source_tag FROM resource_translations WHERE category = ?1 AND (cf_project_id = ?2 OR mr_project_id = ?2) ORDER BY trust_level DESC LIMIT 1",
            };
            if let Some(res) = query_lookup_row(&conn, query, params![cat, p]) {
                return Some(res);
            }
        }

        // 2. Exact slug match (checks both platform slug columns because cross-platform mods share slugs)
        if let Some(s) = sl {
            if let Some(res) = query_lookup_row(
                &conn,
                "SELECT zh_name, zh_short_name, zh_summary, wiki_id, trust_level, source_tag
                 FROM resource_translations
                 WHERE category = ?1 AND (LOWER(cf_slug) = LOWER(?2) OR LOWER(mr_slug) = LOWER(?2))
                 ORDER BY trust_level DESC LIMIT 1",
                params![cat, s],
            ) {
                return Some(res);
            }
        }

        // 3. Embedded mod_id match (e.g. ";create;")
        if let Some(m) = mid {
            let pattern = format!("%;{};%", m.to_ascii_lowercase());
            if let Some(res) = query_lookup_row(
                &conn,
                "SELECT zh_name, zh_short_name, zh_summary, wiki_id, trust_level, source_tag
                 FROM resource_translations
                 WHERE category = ?1 AND (';' || LOWER(mod_ids) || ';') LIKE ?2
                 ORDER BY trust_level DESC LIMIT 1",
                params![cat, pattern],
            ) {
                return Some(res);
            }
        }

        // 4. Case-insensitive raw_name match
        if let Some(rn) = rname {
            if let Some(res) = query_lookup_row(
                &conn,
                "SELECT zh_name, zh_short_name, zh_summary, wiki_id, trust_level, source_tag
                 FROM resource_translations
                 WHERE category = ?1 AND LOWER(raw_name) = LOWER(?2)
                 ORDER BY trust_level DESC LIMIT 1",
                params![cat, rn],
            ) {
                return Some(res);
            }
        }

        None
    }

    /// Look up verified cross-platform identity hints for a mod (`category = 'mod'`)
    /// using exact `project_id` or platform-scoped `slug` matching (`trust_level >= TRUST_VERIFIED`).
    /// Never falls back to `raw_name` to avoid colliding with add-on or similarly named mods.
    pub fn lookup_mod_identity(
        &self,
        source: &str,
        project_id: &str,
        slug: &str,
    ) -> Result<Option<ModIdentityHints>, String> {
        let src_lower = source.trim().to_ascii_lowercase();
        if src_lower != "curseforge" && src_lower != "modrinth" {
            return Ok(None);
        }

        let conn = self.connect()?;
        let pid = project_id.trim();
        let sl = slug.trim();
        let min_trust = TRUST_VERIFIED as i64;

        let map_row = |row: &rusqlite::Row<'_>| -> rusqlite::Result<ModIdentityHints> {
            let cf_project_id: Option<String> = row.get(0)?;
            let cf_slug: Option<String> = row.get(1)?;
            let mr_project_id: Option<String> = row.get(2)?;
            let mr_slug: Option<String> = row.get(3)?;
            let mod_ids_raw: String = row.get(4).unwrap_or_default();
            let raw_name_val: String = row.get(5).unwrap_or_default();

            let clean_opt = |opt: Option<String>| -> Option<String> {
                opt.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
            };
            let mod_ids = mod_ids_raw
                .split(';')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect();
            let raw_name = {
                let trimmed = raw_name_val.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_string())
                }
            };

            Ok(ModIdentityHints {
                cf_project_id: clean_opt(cf_project_id),
                cf_slug: clean_opt(cf_slug),
                mr_project_id: clean_opt(mr_project_id),
                mr_slug: clean_opt(mr_slug),
                mod_ids,
                raw_name,
            })
        };

        // Step A: Exact project_id match on the specified platform
        if !pid.is_empty() {
            let sql = match src_lower.as_str() {
                "curseforge" => {
                    "SELECT cf_project_id, cf_slug, mr_project_id, mr_slug, mod_ids, raw_name
                     FROM resource_translations
                     WHERE category = 'mod' AND trust_level >= ?1 AND cf_project_id = ?2
                     ORDER BY trust_level DESC LIMIT 1"
                }
                _ => {
                    "SELECT cf_project_id, cf_slug, mr_project_id, mr_slug, mod_ids, raw_name
                     FROM resource_translations
                     WHERE category = 'mod' AND trust_level >= ?1 AND mr_project_id = ?2
                     ORDER BY trust_level DESC LIMIT 1"
                }
            };
            match conn.query_row(sql, params![min_trust, pid], map_row) {
                Ok(hints) => return Ok(Some(hints)),
                Err(rusqlite::Error::QueryReturnedNoRows) => {}
                Err(e) => {
                    return Err(format!(
                        "Failed to query mod identity by project_id: {}",
                        e
                    ));
                }
            }
        }

        // Step B: Fallback to exact slug match restricted to the requested platform's slug column,
        // filtering non-conflicting same-platform project IDs inside SQL so collisions do not shadow matches.
        if !sl.is_empty() {
            let sql = match src_lower.as_str() {
                "curseforge" => {
                    "SELECT cf_project_id, cf_slug, mr_project_id, mr_slug, mod_ids, raw_name
                     FROM resource_translations
                     WHERE category = 'mod'
                       AND LOWER(cf_slug) = LOWER(?1)
                       AND trust_level >= ?2
                       AND (?3 = '' OR cf_project_id IS NULL OR cf_project_id = '' OR cf_project_id = ?3)
                     ORDER BY trust_level DESC LIMIT 1"
                }
                _ => {
                    "SELECT cf_project_id, cf_slug, mr_project_id, mr_slug, mod_ids, raw_name
                     FROM resource_translations
                     WHERE category = 'mod'
                       AND LOWER(mr_slug) = LOWER(?1)
                       AND trust_level >= ?2
                       AND (?3 = '' OR mr_project_id IS NULL OR mr_project_id = '' OR mr_project_id = ?3)
                     ORDER BY trust_level DESC LIMIT 1"
                }
            };
            match conn.query_row(sql, params![sl, min_trust, pid], map_row) {
                Ok(hints) => return Ok(Some(hints)),
                Err(rusqlite::Error::QueryReturnedNoRows) => {}
                Err(e) => {
                    return Err(format!("Failed to query mod identity by slug: {}", e));
                }
            }
        }

        Ok(None)
    }

    pub fn search_candidates(
        &self,
        category: &AssetCategory,
        query: &str,
        limit: usize,
    ) -> Vec<TranslationSearchCandidate> {
        let q = query.trim();
        if q.is_empty() {
            return Vec::new();
        }
        let Ok(conn) = self.connect() else {
            return Vec::new();
        };
        let cat = category_to_key(category);
        let like_pattern = format!("%{}%", q);
        let prefix_pattern = format!("{}%", q);
        let limit_i64 = limit.clamp(1, 50) as i64;

        let Ok(mut stmt) = conn.prepare(
            "SELECT category, cf_project_id, cf_slug, mr_project_id, mr_slug, raw_name, zh_name, zh_short_name, wiki_id
             FROM resource_translations
             WHERE category = ?1
               AND (zh_name LIKE ?2 OR zh_short_name LIKE ?2 OR zh_aliases LIKE ?2 OR raw_name LIKE ?2)
             ORDER BY
               CASE
                 WHEN LOWER(zh_short_name) = LOWER(?3) OR LOWER(zh_name) = LOWER(?3) THEN 0
                 WHEN zh_short_name LIKE ?4 OR zh_name LIKE ?4 THEN 1
                 WHEN zh_aliases LIKE ?2 THEN 2
                 ELSE 3
               END,
               trust_level DESC
             LIMIT ?5",
        ) else {
            return Vec::new();
        };

        let Ok(rows) = stmt.query_map(
            params![cat, like_pattern, q, prefix_pattern, limit_i64],
            |row| {
                let wiki_id_val: i64 = row.get(8).unwrap_or(0);
                Ok(TranslationSearchCandidate {
                    category: row.get(0)?,
                    cf_project_id: row.get(1)?,
                    cf_slug: row.get(2)?,
                    mr_project_id: row.get(3)?,
                    mr_slug: row.get(4)?,
                    raw_name: row.get(5)?,
                    zh_name: row.get(6)?,
                    zh_short_name: row.get(7)?,
                    wiki_id: if wiki_id_val > 0 {
                        Some(wiki_id_val as u32)
                    } else {
                        None
                    },
                })
            },
        ) else {
            return Vec::new();
        };

        rows.filter_map(Result::ok).collect()
    }

    pub fn get_hash_mapping(&self, sha1: &str) -> Option<HashMappingEntry> {
        let conn = self.connect().ok()?;
        conn.query_row(
            "SELECT sha1, cf_murmur2, source, project_id, slug, version_id, category, raw_name
             FROM file_hash_map WHERE LOWER(sha1) = LOWER(?1) LIMIT 1",
            params![sha1],
            |row| {
                let murmur_opt: Option<i64> = row.get(1)?;
                Ok(HashMappingEntry {
                    sha1: row.get(0)?,
                    cf_murmur2: murmur_opt.map(|v| v as u32),
                    source: row.get(2)?,
                    project_id: row.get(3)?,
                    slug: row.get(4)?,
                    version_id: row.get(5)?,
                    category: row.get(6)?,
                    raw_name: row.get(7)?,
                })
            },
        )
        .ok()
    }

    pub fn save_hash_mappings(&self, mappings: &[HashMappingEntry]) -> Result<(), String> {
        if mappings.is_empty() {
            return Ok(());
        }
        let mut conn = self.connect()?;
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        let now = now_unix_secs();
        for m in mappings {
            tx.execute(
                "INSERT OR REPLACE INTO file_hash_map
                 (sha1, cf_murmur2, source, project_id, slug, version_id, category, raw_name, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    m.sha1.to_ascii_lowercase(),
                    m.cf_murmur2.map(|v| v as i64),
                    m.source,
                    m.project_id,
                    m.slug,
                    m.version_id,
                    m.category,
                    m.raw_name,
                    now
                ],
            )
            .map_err(|e| e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn get_meta(&self, key: &str) -> Option<String> {
        let conn = self.connect().ok()?;
        conn.query_row(
            "SELECT value FROM translation_meta WHERE key = ?1",
            params![key],
            |row| row.get(0),
        )
        .ok()
    }

    pub fn set_meta(&self, key: &str, value: &str) -> Result<(), String> {
        let conn = self.connect()?;
        conn.execute(
            "INSERT OR REPLACE INTO translation_meta (key, value) VALUES (?1, ?2)",
            params![key, value],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn get_stats(&self) -> Result<TranslationDbStats, String> {
        let conn = self.connect()?;
        let total: i64 = conn
            .query_row("SELECT COUNT(*) FROM resource_translations", [], |r| r.get(0))
            .unwrap_or(0);
        let verified: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM resource_translations WHERE trust_level = ?1",
                params![TRUST_VERIFIED as i64],
                |r| r.get(0),
            )
            .unwrap_or(0);
        let user: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM resource_translations WHERE trust_level = ?1",
                params![TRUST_USER as i64],
                |r| r.get(0),
            )
            .unwrap_or(0);
        let ai: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM resource_translations WHERE trust_level = ?1",
                params![TRUST_AI_DRAFT as i64],
                |r| r.get(0),
            )
            .unwrap_or(0);
        let mut snapshot_version = self
            .get_meta("snapshot_version")
            .unwrap_or_else(|| "v2026.11.14".to_string());
        if snapshot_version.starts_with("v-1794") || snapshot_version == "latest" {
            snapshot_version = chrono::Utc::now().format("v%Y.%m.%d").to_string();
        }
        let last_sync_at = self
            .get_meta("last_sync_at")
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(0);

        Ok(TranslationDbStats {
            total_entries: total.max(0) as usize,
            verified_entries: verified.max(0) as usize,
            user_entries: user.max(0) as usize,
            ai_entries: ai.max(0) as usize,
            snapshot_version,
            last_sync_at,
        })
    }

    /// Remove poisoned `TRUST_USER` rows whose `cf_project_id` or `mr_project_id` matches
    /// a modpack project ID accidentally stamped onto individual mod records.
    pub fn purge_polluted_user_entries(
        &self,
        polluted_pids: &std::collections::HashSet<String>,
    ) -> Result<usize, String> {
        if polluted_pids.is_empty() {
            return Ok(0);
        }
        let conn = self.connect()?;
        let mut deleted = 0usize;
        for pid in polluted_pids {
            let trimmed = pid.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Ok(n) = conn.execute(
                "DELETE FROM resource_translations
                 WHERE trust_level = ?1 AND (cf_project_id = ?2 OR mr_project_id = ?2)",
                params![TRUST_USER as i64, trimmed],
            ) {
                deleted += n;
            }
        }
        Ok(deleted)
    }
}

fn find_existing_row(
    conn: &Connection,
    category: &str,
    cf_pid: Option<&str>,
    cf_slug: Option<&str>,
    mr_pid: Option<&str>,
    mr_slug: Option<&str>,
    raw_name: &str,
) -> Result<Option<(i64, u8)>, String> {
    if let Some(p) = cf_pid {
        if let Ok(row) = conn.query_row(
            "SELECT id, trust_level FROM resource_translations WHERE category = ?1 AND cf_project_id = ?2 LIMIT 1",
            params![category, p],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)? as u8)),
        ) {
            return Ok(Some(row));
        }
    }
    if let Some(p) = mr_pid {
        if let Ok(row) = conn.query_row(
            "SELECT id, trust_level FROM resource_translations WHERE category = ?1 AND mr_project_id = ?2 LIMIT 1",
            params![category, p],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)? as u8)),
        ) {
            return Ok(Some(row));
        }
    }
    if let Some(s) = cf_slug {
        if let Ok(row) = conn.query_row(
            "SELECT id, trust_level FROM resource_translations WHERE category = ?1 AND LOWER(cf_slug) = LOWER(?2) LIMIT 1",
            params![category, s],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)? as u8)),
        ) {
            return Ok(Some(row));
        }
    }
    if let Some(s) = mr_slug {
        if let Ok(row) = conn.query_row(
            "SELECT id, trust_level FROM resource_translations WHERE category = ?1 AND LOWER(mr_slug) = LOWER(?2) LIMIT 1",
            params![category, s],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)? as u8)),
        ) {
            return Ok(Some(row));
        }
    }
    if !raw_name.is_empty() {
        if let Ok(row) = conn.query_row(
            "SELECT id, trust_level FROM resource_translations WHERE category = ?1 AND LOWER(raw_name) = LOWER(?2) LIMIT 1",
            params![category, raw_name],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)? as u8)),
        ) {
            return Ok(Some(row));
        }
    }
    Ok(None)
}

fn query_lookup_row<P: rusqlite::Params>(
    conn: &Connection,
    sql: &str,
    params: P,
) -> Option<TranslationLookupResult> {
    conn.query_row(sql, params, |row| {
        let zh_name: String = row.get(0)?;
        let zh_short_name: String = row.get(1)?;
        let summary_raw: String = row.get(2).unwrap_or_default();
        let wiki_raw: i64 = row.get(3).unwrap_or(0);
        let trust_level: i64 = row.get(4).unwrap_or(TRUST_VERIFIED as i64);
        let source_tag: String = row.get(5).unwrap_or_default();
        Ok(TranslationLookupResult {
            zh_name,
            zh_short_name,
            zh_summary: if summary_raw.trim().is_empty() {
                None
            } else {
                Some(summary_raw)
            },
            wiki_id: if wiki_raw > 0 {
                Some(wiki_raw as u32)
            } else {
                None
            },
            trust_level: trust_level as u8,
            source_tag,
        })
    })
    .ok()
}

/// Async helper to look up a single resource translation in `translations.db` without blocking Tokio.
pub async fn lookup_translation_async(
    category: AssetCategory,
    source: Option<String>,
    project_id: Option<String>,
    slug: Option<String>,
    mod_id: Option<String>,
    raw_name: Option<String>,
) -> Option<TranslationLookupResult> {
    tokio::task::spawn_blocking(move || {
        let db = TranslationDb::open_default().ok()?;
        db.lookup(
            &category,
            source.as_deref(),
            project_id.as_deref(),
            slug.as_deref(),
            mod_id.as_deref(),
            raw_name.as_deref(),
        )
    })
    .await
    .ok()
    .flatten()
}

/// Search translation candidates for Chinese-to-English search augmentation.
#[tauri::command]
pub async fn search_translation_candidates(
    category: AssetCategory,
    query: String,
    limit: Option<usize>,
) -> Result<Vec<TranslationSearchCandidate>, String> {
    let lim = limit.unwrap_or(15);
    tokio::task::spawn_blocking(move || {
        let db = TranslationDb::open_default()?;
        Ok(db.search_candidates(&category, &query, lim))
    })
    .await
    .map_err(|e| format!("Translation search task error: {}", e))?
}

#[tauri::command]
pub async fn get_translation_db_stats() -> Result<TranslationDbStats, String> {
    tokio::task::spawn_blocking(move || {
        let db = TranslationDb::open_default()?;
        db.get_stats()
    })
    .await
    .map_err(|e| format!("Translation stats task error: {}", e))?
}

/// Sync latest consensus translations package from upstream/backend and write to local SQLite db.
#[tauri::command]
pub async fn sync_translations_from_backend(force_full: Option<bool>) -> Result<TranslationDbStats, String> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct BackendMetaResponse {
        snapshot_version: Option<String>,
        snapshot_url: Option<String>,
        #[serde(default)]
        download_url: Option<String>,
        #[serde(default)]
        mirror_urls: Vec<String>,
        #[serde(default)]
        md5: Option<String>,
        #[serde(default)]
        updated_at: i64,
    }

    let backend_url = get_web_backend_url();
    let meta_url = format!("{}/api/translations/meta", backend_url);
    let client = get_http_client();

    let meta_resp = secure_request(client, reqwest::Method::GET, &meta_url, "")
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| format!("Failed to reach translation server: {}", e))?;

    if !meta_resp.status().is_success() {
        return Err(format!("Translation meta API returned {}", meta_resp.status()));
    }

    let meta: BackendMetaResponse = meta_resp
        .json()
        .await
        .map_err(|e| format!("Invalid translation meta JSON: {}", e))?;

    let local_version = tokio::task::spawn_blocking(|| {
        let db = TranslationDb::open_default()?;
        Ok::<_, String>(db.get_meta("snapshot_version").unwrap_or_default())
    })
    .await
    .map_err(|e| e.to_string())??;

    let target_version = meta.snapshot_version.unwrap_or_default();
    let force = force_full.unwrap_or(false);

    // If not forced and local version matches target, skip redundant network download
    if !force && !target_version.is_empty() && local_version == target_version {
        return get_translation_db_stats().await;
    }

    // Candidate download endpoints: prioritize official download_url and mirrors
    let mut candidates = Vec::new();
    if let Some(dl) = &meta.download_url {
        let dl_clean = dl.trim();
        if !dl_clean.is_empty() && !candidates.contains(&dl_clean.to_string()) {
            candidates.push(dl_clean.to_string());
        }
    }
    for m in &meta.mirror_urls {
        let m_clean = m.trim();
        if !m_clean.is_empty() && !candidates.contains(&m_clean.to_string()) {
            candidates.push(m_clean.to_string());
        }
    }
    if let Some(snap) = &meta.snapshot_url {
        let snap_clean = snap.trim();
        if !snap_clean.is_empty() && !candidates.contains(&snap_clean.to_string()) {
            candidates.push(snap_clean.to_string());
        }
    }
    let fallback_backend_url = format!("{}/api/translations/snapshot", backend_url);
    if !candidates.contains(&fallback_backend_url) {
        candidates.push(fallback_backend_url);
    }

    let mut downloaded_bytes: Option<Vec<u8>> = None;
    let mut last_error = String::new();

    for candidate_url in candidates {
        tracing::info!("[Translations] Downloading consensus dictionary from: {}", candidate_url);
        let req = if candidate_url.starts_with(&backend_url) {
            secure_request(client, reqwest::Method::GET, &candidate_url, "")
        } else {
            client.get(&candidate_url)
        };

        match req.timeout(std::time::Duration::from_secs(30)).send().await {
            Ok(resp) if resp.status().is_success() => {
                match resp.bytes().await {
                    Ok(b) => {
                        downloaded_bytes = Some(b.to_vec());
                        break;
                    }
                    Err(e) => {
                        last_error = format!("Failed to read bytes from {}: {}", candidate_url, e);
                    }
                }
            }
            Ok(resp) => {
                last_error = format!("{} returned status {}", candidate_url, resp.status());
            }
            Err(e) => {
                last_error = format!("Request to {} failed: {}", candidate_url, e);
            }
        }
    }

    let bytes = downloaded_bytes
        .ok_or_else(|| format!("Failed to download translation package from all candidate sources: {}", last_error))?;

    let entries = tokio::task::spawn_blocking(move || decode_snapshot_bytes(&bytes))
        .await
        .map_err(|e| e.to_string())??;

    let version_to_save = if !target_version.is_empty() && !target_version.starts_with("v-1794") && target_version != "latest" {
        target_version
    } else {
        chrono::Utc::now().format("v%Y.%m.%d").to_string()
    };
    let updated_to_save = meta.updated_at.max(now_unix_secs());

    tokio::task::spawn_blocking(move || {
        let db = TranslationDb::open_default()?;
        db.upsert_entries_batch(&entries)?;
        db.set_meta("snapshot_version", &version_to_save)?;
        db.set_meta("last_sync_at", &updated_to_save.to_string())?;
        db.get_stats()
    })
    .await
    .map_err(|e| e.to_string())?
}

fn decode_snapshot_bytes(bytes: &[u8]) -> Result<Vec<TranslationEntry>, String> {
    if bytes.len() >= 2 && bytes[0] == 0x1f && bytes[1] == 0x8b {
        let mut decoder = GzDecoder::new(bytes);
        let mut decoded = String::new();
        decoder
            .read_to_string(&mut decoded)
            .map_err(|e| format!("Failed to gunzip snapshot: {}", e))?;
        serde_json::from_str(&decoded).map_err(|e| format!("Invalid gunzipped snapshot JSON: {}", e))
    } else {
        serde_json::from_slice(bytes).map_err(|e| format!("Invalid snapshot JSON: {}", e))
    }
}

/// Save a user-defined Chinese translation override locally.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn set_custom_resource_translation(
    category: AssetCategory,
    source: Option<String>,
    project_id: Option<String>,
    slug: Option<String>,
    raw_name: Option<String>,
    zh_name: String,
    instance_id: Option<String>,
    rel_path: Option<String>,
) -> Result<TranslationLookupResult, String> {
    let trimmed_zh = zh_name.trim().to_string();
    if trimmed_zh.is_empty() {
        return Err("Translation name cannot be empty".to_string());
    }
    let short = extract_short_zh_name(&trimmed_zh);
    let cat_key = category_to_key(&category).to_string();
    let src_lower = source.clone().unwrap_or_default().to_ascii_lowercase();

    let mut effective_project_id = project_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    // Sanitize polluted modpack project IDs and update assets.json display_name_zh when instance_id is supplied
    if let Some(iid) = instance_id.as_deref() {
        if crate::core::fs_utils::is_safe_single_name(iid) {
            let _lock = crate::core::manager::MANIFEST_LOCK.lock().await;
            let instance_dir = crate::core::mojang::get_minecraft_base()
                .join("versions")
                .join(iid);
            let assets_path = instance_dir.join("assets.json");
            if assets_path.exists() {
                if let Ok(content) = tokio::fs::read_to_string(&assets_path).await {
                    if let Ok(mut manifest) =
                        serde_json::from_str::<crate::models::instance::AssetManifest>(&content)
                    {
                        let (polluted, mut changed) =
                            crate::core::resource_rename_ops::sanitize_polluted_modpack_manifest(
                                &instance_dir,
                                &mut manifest,
                            )
                            .await;
                        if let Some(ref pid) = effective_project_id {
                            if polluted.contains(pid) {
                                effective_project_id = None;
                            }
                        }
                        if let Some(rp) = rel_path.as_deref() {
                            if let Some(record) = manifest.assets.get_mut(rp) {
                                record.display_name_zh = Some(trimmed_zh.clone());
                                changed = true;
                            }
                        }
                        if changed {
                            if let Ok(json_str) = serde_json::to_string_pretty(&manifest) {
                                let _ = crate::core::fs_utils::write_file_atomic(
                                    &assets_path,
                                    json_str.as_bytes(),
                                )
                                .await;
                            }
                        }
                    }
                }
            }
        }
    }

    let clean_slug = slug
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("examplemod"))
        .map(str::to_string);
    let clean_raw_name = raw_name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    if effective_project_id.is_some() || clean_slug.is_some() || clean_raw_name.is_some() {
        let (cf_pid, mr_pid) = match src_lower.as_str() {
            "curseforge" => (effective_project_id.clone(), None),
            "modrinth" => (None, effective_project_id.clone()),
            _ => (None, None),
        };
        let (cf_slug, mr_slug) = match src_lower.as_str() {
            "curseforge" => (clean_slug.clone(), None),
            "modrinth" => (None, clean_slug.clone()),
            _ => (clean_slug.clone(), clean_slug.clone()),
        };

        let entry = TranslationEntry {
            category: cat_key,
            cf_project_id: cf_pid,
            cf_slug,
            mr_project_id: mr_pid,
            mr_slug,
            mod_ids: String::new(),
            raw_name: clean_raw_name.unwrap_or_default(),
            zh_name: trimmed_zh.clone(),
            zh_short_name: short.clone(),
            zh_aliases: String::new(),
            zh_summary: String::new(),
            wiki_id: 0,
            trust_level: TRUST_USER,
            source_tag: "user".to_string(),
            updated_at: now_unix_secs(),
        };

        tokio::task::spawn_blocking(move || {
            let db = TranslationDb::open_default()?;
            db.upsert_entries_batch(&[entry])
        })
        .await
        .map_err(|e| e.to_string())??;
    }

    Ok(TranslationLookupResult {
        zh_name: trimmed_zh,
        zh_short_name: short,
        zh_summary: None,
        wiki_id: None,
        trust_level: TRUST_USER,
        source_tag: "user".to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_golden_consensus_seed_accuracy() {
        let dir = tempdir().unwrap();
        let db = TranslationDb::open_at(dir.path().join("translations.db")).unwrap();
        db.seed_builtin_consensus().unwrap();

        let cases = [
            (AssetCategory::Mod, "create", "机械动力"),
            (AssetCategory::Mod, "applied-energistics-2", "应用能源2"),
            (AssetCategory::Mod, "mekanism", "通用机械"),
            (AssetCategory::Mod, "botania", "植物魔法"),
            (AssetCategory::Mod, "tinkers-construct", "匠魂"),
            (AssetCategory::Mod, "farmers-delight", "农夫乐事"),
            (AssetCategory::Mod, "waystones", "传送石碑"),
            (AssetCategory::Mod, "appleskin", "苹果皮"),
            (AssetCategory::Mod, "sophisticated-backpacks", "精妙背包"),
            (AssetCategory::ShaderPack, "complementary-reimagined", "互补光影重制版"),
            (AssetCategory::ResourcePack, "fresh-animations", "灵动动画"),
        ];

        for (cat, slug, expected_short) in cases {
            let found = db
                .lookup(&cat, None, None, Some(slug), None, None)
                .unwrap_or_else(|| panic!("Missing golden consensus entry for {}", slug));
            assert_eq!(found.zh_short_name, expected_short);
            assert_eq!(found.trust_level, TRUST_VERIFIED);
        }
    }

    #[test]
    fn test_trust_level_monotonic_protection() {
        let dir = tempdir().unwrap();
        let db = TranslationDb::open_at(dir.path().join("translations.db")).unwrap();

        // 1. Insert AI draft first
        let ai_entry = TranslationEntry {
            category: "mod".to_string(),
            cf_project_id: None,
            cf_slug: Some("demo-mod".to_string()),
            mr_project_id: None,
            mr_slug: Some("demo-mod".to_string()),
            mod_ids: String::new(),
            raw_name: "Demo Mod".to_string(),
            zh_name: "演示模组机翻".to_string(),
            zh_short_name: "演示模组机翻".to_string(),
            zh_aliases: String::new(),
            zh_summary: String::new(),
            wiki_id: 0,
            trust_level: TRUST_AI_DRAFT,
            source_tag: "remote_ai".to_string(),
            updated_at: 100,
        };
        db.upsert_entries_batch(&[ai_entry]).unwrap();

        // 2. Verified entry MUST upgrade AI draft
        let verified_entry = TranslationEntry {
            category: "mod".to_string(),
            cf_project_id: None,
            cf_slug: Some("demo-mod".to_string()),
            mr_project_id: None,
            mr_slug: Some("demo-mod".to_string()),
            mod_ids: String::new(),
            raw_name: "Demo Mod".to_string(),
            zh_name: "公认模组名".to_string(),
            zh_short_name: "公认模组名".to_string(),
            zh_aliases: String::new(),
            zh_summary: String::new(),
            wiki_id: 999,
            trust_level: TRUST_VERIFIED,
            source_tag: "consensus".to_string(),
            updated_at: 200,
        };
        db.upsert_entries_batch(std::slice::from_ref(&verified_entry)).unwrap();
        let res = db
            .lookup(&AssetCategory::Mod, None, None, Some("demo-mod"), None, None)
            .unwrap();
        assert_eq!(res.zh_short_name, "公认模组名");
        assert_eq!(res.trust_level, TRUST_VERIFIED);

        // 3. User override MUST NOT be overwritten by verified entry
        let user_entry = TranslationEntry {
            zh_name: "我的自定义名".to_string(),
            zh_short_name: "我的自定义名".to_string(),
            trust_level: TRUST_USER,
            source_tag: "user".to_string(),
            updated_at: 300,
            ..verified_entry.clone()
        };
        db.upsert_entries_batch(&[user_entry]).unwrap();
        db.upsert_entries_batch(&[verified_entry]).unwrap();

        let res2 = db
            .lookup(&AssetCategory::Mod, None, None, Some("demo-mod"), None, None)
            .unwrap();
        assert_eq!(res2.zh_short_name, "我的自定义名");
        assert_eq!(res2.trust_level, TRUST_USER);
    }

    #[test]
    fn test_chinese_search_candidates() {
        let dir = tempdir().unwrap();
        let db = TranslationDb::open_at(dir.path().join("translations.db")).unwrap();
        db.seed_builtin_consensus().unwrap();

        let hits = db.search_candidates(&AssetCategory::Mod, "机械动力", 10);
        assert!(!hits.is_empty());
        assert_eq!(hits[0].mr_slug.as_deref(), Some("create"));

        let alias_hits = db.search_candidates(&AssetCategory::Mod, "AE2", 10);
        assert!(alias_hits
            .iter()
            .any(|c| c.cf_slug.as_deref() == Some("applied-energistics-2")));
    }

    #[test]
    fn test_lookup_mod_identity_verified_and_conflict_guards() {
        let dir = tempdir().unwrap();
        let db = TranslationDb::open_at(dir.path().join("translations.db")).unwrap();

        let verified_iris = TranslationEntry {
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
        };

        let ai_draft_mod = TranslationEntry {
            category: "mod".to_string(),
            cf_project_id: Some("999001".to_string()),
            cf_slug: Some("ai-only-mod".to_string()),
            mr_project_id: Some("aiOnlyMr".to_string()),
            mr_slug: Some("ai-only-mod".to_string()),
            mod_ids: "ai_only".to_string(),
            raw_name: "AI Only Mod".to_string(),
            zh_name: "机翻模组".to_string(),
            zh_short_name: "机翻模组".to_string(),
            zh_aliases: String::new(),
            zh_summary: String::new(),
            wiki_id: 0,
            trust_level: TRUST_AI_DRAFT,
            source_tag: "remote_ai".to_string(),
            updated_at: 100,
        };

        db.upsert_entries_batch(&[verified_iris, ai_draft_mod])
            .unwrap();

        // Insert two rows sharing `cf_slug = 'shared-slug'` directly (bypassing upsert_entries_batch
        // deduplication) to simulate a slug collision in a populated translations database.
        {
            let conn = db.connect().unwrap();
            conn.execute(
                "INSERT INTO resource_translations (
                    category, cf_project_id, cf_slug, mr_project_id, mr_slug,
                    mod_ids, raw_name, zh_name, zh_short_name, zh_aliases,
                    zh_summary, wiki_id, trust_level, source_tag, updated_at
                ) VALUES ('mod', '111111', 'shared-slug', 'mrOther', 'other-mr-slug', 'other_mod', 'Other Mod', '其他模组', '其他模组', '', '', 1, ?1, 'user', 200)",
                params![TRUST_USER as i64],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO resource_translations (
                    category, cf_project_id, cf_slug, mr_project_id, mr_slug,
                    mod_ids, raw_name, zh_name, zh_short_name, zh_aliases,
                    zh_summary, wiki_id, trust_level, source_tag, updated_at
                ) VALUES ('mod', NULL, 'shared-slug', 'mrSharedTarget', 'shared-target', 'shared_target', 'Shared Target', '目标模组', '目标模组', '', '', 2, ?1, 'consensus', 100)",
                params![TRUST_VERIFIED as i64],
            )
            .unwrap();
        }

        // 1. Exact project_id lookup returns full cross-platform hints
        let by_pid = db
            .lookup_mod_identity("modrinth", "YL57xq9U", "iris")
            .unwrap()
            .expect("should resolve by exact mr_project_id");
        assert_eq!(by_pid.cf_project_id.as_deref(), Some("455508"));
        assert_eq!(by_pid.cf_slug.as_deref(), Some("irisshaders"));
        assert_eq!(by_pid.mr_project_id.as_deref(), Some("YL57xq9U"));
        assert_eq!(by_pid.mr_slug.as_deref(), Some("iris"));
        assert_eq!(by_pid.mod_ids, vec!["iris".to_string()]);
        assert_eq!(by_pid.raw_name.as_deref(), Some("Iris Shaders"));

        // 2. Fallback to exact slug on the SAME platform when project_id is absent
        let by_slug = db
            .lookup_mod_identity("curseforge", "", "IrIsShAdErS")
            .unwrap()
            .expect("should resolve by exact case-insensitive cf_slug");
        assert_eq!(by_slug.mr_project_id.as_deref(), Some("YL57xq9U"));

        // 3. Rejects cross-column slug matches (cf_slug must NOT match a Modrinth lookup)
        assert_eq!(
            db.lookup_mod_identity("modrinth", "", "irisshaders")
                .unwrap(),
            None
        );

        // 4. Rejects unknown/unrecognized sources
        assert_eq!(
            db.lookup_mod_identity("unknown", "455508", "irisshaders")
                .unwrap(),
            None
        );

        // 5. Rejects TRUST_AI_DRAFT rows
        assert_eq!(
            db.lookup_mod_identity("curseforge", "999001", "ai-only-mod")
                .unwrap(),
            None
        );
        assert_eq!(
            db.lookup_mod_identity("modrinth", "", "ai-only-mod")
                .unwrap(),
            None
        );

        // 6. Rejects slug fallback when same-platform project_id conflicts and no compatible row exists
        assert_eq!(
            db.lookup_mod_identity("modrinth", "DIFFERENT_MR_ID", "iris")
                .unwrap(),
            None
        );
        assert_eq!(
            db.lookup_mod_identity("curseforge", "123456", "irisshaders")
                .unwrap(),
            None
        );

        // 7. On a slug collision where a higher-trust row has a conflicting cf_project_id,
        // the SQL filter skips it and still resolves the compatible row.
        let by_non_conflicting_slug = db
            .lookup_mod_identity("curseforge", "222222", "shared-slug")
            .unwrap()
            .expect("conflicting higher-trust row must not shadow compatible row");
        assert_eq!(
            by_non_conflicting_slug.mr_project_id.as_deref(),
            Some("mrSharedTarget")
        );

        // 8. Verify EXPLAIN QUERY PLAN uses the LOWER(cf_slug)/LOWER(mr_slug) expression indexes
        let conn = db.connect().unwrap();
        let mut cf_stmt = conn
            .prepare(
                "EXPLAIN QUERY PLAN
                 SELECT cf_project_id, cf_slug, mr_project_id, mr_slug, mod_ids, raw_name
                 FROM resource_translations
                 WHERE category = 'mod'
                   AND LOWER(cf_slug) = LOWER(?1)
                   AND trust_level >= ?2
                   AND (?3 = '' OR cf_project_id IS NULL OR cf_project_id = '' OR cf_project_id = ?3)
                 ORDER BY trust_level DESC LIMIT 1",
            )
            .unwrap();
        let cf_plan: Vec<String> = cf_stmt
            .query_map(params!["irisshaders", TRUST_VERIFIED as i64, "455508"], |r| {
                r.get::<_, String>(3)
            })
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert!(
            cf_plan.iter().any(|d| d.contains("idx_rt_cf_slug_lower")),
            "expected idx_rt_cf_slug_lower in CF query plan: {:?}",
            cf_plan
        );

        let mut mr_stmt = conn
            .prepare(
                "EXPLAIN QUERY PLAN
                 SELECT cf_project_id, cf_slug, mr_project_id, mr_slug, mod_ids, raw_name
                 FROM resource_translations
                 WHERE category = 'mod'
                   AND LOWER(mr_slug) = LOWER(?1)
                   AND trust_level >= ?2
                   AND (?3 = '' OR mr_project_id IS NULL OR mr_project_id = '' OR mr_project_id = ?3)
                 ORDER BY trust_level DESC LIMIT 1",
            )
            .unwrap();
        let mr_plan: Vec<String> = mr_stmt
            .query_map(params!["iris", TRUST_VERIFIED as i64, "YL57xq9U"], |r| {
                r.get::<_, String>(3)
            })
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert!(
            mr_plan.iter().any(|d| d.contains("idx_rt_mr_slug_lower")),
            "expected idx_rt_mr_slug_lower in MR query plan: {:?}",
            mr_plan
        );
    }
}
