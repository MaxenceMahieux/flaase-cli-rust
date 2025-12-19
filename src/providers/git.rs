//! Git operations provider for cloning and updating repositories.

use std::path::Path;

use crate::core::context::ExecutionContext;
use crate::core::error::AppError;

/// Git provider for repository operations.
pub struct GitProvider;

impl GitProvider {
    /// Clones a repository using SSH.
    pub fn clone(
        repo_url: &str,
        target_dir: &Path,
        ssh_key: &Path,
        ctx: &ExecutionContext,
    ) -> Result<(), AppError> {
        // Build GIT_SSH_COMMAND with the specific key
        let ssh_command = format!(
            "ssh -i {} -o StrictHostKeyChecking=accept-new -o BatchMode=yes",
            ssh_key.display()
        );

        // Ignore ctx for now - we need to use std::process::Command for env vars
        let _ = ctx;

        // Run with custom SSH command via environment
        let output = std::process::Command::new("git")
            .env("GIT_SSH_COMMAND", &ssh_command)
            .args(["clone", "--depth", "1", repo_url, target_dir.to_str().unwrap()])
            .output()
            .map_err(|e| AppError::Git(format!("Failed to clone repository: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AppError::Git(format!("Failed to clone repository: {}", stderr)));
        }

        Ok(())
    }

    /// Pulls latest changes from the repository.
    pub fn pull(
        repo_dir: &Path,
        ssh_key: &Path,
        _ctx: &ExecutionContext,
    ) -> Result<bool, AppError> {
        let ssh_command = format!(
            "ssh -i {} -o StrictHostKeyChecking=accept-new -o BatchMode=yes",
            ssh_key.display()
        );

        // Fetch first
        let fetch_output = std::process::Command::new("git")
            .current_dir(repo_dir)
            .env("GIT_SSH_COMMAND", &ssh_command)
            .args(["fetch", "origin"])
            .output()
            .map_err(|e| AppError::Git(format!("Failed to fetch: {}", e)))?;

        if !fetch_output.status.success() {
            let stderr = String::from_utf8_lossy(&fetch_output.stderr);
            return Err(AppError::Git(format!("Failed to fetch: {}", stderr)));
        }

        // Check if there are changes
        let diff_output = std::process::Command::new("git")
            .current_dir(repo_dir)
            .args(["diff", "HEAD", "origin/HEAD", "--quiet"])
            .output()
            .map_err(|e| AppError::Git(format!("Failed to check diff: {}", e)))?;

        let has_changes = !diff_output.status.success();

        if has_changes {
            // Pull changes
            let pull_output = std::process::Command::new("git")
                .current_dir(repo_dir)
                .env("GIT_SSH_COMMAND", &ssh_command)
                .args(["pull", "--rebase", "origin"])
                .output()
                .map_err(|e| AppError::Git(format!("Failed to pull: {}", e)))?;

            if !pull_output.status.success() {
                let stderr = String::from_utf8_lossy(&pull_output.stderr);
                return Err(AppError::Git(format!("Failed to pull: {}", stderr)));
            }
        }

        Ok(has_changes)
    }

    /// Gets the current commit hash.
    pub fn get_commit_hash(repo_dir: &Path) -> Result<String, AppError> {
        let output = std::process::Command::new("git")
            .current_dir(repo_dir)
            .args(["rev-parse", "--short", "HEAD"])
            .output()
            .map_err(|e| AppError::Git(format!("Failed to get commit hash: {}", e)))?;

        if !output.status.success() {
            return Err(AppError::Git("Failed to get commit hash".into()));
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Checks if a directory is a git repository.
    pub fn is_repo(path: &Path) -> bool {
        path.join(".git").exists()
    }
}
