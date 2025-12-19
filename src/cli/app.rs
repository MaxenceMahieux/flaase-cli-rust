//! Application initialization command handler.

use std::path::PathBuf;

use crate::core::app_config::{
    AppConfig, CacheConfig, CacheType, DatabaseConfig, DatabaseType, Stack,
};
use crate::core::context::ExecutionContext;
use crate::core::error::AppError;
use crate::core::secrets::{AppSecrets, SecretsManager};
use crate::core::FLAASE_APPS_PATH;
use crate::providers::ssh::{SshKeyType, SshProvider};
use crate::ui;
use crate::utils::validation::{
    is_app_name_available, validate_app_name, validate_domain, validate_git_ssh_url,
};

/// Collected app configuration data during prompts.
#[derive(Debug, Clone)]
struct AppInitData {
    name: String,
    repository: String,
    ssh_key: PathBuf,
    stack: Stack,
    database: Option<DatabaseType>,
    cache: Option<CacheType>,
    domain: String,
    autodeploy: bool,
}

/// Fields that can be modified in the summary.
#[derive(Debug, Clone, Copy)]
enum ModifiableField {
    AppName,
    Repository,
    SshKey,
    Stack,
    Database,
    Cache,
    Domain,
    Autodeploy,
}

/// Executes the app init command.
pub fn init(verbose: bool) -> Result<(), AppError> {
    ui::header();

    let ctx = ExecutionContext::new(false, verbose);

    // Check if server is initialized
    if !crate::core::config::ServerConfig::is_initialized() {
        return Err(AppError::Config(
            "Server not initialized. Run 'fl server init' first.".into(),
        ));
    }

    // Collect all configuration through prompts
    let mut data = collect_app_data(&ctx)?;

    // Show summary and allow modifications
    loop {
        display_summary(&data);

        let action = prompt_summary_action()?;

        match action {
            SummaryAction::Confirm => break,
            SummaryAction::Modify(field) => {
                modify_field(&mut data, field, &ctx)?;
            }
            SummaryAction::Cancel => {
                return Err(AppError::Cancelled);
            }
        }
    }

    // Create the app
    create_app(&data, &ctx)?;

    println!();
    ui::success(&format!(
        "App configured at {}/{}/",
        FLAASE_APPS_PATH, data.name
    ));
    ui::info(&format!("Deploy with: fl deploy {}", data.name));

    Ok(())
}

/// Collects all app configuration through interactive prompts.
fn collect_app_data(ctx: &ExecutionContext) -> Result<AppInitData, AppError> {
    // 1. App name
    let name = prompt_app_name()?;

    // 2. Repository URL
    let repository = prompt_repository()?;

    // 3. SSH key selection or generation
    let ssh_key = prompt_ssh_key(ctx)?;

    // 4. Test SSH connection
    ui::info("Testing SSH connection to repository...");
    let connected = SshProvider::test_git_connection(&repository, &ssh_key, ctx)?;

    if connected {
        ui::success("SSH connection successful");
    } else {
        ui::warning(
            "Could not verify SSH connection. Make sure the key is added to your Git provider.",
        );
        let proceed = ui::confirm("Continue anyway?", false)?;
        if !proceed {
            return Err(AppError::Cancelled);
        }
    }

    println!();

    // 5. Stack selection
    let stack = prompt_stack()?;

    // 6. Database selection
    let database = prompt_database()?;

    // 7. Cache selection
    let cache = prompt_cache()?;

    // 8. Domain
    let domain = prompt_domain()?;

    // 9. Autodeploy
    let autodeploy = prompt_autodeploy()?;

    Ok(AppInitData {
        name,
        repository,
        ssh_key,
        stack,
        database,
        cache,
        domain,
        autodeploy,
    })
}

/// Prompts for app name with validation.
fn prompt_app_name() -> Result<String, AppError> {
    loop {
        let name = ui::input("What is the name of your app?")?;

        // Validate format
        if let Err(e) = validate_app_name(&name) {
            ui::error(&e.to_string());
            continue;
        }

        // Check availability
        if !is_app_name_available(&name) {
            ui::error(&format!("App '{}' already exists", name));
            continue;
        }

        return Ok(name);
    }
}

/// Prompts for repository URL.
fn prompt_repository() -> Result<String, AppError> {
    loop {
        let url = ui::input_with_placeholder(
            "GitHub repository URL?",
            Some("git@github.com:user/repo.git"),
        )?;

        if let Err(e) = validate_git_ssh_url(&url) {
            ui::error(&e.to_string());
            continue;
        }

        return Ok(url);
    }
}

