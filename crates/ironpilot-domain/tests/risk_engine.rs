use core::str::FromStr;

use ironpilot_domain::{
    AssetCode, DecisionId, DomainDecimal, EntryAnchor, EntryConfirmation, EntryPolicy,
    ExchangeAssetBalance, InstrumentId, InvalidationCondition, LocalAssetBalance,
    MaterializationHash, MaterializedRiskInput, OpenPositionDecision, PortfolioReconciler,
    RISK_RULES_VERSION_V1, ReviewPolicy, RiskContext, RiskDecisionId, RiskEngine, RiskInputError,
    RiskOutcome, RiskReason, RiskTier, STRATEGY_SPACE_VERSION_V1_VS, SchemaVersion, SnapshotId,
    StopAnchor, StopPolicy, StrategyAction, StrategyDecision, StrategyIntent, StrategySpaceVersion,
    SymbolRiskState, SystemState, TargetPolicy,
};
use proptest::prelude::*;
use uuid::Uuid;

fn decimal(value: &str) -> DomainDecimal {
    DomainDecimal::from_str(value).expect("test decimal must be valid")
}

fn asset(value: &str) -> AssetCode {
    AssetCode::new(value).expect("test asset must be valid")
}

fn instrument() -> InstrumentId {
    InstrumentId::from_str("bybit:spot:BTCUSDT").expect("test instrument must be valid")
}

fn decision_id(value: u128) -> DecisionId {
    DecisionId::new(Uuid::from_u128(value)).expect("test ID must be valid")
}

fn risk_decision_id(value: u128) -> RiskDecisionId {
    RiskDecisionId::new(Uuid::from_u128(value)).expect("test ID must be valid")
}

fn snapshot_id(value: u128) -> SnapshotId {
    SnapshotId::new(Uuid::from_u128(value)).expect("test ID must be valid")
}

fn validated_open_long(id: u128) -> ironpilot_domain::ValidatedStrategyIntent {
    StrategyIntent::new(
        SchemaVersion::v2(),
        StrategySpaceVersion::vertical_slice_v1(),
        decision_id(id),
        snapshot_id(id + 1_000),
        instrument(),
        StrategyDecision::OpenLong(OpenPositionDecision::new(
            EntryPolicy::new(
                EntryAnchor::DonchianUpper,
                2,
                EntryConfirmation::CloseConfirmed,
            ),
            StopPolicy::new(StopAnchor::RecentSwing),
            TargetPolicy::fixed_rr(),
            RiskTier::Conservative,
            12,
            ReviewPolicy::EveryPrimaryClose,
            vec![InvalidationCondition::BreakoutFailed],
        )),
    )
    .validate_for_vertical_slice()
    .expect("fixture intent must be valid")
}

fn materialized(id: u128, requested: &str, maximum: &str) -> MaterializedRiskInput {
    MaterializedRiskInput::new(
        validated_open_long(id),
        "materializer-v1-test",
        MaterializationHash::new([7; 32]),
        decimal(requested),
        decimal(maximum),
    )
    .expect("fixture materialization must be valid")
}

fn balanced_portfolio() -> ironpilot_domain::PortfolioSnapshot {
    PortfolioReconciler::reconcile(
        vec![
            ExchangeAssetBalance::new(asset("BTC"), decimal("1"), DomainDecimal::ZERO)
                .expect("valid"),
            ExchangeAssetBalance::new(asset("USDT"), decimal("1000"), DomainDecimal::ZERO)
                .expect("valid"),
        ],
        vec![
            LocalAssetBalance::new(asset("BTC"), decimal("1"), decimal("0.5")).expect("valid"),
            LocalAssetBalance::new(asset("USDT"), decimal("1000"), DomainDecimal::ZERO)
                .expect("valid"),
        ],
        1_000,
    )
    .expect("fixture portfolio must reconcile")
}

fn unbalanced_portfolio() -> ironpilot_domain::PortfolioSnapshot {
    PortfolioReconciler::reconcile(
        vec![
            ExchangeAssetBalance::new(asset("BTC"), decimal("1.1"), DomainDecimal::ZERO)
                .expect("valid"),
        ],
        vec![LocalAssetBalance::new(asset("BTC"), decimal("1"), decimal("0.5")).expect("valid")],
        1_000,
    )
    .expect("fixture portfolio must reconcile with a visible difference")
}

