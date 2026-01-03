//! Notification system for deployment events.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use crate::core::app_config::{DiscordNotificationConfig, NotificationConfig, SlackNotificationConfig};
use crate::core::deployments::DeploymentStatus;
use crate::core::error::AppError;

/// Deployment event for notifications.
#[derive(Debug, Clone)]
pub struct DeploymentEvent {
    pub app_name: String,
    pub commit_sha: String,
    pub commit_message: String,
    pub branch: String,
    pub triggered_by: String,
    pub status: DeploymentStatus,
    pub duration_secs: Option<u64>,
    pub error_message: Option<String>,
}

/// Sends notifications for a deployment event.
pub fn send_notifications(
    config: &NotificationConfig,
    event: &DeploymentEvent,
) -> Result<(), AppError> {
    if !config.enabled {
        return Ok(());
    }

    // Check if we should notify for this event
    let should_notify = match event.status {
        DeploymentStatus::Triggered => config.events.on_start,
        DeploymentStatus::PendingApproval => config.events.on_start, // Notify as start
        DeploymentStatus::Success => config.events.on_success,
        DeploymentStatus::Failed => config.events.on_failure,
        DeploymentStatus::RolledBack => config.events.on_failure, // Notify as failure
    };

    if !should_notify {
        return Ok(());
    }

    // Send to Slack
    if let Some(slack) = &config.slack {
        if let Err(e) = send_slack_notification(slack, event) {
            eprintln!("Failed to send Slack notification: {}", e);
        }
    }

    // Send to Discord
    if let Some(discord) = &config.discord {
        if let Err(e) = send_discord_notification(discord, event) {
            eprintln!("Failed to send Discord notification: {}", e);
        }
    }

    Ok(())
}

/// Sends a Slack notification.
fn send_slack_notification(
    config: &SlackNotificationConfig,
    event: &DeploymentEvent,
) -> Result<(), AppError> {
    let (emoji, color, status_text) = match event.status {
        DeploymentStatus::Triggered => (":rocket:", "#3498db", "started"),
        DeploymentStatus::PendingApproval => (":hourglass:", "#f39c12", "awaiting approval"),
        DeploymentStatus::Success => (":white_check_mark:", "#2ecc71", "succeeded"),
        DeploymentStatus::Failed => (":x:", "#e74c3c", "failed"),
        DeploymentStatus::RolledBack => (":rewind:", "#9b59b6", "rolled back"),
    };

    let duration_text = event
        .duration_secs
        .map(|d| format!(" in {}s", d))
        .unwrap_or_default();

    let error_text = event
        .error_message
        .as_ref()
        .map(|e| format!("\n> {}", e))
        .unwrap_or_default();

    let payload = serde_json::json!({
        "username": config.username.as_deref().unwrap_or("Flaase"),
        "icon_emoji": ":flaase:",
        "channel": config.channel,
        "attachments": [{
            "color": color,
            "blocks": [
                {
                    "type": "section",
                    "text": {
                        "type": "mrkdwn",
                        "text": format!(
                            "{} Deployment {} for *{}*{}{}",
                            emoji, status_text, event.app_name, duration_text, error_text
                        )
                    }
                },
                {
                    "type": "context",
                    "elements": [
                        {
                            "type": "mrkdwn",
                            "text": format!(
                                "*Branch:* {} | *Commit:* `{}` | *By:* {}",
                                event.branch, event.commit_sha, event.triggered_by
                            )
                        }
                    ]
                },
                {
                    "type": "context",
                    "elements": [
                        {
                            "type": "mrkdwn",
                            "text": format!("_{}_", truncate_message(&event.commit_message, 100))
                        }
                    ]
                }
            ]
        }]
    });

    send_webhook_request(&config.webhook_url, &payload)
}

/// Sends a Discord notification.
fn send_discord_notification(
    config: &DiscordNotificationConfig,
    event: &DeploymentEvent,
) -> Result<(), AppError> {
    let (emoji, color, status_text) = match event.status {
        DeploymentStatus::Triggered => (":rocket:", 0x3498db, "started"),
        DeploymentStatus::PendingApproval => (":hourglass:", 0xf39c12, "awaiting approval"),
        DeploymentStatus::Success => (":white_check_mark:", 0x2ecc71, "succeeded"),
        DeploymentStatus::Failed => (":x:", 0xe74c3c, "failed"),
        DeploymentStatus::RolledBack => (":rewind:", 0x9b59b6, "rolled back"),
    };

    let duration_text = event
        .duration_secs
        .map(|d| format!(" in {}s", d))
        .unwrap_or_default();

    let mut description = format!(
        "{} Deployment {} for **{}**{}",
        emoji, status_text, event.app_name, duration_text
    );

    if let Some(error) = &event.error_message {
        description.push_str(&format!("\n> {}", error));
    }

    let payload = serde_json::json!({
        "username": config.username.as_deref().unwrap_or("Flaase"),
        "embeds": [{
            "color": color,
            "description": description,
            "fields": [
                {
                    "name": "Branch",
                    "value": event.branch,
                    "inline": true
                },
                {
                    "name": "Commit",
                    "value": format!("`{}`", event.commit_sha),
                    "inline": true
                },
                {
                    "name": "Triggered by",
                    "value": event.triggered_by,
                    "inline": true
                }
            ],
            "footer": {
                "text": truncate_message(&event.commit_message, 100)
            }
        }]
    });

    send_webhook_request(&config.webhook_url, &payload)
}

