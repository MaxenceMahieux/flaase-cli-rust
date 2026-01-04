//! Status command implementation for listing all apps.

use chrono::{DateTime, Utc};
use console::{style, Term};

use crate::core::app_config::AppConfig;
use crate::core::context::ExecutionContext;
use crate::core::error::AppError;
use crate::providers::container::{ContainerRuntime, DockerRuntime};
use crate::ui;

/// App status for display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppStatus {
    Running,
    Stopped,
    Error,
    NotDeployed,
}

impl AppStatus {
    /// Returns the display string with color.
    pub fn display(&self) -> console::StyledObject<&'static str> {
        match self {
            AppStatus::Running => style("running").green(),
            AppStatus::Stopped => style("stopped").yellow(),
            AppStatus::Error => style("error").red(),
            AppStatus::NotDeployed => style("not deployed").dim(),
        }
    }
}

/// Information about an app for the status table.
struct AppInfo {
    name: String,
    status: AppStatus,
    domain: String,
    stack: String,
    deployed_at: Option<DateTime<Utc>>,
}

/// Formats a datetime as a relative time string.
fn format_relative_time(dt: DateTime<Utc>) -> String {
    let now = Utc::now();
    let duration = now.signed_duration_since(dt);

    let seconds = duration.num_seconds();
    let minutes = duration.num_minutes();
    let hours = duration.num_hours();
    let days = duration.num_days();
    let weeks = days / 7;
    let months = days / 30;

    if seconds < 60 {
        "just now".to_string()
    } else if minutes < 60 {
        if minutes == 1 {
            "1 minute ago".to_string()
        } else {
            format!("{} minutes ago", minutes)
        }
    } else if hours < 24 {
        if hours == 1 {
            "1 hour ago".to_string()
        } else {
            format!("{} hours ago", hours)
        }
    } else if days < 7 {
        if days == 1 {
            "1 day ago".to_string()
        } else {
            format!("{} days ago", days)
        }
    } else if weeks < 4 {
        if weeks == 1 {
            "1 week ago".to_string()
        } else {
            format!("{} weeks ago", weeks)
        }
    } else if months < 12 {
        if months == 1 {
            "1 month ago".to_string()
        } else {
            format!("{} months ago", months)
        }
    } else {
        let years = months / 12;
        if years == 1 {
            "1 year ago".to_string()
        } else {
            format!("{} years ago", years)
        }
    }
}

/// Gets the container status for an app.
fn get_app_status(
    app_name: &str,
    deployed_at: Option<DateTime<Utc>>,
    runtime: &DockerRuntime,
    ctx: &ExecutionContext,
) -> AppStatus {
    // If never deployed, return NotDeployed
    if deployed_at.is_none() {
        return AppStatus::NotDeployed;
    }

    let container_name = format!("flaase-{}-web", app_name);

    // Check if container exists and is running
    match runtime.container_is_running(&container_name, ctx) {
        Ok(true) => AppStatus::Running,
        Ok(false) => {
            // Container exists but not running
            match runtime.container_exists(&container_name, ctx) {
                Ok(true) => AppStatus::Stopped,
                Ok(false) => AppStatus::NotDeployed,
                Err(_) => AppStatus::Error,
            }
        }
        Err(_) => AppStatus::Error,
    }
}

/// Prints the status table header.
fn print_table_header(term: &Term, col_widths: &[usize]) {
    let header = format!(
        "  {:<width0$}  {:<width1$}  {:<width2$}  {:<width3$}  {:<width4$}",
        "NAME",
        "STATUS",
        "DOMAIN",
        "STACK",
        "DEPLOYED",
        width0 = col_widths[0],
        width1 = col_widths[1],
        width2 = col_widths[2],
        width3 = col_widths[3],
        width4 = col_widths[4],
    );
    let _ = term.write_line(&style(header).dim().to_string());

    // Separator line
    let total_width: usize = col_widths.iter().sum::<usize>() + (col_widths.len() - 1) * 2 + 2;
    let separator = format!("  {}", "─".repeat(total_width));
    let _ = term.write_line(&style(separator).dim().to_string());
}

