//! Application orchestration boundary.
//!
//! The domain crate must not depend on this crate.

#![forbid(unsafe_code)]

mod config;

pub use config::{
    CONFIG_SCHEMA_VERSION_V1, ConfigValidationError, DeploymentEnvironment, EnvironmentFingerprint,
    ExecutionMode, LlmLimits, MARKET_FEATURES_VERSION_V1, MarketLimits, PermissionConfig,
    QueueLimits, RISK_RULES_VERSION_V1, RuntimeConfig, RuntimeLimits, StartupIdentity,
    StorageLimits, ValidatedRuntimeConfig, VersionConfig,
};
