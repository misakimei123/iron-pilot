//! Infrastructure and interface adapter boundary.
//!
//! Adapters may depend inward on application and domain contracts when those
//! contracts exist. Neither inward crate may depend on this crate.

#![forbid(unsafe_code)]

mod config;
mod persistence;
mod resources;

pub use config::{
    CONFIG_PATH_ENV, ENVIRONMENT_FINGERPRINT_ENV, ENVIRONMENT_NAME_ENV, LoadConfigError,
    load_startup_config, load_startup_config_from_vars, parse_and_validate_yaml, parse_yaml_config,
};
pub use persistence::{
    AuditRow, InstanceLease, LeaseAcquireError, PendingOutboxRow, SqliteRepository, StorageError,
};
pub use resources::{ProcessResourceSampler, ResourceSampleError};
