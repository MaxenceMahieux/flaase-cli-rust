use thiserror::Error;

/// Application-level errors for Flaase operations.
#[derive(Debug, Error)]
pub enum AppError {
    #[error("App '{0}' not found")]
    AppNotFound(String),

    #[error("App '{0}' already exists")]
    AppAlreadyExists(String),

    #[error("Invalid app name '{0}': {1}")]
    InvalidAppName(String, String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Docker error: {0}")]
    Docker(String),

    #[error("Git error: {0}")]
    Git(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}
