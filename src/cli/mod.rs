use clap::{Parser, Subcommand};

pub mod app;
pub mod auth;
pub mod autodeploy;
pub mod deploy;
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

    /// View app logs
    Logs {
        /// Name of the app
        app: String,

        /// Follow log output
        #[arg(short, long)]
        follow: bool,

        /// Number of lines to show
        #[arg(short, long, default_value = "100")]
        lines: u32,
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
    },

    /// Set environment variable(s)
    Set {
        /// Name of the app
        app: String,

        /// KEY=value pairs to set
        #[arg(required = true)]
        vars: Vec<String>,
    },

    /// Remove an environment variable
    Remove {
        /// Name of the app
        app: String,

        /// Key to remove
        key: String,
    },

    /// Edit environment variables in your editor
    Edit {
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

        /// Domain to add
        domain: String,
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
