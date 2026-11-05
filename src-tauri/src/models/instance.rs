use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum InstanceStatus {
    #[default]
    Ready,
    Running,
    Installing,
    CancellingInstall,
    Updating,
    CancellingUpdate,
}

impl<'de> Deserialize<'de> for InstanceStatus {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct InstanceStatusVisitor;

        impl<'de> serde::de::Visitor<'de> for InstanceStatusVisitor {
            type Value = InstanceStatus;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a valid or fallback instance status string")
            }

            fn visit_str<E>(self, value: &str) -> Result<InstanceStatus, E>
            where
                E: serde::de::Error,
            {
                match value {
                    "ready" | "idle" => Ok(InstanceStatus::Ready),
                    "running" => Ok(InstanceStatus::Running),
                    "installing" => Ok(InstanceStatus::Installing),
                    "cancellingInstall" => Ok(InstanceStatus::CancellingInstall),
                    "updating" => Ok(InstanceStatus::Updating),
                    "cancellingUpdate" => Ok(InstanceStatus::CancellingUpdate),
                    _ => Ok(InstanceStatus::Ready),
                }
            }

            fn visit_unit<E>(self) -> Result<InstanceStatus, E>
            where
                E: serde::de::Error,
            {
                Ok(InstanceStatus::Ready)
            }

            fn visit_none<E>(self) -> Result<InstanceStatus, E>
            where
                E: serde::de::Error,
            {
                Ok(InstanceStatus::Ready)
            }
        }

        deserializer.deserialize_any(InstanceStatusVisitor)
    }
}

impl InstanceStatus {
    /// Returns true if the instance is actively undergoing an installation, update, or cancellation task.
    pub fn is_busy(&self) -> bool {
        matches!(
            self,
            Self::Installing | Self::CancellingInstall | Self::Updating | Self::CancellingUpdate
        )
    }

    pub fn is_running(&self) -> bool {
        matches!(self, Self::Running)
    }

    pub fn is_ready(&self) -> bool {
        matches!(self, Self::Ready)
    }

    pub fn can_transition_to(&self, next: InstanceStatus) -> bool {
        match (*self, next) {
            (a, b) if a == b => true,
            // From Ready: can launch to Running, or start installing/updating
            (Self::Ready, Self::Running | Self::Installing | Self::Updating) => true,
            // From Running: can exit back to Ready
            (Self::Running, Self::Ready) => true,
            // From Installing: can finish to Ready, or cancel to CancellingInstall
            (Self::Installing, Self::Ready | Self::CancellingInstall) => true,
            // From CancellingInstall: can finish cleanup to Ready
            (Self::CancellingInstall, Self::Ready) => true,
            // From Updating: can finish to Ready, or cancel to CancellingUpdate
            (Self::Updating, Self::Ready | Self::CancellingUpdate) => true,
            // From CancellingUpdate: can finish rollback to Ready
            (Self::CancellingUpdate, Self::Ready) => true,
            // Recovery/Reset: any state can be recovered to Ready
            (_, Self::Ready) => true,
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
        let mut status = InstanceStatus::Ready;
        assert!(!status.is_busy());
        assert!(status.is_ready());
        assert!(!status.is_running());

        // Ready -> Running -> Ready
        assert!(status.transition_to(InstanceStatus::Running).is_ok());
        assert!(status.is_running());
        assert!(!status.is_busy());
        assert!(status.transition_to(InstanceStatus::Ready).is_ok());
        assert!(status.is_ready());

        // Ready -> Installing -> Ready
        assert!(status.transition_to(InstanceStatus::Installing).is_ok());
        assert!(status.is_busy());
        assert!(status.transition_to(InstanceStatus::Ready).is_ok());
        assert!(!status.is_busy());

        // Ready -> Installing -> CancellingInstall -> Ready
        assert!(status.transition_to(InstanceStatus::Installing).is_ok());
        assert!(status.transition_to(InstanceStatus::CancellingInstall).is_ok());
        assert!(status.is_busy());
        assert!(status.transition_to(InstanceStatus::Ready).is_ok());

        // Ready -> Updating -> CancellingUpdate -> Ready
        assert!(status.transition_to(InstanceStatus::Updating).is_ok());
        assert!(status.transition_to(InstanceStatus::CancellingUpdate).is_ok());
        assert!(status.is_busy());
        assert!(status.transition_to(InstanceStatus::Ready).is_ok());

        // Invalid transitions
        assert!(status.transition_to(InstanceStatus::CancellingUpdate).is_err());
        assert!(status.transition_to(InstanceStatus::CancellingInstall).is_err());
    }

    #[test]
    fn test_instance_status_lenient_deserialization() {
        let parsed_unknown: InstanceStatus = serde_json::from_str("\"someUnknownStatus\"").unwrap();
        assert_eq!(parsed_unknown, InstanceStatus::Ready);

        let parsed_idle: InstanceStatus = serde_json::from_str("\"idle\"").unwrap();
        assert_eq!(parsed_idle, InstanceStatus::Ready);

        let parsed_null: InstanceStatus = serde_json::from_str("null").unwrap();
        assert_eq!(parsed_null, InstanceStatus::Ready);

        let parsed_updating: InstanceStatus = serde_json::from_str("\"updating\"").unwrap();
        assert_eq!(parsed_updating, InstanceStatus::Updating);
    }
}
