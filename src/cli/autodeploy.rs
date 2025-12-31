//! Autodeploy command handlers for GitHub webhook-based deployments.

use crate::cli::webhook;
use crate::core::app_config::{AppConfig, AutodeployConfig};
use crate::core::deployments::{DeploymentHistory, DeploymentStatus};
use crate::core::error::AppError;
use crate::core::secrets::SecretsManager;
use crate::providers::webhook::WebhookProvider;
use crate::ui;

/// Enables autodeploy for an app via GitHub webhook.
pub fn enable(app: &str, branch: Option<&str>) -> Result<(), AppError> {
    let mut config = AppConfig::load(app)?;

    // Check if already enabled
    if config.autodeploy_config.is_some() {
        ui::warning("Autodeploy is already enabled for this app.");
        println!();
        show_webhook_info(&config)?;
        return Ok(());
    }

    // Get branch to watch
    let watch_branch = if let Some(b) = branch {
        b.to_string()
    } else {
        ui::input_with_default("Branch to watch for deployments?", "main")?
    };

    println!();
    ui::step("Configuring autodeploy...");

    // Generate webhook path and secret
    let webhook_path = WebhookProvider::generate_webhook_path(app);
    let webhook_secret = SecretsManager::generate_webhook_secret();

    // Load and update secrets
    let mut secrets = SecretsManager::load_secrets(&config.secrets_path())?;
    secrets.webhook = Some(webhook_secret.clone());
    SecretsManager::save_secrets(&config.secrets_path(), &secrets)?;

    // Update config
    let autodeploy_config = AutodeployConfig::new(&webhook_path).with_branch(&watch_branch);
    config.autodeploy = true;
    config.autodeploy_config = Some(autodeploy_config);
    config.save()?;

    ui::success("Autodeploy enabled!");
    println!();

    // Show setup instructions
    show_setup_instructions(&config, &webhook_secret.secret)?;

    // Propose webhook service installation if not installed
    if !webhook::is_installed() {
        println!();
        println!(
            "{}",
            console::style("The webhook server is required to receive GitHub events.").dim()
        );
        println!();

        if ui::confirm("Install the webhook server as a system service?", true)? {
            println!();
            webhook::install()?;
        } else {
            println!();
            ui::info("You can install it later with:");
            println!("  {}", console::style("fl webhook install").cyan());
        }
    } else if !webhook::is_running() {
        println!();
        ui::warning("The webhook server is installed but not running.");
        println!(
            "  Start it with: {}",
            console::style("systemctl start flaase-webhook").cyan()
        );
    }

    Ok(())
}

/// Disables autodeploy for an app.
pub fn disable(app: &str) -> Result<(), AppError> {
    let mut config = AppConfig::load(app)?;

    if config.autodeploy_config.is_none() {
        ui::info("Autodeploy is not enabled for this app.");
        return Ok(());
    }

    // Confirm
    if !ui::confirm("Disable autodeploy for this app?", false)? {
        ui::info("Cancelled.");
        return Ok(());
    }

    println!();
    ui::step("Disabling autodeploy...");

    // Remove webhook secret
    let mut secrets = SecretsManager::load_secrets(&config.secrets_path())?;
    secrets.webhook = None;
    SecretsManager::save_secrets(&config.secrets_path(), &secrets)?;

    // Update config
    config.autodeploy = false;
    config.autodeploy_config = None;
    config.save()?;

    ui::success("Autodeploy disabled.");
    println!();
    ui::info("Remember to remove the webhook from your GitHub repository settings.");

    Ok(())
}

/// Shows autodeploy status for an app.
pub fn status(app: &str) -> Result<(), AppError> {
    let config = AppConfig::load(app)?;

    println!("Autodeploy status for {}", console::style(app).cyan());
    println!();

    if let Some(autodeploy) = &config.autodeploy_config {
        println!(
            "  Status:  {} Enabled",
            console::style("\u{2713}").green()
        );
        println!("  Branch:  {}", console::style(&autodeploy.branch).cyan());

        // Show webhook URL
        let webhook_url = WebhookProvider::webhook_url(&config.domain, &autodeploy.webhook_path);
        println!("  Webhook: {}", console::style(&webhook_url).dim());

        println!();

        // Show recent deployments
        show_deployment_history(&config)?;
    } else {
        println!(
            "  Status: {} Disabled",
            console::style("\u{2717}").dim()
        );
        println!();
        println!(
            "  Run {} to enable autodeploy.",
            console::style(format!("fl autodeploy enable {}", app)).cyan()
        );
    }

    println!();

    Ok(())
}

