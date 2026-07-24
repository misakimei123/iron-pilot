use core::fmt;

use serde::{Deserialize, Serialize};

use crate::{DecisionId, InstrumentId, InstrumentType, SnapshotId};

pub const STRATEGY_SCHEMA_VERSION_V2: &str = "2.0";
pub const STRATEGY_SPACE_VERSION_V1_VS: &str = "strategy-space-v1-vs";
pub const MAX_WAIT_BARS: u8 = 4;
pub const MAX_HOLDING_BARS: u16 = 96;
pub const MAX_INVALIDATION_CONDITIONS: usize = 8;

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct SchemaVersion(Box<str>);

impl SchemaVersion {
    #[must_use]
    pub fn v2() -> Self {
        Self(STRATEGY_SCHEMA_VERSION_V2.into())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct StrategySpaceVersion(Box<str>);

impl StrategySpaceVersion {
    #[must_use]
    pub fn vertical_slice_v1() -> Self {
        Self(STRATEGY_SPACE_VERSION_V1_VS.into())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum StrategyAction {
    OpenLong,
    NoTrade,
    Hold,
    Exit,
    OpenShort,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StrategyFamily {
    TrendBreakout,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryPolicyType {
    BreakoutRetest,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryAnchor {
    DonchianUpper,
    KeyLocation,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryConfirmation {
    CloseConfirmed,
    RejectionConfirmed,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EntryPolicy {
    #[serde(rename = "type")]
    policy_type: EntryPolicyType,
    anchor: EntryAnchor,
    max_wait_bars: u8,
    confirmation: EntryConfirmation,
}

impl EntryPolicy {
    #[must_use]
    pub const fn new(
        anchor: EntryAnchor,
        max_wait_bars: u8,
        confirmation: EntryConfirmation,
    ) -> Self {
        Self {
            policy_type: EntryPolicyType::BreakoutRetest,
            anchor,
            max_wait_bars,
            confirmation,
        }
    }

    #[must_use]
    pub const fn max_wait_bars(&self) -> u8 {
        self.max_wait_bars
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StopPolicyType {
    StructureWithAtrBuffer,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StopAnchor {
    RecentSwing,
    KeyLocation,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BufferTier {
    Normal,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StopPolicy {
    #[serde(rename = "type")]
    policy_type: StopPolicyType,
    anchor: StopAnchor,
    buffer_tier: BufferTier,
}

impl StopPolicy {
    #[must_use]
    pub const fn new(anchor: StopAnchor) -> Self {
        Self {
            policy_type: StopPolicyType::StructureWithAtrBuffer,
            anchor,
            buffer_tier: BufferTier::Normal,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetPolicyType {
    FixedRrTier,
    NextStructure,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum MinimumRiskReward {
    #[serde(rename = "2R")]
    TwoR,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TrailingAnchor {
    None,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TargetPolicy {
    #[serde(rename = "type")]
    policy_type: TargetPolicyType,
    minimum_rr_tier: MinimumRiskReward,
    trailing_anchor: TrailingAnchor,
}

impl TargetPolicy {
    #[must_use]
    pub const fn fixed_rr() -> Self {
        Self {
            policy_type: TargetPolicyType::FixedRrTier,
            minimum_rr_tier: MinimumRiskReward::TwoR,
            trailing_anchor: TrailingAnchor::None,
        }
    }

    #[must_use]
    pub const fn next_structure() -> Self {
        Self {
            policy_type: TargetPolicyType::NextStructure,
            minimum_rr_tier: MinimumRiskReward::TwoR,
            trailing_anchor: TrailingAnchor::None,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskTier {
    Conservative,
    Normal,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewPolicy {
    EveryPrimaryClose,
    OnInvalidationRisk,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InvalidationCondition {
    BreakoutFailed,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OpenPositionDecision {
    strategy_family: StrategyFamily,
    entry_policy: EntryPolicy,
    stop_policy: StopPolicy,
    target_policy: TargetPolicy,
    risk_tier: RiskTier,
    maximum_holding_bars: u16,
    review_policy: ReviewPolicy,
    invalidation_conditions: Vec<InvalidationCondition>,
}

impl OpenPositionDecision {
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        entry_policy: EntryPolicy,
        stop_policy: StopPolicy,
        target_policy: TargetPolicy,
        risk_tier: RiskTier,
        maximum_holding_bars: u16,
        review_policy: ReviewPolicy,
        invalidation_conditions: Vec<InvalidationCondition>,
    ) -> Self {
        Self {
            strategy_family: StrategyFamily::TrendBreakout,
            entry_policy,
            stop_policy,
            target_policy,
            risk_tier,
            maximum_holding_bars,
            review_policy,
            invalidation_conditions,
        }
    }

    fn validate(&self) -> Result<(), StrategyValidationError> {
        if !(1..=MAX_WAIT_BARS).contains(&self.entry_policy.max_wait_bars()) {
            return Err(StrategyValidationError::MaxWaitBarsOutOfRange {
                value: self.entry_policy.max_wait_bars(),
            });
        }
        if !(1..=MAX_HOLDING_BARS).contains(&self.maximum_holding_bars) {
            return Err(StrategyValidationError::MaximumHoldingBarsOutOfRange {
                value: self.maximum_holding_bars,
            });
        }
        if self.invalidation_conditions.is_empty() {
            return Err(StrategyValidationError::MissingInvalidationCondition);
        }
        if self.invalidation_conditions.len() > MAX_INVALIDATION_CONDITIONS {
            return Err(StrategyValidationError::TooManyInvalidationConditions {
                count: self.invalidation_conditions.len(),
            });
        }

        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PositionReviewDecision {
    review_policy: ReviewPolicy,
}

impl PositionReviewDecision {
    #[must_use]
    pub const fn new(review_policy: ReviewPolicy) -> Self {
        Self { review_policy }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    tag = "action",
    rename_all = "SCREAMING_SNAKE_CASE",
    deny_unknown_fields
)]
pub enum StrategyDecision {
    OpenLong(OpenPositionDecision),
    NoTrade,
    Hold(PositionReviewDecision),
    Exit(PositionReviewDecision),
    OpenShort(OpenPositionDecision),
}

impl StrategyDecision {
    #[must_use]
    pub const fn action(&self) -> StrategyAction {
        match self {
            Self::OpenLong(_) => StrategyAction::OpenLong,
            Self::NoTrade => StrategyAction::NoTrade,
            Self::Hold(_) => StrategyAction::Hold,
            Self::Exit(_) => StrategyAction::Exit,
            Self::OpenShort(_) => StrategyAction::OpenShort,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StrategyIntent {
    schema_version: SchemaVersion,
    strategy_space_version: StrategySpaceVersion,
    decision_id: DecisionId,
    snapshot_id: SnapshotId,
    instrument_id: InstrumentId,
    decision: StrategyDecision,
}

impl StrategyIntent {
    #[must_use]
    pub const fn new(
        schema_version: SchemaVersion,
        strategy_space_version: StrategySpaceVersion,
        decision_id: DecisionId,
        snapshot_id: SnapshotId,
        instrument_id: InstrumentId,
        decision: StrategyDecision,
    ) -> Self {
        Self {
            schema_version,
            strategy_space_version,
            decision_id,
            snapshot_id,
            instrument_id,
            decision,
        }
    }

    #[must_use]
    pub fn strategy_space_version(&self) -> &StrategySpaceVersion {
        &self.strategy_space_version
    }

    #[must_use]
    pub fn instrument_id(&self) -> &InstrumentId {
        &self.instrument_id
    }

    #[must_use]
    pub const fn action(&self) -> StrategyAction {
        self.decision.action()
    }

    pub fn validate_for_vertical_slice(
        self,
    ) -> Result<ValidatedStrategyIntent, StrategyValidationError> {
        if self.schema_version.as_str() != STRATEGY_SCHEMA_VERSION_V2 {
            return Err(StrategyValidationError::UnsupportedSchemaVersion);
        }
        if self.strategy_space_version.as_str() != STRATEGY_SPACE_VERSION_V1_VS {
            return Err(StrategyValidationError::UnsupportedStrategySpaceVersion);
        }
        if self.instrument_id.instrument_type() == InstrumentType::Spot
            && self.action() == StrategyAction::OpenShort
        {
            return Err(StrategyValidationError::OpenShortForbiddenForSpot);
        }
        if self.instrument_id.instrument_type() != InstrumentType::Spot {
            return Err(StrategyValidationError::InstrumentTypeNotExecutable {
                instrument_type: self.instrument_id.instrument_type(),
            });
        }

        match &self.decision {
            StrategyDecision::OpenLong(decision) => decision.validate()?,
            StrategyDecision::OpenShort(_) => {
                return Err(StrategyValidationError::ActionNotExecutable {
                    action: StrategyAction::OpenShort,
                });
            }
            StrategyDecision::NoTrade | StrategyDecision::Hold(_) | StrategyDecision::Exit(_) => {}
        }

        Ok(ValidatedStrategyIntent(self))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatedStrategyIntent(StrategyIntent);

impl ValidatedStrategyIntent {
    #[must_use]
    pub const fn as_intent(&self) -> &StrategyIntent {
        &self.0
    }

    #[must_use]
    pub fn into_intent(self) -> StrategyIntent {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StrategyValidationError {
    UnsupportedSchemaVersion,
    UnsupportedStrategySpaceVersion,
    InstrumentTypeNotExecutable { instrument_type: InstrumentType },
    OpenShortForbiddenForSpot,
    ActionNotExecutable { action: StrategyAction },
    MaxWaitBarsOutOfRange { value: u8 },
    MaximumHoldingBarsOutOfRange { value: u16 },
    MissingInvalidationCondition,
    TooManyInvalidationConditions { count: usize },
}

impl fmt::Display for StrategyValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedSchemaVersion => {
                formatter.write_str("only StrategyIntent schema 2.0 is executable")
            }
            Self::UnsupportedStrategySpaceVersion => formatter
                .write_str("only strategy-space-v1-vs is executable before the vertical slice"),
            Self::InstrumentTypeNotExecutable { instrument_type } => {
                write!(
                    formatter,
                    "instrument type {instrument_type:?} is not executable"
                )
            }
            Self::OpenShortForbiddenForSpot => {
                formatter.write_str("OPEN_SHORT is forbidden for Spot instruments")
            }
            Self::ActionNotExecutable { action } => {
                write!(formatter, "strategy action {action:?} is not executable")
            }
            Self::MaxWaitBarsOutOfRange { value } => {
                write!(
                    formatter,
                    "max_wait_bars {value} is outside 1..={MAX_WAIT_BARS}"
                )
            }
            Self::MaximumHoldingBarsOutOfRange { value } => write!(
                formatter,
                "maximum_holding_bars {value} is outside 1..={MAX_HOLDING_BARS}"
            ),
            Self::MissingInvalidationCondition => {
                formatter.write_str("at least one invalidation condition is required")
            }
            Self::TooManyInvalidationConditions { count } => write!(
                formatter,
                "{count} invalidation conditions exceeds {MAX_INVALIDATION_CONDITIONS}"
            ),
        }
    }
}

impl std::error::Error for StrategyValidationError {}
