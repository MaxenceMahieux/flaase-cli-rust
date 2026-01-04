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

    /// Deployment type: source (git) or image (docker registry).
    #[serde(default)]
    pub deployment_type: DeploymentType,

    // === Source deployment fields (optional if image) ===
    /// Git repository URL (required for source deployments).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    /// SSH key path for git operations (required for source deployments).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssh_key: Option<PathBuf>,
    /// Application stack (required for source deployments).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stack: Option<Stack>,
    /// Detailed stack configuration for customizable stacks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stack_config: Option<StackConfig>,

    // === Image deployment fields (optional if source) ===
    /// Docker image configuration (required for image deployments).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<ImageConfig>,
    /// Volume mounts for the container.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub volumes: Vec<VolumeMount>,

    // === Common fields ===
    /// Legacy single domain field (for backward compatibility).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    /// List of domains for this app.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub domains: Vec<DomainConfig>,
    /// Port the application listens on.
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
    /// Creates a new source-based app configuration (from Git repository).
    #[allow(clippy::too_many_arguments)]
    pub fn new_source(
        name: String,
        repository: String,
        ssh_key: PathBuf,
        stack: Stack,
        stack_config: Option<StackConfig>,
        domain: String,
        port: Option<u16>,
        database: Option<DatabaseConfig>,
        cache: Option<CacheConfig>,
        autodeploy: bool,
    ) -> Self {
        Self {
            name,
            deployment_type: DeploymentType::Source,
            repository: Some(repository),
            ssh_key: Some(ssh_key),
            stack: Some(stack),
            stack_config,
            image: None,
            volumes: Vec::new(),
            domain: None,
            domains: vec![DomainConfig::new(&domain, true)],
            port,
            database,
            cache,
            health_check: None,
            autodeploy,
            autodeploy_config: None,
            created_at: Utc::now(),
            deployed_at: None,
        }
    }

    /// Creates a new image-based app configuration (from Docker registry).
    #[allow(clippy::too_many_arguments)]
    pub fn new_image(
        name: String,
        image: ImageConfig,
        domain: String,
        port: u16,
        volumes: Vec<VolumeMount>,
        database: Option<DatabaseConfig>,
        cache: Option<CacheConfig>,
        health_check: Option<HealthCheckConfig>,
    ) -> Self {
        Self {
            name,
            deployment_type: DeploymentType::Image,
            repository: None,
            ssh_key: None,
            stack: None,
            stack_config: None,
            image: Some(image),
            volumes,
            domain: None,
            domains: vec![DomainConfig::new(&domain, true)],
            port: Some(port),
            database,
            cache,
            health_check,
            autodeploy: false,
            autodeploy_config: None,
            created_at: Utc::now(),
            deployed_at: None,
        }
    }

    /// Returns whether this is a source-based deployment.
    pub fn is_source_deployment(&self) -> bool {
        matches!(self.deployment_type, DeploymentType::Source)
    }

    /// Returns whether this is an image-based deployment.
    pub fn is_image_deployment(&self) -> bool {
        matches!(self.deployment_type, DeploymentType::Image)
    }

    /// Returns the registry credentials path for image deployments.
    pub fn registry_auth_path(&self) -> PathBuf {
        self.app_dir().join(".registry-auth")
    }

    /// Returns the primary domain for this app.
    pub fn primary_domain(&self) -> &str {
        // First check domains vector
        if let Some(primary) = self.domains.iter().find(|d| d.primary) {
            return &primary.domain;
        }
        // Fall back to first domain in vector
        if let Some(first) = self.domains.first() {
            return &first.domain;
        }
        // Legacy fallback to single domain field
        self.domain.as_deref().unwrap_or("localhost")
    }

    /// Returns all domains for this app (including legacy single domain).
    pub fn all_domains(&self) -> Vec<&DomainConfig> {
        self.domains.iter().collect()
    }

    /// Adds a domain to this app.
    pub fn add_domain(&mut self, domain: &str) {
        self.domains.push(DomainConfig::new(domain, false));
    }

    /// Removes a domain from this app. Returns true if removed.
    pub fn remove_domain(&mut self, domain: &str) -> bool {
        if let Some(idx) = self.domains.iter().position(|d| d.domain == domain) {
            self.domains.remove(idx);
            return true;
        }
        false
    }

    /// Checks if a domain is configured for this app.
    pub fn has_domain(&mut self, domain: &str) -> bool {
        self.domains.iter().any(|d| d.domain == domain)
    }

    /// Migrates legacy single-domain config to multi-domain format.
    fn migrate_domains(&mut self) {
        if self.domains.is_empty() {
            if let Some(domain) = self.domain.take() {
                self.domains.push(DomainConfig::new(&domain, true));
            }
        }
    }

    /// Returns the effective port for this app.
    /// Uses configured port, stack default, or 8080 for image deployments.
    pub fn effective_port(&self) -> u16 {
        self.port.unwrap_or_else(|| {
            self.stack
                .as_ref()
                .map(|s| s.default_port())
                .unwrap_or(8080)
        })
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
    /// Automatically migrates legacy single-domain configs to multi-domain format.
    pub fn load(name: &str) -> Result<Self, AppError> {
        let config_path = format!("{}/{}/config.yml", FLAASE_APPS_PATH, name);
        let path = Path::new(&config_path);

        if !path.exists() {
            return Err(AppError::AppNotFound(name.to_string()));
        }

        let content = std::fs::read_to_string(path)
            .map_err(|e| AppError::Config(format!("Failed to read app config: {}", e)))?;

        let mut config: Self = serde_yaml::from_str(&content)
            .map_err(|e| AppError::Config(format!("Failed to parse app config: {}", e)))?;

        // Migrate legacy single-domain to multi-domain format
        config.migrate_domains();

        Ok(config)
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
    Python,
    Go,
    Ruby,
    Rust,
    Java,
    Php,
    Static,
    /// User provides their own Dockerfile
    Dockerfile,
}

impl Stack {
    /// Returns all available stacks.
    pub fn all() -> &'static [Stack] {
        &[
            Stack::NextJs,
            Stack::NodeJs,
            Stack::NestJs,
            Stack::Laravel,
            Stack::Python,
            Stack::Go,
            Stack::Ruby,
            Stack::Rust,
            Stack::Java,
            Stack::Php,
            Stack::Static,
            Stack::Dockerfile,
        ]
    }

    /// Returns the display name.
    pub fn display_name(&self) -> &str {
        match self {
            Stack::NextJs => "Next.js",
            Stack::NodeJs => "Node.js",
            Stack::NestJs => "NestJS",
            Stack::Laravel => "Laravel",
            Stack::Python => "Python",
            Stack::Go => "Go",
            Stack::Ruby => "Ruby",
            Stack::Rust => "Rust",
            Stack::Java => "Java",
            Stack::Php => "PHP",
            Stack::Static => "Static (HTML/CSS/JS)",
            Stack::Dockerfile => "Custom Dockerfile",
        }
    }

    /// Returns the default port for this stack.
    pub fn default_port(&self) -> u16 {
        match self {
            Stack::NextJs => 3000,
            Stack::NodeJs => 3000,
            Stack::NestJs => 3000,
            Stack::Laravel => 8000,
            Stack::Python => 8000,
            Stack::Go => 8080,
            Stack::Ruby => 3000,
            Stack::Rust => 8080,
            Stack::Java => 8080,
            Stack::Php => 8000,
            Stack::Static => 80,
            Stack::Dockerfile => 8080,
        }
    }

    /// Returns whether this stack requires additional configuration.
    pub fn needs_config(&self) -> bool {
        matches!(
            self,
            Stack::Python | Stack::Go | Stack::Ruby | Stack::Rust | Stack::Java | Stack::Php | Stack::Static
        )
    }

    /// Returns whether this stack uses a user-provided Dockerfile.
    pub fn uses_custom_dockerfile(&self) -> bool {
        matches!(self, Stack::Dockerfile)
    }

    /// Returns whether this stack requires a start command to be specified.
    pub fn requires_start_command(&self) -> bool {
        matches!(
            self,
            Stack::Python | Stack::Go | Stack::Ruby | Stack::Rust | Stack::Java | Stack::Php | Stack::NodeJs
        )
    }

    /// Returns whether this stack has a build step.
    pub fn has_build_step(&self) -> bool {
        matches!(
            self,
            Stack::NextJs | Stack::NestJs | Stack::Rust | Stack::Go | Stack::Java
        )
    }

    /// Returns a placeholder for the default start command.
    pub fn default_start_command(&self) -> Option<&'static str> {
        match self {
            Stack::Python => Some("python -m uvicorn main:app --host 0.0.0.0"),
            Stack::Go => Some("./app"),
            Stack::Ruby => Some("bundle exec rails server"),
            Stack::Rust => Some("./app"),
            Stack::Java => Some("java -jar app.jar"),
            Stack::Php => Some("php-fpm"),
            Stack::NodeJs => Some("node dist/index.js"),
            _ => None,
        }
    }

    /// Returns a placeholder for the default build command.
    pub fn default_build_command(&self) -> Option<&'static str> {
        match self {
            Stack::NextJs => Some("npm run build"),
            Stack::NestJs => Some("npm run build"),
            Stack::Rust => Some("cargo build --release"),
            Stack::Go => Some("go build -o app ."),
            Stack::Java => Some("mvn package -DskipTests"),
            _ => None,
        }
    }
}

