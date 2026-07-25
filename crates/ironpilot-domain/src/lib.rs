//! Pure IronPilot domain types and invariants.
//!
//! This crate has no HTTP, database, exchange, LLM, or runtime dependencies.

#![forbid(unsafe_code)]

mod decimal;
mod ids;
mod instrument;
mod market;
mod market_features;
mod replay;
mod state;
mod strategy;

pub use decimal::{DomainDecimal, ParseDomainDecimalError};
pub use ids::{
    AuditEntryId, CorrelationId, DecisionId, EligibilityEventId, FillId, OrderId, OrderIntentId,
    OutboxMessageId, ParseStableIdError, RiskDecisionId, RuntimeInstanceId, SnapshotId,
    TradePlanActionId, TradePlanId,
};
pub use instrument::{
    Exchange, InstrumentId, InstrumentType, ParseInstrumentIdError, Symbol,
    ValidationError as InstrumentValidationError,
};
pub use market::{
    AssetCode, ExchangeServerTime, InstrumentRulesSnapshot, InstrumentTradingStatus,
    MarketMetadataValidationError, RulesHash, SpotInstrumentRules, validated_spot_instrument_rules,
};
pub use market_features::{
    ATR_PERIOD, CandlePattern, ClosedCandle, ContentHash, DONCHIAN_LOWER_PERIOD,
    DONCHIAN_UPPER_PERIOD, EMA_FAST_PERIOD, EMA_SLOW_PERIOD, EligibilityEvent,
    EligibilityEventEngine, EligibilityEventKind, EligibilityPolicy, EmaAlignment,
    FEATURE_CANDLE_WINDOW, KeyLocation, LlmBudgetUsage, MARKET_FEATURES_VERSION_V1,
    MAX_EVENT_DEDUPLICATION_ENTRIES, MAX_TRACKED_ELIGIBILITY_INSTRUMENTS, MarketDataSource,
    MarketFeatureEngine, MarketFeatureError, MarketFeatureSnapshot, MarketTimeframe,
    PatternObservation, PatternSemantic, PrefilterContext, PrefilterDecision,
    PrefilterRejectionReason, TopOfBook, VOLUME_RATIO_PERIOD, WILDER_PERIOD,
};
pub use replay::{
    MARKET_REPLAY_REPORT_VERSION_V1, MARKET_REPLAY_SCHEMA_VERSION_V1, REPLAY_DETERMINISTIC_SEED_V1,
    ReplayClock, ReplayDataset, ReplayEligibilityOutcome, ReplayError, ReplayHash,
    ReplayInstrumentData, ReplayManifest, ReplayRecord, ReplayReport, ReplayRunner,
};
pub use state::{InvalidTransition, OrderState, SystemState, TradePlanState};
pub use strategy::{
    BufferTier, EntryAnchor, EntryConfirmation, EntryPolicy, EntryPolicyType,
    InvalidationCondition, MinimumRiskReward, OpenPositionDecision, PositionReviewDecision,
    ReviewPolicy, RiskTier, STRATEGY_SPACE_VERSION_V1_VS, SchemaVersion, StopAnchor, StopPolicy,
    StopPolicyType, StrategyAction, StrategyDecision, StrategyFamily, StrategyIntent,
    StrategySpaceVersion, StrategyValidationError, TargetPolicy, TargetPolicyType, TrailingAnchor,
    ValidatedStrategyIntent,
};
