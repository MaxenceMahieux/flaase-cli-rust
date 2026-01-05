//! Deployment orchestration for applications.

use std::path::Path;
use std::time::{Duration, Instant};

use chrono::Utc;

use crate::core::app_config::{AppConfig, CacheType, DatabaseType, HealthCheckConfig, Stack};
use crate::core::context::ExecutionContext;
use crate::core::error::AppError;
use crate::core::registry::pull_image;
use crate::core::secrets::SecretsManager;
use crate::core::stack_detection::validate_nextjs_standalone_config;
use crate::providers::container::{ContainerConfig, ContainerRuntime, RestartPolicy};
use crate::providers::git::GitProvider;
use crate::providers::reverse_proxy::ReverseProxy;
use crate::templates::dockerfile;
use crate::ui;

/// Hook execution phase.
#[derive(Debug, Clone, Copy)]
pub enum HookPhase {
    PreBuild,
    PreDeploy,
    PostDeploy,
    OnFailure,
}

/// Deployment step for progress tracking.
#[derive(Debug, Clone, Copy)]
pub enum DeployStep {
    CloneRepository,
    PullImage,
    PreBuildHooks,
    BuildImage,
    RunTests,
    PreDeployHooks,
    StartDatabase,
    StartCache,
    StartApp,
    ConfigureRouting,
    HealthCheck,
    PostDeployHooks,
}

