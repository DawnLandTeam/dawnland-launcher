use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

pub mod download;

pub use download::run_batch_download;

/// A single download task.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadTask {
    /// Unique identifier for this task.
    pub id: String,
    /// URL to download from.
    pub url: String,
    /// Destination file path (absolute).
    pub dest_path: String,
    /// Optional SHA-256 hash for verification.
    pub hash: Option<String>,
}

impl DownloadTask {
    pub fn new(url: String, dest_path: String, hash: Option<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            url,
            dest_path,
            hash,
        }
    }

    pub fn dest_path_buf(&self) -> PathBuf {
        PathBuf::from(&self.dest_path)
    }
}

/// Progress payload sent to frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgress {
    /// Task ID.
    pub task_id: String,
    /// Downloaded bytes so far.
    pub downloaded: u64,
    /// Total bytes (0 if unknown).
    pub total: u64,
    /// Current download speed in bytes per second.
    pub speed: u64,
    /// Whether this task is complete.
    pub completed: bool,
    /// Error message if failed.
    pub error: Option<String>,
}

impl DownloadProgress {
    pub fn progress(task_id: String, downloaded: u64, total: u64, speed: u64) -> Self {
        Self {
            task_id,
            downloaded,
            total,
            speed,
            completed: false,
            error: None,
        }
    }

    pub fn completed(task_id: String) -> Self {
        Self {
            task_id,
            downloaded: 0,
            total: 0,
            speed: 0,
            completed: true,
            error: None,
        }
    }

    pub fn failed(task_id: String, error: String) -> Self {
        Self {
            task_id,
            downloaded: 0,
            total: 0,
            speed: 0,
            completed: true,
            error: Some(error),
        }
    }
}