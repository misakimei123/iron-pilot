//! Application orchestration boundary.
//!
//! The domain crate must not depend on this crate.

#![forbid(unsafe_code)]

mod config;
mod persistence;
mod runtime;

pub use config::{
    CONFIG_SCHEMA_VERSION_V1, ConfigValidationError, DeploymentEnvironment, EnvironmentFingerprint,
    ExecutionMode, LlmLimits, MARKET_FEATURES_VERSION_V1, MarketLimits, PermissionConfig,
    QueueLimits, RuntimeConfig, RuntimeLimits, StartupIdentity, StorageLimits,
    ValidatedRuntimeConfig, VersionConfig,
};
pub use ironpilot_domain::RISK_RULES_VERSION_V1;
pub use persistence::{
    AuditEntry, OutboxMessage, PersistedSystemState, SystemStateChange, UnixMillis,
    ValidationError as PersistenceValidationError,
};
pub use runtime::{
    BoundedQueueReceiver, BoundedQueueSender, HealthIssue, HealthLevel, HealthMonitor,
    HealthSnapshot, QueueClass, QueueSendError, QueueSnapshot, ResourceSample, RuntimeEvent,
    RuntimeSupervisor, ShutdownReport, ShutdownSignal, SpawnError, TaskFailure, TaskImportance,
};
