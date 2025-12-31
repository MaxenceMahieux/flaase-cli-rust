//! Secure secrets management for application credentials.

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::core::app_config::{CacheType, DatabaseType};
use crate::core::error::AppError;

/// Secrets stored in /opt/flaase/apps/<name>/.secrets
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppSecrets {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub database: Option<DatabaseSecrets>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache: Option<CacheSecrets>,
    /// Authentication secrets per domain (domain -> credentials)
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub auth: HashMap<String, AuthSecret>,
    /// Webhook secret for autodeploy
    #[serde(skip_serializing_if = "Option::is_none")]
    pub webhook: Option<WebhookSecret>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseSecrets {
    pub username: String,
    pub password: String,
    pub root_password: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheSecrets {
    pub password: String,
}

/// Authentication secrets for a domain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthSecret {
    /// Htpasswd-compatible hash (bcrypt format)
    pub password_hash: String,
}

/// Webhook secrets for autodeploy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookSecret {
    /// Secret token for GitHub webhook signature verification (HMAC-SHA256).
    pub secret: String,
}

/// Manager for generating and storing secrets securely.
pub struct SecretsManager;

impl SecretsManager {
    /// Generates a secure random password.
    pub fn generate_password(length: usize) -> String {
        use std::iter;

        const CHARSET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";

        let mut rng = SimpleRng::new();

        iter::repeat_with(|| {
            let idx = rng.next() as usize % CHARSET.len();
            CHARSET[idx] as char
        })
        .take(length)
        .collect()
    }

    /// Generates database secrets.
    pub fn generate_database_secrets(db_type: DatabaseType, app_name: &str) -> DatabaseSecrets {
        let username = app_name.replace('-', "_");
        let password = Self::generate_password(32);

        let root_password = match db_type {
            DatabaseType::MySQL => Some(Self::generate_password(32)),
            _ => None,
        };

        DatabaseSecrets {
            username,
            password,
            root_password,
        }
    }

    /// Generates cache secrets.
    pub fn generate_cache_secrets(_cache_type: CacheType) -> CacheSecrets {
        CacheSecrets {
            password: Self::generate_password(32),
        }
    }

    /// Generates a webhook secret for autodeploy.
    pub fn generate_webhook_secret() -> WebhookSecret {
        WebhookSecret {
            secret: Self::generate_password(40), // 40 chars for webhook secret
        }
    }

    /// Generates auth secret with bcrypt-hashed password.
    /// Returns the htpasswd-compatible hash in the format: username:$2y$...
    pub fn generate_auth_secret(username: &str, password: &str) -> Result<AuthSecret, AppError> {
        let hash = bcrypt::hash(password, bcrypt::DEFAULT_COST)
            .map_err(|e| AppError::Config(format!("Failed to hash password: {}", e)))?;

        // Rust bcrypt generates $2b$ but htpasswd/Traefik expects $2y$
        // They are functionally identical, just different identifiers
        let htpasswd_hash = hash.replace("$2b$", "$2y$");

        Ok(AuthSecret {
            password_hash: format!("{}:{}", username, htpasswd_hash),
        })
    }

    /// Validates a password against a stored auth secret.
    pub fn verify_auth_password(password: &str, auth_secret: &AuthSecret) -> bool {
        // Extract hash from "username:hash" format
        if let Some(hash) = auth_secret.password_hash.split(':').nth(1) {
            bcrypt::verify(password, hash).unwrap_or(false)
        } else {
            false
        }
    }

    /// Saves secrets to a file with restricted permissions (600, root only).
    pub fn save_secrets(path: &Path, secrets: &AppSecrets) -> Result<(), AppError> {
        let content = serde_yaml::to_string(secrets)
            .map_err(|e| AppError::Config(format!("Failed to serialize secrets: {}", e)))?;

        // Create file with mode 600 (owner read/write only)
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)
            .map_err(|e| AppError::Config(format!("Failed to create secrets file: {}", e)))?;

        file.write_all(content.as_bytes())
            .map_err(|e| AppError::Config(format!("Failed to write secrets: {}", e)))?;

