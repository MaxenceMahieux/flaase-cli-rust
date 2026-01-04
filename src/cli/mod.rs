use clap::{Parser, Subcommand};

pub mod app;
pub mod auth;
pub mod autodeploy;
pub mod deploy;
pub mod domain;
pub mod env;
pub mod logs;
pub mod server;
pub mod server_status;
pub mod status;
pub mod webhook;

/// Flaase CLI - Simplified VPS deployment
#[derive(Parser)]
#[command(
    name = "flaase",
    bin_name = "fl",
    version,
    about = "A CLI tool for simplified VPS deployment",
    long_about = None,
    after_help = "For more information, visit: https://github.com/MaxenceMahieux/flaase-cli-rust"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Enable verbose output
    #[arg(short, long, global = true)]
    pub verbose: bool,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Initialize and manage server configuration
    Server {
        #[command(subcommand)]
        command: ServerCommands,
    },

    /// Initialize a new app configuration
    Init,

    /// Show status of all deployed apps
    Status,

    /// Deploy an app
    Deploy {
        /// Name of the app to deploy
        app: String,
    },

    /// Update a deployed app
    Update {
        /// Name of the app to update
        app: String,
    },

    /// Stop a running app
    Stop {
        /// Name of the app to stop
        app: String,
    },

    /// Start a stopped app
    Start {
        /// Name of the app to start
        app: String,
    },

    /// Restart an app
    Restart {
        /// Name of the app to restart
        app: String,
    },

    /// Remove an app completely
    Destroy {
        /// Name of the app to destroy
        app: String,
    },

    /// Rollback to a previous deployment
    Rollback {
        /// Name of the app to rollback
        app: String,

        /// Target version (commit SHA). If not provided, rolls back to previous version
        #[arg(long)]
        to: Option<String>,

        /// List available versions for rollback
        #[arg(long, short)]
        list: bool,
    },

    /// View app logs
    Logs {
        /// Name of the app
        app: String,

        /// Follow log output in real-time (default behavior)
        #[arg(short, long)]
        follow: bool,

        /// Don't follow, just show recent logs and exit
        #[arg(long)]
        no_follow: bool,

        /// Number of lines to show
        #[arg(short = 'n', long, default_value = "100")]
        lines: u32,

        /// Filter by service: app, database, cache, or all
        #[arg(short, long, default_value = "app")]
        service: String,

        /// Show logs since timestamp or duration (e.g., "1h", "30m", "2024-01-15")
        #[arg(long)]
        since: Option<String>,
    },

    /// Manage environment variables
    Env {
        #[command(subcommand)]
        command: EnvCommands,
    },

    /// Manage domains
    Domain {
        #[command(subcommand)]
        command: DomainCommands,
    },

    /// Manage auto-deployment
    Autodeploy {
        #[command(subcommand)]
        command: AutodeployCommands,
    },

    /// Manage HTTP Basic Auth for domains
    Auth {
        #[command(subcommand)]
        command: AuthCommands,
    },

    /// Webhook server for autodeploy
    Webhook {
        #[command(subcommand)]
        command: WebhookCommands,
    },
}

#[derive(Subcommand)]
pub enum ServerCommands {
    /// Initialize server for deployments
    Init {
        /// Run without making any changes
        #[arg(long)]
        dry_run: bool,
    },

    /// Show server health status
    Status,
}

#[derive(Subcommand)]
pub enum EnvCommands {
    /// List environment variables
    List {
        /// Name of the app
        app: String,

        /// Show actual values (requires confirmation)
        #[arg(long)]
        show: bool,

        /// Target environment (default: production)
        #[arg(long, short)]
        env: Option<String>,
    },

    /// Set environment variable(s)
    Set {
        /// Name of the app
        app: String,

        /// KEY=value pairs to set
        #[arg(required = true)]
        vars: Vec<String>,

        /// Target environment (default: production)
        #[arg(long, short)]
        env: Option<String>,
    },

    /// Remove an environment variable
    Remove {
        /// Name of the app
        app: String,

        /// Key to remove
        key: String,

        /// Target environment (default: production)
        #[arg(long, short)]
        env: Option<String>,
    },

    /// Edit environment variables in your editor
    Edit {
        /// Name of the app
        app: String,

        /// Target environment (default: production)
        #[arg(long, short)]
        env: Option<String>,
    },

    /// Copy environment variables from one environment to another
    Copy {
        /// Name of the app
        app: String,

        /// Source environment
        from: String,

        /// Target environment
        to: String,
    },

    /// List all environments with their variable counts
    Envs {
        /// Name of the app
        app: String,
    },
}

#[derive(Subcommand)]
pub enum DomainCommands {
    /// List domains for an app
    List {
        /// Name of the app
        app: String,
    },

    /// Add a domain to an app
    Add {
        /// Name of the app
        app: String,

        /// Domain to add (e.g., api.example.com)
        domain: String,

        /// Skip DNS verification
        #[arg(long)]
        skip_dns_check: bool,
    },

