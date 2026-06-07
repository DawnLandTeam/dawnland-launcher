use sha1::{Digest as Sha1Digest, Sha1};
use sha2::{Digest as Sha2Digest, Sha256};
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::io::AsyncReadExt;

#[derive(Debug, thiserror::Error)]
pub enum FsError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Hash mismatch: expected {expected}, got {actual}")]
    HashMismatch { expected: String, actual: String },
    #[error("Target already exists and overwrite is false")]
    TargetExists,
}

/// Computes SHA1 hash of a file asynchronously
pub async fn compute_sha1(path: impl AsRef<Path>) -> Result<String, FsError> {
    let mut file = fs::File::open(path).await?;
    let mut hasher = Sha1::new();
    let mut buffer = [0; 8192];
    
    loop {
        let n = file.read(&mut buffer).await?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    
    Ok(hex::encode(hasher.finalize()))
}

/// Computes SHA256 hash of a file asynchronously
pub async fn compute_sha256(path: impl AsRef<Path>) -> Result<String, FsError> {
    let mut file = fs::File::open(path).await?;
    let mut hasher = <Sha256 as Sha2Digest>::new();
    let mut buffer = [0; 8192];
    
    loop {
        let n = file.read(&mut buffer).await?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    
    Ok(hex::encode(hasher.finalize()))
}

/// Atomically move a file from a temporary location to the final destination.
/// Creates parent directories if they don't exist.
pub async fn atomic_move(temp_path: impl AsRef<Path>, final_path: impl AsRef<Path>, overwrite: bool) -> Result<(), FsError> {
    let temp_path = temp_path.as_ref();
    let final_path = final_path.as_ref();

    if !overwrite && final_path.exists() {
        return Err(FsError::TargetExists);
    }

    if let Some(parent) = final_path.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent).await?;
        }
    }

    // Attempt atomic rename first
    match fs::rename(temp_path, final_path).await {
        Ok(_) => Ok(()),
        Err(e) => {
            // Cross-device link error (EXDEV) happens when moving across partitions
            // Fallback to copy and delete
            if e.raw_os_error() == Some(18) || e.kind() == std::io::ErrorKind::CrossesDevices {
                fs::copy(temp_path, final_path).await?;
                fs::remove_file(temp_path).await?;
                Ok(())
            } else {
                Err(FsError::Io(e))
            }
        }
    }
}

/// Checks if a file is valid against a given SHA1 hash.
pub async fn is_file_valid_sha1(path: impl AsRef<Path>, expected_hash: &str) -> bool {
    let path = path.as_ref();
    if !path.exists() || path.metadata().map(|m| m.len() == 0).unwrap_or(true) {
        return false;
    }
    
    if expected_hash.is_empty() {
        return true;
    }
    
    match compute_sha1(path).await {
        Ok(hash) => hash.eq_ignore_ascii_case(expected_hash),
        Err(_) => false,
    }
}
