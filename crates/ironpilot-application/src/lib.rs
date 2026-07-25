//! Application orchestration boundary.
//!
//! The domain crate must not depend on this crate.

#![forbid(unsafe_code)]

mod config;
mod persistence;
mod runtime;

pub use config::{
    CONFIG_SCHEMA_VERSION_V2, ConfigValidationError, DeploymentEnvironment, EnvironmentFingerprint,
    ExecutionMode, LlmLimits, MarketLimits, PermissionConfig, QueueLimits, RuntimeConfig,
    RuntimeLimits, StartupIdentity, StorageLimits, ValidatedRuntimeConfig, VersionConfig,
};
pub use ironpilot_domain::{
    AI_DECISION_CONTEXT_SCHEMA_VERSION_V1, AI_TRADING_PLAN_SCHEMA_VERSION_V3,
    MARKET_FEATURES_VERSION_V1,
};
pub use persistence::{
    AuditEntry, OutboxMessage, PersistedSystemState, SystemStateChange, UnixMillis,
    ValidationError as PersistenceValidationError,
};
pub use runtime::{
    BoundedQueueReceiver, BoundedQueueSender, HealthIssue, HealthLevel, HealthMonitor,
    HealthSnapshot, QueueClass, QueueSendError, QueueSnapshot, ResourceSample, RuntimeEvent,
    RuntimeSupervisor, ShutdownReport, ShutdownSignal, SpawnError, TaskFailure, TaskImportance,
};
