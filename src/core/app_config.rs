//! Application configuration management.

use std::fmt;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::core::error::AppError;
use crate::core::FLAASE_APPS_PATH;

/// Application configuration stored in /opt/flaase/apps/<name>/config.yml
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub name: String,
    pub repository: String,
    pub ssh_key: PathBuf,
    pub stack: Stack,
    pub domain: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub database: Option<DatabaseConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache: Option<CacheConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health_check: Option<HealthCheckConfig>,
    pub autodeploy: bool,
    /// Detailed autodeploy configuration (webhook settings).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub autodeploy_config: Option<AutodeployConfig>,
    pub created_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deployed_at: Option<DateTime<Utc>>,
}

impl AppConfig {
    /// Creates a new app configuration.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        name: String,
        repository: String,
        ssh_key: PathBuf,
        stack: Stack,
        domain: String,
        database: Option<DatabaseConfig>,
        cache: Option<CacheConfig>,
        autodeploy: bool,
    ) -> Self {
        Self {
            name,
            repository,
            ssh_key,
            stack,
            domain,
            port: None,
            database,
            cache,
            health_check: None,
            autodeploy,
            autodeploy_config: None,
            created_at: Utc::now(),
            deployed_at: None,
        }
    }

    /// Returns the effective port for this app.
    /// Uses configured port or stack default.
    pub fn effective_port(&self) -> u16 {
        self.port.unwrap_or_else(|| self.stack.default_port())
    }

    /// Returns the health check configuration with defaults.
    pub fn effective_health_check(&self) -> HealthCheckConfig {
        self.health_check.clone().unwrap_or_default()
    }

    /// Returns the app directory path.
    pub fn app_dir(&self) -> PathBuf {
        PathBuf::from(format!("{}/{}", FLAASE_APPS_PATH, self.name))
    }

    /// Returns the config file path.
    pub fn config_path(&self) -> PathBuf {
        self.app_dir().join("config.yml")
    }

    /// Returns the .env file path (user variables).
    pub fn env_path(&self) -> PathBuf {
        self.app_dir().join(".env")
    }

    /// Returns the .env.auto file path (auto-generated variables).
    pub fn auto_env_path(&self) -> PathBuf {
        self.app_dir().join(".env.auto")
    }

    /// Returns the .secrets file path.
    pub fn secrets_path(&self) -> PathBuf {
        self.app_dir().join(".secrets")
    }

    /// Returns the repo directory path.
    pub fn repo_path(&self) -> PathBuf {
        self.app_dir().join("repo")
    }

    /// Returns the data directory path.
    pub fn data_path(&self) -> PathBuf {
        self.app_dir().join("data")
    }

    /// Returns the deployments history file path.
    pub fn deployments_path(&self) -> PathBuf {
        self.app_dir().join("deployments.json")
    }

    /// Loads an app configuration from disk.
    pub fn load(name: &str) -> Result<Self, AppError> {
        let config_path = format!("{}/{}/config.yml", FLAASE_APPS_PATH, name);
        let path = Path::new(&config_path);

        if !path.exists() {
            return Err(AppError::AppNotFound(name.to_string()));
        }

        let content = std::fs::read_to_string(path)
            .map_err(|e| AppError::Config(format!("Failed to read app config: {}", e)))?;

        serde_yaml::from_str(&content)
            .map_err(|e| AppError::Config(format!("Failed to parse app config: {}", e)))
    }

    /// Saves the app configuration to disk.
    pub fn save(&self) -> Result<(), AppError> {
        let content = serde_yaml::to_string(self)
            .map_err(|e| AppError::Config(format!("Failed to serialize app config: {}", e)))?;

        std::fs::write(self.config_path(), content)
            .map_err(|e| AppError::Config(format!("Failed to write app config: {}", e)))
    }

    /// Lists all configured apps.
    pub fn list_all() -> Result<Vec<String>, AppError> {
        let apps_path = Path::new(FLAASE_APPS_PATH);

        if !apps_path.exists() {
            return Ok(Vec::new());
        }

        let mut apps = Vec::new();

        let entries = std::fs::read_dir(apps_path)
            .map_err(|e| AppError::Config(format!("Failed to read apps directory: {}", e)))?;

        for entry in entries {
            let entry =
                entry.map_err(|e| AppError::Config(format!("Failed to read entry: {}", e)))?;

            if entry.path().is_dir() {
                if let Some(name) = entry.file_name().to_str() {
                    // Check if config.yml exists
                    if entry.path().join("config.yml").exists() {
                        apps.push(name.to_string());
                    }
                }
            }
        }

        apps.sort();
        Ok(apps)
    }
}

/// Application stack type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Stack {
    #[serde(rename = "nextjs")]
    NextJs,
    #[serde(rename = "nodejs")]
    NodeJs,
    #[serde(rename = "nestjs")]
    NestJs,
    Laravel,
}

impl Stack {
    /// Returns all available stacks.
    pub fn all() -> &'static [Stack] {
        &[Stack::NextJs, Stack::NodeJs, Stack::NestJs, Stack::Laravel]
    }

    /// Returns the display name.
    pub fn display_name(&self) -> &str {
        match self {
            Stack::NextJs => "Next.js",
            Stack::NodeJs => "Node.js",
            Stack::NestJs => "NestJS",
            Stack::Laravel => "Laravel",
        }
    }

    /// Returns the default port for this stack.
    pub fn default_port(&self) -> u16 {
        match self {
            Stack::NextJs => 3000,
            Stack::NodeJs => 3000,
            Stack::NestJs => 3000,
            Stack::Laravel => 8000,
        }
    }
}

