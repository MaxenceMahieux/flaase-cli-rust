//! Webhook server for autodeploy functionality.

// Read trait is needed for read_to_end on request body reader
#[allow(unused_imports)]
use std::io::Read;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tiny_http::{Response, Server, StatusCode};

use crate::core::app_config::{AppConfig, EnvironmentConfig};
use crate::core::deployments::{DeploymentHistory, DeploymentRecord, DeploymentStatus, PendingApproval};
use crate::core::notifications::{send_notifications, DeploymentEvent};
use crate::core::error::AppError;
use crate::core::secrets::SecretsManager;
use crate::core::FLAASE_APPS_PATH;
use crate::providers::webhook::WebhookProvider;
use crate::ui;

/// Rate limiting state for tracking webhook requests per app.
struct RateLimitState {
    /// Map of app name to list of request timestamps.
    requests: HashMap<String, Vec<Instant>>,
}

impl RateLimitState {
    fn new() -> Self {
        Self {
            requests: HashMap::new(),
        }
    }

    /// Checks if a request is allowed under rate limiting rules.
    /// Returns true if allowed, false if rate limited.
    fn check_and_record(&mut self, app_name: &str, max_requests: u32, window_secs: u64) -> bool {
        let now = Instant::now();
        let window = Duration::from_secs(window_secs);

        let timestamps = self.requests.entry(app_name.to_string()).or_default();

        // Remove old timestamps outside the window
        timestamps.retain(|t| now.duration_since(*t) < window);

        // Check if we're at the limit
        if timestamps.len() >= max_requests as usize {
            return false;
        }

        // Record this request
        timestamps.push(now);
        true
    }
}

/// Deployment lock manager using file-based locks.
struct DeploymentLock;

impl DeploymentLock {
    /// Returns the lock file path for an app.
    fn lock_path(app_name: &str) -> PathBuf {
        PathBuf::from(format!("{}/{}/deploy.lock", FLAASE_APPS_PATH, app_name))
    }

    /// Attempts to acquire a deployment lock.
    /// Returns Ok(()) if lock acquired, Err if already locked.
    fn acquire(app_name: &str) -> Result<(), AppError> {
        let lock_path = Self::lock_path(app_name);

        // Check if lock file exists and is recent (less than 30 minutes old)
        if lock_path.exists() {
            if let Ok(metadata) = fs::metadata(&lock_path) {
                if let Ok(modified) = metadata.modified() {
                    let age = SystemTime::now()
                        .duration_since(modified)
                        .unwrap_or(Duration::from_secs(0));

                    // If lock is less than 30 minutes old, consider it active
                    if age < Duration::from_secs(30 * 60) {
                        return Err(AppError::Config(
                            "Deployment already in progress".into(),
                        ));
                    }
                }
            }
        }

        // Create lock file with current timestamp
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        fs::write(&lock_path, timestamp.to_string())
            .map_err(|e| AppError::Config(format!("Failed to create lock file: {}", e)))?;

        Ok(())
    }

    /// Releases a deployment lock.
    fn release(app_name: &str) {
        let lock_path = Self::lock_path(app_name);
        let _ = fs::remove_file(lock_path);
    }

    /// Checks if a deployment is currently locked.
    fn is_locked(app_name: &str) -> bool {
        let lock_path = Self::lock_path(app_name);

        if !lock_path.exists() {
            return false;
        }

        // Check if lock is recent
        if let Ok(metadata) = fs::metadata(&lock_path) {
            if let Ok(modified) = metadata.modified() {
                let age = SystemTime::now()
                    .duration_since(modified)
                    .unwrap_or(Duration::from_secs(0));

                return age < Duration::from_secs(30 * 60);
            }
        }

        false
    }
}

/// Pending approvals storage.
struct PendingApprovalsStore;

impl PendingApprovalsStore {
    /// Returns the path to pending approvals file for an app.
    fn path(app_name: &str) -> PathBuf {
        PathBuf::from(format!("{}/{}/pending_approvals.json", FLAASE_APPS_PATH, app_name))
    }

    /// Loads pending approvals for an app.
    fn load(app_name: &str) -> Result<Vec<PendingApproval>, AppError> {
        let path = Self::path(app_name);
        if !path.exists() {
            return Ok(Vec::new());
        }

        let content = fs::read_to_string(&path)
            .map_err(|e| AppError::Config(format!("Failed to read pending approvals: {}", e)))?;

        serde_json::from_str(&content)
            .map_err(|e| AppError::Config(format!("Failed to parse pending approvals: {}", e)))
    }

