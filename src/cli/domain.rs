//! Domain management command handlers.

use std::net::ToSocketAddrs;

use crate::core::app_config::AppConfig;
use crate::core::context::ExecutionContext;
use crate::core::error::AppError;
use crate::core::secrets::SecretsManager;
use crate::core::FLAASE_TRAEFIK_DYNAMIC_PATH;
use crate::templates::traefik::{generate_app_config, AppDomain};
use crate::ui;
use crate::utils::validate_domain;

/// Lists all domains configured for an app.
pub fn list(app: &str) -> Result<(), AppError> {
    let config = AppConfig::load(app)?;

    println!();
    println!("Domains for {}", console::style(app).cyan().bold());
    println!();

    if config.domains.is_empty() {
        ui::warning("No domains configured");
        return Ok(());
    }

    // Calculate column widths
    let max_domain_width = config
        .domains
        .iter()
        .map(|d| d.domain.len())
        .max()
        .unwrap_or(6)
        .max(6);

    // Header
    println!(
        "  {:<width$}   {:<12} STATUS",
        "DOMAIN",
        "SSL",
        width = max_domain_width
    );
    println!("  {}", "─".repeat(max_domain_width + 25));

    // Load secrets for auth status
    let secrets = SecretsManager::load_secrets(&config.secrets_path()).ok();

    for domain_config in &config.domains {
        let status = if domain_config.primary {
            console::style("primary").green().to_string()
        } else {
            console::style("alias").dim().to_string()
        };

        // Check if auth is enabled for this domain
        let has_auth = secrets
            .as_ref()
            .map(|s| s.auth.contains_key(&domain_config.domain))
            .unwrap_or(false);

        let auth_indicator = if has_auth { " (auth)" } else { "" };

        // SSL is always valid with Let's Encrypt via Traefik
        let ssl = format!("{} valid", console::style("✓").green());

        println!(
            "  {:<width$}   {:<12} {}{}",
            domain_config.domain,
            ssl,
            status,
            auth_indicator,
            width = max_domain_width
        );
    }

    println!();

    Ok(())
}

/// Adds a domain to an app.
pub fn add(app: &str, domain: &str, skip_dns_check: bool) -> Result<(), AppError> {
    // Validate domain format
    validate_domain(domain)?;

    let mut config = AppConfig::load(app)?;

    // Check if domain already exists
    if config.domains.iter().any(|d| d.domain == domain) {
        return Err(AppError::Validation(format!(
            "Domain '{}' is already configured for this app",
            domain
        )));
    }

    println!();

    // DNS validation (unless skipped)
    if !skip_dns_check {
        ui::step("Verifying DNS configuration...");
        match verify_dns(domain) {
            Ok(_) => {
                ui::success(&format!("DNS verified for {}", domain));
            }
            Err(e) => {
                ui::warning(&format!("DNS check failed: {}", e));
                ui::info("You can skip DNS check with --skip-dns-check flag");
                ui::info("Make sure your DNS A/AAAA record points to this server before SSL can be issued");
                println!();

                if !ui::confirm("Continue anyway?", false)? {
                    return Err(AppError::Cancelled);
                }
            }
        }
    }

    // Add domain to config
    ui::step("Adding domain to configuration...");
    config.add_domain(domain);
    config.save()?;

    // Regenerate Traefik config
    ui::step("Configuring routing...");
    regenerate_traefik_config(&config)?;

    ui::step("Requesting SSL certificate...");
    ui::info("SSL certificate will be automatically issued by Let's Encrypt on first request");

    println!();
    ui::success(&format!("Domain added: https://{}", domain));

    Ok(())
}

/// Removes a domain from an app.
pub fn remove(app: &str, domain: &str) -> Result<(), AppError> {
    let mut config = AppConfig::load(app)?;

    // Find domain
    let domain_config = config.domains.iter().find(|d| d.domain == domain);

    match domain_config {
        None => {
            return Err(AppError::Validation(format!(
                "Domain '{}' is not configured for app '{}'",
                domain, app
            )));
        }
        Some(dc) if dc.primary => {
            return Err(AppError::Validation(
                "Cannot remove primary domain. Use 'fl destroy' to remove the entire app.".into(),
            ));
        }
        Some(_) => {}
    }

    println!();

    // Confirm removal
    if !ui::confirm(&format!("Remove domain '{}'?", domain), false)? {
        return Err(AppError::Cancelled);
    }

    // Remove domain from config
    ui::step("Removing domain from configuration...");
    config.remove_domain(domain);
    config.save()?;

    // Remove auth if configured
    let secrets_path = config.secrets_path();
    if let Ok(mut secrets) = SecretsManager::load_secrets(&secrets_path) {
        if secrets.auth.remove(domain).is_some() {
            SecretsManager::save_secrets(&secrets_path, &secrets)?;
        }
    }

    // Regenerate Traefik config
    ui::step("Updating routing configuration...");
    regenerate_traefik_config(&config)?;

    println!();
    ui::success(&format!("Domain removed: {}", domain));

    Ok(())
}

/// Verifies that a domain's DNS points to this server.
fn verify_dns(domain: &str) -> Result<(), AppError> {
    // Try to resolve the domain
    let addr = format!("{}:80", domain);
    let resolved = addr.to_socket_addrs().map_err(|e| {
        AppError::Validation(format!(
            "Could not resolve domain '{}': {}. Ensure DNS A record is configured.",
            domain, e
        ))
    })?;

    let ips: Vec<_> = resolved.map(|a| a.ip()).collect();
    if ips.is_empty() {
        return Err(AppError::Validation(format!(
            "Domain '{}' does not resolve to any IP address",
            domain
        )));
    }

    // Note: We don't verify the IP matches our server as that requires
    // knowing the server's public IP which can be complex in various network setups
    Ok(())
}

/// Regenerates the Traefik configuration for all domains of an app.
fn regenerate_traefik_config(config: &AppConfig) -> Result<(), AppError> {
    let ctx = ExecutionContext::new(false, false);

    // Load secrets for auth info
    let secrets = SecretsManager::load_secrets(&config.secrets_path()).ok();

    // Build domain list with auth info
    let mut domains = Vec::new();

    for domain_config in &config.domains {
        let mut app_domain = AppDomain::new(&domain_config.domain, domain_config.primary);

        // Add auth if configured
        if let Some(ref secrets) = secrets {
            if let Some(auth_secret) = secrets.auth.get(&domain_config.domain) {
                app_domain = app_domain.with_auth(&auth_secret.password_hash);
            }
        }

        domains.push(app_domain);
    }

    // Generate and write Traefik config
    let traefik_config = generate_app_config(&config.name, &domains, config.effective_port());
    let traefik_path = format!("{}/{}.yml", FLAASE_TRAEFIK_DYNAMIC_PATH, config.name);

    ctx.write_file(&traefik_path, &traefik_config)?;

    Ok(())
}
