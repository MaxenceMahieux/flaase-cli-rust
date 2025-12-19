use std::collections::HashMap;
use std::path::Path;

use crate::core::config::OsInfo;
use crate::core::context::ExecutionContext;
use crate::core::error::AppError;

/// Supported operating systems with their versions.
const SUPPORTED_OS: &[(&str, &[&str])] =
    &[("ubuntu", &["22.04", "24.04"]), ("debian", &["11", "12"])];

/// System provider for OS detection, user management, and privilege checks.
pub struct SystemProvider;

impl SystemProvider {
    /// Checks if the current process is running as root.
    pub fn is_root() -> bool {
        unsafe { libc::geteuid() == 0 }
    }

    /// Returns an error if not running as root.
    pub fn require_root() -> Result<(), AppError> {
        if Self::is_root() {
            Ok(())
        } else {
            Err(AppError::RequiresRoot)
        }
    }

    /// Detects the current operating system.
    pub fn detect_os() -> Result<OsInfo, AppError> {
        let os_release = Self::parse_os_release()?;

        let id = os_release
            .get("ID")
            .map(|s| s.to_lowercase())
            .ok_or_else(|| AppError::UnsupportedOs("Could not detect OS ID".into()))?;

        let version = os_release
            .get("VERSION_ID")
            .cloned()
            .ok_or_else(|| AppError::UnsupportedOs("Could not detect OS version".into()))?;

        let codename = os_release.get("VERSION_CODENAME").cloned();

        let name = os_release
            .get("PRETTY_NAME")
            .cloned()
            .unwrap_or_else(|| format!("{} {}", id, version));

        Ok(OsInfo {
            name,
            version,
            codename,
        })
    }

    /// Validates that the detected OS is supported.
    pub fn validate_os(os: &OsInfo) -> Result<(), AppError> {
        let os_id = os.name.to_lowercase();

        for (supported_id, supported_versions) in SUPPORTED_OS {
            if os_id.contains(supported_id) {
                // Check if version is supported
                for supported_version in *supported_versions {
                    if os.version.starts_with(supported_version) {
                        return Ok(());
                    }
                }
                return Err(AppError::UnsupportedOs(format!(
                    "{} version {} is not supported. Supported versions: {}",
                    supported_id,
                    os.version,
                    supported_versions.join(", ")
                )));
            }
        }

        Err(AppError::UnsupportedOs(format!(
            "'{}' is not supported. Supported: Ubuntu 22.04/24.04, Debian 11/12",
            os.name
        )))
    }

    /// Parses /etc/os-release into a key-value map.
    fn parse_os_release() -> Result<HashMap<String, String>, AppError> {
        let path = Path::new("/etc/os-release");

        if !path.exists() {
            return Err(AppError::UnsupportedOs(
                "Could not find /etc/os-release".into(),
            ));
        }

        let content = std::fs::read_to_string(path).map_err(|e| {
            AppError::UnsupportedOs(format!("Failed to read /etc/os-release: {}", e))
        })?;

        let mut map = HashMap::new();

        for line in content.lines() {
            if let Some((key, value)) = line.split_once('=') {
                let value = value.trim_matches('"').to_string();
                map.insert(key.to_string(), value);
            }
        }

        Ok(map)
    }
}

/// User management for creating the deploy user.
pub struct UserManager;

impl UserManager {
    /// Default deploy username.
    pub const DEPLOY_USER: &'static str = "deploy";

    /// Checks if a user exists.
    pub fn user_exists(username: &str, ctx: &ExecutionContext) -> Result<bool, AppError> {
        let output = ctx.run_command("id", &[username])?;
        Ok(output.success)
    }

    /// Creates the deploy user with Docker group access.
    pub fn create_deploy_user(ctx: &ExecutionContext) -> Result<UserInfo, AppError> {
        let username = Self::DEPLOY_USER;

        // Check if user already exists
        if Self::user_exists(username, ctx)? {
            return Self::get_user_info(username, ctx);
        }

        // Create user with home directory
        ctx.run_command(
            "useradd",
            &[
                "--create-home",
                "--shell",
                "/bin/bash",
                "--groups",
                "docker",
                username,
            ],
        )?
        .ensure_success("Failed to create deploy user")?;

        // Disable password login (SSH key only)
        ctx.run_command("passwd", &["--delete", username])?
            .ensure_success("Failed to configure deploy user")?;

        Self::get_user_info(username, ctx)
    }

    /// Adds the deploy user to the docker group.
    pub fn add_to_docker_group(username: &str, ctx: &ExecutionContext) -> Result<(), AppError> {
        ctx.run_command("usermod", &["--append", "--groups", "docker", username])?
            .ensure_success("Failed to add user to docker group")
    }

    /// Gets information about a user.
    pub fn get_user_info(username: &str, ctx: &ExecutionContext) -> Result<UserInfo, AppError> {
        let output = ctx.run_command("id", &["-u", username])?;
        output.ensure_success("Failed to get user ID")?;

        let uid: u32 = output
            .stdout
            .trim()
            .parse()
            .map_err(|_| AppError::UserManagement("Invalid UID".into()))?;

        let output = ctx.run_command("id", &["-g", username])?;
        output.ensure_success("Failed to get group ID")?;

        let gid: u32 = output
            .stdout
            .trim()
            .parse()
            .map_err(|_| AppError::UserManagement("Invalid GID".into()))?;

        Ok(UserInfo {
            username: username.to_string(),
            uid,
            gid,
        })
    }
}

/// Information about a system user.
#[derive(Debug, Clone)]
pub struct UserInfo {
    pub username: String,
    pub uid: u32,
    pub gid: u32,
}

impl From<UserInfo> for crate::core::config::DeployUserInfo {
    fn from(info: UserInfo) -> Self {
        Self {
            username: info.username,
            uid: info.uid,
            gid: info.gid,
        }
    }
}
