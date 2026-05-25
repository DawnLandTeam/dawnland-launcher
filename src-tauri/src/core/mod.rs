pub mod mojang;
pub mod launcher;
pub mod fabric;
pub mod manager;
pub mod java;
pub mod curseforge;
pub mod modrinth;

// Re-export types from mojang for easier importing
pub use crate::core::mojang::{InstallState, VanillaVersion};

// Re-export curseforge types
pub use crate::core::curseforge::UnifiedModProject;