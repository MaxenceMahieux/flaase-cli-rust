//! Webhook server for autodeploy functionality.

// Read trait is needed for read_to_end on request body reader
#[allow(unused_imports)]
use std::io::Read;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tiny_http::{Response, Server, StatusCode};

use crate::core::app_config::AppConfig;
use crate::core::deployments::{DeploymentHistory, DeploymentRecord};
use crate::core::error::AppError;
use crate::core::secrets::SecretsManager;
use crate::providers::webhook::WebhookProvider;
use crate::ui;

/// Default port for the webhook server.
pub const DEFAULT_PORT: u16 = 9876;

/// Systemd service name.
const SERVICE_NAME: &str = "flaase-webhook";

/// Starts the webhook server.
pub fn serve(host: &str, port: u16, verbose: bool) -> Result<(), AppError> {
    let addr = format!("{}:{}", host, port);

    ui::info(&format!("Starting webhook server on {}", addr));

    let server = Server::http(&addr).map_err(|e| {
        AppError::Config(format!("Failed to start webhook server: {}", e))
    })?;

    ui::success(&format!("Webhook server listening on http://{}", addr));
    println!();
    println!("Endpoints:");
    println!("  POST /webhook/{{app-token}}  - GitHub webhook endpoint");
    println!("  GET  /health               - Health check");
    println!();
    println!("Press Ctrl+C to stop the server.");
    println!();

    // Handle graceful shutdown
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();

    ctrlc_handler(r);

    // Main request loop
    for request in server.incoming_requests() {
        if !running.load(Ordering::SeqCst) {
            break;
        }

        let method = request.method().to_string();
        let url = request.url().to_string();

        if verbose {
            let remote = request
                .remote_addr()
                .map(|a| a.to_string())
                .unwrap_or_else(|| "unknown".to_string());
            println!(
                "{} {} {}",
                console::style(&method).cyan(),
                &url,
                console::style(format!("from {}", remote)).dim()
            );
        }

        // Route and handle requests
        match (method.as_str(), url.as_str()) {
            ("GET", "/health") => {
                let response = handle_health();
                let _ = request.respond(response);
            }
            ("POST", path) if path.starts_with("/webhook/") => {
                // handle_webhook takes ownership and responds internally
                handle_webhook(request, path, verbose);
            }
            _ => {
                let response = Response::from_string("Not Found")
                    .with_status_code(StatusCode(404));
                let _ = request.respond(response);
            }
        };
    }

    ui::info("Webhook server stopped.");
    Ok(())
}

/// Sets up Ctrl+C handler for graceful shutdown.
fn ctrlc_handler(running: Arc<AtomicBool>) {
    let _ = ctrlc::set_handler(move || {
        println!();
        ui::info("Shutting down...");
        running.store(false, Ordering::SeqCst);
    });
}

