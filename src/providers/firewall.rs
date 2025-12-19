use crate::core::context::ExecutionContext;
use crate::core::error::AppError;
use crate::providers::package_manager::PackageManager;

/// Trait for firewall operations.
/// Allows for different implementations (ufw, iptables, firewalld, etc.).
pub trait Firewall {
    /// Returns the name of the firewall.
    fn name(&self) -> &str;

    /// Checks if the firewall is installed.
    fn is_installed(&self, ctx: &ExecutionContext) -> Result<bool, AppError>;

    /// Installs the firewall.
    fn install(
        &self,
        pkg_manager: &dyn PackageManager,
        ctx: &ExecutionContext,
    ) -> Result<(), AppError>;

    /// Enables the firewall.
    fn enable(&self, ctx: &ExecutionContext) -> Result<(), AppError>;

    /// Disables the firewall.
    fn disable(&self, ctx: &ExecutionContext) -> Result<(), AppError>;

    /// Checks if the firewall is enabled.
    fn is_enabled(&self, ctx: &ExecutionContext) -> Result<bool, AppError>;

    /// Allows incoming traffic on a port.
    fn allow_port(
        &self,
        port: u16,
        protocol: Protocol,
        ctx: &ExecutionContext,
    ) -> Result<(), AppError>;

    /// Allows incoming traffic on a port range.
    fn allow_port_range(
        &self,
        start: u16,
        end: u16,
        protocol: Protocol,
        ctx: &ExecutionContext,
    ) -> Result<(), AppError>;

    /// Denies incoming traffic on a port.
    fn deny_port(
        &self,
        port: u16,
        protocol: Protocol,
        ctx: &ExecutionContext,
    ) -> Result<(), AppError>;

    /// Sets the default incoming policy.
    fn set_default_incoming(
        &self,
        policy: FirewallPolicy,
        ctx: &ExecutionContext,
    ) -> Result<(), AppError>;

    /// Sets the default outgoing policy.
    fn set_default_outgoing(
        &self,
        policy: FirewallPolicy,
        ctx: &ExecutionContext,
    ) -> Result<(), AppError>;

    /// Gets the firewall status.
    fn status(&self, ctx: &ExecutionContext) -> Result<FirewallStatus, AppError>;

    /// Reloads the firewall rules.
    fn reload(&self, ctx: &ExecutionContext) -> Result<(), AppError>;
}

/// Network protocol for firewall rules.
#[derive(Debug, Clone, Copy)]
pub enum Protocol {
    Tcp,
    Udp,
    Both,
}

impl Protocol {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Tcp => "tcp",
            Self::Udp => "udp",
            Self::Both => "any",
        }
    }
}

/// Firewall policy for default rules.
#[derive(Debug, Clone, Copy)]
pub enum FirewallPolicy {
    Allow,
    Deny,
    Reject,
}

impl FirewallPolicy {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Allow => "allow",
            Self::Deny => "deny",
            Self::Reject => "reject",
        }
    }
}

/// Firewall status information.
#[derive(Debug, Clone)]
pub struct FirewallStatus {
    pub enabled: bool,
    pub rules: Vec<String>,
}

/// UFW (Uncomplicated Firewall) implementation.
pub struct UfwFirewall;

impl UfwFirewall {
    pub fn new() -> Self {
        Self
    }
}

impl Default for UfwFirewall {
    fn default() -> Self {
        Self::new()
    }
}

impl Firewall for UfwFirewall {
    fn name(&self) -> &str {
        "ufw"
    }

    fn is_installed(&self, ctx: &ExecutionContext) -> Result<bool, AppError> {
        match ctx.run_command("which", &["ufw"]) {
            Ok(output) => Ok(output.success),
            Err(_) => Ok(false),
        }
    }

    fn install(
        &self,
        pkg_manager: &dyn PackageManager,
        ctx: &ExecutionContext,
    ) -> Result<(), AppError> {
        pkg_manager.install(&["ufw"], ctx)
    }

