use ironpilot_application::{
    MAX_PAPER_SOAK_OBSERVATIONS, PAPER_SOAK_MAX_OBSERVATION_GAP_MILLIS,
    PAPER_SOAK_REQUIRED_DURATION_MILLIS, PaperSoakEvaluator, PaperSoakFaultEvidence,
    PaperSoakFaultKind, PaperSoakLimits, PaperSoakLlmEvidence, PaperSoakManifest,
    PaperSoakObservation, PaperSoakPendingRequirement, PaperSoakQualificationStatus,
    PaperSoakResourceEvidence, PaperSoakSafetyCounters, PaperSoakVersions, PaperSoakViolation,
};
use ironpilot_domain::DomainDecimal;

const START: u64 = 1_800_000_000_000;
const DAY: u64 = 24 * 60 * 60 * 1_000;

fn decimal(value: &str) -> DomainDecimal {
    value.parse().expect("fixture decimal should parse")
}

fn manifest() -> PaperSoakManifest {
    PaperSoakManifest::new(
        "paper-soak-2026-07-26",
        "paper-prod-a",
        START,
        PaperSoakVersions::new(
            "ironpilot-ai-paper-runtime-v1",
            "ironpilot-ai-decision-context-v1",
            "ironpilot-ai-trading-prompt-v2",
            "deepseek-chat",
            "ironpilot-ai-trading-plan-v3",
            "ironpilot-execution-validator-v1",
            "ironpilot-spot-execution-v1",
            "ironpilot-emergency-core-v1",
        )
        .expect("fixture versions should be valid"),
        PaperSoakLimits::new(
            1_400 * 1024 * 1024,
            200_000,
            1_024,
            256,
            1_000_000,
            100_000_000,
            1_000_000,
            40,
            200_000,
            decimal("2.00"),
            1,
        )
        .expect("fixture limits should be valid"),
    )
    .expect("fixture manifest should be valid")
}

fn observation(sequence: usize, observed_at: u64) -> PaperSoakObservation {
    let days = observed_at.saturating_sub(START).div_ceil(DAY);
    let management_count = u64::try_from(sequence / 10 + 1).expect("fixture count fits u64");
    PaperSoakObservation::new(
        format!("observation-{sequence:05}"),
        "paper-soak-2026-07-26",
        observed_at,
        true,
        true,
        PaperSoakResourceEvidence::new(
            256 * 1024 * 1024,
            20_000,
            10,
            20,
            1,
            2,
            1_000_000 + days * 1_000,
            900_000 + days * 1_000,
            100 + u64::try_from(sequence).expect("fixture sequence fits u64"),
        ),
        PaperSoakLlmEvidence::new(observed_at / DAY, 10, 10_000, decimal("0.10"), 1),
        PaperSoakSafetyCounters::new(0, 0, 0, 0, 0, management_count, management_count, 0),
    )
    .expect("fixture observation should be valid")
}

fn passing_faults() -> Vec<PaperSoakFaultEvidence> {
    PaperSoakFaultKind::ALL
        .into_iter()
        .enumerate()
        .map(|(index, kind)| {
            let injected_at =
                START + u64::try_from(index + 1).expect("fixture index fits u64") * 60 * 60 * 1_000;
            PaperSoakFaultEvidence::new(
                format!("fault-{index}"),
                "paper-soak-2026-07-26",
                kind,
                injected_at,
                injected_at + 60_000,
                true,
                true,
                0,
                0,
                0,
                0,
                0,
                true,
            )
            .expect("fixture fault should be valid")
        })
        .collect()
}

#[test]
fn exact_thirty_day_evidence_is_qualified_and_order_independent() {
    let sample_count = usize::try_from(
        PAPER_SOAK_REQUIRED_DURATION_MILLIS / PAPER_SOAK_MAX_OBSERVATION_GAP_MILLIS,
    )
    .expect("sample count fits usize")
        + 1;
    assert!(sample_count < MAX_PAPER_SOAK_OBSERVATIONS);
    let mut observations = (0..sample_count)
        .map(|sequence| {
            observation(
                sequence,
                START
                    + u64::try_from(sequence).expect("fixture sequence fits u64")
                        * PAPER_SOAK_MAX_OBSERVATION_GAP_MILLIS,
            )
        })
        .collect::<Vec<_>>();
    let mut faults = passing_faults();

    let report = PaperSoakEvaluator::evaluate(&manifest(), &observations, &faults)
        .expect("complete safe evidence should evaluate");
    assert_eq!(report.status(), PaperSoakQualificationStatus::Qualified);
    assert_eq!(
        report.observed_duration_millis(),
        PAPER_SOAK_REQUIRED_DURATION_MILLIS
    );
    assert!(report.pending_requirements().is_empty());
    assert!(report.violations().is_empty());

    observations.reverse();
    faults.reverse();
    let reordered = PaperSoakEvaluator::evaluate(&manifest(), &observations, &faults)
        .expect("input order must not affect evidence");
    assert_eq!(report.evidence_hash(), reordered.evidence_hash());
}

