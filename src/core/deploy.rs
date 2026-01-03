//! Deployment orchestration for applications.

use std::path::Path;
use std::time::{Duration, Instant};

use chrono::Utc;

use crate::core::app_config::{AppConfig, CacheType, DatabaseType, HealthCheckConfig};
use crate::core::context::ExecutionContext;
use crate::core::error::AppError;
use crate::core::secrets::SecretsManager;
use crate::providers::container::{ContainerConfig, ContainerRuntime, RestartPolicy};
use crate::providers::git::GitProvider;
use crate::providers::reverse_proxy::ReverseProxy;
use crate::templates::dockerfile;
use crate::ui;

/// Deployment step for progress tracking.
#[derive(Debug, Clone, Copy)]
pub enum DeployStep {
    CloneRepository,
    BuildImage,
    StartDatabase,
    StartCache,
    StartApp,
    ConfigureRouting,
    HealthCheck,
}

impl DeployStep {
    pub fn display_name(&self) -> &str {
        match self {
            Self::CloneRepository => "Cloning repository",
            Self::BuildImage => "Building image",
            Self::StartDatabase => "Starting database",
            Self::StartCache => "Starting cache",
            Self::StartApp => "Starting app",
            Self::ConfigureRouting => "Configuring routing",
            Self::HealthCheck => "Health check",
        }
    }
}

/// Result of a deployment operation.
pub struct DeployResult {
    pub app_name: String,
    pub url: String,
    pub duration: Duration,
    pub is_first_deploy: bool,
}

/// Deployment orchestrator.
pub struct Deployer<'a> {
    config: &'a AppConfig,
    runtime: &'a dyn ContainerRuntime,
    proxy: &'a dyn ReverseProxy,
    ctx: &'a ExecutionContext,
}

impl<'a> Deployer<'a> {
    /// Creates a new deployer.
    pub fn new(
        config: &'a AppConfig,
        runtime: &'a dyn ContainerRuntime,
        proxy: &'a dyn ReverseProxy,
        ctx: &'a ExecutionContext,
    ) -> Self {
        Self {
            config,
            runtime,
            proxy,
            ctx,
        }
    }

    /// Container name prefix for this app.
    fn container_prefix(&self) -> String {
        format!("flaase-{}", self.config.name)
    }

    /// Network name for this app.
    fn network_name(&self) -> String {
        format!("flaase-{}-network", self.config.name)
    }

    /// Web container name.
    fn web_container_name(&self) -> String {
        format!("{}-web", self.container_prefix())
    }

    /// Database container name.
    fn db_container_name(&self) -> String {
        format!("{}-db", self.container_prefix())
    }

    /// Cache container name.
    fn cache_container_name(&self) -> String {
        format!("{}-cache", self.container_prefix())
    }

    /// Image name for this app.
    fn image_name(&self) -> String {
        format!("flaase-{}", self.config.name)
    }

    /// Executes a full deployment.
    pub fn deploy(&self) -> Result<DeployResult, AppError> {
        let start_time = Instant::now();
        let repo_path = self.config.repo_path();
        let is_first_deploy = !GitProvider::is_repo(&repo_path);

        // Run deployment with cleanup on failure
        match self.deploy_inner(&repo_path) {
            Ok(()) => {
                // Update deployed_at timestamp
                self.update_deployed_at()?;

                let duration = start_time.elapsed();
                let url = format!("https://{}", self.config.primary_domain());

                Ok(DeployResult {
                    app_name: self.config.name.clone(),
                    url,
                    duration,
                    is_first_deploy,
                })
            }
            Err(e) => {
                // Cleanup on failure
                self.cleanup_on_failure();
                Err(e)
            }
        }
    }

    /// Inner deployment logic.
    fn deploy_inner(&self, repo_path: &std::path::Path) -> Result<(), AppError> {
        // Step 1: Clone or pull repository
        let spinner = ui::ProgressBar::spinner(DeployStep::CloneRepository.display_name());
        self.sync_repository(repo_path)?;
        spinner.finish("done");

        // Step 2: Build Docker image
        let spinner = ui::ProgressBar::spinner(DeployStep::BuildImage.display_name());
        self.build_image(repo_path)?;
        spinner.finish("done");

        // Create network
        self.runtime.create_network(&self.network_name(), self.ctx)?;

        // Step 3: Start database (if configured)
        if self.config.database.is_some() {
            let spinner = ui::ProgressBar::spinner(DeployStep::StartDatabase.display_name());
            self.start_database()?;
            spinner.finish("done");
        }

        // Step 4: Start cache (if configured)
        if self.config.cache.is_some() {
            let spinner = ui::ProgressBar::spinner(DeployStep::StartCache.display_name());
            self.start_cache()?;
            spinner.finish("done");
        }

        // Step 5: Start app container
        let spinner = ui::ProgressBar::spinner(DeployStep::StartApp.display_name());
        self.start_app()?;
        spinner.finish("done");

        // Step 6: Configure Traefik routing
        let spinner = ui::ProgressBar::spinner(DeployStep::ConfigureRouting.display_name());
        self.configure_routing()?;
        spinner.finish("done");

        // Step 7: Health check
        let spinner = ui::ProgressBar::spinner(DeployStep::HealthCheck.display_name());
        self.health_check()?;
        spinner.finish("done");

        Ok(())
    }

