use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::core::error::AppError;

/// Base path for all Flaase data on the server.
pub const FLAASE_BASE_PATH: &str = "/opt/flaase";
pub const FLAASE_CONFIG_PATH: &str = "/opt/flaase/config.yml";
pub const FLAASE_APPS_PATH: &str = "/opt/flaase/apps";
pub const FLAASE_TRAEFIK_PATH: &str = "/opt/flaase/traefik";
pub const FLAASE_TRAEFIK_DYNAMIC_PATH: &str = "/opt/flaase/traefik/dynamic";

/// Server-level configuration stored in /opt/flaase/config.yml
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub server: ServerInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerInfo {
    /// Email for SSL certificate notifications (Let's Encrypt).
    pub email: String,

    /// When the server was initialized.
    pub created_at: DateTime<Utc>,

    /// Last time the server config was updated.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<DateTime<Utc>>,

    /// Detected operating system.
    pub os: OsInfo,

    /// Container runtime information.
    pub container_runtime: ContainerRuntimeInfo,

    /// Reverse proxy information.
    pub reverse_proxy: ReverseProxyInfo,

    /// Deploy user information.
    pub deploy_user: DeployUserInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OsInfo {
    pub name: String,
    pub version: String,
    pub codename: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerRuntimeInfo {
    /// Runtime type: "docker" or "kubernetes" (future).
    #[serde(rename = "type")]
    pub runtime_type: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReverseProxyInfo {
    /// Proxy type: "traefik", "nginx", "caddy" (future).
    #[serde(rename = "type")]
    pub proxy_type: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeployUserInfo {
    pub username: String,
    pub uid: u32,
    pub gid: u32,
}

impl ServerConfig {
    /// Creates a new server configuration.
    pub fn new(
        email: String,
        os: OsInfo,
        container_runtime: ContainerRuntimeInfo,
        reverse_proxy: ReverseProxyInfo,
        deploy_user: DeployUserInfo,
    ) -> Self {
        Self {
            server: ServerInfo {
                email,
                created_at: Utc::now(),
                updated_at: None,
                os,
                container_runtime,
                reverse_proxy,
                deploy_user,
            },
        }
    }

    /// Loads the server configuration from disk.
    pub fn load() -> Result<Self, AppError> {
        let path = Path::new(FLAASE_CONFIG_PATH);

        if !path.exists() {
            return Err(AppError::Config(
                "Server not initialized. Run 'fl server init' first.".into(),
            ));
        }

        let content = std::fs::read_to_string(path)
            .map_err(|e| AppError::Config(format!("Failed to read config: {}", e)))?;

        serde_yaml::from_str(&content)
            .map_err(|e| AppError::Config(format!("Failed to parse config: {}", e)))
    }

    /// Saves the server configuration to disk.
    pub fn save(&self) -> Result<(), AppError> {
        let content = serde_yaml::to_string(self)
            .map_err(|e| AppError::Config(format!("Failed to serialize config: {}", e)))?;

        std::fs::write(FLAASE_CONFIG_PATH, content)
            .map_err(|e| AppError::Config(format!("Failed to write config: {}", e)))
    }

    /// Checks if the server has been initialized.
    pub fn is_initialized() -> bool {
        Path::new(FLAASE_CONFIG_PATH).exists()
    }

    /// Returns all directory paths that should be created.
    pub fn required_directories() -> Vec<&'static str> {
        vec![
            FLAASE_BASE_PATH,
            FLAASE_APPS_PATH,
            FLAASE_TRAEFIK_PATH,
            FLAASE_TRAEFIK_DYNAMIC_PATH,
        ]
    }
}

/// Represents the action to take when a component is already installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExistingComponentAction {
    /// Skip installation, keep existing.
    Skip,
    /// Update to latest version.
    Update,
    /// Reinstall from scratch.
    Reinstall,
}

impl std::fmt::Display for ExistingComponentAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Skip => write!(f, "Skip"),
            Self::Update => write!(f, "Update"),
            Self::Reinstall => write!(f, "Reinstall"),
        }
    }
}