impl fmt::Display for Stack {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

// ============================================================================
// Stack Configuration for Custom Stacks
// ============================================================================

/// Detailed stack configuration for customizable stacks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StackConfig {
    /// Runtime version (e.g., "3.12" for Python, "22" for Node)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Package manager used
    #[serde(skip_serializing_if = "Option::is_none")]
    pub package_manager: Option<PackageManager>,
    /// Detected or selected framework
    #[serde(skip_serializing_if = "Option::is_none")]
    pub framework: Option<Framework>,
    /// Build command (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build_command: Option<String>,
    /// Start command (required for most stacks)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_command: Option<String>,
    /// Install command override
    #[serde(skip_serializing_if = "Option::is_none")]
    pub install_command: Option<String>,
}

impl Default for StackConfig {
    fn default() -> Self {
        Self {
            version: None,
            package_manager: None,
            framework: None,
            build_command: None,
            start_command: None,
            install_command: None,
        }
    }
}

/// Package managers supported by Flaase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PackageManager {
    // Node.js
    Npm,
    Yarn,
    Pnpm,
    // Python
    Pip,
    Poetry,
    Pipenv,
    Uv,
    // Ruby
    Bundler,
    // PHP
    Composer,
    // Java
    Maven,
    Gradle,
    // Go
    GoMod,
    // Rust
    Cargo,
    // None (static sites)
    None,
}

