//! Historical v2 deterministic Risk Engine. Not compiled into the v3 domain crate.

use core::fmt;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    DecisionId, DomainDecimal, InstrumentId, PortfolioHash, PortfolioSnapshot, RiskDecisionId,
    STRATEGY_SPACE_VERSION_V1_VS, SnapshotId, StrategyAction, SystemState, ValidatedStrategyIntent,
};

pub const RISK_RULES_VERSION_V1: &str = "ironpilot-risk-rules-v1";
pub const MAX_ACTIVE_TRADE_PLANS: u8 = 2;
pub const MAX_MATERIALIZATION_VERSION_LENGTH: usize = 64;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct MaterializationHash([u8; 32]);

impl MaterializationHash {
    #[must_use]
    pub const fn new(value: [u8; 32]) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Display for MaterializationHash {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_hex(formatter, &self.0)
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RiskDecisionHash([u8; 32]);

impl RiskDecisionHash {
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Display for RiskDecisionHash {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_hex(formatter, &self.0)
    }
}

fn write_hex(formatter: &mut fmt::Formatter<'_>, bytes: &[u8; 32]) -> fmt::Result {
    for byte in bytes {
        write!(formatter, "{byte:02x}")?;
    }
    Ok(())
}

/// A candidate produced by deterministic materialization from a locally
/// validated Strategy Intent.
///
/// This type deliberately contains no free-form strategy fields. Risk can only
/// retain the original action and provenance while tightening quantity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MaterializedRiskInput {
    validated_intent: ValidatedStrategyIntent,
    materialization_algorithm_version: Box<str>,
    materialization_hash: MaterializationHash,
    requested_quantity: DomainDecimal,
    maximum_allowed_quantity: DomainDecimal,
}

impl MaterializedRiskInput {
    pub fn new(
        validated_intent: ValidatedStrategyIntent,
        materialization_algorithm_version: impl Into<Box<str>>,
        materialization_hash: MaterializationHash,
        requested_quantity: DomainDecimal,
        maximum_allowed_quantity: DomainDecimal,
    ) -> Result<Self, RiskInputError> {
        let materialization_algorithm_version = materialization_algorithm_version.into();
        if materialization_algorithm_version.is_empty()
            || materialization_algorithm_version.len() > MAX_MATERIALIZATION_VERSION_LENGTH
        {
            return Err(RiskInputError::InvalidMaterializationVersion);
        }
        if validated_intent.as_intent().action() != StrategyAction::OpenLong {
            return Err(RiskInputError::ActionNotMaterializedForEntryRisk);
        }
        if requested_quantity <= DomainDecimal::ZERO {
            return Err(RiskInputError::NonPositiveRequestedQuantity);
        }
        if maximum_allowed_quantity < DomainDecimal::ZERO {
            return Err(RiskInputError::NegativeMaximumAllowedQuantity);
        }

        Ok(Self {
            validated_intent,
            materialization_algorithm_version,
            materialization_hash,
            requested_quantity,
            maximum_allowed_quantity,
        })
    }

    #[must_use]
    pub const fn validated_intent(&self) -> &ValidatedStrategyIntent {
        &self.validated_intent
    }

    #[must_use]
    pub fn materialization_algorithm_version(&self) -> &str {
        &self.materialization_algorithm_version
    }

    #[must_use]
    pub const fn materialization_hash(&self) -> MaterializationHash {
        self.materialization_hash
    }

    #[must_use]
    pub const fn requested_quantity(&self) -> DomainDecimal {
        self.requested_quantity
    }

