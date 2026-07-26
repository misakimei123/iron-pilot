use core::fmt;
use std::collections::BTreeSet;

use ironpilot_domain::DomainDecimal;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const PAPER_SOAK_EVIDENCE_VERSION_V1: &str = "ironpilot-paper-soak-evidence-v1";
pub const PAPER_SOAK_REQUIRED_DURATION_MILLIS: u64 = 30 * 24 * 60 * 60 * 1_000;
pub const PAPER_SOAK_MAX_OBSERVATION_GAP_MILLIS: u64 = 5 * 60 * 1_000;
pub const MAX_PAPER_SOAK_OBSERVATIONS: usize = 100_000;
pub const MAX_PAPER_SOAK_FAULT_EVIDENCE: usize = 64;

const MILLIS_PER_DAY: u64 = 24 * 60 * 60 * 1_000;
const MAX_EVIDENCE_LABEL_LENGTH: usize = 128;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PaperSoakVersions {
    runtime: Box<str>,
    context: Box<str>,
    prompt: Box<str>,
    model: Box<str>,
    ai_plan: Box<str>,
    validator: Box<str>,
    execution: Box<str>,
    emergency: Box<str>,
}

impl PaperSoakVersions {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        runtime: impl Into<Box<str>>,
        context: impl Into<Box<str>>,
        prompt: impl Into<Box<str>>,
        model: impl Into<Box<str>>,
        ai_plan: impl Into<Box<str>>,
        validator: impl Into<Box<str>>,
        execution: impl Into<Box<str>>,
        emergency: impl Into<Box<str>>,
    ) -> Result<Self, PaperSoakEvidenceError> {
        let value = Self {
            runtime: runtime.into(),
            context: context.into(),
            prompt: prompt.into(),
            model: model.into(),
            ai_plan: ai_plan.into(),
            validator: validator.into(),
            execution: execution.into(),
            emergency: emergency.into(),
        };
        value.validate()?;
        Ok(value)
    }

    fn validate(&self) -> Result<(), PaperSoakEvidenceError> {
        for (field, value) in [
            ("runtime", self.runtime.as_ref()),
            ("context", self.context.as_ref()),
            ("prompt", self.prompt.as_ref()),
            ("model", self.model.as_ref()),
            ("ai_plan", self.ai_plan.as_ref()),
            ("validator", self.validator.as_ref()),
            ("execution", self.execution.as_ref()),
            ("emergency", self.emergency.as_ref()),
        ] {
            validate_label(field, value)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PaperSoakLimits {
    memory_soft_limit_bytes: u64,
    cpu_capacity_milli_percent: u32,
    market_queue_capacity: u32,
    critical_queue_capacity: u32,
    initial_database_bytes: u64,
    maximum_database_bytes: u64,
    maximum_database_growth_bytes_per_day: u64,
    llm_daily_call_limit: u32,
    llm_daily_token_limit: u64,
    llm_daily_cost_limit_usd: DomainDecimal,
    maximum_replans_per_context: u8,
}

impl PaperSoakLimits {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        memory_soft_limit_bytes: u64,
        cpu_capacity_milli_percent: u32,
        market_queue_capacity: u32,
        critical_queue_capacity: u32,
        initial_database_bytes: u64,
        maximum_database_bytes: u64,
        maximum_database_growth_bytes_per_day: u64,
        llm_daily_call_limit: u32,
        llm_daily_token_limit: u64,
        llm_daily_cost_limit_usd: DomainDecimal,
        maximum_replans_per_context: u8,
    ) -> Result<Self, PaperSoakEvidenceError> {
        let value = Self {
            memory_soft_limit_bytes,
            cpu_capacity_milli_percent,
            market_queue_capacity,
            critical_queue_capacity,
            initial_database_bytes,
            maximum_database_bytes,
            maximum_database_growth_bytes_per_day,
            llm_daily_call_limit,
            llm_daily_token_limit,
            llm_daily_cost_limit_usd,
            maximum_replans_per_context,
        };
        value.validate()?;
        Ok(value)
    }

    fn validate(&self) -> Result<(), PaperSoakEvidenceError> {
        if self.memory_soft_limit_bytes == 0
            || self.cpu_capacity_milli_percent == 0
            || self.market_queue_capacity == 0
            || self.critical_queue_capacity == 0
            || self.initial_database_bytes == 0
            || self.maximum_database_bytes < self.initial_database_bytes
            || self.maximum_database_growth_bytes_per_day == 0
            || self.llm_daily_call_limit == 0
            || self.llm_daily_token_limit == 0
            || self.llm_daily_cost_limit_usd <= DomainDecimal::ZERO
            || self.maximum_replans_per_context > 1
        {
            return Err(PaperSoakEvidenceError::InvalidLimits);
        }
        Ok(())
    }

    #[must_use]
    pub const fn memory_soft_limit_bytes(&self) -> u64 {
        self.memory_soft_limit_bytes
    }

    #[must_use]
    pub const fn cpu_capacity_milli_percent(&self) -> u32 {
        self.cpu_capacity_milli_percent
    }

    #[must_use]
    pub const fn market_queue_capacity(&self) -> u32 {
        self.market_queue_capacity
    }

    #[must_use]
    pub const fn critical_queue_capacity(&self) -> u32 {
        self.critical_queue_capacity
    }

    #[must_use]
    pub const fn initial_database_bytes(&self) -> u64 {
        self.initial_database_bytes
    }

    #[must_use]
    pub const fn maximum_database_bytes(&self) -> u64 {
        self.maximum_database_bytes
    }

    #[must_use]
    pub const fn maximum_database_growth_bytes_per_day(&self) -> u64 {
        self.maximum_database_growth_bytes_per_day
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PaperSoakManifest {
    schema_version: Box<str>,
    run_id: Box<str>,
    environment_fingerprint: Box<str>,
    started_at_unix_millis: u64,
    required_duration_millis: u64,
    maximum_observation_gap_millis: u64,
    versions: PaperSoakVersions,
    limits: PaperSoakLimits,
}

impl PaperSoakManifest {
    pub fn new(
        run_id: impl Into<Box<str>>,
        environment_fingerprint: impl Into<Box<str>>,
        started_at_unix_millis: u64,
        versions: PaperSoakVersions,
        limits: PaperSoakLimits,
    ) -> Result<Self, PaperSoakEvidenceError> {
        let value = Self {
            schema_version: PAPER_SOAK_EVIDENCE_VERSION_V1.into(),
            run_id: run_id.into(),
            environment_fingerprint: environment_fingerprint.into(),
            started_at_unix_millis,
            required_duration_millis: PAPER_SOAK_REQUIRED_DURATION_MILLIS,
            maximum_observation_gap_millis: PAPER_SOAK_MAX_OBSERVATION_GAP_MILLIS,
            versions,
            limits,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), PaperSoakEvidenceError> {
        if self.schema_version.as_ref() != PAPER_SOAK_EVIDENCE_VERSION_V1
            || self.required_duration_millis != PAPER_SOAK_REQUIRED_DURATION_MILLIS
            || self.maximum_observation_gap_millis != PAPER_SOAK_MAX_OBSERVATION_GAP_MILLIS
        {
            return Err(PaperSoakEvidenceError::UnsupportedManifest);
        }
        validate_label("run_id", &self.run_id)?;
        validate_label("environment_fingerprint", &self.environment_fingerprint)?;
        self.versions.validate()?;
        self.limits.validate()
    }

    #[must_use]
    pub fn run_id(&self) -> &str {
        &self.run_id
    }

    #[must_use]
    pub const fn started_at_unix_millis(&self) -> u64 {
        self.started_at_unix_millis
    }

    #[must_use]
    pub const fn limits(&self) -> &PaperSoakLimits {
        &self.limits
    }

    pub fn evidence_hash(&self) -> Result<Box<str>, PaperSoakEvidenceError> {
        hash_serializable(self)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PaperSoakResourceEvidence {
    resident_memory_bytes: u64,
    cpu_milli_percent: u32,
    market_queue_depth: u32,
    market_queue_high_watermark: u32,
    critical_queue_depth: u32,
    critical_queue_high_watermark: u32,
    database_allocated_bytes: u64,
    database_used_bytes: u64,
    database_business_rows: u64,
}

impl PaperSoakResourceEvidence {
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub const fn new(
        resident_memory_bytes: u64,
        cpu_milli_percent: u32,
        market_queue_depth: u32,
        market_queue_high_watermark: u32,
        critical_queue_depth: u32,
        critical_queue_high_watermark: u32,
        database_allocated_bytes: u64,
        database_used_bytes: u64,
        database_business_rows: u64,
    ) -> Self {
        Self {
            resident_memory_bytes,
            cpu_milli_percent,
            market_queue_depth,
            market_queue_high_watermark,
            critical_queue_depth,
            critical_queue_high_watermark,
            database_allocated_bytes,
            database_used_bytes,
            database_business_rows,
        }
    }

    fn validate(&self) -> Result<(), PaperSoakEvidenceError> {
        if self.market_queue_depth > self.market_queue_high_watermark
            || self.critical_queue_depth > self.critical_queue_high_watermark
            || self.database_used_bytes > self.database_allocated_bytes
        {
            return Err(PaperSoakEvidenceError::InvalidResourceEvidence);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PaperSoakLlmEvidence {
    utc_day: u64,
    calls_used_or_reserved: u32,
    tokens_used_or_reserved: u64,
    cost_used_or_reserved_usd: DomainDecimal,
    maximum_replans_observed_for_context: u8,
}

impl PaperSoakLlmEvidence {
    #[must_use]
    pub const fn new(
        utc_day: u64,
        calls_used_or_reserved: u32,
        tokens_used_or_reserved: u64,
        cost_used_or_reserved_usd: DomainDecimal,
        maximum_replans_observed_for_context: u8,
    ) -> Self {
        Self {
            utc_day,
            calls_used_or_reserved,
            tokens_used_or_reserved,
            cost_used_or_reserved_usd,
            maximum_replans_observed_for_context,
        }
    }

    fn validate(&self) -> Result<(), PaperSoakEvidenceError> {
        if self.cost_used_or_reserved_usd < DomainDecimal::ZERO {
            return Err(PaperSoakEvidenceError::InvalidLlmEvidence);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PaperSoakSafetyCounters {
    state_divergences: u64,
    unmanaged_sell_effects: u64,
    duplicate_business_effects: u64,
    audit_gaps: u64,
    local_ai_plan_mutations: u64,
    managed_position_reviews: u64,
    ai_management_actions: u64,
    unanswered_managed_position_reviews: u64,
}

impl PaperSoakSafetyCounters {
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub const fn new(
        state_divergences: u64,
        unmanaged_sell_effects: u64,
        duplicate_business_effects: u64,
        audit_gaps: u64,
        local_ai_plan_mutations: u64,
        managed_position_reviews: u64,
        ai_management_actions: u64,
        unanswered_managed_position_reviews: u64,
    ) -> Self {
        Self {
            state_divergences,
            unmanaged_sell_effects,
            duplicate_business_effects,
            audit_gaps,
            local_ai_plan_mutations,
            managed_position_reviews,
            ai_management_actions,
            unanswered_managed_position_reviews,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PaperSoakObservation {
    observation_id: Box<str>,
    run_id: Box<str>,
    observed_at_unix_millis: u64,
    process_alive: bool,
    emergency_path_available: bool,
    resources: PaperSoakResourceEvidence,
    llm: PaperSoakLlmEvidence,
    counters: PaperSoakSafetyCounters,
}

impl PaperSoakObservation {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        observation_id: impl Into<Box<str>>,
        run_id: impl Into<Box<str>>,
        observed_at_unix_millis: u64,
        process_alive: bool,
        emergency_path_available: bool,
        resources: PaperSoakResourceEvidence,
        llm: PaperSoakLlmEvidence,
        counters: PaperSoakSafetyCounters,
    ) -> Result<Self, PaperSoakEvidenceError> {
        let value = Self {
            observation_id: observation_id.into(),
            run_id: run_id.into(),
            observed_at_unix_millis,
            process_alive,
            emergency_path_available,
            resources,
            llm,
            counters,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), PaperSoakEvidenceError> {
        validate_label("observation_id", &self.observation_id)?;
        validate_label("run_id", &self.run_id)?;
        self.resources.validate()?;
        self.llm.validate()
    }

    #[must_use]
    pub fn observation_id(&self) -> &str {
        &self.observation_id
    }

    #[must_use]
    pub fn run_id(&self) -> &str {
        &self.run_id
    }

    #[must_use]
    pub const fn observed_at_unix_millis(&self) -> u64 {
        self.observed_at_unix_millis
    }

    pub fn evidence_hash(&self) -> Result<Box<str>, PaperSoakEvidenceError> {
        hash_serializable(self)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PaperSoakFaultKind {
    ModelTimeout,
    InvalidModelOutput,
    MarketDisconnect,
    Restart,
    ResourcePressure,
    EmergencyIndependence,
}

impl PaperSoakFaultKind {
    pub const ALL: [Self; 6] = [
        Self::ModelTimeout,
        Self::InvalidModelOutput,
        Self::MarketDisconnect,
        Self::Restart,
        Self::ResourcePressure,
        Self::EmergencyIndependence,
    ];
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PaperSoakFaultEvidence {
    fault_id: Box<str>,
    run_id: Box<str>,
    kind: PaperSoakFaultKind,
    injected_at_unix_millis: u64,
    observed_at_unix_millis: u64,
    fail_closed: bool,
    recovered: bool,
    unauthorized_order_effects: u64,
    unmanaged_sell_effects: u64,
    duplicate_business_effects: u64,
    audit_gaps: u64,
    local_ai_plan_mutations: u64,
    emergency_path_available_without_ai: bool,
}

impl PaperSoakFaultEvidence {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        fault_id: impl Into<Box<str>>,
        run_id: impl Into<Box<str>>,
        kind: PaperSoakFaultKind,
        injected_at_unix_millis: u64,
        observed_at_unix_millis: u64,
        fail_closed: bool,
        recovered: bool,
        unauthorized_order_effects: u64,
        unmanaged_sell_effects: u64,
        duplicate_business_effects: u64,
        audit_gaps: u64,
        local_ai_plan_mutations: u64,
        emergency_path_available_without_ai: bool,
    ) -> Result<Self, PaperSoakEvidenceError> {
        let value = Self {
            fault_id: fault_id.into(),
            run_id: run_id.into(),
            kind,
            injected_at_unix_millis,
            observed_at_unix_millis,
            fail_closed,
            recovered,
            unauthorized_order_effects,
            unmanaged_sell_effects,
            duplicate_business_effects,
            audit_gaps,
            local_ai_plan_mutations,
            emergency_path_available_without_ai,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), PaperSoakEvidenceError> {
        validate_label("fault_id", &self.fault_id)?;
        validate_label("run_id", &self.run_id)?;
        if self.observed_at_unix_millis < self.injected_at_unix_millis {
            return Err(PaperSoakEvidenceError::FaultTimeOutOfOrder);
        }
        Ok(())
    }

    #[must_use]
    pub fn fault_id(&self) -> &str {
        &self.fault_id
    }

    #[must_use]
    pub fn run_id(&self) -> &str {
        &self.run_id
    }

    #[must_use]
    pub const fn kind(&self) -> PaperSoakFaultKind {
        self.kind
    }

    #[must_use]
    pub const fn injected_at_unix_millis(&self) -> u64 {
        self.injected_at_unix_millis
    }

    #[must_use]
    pub const fn observed_at_unix_millis(&self) -> u64 {
        self.observed_at_unix_millis
    }

    pub fn evidence_hash(&self) -> Result<Box<str>, PaperSoakEvidenceError> {
        hash_serializable(self)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PaperSoakQualificationStatus {
    Collecting,
    Disqualified,
    Qualified,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PaperSoakPendingRequirement {
    Duration {
        observed_millis: u64,
        required_millis: u64,
    },
    FaultEvidence(PaperSoakFaultKind),
    AiManagedPositionEvidence,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PaperSoakViolation {
    ObservationBeforeRun,
    ObservationGap,
    CounterRegression,
    LlmCounterRegression,
    ProcessUnavailable,
    EmergencyUnavailable,
    StateDivergence,
    UnmanagedSell,
    DuplicateBusinessEffect,
    AuditGap,
    LocalAiPlanMutation,
    UnansweredManagedPositionReview,
    MemoryLimitExceeded,
    CpuLimitExceeded,
    MarketQueueLimitExceeded,
    CriticalQueueLimitExceeded,
    DatabaseLimitExceeded,
    DatabaseGrowthLimitExceeded,
    LlmCallLimitExceeded,
    LlmTokenLimitExceeded,
    LlmCostLimitExceeded,
    ReplanLimitExceeded,
    FaultNotFailClosed(PaperSoakFaultKind),
    FaultNotRecovered(PaperSoakFaultKind),
    FaultBusinessEffect(PaperSoakFaultKind),
    EmergencyNotIndependent(PaperSoakFaultKind),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PaperSoakQualificationReport {
    schema_version: Box<str>,
    run_id: Box<str>,
    status: PaperSoakQualificationStatus,
    observed_duration_millis: u64,
    observation_count: usize,
    fault_evidence_count: usize,
    covered_faults: BTreeSet<PaperSoakFaultKind>,
    pending_requirements: BTreeSet<PaperSoakPendingRequirement>,
    violations: BTreeSet<PaperSoakViolation>,
    peak_resident_memory_bytes: u64,
    peak_cpu_milli_percent: u32,
    peak_database_allocated_bytes: u64,
    final_database_business_rows: u64,
    evidence_hash: Box<str>,
}

impl PaperSoakQualificationReport {
    #[must_use]
    pub const fn status(&self) -> PaperSoakQualificationStatus {
        self.status
    }

    #[must_use]
    pub const fn observed_duration_millis(&self) -> u64 {
        self.observed_duration_millis
    }

    #[must_use]
    pub const fn observation_count(&self) -> usize {
        self.observation_count
    }

    #[must_use]
    pub const fn fault_evidence_count(&self) -> usize {
        self.fault_evidence_count
    }

    #[must_use]
    pub const fn pending_requirements(&self) -> &BTreeSet<PaperSoakPendingRequirement> {
        &self.pending_requirements
    }

    #[must_use]
    pub const fn violations(&self) -> &BTreeSet<PaperSoakViolation> {
        &self.violations
    }

    #[must_use]
    pub fn evidence_hash(&self) -> &str {
        &self.evidence_hash
    }
}

pub struct PaperSoakEvaluator;

impl PaperSoakEvaluator {
    pub fn evaluate(
        manifest: &PaperSoakManifest,
        observations: &[PaperSoakObservation],
        fault_evidence: &[PaperSoakFaultEvidence],
    ) -> Result<PaperSoakQualificationReport, PaperSoakEvidenceError> {
        manifest.validate()?;
        if observations.is_empty() || observations.len() > MAX_PAPER_SOAK_OBSERVATIONS {
            return Err(PaperSoakEvidenceError::ObservationCountOutOfRange);
        }
        if fault_evidence.len() > MAX_PAPER_SOAK_FAULT_EVIDENCE {
            return Err(PaperSoakEvidenceError::FaultEvidenceCountOutOfRange);
        }

        let mut observations = observations.to_vec();
        for observation in &observations {
            observation.validate()?;
            if observation.run_id() != manifest.run_id() {
                return Err(PaperSoakEvidenceError::RunIdMismatch);
            }
        }
        observations.sort_by(|left, right| {
            left.observed_at_unix_millis
                .cmp(&right.observed_at_unix_millis)
                .then_with(|| left.observation_id.cmp(&right.observation_id))
        });
        if observations
            .windows(2)
            .any(|pair| pair[0].observed_at_unix_millis == pair[1].observed_at_unix_millis)
        {
            return Err(PaperSoakEvidenceError::DuplicateObservationTime);
        }

        let mut faults = fault_evidence.to_vec();
        for evidence in &faults {
            evidence.validate()?;
            if evidence.run_id() != manifest.run_id() {
                return Err(PaperSoakEvidenceError::RunIdMismatch);
            }
        }
        faults.sort_by(|left, right| {
            left.kind
                .cmp(&right.kind)
                .then_with(|| {
                    left.injected_at_unix_millis
                        .cmp(&right.injected_at_unix_millis)
                })
                .then_with(|| left.fault_id.cmp(&right.fault_id))
        });

        let mut pending = BTreeSet::new();
        let mut violations = BTreeSet::new();
        let mut covered_faults = BTreeSet::new();
        let mut peak_resident_memory_bytes = 0;
        let mut peak_cpu_milli_percent = 0;
        let mut peak_database_allocated_bytes = 0;

        let first = &observations[0];
        let last = observations
            .last()
            .ok_or(PaperSoakEvidenceError::ObservationCountOutOfRange)?;
        if first.observed_at_unix_millis < manifest.started_at_unix_millis {
            violations.insert(PaperSoakViolation::ObservationBeforeRun);
        }
        if first
            .observed_at_unix_millis
            .saturating_sub(manifest.started_at_unix_millis)
            > manifest.maximum_observation_gap_millis
        {
            violations.insert(PaperSoakViolation::ObservationGap);
        }

        for pair in observations.windows(2) {
            if pair[1]
                .observed_at_unix_millis
                .saturating_sub(pair[0].observed_at_unix_millis)
                > manifest.maximum_observation_gap_millis
            {
                violations.insert(PaperSoakViolation::ObservationGap);
            }
            if counters_regressed(&pair[0].counters, &pair[1].counters) {
                violations.insert(PaperSoakViolation::CounterRegression);
            }
            if pair[1].llm.utc_day < pair[0].llm.utc_day
                || (pair[1].llm.utc_day == pair[0].llm.utc_day
                    && (pair[1].llm.calls_used_or_reserved < pair[0].llm.calls_used_or_reserved
                        || pair[1].llm.tokens_used_or_reserved
                            < pair[0].llm.tokens_used_or_reserved
                        || pair[1].llm.cost_used_or_reserved_usd
                            < pair[0].llm.cost_used_or_reserved_usd))
            {
                violations.insert(PaperSoakViolation::LlmCounterRegression);
            }
        }

        for observation in &observations {
            evaluate_observation(manifest, observation, &mut violations);
            peak_resident_memory_bytes =
                peak_resident_memory_bytes.max(observation.resources.resident_memory_bytes);
            peak_cpu_milli_percent =
                peak_cpu_milli_percent.max(observation.resources.cpu_milli_percent);
            peak_database_allocated_bytes =
                peak_database_allocated_bytes.max(observation.resources.database_allocated_bytes);
        }

        for evidence in &faults {
            if evidence.injected_at_unix_millis < manifest.started_at_unix_millis
                || evidence.observed_at_unix_millis > last.observed_at_unix_millis
            {
                return Err(PaperSoakEvidenceError::FaultOutsideObservedWindow);
            }
            if !evidence.fail_closed {
                violations.insert(PaperSoakViolation::FaultNotFailClosed(evidence.kind));
            }
            if !evidence.recovered {
                violations.insert(PaperSoakViolation::FaultNotRecovered(evidence.kind));
            }
            if evidence.unauthorized_order_effects != 0
                || evidence.unmanaged_sell_effects != 0
                || evidence.duplicate_business_effects != 0
                || evidence.audit_gaps != 0
                || evidence.local_ai_plan_mutations != 0
            {
                violations.insert(PaperSoakViolation::FaultBusinessEffect(evidence.kind));
            }
            if !evidence.emergency_path_available_without_ai {
                violations.insert(PaperSoakViolation::EmergencyNotIndependent(evidence.kind));
            }
            if evidence.fail_closed
                && evidence.recovered
                && evidence.unauthorized_order_effects == 0
                && evidence.unmanaged_sell_effects == 0
                && evidence.duplicate_business_effects == 0
                && evidence.audit_gaps == 0
                && evidence.local_ai_plan_mutations == 0
                && evidence.emergency_path_available_without_ai
            {
                covered_faults.insert(evidence.kind);
            }
        }

        let observed_duration_millis = last
            .observed_at_unix_millis
            .saturating_sub(manifest.started_at_unix_millis);
        if observed_duration_millis < manifest.required_duration_millis {
            pending.insert(PaperSoakPendingRequirement::Duration {
                observed_millis: observed_duration_millis,
                required_millis: manifest.required_duration_millis,
            });
        }
        for kind in PaperSoakFaultKind::ALL {
            if !covered_faults.contains(&kind) {
                pending.insert(PaperSoakPendingRequirement::FaultEvidence(kind));
            }
        }
        if last.counters.managed_position_reviews == 0 || last.counters.ai_management_actions == 0 {
            pending.insert(PaperSoakPendingRequirement::AiManagedPositionEvidence);
        }

        let status = if !violations.is_empty() {
            PaperSoakQualificationStatus::Disqualified
        } else if pending.is_empty() {
            PaperSoakQualificationStatus::Qualified
        } else {
            PaperSoakQualificationStatus::Collecting
        };
        let hash_material = (
            manifest,
            &observations,
            &faults,
            status,
            observed_duration_millis,
            &covered_faults,
            &pending,
            &violations,
        );
        let evidence_hash = hash_serializable(&hash_material)?;

        Ok(PaperSoakQualificationReport {
            schema_version: PAPER_SOAK_EVIDENCE_VERSION_V1.into(),
            run_id: manifest.run_id.clone(),
            status,
            observed_duration_millis,
            observation_count: observations.len(),
            fault_evidence_count: faults.len(),
            covered_faults,
            pending_requirements: pending,
            violations,
            peak_resident_memory_bytes,
            peak_cpu_milli_percent,
            peak_database_allocated_bytes,
            final_database_business_rows: last.resources.database_business_rows,
            evidence_hash,
        })
    }
}

fn evaluate_observation(
    manifest: &PaperSoakManifest,
    observation: &PaperSoakObservation,
    violations: &mut BTreeSet<PaperSoakViolation>,
) {
    let limits = &manifest.limits;
    let resources = observation.resources;
    let counters = observation.counters;
    let llm = observation.llm;

    if !observation.process_alive {
        violations.insert(PaperSoakViolation::ProcessUnavailable);
    }
    if !observation.emergency_path_available {
        violations.insert(PaperSoakViolation::EmergencyUnavailable);
    }
    if counters.state_divergences != 0 {
        violations.insert(PaperSoakViolation::StateDivergence);
    }
    if counters.unmanaged_sell_effects != 0 {
        violations.insert(PaperSoakViolation::UnmanagedSell);
    }
    if counters.duplicate_business_effects != 0 {
        violations.insert(PaperSoakViolation::DuplicateBusinessEffect);
    }
    if counters.audit_gaps != 0 {
        violations.insert(PaperSoakViolation::AuditGap);
    }
    if counters.local_ai_plan_mutations != 0 {
        violations.insert(PaperSoakViolation::LocalAiPlanMutation);
    }
    if counters.unanswered_managed_position_reviews != 0 {
        violations.insert(PaperSoakViolation::UnansweredManagedPositionReview);
    }
    if resources.resident_memory_bytes > limits.memory_soft_limit_bytes {
        violations.insert(PaperSoakViolation::MemoryLimitExceeded);
    }
    if resources.cpu_milli_percent > limits.cpu_capacity_milli_percent {
        violations.insert(PaperSoakViolation::CpuLimitExceeded);
    }
    if resources.market_queue_depth > limits.market_queue_capacity
        || resources.market_queue_high_watermark > limits.market_queue_capacity
    {
        violations.insert(PaperSoakViolation::MarketQueueLimitExceeded);
    }
    if resources.critical_queue_depth > limits.critical_queue_capacity
        || resources.critical_queue_high_watermark > limits.critical_queue_capacity
    {
        violations.insert(PaperSoakViolation::CriticalQueueLimitExceeded);
    }
    if resources.database_allocated_bytes > limits.maximum_database_bytes {
        violations.insert(PaperSoakViolation::DatabaseLimitExceeded);
    }
    let elapsed = observation
        .observed_at_unix_millis
        .saturating_sub(manifest.started_at_unix_millis);
    let elapsed_days = elapsed.div_ceil(MILLIS_PER_DAY);
    let growth_allowance = limits
        .maximum_database_growth_bytes_per_day
        .saturating_mul(elapsed_days);
    let growth_limit = limits
        .initial_database_bytes
        .saturating_add(growth_allowance)
        .min(limits.maximum_database_bytes);
    if resources.database_allocated_bytes > growth_limit {
        violations.insert(PaperSoakViolation::DatabaseGrowthLimitExceeded);
    }
    if llm.calls_used_or_reserved > limits.llm_daily_call_limit {
        violations.insert(PaperSoakViolation::LlmCallLimitExceeded);
    }
    if llm.tokens_used_or_reserved > limits.llm_daily_token_limit {
        violations.insert(PaperSoakViolation::LlmTokenLimitExceeded);
    }
    if llm.cost_used_or_reserved_usd > limits.llm_daily_cost_limit_usd {
        violations.insert(PaperSoakViolation::LlmCostLimitExceeded);
    }
    if llm.maximum_replans_observed_for_context > limits.maximum_replans_per_context {
        violations.insert(PaperSoakViolation::ReplanLimitExceeded);
    }
}

fn counters_regressed(previous: &PaperSoakSafetyCounters, next: &PaperSoakSafetyCounters) -> bool {
    next.state_divergences < previous.state_divergences
        || next.unmanaged_sell_effects < previous.unmanaged_sell_effects
        || next.duplicate_business_effects < previous.duplicate_business_effects
        || next.audit_gaps < previous.audit_gaps
        || next.local_ai_plan_mutations < previous.local_ai_plan_mutations
        || next.managed_position_reviews < previous.managed_position_reviews
        || next.ai_management_actions < previous.ai_management_actions
}

fn validate_label(field: &'static str, value: &str) -> Result<(), PaperSoakEvidenceError> {
    if value.is_empty()
        || value.len() > MAX_EVIDENCE_LABEL_LENGTH
        || !value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':' | b'/')
        })
    {
        return Err(PaperSoakEvidenceError::InvalidLabel { field });
    }
    Ok(())
}

fn hash_serializable<T: Serialize + ?Sized>(value: &T) -> Result<Box<str>, PaperSoakEvidenceError> {
    let bytes =
        serde_json::to_vec(value).map_err(|_| PaperSoakEvidenceError::SerializationFailed)?;
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(64);
    for byte in digest {
        use core::fmt::Write as _;
        write!(&mut output, "{byte:02x}")
            .map_err(|_| PaperSoakEvidenceError::SerializationFailed)?;
    }
    Ok(output.into_boxed_str())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PaperSoakEvidenceError {
    InvalidLabel { field: &'static str },
    InvalidLimits,
    InvalidResourceEvidence,
    InvalidLlmEvidence,
    UnsupportedManifest,
    ObservationCountOutOfRange,
    FaultEvidenceCountOutOfRange,
    DuplicateObservationTime,
    RunIdMismatch,
    FaultTimeOutOfOrder,
    FaultOutsideObservedWindow,
    SerializationFailed,
}

impl fmt::Display for PaperSoakEvidenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLabel { field } => write!(formatter, "{field} is invalid"),
            Self::InvalidLimits => formatter.write_str("paper soak limits are invalid"),
            Self::InvalidResourceEvidence => {
                formatter.write_str("paper soak resource evidence is invalid")
            }
            Self::InvalidLlmEvidence => formatter.write_str("paper soak LLM evidence is invalid"),
            Self::UnsupportedManifest => formatter.write_str("paper soak manifest is unsupported"),
            Self::ObservationCountOutOfRange => {
                formatter.write_str("paper soak observation count is out of range")
            }
            Self::FaultEvidenceCountOutOfRange => {
                formatter.write_str("paper soak fault evidence count is out of range")
            }
            Self::DuplicateObservationTime => {
                formatter.write_str("paper soak observations share a timestamp")
            }
            Self::RunIdMismatch => formatter.write_str("paper soak run ID does not match"),
            Self::FaultTimeOutOfOrder => formatter.write_str("fault evidence time is out of order"),
            Self::FaultOutsideObservedWindow => {
                formatter.write_str("fault evidence is outside the observed window")
            }
            Self::SerializationFailed => {
                formatter.write_str("paper soak evidence serialization failed")
            }
        }
    }
}

impl std::error::Error for PaperSoakEvidenceError {}
