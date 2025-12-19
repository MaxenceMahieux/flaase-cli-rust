//! Validation utilities for user input.

use std::path::Path;

use crate::core::error::AppError;
use crate::core::FLAASE_APPS_PATH;

/// Validates an app name.
/// Must be lowercase, alphanumeric with hyphens, 2-50 characters.
pub fn validate_app_name(name: &str) -> Result<(), AppError> {
    if name.is_empty() {
        return Err(AppError::Validation("App name cannot be empty".into()));
    }

    if name.len() < 2 {
        return Err(AppError::Validation(
            "App name must be at least 2 characters".into(),
        ));
    }

    if name.len() > 50 {
        return Err(AppError::Validation(
            "App name must be at most 50 characters".into(),
        ));
    }

    if !name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(AppError::Validation(
            "App name must contain only lowercase letters, numbers, and hyphens".into(),
        ));
    }

    if name.starts_with('-') || name.ends_with('-') {
        return Err(AppError::Validation(
            "App name cannot start or end with a hyphen".into(),
        ));
    }

    if name.contains("--") {
        return Err(AppError::Validation(
            "App name cannot contain consecutive hyphens".into(),
        ));
    }

    Ok(())
}

/// Checks if an app name is already taken.
pub fn is_app_name_available(name: &str) -> bool {
    let app_path = format!("{}/{}", FLAASE_APPS_PATH, name);
    !Path::new(&app_path).exists()
}

/// Validates a Git SSH URL.
/// Must be in format: git@host:user/repo.git
pub fn validate_git_ssh_url(url: &str) -> Result<(), AppError> {
    if url.is_empty() {
        return Err(AppError::Validation(
            "Repository URL cannot be empty".into(),
        ));
    }

    // Check for SSH format: git@github.com:user/repo.git
    if !url.starts_with("git@") {
        return Err(AppError::Validation(
            "Repository URL must be in SSH format (git@host:user/repo.git)".into(),
        ));
    }

    if !url.contains(':') {
        return Err(AppError::Validation(
            "Invalid SSH URL format. Expected: git@host:user/repo.git".into(),
        ));
    }

    if !url.ends_with(".git") {
        return Err(AppError::Validation(
            "Repository URL must end with .git".into(),
        ));
    }

    // Extract and validate the path part (user/repo.git)
    let path_part = url.split(':').nth(1);
    if let Some(path) = path_part {
        if !path.contains('/') {
            return Err(AppError::Validation(
                "Invalid repository path. Expected: user/repo.git".into(),
            ));
        }
    } else {
        return Err(AppError::Validation(
            "Invalid SSH URL format. Expected: git@host:user/repo.git".into(),
        ));
    }

    Ok(())
}

/// Validates a domain name.
pub fn validate_domain(domain: &str) -> Result<(), AppError> {
    if domain.is_empty() {
        return Err(AppError::Validation("Domain cannot be empty".into()));
    }

    // Basic domain validation
    if domain.starts_with('.') || domain.ends_with('.') {
        return Err(AppError::Validation(
            "Domain cannot start or end with a dot".into(),
        ));
    }

    if domain.starts_with('-') || domain.ends_with('-') {
        return Err(AppError::Validation(
            "Domain cannot start or end with a hyphen".into(),
        ));
    }

    // Check for valid characters
    if !domain
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.')
    {
        return Err(AppError::Validation(
            "Domain must contain only letters, numbers, hyphens, and dots".into(),
        ));
    }

    // Must have at least one dot (e.g., example.com)
    if !domain.contains('.') {
        return Err(AppError::Validation(
            "Domain must include a TLD (e.g., example.com)".into(),
        ));
    }

    // Check that parts between dots are valid
    for part in domain.split('.') {
        if part.is_empty() {
            return Err(AppError::Validation(
                "Domain cannot have empty parts between dots".into(),
            ));
        }
        if part.starts_with('-') || part.ends_with('-') {
            return Err(AppError::Validation(
                "Domain parts cannot start or end with a hyphen".into(),
            ));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_app_name_valid() {
        assert!(validate_app_name("my-app").is_ok());
        assert!(validate_app_name("app123").is_ok());
        assert!(validate_app_name("my-app-2").is_ok());
    }

    #[test]
    fn test_validate_app_name_invalid() {
        assert!(validate_app_name("").is_err());
        assert!(validate_app_name("a").is_err());
        assert!(validate_app_name("My-App").is_err());
        assert!(validate_app_name("my_app").is_err());
        assert!(validate_app_name("-myapp").is_err());
        assert!(validate_app_name("myapp-").is_err());
        assert!(validate_app_name("my--app").is_err());
    }

    #[test]
    fn test_validate_git_ssh_url_valid() {
        assert!(validate_git_ssh_url("git@github.com:user/repo.git").is_ok());
        assert!(validate_git_ssh_url("git@gitlab.com:org/project.git").is_ok());
    }

    #[test]
    fn test_validate_git_ssh_url_invalid() {
        assert!(validate_git_ssh_url("").is_err());
        assert!(validate_git_ssh_url("https://github.com/user/repo").is_err());
        assert!(validate_git_ssh_url("git@github.com:repo").is_err());
        assert!(validate_git_ssh_url("git@github.com:user/repo").is_err());
    }

    #[test]
    fn test_validate_domain_valid() {
        assert!(validate_domain("example.com").is_ok());
        assert!(validate_domain("my-app.example.com").is_ok());
        assert!(validate_domain("sub.domain.example.com").is_ok());
    }

    #[test]
    fn test_validate_domain_invalid() {
        assert!(validate_domain("").is_err());
        assert!(validate_domain("example").is_err());
        assert!(validate_domain(".example.com").is_err());
        assert!(validate_domain("example.com.").is_err());
        assert!(validate_domain("-example.com").is_err());
    }
}