    /// Cleanup containers on deployment failure.
    fn cleanup_on_failure(&self) {
        ui::warning("Cleaning up failed deployment...");

        // Stop and remove web container
        let web = self.web_container_name();
        if self.runtime.container_exists(&web, self.ctx).unwrap_or(false) {
            let _ = self.runtime.stop_container(&web, self.ctx);
            let _ = self.runtime.remove_container(&web, self.ctx);
        }

        // Note: We don't cleanup database/cache on failure as they might contain data
        // from previous deployments. User can manually clean with `fl destroy`.
    }

    /// Syncs the repository (clone or pull).
    fn sync_repository(&self, repo_path: &Path) -> Result<(), AppError> {
        if GitProvider::is_repo(repo_path) {
            // Pull latest changes
            let _has_changes = GitProvider::pull(repo_path, &self.config.ssh_key, self.ctx)?;
        } else {
            // Clone repository
            GitProvider::clone(
                &self.config.repository,
                repo_path,
                &self.config.ssh_key,
                self.ctx,
            )?;
        }

        Ok(())
    }

    /// Builds the Docker image.
    fn build_image(&self, repo_path: &Path) -> Result<(), AppError> {
        // Check if Dockerfile exists, otherwise generate one
        if !dockerfile::exists(repo_path) {
            let port = self.config.effective_port();
            let dockerfile_content = dockerfile::generate(self.config.stack, port);
            let dockerfile_path = dockerfile::path(repo_path);

            if self.ctx.is_dry_run() {
                ui::info(&format!("[DRY-RUN] Write Dockerfile to {:?}", dockerfile_path));
            } else {
                std::fs::write(&dockerfile_path, dockerfile_content)
                    .map_err(|e| AppError::Deploy(format!("Failed to write Dockerfile: {}", e)))?;
            }
        }

        // Build the image
        self.runtime.build_image(
            &self.image_name(),
            repo_path.to_str().unwrap(),
            self.ctx,
        )?;

        Ok(())
    }

    /// Starts the database container.
    fn start_database(&self) -> Result<(), AppError> {
        let db_config = self.config.database.as_ref().unwrap();
        let container_name = self.db_container_name();

        // Check if already running
        if self.runtime.container_is_running(&container_name, self.ctx)? {
            return Ok(());
        }

        // Remove existing container if exists
        if self.runtime.container_exists(&container_name, self.ctx)? {
            self.runtime.remove_container(&container_name, self.ctx)?;
        }

        // Load secrets
        let secrets = SecretsManager::load_secrets(&self.config.secrets_path())?;
        let db_secrets = secrets.database.as_ref().ok_or_else(|| {
            AppError::Deploy("Database secrets not found".into())
        })?;

        // Build container config based on database type
        let mut container = ContainerConfig::new(&container_name, db_config.db_type.docker_image())
            .network(&self.network_name())
            .restart(RestartPolicy::UnlessStopped)
            .label("flaase.managed", "true")
            .label("flaase.app", &self.config.name)
            .label("flaase.service", "database");

        // Add data volume
        let data_path = format!("{}/db", self.config.data_path().display());
        self.ctx.create_dir(&data_path)?;

        match db_config.db_type {
            DatabaseType::PostgreSQL => {
                container = container
                    .env("POSTGRES_USER", &db_secrets.username)
                    .env("POSTGRES_PASSWORD", &db_secrets.password)
                    .env("POSTGRES_DB", &db_config.name)
                    .volume(&data_path, "/var/lib/postgresql/data");
            }
            DatabaseType::MySQL => {
                container = container
                    .env("MYSQL_USER", &db_secrets.username)
                    .env("MYSQL_PASSWORD", &db_secrets.password)
                    .env("MYSQL_DATABASE", &db_config.name)
                    .env("MYSQL_ROOT_PASSWORD", &db_secrets.password)
                    .volume(&data_path, "/var/lib/mysql");
            }
            DatabaseType::MongoDB => {
                container = container
                    .env("MONGO_INITDB_ROOT_USERNAME", &db_secrets.username)
                    .env("MONGO_INITDB_ROOT_PASSWORD", &db_secrets.password)
                    .volume(&data_path, "/data/db");
            }
        }

        self.runtime.run_container(&container, self.ctx)?;

        // Wait for database to be ready
        std::thread::sleep(Duration::from_secs(5));

        Ok(())
    }