impl PackageManager {
    /// Returns package managers for a given stack.
    pub fn for_stack(stack: Stack) -> &'static [PackageManager] {
        match stack {
            Stack::NextJs | Stack::NodeJs | Stack::NestJs => {
                &[PackageManager::Npm, PackageManager::Yarn, PackageManager::Pnpm]
            }
            Stack::Python => &[
                PackageManager::Pip,
                PackageManager::Poetry,
                PackageManager::Uv,
                PackageManager::Pipenv,
            ],
            Stack::Ruby => &[PackageManager::Bundler],
            Stack::Php | Stack::Laravel => &[PackageManager::Composer],
            Stack::Java => &[PackageManager::Maven, PackageManager::Gradle],
            Stack::Go => &[PackageManager::GoMod],
            Stack::Rust => &[PackageManager::Cargo],
            Stack::Static | Stack::Dockerfile => &[PackageManager::None],
        }
    }

    /// Returns the display name.
    pub fn display_name(&self) -> &str {
        match self {
            PackageManager::Npm => "npm",
            PackageManager::Yarn => "yarn",
            PackageManager::Pnpm => "pnpm",
            PackageManager::Pip => "pip (requirements.txt)",
            PackageManager::Poetry => "poetry (pyproject.toml)",
            PackageManager::Pipenv => "pipenv (Pipfile)",
            PackageManager::Uv => "uv (pyproject.toml)",
            PackageManager::Bundler => "bundler (Gemfile)",
            PackageManager::Composer => "composer",
            PackageManager::Maven => "maven (pom.xml)",
            PackageManager::Gradle => "gradle (build.gradle)",
            PackageManager::GoMod => "go modules",
            PackageManager::Cargo => "cargo",
            PackageManager::None => "none",
        }
    }

    /// Returns the lockfile name for this package manager.
    pub fn lockfile(&self) -> Option<&str> {
        match self {
            PackageManager::Npm => Some("package-lock.json"),
            PackageManager::Yarn => Some("yarn.lock"),
            PackageManager::Pnpm => Some("pnpm-lock.yaml"),
            PackageManager::Pip => None, // requirements.txt is not a lockfile
            PackageManager::Poetry => Some("poetry.lock"),
            PackageManager::Pipenv => Some("Pipfile.lock"),
            PackageManager::Uv => Some("uv.lock"),
            PackageManager::Bundler => Some("Gemfile.lock"),
            PackageManager::Composer => Some("composer.lock"),
            PackageManager::Maven => None,
            PackageManager::Gradle => Some("gradle.lockfile"),
            PackageManager::GoMod => Some("go.sum"),
            PackageManager::Cargo => Some("Cargo.lock"),
            PackageManager::None => None,
        }
    }
}