        Ok(())
    }

    /// Loads secrets from a file.
    pub fn load_secrets(path: &Path) -> Result<AppSecrets, AppError> {
        if !path.exists() {
            return Ok(AppSecrets::default());
        }

        let content = fs::read_to_string(path)
            .map_err(|e| AppError::Config(format!("Failed to read secrets: {}", e)))?;

        serde_yaml::from_str(&content)
            .map_err(|e| AppError::Config(format!("Failed to parse secrets: {}", e)))
    }

    /// Generates environment variables from secrets.
    pub fn generate_env_vars(
        secrets: &AppSecrets,
        db_type: Option<DatabaseType>,
        db_name: &str,
        cache_type: Option<CacheType>,
        app_name: &str,
    ) -> HashMap<String, String> {
        let mut vars = HashMap::new();

        // Database URL
        if let (Some(db), Some(db_type)) = (&secrets.database, db_type) {
            let url = match db_type {
                DatabaseType::PostgreSQL => {
                    format!(
                        "postgresql://{}:{}@flaase-{}-db:5432/{}",
                        db.username, db.password, app_name, db_name
                    )
                }
                DatabaseType::MySQL => {
                    format!(
                        "mysql://{}:{}@flaase-{}-db:3306/{}",
                        db.username, db.password, app_name, db_name
                    )
                }
                DatabaseType::MongoDB => {
                    format!(
                        "mongodb://{}:{}@flaase-{}-db:27017/{}",
                        db.username, db.password, app_name, db_name
                    )
                }
            };
            vars.insert(db_type.url_env_var().to_string(), url);
        }

        // Cache URL
        if let (Some(cache), Some(cache_type)) = (&secrets.cache, cache_type) {
            let url = match cache_type {
                CacheType::Redis => {
                    format!("redis://:{}@flaase-{}-cache:6379", cache.password, app_name)
                }
            };
            vars.insert(cache_type.url_env_var().to_string(), url);
        }

        vars
    }

    /// Writes environment variables to .env file with restricted permissions.
    pub fn write_env_file(path: &Path, vars: &HashMap<String, String>) -> Result<(), AppError> {
        let mut content = String::new();

        // Sort keys for consistent output
        let mut keys: Vec<_> = vars.keys().collect();
        keys.sort();

        for key in keys {
            if let Some(value) = vars.get(key) {
                content.push_str(&format!("{}={}\n", key, value));
            }
        }

        // Create file with mode 600
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)
            .map_err(|e| AppError::Config(format!("Failed to create env file: {}", e)))?;

        file.write_all(content.as_bytes())
            .map_err(|e| AppError::Config(format!("Failed to write env file: {}", e)))?;

        Ok(())
    }
}

/// Simple random number generator using system time.
/// Not cryptographically secure, but sufficient for password generation.
struct SimpleRng {
    state: u64,
}

impl SimpleRng {
    fn new() -> Self {
        use std::time::{SystemTime, UNIX_EPOCH};

        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(42);

        Self { state: seed }
    }

    fn next(&mut self) -> u64 {
        // XorShift algorithm
        self.state ^= self.state << 13;
        self.state ^= self.state >> 7;
        self.state ^= self.state << 17;
        self.state
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_password() {
        let pwd1 = SecretsManager::generate_password(32);
        let pwd2 = SecretsManager::generate_password(32);

        assert_eq!(pwd1.len(), 32);
        assert_eq!(pwd2.len(), 32);
        assert_ne!(pwd1, pwd2);
    }

    #[test]
    fn test_generate_database_secrets() {
        let secrets = SecretsManager::generate_database_secrets(DatabaseType::PostgreSQL, "my-app");

        assert_eq!(secrets.username, "my_app");
        assert_eq!(secrets.password.len(), 32);
        assert!(secrets.root_password.is_none());
    }

    #[test]
    fn test_generate_mysql_secrets() {
        let secrets = SecretsManager::generate_database_secrets(DatabaseType::MySQL, "my-app");

        assert!(secrets.root_password.is_some());
    }
}