    fn enable(&self, ctx: &ExecutionContext) -> Result<(), AppError> {
        // --force to avoid interactive prompt
        ctx.run_command("ufw", &["--force", "enable"])?
            .ensure_success("Failed to enable firewall")?;
        Ok(())
    }

    fn disable(&self, ctx: &ExecutionContext) -> Result<(), AppError> {
        ctx.run_command("ufw", &["disable"])?
            .ensure_success("Failed to disable firewall")?;
        Ok(())
    }

    fn is_enabled(&self, ctx: &ExecutionContext) -> Result<bool, AppError> {
        let output = ctx.run_command("ufw", &["status"])?;
        Ok(output.stdout.contains("Status: active"))
    }

    fn allow_port(
        &self,
        port: u16,
        protocol: Protocol,
        ctx: &ExecutionContext,
    ) -> Result<(), AppError> {
        let port_spec = match protocol {
            Protocol::Both => format!("{}", port),
            _ => format!("{}/{}", port, protocol.as_str()),
        };

        ctx.run_command("ufw", &["allow", &port_spec])?
            .ensure_success(&format!("Failed to allow port {}", port))?;
        Ok(())
    }

    fn allow_port_range(
        &self,
        start: u16,
        end: u16,
        protocol: Protocol,
        ctx: &ExecutionContext,
    ) -> Result<(), AppError> {
        let range_spec = format!("{}:{}/{}", start, end, protocol.as_str());

        ctx.run_command("ufw", &["allow", &range_spec])?
            .ensure_success(&format!("Failed to allow port range {}-{}", start, end))?;
        Ok(())
    }

    fn deny_port(
        &self,
        port: u16,
        protocol: Protocol,
        ctx: &ExecutionContext,
    ) -> Result<(), AppError> {
        let port_spec = match protocol {
            Protocol::Both => format!("{}", port),
            _ => format!("{}/{}", port, protocol.as_str()),
        };

        ctx.run_command("ufw", &["deny", &port_spec])?
            .ensure_success(&format!("Failed to deny port {}", port))?;
        Ok(())
    }

    fn set_default_incoming(
        &self,
        policy: FirewallPolicy,
        ctx: &ExecutionContext,
    ) -> Result<(), AppError> {
        ctx.run_command("ufw", &["default", policy.as_str(), "incoming"])?
            .ensure_success("Failed to set default incoming policy")?;
        Ok(())
    }

    fn set_default_outgoing(
        &self,
        policy: FirewallPolicy,
        ctx: &ExecutionContext,
    ) -> Result<(), AppError> {
        ctx.run_command("ufw", &["default", policy.as_str(), "outgoing"])?
            .ensure_success("Failed to set default outgoing policy")?;
        Ok(())
    }

    fn status(&self, ctx: &ExecutionContext) -> Result<FirewallStatus, AppError> {
        let output = ctx.run_command("ufw", &["status", "verbose"])?;

        let enabled = output.stdout.contains("Status: active");
        let rules: Vec<String> = output
            .stdout
            .lines()
            .skip_while(|l| !l.contains("--"))
            .skip(1)
            .filter(|l| !l.is_empty())
            .map(|l| l.to_string())
            .collect();

        Ok(FirewallStatus { enabled, rules })
    }

    fn reload(&self, ctx: &ExecutionContext) -> Result<(), AppError> {
        ctx.run_command("ufw", &["reload"])?
            .ensure_success("Failed to reload firewall")?;
        Ok(())
    }
}

/// Creates the appropriate firewall for the current system.
pub fn create_firewall() -> Box<dyn Firewall> {
    Box::new(UfwFirewall::new())
}

/// Required ports for Flaase.
pub struct RequiredPorts;

impl RequiredPorts {
    /// SSH port.
    pub const SSH: u16 = 22;
    /// HTTP port.
    pub const HTTP: u16 = 80;
    /// HTTPS port.
    pub const HTTPS: u16 = 443;

    /// All required ports.
    pub fn all() -> &'static [u16] {
        &[Self::SSH, Self::HTTP, Self::HTTPS]
    }
}
