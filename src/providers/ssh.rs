//! SSH key management provider.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use crate::core::context::ExecutionContext;
use crate::core::error::AppError;

/// SSH key type for generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SshKeyType {
    Ed25519,
    Rsa4096,
    Ecdsa,
}

impl SshKeyType {
    /// Returns all available key types.
    pub fn all() -> &'static [SshKeyType] {
        &[SshKeyType::Ed25519, SshKeyType::Rsa4096, SshKeyType::Ecdsa]
    }

    /// Returns the display name.
    pub fn display_name(&self) -> &str {
        match self {
            SshKeyType::Ed25519 => "Ed25519 (recommended)",
            SshKeyType::Rsa4096 => "RSA 4096-bit",
            SshKeyType::Ecdsa => "ECDSA",
        }
    }

    /// Returns the ssh-keygen type argument.
    pub fn keygen_type(&self) -> &str {
        match self {
            SshKeyType::Ed25519 => "ed25519",
            SshKeyType::Rsa4096 => "rsa",
            SshKeyType::Ecdsa => "ecdsa",
        }
    }

    /// Returns additional ssh-keygen arguments.
    pub fn keygen_args(&self) -> Vec<&str> {
        match self {
            SshKeyType::Rsa4096 => vec!["-b", "4096"],
            _ => vec![],
        }
    }
}

/// SSH key information.
#[derive(Debug, Clone)]
pub struct SshKeyInfo {
    pub path: PathBuf,
    pub key_type: String,
    pub comment: Option<String>,
}

impl SshKeyInfo {
    /// Returns the display string for the key.
    pub fn display(&self) -> String {
        let path_str = self.path.display();
        if let Some(comment) = &self.comment {
            format!("{} ({})", path_str, comment)
        } else {
            format!("{} ({})", path_str, self.key_type)
        }
    }
}

/// SSH provider for key management and connection testing.
pub struct SshProvider;

