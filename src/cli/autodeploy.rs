//! Autodeploy command handlers for GitHub webhook-based deployments.

use crate::cli::webhook;
use crate::core::app_config::{
    AppConfig, AutodeployConfig, DiscordNotificationConfig, NotificationConfig,
    RateLimitConfig, SlackNotificationConfig,
};
use crate::core::deployments::{DeploymentHistory, DeploymentStatus};
use crate::core::error::AppError;
use crate::core::notifications::test_notification;
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
        let webhook_url = WebhookProvider::webhook_url(config.primary_domain(), &autodeploy.webhook_path);
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
        let webhook_url = WebhookProvider::webhook_url(config.primary_domain(), &autodeploy.webhook_path);

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
    let webhook_url = WebhookProvider::webhook_url(config.primary_domain(), &autodeploy.webhook_path);
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

// ============================================================================
// Rate Limiting Commands
// ============================================================================

/// Configures rate limiting for an app.
pub fn rate_limit(
    app: &str,
    enable: bool,
    disable: bool,
    max_deploys: Option<u32>,
    window: Option<u64>,
) -> Result<(), AppError> {
    let mut config = AppConfig::load(app)?;

    if config.autodeploy_config.is_none() {
        return Err(AppError::Validation(
            "Autodeploy is not enabled for this app.".into(),
        ));
    }

    let autodeploy = config.autodeploy_config.as_mut().unwrap();

    // Initialize rate limit config if not present
    if autodeploy.rate_limit.is_none() {
        autodeploy.rate_limit = Some(RateLimitConfig::default());
    }

    let rate_limit = autodeploy.rate_limit.as_mut().unwrap();

    // Handle enable/disable
    if enable {
        rate_limit.enabled = true;
        ui::success("Rate limiting enabled");
    } else if disable {
        rate_limit.enabled = false;
        ui::success("Rate limiting disabled");
    }

    // Update max_deploys if provided
    if let Some(max) = max_deploys {
        rate_limit.max_deploys = max;
        ui::info(&format!("Max deployments set to {}", max));
    }

    // Update window if provided
    if let Some(w) = window {
        rate_limit.window_seconds = w;
        ui::info(&format!("Time window set to {}s", w));
    }

    // Extract values for display before saving
    let enabled = rate_limit.enabled;
    let max = rate_limit.max_deploys;
    let window_secs = rate_limit.window_seconds;

    config.save()?;

    // Show current configuration
    println!();
    println!("Rate limiting for {}:", console::style(app).cyan());
    println!();
    println!(
        "  Enabled: {}",
        if enabled {
            console::style("Yes").green()
        } else {
            console::style("No").dim()
        }
    );
    println!("  Max deployments: {}", max);
    println!("  Time window: {}s", window_secs);
    println!();

    Ok(())
}

// ============================================================================
// Notification Commands
// ============================================================================

/// Shows notification configuration for an app.
pub fn notify_status(app: &str) -> Result<(), AppError> {
    let config = AppConfig::load(app)?;

    if config.autodeploy_config.is_none() {
        return Err(AppError::Validation(
            "Autodeploy is not enabled for this app.".into(),
        ));
    }

    let autodeploy = config.autodeploy_config.as_ref().unwrap();

    println!("Notification settings for {}", console::style(app).cyan());
    println!();

    match &autodeploy.notifications {
        None => {
            println!("  Status: {} Not configured", console::style("⚪").dim());
            println!();
            println!(
                "  Configure with: {}",
                console::style(format!("fl autodeploy notify slack {} --webhook-url <url>", app)).cyan()
            );
        }
        Some(notif) => {
            println!(
                "  Status: {}",
                if notif.enabled {
                    format!("{} Enabled", console::style("✓").green())
                } else {
                    format!("{} Disabled", console::style("✗").dim())
                }
            );
            println!();

            // Slack
            if let Some(slack) = &notif.slack {
                println!("  Slack:");
                println!("    Webhook: {}...{}", &slack.webhook_url[..30.min(slack.webhook_url.len())],
                    if slack.webhook_url.len() > 30 { "***" } else { "" });
                if let Some(channel) = &slack.channel {
                    println!("    Channel: {}", channel);
                }
                if let Some(username) = &slack.username {
                    println!("    Username: {}", username);
                }
            } else {
                println!("  Slack: {}", console::style("Not configured").dim());
            }

            // Discord
            if let Some(discord) = &notif.discord {
                println!("  Discord:");
                println!("    Webhook: {}...{}", &discord.webhook_url[..30.min(discord.webhook_url.len())],
                    if discord.webhook_url.len() > 30 { "***" } else { "" });
                if let Some(username) = &discord.username {
                    println!("    Username: {}", username);
                }
            } else {
                println!("  Discord: {}", console::style("Not configured").dim());
            }

            println!();
            println!("  Events:");
            println!(
                "    On start:   {}",
                if notif.events.on_start { "Yes" } else { "No" }
            );
            println!(
                "    On success: {}",
                if notif.events.on_success { "Yes" } else { "No" }
            );
            println!(
                "    On failure: {}",
                if notif.events.on_failure { "Yes" } else { "No" }
            );
        }
    }

    println!();
    Ok(())
}

