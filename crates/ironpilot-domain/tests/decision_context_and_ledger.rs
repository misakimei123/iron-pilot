use core::str::FromStr;

use ironpilot_domain::{
    AccountOrderFact, AccountOrderSide, AccountOrderStatus, AiDecisionContext, AiDecisionContextId,
    AiOrderType, AiProviderResponseId, AiRawResponse, AiTradePlanLedgerEntry, AiTradingPlan,
    AssetCode, ClosedCandle, DomainDecimal, ExchangeAssetBalance, ExchangeServerTime,
    FEATURE_CANDLE_WINDOW, InstrumentId, InstrumentRulesSnapshot, InstrumentTradingStatus,
    LocalAssetBalance, ManagedPosition, MarketDataSource, MarketFeatureEngine, MarketTimeframe,
    PortfolioReconciler, RulesHash, TopOfBook, TradePlanActionId, TradePlanId,
    TradePlanLedgerDisposition, TradePlanState, validated_spot_instrument_rules,
};
use serde_json::{Value, json};

const END_AT: u64 = 1_800_000_000_000;
const AS_OF: u64 = END_AT + 1_000;

fn decimal(value: &str) -> DomainDecimal {
    DomainDecimal::from_str(value).expect("test decimal must be valid")
}

fn instrument(symbol: &str) -> InstrumentId {
    InstrumentId::from_str(&format!("bybit:spot:{symbol}")).expect("test instrument must be valid")
}

fn stable_id<T: FromStr>(value: u128) -> T
where
    T::Err: core::fmt::Debug,
{
    T::from_str(&format!("{value:032x}")).expect("test ID must be valid")
}

fn linear_candles(timeframe: MarketTimeframe) -> Vec<ClosedCandle> {
    let duration = timeframe.duration_millis();
    let first_open =
        END_AT - duration * u64::try_from(FEATURE_CANDLE_WINDOW).expect("window fits u64");
    (0..FEATURE_CANDLE_WINDOW)
        .map(|index| {
            let price = 100 + i64::try_from(index).expect("index fits i64");
            ClosedCandle::new(
                instrument("BTCUSDT"),
                timeframe,
                first_open + duration * u64::try_from(index).expect("index fits u64"),
                decimal(&price.to_string()),
                decimal(&(price + 1).to_string()),
                decimal(&(price - 1).to_string()),
                decimal(&price.to_string()),
                decimal("10"),
                decimal(&(price * 10).to_string()),
                true,
            )
            .expect("candle fixture must be valid")
        })
        .collect()
}

fn top_of_book(bid: &str) -> TopOfBook {
    TopOfBook::new(
        instrument("BTCUSDT"),
        END_AT,
        END_AT + 500,
        decimal(bid),
        decimal("10"),
        decimal("219.1"),
        decimal("12"),
    )
    .expect("book fixture must be valid")
}

fn rules_snapshot() -> InstrumentRulesSnapshot {
    let rules = validated_spot_instrument_rules(
        instrument("BTCUSDT"),
        AssetCode::new("BTC").expect("asset is valid"),
        AssetCode::new("USDT").expect("asset is valid"),
        InstrumentTradingStatus::Trading,
        decimal("0.10"),
        decimal("0.000001"),
        decimal("0.000001"),
        decimal("5"),
        decimal("100"),
        decimal("50"),
        decimal("25"),
        decimal("0.01"),
        decimal("0.02"),
    )
    .expect("rules fixture must be valid");
    InstrumentRulesSnapshot::new(
        vec![rules],
        ExchangeServerTime::new(END_AT / 1_000, END_AT * 1_000_000, END_AT)
            .expect("server time is consistent"),
        END_AT,
        END_AT + 60_000,
        RulesHash::from_sha256([7; 32]),
    )
    .expect("rules snapshot must be valid")
}

fn portfolio(observed_at: u64) -> ironpilot_domain::PortfolioSnapshot {
    PortfolioReconciler::reconcile(
        vec![
            ExchangeAssetBalance::new(
                AssetCode::new("BTC").expect("asset is valid"),
                decimal("0.5"),
                decimal("0"),
            )
            .expect("balance is valid"),
            ExchangeAssetBalance::new(
                AssetCode::new("USDT").expect("asset is valid"),
                decimal("1000"),
                decimal("0"),
            )
            .expect("balance is valid"),
        ],
        vec![
            LocalAssetBalance::new(
                AssetCode::new("BTC").expect("asset is valid"),
                decimal("0.5"),
                decimal("0.4"),
            )
            .expect("local balance is valid"),
            LocalAssetBalance::new(
                AssetCode::new("USDT").expect("asset is valid"),
                decimal("1000"),
                decimal("0"),
            )
            .expect("local balance is valid"),
        ],
        observed_at,
    )
    .expect("portfolio fixture must be valid")
}