impl fmt::Display for PackageManager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

/// Detected or selected framework.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Framework {
    // Python
    Django,
    Flask,
    FastApi,
    // Ruby
    Rails,
    Sinatra,
    // PHP
    Symfony,
    // Java
    SpringBoot,
    Quarkus,
    // Go
    Gin,
    Echo,
    Fiber,
    Chi,
    // Rust
    Actix,
    Axum,
    Rocket,
    // Generic
    Express,
    Fastify,
    Hono,
    Other,
}

impl Framework {
    /// Returns frameworks for a given stack.
    pub fn for_stack(stack: Stack) -> &'static [Framework] {
        match stack {
            Stack::Python => &[Framework::Django, Framework::FastApi, Framework::Flask, Framework::Other],
            Stack::Ruby => &[Framework::Rails, Framework::Sinatra, Framework::Other],
            Stack::Php => &[Framework::Symfony, Framework::Other],
            Stack::Java => &[Framework::SpringBoot, Framework::Quarkus, Framework::Other],
            Stack::Go => &[Framework::Gin, Framework::Echo, Framework::Fiber, Framework::Chi, Framework::Other],
            Stack::Rust => &[Framework::Actix, Framework::Axum, Framework::Rocket, Framework::Other],
            Stack::NodeJs => &[Framework::Express, Framework::Fastify, Framework::Hono, Framework::Other],
            _ => &[],
        }
    }

    /// Returns the display name.
    pub fn display_name(&self) -> &str {
        match self {
            Framework::Django => "Django",
            Framework::Flask => "Flask",
            Framework::FastApi => "FastAPI",
            Framework::Rails => "Rails",
            Framework::Sinatra => "Sinatra",
            Framework::Symfony => "Symfony",
            Framework::SpringBoot => "Spring Boot",
            Framework::Quarkus => "Quarkus",
            Framework::Gin => "Gin",
            Framework::Echo => "Echo",
            Framework::Fiber => "Fiber",
            Framework::Chi => "Chi",
            Framework::Actix => "Actix Web",
            Framework::Axum => "Axum",
            Framework::Rocket => "Rocket",
            Framework::Express => "Express",
            Framework::Fastify => "Fastify",
            Framework::Hono => "Hono",
            Framework::Other => "Other / None",
        }
    }

    /// Returns default start command suggestion for this framework.
    pub fn default_start_command(&self, _port: u16) -> &'static str {
        match self {
            Framework::Django => "gunicorn config.wsgi:application --bind 0.0.0.0:8000",
            Framework::Flask => "gunicorn app:app --bind 0.0.0.0:8000",
            Framework::FastApi => "uvicorn main:app --host 0.0.0.0 --port 8000",
            Framework::Rails => "rails server -b 0.0.0.0 -p 3000",
            Framework::Sinatra => "ruby app.rb -o 0.0.0.0",
            Framework::Symfony => "php bin/console server:start 0.0.0.0:8000",
            Framework::SpringBoot => "java -jar target/*.jar",
            Framework::Quarkus => "./target/quarkus-app/quarkus-run.jar",
            Framework::Gin | Framework::Echo | Framework::Fiber | Framework::Chi => "./main",
            Framework::Actix | Framework::Axum | Framework::Rocket => "./app",
            Framework::Express | Framework::Fastify | Framework::Hono => "node dist/index.js",
            Framework::Other => "",
        }
    }
}