impl fmt::Display for Stack {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

/// Database configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    #[serde(rename = "type")]
    pub db_type: DatabaseType,
    pub name: String,
}

impl DatabaseConfig {
    pub fn new(db_type: DatabaseType, app_name: &str) -> Self {
        // Convert app-name to app_name for database name
        let db_name = app_name.replace('-', "_");
        Self {
            db_type,
            name: db_name,
        }
    }
}

/// Database type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DatabaseType {
    PostgreSQL,
    MySQL,
    MongoDB,
}

impl DatabaseType {
    /// Returns all available database types.
    pub fn all() -> &'static [DatabaseType] {
        &[
            DatabaseType::PostgreSQL,
            DatabaseType::MySQL,
            DatabaseType::MongoDB,
        ]
    }

    /// Returns the display name.
    pub fn display_name(&self) -> &str {
        match self {
            DatabaseType::PostgreSQL => "PostgreSQL",
            DatabaseType::MySQL => "MySQL",
            DatabaseType::MongoDB => "MongoDB",
        }
    }

    /// Returns the default port.
    pub fn default_port(&self) -> u16 {
        match self {
            DatabaseType::PostgreSQL => 5432,
            DatabaseType::MySQL => 3306,
            DatabaseType::MongoDB => 27017,
        }
    }

    /// Returns the Docker image.
    pub fn docker_image(&self) -> &str {
        match self {
            DatabaseType::PostgreSQL => "postgres:16-alpine",
            DatabaseType::MySQL => "mysql:8",
            DatabaseType::MongoDB => "mongo:7",
        }
    }

    /// Returns the environment variable name for the connection URL.
    pub fn url_env_var(&self) -> &str {
        match self {
            DatabaseType::PostgreSQL => "DATABASE_URL",
            DatabaseType::MySQL => "DATABASE_URL",
            DatabaseType::MongoDB => "MONGODB_URL",
        }
    }
}

impl fmt::Display for DatabaseType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

/// Cache configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    #[serde(rename = "type")]
    pub cache_type: CacheType,
}

impl CacheConfig {
    pub fn new(cache_type: CacheType) -> Self {
        Self { cache_type }
    }
}

/// Cache type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CacheType {
    Redis,
}

impl CacheType {
    /// Returns all available cache types.
    pub fn all() -> &'static [CacheType] {
        &[CacheType::Redis]
    }

    /// Returns the display name.
    pub fn display_name(&self) -> &str {
        match self {
            CacheType::Redis => "Redis",
        }
    }

    /// Returns the default port.
    pub fn default_port(&self) -> u16 {
        match self {
            CacheType::Redis => 6379,
        }
    }

    /// Returns the Docker image.
    pub fn docker_image(&self) -> &str {
        match self {
            CacheType::Redis => "redis:7-alpine",
        }
    }

    /// Returns the environment variable name for the connection URL.
    pub fn url_env_var(&self) -> &str {
        match self {
            CacheType::Redis => "REDIS_URL",
        }
    }
}

impl fmt::Display for CacheType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

/// Health check configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheckConfig {
    /// HTTP endpoint to check (default: "/health" or "/").
    #[serde(default = "HealthCheckConfig::default_endpoint")]
    pub endpoint: String,
    /// Timeout in seconds for each check (default: 30).
    #[serde(default = "HealthCheckConfig::default_timeout")]
    pub timeout: u32,
    /// Number of retries before marking as unhealthy (default: 3).
    #[serde(default = "HealthCheckConfig::default_retries")]
    pub retries: u32,
    /// Interval between retries in seconds (default: 5).
    #[serde(default = "HealthCheckConfig::default_interval")]
    pub interval: u32,
}

impl HealthCheckConfig {
    fn default_endpoint() -> String {
        "/".to_string()
    }

    fn default_timeout() -> u32 {
        30
    }

    fn default_retries() -> u32 {
        3
    }

    fn default_interval() -> u32 {
        5
    }
}

impl Default for HealthCheckConfig {
    fn default() -> Self {
        Self {
            endpoint: Self::default_endpoint(),
            timeout: Self::default_timeout(),
            retries: Self::default_retries(),
            interval: Self::default_interval(),
        }
    }
}

/// Domain configuration with optional authentication.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainConfig {
    pub domain: String,
    #[serde(default)]
    pub primary: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth: Option<DomainAuth>,
}

impl DomainConfig {
    pub fn new(domain: &str, primary: bool) -> Self {
        Self {
            domain: domain.to_string(),
            primary,
            auth: None,
        }
    }

    pub fn with_auth(mut self, username: &str) -> Self {
        self.auth = Some(DomainAuth {
            enabled: true,
            username: username.to_string(),
        });
        self
    }
}

/// Domain authentication configuration.
/// Password hash is stored in the secrets file, not here.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainAuth {
    pub enabled: bool,
    pub username: String,
}

/// Autodeploy configuration for an app.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutodeployConfig {
    /// Whether autodeploy is enabled.
    pub enabled: bool,
    /// Branch to watch for deployments.
    #[serde(default = "AutodeployConfig::default_branch")]
    pub branch: String,
    /// Webhook endpoint path (unique per app).
    pub webhook_path: String,
}

impl AutodeployConfig {
    fn default_branch() -> String {
        "main".to_string()
    }

    pub fn new(webhook_path: &str) -> Self {
        Self {
            enabled: true,
            branch: Self::default_branch(),
            webhook_path: webhook_path.to_string(),
        }
    }

    pub fn with_branch(mut self, branch: &str) -> Self {
        self.branch = branch.to_string();
        self
    }
}
