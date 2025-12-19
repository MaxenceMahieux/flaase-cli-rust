use crate::core::config::{ExistingComponentAction, ServerConfig, FLAASE_BASE_PATH};
use crate::core::context::ExecutionContext;
use crate::core::error::AppError;
use crate::providers::{
    create_container_runtime, create_firewall, create_package_manager, create_reverse_proxy,
    ContainerRuntime, Firewall, PackageManager, Protocol, RequiredPorts, ReverseProxy,
    SystemProvider, UserManager,
};
use crate::ui;

/// Executes the server init command.
pub fn init(dry_run: bool, verbose: bool) -> Result<(), AppError> {
    ui::header();

    // Create execution context
    let ctx = ExecutionContext::new(dry_run, verbose);

    if dry_run {
        ui::warning("Running in dry-run mode. No changes will be made.");
        println!();
    }

    // Step 1: Check root privileges
    ui::info("Checking root privileges...");
    if dry_run {
        ui::info("[DRY-RUN] Skipping root check");
    } else {
        SystemProvider::require_root()?;
    }
    ui::success("Running as root");

    // Step 2: Detect and validate OS
    ui::info("Detecting operating system...");
    let os_info = SystemProvider::detect_os()?;
    ui::success(&format!("Detected: {}", os_info.name));

    SystemProvider::validate_os(&os_info)?;
    ui::success("Operating system is supported");
    println!();

    // Initialize providers
    let pkg_manager = create_package_manager();
    let container_runtime = create_container_runtime();
    let firewall = create_firewall();
    let reverse_proxy = create_reverse_proxy();

    // Step 3: Install container runtime (Docker)
    install_container_runtime(&*container_runtime, &*pkg_manager, &ctx)?;

    // Step 4: Configure firewall
    configure_firewall(&*firewall, &*pkg_manager, &ctx)?;

    // Step 5: Create directories
    create_directories(&ctx)?;

    // Step 6: Create deploy user
    let user_info = create_deploy_user(&ctx)?;

    // Step 7: Get email for SSL
    println!();
    ui::info("Email is required for SSL certificate notifications (Let's Encrypt).");
    let email = ui::input("Email for SSL certificates")?;

    if email.is_empty() {
        return Err(AppError::Config("Email is required".into()));
    }

    // Step 8: Install reverse proxy (Traefik)
    install_reverse_proxy(&*reverse_proxy, &*container_runtime, &email, &ctx)?;

    // Step 9: Save configuration
    println!();
    ui::info("Saving server configuration...");

    let runtime_info = container_runtime.get_info(&ctx)?;
    let proxy_info = reverse_proxy.get_info(&*container_runtime, &ctx)?;

    let config = ServerConfig::new(email, os_info, runtime_info, proxy_info, user_info.into());

    if !ctx.is_dry_run() {
        config.save()?;
    } else {
        ui::info("[DRY-RUN] Would save configuration");
    }

    ui::success("Server configuration saved");

    // Done!
    println!();
    ui::success("Server initialization complete!");
    ui::info("You can now configure apps with: fl init");

    Ok(())
}

/// Installs the container runtime with idempotency.
fn install_container_runtime(
    runtime: &dyn ContainerRuntime,
    pkg_manager: &dyn PackageManager,
    ctx: &ExecutionContext,
) -> Result<(), AppError> {
    ui::info(&format!("Checking {}...", runtime.name()));

    let is_installed = runtime.is_installed(ctx)?;

    if is_installed {
        let version = runtime
            .get_version(ctx)
            .unwrap_or_else(|_| "unknown".to_string());
        ui::success(&format!(
            "{} {} is already installed",
            runtime.name(),
            version
        ));

        // Ask what to do
        let action = ask_existing_action(runtime.name())?;

        match action {
            ExistingComponentAction::Skip => {
                ui::info(&format!("Skipping {} installation", runtime.name()));
            }
            ExistingComponentAction::Update => {
                ui::info(&format!("Updating {}...", runtime.name()));
                pkg_manager.update(ctx)?;
                // Docker updates through package manager
                runtime.install(pkg_manager, ctx)?;
                ui::success(&format!("{} updated", runtime.name()));
            }
            ExistingComponentAction::Reinstall => {
                ui::info(&format!("Reinstalling {}...", runtime.name()));
                runtime.install(pkg_manager, ctx)?;
                ui::success(&format!("{} reinstalled", runtime.name()));
            }
        }
    } else {
        ui::info(&format!("Installing {}...", runtime.name()));
        runtime.install(pkg_manager, ctx)?;
        ui::success(&format!("{} installed", runtime.name()));
    }

    // Ensure service is running
    if !runtime.is_running(ctx)? {
        ui::info(&format!("Starting {} service...", runtime.name()));
        runtime.start_service(ctx)?;
    }

    // Enable on boot
    runtime.enable_service(ctx)?;
    ui::success(&format!(
        "{} service is running and enabled",
        runtime.name()
    ));
    println!();

    Ok(())
}