    /// Saves pending approvals for an app.
    fn save(app_name: &str, approvals: &[PendingApproval]) -> Result<(), AppError> {
        let path = Self::path(app_name);
        let content = serde_json::to_string_pretty(approvals)
            .map_err(|e| AppError::Config(format!("Failed to serialize pending approvals: {}", e)))?;

        fs::write(&path, content)
            .map_err(|e| AppError::Config(format!("Failed to write pending approvals: {}", e)))
    }

    /// Adds a new pending approval.
    fn add(app_name: &str, approval: PendingApproval) -> Result<(), AppError> {
        let mut approvals = Self::load(app_name)?;

        // Remove expired approvals
        approvals.retain(|a| !a.is_expired());

        // Add new approval
        approvals.push(approval);

        Self::save(app_name, &approvals)
    }

    /// Gets a pending approval by ID or the latest one.
    pub fn get(app_name: &str, approval_id: Option<&str>) -> Result<Option<PendingApproval>, AppError> {
        let mut approvals = Self::load(app_name)?;

        // Remove expired approvals
        approvals.retain(|a| !a.is_expired());
        Self::save(app_name, &approvals)?;

        match approval_id {
            Some(id) => Ok(approvals.into_iter().find(|a| a.approval_id == id)),
            None => Ok(approvals.into_iter().next()),
        }
    }

    /// Removes a pending approval by ID.
    pub fn remove(app_name: &str, approval_id: &str) -> Result<(), AppError> {
        let mut approvals = Self::load(app_name)?;
        approvals.retain(|a| a.approval_id != approval_id);
        Self::save(app_name, &approvals)
    }

    /// Lists all pending approvals for an app.
    pub fn list(app_name: &str) -> Result<Vec<PendingApproval>, AppError> {
        let mut approvals = Self::load(app_name)?;

        // Remove expired approvals
        let before_len = approvals.len();
        approvals.retain(|a| !a.is_expired());

        // Save if any were removed
        if approvals.len() != before_len {
            Self::save(app_name, &approvals)?;
        }

        Ok(approvals)
    }
}

/// Determines the target environment based on the branch.
fn determine_environment<'a>(
    branch: &str,
    environments: Option<&'a Vec<EnvironmentConfig>>,
) -> (String, Option<&'a EnvironmentConfig>) {
    match environments {
        Some(envs) => {
            // Find environment matching this branch
            if let Some(env) = envs.iter().find(|e| e.branch == branch) {
                (env.name.clone(), Some(env))
            } else {
                // Default to production if no match
                ("production".to_string(), None)
            }
        }
        None => ("production".to_string(), None),
    }
}

