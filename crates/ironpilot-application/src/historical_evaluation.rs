use core::fmt;
use std::collections::{BTreeMap, BTreeSet};

use ironpilot_domain::{
    AI_DECISION_CONTEXT_SCHEMA_VERSION_V1, AI_TRADING_PLAN_SCHEMA_VERSION_V3, DomainDecimal,
};
use quant_metrics::{
    expectancy as metric_expectancy, max_drawdown as metric_max_drawdown,
    total_return as metric_total_return,
};
use rust_decimal::Decimal;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::{
    EXECUTION_VALIDATOR_VERSION_V1, PAPER_MATCHING_ENGINE_VERSION_V1,
    SPOT_EXECUTION_SCHEMA_VERSION_V1,
};

pub const FULL_HISTORICAL_EVALUATION_VERSION_V1: &str = "ironpilot-full-historical-evaluation-v1";
pub const HISTORICAL_METRICS_LIBRARY_VERSION_V1: &str = "quant-metrics-0.7.0";
pub const MAX_HISTORICAL_EVALUATION_RECORDS: usize = 100_000;
pub const MAX_HISTORICAL_STRESS_SCENARIOS: usize = 8;
pub const MAX_HISTORICAL_REJECTION_REASONS: usize = 32;
const MAX_EVIDENCE_LABEL_LENGTH: usize = 128;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum HistoricalEvaluationArm {
    RuleOnlyBaseline,
    DeterministicAiPlanStub,
    RecordedAiTradingPlan,
}