fn context(
    portfolio: &ironpilot_domain::PortfolioSnapshot,
    system_state: SystemState,
    symbol_state: SymbolRiskState,
    active_trade_plans: u8,
) -> RiskContext<'_> {
    RiskContext::new(system_state, symbol_state, active_trade_plans, 2, portfolio)
        .expect("fixture context must be valid")
}

#[test]
fn approved_input_retains_strategy_and_materialization_provenance() {
    let portfolio = balanced_portfolio();
    let decision = RiskEngine::evaluate(
        risk_decision_id(10),
        materialized(20, "0.5", "0.8"),
        context(
            &portfolio,
            SystemState::EntryEnabled,
            SymbolRiskState::EntryEnabled,
            0,
        ),
        2_000,
    );

    assert_eq!(decision.rules_version(), RISK_RULES_VERSION_V1);
    assert_eq!(
        decision.strategy_space_version(),
        STRATEGY_SPACE_VERSION_V1_VS
    );
    assert_eq!(decision.outcome(), RiskOutcome::Approve);
    assert_eq!(decision.reason(), RiskReason::WithinAllLimits);
    assert_eq!(decision.decision_id(), decision_id(20));
    assert_eq!(decision.snapshot_id(), snapshot_id(1_020));
    assert_eq!(decision.instrument_id(), &instrument());
    assert_eq!(decision.action(), StrategyAction::OpenLong);
    assert_eq!(decision.maximum_allowed_quantity(), decimal("0.8"));
    assert_eq!(decision.system_state(), SystemState::EntryEnabled);
    assert_eq!(decision.symbol_state(), SymbolRiskState::EntryEnabled);
    assert_eq!(decision.active_trade_plans(), 0);
    assert_eq!(decision.max_active_trade_plans(), 2);
    assert_eq!(decision.approved_quantity(), Some(decimal("0.5")));
    let authorization = decision
        .authorization()
        .expect("approval must create execution authorization");
    assert_eq!(authorization.decision_id(), decision_id(20));
    assert_eq!(authorization.snapshot_id(), snapshot_id(1_020));
    assert_eq!(authorization.instrument_id(), &instrument());
    assert_eq!(authorization.action(), StrategyAction::OpenLong);
    assert_eq!(
        authorization.materialization_hash(),
        MaterializationHash::new([7; 32])
    );
    assert_eq!(authorization.approved_quantity(), decimal("0.5"));
}

#[test]
fn adjustment_can_only_reduce_the_materialized_quantity() {
    let portfolio = balanced_portfolio();
    let decision = RiskEngine::evaluate(
        risk_decision_id(11),
        materialized(21, "1.25", "0.75"),
        context(
            &portfolio,
            SystemState::EntryEnabled,
            SymbolRiskState::EntryEnabled,
            0,
        ),
        2_000,
    );

    assert_eq!(decision.outcome(), RiskOutcome::AdjustDown);
    assert_eq!(
        decision.reason(),
        RiskReason::QuantityAdjustedToMaterializedMaximum
    );
    assert_eq!(decision.approved_quantity(), Some(decimal("0.75")));
    assert!(
        decision
            .authorization()
            .expect("adjustment must authorize only the tightened candidate")
            .approved_quantity()
            < decision.requested_quantity()
    );
}

#[test]
fn zero_allowance_and_portfolio_difference_reject_without_authorization() {
    let balanced = balanced_portfolio();
    let zero_allowance = RiskEngine::evaluate(
        risk_decision_id(12),
        materialized(22, "0.5", "0"),
        context(
            &balanced,
            SystemState::EntryEnabled,
            SymbolRiskState::EntryEnabled,
            0,
        ),
        2_000,
    );
    assert_eq!(zero_allowance.outcome(), RiskOutcome::Reject);
    assert_eq!(zero_allowance.reason(), RiskReason::ZeroRiskAllowance);
    assert!(zero_allowance.authorization().is_none());

    let unbalanced = unbalanced_portfolio();
    let difference = RiskEngine::evaluate(
        risk_decision_id(13),
        materialized(23, "0.5", "0.8"),
        context(
            &unbalanced,
            SystemState::EntryEnabled,
            SymbolRiskState::EntryEnabled,
            0,
        ),
        2_000,
    );
    assert_eq!(difference.outcome(), RiskOutcome::Reject);
    assert_eq!(difference.reason(), RiskReason::PortfolioNotReconciled);
    assert!(difference.authorization().is_none());
}

