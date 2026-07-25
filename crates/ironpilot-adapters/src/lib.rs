//! Infrastructure and interface adapter boundary.
//!
//! Adapters may depend inward on application and domain contracts when those
//! contracts exist. Neither inward crate may depend on this crate.

#![forbid(unsafe_code)]

mod bybit_public;
mod bybit_public_websocket;
mod config;
mod persistence;
mod resources;

pub use bybit_public::{
    BYBIT_MAINNET_PUBLIC_REST_URL, BybitPublicRestClient, BybitPublicRestError,
    DEFAULT_INSTRUMENT_RULES_TTL, INSTRUMENT_RULES_HASH_SCHEMA_V1, MAX_INSTRUMENT_RULES_TTL,
    PublicRestErrorKind,
};
pub use bybit_public_websocket::{
    BYBIT_MAINNET_SPOT_PUBLIC_WEBSOCKET_URL, BestBookSnapshot, BybitMarketEvent,
    BybitMarketStreamError, BybitPublicWebSocketClient, FeedFreshnessRegistry,
    FeedFreshnessSnapshot, KlineInterval, KlineUpdate, MarketStreamErrorKind, SubscriptionPlan,
    SymbolFreshness, TopicFreshness,
};
pub use config::{
    CONFIG_PATH_ENV, ENVIRONMENT_FINGERPRINT_ENV, ENVIRONMENT_NAME_ENV, LoadConfigError,
    load_startup_config, load_startup_config_from_vars, parse_and_validate_yaml, parse_yaml_config,
};
pub use persistence::{
    AuditRow, InstanceLease, LeaseAcquireError, PendingOutboxRow, SqliteRepository, StorageError,
};
pub use resources::{ProcessResourceSampler, ResourceSampleError};
