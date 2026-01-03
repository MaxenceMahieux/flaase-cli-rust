//! Environment variable command handlers.

use std::path::PathBuf;
use std::process::Command;

use crate::core::app_config::AppConfig;
use crate::core::env::{EnvManager, EnvSource};
use crate::core::error::AppError;
use crate::core::FLAASE_APPS_PATH;
use crate::ui;

/// Gets the env file path for a specific environment.
fn get_env_path(app_dir: &PathBuf, environment: Option<&str>) -> PathBuf {
    let env = environment.unwrap_or("production");
    if env == "production" || env.is_empty() {
        app_dir.join(".env")
    } else {
        app_dir.join(format!(".env.{}", env))
    }
}

/// Lists environment variables for an app.
pub fn list(app: &str, show_values: bool, environment: Option<&str>) -> Result<(), AppError> {
    let app_dir = get_app_dir(app)?;
    let env_name = environment.unwrap_or("production");
    let env_path = get_env_path(&app_dir, environment);

    let vars = if env_path.exists() {
        EnvManager::load_from_file(&env_path)?
    } else if environment.is_some() {
        // Environment-specific file doesn't exist
        Vec::new()
    } else {
        // Load default (includes auto-generated)
        EnvManager::load(&app_dir)?
    };

    if vars.is_empty() {
        ui::info(&format!("No environment variables for {} ({})", app, env_name));
        return Ok(());
    }

    // If --show, ask for confirmation
    if show_values {
        ui::warning("Values will be displayed in plain text.");
        let confirm = ui::confirm("Are you sure?", false)?;
        if !confirm {
            return Ok(());
        }
        println!();
    }

    println!("Environment variables for {} ({}):", app, console::style(env_name).cyan());
    println!();

    // Calculate column widths
    let max_key_len = vars.iter().map(|v| v.key.len()).max().unwrap_or(0).max(4);
    let max_val_len = if show_values {
        vars.iter().map(|v| v.value.len()).max().unwrap_or(0).max(5)
    } else {
        22
    };

    // Header
    println!(
        "  {:<width_key$}   {:<width_val$}   Source",
        "Name",
        "Value",
        width_key = max_key_len,
        width_val = max_val_len
    );
    println!("  {}", "─".repeat(max_key_len + max_val_len + 12));

    // Variables
    for var in &vars {
        let value = if show_values {
            var.value.clone()
        } else {
            var.masked_value()
        };

        let source = match var.source {
            EnvSource::Auto => "(auto)",
            EnvSource::User => "",
        };

        println!(
            "  {:<width_key$}   {:<width_val$}   {}",
            var.key,
            value,
            source,
            width_key = max_key_len,
            width_val = max_val_len
        );
    }

    println!();

    let (user_count, auto_count) = EnvManager::count(&vars);
    if auto_count > 0 {
        println!(
            "{} user variable{}, {} auto-generated",
            user_count,
            if user_count == 1 { "" } else { "s" },
            auto_count
        );
    } else {
        println!(
            "{} variable{}",
            user_count,
            if user_count == 1 { "" } else { "s" }
        );
    }

    Ok(())
}

/// Sets environment variables for an app.
pub fn set(app: &str, assignments: &[String], environment: Option<&str>) -> Result<(), AppError> {
    let app_dir = get_app_dir(app)?;
    let env_name = environment.unwrap_or("production");
    let env_path = get_env_path(&app_dir, environment);

    // Parse all assignments
    let mut parsed: Vec<(String, String)> = Vec::new();
    for assignment in assignments {
        let (key, value) = EnvManager::parse_assignment(assignment)?;
        parsed.push((key, value));
    }

    // Set variables in the environment-specific file
    let count = EnvManager::set_to_file(&env_path, &parsed)?;

    ui::success(&format!(
        "Set {} environment variable{} for {}",
        count,
        if count == 1 { "" } else { "s" },
        env_name
    ));

    // Ask to restart only if production
    if env_name == "production" {
        prompt_restart(app)?;
    }

    Ok(())
}

/// Removes an environment variable from an app.
pub fn remove(app: &str, key: &str, environment: Option<&str>) -> Result<(), AppError> {
    let app_dir = get_app_dir(app)?;
    let env_name = environment.unwrap_or("production");
    let env_path = get_env_path(&app_dir, environment);

    let removed = EnvManager::remove_from_file(&env_path, key)?;

    if removed {
        ui::success(&format!("Removed {} from {}", key, env_name));
        if env_name == "production" {
            prompt_restart(app)?;
        }
    } else {
        ui::warning(&format!("Variable '{}' not found in {}", key, env_name));
    }

    Ok(())
}

