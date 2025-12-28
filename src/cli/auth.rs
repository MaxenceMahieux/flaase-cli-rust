//! Authentication command handlers for HTTP Basic Auth.

use crate::core::app_config::AppConfig;
use crate::core::error::AppError;
use crate::core::secrets::{AppSecrets, SecretsManager};
use crate::ui;

/// Lists authentication status for all domains of an app.
pub fn list(app: &str) -> Result<(), AppError> {
    let config = AppConfig::load(app)?;

    println!("Authentication for {}", app);
    println!();

    // Currently we only have a single domain per app
    // When multi-domain support is added, this will iterate over all domains
    let domain = &config.domain;
    let secrets = SecretsManager::load_secrets(&config.secrets_path())?;

    // Check if auth is configured for this domain
    let auth_status = if let Some(auth_secret) = secrets.auth.get(domain) {
        // Extract username from htpasswd line
        let username = auth_secret
            .password_hash
            .split(':')
            .next()
            .unwrap_or("unknown");
        format!(
            "{} enabled ({})",
            console::style("\u{2713}").green(),
            username
        )
    } else {
        format!("{} disabled", console::style("\u{2717}").dim())
    };

    // Calculate column widths
    let domain_width = domain.len().max(6);

    // Header
    println!(
        "  {:<width$}   AUTH",
        "DOMAIN",
        width = domain_width
    );
    println!("  {}", "─".repeat(domain_width + 20));

    // Domain row
    println!(
        "  {:<width$}   {}",
        domain,
        auth_status,
        width = domain_width
    );

    println!();

    Ok(())
}

/// Adds Basic Auth to a domain.
pub fn add(
    app: &str,
    domain: &str,
    user: Option<&str>,
    password_arg: Option<&str>,
) -> Result<(), AppError> {
    let config = AppConfig::load(app)?;

    // Validate domain belongs to this app
    if config.domain != domain {
        return Err(AppError::Validation(format!(
            "Domain '{}' is not configured for app '{}'. Current domain: {}",
            domain, app, config.domain
        )));
    }

    // Check if auth already exists
    let mut secrets = SecretsManager::load_secrets(&config.secrets_path())?;
    if secrets.auth.contains_key(domain) {
        ui::warning(&format!("Auth already enabled on {}. Use 'fl auth update' to change credentials.", domain));
        return Ok(());
    }

    // Get username
    let username = if let Some(u) = user {
        u.to_string()
    } else {
        ui::input("Username?")?
    };

    if username.is_empty() {
        return Err(AppError::Validation("Username cannot be empty".into()));
    }

    // Get password
    let password = if let Some(p) = password_arg {
        p.to_string()
    } else {
        let pwd = ui::password("Password?")?;
        if pwd.is_empty() {
            return Err(AppError::Validation("Password cannot be empty".into()));
        }
        pwd
    };

    println!();
    ui::step("Configuring authentication...");

    // Generate auth secret
    let auth_secret = SecretsManager::generate_auth_secret(&username, &password)?;

    // Save to secrets
    secrets.auth.insert(domain.to_string(), auth_secret);
    SecretsManager::save_secrets(&config.secrets_path(), &secrets)?;

    // Update Traefik config
    update_traefik_config(&config, &secrets)?;

    ui::success(&format!("Basic Auth enabled on {}", domain));

    Ok(())
}

/// Removes Basic Auth from a domain.
pub fn remove(app: &str, domain: &str) -> Result<(), AppError> {
    let config = AppConfig::load(app)?;

    // Validate domain belongs to this app
    if config.domain != domain {
        return Err(AppError::Validation(format!(
            "Domain '{}' is not configured for app '{}'. Current domain: {}",
            domain, app, config.domain
        )));
    }

    // Load secrets
    let mut secrets = SecretsManager::load_secrets(&config.secrets_path())?;

    if !secrets.auth.contains_key(domain) {
        ui::info(&format!("Auth is not enabled on {}", domain));
        return Ok(());
    }

    // Remove auth
    secrets.auth.remove(domain);
    SecretsManager::save_secrets(&config.secrets_path(), &secrets)?;

    // Update Traefik config
    update_traefik_config(&config, &secrets)?;

    ui::success(&format!("Basic Auth disabled on {}", domain));

    Ok(())
}

/// Updates credentials for a domain with existing auth.
pub fn update(
    app: &str,
    domain: &str,
    user: Option<&str>,
    password_arg: Option<&str>,
) -> Result<(), AppError> {
    let config = AppConfig::load(app)?;

    // Validate domain belongs to this app
    if config.domain != domain {
        return Err(AppError::Validation(format!(
            "Domain '{}' is not configured for app '{}'. Current domain: {}",
            domain, app, config.domain
        )));
    }

    // Load secrets
    let mut secrets = SecretsManager::load_secrets(&config.secrets_path())?;

    if !secrets.auth.contains_key(domain) {
        ui::warning(&format!(
            "Auth is not enabled on {}. Use 'fl auth add' first.",
            domain
        ));
        return Ok(());
    }

    // Get current username as default
    let current_username = secrets
        .auth
        .get(domain)
        .and_then(|s| s.password_hash.split(':').next())
        .unwrap_or("admin")
        .to_string();

    // Get username
    let username = if let Some(u) = user {
        u.to_string()
    } else {
        let input = ui::input_with_default("Username?", &current_username)?;
        if input.is_empty() {
            current_username.clone()
        } else {
            input
        }
    };

    // Get password
    let password = if let Some(p) = password_arg {
        p.to_string()
    } else {
        let pwd = ui::password("New password?")?;
        if pwd.is_empty() {
            return Err(AppError::Validation("Password cannot be empty".into()));
        }
        pwd
    };

    println!();
    ui::step("Updating authentication...");

    // Generate new auth secret
    let auth_secret = SecretsManager::generate_auth_secret(&username, &password)?;

    // Save to secrets
    secrets.auth.insert(domain.to_string(), auth_secret);
    SecretsManager::save_secrets(&config.secrets_path(), &secrets)?;

    // Update Traefik config
    update_traefik_config(&config, &secrets)?;

    ui::success(&format!("Basic Auth updated on {}", domain));

    Ok(())
}

/// Updates the Traefik configuration with auth middleware.
fn update_traefik_config(config: &AppConfig, secrets: &AppSecrets) -> Result<(), AppError> {
    use crate::core::context::ExecutionContext;
    use crate::templates::traefik::{generate_app_config, AppDomain};

    let ctx = ExecutionContext::new(false, false);

    // Build domain list with auth info
    let mut domains = Vec::new();

    // Currently single domain, will be extended for multi-domain
    let domain_name = &config.domain;
    let mut app_domain = AppDomain::new(domain_name, true);

    // Add auth if configured
    if let Some(auth_secret) = secrets.auth.get(domain_name) {
        app_domain = app_domain.with_auth(&auth_secret.password_hash);
    }

    domains.push(app_domain);

    // Generate and write Traefik config
    let traefik_config = generate_app_config(&config.name, &domains, config.effective_port());

    let traefik_path = format!(
        "{}/{}.yml",
        crate::core::FLAASE_TRAEFIK_DYNAMIC_PATH,
        config.name
    );

    ctx.write_file(&traefik_path, &traefik_config)?;

    Ok(())
}
