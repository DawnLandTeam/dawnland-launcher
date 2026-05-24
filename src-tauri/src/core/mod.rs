pub mod mojang;
pub mod launcher;
pub mod fabric;
pub mod manager;
pub mod java;

// Re-export types from mojang for easier importing
pub use crate::core::mojang::{InstallState, VanillaVersion};