use core::fmt;

use ironpilot_domain::{
    AI_TRADING_PLAN_SCHEMA_VERSION_V3, AccountOrderFact, AccountOrderSide, AiDecisionContext,
    AiOrder, AiOrderType, AiTradingAction, AiTradingPlan, AiTradingPlanHash, DomainDecimal,
    InstrumentId, InstrumentRulesSnapshot, ManagedPosition, PortfolioSnapshot, SpotInstrumentRules,
    TopOfBook, TradePlanActionId, TradePlanId, TradePlanState,
};
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::{
    AiPlanRejectionFeedback, AiPromptError, ExecutionMode, MAX_REPLAN_REASONS,
    ValidatedRuntimeConfig,
};

pub const EXECUTION_VALIDATOR_VERSION_V1: &str = "ironpilot-execution-validator-v1";
pub const MAX_VALIDATION_REJECTIONS: usize = MAX_REPLAN_REASONS;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExecutionAuthorization {
    execution_mode: ExecutionMode,
    ai_trading_plans: bool,
    instrument_ids: Vec<InstrumentId>,
}

impl ExecutionAuthorization {
    pub fn new(
        execution_mode: ExecutionMode,
        ai_trading_plans: bool,
        mut instrument_ids: Vec<InstrumentId>,
    ) -> Result<Self, ExecutionValidationInputError> {
        if instrument_ids.is_empty() {
            return Err(ExecutionValidationInputError::EmptyInstrumentScope);
        }
        instrument_ids.sort();
        if instrument_ids.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(ExecutionValidationInputError::DuplicateInstrument);
        }
        Ok(Self {
            execution_mode,
            ai_trading_plans,
            instrument_ids,
        })
    }

    #[must_use]
    pub fn from_runtime_config(config: &ValidatedRuntimeConfig) -> Self {
        let mut instrument_ids: Vec<_> = config.instrument_ids().cloned().collect();
        instrument_ids.sort();
        Self {
            execution_mode: config.permissions().execution_mode(),
            ai_trading_plans: config.permissions().ai_trading_plans(),
            instrument_ids,
        }
    }

    #[must_use]
    pub const fn execution_mode(&self) -> ExecutionMode {
        self.execution_mode
    }

    #[must_use]
    pub const fn ai_trading_plans(&self) -> bool {
        self.ai_trading_plans
    }

    #[must_use]
    pub fn allows_instrument(&self, instrument_id: &InstrumentId) -> bool {
        self.instrument_ids.binary_search(instrument_id).is_ok()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExecutionValidationPolicy {
    taker_fee_rate: DomainDecimal,
    maximum_book_age_millis: u64,
    maximum_price_limit_age_millis: u64,
}

impl ExecutionValidationPolicy {
    pub fn new(
        taker_fee_rate: DomainDecimal,
        maximum_book_age_millis: u64,
        maximum_price_limit_age_millis: u64,
    ) -> Result<Self, ExecutionValidationInputError> {
        if taker_fee_rate < DomainDecimal::ZERO || taker_fee_rate >= decimal_one() {
            return Err(ExecutionValidationInputError::InvalidFeeRate);
        }
        if maximum_book_age_millis == 0 || maximum_price_limit_age_millis == 0 {
            return Err(ExecutionValidationInputError::InvalidFreshnessLimit);
        }
        Ok(Self {
            taker_fee_rate,
            maximum_book_age_millis,
            maximum_price_limit_age_millis,
        })
    }

    #[must_use]
    pub const fn taker_fee_rate(self) -> DomainDecimal {
        self.taker_fee_rate
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpotOrderPriceLimits {
    instrument_id: InstrumentId,
    highest_buy_price: DomainDecimal,
    lowest_sell_price: DomainDecimal,
    observed_at_unix_millis: u64,
}

impl SpotOrderPriceLimits {
    pub fn new(
        instrument_id: InstrumentId,
        highest_buy_price: DomainDecimal,
        lowest_sell_price: DomainDecimal,
        observed_at_unix_millis: u64,
    ) -> Result<Self, ExecutionValidationInputError> {
        if highest_buy_price <= DomainDecimal::ZERO || lowest_sell_price <= DomainDecimal::ZERO {
            return Err(ExecutionValidationInputError::InvalidPriceLimit);
        }
        if observed_at_unix_millis == 0 {
            return Err(ExecutionValidationInputError::InvalidTimestamp);
        }
        Ok(Self {
            instrument_id,
            highest_buy_price,
            lowest_sell_price,
            observed_at_unix_millis,
        })
    }

    #[must_use]
    pub fn instrument_id(&self) -> &InstrumentId {
        &self.instrument_id
    }

    #[must_use]
    pub const fn highest_buy_price(&self) -> DomainDecimal {
        self.highest_buy_price
    }

    #[must_use]
    pub const fn lowest_sell_price(&self) -> DomainDecimal {
        self.lowest_sell_price
    }

    #[must_use]
    pub const fn observed_at_unix_millis(&self) -> u64 {
        self.observed_at_unix_millis
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManagedPositionExecutionFact {
    trade_plan_id: TradePlanId,
    position: ManagedPosition,
    average_entry_price: DomainDecimal,
    current_protective_stop_price: DomainDecimal,
}

impl ManagedPositionExecutionFact {
    pub fn new(
        trade_plan_id: TradePlanId,
        position: ManagedPosition,
        average_entry_price: DomainDecimal,
        current_protective_stop_price: DomainDecimal,
    ) -> Result<Self, ExecutionValidationInputError> {
        if average_entry_price <= DomainDecimal::ZERO
            || current_protective_stop_price <= DomainDecimal::ZERO
        {
            return Err(ExecutionValidationInputError::InvalidManagedPositionPrice);
        }
        Ok(Self {
            trade_plan_id,
            position,
            average_entry_price,
            current_protective_stop_price,
        })
    }

    #[must_use]
    pub const fn trade_plan_id(&self) -> TradePlanId {
        self.trade_plan_id
    }

    #[must_use]
    pub const fn position(&self) -> &ManagedPosition {
        &self.position
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActiveTradePlanFact {
    trade_plan_id: TradePlanId,
    instrument_id: InstrumentId,
    state: TradePlanState,
}

impl ActiveTradePlanFact {
    #[must_use]
    pub const fn new(
        trade_plan_id: TradePlanId,
        instrument_id: InstrumentId,
        state: TradePlanState,
    ) -> Self {
        Self {
            trade_plan_id,
            instrument_id,
            state,
        }
    }
}

pub struct ExecutionValidationRequest<'a> {
    pub action_id: TradePlanActionId,
    pub trade_plan_id: TradePlanId,
    pub context: &'a AiDecisionContext,
    pub plan: &'a AiTradingPlan,
    pub rules: &'a InstrumentRulesSnapshot,
    pub portfolio: &'a PortfolioSnapshot,
    pub managed_positions: &'a [ManagedPositionExecutionFact],
    pub open_orders: &'a [AccountOrderFact],
    pub active_trade_plans: &'a [ActiveTradePlanFact],
    pub top_of_book: &'a TopOfBook,
    pub price_limits: &'a SpotOrderPriceLimits,
    pub current_maximum_loss_quote: DomainDecimal,
    pub authorization: &'a ExecutionAuthorization,
    pub policy: ExecutionValidationPolicy,
    pub validated_at_unix_millis: u64,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ExecutionValidationRejection {
    UnsupportedSchema,
    ContextMismatch,
    InstrumentMismatch,
    InstrumentNotAuthorized,
    AiTradingNotAuthorized,
    ExecutionModeNotAuthorized,
    StaleContext,
    StalePlan,
    StaleOrder,
    StaleInstrumentRules,
    InstrumentRulesChanged,
    MissingInstrumentRules,
    StaleBook,
    StalePriceLimits,
    AccountStateChanged,
    MaximumLossAuthorizationChanged,
    TargetTradePlanMismatch,
    TargetTradePlanUnavailable,
    ConflictingOrder,
    MissingEntryOrder,
    UnbalancedPortfolio,
    InvalidPriceIncrement,
    InvalidQuantityIncrement,
    OrderQuantityTooLarge,
    MinimumOrderAmount,
    PriceOutsideExchangeLimit,
    InvalidTimeInForce,
    InsufficientBalance,
    ManagedAssetViolation,
    InvalidExitQuantity,
    DeclaredMaximumLossTooLow,
    MaximumLossExceeded,
    ArithmeticFailure,
}

impl ExecutionValidationRejection {
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::UnsupportedSchema => "UNSUPPORTED_SCHEMA",
            Self::ContextMismatch => "CONTEXT_MISMATCH",
            Self::InstrumentMismatch => "INSTRUMENT_MISMATCH",
            Self::InstrumentNotAuthorized => "INSTRUMENT_NOT_AUTHORIZED",
            Self::AiTradingNotAuthorized => "AI_TRADING_NOT_AUTHORIZED",
            Self::ExecutionModeNotAuthorized => "EXECUTION_MODE_NOT_AUTHORIZED",
            Self::StaleContext => "STALE_CONTEXT",
            Self::StalePlan => "STALE_PLAN",
            Self::StaleOrder => "STALE_ORDER",
            Self::StaleInstrumentRules => "STALE_INSTRUMENT_RULES",
            Self::InstrumentRulesChanged => "INSTRUMENT_RULES_CHANGED",
            Self::MissingInstrumentRules => "MISSING_INSTRUMENT_RULES",
            Self::StaleBook => "STALE_BOOK",
            Self::StalePriceLimits => "STALE_PRICE_LIMITS",
            Self::AccountStateChanged => "ACCOUNT_STATE_CHANGED",
            Self::MaximumLossAuthorizationChanged => "MAXIMUM_LOSS_AUTHORIZATION_CHANGED",
            Self::TargetTradePlanMismatch => "TARGET_TRADE_PLAN_MISMATCH",
            Self::TargetTradePlanUnavailable => "TARGET_TRADE_PLAN_UNAVAILABLE",
            Self::ConflictingOrder => "CONFLICTING_ORDER",
            Self::MissingEntryOrder => "MISSING_ENTRY_ORDER",
            Self::UnbalancedPortfolio => "UNBALANCED_PORTFOLIO",
            Self::InvalidPriceIncrement => "INVALID_PRICE_INCREMENT",
            Self::InvalidQuantityIncrement => "INVALID_QUANTITY_INCREMENT",
            Self::OrderQuantityTooLarge => "ORDER_QUANTITY_TOO_LARGE",
            Self::MinimumOrderAmount => "MINIMUM_ORDER_AMOUNT",
            Self::PriceOutsideExchangeLimit => "PRICE_OUTSIDE_EXCHANGE_LIMIT",
            Self::InvalidTimeInForce => "INVALID_TIME_IN_FORCE",
            Self::InsufficientBalance => "INSUFFICIENT_BALANCE",
            Self::ManagedAssetViolation => "MANAGED_ASSET_VIOLATION",
            Self::InvalidExitQuantity => "INVALID_EXIT_QUANTITY",
            Self::DeclaredMaximumLossTooLow => "DECLARED_MAXIMUM_LOSS_TOO_LOW",
            Self::MaximumLossExceeded => "MAXIMUM_LOSS_EXCEEDED",
            Self::ArithmeticFailure => "ARITHMETIC_FAILURE",
        }
    }

    #[must_use]
    pub const fn feedback(self) -> &'static str {
        match self {
            Self::InvalidPriceIncrement => {
                "Every AI-supplied price must be an exact exchange tick multiple."
            }
            Self::InvalidQuantityIncrement => {
                "Every AI-supplied quantity must be an exact exchange quantity-step multiple."
            }
            Self::MinimumOrderAmount => {
                "The AI-supplied order does not meet the exchange minimum amount."
            }
            Self::PriceOutsideExchangeLimit => {
                "The AI-supplied limit price is outside the current exchange price limit."
            }
            Self::DeclaredMaximumLossTooLow => {
                "The declared maximum loss is below the independently recalculated worst loss."
            }
            Self::MaximumLossExceeded => {
                "The independently recalculated or declared loss exceeds user authorization."
            }
            Self::AccountStateChanged => {
                "Account, managed-position, or open-order facts changed; request a fresh Context."
            }
            Self::StaleContext | Self::StalePlan | Self::StaleOrder => {
                "The Context or AI plan is stale; request a fresh Context and plan."
            }
            _ => "The plan failed deterministic execution compatibility or authorization checks.",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExecutionValidationOutcome {
    Accept,
    Reject,
}

impl ExecutionValidationOutcome {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Accept => "ACCEPT",
            Self::Reject => "REJECT",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ExecutionValidationHash([u8; 32]);

impl fmt::Display for ExecutionValidationHash {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExecutionValidationDecision {
    action_id: TradePlanActionId,
    trade_plan_id: TradePlanId,
    context_hash: Box<str>,
    plan_id: Box<str>,
    plan_hash: AiTradingPlanHash,
    outcome: ExecutionValidationOutcome,
    rejections: Vec<ExecutionValidationRejection>,
    recalculated_maximum_loss_quote: Option<DomainDecimal>,
    authorized_maximum_loss_quote: DomainDecimal,
    validated_at_unix_millis: u64,
    evidence_json: Box<str>,
    validation_hash: ExecutionValidationHash,
}

impl ExecutionValidationDecision {
    #[must_use]
    pub const fn action_id(&self) -> TradePlanActionId {
        self.action_id
    }

    #[must_use]
    pub const fn trade_plan_id(&self) -> TradePlanId {
        self.trade_plan_id
    }

    #[must_use]
    pub fn context_hash(&self) -> &str {
        &self.context_hash
    }

    #[must_use]
    pub fn plan_id(&self) -> &str {
        &self.plan_id
    }

    #[must_use]
    pub const fn plan_hash(&self) -> AiTradingPlanHash {
        self.plan_hash
    }

    #[must_use]
    pub const fn outcome(&self) -> ExecutionValidationOutcome {
        self.outcome
    }

    #[must_use]
    pub fn rejections(&self) -> &[ExecutionValidationRejection] {
        &self.rejections
    }

    #[must_use]
    pub const fn recalculated_maximum_loss_quote(&self) -> Option<DomainDecimal> {
        self.recalculated_maximum_loss_quote
    }

    #[must_use]
    pub const fn authorized_maximum_loss_quote(&self) -> DomainDecimal {
        self.authorized_maximum_loss_quote
    }

    #[must_use]
    pub const fn validated_at_unix_millis(&self) -> u64 {
        self.validated_at_unix_millis
    }

    #[must_use]
    pub fn evidence_json(&self) -> &str {
        &self.evidence_json
    }

    #[must_use]
    pub const fn validation_hash(&self) -> ExecutionValidationHash {
        self.validation_hash
    }

    #[must_use]
    pub fn authorizes_unchanged(&self, plan: &AiTradingPlan) -> bool {
        self.outcome == ExecutionValidationOutcome::Accept
            && self.plan_id.as_ref() == plan.plan_id().to_string()
            && self.plan_hash == plan.plan_hash()
    }

    #[must_use]
    pub fn rejection_feedback(&self) -> Vec<(&'static str, &'static str)> {
        self.rejections
            .iter()
            .map(|reason| (reason.code(), reason.feedback()))
            .collect()
    }

    pub fn replan_feedback(
        &self,
        context: &AiDecisionContext,
        plan: &AiTradingPlan,
    ) -> Result<AiPlanRejectionFeedback, AiPromptError> {
        if self.outcome != ExecutionValidationOutcome::Reject
            || self.plan_id.as_ref() != plan.plan_id().to_string()
            || self.plan_hash != plan.plan_hash()
        {
            return Err(AiPromptError::ReplanProvenanceMismatch);
        }
        AiPlanRejectionFeedback::new(
            context,
            plan,
            self.rejections
                .iter()
                .map(|reason| format!("{}: {}", reason.code(), reason.feedback())),
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExecutionValidator;

impl ExecutionValidator {
    #[must_use]
    pub fn validate(request: ExecutionValidationRequest<'_>) -> ExecutionValidationDecision {
        let mut state = ValidationState::new(&request);
        state.validate_common(&request);
        if let Some(rules) = state.rules.clone() {
            state.validate_action(&request, &rules);
        }
        state.finish(&request)
    }
}

struct ValidationState {
    rejections: Vec<ExecutionValidationRejection>,
    recalculated_maximum_loss_quote: Option<DomainDecimal>,
    rules: Option<SpotInstrumentRules>,
}

impl ValidationState {
    fn new(_request: &ExecutionValidationRequest<'_>) -> Self {
        Self {
            rejections: Vec::new(),
            recalculated_maximum_loss_quote: None,
            rules: None,
        }
    }

    fn reject(&mut self, reason: ExecutionValidationRejection) {
        if self.rejections.len() < MAX_VALIDATION_REJECTIONS && !self.rejections.contains(&reason) {
            self.rejections.push(reason);
        }
    }

    fn validate_common(&mut self, request: &ExecutionValidationRequest<'_>) {
        let now = request.validated_at_unix_millis;
        let context = request.context;
        let plan = request.plan;
        if plan.schema_version() != AI_TRADING_PLAN_SCHEMA_VERSION_V3 {
            self.reject(ExecutionValidationRejection::UnsupportedSchema);
        }
        if plan.context_id() != context.context_id() {
            self.reject(ExecutionValidationRejection::ContextMismatch);
        }
        if plan.instrument_id() != context.instrument_id() {
            self.reject(ExecutionValidationRejection::InstrumentMismatch);
        }
        if !request
            .authorization
            .allows_instrument(plan.instrument_id())
        {
            self.reject(ExecutionValidationRejection::InstrumentNotAuthorized);
        }
        if !request.authorization.ai_trading_plans() {
            self.reject(ExecutionValidationRejection::AiTradingNotAuthorized);
        }
        if now == 0 || context.is_expired_at(now) {
            self.reject(ExecutionValidationRejection::StaleContext);
        }
        if now >= plan.valid_until_unix_millis()
            || plan.valid_until_unix_millis() > context.valid_until_unix_millis()
        {
            self.reject(ExecutionValidationRejection::StalePlan);
        }
        if let Some(order) = plan.order()
            && now >= order.expires_at_unix_millis()
        {
            self.reject(ExecutionValidationRejection::StaleOrder);
        }
        if request.rules.is_expired_at(now)
            || request.rules.observed_at_unix_millis() > now
            || request.rules.server_time().response_unix_millis() > now
        {
            self.reject(ExecutionValidationRejection::StaleInstrumentRules);
        }
        if request.rules.rules_hash() != context.instrument_rules_hash() {
            self.reject(ExecutionValidationRejection::InstrumentRulesChanged);
        }
        self.rules = request
            .rules
            .rules()
            .iter()
            .find(|rules| rules.instrument_id() == plan.instrument_id())
            .cloned();
        if self.rules.is_none() {
            self.reject(ExecutionValidationRejection::MissingInstrumentRules);
        }
        if request.top_of_book.instrument_id() != plan.instrument_id()
            || request.top_of_book.observed_at_unix_millis() > now
            || now.saturating_sub(request.top_of_book.observed_at_unix_millis())
                > request.policy.maximum_book_age_millis
        {
            self.reject(ExecutionValidationRejection::StaleBook);
        }
        if request.price_limits.instrument_id() != plan.instrument_id()
            || request.price_limits.observed_at_unix_millis() > now
            || now.saturating_sub(request.price_limits.observed_at_unix_millis())
                > request.policy.maximum_price_limit_age_millis
        {
            self.reject(ExecutionValidationRejection::StalePriceLimits);
        }
        if request.portfolio.observed_at_unix_millis() > now
            || request.portfolio.snapshot_hash() != context.portfolio_hash()
            || !managed_positions_match_context(request)
            || !open_orders_match_context(request)
        {
            self.reject(ExecutionValidationRejection::AccountStateChanged);
        }
        if request.current_maximum_loss_quote <= DomainDecimal::ZERO
            || request.current_maximum_loss_quote != context.maximum_loss_quote()
        {
            self.reject(ExecutionValidationRejection::MaximumLossAuthorizationChanged);
        }
        match plan.target_trade_plan_id() {
            Some(target) if target != request.trade_plan_id => {
                self.reject(ExecutionValidationRejection::TargetTradePlanMismatch);
            }
            None if !matches!(
                plan.action(),
                AiTradingAction::OpenLong | AiTradingAction::NoTrade
            ) =>
            {
                self.reject(ExecutionValidationRejection::TargetTradePlanMismatch);
            }
            _ => {}
        }
    }

    fn validate_action(
        &mut self,
        request: &ExecutionValidationRequest<'_>,
        rules: &SpotInstrumentRules,
    ) {
        match request.plan.action() {
            AiTradingAction::OpenLong => self.validate_open(request, rules),
            AiTradingAction::NoTrade => {}
            AiTradingAction::Hold => {
                self.require_target_state(
                    request,
                    &[
                        TradePlanState::Accepted,
                        TradePlanState::EntryPending,
                        TradePlanState::Active,
                        TradePlanState::RecoveryRequired,
                    ],
                );
            }
            AiTradingAction::CancelEntry => self.validate_cancel_entry(request),
            AiTradingAction::ModifyProtection => {
                self.validate_modify_protection(request, rules);
            }
            AiTradingAction::Reduce => self.validate_sell(request, rules, false),
            AiTradingAction::Exit => self.validate_sell(request, rules, true),
        }
    }

    fn require_paper_permission(&mut self, request: &ExecutionValidationRequest<'_>) {
        if request.authorization.execution_mode() != ExecutionMode::Paper {
            self.reject(ExecutionValidationRejection::ExecutionModeNotAuthorized);
        }
    }

    fn require_target_state(
        &mut self,
        request: &ExecutionValidationRequest<'_>,
        allowed: &[TradePlanState],
    ) {
        let available = request.active_trade_plans.iter().any(|fact| {
            fact.trade_plan_id == request.trade_plan_id
                && fact.instrument_id == *request.plan.instrument_id()
                && allowed.contains(&fact.state)
        });
        if !available {
            self.reject(ExecutionValidationRejection::TargetTradePlanUnavailable);
        }
    }

    fn validate_open(
        &mut self,
        request: &ExecutionValidationRequest<'_>,
        rules: &SpotInstrumentRules,
    ) {
        self.require_paper_permission(request);
        if !request.portfolio.allows_new_entries() {
            self.reject(ExecutionValidationRejection::UnbalancedPortfolio);
        }
        if request.active_trade_plans.iter().any(|fact| {
            fact.instrument_id == *request.plan.instrument_id() && !fact.state.is_terminal()
        }) || request
            .open_orders
            .iter()
            .any(|order| order.instrument_id() == request.plan.instrument_id())
        {
            self.reject(ExecutionValidationRejection::ConflictingOrder);
        }
        let Some(order) = request.plan.order() else {
            return;
        };
        let Some(entry_price) = self.validate_order(request, rules, order, AccountOrderSide::Buy)
        else {
            return;
        };
        self.validate_protection_prices(request.plan, rules);
        let quantity = order.quantity();
        let Some(entry_notional) = entry_price.checked_mul(quantity) else {
            self.reject(ExecutionValidationRejection::ArithmeticFailure);
            return;
        };
        let Some(entry_fee) = entry_notional.checked_mul(request.policy.taker_fee_rate()) else {
            self.reject(ExecutionValidationRejection::ArithmeticFailure);
            return;
        };
        let required_quote = entry_notional
            .checked_add(entry_fee)
            .and_then(|value| value.checked_add(order.max_slippage_quote()));
        let available_quote = request
            .portfolio
            .assets()
            .iter()
            .find(|asset| asset.asset() == rules.quote_asset())
            .map(|asset| asset.exchange_available_quantity());
        if required_quote.is_none() || available_quote.is_none() || available_quote < required_quote
        {
            self.reject(ExecutionValidationRejection::InsufficientBalance);
        }
        let Some(stop) = request.plan.protective_stop() else {
            return;
        };
        let stop_price = stop.limit_price().unwrap_or(stop.trigger_price());
        let recalculated = worst_long_loss(
            entry_price,
            stop_price,
            quantity,
            request.policy.taker_fee_rate(),
            order.max_slippage_quote(),
        );
        self.apply_maximum_loss(request, recalculated);
    }

    fn validate_cancel_entry(&mut self, request: &ExecutionValidationRequest<'_>) {
        self.require_paper_permission(request);
        self.require_target_state(
            request,
            &[TradePlanState::Accepted, TradePlanState::EntryPending],
        );
        if !request.open_orders.iter().any(|order| {
            order.instrument_id() == request.plan.instrument_id()
                && order.side() == AccountOrderSide::Buy
        }) {
            self.reject(ExecutionValidationRejection::MissingEntryOrder);
        }
    }

    fn validate_modify_protection(
        &mut self,
        request: &ExecutionValidationRequest<'_>,
        rules: &SpotInstrumentRules,
    ) {
        self.require_paper_permission(request);
        self.require_target_state(
            request,
            &[TradePlanState::Active, TradePlanState::RecoveryRequired],
        );
        let Some(position) = managed_position_for(request) else {
            self.reject(ExecutionValidationRejection::ManagedAssetViolation);
            return;
        };
        self.validate_protection_prices(request.plan, rules);
        let total_take_profit = request
            .plan
            .take_profits()
            .iter()
            .try_fold(DomainDecimal::ZERO, |sum, target| {
                sum.checked_add(target.quantity())
            });
        if total_take_profit.is_none()
            || total_take_profit.is_some_and(|total| total > position.position.quantity())
        {
            self.reject(ExecutionValidationRejection::ManagedAssetViolation);
        }
        let stop_price = request
            .plan
            .protective_stop()
            .map_or(position.current_protective_stop_price, |stop| {
                stop.limit_price().unwrap_or(stop.trigger_price())
            });
        let recalculated = worst_long_loss(
            position.average_entry_price,
            stop_price,
            position.position.quantity(),
            request.policy.taker_fee_rate(),
            DomainDecimal::ZERO,
        );
        self.apply_maximum_loss(request, recalculated);
    }

    fn validate_sell(
        &mut self,
        request: &ExecutionValidationRequest<'_>,
        rules: &SpotInstrumentRules,
        full_exit: bool,
    ) {
        self.require_paper_permission(request);
        self.require_target_state(
            request,
            &[TradePlanState::Active, TradePlanState::RecoveryRequired],
        );
        if request.open_orders.iter().any(|order| {
            order.instrument_id() == request.plan.instrument_id()
                && order.side() == AccountOrderSide::Sell
        }) {
            self.reject(ExecutionValidationRejection::ConflictingOrder);
        }
        let Some(position) = managed_position_for(request) else {
            self.reject(ExecutionValidationRejection::ManagedAssetViolation);
            return;
        };
        let Some(order) = request.plan.order() else {
            return;
        };
        self.validate_order(request, rules, order, AccountOrderSide::Sell);
        if (full_exit && order.quantity() != position.position.quantity())
            || (!full_exit && order.quantity() >= position.position.quantity())
        {
            self.reject(ExecutionValidationRejection::InvalidExitQuantity);
        }
        let available_base = request
            .portfolio
            .assets()
            .iter()
            .find(|asset| asset.asset() == rules.base_asset())
            .map_or(DomainDecimal::ZERO, |asset| {
                asset.exchange_available_quantity()
            });
        if position
            .position
            .authorize_sell(order.quantity(), available_base)
            .is_err()
        {
            self.reject(ExecutionValidationRejection::ManagedAssetViolation);
        }
    }

    fn validate_order(
        &mut self,
        request: &ExecutionValidationRequest<'_>,
        rules: &SpotInstrumentRules,
        order: &AiOrder,
        side: AccountOrderSide,
    ) -> Option<DomainDecimal> {
        if !is_multiple(order.quantity(), rules.base_precision()) {
            self.reject(ExecutionValidationRejection::InvalidQuantityIncrement);
        }
        let maximum = match order.order_type() {
            AiOrderType::Limit => rules.maximum_limit_order_quantity(),
            AiOrderType::Market => rules.maximum_market_order_quantity(),
        };
        if order.quantity() > maximum {
            self.reject(ExecutionValidationRejection::OrderQuantityTooLarge);
        }
        if order.order_type() == AiOrderType::Market
            && order.time_in_force() != ironpilot_domain::AiTimeInForce::Ioc
        {
            self.reject(ExecutionValidationRejection::InvalidTimeInForce);
        }
        let reference_price = match order.order_type() {
            AiOrderType::Limit => {
                let price = order
                    .limit_price()
                    .expect("AITradingPlan validates LIMIT price presence");
                if !is_multiple(price, rules.price_tick()) {
                    self.reject(ExecutionValidationRejection::InvalidPriceIncrement);
                }
                let outside = match side {
                    AccountOrderSide::Buy => price > request.price_limits.highest_buy_price(),
                    AccountOrderSide::Sell => price < request.price_limits.lowest_sell_price(),
                };
                if outside {
                    self.reject(ExecutionValidationRejection::PriceOutsideExchangeLimit);
                }
                price
            }
            AiOrderType::Market => match side {
                AccountOrderSide::Buy => request.top_of_book.ask_price(),
                AccountOrderSide::Sell => request.top_of_book.bid_price(),
            },
        };
        match reference_price.checked_mul(order.quantity()) {
            Some(notional) if notional >= rules.minimum_order_amount() => {}
            Some(_) => self.reject(ExecutionValidationRejection::MinimumOrderAmount),
            None => {
                self.reject(ExecutionValidationRejection::ArithmeticFailure);
                return None;
            }
        }
        Some(reference_price)
    }

    fn validate_protection_prices(&mut self, plan: &AiTradingPlan, rules: &SpotInstrumentRules) {
        if let Some(stop) = plan.protective_stop() {
            for price in [Some(stop.trigger_price()), stop.limit_price()]
                .into_iter()
                .flatten()
            {
                if !is_multiple(price, rules.price_tick()) {
                    self.reject(ExecutionValidationRejection::InvalidPriceIncrement);
                }
            }
        }
        for target in plan.take_profits() {
            if !is_multiple(target.price(), rules.price_tick()) {
                self.reject(ExecutionValidationRejection::InvalidPriceIncrement);
            }
            if !is_multiple(target.quantity(), rules.base_precision()) {
                self.reject(ExecutionValidationRejection::InvalidQuantityIncrement);
            }
        }
    }

    fn apply_maximum_loss(
        &mut self,
        request: &ExecutionValidationRequest<'_>,
        recalculated: Option<DomainDecimal>,
    ) {
        let Some(recalculated) = recalculated else {
            self.reject(ExecutionValidationRejection::ArithmeticFailure);
            return;
        };
        self.recalculated_maximum_loss_quote = Some(recalculated);
        let Some(declared) = request.plan.declared_max_loss_quote() else {
            self.reject(ExecutionValidationRejection::DeclaredMaximumLossTooLow);
            return;
        };
        if declared < recalculated {
            self.reject(ExecutionValidationRejection::DeclaredMaximumLossTooLow);
        }
        if declared > request.current_maximum_loss_quote
            || recalculated > request.current_maximum_loss_quote
        {
            self.reject(ExecutionValidationRejection::MaximumLossExceeded);
        }
    }

    fn finish(mut self, request: &ExecutionValidationRequest<'_>) -> ExecutionValidationDecision {
        self.rejections.sort();
        let outcome = if self.rejections.is_empty() {
            ExecutionValidationOutcome::Accept
        } else {
            ExecutionValidationOutcome::Reject
        };
        let plan_hash = request.plan.plan_hash();
        let context_hash = request.context.context_hash().to_string();
        let plan_id = request.plan.plan_id().to_string();
        let evidence = json!({
            "validator_version": EXECUTION_VALIDATOR_VERSION_V1,
            "action_id": request.action_id.to_string(),
            "trade_plan_id": request.trade_plan_id.to_string(),
            "context_id": request.context.context_id().to_string(),
            "context_hash": context_hash,
            "plan_id": plan_id,
            "plan_hash": plan_hash.to_string(),
            "outcome": outcome.as_str(),
            "rejections": self.rejections.iter().map(|reason| reason.code()).collect::<Vec<_>>(),
            "recalculated_maximum_loss_quote": self.recalculated_maximum_loss_quote,
            "authorized_maximum_loss_quote": request.current_maximum_loss_quote,
            "validated_at_unix_millis": request.validated_at_unix_millis
        });
        let evidence_json = serde_json::to_string(&evidence)
            .expect("validated execution evidence must serialize")
            .into_boxed_str();
        let validation_hash =
            ExecutionValidationHash(Sha256::digest(evidence_json.as_bytes()).into());
        ExecutionValidationDecision {
            action_id: request.action_id,
            trade_plan_id: request.trade_plan_id,
            context_hash: context_hash.into_boxed_str(),
            plan_id: plan_id.into_boxed_str(),
            plan_hash,
            outcome,
            rejections: self.rejections,
            recalculated_maximum_loss_quote: self.recalculated_maximum_loss_quote,
            authorized_maximum_loss_quote: request.current_maximum_loss_quote,
            validated_at_unix_millis: request.validated_at_unix_millis,
            evidence_json,
            validation_hash,
        }
    }
}

fn managed_positions_match_context(request: &ExecutionValidationRequest<'_>) -> bool {
    let mut current: Vec<_> = request
        .managed_positions
        .iter()
        .map(|fact| fact.position.clone())
        .collect();
    current.sort_by(|left, right| left.instrument_id().cmp(right.instrument_id()));
    current == request.context.managed_positions()
}

fn open_orders_match_context(request: &ExecutionValidationRequest<'_>) -> bool {
    let mut current = request.open_orders.to_vec();
    current.sort_by(|left, right| {
        left.instrument_id()
            .cmp(right.instrument_id())
            .then_with(|| left.exchange_order_id().cmp(right.exchange_order_id()))
    });
    current == request.context.open_orders()
}

fn managed_position_for<'a>(
    request: &'a ExecutionValidationRequest<'_>,
) -> Option<&'a ManagedPositionExecutionFact> {
    request.managed_positions.iter().find(|fact| {
        fact.trade_plan_id == request.trade_plan_id
            && fact.position.instrument_id() == request.plan.instrument_id()
    })
}

fn worst_long_loss(
    entry_price: DomainDecimal,
    stop_price: DomainDecimal,
    quantity: DomainDecimal,
    fee_rate: DomainDecimal,
    slippage_quote: DomainDecimal,
) -> Option<DomainDecimal> {
    let entry_notional = entry_price.checked_mul(quantity)?;
    let stop_notional = stop_price.checked_mul(quantity)?;
    let gross_loss = entry_notional
        .checked_sub(stop_notional)
        .unwrap_or(DomainDecimal::ZERO);
    let fees = entry_notional
        .checked_add(stop_notional)?
        .checked_mul(fee_rate)?;
    gross_loss.checked_add(fees)?.checked_add(slippage_quote)
}

fn is_multiple(value: DomainDecimal, increment: DomainDecimal) -> bool {
    increment > DomainDecimal::ZERO
        && value
            .checked_rem(increment)
            .is_some_and(|remainder| remainder == DomainDecimal::ZERO)
}

fn decimal_one() -> DomainDecimal {
    DomainDecimal::from_mantissa_scale(1, 0).expect("one is a valid domain decimal")
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExecutionValidationInputError {
    EmptyInstrumentScope,
    DuplicateInstrument,
    InvalidFeeRate,
    InvalidFreshnessLimit,
    InvalidPriceLimit,
    InvalidTimestamp,
    InvalidManagedPositionPrice,
}

impl fmt::Display for ExecutionValidationInputError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::EmptyInstrumentScope => "execution authorization instrument scope is empty",
            Self::DuplicateInstrument => {
                "execution authorization instrument scope contains a duplicate"
            }
            Self::InvalidFeeRate => "execution validation fee rate must be in [0, 1)",
            Self::InvalidFreshnessLimit => "execution validation freshness limits must be positive",
            Self::InvalidPriceLimit => "exchange order price limits must be positive",
            Self::InvalidTimestamp => "execution validation timestamp must be positive",
            Self::InvalidManagedPositionPrice => {
                "managed position entry and protective-stop prices must be positive"
            }
        })
    }
}

impl std::error::Error for ExecutionValidationInputError {}