impl DeployStep {
    pub fn display_name(&self) -> &str {
        match self {
            Self::CloneRepository => "Cloning repository",
            Self::PullImage => "Pulling image",
            Self::PreBuildHooks => "Running pre-build hooks",
            Self::BuildImage => "Building image",
            Self::RunTests => "Running tests",
            Self::PreDeployHooks => "Running pre-deploy hooks",
            Self::StartDatabase => "Starting database",
            Self::StartCache => "Starting cache",
            Self::StartApp => "Starting app",
            Self::ConfigureRouting => "Configuring routing",
            Self::HealthCheck => "Health check",
            Self::PostDeployHooks => "Running post-deploy hooks",
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

/// Result of an update operation.
pub struct UpdateResult {
    pub app_name: String,
    pub url: String,
    pub duration: Duration,
    pub old_commit: Option<String>,
    pub new_commit: String,
    pub had_changes: bool,
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

    /// Web container name (standard deployment).
    fn web_container_name(&self) -> String {
        format!("{}-web", self.container_prefix())
    }

    /// Blue container name (blue-green deployment).
    fn blue_container_name(&self) -> String {
        format!("{}-web-blue", self.container_prefix())
    }

    /// Green container name (blue-green deployment).
    fn green_container_name(&self) -> String {
        format!("{}-web-green", self.container_prefix())
    }

    /// Checks if blue-green deployment is enabled.
    fn is_blue_green_enabled(&self) -> bool {
        self.config.autodeploy_config
            .as_ref()
            .and_then(|ad| ad.blue_green.as_ref())
            .map(|bg| bg.enabled)
            .unwrap_or(false)
    }

    /// Gets the blue-green configuration.
    fn blue_green_config(&self) -> Option<&crate::core::app_config::BlueGreenConfig> {
        self.config.autodeploy_config
            .as_ref()
            .and_then(|ad| ad.blue_green.as_ref())
    }

    /// Determines which slot is currently active (receiving traffic).
    /// Returns "blue", "green", or "none".
    fn active_slot(&self) -> Result<&'static str, AppError> {
        let blue = self.blue_container_name();
        let green = self.green_container_name();

        let blue_running = self.runtime.container_is_running(&blue, self.ctx).unwrap_or(false);
        let green_running = self.runtime.container_is_running(&green, self.ctx).unwrap_or(false);

        // Check Traefik config to see which one is receiving traffic
        // For simplicity, we'll assume the one that's running is active
        // In a more complex setup, we'd check the Traefik config
        if blue_running && !green_running {
            Ok("blue")
        } else if green_running && !blue_running {
            Ok("green")
        } else if blue_running && green_running {
            // Both running - check which one was started last
            // For now, default to blue as active
            Ok("blue")
        } else {
            Ok("none")
        }
    }

    /// Gets the container name for the active slot.
    fn active_container_name(&self) -> Result<Option<String>, AppError> {
        match self.active_slot()? {
            "blue" => Ok(Some(self.blue_container_name())),
            "green" => Ok(Some(self.green_container_name())),
            _ => Ok(None),
        }
    }

    /// Gets the container name for the inactive slot (for new deployment).
    fn inactive_slot_container_name(&self) -> Result<String, AppError> {
        match self.active_slot()? {
            "blue" => Ok(self.green_container_name()),
            "green" => Ok(self.blue_container_name()),
            _ => Ok(self.blue_container_name()), // Default to blue for first deployment
        }
    }

    /// Database container name.
    fn db_container_name(&self) -> String {
        format!("{}-db", self.container_prefix())
    }

    /// Cache container name.
    fn cache_container_name(&self) -> String {
        format!("{}-cache", self.container_prefix())
    }

    /// Base image name for this app.
    fn image_name(&self) -> String {
        format!("flaase-{}", self.config.name)
    }

    /// Current (latest) image tag.
    fn current_image_tag(&self) -> String {
        format!("{}:latest", self.image_name())
    }

    /// Previous image tag (for rollback).
    fn previous_image_tag(&self) -> String {
        format!("{}:previous", self.image_name())
    }

    /// Versioned image tag using commit SHA.
    fn versioned_image_tag(&self, commit_sha: &str) -> String {
        let short_sha = if commit_sha.len() >= 7 {
            &commit_sha[..7]
        } else {
            commit_sha
        };
        format!("{}:{}", self.image_name(), short_sha)
    }

    /// Checks if an image exists.
    fn image_exists(&self, tag: &str) -> Result<bool, AppError> {
        let result = self.ctx.run_command("docker", &["image", "inspect", tag]);
        Ok(result.is_ok() && result.unwrap().success)
    }

    /// Tags an image.
    fn tag_image(&self, source: &str, target: &str) -> Result<(), AppError> {
        if self.ctx.is_dry_run() {
            ui::info(&format!("[DRY-RUN] docker tag {} {}", source, target));
            return Ok(());
        }
        self.ctx.run_command("docker", &["tag", source, target])?
            .ensure_success("Failed to tag image")?;
        Ok(())
    }

    /// Gets the current commit SHA from the repo.
    fn get_commit_sha(&self, repo_path: &Path) -> Result<String, AppError> {
        GitProvider::get_commit_hash(repo_path)
    }

    /// Executes a full deployment.
    pub fn deploy(&self) -> Result<DeployResult, AppError> {
        let start_time = Instant::now();

        // Branch based on deployment type
        let deploy_result = if self.config.is_image_deployment() {
            self.deploy_image_inner()
        } else {
            let repo_path = self.config.repo_path();
            self.deploy_source_inner(&repo_path)
        };

        let is_first_deploy = self.config.deployed_at.is_none();

        match deploy_result {
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
                // Run failure hooks if configured (only for source deployments)
                if self.config.is_source_deployment() {
                    let repo_path = self.config.repo_path();
                    if self.has_hooks(HookPhase::OnFailure) {
                        ui::warning("Running failure hooks...");
                        let _ = self.run_hooks(HookPhase::OnFailure, &repo_path);
                    }
                }

                // Attempt auto-rollback if enabled and previous version exists
                if self.should_auto_rollback() && self.can_rollback() {
                    ui::warning("Deployment failed, attempting auto-rollback...");
                    match self.rollback(None) {
                        Ok(_) => {
                            ui::success("Auto-rollback successful");
                            return Err(AppError::Deploy(format!(
                                "Deployment failed but auto-rollback succeeded. Original error: {}",
                                e
                            )));
                        }
                        Err(rb_err) => {
                            ui::error(&format!("Auto-rollback failed: {}", rb_err));
                        }
                    }
                }

                // Cleanup on failure
                self.cleanup_on_failure();
                Err(e)
            }
        }
    }

    /// Executes an update (zero-downtime deployment with before/after info).
    pub fn update(&self) -> Result<UpdateResult, AppError> {
        let start_time = Instant::now();
        let repo_path = self.config.repo_path();

        // Check if app was previously deployed
        if !GitProvider::is_repo(&repo_path) {
            return Err(AppError::Deploy(
                "App not deployed yet. Use 'fl deploy' for initial deployment.".into()
            ));
        }

        // Get current commit SHA before pulling
        let old_commit = self.get_commit_sha(&repo_path).ok();

        // Run update with rollback on failure
        match self.update_inner(&repo_path) {
            Ok((new_commit, had_changes)) => {
                // Update deployed_at timestamp
                self.update_deployed_at()?;

                let duration = start_time.elapsed();
                let url = format!("https://{}", self.config.primary_domain());

                Ok(UpdateResult {
                    app_name: self.config.name.clone(),
                    url,
                    duration,
                    old_commit,
                    new_commit,
                    had_changes,
                })
            }
            Err(e) => {
                // Run failure hooks if configured
                if self.has_hooks(HookPhase::OnFailure) {
                    ui::warning("Running failure hooks...");
                    let _ = self.run_hooks(HookPhase::OnFailure, &repo_path);
                }

                // Attempt auto-rollback if enabled and previous version exists
                if self.should_auto_rollback() && self.can_rollback() {
                    ui::warning("Update failed, attempting auto-rollback...");
                    match self.rollback(None) {
                        Ok(_) => {
                            ui::success("Auto-rollback successful");
                            return Err(AppError::Deploy(format!(
                                "Update failed but auto-rollback succeeded. Original error: {}",
                                e
                            )));
                        }
                        Err(rb_err) => {
                            ui::error(&format!("Auto-rollback failed: {}", rb_err));
                        }
                    }
                }

                // Cleanup on failure
                self.cleanup_on_failure();
                Err(e)
            }
        }
    }

    /// Inner update logic - returns (new_commit_sha, had_changes).
    fn update_inner(&self, repo_path: &std::path::Path) -> Result<(String, bool), AppError> {
        // Step 1: Pull latest changes
        let spinner = ui::ProgressBar::spinner("Pulling latest changes");
        let ssh_key = self.config.ssh_key.as_ref().ok_or_else(|| {
            AppError::Config("SSH key required for source deployments".into())
        })?;
        let had_changes = GitProvider::pull(repo_path, ssh_key, self.ctx)?;
        spinner.finish(if had_changes { "updated" } else { "no changes" });

        // Get new commit SHA
        let new_commit = self.get_commit_sha(repo_path)?;

        // If no changes and app is running, we're done
        if !had_changes {
            let container = self.web_container_name();
            if self.runtime.container_is_running(&container, self.ctx).unwrap_or(false) {
                return Ok((new_commit, false));
            }
            // App not running, continue with deployment
        }

        // Validate Next.js standalone configuration if applicable
        self.validate_stack_requirements(repo_path)?;

        // Step 2: Run pre-build hooks
        if self.has_hooks(HookPhase::PreBuild) {
            let spinner = ui::ProgressBar::spinner(DeployStep::PreBuildHooks.display_name());
            self.run_hooks(HookPhase::PreBuild, repo_path)?;
            spinner.finish("done");
        }

        // Step 3: Build Docker image
        let spinner = ui::ProgressBar::spinner(DeployStep::BuildImage.display_name());
        let _commit_sha = self.build_image(repo_path)?;
        spinner.finish("done");

        // Step 4: Run tests
        if self.has_tests_enabled() {
            let spinner = ui::ProgressBar::spinner(DeployStep::RunTests.display_name());
            self.run_tests(repo_path)?;
            spinner.finish("done");
        }

        // Step 5: Run pre-deploy hooks
        if self.has_hooks(HookPhase::PreDeploy) {
            let spinner = ui::ProgressBar::spinner(DeployStep::PreDeployHooks.display_name());
            self.run_hooks(HookPhase::PreDeploy, repo_path)?;
            spinner.finish("done");
        }

        // Ensure network exists
        self.runtime.create_network(&self.network_name(), self.ctx)?;

        // Step 6: Start database (if configured and not running)
        if self.config.database.is_some() {
            let db_container = self.db_container_name();
            if !self.runtime.container_is_running(&db_container, self.ctx).unwrap_or(false) {
                let spinner = ui::ProgressBar::spinner(DeployStep::StartDatabase.display_name());
                self.start_database()?;
                spinner.finish("done");
            }
        }

        // Step 7: Start cache (if configured and not running)
        if self.config.cache.is_some() {
            let cache_container = self.cache_container_name();
            if !self.runtime.container_is_running(&cache_container, self.ctx).unwrap_or(false) {
                let spinner = ui::ProgressBar::spinner(DeployStep::StartCache.display_name());
                self.start_cache()?;
                spinner.finish("done");
            }
        }

        // Step 8: Start app container (with blue-green if enabled)
        // This handles:
        // - Starting new container
        // - Health check on new container
        // - Switching traffic only if health check passes
        // - Stopping old container
        let spinner = ui::ProgressBar::spinner(DeployStep::StartApp.display_name());
        self.start_app()?;
        spinner.finish("done");

        // Step 9: Configure Traefik routing (if not blue-green, which handles this)
        if !self.is_blue_green_enabled() {
            let spinner = ui::ProgressBar::spinner(DeployStep::ConfigureRouting.display_name());
            self.configure_routing()?;
            spinner.finish("done");

            // Step 10: Health check (if not blue-green, which already did this)
            let spinner = ui::ProgressBar::spinner(DeployStep::HealthCheck.display_name());
            self.health_check()?;
            spinner.finish("done");
        }

        // Step 11: Run post-deploy hooks
        if self.has_hooks(HookPhase::PostDeploy) {
            let spinner = ui::ProgressBar::spinner(DeployStep::PostDeployHooks.display_name());
            self.run_hooks(HookPhase::PostDeploy, repo_path)?;
            spinner.finish("done");
        }

        Ok((new_commit, had_changes))
    }

    /// Inner deployment logic.
    /// Inner deployment logic for source-based deployments (from Git).
    fn deploy_source_inner(&self, repo_path: &std::path::Path) -> Result<(), AppError> {
        // Step 1: Clone or pull repository
        let spinner = ui::ProgressBar::spinner(DeployStep::CloneRepository.display_name());
        self.sync_repository(repo_path)?;
        spinner.finish("done");

        // Validate Next.js standalone configuration if applicable
        self.validate_stack_requirements(repo_path)?;

        // Step 2: Run pre-build hooks
        if self.has_hooks(HookPhase::PreBuild) {
            let spinner = ui::ProgressBar::spinner(DeployStep::PreBuildHooks.display_name());
            self.run_hooks(HookPhase::PreBuild, repo_path)?;
            spinner.finish("done");
        }

        // Step 3: Build Docker image
        let spinner = ui::ProgressBar::spinner(DeployStep::BuildImage.display_name());
        let _commit_sha = self.build_image(repo_path)?;
        spinner.finish("done");

        // Step 4: Run tests
        if self.has_tests_enabled() {
            let spinner = ui::ProgressBar::spinner(DeployStep::RunTests.display_name());
            self.run_tests(repo_path)?;
            spinner.finish("done");
        }

        // Step 5: Run pre-deploy hooks
        if self.has_hooks(HookPhase::PreDeploy) {
            let spinner = ui::ProgressBar::spinner(DeployStep::PreDeployHooks.display_name());
            self.run_hooks(HookPhase::PreDeploy, repo_path)?;
            spinner.finish("done");
        }

        // Create network
        self.runtime.create_network(&self.network_name(), self.ctx)?;

        // Step 6: Start database (if configured)
        if self.config.database.is_some() {
            let spinner = ui::ProgressBar::spinner(DeployStep::StartDatabase.display_name());
            self.start_database()?;
            spinner.finish("done");
        }

        // Step 7: Start cache (if configured)
        if self.config.cache.is_some() {
            let spinner = ui::ProgressBar::spinner(DeployStep::StartCache.display_name());
            self.start_cache()?;
            spinner.finish("done");
        }

        // Step 8: Start app container
        let spinner = ui::ProgressBar::spinner(DeployStep::StartApp.display_name());
        self.start_app()?;
        spinner.finish("done");

        // Step 9: Configure Traefik routing
        let spinner = ui::ProgressBar::spinner(DeployStep::ConfigureRouting.display_name());
        self.configure_routing()?;
        spinner.finish("done");

        // Step 10: Health check
        let spinner = ui::ProgressBar::spinner(DeployStep::HealthCheck.display_name());
        self.health_check()?;
        spinner.finish("done");

        // Step 11: Run post-deploy hooks
        if self.has_hooks(HookPhase::PostDeploy) {
            let spinner = ui::ProgressBar::spinner(DeployStep::PostDeployHooks.display_name());
            self.run_hooks(HookPhase::PostDeploy, repo_path)?;
            spinner.finish("done");
        }

        Ok(())
    }

    /// Inner deployment logic for image-based deployments (from registry).
    fn deploy_image_inner(&self) -> Result<(), AppError> {
        let image_config = self.config.image.as_ref().ok_or_else(|| {
            AppError::Config("Image configuration required for image deployments".into())
        })?;

        // Step 1: Pull Docker image from registry
        let spinner = ui::ProgressBar::spinner(DeployStep::PullImage.display_name());
        // Load credentials for private registries
        let credentials = if image_config.private {
            crate::core::registry::load_credentials(&self.config.registry_auth_path())?
        } else {
            None
        };
        pull_image(image_config, credentials.as_ref(), self.ctx)?;
        spinner.finish("done");

        // Create network
        self.runtime.create_network(&self.network_name(), self.ctx)?;

        // Step 2: Start database (if configured)
        if self.config.database.is_some() {
            let spinner = ui::ProgressBar::spinner(DeployStep::StartDatabase.display_name());
            self.start_database()?;
            spinner.finish("done");
        }

        // Step 3: Start cache (if configured)
        if self.config.cache.is_some() {
            let spinner = ui::ProgressBar::spinner(DeployStep::StartCache.display_name());
            self.start_cache()?;
            spinner.finish("done");
        }

        // Step 4: Start app container
        let spinner = ui::ProgressBar::spinner(DeployStep::StartApp.display_name());
        self.start_app()?;
        spinner.finish("done");

        // Step 5: Configure Traefik routing
        let spinner = ui::ProgressBar::spinner(DeployStep::ConfigureRouting.display_name());
        self.configure_routing()?;
        spinner.finish("done");

        // Step 6: Health check
        let spinner = ui::ProgressBar::spinner(DeployStep::HealthCheck.display_name());
        self.health_check()?;
        spinner.finish("done");

        Ok(())
    }

    /// Returns the Docker image to use for the app container.
    /// For image deployments: the pulled image reference
    /// For source deployments: the locally built image
    fn app_image(&self) -> String {
        if let Some(image_config) = &self.config.image {
            image_config.full_reference()
        } else {
            self.current_image_tag()
        }
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
        let repository = self.config.repository.as_ref().ok_or_else(|| {
            AppError::Config("Repository required for source deployments".into())
        })?;
        let ssh_key = self.config.ssh_key.as_ref().ok_or_else(|| {
            AppError::Config("SSH key required for source deployments".into())
        })?;

        if GitProvider::is_repo(repo_path) {
            // Pull latest changes
            let _has_changes = GitProvider::pull(repo_path, ssh_key, self.ctx)?;
        } else {
            // Clone repository
            GitProvider::clone(repository, repo_path, ssh_key, self.ctx)?;
        }

        Ok(())
    }

    /// Validates stack-specific requirements before building.
    /// Currently checks Next.js standalone output configuration.
    fn validate_stack_requirements(&self, repo_path: &Path) -> Result<(), AppError> {
        // Only validate for Next.js stack
        if let Some(Stack::NextJs) = self.config.stack {
            // Skip validation if user provides their own Dockerfile
            if dockerfile::exists(repo_path) {
                return Ok(());
            }

            // Validate Next.js standalone configuration
            if let Err(msg) = validate_nextjs_standalone_config(repo_path) {
                return Err(AppError::Validation(msg));
            }
        }

        Ok(())
    }

    // ========================================================================
    // Test Execution
    // ========================================================================

    /// Checks if tests are enabled for this app.
    fn has_tests_enabled(&self) -> bool {
        self.config.autodeploy_config
            .as_ref()
            .and_then(|ad| ad.tests.as_ref())
            .map(|t| t.enabled)
            .unwrap_or(false)
    }

    /// Runs tests for the app.
    fn run_tests(&self, repo_path: &Path) -> Result<(), AppError> {
        let test_config = self.config.autodeploy_config
            .as_ref()
            .and_then(|ad| ad.tests.as_ref())
            .ok_or_else(|| AppError::TestsFailed("Test config not found".into()))?;

        if !test_config.enabled {
            return Ok(());
        }

        if self.ctx.is_dry_run() {
            ui::info(&format!("[DRY-RUN] Run tests: {}", test_config.command));
            return Ok(());
        }

        ui::info(&format!("Running: {}", test_config.command));

        let output = std::process::Command::new("sh")
            .current_dir(repo_path)
            .args(["-c", &test_config.command])
            .output()
            .map_err(|e| AppError::TestsFailed(format!("Failed to execute tests: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            let error_output = if !stderr.is_empty() {
                stderr.to_string()
            } else {
                stdout.to_string()
            };

            if test_config.fail_deployment_on_error {
                return Err(AppError::TestsFailed(format!(
                    "Tests failed:\n{}",
                    error_output.lines().take(20).collect::<Vec<_>>().join("\n")
                )));
            } else {
                ui::warning(&format!("Tests failed (non-blocking): {}", error_output.lines().next().unwrap_or("")));
            }
        }

        Ok(())
    }

    // ========================================================================
    // Hooks System
    // ========================================================================

    /// Checks if hooks are configured for a phase.
    fn has_hooks(&self, phase: HookPhase) -> bool {
        self.config.autodeploy_config
            .as_ref()
            .and_then(|ad| ad.hooks.as_ref())
            .map(|h| match phase {
                HookPhase::PreBuild => !h.pre_build.is_empty(),
                HookPhase::PreDeploy => !h.pre_deploy.is_empty(),
                HookPhase::PostDeploy => !h.post_deploy.is_empty(),
                HookPhase::OnFailure => !h.on_failure.is_empty(),
            })
            .unwrap_or(false)
    }

    /// Runs hooks for a phase.
    fn run_hooks(&self, phase: HookPhase, repo_path: &Path) -> Result<(), AppError> {
        let hooks_config = self.config.autodeploy_config
            .as_ref()
            .and_then(|ad| ad.hooks.as_ref());

        let hooks = match hooks_config {
            Some(h) => match phase {
                HookPhase::PreBuild => &h.pre_build,
                HookPhase::PreDeploy => &h.pre_deploy,
                HookPhase::PostDeploy => &h.post_deploy,
                HookPhase::OnFailure => &h.on_failure,
            },
            None => return Ok(()),
        };

        for hook in hooks {
            ui::info(&format!("  Hook: {}", hook.name));

            let result = if hook.run_in_container {
                self.run_hook_in_container(hook)
            } else {
                self.run_hook_on_host(hook, repo_path)
            };

            match result {
                Ok(_) => {
                    ui::success(&format!("    {} completed", hook.name));
                }
                Err(e) if hook.required => {
                    return Err(AppError::HookFailed(format!("{}: {}", hook.name, e)));
                }
                Err(e) => {
                    ui::warning(&format!("    {} failed (non-blocking): {}", hook.name, e));
                }
            }
        }

        Ok(())
    }

    /// Runs a hook on the host (in the repo directory).
    fn run_hook_on_host(&self, hook: &crate::core::app_config::HookCommand, repo_path: &Path) -> Result<(), AppError> {
        if self.ctx.is_dry_run() {
            ui::info(&format!("[DRY-RUN] Run hook: {}", hook.command));
            return Ok(());
        }

        let output = std::process::Command::new("sh")
            .current_dir(repo_path)
            .args(["-c", &hook.command])
            .output()
            .map_err(|e| AppError::HookFailed(format!("Failed to execute: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AppError::HookFailed(stderr.to_string()));
        }

        Ok(())
    }

    /// Runs a hook inside the app container.
    fn run_hook_in_container(&self, hook: &crate::core::app_config::HookCommand) -> Result<(), AppError> {
        let container_name = self.web_container_name();

        if !self.runtime.container_is_running(&container_name, self.ctx)? {
            return Err(AppError::HookFailed("Container not running".into()));
        }

        self.runtime.exec_in_container(
            &container_name,
            &["sh", "-c", &hook.command],
            self.ctx,
        )?;

        Ok(())
    }

    /// Builds the Docker image with caching and versioning.
    fn build_image(&self, repo_path: &Path) -> Result<String, AppError> {
        // Get commit SHA for versioning
        let commit_sha = self.get_commit_sha(repo_path)?;
        let versioned_tag = self.versioned_image_tag(&commit_sha);
        let latest_tag = self.current_image_tag();
        let previous_tag = self.previous_image_tag();

        // Check if Dockerfile exists, otherwise generate one (only for source deployments)
        if !dockerfile::exists(repo_path) {
            let stack = self.config.stack.as_ref().ok_or_else(|| {
                AppError::Config("Stack required for source deployments".into())
            })?;
            let port = self.config.effective_port();
            let dockerfile_content = dockerfile::generate(*stack, port);
            let dockerfile_path = dockerfile::path(repo_path);

            if self.ctx.is_dry_run() {
                ui::info(&format!("[DRY-RUN] Write Dockerfile to {:?}", dockerfile_path));
            } else {
                std::fs::write(&dockerfile_path, dockerfile_content)
                    .map_err(|e| AppError::Deploy(format!("Failed to write Dockerfile: {}", e)))?;
            }
        }

        // Backup current image as previous (for rollback)
        if self.image_exists(&latest_tag)? {
            self.tag_image(&latest_tag, &previous_tag)?;
        }

        // Get build config
        let build_config = self.config.autodeploy_config
            .as_ref()
            .and_then(|ad| ad.build.as_ref());

        // Build with caching if enabled
        let use_buildkit = build_config
            .map(|bc| bc.buildkit)
            .unwrap_or(true);
        let use_cache = build_config
            .map(|bc| bc.cache_enabled)
            .unwrap_or(true);

        if self.ctx.is_dry_run() {
            ui::info(&format!("[DRY-RUN] Build image {} with BUILDKIT={}", versioned_tag, use_buildkit));
        } else {
            // Set BuildKit environment variable if enabled
            if use_buildkit {
                std::env::set_var("DOCKER_BUILDKIT", "1");
            }

            // Build command with cache-from if enabled
            let mut args = vec!["build", "-t", &versioned_tag];

            if use_cache && self.image_exists(&latest_tag)? {
                args.push("--cache-from");
                args.push(&latest_tag);
            }

            args.push(repo_path.to_str().unwrap());

            self.ctx.run_command_streaming("docker", &args)?
                .ensure_success("Failed to build Docker image")?;

            // Tag as latest
            self.tag_image(&versioned_tag, &latest_tag)?;
        }

        Ok(commit_sha)
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
    /// Uses blue-green deployment if enabled, otherwise standard deployment.
    fn start_app(&self) -> Result<(), AppError> {
        if self.is_blue_green_enabled() {
            self.start_app_blue_green()
        } else {
            self.start_app_standard()
        }
    }

    /// Standard deployment (stop old, start new).
    fn start_app_standard(&self) -> Result<(), AppError> {
        let container_name = self.web_container_name();
        let port = self.config.effective_port();

        // Check if already running - stop it first
        if self.runtime.container_exists(&container_name, self.ctx)? {
            self.runtime.stop_container(&container_name, self.ctx).ok();
            self.runtime.remove_container(&container_name, self.ctx)?;
        }

        // Find available host port
        let host_port = self.runtime.find_available_port(port, self.ctx)?;

        let mut container = ContainerConfig::new(&container_name, &self.app_image())
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

        // Set NODE_ENV for JS stacks (only for source deployments)
        if self.config.is_source_deployment() {
            container = container.env("NODE_ENV", "production");
        }

        // Add volume mounts for image deployments
        if !self.config.volumes.is_empty() {
            for vol in &self.config.volumes {
                let host_path = format!("{}/{}", self.config.data_path().display(), vol.volume_name);
                self.ctx.create_dir(&host_path)?;
                container = container.volume(&host_path, &vol.container_path);
            }
        }

        self.runtime.run_container(&container, self.ctx)?;

        // Connect to Traefik network for routing
        self.runtime.connect_network(&container_name, "flaase-network", self.ctx)?;

        Ok(())
    }

    /// Blue-green deployment (zero-downtime).
    fn start_app_blue_green(&self) -> Result<(), AppError> {
        let port = self.config.effective_port();
        let active_slot = self.active_slot()?;
        let new_container = self.inactive_slot_container_name()?;
        let old_container = self.active_container_name()?;

        ui::info(&format!(
            "Blue-green deployment: {} -> {}",
            if active_slot == "none" { "initial" } else { active_slot },
            if new_container.contains("blue") { "blue" } else { "green" }
        ));

        // Remove new container if it exists (from failed previous deployment)
        if self.runtime.container_exists(&new_container, self.ctx)? {
            self.runtime.stop_container(&new_container, self.ctx).ok();
            self.runtime.remove_container(&new_container, self.ctx)?;
        }

        // Find available host port
        let host_port = self.runtime.find_available_port(port, self.ctx)?;

        // Determine slot label
        let slot = if new_container.contains("blue") { "blue" } else { "green" };

        let mut container = ContainerConfig::new(&new_container, &self.app_image())
            .port(host_port, port)
            .network(&self.network_name())
            .restart(RestartPolicy::UnlessStopped)
            .label("flaase.managed", "true")
            .label("flaase.app", &self.config.name)
            .label("flaase.service", "web")
            .label("flaase.slot", slot);

        // Add environment files
        let env_path = self.config.env_path();
        let auto_env_path = self.config.auto_env_path();

        if auto_env_path.exists() {
            container = container.env_file(auto_env_path.to_str().unwrap());
        }
        if env_path.exists() {
            container = container.env_file(env_path.to_str().unwrap());
        }

        // Set NODE_ENV for JS stacks (only for source deployments)
        if self.config.is_source_deployment() {
            container = container.env("NODE_ENV", "production");
        }

        // Add volume mounts for image deployments
        if !self.config.volumes.is_empty() {
            for vol in &self.config.volumes {
                let host_path = format!("{}/{}", self.config.data_path().display(), vol.volume_name);
                self.ctx.create_dir(&host_path)?;
                container = container.volume(&host_path, &vol.container_path);
            }
        }

        // Start new container
        ui::info(&format!("  Starting new container: {}", new_container));
        self.runtime.run_container(&container, self.ctx)?;

        // Connect to Traefik network for routing
        self.runtime.connect_network(&new_container, "flaase-network", self.ctx)?;

        // Health check on new container before switching traffic
        ui::info("  Running health check on new container...");
        self.health_check_container(&new_container)?;

        // Switch traffic to new container (update Traefik config)
        ui::info("  Switching traffic to new container...");
        self.configure_routing_for_container(&new_container)?;

        // Handle old container
        if let Some(old) = old_container {
            let bg_config = self.blue_green_config();
            let keep_seconds = bg_config.map(|c| c.keep_old_seconds).unwrap_or(300);
            let auto_cleanup = bg_config.map(|c| c.auto_cleanup).unwrap_or(true);

            if keep_seconds == 0 {
                // Stop immediately
                ui::info(&format!("  Stopping old container: {}", old));
                self.runtime.stop_container(&old, self.ctx).ok();
                self.runtime.remove_container(&old, self.ctx).ok();
            } else if auto_cleanup {
                // Schedule cleanup in background
                ui::info(&format!(
                    "  Old container {} will be stopped in {}s (instant rollback available)",
                    old, keep_seconds
                ));
                self.schedule_container_cleanup(&old, keep_seconds);
            } else {
                ui::info(&format!(
                    "  Old container {} kept running (manual cleanup required)",
                    old
                ));
            }
        }

        ui::success("  Blue-green deployment complete!");
        Ok(())
    }

    /// Performs health check on a specific container.
    fn health_check_container(&self, container_name: &str) -> Result<(), AppError> {
        if self.ctx.is_dry_run() {
            return Ok(());
        }

        let health_config = self.config.effective_health_check();
        let port = self.config.effective_port();

        for attempt in 1..=health_config.retries {
            // Check if container is running
            if !self.runtime.container_is_running(container_name, self.ctx)? {
                return Err(AppError::Deploy(format!(
                    "Container {} stopped unexpectedly",
                    container_name
                )));
            }

            // Try HTTP health check via Traefik
            let url = format!("http://{}:{}{}", container_name, port, health_config.endpoint);
            let timeout = health_config.timeout.to_string();

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
                return Ok(());
            }

            // Fallback: check inside the container itself
            let wget_result = self.runtime.exec_in_container(
                container_name,
                &["wget", "-q", "--spider", &format!("http://localhost:{}{}", port, health_config.endpoint)],
                self.ctx,
            );

            if wget_result.is_ok() {
                return Ok(());
            }

            if attempt < health_config.retries {
                std::thread::sleep(Duration::from_secs(health_config.interval as u64));
            }
        }

        // Get container logs for debugging
        let logs = self.runtime.get_logs(container_name, 30, self.ctx)?;

        Err(AppError::Deploy(format!(
            "Health check failed for {} after {} attempts.\n\nRecent logs:\n{}",
            container_name, health_config.retries, logs
        )))
    }

    /// Configures Traefik routing to point to a specific container.
    fn configure_routing_for_container(&self, container_name: &str) -> Result<(), AppError> {
        use crate::core::secrets::SecretsManager;
        use crate::templates::traefik::{generate_app_config_with_service, AppDomain};

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

        // Generate and write Traefik config pointing to specific container
        let traefik_config = generate_app_config_with_service(
            &self.config.name,
            &domains,
            port,
            container_name,
        );
        let traefik_path = format!(
            "{}/{}.yml",
            crate::core::FLAASE_TRAEFIK_DYNAMIC_PATH,
            self.config.name
        );

        self.ctx.write_file(&traefik_path, &traefik_config)
    }

    /// Schedules container cleanup in background.
    fn schedule_container_cleanup(&self, container_name: &str, delay_seconds: u64) {
        let container = container_name.to_string();

        // Spawn a background thread to cleanup after delay
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(delay_seconds));

            // Use docker directly since we don't have access to runtime here
            let _ = std::process::Command::new("docker")
                .args(["stop", &container])
                .output();
            let _ = std::process::Command::new("docker")
                .args(["rm", &container])
                .output();
        });
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

    // ========================================================================
    // Rollback System
    // ========================================================================

    /// Checks if rollback is enabled and should be performed.
    fn should_auto_rollback(&self) -> bool {
        self.config.autodeploy_config
            .as_ref()
            .and_then(|ad| ad.rollback.as_ref())
            .map(|r| r.enabled && r.auto_rollback_on_failure)
            .unwrap_or(false)
    }

    /// Checks if a previous image exists for rollback.
    pub fn can_rollback(&self) -> bool {
        self.image_exists(&self.previous_image_tag()).unwrap_or(false)
    }

    /// Rolls back to the previous deployment.
    pub fn rollback(&self, target_sha: Option<&str>) -> Result<(), AppError> {
        let target_tag = match target_sha {
            Some(sha) => self.versioned_image_tag(sha),
            None => self.previous_image_tag(),
        };

        if !self.image_exists(&target_tag)? {
            return Err(AppError::RollbackFailed(
                "No previous version available for rollback".into()
            ));
        }

        ui::info(&format!("Rolling back to image: {}", target_tag));

        // Tag rollback target as latest
        let latest_tag = self.current_image_tag();
        self.tag_image(&target_tag, &latest_tag)?;

        // Restart app with rolled-back image
        let spinner = ui::ProgressBar::spinner("Restarting app with previous version");
        self.start_app()?;
        spinner.finish("done");

        // Reconfigure routing
        let spinner = ui::ProgressBar::spinner("Reconfiguring routing");
        self.configure_routing()?;
        spinner.finish("done");

        // Health check
        let spinner = ui::ProgressBar::spinner("Running health check");
        self.health_check()?;
        spinner.finish("done");

        ui::success("Rollback completed successfully");

        Ok(())
    }

    /// Lists available versions for rollback.
    pub fn list_available_versions(&self) -> Result<Vec<String>, AppError> {
        let output = self.ctx.run_command(
            "docker",
            &["images", &self.image_name(), "--format", "{{.Tag}}"]
        )?;

        if !output.success {
            return Ok(Vec::new());
        }

        let versions: Vec<String> = output.stdout
            .lines()
            .filter(|t| !t.is_empty() && *t != "latest" && *t != "<none>")
            .map(|t| t.to_string())
            .collect();

        Ok(versions)
    }

    /// Destroys all resources for this app.
    /// If keep_data is true, database and cache volumes are preserved.
    pub fn destroy(&self, keep_data: bool) -> Result<(), AppError> {
        // Remove containers (they should already be stopped)
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

        // Remove volumes if not keeping data
        if !keep_data {
            let volumes = [
                format!("flaase-{}-db-data", self.config.name),
                format!("flaase-{}-cache-data", self.config.name),
            ];

            for volume in &volumes {
                // Use -f to ignore errors if volume doesn't exist
                self.ctx
                    .run_command("docker", &["volume", "rm", "-f", volume])
                    .ok();
            }
        }

        // Remove network
        let network = self.network_name();
        if self.runtime.network_exists(&network, self.ctx)? {
            self.ctx
                .run_command("docker", &["network", "rm", &network])
                .ok(); // Ignore errors, network might be in use
        }

        // Remove Traefik config
        self.proxy.remove_app_config(&self.config.name, self.ctx)?;

        // Remove Docker images (current and previous)
        let image = self.image_name();
        self.ctx
            .run_command("docker", &["rmi", "-f", &image])
            .ok();
        self.ctx
            .run_command("docker", &["rmi", "-f", &format!("{}:latest", image)])
            .ok();
        self.ctx
            .run_command("docker", &["rmi", "-f", &format!("{}:previous", image)])
            .ok();

        // Also remove any versioned tags
        let output = self.ctx.run_command(
            "docker",
            &["images", "--format", "{{.Repository}}:{{.Tag}}", &image],
        );
        if let Ok(output) = output {
            for line in output.stdout.lines() {
                if !line.is_empty() {
                    self.ctx.run_command("docker", &["rmi", "-f", line]).ok();
                }
            }
        }

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