    /// Remove a domain from an app
    Remove {
        /// Name of the app
        app: String,

        /// Domain to remove
        domain: String,
    },
}

#[derive(Subcommand)]
pub enum AutodeployCommands {
    /// Enable auto-deployment via GitHub webhook
    Enable {
        /// Name of the app
        app: String,

        /// Branch to watch for deployments (default: main)
        #[arg(long, short)]
        branch: Option<String>,
    },

    /// Disable auto-deployment
    Disable {
        /// Name of the app
        app: String,
    },

    /// Show auto-deployment status
    Status {
        /// Name of the app
        app: String,
    },

    /// Show webhook secret (for reconfiguration)
    Secret {
        /// Name of the app
        app: String,
    },

    /// Regenerate webhook secret
    Regenerate {
        /// Name of the app
        app: String,
    },

    /// View deployment logs
    Logs {
        /// Name of the app
        app: String,

        /// Number of recent deployments to show (default: 10)
        #[arg(long, short, default_value = "10")]
        limit: usize,
    },

    /// Configure notifications (Slack/Discord)
    #[command(subcommand)]
    Notify(NotifyCommands),

    /// Configure rate limiting
    RateLimit {
        /// Name of the app
        app: String,

        /// Enable rate limiting
        #[arg(long)]
        enable: bool,

        /// Disable rate limiting
        #[arg(long)]
        disable: bool,

        /// Maximum deployments allowed in the time window
        #[arg(long)]
        max_deploys: Option<u32>,

        /// Time window in seconds
        #[arg(long)]
        window: Option<u64>,
    },

    /// Configure test execution before deployment
    Test {
        /// Name of the app
        app: String,

        /// Enable test execution
        #[arg(long)]
        enable: bool,

        /// Disable test execution
        #[arg(long)]
        disable: bool,

        /// Test command to run (e.g., "npm test", "cargo test")
        #[arg(long)]
        command: Option<String>,

        /// Timeout in seconds (default: 300)
        #[arg(long)]
        timeout: Option<u64>,

        /// Whether to fail deployment on test errors
        #[arg(long)]
        fail_on_error: Option<bool>,
    },

    /// Manage deployment hooks
    #[command(subcommand)]
    Hooks(HooksCommands),

    /// Configure rollback settings
    RollbackConfig {
        /// Name of the app
        app: String,

        /// Enable rollback
        #[arg(long)]
        enable: bool,

        /// Disable rollback
        #[arg(long)]
        disable: bool,

        /// Number of versions to keep for rollback (default: 3)
        #[arg(long)]
        keep_versions: Option<u32>,

        /// Enable auto-rollback on deployment failure
        #[arg(long)]
        auto_rollback: Option<bool>,
    },

    /// Manage deployment environments
    #[command(subcommand)]
    Env(EnvDeployCommands),

    /// Manage approval gates
    #[command(subcommand)]
    Approval(ApprovalCommands),

    /// Configure Docker build settings
    Build {
        /// Name of the app
        app: String,

        /// Enable/disable Docker build cache
        #[arg(long)]
        cache: Option<bool>,

        /// Enable/disable BuildKit
        #[arg(long)]
        buildkit: Option<bool>,

        /// Docker registry to use for cache (e.g., "registry.example.com/myapp")
        #[arg(long)]
        cache_from: Option<String>,
    },

    /// Configure blue-green deployment (zero-downtime)
    BlueGreen {
        /// Name of the app
        app: String,

        /// Enable blue-green deployment
        #[arg(long)]
        enable: bool,

        /// Disable blue-green deployment
        #[arg(long)]
        disable: bool,

        /// How long to keep old container running after switch (seconds, for instant rollback)
        #[arg(long)]
        keep_old: Option<u64>,

        /// Disable auto-cleanup of old container
        #[arg(long)]
        no_auto_cleanup: bool,
    },
}

#[derive(Subcommand)]
pub enum HooksCommands {
    /// List all hooks
    List {
        /// Name of the app
        app: String,
    },

    /// Add a hook
    Add {
        /// Name of the app
        app: String,

        /// Hook phase: pre_build, pre_deploy, post_deploy, on_failure
        phase: String,

        /// Hook name
        name: String,

        /// Command to run
        command: String,

        /// Timeout in seconds (default: 60)
        #[arg(long, default_value = "60")]
        timeout: u64,

        /// Fail deployment if hook fails
        #[arg(long)]
        required: bool,

        /// Run inside the app container instead of on host
        #[arg(long)]
        in_container: bool,
    },

    /// Remove a hook
    Remove {
        /// Name of the app
        app: String,

        /// Hook phase: pre_build, pre_deploy, post_deploy, on_failure
        phase: String,

        /// Hook name
        name: String,
    },
}

#[derive(Subcommand)]
pub enum EnvDeployCommands {
    /// List environments
    List {
        /// Name of the app
        app: String,
    },

