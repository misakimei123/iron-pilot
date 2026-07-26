use core::str::FromStr;

use ironpilot_application::{
    FullHistoricalEvaluationError, FullHistoricalStrategyEvaluator, HistoricalDecisionOutcome,
    HistoricalEvaluationArm, HistoricalEvaluationManifest, HistoricalEvaluationRecord,
    HistoricalIndependentReference, HistoricalReferenceArmMetrics, HistoricalStressScenario,
};
use ironpilot_domain::DomainDecimal;
use serde_json::Value;

fn decimal(value: &str) -> DomainDecimal {
    DomainDecimal::from_str(value).expect("test decimal must be exact")
}

fn hash(character: char) -> Box<str> {
    character.to_string().repeat(64).into_boxed_str()
}

fn manifest() -> HistoricalEvaluationManifest {
    let evidence = records();
    HistoricalEvaluationManifest::new(
        FullHistoricalStrategyEvaluator::dataset_hash(&evidence).expect("dataset binding"),
        "ironpilot-deepseek-trading-prompt-v2",
        "deepseek-v4-recorded",
        FullHistoricalStrategyEvaluator::deterministic_stub_plan_set_hash(&evidence)
            .expect("stub plan binding"),
        FullHistoricalStrategyEvaluator::recorded_plan_set_hash(&evidence)
            .expect("recorded plan binding"),
        50,
        300,
        200,
        decimal("1000"),
        decimal("25"),
        decimal("0.001"),
        decimal("0.002"),
        vec![
            HistoricalStressScenario::new("DOUBLE_COST", decimal("2"), decimal("2"))
                .expect("stress scenario"),
        ],
    )
    .expect("manifest must be valid")
}

fn record(
    comparison_id: &str,
    arm: HistoricalEvaluationArm,
    decision_at: u64,
    outcome: HistoricalDecisionOutcome,
    gross_pnl: &str,
    fees: &str,
    slippage: &str,
) -> HistoricalEvaluationRecord {
    let rejection_reasons = if outcome == HistoricalDecisionOutcome::Rejected {
        vec![Box::from("MAXIMUM_LOSS_EXCEEDED")]
    } else {
        Vec::new()
    };
    HistoricalEvaluationRecord::new(
        comparison_id,
        arm,
        if comparison_id == "trade-1" {
            hash('a')
        } else {
            hash('b')
        },
        match arm {
            HistoricalEvaluationArm::RuleOnlyBaseline => hash('4'),
            HistoricalEvaluationArm::DeterministicAiPlanStub => hash('5'),
            HistoricalEvaluationArm::RecordedAiTradingPlan => hash('6'),
        },
        decision_at - 10,
        decision_at,
        decision_at + 10,
        outcome,
        decimal(gross_pnl),
        decimal(fees),
        decimal(slippage),
        if arm == HistoricalEvaluationArm::RuleOnlyBaseline {
            None
        } else {
            Some(if comparison_id == "trade-1" {
                hash('c')
            } else {
                hash('d')
            })
        },
        0,
        rejection_reasons,
        Vec::new(),
    )
}

fn records() -> Vec<HistoricalEvaluationRecord> {
    vec![
        record(
            "trade-1",
            HistoricalEvaluationArm::RuleOnlyBaseline,
            100,
            HistoricalDecisionOutcome::Executed,
            "12",
            "1",
            "1",
        ),
        record(
            "trade-1",
            HistoricalEvaluationArm::DeterministicAiPlanStub,
            100,
            HistoricalDecisionOutcome::Executed,
            "15",
            "1",
            "1",
        ),
        record(
            "trade-1",
            HistoricalEvaluationArm::RecordedAiTradingPlan,
            100,
            HistoricalDecisionOutcome::Executed,
            "18",
            "1",
            "1",
        ),
        record(
            "trade-2",
            HistoricalEvaluationArm::RuleOnlyBaseline,
            200,
            HistoricalDecisionOutcome::Executed,
            "-8",
            "1",
            "1",
        ),
        record(
            "trade-2",
            HistoricalEvaluationArm::DeterministicAiPlanStub,
            200,
            HistoricalDecisionOutcome::Rejected,
            "0",
            "0",
            "0",
        ),
        record(
            "trade-2",
            HistoricalEvaluationArm::RecordedAiTradingPlan,
            200,
            HistoricalDecisionOutcome::Executed,
            "-4",
            "1",
            "1",
        ),
    ]
}

fn reference() -> HistoricalIndependentReference {
    HistoricalIndependentReference::new(
        "independent-ledger-reference-v1",
        hash('3'),
        vec![
            HistoricalReferenceArmMetrics::new(
                HistoricalEvaluationArm::RecordedAiTradingPlan,
                decimal("10"),
                decimal("6"),
                2,
                decimal("-6"),
            ),
            HistoricalReferenceArmMetrics::new(
                HistoricalEvaluationArm::RuleOnlyBaseline,
                decimal("0"),
                decimal("10"),
                2,
                decimal("-10"),
            ),
            HistoricalReferenceArmMetrics::new(
                HistoricalEvaluationArm::DeterministicAiPlanStub,
                decimal("13"),
                decimal("0"),
                1,
                decimal("0"),
            ),
        ],
    )
}