/// Handles health check requests.
fn handle_health() -> Response<std::io::Cursor<Vec<u8>>> {
    Response::from_string(r#"{"status":"ok"}"#)
        .with_header(
            tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
        )
        .with_status_code(StatusCode(200))
}

/// Handles webhook requests from GitHub.
fn handle_webhook(mut request: tiny_http::Request, path: &str, verbose: bool) {
    // Extract webhook path token
    let webhook_token = path.trim_start_matches("/webhook/");

    if webhook_token.is_empty() {
        let _ = request.respond(json_error(400, "Missing webhook token"));
        return;
    }

    // Find app by webhook path
    let (app_config, app_secrets) = match find_app_by_webhook_path(webhook_token) {
        Ok(Some((config, secrets))) => (config, secrets),
        Ok(None) => {
            if verbose {
                ui::warning(&format!("No app found for webhook path: {}", webhook_token));
            }
            let _ = request.respond(json_error(404, "Webhook not found"));
            return;
        }
        Err(e) => {
            ui::error(&format!("Error finding app: {}", e));
            let _ = request.respond(json_error(500, "Internal error"));
            return;
        }
    };

    // Get headers before reading body (need to clone values we need)
    let signature = request
        .headers()
        .iter()
        .find(|h| h.field.as_str().to_ascii_lowercase() == "x-hub-signature-256")
        .map(|h| h.value.to_string());

    let event_type = request
        .headers()
        .iter()
        .find(|h| h.field.as_str().to_ascii_lowercase() == "x-github-event")
        .map(|h| h.value.to_string())
        .unwrap_or_else(|| "unknown".to_string());

    // Read request body
    let mut body = Vec::new();
    if let Err(e) = request.as_reader().read_to_end(&mut body) {
        ui::error(&format!("Failed to read request body: {}", e));
        let _ = request.respond(json_error(400, "Failed to read body"));
        return;
    }

    // Validate GitHub signature
    let webhook_secret = match &app_secrets.webhook {
        Some(ws) => &ws.secret,
        None => {
            ui::error("No webhook secret configured for app");
            let _ = request.respond(json_error(500, "Webhook not configured"));
            return;
        }
    };

    match &signature {
        Some(sig) => {
            if !WebhookProvider::validate_signature(&body, sig, webhook_secret) {
                if verbose {
                    ui::warning("Invalid webhook signature");
                }
                let _ = request.respond(json_error(401, "Invalid signature"));
                return;
            }
        }
        None => {
            if verbose {
                ui::warning("Missing X-Hub-Signature-256 header");
            }
            let _ = request.respond(json_error(401, "Missing signature"));
            return;
        }
    }

    if verbose {
        println!(
            "  {} Received {} event for {}",
            console::style("\u{2713}").green(),
            console::style(&event_type).cyan(),
            console::style(&app_config.name).bold()
        );
    }

    // Only handle push events
    if event_type != "push" {
        let _ = request.respond(json_response(200, &format!("Ignored event type: {}", event_type)));
        return;
    }

    // Parse push event to get branch
    let payload: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            ui::error(&format!("Failed to parse webhook payload: {}", e));
            let _ = request.respond(json_error(400, "Invalid JSON payload"));
            return;
        }
    };

    // Extract branch from ref (refs/heads/main -> main)
    let ref_str = payload["ref"].as_str().unwrap_or("");
    let branch = ref_str.strip_prefix("refs/heads/").unwrap_or(ref_str);

    // Check if this is the watched branch
    let autodeploy_config = match &app_config.autodeploy_config {
        Some(c) => c,
        None => {
            let _ = request.respond(json_response(200, "Autodeploy not configured"));
            return;
        }
    };

    if branch != autodeploy_config.branch {
        if verbose {
            println!(
                "  {} Ignoring push to {} (watching {})",
                console::style("-").dim(),
                branch,
                autodeploy_config.branch
            );
        }
        let _ = request.respond(json_response(200, &format!("Ignored branch: {}", branch)));
        return;
    }

    // Trigger deployment
    let commit_sha = payload["after"]
        .as_str()
        .unwrap_or("")
        .chars()
        .take(7)
        .collect::<String>();

    let commit_msg = payload["head_commit"]["message"]
        .as_str()
        .unwrap_or("")
        .lines()
        .next()
        .unwrap_or("")
        .to_string();

    let pusher = payload["pusher"]["name"]
        .as_str()
        .unwrap_or("unknown")
        .to_string();

    println!(
        "  {} Deploying {} @ {} - {}",
        console::style("\u{279C}").cyan(),
        console::style(&app_config.name).bold(),
        console::style(&commit_sha).yellow(),
        &commit_msg
    );

    // Log deployment to history
    let deployment_record = DeploymentRecord::from_webhook(
        &commit_sha,
        &commit_msg,
        branch,
        &pusher,
    );

    if let Err(e) = log_deployment(&app_config, deployment_record) {
        if verbose {
            ui::warning(&format!("Failed to log deployment: {}", e));
        }
    }

    // Run fl update in background
    match trigger_update(&app_config.name) {
        Ok(_) => {
            println!(
                "  {} Deployment triggered for {}",
                console::style("\u{2713}").green(),
                app_config.name
            );
            let _ = request.respond(json_response(200, "Deployment triggered"));
        }
        Err(e) => {
            ui::error(&format!("Failed to trigger deployment: {}", e));
            let _ = request.respond(json_error(500, "Failed to trigger deployment"));
        }
    }
}

