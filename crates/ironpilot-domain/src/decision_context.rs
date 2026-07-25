use core::fmt;
use std::collections::BTreeSet;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::market_features::TimeframeFeatures;
use crate::{
    AI_DECISION_CONTEXT_SCHEMA_VERSION_V1, AiDecisionContextId, AiOrderType, AiProviderResponseId,
    AiTradingAction, AiTradingPlan, CandlePattern, ClosedCandle, DomainDecimal, EmaAlignment,
    FEATURE_CANDLE_WINDOW, InstrumentId, InstrumentRulesSnapshot, InstrumentTradingStatus,
    KeyLocation, MARKET_FEATURES_VERSION_V1, ManagedPosition, MarketDataSource,
    MarketFeatureEngine, MarketFeatureError, MarketFeatureSnapshot, MarketTimeframe,
    PORTFOLIO_SCHEMA_VERSION_V1, PatternSemantic, PortfolioReconciliationStatus, PortfolioSnapshot,
    SpotInstrumentRules, TopOfBook, TradePlanActionId, TradePlanId, TradePlanState,
};

pub const MAX_CONTEXT_MANAGED_POSITIONS: usize = 8;
pub const MAX_CONTEXT_OPEN_ORDERS: usize = 64;
pub const MAX_PROVIDER_LABEL_LENGTH: usize = 128;
pub const MAX_RAW_AI_RESPONSE_BYTES: usize = 128 * 1_024;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AiDecisionContextHash([u8; 32]);

