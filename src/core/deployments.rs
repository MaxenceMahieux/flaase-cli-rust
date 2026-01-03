//! Deployment history tracking for autodeploy.

use std::path::Path;

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::core::error::AppError;

/// Maximum number of deployments to keep in history.
const MAX_HISTORY_SIZE: usize = 20;

/// Deployment history for an app.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DeploymentHistory {
    pub deployments: Vec<DeploymentRecord>,
}

/// A single deployment record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentRecord {
    /// Unique deployment ID.
    #[serde(default = "DeploymentRecord::generate_id")]
    pub deployment_id: String,
    /// When the deployment was triggered.
    pub timestamp: DateTime<Utc>,
    /// Git commit SHA (short form).
    pub commit_sha: String,
    /// Git commit message (first line).
    pub commit_message: String,
    /// Branch that was deployed.
    pub branch: String,
    /// Who triggered the deployment (GitHub username).
    pub triggered_by: String,
    /// Deployment status.
    pub status: DeploymentStatus,
    /// Source of the deployment.
    pub source: DeploymentSource,
    /// Docker image tag used for this deployment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_tag: Option<String>,
    /// Environment deployed to (e.g., "staging", "production").
    #[serde(default = "DeploymentRecord::default_environment")]
    pub environment: String,
    /// Whether tests passed for this deployment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tests_passed: Option<bool>,
    /// Duration of the deployment in seconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_seconds: Option<u64>,
    /// If this was a rollback, the deployment ID we rolled back from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rollback_from: Option<String>,
}

impl DeploymentRecord {
    fn generate_id() -> String {
        use std::time::{SystemTime, UNIX_EPOCH};
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        format!("dep-{:x}", timestamp)
    }

    fn default_environment() -> String {
        "production".to_string()
    }
}

/// Deployment status.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum DeploymentStatus {
    /// Deployment was triggered successfully.
    Triggered,
    /// Awaiting manual approval.
    PendingApproval,
    /// Deployment completed successfully.
    Success,
    /// Deployment failed.
    Failed,
    /// Deployment was rolled back.
    RolledBack,
}

/// Source of the deployment.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DeploymentSource {
    /// Triggered by webhook (autodeploy).
    Webhook,
    /// Triggered manually via CLI.
    Manual,
    /// Rollback from a previous deployment.
    Rollback,
}

/// Pending approval for a deployment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingApproval {
    /// Unique approval ID.
    pub approval_id: String,
    /// App name.
    pub app_name: String,
    /// Commit SHA to deploy.
    pub commit_sha: String,
    /// Commit message.
    pub commit_message: String,
    /// Branch.
    pub branch: String,
    /// Target environment.
    pub environment: String,
    /// Who requested the deployment.
    pub requested_by: String,
    /// When the approval was requested.
    pub requested_at: DateTime<Utc>,
    /// When the approval expires.
    pub expires_at: DateTime<Utc>,
    /// Approval token for verification.
    pub approval_token: String,
}

impl PendingApproval {
    /// Creates a new pending approval.
    pub fn new(
        app_name: &str,
        commit_sha: &str,
        commit_message: &str,
        branch: &str,
        environment: &str,
        requested_by: &str,
        timeout_minutes: u64,
    ) -> Self {
        let now = Utc::now();
        Self {
            approval_id: format!("apr-{:x}", now.timestamp_millis()),
            app_name: app_name.to_string(),
            commit_sha: commit_sha.to_string(),
            commit_message: commit_message.to_string(),
            branch: branch.to_string(),
            environment: environment.to_string(),
            requested_by: requested_by.to_string(),
            requested_at: now,
            expires_at: now + Duration::minutes(timeout_minutes as i64),
            approval_token: generate_approval_token(),
        }
    }

    /// Checks if the approval has expired.
    pub fn is_expired(&self) -> bool {
        Utc::now() > self.expires_at
    }
}

/// Generates a random approval token.
fn generate_approval_token() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{:x}", timestamp)
}

impl DeploymentHistory {
    /// Loads deployment history from a file.
    pub fn load(path: &Path) -> Result<Self, AppError> {
        if !path.exists() {
            return Ok(Self::default());
        }

        let content = std::fs::read_to_string(path)
            .map_err(|e| AppError::Config(format!("Failed to read deployments: {}", e)))?;

        serde_json::from_str(&content)
            .map_err(|e| AppError::Config(format!("Failed to parse deployments: {}", e)))
    }

