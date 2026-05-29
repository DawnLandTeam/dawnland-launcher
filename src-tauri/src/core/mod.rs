pub mod mojang;
pub mod launcher;
pub mod fabric;
pub mod forge;
pub mod manager;
pub mod java;
pub mod curseforge;
pub mod modrinth;
pub mod server;
pub mod utils;

// Re-export types from mojang for easier importing
pub use crate::core::mojang::{InstallState, VanillaVersion};

// Re-export modrinth types
pub use crate::core::modrinth::{UnifiedModProject, UnifiedModFile};