impl AiDecisionContextHash {
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Display for AiDecisionContextHash {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_hex(formatter, &self.0)
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AiRawResponseHash([u8; 32]);

impl AiRawResponseHash {
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Display for AiRawResponseHash {
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

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum AccountOrderSide {
    Buy,
    Sell,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum AccountOrderStatus {
    New,
    PartiallyFilled,
    PendingCancel,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountOrderFact {
    exchange_order_id: Box<str>,
    order_link_id: Option<Box<str>>,
    instrument_id: InstrumentId,
    side: AccountOrderSide,
    order_type: AiOrderType,
    limit_price: Option<DomainDecimal>,
    original_quantity: DomainDecimal,
    filled_quantity: DomainDecimal,
    status: AccountOrderStatus,
    observed_at_unix_millis: u64,
}

impl AccountOrderFact {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        exchange_order_id: impl Into<Box<str>>,
        order_link_id: Option<impl Into<Box<str>>>,
        instrument_id: InstrumentId,
        side: AccountOrderSide,
        order_type: AiOrderType,
        limit_price: Option<DomainDecimal>,
        original_quantity: DomainDecimal,
        filled_quantity: DomainDecimal,
        status: AccountOrderStatus,
        observed_at_unix_millis: u64,
    ) -> Result<Self, DecisionContextError> {
        let exchange_order_id = exchange_order_id.into();
        validate_label("exchange_order_id", &exchange_order_id)?;
        let order_link_id = order_link_id.map(Into::into);
        if let Some(value) = &order_link_id {
            validate_label("order_link_id", value)?;
        }
        if original_quantity <= DomainDecimal::ZERO
            || filled_quantity < DomainDecimal::ZERO
            || filled_quantity > original_quantity
        {
            return Err(DecisionContextError::InvalidOrderQuantity);
        }
        match (order_type, limit_price) {
            (AiOrderType::Limit, Some(price)) if price > DomainDecimal::ZERO => {}
            (AiOrderType::Market, None) => {}
            _ => return Err(DecisionContextError::InvalidOrderPrice),
        }
        if observed_at_unix_millis == 0 {
            return Err(DecisionContextError::InvalidTimestamp {
                field: "open_orders.observed_at",
            });
        }
        Ok(Self {
            exchange_order_id,
            order_link_id,
            instrument_id,
            side,
            order_type,
            limit_price,
            original_quantity,
            filled_quantity,
            status,
            observed_at_unix_millis,
        })
    }

    #[must_use]
    pub fn exchange_order_id(&self) -> &str {
        &self.exchange_order_id
    }

    #[must_use]
    pub fn instrument_id(&self) -> &InstrumentId {
        &self.instrument_id
    }

    #[must_use]
    pub const fn observed_at_unix_millis(&self) -> u64 {
        self.observed_at_unix_millis
    }

    fn to_json(&self) -> Value {
        json!({
            "exchange_order_id": self.exchange_order_id,
            "order_link_id": self.order_link_id,
            "instrument_id": self.instrument_id.to_string(),
            "side": account_order_side_name(self.side),
            "order_type": order_type_name(self.order_type),
            "limit_price": self.limit_price,
            "original_quantity": self.original_quantity,
            "filled_quantity": self.filled_quantity,
            "status": account_order_status_name(self.status),
            "observed_at_unix_millis": self.observed_at_unix_millis
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AiDecisionContext {
    context_id: AiDecisionContextId,
    instrument_id: InstrumentId,
    as_of_unix_millis: u64,
    valid_until_unix_millis: u64,
    maximum_loss_quote: DomainDecimal,
    payload_json: Box<str>,
    context_hash: AiDecisionContextHash,
}

impl AiDecisionContext {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        context_id: AiDecisionContextId,
        as_of_unix_millis: u64,
        primary_candles: Vec<ClosedCandle>,
        confirmation_candles: Vec<ClosedCandle>,
        top_of_book: TopOfBook,
        market_features: MarketFeatureSnapshot,
        instrument_rules: &InstrumentRulesSnapshot,
        portfolio: &PortfolioSnapshot,
        mut managed_positions: Vec<ManagedPosition>,
        mut open_orders: Vec<AccountOrderFact>,
        maximum_loss_quote: DomainDecimal,
    ) -> Result<Self, DecisionContextError> {
        if as_of_unix_millis == 0 {
            return Err(DecisionContextError::InvalidTimestamp {
                field: "as_of_unix_millis",
            });
        }
        if maximum_loss_quote <= DomainDecimal::ZERO {
            return Err(DecisionContextError::InvalidMaximumLoss);
        }
        if primary_candles.len() != FEATURE_CANDLE_WINDOW
            || confirmation_candles.len() != FEATURE_CANDLE_WINDOW
        {
            return Err(DecisionContextError::IncompleteCandleWindow);
        }
        let recomputed = MarketFeatureEngine::compute(
            &primary_candles,
            &confirmation_candles,
            &top_of_book,
            as_of_unix_millis,
            market_features.source(),
        )
        .map_err(DecisionContextError::MarketFeature)?;
        if recomputed != market_features {
            return Err(DecisionContextError::FeatureSnapshotMismatch);
        }
        let instrument_id = market_features.instrument_id().clone();
        if market_features.is_expired_at(as_of_unix_millis) {
            return Err(DecisionContextError::ExpiredMarketFeatures);
        }
        if instrument_rules.observed_at_unix_millis() > as_of_unix_millis
            || instrument_rules.server_time().response_unix_millis() > as_of_unix_millis
        {
            return Err(DecisionContextError::FutureInstrumentRules);
        }
        if instrument_rules.is_expired_at(as_of_unix_millis) {
            return Err(DecisionContextError::ExpiredInstrumentRules);
        }
        let rules = instrument_rules
            .rules()
            .iter()
            .find(|rules| rules.instrument_id() == &instrument_id)
            .ok_or(DecisionContextError::MissingInstrumentRules)?;
        if portfolio.observed_at_unix_millis() > as_of_unix_millis {
            return Err(DecisionContextError::FuturePortfolio);
        }
        if managed_positions.len() > MAX_CONTEXT_MANAGED_POSITIONS {
            return Err(DecisionContextError::ManagedPositionCapacityExceeded);
        }
        managed_positions.sort_by(|left, right| left.instrument_id().cmp(right.instrument_id()));
        if managed_positions
            .windows(2)
            .any(|pair| pair[0].instrument_id() == pair[1].instrument_id())
        {
            return Err(DecisionContextError::DuplicateManagedPosition);
        }
        if open_orders.len() > MAX_CONTEXT_OPEN_ORDERS {
            return Err(DecisionContextError::OpenOrderCapacityExceeded);
        }
        if open_orders
            .iter()
            .any(|order| order.observed_at_unix_millis() > as_of_unix_millis)
        {
            return Err(DecisionContextError::FutureOpenOrder);
        }
        open_orders.sort_by(|left, right| {
            left.instrument_id()
                .cmp(right.instrument_id())
                .then_with(|| left.exchange_order_id().cmp(right.exchange_order_id()))
        });
        let mut order_ids = BTreeSet::new();
        if open_orders
            .iter()
            .any(|order| !order_ids.insert(order.exchange_order_id()))
        {
            return Err(DecisionContextError::DuplicateOpenOrder);
        }

        let valid_until_unix_millis = market_features
            .valid_until_unix_millis()
            .min(instrument_rules.valid_until_unix_millis());
        let payload = json!({
            "schema_version": AI_DECISION_CONTEXT_SCHEMA_VERSION_V1,
            "context_id": context_id.to_string(),
            "instrument_id": instrument_id.to_string(),
            "as_of_unix_millis": as_of_unix_millis,
            "valid_until_unix_millis": valid_until_unix_millis,
            "versions": {
                "market_features": MARKET_FEATURES_VERSION_V1,
                "portfolio": PORTFOLIO_SCHEMA_VERSION_V1
            },
            "market": {
                "candles_15m": primary_candles.iter().map(candle_json).collect::<Vec<_>>(),
                "candles_1h": confirmation_candles.iter().map(candle_json).collect::<Vec<_>>(),
                "top_of_book": top_of_book_json(&top_of_book),
                "features": market_features_json(&market_features)
            },
            "instrument_rules": instrument_rules_json(instrument_rules, rules),
            "account": {
                "portfolio": portfolio_json(portfolio),
                "managed_positions": managed_positions.iter().map(managed_position_json).collect::<Vec<_>>(),
                "open_orders": open_orders.iter().map(AccountOrderFact::to_json).collect::<Vec<_>>()
            },
            "user_authorization": {
                "maximum_loss_quote": maximum_loss_quote
            }
        });
        let payload_json = serde_json::to_string(&payload)
            .expect("validated AI Decision Context payload must serialize")
            .into_boxed_str();
        let context_hash = AiDecisionContextHash(Sha256::digest(payload_json.as_bytes()).into());
        Ok(Self {
            context_id,
            instrument_id,
            as_of_unix_millis,
            valid_until_unix_millis,
            maximum_loss_quote,
            payload_json,
            context_hash,
        })
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
    pub const fn as_of_unix_millis(&self) -> u64 {
        self.as_of_unix_millis
    }

    #[must_use]
    pub const fn valid_until_unix_millis(&self) -> u64 {
        self.valid_until_unix_millis
    }

    #[must_use]
    pub const fn maximum_loss_quote(&self) -> DomainDecimal {
        self.maximum_loss_quote
    }

    #[must_use]
    pub fn to_json(&self) -> &str {
        &self.payload_json
    }

    #[must_use]
    pub const fn context_hash(&self) -> AiDecisionContextHash {
        self.context_hash
    }

    #[must_use]
    pub const fn is_expired_at(&self, unix_millis: u64) -> bool {
        unix_millis >= self.valid_until_unix_millis
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AiRawResponse {
    response_id: AiProviderResponseId,
    context_id: AiDecisionContextId,
    provider: Box<str>,
    model: Box<str>,
    received_at_unix_millis: u64,
    raw_response: Box<str>,
    response_hash: AiRawResponseHash,
}

impl AiRawResponse {
    pub fn new(
        response_id: AiProviderResponseId,
        context_id: AiDecisionContextId,
        provider: impl Into<Box<str>>,
        model: impl Into<Box<str>>,
        received_at_unix_millis: u64,
        raw_response: impl Into<Box<str>>,
    ) -> Result<Self, DecisionContextError> {
        let provider = provider.into();
        let model = model.into();
        let raw_response = raw_response.into();
        validate_label("provider", &provider)?;
        validate_label("model", &model)?;
        if received_at_unix_millis == 0 {
            return Err(DecisionContextError::InvalidTimestamp {
                field: "response.received_at_unix_millis",
            });
        }
        if raw_response.trim().is_empty() || raw_response.len() > MAX_RAW_AI_RESPONSE_BYTES {
            return Err(DecisionContextError::InvalidRawResponse);
        }
        let hash_payload = json!({
            "response_id": response_id.to_string(),
            "context_id": context_id.to_string(),
            "provider": provider,
            "model": model,
            "received_at_unix_millis": received_at_unix_millis,
            "raw_response": raw_response
        });
        let response_hash =
            AiRawResponseHash(Sha256::digest(hash_payload.to_string().as_bytes()).into());
        Ok(Self {
            response_id,
            context_id,
            provider,
            model,
            received_at_unix_millis,
            raw_response,
            response_hash,
        })
    }

    #[must_use]
    pub const fn response_id(&self) -> AiProviderResponseId {
        self.response_id
    }

    #[must_use]
    pub const fn context_id(&self) -> AiDecisionContextId {
        self.context_id
    }

    #[must_use]
    pub fn provider(&self) -> &str {
        &self.provider
    }

    #[must_use]
    pub fn model(&self) -> &str {
        &self.model
    }

    #[must_use]
    pub const fn received_at_unix_millis(&self) -> u64 {
        self.received_at_unix_millis
    }

    #[must_use]
    pub fn raw_response(&self) -> &str {
        &self.raw_response
    }

    #[must_use]
    pub const fn response_hash(&self) -> AiRawResponseHash {
        self.response_hash
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TradePlanLedgerDisposition {
    Create { initial_state: TradePlanState },
    AppendToExisting,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AiTradePlanLedgerEntry {
    context: AiDecisionContext,
    response: AiRawResponse,
    plan: AiTradingPlan,
    trade_plan_id: TradePlanId,
    action_id: TradePlanActionId,
    recorded_at_unix_millis: u64,
    disposition: TradePlanLedgerDisposition,
}

impl AiTradePlanLedgerEntry {
    pub fn new(
        context: AiDecisionContext,
        response: AiRawResponse,
        plan: AiTradingPlan,
        trade_plan_id: TradePlanId,
        action_id: TradePlanActionId,
        recorded_at_unix_millis: u64,
    ) -> Result<Self, DecisionContextError> {
        if response.context_id() != context.context_id()
            || plan.context_id() != context.context_id()
            || plan.instrument_id() != context.instrument_id()
        {
            return Err(DecisionContextError::ProvenanceMismatch);
        }
        if response.received_at_unix_millis() < context.as_of_unix_millis()
            || response.received_at_unix_millis() > recorded_at_unix_millis
            || recorded_at_unix_millis >= context.valid_until_unix_millis()
            || recorded_at_unix_millis >= plan.valid_until_unix_millis()
        {
            return Err(DecisionContextError::InvalidLedgerTimestamp);
        }
        let disposition = match plan.action() {
            AiTradingAction::OpenLong => {
                if plan.target_trade_plan_id().is_some() {
                    return Err(DecisionContextError::TradePlanTargetMismatch);
                }
                TradePlanLedgerDisposition::Create {
                    initial_state: TradePlanState::Proposed,
                }
            }
            AiTradingAction::NoTrade => {
                if plan.target_trade_plan_id().is_some() {
                    return Err(DecisionContextError::TradePlanTargetMismatch);
                }
                TradePlanLedgerDisposition::Create {
                    initial_state: TradePlanState::Closed,
                }
            }
            AiTradingAction::Hold
            | AiTradingAction::CancelEntry
            | AiTradingAction::ModifyProtection
            | AiTradingAction::Reduce
            | AiTradingAction::Exit => {
                if plan.target_trade_plan_id() != Some(trade_plan_id) {
                    return Err(DecisionContextError::TradePlanTargetMismatch);
                }
                TradePlanLedgerDisposition::AppendToExisting
            }
        };
        Ok(Self {
            context,
            response,
            plan,
            trade_plan_id,
            action_id,
            recorded_at_unix_millis,
            disposition,
        })
    }

    #[must_use]
    pub const fn context(&self) -> &AiDecisionContext {
        &self.context
    }

    #[must_use]
    pub const fn response(&self) -> &AiRawResponse {
        &self.response
    }

    #[must_use]
    pub const fn plan(&self) -> &AiTradingPlan {
        &self.plan
    }

    #[must_use]
    pub const fn trade_plan_id(&self) -> TradePlanId {
        self.trade_plan_id
    }

    #[must_use]
    pub const fn action_id(&self) -> TradePlanActionId {
        self.action_id
    }

    #[must_use]
    pub const fn recorded_at_unix_millis(&self) -> u64 {
        self.recorded_at_unix_millis
    }

    #[must_use]
    pub const fn disposition(&self) -> TradePlanLedgerDisposition {
        self.disposition
    }

    #[must_use]
    pub fn trace_json(&self) -> Value {
        json!({
            "context_id": self.context.context_id().to_string(),
            "context_hash": self.context.context_hash().to_string(),
            "response_id": self.response.response_id().to_string(),
            "response_hash": self.response.response_hash().to_string(),
            "ai_plan_id": self.plan.plan_id().to_string(),
            "ai_plan_hash": self.plan.plan_hash().to_string(),
            "trade_plan_id": self.trade_plan_id.to_string(),
            "action_id": self.action_id.to_string(),
            "action": self.plan.action().as_str(),
            "recorded_at_unix_millis": self.recorded_at_unix_millis
        })
    }
}

fn validate_label(field: &'static str, value: &str) -> Result<(), DecisionContextError> {
    if value.is_empty()
        || value.len() > MAX_PROVIDER_LABEL_LENGTH
        || value.chars().any(char::is_control)
    {
        return Err(DecisionContextError::InvalidLabel { field });
    }
    Ok(())
}

fn candle_json(candle: &ClosedCandle) -> Value {
    json!({
        "open_at_unix_millis": candle.open_at_unix_millis(),
        "close_at_unix_millis": candle.close_at_unix_millis(),
        "open": candle.open(),
        "high": candle.high(),
        "low": candle.low(),
        "close": candle.close(),
        "volume": candle.volume(),
        "turnover": candle.turnover()
    })
}

fn top_of_book_json(book: &TopOfBook) -> Value {
    json!({
        "source_generated_at_unix_millis": book.source_generated_at_unix_millis(),
        "observed_at_unix_millis": book.observed_at_unix_millis(),
        "bid_price": book.bid_price(),
        "bid_quantity": book.bid_quantity(),
        "ask_price": book.ask_price(),
        "ask_quantity": book.ask_quantity()
    })
}

fn market_features_json(snapshot: &MarketFeatureSnapshot) -> Value {
    json!({
        "feature_version": snapshot.feature_version(),
        "generated_at_unix_millis": snapshot.generated_at_unix_millis(),
        "valid_until_unix_millis": snapshot.valid_until_unix_millis(),
        "source": market_data_source_name(snapshot.source()),
        "input_hash": snapshot.input_hash().to_string(),
        "primary_15m": timeframe_features_json(snapshot.primary()),
        "confirmation_1h": timeframe_features_json(snapshot.confirmation()),
        "bid_price": snapshot.bid_price(),
        "ask_price": snapshot.ask_price(),
        "spread_bps": snapshot.spread_bps(),
        "snapshot_hash": snapshot.snapshot_hash().to_string()
    })
}

fn timeframe_features_json(features: &TimeframeFeatures) -> Value {
    let pattern = features.pattern().map(|pattern| {
        json!({
            "pattern": candle_pattern_name(pattern.pattern()),
            "semantic": pattern_semantic_name(pattern.semantic())
        })
    });
    json!({
        "timeframe": timeframe_name(features.timeframe()),
        "candle_open_at_unix_millis": features.candle_open_at_unix_millis(),
        "candle_close_at_unix_millis": features.candle_close_at_unix_millis(),
        "latest_open": features.latest_open(),
        "latest_high": features.latest_high(),
        "latest_low": features.latest_low(),
        "latest_close": features.latest_close(),
        "latest_volume": features.latest_volume(),
        "latest_turnover": features.latest_turnover(),
        "donchian_upper": features.donchian_upper(),
        "donchian_lower": features.donchian_lower(),
        "ema_fast": features.ema_fast(),
        "ema_slow": features.ema_slow(),
        "rsi": features.rsi(),
        "atr": features.atr(),
        "adx": features.adx(),
        "volume_ratio": features.volume_ratio(),
        "ema_alignment": ema_alignment_name(features.ema_alignment()),
        "key_location": key_location_name(features.key_location()),
        "pattern": pattern
    })
}

fn instrument_rules_json(snapshot: &InstrumentRulesSnapshot, rules: &SpotInstrumentRules) -> Value {
    json!({
        "observed_at_unix_millis": snapshot.observed_at_unix_millis(),
        "valid_until_unix_millis": snapshot.valid_until_unix_millis(),
        "exchange_server_time_unix_millis": snapshot.server_time().response_unix_millis(),
        "rules_hash": snapshot.rules_hash().to_string(),
        "instrument_id": rules.instrument_id().to_string(),
        "base_asset": rules.base_asset().as_str(),
        "quote_asset": rules.quote_asset().as_str(),
        "trading_status": instrument_trading_status_name(rules.trading_status()),
        "price_tick": rules.price_tick(),
        "base_precision": rules.base_precision(),
        "quote_precision": rules.quote_precision(),
        "minimum_order_amount": rules.minimum_order_amount(),
        "maximum_limit_order_quantity": rules.maximum_limit_order_quantity(),
        "maximum_market_order_quantity": rules.maximum_market_order_quantity(),
        "maximum_post_only_order_quantity": rules.maximum_post_only_order_quantity(),
        "price_limit_ratio_x": rules.price_limit_ratio_x(),
        "price_limit_ratio_y": rules.price_limit_ratio_y()
    })
}

fn portfolio_json(snapshot: &PortfolioSnapshot) -> Value {
    json!({
        "schema_version": snapshot.schema_version(),
        "observed_at_unix_millis": snapshot.observed_at_unix_millis(),
        "status": portfolio_status_name(snapshot.status()),
        "snapshot_hash": snapshot.snapshot_hash().to_string(),
        "assets": snapshot.assets().iter().map(|asset| {
            json!({
                "asset": asset.asset().as_str(),
                "exchange_available_quantity": asset.exchange_available_quantity(),
                "exchange_locked_quantity": asset.exchange_locked_quantity(),
                "exchange_total_quantity": asset.exchange_total_quantity(),
                "local_expected_quantity": asset.local_expected_quantity(),
                "managed_quantity": asset.managed_quantity(),
                "unknown_quantity": asset.unknown_quantity(),
                "shortfall_quantity": asset.shortfall_quantity()
            })
        }).collect::<Vec<_>>()
    })
}

fn managed_position_json(position: &ManagedPosition) -> Value {
    json!({
        "instrument_id": position.instrument_id().to_string(),
        "base_asset": position.base_asset().as_str(),
        "quantity": position.quantity()
    })
}

const fn market_data_source_name(source: MarketDataSource) -> &'static str {
    match source {
        MarketDataSource::RestBootstrap => "REST_BOOTSTRAP",
        MarketDataSource::WebSocketLive => "WEBSOCKET_LIVE",
        MarketDataSource::Replay => "REPLAY",
    }
}

const fn timeframe_name(value: MarketTimeframe) -> &'static str {
    match value {
        MarketTimeframe::FifteenMinutes => "15m",
        MarketTimeframe::OneHour => "1h",
    }
}

const fn ema_alignment_name(value: EmaAlignment) -> &'static str {
    match value {
        EmaAlignment::StrongBullish => "STRONG_BULLISH",
        EmaAlignment::Bullish => "BULLISH",
        EmaAlignment::StrongBearish => "STRONG_BEARISH",
        EmaAlignment::Bearish => "BEARISH",
        EmaAlignment::Mixed => "MIXED",
    }
}

const fn key_location_name(value: KeyLocation) -> &'static str {
    match value {
        KeyLocation::None => "NONE",
        KeyLocation::Support => "SUPPORT",
        KeyLocation::Resistance => "RESISTANCE",
    }
}

const fn candle_pattern_name(value: CandlePattern) -> &'static str {
    match value {
        CandlePattern::BigBullish => "BIG_BULLISH",
        CandlePattern::BigBearish => "BIG_BEARISH",
        CandlePattern::Hammer => "HAMMER",
        CandlePattern::HangingMan => "HANGING_MAN",
        CandlePattern::ShootingStar => "SHOOTING_STAR",
        CandlePattern::InvertedHammer => "INVERTED_HAMMER",
        CandlePattern::BullishEngulfing => "BULLISH_ENGULFING",
        CandlePattern::BearishEngulfing => "BEARISH_ENGULFING",
        CandlePattern::BullishHarami => "BULLISH_HARAMI",
        CandlePattern::BearishHarami => "BEARISH_HARAMI",
        CandlePattern::Doji => "DOJI",
    }
}

const fn pattern_semantic_name(value: PatternSemantic) -> &'static str {
    match value {
        PatternSemantic::BullishAttack => "BULLISH_ATTACK",
        PatternSemantic::BearishAttack => "BEARISH_ATTACK",
        PatternSemantic::BullishSupportRejection => "BULLISH_SUPPORT_REJECTION",
        PatternSemantic::BearishExhaustion => "BEARISH_EXHAUSTION",
        PatternSemantic::BearishResistanceRejection => "BEARISH_RESISTANCE_REJECTION",
        PatternSemantic::BullishSupportTest => "BULLISH_SUPPORT_TEST",
        PatternSemantic::BullishReversal => "BULLISH_REVERSAL",
        PatternSemantic::BearishReversal => "BEARISH_REVERSAL",
        PatternSemantic::BearishMomentumExhaustion => "BEARISH_MOMENTUM_EXHAUSTION",
        PatternSemantic::BullishMomentumExhaustion => "BULLISH_MOMENTUM_EXHAUSTION",
        PatternSemantic::Indecision => "INDECISION",
    }
}

const fn account_order_side_name(value: AccountOrderSide) -> &'static str {
    match value {
        AccountOrderSide::Buy => "BUY",
        AccountOrderSide::Sell => "SELL",
    }
}

const fn account_order_status_name(value: AccountOrderStatus) -> &'static str {
    match value {
        AccountOrderStatus::New => "NEW",
        AccountOrderStatus::PartiallyFilled => "PARTIALLY_FILLED",
        AccountOrderStatus::PendingCancel => "PENDING_CANCEL",
    }
}

const fn order_type_name(value: AiOrderType) -> &'static str {
    match value {
        AiOrderType::Limit => "LIMIT",
        AiOrderType::Market => "MARKET",
    }
}

const fn portfolio_status_name(value: PortfolioReconciliationStatus) -> &'static str {
    match value {
        PortfolioReconciliationStatus::Balanced => "BALANCED",
        PortfolioReconciliationStatus::BalanceDifference => "BALANCE_DIFFERENCE",
    }
}

const fn instrument_trading_status_name(value: InstrumentTradingStatus) -> &'static str {
    match value {
        InstrumentTradingStatus::Trading => "TRADING",
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DecisionContextError {
    InvalidTimestamp { field: &'static str },
    InvalidMaximumLoss,
    IncompleteCandleWindow,
    MarketFeature(MarketFeatureError),
    FeatureSnapshotMismatch,
    ExpiredMarketFeatures,
    FutureInstrumentRules,
    ExpiredInstrumentRules,
    MissingInstrumentRules,
    FuturePortfolio,
    ManagedPositionCapacityExceeded,
    DuplicateManagedPosition,
    OpenOrderCapacityExceeded,
    FutureOpenOrder,
    DuplicateOpenOrder,
    InvalidLabel { field: &'static str },
    InvalidOrderQuantity,
    InvalidOrderPrice,
    InvalidRawResponse,
    ProvenanceMismatch,
    InvalidLedgerTimestamp,
    TradePlanTargetMismatch,
}

impl fmt::Display for DecisionContextError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidTimestamp { field } => write!(formatter, "{field} is invalid"),
            Self::InvalidMaximumLoss => formatter.write_str("user maximum loss must be positive"),
            Self::IncompleteCandleWindow => write!(
                formatter,
                "AI Decision Context requires exactly {FEATURE_CANDLE_WINDOW} candles per timeframe"
            ),
            Self::MarketFeature(error) => write!(formatter, "market facts are invalid: {error}"),
            Self::FeatureSnapshotMismatch => formatter
                .write_str("market features do not reproduce from the supplied raw market facts"),
            Self::ExpiredMarketFeatures => {
                formatter.write_str("market features are expired at context time")
            }
            Self::FutureInstrumentRules => {
                formatter.write_str("instrument rules contain future data")
            }
            Self::ExpiredInstrumentRules => {
                formatter.write_str("instrument rules are expired at context time")
            }
            Self::MissingInstrumentRules => {
                formatter.write_str("target instrument rules are missing")
            }
            Self::FuturePortfolio => formatter.write_str("portfolio contains future data"),
            Self::ManagedPositionCapacityExceeded => write!(
                formatter,
                "managed positions exceeds {MAX_CONTEXT_MANAGED_POSITIONS}"
            ),
            Self::DuplicateManagedPosition => {
                formatter.write_str("managed positions contain a duplicate instrument")
            }
            Self::OpenOrderCapacityExceeded => {
                write!(formatter, "open orders exceeds {MAX_CONTEXT_OPEN_ORDERS}")
            }
            Self::FutureOpenOrder => formatter.write_str("open order contains future data"),
            Self::DuplicateOpenOrder => {
                formatter.write_str("open orders contain a duplicate exchange order ID")
            }
            Self::InvalidLabel { field } => write!(formatter, "{field} is invalid"),
            Self::InvalidOrderQuantity => formatter.write_str("open order quantities are invalid"),
            Self::InvalidOrderPrice => {
                formatter.write_str("LIMIT open orders require price and MARKET orders forbid it")
            }
            Self::InvalidRawResponse => {
                formatter.write_str("raw AI response is empty or too large")
            }
            Self::ProvenanceMismatch => formatter
                .write_str("Context, raw response, and AITradingPlan provenance does not match"),
            Self::InvalidLedgerTimestamp => {
                formatter.write_str("TradePlan ledger timestamps are inconsistent or stale")
            }
            Self::TradePlanTargetMismatch => {
                formatter.write_str("AITradingPlan target does not match the ledger TradePlan")
            }
        }
    }
}

impl std::error::Error for DecisionContextError {}
