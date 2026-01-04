pub mod app_config;
pub mod config;
pub mod context;
pub mod deploy;
pub mod deployments;
pub mod env;
pub mod error;
pub mod notifications;
pub mod registry;
pub mod secrets;
pub mod stack_detection;

pub use app_config::{
    AppConfig, ApprovalConfig, AutodeployConfig, BuildConfig, CacheConfig, CacheType,
    DatabaseConfig, DatabaseType, DeploymentType, DiscordNotificationConfig, DomainAuth,
    DomainConfig, EnvironmentConfig, Framework, HealthCheckConfig, HookCommand, HooksConfig,
    ImageConfig, NotificationConfig, NotificationEvents, PackageManager, RateLimitConfig,
    Registry, RegistryCredentials, RollbackConfig, SlackNotificationConfig, Stack, StackConfig,
    TestConfig, VolumeMount,
};
pub use stack_detection::{detect_stack, DetectionConfidence, DetectionResult};
pub use registry::{detect_default_port, parse_image_reference, pull_image};
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