    /// Starts the cache container.
    fn start_cache(&self) -> Result<(), AppError> {
        let cache_config = self.config.cache.as_ref().unwrap();
        let container_name = self.cache_container_name();

        // Check if already running
        if self.runtime.container_is_running(&container_name, self.ctx)? {
            return Ok(());
        }

        // Remove existing container if exists
        if self.runtime.container_exists(&container_name, self.ctx)? {
            self.runtime.remove_container(&container_name, self.ctx)?;
        }

        // Load secrets for password
        let secrets = SecretsManager::load_secrets(&self.config.secrets_path())?;

        let mut container = ContainerConfig::new(&container_name, cache_config.cache_type.docker_image())
            .network(&self.network_name())
            .restart(RestartPolicy::UnlessStopped)
            .label("flaase.managed", "true")
            .label("flaase.app", &self.config.name)
            .label("flaase.service", "cache");

        match cache_config.cache_type {
            CacheType::Redis => {
                if let Some(cache_secrets) = &secrets.cache {
                    container = container.command(vec![
                        "redis-server".to_string(),
                        "--requirepass".to_string(),
                        cache_secrets.password.clone(),
                    ]);
                }
            }
        }

        self.runtime.run_container(&container, self.ctx)?;

        // Wait for cache to be ready
        std::thread::sleep(Duration::from_secs(2));

        Ok(())
    }

    /// Starts the app container.
    fn start_app(&self) -> Result<(), AppError> {
        let container_name = self.web_container_name();
        let port = self.config.effective_port();

        // Check if already running - stop it first
        if self.runtime.container_exists(&container_name, self.ctx)? {
            self.runtime.stop_container(&container_name, self.ctx).ok();
            self.runtime.remove_container(&container_name, self.ctx)?;
        }

        // Find available host port
        let host_port = self.runtime.find_available_port(port, self.ctx)?;

        let mut container = ContainerConfig::new(&container_name, &self.image_name())
            .port(host_port, port)
            .network(&self.network_name())
            .restart(RestartPolicy::UnlessStopped)
            .label("flaase.managed", "true")
            .label("flaase.app", &self.config.name)
            .label("flaase.service", "web");

        // Add environment files
        let env_path = self.config.env_path();
        let auto_env_path = self.config.auto_env_path();

        if auto_env_path.exists() {
            container = container.env_file(auto_env_path.to_str().unwrap());
        }
        if env_path.exists() {
            container = container.env_file(env_path.to_str().unwrap());
        }

        // Set NODE_ENV for JS stacks
        container = container.env("NODE_ENV", "production");

        self.runtime.run_container(&container, self.ctx)?;

        // Connect to Traefik network for routing
        self.runtime.connect_network(&container_name, "flaase-network", self.ctx)?;

        Ok(())
    }

    /// Configures Traefik routing for all domains.
    fn configure_routing(&self) -> Result<(), AppError> {
        use crate::core::secrets::SecretsManager;
        use crate::templates::traefik::{generate_app_config, AppDomain};

        let port = self.config.effective_port();

        // Load secrets for auth info
        let secrets = SecretsManager::load_secrets(&self.config.secrets_path()).ok();

        // Build domain list with auth info
        let mut domains = Vec::new();
        for domain_config in &self.config.domains {
            let mut app_domain = AppDomain::new(&domain_config.domain, domain_config.primary);

            // Add auth if configured
            if let Some(ref secrets) = secrets {
                if let Some(auth_secret) = secrets.auth.get(&domain_config.domain) {
                    app_domain = app_domain.with_auth(&auth_secret.password_hash);
                }
            }

            domains.push(app_domain);
        }

        // Generate and write Traefik config
        let traefik_config = generate_app_config(&self.config.name, &domains, port);
        let traefik_path = format!(
            "{}/{}.yml",
            crate::core::FLAASE_TRAEFIK_DYNAMIC_PATH,
            self.config.name
        );

        self.ctx.write_file(&traefik_path, &traefik_config)
    }