/// Shows recent deployment history for an app.
fn show_deployment_history(config: &AppConfig) -> Result<(), AppError> {
    let history = DeploymentHistory::load(&config.deployments_path())?;
    let recent = history.recent(5);

    if recent.is_empty() {
        println!(
            "  Recent deployments: {}",
            console::style("None").dim()
        );
        return Ok(());
    }

    println!("  Recent deployments:");
    println!();

    // Table header
    println!(
        "    {}  {}  {}  {}",
        console::style(format!("{:<19}", "DATE")).dim(),
        console::style(format!("{:<7}", "COMMIT")).dim(),
        console::style(format!("{:<10}", "STATUS")).dim(),
        console::style("MESSAGE").dim()
    );

    for record in recent {
        let date = record.timestamp.format("%Y-%m-%d %H:%M:%S");

        let status_str = match record.status {
            DeploymentStatus::Triggered => console::style("triggered").yellow().to_string(),
            DeploymentStatus::Success => console::style("success").green().to_string(),
            DeploymentStatus::Failed => console::style("failed").red().to_string(),
        };

        // Truncate commit message to 40 chars
        let msg: String = record.commit_message.chars().take(40).collect();
        let msg = if record.commit_message.len() > 40 {
            format!("{}...", msg)
        } else {
            msg
        };

        println!(
            "    {}  {}  {:<10}  {}",
            console::style(date).dim(),
            console::style(&record.commit_sha).yellow(),
            status_str,
            msg
        );
    }

    Ok(())
}

/// Shows webhook information for an already configured app.
fn show_webhook_info(config: &AppConfig) -> Result<(), AppError> {
    if let Some(autodeploy) = &config.autodeploy_config {
        let webhook_url = WebhookProvider::webhook_url(&config.domain, &autodeploy.webhook_path);

        println!("Current configuration:");
        println!("  Branch:  {}", console::style(&autodeploy.branch).cyan());
        println!("  Webhook: {}", console::style(&webhook_url).dim());
        println!();
        println!(
            "To view the webhook secret, run: {}",
            console::style(format!("fl autodeploy secret {}", config.name)).cyan()
        );
    }

    Ok(())
}

/// Converts a Git SSH URL to a GitHub HTTPS URL for the settings page.
/// e.g., "git@github.com:user/repo.git" -> "https://github.com/user/repo/settings/hooks/new"
fn repo_to_github_settings_url(repo: &str) -> String {
    // Handle SSH format: git@github.com:user/repo.git
    if repo.starts_with("git@github.com:") {
        let path = repo
            .trim_start_matches("git@github.com:")
            .trim_end_matches(".git");
        return format!("https://github.com/{}/settings/hooks/new", path);
    }

    // Handle HTTPS format: https://github.com/user/repo.git
    if repo.starts_with("https://github.com/") {
        let path = repo
            .trim_start_matches("https://github.com/")
            .trim_end_matches(".git");
        return format!("https://github.com/{}/settings/hooks/new", path);
    }

    // Fallback: just append settings path
    format!("{}/settings/hooks/new", repo.trim_end_matches(".git"))
}

/// Shows setup instructions for configuring the GitHub webhook.
fn show_setup_instructions(config: &AppConfig, secret: &str) -> Result<(), AppError> {
    let autodeploy = config.autodeploy_config.as_ref().unwrap();
    let webhook_url = WebhookProvider::webhook_url(&config.domain, &autodeploy.webhook_path);
    let github_settings_url = repo_to_github_settings_url(&config.repository);

    println!("{}", console::style("GitHub Webhook Setup").bold());
    println!();
    println!("1. Open this link to add a webhook to your repository:");
    println!();
    println!("   {}", console::style(&github_settings_url).cyan().underlined());
    println!();
    println!("2. Fill in the form with these values:");
    println!();
    println!(
        "   Payload URL:  {}",
        console::style(&webhook_url).cyan().bold()
    );
    println!(
        "   Content type: {}",
        console::style("application/json").cyan()
    );
    println!(
        "   Secret:       {}",
        console::style(secret).yellow().bold()
    );
    println!();
    println!("3. Under 'Which events would you like to trigger this webhook?':");
    println!("   Select: {}", console::style("Just the push event").cyan());
    println!();
    println!("4. Ensure '{}' is checked", console::style("Active").cyan());
    println!();
    println!("{}", console::style("Save the webhook and you're done!").green());
    println!();
    println!(
        "{}",
        console::style("Note: Keep the secret safe! You can view it again with:").dim()
    );
    println!(
        "   {}",
        console::style(format!("fl autodeploy secret {}", config.name)).dim()
    );

    Ok(())
}