/// Prompts for SSH key selection or generation.
fn prompt_ssh_key(ctx: &ExecutionContext) -> Result<PathBuf, AppError> {
    let keys = SshProvider::list_keys()?;

    if keys.is_empty() {
        ui::info("No SSH keys found. Let's generate one.");
        return generate_new_key(ctx);
    }

    // Build options list
    let mut options: Vec<String> = keys.iter().map(|k| k.display()).collect();
    options.push("Generate new key".to_string());

    let selected = ui::select("Which SSH key to use for cloning?", &options)?;

    if selected == keys.len() {
        // Generate new key
        generate_new_key(ctx)
    } else {
        Ok(keys[selected].path.clone())
    }
}

/// Generates a new SSH key.
fn generate_new_key(ctx: &ExecutionContext) -> Result<PathBuf, AppError> {
    // Key type selection
    let key_types: Vec<&str> = SshKeyType::all().iter().map(|t| t.display_name()).collect();
    let type_idx = ui::select("Which key type?", &key_types)?;
    let key_type = SshKeyType::all()[type_idx];

    // Filename
    let default_name = match key_type {
        SshKeyType::Ed25519 => "id_ed25519_flaase",
        SshKeyType::Rsa4096 => "id_rsa_flaase",
        SshKeyType::Ecdsa => "id_ecdsa_flaase",
    };

    let filename = ui::input_with_default("Key filename?", default_name)?;

    // Comment (optional)
    let comment = ui::input_with_placeholder("Key comment (optional)?", Some("flaase deploy key"))?;
    let comment_opt = if comment.is_empty() {
        None
    } else {
        Some(comment.as_str())
    };

    ui::info("Generating SSH key...");
    let key_path = SshProvider::generate_key(key_type, &filename, comment_opt, ctx)?;
    ui::success(&format!("Key generated: {}", key_path.display()));

    // Show public key
    println!();
    ui::info("Add this public key to your Git provider:");
    println!();

    let pub_key = SshProvider::get_public_key(&key_path)?;
    println!("{}", pub_key);

    println!();
    ui::confirm(
        "Press Enter when you've added the key to your Git provider",
        true,
    )?;

    Ok(key_path)
}

/// Prompts for stack selection.
fn prompt_stack() -> Result<Stack, AppError> {
    let stacks: Vec<&str> = Stack::all().iter().map(|s| s.display_name()).collect();
    let selected = ui::select("Which stack does your app use?", &stacks)?;
    Ok(Stack::all()[selected])
}

/// Prompts for database selection.
fn prompt_database() -> Result<Option<DatabaseType>, AppError> {
    let mut options: Vec<&str> = DatabaseType::all()
        .iter()
        .map(|d| d.display_name())
        .collect();
    options.push("None");

    let selected = ui::select("Do you need a database?", &options)?;

    if selected == DatabaseType::all().len() {
        Ok(None)
    } else {
        Ok(Some(DatabaseType::all()[selected]))
    }
}

/// Prompts for cache selection.
fn prompt_cache() -> Result<Option<CacheType>, AppError> {
    let mut options: Vec<&str> = CacheType::all().iter().map(|c| c.display_name()).collect();
    options.push("None");

    let selected = ui::select("Do you need a cache?", &options)?;

    if selected == CacheType::all().len() {
        Ok(None)
    } else {
        Ok(Some(CacheType::all()[selected]))
    }
}

/// Prompts for domain name.
fn prompt_domain() -> Result<String, AppError> {
    loop {
        let domain = ui::input_with_placeholder("Domain name?", Some("my-app.com"))?;

        if let Err(e) = validate_domain(&domain) {
            ui::error(&e.to_string());
            continue;
        }

        return Ok(domain);
    }
}

/// Prompts for autodeploy setting.
fn prompt_autodeploy() -> Result<bool, AppError> {
    Ok(ui::confirm("Enable autodeploy on git push?", true)?)
}

/// Displays the configuration summary.
fn display_summary(data: &AppInitData) {
    println!();

    let db_str = data
        .database
        .as_ref()
        .map(|d| d.display_name())
        .unwrap_or("None");
    let cache_str = data
        .cache
        .as_ref()
        .map(|c| c.display_name())
        .unwrap_or("None");
    let autodeploy_str = if data.autodeploy { "Yes" } else { "No" };

    println!("  Configuration Summary");
    println!("  {}", "-".repeat(50));
    println!();
    println!("  App name:     {}", data.name);
    println!("  Repository:   {}", data.repository);
    println!("  SSH Key:      {}", data.ssh_key.display());
    println!("  Stack:        {}", data.stack.display_name());
    println!("  Database:     {}", db_str);
    println!("  Cache:        {}", cache_str);
    println!("  Domain:       {}", data.domain);
    println!("  Autodeploy:   {}", autodeploy_str);
    println!();
}

