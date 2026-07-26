use core::fmt;
use std::collections::BTreeSet;

use ironpilot_application::{
    ActiveTradePlanFact, AuditEntry, ExecutionAuthorization, ExecutionOrderIdSet,
    ExecutionValidationOutcome, ExecutionValidationPolicy, ExecutionValidationRejection,
    ExecutionValidationRequest, ExecutionValidator, ManagedPositionExecutionFact,
    PaperExecutionPolicy, PaperMarketObservation, PersistenceValidationError, SpotExecutionPort,
    SpotExecutionRequest, SpotExecutionRequestError, SpotOrderPriceLimits, UnixMillis,
};
use ironpilot_domain::{
    AccountOrderFact, AiTradePlanLedgerEntry, AuditEntryId, DomainDecimal, InstrumentRulesSnapshot,
    PortfolioSnapshot, RuntimeInstanceId, SpotInstrumentRules,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{PaperExecutionAdapterError, SqlitePaperExecutionPort, SqliteRepository, StorageError};

pub const MINIMAL_HISTORICAL_HARNESS_VERSION_V1: &str = "ironpilot-minimal-historical-harness-v1";
pub const MAX_MINIMAL_HISTORICAL_OBSERVATIONS: usize = 10_000;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoricalValidationFacts {
    rules: InstrumentRulesSnapshot,
    portfolio: PortfolioSnapshot,
    managed_positions: Vec<ManagedPositionExecutionFact>,
    open_orders: Vec<AccountOrderFact>,
    active_trade_plans: Vec<ActiveTradePlanFact>,
    top_of_book: ironpilot_domain::TopOfBook,
    price_limits: SpotOrderPriceLimits,
    current_maximum_loss_quote: DomainDecimal,
    authorization: ExecutionAuthorization,
    policy: ExecutionValidationPolicy,
    validated_at_unix_millis: u64,
}

pub type OwnedExecutionValidationFacts = HistoricalValidationFacts;

impl HistoricalValidationFacts {
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        rules: InstrumentRulesSnapshot,
        portfolio: PortfolioSnapshot,
        managed_positions: Vec<ManagedPositionExecutionFact>,
        open_orders: Vec<AccountOrderFact>,
        active_trade_plans: Vec<ActiveTradePlanFact>,
        top_of_book: ironpilot_domain::TopOfBook,
        price_limits: SpotOrderPriceLimits,
        current_maximum_loss_quote: DomainDecimal,
        authorization: ExecutionAuthorization,
        policy: ExecutionValidationPolicy,
        validated_at_unix_millis: u64,
    ) -> Self {
        Self {
            rules,
            portfolio,
            managed_positions,
            open_orders,
            active_trade_plans,
            top_of_book,
            price_limits,
            current_maximum_loss_quote,
            authorization,
            policy,
            validated_at_unix_millis,
        }
    }

    fn request<'a>(&'a self, entry: &'a AiTradePlanLedgerEntry) -> ExecutionValidationRequest<'a> {
        ExecutionValidationRequest {
            action_id: entry.action_id(),
            trade_plan_id: entry.trade_plan_id(),
            context: entry.context(),
            plan: entry.plan(),
            rules: &self.rules,
            portfolio: &self.portfolio,
            managed_positions: &self.managed_positions,
            open_orders: &self.open_orders,
            active_trade_plans: &self.active_trade_plans,
            top_of_book: &self.top_of_book,
            price_limits: &self.price_limits,
            current_maximum_loss_quote: self.current_maximum_loss_quote,
            authorization: &self.authorization,
            policy: self.policy,
            validated_at_unix_millis: self.validated_at_unix_millis,
        }
    }

    #[must_use]
    pub fn validate(
        &self,
        entry: &AiTradePlanLedgerEntry,
    ) -> ironpilot_application::ExecutionValidationDecision {
        ExecutionValidator::validate(self.request(entry))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MinimalHistoricalReplayInput {
    entry: AiTradePlanLedgerEntry,
    validation_facts: HistoricalValidationFacts,
    execution_order_ids: ExecutionOrderIdSet,
    execution_submitted_at_unix_millis: u64,
    instrument_rules: SpotInstrumentRules,
    observations: Vec<PaperMarketObservation>,
}

impl MinimalHistoricalReplayInput {
    #[must_use]
    pub fn new(
        entry: AiTradePlanLedgerEntry,
        validation_facts: HistoricalValidationFacts,
        execution_order_ids: ExecutionOrderIdSet,
        execution_submitted_at_unix_millis: u64,
        instrument_rules: SpotInstrumentRules,
        observations: Vec<PaperMarketObservation>,
    ) -> Self {
        Self {
            entry,
            validation_facts,
            execution_order_ids,
            execution_submitted_at_unix_millis,
            instrument_rules,
            observations,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct HistoricalLedgerHash([u8; 32]);

impl fmt::Display for HistoricalLedgerHash {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum HistoricalLedgerRecordKind {
    Decision,
    Observation,
}

impl HistoricalLedgerRecordKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Decision => "DECISION",
            Self::Observation => "OBSERVATION",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoricalLedgerRecord {
    kind: HistoricalLedgerRecordKind,
    source_unix_millis: u64,
    record_hash: HistoricalLedgerHash,
    cumulative_hash: HistoricalLedgerHash,
}

impl HistoricalLedgerRecord {
    #[must_use]
    pub const fn kind(&self) -> HistoricalLedgerRecordKind {
        self.kind
    }

    #[must_use]
    pub const fn source_unix_millis(&self) -> u64 {
        self.source_unix_millis
    }

    #[must_use]
    pub const fn record_hash(&self) -> HistoricalLedgerHash {
        self.record_hash
    }

    #[must_use]
    pub const fn cumulative_hash(&self) -> HistoricalLedgerHash {
        self.cumulative_hash
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MinimalHistoricalReplayReport {
    context_hash: Box<str>,
    plan_hash: Box<str>,
    validation_hash: Box<str>,
    execution_request_hash: Box<str>,
    records: Vec<HistoricalLedgerRecord>,
    fill_ids: Vec<Box<str>>,
    ledger_hash: HistoricalLedgerHash,
}

impl MinimalHistoricalReplayReport {
    #[must_use]
    pub fn records(&self) -> &[HistoricalLedgerRecord] {
        &self.records
    }

    #[must_use]
    pub fn fill_ids(&self) -> &[Box<str>] {
        &self.fill_ids
    }

    #[must_use]
    pub const fn ledger_hash(&self) -> HistoricalLedgerHash {
        self.ledger_hash
    }

    #[must_use]
    pub fn to_json(&self) -> String {
        json!({
            "schema_version": MINIMAL_HISTORICAL_HARNESS_VERSION_V1,
            "context_hash": self.context_hash,
            "plan_hash": self.plan_hash,
            "validation_hash": self.validation_hash,
            "execution_request_hash": self.execution_request_hash,
            "records": self.records.iter().map(|record| json!({
                "kind": record.kind.as_str(),
                "source_unix_millis": record.source_unix_millis,
                "record_hash": record.record_hash.to_string(),
                "cumulative_hash": record.cumulative_hash.to_string()
            })).collect::<Vec<_>>(),
            "fill_ids": self.fill_ids,
            "ledger_hash": self.ledger_hash.to_string()
        })
        .to_string()
    }
}

pub struct SqliteMinimalHistoricalHarness<'a> {
    repository: &'a SqliteRepository,
    owner_id: RuntimeInstanceId,
    paper_policy: PaperExecutionPolicy,
}

impl<'a> SqliteMinimalHistoricalHarness<'a> {
    #[must_use]
    pub const fn new(
        repository: &'a SqliteRepository,
        owner_id: RuntimeInstanceId,
        paper_policy: PaperExecutionPolicy,
    ) -> Self {
        Self {
            repository,
            owner_id,
            paper_policy,
        }
    }

    pub async fn run(
        &self,
        input: &MinimalHistoricalReplayInput,
    ) -> Result<MinimalHistoricalReplayReport, MinimalHistoricalHarnessError> {
        validate_observation_prefix(input)?;

        let decision = input.validation_facts.validate(&input.entry);
        if decision.outcome() != ExecutionValidationOutcome::Accept {
            return Err(MinimalHistoricalHarnessError::ValidationRejected(
                decision.rejections().to_vec(),
            ));
        }
        let request = SpotExecutionRequest::from_accepted_plan(
            input.entry.context(),
            &decision,
            input.entry.plan(),
            input.execution_order_ids.clone(),
            input.execution_submitted_at_unix_millis,
        )?;
        let ledger_audit = ledger_audit(&input.entry)?;
        let validation_audit = validation_audit(&decision)?;

        self.repository
            .persist_ai_trade_plan_ledger(self.owner_id, &input.entry, &ledger_audit)
            .await?;
        self.repository
            .persist_execution_validation(self.owner_id, &decision, &validation_audit)
            .await?;

        let paper =
            SqlitePaperExecutionPort::new(self.repository, self.owner_id, self.paper_policy);
        paper.submit(&request).await?;

        let decision_payload = json!({
            "schema_version": MINIMAL_HISTORICAL_HARNESS_VERSION_V1,
            "context_hash": input.entry.context().context_hash().to_string(),
            "plan_hash": input.entry.plan().plan_hash().to_string(),
            "validation_hash": decision.validation_hash().to_string(),
            "execution_request_hash": request.request_hash().to_string()
        });
        let mut records = Vec::with_capacity(input.observations.len() + 1);
        push_record(
            &mut records,
            HistoricalLedgerRecordKind::Decision,
            input.execution_submitted_at_unix_millis,
            &decision_payload,
        );
        let mut fill_ids = Vec::new();
        for observation in &input.observations {
            let report = paper
                .process_observation(observation, &input.instrument_rules)
                .await?;
            let observation_fill_ids = report
                .fill_ids()
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>();
            fill_ids.extend(
                observation_fill_ids
                    .iter()
                    .cloned()
                    .map(String::into_boxed_str),
            );
            let payload = json!({
                "observation": serde_json::from_str::<Value>(&observation.payload_json())
                    .expect("validated paper observation must serialize"),
                "fill_ids": observation_fill_ids
            });
            push_record(
                &mut records,
                HistoricalLedgerRecordKind::Observation,
                observation.observed_at_unix_millis(),
                &payload,
            );
        }
        let ledger_hash = records
            .last()
            .expect("historical replay always has a decision record")
            .cumulative_hash();
        Ok(MinimalHistoricalReplayReport {
            context_hash: input
                .entry
                .context()
                .context_hash()
                .to_string()
                .into_boxed_str(),
            plan_hash: input.entry.plan().plan_hash().to_string().into_boxed_str(),
            validation_hash: decision.validation_hash().to_string().into_boxed_str(),
            execution_request_hash: request.request_hash().to_string().into_boxed_str(),
            records,
            fill_ids,
            ledger_hash,
        })
    }
}

fn validate_observation_prefix(
    input: &MinimalHistoricalReplayInput,
) -> Result<(), MinimalHistoricalHarnessError> {
    if input.observations.is_empty()
        || input.observations.len() > MAX_MINIMAL_HISTORICAL_OBSERVATIONS
    {
        return Err(MinimalHistoricalHarnessError::ObservationCountOutOfRange);
    }
    if input.instrument_rules.instrument_id() != input.entry.context().instrument_id() {
        return Err(MinimalHistoricalHarnessError::InstrumentMismatch);
    }
    let context_as_of = input.entry.context().as_of_unix_millis();
    let mut previous_observed_at = None;
    let mut observation_ids = BTreeSet::new();
    for observation in &input.observations {
        if observation.instrument_id() != input.entry.context().instrument_id() {
            return Err(MinimalHistoricalHarnessError::InstrumentMismatch);
        }
        if observation.source_generated_at_unix_millis() <= context_as_of {
            return Err(MinimalHistoricalHarnessError::DecisionFactReuse);
        }
        if previous_observed_at
            .is_some_and(|previous| observation.observed_at_unix_millis() <= previous)
        {
            return Err(MinimalHistoricalHarnessError::ObservationOutOfOrder);
        }
        if !observation_ids.insert(observation.observation_id()) {
            return Err(MinimalHistoricalHarnessError::DuplicateObservation);
        }
        previous_observed_at = Some(observation.observed_at_unix_millis());
    }
    Ok(())
}

fn push_record(
    records: &mut Vec<HistoricalLedgerRecord>,
    kind: HistoricalLedgerRecordKind,
    source_unix_millis: u64,
    payload: &Value,
) {
    let payload_json =
        serde_json::to_string(payload).expect("historical record payload must serialize");
    let record_hash = HistoricalLedgerHash(Sha256::digest(payload_json.as_bytes()).into());
    let mut cumulative = Sha256::new();
    cumulative.update(MINIMAL_HISTORICAL_HARNESS_VERSION_V1.as_bytes());
    if let Some(previous) = records.last() {
        cumulative.update(previous.cumulative_hash.0);
    }
    cumulative.update(record_hash.0);
    let cumulative_hash = HistoricalLedgerHash(cumulative.finalize().into());
    records.push(HistoricalLedgerRecord {
        kind,
        source_unix_millis,
        record_hash,
        cumulative_hash,
    });
}

fn ledger_audit(
    entry: &AiTradePlanLedgerEntry,
) -> Result<AuditEntry, MinimalHistoricalHarnessError> {
    AuditEntry::new(
        stable_audit_id("historical-ai-plan", &entry.action_id().to_string())?,
        unix_millis(entry.recorded_at_unix_millis())?,
        "AI_TRADE_PLAN_RECORDED",
        Some(entry.plan().plan_id().to_string()),
        entry.trace_json(),
    )
    .map_err(MinimalHistoricalHarnessError::InvalidAudit)
}

fn validation_audit(
    decision: &ironpilot_application::ExecutionValidationDecision,
) -> Result<AuditEntry, MinimalHistoricalHarnessError> {
    AuditEntry::new(
        stable_audit_id("historical-validation", &decision.action_id().to_string())?,
        unix_millis(decision.validated_at_unix_millis())?,
        "EXECUTION_VALIDATION_RECORDED",
        Some(decision.action_id().to_string()),
        serde_json::from_str(decision.evidence_json())
            .expect("validation evidence must be valid JSON"),
    )
    .map_err(MinimalHistoricalHarnessError::InvalidAudit)
}

fn stable_audit_id(
    namespace: &str,
    value: &str,
) -> Result<AuditEntryId, MinimalHistoricalHarnessError> {
    let digest = Sha256::digest(format!("{namespace}:{value}").as_bytes());
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    AuditEntryId::new(Uuid::from_bytes(bytes))
        .map_err(|_| MinimalHistoricalHarnessError::InvalidStableId)
}

fn unix_millis(value: u64) -> Result<UnixMillis, MinimalHistoricalHarnessError> {
    let value =
        i64::try_from(value).map_err(|_| MinimalHistoricalHarnessError::InvalidTimestamp)?;
    UnixMillis::new(value).map_err(MinimalHistoricalHarnessError::InvalidAudit)
}

#[derive(Debug)]
pub enum MinimalHistoricalHarnessError {
    InstrumentMismatch,
    DecisionFactReuse,
    ObservationOutOfOrder,
    DuplicateObservation,
    ObservationCountOutOfRange,
    ValidationRejected(Vec<ExecutionValidationRejection>),
    InvalidTimestamp,
    InvalidStableId,
    InvalidAudit(PersistenceValidationError),
    ExecutionRequest(SpotExecutionRequestError),
    Storage(StorageError),
    Paper(PaperExecutionAdapterError),
}

impl fmt::Display for MinimalHistoricalHarnessError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InstrumentMismatch => {
                formatter.write_str("historical input instruments do not match")
            }
            Self::DecisionFactReuse => formatter.write_str(
                "historical paper execution cannot reuse facts available to the AI decision",
            ),
            Self::ObservationOutOfOrder => {
                formatter.write_str("historical observations are not strictly ordered")
            }
            Self::DuplicateObservation => {
                formatter.write_str("historical observations contain a duplicate ID")
            }
            Self::ObservationCountOutOfRange => write!(
                formatter,
                "historical observation count is outside 1..={MAX_MINIMAL_HISTORICAL_OBSERVATIONS}"
            ),
            Self::ValidationRejected(rejections) => {
                formatter.write_str("recorded AI plan was rejected: ")?;
                for (index, rejection) in rejections.iter().enumerate() {
                    if index > 0 {
                        formatter.write_str(",")?;
                    }
                    formatter.write_str(rejection.code())?;
                }
                Ok(())
            }
            Self::InvalidTimestamp => {
                formatter.write_str("historical timestamp exceeds the persistence range")
            }
            Self::InvalidStableId => formatter.write_str("historical stable audit ID is invalid"),
            Self::InvalidAudit(error) => write!(formatter, "{error}"),
            Self::ExecutionRequest(error) => write!(formatter, "{error}"),
            Self::Storage(error) => write!(formatter, "{error}"),
            Self::Paper(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for MinimalHistoricalHarnessError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidAudit(error) => Some(error),
            Self::ExecutionRequest(error) => Some(error),
            Self::Storage(error) => Some(error),
            Self::Paper(error) => Some(error),
            _ => None,
        }
    }
}

impl From<SpotExecutionRequestError> for MinimalHistoricalHarnessError {
    fn from(error: SpotExecutionRequestError) -> Self {
        Self::ExecutionRequest(error)
    }
}

impl From<StorageError> for MinimalHistoricalHarnessError {
    fn from(error: StorageError) -> Self {
        Self::Storage(error)
    }
}

impl From<PaperExecutionAdapterError> for MinimalHistoricalHarnessError {
    fn from(error: PaperExecutionAdapterError) -> Self {
        Self::Paper(error)
    }
}
