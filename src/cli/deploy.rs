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
pub fn destroy(app_name: &str, force: bool, mut keep_data: bool, verbose: bool) -> Result<(), AppError> {
    ui::header();

    let config = AppConfig::load(app_name)?;
    let ctx = ExecutionContext::new(false, verbose);
    let runtime = create_container_runtime();
    let proxy = create_reverse_proxy();

    // Check if app is currently running
    let web_container = format!("flaase-{}-web", app_name);
    let is_running = runtime.container_is_running(&web_container, &ctx).unwrap_or(false);

    if is_running {
        ui::warning(&format!("App '{}' is currently running.", app_name));
        println!();
    }

    let has_database = config.database.is_some();
    let has_cache = config.cache.is_some();
    let has_data = has_database || has_cache;

    if !force {
        // Show what will be deleted
        ui::warning("This will permanently delete:");
        println!();
        println!("  {} App container (flaase-{}-web)", console::style("•").dim(), app_name);

        if has_database {
            println!("  {} Database container (flaase-{}-db)", console::style("•").dim(), app_name);
        }
        if has_cache {
            println!("  {} Cache container (flaase-{}-cache)", console::style("•").dim(), app_name);
        }

        println!("  {} Docker network", console::style("•").dim());
        println!("  {} Traefik routing config", console::style("•").dim());
        println!("  {} App directory (/opt/flaase/apps/{})", console::style("•").dim(), app_name);
        println!("  {} Docker images", console::style("•").dim());

        if !keep_data && has_data {
            println!(
                "  {} {} Database/cache volumes (ALL DATA WILL BE LOST)",
                console::style("•").red(),
                console::style("⚠").red().bold()
            );
        }

        println!();

        // Require typing app name to confirm
        let prompt = format!(
            "Type '{}' to confirm destruction",
            console::style(app_name).cyan().bold()
        );
        let input = ui::input(&prompt)?;

        if input.trim() != app_name {
            println!();
            ui::error("App name doesn't match. Destruction cancelled.");
            return Err(AppError::Cancelled);
        }

        // Ask about data if not specified via --keep-data
        if !keep_data && has_data {
            println!();
            ui::warning("Database and cache volumes contain your application data.");
            let delete_data = ui::confirm(
                "Delete volumes? (THIS CANNOT BE UNDONE)",
                false,
            )?;

            if !delete_data {
                keep_data = true;
                ui::info("Volumes will be preserved.");
            }
        }
    }

    println!();

    // Step 1: Stop containers
    let spinner = ui::ProgressBar::spinner("Stopping containers...");

    let deployer = Deployer::new(&config, runtime.as_ref(), proxy.as_ref(), &ctx);

    // Stop all containers
    for container in &[
        format!("flaase-{}-web", app_name),
        format!("flaase-{}-db", app_name),
        format!("flaase-{}-cache", app_name),
    ] {
        if runtime.container_exists(container, &ctx).unwrap_or(false) {
            let _ = runtime.stop_container(container, &ctx);
        }
    }
    spinner.finish("stopped");

    // Step 2: Remove containers and optionally volumes
    let spinner = ui::ProgressBar::spinner(if keep_data {
        "Removing containers..."
    } else {
        "Removing containers and volumes..."
    });

    deployer.destroy(keep_data)?;
    spinner.finish("removed");

    // Step 3: Cleanup app directory
    let spinner = ui::ProgressBar::spinner("Cleaning up...");

    let app_dir = config.app_dir();
    if app_dir.exists() {
        std::fs::remove_dir_all(&app_dir)
            .map_err(|e| AppError::Deploy(format!("Failed to remove app directory: {}", e)))?;
    }
    spinner.finish("done");

    println!();

    if keep_data {
        ui::success(&format!("App '{}' has been destroyed (data preserved)", app_name));
        println!();
        ui::info("To delete remaining volumes later:");
        if has_database {
            println!("  docker volume rm flaase-{}-db-data", app_name);
        }
        if has_cache {
            println!("  docker volume rm flaase-{}-cache-data", app_name);
        }
    } else {
        ui::success(&format!("App '{}' has been completely destroyed", app_name));
    }

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