    #[must_use]
    pub const fn maximum_allowed_quantity(&self) -> DomainDecimal {
        self.maximum_allowed_quantity
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SymbolRiskState {
    EntryEnabled,
    ReduceOnly,
    Halted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RiskContext<'a> {
    system_state: SystemState,
    symbol_state: SymbolRiskState,
    active_trade_plans: u8,
    max_active_trade_plans: u8,
    portfolio_snapshot: &'a PortfolioSnapshot,
}

impl<'a> RiskContext<'a> {
    pub fn new(
        system_state: SystemState,
        symbol_state: SymbolRiskState,
        active_trade_plans: u8,
        max_active_trade_plans: u8,
        portfolio_snapshot: &'a PortfolioSnapshot,
    ) -> Result<Self, RiskInputError> {
        if max_active_trade_plans == 0 || max_active_trade_plans > MAX_ACTIVE_TRADE_PLANS {
            return Err(RiskInputError::InvalidActiveTradePlanLimit);
        }
        Ok(Self {
            system_state,
            symbol_state,
            active_trade_plans,
            max_active_trade_plans,
            portfolio_snapshot,
        })
    }

    #[must_use]
    pub const fn system_state(&self) -> SystemState {
        self.system_state
    }

    #[must_use]
    pub const fn symbol_state(&self) -> SymbolRiskState {
        self.symbol_state
    }

    #[must_use]
    pub const fn active_trade_plans(&self) -> u8 {
        self.active_trade_plans
    }

    #[must_use]
    pub const fn max_active_trade_plans(&self) -> u8 {
        self.max_active_trade_plans
    }

    #[must_use]
    pub const fn portfolio_snapshot(&self) -> &PortfolioSnapshot {
        self.portfolio_snapshot
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RiskOutcome {
    Approve,
    AdjustDown,
    Reject,
    ReduceOnly,
    HaltSymbol,
    HaltSystem,
}

impl RiskOutcome {
    pub const ALL: [Self; 6] = [
        Self::Approve,
        Self::AdjustDown,
        Self::Reject,
        Self::ReduceOnly,
        Self::HaltSymbol,
        Self::HaltSystem,
    ];

    #[must_use]
    pub const fn permits_execution(self) -> bool {
        matches!(self, Self::Approve | Self::AdjustDown)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RiskReason {
    WithinAllLimits,
    QuantityAdjustedToMaterializedMaximum,
    ZeroRiskAllowance,
    PortfolioNotReconciled,
    ActiveTradePlanLimitReached,
    SystemNotEntryEnabled,
    SymbolReduceOnly,
    SymbolHalted,
    SystemHalted,
    ActiveTradePlanInvariantBreached,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RiskAuthorization {
    risk_decision_id: RiskDecisionId,
    decision_id: DecisionId,
    snapshot_id: SnapshotId,
    instrument_id: InstrumentId,
    action: StrategyAction,
    materialization_hash: MaterializationHash,
    approved_quantity: DomainDecimal,
}

impl RiskAuthorization {
    #[must_use]
    pub const fn risk_decision_id(&self) -> RiskDecisionId {
        self.risk_decision_id
    }

    #[must_use]
    pub const fn decision_id(&self) -> DecisionId {
        self.decision_id
    }

    #[must_use]
    pub const fn snapshot_id(&self) -> SnapshotId {
        self.snapshot_id
    }

    #[must_use]
    pub const fn instrument_id(&self) -> &InstrumentId {
        &self.instrument_id
    }

    #[must_use]
    pub const fn action(&self) -> StrategyAction {
        self.action
    }

    #[must_use]
    pub const fn materialization_hash(&self) -> MaterializationHash {
        self.materialization_hash
    }

    #[must_use]
    pub const fn approved_quantity(&self) -> DomainDecimal {
        self.approved_quantity
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RiskDecision {
    risk_decision_id: RiskDecisionId,
    decision_id: DecisionId,
    snapshot_id: SnapshotId,
    instrument_id: InstrumentId,
    action: StrategyAction,
    strategy_space_version: &'static str,
    rules_version: &'static str,
    materialization_algorithm_version: Box<str>,
    materialization_hash: MaterializationHash,
    portfolio_hash: PortfolioHash,
    outcome: RiskOutcome,
    reason: RiskReason,
    requested_quantity: DomainDecimal,
    maximum_allowed_quantity: DomainDecimal,
    approved_quantity: Option<DomainDecimal>,
    system_state: SystemState,
    symbol_state: SymbolRiskState,
    active_trade_plans: u8,
    max_active_trade_plans: u8,
    decided_at_unix_millis: u64,
    authorization: Option<RiskAuthorization>,
    decision_hash: RiskDecisionHash,
}

impl RiskDecision {
    #[must_use]
    pub const fn risk_decision_id(&self) -> RiskDecisionId {
        self.risk_decision_id
    }

    #[must_use]
    pub const fn decision_id(&self) -> DecisionId {
        self.decision_id
    }

    #[must_use]
    pub const fn snapshot_id(&self) -> SnapshotId {
        self.snapshot_id
    }

    #[must_use]
    pub const fn instrument_id(&self) -> &InstrumentId {
        &self.instrument_id
    }

    #[must_use]
    pub const fn action(&self) -> StrategyAction {
        self.action
    }

    #[must_use]
    pub const fn strategy_space_version(&self) -> &'static str {
        self.strategy_space_version
    }

    #[must_use]
    pub const fn rules_version(&self) -> &'static str {
        self.rules_version
    }

    #[must_use]
    pub fn materialization_algorithm_version(&self) -> &str {
        &self.materialization_algorithm_version
    }

    #[must_use]
    pub const fn materialization_hash(&self) -> MaterializationHash {
        self.materialization_hash
    }

    #[must_use]
    pub const fn portfolio_hash(&self) -> PortfolioHash {
        self.portfolio_hash
    }

    #[must_use]
    pub const fn outcome(&self) -> RiskOutcome {
        self.outcome
    }

    #[must_use]
    pub const fn reason(&self) -> RiskReason {
        self.reason
    }

    #[must_use]
    pub const fn requested_quantity(&self) -> DomainDecimal {
        self.requested_quantity
    }

    #[must_use]
    pub const fn maximum_allowed_quantity(&self) -> DomainDecimal {
        self.maximum_allowed_quantity
    }

    #[must_use]
    pub const fn approved_quantity(&self) -> Option<DomainDecimal> {
        self.approved_quantity
    }

    #[must_use]
    pub const fn system_state(&self) -> SystemState {
        self.system_state
    }

    #[must_use]
    pub const fn symbol_state(&self) -> SymbolRiskState {
        self.symbol_state
    }

    #[must_use]
    pub const fn active_trade_plans(&self) -> u8 {
        self.active_trade_plans
    }

    #[must_use]
    pub const fn max_active_trade_plans(&self) -> u8 {
        self.max_active_trade_plans
    }

    #[must_use]
    pub const fn decided_at_unix_millis(&self) -> u64 {
        self.decided_at_unix_millis
    }

    #[must_use]
    pub const fn authorization(&self) -> Option<&RiskAuthorization> {
        self.authorization.as_ref()
    }

    #[must_use]
    pub const fn decision_hash(&self) -> RiskDecisionHash {
        self.decision_hash
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RiskEngine;

impl RiskEngine {
    #[must_use]
    pub fn evaluate(
        risk_decision_id: RiskDecisionId,
        input: MaterializedRiskInput,
        context: RiskContext<'_>,
        decided_at_unix_millis: u64,
    ) -> RiskDecision {
        let (outcome, reason, approved_quantity) = Self::classify(&input, context);
        let intent = input.validated_intent.as_intent();
        let decision_id = intent.decision_id();
        let snapshot_id = intent.snapshot_id();
        let instrument_id = intent.instrument_id().clone();
        let action = intent.action();
        let authorization = approved_quantity.map(|approved_quantity| RiskAuthorization {
            risk_decision_id,
            decision_id,
            snapshot_id,
            instrument_id: instrument_id.clone(),
            action,
            materialization_hash: input.materialization_hash,
            approved_quantity,
        });
        let decision_hash = hash_risk_decision(
            risk_decision_id,
            decision_id,
            snapshot_id,
            &instrument_id,
            action,
            &input.materialization_algorithm_version,
            input.materialization_hash,
            context.portfolio_snapshot.snapshot_hash(),
            outcome,
            reason,
            input.requested_quantity,
            input.maximum_allowed_quantity,
            approved_quantity,
            context.system_state,
            context.symbol_state,
            context.active_trade_plans,
            context.max_active_trade_plans,
            decided_at_unix_millis,
        );

        RiskDecision {
            risk_decision_id,
            decision_id,
            snapshot_id,
            instrument_id,
            action,
            strategy_space_version: STRATEGY_SPACE_VERSION_V1_VS,
            rules_version: RISK_RULES_VERSION_V1,
            materialization_algorithm_version: input.materialization_algorithm_version,
            materialization_hash: input.materialization_hash,
            portfolio_hash: context.portfolio_snapshot.snapshot_hash(),
            outcome,
            reason,
            requested_quantity: input.requested_quantity,
            maximum_allowed_quantity: input.maximum_allowed_quantity,
            approved_quantity,
            system_state: context.system_state,
            symbol_state: context.symbol_state,
            active_trade_plans: context.active_trade_plans,
            max_active_trade_plans: context.max_active_trade_plans,
            decided_at_unix_millis,
            authorization,
            decision_hash,
        }
    }

    fn classify(
        input: &MaterializedRiskInput,
        context: RiskContext<'_>,
    ) -> (RiskOutcome, RiskReason, Option<DomainDecimal>) {
        if context.active_trade_plans > MAX_ACTIVE_TRADE_PLANS {
            return (
                RiskOutcome::HaltSystem,
                RiskReason::ActiveTradePlanInvariantBreached,
                None,
            );
        }
        if context.system_state == SystemState::Halted {
            return (RiskOutcome::HaltSystem, RiskReason::SystemHalted, None);
        }
        if context.symbol_state == SymbolRiskState::Halted {
            return (RiskOutcome::HaltSymbol, RiskReason::SymbolHalted, None);
        }
        if context.system_state != SystemState::EntryEnabled {
            return (
                RiskOutcome::ReduceOnly,
                RiskReason::SystemNotEntryEnabled,
                None,
            );
        }
        if context.symbol_state == SymbolRiskState::ReduceOnly {
            return (RiskOutcome::ReduceOnly, RiskReason::SymbolReduceOnly, None);
        }
        if !context.portfolio_snapshot.allows_new_entries() {
            return (
                RiskOutcome::Reject,
                RiskReason::PortfolioNotReconciled,
                None,
            );
        }
        if context.active_trade_plans >= context.max_active_trade_plans {
            return (
                RiskOutcome::Reject,
                RiskReason::ActiveTradePlanLimitReached,
                None,
            );
        }
        if input.maximum_allowed_quantity == DomainDecimal::ZERO {
            return (RiskOutcome::Reject, RiskReason::ZeroRiskAllowance, None);
        }
        if input.requested_quantity > input.maximum_allowed_quantity {
            return (
                RiskOutcome::AdjustDown,
                RiskReason::QuantityAdjustedToMaterializedMaximum,
                Some(input.maximum_allowed_quantity),
            );
        }
        (
            RiskOutcome::Approve,
            RiskReason::WithinAllLimits,
            Some(input.requested_quantity),
        )
    }
}

#[allow(clippy::too_many_arguments)]
fn hash_risk_decision(
    risk_decision_id: RiskDecisionId,
    decision_id: DecisionId,
    snapshot_id: SnapshotId,
    instrument_id: &InstrumentId,
    action: StrategyAction,
    materialization_algorithm_version: &str,
    materialization_hash: MaterializationHash,
    portfolio_hash: PortfolioHash,
    outcome: RiskOutcome,
    reason: RiskReason,
    requested_quantity: DomainDecimal,
    maximum_allowed_quantity: DomainDecimal,
    approved_quantity: Option<DomainDecimal>,
    system_state: SystemState,
    symbol_state: SymbolRiskState,
    active_trade_plans: u8,
    max_active_trade_plans: u8,
    decided_at_unix_millis: u64,
) -> RiskDecisionHash {
    let mut hasher = RiskHasher::new("risk-decision-v1");
    hasher.field(RISK_RULES_VERSION_V1);
    hasher.field(&risk_decision_id.to_string());
    hasher.field(&decision_id.to_string());
    hasher.field(&snapshot_id.to_string());
    hasher.field(&instrument_id.to_string());
    hasher.field(action_name(action));
    hasher.field(STRATEGY_SPACE_VERSION_V1_VS);
    hasher.field(materialization_algorithm_version);
    hasher.bytes(materialization_hash.as_bytes());
    hasher.bytes(portfolio_hash.as_bytes());
    hasher.field(outcome_name(outcome));
    hasher.field(reason_name(reason));
    hasher.decimal(requested_quantity);
    hasher.decimal(maximum_allowed_quantity);
    match approved_quantity {
        Some(value) => {
            hasher.field("some");
            hasher.decimal(value);
        }
        None => hasher.field("none"),
    }
    hasher.field(system_state_name(system_state));
    hasher.field(symbol_state_name(symbol_state));
    hasher.u8(active_trade_plans);
    hasher.u8(max_active_trade_plans);
    hasher.u64(decided_at_unix_millis);
    hasher.finish()
}

const fn action_name(action: StrategyAction) -> &'static str {
    match action {
        StrategyAction::OpenLong => "OPEN_LONG",
        StrategyAction::NoTrade => "NO_TRADE",
        StrategyAction::Hold => "HOLD",
        StrategyAction::Exit => "EXIT",
        StrategyAction::OpenShort => "OPEN_SHORT",
    }
}

const fn outcome_name(outcome: RiskOutcome) -> &'static str {
    match outcome {
        RiskOutcome::Approve => "APPROVE",
        RiskOutcome::AdjustDown => "ADJUST_DOWN",
        RiskOutcome::Reject => "REJECT",
        RiskOutcome::ReduceOnly => "REDUCE_ONLY",
        RiskOutcome::HaltSymbol => "HALT_SYMBOL",
        RiskOutcome::HaltSystem => "HALT_SYSTEM",
    }
}

const fn reason_name(reason: RiskReason) -> &'static str {
    match reason {
        RiskReason::WithinAllLimits => "WITHIN_ALL_LIMITS",
        RiskReason::QuantityAdjustedToMaterializedMaximum => {
            "QUANTITY_ADJUSTED_TO_MATERIALIZED_MAXIMUM"
        }
        RiskReason::ZeroRiskAllowance => "ZERO_RISK_ALLOWANCE",
        RiskReason::PortfolioNotReconciled => "PORTFOLIO_NOT_RECONCILED",
        RiskReason::ActiveTradePlanLimitReached => "ACTIVE_TRADE_PLAN_LIMIT_REACHED",
        RiskReason::SystemNotEntryEnabled => "SYSTEM_NOT_ENTRY_ENABLED",
        RiskReason::SymbolReduceOnly => "SYMBOL_REDUCE_ONLY",
        RiskReason::SymbolHalted => "SYMBOL_HALTED",
        RiskReason::SystemHalted => "SYSTEM_HALTED",
        RiskReason::ActiveTradePlanInvariantBreached => "ACTIVE_TRADE_PLAN_INVARIANT_BREACHED",
    }
}

const fn system_state_name(state: SystemState) -> &'static str {
    match state {
        SystemState::Starting => "STARTING",
        SystemState::Recovering => "RECOVERING",
        SystemState::Observing => "OBSERVING",
        SystemState::EntryEnabled => "ENTRY_ENABLED",
        SystemState::ReduceOnly => "REDUCE_ONLY",
        SystemState::Halted => "HALTED",
    }
}

const fn symbol_state_name(state: SymbolRiskState) -> &'static str {
    match state {
        SymbolRiskState::EntryEnabled => "ENTRY_ENABLED",
        SymbolRiskState::ReduceOnly => "REDUCE_ONLY",
        SymbolRiskState::Halted => "HALTED",
    }
}

struct RiskHasher(Sha256);

impl RiskHasher {
    fn new(schema: &str) -> Self {
        let mut hasher = Self(Sha256::new());
        hasher.field(schema);
        hasher
    }

    fn field(&mut self, value: &str) {
        self.bytes(value.as_bytes());
    }

    fn decimal(&mut self, value: DomainDecimal) {
        self.field(&value.as_decimal().normalize().to_string());
    }

    fn u64(&mut self, value: u64) {
        self.field(&value.to_string());
    }

    fn u8(&mut self, value: u8) {
        self.field(&value.to_string());
    }

    fn bytes(&mut self, value: &[u8]) {
        self.0.update(value.len().to_be_bytes());
        self.0.update(value);
    }

    fn finish(self) -> RiskDecisionHash {
        RiskDecisionHash(self.0.finalize().into())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RiskInputError {
    InvalidMaterializationVersion,
    ActionNotMaterializedForEntryRisk,
    NonPositiveRequestedQuantity,
    NegativeMaximumAllowedQuantity,
    InvalidActiveTradePlanLimit,
}

impl fmt::Display for RiskInputError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidMaterializationVersion => {
                "materialization algorithm version must be non-empty and bounded"
            }
            Self::ActionNotMaterializedForEntryRisk => {
                "P3-02 entry risk accepts only a validated OPEN_LONG materialization"
            }
            Self::NonPositiveRequestedQuantity => {
                "materialized requested quantity must be positive"
            }
            Self::NegativeMaximumAllowedQuantity => {
                "materialized maximum allowed quantity must not be negative"
            }
            Self::InvalidActiveTradePlanLimit => {
                "active TradePlan limit must be within the frozen 1..=2 bound"
            }
        })
    }
}

impl std::error::Error for RiskInputError {}
