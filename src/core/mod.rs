pub mod app_config;
pub mod config;
pub mod context;
pub mod deploy;
pub mod deployments;
pub mod env;
pub mod error;
pub mod notifications;
pub mod secrets;
pub mod stack_detection;

pub use app_config::{
    AppConfig, ApprovalConfig, AutodeployConfig, BuildConfig, CacheConfig, CacheType,
    DatabaseConfig, DatabaseType, DiscordNotificationConfig, DomainAuth, DomainConfig,
    EnvironmentConfig, Framework, HealthCheckConfig, HookCommand, HooksConfig, NotificationConfig,
    NotificationEvents, PackageManager, RateLimitConfig, RollbackConfig, SlackNotificationConfig,
    Stack, StackConfig, TestConfig,
};
pub use stack_detection::{detect_stack, DetectionConfidence, DetectionResult};
pub use config::{
    ExistingComponentAction, ServerConfig, FLAASE_APPS_PATH, FLAASE_BASE_PATH, FLAASE_CONFIG_PATH,
    FLAASE_TRAEFIK_DYNAMIC_PATH, FLAASE_TRAEFIK_PATH,
};
pub use context::{CommandOutput, ExecutionContext};
pub use deploy::{format_duration, DeployResult, Deployer, DeployStep, UpdateResult};
pub use deployments::{
    DeploymentHistory, DeploymentRecord, DeploymentSource, DeploymentStatus, PendingApproval,
};
pub use env::{EnvManager, EnvSource, EnvVar};
pub use error::AppError;
pub use notifications::{send_notifications, test_notification, DeploymentEvent};
pub use secrets::{AppSecrets, AuthSecret, SecretsManager, WebhookSecret};