/// Enables notifications for an app.
pub fn notify_enable(app: &str) -> Result<(), AppError> {
    let mut config = AppConfig::load(app)?;

    if config.autodeploy_config.is_none() {
        return Err(AppError::Validation(
            "Autodeploy is not enabled for this app.".into(),
        ));
    }

    let autodeploy = config.autodeploy_config.as_mut().unwrap();

    // Initialize notifications if not present
    if autodeploy.notifications.is_none() {
        autodeploy.notifications = Some(NotificationConfig::default());
    }

    let notif = autodeploy.notifications.as_mut().unwrap();

    // Check if at least one provider is configured
    if notif.slack.is_none() && notif.discord.is_none() {
        return Err(AppError::Validation(
            "Configure at least one notification provider first (Slack or Discord)".into(),
        ));
    }

    notif.enabled = true;
    config.save()?;

    ui::success("Notifications enabled");
    Ok(())
}

/// Disables notifications for an app.
pub fn notify_disable(app: &str) -> Result<(), AppError> {
    let mut config = AppConfig::load(app)?;

    if config.autodeploy_config.is_none() {
        return Err(AppError::Validation(
            "Autodeploy is not enabled for this app.".into(),
        ));
    }

    let autodeploy = config.autodeploy_config.as_mut().unwrap();

    if let Some(notif) = autodeploy.notifications.as_mut() {
        notif.enabled = false;
    }

    config.save()?;

    ui::success("Notifications disabled");
    Ok(())
}

/// Configures Slack notifications for an app.
pub fn notify_slack(
    app: &str,
    webhook_url: Option<&str>,
    channel: Option<&str>,
    username: Option<&str>,
    remove: bool,
) -> Result<(), AppError> {
    let mut config = AppConfig::load(app)?;

    if config.autodeploy_config.is_none() {
        return Err(AppError::Validation(
            "Autodeploy is not enabled for this app.".into(),
        ));
    }

    let autodeploy = config.autodeploy_config.as_mut().unwrap();

    // Initialize notifications if not present
    if autodeploy.notifications.is_none() {
        autodeploy.notifications = Some(NotificationConfig::default());
    }

    let notif = autodeploy.notifications.as_mut().unwrap();

    if remove {
        notif.slack = None;
        config.save()?;
        ui::success("Slack configuration removed");
        return Ok(());
    }

    // Get or create Slack config
    let slack = notif.slack.get_or_insert_with(|| SlackNotificationConfig {
        webhook_url: String::new(),
        channel: None,
        username: None,
    });

    // Update webhook URL if provided
    if let Some(url) = webhook_url {
        if !url.starts_with("https://hooks.slack.com/") {
            ui::warning("Webhook URL doesn't look like a Slack webhook URL");
        }
        slack.webhook_url = url.to_string();
    }

    // Require webhook URL
    if slack.webhook_url.is_empty() {
        return Err(AppError::Validation(
            "Webhook URL is required. Use --webhook-url <url>".into(),
        ));
    }

    // Update optional fields
    if let Some(ch) = channel {
        slack.channel = Some(ch.to_string());
    }
    if let Some(user) = username {
        slack.username = Some(user.to_string());
    }

    // Enable notifications automatically
    notif.enabled = true;

    config.save()?;

    ui::success("Slack notifications configured");
    println!();
    println!(
        "  Test with: {}",
        console::style(format!("fl autodeploy notify test {}", app)).cyan()
    );

    Ok(())
}

