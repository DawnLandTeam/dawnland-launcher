#![allow(dead_code)]
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[tokio::test]
    async fn test_compute_sha1() {
        let mut temp_file = NamedTempFile::new().unwrap();
        write!(temp_file, "hello world").unwrap();
        
        // sha1("hello world") = 2aae6c35c94fcfb415dbe95f408b9ce91ee846ed
        let hash = compute_sha1(temp_file.path()).await.unwrap();
        assert_eq!(hash, "2aae6c35c94fcfb415dbe95f408b9ce91ee846ed");
    }

    #[tokio::test]
    async fn test_compute_sha256() {
        let mut temp_file = NamedTempFile::new().unwrap();
        write!(temp_file, "hello world").unwrap();
        
        // sha256("hello world") = b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9
        let hash = compute_sha256(temp_file.path()).await.unwrap();
        assert_eq!(hash, "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9");
    }

    #[tokio::test]
    async fn test_is_file_valid_sha1() {
        let mut temp_file = NamedTempFile::new().unwrap();
        write!(temp_file, "hello world").unwrap();
        let path = temp_file.path();

        // Valid hash
        assert!(is_file_valid_sha1(path, "2aae6c35c94fcfb415dbe95f408b9ce91ee846ed").await);
        // Case-insensitive check
        assert!(is_file_valid_sha1(path, "2AAE6C35C94FCFB415DBE95F408B9CE91EE846ED").await);
        // Invalid hash
        assert!(!is_file_valid_sha1(path, "wrong_hash").await);
        // Empty hash (always valid by implementation)
        assert!(is_file_valid_sha1(path, "").await);
    }

    #[tokio::test]
    async fn test_atomic_move_success() {
        let temp_dir = tempfile::tempdir().unwrap();
        let source_path = temp_dir.path().join("source.txt");
        let dest_path = temp_dir.path().join("subdir").join("dest.txt");

        fs::write(&source_path, "move me").await.unwrap();

        // Perform move
        atomic_move(&source_path, &dest_path, false).await.unwrap();

        assert!(!source_path.exists());
        assert!(dest_path.exists());
        let content = fs::read_to_string(&dest_path).await.unwrap();
        assert_eq!(content, "move me");
    }

    #[tokio::test]
    async fn test_atomic_move_no_overwrite() {
        let temp_dir = tempfile::tempdir().unwrap();
        let source_path = temp_dir.path().join("source.txt");
        let dest_path = temp_dir.path().join("dest.txt");

        fs::write(&source_path, "new content").await.unwrap();
        fs::write(&dest_path, "old content").await.unwrap();

        // Should fail because overwrite is false
        let result = atomic_move(&source_path, &dest_path, false).await;
        assert!(matches!(result, Err(FsError::TargetExists)));
        
        let content = fs::read_to_string(&dest_path).await.unwrap();
        assert_eq!(content, "old content");
    }
}