#[test]
fn active_trade_plan_limit_rejects_and_invariant_breach_halts_system() {
    let portfolio = balanced_portfolio();
    let at_limit = RiskEngine::evaluate(
        risk_decision_id(14),
        materialized(24, "0.5", "0.8"),
        context(
            &portfolio,
            SystemState::EntryEnabled,
            SymbolRiskState::EntryEnabled,
            2,
        ),
        2_000,
    );
    assert_eq!(at_limit.outcome(), RiskOutcome::Reject);
    assert_eq!(at_limit.reason(), RiskReason::ActiveTradePlanLimitReached);
    assert!(at_limit.authorization().is_none());

    let breached = RiskEngine::evaluate(
        risk_decision_id(15),
        materialized(25, "0.5", "0.8"),
        context(
            &portfolio,
            SystemState::EntryEnabled,
            SymbolRiskState::EntryEnabled,
            3,
        ),
        2_000,
    );
    assert_eq!(breached.outcome(), RiskOutcome::HaltSystem);
    assert_eq!(
        breached.reason(),
        RiskReason::ActiveTradePlanInvariantBreached
    );
    assert!(breached.authorization().is_none());
}

#[test]
fn system_and_symbol_degradation_never_construct_execution_authorization() {
    let portfolio = balanced_portfolio();
    let cases = [
        (
            SystemState::Observing,
            SymbolRiskState::EntryEnabled,
            RiskOutcome::ReduceOnly,
            RiskReason::SystemNotEntryEnabled,
        ),
        (
            SystemState::EntryEnabled,
            SymbolRiskState::ReduceOnly,
            RiskOutcome::ReduceOnly,
            RiskReason::SymbolReduceOnly,
        ),
        (
            SystemState::EntryEnabled,
            SymbolRiskState::Halted,
            RiskOutcome::HaltSymbol,
            RiskReason::SymbolHalted,
        ),
        (
            SystemState::Halted,
            SymbolRiskState::EntryEnabled,
            RiskOutcome::HaltSystem,
            RiskReason::SystemHalted,
        ),
    ];

    for (index, (system, symbol, expected_outcome, expected_reason)) in
        cases.into_iter().enumerate()
    {
        let decision = RiskEngine::evaluate(
            risk_decision_id(100 + index as u128),
            materialized(200 + index as u128, "0.5", "0.8"),
            context(&portfolio, system, symbol, 0),
            2_000,
        );
        assert_eq!(decision.outcome(), expected_outcome);
        assert_eq!(decision.reason(), expected_reason);
        assert!(!decision.outcome().permits_execution());
        assert!(decision.authorization().is_none());
    }
}

#[test]
fn risk_input_requires_a_validated_vertical_slice_open_long() {
    let no_trade = StrategyIntent::new(
        SchemaVersion::v2(),
        StrategySpaceVersion::vertical_slice_v1(),
        decision_id(30),
        snapshot_id(31),
        instrument(),
        StrategyDecision::NoTrade,
    )
    .validate_for_vertical_slice()
    .expect("NO_TRADE is valid but not materializable for entry risk");
    assert_eq!(
        MaterializedRiskInput::new(
            no_trade,
            "materializer-v1-test",
            MaterializationHash::new([1; 32]),
            decimal("1"),
            decimal("1"),
        ),
        Err(RiskInputError::ActionNotMaterializedForEntryRisk)
    );

    let unsupported_json = format!(
        r#"{{
            "schema_version":"2.0",
            "strategy_space_version":"strategy-space-v1",
            "decision_id":"{}",
            "snapshot_id":"{}",
            "instrument_id":"bybit:spot:BTCUSDT",
            "decision":{{"action":"NO_TRADE"}}
        }}"#,
        decision_id(32),
        snapshot_id(33)
    );
    let unsupported: StrategyIntent =
        serde_json::from_str(&unsupported_json).expect("schema-shaped fixture must deserialize");
    assert_eq!(
        unsupported.validate_for_vertical_slice(),
        Err(ironpilot_domain::StrategyValidationError::UnsupportedStrategySpaceVersion)
    );
    assert_eq!(
        STRATEGY_SPACE_VERSION_V1_VS, "strategy-space-v1-vs",
        "the executable provenance version is frozen"
    );
}