impl fmt::Display for Framework {
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
    /// Branch to watch for deployments (used when environments is not configured).
    #[serde(default = "AutodeployConfig::default_branch")]
    pub branch: String,
    /// Webhook endpoint path (unique per app).
    pub webhook_path: String,
    /// Rate limiting configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_limit: Option<RateLimitConfig>,
    /// Notification configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notifications: Option<NotificationConfig>,
    /// Test execution configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tests: Option<TestConfig>,
    /// Pre/post deployment hooks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hooks: Option<HooksConfig>,
    /// Rollback configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rollback: Option<RollbackConfig>,
    /// Multi-environment configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environments: Option<Vec<EnvironmentConfig>>,
    /// Manual approval gates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval: Option<ApprovalConfig>,
    /// Docker build configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build: Option<BuildConfig>,
    /// Blue-green deployment configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blue_green: Option<BlueGreenConfig>,
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
            rate_limit: Some(RateLimitConfig::default()),
            notifications: None,
            tests: None,
            hooks: None,
            rollback: None,
            environments: None,
            approval: None,
            build: None,
            blue_green: None,
        }
    }

    pub fn with_branch(mut self, branch: &str) -> Self {
        self.branch = branch.to_string();
        self
    }
}

/// Rate limiting configuration for autodeploy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    /// Whether rate limiting is enabled.
    #[serde(default = "RateLimitConfig::default_enabled")]
    pub enabled: bool,
    /// Maximum number of deployments allowed in the time window.
    #[serde(default = "RateLimitConfig::default_max_deploys")]
    pub max_deploys: u32,
    /// Time window in seconds for rate limiting.
    #[serde(default = "RateLimitConfig::default_window_seconds")]
    pub window_seconds: u64,
}

impl RateLimitConfig {
    fn default_enabled() -> bool {
        true
    }

    fn default_max_deploys() -> u32 {
        5
    }

    fn default_window_seconds() -> u64 {
        300 // 5 minutes
    }
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            enabled: Self::default_enabled(),
            max_deploys: Self::default_max_deploys(),
            window_seconds: Self::default_window_seconds(),
        }
    }
}

/// Notification configuration for autodeploy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationConfig {
    /// Whether notifications are enabled.
    #[serde(default)]
    pub enabled: bool,
    /// Slack webhook configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slack: Option<SlackNotificationConfig>,
    /// Discord webhook configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub discord: Option<DiscordNotificationConfig>,
    /// Email SMTP configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<EmailNotificationConfig>,
    /// Events to notify on.
    #[serde(default)]
    pub events: NotificationEvents,
}

impl Default for NotificationConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            slack: None,
            discord: None,
            email: None,
            events: NotificationEvents::default(),
        }
    }
}

/// Slack webhook configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlackNotificationConfig {
    /// Slack webhook URL.
    pub webhook_url: String,
    /// Optional channel override (uses webhook default if not set).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
    /// Optional username override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
}

/// Discord webhook configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordNotificationConfig {
    /// Discord webhook URL.
    pub webhook_url: String,
    /// Optional username override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
}

/// Email SMTP configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailNotificationConfig {
    /// SMTP server host.
    pub smtp_host: String,
    /// SMTP server port (default: 587 for TLS, 465 for SSL).
    #[serde(default = "EmailNotificationConfig::default_port")]
    pub smtp_port: u16,
    /// SMTP username for authentication.
    pub smtp_user: String,
    /// SMTP password for authentication.
    pub smtp_password: String,
    /// Sender email address.
    pub from_email: String,
    /// Sender name (optional).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_name: Option<String>,
    /// Recipient email addresses.
    pub to_emails: Vec<String>,
    /// Use STARTTLS (default: true).
    #[serde(default = "EmailNotificationConfig::default_starttls")]
    pub starttls: bool,
}

