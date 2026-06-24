use serde::Serialize;

#[derive(thiserror::Error, Debug)]
pub enum DawnlandError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Network(#[from] reqwest::Error),
    #[error("{0}")]
    ZipError(String),
    #[error("{0}")]
    ProcessError(String),
    #[error("{0}")]
    JsonError(#[from] serde_json::Error),
    #[error("{0}")]
    InstanceNotFound(String),
    #[error("No compatible Java {required_version} found")]
    NoCompatibleJava { required_version: u32 },
    #[error("Update file is corrupted (MD5 mismatch). Update aborted.")]
    Md5Mismatch,
    #[error("{0}")]
    Unknown(String),
}

impl From<String> for DawnlandError {
    fn from(s: String) -> Self {
        DawnlandError::Unknown(s)
    }
}

#[derive(Debug, Serialize)]
pub struct AppError {
    pub code: String,
    pub message: String,
}

impl From<DawnlandError> for AppError {
    fn from(err: DawnlandError) -> Self {
        let code = match &err {
            DawnlandError::Io(e) => match e.kind() {
                std::io::ErrorKind::NotFound => "FILE_NOT_FOUND",
                std::io::ErrorKind::PermissionDenied => "PERMISSION_DENIED",
                _ => "IO_ERROR",
            },
            DawnlandError::Network(_) => "NETWORK_ERROR",
            DawnlandError::ZipError(_) => "ZIP_EXTRACTION_FAILED",
            DawnlandError::ProcessError(_) => "PROCESS_ERROR",
            DawnlandError::JsonError(_) => "JSON_PARSE_ERROR",
            DawnlandError::InstanceNotFound(_) => "INSTANCE_NOT_FOUND",
            DawnlandError::NoCompatibleJava { .. } => "NO_COMPATIBLE_JAVA",
            DawnlandError::Md5Mismatch => "MD5_MISMATCH",
            DawnlandError::Unknown(_) => "UNKNOWN_ERROR",
        };

        AppError {
            code: code.to_string(),
            message: err.to_string(),
        }
    }
}

// Convert standard errors directly to AppError to use `?` smoothly
impl From<std::io::Error> for AppError {
    fn from(err: std::io::Error) -> Self {
        DawnlandError::Io(err).into()
    }
}

impl From<reqwest::Error> for AppError {
    fn from(err: reqwest::Error) -> Self {
        DawnlandError::Network(err).into()
    }
}

impl From<serde_json::Error> for AppError {
    fn from(err: serde_json::Error) -> Self {
        DawnlandError::JsonError(err).into()
    }
}

impl From<String> for AppError {
    fn from(err: String) -> Self {
        DawnlandError::Unknown(err).into()
    }
}