/// Shows the webhook secret for an app (for reconfiguration).
pub fn secret(app: &str) -> Result<(), AppError> {
    let config = AppConfig::load(app)?;

    if config.autodeploy_config.is_none() {
        return Err(AppError::Validation(
            "Autodeploy is not enabled for this app.".into(),
        ));
    }

    // Confirm before showing secret
    println!(
        "{}",
        console::style("Warning: You are about to display a secret token.").yellow()
    );
    if !ui::confirm("Continue?", false)? {
        return Ok(());
    }

    let secrets = SecretsManager::load_secrets(&config.secrets_path())?;

    if let Some(webhook) = &secrets.webhook {
        println!();
        println!("Webhook secret for {}:", console::style(app).cyan());
        println!();
        println!("  {}", console::style(&webhook.secret).yellow().bold());
        println!();
    } else {
        return Err(AppError::Config("Webhook secret not found.".into()));
    }

    Ok(())
}

/// Regenerates the webhook secret for an app.
pub fn regenerate(app: &str) -> Result<(), AppError> {
    let config = AppConfig::load(app)?;

    if config.autodeploy_config.is_none() {
        return Err(AppError::Validation(
            "Autodeploy is not enabled for this app.".into(),
        ));
    }

    println!(
        "{}",
        console::style("Warning: Regenerating the secret will invalidate the current webhook.").yellow()
    );
    println!("You will need to update the secret in your GitHub repository settings.");
    println!();

    if !ui::confirm("Regenerate webhook secret?", false)? {
        ui::info("Cancelled.");
        return Ok(());
    }

    println!();
    ui::step("Regenerating webhook secret...");

    // Generate new secret
    let new_secret = SecretsManager::generate_webhook_secret();

    // Update secrets
    let mut secrets = SecretsManager::load_secrets(&config.secrets_path())?;
    secrets.webhook = Some(new_secret.clone());
    SecretsManager::save_secrets(&config.secrets_path(), &secrets)?;

    ui::success("Webhook secret regenerated!");
    println!();

    // Show the new secret
    println!("New webhook secret:");
    println!();
    println!("  {}", console::style(&new_secret.secret).yellow().bold());
    println!();
    println!(
        "{}",
        console::style("Update this secret in your GitHub repository webhook settings.").dim()
    );

    Ok(())
}

/// Shows deployment logs for an app.
pub fn logs(app: &str, limit: usize) -> Result<(), AppError> {
    let config = AppConfig::load(app)?;
    let history = DeploymentHistory::load(&config.deployments_path())?;
    let deployments = history.recent(limit);

    println!(
        "Deployment logs for {}",
        console::style(app).cyan().bold()
    );
    println!();

    if deployments.is_empty() {
        println!(
            "  {}",
            console::style("No deployments recorded yet.").dim()
        );
        println!();
        return Ok(());
    }

    // Table header
    println!(
        "  {}  {}  {}  {}  {}",
        console::style(format!("{:<19}", "DATE")).dim(),
        console::style(format!("{:<7}", "COMMIT")).dim(),
        console::style(format!("{:<10}", "STATUS")).dim(),
        console::style(format!("{:<10}", "SOURCE")).dim(),
        console::style("MESSAGE").dim()
    );

    for record in deployments {
        let date = record.timestamp.format("%Y-%m-%d %H:%M:%S");

        let status_str = match record.status {
            DeploymentStatus::Triggered => console::style(format!("{:<10}", "triggered"))
                .yellow()
                .to_string(),
            DeploymentStatus::Success => console::style(format!("{:<10}", "success"))
                .green()
                .to_string(),
            DeploymentStatus::Failed => console::style(format!("{:<10}", "failed"))
                .red()
                .to_string(),
        };

        let source_str = match record.source {
            crate::core::deployments::DeploymentSource::Webhook => {
                console::style(format!("{:<10}", "webhook")).cyan().to_string()
            }
            crate::core::deployments::DeploymentSource::Manual => {
                console::style(format!("{:<10}", "manual")).dim().to_string()
            }
        };

        // Truncate commit message to 40 chars
        let msg: String = record.commit_message.chars().take(40).collect();
        let msg = if record.commit_message.len() > 40 {
            format!("{}...", msg)
        } else {
            msg
        };

        println!(
            "  {}  {}  {}  {}  {}",
            console::style(date).dim(),
            console::style(&record.commit_sha).yellow(),
            status_str,
            source_str,
            msg
        );

        // Show triggered by for webhook deployments
        if matches!(
            record.source,
            crate::core::deployments::DeploymentSource::Webhook
        ) && record.triggered_by != "cli"
        {
            println!(
                "  {}  {}",
                console::style("                   ").dim(),
                console::style(format!("by @{}", record.triggered_by)).dim()
            );
        }
    }

    println!();

    // Show total count if there are more
    let total = history.deployments.len();
    if total > limit {
        println!(
            "  {} {} deployments total. Use {} to see more.",
            console::style(format!("{}", total)).bold(),
            console::style("deployments recorded,").dim(),
            console::style(format!("fl autodeploy logs {} --limit {}", app, total)).cyan()
        );
        println!();
    }

    Ok(())
}