fn open_order(id: &str, observed_at: u64) -> AccountOrderFact {
    AccountOrderFact::new(
        id,
        Some(format!("ironpilot-{id}")),
        instrument("BTCUSDT"),
        AccountOrderSide::Buy,
        AiOrderType::Limit,
        Some(decimal("210")),
        decimal("0.10"),
        decimal("0.02"),
        AccountOrderStatus::PartiallyFilled,
        observed_at,
    )
    .expect("open order fixture must be valid")
}

fn context_with(
    context_id: AiDecisionContextId,
    book: TopOfBook,
    orders: Vec<AccountOrderFact>,
    portfolio_observed_at: u64,
) -> Result<AiDecisionContext, ironpilot_domain::DecisionContextError> {
    let primary = linear_candles(MarketTimeframe::FifteenMinutes);
    let confirmation = linear_candles(MarketTimeframe::OneHour);
    let features = MarketFeatureEngine::compute(
        &primary,
        &confirmation,
        &top_of_book("218.9"),
        AS_OF,
        MarketDataSource::WebSocketLive,
    )
    .expect("feature fixture must be valid");
    AiDecisionContext::new(
        context_id,
        AS_OF,
        primary,
        confirmation,
        book,
        features,
        &rules_snapshot(),
        &portfolio(portfolio_observed_at),
        vec![
            ManagedPosition::new(
                instrument("ETHUSDT"),
                AssetCode::new("ETH").expect("asset is valid"),
                decimal("2"),
            )
            .expect("position is valid"),
            ManagedPosition::new(
                instrument("BTCUSDT"),
                AssetCode::new("BTC").expect("asset is valid"),
                decimal("0.4"),
            )
            .expect("position is valid"),
        ],
        orders,
        decimal("25.00"),
    )
}

fn context() -> AiDecisionContext {
    context_with(
        stable_id(1),
        top_of_book("218.9"),
        vec![open_order("order-b", END_AT), open_order("order-a", END_AT)],
        END_AT,
    )
    .expect("context fixture must be valid")
}

fn open_long_plan(context_id: AiDecisionContextId) -> AiTradingPlan {
    AiTradingPlan::from_json(
        &json!({
            "schema_version": "3.0",
            "plan_id": "00000000-0000-0000-0000-000000000010",
            "context_id": context_id.to_string(),
            "instrument_id": "bybit:spot:BTCUSDT",
            "action": "OPEN_LONG",
            "valid_until": END_AT + 20_000,
            "order": {
                "type": "LIMIT",
                "quantity": "0.10",
                "limit_price": "210.00",
                "time_in_force": "GTC",
                "expires_at": END_AT + 20_000,
                "max_slippage_quote": "1.00"
            },
            "protective_stop": {
                "trigger_price": "200.00",
                "order_type": "MARKET"
            },
            "take_profits": [{"price": "230.00", "quantity": "0.10"}],
            "declared_max_loss_quote": "2.00",
            "review": {
                "next_review_at": END_AT + 10_000,
                "max_holding_until": END_AT + 100_000
            },
            "confidence": "0.70",
            "thesis": "The complete supplied facts support this AI-selected entry.",
            "invalidation": "Exit if subsequent market facts invalidate the thesis.",
            "risks": ["The market can reverse."]
        })
        .to_string(),
    )
    .expect("AI plan fixture must be valid")
}

#[test]
fn context_contains_complete_reproducible_facts_without_local_recommendation() {
    let context = context();
    let value: Value = serde_json::from_str(context.to_json()).expect("context JSON must parse");

    assert_eq!(
        value["market"]["candles_15m"]
            .as_array()
            .expect("15m candles are an array")
            .len(),
        FEATURE_CANDLE_WINDOW
    );
    assert_eq!(
        value["market"]["candles_1h"]
            .as_array()
            .expect("1h candles are an array")
            .len(),
        FEATURE_CANDLE_WINDOW
    );
    assert_eq!(value["market"]["features"]["primary_15m"]["rsi"], "100");
    assert_eq!(
        value["instrument_rules"]["price_tick"],
        decimal("0.10").to_string()
    );
    assert_eq!(
        value["account"]["portfolio"]["assets"]
            .as_array()
            .map(Vec::len),
        Some(2)
    );
    assert_eq!(
        value["account"]["managed_positions"]
            .as_array()
            .map(Vec::len),
        Some(2)
    );
    assert_eq!(
        value["account"]["open_orders"].as_array().map(Vec::len),
        Some(2)
    );
    assert_eq!(value["user_authorization"]["maximum_loss_quote"], "25.00");
    for forbidden in [
        "\"action\"",
        "\"recommendation\"",
        "\"strategy_space\"",
        "\"risk_tier\"",
        "\"entry_anchor\"",
        "\"eligibility\"",
    ] {
        assert!(
            !context.to_json().contains(forbidden),
            "Context must not contain local trading recommendation field {forbidden}"
        );
    }
}

