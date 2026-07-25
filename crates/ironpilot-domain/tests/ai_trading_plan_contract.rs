use ironpilot_domain::{
    AI_TRADING_PLAN_SCHEMA_VERSION_V3, AiOrderType, AiTimeInForce, AiTradingAction, AiTradingPlan,
};
use serde_json::{Value, json};

fn open_long_value() -> Value {
    json!({
        "schema_version": "3.0",
        "plan_id": "00000000-0000-0000-0000-000000000001",
        "context_id": "00000000-0000-0000-0000-000000000002",
        "instrument_id": "bybit:spot:BTCUSDT",
        "action": "OPEN_LONG",
        "valid_until": 1_785_000_000_000_u64,
        "order": {
            "type": "LIMIT",
            "quantity": "0.0025",
            "limit_price": "64250.00",
            "time_in_force": "GTC",
            "expires_at": 1_785_000_000_000_u64,
            "max_slippage_quote": "1.50"
        },
        "protective_stop": {
            "trigger_price": "63120.00",
            "order_type": "MARKET"
        },
        "take_profits": [
            {
                "price": "65980.00",
                "quantity": "0.0010"
            },
            {
                "price": "67100.00",
                "quantity": "0.0015"
            }
        ],
        "declared_max_loss_quote": "4.33",
        "review": {
            "next_review_at": 1_784_999_100_000_u64,
            "max_holding_until": 1_785_081_600_000_u64
        },
        "confidence": "0.72",
        "thesis": "15m and 1h facts support an AI-selected long entry.",
        "invalidation": "Exit when the AI-observed market structure no longer supports the thesis.",
        "risks": [
            "The breakout may fail after volatility expands."
        ]
    })
}

fn without_execution_fields(value: &mut Value) {
    let object = value.as_object_mut().expect("fixture is an object");
    for field in [
        "order",
        "protective_stop",
        "take_profits",
        "declared_max_loss_quote",
    ] {
        object.remove(field);
    }
}

fn target_trade_plan_id() -> Value {
    json!("00000000-0000-0000-0000-000000000003")
}

#[test]
fn complete_open_long_roundtrips_with_exact_ai_parameters_and_stable_hash() {
    let raw = open_long_value().to_string();
    let first = AiTradingPlan::from_json(&raw).expect("complete AI plan must parse");
    let second = AiTradingPlan::from_json(&first.to_json()).expect("roundtrip must parse");

    assert_eq!(first, second);
    assert_eq!(first.schema_version(), AI_TRADING_PLAN_SCHEMA_VERSION_V3);
    assert_eq!(first.action(), AiTradingAction::OpenLong);
    assert_eq!(
        first
            .order()
            .expect("OPEN_LONG has an order")
            .quantity()
            .to_string(),
        "0.0025"
    );
    assert_eq!(
        first
            .order()
            .expect("OPEN_LONG has an order")
            .limit_price()
            .expect("LIMIT has a price")
            .to_string(),
        "64250.00"
    );
    assert_eq!(
        first
            .protective_stop()
            .expect("OPEN_LONG has a stop")
            .trigger_price()
            .to_string(),
        "63120.00"
    );
    assert_eq!(first.take_profits().len(), 2);
    assert_eq!(
        first
            .declared_max_loss_quote()
            .expect("OPEN_LONG declares loss")
            .to_string(),
        "4.33"
    );
    assert_eq!(first.plan_hash(), second.plan_hash());
}

#[test]
fn every_v3_spot_action_has_an_explicit_non_materialized_shape() {
    let mut fixtures = Vec::new();

    let mut no_trade = open_long_value();
    no_trade["action"] = json!("NO_TRADE");
    without_execution_fields(&mut no_trade);
    no_trade
        .as_object_mut()
        .expect("fixture is an object")
        .remove("review");
    fixtures.push((no_trade, AiTradingAction::NoTrade));

    let mut hold = open_long_value();
    hold["action"] = json!("HOLD");
    hold["target_trade_plan_id"] = target_trade_plan_id();
    without_execution_fields(&mut hold);
    fixtures.push((hold, AiTradingAction::Hold));

    let mut cancel = open_long_value();
    cancel["action"] = json!("CANCEL_ENTRY");
    cancel["target_trade_plan_id"] = target_trade_plan_id();
    without_execution_fields(&mut cancel);
    cancel
        .as_object_mut()
        .expect("fixture is an object")
        .remove("review");
    fixtures.push((cancel, AiTradingAction::CancelEntry));

    let mut modify = open_long_value();
    modify["action"] = json!("MODIFY_PROTECTION");
    modify["target_trade_plan_id"] = target_trade_plan_id();
    modify
        .as_object_mut()
        .expect("fixture is an object")
        .remove("order");
    fixtures.push((modify, AiTradingAction::ModifyProtection));

    for action in ["REDUCE", "EXIT"] {
        let mut value = open_long_value();
        value["action"] = json!(action);
        value["target_trade_plan_id"] = target_trade_plan_id();
        let object = value.as_object_mut().expect("fixture is an object");
        object.remove("protective_stop");
        object.remove("take_profits");
        object.remove("declared_max_loss_quote");
        value["order"] = json!({
            "type": "MARKET",
            "quantity": "0.0010",
            "time_in_force": "IOC",
            "expires_at": 1_785_000_000_000_u64,
            "max_slippage_quote": "2.00"
        });
        fixtures.push((
            value,
            if action == "REDUCE" {
                AiTradingAction::Reduce
            } else {
                AiTradingAction::Exit
            },
        ));
    }

    for (value, expected) in fixtures {
        let plan =
            AiTradingPlan::from_json(&value.to_string()).expect("action shape must be accepted");
        assert_eq!(plan.action(), expected);
    }
}