    /// Add an environment
    Add {
        /// Name of the app
        app: String,

        /// Environment name (e.g., "staging", "production")
        name: String,

        /// Branch that triggers this environment
        branch: String,

        /// Auto-deploy when branch is pushed
        #[arg(long)]
        auto_deploy: bool,

        /// Domains for this environment (can specify multiple)
        #[arg(long)]
        domain: Option<Vec<String>>,
    },

    /// Remove an environment
    Remove {
        /// Name of the app
        app: String,

        /// Environment name
        name: String,
    },
}

#[derive(Subcommand)]
pub enum ApprovalCommands {
    /// Configure approval settings
    Config {
        /// Name of the app
        app: String,

        /// Enable approval gates
        #[arg(long)]
        enable: bool,

        /// Disable approval gates
        #[arg(long)]
        disable: bool,

        /// Approval timeout in minutes (default: 60)
        #[arg(long)]
        timeout: Option<u64>,
    },

    /// List pending approvals
    Pending {
        /// Name of the app
        app: String,
    },

    /// Approve a pending deployment
    Approve {
        /// Name of the app
        app: String,

        /// Approval ID (optional, uses latest if not provided)
        approval_id: Option<String>,
    },

    /// Reject a pending deployment
    Reject {
        /// Name of the app
        app: String,

        /// Approval ID (optional, uses latest if not provided)
        approval_id: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum NotifyCommands {
    /// Show notification configuration
    Status {
        /// Name of the app
        app: String,
    },

    /// Enable notifications
    Enable {
        /// Name of the app
        app: String,
    },

    /// Disable notifications
    Disable {
        /// Name of the app
        app: String,
    },

    /// Configure Slack notifications
    Slack {
        /// Name of the app
        app: String,

        /// Slack webhook URL
        #[arg(long)]
        webhook_url: Option<String>,

        /// Channel override (optional)
        #[arg(long)]
        channel: Option<String>,

        /// Username override (optional)
        #[arg(long)]
        username: Option<String>,

        /// Remove Slack configuration
        #[arg(long)]
        remove: bool,
    },

    /// Configure Discord notifications
    Discord {
        /// Name of the app
        app: String,

        /// Discord webhook URL
        #[arg(long)]
        webhook_url: Option<String>,

        /// Username override (optional)
        #[arg(long)]
        username: Option<String>,

        /// Remove Discord configuration
        #[arg(long)]
        remove: bool,
    },

    /// Configure Email notifications (SMTP)
    Email {
        /// Name of the app
        app: String,

        /// SMTP server host
        #[arg(long)]
        smtp_host: Option<String>,

        /// SMTP server port (default: 587)
        #[arg(long)]
        smtp_port: Option<u16>,

        /// SMTP username
        #[arg(long)]
        smtp_user: Option<String>,

        /// SMTP password
        #[arg(long)]
        smtp_password: Option<String>,

        /// Sender email address
        #[arg(long)]
        from_email: Option<String>,

        /// Sender name (optional)
        #[arg(long)]
        from_name: Option<String>,

        /// Recipient email addresses (comma-separated)
        #[arg(long)]
        to_emails: Option<String>,

        /// Use STARTTLS (default: true)
        #[arg(long)]
        starttls: Option<bool>,

        /// Remove email configuration
        #[arg(long)]
        remove: bool,
    },

    /// Configure which events trigger notifications
    Events {
        /// Name of the app
        app: String,

        /// Notify on deployment start
        #[arg(long)]
        on_start: Option<bool>,

        /// Notify on deployment success
        #[arg(long)]
        on_success: Option<bool>,

        /// Notify on deployment failure
        #[arg(long)]
        on_failure: Option<bool>,
    },

    /// Send a test notification
    Test {
        /// Name of the app
        app: String,
    },
}

#[derive(Subcommand)]
pub enum AuthCommands {
    /// List auth status for all domains
    List {
        /// Name of the app
        app: String,
    },

    /// Add Basic Auth to a domain
    Add {
        /// Name of the app
        app: String,

        /// Domain to protect
        domain: String,

        /// Username (interactive if not provided)
        #[arg(long, short)]
        user: Option<String>,

        /// Password (interactive if not provided)
        #[arg(long, short)]
        password: Option<String>,
    },

    /// Remove Basic Auth from a domain
    Remove {
        /// Name of the app
        app: String,

        /// Domain to unprotect
        domain: String,
    },

    /// Update credentials for a domain
    Update {
        /// Name of the app
        app: String,

        /// Domain to update
        domain: String,

        /// New username (interactive if not provided)
        #[arg(long, short)]
        user: Option<String>,

        /// New password (interactive if not provided)
        #[arg(long, short)]
        password: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum WebhookCommands {
    /// Start the webhook server
    Serve {
        /// Port to listen on
        #[arg(long, short, default_value = "9876")]
        port: u16,

        /// Host to bind to
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
    },

    /// Install webhook server as a systemd service
    Install,

    /// Uninstall the systemd service
    Uninstall,

    /// Show webhook server status
    Status,
}
