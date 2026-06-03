pub mod curseforge;
pub mod fabric;
pub mod forge;
pub mod java;
pub mod launcher;
pub mod manager;
pub mod modpack;
pub mod modrinth;
pub mod mojang;
pub mod ping;
pub mod server;
pub mod utils;

// Re-export types from mojang for easier importing
pub use crate::core::mojang::{InstallState, VanillaVersion};

// Re-export modrinth types
pub use crate::core::modrinth::{OnlineModpackVersion, UnifiedModFile, UnifiedModProject};