/// Prints a single app row.
fn print_app_row(term: &Term, app: &AppInfo, col_widths: &[usize]) {
    let deployed_str = match &app.deployed_at {
        Some(dt) => format_relative_time(*dt),
        None => "-".to_string(),
    };

    let status_str = format!("{}", app.status.display());

    // We need to handle the styled status separately for proper alignment
    let _ = term.write_line(&format!(
        "  {:<width0$}  {:<width1$}  {:<width2$}  {:<width3$}  {:<width4$}",
        app.name,
        status_str,
        app.domain,
        app.stack,
        deployed_str,
        width0 = col_widths[0],
        width1 = col_widths[1] + 10, // Add extra width for ANSI codes
        width2 = col_widths[2],
        width3 = col_widths[3],
        width4 = col_widths[4],
    ));
}

/// Prints the summary line.
fn print_summary(term: &Term, apps: &[AppInfo]) {
    let total = apps.len();
    let running = apps.iter().filter(|a| a.status == AppStatus::Running).count();
    let stopped = apps.iter().filter(|a| a.status == AppStatus::Stopped).count();
    let errors = apps.iter().filter(|a| a.status == AppStatus::Error).count();
    let not_deployed = apps
        .iter()
        .filter(|a| a.status == AppStatus::NotDeployed)
        .count();

    let mut parts = Vec::new();

    if running > 0 {
        parts.push(format!("{} {}", style(running).green(), "running"));
    }
    if stopped > 0 {
        parts.push(format!("{} {}", style(stopped).yellow(), "stopped"));
    }
    if errors > 0 {
        parts.push(format!("{} {}", style(errors).red(), "error"));
    }
    if not_deployed > 0 {
        parts.push(format!("{} {}", style(not_deployed).dim(), "not deployed"));
    }

    let _ = term.write_line("");
    let summary = if parts.is_empty() {
        format!("{} apps", total)
    } else {
        format!("{} apps ({})", total, parts.join(", "))
    };
    let _ = term.write_line(&summary);
}

/// Main status command handler.
pub fn status(_verbose: bool) -> Result<(), AppError> {
    let term = Term::stdout();
    let ctx = ExecutionContext::new(false, false);
    let runtime = DockerRuntime::new();

    // Get all apps
    let app_names = AppConfig::list_all()?;

    if app_names.is_empty() {
        ui::info("No apps configured");
        println!();
        println!(
            "Run {} to configure your first app",
            style("fl init").cyan()
        );
        return Ok(());
    }

    // Load app info
    let mut apps: Vec<AppInfo> = Vec::new();

    for name in &app_names {
        match AppConfig::load(name) {
            Ok(config) => {
                let status = get_app_status(name, config.deployed_at, &runtime, &ctx);
                let domain = config.primary_domain().to_string();
                apps.push(AppInfo {
                    name: config.name,
                    status,
                    domain,
                    stack: config.stack.as_ref().map(|s| s.display_name()).unwrap_or("Image").to_string(),
                    deployed_at: config.deployed_at,
                });
            }
            Err(_) => {
                // Config exists but failed to load
                apps.push(AppInfo {
                    name: name.clone(),
                    status: AppStatus::Error,
                    domain: "-".to_string(),
                    stack: "-".to_string(),
                    deployed_at: None,
                });
            }
        }
    }

    // Calculate column widths
    let col_widths = [
        apps.iter()
            .map(|a| a.name.len())
            .max()
            .unwrap_or(4)
            .max(4), // NAME
        12, // STATUS (fixed width for alignment)
        apps.iter()
            .map(|a| a.domain.len())
            .max()
            .unwrap_or(6)
            .max(6), // DOMAIN
        apps.iter()
            .map(|a| a.stack.len())
            .max()
            .unwrap_or(5)
            .max(5), // STACK
        12, // DEPLOYED (relative time)
    ];

    // Print header
    ui::section("Apps");
    print_table_header(&term, &col_widths);

    // Print each app
    for app in &apps {
        print_app_row(&term, app, &col_widths);
    }

    // Print summary
    print_summary(&term, &apps);

    Ok(())
}