#[test]
fn unknown_fields_float_decimals_and_invalid_units_fail_closed() {
    let mut unknown_root = open_long_value();
    unknown_root["risk_tier"] = json!("normal");
    assert!(AiTradingPlan::from_json(&unknown_root.to_string()).is_err());

    let mut unknown_nested = open_long_value();
    unknown_nested["order"]["entry_anchor"] = json!("donchian_upper");
    assert!(AiTradingPlan::from_json(&unknown_nested.to_string()).is_err());

    let mut float_quantity = open_long_value();
    float_quantity["order"]["quantity"] = json!(0.0025);
    assert!(AiTradingPlan::from_json(&float_quantity.to_string()).is_err());

    let mut unit_bearing_quantity = open_long_value();
    unit_bearing_quantity["order"]["quantity"] = json!("0.0025 BTC");
    assert!(AiTradingPlan::from_json(&unit_bearing_quantity.to_string()).is_err());
}

#[test]
fn spot_scope_versions_and_action_fields_are_strict() {
    let mut perpetual = open_long_value();
    perpetual["instrument_id"] = json!("bybit:linear_perpetual:BTCUSDT");
    assert!(AiTradingPlan::from_json(&perpetual.to_string()).is_err());

    let mut short = open_long_value();
    short["action"] = json!("OPEN_SHORT");
    assert!(AiTradingPlan::from_json(&short.to_string()).is_err());

    let mut old_version = open_long_value();
    old_version["schema_version"] = json!("2.0");
    assert!(AiTradingPlan::from_json(&old_version.to_string()).is_err());

    let mut missing_stop = open_long_value();
    missing_stop
        .as_object_mut()
        .expect("fixture is an object")
        .remove("protective_stop");
    assert!(AiTradingPlan::from_json(&missing_stop.to_string()).is_err());

    let mut wrong_take_profit_total = open_long_value();
    wrong_take_profit_total["take_profits"][0]["quantity"] = json!("0.0009");
    assert!(AiTradingPlan::from_json(&wrong_take_profit_total.to_string()).is_err());
}

#[test]
fn order_and_protection_types_do_not_allow_local_price_inference() {
    let plan =
        AiTradingPlan::from_json(&open_long_value().to_string()).expect("fixture must parse");
    let order = plan.order().expect("OPEN_LONG has an order");
    assert_eq!(order.order_type(), AiOrderType::Limit);
    assert_eq!(order.time_in_force(), AiTimeInForce::Gtc);

    let mut limit_without_price = open_long_value();
    limit_without_price["order"]
        .as_object_mut()
        .expect("order is an object")
        .remove("limit_price");
    assert!(AiTradingPlan::from_json(&limit_without_price.to_string()).is_err());

    let mut market_with_price = open_long_value();
    market_with_price["order"]["type"] = json!("MARKET");
    assert!(AiTradingPlan::from_json(&market_with_price.to_string()).is_err());
}

#[test]
fn v2_strategy_intent_cannot_parse_as_an_ai_trading_plan() {
    let legacy = json!({
        "schema_version": "2.0",
        "strategy_space_version": "strategy-space-v1-vs",
        "decision_id": "00000000-0000-0000-0000-000000000001",
        "snapshot_id": "00000000-0000-0000-0000-000000000002",
        "instrument_id": "bybit:spot:BTCUSDT",
        "decision": {
            "action": "OPEN_LONG",
            "risk_tier": "normal"
        }
    });

    assert!(AiTradingPlan::from_json(&legacy.to_string()).is_err());
}

#[test]
fn active_domain_surface_contains_no_v2_strategy_or_risk_authority() {
    let active_surface = include_str!("../src/lib.rs");
    let active_state = include_str!("../src/state.rs");
    for forbidden in [
        "mod strategy;",
        "mod risk;",
        "MaterializedRiskInput",
        "RiskDecision",
        "StrategyIntent",
        "RiskTier",
        "EntryAnchor",
    ] {
        assert!(
            !active_surface.contains(forbidden),
            "{forbidden} must not be in the active domain surface"
        );
    }
    for forbidden_state in ["Materialized", "RiskApproved"] {
        assert!(
            !active_state.contains(forbidden_state),
            "{forbidden_state} must not remain in the v3 TradePlan state machine"
        );
    }
}
