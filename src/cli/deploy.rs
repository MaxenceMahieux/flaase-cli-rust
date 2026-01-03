//! Deployment command handler.

use crate::core::app_config::AppConfig;
use crate::core::context::ExecutionContext;
use crate::core::deploy::{format_duration, Deployer};
use crate::core::error::AppError;
use crate::providers::{create_container_runtime, create_reverse_proxy};
use crate::ui;

/// Executes the deploy command.
pub fn deploy(app_name: &str, verbose: bool) -> Result<(), AppError> {
    ui::header();

    // Load app config
    let config = AppConfig::load(app_name)?;

    // Check if server is initialized
    if !crate::core::config::ServerConfig::is_initialized() {
        return Err(AppError::Config(
            "Server not initialized. Run 'fl server init' first.".into(),
        ));
    }

    let ctx = ExecutionContext::new(false, verbose);
    let runtime = create_container_runtime();
    let proxy = create_reverse_proxy();

    ui::section(&format!("Deploying {}", app_name));

    let deployer = Deployer::new(&config, runtime.as_ref(), proxy.as_ref(), &ctx);

    match deployer.deploy() {
        Ok(result) => {
            println!();
            ui::success(&format!(
                "Deployed in {}",
                format_duration(result.duration)
            ));
            println!();
            ui::url(&result.url);

            Ok(())
        }
        Err(e) => {
            ui::step_failed();
            println!();
            ui::error(&format!("Deployment failed: {}", e));
            println!();
            ui::info("To see logs, run:");
            ui::info(&format!("  fl logs {}", app_name));
            println!();
            ui::info("To cleanup failed deployment:");
            ui::info(&format!("  docker rm -f flaase-{}-web", app_name));

            Err(e)
        }
    }
}

/// Stops an app.
pub fn stop(app_name: &str, verbose: bool) -> Result<(), AppError> {
    // Load app config
    let config = AppConfig::load(app_name)?;
    let ctx = ExecutionContext::new(false, verbose);
    let runtime = create_container_runtime();
    let proxy = create_reverse_proxy();

    let spinner = ui::ProgressBar::spinner(&format!("Stopping {}", app_name));

    let deployer = Deployer::new(&config, runtime.as_ref(), proxy.as_ref(), &ctx);

    match deployer.stop() {
        Ok(()) => {
            spinner.finish("stopped");
            println!();
            ui::success("App stopped");
            Ok(())
        }
        Err(e) => {
            spinner.finish("failed");
            Err(e)
        }
    }
}

/// Starts a stopped app.
pub fn start(app_name: &str, verbose: bool) -> Result<(), AppError> {
    // Load app config
    let config = AppConfig::load(app_name)?;
    let ctx = ExecutionContext::new(false, verbose);
    let runtime = create_container_runtime();
    let proxy = create_reverse_proxy();

    let spinner = ui::ProgressBar::spinner(&format!("Starting {}", app_name));

    let deployer = Deployer::new(&config, runtime.as_ref(), proxy.as_ref(), &ctx);

    match deployer.start() {
        Ok(()) => {
            spinner.finish("running");
            println!();
            ui::success(&format!("App running at https://{}", config.primary_domain()));
            Ok(())
        }
        Err(e) => {
            spinner.finish("failed");
            println!();
            ui::error(&format!("Failed to start: {}", e));
            Err(e)
        }
    }
}

/// Restarts an app.
pub fn restart(app_name: &str, verbose: bool) -> Result<(), AppError> {
    // Load app config
    let config = AppConfig::load(app_name)?;
    let ctx = ExecutionContext::new(false, verbose);
    let runtime = create_container_runtime();
    let proxy = create_reverse_proxy();

    let spinner = ui::ProgressBar::spinner(&format!("Restarting {}", app_name));

    let deployer = Deployer::new(&config, runtime.as_ref(), proxy.as_ref(), &ctx);

    // Stop without maintenance page (we're restarting immediately)
    if let Err(e) = runtime.stop_container(&format!("flaase-{}-web", app_name), &ctx) {
        // Container might not be running, continue anyway
        if verbose {
            ui::warning(&format!("Stop warning: {}", e));
        }
    }

    match deployer.start() {
        Ok(()) => {
            spinner.finish("restarted");
            println!();
            ui::success(&format!("App restarted at https://{}", config.primary_domain()));
            Ok(())
        }
        Err(e) => {
            spinner.finish("failed");
            println!();
            ui::error(&format!("Failed to restart: {}", e));
            Err(e)
        }
    }
}

