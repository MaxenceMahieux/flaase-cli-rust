use thiserror::Error;

/// Application-level errors for Flaase operations.
#[derive(Debug, Error)]
pub enum AppError {
    // App errors
    #[error("App '{0}' not found")]
    AppNotFound(String),

    #[error("App '{0}' already exists")]
    AppAlreadyExists(String),

    #[error("Invalid app name '{0}': {1}")]
    InvalidAppName(String, String),

    // Configuration errors
    #[error("Configuration error: {0}")]
    Config(String),

    // System errors
    #[error("Unsupported operating system: {0}")]
    UnsupportedOs(String),

    #[error("This command requires root privileges. Please run with sudo.")]
    RequiresRoot,

    #[error("Command execution failed: {0}")]
    Command(String),

    // Provider errors
    #[error("Docker error: {0}")]
    Docker(String),

    #[error("Package manager error: {0}")]
    PackageManager(String),

    #[error("Firewall error: {0}")]
    Firewall(String),

    #[error("Reverse proxy error: {0}")]
    ReverseProxy(String),

    #[error("User management error: {0}")]
    UserManagement(String),

    // SSH errors
    #[error("SSH error: {0}")]
    Ssh(String),

    // Validation errors
    #[error("Validation error: {0}")]
    Validation(String),

    // Other errors
    #[error("Git error: {0}")]
    Git(String),

    #[error("Template error: {0}")]
    Template(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Deployment error: {0}")]
    Deploy(String),

    #[error("Operation cancelled by user")]
    Cancelled,
}