#[test]
fn full_evaluation_is_deterministic_comparable_and_independently_tied_out() {
    let first = FullHistoricalStrategyEvaluator::evaluate(manifest(), records(), reference())
        .expect("three comparable arms with an exact independent reference must evaluate");
    let mut reversed = records();
    reversed.reverse();
    let second = FullHistoricalStrategyEvaluator::evaluate(manifest(), reversed, reference())
        .expect("input order must not affect historical evaluation");

    assert_eq!(first, second);
    assert_eq!(first.report_hash(), second.report_hash());
    assert_eq!(first.recorded_vs_rule_net_pnl_delta_quote(), decimal("10"));
    assert_eq!(first.recorded_vs_stub_net_pnl_delta_quote(), decimal("-3"));
    assert_eq!(first.trade_differences().len(), 2);

    let recorded = first
        .arms()
        .iter()
        .find(|metrics| metrics.arm() == HistoricalEvaluationArm::RecordedAiTradingPlan)
        .expect("recorded AI metrics");
    assert_eq!(recorded.full_sample().net_pnl_quote(), decimal("10"));
    assert_eq!(
        recorded.full_sample().maximum_drawdown_quote(),
        decimal("6")
    );
    assert_eq!(recorded.full_sample().expectancy_quote(), decimal("5"));
    assert_eq!(recorded.full_sample().trade_count(), 2);
    assert_eq!(recorded.full_sample().total_return_percent(), decimal("1"));
    assert!(
        recorded.full_sample().maximum_drawdown_percent() > DomainDecimal::ZERO,
        "mature metrics library must calculate a non-zero drawdown"
    );
    assert_eq!(recorded.out_of_sample().net_pnl_quote(), decimal("-6"));
    assert_eq!(recorded.full_sample().total_cost_quote(), decimal("4"));

    let recorded_stress = first
        .stress_results()
        .iter()
        .find(|result| {
            result.scenario() == "DOUBLE_COST"
                && result.arm() == HistoricalEvaluationArm::RecordedAiTradingPlan
        })
        .expect("recorded AI stress result");
    assert_eq!(recorded_stress.net_pnl_quote(), decimal("6"));
    assert!(
        recorded_stress.maximum_drawdown_percent() > DomainDecimal::ZERO,
        "stress result must include library-calculated drawdown"
    );

    let payload: Value = serde_json::from_str(&first.to_json()).expect("report JSON must be valid");
    assert_eq!(payload["rule_only_production_eligible"], false);
    assert_eq!(payload["safety_invariants_passed"], true);
    assert_eq!(payload["independent_reference"]["tie_out"], true);
    assert_eq!(
        payload["manifest"]["context_schema_version"],
        "ironpilot-ai-decision-context-v1"
    );
    assert_eq!(
        payload["manifest"]["validator_version"],
        "ironpilot-execution-validator-v1"
    );
    assert_eq!(
        payload["manifest"]["execution_version"],
        "ironpilot-spot-execution-v1"
    );
    assert_eq!(
        payload["manifest"]["metrics_library_version"],
        "quant-metrics-0.7.0"
    );
}

