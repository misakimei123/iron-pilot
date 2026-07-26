use core::str::FromStr;

use ironpilot_application::{
    ExecutionAuthorization, ExecutionMode, ExecutionOrderIdSet, ExecutionOrderIds,
    ExecutionOrderRole, ExecutionValidationOutcome, ExecutionValidationPolicy,
    ExecutionValidationRejection, ExecutionValidationRequest, ExecutionValidator,
    PaperExecutionError, PaperExecutionPolicy, PaperMarketObservation, PaperMatchingEngine,
    PaperOpenOrder, PaperOrderEvaluation, SpotExecutionRequest, SpotOrderPriceLimits,
};
use ironpilot_domain::{
    AccountOrderFact, AccountOrderSide, AccountOrderStatus, AiDecisionContext, AiDecisionContextId,
    AiOrderType, AiTradingPlan, AssetCode, ClosedCandle, DomainDecimal, ExchangeAssetBalance,
    ExchangeServerTime, FEATURE_CANDLE_WINDOW, InstrumentId, InstrumentRulesSnapshot,
    InstrumentTradingStatus, LocalAssetBalance, MarketDataSource, MarketFeatureEngine,
    MarketTimeframe, OrderId, OrderIntentId, PortfolioReconciler, PortfolioSnapshot, RulesHash,
    SnapshotId, TopOfBook, TradePlanActionId, TradePlanId, validated_spot_instrument_rules,
};
use serde_json::{Value, json};

const END_AT: u64 = 1_800_000_000_000;
const AS_OF: u64 = END_AT + 1_000;
const VALIDATED_AT: u64 = END_AT + 3_000;

fn decimal(value: &str) -> DomainDecimal {
    DomainDecimal::from_str(value).expect("test decimal must be valid")
}

fn instrument() -> InstrumentId {
    InstrumentId::from_str("bybit:spot:BTCUSDT").expect("test instrument must be valid")
}

fn stable_id<T: FromStr>(value: u128) -> T
where
    T::Err: core::fmt::Debug,
{
    T::from_str(&format!("{value:032x}")).expect("test ID must be valid")
}