impl HistoricalEvaluationArm {
    const ALL: [Self; 3] = [
        Self::RuleOnlyBaseline,
        Self::DeterministicAiPlanStub,
        Self::RecordedAiTradingPlan,
    ];

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RuleOnlyBaseline => "RULE_ONLY_BASELINE",
            Self::DeterministicAiPlanStub => "DETERMINISTIC_AI_PLAN_STUB",
            Self::RecordedAiTradingPlan => "RECORDED_AI_TRADING_PLAN",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HistoricalDecisionOutcome {
    Executed,
    NoTrade,
    Rejected,
}

impl HistoricalDecisionOutcome {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Executed => "EXECUTED",
            Self::NoTrade => "NO_TRADE",
            Self::Rejected => "REJECTED",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoricalStressScenario {
    name: Box<str>,
    fee_multiplier: DomainDecimal,
    slippage_multiplier: DomainDecimal,
}

impl HistoricalStressScenario {
    pub fn new(
        name: impl Into<Box<str>>,
        fee_multiplier: DomainDecimal,
        slippage_multiplier: DomainDecimal,
    ) -> Result<Self, FullHistoricalEvaluationError> {
        let name = name.into();
        if !valid_label(&name)
            || fee_multiplier <= DomainDecimal::ZERO
            || slippage_multiplier <= DomainDecimal::ZERO
        {
            return Err(FullHistoricalEvaluationError::InvalidManifest);
        }
        Ok(Self {
            name,
            fee_multiplier,
            slippage_multiplier,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoricalEvaluationManifest {
    dataset_hash: Box<str>,
    prompt_version: Box<str>,
    model_version: Box<str>,
    deterministic_stub_plan_set_hash: Box<str>,
    recorded_plan_set_hash: Box<str>,
    evaluation_start_unix_millis: u64,
    evaluation_end_unix_millis: u64,
    out_of_sample_start_unix_millis: u64,
    starting_equity_quote: DomainDecimal,
    maximum_loss_quote: DomainDecimal,
    base_fee_rate: DomainDecimal,
    base_slippage_rate: DomainDecimal,
    stress_scenarios: Vec<HistoricalStressScenario>,
    manifest_hash: Box<str>,
}

impl HistoricalEvaluationManifest {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        dataset_hash: impl Into<Box<str>>,
        prompt_version: impl Into<Box<str>>,
        model_version: impl Into<Box<str>>,
        deterministic_stub_plan_set_hash: impl Into<Box<str>>,
        recorded_plan_set_hash: impl Into<Box<str>>,
        evaluation_start_unix_millis: u64,
        evaluation_end_unix_millis: u64,
        out_of_sample_start_unix_millis: u64,
        starting_equity_quote: DomainDecimal,
        maximum_loss_quote: DomainDecimal,
        base_fee_rate: DomainDecimal,
        base_slippage_rate: DomainDecimal,
        mut stress_scenarios: Vec<HistoricalStressScenario>,
    ) -> Result<Self, FullHistoricalEvaluationError> {
        let dataset_hash = dataset_hash.into();
        let prompt_version = prompt_version.into();
        let model_version = model_version.into();
        let deterministic_stub_plan_set_hash = deterministic_stub_plan_set_hash.into();
        let recorded_plan_set_hash = recorded_plan_set_hash.into();
        stress_scenarios.sort_by(|left, right| left.name.cmp(&right.name));
        let scenario_names = stress_scenarios
            .iter()
            .map(|scenario| scenario.name.as_ref())
            .collect::<BTreeSet<_>>();
        if !valid_hash(&dataset_hash)
            || !valid_hash(&deterministic_stub_plan_set_hash)
            || !valid_hash(&recorded_plan_set_hash)
            || !valid_label(&prompt_version)
            || !valid_label(&model_version)
            || evaluation_start_unix_millis >= out_of_sample_start_unix_millis
            || out_of_sample_start_unix_millis > evaluation_end_unix_millis
            || starting_equity_quote <= DomainDecimal::ZERO
            || maximum_loss_quote <= DomainDecimal::ZERO
            || base_fee_rate < DomainDecimal::ZERO
            || base_slippage_rate < DomainDecimal::ZERO
            || stress_scenarios.is_empty()
            || stress_scenarios.len() > MAX_HISTORICAL_STRESS_SCENARIOS
            || scenario_names.len() != stress_scenarios.len()
        {
            return Err(FullHistoricalEvaluationError::InvalidManifest);
        }
        let mut manifest = Self {
            dataset_hash,
            prompt_version,
            model_version,
            deterministic_stub_plan_set_hash,
            recorded_plan_set_hash,
            evaluation_start_unix_millis,
            evaluation_end_unix_millis,
            out_of_sample_start_unix_millis,
            starting_equity_quote,
            maximum_loss_quote,
            base_fee_rate,
            base_slippage_rate,
            stress_scenarios,
            manifest_hash: Box::from(""),
        };
        manifest.manifest_hash = hash_json(&manifest.payload()).into_boxed_str();
        Ok(manifest)
    }

    fn payload(&self) -> Value {
        json!({
            "schema_version": FULL_HISTORICAL_EVALUATION_VERSION_V1,
            "dataset_hash": self.dataset_hash,
            "context_schema_version": AI_DECISION_CONTEXT_SCHEMA_VERSION_V1,
            "prompt_version": self.prompt_version,
            "model_version": self.model_version,
            "ai_trading_plan_schema_version": AI_TRADING_PLAN_SCHEMA_VERSION_V3,
            "deterministic_stub_plan_set_hash": self.deterministic_stub_plan_set_hash,
            "recorded_plan_set_hash": self.recorded_plan_set_hash,
            "validator_version": EXECUTION_VALIDATOR_VERSION_V1,
            "execution_version": SPOT_EXECUTION_SCHEMA_VERSION_V1,
            "matching_version": PAPER_MATCHING_ENGINE_VERSION_V1,
            "metrics_library_version": HISTORICAL_METRICS_LIBRARY_VERSION_V1,
            "evaluation_start_unix_millis": self.evaluation_start_unix_millis,
            "evaluation_end_unix_millis": self.evaluation_end_unix_millis,
            "out_of_sample_start_unix_millis": self.out_of_sample_start_unix_millis,
            "starting_equity_quote": self.starting_equity_quote.to_string(),
            "maximum_loss_quote": self.maximum_loss_quote.to_string(),
            "base_fee_rate": self.base_fee_rate.to_string(),
            "base_slippage_rate": self.base_slippage_rate.to_string(),
            "stress_scenarios": self.stress_scenarios.iter().map(|scenario| json!({
                "name": scenario.name,
                "fee_multiplier": scenario.fee_multiplier.to_string(),
                "slippage_multiplier": scenario.slippage_multiplier.to_string()
            })).collect::<Vec<_>>()
        })
    }

    #[must_use]
    pub fn manifest_hash(&self) -> &str {
        &self.manifest_hash
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoricalEvaluationRecord {
    comparison_id: Box<str>,
    arm: HistoricalEvaluationArm,
    market_fact_hash: Box<str>,
    execution_evidence_hash: Box<str>,
    facts_as_of_unix_millis: u64,
    decision_at_unix_millis: u64,
    settled_at_unix_millis: u64,
    outcome: HistoricalDecisionOutcome,
    gross_pnl_quote: DomainDecimal,
    fees_quote: DomainDecimal,
    slippage_cost_quote: DomainDecimal,
    ai_plan_hash: Option<Box<str>>,
    local_parameter_mutations: u32,
    rejection_reasons: Vec<Box<str>>,
    safety_failures: Vec<Box<str>>,
}

impl HistoricalEvaluationRecord {
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        comparison_id: impl Into<Box<str>>,
        arm: HistoricalEvaluationArm,
        market_fact_hash: impl Into<Box<str>>,
        execution_evidence_hash: impl Into<Box<str>>,
        facts_as_of_unix_millis: u64,
        decision_at_unix_millis: u64,
        settled_at_unix_millis: u64,
        outcome: HistoricalDecisionOutcome,
        gross_pnl_quote: DomainDecimal,
        fees_quote: DomainDecimal,
        slippage_cost_quote: DomainDecimal,
        ai_plan_hash: Option<Box<str>>,
        local_parameter_mutations: u32,
        rejection_reasons: Vec<Box<str>>,
        safety_failures: Vec<Box<str>>,
    ) -> Self {
        Self {
            comparison_id: comparison_id.into(),
            arm,
            market_fact_hash: market_fact_hash.into(),
            execution_evidence_hash: execution_evidence_hash.into(),
            facts_as_of_unix_millis,
            decision_at_unix_millis,
            settled_at_unix_millis,
            outcome,
            gross_pnl_quote,
            fees_quote,
            slippage_cost_quote,
            ai_plan_hash,
            local_parameter_mutations,
            rejection_reasons,
            safety_failures,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoricalReferenceArmMetrics {
    arm: HistoricalEvaluationArm,
    net_pnl_quote: DomainDecimal,
    maximum_drawdown_quote: DomainDecimal,
    trade_count: u64,
    out_of_sample_net_pnl_quote: DomainDecimal,
}

impl HistoricalReferenceArmMetrics {
    #[must_use]
    pub const fn new(
        arm: HistoricalEvaluationArm,
        net_pnl_quote: DomainDecimal,
        maximum_drawdown_quote: DomainDecimal,
        trade_count: u64,
        out_of_sample_net_pnl_quote: DomainDecimal,
    ) -> Self {
        Self {
            arm,
            net_pnl_quote,
            maximum_drawdown_quote,
            trade_count,
            out_of_sample_net_pnl_quote,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoricalIndependentReference {
    source: Box<str>,
    artifact_hash: Box<str>,
    arms: Vec<HistoricalReferenceArmMetrics>,
}

impl HistoricalIndependentReference {
    #[must_use]
    pub fn new(
        source: impl Into<Box<str>>,
        artifact_hash: impl Into<Box<str>>,
        arms: Vec<HistoricalReferenceArmMetrics>,
    ) -> Self {
        Self {
            source: source.into(),
            artifact_hash: artifact_hash.into(),
            arms,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoricalPeriodMetrics {
    gross_pnl_quote: DomainDecimal,
    net_pnl_quote: DomainDecimal,
    total_cost_quote: DomainDecimal,
    maximum_drawdown_quote: DomainDecimal,
    maximum_drawdown_percent: DomainDecimal,
    total_return_percent: DomainDecimal,
    expectancy_quote: DomainDecimal,
    decision_count: u64,
    trade_count: u64,
    no_trade_count: u64,
    rejection_count: u64,
}

impl HistoricalPeriodMetrics {
    #[must_use]
    pub const fn net_pnl_quote(&self) -> DomainDecimal {
        self.net_pnl_quote
    }

    #[must_use]
    pub const fn total_cost_quote(&self) -> DomainDecimal {
        self.total_cost_quote
    }

    #[must_use]
    pub const fn maximum_drawdown_quote(&self) -> DomainDecimal {
        self.maximum_drawdown_quote
    }

    #[must_use]
    pub const fn maximum_drawdown_percent(&self) -> DomainDecimal {
        self.maximum_drawdown_percent
    }

    #[must_use]
    pub const fn total_return_percent(&self) -> DomainDecimal {
        self.total_return_percent
    }

    #[must_use]
    pub const fn expectancy_quote(&self) -> DomainDecimal {
        self.expectancy_quote
    }

    #[must_use]
    pub const fn decision_count(&self) -> u64 {
        self.decision_count
    }

    #[must_use]
    pub const fn trade_count(&self) -> u64 {
        self.trade_count
    }

    fn to_json(&self) -> Value {
        json!({
            "gross_pnl_quote": self.gross_pnl_quote.to_string(),
            "net_pnl_quote": self.net_pnl_quote.to_string(),
            "total_cost_quote": self.total_cost_quote.to_string(),
            "maximum_drawdown_quote": self.maximum_drawdown_quote.to_string(),
            "maximum_drawdown_percent": self.maximum_drawdown_percent.to_string(),
            "total_return_percent": self.total_return_percent.to_string(),
            "expectancy_quote": self.expectancy_quote.to_string(),
            "decision_count": self.decision_count,
            "trade_count": self.trade_count,
            "no_trade_count": self.no_trade_count,
            "rejection_count": self.rejection_count
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoricalArmMetrics {
    arm: HistoricalEvaluationArm,
    full_sample: HistoricalPeriodMetrics,
    out_of_sample: HistoricalPeriodMetrics,
    rejection_reasons: Vec<(Box<str>, u64)>,
}

impl HistoricalArmMetrics {
    #[must_use]
    pub const fn arm(&self) -> HistoricalEvaluationArm {
        self.arm
    }

    #[must_use]
    pub const fn full_sample(&self) -> &HistoricalPeriodMetrics {
        &self.full_sample
    }

    #[must_use]
    pub const fn out_of_sample(&self) -> &HistoricalPeriodMetrics {
        &self.out_of_sample
    }

    #[must_use]
    pub fn rejection_reasons(&self) -> &[(Box<str>, u64)] {
        &self.rejection_reasons
    }

    fn to_json(&self) -> Value {
        json!({
            "arm": self.arm.as_str(),
            "production_eligible": false,
            "full_sample": self.full_sample.to_json(),
            "out_of_sample": self.out_of_sample.to_json(),
            "rejection_reasons": self.rejection_reasons.iter().map(|(code, count)| json!({
                "code": code,
                "count": count
            })).collect::<Vec<_>>()
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoricalStressResult {
    scenario: Box<str>,
    arm: HistoricalEvaluationArm,
    net_pnl_quote: DomainDecimal,
    total_cost_quote: DomainDecimal,
    maximum_drawdown_quote: DomainDecimal,
    maximum_drawdown_percent: DomainDecimal,
}

impl HistoricalStressResult {
    #[must_use]
    pub fn scenario(&self) -> &str {
        &self.scenario
    }

    #[must_use]
    pub const fn arm(&self) -> HistoricalEvaluationArm {
        self.arm
    }

    #[must_use]
    pub const fn net_pnl_quote(&self) -> DomainDecimal {
        self.net_pnl_quote
    }

    #[must_use]
    pub const fn maximum_drawdown_percent(&self) -> DomainDecimal {
        self.maximum_drawdown_percent
    }

    fn to_json(&self) -> Value {
        json!({
            "scenario": self.scenario,
            "arm": self.arm.as_str(),
            "net_pnl_quote": self.net_pnl_quote.to_string(),
            "total_cost_quote": self.total_cost_quote.to_string(),
            "maximum_drawdown_quote": self.maximum_drawdown_quote.to_string(),
            "maximum_drawdown_percent": self.maximum_drawdown_percent.to_string()
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoricalTradeDifference {
    comparison_id: Box<str>,
    rule_only_execution_evidence_hash: Box<str>,
    stub_execution_evidence_hash: Box<str>,
    recorded_execution_evidence_hash: Box<str>,
    stub_ai_plan_hash: Box<str>,
    recorded_ai_plan_hash: Box<str>,
    rule_only_outcome: HistoricalDecisionOutcome,
    stub_outcome: HistoricalDecisionOutcome,
    recorded_outcome: HistoricalDecisionOutcome,
    rule_only_net_pnl_quote: DomainDecimal,
    stub_net_pnl_quote: DomainDecimal,
    recorded_net_pnl_quote: DomainDecimal,
    recorded_vs_rule_delta_quote: DomainDecimal,
    recorded_vs_stub_delta_quote: DomainDecimal,
}

impl HistoricalTradeDifference {
    #[must_use]
    pub fn comparison_id(&self) -> &str {
        &self.comparison_id
    }

    #[must_use]
    pub const fn recorded_vs_rule_delta_quote(&self) -> DomainDecimal {
        self.recorded_vs_rule_delta_quote
    }

    #[must_use]
    pub const fn recorded_vs_stub_delta_quote(&self) -> DomainDecimal {
        self.recorded_vs_stub_delta_quote
    }

    fn to_json(&self) -> Value {
        json!({
            "comparison_id": self.comparison_id,
            "rule_only": {
                "execution_evidence_hash": self.rule_only_execution_evidence_hash,
                "outcome": self.rule_only_outcome.as_str(),
                "net_pnl_quote": self.rule_only_net_pnl_quote.to_string()
            },
            "deterministic_ai_stub": {
                "ai_plan_hash": self.stub_ai_plan_hash,
                "execution_evidence_hash": self.stub_execution_evidence_hash,
                "outcome": self.stub_outcome.as_str(),
                "net_pnl_quote": self.stub_net_pnl_quote.to_string()
            },
            "recorded_ai": {
                "ai_plan_hash": self.recorded_ai_plan_hash,
                "execution_evidence_hash": self.recorded_execution_evidence_hash,
                "outcome": self.recorded_outcome.as_str(),
                "net_pnl_quote": self.recorded_net_pnl_quote.to_string()
            },
            "recorded_vs_rule_delta_quote": self.recorded_vs_rule_delta_quote.to_string(),
            "recorded_vs_stub_delta_quote": self.recorded_vs_stub_delta_quote.to_string()
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FullHistoricalEvaluationReport {
    manifest: HistoricalEvaluationManifest,
    arms: Vec<HistoricalArmMetrics>,
    stress_results: Vec<HistoricalStressResult>,
    trade_differences: Vec<HistoricalTradeDifference>,
    recorded_vs_rule_net_pnl_delta_quote: DomainDecimal,
    recorded_vs_stub_net_pnl_delta_quote: DomainDecimal,
    recorded_vs_rule_decision_divergence_count: u64,
    recorded_vs_stub_decision_divergence_count: u64,
    independent_reference_source: Box<str>,
    independent_reference_artifact_hash: Box<str>,
    report_hash: Box<str>,
}

impl FullHistoricalEvaluationReport {
    #[must_use]
    pub fn arms(&self) -> &[HistoricalArmMetrics] {
        &self.arms
    }

    #[must_use]
    pub fn report_hash(&self) -> &str {
        &self.report_hash
    }

    #[must_use]
    pub fn stress_results(&self) -> &[HistoricalStressResult] {
        &self.stress_results
    }

    #[must_use]
    pub fn trade_differences(&self) -> &[HistoricalTradeDifference] {
        &self.trade_differences
    }

    #[must_use]
    pub const fn recorded_vs_rule_net_pnl_delta_quote(&self) -> DomainDecimal {
        self.recorded_vs_rule_net_pnl_delta_quote
    }

    #[must_use]
    pub const fn recorded_vs_stub_net_pnl_delta_quote(&self) -> DomainDecimal {
        self.recorded_vs_stub_net_pnl_delta_quote
    }

    #[must_use]
    pub fn to_json(&self) -> String {
        let mut payload = self.payload();
        payload["report_hash"] = json!(self.report_hash);
        serde_json::to_string(&payload).expect("historical evaluation report must serialize")
    }

    fn payload(&self) -> Value {
        json!({
            "schema_version": FULL_HISTORICAL_EVALUATION_VERSION_V1,
            "manifest": self.manifest.payload(),
            "manifest_hash": self.manifest.manifest_hash,
            "arms": self.arms.iter().map(HistoricalArmMetrics::to_json).collect::<Vec<_>>(),
            "ai_decision_contribution": {
                "recorded_vs_rule_net_pnl_delta_quote": self.recorded_vs_rule_net_pnl_delta_quote.to_string(),
                "recorded_vs_stub_net_pnl_delta_quote": self.recorded_vs_stub_net_pnl_delta_quote.to_string(),
                "recorded_vs_rule_decision_divergence_count": self.recorded_vs_rule_decision_divergence_count,
                "recorded_vs_stub_decision_divergence_count": self.recorded_vs_stub_decision_divergence_count
            },
            "stress_results": self.stress_results.iter().map(HistoricalStressResult::to_json).collect::<Vec<_>>(),
            "trade_differences": self.trade_differences.iter().map(HistoricalTradeDifference::to_json).collect::<Vec<_>>(),
            "independent_reference": {
                "source": self.independent_reference_source,
                "artifact_hash": self.independent_reference_artifact_hash,
                "tie_out": true
            },
            "safety_invariants_passed": true,
            "rule_only_production_eligible": false
        })
    }
}

pub struct FullHistoricalStrategyEvaluator;

impl FullHistoricalStrategyEvaluator {
    pub fn dataset_hash(
        records: &[HistoricalEvaluationRecord],
    ) -> Result<Box<str>, FullHistoricalEvaluationError> {
        evidence_binding_hash(records)
    }

    pub fn recorded_plan_set_hash(
        records: &[HistoricalEvaluationRecord],
    ) -> Result<Box<str>, FullHistoricalEvaluationError> {
        plan_set_binding_hash(records, HistoricalEvaluationArm::RecordedAiTradingPlan)
    }

    pub fn deterministic_stub_plan_set_hash(
        records: &[HistoricalEvaluationRecord],
    ) -> Result<Box<str>, FullHistoricalEvaluationError> {
        plan_set_binding_hash(records, HistoricalEvaluationArm::DeterministicAiPlanStub)
    }

    pub fn evaluate(
        manifest: HistoricalEvaluationManifest,
        records: Vec<HistoricalEvaluationRecord>,
        reference: HistoricalIndependentReference,
    ) -> Result<FullHistoricalEvaluationReport, FullHistoricalEvaluationError> {
        validate_manifest_hash(&manifest)?;
        let records = validate_and_normalize_records(&manifest, records)?;
        let groups = comparison_groups(&records)?;
        if manifest.dataset_hash != Self::dataset_hash(&records)?
            || manifest.deterministic_stub_plan_set_hash
                != Self::deterministic_stub_plan_set_hash(&records)?
            || manifest.recorded_plan_set_hash != Self::recorded_plan_set_hash(&records)?
        {
            return Err(FullHistoricalEvaluationError::EvidenceBindingMismatch);
        }

        let mut arms = Vec::with_capacity(HistoricalEvaluationArm::ALL.len());
        for arm in HistoricalEvaluationArm::ALL {
            arms.push(compute_arm_metrics(&manifest, &records, arm)?);
        }
        validate_reference(&reference, &arms)?;

        let mut stress_results = Vec::new();
        for scenario in &manifest.stress_scenarios {
            for arm in HistoricalEvaluationArm::ALL {
                stress_results.push(compute_stress_result(&manifest, &records, scenario, arm)?);
            }
        }

        let mut trade_differences = Vec::with_capacity(groups.len());
        let mut recorded_vs_rule_decision_divergence_count = 0_u64;
        let mut recorded_vs_stub_decision_divergence_count = 0_u64;
        for (comparison_id, group) in groups {
            let rule = group
                .get(&HistoricalEvaluationArm::RuleOnlyBaseline)
                .expect("validated comparison group has rule-only");
            let stub = group
                .get(&HistoricalEvaluationArm::DeterministicAiPlanStub)
                .expect("validated comparison group has stub");
            let recorded = group
                .get(&HistoricalEvaluationArm::RecordedAiTradingPlan)
                .expect("validated comparison group has recorded AI");
            let rule_net = record_net_pnl(rule)?;
            let stub_net = record_net_pnl(stub)?;
            let recorded_net = record_net_pnl(recorded)?;
            if recorded.outcome != rule.outcome {
                recorded_vs_rule_decision_divergence_count =
                    recorded_vs_rule_decision_divergence_count.saturating_add(1);
            }
            if recorded.outcome != stub.outcome {
                recorded_vs_stub_decision_divergence_count =
                    recorded_vs_stub_decision_divergence_count.saturating_add(1);
            }
            trade_differences.push(HistoricalTradeDifference {
                comparison_id,
                rule_only_execution_evidence_hash: rule.execution_evidence_hash.clone(),
                stub_execution_evidence_hash: stub.execution_evidence_hash.clone(),
                recorded_execution_evidence_hash: recorded.execution_evidence_hash.clone(),
                stub_ai_plan_hash: stub
                    .ai_plan_hash
                    .clone()
                    .expect("validated stub record has AI plan provenance"),
                recorded_ai_plan_hash: recorded
                    .ai_plan_hash
                    .clone()
                    .expect("validated recorded AI record has plan provenance"),
                rule_only_outcome: rule.outcome,
                stub_outcome: stub.outcome,
                recorded_outcome: recorded.outcome,
                rule_only_net_pnl_quote: rule_net,
                stub_net_pnl_quote: stub_net,
                recorded_net_pnl_quote: recorded_net,
                recorded_vs_rule_delta_quote: checked_sub(recorded_net, rule_net)?,
                recorded_vs_stub_delta_quote: checked_sub(recorded_net, stub_net)?,
            });
        }

        let recorded_metrics = arm_metrics(&arms, HistoricalEvaluationArm::RecordedAiTradingPlan);
        let rule_metrics = arm_metrics(&arms, HistoricalEvaluationArm::RuleOnlyBaseline);
        let stub_metrics = arm_metrics(&arms, HistoricalEvaluationArm::DeterministicAiPlanStub);
        let recorded_vs_rule_net_pnl_delta_quote = checked_sub(
            recorded_metrics.full_sample.net_pnl_quote,
            rule_metrics.full_sample.net_pnl_quote,
        )?;
        let recorded_vs_stub_net_pnl_delta_quote = checked_sub(
            recorded_metrics.full_sample.net_pnl_quote,
            stub_metrics.full_sample.net_pnl_quote,
        )?;
        let mut report = FullHistoricalEvaluationReport {
            manifest,
            arms,
            stress_results,
            trade_differences,
            recorded_vs_rule_net_pnl_delta_quote,
            recorded_vs_stub_net_pnl_delta_quote,
            recorded_vs_rule_decision_divergence_count,
            recorded_vs_stub_decision_divergence_count,
            independent_reference_source: reference.source,
            independent_reference_artifact_hash: reference.artifact_hash,
            report_hash: Box::from(""),
        };
        report.report_hash = hash_json(&report.payload()).into_boxed_str();
        Ok(report)
    }
}

fn validate_manifest_hash(
    manifest: &HistoricalEvaluationManifest,
) -> Result<(), FullHistoricalEvaluationError> {
    if manifest.manifest_hash.as_ref() != hash_json(&manifest.payload()) {
        return Err(FullHistoricalEvaluationError::InvalidManifest);
    }
    Ok(())
}

fn validate_and_normalize_records(
    manifest: &HistoricalEvaluationManifest,
    mut records: Vec<HistoricalEvaluationRecord>,
) -> Result<Vec<HistoricalEvaluationRecord>, FullHistoricalEvaluationError> {
    if records.is_empty() || records.len() > MAX_HISTORICAL_EVALUATION_RECORDS {
        return Err(FullHistoricalEvaluationError::RecordCountOutOfRange);
    }
    for record in &records {
        if !valid_label(&record.comparison_id)
            || !valid_hash(&record.market_fact_hash)
            || !valid_hash(&record.execution_evidence_hash)
            || record.decision_at_unix_millis < manifest.evaluation_start_unix_millis
            || record.decision_at_unix_millis > manifest.evaluation_end_unix_millis
            || record.settled_at_unix_millis < record.decision_at_unix_millis
            || record.fees_quote < DomainDecimal::ZERO
            || record.slippage_cost_quote < DomainDecimal::ZERO
            || record.rejection_reasons.len() > MAX_HISTORICAL_REJECTION_REASONS
            || record
                .rejection_reasons
                .iter()
                .any(|reason| !valid_label(reason))
        {
            return Err(FullHistoricalEvaluationError::InvalidRecord);
        }
        if record.facts_as_of_unix_millis > record.decision_at_unix_millis {
            return Err(FullHistoricalEvaluationError::FutureData);
        }
        if record.local_parameter_mutations != 0 {
            return Err(FullHistoricalEvaluationError::LocalPlanMutation);
        }
        if !record.safety_failures.is_empty() {
            return Err(FullHistoricalEvaluationError::SafetyInvariantFailure);
        }
        match record.arm {
            HistoricalEvaluationArm::RuleOnlyBaseline => {
                if record.ai_plan_hash.is_some() {
                    return Err(FullHistoricalEvaluationError::InvalidRecord);
                }
            }
            HistoricalEvaluationArm::DeterministicAiPlanStub
            | HistoricalEvaluationArm::RecordedAiTradingPlan => {
                if !record.ai_plan_hash.as_deref().is_some_and(valid_hash) {
                    return Err(FullHistoricalEvaluationError::AiPlanProvenanceMissing);
                }
            }
        }
        match record.outcome {
            HistoricalDecisionOutcome::Executed => {
                if record.settled_at_unix_millis == record.decision_at_unix_millis
                    || !record.rejection_reasons.is_empty()
                {
                    return Err(FullHistoricalEvaluationError::InvalidRecord);
                }
            }
            HistoricalDecisionOutcome::NoTrade => {
                if record.gross_pnl_quote != DomainDecimal::ZERO
                    || record.fees_quote != DomainDecimal::ZERO
                    || record.slippage_cost_quote != DomainDecimal::ZERO
                    || !record.rejection_reasons.is_empty()
                {
                    return Err(FullHistoricalEvaluationError::InvalidRecord);
                }
            }
            HistoricalDecisionOutcome::Rejected => {
                if record.gross_pnl_quote != DomainDecimal::ZERO
                    || record.fees_quote != DomainDecimal::ZERO
                    || record.slippage_cost_quote != DomainDecimal::ZERO
                    || record.rejection_reasons.is_empty()
                {
                    return Err(FullHistoricalEvaluationError::InvalidRecord);
                }
            }
        }
    }
    records.sort_by(|left, right| {
        left.decision_at_unix_millis
            .cmp(&right.decision_at_unix_millis)
            .then_with(|| left.comparison_id.cmp(&right.comparison_id))
            .then_with(|| left.arm.cmp(&right.arm))
    });
    if !records
        .iter()
        .any(|record| record.decision_at_unix_millis < manifest.out_of_sample_start_unix_millis)
        || !records.iter().any(|record| {
            record.decision_at_unix_millis >= manifest.out_of_sample_start_unix_millis
        })
    {
        return Err(FullHistoricalEvaluationError::InvalidRecord);
    }
    Ok(records)
}

fn comparison_groups(
    records: &[HistoricalEvaluationRecord],
) -> Result<
    BTreeMap<Box<str>, BTreeMap<HistoricalEvaluationArm, &HistoricalEvaluationRecord>>,
    FullHistoricalEvaluationError,
> {
    let mut groups =
        BTreeMap::<Box<str>, BTreeMap<HistoricalEvaluationArm, &HistoricalEvaluationRecord>>::new();
    for record in records {
        if groups
            .entry(record.comparison_id.clone())
            .or_default()
            .insert(record.arm, record)
            .is_some()
        {
            return Err(FullHistoricalEvaluationError::IncomparableArms);
        }
    }
    for group in groups.values() {
        if group.len() != HistoricalEvaluationArm::ALL.len()
            || HistoricalEvaluationArm::ALL
                .iter()
                .any(|arm| !group.contains_key(arm))
        {
            return Err(FullHistoricalEvaluationError::IncomparableArms);
        }
        let first = group
            .values()
            .next()
            .expect("validated comparison group is non-empty");
        if group.values().any(|record| {
            record.market_fact_hash != first.market_fact_hash
                || record.facts_as_of_unix_millis != first.facts_as_of_unix_millis
                || record.decision_at_unix_millis != first.decision_at_unix_millis
        }) {
            return Err(FullHistoricalEvaluationError::IncomparableArms);
        }
    }
    Ok(groups)
}

fn compute_arm_metrics(
    manifest: &HistoricalEvaluationManifest,
    records: &[HistoricalEvaluationRecord],
    arm: HistoricalEvaluationArm,
) -> Result<HistoricalArmMetrics, FullHistoricalEvaluationError> {
    let arm_records = records
        .iter()
        .filter(|record| record.arm == arm)
        .collect::<Vec<_>>();
    let out_of_sample_records = arm_records
        .iter()
        .copied()
        .filter(|record| record.decision_at_unix_millis >= manifest.out_of_sample_start_unix_millis)
        .collect::<Vec<_>>();
    let mut rejection_reasons = BTreeMap::<Box<str>, u64>::new();
    for record in &arm_records {
        for reason in &record.rejection_reasons {
            let count = rejection_reasons.entry(reason.clone()).or_default();
            *count = count.saturating_add(1);
        }
    }
    Ok(HistoricalArmMetrics {
        arm,
        full_sample: compute_period_metrics(manifest.starting_equity_quote, &arm_records)?,
        out_of_sample: compute_period_metrics(
            manifest.starting_equity_quote,
            &out_of_sample_records,
        )?,
        rejection_reasons: rejection_reasons.into_iter().collect(),
    })
}

fn compute_period_metrics(
    starting_equity: DomainDecimal,
    records: &[&HistoricalEvaluationRecord],
) -> Result<HistoricalPeriodMetrics, FullHistoricalEvaluationError> {
    let mut gross_pnl = DomainDecimal::ZERO;
    let mut net_pnl = DomainDecimal::ZERO;
    let mut total_cost = DomainDecimal::ZERO;
    let mut equity = starting_equity;
    let mut peak = starting_equity;
    let mut maximum_drawdown = DomainDecimal::ZERO;
    let mut equity_curve = vec![starting_equity.as_decimal()];
    let mut executed_trade_pnls = Vec::new();
    let mut trade_count = 0_u64;
    let mut no_trade_count = 0_u64;
    let mut rejection_count = 0_u64;
    for record in records {
        let cost = checked_add(record.fees_quote, record.slippage_cost_quote)?;
        let record_net = record_net_pnl(record)?;
        gross_pnl = checked_add(gross_pnl, record.gross_pnl_quote)?;
        total_cost = checked_add(total_cost, cost)?;
        net_pnl = checked_add(net_pnl, record_net)?;
        equity = checked_add(equity, record_net)?;
        equity_curve.push(equity.as_decimal());
        if equity > peak {
            peak = equity;
        }
        let drawdown = checked_sub(peak, equity)?;
        if drawdown > maximum_drawdown {
            maximum_drawdown = drawdown;
        }
        match record.outcome {
            HistoricalDecisionOutcome::Executed => {
                trade_count = trade_count.saturating_add(1);
                executed_trade_pnls.push(record_net.as_decimal());
            }
            HistoricalDecisionOutcome::NoTrade => {
                no_trade_count = no_trade_count.saturating_add(1);
            }
            HistoricalDecisionOutcome::Rejected => {
                rejection_count = rejection_count.saturating_add(1);
            }
        }
    }
    let total_return_percent = metric_decimal(
        metric_total_return(&equity_curve)
            .map_err(|_| FullHistoricalEvaluationError::MetricLibrary)?,
    )?;
    let raw_drawdown_percent = metric_decimal(
        metric_max_drawdown(&equity_curve)
            .map_err(|_| FullHistoricalEvaluationError::MetricLibrary)?,
    )?;
    let maximum_drawdown_percent = if raw_drawdown_percent < DomainDecimal::ZERO {
        checked_sub(DomainDecimal::ZERO, raw_drawdown_percent)?
    } else {
        raw_drawdown_percent
    };
    let expectancy_quote = if trade_count == 0 {
        DomainDecimal::ZERO
    } else {
        metric_decimal(
            metric_expectancy(&executed_trade_pnls)
                .map_err(|_| FullHistoricalEvaluationError::MetricLibrary)?,
        )?
    };
    Ok(HistoricalPeriodMetrics {
        gross_pnl_quote: gross_pnl,
        net_pnl_quote: net_pnl,
        total_cost_quote: total_cost,
        maximum_drawdown_quote: maximum_drawdown,
        maximum_drawdown_percent,
        total_return_percent,
        expectancy_quote,
        decision_count: u64::try_from(records.len())
            .map_err(|_| FullHistoricalEvaluationError::ArithmeticOverflow)?,
        trade_count,
        no_trade_count,
        rejection_count,
    })
}

fn compute_stress_result(
    manifest: &HistoricalEvaluationManifest,
    records: &[HistoricalEvaluationRecord],
    scenario: &HistoricalStressScenario,
    arm: HistoricalEvaluationArm,
) -> Result<HistoricalStressResult, FullHistoricalEvaluationError> {
    let mut net_pnl = DomainDecimal::ZERO;
    let mut total_cost = DomainDecimal::ZERO;
    let mut equity = manifest.starting_equity_quote;
    let mut peak = equity;
    let mut maximum_drawdown = DomainDecimal::ZERO;
    let mut equity_curve = vec![equity.as_decimal()];
    for record in records.iter().filter(|record| record.arm == arm) {
        let stressed_fee = checked_mul(record.fees_quote, scenario.fee_multiplier)?;
        let stressed_slippage =
            checked_mul(record.slippage_cost_quote, scenario.slippage_multiplier)?;
        let stressed_cost = checked_add(stressed_fee, stressed_slippage)?;
        let record_net = checked_sub(record.gross_pnl_quote, stressed_cost)?;
        total_cost = checked_add(total_cost, stressed_cost)?;
        net_pnl = checked_add(net_pnl, record_net)?;
        equity = checked_add(equity, record_net)?;
        equity_curve.push(equity.as_decimal());
        if equity > peak {
            peak = equity;
        }
        let drawdown = checked_sub(peak, equity)?;
        if drawdown > maximum_drawdown {
            maximum_drawdown = drawdown;
        }
    }
    let raw_drawdown_percent = metric_decimal(
        metric_max_drawdown(&equity_curve)
            .map_err(|_| FullHistoricalEvaluationError::MetricLibrary)?,
    )?;
    let maximum_drawdown_percent = if raw_drawdown_percent < DomainDecimal::ZERO {
        checked_sub(DomainDecimal::ZERO, raw_drawdown_percent)?
    } else {
        raw_drawdown_percent
    };
    Ok(HistoricalStressResult {
        scenario: scenario.name.clone(),
        arm,
        net_pnl_quote: net_pnl,
        total_cost_quote: total_cost,
        maximum_drawdown_quote: maximum_drawdown,
        maximum_drawdown_percent,
    })
}

fn validate_reference(
    reference: &HistoricalIndependentReference,
    arms: &[HistoricalArmMetrics],
) -> Result<(), FullHistoricalEvaluationError> {
    if !valid_label(&reference.source)
        || !valid_hash(&reference.artifact_hash)
        || reference.arms.len() != HistoricalEvaluationArm::ALL.len()
    {
        return Err(FullHistoricalEvaluationError::IndependentReferenceMismatch);
    }
    let mut seen = BTreeSet::new();
    for expected in &reference.arms {
        if !seen.insert(expected.arm) {
            return Err(FullHistoricalEvaluationError::IndependentReferenceMismatch);
        }
        let actual = arm_metrics(arms, expected.arm);
        if actual.full_sample.net_pnl_quote != expected.net_pnl_quote
            || actual.full_sample.maximum_drawdown_quote != expected.maximum_drawdown_quote
            || actual.full_sample.trade_count != expected.trade_count
            || actual.out_of_sample.net_pnl_quote != expected.out_of_sample_net_pnl_quote
        {
            return Err(FullHistoricalEvaluationError::IndependentReferenceMismatch);
        }
    }
    if seen.len() != HistoricalEvaluationArm::ALL.len() {
        return Err(FullHistoricalEvaluationError::IndependentReferenceMismatch);
    }
    Ok(())
}

fn arm_metrics(
    arms: &[HistoricalArmMetrics],
    arm: HistoricalEvaluationArm,
) -> &HistoricalArmMetrics {
    arms.iter()
        .find(|metrics| metrics.arm == arm)
        .expect("all historical evaluation arms are computed")
}

fn record_net_pnl(
    record: &HistoricalEvaluationRecord,
) -> Result<DomainDecimal, FullHistoricalEvaluationError> {
    checked_sub(
        record.gross_pnl_quote,
        checked_add(record.fees_quote, record.slippage_cost_quote)?,
    )
}

fn checked_add(
    left: DomainDecimal,
    right: DomainDecimal,
) -> Result<DomainDecimal, FullHistoricalEvaluationError> {
    left.checked_add(right)
        .ok_or(FullHistoricalEvaluationError::ArithmeticOverflow)
}

fn checked_sub(
    left: DomainDecimal,
    right: DomainDecimal,
) -> Result<DomainDecimal, FullHistoricalEvaluationError> {
    left.checked_sub(right)
        .ok_or(FullHistoricalEvaluationError::ArithmeticOverflow)
}

fn checked_mul(
    left: DomainDecimal,
    right: DomainDecimal,
) -> Result<DomainDecimal, FullHistoricalEvaluationError> {
    left.checked_mul(right)
        .ok_or(FullHistoricalEvaluationError::ArithmeticOverflow)
}

fn metric_decimal(value: Decimal) -> Result<DomainDecimal, FullHistoricalEvaluationError> {
    DomainDecimal::from_mantissa_scale(value.mantissa(), value.scale())
        .map_err(|_| FullHistoricalEvaluationError::ArithmeticOverflow)
}

fn valid_label(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_EVIDENCE_LABEL_LENGTH
        && !value.chars().any(char::is_control)
}

fn valid_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn evidence_binding_hash(
    records: &[HistoricalEvaluationRecord],
) -> Result<Box<str>, FullHistoricalEvaluationError> {
    if records.is_empty() || records.len() > MAX_HISTORICAL_EVALUATION_RECORDS {
        return Err(FullHistoricalEvaluationError::RecordCountOutOfRange);
    }
    let mut bindings = records
        .iter()
        .map(|record| {
            if !valid_label(&record.comparison_id) || !valid_hash(&record.market_fact_hash) {
                return Err(FullHistoricalEvaluationError::InvalidRecord);
            }
            Ok(json!({
                "comparison_id": record.comparison_id,
                "market_fact_hash": record.market_fact_hash,
                "facts_as_of_unix_millis": record.facts_as_of_unix_millis,
                "decision_at_unix_millis": record.decision_at_unix_millis,
                "arm": record.arm.as_str()
            }))
        })
        .collect::<Result<Vec<_>, FullHistoricalEvaluationError>>()?;
    if bindings.is_empty() {
        return Err(FullHistoricalEvaluationError::RecordCountOutOfRange);
    }
    bindings.sort_by_key(ToString::to_string);
    Ok(hash_json(&json!({
        "binding": "historical-comparison-dataset-v1",
        "records": bindings
    }))
    .into_boxed_str())
}

fn plan_set_binding_hash(
    records: &[HistoricalEvaluationRecord],
    arm: HistoricalEvaluationArm,
) -> Result<Box<str>, FullHistoricalEvaluationError> {
    let mut bindings = records
        .iter()
        .filter(|record| record.arm == arm)
        .map(|record| {
            let plan_hash = record
                .ai_plan_hash
                .as_deref()
                .filter(|hash| valid_hash(hash))
                .ok_or(FullHistoricalEvaluationError::AiPlanProvenanceMissing)?;
            Ok(json!({
                "comparison_id": record.comparison_id,
                "ai_plan_hash": plan_hash
            }))
        })
        .collect::<Result<Vec<_>, FullHistoricalEvaluationError>>()?;
    if bindings.is_empty() {
        return Err(FullHistoricalEvaluationError::AiPlanProvenanceMissing);
    }
    bindings.sort_by_key(ToString::to_string);
    Ok(hash_json(&json!({
        "binding": format!("{}-plan-set-v1", arm.as_str()),
        "plans": bindings
    }))
    .into_boxed_str())
}

fn hash_json(value: &Value) -> String {
    let payload = serde_json::to_string(value).expect("historical evidence must serialize");
    let digest = Sha256::digest(payload.as_bytes());
    let mut output = String::with_capacity(64);
    for byte in digest {
        use fmt::Write as _;
        write!(output, "{byte:02x}").expect("writing to a String cannot fail");
    }
    output
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FullHistoricalEvaluationError {
    InvalidManifest,
    RecordCountOutOfRange,
    InvalidRecord,
    FutureData,
    IncomparableArms,
    EvidenceBindingMismatch,
    AiPlanProvenanceMissing,
    LocalPlanMutation,
    SafetyInvariantFailure,
    ArithmeticOverflow,
    MetricLibrary,
    IndependentReferenceMismatch,
}

impl fmt::Display for FullHistoricalEvaluationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidManifest => formatter.write_str(
                "historical evaluation manifest is invalid or its immutable hash changed",
            ),
            Self::RecordCountOutOfRange => formatter
                .write_str("historical evaluation record count is outside the bounded range"),
            Self::InvalidRecord => {
                formatter.write_str("historical evaluation record is internally inconsistent")
            }
            Self::FutureData => {
                formatter.write_str("historical evaluation record contains future facts")
            }
            Self::IncomparableArms => formatter.write_str(
                "historical evaluation arms do not contain exactly comparable market facts",
            ),
            Self::EvidenceBindingMismatch => formatter.write_str(
                "historical evaluation records do not match the immutable dataset or plan-set binding",
            ),
            Self::AiPlanProvenanceMissing => formatter
                .write_str("AI historical evaluation arm is missing immutable plan provenance"),
            Self::LocalPlanMutation => {
                formatter.write_str("historical evaluation detected local AI plan mutation")
            }
            Self::SafetyInvariantFailure => formatter.write_str(
                "historical evaluation safety invariant failed; profitability cannot override it",
            ),
            Self::ArithmeticOverflow => {
                formatter.write_str("historical evaluation exact decimal arithmetic failed")
            }
            Self::MetricLibrary => {
                formatter.write_str("mature historical metrics library rejected the input")
            }
            Self::IndependentReferenceMismatch => formatter
                .write_str("historical evaluation does not tie out to the independent reference"),
        }
    }
}

impl std::error::Error for FullHistoricalEvaluationError {}