#[test]
fn future_incomparable_mutated_unsafe_and_unreferenced_evidence_fails_closed() {
    let mut missing_arm = records();
    missing_arm.pop();
    assert_eq!(
        FullHistoricalStrategyEvaluator::evaluate(manifest(), missing_arm, reference()),
        Err(FullHistoricalEvaluationError::IncomparableArms)
    );

    let mut mismatched = records();
    mismatched[1] = HistoricalEvaluationRecord::new(
        "trade-1",
        HistoricalEvaluationArm::DeterministicAiPlanStub,
        hash('f'),
        hash('5'),
        90,
        100,
        110,
        HistoricalDecisionOutcome::Executed,
        decimal("15"),
        decimal("1"),
        decimal("1"),
        Some(hash('c')),
        0,
        Vec::new(),
        Vec::new(),
    );
    assert_eq!(
        FullHistoricalStrategyEvaluator::evaluate(manifest(), mismatched, reference()),
        Err(FullHistoricalEvaluationError::IncomparableArms)
    );

    let mut future = records();
    future[0] = HistoricalEvaluationRecord::new(
        "trade-1",
        HistoricalEvaluationArm::RuleOnlyBaseline,
        hash('a'),
        hash('4'),
        101,
        100,
        110,
        HistoricalDecisionOutcome::Executed,
        decimal("12"),
        decimal("1"),
        decimal("1"),
        None,
        0,
        Vec::new(),
        Vec::new(),
    );
    assert_eq!(
        FullHistoricalStrategyEvaluator::evaluate(manifest(), future, reference()),
        Err(FullHistoricalEvaluationError::FutureData)
    );

    let mut mutated = records();
    mutated[2] = HistoricalEvaluationRecord::new(
        "trade-1",
        HistoricalEvaluationArm::RecordedAiTradingPlan,
        hash('a'),
        hash('6'),
        90,
        100,
        110,
        HistoricalDecisionOutcome::Executed,
        decimal("18"),
        decimal("1"),
        decimal("1"),
        Some(hash('c')),
        1,
        Vec::new(),
        Vec::new(),
    );
    assert_eq!(
        FullHistoricalStrategyEvaluator::evaluate(manifest(), mutated, reference()),
        Err(FullHistoricalEvaluationError::LocalPlanMutation)
    );

    let mut unsafe_profit = records();
    unsafe_profit[2] = HistoricalEvaluationRecord::new(
        "trade-1",
        HistoricalEvaluationArm::RecordedAiTradingPlan,
        hash('a'),
        hash('6'),
        90,
        100,
        110,
        HistoricalDecisionOutcome::Executed,
        decimal("1000000"),
        decimal("1"),
        decimal("1"),
        Some(hash('c')),
        0,
        Vec::new(),
        vec![Box::from("UNAUTHORIZED_ASSET_SALE")],
    );
    assert_eq!(
        FullHistoricalStrategyEvaluator::evaluate(manifest(), unsafe_profit, reference()),
        Err(FullHistoricalEvaluationError::SafetyInvariantFailure)
    );

    let bad_reference = HistoricalIndependentReference::new(
        "independent-ledger-reference-v1",
        hash('3'),
        vec![
            HistoricalReferenceArmMetrics::new(
                HistoricalEvaluationArm::RuleOnlyBaseline,
                decimal("999"),
                decimal("10"),
                2,
                decimal("-10"),
            ),
            HistoricalReferenceArmMetrics::new(
                HistoricalEvaluationArm::DeterministicAiPlanStub,
                decimal("13"),
                decimal("0"),
                1,
                decimal("0"),
            ),
            HistoricalReferenceArmMetrics::new(
                HistoricalEvaluationArm::RecordedAiTradingPlan,
                decimal("10"),
                decimal("6"),
                2,
                decimal("-6"),
            ),
        ],
    );
    assert_eq!(
        FullHistoricalStrategyEvaluator::evaluate(manifest(), records(), bad_reference),
        Err(FullHistoricalEvaluationError::IndependentReferenceMismatch)
    );
}

#[test]
fn manifest_requires_oos_stress_and_immutable_versioned_evidence() {
    assert_eq!(
        HistoricalEvaluationManifest::new(
            hash('1'),
            "ironpilot-deepseek-trading-prompt-v2",
            "deepseek-v4-recorded",
            hash('4'),
            hash('2'),
            50,
            300,
            200,
            decimal("1000"),
            decimal("25"),
            decimal("0.001"),
            decimal("0.002"),
            Vec::new(),
        ),
        Err(FullHistoricalEvaluationError::InvalidManifest)
    );
    assert_eq!(
        HistoricalEvaluationManifest::new(
            "not-a-hash",
            "ironpilot-deepseek-trading-prompt-v2",
            "deepseek-v4-recorded",
            hash('4'),
            hash('2'),
            50,
            300,
            200,
            decimal("1000"),
            decimal("25"),
            decimal("0.001"),
            decimal("0.002"),
            vec![
                HistoricalStressScenario::new("DOUBLE_COST", decimal("2"), decimal("2"))
                    .expect("stress scenario"),
            ],
        ),
        Err(FullHistoricalEvaluationError::InvalidManifest)
    );
    assert_eq!(manifest().manifest_hash().len(), 64);

    let evidence = records();
    let wrong_binding_manifest = HistoricalEvaluationManifest::new(
        hash('1'),
        "ironpilot-deepseek-trading-prompt-v2",
        "deepseek-v4-recorded",
        FullHistoricalStrategyEvaluator::deterministic_stub_plan_set_hash(&evidence)
            .expect("stub plan binding"),
        FullHistoricalStrategyEvaluator::recorded_plan_set_hash(&evidence)
            .expect("recorded plan binding"),
        50,
        300,
        200,
        decimal("1000"),
        decimal("25"),
        decimal("0.001"),
        decimal("0.002"),
        vec![
            HistoricalStressScenario::new("DOUBLE_COST", decimal("2"), decimal("2"))
                .expect("stress scenario"),
        ],
    )
    .expect("a syntactically valid but incorrect binding can be constructed for verification");
    assert_eq!(
        FullHistoricalStrategyEvaluator::evaluate(wrong_binding_manifest, evidence, reference()),
        Err(FullHistoricalEvaluationError::EvidenceBindingMismatch)
    );
}