/// Sends a webhook request using raw TCP/TLS.
fn send_webhook_request(url: &str, payload: &serde_json::Value) -> Result<(), AppError> {
    let body = serde_json::to_string(payload)
        .map_err(|e| AppError::Config(format!("Failed to serialize payload: {}", e)))?;

    // Parse URL
    let (host, port, path, use_tls) = parse_webhook_url(url)?;

    // Build HTTP request
    let request = format!(
        "POST {} HTTP/1.1\r\n\
         Host: {}\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\
         User-Agent: Flaase/1.0\r\n\
         \r\n\
         {}",
        path,
        host,
        body.len(),
        body
    );

    if use_tls {
        send_https_request(&host, port, &request)?;
    } else {
        send_http_request(&host, port, &request)?;
    }

    Ok(())
}

/// Parses a webhook URL into components.
fn parse_webhook_url(url: &str) -> Result<(String, u16, String, bool), AppError> {
    let use_tls = url.starts_with("https://");
    let url_without_scheme = url
        .trim_start_matches("https://")
        .trim_start_matches("http://");

    let (host_port, path) = match url_without_scheme.find('/') {
        Some(idx) => (
            &url_without_scheme[..idx],
            url_without_scheme[idx..].to_string(),
        ),
        None => (url_without_scheme, "/".to_string()),
    };

    let (host, port) = match host_port.find(':') {
        Some(idx) => {
            let port = host_port[idx + 1..]
                .parse()
                .map_err(|_| AppError::Config("Invalid port in URL".into()))?;
            (host_port[..idx].to_string(), port)
        }
        None => (
            host_port.to_string(),
            if use_tls { 443 } else { 80 },
        ),
    };

    Ok((host, port, path, use_tls))
}

/// Sends an HTTP request.
fn send_http_request(host: &str, port: u16, request: &str) -> Result<(), AppError> {
    let addr = format!("{}:{}", host, port);
    let mut stream = TcpStream::connect_timeout(
        &addr.parse().map_err(|e| AppError::Config(format!("Invalid address: {}", e)))?,
        Duration::from_secs(10),
    )
    .map_err(|e| AppError::Config(format!("Failed to connect: {}", e)))?;

    stream
        .set_write_timeout(Some(Duration::from_secs(10)))
        .ok();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .ok();

    stream
        .write_all(request.as_bytes())
        .map_err(|e| AppError::Config(format!("Failed to send request: {}", e)))?;

    // Read response (we don't really need it, but consume it)
    let mut response = vec![0u8; 1024];
    let _ = stream.read(&mut response);

    Ok(())
}

/// Sends an HTTPS request using native-tls or rustls.
/// Falls back to spawning curl if TLS is not available.
fn send_https_request(host: &str, port: u16, request: &str) -> Result<(), AppError> {
    // Use curl as a reliable fallback for HTTPS
    use std::process::Command;

    // Extract the body from the request
    let body_start = request.find("\r\n\r\n").unwrap_or(request.len()) + 4;
    let body = &request[body_start..];

    // Extract the path from the request
    let path_start = request.find(' ').unwrap_or(0) + 1;
    let path_end = request[path_start..].find(' ').unwrap_or(request.len() - path_start) + path_start;
    let path = &request[path_start..path_end];

    let url = format!("https://{}:{}{}", host, port, path);

    let output = Command::new("curl")
        .args([
            "-s",
            "-X", "POST",
            "-H", "Content-Type: application/json",
            "-d", body,
            "--max-time", "10",
            &url,
        ])
        .output()
        .map_err(|e| AppError::Config(format!("Failed to execute curl: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AppError::Config(format!(
            "Webhook request failed: {}",
            stderr
        )));
    }

    Ok(())
}

/// Truncates a message to a maximum length.
fn truncate_message(msg: &str, max_len: usize) -> String {
    let first_line = msg.lines().next().unwrap_or(msg);
    if first_line.len() > max_len {
        format!("{}...", &first_line[..max_len - 3])
    } else {
        first_line.to_string()
    }
}

/// Tests a notification configuration by sending a test message.
pub fn test_notification(config: &NotificationConfig, app_name: &str) -> Result<(), AppError> {
    let test_event = DeploymentEvent {
        app_name: app_name.to_string(),
        commit_sha: "abc1234".to_string(),
        commit_message: "Test notification from Flaase".to_string(),
        branch: "main".to_string(),
        triggered_by: "flaase".to_string(),
        status: DeploymentStatus::Success,
        duration_secs: Some(42),
        error_message: None,
    };

    // Force send regardless of event settings
    if let Some(slack) = &config.slack {
        send_slack_notification(slack, &test_event)?;
    }

    if let Some(discord) = &config.discord {
        send_discord_notification(discord, &test_event)?;
    }

    Ok(())
}