impl EmailNotificationConfig {
    fn default_port() -> u16 {
        587
    }

    fn default_starttls() -> bool {
        true
    }
}

/// Events to send notifications for.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationEvents {
    /// Notify on deployment start.
    #[serde(default = "NotificationEvents::default_on_start")]
    pub on_start: bool,
    /// Notify on deployment success.
    #[serde(default = "NotificationEvents::default_on_success")]
    pub on_success: bool,
    /// Notify on deployment failure.
    #[serde(default = "NotificationEvents::default_on_failure")]
    pub on_failure: bool,
}

impl NotificationEvents {
    fn default_on_start() -> bool {
        false
    }

    fn default_on_success() -> bool {
        true
    }

    fn default_on_failure() -> bool {
        true
    }
}

impl Default for NotificationEvents {
    fn default() -> Self {
        Self {
            on_start: Self::default_on_start(),
            on_success: Self::default_on_success(),
            on_failure: Self::default_on_failure(),
        }
    }
}

// ============================================================================
// CI/CD Configuration Structures
// ============================================================================

/// Test execution configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestConfig {
    /// Whether tests are enabled.
    #[serde(default = "TestConfig::default_enabled")]
    pub enabled: bool,
    /// Test command to run (e.g., "npm test", "composer test").
    #[serde(default = "TestConfig::default_command")]
    pub command: String,
    /// Timeout in seconds (default: 300 = 5 min).
    #[serde(default = "TestConfig::default_timeout")]
    pub timeout_seconds: u64,
    /// Whether to fail deployment if tests fail.
    #[serde(default = "TestConfig::default_fail_on_error")]
    pub fail_deployment_on_error: bool,
}

impl TestConfig {
    fn default_enabled() -> bool {
        true
    }

    fn default_command() -> String {
        "npm test".to_string()
    }

    fn default_timeout() -> u64 {
        300
    }

    fn default_fail_on_error() -> bool {
        true
    }
}

impl Default for TestConfig {
    fn default() -> Self {
        Self {
            enabled: Self::default_enabled(),
            command: Self::default_command(),
            timeout_seconds: Self::default_timeout(),
            fail_deployment_on_error: Self::default_fail_on_error(),
        }
    }
}

/// Pre/Post deployment hooks configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HooksConfig {
    /// Hooks to run before Docker build.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pre_build: Vec<HookCommand>,
    /// Hooks to run after build, before container swap.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pre_deploy: Vec<HookCommand>,
    /// Hooks to run after successful deployment.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub post_deploy: Vec<HookCommand>,
    /// Hooks to run on deployment failure.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub on_failure: Vec<HookCommand>,
}

/// Individual hook command.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookCommand {
    /// Human-readable name for the hook.
    pub name: String,
    /// Command to execute.
    pub command: String,
    /// Timeout in seconds.
    #[serde(default = "HookCommand::default_timeout")]
    pub timeout_seconds: u64,
    /// Whether deployment should fail if this hook fails.
    #[serde(default = "HookCommand::default_required")]
    pub required: bool,
    /// Run inside the app container (vs on host).
    #[serde(default)]
    pub run_in_container: bool,
}

impl HookCommand {
    fn default_timeout() -> u64 {
        60
    }

    fn default_required() -> bool {
        true
    }

    pub fn new(name: &str, command: &str) -> Self {
        Self {
            name: name.to_string(),
            command: command.to_string(),
            timeout_seconds: Self::default_timeout(),
            required: Self::default_required(),
            run_in_container: false,
        }
    }
}

/// Rollback configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollbackConfig {
    /// Whether rollback is enabled.
    #[serde(default = "RollbackConfig::default_enabled")]
    pub enabled: bool,
    /// Number of previous versions to keep.
    #[serde(default = "RollbackConfig::default_keep_versions")]
    pub keep_versions: u32,
    /// Whether to auto-rollback on health check failure.
    #[serde(default = "RollbackConfig::default_auto_rollback")]
    pub auto_rollback_on_failure: bool,
}