#[test]
fn short_window_and_missing_drills_remain_collecting() {
    let observations = vec![
        observation(0, START),
        observation(1, START + PAPER_SOAK_MAX_OBSERVATION_GAP_MILLIS),
    ];
    let report = PaperSoakEvaluator::evaluate(&manifest(), &observations, &[])
        .expect("partial evidence should evaluate");

    assert_eq!(report.status(), PaperSoakQualificationStatus::Collecting);
    assert!(report.pending_requirements().iter().any(|pending| {
        matches!(
            pending,
            PaperSoakPendingRequirement::Duration {
                required_millis: PAPER_SOAK_REQUIRED_DURATION_MILLIS,
                ..
            }
        )
    }));
    for kind in PaperSoakFaultKind::ALL {
        assert!(
            report
                .pending_requirements()
                .contains(&PaperSoakPendingRequirement::FaultEvidence(kind))
        );
    }
}

#[test]
fn safety_resource_budget_and_fault_failures_disqualify_profitable_runtime_evidence() {
    let bad = PaperSoakObservation::new(
        "observation-bad",
        "paper-soak-2026-07-26",
        START + PAPER_SOAK_MAX_OBSERVATION_GAP_MILLIS,
        true,
        false,
        PaperSoakResourceEvidence::new(
            1_401 * 1024 * 1024,
            200_001,
            1_025,
            1_025,
            257,
            257,
            3_000_000,
            2_000_000,
            1_000_000,
        ),
        PaperSoakLlmEvidence::new(START / DAY, 41, 200_001, decimal("2.01"), 2),
        PaperSoakSafetyCounters::new(1, 1, 1, 1, 1, 9_999, 9_999, 1),
    )
    .expect("bad safety evidence is structurally valid");
    let fault = PaperSoakFaultEvidence::new(
        "fault-bad",
        "paper-soak-2026-07-26",
        PaperSoakFaultKind::ModelTimeout,
        START + 1_000,
        START + 2_000,
        false,
        false,
        1,
        1,
        1,
        1,
        1,
        false,
    )
    .expect("bad fault evidence is structurally valid");

    let report = PaperSoakEvaluator::evaluate(&manifest(), &[observation(0, START), bad], &[fault])
        .expect("unsafe evidence should evaluate to disqualified");
    assert_eq!(report.status(), PaperSoakQualificationStatus::Disqualified);
    for violation in [
        PaperSoakViolation::EmergencyUnavailable,
        PaperSoakViolation::StateDivergence,
        PaperSoakViolation::UnmanagedSell,
        PaperSoakViolation::DuplicateBusinessEffect,
        PaperSoakViolation::AuditGap,
        PaperSoakViolation::LocalAiPlanMutation,
        PaperSoakViolation::MemoryLimitExceeded,
        PaperSoakViolation::CpuLimitExceeded,
        PaperSoakViolation::MarketQueueLimitExceeded,
        PaperSoakViolation::CriticalQueueLimitExceeded,
        PaperSoakViolation::DatabaseGrowthLimitExceeded,
        PaperSoakViolation::LlmCallLimitExceeded,
        PaperSoakViolation::LlmTokenLimitExceeded,
        PaperSoakViolation::LlmCostLimitExceeded,
        PaperSoakViolation::ReplanLimitExceeded,
        PaperSoakViolation::FaultNotFailClosed(PaperSoakFaultKind::ModelTimeout),
        PaperSoakViolation::FaultNotRecovered(PaperSoakFaultKind::ModelTimeout),
        PaperSoakViolation::FaultBusinessEffect(PaperSoakFaultKind::ModelTimeout),
        PaperSoakViolation::EmergencyNotIndependent(PaperSoakFaultKind::ModelTimeout),
    ] {
        assert!(
            report.violations().contains(&violation),
            "missing violation {violation:?}"
        );
    }
}

#[test]
fn gaps_counter_regression_and_manifest_duration_tampering_fail_closed() {
    let first = observation(10, START);
    let second = observation(0, START + PAPER_SOAK_MAX_OBSERVATION_GAP_MILLIS + 1);
    let report = PaperSoakEvaluator::evaluate(&manifest(), &[first, second], &[])
        .expect("unsafe series should evaluate");
    assert_eq!(report.status(), PaperSoakQualificationStatus::Disqualified);
    assert!(
        report
            .violations()
            .contains(&PaperSoakViolation::ObservationGap)
    );
    assert!(
        report
            .violations()
            .contains(&PaperSoakViolation::CounterRegression)
    );

    let mut value = serde_json::to_value(manifest()).expect("manifest should serialize");
    value["required_duration_millis"] = serde_json::json!(1);
    let tampered: PaperSoakManifest =
        serde_json::from_value(value).expect("structural JSON should deserialize");
    assert!(PaperSoakEvaluator::evaluate(&tampered, &[observation(0, START)], &[]).is_err());
}