/// Actions available after viewing summary.
enum SummaryAction {
    Confirm,
    Modify(ModifiableField),
    Cancel,
}

/// Prompts for action after viewing summary.
fn prompt_summary_action() -> Result<SummaryAction, AppError> {
    let options = [
        "Confirm and create",
        "Modify app name",
        "Modify repository",
        "Modify SSH key",
        "Modify stack",
        "Modify database",
        "Modify cache",
        "Modify domain",
        "Modify autodeploy",
        "Cancel",
    ];

    let selected = ui::select("What would you like to do?", &options)?;

    Ok(match selected {
        0 => SummaryAction::Confirm,
        1 => SummaryAction::Modify(ModifiableField::AppName),
        2 => SummaryAction::Modify(ModifiableField::Repository),
        3 => SummaryAction::Modify(ModifiableField::SshKey),
        4 => SummaryAction::Modify(ModifiableField::Stack),
        5 => SummaryAction::Modify(ModifiableField::Database),
        6 => SummaryAction::Modify(ModifiableField::Cache),
        7 => SummaryAction::Modify(ModifiableField::Domain),
        8 => SummaryAction::Modify(ModifiableField::Autodeploy),
        _ => SummaryAction::Cancel,
    })
}

/// Modifies a specific field.
fn modify_field(
    data: &mut AppInitData,
    field: ModifiableField,
    ctx: &ExecutionContext,
) -> Result<(), AppError> {
    match field {
        ModifiableField::AppName => {
            data.name = prompt_app_name()?;
        }
        ModifiableField::Repository => {
            data.repository = prompt_repository()?;
            // Re-test SSH connection
            ui::info("Testing SSH connection to repository...");
            let connected = SshProvider::test_git_connection(&data.repository, &data.ssh_key, ctx)?;
            if connected {
                ui::success("SSH connection successful");
            } else {
                ui::warning("Could not verify SSH connection");
            }
        }
        ModifiableField::SshKey => {
            data.ssh_key = prompt_ssh_key(ctx)?;
            // Re-test SSH connection
            ui::info("Testing SSH connection to repository...");
            let connected = SshProvider::test_git_connection(&data.repository, &data.ssh_key, ctx)?;
            if connected {
                ui::success("SSH connection successful");
            } else {
                ui::warning("Could not verify SSH connection");
            }
        }
        ModifiableField::Stack => {
            data.stack = prompt_stack()?;
        }
        ModifiableField::Database => {
            data.database = prompt_database()?;
        }
        ModifiableField::Cache => {
            data.cache = prompt_cache()?;
        }
        ModifiableField::Domain => {
            data.domain = prompt_domain()?;
        }
        ModifiableField::Autodeploy => {
            data.autodeploy = prompt_autodeploy()?;
        }
    }

    Ok(())
}

/// Creates the app directories and configuration files.
fn create_app(data: &AppInitData, ctx: &ExecutionContext) -> Result<(), AppError> {
    ui::info("Creating app configuration...");

    // Create app directory structure
    let app_dir = format!("{}/{}", FLAASE_APPS_PATH, data.name);
    ctx.create_dir(&app_dir)?;
    ctx.create_dir(&format!("{}/repo", app_dir))?;
    ctx.create_dir(&format!("{}/data", app_dir))?;

    // Build app config
    let database_config = data
        .database
        .map(|db_type| DatabaseConfig::new(db_type, &data.name));

    let cache_config = data.cache.map(CacheConfig::new);

    let config = AppConfig::new(
        data.name.clone(),
        data.repository.clone(),
        data.ssh_key.clone(),
        data.stack,
        data.domain.clone(),
        database_config.clone(),
        cache_config.clone(),
        data.autodeploy,
    );

    // Save config.yml
    config.save()?;

    // Generate and save secrets if needed
    let mut secrets = AppSecrets::default();

    if let Some(db_type) = data.database {
        secrets.database = Some(SecretsManager::generate_database_secrets(
            db_type, &data.name,
        ));
    }

    if let Some(cache_type) = data.cache {
        secrets.cache = Some(SecretsManager::generate_cache_secrets(cache_type));
    }

    // Save secrets file
    if secrets.database.is_some() || secrets.cache.is_some() {
        SecretsManager::save_secrets(&config.secrets_path(), &secrets)?;

        // Generate .env file with connection URLs
        let db_name = database_config
            .as_ref()
            .map(|d| d.name.as_str())
            .unwrap_or("");
        let env_vars = SecretsManager::generate_env_vars(
            &secrets,
            data.database,
            db_name,
            data.cache,
            &data.name,
        );

        SecretsManager::write_env_file(&config.auto_env_path(), &env_vars)?;
    }

    Ok(())
}