/// Logs a deployment to the app's deployment history.
fn log_deployment(config: &AppConfig, record: DeploymentRecord) -> Result<(), AppError> {
    let path = config.deployments_path();
    let mut history = DeploymentHistory::load(&path)?;
    history.add(record);
    history.save(&path)?;
    Ok(())
}

/// Finds an app by its webhook path.
fn find_app_by_webhook_path(
    webhook_path: &str,
) -> Result<Option<(AppConfig, crate::core::secrets::AppSecrets)>, AppError> {
    let apps = AppConfig::list_all()?;

    for app_name in apps {
        let config = AppConfig::load(&app_name)?;

        if let Some(autodeploy) = &config.autodeploy_config {
            if autodeploy.webhook_path == webhook_path {
                let secrets = SecretsManager::load_secrets(&config.secrets_path())?;
                return Ok(Some((config, secrets)));
            }
        }
    }

    Ok(None)
}

/// Triggers an app update using fl update command.
fn trigger_update(app_name: &str) -> Result<(), AppError> {
    // Get the path to the current executable
    let exe_path = std::env::current_exe()
        .map_err(|e| AppError::Config(format!("Failed to get executable path: {}", e)))?;

    // Spawn fl update in background
    Command::new(&exe_path)
        .args(["update", app_name])
        .spawn()
        .map_err(|e| AppError::Config(format!("Failed to spawn update command: {}", e)))?;

    Ok(())
}

