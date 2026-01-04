//! Application initialization command handler.

use std::path::PathBuf;

use crate::core::app_config::{
    AppConfig, CacheConfig, CacheType, DatabaseConfig, DatabaseType, DeploymentType, Framework,
    HealthCheckConfig, ImageConfig, PackageManager, RegistryCredentials, Stack, StackConfig,
    VolumeMount,
};
use crate::core::context::ExecutionContext;
use crate::core::error::AppError;
use crate::core::registry::{detect_default_port, parse_image_reference, save_credentials};
use crate::core::secrets::{AppSecrets, SecretsManager};
use crate::core::FLAASE_APPS_PATH;
use crate::providers::ssh::{SshKeyType, SshProvider};
use crate::ui;
use crate::utils::validation::{
    is_app_name_available, validate_app_name, validate_domain, validate_git_ssh_url,
};

/// Source deployment configuration.
#[derive(Debug, Clone)]
struct SourceInitData {
    name: String,
    repository: String,
    ssh_key: PathBuf,
    stack: Stack,
    stack_config: Option<StackConfig>,
    port: Option<u16>,
    database: Option<DatabaseType>,
    cache: Option<CacheType>,
    domain: String,
    autodeploy: bool,
}

/// Image deployment configuration.
#[derive(Debug, Clone)]
struct ImageInitData {
    name: String,
    image: ImageConfig,
    port: u16,
    volumes: Vec<VolumeMount>,
    database: Option<DatabaseType>,
    cache: Option<CacheType>,
    domain: String,
    health_check: HealthCheckConfig,
    credentials: Option<RegistryCredentials>,
}

/// Fields that can be modified in the summary (source deployment).
#[derive(Debug, Clone, Copy)]
enum SourceModifiableField {
    AppName,
    Repository,
    SshKey,
    Stack,
    StackConfig,
    Port,
    Database,
    Cache,
    Domain,
    Autodeploy,
}

/// Fields that can be modified in the summary (image deployment).
#[derive(Debug, Clone, Copy)]
enum ImageModifiableField {
    AppName,
    Image,
    Port,
    Volumes,
    Database,
    Cache,
    Domain,
    HealthCheck,
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

    // Ask for deployment type
    let deployment_type = prompt_deployment_type()?;

    match deployment_type {
        DeploymentType::Source => init_source_deployment(&ctx),
        DeploymentType::Image => init_image_deployment(&ctx),
    }
}

/// Prompts for deployment type.
fn prompt_deployment_type() -> Result<DeploymentType, AppError> {
    let options = ["From Git repository", "From Docker image"];
    let selected = ui::select("What type of deployment?", &options)?;

    Ok(match selected {
        0 => DeploymentType::Source,
        _ => DeploymentType::Image,
    })
}

/// Initializes a source-based deployment (from Git repository).
fn init_source_deployment(ctx: &ExecutionContext) -> Result<(), AppError> {
    // Collect all configuration through prompts
    let mut data = collect_source_data(ctx)?;

    // Show summary and allow modifications
    loop {
        display_source_summary(&data);

        let action = prompt_source_summary_action()?;

        match action {
            SourceSummaryAction::Confirm => break,
            SourceSummaryAction::Modify(field) => {
                modify_source_field(&mut data, field, ctx)?;
            }
            SourceSummaryAction::Cancel => {
                return Err(AppError::Cancelled);
            }
        }
    }

    // Create the app
    create_source_app(&data, ctx)?;

    println!();
    ui::success(&format!(
        "App configured at {}/{}/",
        FLAASE_APPS_PATH, data.name
    ));
    ui::info(&format!("Deploy with: fl deploy {}", data.name));

    Ok(())
}

