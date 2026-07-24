//! Pure IronPilot domain types and invariants.
//!
//! This crate has no HTTP, database, exchange, LLM, or runtime dependencies.

#![forbid(unsafe_code)]

mod decimal;
mod ids;
mod instrument;
mod state;
mod strategy;

pub use decimal::{DomainDecimal, ParseDomainDecimalError};
pub use ids::{
    DecisionId, EligibilityEventId, FillId, OrderId, OrderIntentId, ParseStableIdError,
    RiskDecisionId, SnapshotId, TradePlanActionId, TradePlanId,
};
pub use instrument::{
    Exchange, InstrumentId, InstrumentType, ParseInstrumentIdError, Symbol,
    ValidationError as InstrumentValidationError,
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
