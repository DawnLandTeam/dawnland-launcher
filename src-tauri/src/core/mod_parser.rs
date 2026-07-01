use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use tracing::{debug, error, info};

#[derive(Debug, Clone, Default)]
pub struct ModMetadata {
    pub mod_id: Option<String>,
    pub name: Option<String>,
    pub version: Option<String>,
    pub has_icon: bool,
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
                has_icon INTEGER
            )",
            [],
        )?;

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
            if let Ok(mut stmt) =
                conn.prepare("SELECT cache_key, mod_id, name, version, has_icon FROM mod_cache")
            {
                if let Ok(mut rows) = stmt.query([]) {
                    while let Ok(Some(row)) = rows.next() {
                        if let Ok(key) = row.get::<_, String>(0) {
                            map.insert(
                                key,
                                ModMetadata {
                                    mod_id: row.get(1).ok(),
                                    name: row.get(2).ok(),
                                    version: row.get(3).ok(),
                                    has_icon: row.get::<_, i32>(4).unwrap_or(0) == 1,
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

    pub fn get_icon_path(&self, cache_key: &str) -> PathBuf {
        self.icons_dir.join(format!("{}.png", cache_key))
    }

    pub fn parse_mod(&self, file_path: &Path, cache_key: &str) -> ModMetadata {
        let mut meta = ModMetadata {
            mod_id: None,
            name: None,
            version: None,
            has_icon: false,
        };

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

        let mut has_fabric = false;
        let mut content = String::new();

        let mut found = false;
        if let Ok(mut f) = archive.by_name("fabric.mod.json") {
            let _ = f.read_to_string(&mut content);
            found = true;
        }
        if !found {
            if let Ok(mut f) = archive.by_name("quilt.mod.json") {
                let _ = f.read_to_string(&mut content);
                found = true;
            }
        }
        has_fabric = found;

        if has_fabric && !content.is_empty() {
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

                if let Some(icon_path) = json.get("icon").and_then(|v| v.as_str()) {
                    let icon_path = icon_path.trim_start_matches('/');
                    let icon_name = meta.mod_id.as_deref().unwrap_or(cache_key);
                    let dest_path = self.get_icon_path(icon_name);

                    if dest_path.exists() {
                        meta.has_icon = true;
                    } else if let Ok(mut icon_file) = archive.by_name(icon_path) {
                        let mut buffer = Vec::new();
                        if icon_file.read_to_end(&mut buffer).is_ok()
                            && fs::write(dest_path, &buffer).is_ok() {
                                meta.has_icon = true;
                            }
                    }
                }
            }
            return meta;
        }

        let mut has_forge = false;
        let mut content = String::new();

        let mut found = false;
        if let Ok(mut f) = archive.by_name("META-INF/mods.toml") {
            let _ = f.read_to_string(&mut content);
            found = true;
        }
        if !found {
            if let Ok(mut f) = archive.by_name("META-INF/neoforge.mods.toml") {
                let _ = f.read_to_string(&mut content);
                found = true;
            }
        }
        has_forge = found;

        if has_forge && !content.is_empty() {
            if let Ok(toml_val) = content.parse::<toml::Value>() {
                if let Some(mods) = toml_val
                    .get("mods")
                    .and_then(|m| m.as_array())
                    .and_then(|arr| arr.first())
                {
                    meta.mod_id = mods
                        .get("modId")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    meta.name = mods
                        .get("displayName")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    meta.version = mods
                        .get("version")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());

                    if let Some(logo_path) = mods.get("logoFile").and_then(|v| v.as_str()) {
                        let logo_path = logo_path.trim_start_matches('/');
                        let icon_name = meta.mod_id.as_deref().unwrap_or(cache_key);
                        let dest_path = self.get_icon_path(icon_name);

                        if dest_path.exists() {
                            meta.has_icon = true;
                        } else if let Ok(mut icon_file) = archive.by_name(logo_path) {
                            let mut buffer = Vec::new();
                            if icon_file.read_to_end(&mut buffer).is_ok()
                                && fs::write(dest_path, &buffer).is_ok() {
                                    meta.has_icon = true;
                                }
                        }
                    }
                }
            }
        }

        meta
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn test_mod_parser_cache_db() {
        let dir = tempdir().unwrap();
        let parser = ModParser::new(dir.path());
        
        let meta = ModMetadata {
            mod_id: Some("test_mod".to_string()),
            name: Some("Test Mod".to_string()),
            version: Some("1.0.0".to_string()),
            has_icon: true,
        };

        parser.set_cache_entry("hash123", &meta);
        
        let cache = parser.load_all_cache();
        assert_eq!(cache.len(), 1);
        let loaded = cache.get("hash123").unwrap();
        
        assert_eq!(loaded.mod_id, Some("test_mod".to_string()));
        assert_eq!(loaded.name, Some("Test Mod".to_string()));
        assert_eq!(loaded.version, Some("1.0.0".to_string()));
        assert_eq!(loaded.has_icon, true);
    }
}