    /// Saves deployment history to a file.
    pub fn save(&self, path: &Path) -> Result<(), AppError> {
        let content = serde_json::to_string_pretty(self)
            .map_err(|e| AppError::Config(format!("Failed to serialize deployments: {}", e)))?;

        std::fs::write(path, content)
            .map_err(|e| AppError::Config(format!("Failed to write deployments: {}", e)))
    }

    /// Adds a new deployment record to history.
    pub fn add(&mut self, record: DeploymentRecord) {
        self.deployments.insert(0, record);

        // Trim to max size
        if self.deployments.len() > MAX_HISTORY_SIZE {
            self.deployments.truncate(MAX_HISTORY_SIZE);
        }
    }

    /// Updates the status of the most recent deployment.
    pub fn update_latest_status(&mut self, status: DeploymentStatus) {
        if let Some(record) = self.deployments.first_mut() {
            record.status = status;
        }
    }

    /// Returns the most recent deployments (up to limit).
    pub fn recent(&self, limit: usize) -> &[DeploymentRecord] {
        let end = limit.min(self.deployments.len());
        &self.deployments[..end]
    }
}

impl DeploymentRecord {
    /// Creates a new deployment record for a webhook-triggered deployment.
    pub fn from_webhook(
        commit_sha: &str,
        commit_message: &str,
        branch: &str,
        triggered_by: &str,
        environment: &str,
    ) -> Self {
        Self {
            deployment_id: Self::generate_id(),
            timestamp: Utc::now(),
            commit_sha: commit_sha.to_string(),
            commit_message: commit_message.to_string(),
            branch: branch.to_string(),
            triggered_by: triggered_by.to_string(),
            status: DeploymentStatus::Triggered,
            source: DeploymentSource::Webhook,
            image_tag: None,
            environment: environment.to_string(),
            tests_passed: None,
            duration_seconds: None,
            rollback_from: None,
        }
    }

    /// Creates a new deployment record for a manual deployment.
    pub fn manual(commit_sha: &str, commit_message: &str, branch: &str) -> Self {
        Self {
            deployment_id: Self::generate_id(),
            timestamp: Utc::now(),
            commit_sha: commit_sha.to_string(),
            commit_message: commit_message.to_string(),
            branch: branch.to_string(),
            triggered_by: "cli".to_string(),
            status: DeploymentStatus::Triggered,
            source: DeploymentSource::Manual,
            image_tag: None,
            environment: "production".to_string(),
            tests_passed: None,
            duration_seconds: None,
            rollback_from: None,
        }
    }

    /// Creates a new deployment record for a rollback.
    pub fn rollback(
        commit_sha: &str,
        commit_message: &str,
        branch: &str,
        from_deployment_id: &str,
    ) -> Self {
        Self {
            deployment_id: Self::generate_id(),
            timestamp: Utc::now(),
            commit_sha: commit_sha.to_string(),
            commit_message: commit_message.to_string(),
            branch: branch.to_string(),
            triggered_by: "cli".to_string(),
            status: DeploymentStatus::Triggered,
            source: DeploymentSource::Rollback,
            image_tag: None,
            environment: "production".to_string(),
            tests_passed: None,
            duration_seconds: None,
            rollback_from: Some(from_deployment_id.to_string()),
        }
    }

    /// Sets the image tag for this deployment.
    pub fn with_image_tag(mut self, tag: &str) -> Self {
        self.image_tag = Some(tag.to_string());
        self
    }

    /// Sets the tests result.
    pub fn with_tests_result(mut self, passed: bool) -> Self {
        self.tests_passed = Some(passed);
        self
    }

    /// Sets the duration.
    pub fn with_duration(mut self, seconds: u64) -> Self {
        self.duration_seconds = Some(seconds);
        self
    }
}

impl std::fmt::Display for DeploymentStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeploymentStatus::Triggered => write!(f, "triggered"),
            DeploymentStatus::PendingApproval => write!(f, "pending_approval"),
            DeploymentStatus::Success => write!(f, "success"),
            DeploymentStatus::Failed => write!(f, "failed"),
            DeploymentStatus::RolledBack => write!(f, "rolled_back"),
        }
    }
}