/// Destroys an app completely.
pub fn destroy(app_name: &str, verbose: bool) -> Result<(), AppError> {
    ui::header();

    let config = AppConfig::load(app_name)?;

    // Confirmation prompt
    ui::warning(&format!(
        "This will permanently delete app '{}' and all its data.",
        app_name
    ));
    println!();

    let confirm = ui::confirm(&format!("Are you sure you want to destroy '{}'?", app_name), false)?;

    if !confirm {
        return Err(AppError::Cancelled);
    }

    let ctx = ExecutionContext::new(false, verbose);
    let runtime = create_container_runtime();
    let proxy = create_reverse_proxy();

    ui::info(&format!("Destroying {}...", app_name));

    let deployer = Deployer::new(&config, runtime.as_ref(), proxy.as_ref(), &ctx);
    deployer.destroy()?;

    // Remove app directory
    let app_dir = config.app_dir();
    if app_dir.exists() {
        std::fs::remove_dir_all(&app_dir)
            .map_err(|e| AppError::Deploy(format!("Failed to remove app directory: {}", e)))?;
    }

    ui::success(&format!("App '{}' has been destroyed", app_name));

    Ok(())
}

/// Updates a deployed app.
pub fn update(app_name: &str, verbose: bool) -> Result<(), AppError> {
    // Update is the same as deploy for now
    // In the future, this could implement zero-downtime blue-green deployment
    deploy(app_name, verbose)
}

/// Rolls back to a previous deployment.
pub fn rollback(app_name: &str, target: Option<&str>, list: bool, verbose: bool) -> Result<(), AppError> {
    ui::header();

    let config = AppConfig::load(app_name)?;
    let ctx = ExecutionContext::new(false, verbose);
    let runtime = create_container_runtime();
    let proxy = create_reverse_proxy();

    let deployer = Deployer::new(&config, runtime.as_ref(), proxy.as_ref(), &ctx);

    // List available versions
    if list {
        ui::section(&format!("Available versions for {}", app_name));
        println!();

        let versions = deployer.list_available_versions()?;

        if versions.is_empty() {
            ui::info("No previous versions available for rollback.");
            ui::info("Deploy at least once to create a version history.");
            return Ok(());
        }

        // Check which versions exist
        let has_previous = deployer.can_rollback();

        println!("  {}  previous (quick rollback)",
            if has_previous {
                console::style("●").green()
            } else {
                console::style("○").dim()
            }
        );

        for version in &versions {
            if version != "previous" {
                println!("  {}  {}", console::style("●").cyan(), version);
            }
        }

        println!();
        ui::info("Usage:");
        ui::info(&format!("  fl rollback {}              # Rollback to previous", app_name));
        ui::info(&format!("  fl rollback {} --to <sha>   # Rollback to specific version", app_name));

        return Ok(());
    }

    // Check if rollback is possible
    if target.is_none() && !deployer.can_rollback() {
        return Err(AppError::RollbackFailed(
            "No previous version available. Use --list to see available versions.".into()
        ));
    }

    // Confirmation
    let target_display = target.unwrap_or("previous");
    ui::warning(&format!(
        "This will rollback '{}' to version: {}",
        app_name, target_display
    ));
    println!();

    let confirm = ui::confirm("Continue with rollback?", true)?;

    if !confirm {
        return Err(AppError::Cancelled);
    }

    ui::section(&format!("Rolling back {}", app_name));
    println!();

    match deployer.rollback(target) {
        Ok(()) => {
            println!();
            ui::success(&format!(
                "Rolled back to {}",
                target_display
            ));
            println!();
            ui::url(&format!("https://{}", config.primary_domain()));
            Ok(())
        }
        Err(e) => {
            println!();
            ui::error(&format!("Rollback failed: {}", e));
            Err(e)
        }
    }
}
