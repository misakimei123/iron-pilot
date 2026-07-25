// Historical P1-02 evidence. This file is intentionally outside Cargo's tests directory.
use ironpilot_domain::{
    StrategyAction, StrategyIntent, StrategyValidationError, ValidatedStrategyIntent,
};
use serde_json::{Value, json};

fn valid_open_long_json() -> Value {
    json!({
        "schema_version": "2.0",
        "strategy_space_version": "strategy-space-v1-vs",
        "decision_id": "018f0f3e-7b4d-7cc0-a6c8-7f8519262a1f",
        "snapshot_id": "018f0f3e-8a9c-7e74-8d84-7011a69cd85f",
        "instrument_id": "bybit:spot:BTCUSDT",
        "decision": {
            "action": "OPEN_LONG",
            "strategy_family": "trend_breakout",
            "entry_policy": {
                "type": "breakout_retest",
                "anchor": "donchian_upper",
                "max_wait_bars": 2,
                "confirmation": "close_confirmed"
            },
            "stop_policy": {
                "type": "structure_with_atr_buffer",
                "anchor": "recent_swing",
                "buffer_tier": "normal"
            },
            "target_policy": {
                "type": "fixed_rr_tier",
                "minimum_rr_tier": "2R",
                "trailing_anchor": "none"
            },
            "risk_tier": "conservative",
            "maximum_holding_bars": 12,
            "review_policy": "every_primary_close",
            "invalidation_conditions": ["breakout_failed"]
        }
    })
}

fn deserialize_and_validate(value: &Value) -> Result<ValidatedStrategyIntent, String> {
    let intent: StrategyIntent =
        serde_json::from_value(value.clone()).map_err(|error| error.to_string())?;
    intent
        .validate_for_vertical_slice()
        .map_err(|error| error.to_string())
}

#[test]
fn vertical_slice_open_long_contract_is_executable() {
    let validated =
        deserialize_and_validate(&valid_open_long_json()).expect("valid contract must pass");

    assert_eq!(validated.as_intent().action(), StrategyAction::OpenLong);
}

#[test]
fn non_vertical_slice_strategy_space_is_not_executable() {
    let mut value = valid_open_long_json();
    value["strategy_space_version"] = json!("strategy-space-v2");
    let intent: StrategyIntent =
        serde_json::from_value(value).expect("raw future version remains auditable");

    assert_eq!(
        intent.validate_for_vertical_slice(),
        Err(StrategyValidationError::UnsupportedStrategySpaceVersion)
    );
}

#[test]
fn legacy_schema_is_not_executable() {
    let mut value = valid_open_long_json();
    value["schema_version"] = json!("1.0");
    let intent: StrategyIntent =
        serde_json::from_value(value).expect("raw legacy version remains auditable");

    assert_eq!(
        intent.validate_for_vertical_slice(),
        Err(StrategyValidationError::UnsupportedSchemaVersion)
    );
}

#[test]
fn spot_open_short_is_rejected() {
    let mut value = valid_open_long_json();
    value["decision"]["action"] = json!("OPEN_SHORT");
    let intent: StrategyIntent =
        serde_json::from_value(value).expect("OPEN_SHORT is a known protocol action");

    assert_eq!(
        intent.validate_for_vertical_slice(),
        Err(StrategyValidationError::OpenShortForbiddenForSpot)
    );
}

#[test]
fn future_strategy_family_and_unknown_action_are_rejected_by_serde() {
    let mut future_family = valid_open_long_json();
    future_family["decision"]["strategy_family"] = json!("trend_pullback");
    assert!(serde_json::from_value::<StrategyIntent>(future_family).is_err());

    let mut unknown_action = valid_open_long_json();
    unknown_action["decision"]["action"] = json!("OPEN_SIDEWAYS");
    assert!(serde_json::from_value::<StrategyIntent>(unknown_action).is_err());
}

#[test]
fn free_price_quantity_leverage_and_execution_authority_are_rejected() {
    for forbidden in [
        ("absolute_price", json!("65000.0")),
        ("quantity", json!("0.5")),
        ("leverage", json!(2)),
        ("execution_authority", json!(true)),
        ("order_id", json!("external-order")),
        ("risk_limit", json!("100.0")),
    ] {
        let mut value = valid_open_long_json();
        value["decision"][forbidden.0] = forbidden.1;

        assert!(
            serde_json::from_value::<StrategyIntent>(value).is_err(),
            "{} must not be accepted",
            forbidden.0
        );
    }
}

#[test]
fn invalid_wait_hold_and_invalidation_bounds_fail_closed() {
    let mut zero_wait = valid_open_long_json();
    zero_wait["decision"]["entry_policy"]["max_wait_bars"] = json!(0);
    assert!(deserialize_and_validate(&zero_wait).is_err());

    let mut excessive_hold = valid_open_long_json();
    excessive_hold["decision"]["maximum_holding_bars"] = json!(97);
    assert!(deserialize_and_validate(&excessive_hold).is_err());

    let mut missing_invalidation = valid_open_long_json();
    missing_invalidation["decision"]["invalidation_conditions"] = json!([]);
    assert!(deserialize_and_validate(&missing_invalidation).is_err());
}

#[test]
fn no_trade_hold_and_exit_are_in_the_minimum_strategy_space() {
    for decision in [
        json!({ "action": "NO_TRADE" }),
        json!({ "action": "HOLD", "review_policy": "every_primary_close" }),
        json!({ "action": "EXIT", "review_policy": "on_invalidation_risk" }),
    ] {
        let mut value = valid_open_long_json();
        value["decision"] = decision;

        assert!(deserialize_and_validate(&value).is_ok());
    }
}