fn candles(timeframe: MarketTimeframe) -> Vec<ClosedCandle> {
    let duration = timeframe.duration_millis();
    let first_open =
        END_AT - duration * u64::try_from(FEATURE_CANDLE_WINDOW).expect("window fits u64");
    (0..FEATURE_CANDLE_WINDOW)
        .map(|index| {
            let price = 100 + i64::try_from(index).expect("index fits i64");
            ClosedCandle::new(
                instrument(),
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

fn rules_snapshot() -> InstrumentRulesSnapshot {
    let rules = validated_spot_instrument_rules(
        instrument(),
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
            .expect("server time is valid"),
        END_AT,
        END_AT + 60_000,
        RulesHash::from_sha256([7; 32]),
    )
    .expect("rules snapshot must be valid")
}

fn portfolio(maximum_quote: &str) -> PortfolioSnapshot {
    PortfolioReconciler::reconcile(
        vec![
            ExchangeAssetBalance::new(
                AssetCode::new("BTC").expect("asset is valid"),
                decimal("0"),
                decimal("0"),
            )
            .expect("balance is valid"),
            ExchangeAssetBalance::new(
                AssetCode::new("USDT").expect("asset is valid"),
                decimal(maximum_quote),
                decimal("0"),
            )
            .expect("balance is valid"),
        ],
        vec![
            LocalAssetBalance::new(
                AssetCode::new("BTC").expect("asset is valid"),
                decimal("0"),
                decimal("0"),
            )
            .expect("local balance is valid"),
            LocalAssetBalance::new(
                AssetCode::new("USDT").expect("asset is valid"),
                decimal(maximum_quote),
                decimal("0"),
            )
            .expect("local balance is valid"),
        ],
        END_AT,
    )
    .expect("portfolio fixture must be valid")
}

struct OpenFixture {
    context: AiDecisionContext,
    plan: AiTradingPlan,
    rules: InstrumentRulesSnapshot,
    portfolio: PortfolioSnapshot,
    book: TopOfBook,
    limits: SpotOrderPriceLimits,
    authorization: ExecutionAuthorization,
    policy: ExecutionValidationPolicy,
    open_orders: Vec<AccountOrderFact>,
    trade_plan_id: TradePlanId,
    action_id: TradePlanActionId,
}

impl OpenFixture {
    fn new(maximum_loss: &str) -> Self {
        Self::new_with_orders(maximum_loss, Vec::new())
    }

    fn new_with_orders(maximum_loss: &str, open_orders: Vec<AccountOrderFact>) -> Self {
        let primary = candles(MarketTimeframe::FifteenMinutes);
        let confirmation = candles(MarketTimeframe::OneHour);
        let book = TopOfBook::new(
            instrument(),
            END_AT,
            END_AT + 500,
            decimal("209.90"),
            decimal("10"),
            decimal("210.10"),
            decimal("12"),
        )
        .expect("book fixture must be valid");
        let features = MarketFeatureEngine::compute(
            &primary,
            &confirmation,
            &book,
            AS_OF,
            MarketDataSource::WebSocketLive,
        )
        .expect("feature fixture must be valid");
        let rules = rules_snapshot();
        let portfolio = portfolio("1000");
        let context_id = stable_id(1);
        let context = AiDecisionContext::new(
            context_id,
            AS_OF,
            primary,
            confirmation,
            book.clone(),
            features,
            &rules,
            &portfolio,
            Vec::new(),
            open_orders.clone(),
            decimal(maximum_loss),
        )
        .expect("context fixture must be valid");
        let plan = plan_with(context_id, |_| {});
        Self {
            context,
            plan,
            rules,
            portfolio,
            book,
            limits: SpotOrderPriceLimits::new(
                instrument(),
                decimal("220"),
                decimal("200"),
                END_AT + 2_500,
            )
            .expect("price limits must be valid"),
            authorization: ExecutionAuthorization::new(
                ExecutionMode::Paper,
                true,
                vec![instrument()],
            )
            .expect("authorization must be valid"),
            policy: ExecutionValidationPolicy::new(decimal("0.001"), 5_000, 5_000)
                .expect("policy must be valid"),
            open_orders,
            trade_plan_id: stable_id(2),
            action_id: stable_id(3),
        }
    }

    fn validate(
        &self,
        plan: &AiTradingPlan,
        validated_at: u64,
    ) -> ironpilot_application::ExecutionValidationDecision {
        ExecutionValidator::validate(ExecutionValidationRequest {
            action_id: self.action_id,
            trade_plan_id: self.trade_plan_id,
            context: &self.context,
            plan,
            rules: &self.rules,
            portfolio: &self.portfolio,
            managed_positions: &[],
            open_orders: &self.open_orders,
            active_trade_plans: &[],
            top_of_book: &self.book,
            price_limits: &self.limits,
            current_maximum_loss_quote: self.context.maximum_loss_quote(),
            authorization: &self.authorization,
            policy: self.policy,
            validated_at_unix_millis: validated_at,
        })
    }
}

fn open_order() -> AccountOrderFact {
    AccountOrderFact::new(
        "existing-order",
        Some("existing-link"),
        instrument(),
        AccountOrderSide::Buy,
        AiOrderType::Limit,
        Some(decimal("205")),
        decimal("0.10"),
        decimal("0"),
        AccountOrderStatus::New,
        END_AT,
    )
    .expect("open-order fixture must be valid")
}

fn plan_with(context_id: AiDecisionContextId, mutate: impl FnOnce(&mut Value)) -> AiTradingPlan {
    let mut value = json!({
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
        "declared_max_loss_quote": "3.00",
        "review": {
            "next_review_at": END_AT + 10_000,
            "max_holding_until": END_AT + 100_000
        },
        "confidence": "0.70",
        "thesis": "Complete facts support this AI-selected entry.",
        "invalidation": "Exit if subsequent facts invalidate the thesis.",
        "risks": ["The market can reverse."]
    });
    mutate(&mut value);
    AiTradingPlan::from_json(&value.to_string())
        .expect("mutated test plan must remain schema-valid")
}

#[test]
fn exact_plan_is_accepted_without_returning_or_rewriting_trade_fields() {
    let fixture = OpenFixture::new("25");
    let decision = fixture.validate(&fixture.plan, VALIDATED_AT);

    assert_eq!(decision.outcome(), ExecutionValidationOutcome::Accept);
    assert!(decision.rejections().is_empty());
    assert_eq!(
        decision.recalculated_maximum_loss_quote(),
        Some(decimal("2.041"))
    );
    assert!(decision.authorizes_unchanged(&fixture.plan));
    assert_eq!(
        decision.plan_hash(),
        fixture.plan.plan_hash(),
        "validation evidence must bind the exact AI plan"
    );
    assert!(
        !decision.evidence_json().contains("adjust"),
        "validation evidence must not expose an adjustment path"
    );
}

#[test]
fn tick_quantity_minimum_amount_and_price_limit_fail_closed() {
    let fixture = OpenFixture::new("25");
    let cases = [
        (
            plan_with(fixture.context.context_id(), |value| {
                value["order"]["limit_price"] = json!("210.05");
            }),
            ExecutionValidationRejection::InvalidPriceIncrement,
        ),
        (
            plan_with(fixture.context.context_id(), |value| {
                value["order"]["quantity"] = json!("0.1000005");
                value["take_profits"][0]["quantity"] = json!("0.1000005");
            }),
            ExecutionValidationRejection::InvalidQuantityIncrement,
        ),
        (
            plan_with(fixture.context.context_id(), |value| {
                value["order"]["quantity"] = json!("0.01");
                value["take_profits"][0]["quantity"] = json!("0.01");
            }),
            ExecutionValidationRejection::MinimumOrderAmount,
        ),
        (
            plan_with(fixture.context.context_id(), |value| {
                value["order"]["limit_price"] = json!("220.10");
            }),
            ExecutionValidationRejection::PriceOutsideExchangeLimit,
        ),
    ];

    for (plan, expected) in cases {
        let decision = fixture.validate(&plan, VALIDATED_AT);
        assert_eq!(decision.outcome(), ExecutionValidationOutcome::Reject);
        assert!(
            decision.rejections().contains(&expected),
            "missing rejection {expected:?}: {:?}",
            decision.rejections()
        );
    }
}

#[test]
fn fees_slippage_and_user_maximum_loss_are_independently_enforced() {
    let low_declaration = OpenFixture::new("25");
    let plan = plan_with(low_declaration.context.context_id(), |value| {
        value["declared_max_loss_quote"] = json!("2.00");
    });
    let decision = low_declaration.validate(&plan, VALIDATED_AT);
    assert!(
        decision
            .rejections()
            .contains(&ExecutionValidationRejection::DeclaredMaximumLossTooLow)
    );
    let feedback = decision
        .replan_feedback(&low_declaration.context, &plan)
        .expect("rejected validation must produce bounded AI feedback");
    assert!(
        feedback.reasons()[0].contains("DECLARED_MAXIMUM_LOSS_TOO_LOW"),
        "rejection codes must flow into the one bounded AI replan"
    );

    let over_authorization = OpenFixture::new("2.02");
    let decision = over_authorization.validate(&over_authorization.plan, VALIDATED_AT);
    assert!(
        decision
            .rejections()
            .contains(&ExecutionValidationRejection::MaximumLossExceeded)
    );
}

#[test]
fn stale_context_and_any_post_validation_field_change_cannot_authorize_execution() {
    let fixture = OpenFixture::new("25");
    let stale = fixture.validate(&fixture.plan, fixture.context.valid_until_unix_millis());
    assert_eq!(stale.outcome(), ExecutionValidationOutcome::Reject);
    assert!(
        stale
            .rejections()
            .contains(&ExecutionValidationRejection::StaleContext)
    );

    let accepted = fixture.validate(&fixture.plan, VALIDATED_AT);
    let changed_plans = [
        plan_with(fixture.context.context_id(), |value| {
            value["order"]["limit_price"] = json!("211.00");
        }),
        plan_with(fixture.context.context_id(), |value| {
            value["order"]["quantity"] = json!("0.11");
            value["take_profits"][0]["quantity"] = json!("0.11");
        }),
        plan_with(fixture.context.context_id(), |value| {
            value["protective_stop"]["trigger_price"] = json!("199.00");
        }),
        plan_with(fixture.context.context_id(), |value| {
            value["take_profits"][0]["price"] = json!("231.00");
        }),
    ];
    for changed in changed_plans {
        assert_ne!(fixture.plan.plan_hash(), changed.plan_hash());
        assert!(!accepted.authorizes_unchanged(&changed));
    }
}

#[test]
fn observe_only_permission_rejects_an_order_bearing_plan() {
    let mut fixture = OpenFixture::new("25");
    fixture.authorization =
        ExecutionAuthorization::new(ExecutionMode::ObserveOnly, true, vec![instrument()])
            .expect("authorization must be valid");

    let decision = fixture.validate(&fixture.plan, VALIDATED_AT);
    assert_eq!(decision.outcome(), ExecutionValidationOutcome::Reject);
    assert!(
        decision
            .rejections()
            .contains(&ExecutionValidationRejection::ExecutionModeNotAuthorized)
    );
}

#[test]
fn a_conflicting_exchange_order_rejects_the_whole_plan() {
    let fixture = OpenFixture::new_with_orders("25", vec![open_order()]);
    let decision = fixture.validate(&fixture.plan, VALIDATED_AT);

    assert_eq!(decision.outcome(), ExecutionValidationOutcome::Reject);
    assert!(
        decision
            .rejections()
            .contains(&ExecutionValidationRejection::ConflictingOrder)
    );
}

fn execution_ids() -> ExecutionOrderIdSet {
    ExecutionOrderIdSet::new(
        Some(ExecutionOrderIds::new(
            stable_id::<OrderIntentId>(100),
            stable_id::<OrderId>(101),
        )),
        Some(ExecutionOrderIds::new(
            stable_id::<OrderIntentId>(102),
            stable_id::<OrderId>(103),
        )),
        vec![ExecutionOrderIds::new(
            stable_id::<OrderIntentId>(104),
            stable_id::<OrderId>(105),
        )],
    )
    .expect("execution IDs must be valid")
}

#[test]
fn execution_request_preserves_every_ai_order_and_protection_field() {
    let fixture = OpenFixture::new("25");
    let decision = fixture.validate(&fixture.plan, VALIDATED_AT);
    let request = SpotExecutionRequest::from_accepted_plan(
        &fixture.context,
        &decision,
        &fixture.plan,
        execution_ids(),
        VALIDATED_AT + 1,
    )
    .expect("accepted plan must create an execution request");

    assert_eq!(request.orders().len(), 3);
    let entry = &request.orders()[0];
    assert_eq!(entry.role(), ExecutionOrderRole::Entry);
    assert_eq!(entry.order_type(), AiOrderType::Limit);
    assert_eq!(entry.quantity(), Some(decimal("0.10")));
    assert_eq!(entry.limit_price(), Some(decimal("210.00")));
    assert_eq!(
        entry.time_in_force(),
        Some(ironpilot_domain::AiTimeInForce::Gtc)
    );
    assert_eq!(entry.max_slippage_quote(), decimal("1.00"));
    let stop = &request.orders()[1];
    assert_eq!(stop.role(), ExecutionOrderRole::ProtectiveStop);
    assert_eq!(stop.trigger_price(), Some(decimal("200.00")));
    assert_eq!(stop.quantity(), None);
    let target = &request.orders()[2];
    assert_eq!(target.role(), ExecutionOrderRole::TakeProfit { index: 0 });
    assert_eq!(target.limit_price(), Some(decimal("230.00")));
    assert_eq!(target.quantity(), Some(decimal("0.10")));
    assert_eq!(request.source_plan_json(), fixture.plan.to_json());

    let duplicate = SpotExecutionRequest::from_accepted_plan(
        &fixture.context,
        &decision,
        &fixture.plan,
        execution_ids(),
        VALIDATED_AT + 1,
    )
    .expect("same request must remain reproducible");
    assert_eq!(request.request_hash(), duplicate.request_hash());
    assert_eq!(request.payload_json(), duplicate.payload_json());
}

#[test]
fn paper_limit_matching_is_partial_fee_aware_and_rejects_decision_bar_reuse() {
    let fixture = OpenFixture::new("25");
    let decision = fixture.validate(&fixture.plan, VALIDATED_AT);
    let request = SpotExecutionRequest::from_accepted_plan(
        &fixture.context,
        &decision,
        &fixture.plan,
        execution_ids(),
        VALIDATED_AT + 1,
    )
    .expect("accepted plan must create an execution request");
    let order = PaperOpenOrder::new(
        request.orders()[0].clone(),
        instrument(),
        request.context_as_of_unix_millis(),
        request.created_at_unix_millis(),
        decimal("0.10"),
    )
    .expect("paper order must be valid");
    let policy = PaperExecutionPolicy::new(decimal("0.001"), decimal("0.002"), decimal("0.001"))
        .expect("paper policy must be valid");
    let observation = PaperMarketObservation::new(
        stable_id::<SnapshotId>(200),
        instrument(),
        AS_OF + 1,
        VALIDATED_AT + 2,
        decimal("209.90"),
        decimal("210.10"),
        decimal("209.00"),
        decimal("211.00"),
        decimal("0.04"),
    )
    .expect("observation must be valid");
    let PaperOrderEvaluation::Fill(fill) =
        PaperMatchingEngine::evaluate(&order, &observation, decimal("0.04"), policy)
            .expect("matching must succeed")
    else {
        panic!("limit order should partially fill");
    };
    assert_eq!(fill.base_quantity(), decimal("0.04"));
    assert_eq!(fill.execution_price(), decimal("210.00"));
    assert_eq!(fill.quote_quantity(), decimal("8.4000"));
    assert_eq!(fill.fee_quote(), decimal("0.0084000"));

    let reused = PaperMarketObservation::new(
        stable_id::<SnapshotId>(201),
        instrument(),
        AS_OF,
        VALIDATED_AT + 2,
        decimal("209.90"),
        decimal("210.10"),
        decimal("209.00"),
        decimal("211.00"),
        decimal("0.04"),
    )
    .expect("observation shape must be valid");
    assert_eq!(
        PaperMatchingEngine::evaluate(&order, &reused, decimal("0.04"), policy),
        Err(PaperExecutionError::DecisionBarReuse)
    );
}

#[test]
fn paper_market_matching_applies_bounded_slippage_and_taker_fee() {
    let fixture = OpenFixture::new("25");
    let market_plan = plan_with(fixture.context.context_id(), |value| {
        value["order"]["type"] = json!("MARKET");
        value["order"]["time_in_force"] = json!("IOC");
        value["order"]
            .as_object_mut()
            .expect("order must be an object")
            .remove("limit_price");
    });
    let decision = fixture.validate(&market_plan, VALIDATED_AT);
    assert_eq!(decision.outcome(), ExecutionValidationOutcome::Accept);
    let request = SpotExecutionRequest::from_accepted_plan(
        &fixture.context,
        &decision,
        &market_plan,
        execution_ids(),
        VALIDATED_AT + 1,
    )
    .expect("market request must be valid");
    let order = PaperOpenOrder::new(
        request.orders()[0].clone(),
        instrument(),
        request.context_as_of_unix_millis(),
        request.created_at_unix_millis(),
        decimal("0.10"),
    )
    .expect("paper order must be valid");
    let observation = PaperMarketObservation::new(
        stable_id::<SnapshotId>(202),
        instrument(),
        AS_OF + 1,
        VALIDATED_AT + 2,
        decimal("209.90"),
        decimal("210.10"),
        decimal("205"),
        decimal("215"),
        decimal("1"),
    )
    .expect("observation must be valid");
    let policy = PaperExecutionPolicy::new(decimal("0.001"), decimal("0.001"), decimal("0.001"))
        .expect("paper policy must be valid");
    let PaperOrderEvaluation::Fill(fill) =
        PaperMatchingEngine::evaluate(&order, &observation, decimal("1"), policy)
            .expect("matching must succeed")
    else {
        panic!("market order should fill");
    };
    assert_eq!(fill.execution_price(), decimal("210.31010"));
    assert_eq!(fill.quote_quantity(), decimal("21.0310100"));
    assert_eq!(fill.fee_quote(), decimal("0.0210310100"));
}
