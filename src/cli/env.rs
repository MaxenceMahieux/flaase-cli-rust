//! Environment variable command handlers.

use std::path::PathBuf;
use std::process::Command;

use crate::core::app_config::AppConfig;
use crate::core::env::{EnvManager, EnvSource};
use crate::core::error::AppError;
use crate::core::FLAASE_APPS_PATH;
use crate::ui;

/// Lists environment variables for an app.
pub fn list(app: &str, show_values: bool) -> Result<(), AppError> {
    let app_dir = get_app_dir(app)?;
    let vars = EnvManager::load(&app_dir)?;

    if vars.is_empty() {
        ui::info(&format!("No environment variables for {}", app));
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

    println!("Environment variables for {}", app);
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
pub fn set(app: &str, assignments: &[String]) -> Result<(), AppError> {
    let app_dir = get_app_dir(app)?;

    // Parse all assignments
    let mut parsed: Vec<(String, String)> = Vec::new();
    for assignment in assignments {
        let (key, value) = EnvManager::parse_assignment(assignment)?;
        parsed.push((key, value));
    }

    // Set variables
    let count = EnvManager::set(&app_dir, &parsed)?;

    ui::success(&format!(
        "Set {} environment variable{}",
        count,
        if count == 1 { "" } else { "s" }
    ));

    // Ask to restart
    prompt_restart(app)?;

    Ok(())
}

/// Removes an environment variable from an app.
pub fn remove(app: &str, key: &str) -> Result<(), AppError> {
    let app_dir = get_app_dir(app)?;

    let removed = EnvManager::remove(&app_dir, key)?;

    if removed {
        ui::success(&format!("Removed {}", key));
        prompt_restart(app)?;
    } else {
        ui::warning(&format!("Variable '{}' not found", key));
    }

    Ok(())
}

/// Opens the env file in the user's editor.
pub fn edit(app: &str) -> Result<(), AppError> {
    let app_dir = get_app_dir(app)?;
    let env_path = EnvManager::get_user_env_path(&app_dir);

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

    ui::success("Environment file saved");

    // Validate the file after editing
    match EnvManager::load_user(&app_dir) {
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
