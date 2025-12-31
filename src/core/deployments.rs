//! Deployment history tracking for autodeploy.

use std::path::Path;

use chrono::{DateTime, Utc};
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
}

/// Deployment status.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum DeploymentStatus {
    /// Deployment was triggered successfully.
    Triggered,
    /// Deployment completed successfully.
    Success,
    /// Deployment failed.
    Failed,
}

/// Source of the deployment.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DeploymentSource {
    /// Triggered by webhook (autodeploy).
    Webhook,
    /// Triggered manually via CLI.
    Manual,
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
    ) -> Self {
        Self {
            timestamp: Utc::now(),
            commit_sha: commit_sha.to_string(),
            commit_message: commit_message.to_string(),
            branch: branch.to_string(),
            triggered_by: triggered_by.to_string(),
            status: DeploymentStatus::Triggered,
            source: DeploymentSource::Webhook,
        }
    }

    /// Creates a new deployment record for a manual deployment.
    pub fn manual(commit_sha: &str, commit_message: &str, branch: &str) -> Self {
        Self {
            timestamp: Utc::now(),
            commit_sha: commit_sha.to_string(),
            commit_message: commit_message.to_string(),
            branch: branch.to_string(),
            triggered_by: "cli".to_string(),
            status: DeploymentStatus::Triggered,
            source: DeploymentSource::Manual,
        }
    }
}

impl std::fmt::Display for DeploymentStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeploymentStatus::Triggered => write!(f, "triggered"),
            DeploymentStatus::Success => write!(f, "success"),
            DeploymentStatus::Failed => write!(f, "failed"),
        }
    }
}
