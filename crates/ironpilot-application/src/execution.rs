use core::fmt;
use core::future::Future;
use core::pin::Pin;
use std::collections::BTreeSet;

use ironpilot_domain::{
    AccountOrderSide, AiDecisionContext, AiOrderType, AiTimeInForce, AiTradingAction,
    AiTradingPlan, DomainDecimal, InstrumentId, OrderId, OrderIntentId, SnapshotId,
    TradePlanActionId, TradePlanId,
};
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::{ExecutionValidationDecision, ExecutionValidationOutcome};

pub const SPOT_EXECUTION_SCHEMA_VERSION_V1: &str = "ironpilot-spot-execution-v1";
pub const PAPER_MATCHING_ENGINE_VERSION_V1: &str = "ironpilot-paper-matching-v1";
pub const MAX_EXECUTION_ORDERS_PER_ACTION: usize = 10;

pub type ExecutionFuture<'a, T, E> = Pin<Box<dyn Future<Output = Result<T, E>> + Send + 'a>>;

pub trait SpotExecutionPort: Send + Sync {
    type Error;

    fn submit<'a>(
        &'a self,
        request: &'a SpotExecutionRequest,
    ) -> ExecutionFuture<'a, ExecutionReceipt, Self::Error>;
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ExecutionVenue {
    Paper,
    Backtest,
    Testnet,
}