/// Opens the env file in the user's editor.
pub fn edit(app: &str, environment: Option<&str>) -> Result<(), AppError> {
    let app_dir = get_app_dir(app)?;
    let env_name = environment.unwrap_or("production");
    let env_path = get_env_path(&app_dir, environment);

    // Ensure the file exists
    if !env_path.exists() {
        std::fs::write(
            &env_path,
            "# Environment variables for this app\n# Format: KEY=value\n\n",
        )
        .map_err(|e| AppError::Config(format!("Failed to create env file: {}", e)))?;

        // Set permissions
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&env_path, std::fs::Permissions::from_mode(0o600))
                .map_err(|e| AppError::Config(format!("Failed to set permissions: {}", e)))?;
        }
    }

    let editor = EnvManager::get_editor();

    ui::info(&format!("Opening {} in {}...", env_path.display(), editor));

    // Open editor
    let status = Command::new(&editor)
        .arg(&env_path)
        .status()
        .map_err(|e| AppError::Command(format!("Failed to open editor '{}': {}", editor, e)))?;

    if !status.success() {
        return Err(AppError::Command("Editor exited with error".into()));
    }

    ui::success(&format!("Environment file saved ({})", env_name));

    // Validate the file after editing
    match EnvManager::load_from_file(&env_path) {
        Ok(vars) => {
            ui::info(&format!(
                "{} variable{} defined",
                vars.len(),
                if vars.len() == 1 { "" } else { "s" }
            ));
        }
        Err(e) => {
            ui::warning(&format!("Warning: {}", e));
        }
    }

    // Ask to restart
    prompt_restart(app)?;

    Ok(())
}

/// Gets the app directory and validates it exists.
fn get_app_dir(app: &str) -> Result<PathBuf, AppError> {
    // Check if app exists
    let _ = AppConfig::load(app)?;

    Ok(PathBuf::from(format!("{}/{}", FLAASE_APPS_PATH, app)))
}

/// Prompts the user to restart the app.
fn prompt_restart(app: &str) -> Result<(), AppError> {
    println!();

    let restart = ui::confirm(
        &format!("Restart {} for changes to take effect?", app),
        true,
    )?;

    if restart {
        // TODO: Call restart when implemented
        ui::warning(&format!(
            "Restart not yet implemented. Run: fl restart {}",
            app
        ));
    } else {
        ui::info(&format!(
            "Run 'fl restart {}' for changes to take effect",
            app
        ));
    }

    Ok(())
}

/// Copies environment variables from one environment to another.
pub fn copy(app: &str, from: &str, to: &str) -> Result<(), AppError> {
    let app_dir = get_app_dir(app)?;

    let from_path = get_env_path(&app_dir, Some(from));
    let to_path = get_env_path(&app_dir, Some(to));

    if !from_path.exists() {
        return Err(AppError::Config(format!(
            "Source environment '{}' does not exist",
            from
        )));
    }

    // Confirm if target exists
    if to_path.exists() {
        ui::warning(&format!(
            "Environment '{}' already exists and will be overwritten.",
            to
        ));
        let confirm = ui::confirm("Continue?", false)?;
        if !confirm {
            return Ok(());
        }
    }

    let count = EnvManager::copy_env_file(&from_path, &to_path)?;

    ui::success(&format!(
        "Copied {} variable{} from {} to {}",
        count,
        if count == 1 { "" } else { "s" },
        from,
        to
    ));

    Ok(())
}

/// Lists all environments with their variable counts.
pub fn envs(app: &str) -> Result<(), AppError> {
    let app_dir = get_app_dir(app)?;

    let environments = EnvManager::list_environments(&app_dir)?;

    if environments.is_empty() {
        ui::info(&format!("No environments configured for {}", app));
        ui::info("Use 'fl env set <app> KEY=value --env <name>' to create one.");
        return Ok(());
    }

    println!("Environments for {}:", app);
    println!();

    for env_name in &environments {
        let env_path = get_env_path(&app_dir, Some(env_name));
        let vars = EnvManager::load_from_file(&env_path)?;

        let status = if env_name == "production" {
            console::style("(default)").dim()
        } else {
            console::style("").dim()
        };

        println!(
            "  {}  {} variable{} {}",
            console::style(env_name).cyan(),
            vars.len(),
            if vars.len() == 1 { "" } else { "s" },
            status
        );
    }

    println!();
    ui::info("Use 'fl env list <app> --env <name>' to view variables.");

    Ok(())
}
