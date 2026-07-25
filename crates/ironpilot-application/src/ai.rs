use core::fmt;

use ironpilot_domain::{
    AiDecisionContext, AiTradingPlan, MAX_PLAN_RISKS, MAX_PLAN_TEXT_LENGTH, MAX_TAKE_PROFITS,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub const AI_TRADING_PROMPT_VERSION_V1: &str = "ironpilot-deepseek-trading-prompt-v1";
pub const PROMPT_CANDLES_PER_TIMEFRAME: usize = 120;
pub const MAX_REPLAN_REASONS: usize = 8;
pub const MAX_REPLAN_REASON_LENGTH: usize = 512;

const SYSTEM_INSTRUCTIONS: &str = r#"You are the trading-decision authority for an AI-dominant Bybit Spot system.
Use only the supplied JSON facts. Independently decide whether to trade or manage the existing plan.
Return exactly one JSON object conforming to AITradingPlan schema_version "3.0", with no Markdown and no text outside the JSON object.
All prices, quantities, losses, slippage, and confidence values must be exact base-10 JSON strings, never JSON numbers.
The plan_id must be a new non-nil UUID. Copy context_id and instrument_id exactly from the Decision Context.
Choose exactly one action: OPEN_LONG, NO_TRADE, HOLD, CANCEL_ENTRY, MODIFY_PROTECTION, REDUCE, or EXIT.
OPEN_LONG requires order, protective_stop, one or more take_profits whose quantities sum exactly to order.quantity, declared_max_loss_quote, and review; omit target_trade_plan_id.
NO_TRADE omits target_trade_plan_id, order, protective_stop, take_profits, declared_max_loss_quote, and review.
HOLD requires target_trade_plan_id and review; omit execution fields.
CANCEL_ENTRY requires target_trade_plan_id; omit execution fields and review.
MODIFY_PROTECTION requires target_trade_plan_id, protective_stop and/or take_profits, declared_max_loss_quote, and review; omit order.
REDUCE and EXIT require target_trade_plan_id, order, and review; omit protective_stop, take_profits, and declared_max_loss_quote.
LIMIT orders require a positive limit_price. MARKET orders must omit limit_price. Use only GTC, IOC, or FOK.
Every plan requires valid_until, confidence, non-empty thesis, non-empty invalidation, and a risks array.
Use instrument rules exactly as provided. Do not assume the system will round, resize, move a stop, or repair an invalid plan.
Do not exceed user_authorization.maximum_loss_quote. If no complete defensible plan exists, output NO_TRADE."#;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AiTradingPromptHash([u8; 32]);

impl fmt::Display for AiTradingPromptHash {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AiPlanRejectionFeedback {
    rejected_plan_json: Box<str>,
    reasons: Vec<Box<str>>,
}

impl AiPlanRejectionFeedback {
    pub fn new<I, S>(
        context: &AiDecisionContext,
        rejected_plan: &AiTradingPlan,
        reasons: I,
    ) -> Result<Self, AiPromptError>
    where
        I: IntoIterator<Item = S>,
        S: Into<Box<str>>,
    {
        if rejected_plan.context_id() != context.context_id()
            || rejected_plan.instrument_id() != context.instrument_id()
        {
            return Err(AiPromptError::ReplanProvenanceMismatch);
        }
        let reasons = reasons.into_iter().map(Into::into).collect::<Vec<_>>();
        if reasons.is_empty() || reasons.len() > MAX_REPLAN_REASONS {
            return Err(AiPromptError::InvalidReplanReasonCount);
        }
        if reasons.iter().any(|reason| {
            reason.trim().is_empty()
                || reason.len() > MAX_REPLAN_REASON_LENGTH
                || reason.chars().any(char::is_control)
        }) {
            return Err(AiPromptError::InvalidReplanReason);
        }
        Ok(Self {
            rejected_plan_json: rejected_plan.to_json().into_boxed_str(),
            reasons,
        })
    }

    #[must_use]
    pub fn rejected_plan_json(&self) -> &str {
        &self.rejected_plan_json
    }

    #[must_use]
    pub fn reasons(&self) -> &[Box<str>] {
        &self.reasons
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AiTradingPrompt {
    version: &'static str,
    system_message: &'static str,
    user_message: Box<str>,
    prompt_hash: AiTradingPromptHash,
    is_replan: bool,
}

impl AiTradingPrompt {
    pub fn initial(context: &AiDecisionContext) -> Result<Self, AiPromptError> {
        Self::build(context, None)
    }

    pub fn replan(
        context: &AiDecisionContext,
        feedback: &AiPlanRejectionFeedback,
    ) -> Result<Self, AiPromptError> {
        Self::build(context, Some(feedback))
    }

    fn build(
        context: &AiDecisionContext,
        feedback: Option<&AiPlanRejectionFeedback>,
    ) -> Result<Self, AiPromptError> {
        let mut prompt_context: Value = serde_json::from_str(context.to_json())
            .map_err(|_| AiPromptError::InvalidDecisionContextJson)?;
        let market = prompt_context
            .get_mut("market")
            .and_then(Value::as_object_mut)
            .ok_or(AiPromptError::InvalidDecisionContextJson)?;
        let source_candle_counts = json!({
            "candles_15m": retain_recent_candles(market, "candles_15m")?,
            "candles_1h": retain_recent_candles(market, "candles_1h")?
        });
        let replan_feedback = feedback.map(|feedback| {
            let rejected_plan: Value = serde_json::from_str(feedback.rejected_plan_json())
                .expect("validated AITradingPlan must serialize as JSON");
            json!({
                "instruction": "The previous plan was rejected. Reconsider the same facts and return one complete replacement plan. Do not merely patch or repeat the rejected fields.",
                "rejected_plan": rejected_plan,
                "rejection_reasons": feedback.reasons()
            })
        });
        let prompt_payload = json!({
            "prompt_version": AI_TRADING_PROMPT_VERSION_V1,
            "source_context": {
                "context_id": context.context_id().to_string(),
                "context_hash": context.context_hash().to_string(),
                "canonical_candle_counts": source_candle_counts,
                "prompt_candles_per_timeframe": PROMPT_CANDLES_PER_TIMEFRAME
            },
            "decision_context": prompt_context,
            "replan_feedback": replan_feedback,
            "output_limits": {
                "maximum_take_profits": MAX_TAKE_PROFITS,
                "maximum_risks": MAX_PLAN_RISKS,
                "maximum_text_length": MAX_PLAN_TEXT_LENGTH
            }
        });
        let user_message = serde_json::to_string(&prompt_payload)
            .expect("bounded prompt payload must serialize")
            .into_boxed_str();
        let mut hasher = Sha256::new();
        hasher.update(AI_TRADING_PROMPT_VERSION_V1.as_bytes());
        hasher.update([0]);
        hasher.update(SYSTEM_INSTRUCTIONS.as_bytes());
        hasher.update([0]);
        hasher.update(user_message.as_bytes());
        let prompt_hash = AiTradingPromptHash(hasher.finalize().into());
        Ok(Self {
            version: AI_TRADING_PROMPT_VERSION_V1,
            system_message: SYSTEM_INSTRUCTIONS,
            user_message,
            prompt_hash,
            is_replan: feedback.is_some(),
        })
    }

    #[must_use]
    pub const fn version(&self) -> &'static str {
        self.version
    }

    #[must_use]
    pub const fn system_message(&self) -> &'static str {
        self.system_message
    }

    #[must_use]
    pub fn user_message(&self) -> &str {
        &self.user_message
    }

    #[must_use]
    pub const fn prompt_hash(&self) -> AiTradingPromptHash {
        self.prompt_hash
    }

    #[must_use]
    pub const fn is_replan(&self) -> bool {
        self.is_replan
    }

    #[must_use]
    pub fn conservative_input_token_bound(&self) -> u64 {
        u64::try_from(
            self.system_message
                .len()
                .saturating_add(self.user_message.len()),
        )
        .unwrap_or(u64::MAX)
    }
}

fn retain_recent_candles(
    market: &mut serde_json::Map<String, Value>,
    field: &'static str,
) -> Result<usize, AiPromptError> {
    let candles = market
        .get_mut(field)
        .and_then(Value::as_array_mut)
        .ok_or(AiPromptError::InvalidDecisionContextJson)?;
    let canonical_count = candles.len();
    if canonical_count < PROMPT_CANDLES_PER_TIMEFRAME {
        return Err(AiPromptError::InsufficientPromptCandles { field });
    }
    let keep_from = canonical_count - PROMPT_CANDLES_PER_TIMEFRAME;
    candles.drain(..keep_from);
    Ok(canonical_count)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AiPromptError {
    InvalidDecisionContextJson,
    InsufficientPromptCandles { field: &'static str },
    ReplanProvenanceMismatch,
    InvalidReplanReasonCount,
    InvalidReplanReason,
}

impl fmt::Display for AiPromptError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDecisionContextJson => {
                formatter.write_str("AI Decision Context JSON is invalid")
            }
            Self::InsufficientPromptCandles { field } => write!(
                formatter,
                "{field} does not contain the required prompt candle window"
            ),
            Self::ReplanProvenanceMismatch => {
                formatter.write_str("rejected plan does not belong to this Decision Context")
            }
            Self::InvalidReplanReasonCount => write!(
                formatter,
                "replan feedback requires 1..={MAX_REPLAN_REASONS} rejection reasons"
            ),
            Self::InvalidReplanReason => {
                formatter.write_str("replan feedback contains an invalid rejection reason")
            }
        }
    }
}

impl std::error::Error for AiPromptError {}