/// Checks if deployment requires approval.
fn requires_approval(
    env_config: Option<&EnvironmentConfig>,
    approval_config: Option<&crate::core::app_config::ApprovalConfig>,
) -> bool {
    // If environment config exists and auto_deploy is false, require approval
    if let Some(env) = env_config {
        if !env.auto_deploy {
            return true;
        }
    }

    // If approval config exists and is enabled, require approval
    if let Some(approval) = approval_config {
        if approval.enabled {
            return true;
        }
    }

    false
}

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

    // Rate limiting state (shared across requests)
    let rate_limit_state = Arc::new(Mutex::new(RateLimitState::new()));

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
        // Support both /webhook/xxx (direct) and /flaase/webhook/xxx (via Traefik)
        match (method.as_str(), url.as_str()) {
            ("GET", "/health") => {
                let response = handle_health();
                let _ = request.respond(response);
            }
            ("POST", path) if path.starts_with("/flaase/webhook/") => {
                // Strip /flaase prefix for handler
                let webhook_path = path.strip_prefix("/flaase").unwrap_or(path);
                handle_webhook(request, webhook_path, verbose, Arc::clone(&rate_limit_state));
            }
            ("POST", path) if path.starts_with("/webhook/") => {
                handle_webhook(request, path, verbose, Arc::clone(&rate_limit_state));
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
fn handle_webhook(
    mut request: tiny_http::Request,
    path: &str,
    verbose: bool,
    rate_limit_state: Arc<Mutex<RateLimitState>>,
) {
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

    // Determine target environment based on branch
    let (environment, env_config) = determine_environment(
        branch,
        autodeploy_config.environments.as_ref(),
    );

    // Check if this branch should trigger deployment
    // Either it's the main autodeploy branch OR it's mapped to an environment
    let should_deploy = branch == autodeploy_config.branch || env_config.is_some();

    if !should_deploy {
        if verbose {
            println!(
                "  {} Ignoring push to {} (watching {} and no environment mapping)",
                console::style("-").dim(),
                branch,
                autodeploy_config.branch
            );
        }
        let _ = request.respond(json_response(200, &format!("Ignored branch: {}", branch)));
        return;
    }

    if verbose && env_config.is_some() {
        println!(
            "  {} Branch {} mapped to environment {}",
            console::style("\u{279C}").cyan(),
            console::style(branch).yellow(),
            console::style(&environment).green()
        );
    }

    // Check rate limiting
    if let Some(rate_limit) = &autodeploy_config.rate_limit {
        if rate_limit.enabled {
            let mut state = rate_limit_state.lock().unwrap();
            if !state.check_and_record(
                &app_config.name,
                rate_limit.max_deploys,
                rate_limit.window_seconds,
            ) {
                if verbose {
                    ui::warning(&format!(
                        "Rate limit exceeded for {} ({} deploys in {}s)",
                        app_config.name, rate_limit.max_deploys, rate_limit.window_seconds
                    ));
                }
                let _ = request.respond(json_error(429, "Rate limit exceeded"));
                return;
            }
        }
    }

    // Check deployment lock
    if DeploymentLock::is_locked(&app_config.name) {
        if verbose {
            ui::warning(&format!("Deployment already in progress for {}", app_config.name));
        }
        let _ = request.respond(json_error(409, "Deployment already in progress"));
        return;
    }

    // Extract deployment info
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

    // Check if this deployment requires approval
    let needs_approval = requires_approval(env_config, autodeploy_config.approval.as_ref());

    if needs_approval {
        let timeout_minutes = autodeploy_config
            .approval
            .as_ref()
            .map(|a| a.timeout_minutes)
            .unwrap_or(60);

        let approval = PendingApproval::new(
            &app_config.name,
            &commit_sha,
            &commit_msg,
            branch,
            &environment,
            &pusher,
            timeout_minutes,
        );

        println!(
            "  {} Deployment for {} requires approval (env: {})",
            console::style("\u{23F3}").yellow(),
            console::style(&app_config.name).bold(),
            console::style(&environment).cyan()
        );

        // Save pending approval
        if let Err(e) = PendingApprovalsStore::add(&app_config.name, approval.clone()) {
            ui::error(&format!("Failed to save pending approval: {}", e));
            let _ = request.respond(json_error(500, "Failed to save approval request"));
            return;
        }

        // Log deployment with pending approval status
        let deployment_record = DeploymentRecord::from_webhook(
            &commit_sha,
            &commit_msg,
            branch,
            &pusher,
            &environment,
        );
        let mut record = deployment_record;
        record.status = DeploymentStatus::PendingApproval;

        if let Err(e) = log_deployment(&app_config, record) {
            if verbose {
                ui::warning(&format!("Failed to log deployment: {}", e));
            }
        }

        // Send notification for pending approval
        if let Some(ref notif) = autodeploy_config.notifications {
            let event = DeploymentEvent {
                app_name: app_config.name.clone(),
                commit_sha: commit_sha.clone(),
                commit_message: commit_msg.clone(),
                branch: branch.to_string(),
                triggered_by: pusher.clone(),
                status: DeploymentStatus::PendingApproval,
                duration_secs: None,
                error_message: None,
            };
            let _ = send_notifications(notif, &event);
        }

        let _ = request.respond(json_response(
            202,
            &format!(
                "Awaiting approval. ID: {}. Run: fl autodeploy approve {} {}",
                approval.approval_id, app_config.name, approval.approval_id
            ),
        ));
        return;
    }

    println!(
        "  {} Deploying {} @ {} - {}",
        console::style("\u{279C}").cyan(),
        console::style(&app_config.name).bold(),
        console::style(&commit_sha).yellow(),
        &commit_msg
    );

    // Log deployment to history (status: triggered)
    let deployment_record = DeploymentRecord::from_webhook(
        &commit_sha,
        &commit_msg,
        branch,
        &pusher,
        &environment,
    );

    if let Err(e) = log_deployment(&app_config, deployment_record) {
        if verbose {
            ui::warning(&format!("Failed to log deployment: {}", e));
        }
    }

    // Send start notification
    let notification_config = autodeploy_config.notifications.clone();
    if let Some(ref notif) = notification_config {
        let start_event = DeploymentEvent {
            app_name: app_config.name.clone(),
            commit_sha: commit_sha.clone(),
            commit_message: commit_msg.clone(),
            branch: branch.to_string(),
            triggered_by: pusher.clone(),
            status: DeploymentStatus::Triggered,
            duration_secs: None,
            error_message: None,
        };
        let _ = send_notifications(notif, &start_event);
    }

    // Respond immediately to GitHub (deployment runs in background thread)
    let _ = request.respond(json_response(200, "Deployment triggered"));

    // Clone values needed for the background thread
    let app_name = app_config.name.clone();
    let branch_owned = branch.to_string();

    // Run deployment in background thread with status tracking
    std::thread::spawn(move || {
        // Acquire deployment lock
        if let Err(e) = DeploymentLock::acquire(&app_name) {
            eprintln!("Failed to acquire lock for {}: {}", app_name, e);
            return;
        }

        let start_time = Instant::now();

        // Run deployment and capture result
        let result = run_deployment(&app_name);

        let duration_secs = start_time.elapsed().as_secs();

        // Update deployment status
        let (status, error_msg) = match &result {
            Ok(_) => {
                println!(
                    "  {} Deployment succeeded for {} ({}s)",
                    console::style("\u{2713}").green(),
                    app_name,
                    duration_secs
                );
                (DeploymentStatus::Success, None)
            }
            Err(e) => {
                eprintln!(
                    "  {} Deployment failed for {}: {}",
                    console::style("\u{2717}").red(),
                    app_name,
                    e
                );
                (DeploymentStatus::Failed, Some(e.to_string()))
            }
        };

        // Update deployment history with final status
        if let Ok(config) = AppConfig::load(&app_name) {
            let path = config.deployments_path();
            if let Ok(mut history) = DeploymentHistory::load(&path) {
                history.update_latest_status(status.clone());
                let _ = history.save(&path);
            }
        }

        // Send completion notification
        if let Some(ref notif) = notification_config {
            let event = DeploymentEvent {
                app_name: app_name.clone(),
                commit_sha,
                commit_message: commit_msg,
                branch: branch_owned,
                triggered_by: pusher,
                status,
                duration_secs: Some(duration_secs),
                error_message: error_msg,
            };
            let _ = send_notifications(notif, &event);
        }

        // Release deployment lock
        DeploymentLock::release(&app_name);
    });
}

/// Runs the deployment synchronously and returns the result.
fn run_deployment(app_name: &str) -> Result<(), AppError> {
    // Get the path to the current executable
    let exe_path = std::env::current_exe()
        .map_err(|e| AppError::Config(format!("Failed to get executable path: {}", e)))?;

    // Run fl update and wait for completion
    let output = Command::new(&exe_path)
        .args(["update", app_name])
        .output()
        .map_err(|e| AppError::Config(format!("Failed to run update command: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let error_msg = if !stderr.is_empty() {
            stderr.to_string()
        } else if !stdout.is_empty() {
            stdout.to_string()
        } else {
            "Deployment failed with unknown error".to_string()
        };
        return Err(AppError::Deploy(error_msg));
    }

    Ok(())
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

/// Gets the host address accessible from Docker containers.
/// Tries to detect the Docker bridge gateway IP at runtime.
/// Falls back to 172.17.0.1 (common default) or host.docker.internal.
fn get_docker_host_address() -> String {
    // Try to get gateway from docker network inspect (works on Linux)
    if let Ok(output) = Command::new("docker")
        .args(["network", "inspect", "bridge", "--format", "{{range .IPAM.Config}}{{.Gateway}}{{end}}"])
        .output()
    {
        let gateway = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !gateway.is_empty() && output.status.success() {
            return gateway;
        }
    }

    // Fallback: try common Linux default, otherwise use host.docker.internal
    // Check if we're likely on Linux by looking for /etc/os-release
    if std::path::Path::new("/etc/os-release").exists() {
        return "172.17.0.1".to_string();
    }

    // macOS/Windows: use host.docker.internal
    "host.docker.internal".to_string()
}

/// Generates Traefik configuration for webhook routing.
fn generate_traefik_webhook_config() -> String {
    let host_address = get_docker_host_address();

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
          - url: "http://{host}:{port}"
"#,
        host = host_address,
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
    // Always bind to 0.0.0.0 so Docker containers can reach the server via bridge gateway
    ui::step("Creating systemd service...");
    let bind_host = "0.0.0.0";

    let service_content = format!(
        r#"[Unit]
Description=Flaase Webhook Server
Documentation=https://github.com/MaxenceMahieux/flaase-cli-rust
After=network.target

[Service]
Type=simple
ExecStart={exe_path} webhook serve --host {host}
Restart=always
RestartSec=5
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=multi-user.target
"#,
        exe_path = exe_path.display(),
        host = bind_host
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

/// Approves a pending deployment.
pub fn approve_deployment(app_name: &str, approval_id: Option<&str>) -> Result<(), AppError> {
    let approval = match PendingApprovalsStore::get(app_name, approval_id)? {
        Some(a) => a,
        None => {
            return Err(AppError::Approval("No pending approval found".into()));
        }
    };

    if approval.is_expired() {
        PendingApprovalsStore::remove(app_name, &approval.approval_id)?;
        return Err(AppError::Approval("Approval request has expired".into()));
    }

    ui::success(&format!(
        "Approved deployment {} for {} (env: {})",
        approval.approval_id, app_name, approval.environment
    ));

    println!(
        "  Commit: {} - {}",
        console::style(&approval.commit_sha).yellow(),
        &approval.commit_message
    );

    // Remove pending approval
    PendingApprovalsStore::remove(app_name, &approval.approval_id)?;

    // Update deployment status
    let config = AppConfig::load(app_name)?;
    let path = config.deployments_path();
    if let Ok(mut history) = DeploymentHistory::load(&path) {
        history.update_latest_status(DeploymentStatus::Triggered);
        let _ = history.save(&path);
    }

    // Trigger deployment
    ui::step("Starting deployment...");
    run_deployment(app_name)?;

    ui::success("Deployment completed successfully!");

    Ok(())
}

/// Rejects a pending deployment.
pub fn reject_deployment(app_name: &str, approval_id: Option<&str>) -> Result<(), AppError> {
    let approval = match PendingApprovalsStore::get(app_name, approval_id)? {
        Some(a) => a,
        None => {
            return Err(AppError::Approval("No pending approval found".into()));
        }
    };

    // Remove pending approval
    PendingApprovalsStore::remove(app_name, &approval.approval_id)?;

    // Update deployment status to failed
    let config = AppConfig::load(app_name)?;
    let path = config.deployments_path();
    if let Ok(mut history) = DeploymentHistory::load(&path) {
        history.update_latest_status(DeploymentStatus::Failed);
        let _ = history.save(&path);
    }

    ui::info(&format!(
        "Rejected deployment {} for {}",
        approval.approval_id, app_name
    ));

    Ok(())
}

/// Lists pending approvals for an app.
pub fn list_pending_approvals(app_name: &str) -> Result<(), AppError> {
    let approvals = PendingApprovalsStore::list(app_name)?;

    if approvals.is_empty() {
        ui::info(&format!("No pending approvals for {}", app_name));
        return Ok(());
    }

    println!("Pending approvals for {}:", console::style(app_name).bold());
    println!();

    for approval in approvals {
        let expires_in = approval.expires_at.signed_duration_since(chrono::Utc::now());
        let expires_str = if expires_in.num_minutes() > 0 {
            format!("{}m", expires_in.num_minutes())
        } else {
            "expired".to_string()
        };

        println!(
            "  {} {} @ {} (env: {}, expires: {})",
            console::style(&approval.approval_id).cyan(),
            console::style(&approval.branch).dim(),
            console::style(&approval.commit_sha).yellow(),
            console::style(&approval.environment).green(),
            if expires_in.num_minutes() > 0 {
                console::style(&expires_str).dim()
            } else {
                console::style(&expires_str).red()
            }
        );
        println!("    {} by {}", &approval.commit_message, &approval.requested_by);
    }

    println!();
    println!(
        "To approve: {}",
        console::style(format!("fl autodeploy approve {} <approval-id>", app_name)).cyan()
    );
    println!(
        "To reject:  {}",
        console::style(format!("fl autodeploy reject {} <approval-id>", app_name)).cyan()
    );

    Ok(())
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
