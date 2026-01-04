//! Docker registry operations and image management.

use std::path::Path;

use crate::core::app_config::{ImageConfig, Registry, RegistryCredentials};
use crate::core::context::ExecutionContext;
use crate::core::error::AppError;

/// Parses an image reference string into an ImageConfig.
///
/// Supports formats:
/// - `nginx` -> DockerHub, nginx, latest
/// - `nginx:1.25` -> DockerHub, nginx, 1.25
/// - `ghcr.io/user/app:v1` -> Ghcr, user/app, v1
/// - `gcr.io/project/app:latest` -> Gcr, project/app, latest
/// - `registry.example.com/app:tag` -> Custom, app, tag
pub fn parse_image_reference(input: &str) -> Result<ImageConfig, AppError> {
    let input = input.trim();

    if input.is_empty() {
        return Err(AppError::Validation("Image reference cannot be empty".into()));
    }

    // Split by @ for digest
    let (name_tag, digest) = if let Some(idx) = input.find('@') {
        (&input[..idx], Some(input[idx + 1..].to_string()))
    } else {
        (input, None)
    };

    // Split by : for tag (but be careful with registry ports)
    let (full_name, tag) = parse_name_and_tag(name_tag);

    // Determine registry from the name
    let (registry, name) = parse_registry_and_name(&full_name)?;

    Ok(ImageConfig {
        name,
        tag: tag.unwrap_or_else(|| "latest".to_string()),
        digest,
        registry,
        private: false,
    })
}

/// Parses name and tag from a string like "nginx:1.25" or "ghcr.io/user/app:v1".
fn parse_name_and_tag(input: &str) -> (String, Option<String>) {
    // Count colons to handle registry ports (e.g., localhost:5000/app:tag)
    let parts: Vec<&str> = input.split(':').collect();

    match parts.len() {
        1 => (input.to_string(), None),
        2 => {
            // Could be name:tag or registry:port/name
            if parts[1].contains('/') {
                // It's registry:port/name, no tag
                (input.to_string(), None)
            } else {
                // It's name:tag
                (parts[0].to_string(), Some(parts[1].to_string()))
            }
        }
        3 => {
            // registry:port/name:tag
            let tag = parts[2].to_string();
            let name = format!("{}:{}", parts[0], parts[1]);
            (name, Some(tag))
        }
        _ => (input.to_string(), None),
    }
}

/// Parses registry and image name from a full name.
fn parse_registry_and_name(full_name: &str) -> Result<(Registry, String), AppError> {
    // Check for known registries
    if full_name.starts_with("ghcr.io/") {
        let name = full_name.strip_prefix("ghcr.io/").unwrap().to_string();
        return Ok((Registry::Ghcr, name));
    }

    if full_name.starts_with("gcr.io/") {
        let name = full_name.strip_prefix("gcr.io/").unwrap().to_string();
        return Ok((Registry::Gcr, name));
    }

    // Check for ECR pattern: <account>.dkr.ecr.<region>.amazonaws.com/name
    if full_name.contains(".dkr.ecr.") && full_name.contains(".amazonaws.com/") {
        if let Some(idx) = full_name.find(".amazonaws.com/") {
            let registry_part = &full_name[..idx];
            let name = full_name[idx + 15..].to_string();

            // Extract region from pattern like "123456789.dkr.ecr.us-east-1"
            if let Some(region_start) = registry_part.find(".dkr.ecr.") {
                let region = registry_part[region_start + 9..].to_string();
                return Ok((Registry::Ecr { region }, name));
            }
        }
    }

    // Check if it looks like a custom registry (contains dots and slashes)
    if full_name.contains('/') && full_name.split('/').next().map(|s| s.contains('.')).unwrap_or(false) {
        let parts: Vec<&str> = full_name.splitn(2, '/').collect();
        if parts.len() == 2 {
            let registry_url = parts[0].to_string();
            let name = parts[1].to_string();
            return Ok((Registry::Custom { url: registry_url }, name));
        }
    }

    // Default to Docker Hub
    Ok((Registry::DockerHub, full_name.to_string()))
}

/// Returns the default port for well-known images.
pub fn detect_default_port(image_name: &str) -> Option<u16> {
    // Extract base image name (without registry prefix and tag)
    let base_name = image_name
        .split('/')
        .last()
        .unwrap_or(image_name)
        .split(':')
        .next()
        .unwrap_or(image_name)
        .to_lowercase();

    match base_name.as_str() {
        // Web servers
        "nginx" | "httpd" | "apache" => Some(80),
        "traefik" => Some(80),
        "caddy" => Some(80),

        // Databases
        "postgres" | "postgresql" => Some(5432),
        "mysql" | "mariadb" => Some(3306),
        "mongo" | "mongodb" => Some(27017),
        "redis" => Some(6379),
        "memcached" => Some(11211),
        "elasticsearch" => Some(9200),

        // Message queues
        "rabbitmq" => Some(5672),
        "nats" => Some(4222),
        "kafka" => Some(9092),

        // Monitoring
        "grafana" => Some(3000),
        "prometheus" => Some(9090),
        "jaeger" => Some(16686),

        // Other common images
        "registry" => Some(5000),
        "minio" => Some(9000),
        "vault" => Some(8200),
        "consul" => Some(8500),
        "gitea" => Some(3000),
        "drone" => Some(80),
        "jenkins" => Some(8080),
        "sonarqube" => Some(9000),

        _ => None,
    }
}

