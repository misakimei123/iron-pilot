//! Pure IronPilot domain types and invariants.
//!
//! This crate has no HTTP, database, exchange, LLM, or runtime dependencies.

#![forbid(unsafe_code)]

mod ai_trading_plan;
mod decimal;
mod ids;
mod instrument;
mod market;
mod market_features;
mod portfolio;
mod replay;
mod state;

pub use ai_trading_plan::{
    AI_DECISION_CONTEXT_SCHEMA_VERSION_V1, AI_TRADING_PLAN_SCHEMA_VERSION_V3, AiOrder, AiOrderType,
    AiProtectiveStop, AiReviewSchedule, AiTakeProfit, AiTimeInForce, AiTradingAction,
    AiTradingPlan, AiTradingPlanHash, AiTradingPlanParseError, AiTradingPlanValidationError,
    MAX_PLAN_RISKS, MAX_PLAN_TEXT_LENGTH, MAX_TAKE_PROFITS,
};
pub use decimal::{DomainDecimal, ParseDomainDecimalError};
pub use ids::{
    AiDecisionContextId, AiTradingPlanId, AuditEntryId, CorrelationId, DecisionId,
    EligibilityEventId, FillId, ManagedLotId, OrderId, OrderIntentId, OutboxMessageId,
    ParseStableIdError, ReconciliationRunId, RuntimeInstanceId, SnapshotId, TradePlanActionId,
    TradePlanId,
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
pub use portfolio::{
    AssetReconciliation, ExchangeAssetBalance, LocalAssetBalance, MAX_PORTFOLIO_ASSETS,
    ManagedPosition, PORTFOLIO_SCHEMA_VERSION_V1, PortfolioError, PortfolioFill, PortfolioFillSide,
    PortfolioHash, PortfolioReconciler, PortfolioReconciliationStatus, PortfolioSnapshot,
    SellAuthorization,
};
pub use replay::{
    MARKET_REPLAY_REPORT_VERSION_V2, MARKET_REPLAY_SCHEMA_VERSION_V2, REPLAY_DETERMINISTIC_SEED_V1,
    ReplayClock, ReplayDataset, ReplayEligibilityOutcome, ReplayError, ReplayHash,
    ReplayInstrumentData, ReplayManifest, ReplayRecord, ReplayReport, ReplayRunner,
};
pub use state::{InvalidTransition, OrderState, SystemState, TradePlanState};