    /// Performs health check on the app.
    fn health_check(&self) -> Result<(), AppError> {
        if self.ctx.is_dry_run() {
            return Ok(());
        }

        let health_config = self.config.effective_health_check();
        let container_name = self.web_container_name();

        for attempt in 1..=health_config.retries {
            // Check if container is running
            if !self.runtime.container_is_running(&container_name, self.ctx)? {
                return Err(AppError::Deploy("Container stopped unexpectedly".into()));
            }

            // Try HTTP health check
            if self.check_http_health(&health_config) {
                return Ok(());
            }

            if attempt < health_config.retries {
                std::thread::sleep(Duration::from_secs(health_config.interval as u64));
            }
        }

        // Get container logs for debugging
        let logs = self.runtime.get_logs(&container_name, 50, self.ctx)?;

        Err(AppError::Deploy(format!(
            "Health check failed after {} attempts.\n\nRecent logs:\n{}",
            health_config.retries, logs
        )))
    }

    /// Checks HTTP health of the app.
    fn check_http_health(&self, config: &HealthCheckConfig) -> bool {
        let container_name = self.web_container_name();
        let port = self.config.effective_port();
        let endpoint = &config.endpoint;

        // First check if container is running
        if !self.runtime.container_is_running(&container_name, self.ctx).unwrap_or(false) {
            return false;
        }

        // Try health check via Traefik container (which is on the same network)
        let url = format!("http://{}:{}{}", container_name, port, endpoint);
        let timeout = config.timeout.to_string();

        let result = self.ctx.run_command(
            "docker",
            &[
                "exec", "flaase-traefik",
                "wget", "-q", "--spider",
                "--timeout", &timeout,
                &url,
            ],
        );

        if result.is_ok() && result.as_ref().unwrap().success {
            return true;
        }

        // Fallback: check inside the app container itself
        let wget_result = self.runtime.exec_in_container(
            &container_name,
            &["wget", "-q", "--spider", &format!("http://localhost:{}{}", port, endpoint)],
            self.ctx,
        );

        if wget_result.is_ok() {
            return true;
        }

        // Last resort: just check if container is still running after startup
        std::thread::sleep(Duration::from_secs(2));
        self.runtime.container_is_running(&container_name, self.ctx).unwrap_or(false)
    }

    /// Updates the deployed_at timestamp in the config.
    fn update_deployed_at(&self) -> Result<(), AppError> {
        if self.ctx.is_dry_run() {
            return Ok(());
        }

        let mut config = self.config.clone();
        config.deployed_at = Some(Utc::now());
        config.save()
    }

    /// Stops the web container (database and cache stay running).
    pub fn stop(&self) -> Result<(), AppError> {
        let container = self.web_container_name();

        if self.runtime.container_is_running(&container, self.ctx)? {
            self.runtime.stop_container(&container, self.ctx)?;
        }

        // Update Traefik to show 503 maintenance page
        self.proxy.write_maintenance_config(&self.config.name, self.ctx)?;

        Ok(())
    }

    /// Starts the web container and runs health check.
    pub fn start(&self) -> Result<(), AppError> {
        // Ensure database is running if configured
        if self.config.database.is_some() {
            let db_container = self.db_container_name();
            if !self.runtime.container_is_running(&db_container, self.ctx)? {
                self.start_database()?;
            }
        }

        // Ensure cache is running if configured
        if self.config.cache.is_some() {
            let cache_container = self.cache_container_name();
            if !self.runtime.container_is_running(&cache_container, self.ctx)? {
                self.start_cache()?;
            }
        }

        // Start app container
        self.start_app()?;

        // Restore normal Traefik routing (remove maintenance page)
        self.configure_routing()?;

        // Run health check
        self.health_check()?;

        Ok(())
    }

    /// Destroys all resources for this app.
    pub fn destroy(&self) -> Result<(), AppError> {
        // Stop and remove containers
        let containers = [
            self.web_container_name(),
            self.db_container_name(),
            self.cache_container_name(),
        ];

        for container in &containers {
            if self.runtime.container_exists(container, self.ctx)? {
                self.runtime.stop_container(container, self.ctx).ok();
                self.runtime.remove_container(container, self.ctx)?;
            }
        }

        // Remove network
        let network = self.network_name();
        if self.runtime.network_exists(&network, self.ctx)? {
            self.ctx.run_command("docker", &["network", "rm", &network])?;
        }

        // Remove Traefik config
        self.proxy.remove_app_config(&self.config.name, self.ctx)?;

        // Remove Docker image
        self.ctx.run_command("docker", &["rmi", "-f", &self.image_name()]).ok();

        Ok(())
    }
}

/// Formats a duration for display.
pub fn format_duration(duration: Duration) -> String {
    let secs = duration.as_secs();

    if secs < 60 {
        format!("{}s", secs)
    } else {
        let mins = secs / 60;
        let remaining_secs = secs % 60;
        format!("{}m {}s", mins, remaining_secs)
    }
}