/// Configures Discord notifications for an app.
pub fn notify_discord(
    app: &str,
    webhook_url: Option<&str>,
    username: Option<&str>,
    remove: bool,
) -> Result<(), AppError> {
    let mut config = AppConfig::load(app)?;

    if config.autodeploy_config.is_none() {
        return Err(AppError::Validation(
            "Autodeploy is not enabled for this app.".into(),
        ));
    }

    let autodeploy = config.autodeploy_config.as_mut().unwrap();

    // Initialize notifications if not present
    if autodeploy.notifications.is_none() {
        autodeploy.notifications = Some(NotificationConfig::default());
    }

    let notif = autodeploy.notifications.as_mut().unwrap();

    if remove {
        notif.discord = None;
        config.save()?;
        ui::success("Discord configuration removed");
        return Ok(());
    }

    // Get or create Discord config
    let discord = notif.discord.get_or_insert_with(|| DiscordNotificationConfig {
        webhook_url: String::new(),
        username: None,
    });

    // Update webhook URL if provided
    if let Some(url) = webhook_url {
        if !url.starts_with("https://discord.com/api/webhooks/")
            && !url.starts_with("https://discordapp.com/api/webhooks/") {
            ui::warning("Webhook URL doesn't look like a Discord webhook URL");
        }
        discord.webhook_url = url.to_string();
    }

    // Require webhook URL
    if discord.webhook_url.is_empty() {
        return Err(AppError::Validation(
            "Webhook URL is required. Use --webhook-url <url>".into(),
        ));
    }

    // Update optional fields
    if let Some(user) = username {
        discord.username = Some(user.to_string());
    }

    // Enable notifications automatically
    notif.enabled = true;

    config.save()?;

    ui::success("Discord notifications configured");
    println!();
    println!(
        "  Test with: {}",
        console::style(format!("fl autodeploy notify test {}", app)).cyan()
    );

    Ok(())
}

/// Configures notification events for an app.
pub fn notify_events(
    app: &str,
    on_start: Option<bool>,
    on_success: Option<bool>,
    on_failure: Option<bool>,
) -> Result<(), AppError> {
    let mut config = AppConfig::load(app)?;

    if config.autodeploy_config.is_none() {
        return Err(AppError::Validation(
            "Autodeploy is not enabled for this app.".into(),
        ));
    }

    let autodeploy = config.autodeploy_config.as_mut().unwrap();

    // Initialize notifications if not present
    if autodeploy.notifications.is_none() {
        autodeploy.notifications = Some(NotificationConfig::default());
    }

    let notif = autodeploy.notifications.as_mut().unwrap();

    // Update events
    if let Some(v) = on_start {
        notif.events.on_start = v;
    }
    if let Some(v) = on_success {
        notif.events.on_success = v;
    }
    if let Some(v) = on_failure {
        notif.events.on_failure = v;
    }

    // Extract values for display before saving
    let start = notif.events.on_start;
    let success = notif.events.on_success;
    let failure = notif.events.on_failure;

    config.save()?;

    ui::success("Notification events updated");
    println!();
    println!("  On start:   {}", if start { "Yes" } else { "No" });
    println!("  On success: {}", if success { "Yes" } else { "No" });
    println!("  On failure: {}", if failure { "Yes" } else { "No" });

    Ok(())
}

/// Sends a test notification for an app.
pub fn notify_test(app: &str) -> Result<(), AppError> {
    let config = AppConfig::load(app)?;

    if config.autodeploy_config.is_none() {
        return Err(AppError::Validation(
            "Autodeploy is not enabled for this app.".into(),
        ));
    }

    let autodeploy = config.autodeploy_config.as_ref().unwrap();

    match &autodeploy.notifications {
        None => {
            return Err(AppError::Validation(
                "Notifications are not configured for this app.".into(),
            ));
        }
        Some(notif) => {
            if notif.slack.is_none() && notif.discord.is_none() {
                return Err(AppError::Validation(
                    "No notification providers configured.".into(),
                ));
            }

            ui::step("Sending test notification...");

            test_notification(notif, app)?;

            ui::success("Test notification sent!");
        }
    }

    Ok(())
}