/// Pulls a Docker image from a registry.
pub fn pull_image(
    image: &ImageConfig,
    credentials: Option<&RegistryCredentials>,
    ctx: &ExecutionContext,
) -> Result<(), AppError> {
    let image_ref = image.full_reference();

    // If credentials are provided, login first
    if let Some(creds) = credentials {
        docker_login(image, creds, ctx)?;
    }

    // Pull the image
    let output = ctx.run_command("docker", &["pull", &image_ref])?;

    if !output.success {
        // Logout if we logged in
        if credentials.is_some() {
            let _ = docker_logout(image, ctx);
        }
        return Err(AppError::Docker(format!(
            "Failed to pull image {}: {}",
            image_ref, output.stderr
        )));
    }

    // Logout if we logged in
    if credentials.is_some() {
        let _ = docker_logout(image, ctx);
    }

    Ok(())
}

/// Logs into a Docker registry.
fn docker_login(
    image: &ImageConfig,
    creds: &RegistryCredentials,
    ctx: &ExecutionContext,
) -> Result<(), AppError> {
    let registry_url = match &image.registry {
        Registry::DockerHub => "docker.io".to_string(),
        Registry::Ghcr => "ghcr.io".to_string(),
        Registry::Gcr => "gcr.io".to_string(),
        Registry::Ecr { region } => format!("{}.dkr.ecr.amazonaws.com", region),
        Registry::Custom { url } => url.clone(),
    };

    let output = ctx.run_command(
        "docker",
        &[
            "login",
            &registry_url,
            "-u",
            &creds.username,
            "--password-stdin",
        ],
    )?;

    if !output.success {
        return Err(AppError::Docker(format!(
            "Failed to login to registry {}: {}",
            registry_url, output.stderr
        )));
    }

    Ok(())
}

/// Logs out from a Docker registry.
fn docker_logout(image: &ImageConfig, ctx: &ExecutionContext) -> Result<(), AppError> {
    let registry_url = match &image.registry {
        Registry::DockerHub => "docker.io".to_string(),
        Registry::Ghcr => "ghcr.io".to_string(),
        Registry::Gcr => "gcr.io".to_string(),
        Registry::Ecr { region } => format!("{}.dkr.ecr.amazonaws.com", region),
        Registry::Custom { url } => url.clone(),
    };

    ctx.run_command("docker", &["logout", &registry_url])?;
    Ok(())
}

/// Saves registry credentials securely.
pub fn save_credentials(
    credentials_path: &Path,
    credentials: &RegistryCredentials,
) -> Result<(), AppError> {
    use std::os::unix::fs::PermissionsExt;

    // Serialize credentials (password is skipped by serde)
    // We need to store the auth token instead
    let content = serde_json::to_string_pretty(&credentials)
        .map_err(|e| AppError::Config(format!("Failed to serialize credentials: {}", e)))?;

    std::fs::write(credentials_path, content)
        .map_err(|e| AppError::Config(format!("Failed to write credentials: {}", e)))?;

    // Set restrictive permissions (600)
    let perms = std::fs::Permissions::from_mode(0o600);
    std::fs::set_permissions(credentials_path, perms)
        .map_err(|e| AppError::Config(format!("Failed to set credentials permissions: {}", e)))?;

    Ok(())
}

/// Loads registry credentials.
pub fn load_credentials(credentials_path: &Path) -> Result<Option<RegistryCredentials>, AppError> {
    if !credentials_path.exists() {
        return Ok(None);
    }

    let content = std::fs::read_to_string(credentials_path)
        .map_err(|e| AppError::Config(format!("Failed to read credentials: {}", e)))?;

    let credentials: RegistryCredentials = serde_json::from_str(&content)
        .map_err(|e| AppError::Config(format!("Failed to parse credentials: {}", e)))?;

    Ok(Some(credentials))
}

/// Checks if an image exists locally.
pub fn image_exists_locally(image: &ImageConfig, ctx: &ExecutionContext) -> Result<bool, AppError> {
    let image_ref = image.full_reference();
    let output = ctx.run_command("docker", &["image", "inspect", &image_ref])?;
    Ok(output.success)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_image() {
        let config = parse_image_reference("nginx").unwrap();
        assert_eq!(config.name, "nginx");
        assert_eq!(config.tag, "latest");
        assert!(matches!(config.registry, Registry::DockerHub));
    }

    #[test]
    fn test_parse_image_with_tag() {
        let config = parse_image_reference("nginx:1.25").unwrap();
        assert_eq!(config.name, "nginx");
        assert_eq!(config.tag, "1.25");
        assert!(matches!(config.registry, Registry::DockerHub));
    }

    #[test]
    fn test_parse_ghcr_image() {
        let config = parse_image_reference("ghcr.io/user/app:v1.0").unwrap();
        assert_eq!(config.name, "user/app");
        assert_eq!(config.tag, "v1.0");
        assert!(matches!(config.registry, Registry::Ghcr));
    }

    #[test]
    fn test_parse_gcr_image() {
        let config = parse_image_reference("gcr.io/my-project/my-app:latest").unwrap();
        assert_eq!(config.name, "my-project/my-app");
        assert_eq!(config.tag, "latest");
        assert!(matches!(config.registry, Registry::Gcr));
    }

    #[test]
    fn test_parse_custom_registry() {
        let config = parse_image_reference("registry.example.com/myapp:v2").unwrap();
        assert_eq!(config.name, "myapp");
        assert_eq!(config.tag, "v2");
        assert!(matches!(config.registry, Registry::Custom { .. }));
    }

    #[test]
    fn test_detect_nginx_port() {
        assert_eq!(detect_default_port("nginx"), Some(80));
        assert_eq!(detect_default_port("nginx:1.25"), Some(80));
    }

    #[test]
    fn test_detect_postgres_port() {
        assert_eq!(detect_default_port("postgres"), Some(5432));
    }

    #[test]
    fn test_detect_unknown_port() {
        assert_eq!(detect_default_port("my-custom-app"), None);
    }
}