/// Configures the firewall with idempotency.
fn configure_firewall(
    firewall: &dyn Firewall,
    pkg_manager: &dyn PackageManager,
    ctx: &ExecutionContext,
) -> Result<(), AppError> {
    ui::info(&format!("Checking {} firewall...", firewall.name()));

    let is_installed = firewall.is_installed(ctx)?;

    if !is_installed {
        ui::info(&format!("Installing {}...", firewall.name()));
        firewall.install(pkg_manager, ctx)?;
        ui::success(&format!("{} installed", firewall.name()));
    } else {
        ui::success(&format!("{} is already installed", firewall.name()));
    }

    // Configure required ports
    ui::info("Configuring firewall rules...");

    for port in RequiredPorts::all() {
        firewall.allow_port(*port, Protocol::Tcp, ctx)?;
        if ctx.is_verbose() {
            ui::info(&format!("Allowed port {}/tcp", port));
        }
    }

    ui::success(&format!(
        "Allowed ports: {}",
        RequiredPorts::all()
            .iter()
            .map(|p| p.to_string())
            .collect::<Vec<_>>()
            .join(", ")
    ));

    // Enable firewall if not already
    if !firewall.is_enabled(ctx)? {
        ui::info("Enabling firewall...");
        firewall.enable(ctx)?;
    }

    ui::success("Firewall configured and enabled");
    println!();

    Ok(())
}

/// Creates the required directories.
fn create_directories(ctx: &ExecutionContext) -> Result<(), AppError> {
    ui::info("Creating directories...");

    for dir in ServerConfig::required_directories() {
        ctx.create_dir(dir)?;
    }

    ui::success(&format!("Created {}", FLAASE_BASE_PATH));
    println!();

    Ok(())
}

/// Creates the deploy user.
fn create_deploy_user(ctx: &ExecutionContext) -> Result<crate::providers::UserInfo, AppError> {
    ui::info(&format!(
        "Setting up '{}' user...",
        UserManager::DEPLOY_USER
    ));

    let user_info = UserManager::create_deploy_user(ctx)?;

    // Setup SSH directory
    let ssh_dir = format!("/home/{}/.ssh", user_info.username);
    ctx.create_dir(&ssh_dir)?;

    // Set proper ownership
    ctx.run_command(
        "chown",
        &[
            "-R",
            &format!("{}:{}", user_info.username, user_info.username),
            &ssh_dir,
        ],
    )?;
    ctx.run_command("chmod", &["700", &ssh_dir])?;

    ui::success(&format!(
        "User '{}' created (uid: {}, gid: {})",
        user_info.username, user_info.uid, user_info.gid
    ));
    println!();

    Ok(user_info)
}

/// Installs the reverse proxy with idempotency.
fn install_reverse_proxy(
    proxy: &dyn ReverseProxy,
    runtime: &dyn ContainerRuntime,
    email: &str,
    ctx: &ExecutionContext,
) -> Result<(), AppError> {
    ui::info(&format!("Checking {}...", proxy.name()));

    let is_installed = proxy.is_installed(runtime, ctx)?;

    if is_installed {
        let is_running = proxy.is_running(runtime, ctx)?;

        if is_running {
            let version = proxy
                .get_version(runtime, ctx)
                .unwrap_or_else(|_| "unknown".to_string());
            ui::success(&format!("{} {} is already running", proxy.name(), version));

            let action = ask_existing_action(proxy.name())?;

            match action {
                ExistingComponentAction::Skip => {
                    ui::info(&format!("Skipping {} installation", proxy.name()));
                    return Ok(());
                }
                ExistingComponentAction::Update | ExistingComponentAction::Reinstall => {
                    ui::info(&format!("Reinstalling {}...", proxy.name()));
                    proxy.install(runtime, email, ctx)?;
                    ui::success(&format!("{} reinstalled", proxy.name()));
                }
            }
        } else {
            ui::warning(&format!(
                "{} container exists but is not running",
                proxy.name()
            ));
            ui::info(&format!("Starting {}...", proxy.name()));
            proxy.install(runtime, email, ctx)?;
            ui::success(&format!("{} started", proxy.name()));
        }
    } else {
        ui::info(&format!("Installing {}...", proxy.name()));
        proxy.install(runtime, email, ctx)?;
        ui::success(&format!("{} installed and running", proxy.name()));
    }

    println!();
    Ok(())
}

/// Asks the user what to do with an existing component.
fn ask_existing_action(component_name: &str) -> Result<ExistingComponentAction, AppError> {
    let options = ["Skip (keep existing)", "Update", "Reinstall"];
    let selected = ui::select(
        &format!(
            "{} is already installed. What would you like to do?",
            component_name
        ),
        &options,
    )?;

    Ok(match selected {
        0 => ExistingComponentAction::Skip,
        1 => ExistingComponentAction::Update,
        2 => ExistingComponentAction::Reinstall,
        _ => ExistingComponentAction::Skip,
    })
}
