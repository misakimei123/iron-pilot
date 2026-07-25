use core::fmt;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    AiDecisionContextId, AiTradingPlanId, DomainDecimal, InstrumentId, InstrumentType, TradePlanId,
};

pub const AI_DECISION_CONTEXT_SCHEMA_VERSION_V1: &str = "ironpilot-ai-decision-context-v1";
pub const AI_TRADING_PLAN_SCHEMA_VERSION_V3: &str = "3.0";
pub const MAX_TAKE_PROFITS: usize = 8;
pub const MAX_PLAN_RISKS: usize = 8;
pub const MAX_PLAN_TEXT_LENGTH: usize = 2_048;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AiTradingPlanHash([u8; 32]);

impl AiTradingPlanHash {
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Display for AiTradingPlanHash {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AiTradingAction {
    OpenLong,
    NoTrade,
    Hold,
    CancelEntry,
    ModifyProtection,
    Reduce,
    Exit,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AiOrderType {
    Limit,
    Market,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AiTimeInForce {
    Gtc,
    Ioc,
    Fok,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AiOrder {
    #[serde(rename = "type")]
    order_type: AiOrderType,
    quantity: DomainDecimal,
    #[serde(skip_serializing_if = "Option::is_none")]
    limit_price: Option<DomainDecimal>,
    time_in_force: AiTimeInForce,
    expires_at: u64,
    max_slippage_quote: DomainDecimal,
}

impl AiOrder {
    #[must_use]
    pub const fn order_type(&self) -> AiOrderType {
        self.order_type
    }

    #[must_use]
    pub const fn quantity(&self) -> DomainDecimal {
        self.quantity
    }

    #[must_use]
    pub const fn limit_price(&self) -> Option<DomainDecimal> {
        self.limit_price
    }

    #[must_use]
    pub const fn time_in_force(&self) -> AiTimeInForce {
        self.time_in_force
    }

    #[must_use]
    pub const fn expires_at_unix_millis(&self) -> u64 {
        self.expires_at
    }

    #[must_use]
    pub const fn max_slippage_quote(&self) -> DomainDecimal {
        self.max_slippage_quote
    }

    fn validate(&self, valid_until: u64) -> Result<(), AiTradingPlanValidationError> {
        if self.quantity <= DomainDecimal::ZERO {
            return Err(AiTradingPlanValidationError::NonPositiveDecimal {
                field: "order.quantity",
            });
        }
        if self.max_slippage_quote < DomainDecimal::ZERO {
            return Err(AiTradingPlanValidationError::NegativeDecimal {
                field: "order.max_slippage_quote",
            });
        }
        if self.expires_at == 0 || self.expires_at > valid_until {
            return Err(AiTradingPlanValidationError::InvalidTimestamp {
                field: "order.expires_at",
            });
        }
        match (self.order_type, self.limit_price) {
            (AiOrderType::Limit, Some(price)) if price > DomainDecimal::ZERO => {}
            (AiOrderType::Limit, _) => {
                return Err(AiTradingPlanValidationError::InvalidOrderPrice);
            }
            (AiOrderType::Market, None) => {}
            (AiOrderType::Market, Some(_)) => {
                return Err(AiTradingPlanValidationError::InvalidOrderPrice);
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AiProtectiveStop {
    trigger_price: DomainDecimal,
    order_type: AiOrderType,
    #[serde(skip_serializing_if = "Option::is_none")]
    limit_price: Option<DomainDecimal>,
}

impl AiProtectiveStop {
    #[must_use]
    pub const fn trigger_price(&self) -> DomainDecimal {
        self.trigger_price
    }

    #[must_use]
    pub const fn order_type(&self) -> AiOrderType {
        self.order_type
    }

    #[must_use]
    pub const fn limit_price(&self) -> Option<DomainDecimal> {
        self.limit_price
    }

    fn validate(&self) -> Result<(), AiTradingPlanValidationError> {
        if self.trigger_price <= DomainDecimal::ZERO {
            return Err(AiTradingPlanValidationError::NonPositiveDecimal {
                field: "protective_stop.trigger_price",
            });
        }
        match (self.order_type, self.limit_price) {
            (AiOrderType::Limit, Some(price)) if price > DomainDecimal::ZERO => Ok(()),
            (AiOrderType::Market, None) => Ok(()),
            _ => Err(AiTradingPlanValidationError::InvalidProtectiveStopPrice),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AiTakeProfit {
    price: DomainDecimal,
    quantity: DomainDecimal,
}

impl AiTakeProfit {
    #[must_use]
    pub const fn price(&self) -> DomainDecimal {
        self.price
    }

    #[must_use]
    pub const fn quantity(&self) -> DomainDecimal {
        self.quantity
    }

    fn validate(&self) -> Result<(), AiTradingPlanValidationError> {
        if self.price <= DomainDecimal::ZERO {
            return Err(AiTradingPlanValidationError::NonPositiveDecimal {
                field: "take_profits.price",
            });
        }
        if self.quantity <= DomainDecimal::ZERO {
            return Err(AiTradingPlanValidationError::NonPositiveDecimal {
                field: "take_profits.quantity",
            });
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AiReviewSchedule {
    next_review_at: u64,
    max_holding_until: u64,
}

impl AiReviewSchedule {
    #[must_use]
    pub const fn next_review_at_unix_millis(&self) -> u64 {
        self.next_review_at
    }

    #[must_use]
    pub const fn max_holding_until_unix_millis(&self) -> u64 {
        self.max_holding_until
    }

    fn validate(&self) -> Result<(), AiTradingPlanValidationError> {
        if self.next_review_at == 0
            || self.max_holding_until == 0
            || self.next_review_at > self.max_holding_until
        {
            return Err(AiTradingPlanValidationError::InvalidReviewSchedule);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AiTradingPlan {
    schema_version: Box<str>,
    plan_id: AiTradingPlanId,
    context_id: AiDecisionContextId,
    instrument_id: InstrumentId,
    action: AiTradingAction,
    #[serde(skip_serializing_if = "Option::is_none")]
    target_trade_plan_id: Option<TradePlanId>,
    valid_until: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    order: Option<AiOrder>,
    #[serde(skip_serializing_if = "Option::is_none")]
    protective_stop: Option<AiProtectiveStop>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    take_profits: Vec<AiTakeProfit>,
    #[serde(skip_serializing_if = "Option::is_none")]
    declared_max_loss_quote: Option<DomainDecimal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    review: Option<AiReviewSchedule>,
    confidence: DomainDecimal,
    thesis: Box<str>,
    invalidation: Box<str>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    risks: Vec<Box<str>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawAiTradingPlan {
    schema_version: Box<str>,
    plan_id: AiTradingPlanId,
    context_id: AiDecisionContextId,
    instrument_id: InstrumentId,
    action: AiTradingAction,
    target_trade_plan_id: Option<TradePlanId>,
    valid_until: u64,
    order: Option<AiOrder>,
    protective_stop: Option<AiProtectiveStop>,
    #[serde(default)]
    take_profits: Vec<AiTakeProfit>,
    declared_max_loss_quote: Option<DomainDecimal>,
    review: Option<AiReviewSchedule>,
    confidence: DomainDecimal,
    thesis: Box<str>,
    invalidation: Box<str>,
    #[serde(default)]
    risks: Vec<Box<str>>,
}

impl AiTradingPlan {
    pub fn from_json(value: &str) -> Result<Self, AiTradingPlanParseError> {
        let raw: RawAiTradingPlan = serde_json::from_str(value)
            .map_err(|error| AiTradingPlanParseError::Json(error.to_string().into()))?;
        Self::try_from(raw).map_err(AiTradingPlanParseError::Validation)
    }

    #[must_use]
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("validated AI trading plan must serialize")
    }

    #[must_use]
    pub fn plan_hash(&self) -> AiTradingPlanHash {
        AiTradingPlanHash(Sha256::digest(self.to_json().as_bytes()).into())
    }

    #[must_use]
    pub fn schema_version(&self) -> &str {
        &self.schema_version
    }

    #[must_use]
    pub const fn plan_id(&self) -> AiTradingPlanId {
        self.plan_id
    }

    #[must_use]
    pub const fn context_id(&self) -> AiDecisionContextId {
        self.context_id
    }

    #[must_use]
    pub fn instrument_id(&self) -> &InstrumentId {
        &self.instrument_id
    }

    #[must_use]
    pub const fn action(&self) -> AiTradingAction {
        self.action
    }

    #[must_use]
    pub const fn target_trade_plan_id(&self) -> Option<TradePlanId> {
        self.target_trade_plan_id
    }

    #[must_use]
    pub const fn valid_until_unix_millis(&self) -> u64 {
        self.valid_until
    }

    #[must_use]
    pub const fn order(&self) -> Option<&AiOrder> {
        self.order.as_ref()
    }

    #[must_use]
    pub const fn protective_stop(&self) -> Option<&AiProtectiveStop> {
        self.protective_stop.as_ref()
    }

    #[must_use]
    pub fn take_profits(&self) -> &[AiTakeProfit] {
        &self.take_profits
    }

    #[must_use]
    pub const fn declared_max_loss_quote(&self) -> Option<DomainDecimal> {
        self.declared_max_loss_quote
    }

    #[must_use]
    pub const fn review(&self) -> Option<&AiReviewSchedule> {
        self.review.as_ref()
    }

    #[must_use]
    pub const fn confidence(&self) -> DomainDecimal {
        self.confidence
    }

    #[must_use]
    pub fn thesis(&self) -> &str {
        &self.thesis
    }

    #[must_use]
    pub fn invalidation(&self) -> &str {
        &self.invalidation
    }

    #[must_use]
    pub fn risks(&self) -> &[Box<str>] {
        &self.risks
    }
}

impl TryFrom<RawAiTradingPlan> for AiTradingPlan {
    type Error = AiTradingPlanValidationError;

    fn try_from(raw: RawAiTradingPlan) -> Result<Self, Self::Error> {
        let plan = Self {
            schema_version: raw.schema_version,
            plan_id: raw.plan_id,
            context_id: raw.context_id,
            instrument_id: raw.instrument_id,
            action: raw.action,
            target_trade_plan_id: raw.target_trade_plan_id,
            valid_until: raw.valid_until,
            order: raw.order,
            protective_stop: raw.protective_stop,
            take_profits: raw.take_profits,
            declared_max_loss_quote: raw.declared_max_loss_quote,
            review: raw.review,
            confidence: raw.confidence,
            thesis: raw.thesis,
            invalidation: raw.invalidation,
            risks: raw.risks,
        };
        plan.validate()?;
        Ok(plan)
    }
}

impl AiTradingPlan {
    fn validate(&self) -> Result<(), AiTradingPlanValidationError> {
        if self.schema_version.as_ref() != AI_TRADING_PLAN_SCHEMA_VERSION_V3 {
            return Err(AiTradingPlanValidationError::UnsupportedSchemaVersion);
        }
        if self.instrument_id.instrument_type() != InstrumentType::Spot {
            return Err(AiTradingPlanValidationError::SpotInstrumentRequired);
        }
        if self.valid_until == 0 {
            return Err(AiTradingPlanValidationError::InvalidTimestamp {
                field: "valid_until",
            });
        }
        if !(DomainDecimal::ZERO..=decimal_one()).contains(&self.confidence) {
            return Err(AiTradingPlanValidationError::ConfidenceOutOfRange);
        }
        validate_text("thesis", &self.thesis)?;
        validate_text("invalidation", &self.invalidation)?;
        if self.risks.len() > MAX_PLAN_RISKS {
            return Err(AiTradingPlanValidationError::TooManyRisks);
        }
        for risk in &self.risks {
            validate_text("risks", risk)?;
        }
        if self.take_profits.len() > MAX_TAKE_PROFITS {
            return Err(AiTradingPlanValidationError::TooManyTakeProfits);
        }
        for take_profit in &self.take_profits {
            take_profit.validate()?;
        }
        if let Some(order) = &self.order {
            order.validate(self.valid_until)?;
        }
        if let Some(stop) = &self.protective_stop {
            stop.validate()?;
        }
        if let Some(review) = &self.review {
            review.validate()?;
        }
        if let Some(max_loss) = self.declared_max_loss_quote
            && max_loss <= DomainDecimal::ZERO
        {
            return Err(AiTradingPlanValidationError::NonPositiveDecimal {
                field: "declared_max_loss_quote",
            });
        }
        self.validate_action_fields()
    }

    fn validate_action_fields(&self) -> Result<(), AiTradingPlanValidationError> {
        let has_execution_fields = self.order.is_some()
            || self.protective_stop.is_some()
            || !self.take_profits.is_empty()
            || self.declared_max_loss_quote.is_some();
        match self.action {
            AiTradingAction::OpenLong => {
                if self.target_trade_plan_id.is_some()
                    || self.order.is_none()
                    || self.protective_stop.is_none()
                    || self.take_profits.is_empty()
                    || self.declared_max_loss_quote.is_none()
                    || self.review.is_none()
                {
                    return Err(AiTradingPlanValidationError::ActionFieldMismatch {
                        action: self.action,
                    });
                }
                let order_quantity = self
                    .order
                    .as_ref()
                    .expect("OPEN_LONG order checked above")
                    .quantity;
                let total = self
                    .take_profits
                    .iter()
                    .try_fold(DomainDecimal::ZERO, |sum, take_profit| {
                        sum.checked_add(take_profit.quantity)
                    });
                if total != Some(order_quantity) {
                    return Err(AiTradingPlanValidationError::TakeProfitQuantityMismatch);
                }
            }
            AiTradingAction::NoTrade => {
                if self.target_trade_plan_id.is_some()
                    || has_execution_fields
                    || self.review.is_some()
                {
                    return Err(AiTradingPlanValidationError::ActionFieldMismatch {
                        action: self.action,
                    });
                }
            }
            AiTradingAction::Hold => {
                if self.target_trade_plan_id.is_none()
                    || has_execution_fields
                    || self.review.is_none()
                {
                    return Err(AiTradingPlanValidationError::ActionFieldMismatch {
                        action: self.action,
                    });
                }
            }
            AiTradingAction::CancelEntry => {
                if self.target_trade_plan_id.is_none()
                    || has_execution_fields
                    || self.review.is_some()
                {
                    return Err(AiTradingPlanValidationError::ActionFieldMismatch {
                        action: self.action,
                    });
                }
            }
            AiTradingAction::ModifyProtection => {
                if self.target_trade_plan_id.is_none()
                    || self.order.is_some()
                    || (self.protective_stop.is_none() && self.take_profits.is_empty())
                    || self.declared_max_loss_quote.is_none()
                    || self.review.is_none()
                {
                    return Err(AiTradingPlanValidationError::ActionFieldMismatch {
                        action: self.action,
                    });
                }
            }
            AiTradingAction::Reduce | AiTradingAction::Exit => {
                if self.target_trade_plan_id.is_none()
                    || self.order.is_none()
                    || self.protective_stop.is_some()
                    || !self.take_profits.is_empty()
                    || self.declared_max_loss_quote.is_some()
                    || self.review.is_none()
                {
                    return Err(AiTradingPlanValidationError::ActionFieldMismatch {
                        action: self.action,
                    });
                }
            }
        }
        Ok(())
    }
}

fn decimal_one() -> DomainDecimal {
    DomainDecimal::from_mantissa_scale(1, 0).expect("one is a valid domain decimal")
}

fn validate_text(field: &'static str, value: &str) -> Result<(), AiTradingPlanValidationError> {
    if value.trim().is_empty() || value.len() > MAX_PLAN_TEXT_LENGTH {
        return Err(AiTradingPlanValidationError::InvalidText { field });
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AiTradingPlanParseError {
    Json(Box<str>),
    Validation(AiTradingPlanValidationError),
}

impl fmt::Display for AiTradingPlanParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(error) => write!(formatter, "invalid AITradingPlan JSON: {error}"),
            Self::Validation(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for AiTradingPlanParseError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AiTradingPlanValidationError {
    UnsupportedSchemaVersion,
    SpotInstrumentRequired,
    InvalidTimestamp { field: &'static str },
    NonPositiveDecimal { field: &'static str },
    NegativeDecimal { field: &'static str },
    InvalidOrderPrice,
    InvalidProtectiveStopPrice,
    InvalidReviewSchedule,
    ConfidenceOutOfRange,
    InvalidText { field: &'static str },
    TooManyRisks,
    TooManyTakeProfits,
    ActionFieldMismatch { action: AiTradingAction },
    TakeProfitQuantityMismatch,
}

impl fmt::Display for AiTradingPlanValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedSchemaVersion => {
                formatter.write_str("only AITradingPlan schema 3.0 is active")
            }
            Self::SpotInstrumentRequired => {
                formatter.write_str("AITradingPlan v3 requires a Spot instrument")
            }
            Self::InvalidTimestamp { field } => write!(formatter, "{field} is invalid"),
            Self::NonPositiveDecimal { field } => write!(formatter, "{field} must be positive"),
            Self::NegativeDecimal { field } => write!(formatter, "{field} must not be negative"),
            Self::InvalidOrderPrice => formatter.write_str(
                "LIMIT orders require a positive limit_price and MARKET orders forbid it",
            ),
            Self::InvalidProtectiveStopPrice => formatter.write_str(
                "LIMIT protective stops require a positive limit_price and MARKET stops forbid it",
            ),
            Self::InvalidReviewSchedule => formatter.write_str(
                "review times must be positive and next_review_at must not exceed max_holding_until",
            ),
            Self::ConfidenceOutOfRange => {
                formatter.write_str("confidence must be an exact decimal from 0 through 1")
            }
            Self::InvalidText { field } => {
                write!(formatter, "{field} must be non-empty and bounded")
            }
            Self::TooManyRisks => write!(formatter, "risks exceeds {MAX_PLAN_RISKS} entries"),
            Self::TooManyTakeProfits => {
                write!(formatter, "take_profits exceeds {MAX_TAKE_PROFITS} entries")
            }
            Self::ActionFieldMismatch { action } => {
                write!(formatter, "fields do not match {action:?} action semantics")
            }
            Self::TakeProfitQuantityMismatch => formatter
                .write_str("OPEN_LONG take-profit quantities must exactly cover order quantity"),
        }
    }
}

impl std::error::Error for AiTradingPlanValidationError {}