/// Creates a JSON error response.
fn json_error(status: u16, message: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    let body = format!(r#"{{"error":"{}"}}"#, message);
    Response::from_string(body)
        .with_header(
            tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
        )
        .with_status_code(StatusCode(status))
}

/// Creates a JSON success response.
fn json_response(status: u16, message: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    let body = format!(r#"{{"message":"{}"}}"#, message);
    Response::from_string(body)
        .with_header(
            tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
        )
        .with_status_code(StatusCode(status))
}

/// Generates Traefik configuration for webhook routing.
fn generate_traefik_webhook_config() -> String {
    format!(
        r#"# Traefik configuration for Flaase webhook endpoint
# Generated by Flaase

http:
  routers:
    flaase-webhook:
      rule: "PathPrefix(`/flaase/webhook/`)"
      entryPoints:
        - websecure
      service: flaase-webhook
      priority: 100
      tls:
        certResolver: letsencrypt

    flaase-webhook-http:
      rule: "PathPrefix(`/flaase/webhook/`)"
      entryPoints:
        - web
      service: flaase-webhook
      priority: 100

  services:
    flaase-webhook:
      loadBalancer:
        servers:
          - url: "http://host.docker.internal:{port}"
"#,
        port = DEFAULT_PORT
    )
}

/// Installs the webhook server as a systemd service.
pub fn install() -> Result<(), AppError> {
    ui::step("Installing webhook server...");

    // Get the path to the current executable
    let exe_path = std::env::current_exe()
        .map_err(|e| AppError::Config(format!("Failed to get executable path: {}", e)))?;

    // 1. Write Traefik configuration for webhook routing
    ui::step("Configuring Traefik routing...");
    let traefik_config = generate_traefik_webhook_config();
    let traefik_path = format!(
        "{}/flaase-webhook.yml",
        crate::core::FLAASE_TRAEFIK_DYNAMIC_PATH
    );

    std::fs::write(&traefik_path, traefik_config)
        .map_err(|e| AppError::Config(format!("Failed to write Traefik config: {}", e)))?;

    // 2. Create systemd service
    ui::step("Creating systemd service...");
    let service_content = format!(
        r#"[Unit]
Description=Flaase Webhook Server
Documentation=https://github.com/MaxenceMahieux/flaase-cli-rust
After=network.target

[Service]
Type=simple
ExecStart={exe_path} webhook serve
Restart=always
RestartSec=5
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=multi-user.target
"#,
        exe_path = exe_path.display()
    );

    let service_path = format!("/etc/systemd/system/{}.service", SERVICE_NAME);

    std::fs::write(&service_path, service_content)
        .map_err(|e| AppError::Config(format!("Failed to write service file: {}", e)))?;

    // 3. Reload systemd and start service
    ui::step("Starting service...");
    Command::new("systemctl")
        .args(["daemon-reload"])
        .status()
        .map_err(|e| AppError::Config(format!("Failed to reload systemd: {}", e)))?;

    Command::new("systemctl")
        .args(["enable", SERVICE_NAME])
        .status()
        .map_err(|e| AppError::Config(format!("Failed to enable service: {}", e)))?;

    Command::new("systemctl")
        .args(["start", SERVICE_NAME])
        .status()
        .map_err(|e| AppError::Config(format!("Failed to start service: {}", e)))?;

    ui::success("Webhook server installed and started!");
    println!();
    println!("Traefik will route /flaase/webhook/* to the webhook server.");
    println!();
    println!("Service commands:");
    println!("  systemctl status {}   - Check status", SERVICE_NAME);
    println!("  systemctl restart {}  - Restart service", SERVICE_NAME);
    println!("  journalctl -u {} -f   - View logs", SERVICE_NAME);

    Ok(())
}

/// Uninstalls the webhook server systemd service.
pub fn uninstall() -> Result<(), AppError> {
    ui::step("Uninstalling webhook server...");

    let service_path = format!("/etc/systemd/system/{}.service", SERVICE_NAME);
    let traefik_path = format!(
        "{}/flaase-webhook.yml",
        crate::core::FLAASE_TRAEFIK_DYNAMIC_PATH
    );

    // Stop service
    let _ = Command::new("systemctl")
        .args(["stop", SERVICE_NAME])
        .status();

    // Disable service
    let _ = Command::new("systemctl")
        .args(["disable", SERVICE_NAME])
        .status();

    // Remove service file
    if std::path::Path::new(&service_path).exists() {
        std::fs::remove_file(&service_path)
            .map_err(|e| AppError::Config(format!("Failed to remove service file: {}", e)))?;
    }

    // Remove Traefik config
    if std::path::Path::new(&traefik_path).exists() {
        std::fs::remove_file(&traefik_path)
            .map_err(|e| AppError::Config(format!("Failed to remove Traefik config: {}", e)))?;
    }

    // Reload systemd
    Command::new("systemctl")
        .args(["daemon-reload"])
        .status()
        .map_err(|e| AppError::Config(format!("Failed to reload systemd: {}", e)))?;

    ui::success("Webhook server uninstalled.");

    Ok(())
}

/// Checks if the webhook service is installed.
pub fn is_installed() -> bool {
    let service_path = format!("/etc/systemd/system/{}.service", SERVICE_NAME);
    std::path::Path::new(&service_path).exists()
}

/// Checks if the webhook service is running.
pub fn is_running() -> bool {
    if !is_installed() {
        return false;
    }

    Command::new("systemctl")
        .args(["is-active", SERVICE_NAME])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().eq("active"))
        .unwrap_or(false)
}

/// Shows the webhook server status.
pub fn status() -> Result<(), AppError> {
    println!("Webhook Server Status");
    println!();

    // Check if service is installed
    let service_path = format!("/etc/systemd/system/{}.service", SERVICE_NAME);
    let installed = std::path::Path::new(&service_path).exists();

    if !installed {
        println!(
            "  Service: {} Not installed",
            console::style("\u{2717}").dim()
        );
        println!();
        println!(
            "  Run {} to install the service.",
            console::style("fl webhook install").cyan()
        );
        return Ok(());
    }

    // Check service status
    let output = Command::new("systemctl")
        .args(["is-active", SERVICE_NAME])
        .output()
        .map_err(|e| AppError::Config(format!("Failed to check service status: {}", e)))?;

    let is_active = String::from_utf8_lossy(&output.stdout)
        .trim()
        .eq("active");

    if is_active {
        println!(
            "  Service: {} Running",
            console::style("\u{2713}").green()
        );
    } else {
        println!(
            "  Service: {} Stopped",
            console::style("\u{2717}").red()
        );
    }

    println!("  Port:    {}", DEFAULT_PORT);
    println!();

    // Count apps with autodeploy enabled
    let apps = AppConfig::list_all().unwrap_or_default();
    let autodeploy_count = apps
        .iter()
        .filter_map(|name| AppConfig::load(name).ok())
        .filter(|config| config.autodeploy_config.is_some())
        .count();

    println!("  Apps with autodeploy: {}", autodeploy_count);

    Ok(())
}
