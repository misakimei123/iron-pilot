//! Application orchestration boundary.
//!
//! The domain crate must not depend on this crate.

#![forbid(unsafe_code)]

mod ai;
mod config;
mod execution;
mod execution_validation;
mod persistence;
mod runtime;

pub use ai::{
    AI_TRADING_PROMPT_VERSION_V1, AI_TRADING_PROMPT_VERSION_V2, AiPlanRejectionFeedback,
    AiPromptError, AiRuntimeTradePlanFact, AiTradingPrompt, AiTradingPromptHash,
    AiTradingRuntimeState, AiTradingRuntimeStateHash, MAX_REPLAN_REASON_LENGTH, MAX_REPLAN_REASONS,
    MAX_RUNTIME_ACTIVE_TRADE_PLANS, MAX_RUNTIME_STATE_BYTES, PROMPT_CANDLES_PER_TIMEFRAME,
};
pub use config::{
    CONFIG_SCHEMA_VERSION_V2, ConfigValidationError, DeploymentEnvironment, EnvironmentFingerprint,
    ExecutionMode, LlmLimits, MarketLimits, PermissionConfig, QueueLimits, RuntimeConfig,
    RuntimeLimits, StartupIdentity, StorageLimits, ValidatedRuntimeConfig, VersionConfig,
};
pub use execution::{
    ExecutionCommandKind, ExecutionEffect, ExecutionFuture, ExecutionOrderIdSet, ExecutionOrderIds,
    ExecutionOrderRole, ExecutionReceipt, ExecutionVenue, MAX_EXECUTION_ORDERS_PER_ACTION,
    PAPER_MATCHING_ENGINE_VERSION_V1, PaperExecutionError, PaperExecutionPolicy,
    PaperMarketObservation, PaperMatch, PaperMatchingEngine, PaperOpenOrder, PaperOrderEvaluation,
    PlannedSpotOrder, SPOT_EXECUTION_SCHEMA_VERSION_V1, SpotExecutionPort, SpotExecutionRequest,
    SpotExecutionRequestError, SpotExecutionRequestHash,
};
pub use execution_validation::{
    ActiveTradePlanFact, EXECUTION_VALIDATOR_VERSION_V1, ExecutionAuthorization,
    ExecutionValidationDecision, ExecutionValidationHash, ExecutionValidationInputError,
    ExecutionValidationOutcome, ExecutionValidationPolicy, ExecutionValidationRejection,
    ExecutionValidationRequest, ExecutionValidator, MAX_VALIDATION_REJECTIONS,
    ManagedPositionExecutionFact, SpotOrderPriceLimits,
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