impl RollbackConfig {
    fn default_enabled() -> bool {
        true
    }

    fn default_keep_versions() -> u32 {
        3
    }

    fn default_auto_rollback() -> bool {
        true
    }
}

impl Default for RollbackConfig {
    fn default() -> Self {
        Self {
            enabled: Self::default_enabled(),
            keep_versions: Self::default_keep_versions(),
            auto_rollback_on_failure: Self::default_auto_rollback(),
        }
    }
}

/// Environment configuration for multi-environment deployments.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvironmentConfig {
    /// Environment name (e.g., "staging", "production").
    pub name: String,
    /// Git branch that triggers this environment.
    pub branch: String,
    /// Domains for this environment.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub domains: Vec<String>,
    /// Whether to auto-deploy or require approval.
    #[serde(default = "EnvironmentConfig::default_auto_deploy")]
    pub auto_deploy: bool,
}

impl EnvironmentConfig {
    fn default_auto_deploy() -> bool {
        true
    }

    pub fn new(name: &str, branch: &str) -> Self {
        Self {
            name: name.to_string(),
            branch: branch.to_string(),
            domains: Vec::new(),
            auto_deploy: Self::default_auto_deploy(),
        }
    }
}

/// Manual approval gates configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalConfig {
    /// Whether approval gates are enabled.
    #[serde(default)]
    pub enabled: bool,
    /// Timeout in minutes for approval requests.
    #[serde(default = "ApprovalConfig::default_timeout")]
    pub timeout_minutes: u64,
    /// Channels to notify for approval requests.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notify_channels: Vec<String>,
}

impl ApprovalConfig {
    fn default_timeout() -> u64 {
        60
    }
}

impl Default for ApprovalConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            timeout_minutes: Self::default_timeout(),
            notify_channels: Vec::new(),
        }
    }
}

/// Docker build optimization configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildConfig {
    /// Whether Docker build caching is enabled.
    #[serde(default = "BuildConfig::default_cache_enabled")]
    pub cache_enabled: bool,
    /// Whether to use BuildKit for improved caching.
    #[serde(default = "BuildConfig::default_buildkit")]
    pub buildkit: bool,
    /// Optional registry for cache-from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_from: Option<String>,
}

impl BuildConfig {
    fn default_cache_enabled() -> bool {
        true
    }

    fn default_buildkit() -> bool {
        true
    }
}

/// Blue-green deployment configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlueGreenConfig {
    /// Whether blue-green deployment is enabled.
    #[serde(default)]
    pub enabled: bool,
    /// How long to keep the old container running after switch (seconds).
    /// Set to 0 to stop immediately, or higher for instant rollback capability.
    #[serde(default = "BlueGreenConfig::default_keep_old_seconds")]
    pub keep_old_seconds: u64,
    /// Whether to auto-cleanup old container after keep_old_seconds.
    #[serde(default = "BlueGreenConfig::default_auto_cleanup")]
    pub auto_cleanup: bool,
}

impl Default for BlueGreenConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            keep_old_seconds: Self::default_keep_old_seconds(),
            auto_cleanup: Self::default_auto_cleanup(),
        }
    }
}

impl BlueGreenConfig {
    fn default_keep_old_seconds() -> u64 {
        300 // 5 minutes
    }

    fn default_auto_cleanup() -> bool {
        true
    }
}

impl Default for BuildConfig {
    fn default() -> Self {
        Self {
            cache_enabled: Self::default_cache_enabled(),
            buildkit: Self::default_buildkit(),
            cache_from: None,
        }
    }
}

// ============================================================================
// Deployment Type
// ============================================================================

/// Type of deployment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum DeploymentType {
    /// Deploy from Git repository (build from source).
    #[default]
    Source,
    /// Deploy from Docker registry (pre-built image).
    Image,
}

impl DeploymentType {
    pub fn display_name(&self) -> &str {
        match self {
            DeploymentType::Source => "From Git repository",
            DeploymentType::Image => "From Docker image",
        }
    }
}

impl fmt::Display for DeploymentType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

// ============================================================================
// Docker Image Configuration
// ============================================================================