/// Initializes an image-based deployment (from Docker registry).
fn init_image_deployment(ctx: &ExecutionContext) -> Result<(), AppError> {
    // Collect all configuration through prompts
    let mut data = collect_image_data()?;

    // Show summary and allow modifications
    loop {
        display_image_summary(&data);

        let action = prompt_image_summary_action()?;

        match action {
            ImageSummaryAction::Confirm => break,
            ImageSummaryAction::Modify(field) => {
                modify_image_field(&mut data, field)?;
            }
            ImageSummaryAction::Cancel => {
                return Err(AppError::Cancelled);
            }
        }
    }

    // Create the app
    create_image_app(&data, ctx)?;

    println!();
    ui::success(&format!(
        "App configured at {}/{}/",
        FLAASE_APPS_PATH, data.name
    ));
    ui::info(&format!("Deploy with: fl deploy {}", data.name));

    Ok(())
}

// ============================================================================
// Source Deployment Functions
// ============================================================================

/// Actions available after viewing source summary.
enum SourceSummaryAction {
    Confirm,
    Modify(SourceModifiableField),
    Cancel,
}

/// Collects source deployment configuration through interactive prompts.
fn collect_source_data(ctx: &ExecutionContext) -> Result<SourceInitData, AppError> {
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

    // Get stack configuration details
    let stack_config = prompt_stack_config(stack)?;

    // Get port if not using default
    let port = prompt_port(stack)?;

    Ok(SourceInitData {
        name,
        repository,
        ssh_key,
        stack,
        stack_config,
        port,
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

/// Prompts for stack configuration details.
fn prompt_stack_config(stack: Stack) -> Result<Option<StackConfig>, AppError> {
    // Skip for stacks that don't need extra config
    if matches!(stack, Stack::NextJs | Stack::NestJs | Stack::Laravel) {
        return Ok(None);
    }

    // For Dockerfile stack, no config needed
    if stack == Stack::Dockerfile {
        ui::info("Using existing Dockerfile from repository.");
        return Ok(None);
    }

    println!();
    ui::info(&format!("Configure {} settings (press Enter to skip):", stack.display_name()));

    // Version
    let version_placeholder = match stack {
        Stack::NodeJs => Some("22"),
        Stack::Python => Some("3.12"),
        Stack::Go => Some("1.22"),
        Stack::Ruby => Some("3.3"),
        Stack::Rust => Some("1.75"),
        Stack::Java => Some("21"),
        Stack::Php => Some("8.3"),
        _ => None,
    };

    let version = if let Some(placeholder) = version_placeholder {
        let input = ui::input_with_placeholder(
            &format!("{} version?", stack.display_name()),
            Some(placeholder),
        )?;
        if input.is_empty() { None } else { Some(input) }
    } else {
        None
    };

    // Package manager (for stacks with multiple options)
    let package_manager = prompt_package_manager(stack)?;

    // Framework (optional)
    let framework = prompt_framework(stack)?;

    // Start command (required for custom stacks)
    let start_command = if stack.requires_start_command() {
        let placeholder = stack.default_start_command();
        let cmd = ui::input_with_placeholder("Start command?", placeholder)?;
        if cmd.is_empty() { None } else { Some(cmd) }
    } else {
        None
    };

    // Build command (optional)
    let build_command = if stack.has_build_step() {
        let placeholder = stack.default_build_command();
        let cmd = ui::input_with_placeholder("Build command? (optional)", placeholder)?;
        if cmd.is_empty() { None } else { Some(cmd) }
    } else {
        None
    };

    // If nothing was configured, return None
    if version.is_none()
        && package_manager.is_none()
        && framework.is_none()
        && start_command.is_none()
        && build_command.is_none()
    {
        return Ok(None);
    }

    Ok(Some(StackConfig {
        version,
        package_manager,
        framework,
        build_command,
        start_command,
        install_command: None,
    }))
}

/// Prompts for package manager selection.
fn prompt_package_manager(stack: Stack) -> Result<Option<PackageManager>, AppError> {
    let options: Vec<(&str, PackageManager)> = match stack {
        Stack::NodeJs => vec![
            ("npm (default)", PackageManager::Npm),
            ("yarn", PackageManager::Yarn),
            ("pnpm", PackageManager::Pnpm),
        ],
        Stack::Python => vec![
            ("pip (default)", PackageManager::Pip),
            ("poetry", PackageManager::Poetry),
            ("pipenv", PackageManager::Pipenv),
            ("uv", PackageManager::Uv),
        ],
        Stack::Java => vec![
            ("Maven (default)", PackageManager::Maven),
            ("Gradle", PackageManager::Gradle),
        ],
        _ => return Ok(None),
    };

    if options.is_empty() {
        return Ok(None);
    }

    let labels: Vec<&str> = options.iter().map(|(l, _)| *l).collect();
    let selected = ui::select("Package manager?", &labels)?;
    Ok(Some(options[selected].1))
}

/// Prompts for framework selection.
fn prompt_framework(stack: Stack) -> Result<Option<Framework>, AppError> {
    let options: Vec<(&str, Framework)> = match stack {
        Stack::NodeJs => vec![
            ("Express", Framework::Express),
            ("Fastify", Framework::Fastify),
            ("Hono", Framework::Hono),
            ("Other / None", Framework::Other),
        ],
        Stack::Python => vec![
            ("Django", Framework::Django),
            ("FastAPI", Framework::FastApi),
            ("Flask", Framework::Flask),
            ("Other / None", Framework::Other),
        ],
        Stack::Go => vec![
            ("Gin", Framework::Gin),
            ("Echo", Framework::Echo),
            ("Fiber", Framework::Fiber),
            ("Chi", Framework::Chi),
            ("Other / None", Framework::Other),
        ],
        Stack::Ruby => vec![
            ("Rails", Framework::Rails),
            ("Sinatra", Framework::Sinatra),
            ("Other / None", Framework::Other),
        ],
        Stack::Rust => vec![
            ("Axum", Framework::Axum),
            ("Actix", Framework::Actix),
            ("Rocket", Framework::Rocket),
            ("Other / None", Framework::Other),
        ],
        Stack::Java => vec![
            ("Spring Boot", Framework::SpringBoot),
            ("Quarkus", Framework::Quarkus),
            ("Other / None", Framework::Other),
        ],
        Stack::Php => vec![
            ("Symfony", Framework::Symfony),
            ("Other / None", Framework::Other),
        ],
        _ => return Ok(None),
    };

    if options.is_empty() {
        return Ok(None);
    }

    let use_framework = ui::confirm("Specify a framework?", false)?;
    if !use_framework {
        return Ok(None);
    }

    let labels: Vec<&str> = options.iter().map(|(l, _)| *l).collect();
    let selected = ui::select("Which framework?", &labels)?;

    let framework = options[selected].1;
    if framework == Framework::Other {
        Ok(None)
    } else {
        Ok(Some(framework))
    }
}

/// Prompts for custom port.
fn prompt_port(stack: Stack) -> Result<Option<u16>, AppError> {
    let default_port = stack.default_port();

    let use_custom = ui::confirm(
        &format!("Use custom port? (default: {})", default_port),
        false,
    )?;

    if !use_custom {
        return Ok(None);
    }

    loop {
        let input = ui::input("Port number?")?;
        match input.parse::<u16>() {
            Ok(port) if port > 0 => return Ok(Some(port)),
            _ => {
                ui::error("Please enter a valid port number (1-65535)");
                continue;
            }
        }
    }
}

/// Displays the source deployment configuration summary.
fn display_source_summary(data: &SourceInitData) {
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

    // Format stack config details
    let stack_details = format_stack_config(&data.stack_config);
    let port_str = data
        .port
        .map(|p| p.to_string())
        .unwrap_or_else(|| format!("{} (default)", data.stack.default_port()));

    println!("  Configuration Summary (Source Deployment)");
    println!("  {}", "-".repeat(50));
    println!();
    println!("  App name:     {}", data.name);
    println!("  Repository:   {}", data.repository);
    println!("  SSH Key:      {}", data.ssh_key.display());
    println!("  Stack:        {}", data.stack.display_name());
    if !stack_details.is_empty() {
        println!("  Stack config: {}", stack_details);
    }
    println!("  Port:         {}", port_str);
    println!("  Database:     {}", db_str);
    println!("  Cache:        {}", cache_str);
    println!("  Domain:       {}", data.domain);
    println!("  Autodeploy:   {}", autodeploy_str);
    println!();
}

/// Formats stack configuration for display.
fn format_stack_config(config: &Option<StackConfig>) -> String {
    let config = match config {
        Some(c) => c,
        None => return String::new(),
    };

    let mut parts = Vec::new();

    if let Some(version) = &config.version {
        parts.push(format!("v{}", version));
    }
    if let Some(pm) = &config.package_manager {
        parts.push(pm.display_name().to_string());
    }
    if let Some(fw) = &config.framework {
        parts.push(fw.display_name().to_string());
    }

    parts.join(", ")
}

/// Prompts for action after viewing source summary.
fn prompt_source_summary_action() -> Result<SourceSummaryAction, AppError> {
    let options = [
        "Confirm and create",
        "Modify app name",
        "Modify repository",
        "Modify SSH key",
        "Modify stack",
        "Modify stack config",
        "Modify port",
        "Modify database",
        "Modify cache",
        "Modify domain",
        "Modify autodeploy",
        "Cancel",
    ];

    let selected = ui::select("What would you like to do?", &options)?;

    Ok(match selected {
        0 => SourceSummaryAction::Confirm,
        1 => SourceSummaryAction::Modify(SourceModifiableField::AppName),
        2 => SourceSummaryAction::Modify(SourceModifiableField::Repository),
        3 => SourceSummaryAction::Modify(SourceModifiableField::SshKey),
        4 => SourceSummaryAction::Modify(SourceModifiableField::Stack),
        5 => SourceSummaryAction::Modify(SourceModifiableField::StackConfig),
        6 => SourceSummaryAction::Modify(SourceModifiableField::Port),
        7 => SourceSummaryAction::Modify(SourceModifiableField::Database),
        8 => SourceSummaryAction::Modify(SourceModifiableField::Cache),
        9 => SourceSummaryAction::Modify(SourceModifiableField::Domain),
        10 => SourceSummaryAction::Modify(SourceModifiableField::Autodeploy),
        _ => SourceSummaryAction::Cancel,
    })
}

/// Modifies a specific field in source deployment.
fn modify_source_field(
    data: &mut SourceInitData,
    field: SourceModifiableField,
    ctx: &ExecutionContext,
) -> Result<(), AppError> {
    match field {
        SourceModifiableField::AppName => {
            data.name = prompt_app_name()?;
        }
        SourceModifiableField::Repository => {
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
        SourceModifiableField::SshKey => {
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
        SourceModifiableField::Stack => {
            data.stack = prompt_stack()?;
            // Reset stack config when stack changes
            data.stack_config = prompt_stack_config(data.stack)?;
        }
        SourceModifiableField::StackConfig => {
            data.stack_config = prompt_stack_config(data.stack)?;
        }
        SourceModifiableField::Port => {
            data.port = prompt_port(data.stack)?;
        }
        SourceModifiableField::Database => {
            data.database = prompt_database()?;
        }
        SourceModifiableField::Cache => {
            data.cache = prompt_cache()?;
        }
        SourceModifiableField::Domain => {
            data.domain = prompt_domain()?;
        }
        SourceModifiableField::Autodeploy => {
            data.autodeploy = prompt_autodeploy()?;
        }
    }

    Ok(())
}

/// Creates the source app directories and configuration files.
fn create_source_app(data: &SourceInitData, ctx: &ExecutionContext) -> Result<(), AppError> {
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

    let config = AppConfig::new_source(
        data.name.clone(),
        data.repository.clone(),
        data.ssh_key.clone(),
        data.stack,
        data.stack_config.clone(),
        data.domain.clone(),
        data.port,
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

// ============================================================================
// Image Deployment Functions
// ============================================================================

/// Actions available after viewing image summary.
enum ImageSummaryAction {
    Confirm,
    Modify(ImageModifiableField),
    Cancel,
}

/// Collects image deployment configuration through interactive prompts.
fn collect_image_data() -> Result<ImageInitData, AppError> {
    // 1. App name
    let name = prompt_app_name()?;

    // 2. Docker image
    let (image, credentials) = prompt_docker_image()?;

    // 3. Port
    let detected_port = detect_default_port(&image.name);
    let port = prompt_image_port(detected_port)?;

    // 4. Volumes
    let volumes = prompt_volumes(&name)?;

    // 5. Database selection
    let database = prompt_database()?;

    // 6. Cache selection
    let cache = prompt_cache()?;

    // 7. Domain
    let domain = prompt_domain()?;

    // 8. Health check
    let health_check = prompt_health_check()?;

    Ok(ImageInitData {
        name,
        image,
        port,
        volumes,
        database,
        cache,
        domain,
        health_check,
        credentials,
    })
}

/// Prompts for Docker image configuration.
fn prompt_docker_image() -> Result<(ImageConfig, Option<RegistryCredentials>), AppError> {
    loop {
        let input = ui::input_with_placeholder("Docker image?", Some("nginx:latest"))?;

        match parse_image_reference(&input) {
            Ok(mut image) => {
                // Check if private registry
                let is_private = ui::confirm("Is this a private registry?", false)?;
                image.private = is_private;

                let credentials = if is_private {
                    let username = ui::input("Registry username?")?;
                    let password = ui::password("Registry password?")?;
                    Some(RegistryCredentials::new(&username, &password))
                } else {
                    None
                };

                return Ok((image, credentials));
            }
            Err(e) => {
                ui::error(&e.to_string());
                continue;
            }
        }
    }
}

/// Prompts for port with auto-detected default.
fn prompt_image_port(detected: Option<u16>) -> Result<u16, AppError> {
    let default_port = detected.unwrap_or(8080);
    let prompt = format!("Which port does the app expose? (detected: {})", default_port);

    loop {
        let input = ui::input_with_default(&prompt, &default_port.to_string())?;
        match input.parse::<u16>() {
            Ok(port) if port > 0 => return Ok(port),
            _ => {
                ui::error("Please enter a valid port number (1-65535)");
                continue;
            }
        }
    }
}

/// Prompts for volume configuration.
fn prompt_volumes(app_name: &str) -> Result<Vec<VolumeMount>, AppError> {
    let mut volumes = Vec::new();

    if !ui::confirm("Do you need persistent volumes?", false)? {
        return Ok(volumes);
    }

    loop {
        let container_path = ui::input_with_placeholder(
            "Container path for volume?",
            Some("/var/lib/data"),
        )?;

        if container_path.is_empty() {
            break;
        }

        // Generate volume name from app name and path
        let path_suffix = container_path
            .trim_start_matches('/')
            .replace('/', "-")
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '-')
            .collect::<String>();
        let volume_name = format!("{}-{}", app_name, path_suffix);

        volumes.push(VolumeMount::new(&container_path, &volume_name));

        if !ui::confirm("Add another volume?", false)? {
            break;
        }
    }

    Ok(volumes)
}

/// Prompts for health check configuration.
fn prompt_health_check() -> Result<HealthCheckConfig, AppError> {
    let endpoint = ui::input_with_default("Health check endpoint?", "/")?;
    let timeout = ui::input_with_default("Health check timeout (seconds)?", "30")?
        .parse::<u32>()
        .unwrap_or(30);

    Ok(HealthCheckConfig {
        endpoint,
        timeout,
        interval: 5,
        retries: 3,
    })
}

/// Displays the image deployment configuration summary.
fn display_image_summary(data: &ImageInitData) {
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
    let volumes_str = if data.volumes.is_empty() {
        "None".to_string()
    } else {
        data.volumes
            .iter()
            .map(|v| v.container_path.clone())
            .collect::<Vec<_>>()
            .join(", ")
    };
    let private_str = if data.image.private { " (private)" } else { "" };

    println!("  Configuration Summary (Image Deployment)");
    println!("  {}", "-".repeat(50));
    println!();
    println!("  App name:     {}", data.name);
    println!("  Image:        {}{}", data.image.full_reference(), private_str);
    println!("  Registry:     {}", data.image.registry.display_name());
    println!("  Port:         {}", data.port);
    println!("  Volumes:      {}", volumes_str);
    println!("  Database:     {}", db_str);
    println!("  Cache:        {}", cache_str);
    println!("  Domain:       {}", data.domain);
    println!("  Health check: {} ({}s timeout)", data.health_check.endpoint, data.health_check.timeout);
    println!();
}

/// Prompts for action after viewing image summary.
fn prompt_image_summary_action() -> Result<ImageSummaryAction, AppError> {
    let options = [
        "Confirm and create",
        "Modify app name",
        "Modify image",
        "Modify port",
        "Modify volumes",
        "Modify database",
        "Modify cache",
        "Modify domain",
        "Modify health check",
        "Cancel",
    ];

    let selected = ui::select("What would you like to do?", &options)?;

    Ok(match selected {
        0 => ImageSummaryAction::Confirm,
        1 => ImageSummaryAction::Modify(ImageModifiableField::AppName),
        2 => ImageSummaryAction::Modify(ImageModifiableField::Image),
        3 => ImageSummaryAction::Modify(ImageModifiableField::Port),
        4 => ImageSummaryAction::Modify(ImageModifiableField::Volumes),
        5 => ImageSummaryAction::Modify(ImageModifiableField::Database),
        6 => ImageSummaryAction::Modify(ImageModifiableField::Cache),
        7 => ImageSummaryAction::Modify(ImageModifiableField::Domain),
        8 => ImageSummaryAction::Modify(ImageModifiableField::HealthCheck),
        _ => ImageSummaryAction::Cancel,
    })
}

/// Modifies a specific field in image deployment.
fn modify_image_field(
    data: &mut ImageInitData,
    field: ImageModifiableField,
) -> Result<(), AppError> {
    match field {
        ImageModifiableField::AppName => {
            data.name = prompt_app_name()?;
        }
        ImageModifiableField::Image => {
            let (image, credentials) = prompt_docker_image()?;
            data.image = image;
            data.credentials = credentials;
        }
        ImageModifiableField::Port => {
            let detected = detect_default_port(&data.image.name);
            data.port = prompt_image_port(detected)?;
        }
        ImageModifiableField::Volumes => {
            data.volumes = prompt_volumes(&data.name)?;
        }
        ImageModifiableField::Database => {
            data.database = prompt_database()?;
        }
        ImageModifiableField::Cache => {
            data.cache = prompt_cache()?;
        }
        ImageModifiableField::Domain => {
            data.domain = prompt_domain()?;
        }
        ImageModifiableField::HealthCheck => {
            data.health_check = prompt_health_check()?;
        }
    }

    Ok(())
}

/// Creates the image app directories and configuration files.
fn create_image_app(data: &ImageInitData, ctx: &ExecutionContext) -> Result<(), AppError> {
    ui::info("Creating app configuration...");

    // Create app directory structure
    let app_dir = format!("{}/{}", FLAASE_APPS_PATH, data.name);
    ctx.create_dir(&app_dir)?;
    ctx.create_dir(&format!("{}/data", app_dir))?;

    // Build app config
    let database_config = data
        .database
        .map(|db_type| DatabaseConfig::new(db_type, &data.name));

    let cache_config = data.cache.map(CacheConfig::new);

    let config = AppConfig::new_image(
        data.name.clone(),
        data.image.clone(),
        data.domain.clone(),
        data.port,
        data.volumes.clone(),
        database_config.clone(),
        cache_config.clone(),
        Some(data.health_check.clone()),
    );

    // Save config.yml
    config.save()?;

    // Save registry credentials if provided
    if let Some(creds) = &data.credentials {
        save_credentials(&config.registry_auth_path(), creds)?;
    }

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
