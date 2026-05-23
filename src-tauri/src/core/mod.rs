pub mod mojang;
pub mod launcher;

// Re-export types from mojang for easier importing
pub use crate::core::mojang::{InstallState, VanillaVersion, InstallPhase};