/// Docker image configuration for image-based deployments.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageConfig {
    /// Image name (e.g., "nginx", "ghcr.io/user/app").
    pub name: String,
    /// Image tag (e.g., "latest", "v1.0.0").
    #[serde(default = "ImageConfig::default_tag")]
    pub tag: String,
    /// Image digest for reproducibility (e.g., "sha256:abc123...").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
    /// Docker registry.
    #[serde(default)]
    pub registry: Registry,
    /// Whether the registry requires authentication.
    #[serde(default)]
    pub private: bool,
}

impl ImageConfig {
    fn default_tag() -> String {
        "latest".to_string()
    }

    /// Returns the full image reference (registry/name:tag).
    pub fn full_reference(&self) -> String {
        let registry_prefix = match &self.registry {
            Registry::DockerHub => String::new(),
            Registry::Ghcr => "ghcr.io/".to_string(),
            Registry::Gcr => "gcr.io/".to_string(),
            Registry::Ecr { region } => format!("{}.dkr.ecr.amazonaws.com/", region),
            Registry::Custom { url } => format!("{}/", url.trim_end_matches('/')),
        };

        if let Some(digest) = &self.digest {
            format!("{}{}@{}", registry_prefix, self.name, digest)
        } else {
            format!("{}{}:{}", registry_prefix, self.name, self.tag)
        }
    }

    /// Returns the display name for the image.
    pub fn display_name(&self) -> String {
        format!("{}:{}", self.name, self.tag)
    }
}

/// Docker registry type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Registry {
    /// Docker Hub (docker.io).
    #[default]
    DockerHub,
    /// GitHub Container Registry (ghcr.io).
    Ghcr,
    /// Google Container Registry (gcr.io).
    Gcr,
    /// Amazon Elastic Container Registry.
    Ecr { region: String },
    /// Custom/private registry.
    Custom { url: String },
}

impl Registry {
    /// Returns the display name.
    pub fn display_name(&self) -> &str {
        match self {
            Registry::DockerHub => "Docker Hub",
            Registry::Ghcr => "GitHub Container Registry",
            Registry::Gcr => "Google Container Registry",
            Registry::Ecr { .. } => "Amazon ECR",
            Registry::Custom { .. } => "Custom Registry",
        }
    }

    /// Returns whether this registry typically requires authentication.
    pub fn requires_auth(&self) -> bool {
        match self {
            Registry::DockerHub => false, // Public images don't need auth
            Registry::Ghcr => false,      // Public images don't need auth
            Registry::Gcr => true,        // Usually needs auth
            Registry::Ecr { .. } => true, // Always needs auth
            Registry::Custom { .. } => true, // Assume private
        }
    }
}

impl fmt::Display for Registry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

// ============================================================================
// Volume Configuration
// ============================================================================

/// Volume mount configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolumeMount {
    /// Path inside the container.
    pub container_path: String,
    /// Named volume name (Docker will create it).
    pub volume_name: String,
    /// Whether the volume is read-only.
    #[serde(default)]
    pub read_only: bool,
}

impl VolumeMount {
    pub fn new(container_path: &str, volume_name: &str) -> Self {
        Self {
            container_path: container_path.to_string(),
            volume_name: volume_name.to_string(),
            read_only: false,
        }
    }

    /// Returns the Docker volume mount string.
    pub fn to_docker_arg(&self) -> String {
        if self.read_only {
            format!("{}:{}:ro", self.volume_name, self.container_path)
        } else {
            format!("{}:{}", self.volume_name, self.container_path)
        }
    }
}

// ============================================================================
// Registry Credentials
// ============================================================================

/// Registry authentication credentials.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryCredentials {
    pub username: String,
    #[serde(skip_serializing)]
    pub password: String,
    /// Base64-encoded auth string for Docker.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_token: Option<String>,
}

impl RegistryCredentials {
    pub fn new(username: &str, password: &str) -> Self {
        use base64::{engine::general_purpose::STANDARD, Engine};
        let auth_token = STANDARD.encode(format!("{}:{}", username, password));
        Self {
            username: username.to_string(),
            password: password.to_string(),
            auth_token: Some(auth_token),
        }
    }
}