impl ExecutionVenue {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Paper => "PAPER",
            Self::Backtest => "BACKTEST",
            Self::Testnet => "TESTNET",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExecutionEffect {
    Applied,
    DuplicateNoEffect,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExecutionReceipt {
    venue: ExecutionVenue,
    action_id: TradePlanActionId,
    effect: ExecutionEffect,
}

impl ExecutionReceipt {
    #[must_use]
    pub const fn new(
        venue: ExecutionVenue,
        action_id: TradePlanActionId,
        effect: ExecutionEffect,
    ) -> Self {
        Self {
            venue,
            action_id,
            effect,
        }
    }

    #[must_use]
    pub const fn venue(self) -> ExecutionVenue {
        self.venue
    }

    #[must_use]
    pub const fn action_id(self) -> TradePlanActionId {
        self.action_id
    }

    #[must_use]
    pub const fn effect(self) -> ExecutionEffect {
        self.effect
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ExecutionCommandKind {
    OpenLong,
    CancelEntry,
    ModifyProtection,
    Reduce,
    Exit,
}

impl ExecutionCommandKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OpenLong => "OPEN_LONG",
            Self::CancelEntry => "CANCEL_ENTRY",
            Self::ModifyProtection => "MODIFY_PROTECTION",
            Self::Reduce => "REDUCE",
            Self::Exit => "EXIT",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ExecutionOrderRole {
    Entry,
    ProtectiveStop,
    TakeProfit { index: u8 },
    Reduction,
    Exit,
}

impl ExecutionOrderRole {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Entry => "ENTRY",
            Self::ProtectiveStop => "PROTECTIVE_STOP",
            Self::TakeProfit { .. } => "TAKE_PROFIT",
            Self::Reduction => "REDUCTION",
            Self::Exit => "EXIT",
        }
    }

    #[must_use]
    pub const fn take_profit_index(self) -> Option<u8> {
        match self {
            Self::TakeProfit { index } => Some(index),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ExecutionOrderIds {
    order_intent_id: OrderIntentId,
    order_id: OrderId,
}

impl ExecutionOrderIds {
    #[must_use]
    pub const fn new(order_intent_id: OrderIntentId, order_id: OrderId) -> Self {
        Self {
            order_intent_id,
            order_id,
        }
    }

    #[must_use]
    pub const fn order_intent_id(self) -> OrderIntentId {
        self.order_intent_id
    }

    #[must_use]
    pub const fn order_id(self) -> OrderId {
        self.order_id
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExecutionOrderIdSet {
    primary: Option<ExecutionOrderIds>,
    protective_stop: Option<ExecutionOrderIds>,
    take_profits: Vec<ExecutionOrderIds>,
}

impl ExecutionOrderIdSet {
    pub fn new(
        primary: Option<ExecutionOrderIds>,
        protective_stop: Option<ExecutionOrderIds>,
        take_profits: Vec<ExecutionOrderIds>,
    ) -> Result<Self, SpotExecutionRequestError> {
        let count = usize::from(primary.is_some())
            + usize::from(protective_stop.is_some())
            + take_profits.len();
        if count > MAX_EXECUTION_ORDERS_PER_ACTION {
            return Err(SpotExecutionRequestError::TooManyOrders);
        }
        let mut intent_ids = BTreeSet::new();
        let mut order_ids = BTreeSet::new();
        if primary
            .into_iter()
            .chain(protective_stop)
            .chain(take_profits.iter().copied())
            .any(|ids| {
                !intent_ids.insert(ids.order_intent_id()) || !order_ids.insert(ids.order_id())
            })
        {
            return Err(SpotExecutionRequestError::DuplicateOrderId);
        }
        Ok(Self {
            primary,
            protective_stop,
            take_profits,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlannedSpotOrder {
    ids: ExecutionOrderIds,
    role: ExecutionOrderRole,
    side: AccountOrderSide,
    order_type: AiOrderType,
    quantity: Option<DomainDecimal>,
    limit_price: Option<DomainDecimal>,
    trigger_price: Option<DomainDecimal>,
    time_in_force: Option<AiTimeInForce>,
    expires_at_unix_millis: u64,
    max_slippage_quote: DomainDecimal,
}

impl PlannedSpotOrder {
    #[allow(clippy::too_many_arguments)]
    pub fn from_persisted(
        ids: ExecutionOrderIds,
        role: ExecutionOrderRole,
        side: AccountOrderSide,
        order_type: AiOrderType,
        quantity: Option<DomainDecimal>,
        limit_price: Option<DomainDecimal>,
        trigger_price: Option<DomainDecimal>,
        time_in_force: Option<AiTimeInForce>,
        expires_at_unix_millis: u64,
        max_slippage_quote: DomainDecimal,
    ) -> Result<Self, PaperExecutionError> {
        let valid_role = match role {
            ExecutionOrderRole::Entry
            | ExecutionOrderRole::Reduction
            | ExecutionOrderRole::Exit
            | ExecutionOrderRole::TakeProfit { .. } => quantity.is_some(),
            ExecutionOrderRole::ProtectiveStop => quantity.is_none() && trigger_price.is_some(),
        };
        let valid_price = match order_type {
            AiOrderType::Limit => limit_price.is_some(),
            AiOrderType::Market => limit_price.is_none(),
        };
        if !valid_role
            || !valid_price
            || quantity.is_some_and(|value| value <= DomainDecimal::ZERO)
            || trigger_price.is_some_and(|value| value <= DomainDecimal::ZERO)
            || expires_at_unix_millis == 0
            || max_slippage_quote < DomainDecimal::ZERO
        {
            return Err(PaperExecutionError::InvalidOpenOrder);
        }
        Ok(Self {
            ids,
            role,
            side,
            order_type,
            quantity,
            limit_price,
            trigger_price,
            time_in_force,
            expires_at_unix_millis,
            max_slippage_quote,
        })
    }

    #[must_use]
    pub const fn ids(&self) -> ExecutionOrderIds {
        self.ids
    }

    #[must_use]
    pub const fn role(&self) -> ExecutionOrderRole {
        self.role
    }

    #[must_use]
    pub const fn side(&self) -> AccountOrderSide {
        self.side
    }

    #[must_use]
    pub const fn order_type(&self) -> AiOrderType {
        self.order_type
    }

    #[must_use]
    pub const fn quantity(&self) -> Option<DomainDecimal> {
        self.quantity
    }

    #[must_use]
    pub const fn limit_price(&self) -> Option<DomainDecimal> {
        self.limit_price
    }

    #[must_use]
    pub const fn trigger_price(&self) -> Option<DomainDecimal> {
        self.trigger_price
    }

    #[must_use]
    pub const fn time_in_force(&self) -> Option<AiTimeInForce> {
        self.time_in_force
    }

    #[must_use]
    pub const fn expires_at_unix_millis(&self) -> u64 {
        self.expires_at_unix_millis
    }

    #[must_use]
    pub const fn max_slippage_quote(&self) -> DomainDecimal {
        self.max_slippage_quote
    }

    #[must_use]
    pub fn payload_json(&self) -> String {
        json!({
            "order_intent_id": self.ids.order_intent_id().to_string(),
            "order_id": self.ids.order_id().to_string(),
            "role": self.role.as_str(),
            "take_profit_index": self.role.take_profit_index(),
            "side": side_name(self.side),
            "order_type": order_type_name(self.order_type),
            "quantity": self.quantity,
            "limit_price": self.limit_price,
            "trigger_price": self.trigger_price,
            "time_in_force": self.time_in_force.map(time_in_force_name),
            "expires_at_unix_millis": self.expires_at_unix_millis,
            "max_slippage_quote": self.max_slippage_quote
        })
        .to_string()
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SpotExecutionRequestHash([u8; 32]);

impl fmt::Display for SpotExecutionRequestHash {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_hex(formatter, &self.0)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpotExecutionRequest {
    action_id: TradePlanActionId,
    trade_plan_id: TradePlanId,
    context_as_of_unix_millis: u64,
    context_hash: Box<str>,
    validation_hash: Box<str>,
    source_plan_hash: Box<str>,
    source_plan_json: Box<str>,
    instrument_id: InstrumentId,
    command: ExecutionCommandKind,
    orders: Vec<PlannedSpotOrder>,
    created_at_unix_millis: u64,
    payload_json: Box<str>,
    request_hash: SpotExecutionRequestHash,
}

impl SpotExecutionRequest {
    pub fn from_accepted_plan(
        context: &AiDecisionContext,
        validation: &ExecutionValidationDecision,
        plan: &AiTradingPlan,
        ids: ExecutionOrderIdSet,
        created_at_unix_millis: u64,
    ) -> Result<Self, SpotExecutionRequestError> {
        if validation.outcome() != ExecutionValidationOutcome::Accept
            || !validation.authorizes_unchanged(plan)
        {
            return Err(SpotExecutionRequestError::PlanNotAccepted);
        }
        if plan.context_id() != context.context_id()
            || plan.instrument_id() != context.instrument_id()
            || validation.context_hash() != context.context_hash().to_string()
        {
            return Err(SpotExecutionRequestError::ProvenanceMismatch);
        }
        if created_at_unix_millis < validation.validated_at_unix_millis()
            || created_at_unix_millis >= plan.valid_until_unix_millis()
        {
            return Err(SpotExecutionRequestError::InvalidTimestamp);
        }
        let command = command_kind(plan.action())?;
        validate_id_shape(command, plan, &ids)?;
        let orders = build_orders(plan, &ids)?;
        let context_hash = context.context_hash().to_string();
        let validation_hash = validation.validation_hash().to_string();
        let source_plan_hash = plan.plan_hash().to_string();
        let source_plan_json = plan.to_json();
        let order_payloads: Vec<_> = orders
            .iter()
            .map(|order| {
                serde_json::from_str::<serde_json::Value>(&order.payload_json())
                    .expect("planned order payload must be JSON")
            })
            .collect();
        let payload = json!({
            "schema_version": SPOT_EXECUTION_SCHEMA_VERSION_V1,
            "action_id": validation.action_id().to_string(),
            "trade_plan_id": validation.trade_plan_id().to_string(),
            "context_id": context.context_id().to_string(),
            "context_as_of_unix_millis": context.as_of_unix_millis(),
            "context_hash": context_hash,
            "validation_hash": validation_hash,
            "source_plan_hash": source_plan_hash,
            "source_plan": serde_json::from_str::<serde_json::Value>(&source_plan_json)
                .expect("validated AI plan must serialize"),
            "instrument_id": plan.instrument_id().to_string(),
            "command": command.as_str(),
            "orders": order_payloads,
            "created_at_unix_millis": created_at_unix_millis
        });
        let payload_json = serde_json::to_string(&payload)
            .expect("validated execution request must serialize")
            .into_boxed_str();
        let request_hash = SpotExecutionRequestHash(Sha256::digest(payload_json.as_bytes()).into());
        Ok(Self {
            action_id: validation.action_id(),
            trade_plan_id: validation.trade_plan_id(),
            context_as_of_unix_millis: context.as_of_unix_millis(),
            context_hash: context_hash.into_boxed_str(),
            validation_hash: validation_hash.into_boxed_str(),
            source_plan_hash: source_plan_hash.into_boxed_str(),
            source_plan_json: source_plan_json.into_boxed_str(),
            instrument_id: plan.instrument_id().clone(),
            command,
            orders,
            created_at_unix_millis,
            payload_json,
            request_hash,
        })
    }

    #[must_use]
    pub const fn action_id(&self) -> TradePlanActionId {
        self.action_id
    }

    #[must_use]
    pub const fn trade_plan_id(&self) -> TradePlanId {
        self.trade_plan_id
    }

    #[must_use]
    pub const fn context_as_of_unix_millis(&self) -> u64 {
        self.context_as_of_unix_millis
    }

    #[must_use]
    pub fn context_hash(&self) -> &str {
        &self.context_hash
    }

    #[must_use]
    pub fn validation_hash(&self) -> &str {
        &self.validation_hash
    }

    #[must_use]
    pub fn source_plan_hash(&self) -> &str {
        &self.source_plan_hash
    }

    #[must_use]
    pub fn source_plan_json(&self) -> &str {
        &self.source_plan_json
    }

    #[must_use]
    pub fn instrument_id(&self) -> &InstrumentId {
        &self.instrument_id
    }

    #[must_use]
    pub const fn command(&self) -> ExecutionCommandKind {
        self.command
    }

    #[must_use]
    pub fn orders(&self) -> &[PlannedSpotOrder] {
        &self.orders
    }

    #[must_use]
    pub const fn created_at_unix_millis(&self) -> u64 {
        self.created_at_unix_millis
    }

    #[must_use]
    pub fn payload_json(&self) -> &str {
        &self.payload_json
    }

    #[must_use]
    pub const fn request_hash(&self) -> SpotExecutionRequestHash {
        self.request_hash
    }
}

fn command_kind(
    action: AiTradingAction,
) -> Result<ExecutionCommandKind, SpotExecutionRequestError> {
    match action {
        AiTradingAction::OpenLong => Ok(ExecutionCommandKind::OpenLong),
        AiTradingAction::CancelEntry => Ok(ExecutionCommandKind::CancelEntry),
        AiTradingAction::ModifyProtection => Ok(ExecutionCommandKind::ModifyProtection),
        AiTradingAction::Reduce => Ok(ExecutionCommandKind::Reduce),
        AiTradingAction::Exit => Ok(ExecutionCommandKind::Exit),
        AiTradingAction::NoTrade | AiTradingAction::Hold => {
            Err(SpotExecutionRequestError::ActionHasNoExecutionEffect)
        }
    }
}

fn validate_id_shape(
    command: ExecutionCommandKind,
    plan: &AiTradingPlan,
    ids: &ExecutionOrderIdSet,
) -> Result<(), SpotExecutionRequestError> {
    let matches = match command {
        ExecutionCommandKind::OpenLong => {
            ids.primary.is_some()
                && ids.protective_stop.is_some()
                && ids.take_profits.len() == plan.take_profits().len()
        }
        ExecutionCommandKind::CancelEntry => {
            ids.primary.is_none() && ids.protective_stop.is_none() && ids.take_profits.is_empty()
        }
        ExecutionCommandKind::ModifyProtection => {
            ids.primary.is_none()
                && ids.protective_stop.is_some() == plan.protective_stop().is_some()
                && ids.take_profits.len() == plan.take_profits().len()
        }
        ExecutionCommandKind::Reduce | ExecutionCommandKind::Exit => {
            ids.primary.is_some() && ids.protective_stop.is_none() && ids.take_profits.is_empty()
        }
    };
    if matches {
        Ok(())
    } else {
        Err(SpotExecutionRequestError::OrderIdShapeMismatch)
    }
}

fn build_orders(
    plan: &AiTradingPlan,
    ids: &ExecutionOrderIdSet,
) -> Result<Vec<PlannedSpotOrder>, SpotExecutionRequestError> {
    let mut orders = Vec::new();
    let protection_expiry = plan
        .review()
        .map_or(plan.valid_until_unix_millis(), |review| {
            review.max_holding_until_unix_millis()
        });
    if let Some(primary_ids) = ids.primary {
        let order = plan
            .order()
            .ok_or(SpotExecutionRequestError::OrderIdShapeMismatch)?;
        let (role, side) = match plan.action() {
            AiTradingAction::OpenLong => (ExecutionOrderRole::Entry, AccountOrderSide::Buy),
            AiTradingAction::Reduce => (ExecutionOrderRole::Reduction, AccountOrderSide::Sell),
            AiTradingAction::Exit => (ExecutionOrderRole::Exit, AccountOrderSide::Sell),
            _ => return Err(SpotExecutionRequestError::OrderIdShapeMismatch),
        };
        orders.push(PlannedSpotOrder {
            ids: primary_ids,
            role,
            side,
            order_type: order.order_type(),
            quantity: Some(order.quantity()),
            limit_price: order.limit_price(),
            trigger_price: None,
            time_in_force: Some(order.time_in_force()),
            expires_at_unix_millis: order.expires_at_unix_millis(),
            max_slippage_quote: order.max_slippage_quote(),
        });
    }
    if let Some(stop_ids) = ids.protective_stop {
        let stop = plan
            .protective_stop()
            .ok_or(SpotExecutionRequestError::OrderIdShapeMismatch)?;
        orders.push(PlannedSpotOrder {
            ids: stop_ids,
            role: ExecutionOrderRole::ProtectiveStop,
            side: AccountOrderSide::Sell,
            order_type: stop.order_type(),
            quantity: None,
            limit_price: stop.limit_price(),
            trigger_price: Some(stop.trigger_price()),
            time_in_force: None,
            expires_at_unix_millis: protection_expiry,
            max_slippage_quote: DomainDecimal::ZERO,
        });
    }
    for (index, (target, target_ids)) in plan
        .take_profits()
        .iter()
        .zip(ids.take_profits.iter().copied())
        .enumerate()
    {
        orders.push(PlannedSpotOrder {
            ids: target_ids,
            role: ExecutionOrderRole::TakeProfit {
                index: u8::try_from(index).map_err(|_| SpotExecutionRequestError::TooManyOrders)?,
            },
            side: AccountOrderSide::Sell,
            order_type: AiOrderType::Limit,
            quantity: Some(target.quantity()),
            limit_price: Some(target.price()),
            trigger_price: None,
            time_in_force: Some(AiTimeInForce::Gtc),
            expires_at_unix_millis: protection_expiry,
            max_slippage_quote: DomainDecimal::ZERO,
        });
    }
    Ok(orders)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PaperExecutionPolicy {
    maker_fee_rate: DomainDecimal,
    taker_fee_rate: DomainDecimal,
    market_slippage_rate: DomainDecimal,
}

impl PaperExecutionPolicy {
    pub fn new(
        maker_fee_rate: DomainDecimal,
        taker_fee_rate: DomainDecimal,
        market_slippage_rate: DomainDecimal,
    ) -> Result<Self, PaperExecutionError> {
        if [maker_fee_rate, taker_fee_rate, market_slippage_rate]
            .into_iter()
            .any(|value| value < DomainDecimal::ZERO || value >= decimal_one())
        {
            return Err(PaperExecutionError::InvalidPolicy);
        }
        Ok(Self {
            maker_fee_rate,
            taker_fee_rate,
            market_slippage_rate,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaperMarketObservation {
    observation_id: SnapshotId,
    instrument_id: InstrumentId,
    source_generated_at_unix_millis: u64,
    observed_at_unix_millis: u64,
    bid_price: DomainDecimal,
    ask_price: DomainDecimal,
    traded_low: DomainDecimal,
    traded_high: DomainDecimal,
    available_base_liquidity: DomainDecimal,
}

impl PaperMarketObservation {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        observation_id: SnapshotId,
        instrument_id: InstrumentId,
        source_generated_at_unix_millis: u64,
        observed_at_unix_millis: u64,
        bid_price: DomainDecimal,
        ask_price: DomainDecimal,
        traded_low: DomainDecimal,
        traded_high: DomainDecimal,
        available_base_liquidity: DomainDecimal,
    ) -> Result<Self, PaperExecutionError> {
        if source_generated_at_unix_millis == 0
            || source_generated_at_unix_millis > observed_at_unix_millis
        {
            return Err(PaperExecutionError::InvalidObservationTime);
        }
        if [
            bid_price,
            ask_price,
            traded_low,
            traded_high,
            available_base_liquidity,
        ]
        .into_iter()
        .any(|value| value <= DomainDecimal::ZERO)
            || bid_price >= ask_price
            || traded_low > traded_high
        {
            return Err(PaperExecutionError::InvalidObservationPrice);
        }
        Ok(Self {
            observation_id,
            instrument_id,
            source_generated_at_unix_millis,
            observed_at_unix_millis,
            bid_price,
            ask_price,
            traded_low,
            traded_high,
            available_base_liquidity,
        })
    }

    #[must_use]
    pub const fn observation_id(&self) -> SnapshotId {
        self.observation_id
    }

    #[must_use]
    pub fn instrument_id(&self) -> &InstrumentId {
        &self.instrument_id
    }

    #[must_use]
    pub const fn source_generated_at_unix_millis(&self) -> u64 {
        self.source_generated_at_unix_millis
    }

    #[must_use]
    pub const fn observed_at_unix_millis(&self) -> u64 {
        self.observed_at_unix_millis
    }

    #[must_use]
    pub const fn available_base_liquidity(&self) -> DomainDecimal {
        self.available_base_liquidity
    }

    #[must_use]
    pub fn payload_json(&self) -> String {
        json!({
            "matching_version": PAPER_MATCHING_ENGINE_VERSION_V1,
            "observation_id": self.observation_id.to_string(),
            "instrument_id": self.instrument_id.to_string(),
            "source_generated_at_unix_millis": self.source_generated_at_unix_millis,
            "observed_at_unix_millis": self.observed_at_unix_millis,
            "bid_price": self.bid_price,
            "ask_price": self.ask_price,
            "traded_low": self.traded_low,
            "traded_high": self.traded_high,
            "available_base_liquidity": self.available_base_liquidity
        })
        .to_string()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaperOpenOrder {
    order: PlannedSpotOrder,
    instrument_id: InstrumentId,
    decision_as_of_unix_millis: u64,
    submitted_at_unix_millis: u64,
    remaining_quantity: DomainDecimal,
}

impl PaperOpenOrder {
    pub fn new(
        order: PlannedSpotOrder,
        instrument_id: InstrumentId,
        decision_as_of_unix_millis: u64,
        submitted_at_unix_millis: u64,
        remaining_quantity: DomainDecimal,
    ) -> Result<Self, PaperExecutionError> {
        if decision_as_of_unix_millis == 0
            || submitted_at_unix_millis < decision_as_of_unix_millis
            || remaining_quantity <= DomainDecimal::ZERO
        {
            return Err(PaperExecutionError::InvalidOpenOrder);
        }
        if order
            .quantity()
            .is_some_and(|quantity| remaining_quantity > quantity)
        {
            return Err(PaperExecutionError::InvalidOpenOrder);
        }
        Ok(Self {
            order,
            instrument_id,
            decision_as_of_unix_millis,
            submitted_at_unix_millis,
            remaining_quantity,
        })
    }

    #[must_use]
    pub const fn order(&self) -> &PlannedSpotOrder {
        &self.order
    }

    #[must_use]
    pub const fn remaining_quantity(&self) -> DomainDecimal {
        self.remaining_quantity
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PaperMatch {
    order_id: OrderId,
    role: ExecutionOrderRole,
    side: AccountOrderSide,
    base_quantity: DomainDecimal,
    execution_price: DomainDecimal,
    quote_quantity: DomainDecimal,
    fee_quote: DomainDecimal,
    occurred_at_unix_millis: u64,
}

impl PaperMatch {
    #[must_use]
    pub const fn order_id(self) -> OrderId {
        self.order_id
    }

    #[must_use]
    pub const fn role(self) -> ExecutionOrderRole {
        self.role
    }

    #[must_use]
    pub const fn side(self) -> AccountOrderSide {
        self.side
    }

    #[must_use]
    pub const fn base_quantity(self) -> DomainDecimal {
        self.base_quantity
    }

    #[must_use]
    pub const fn execution_price(self) -> DomainDecimal {
        self.execution_price
    }

    #[must_use]
    pub const fn quote_quantity(self) -> DomainDecimal {
        self.quote_quantity
    }

    #[must_use]
    pub const fn fee_quote(self) -> DomainDecimal {
        self.fee_quote
    }

    #[must_use]
    pub const fn occurred_at_unix_millis(self) -> u64 {
        self.occurred_at_unix_millis
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaperOrderEvaluation {
    NoFill,
    Expired,
    SlippageLimitExceeded,
    Fill(PaperMatch),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PaperMatchingEngine;

impl PaperMatchingEngine {
    pub fn evaluate(
        order: &PaperOpenOrder,
        observation: &PaperMarketObservation,
        available_liquidity: DomainDecimal,
        policy: PaperExecutionPolicy,
    ) -> Result<PaperOrderEvaluation, PaperExecutionError> {
        if observation.instrument_id() != &order.instrument_id {
            return Err(PaperExecutionError::InstrumentMismatch);
        }
        if observation.source_generated_at_unix_millis <= order.decision_as_of_unix_millis
            || observation.observed_at_unix_millis < order.submitted_at_unix_millis
        {
            return Err(PaperExecutionError::DecisionBarReuse);
        }
        if observation.observed_at_unix_millis >= order.order.expires_at_unix_millis {
            return Ok(PaperOrderEvaluation::Expired);
        }
        if available_liquidity <= DomainDecimal::ZERO {
            return Ok(PaperOrderEvaluation::NoFill);
        }
        if !is_triggered(&order.order, observation) {
            return Ok(PaperOrderEvaluation::NoFill);
        }
        let fill_quantity = order.remaining_quantity.min(available_liquidity);
        let (execution_price, fee_rate) =
            execution_price(&order.order, observation, fill_quantity, policy)?;
        let Some(quote_quantity) = execution_price.checked_mul(fill_quantity) else {
            return Err(PaperExecutionError::ArithmeticFailure);
        };
        let Some(fee_quote) = quote_quantity.checked_mul(fee_rate) else {
            return Err(PaperExecutionError::ArithmeticFailure);
        };
        Ok(PaperOrderEvaluation::Fill(PaperMatch {
            order_id: order.order.ids.order_id(),
            role: order.order.role,
            side: order.order.side,
            base_quantity: fill_quantity,
            execution_price,
            quote_quantity,
            fee_quote,
            occurred_at_unix_millis: observation.observed_at_unix_millis,
        }))
    }
}

fn is_triggered(order: &PlannedSpotOrder, observation: &PaperMarketObservation) -> bool {
    match order.role {
        ExecutionOrderRole::ProtectiveStop => {
            observation.traded_low <= order.trigger_price.expect("protective stop has a trigger")
                && (order.order_type != AiOrderType::Limit
                    || observation.traded_high
                        >= order.limit_price.expect("limit stop has a limit price"))
        }
        ExecutionOrderRole::TakeProfit { .. } => {
            observation.traded_high >= order.limit_price.expect("take profit has a price")
        }
        _ => match (order.side, order.order_type) {
            (_, AiOrderType::Market) => true,
            (AccountOrderSide::Buy, AiOrderType::Limit) => {
                observation.traded_low <= order.limit_price.expect("limit order has a price")
            }
            (AccountOrderSide::Sell, AiOrderType::Limit) => {
                observation.traded_high >= order.limit_price.expect("limit order has a price")
            }
        },
    }
}

fn execution_price(
    order: &PlannedSpotOrder,
    observation: &PaperMarketObservation,
    fill_quantity: DomainDecimal,
    policy: PaperExecutionPolicy,
) -> Result<(DomainDecimal, DomainDecimal), PaperExecutionError> {
    if order.order_type == AiOrderType::Limit {
        return Ok((
            order.limit_price.expect("limit order has a price"),
            policy.maker_fee_rate,
        ));
    }
    let reference_price = match order.side {
        AccountOrderSide::Buy => observation.ask_price,
        AccountOrderSide::Sell => observation.bid_price,
    };
    if order.role == ExecutionOrderRole::ProtectiveStop {
        return Ok((reference_price, policy.taker_fee_rate));
    }
    let reference_notional = reference_price
        .checked_mul(fill_quantity)
        .ok_or(PaperExecutionError::ArithmeticFailure)?;
    let slippage_quote = reference_notional
        .checked_mul(policy.market_slippage_rate)
        .ok_or(PaperExecutionError::ArithmeticFailure)?;
    if slippage_quote > order.max_slippage_quote {
        return Err(PaperExecutionError::SlippageLimitExceeded);
    }
    let unit_slippage = slippage_quote
        .checked_div(fill_quantity)
        .ok_or(PaperExecutionError::ArithmeticFailure)?;
    let execution_price = match order.side {
        AccountOrderSide::Buy => reference_price.checked_add(unit_slippage),
        AccountOrderSide::Sell => reference_price.checked_sub(unit_slippage),
    }
    .ok_or(PaperExecutionError::ArithmeticFailure)?;
    if execution_price <= DomainDecimal::ZERO {
        return Err(PaperExecutionError::ArithmeticFailure);
    }
    Ok((execution_price, policy.taker_fee_rate))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpotExecutionRequestError {
    PlanNotAccepted,
    ProvenanceMismatch,
    InvalidTimestamp,
    ActionHasNoExecutionEffect,
    OrderIdShapeMismatch,
    DuplicateOrderId,
    TooManyOrders,
}

impl fmt::Display for SpotExecutionRequestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::PlanNotAccepted => "execution requires an accepted unchanged AI plan",
            Self::ProvenanceMismatch => {
                "execution request provenance does not match Context and validation evidence"
            }
            Self::InvalidTimestamp => "execution request timestamp is outside plan validity",
            Self::ActionHasNoExecutionEffect => "AI action has no execution effect",
            Self::OrderIdShapeMismatch => "execution order IDs do not match the AI action fields",
            Self::DuplicateOrderId => "execution order IDs must be unique",
            Self::TooManyOrders => "execution order count exceeds the fixed bound",
        })
    }
}

impl std::error::Error for SpotExecutionRequestError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaperExecutionError {
    InvalidPolicy,
    InvalidObservationTime,
    InvalidObservationPrice,
    InvalidOpenOrder,
    InstrumentMismatch,
    DecisionBarReuse,
    SlippageLimitExceeded,
    ArithmeticFailure,
}

impl fmt::Display for PaperExecutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidPolicy => "paper fee and slippage rates must be in [0, 1)",
            Self::InvalidObservationTime => "paper observation timestamps are invalid",
            Self::InvalidObservationPrice => "paper observation prices or liquidity are invalid",
            Self::InvalidOpenOrder => "paper open order state is invalid",
            Self::InstrumentMismatch => {
                "paper observation instrument does not match the open order"
            }
            Self::DecisionBarReuse => {
                "paper execution cannot reuse the market fact that produced the AI decision"
            }
            Self::SlippageLimitExceeded => "paper market slippage exceeds the AI-supplied maximum",
            Self::ArithmeticFailure => "paper execution decimal arithmetic failed",
        })
    }
}

impl std::error::Error for PaperExecutionError {}

const fn side_name(side: AccountOrderSide) -> &'static str {
    match side {
        AccountOrderSide::Buy => "BUY",
        AccountOrderSide::Sell => "SELL",
    }
}

const fn order_type_name(order_type: AiOrderType) -> &'static str {
    match order_type {
        AiOrderType::Limit => "LIMIT",
        AiOrderType::Market => "MARKET",
    }
}

const fn time_in_force_name(time_in_force: AiTimeInForce) -> &'static str {
    match time_in_force {
        AiTimeInForce::Gtc => "GTC",
        AiTimeInForce::Ioc => "IOC",
        AiTimeInForce::Fok => "FOK",
    }
}

fn decimal_one() -> DomainDecimal {
    DomainDecimal::from_mantissa_scale(1, 0).expect("one is a valid domain decimal")
}

fn write_hex(formatter: &mut fmt::Formatter<'_>, bytes: &[u8]) -> fmt::Result {
    for byte in bytes {
        write!(formatter, "{byte:02x}")?;
    }
    Ok(())
}