#[test]
fn decision_hash_is_deterministic_and_binds_inputs_and_outcome() {
    let portfolio = balanced_portfolio();
    let first = RiskEngine::evaluate(
        risk_decision_id(40),
        materialized(41, "0.5", "0.8"),
        context(
            &portfolio,
            SystemState::EntryEnabled,
            SymbolRiskState::EntryEnabled,
            0,
        ),
        2_000,
    );
    let second = RiskEngine::evaluate(
        risk_decision_id(40),
        materialized(41, "0.5", "0.8"),
        context(
            &portfolio,
            SystemState::EntryEnabled,
            SymbolRiskState::EntryEnabled,
            0,
        ),
        2_000,
    );
    let changed = RiskEngine::evaluate(
        risk_decision_id(40),
        materialized(41, "0.9", "0.8"),
        context(
            &portfolio,
            SystemState::EntryEnabled,
            SymbolRiskState::EntryEnabled,
            0,
        ),
        2_000,
    );

    assert_eq!(first.decision_hash(), second.decision_hash());
    assert_ne!(first.decision_hash(), changed.decision_hash());
    assert_eq!(first.outcome(), RiskOutcome::Approve);
    assert_eq!(changed.outcome(), RiskOutcome::AdjustDown);
}

#[test]
fn malformed_materialization_and_limits_fail_closed_before_evaluation() {
    assert_eq!(
        MaterializedRiskInput::new(
            validated_open_long(50),
            "",
            MaterializationHash::new([1; 32]),
            decimal("1"),
            decimal("1"),
        ),
        Err(RiskInputError::InvalidMaterializationVersion)
    );
    assert_eq!(
        MaterializedRiskInput::new(
            validated_open_long(51),
            "materializer-v1-test",
            MaterializationHash::new([1; 32]),
            DomainDecimal::ZERO,
            decimal("1"),
        ),
        Err(RiskInputError::NonPositiveRequestedQuantity)
    );

    let portfolio = balanced_portfolio();
    assert_eq!(
        RiskContext::new(
            SystemState::EntryEnabled,
            SymbolRiskState::EntryEnabled,
            0,
            0,
            &portfolio,
        ),
        Err(RiskInputError::InvalidActiveTradePlanLimit)
    );
    assert_eq!(
        RiskContext::new(
            SystemState::EntryEnabled,
            SymbolRiskState::EntryEnabled,
            0,
            3,
            &portfolio,
        ),
        Err(RiskInputError::InvalidActiveTradePlanLimit)
    );
}

#[test]
fn outcome_contract_is_closed_to_the_six_planned_results() {
    assert_eq!(
        RiskOutcome::ALL,
        [
            RiskOutcome::Approve,
            RiskOutcome::AdjustDown,
            RiskOutcome::Reject,
            RiskOutcome::ReduceOnly,
            RiskOutcome::HaltSymbol,
            RiskOutcome::HaltSystem,
        ]
    );
    assert_eq!(
        RiskOutcome::ALL
            .into_iter()
            .map(|outcome| serde_json::to_string(&outcome).expect("outcome must serialize"))
            .collect::<Vec<_>>(),
        vec![
            "\"APPROVE\"",
            "\"ADJUST_DOWN\"",
            "\"REJECT\"",
            "\"REDUCE_ONLY\"",
            "\"HALT_SYMBOL\"",
            "\"HALT_SYSTEM\"",
        ]
    );
}

proptest! {
    #[test]
    fn authorization_never_increases_quantity(
        requested_microunits in 1_i64..1_000_000,
        maximum_microunits in 0_i64..1_000_000,
    ) {
        let requested = DomainDecimal::from_mantissa_scale(i128::from(requested_microunits), 6)
            .expect("generated quantity must fit");
        let maximum = DomainDecimal::from_mantissa_scale(i128::from(maximum_microunits), 6)
            .expect("generated quantity must fit");
        let portfolio = balanced_portfolio();
        let input = MaterializedRiskInput::new(
            validated_open_long(60),
            "materializer-v1-test",
            MaterializationHash::new([9; 32]),
            requested,
            maximum,
        )
        .expect("generated materialization must be valid");
        let decision = RiskEngine::evaluate(
            risk_decision_id(61),
            input,
            context(
                &portfolio,
                SystemState::EntryEnabled,
                SymbolRiskState::EntryEnabled,
                0,
            ),
            2_000,
        );

        if let Some(authorization) = decision.authorization() {
            prop_assert!(decision.outcome().permits_execution());
            prop_assert!(authorization.approved_quantity() <= requested);
            prop_assert!(authorization.approved_quantity() <= maximum);
        } else {
            prop_assert!(!decision.outcome().permits_execution());
        }
    }
}
