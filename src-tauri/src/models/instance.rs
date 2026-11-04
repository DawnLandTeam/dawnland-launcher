use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum InstanceStatus {
    #[default]
    Idle,
    Installing,
    CancellingInstall,
    Updating,
    CancellingUpdate,
}

impl InstanceStatus {
    pub fn is_busy(&self) -> bool {
        !matches!(self, Self::Idle)
    }

    pub fn can_transition_to(&self, next: InstanceStatus) -> bool {
        match (*self, next) {
            (a, b) if a == b => true,
            // From Idle: can start installing or updating
            (Self::Idle, Self::Installing | Self::Updating) => true,
            // From Installing: can finish to Idle, or cancel to CancellingInstall
            (Self::Installing, Self::Idle | Self::CancellingInstall) => true,
            // From CancellingInstall: can finish cleanup to Idle
            (Self::CancellingInstall, Self::Idle) => true,
            // From Updating: can finish to Idle, or cancel to CancellingUpdate
            (Self::Updating, Self::Idle | Self::CancellingUpdate) => true,
            // From CancellingUpdate: can finish rollback to Idle
            (Self::CancellingUpdate, Self::Idle) => true,
            // Recovery/Reset: any state can be recovered to Idle
            (_, Self::Idle) => true,
            _ => false,
        }
    }

    pub fn transition_to(&mut self, next: InstanceStatus) -> Result<(), String> {
        if self.can_transition_to(next) {
            *self = next;
            Ok(())
        } else {
            Err(format!(
                "Invalid instance status transition from {:?} to {:?}",
                self, next
            ))
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum LoaderType {
    #[default]
    Vanilla,
    Forge,
    Fabric,
    NeoForge,
    Quilt,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AssetCategory {
    Mod,
    ResourcePack,
    ShaderPack,
    Datapack,
    Save,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AssetSourceType {
    CurseForge,
    Modrinth,
    CustomUrl,
    LocalImport,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetRecord {
    pub category: AssetCategory,
    pub source_type: AssetSourceType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub download_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hash_sha1: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hash_sha512: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub managed_by_modpack: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AssetManifest {
    #[serde(deserialize_with = "deserialize_assets_and_migrate")]
    pub assets: HashMap<String, AssetRecord>,
    #[serde(default)]
    pub overrides: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AssetRecordProxy {
    category: AssetCategory,
    source_type: AssetSourceType,
    project_id: Option<String>,
    version_id: Option<String>,
    download_url: Option<String>,
    hash_sha1: Option<String>,
    hash_sha512: Option<String>,
    size: Option<u64>,
    #[serde(default = "default_true")]
    enabled: bool,
    managed_by_modpack: Option<bool>,
}

fn deserialize_assets_and_migrate<'de, D>(deserializer: D) -> Result<HashMap<String, AssetRecord>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let proxy_map: HashMap<String, AssetRecordProxy> = HashMap::deserialize(deserializer)?;
    let mut map = HashMap::new();
    for (k, proxy) in proxy_map {
        // Only apply the heuristic if the field was genuinely MISSING from the JSON.
        // If it was explicitly saved as `false`, it stays `false`.
        let managed = proxy.managed_by_modpack.unwrap_or_else(|| {
            proxy.version_id.is_some()
        });
        
        map.insert(k, AssetRecord {
            category: proxy.category,
            source_type: proxy.source_type,
            project_id: proxy.project_id,
            version_id: proxy.version_id,
            download_url: proxy.download_url,
            hash_sha1: proxy.hash_sha1,
            hash_sha512: proxy.hash_sha512,
            size: proxy.size,
            enabled: proxy.enabled,
            managed_by_modpack: managed,
        });
    }
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_instance_status_transitions() {
        let mut status = InstanceStatus::Idle;
        assert!(!status.is_busy());

        // Idle -> Installing -> Idle
        assert!(status.transition_to(InstanceStatus::Installing).is_ok());
        assert!(status.is_busy());
        assert!(status.transition_to(InstanceStatus::Idle).is_ok());
        assert!(!status.is_busy());

        // Idle -> Installing -> CancellingInstall -> Idle
        assert!(status.transition_to(InstanceStatus::Installing).is_ok());
        assert!(status.transition_to(InstanceStatus::CancellingInstall).is_ok());
        assert!(status.is_busy());
        assert!(status.transition_to(InstanceStatus::Idle).is_ok());

        // Idle -> Updating -> CancellingUpdate -> Idle
        assert!(status.transition_to(InstanceStatus::Updating).is_ok());
        assert!(status.transition_to(InstanceStatus::CancellingUpdate).is_ok());
        assert!(status.is_busy());
        assert!(status.transition_to(InstanceStatus::Idle).is_ok());

        // Invalid transitions
        assert!(status.transition_to(InstanceStatus::CancellingUpdate).is_err());
        assert!(status.transition_to(InstanceStatus::CancellingInstall).is_err());
    }
}
