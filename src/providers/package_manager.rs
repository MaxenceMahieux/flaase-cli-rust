use crate::core::context::ExecutionContext;
use crate::core::error::AppError;

/// Trait for package manager operations.
/// Allows for different implementations (apt, yum, dnf, etc.).
pub trait PackageManager {
    /// Returns the name of the package manager.
    fn name(&self) -> &str;

    /// Updates the package list.
    fn update(&self, ctx: &ExecutionContext) -> Result<(), AppError>;

    /// Installs one or more packages.
    fn install(&self, packages: &[&str], ctx: &ExecutionContext) -> Result<(), AppError>;

    /// Checks if a package is installed.
    fn is_installed(&self, package: &str, ctx: &ExecutionContext) -> Result<bool, AppError>;

    /// Removes one or more packages.
    fn remove(&self, packages: &[&str], ctx: &ExecutionContext) -> Result<(), AppError>;
}

/// APT package manager implementation for Debian/Ubuntu.
pub struct AptManager;

impl AptManager {
    pub fn new() -> Self {
        Self
    }
}

impl Default for AptManager {
    fn default() -> Self {
        Self::new()
    }
}

impl PackageManager for AptManager {
    fn name(&self) -> &str {
        "apt"
    }

    fn update(&self, ctx: &ExecutionContext) -> Result<(), AppError> {
        // Set non-interactive mode
        std::env::set_var("DEBIAN_FRONTEND", "noninteractive");

        ctx.run_command_streaming("apt-get", &["update"])?
            .ensure_success("Failed to update package list")?;

        Ok(())
    }

    fn install(&self, packages: &[&str], ctx: &ExecutionContext) -> Result<(), AppError> {
        if packages.is_empty() {
            return Ok(());
        }

        // Set non-interactive mode
        std::env::set_var("DEBIAN_FRONTEND", "noninteractive");

        let mut args = vec!["install", "-y", "--no-install-recommends"];
        args.extend(packages);

        ctx.run_command_streaming("apt-get", &args)?
            .ensure_success(&format!(
                "Failed to install packages: {}",
                packages.join(", ")
            ))?;

        Ok(())
    }

    fn is_installed(&self, package: &str, ctx: &ExecutionContext) -> Result<bool, AppError> {
        let output = ctx.run_command("dpkg", &["-s", package])?;
        Ok(output.success && output.stdout.contains("Status: install ok installed"))
    }

    fn remove(&self, packages: &[&str], ctx: &ExecutionContext) -> Result<(), AppError> {
        if packages.is_empty() {
            return Ok(());
        }

        let mut args = vec!["remove", "-y"];
        args.extend(packages);

        ctx.run_command_streaming("apt-get", &args)?
            .ensure_success(&format!(
                "Failed to remove packages: {}",
                packages.join(", ")
            ))?;

        Ok(())
    }
}

/// Creates the appropriate package manager for the current OS.
pub fn create_package_manager() -> Box<dyn PackageManager> {
    // For now, we only support apt-based systems
    // In the future, we could detect the OS and return the appropriate manager
    Box::new(AptManager::new())
}