#[test]
fn input_order_does_not_change_context_json_or_hash() {
    let context_id = stable_id(20);
    let first = context_with(
        context_id,
        top_of_book("218.9"),
        vec![open_order("order-b", END_AT), open_order("order-a", END_AT)],
        END_AT,
    )
    .expect("first context must be valid");
    let second = context_with(
        context_id,
        top_of_book("218.9"),
        vec![open_order("order-a", END_AT), open_order("order-b", END_AT)],
        END_AT,
    )
    .expect("second context must be valid");

    assert_eq!(first.to_json(), second.to_json());
    assert_eq!(first.context_hash(), second.context_hash());
}

#[test]
fn future_or_non_reproducible_facts_fail_closed() {
    let mismatch = context_with(stable_id(21), top_of_book("218.8"), Vec::new(), END_AT);
    assert!(matches!(
        mismatch,
        Err(ironpilot_domain::DecisionContextError::FeatureSnapshotMismatch)
    ));

    let future_order = context_with(
        stable_id(22),
        top_of_book("218.9"),
        vec![open_order("future", AS_OF + 1)],
        END_AT,
    );
    assert!(matches!(
        future_order,
        Err(ironpilot_domain::DecisionContextError::FutureOpenOrder)
    ));

    let future_portfolio = context_with(stable_id(23), top_of_book("218.9"), Vec::new(), AS_OF + 1);
    assert!(matches!(
        future_portfolio,
        Err(ironpilot_domain::DecisionContextError::FuturePortfolio)
    ));
}

#[test]
fn ledger_binds_context_raw_response_plan_and_action_provenance() {
    let context = context();
    let response = AiRawResponse::new(
        stable_id::<AiProviderResponseId>(30),
        context.context_id(),
        "deepseek",
        "deepseek-chat",
        END_AT + 2_000,
        open_long_plan(context.context_id()).to_json(),
    )
    .expect("raw response must be valid");
    let plan = open_long_plan(context.context_id());
    let entry = AiTradePlanLedgerEntry::new(
        context.clone(),
        response.clone(),
        plan.clone(),
        stable_id::<TradePlanId>(31),
        stable_id::<TradePlanActionId>(32),
        END_AT + 3_000,
    )
    .expect("ledger entry must be valid");

    assert_eq!(
        entry.disposition(),
        TradePlanLedgerDisposition::Create {
            initial_state: TradePlanState::Proposed
        }
    );
    let trace = entry.trace_json();
    assert_eq!(trace["context_hash"], context.context_hash().to_string());
    assert_eq!(trace["response_hash"], response.response_hash().to_string());
    assert_eq!(trace["ai_plan_hash"], plan.plan_hash().to_string());
    assert_eq!(trace["action"], "OPEN_LONG");
}

#[test]
fn ledger_rejects_cross_context_response_and_stale_recording() {
    let context = context();
    let wrong_response = AiRawResponse::new(
        stable_id::<AiProviderResponseId>(40),
        stable_id::<AiDecisionContextId>(41),
        "deepseek",
        "deepseek-chat",
        END_AT + 2_000,
        "{}",
    )
    .expect("response shape is valid");
    assert!(
        AiTradePlanLedgerEntry::new(
            context.clone(),
            wrong_response,
            open_long_plan(context.context_id()),
            stable_id::<TradePlanId>(42),
            stable_id::<TradePlanActionId>(43),
            END_AT + 3_000,
        )
        .is_err()
    );

    let response = AiRawResponse::new(
        stable_id::<AiProviderResponseId>(44),
        context.context_id(),
        "deepseek",
        "deepseek-chat",
        END_AT + 2_000,
        "{}",
    )
    .expect("response shape is valid");
    assert!(
        AiTradePlanLedgerEntry::new(
            context.clone(),
            response,
            open_long_plan(context.context_id()),
            stable_id::<TradePlanId>(45),
            stable_id::<TradePlanActionId>(46),
            context.valid_until_unix_millis(),
        )
        .is_err()
    );
}
