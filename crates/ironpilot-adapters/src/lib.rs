//! Infrastructure and interface adapter boundary.
//!
//! Adapters may depend inward on application and domain contracts when those
//! contracts exist. Neither inward crate may depend on this crate.

#![forbid(unsafe_code)]

mod bybit_public;
mod bybit_public_websocket;
mod config;
mod deepseek;
mod emergency;
mod historical;
mod paper_execution;
mod paper_runtime;
mod persistence;
mod resources;
mod telegram;

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
pub use deepseek::{
    DEEPSEEK_API_BASE_URL, DEEPSEEK_API_KEY_ENV, DEEPSEEK_PROVIDER_NAME,
    DeepSeekAiTradingPlanProvider, DeepSeekAttemptEvidence, DeepSeekAttemptOutcome,
    DeepSeekBudgetLimits, DeepSeekBudgetSnapshot, DeepSeekModel, DeepSeekPlanGeneration,
    DeepSeekPricing, DeepSeekProviderConfig, DeepSeekProviderError, DeepSeekProviderErrorKind,
    DeepSeekUsage, MAX_DEEPSEEK_OUTPUT_TOKENS, MAX_DEEPSEEK_REQUEST_TIMEOUT,
    MAX_DEEPSEEK_RESPONSE_BYTES,
};
pub use emergency::{
    EMERGENCY_CORE_VERSION_V1, EmergencyAdapterError, EmergencyExecutionReport,
    MAX_EMERGENCY_OBSERVATION_AGE_MILLIS, SqlitePaperEmergencyController,
};
pub use historical::{
    HistoricalLedgerHash, HistoricalLedgerRecord, HistoricalLedgerRecordKind,
    HistoricalValidationFacts, MAX_MINIMAL_HISTORICAL_OBSERVATIONS,
    MINIMAL_HISTORICAL_HARNESS_VERSION_V1, MinimalHistoricalHarnessError,
    MinimalHistoricalReplayInput, MinimalHistoricalReplayReport, OwnedExecutionValidationFacts,
    SqliteMinimalHistoricalHarness,
};
pub use paper_execution::{
    PaperExecutionAdapterError, PaperExecutionReport, SqlitePaperExecutionPort,
};
pub use paper_runtime::{
    MAX_PAPER_RUNTIME_ATTEMPTS, MAX_PAPER_RUNTIME_OBSERVATIONS, PAPER_RUNTIME_VERSION_V1,
    PaperRuntimeActionAttempt, PaperRuntimeAiProvider, PaperRuntimeCycleId, PaperRuntimeCycleInput,
    PaperRuntimeCycleReport, PaperRuntimeEffect, PaperRuntimeError, PaperRuntimeFacts,
    PaperRuntimeOutcome, PaperRuntimeProviderError, PaperRuntimeProviderFuture,
    RuntimeAiGeneration, SqliteAiPaperRuntime,
};
pub use persistence::{
    AiTradePlanTraceRow, AuditRow, ExecutionValidationRow, InstanceLease, LeaseAcquireError,
    PendingOutboxRow, PersistenceEffect, SqliteRepository, StorageError,
};
pub use resources::{ProcessResourceSampler, ResourceSampleError};
pub use telegram::{
    MAX_TELEGRAM_MESSAGE_CHARS, MAX_TELEGRAM_NOTIFICATION_EVENTS, MAX_TELEGRAM_QUERY_ROWS,
    MAX_TELEGRAM_READONLY_CHATS, MAX_TELEGRAM_UPDATES_PER_POLL, TELEGRAM_BOT_API_BASE_URL,
    TELEGRAM_BOT_TOKEN_ENV, TELEGRAM_READONLY_VERSION_V1, TelegramNotificationReport,
    TelegramPollReport, TelegramReadOnlyAdapter, TelegramReadOnlyCommand, TelegramReadOnlyConfig,
    TelegramReadOnlyError, TelegramReadOnlyText,
};