impl SshProvider {
    /// Default SSH directory.
    fn ssh_dir() -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
        PathBuf::from(home).join(".ssh")
    }

    /// Lists available SSH private keys.
    pub fn list_keys() -> Result<Vec<SshKeyInfo>, AppError> {
        let ssh_dir = Self::ssh_dir();

        if !ssh_dir.exists() {
            return Ok(Vec::new());
        }

        let mut keys = Vec::new();

        let entries = fs::read_dir(&ssh_dir)
            .map_err(|e| AppError::Ssh(format!("Failed to read SSH directory: {}", e)))?;

        for entry in entries {
            let entry = entry.map_err(|e| AppError::Ssh(format!("Failed to read entry: {}", e)))?;
            let path = entry.path();

            // Skip directories and public keys
            if path.is_dir() {
                continue;
            }

            if let Some(ext) = path.extension() {
                if ext == "pub" {
                    continue;
                }
            }

            // Skip known_hosts, config, etc.
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name == "known_hosts"
                    || name == "authorized_keys"
                    || name == "config"
                    || name.starts_with("known_hosts")
                {
                    continue;
                }
            }

            // Check if it's a private key by reading first line
            if let Ok(content) = fs::read_to_string(&path) {
                if content.starts_with("-----BEGIN") && content.contains("PRIVATE KEY") {
                    let key_type = Self::detect_key_type(&content);
                    let comment = Self::read_key_comment(&path);

                    keys.push(SshKeyInfo {
                        path,
                        key_type,
                        comment,
                    });
                }
            }
        }

        // Sort by path
        keys.sort_by(|a, b| a.path.cmp(&b.path));

        Ok(keys)
    }

    /// Detects the key type from the private key content.
    fn detect_key_type(content: &str) -> String {
        if content.contains("OPENSSH PRIVATE KEY") {
            // Modern OpenSSH format - could be ed25519, ecdsa, or rsa
            if content.len() < 1000 {
                "ed25519".to_string()
            } else {
                "openssh".to_string()
            }
        } else if content.contains("RSA PRIVATE KEY") {
            "rsa".to_string()
        } else if content.contains("EC PRIVATE KEY") {
            "ecdsa".to_string()
        } else if content.contains("DSA PRIVATE KEY") {
            "dsa".to_string()
        } else {
            "unknown".to_string()
        }
    }

    /// Reads the comment from the public key file.
    fn read_key_comment(private_key_path: &Path) -> Option<String> {
        let pub_path = private_key_path.with_extension("pub");

        if let Ok(content) = fs::read_to_string(&pub_path) {
            // Public key format: type base64 comment
            let parts: Vec<&str> = content.split_whitespace().collect();
            if parts.len() >= 3 {
                return Some(parts[2..].join(" "));
            }
        }

        None
    }

    /// Generates a new SSH key pair.
    pub fn generate_key(
        key_type: SshKeyType,
        filename: &str,
        comment: Option<&str>,
        ctx: &ExecutionContext,
    ) -> Result<PathBuf, AppError> {
        let ssh_dir = Self::ssh_dir();

        // Ensure .ssh directory exists
        if !ssh_dir.exists() {
            ctx.create_dir(ssh_dir.to_str().unwrap())?;
            // Set permissions to 700
            fs::set_permissions(&ssh_dir, fs::Permissions::from_mode(0o700)).map_err(|e| {
                AppError::Ssh(format!("Failed to set SSH directory permissions: {}", e))
            })?;
        }

        let key_path = ssh_dir.join(filename);

        // Check if key already exists
        if key_path.exists() {
            return Err(AppError::Ssh(format!(
                "Key already exists: {}",
                key_path.display()
            )));
        }

        // Build ssh-keygen command
        let mut args = vec![
            "-t",
            key_type.keygen_type(),
            "-f",
            key_path.to_str().unwrap(),
            "-N",
            "", // Empty passphrase
        ];

        // Add type-specific arguments
        args.extend(key_type.keygen_args());

        // Add comment if provided
        if let Some(c) = comment {
            args.push("-C");
            args.push(c);
        }

        let output = ctx.run_command("ssh-keygen", &args)?;
        output.ensure_success("Failed to generate SSH key")?;

        Ok(key_path)
    }

    /// Tests SSH connection to a Git repository.
    pub fn test_git_connection(
        repo_url: &str,
        key_path: &Path,
        ctx: &ExecutionContext,
    ) -> Result<bool, AppError> {
        // Extract host from SSH URL (git@github.com:user/repo.git -> github.com)
        let host = Self::extract_host(repo_url)?;

        // Use ssh to test connection with specific key
        let output = ctx.run_command(
            "ssh",
            &[
                "-i",
                key_path.to_str().unwrap(),
                "-o",
                "StrictHostKeyChecking=accept-new",
                "-o",
                "BatchMode=yes",
                "-o",
                "ConnectTimeout=10",
                "-T",
                &format!("git@{}", host),
            ],
        )?;

        // GitHub/GitLab return exit code 1 but with successful message
        // Check stderr for success indicators
        let success = output.success
            || output.stderr.contains("successfully authenticated")
            || output.stderr.contains("Welcome to GitLab")
            || output.stdout.contains("successfully authenticated")
            || output.stdout.contains("Welcome to GitLab");

        Ok(success)
    }

    /// Extracts the host from a Git SSH URL.
    fn extract_host(url: &str) -> Result<String, AppError> {
        // Format: git@github.com:user/repo.git
        let without_prefix = url
            .strip_prefix("git@")
            .ok_or_else(|| AppError::Ssh("Invalid SSH URL format".into()))?;

        let host = without_prefix
            .split(':')
            .next()
            .ok_or_else(|| AppError::Ssh("Invalid SSH URL format".into()))?;

        Ok(host.to_string())
    }

    /// Gets the public key content for display.
    pub fn get_public_key(private_key_path: &Path) -> Result<String, AppError> {
        let pub_path = private_key_path.with_extension("pub");

        fs::read_to_string(&pub_path)
            .map_err(|e| AppError::Ssh(format!("Failed to read public key: {}", e)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_host() {
        assert_eq!(
            SshProvider::extract_host("git@github.com:user/repo.git").unwrap(),
            "github.com"
        );
        assert_eq!(
            SshProvider::extract_host("git@gitlab.com:org/project.git").unwrap(),
            "gitlab.com"
        );
    }

    #[test]
    fn test_key_type_display() {
        assert_eq!(SshKeyType::Ed25519.display_name(), "Ed25519 (recommended)");
        assert_eq!(SshKeyType::Rsa4096.keygen_type(), "rsa");
    }
}
