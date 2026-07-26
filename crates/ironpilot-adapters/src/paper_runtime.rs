use core::fmt;
use core::future::Future;
use core::pin::Pin;
use std::collections::BTreeSet;

use ironpilot_application::{
    AiTradingRuntimeState, AuditEntry, ExecutionOrderIdSet, ExecutionValidationOutcome,
    PaperExecutionPolicy, PaperMarketObservation, PersistenceValidationError, SpotExecutionPort,
    SpotExecutionRequest, SpotExecutionRequestError, UnixMillis,
};
use ironpilot_domain::{
    AccountOrderFact, AiDecisionContext, AiDecisionContextId, AiRawResponse,
    AiTradePlanLedgerEntry, AiTradingAction, AiTradingPlan, ClosedCandle, DecisionContextError,
    DomainDecimal, InstrumentId, InstrumentRulesSnapshot, ManagedPosition, MarketFeatureSnapshot,
    PortfolioSnapshot, RuntimeInstanceId, SpotInstrumentRules, TopOfBook, TradePlanActionId,
    TradePlanId,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::deepseek::{
    DeepSeekAiTradingPlanProvider, DeepSeekAttemptEvidence, DeepSeekPlanGeneration,
    DeepSeekProviderError, DeepSeekProviderErrorKind,
};
use crate::persistence::{domain_timestamp, ensure_instance_lease, insert_audit};
use crate::{
    OwnedExecutionValidationFacts, PaperExecutionAdapterError, SqlitePaperExecutionPort,
    SqliteRepository, StorageError,
};

pub const PAPER_RUNTIME_VERSION_V1: &str = "ironpilot-ai-paper-runtime-v1";
pub const MAX_PAPER_RUNTIME_ATTEMPTS: usize = 2;
pub const MAX_PAPER_RUNTIME_OBSERVATIONS: usize = 10_000;

pub type PaperRuntimeProviderFuture<'a, T, E> =
    Pin<Box<dyn Future<Output = Result<T, E>> + Send + 'a>>;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PaperRuntimeCycleId(Uuid);

impl PaperRuntimeCycleId {
    pub fn new(value: Uuid) -> Result<Self, PaperRuntimeError> {
        if value.is_nil() {
            return Err(PaperRuntimeError::InvalidCycleId);
        }
        Ok(Self(value))
    }
}

impl fmt::Display for PaperRuntimeCycleId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaperRuntimeFacts {
    context_id: AiDecisionContextId,
    as_of_unix_millis: u64,
    primary_candles: Vec<ClosedCandle>,
    confirmation_candles: Vec<ClosedCandle>,
    top_of_book: TopOfBook,
    market_features: MarketFeatureSnapshot,
    instrument_rules: InstrumentRulesSnapshot,
    portfolio: PortfolioSnapshot,
    managed_positions: Vec<ManagedPosition>,
    open_orders: Vec<AccountOrderFact>,
    maximum_loss_quote: DomainDecimal,
    execution_rules: SpotInstrumentRules,
    provider_state: AiTradingRuntimeState,
}

impl PaperRuntimeFacts {
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        context_id: AiDecisionContextId,
        as_of_unix_millis: u64,
        primary_candles: Vec<ClosedCandle>,
        confirmation_candles: Vec<ClosedCandle>,
        top_of_book: TopOfBook,
        market_features: MarketFeatureSnapshot,
        instrument_rules: InstrumentRulesSnapshot,
        portfolio: PortfolioSnapshot,
        managed_positions: Vec<ManagedPosition>,
        open_orders: Vec<AccountOrderFact>,
        maximum_loss_quote: DomainDecimal,
        execution_rules: SpotInstrumentRules,
        provider_state: AiTradingRuntimeState,
    ) -> Self {
        Self {
            context_id,
            as_of_unix_millis,
            primary_candles,
            confirmation_candles,
            top_of_book,
            market_features,
            instrument_rules,
            portfolio,
            managed_positions,
            open_orders,
            maximum_loss_quote,
            execution_rules,
            provider_state,
        }
    }

    fn build_context(&self) -> Result<AiDecisionContext, DecisionContextError> {
        AiDecisionContext::new(
            self.context_id,
            self.as_of_unix_millis,
            self.primary_candles.clone(),
            self.confirmation_candles.clone(),
            self.top_of_book.clone(),
            self.market_features.clone(),
            &self.instrument_rules,
            &self.portfolio,
            self.managed_positions.clone(),
            self.open_orders.clone(),
            self.maximum_loss_quote,
        )
    }

    fn instrument_id(&self) -> &InstrumentId {
        self.market_features.instrument_id()
    }

    fn provider_state(&self) -> &AiTradingRuntimeState {
        &self.provider_state
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaperRuntimeActionAttempt {
    trade_plan_id: TradePlanId,
    action_id: TradePlanActionId,
    execution_order_ids: ExecutionOrderIdSet,
    validation_facts: OwnedExecutionValidationFacts,
    recorded_at_unix_millis: u64,
    submitted_at_unix_millis: u64,
}

impl PaperRuntimeActionAttempt {
    #[must_use]
    pub fn new(
        trade_plan_id: TradePlanId,
        action_id: TradePlanActionId,
        execution_order_ids: ExecutionOrderIdSet,
        validation_facts: OwnedExecutionValidationFacts,
        recorded_at_unix_millis: u64,
        submitted_at_unix_millis: u64,
    ) -> Self {
        Self {
            trade_plan_id,
            action_id,
            execution_order_ids,
            validation_facts,
            recorded_at_unix_millis,
            submitted_at_unix_millis,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaperRuntimeCycleInput {
    cycle_id: PaperRuntimeCycleId,
    started_at_unix_millis: u64,
    facts: PaperRuntimeFacts,
    attempts: Vec<PaperRuntimeActionAttempt>,
    observations: Vec<PaperMarketObservation>,
}

impl PaperRuntimeCycleInput {
    pub fn new(
        cycle_id: PaperRuntimeCycleId,
        started_at_unix_millis: u64,
        facts: PaperRuntimeFacts,
        attempts: Vec<PaperRuntimeActionAttempt>,
        observations: Vec<PaperMarketObservation>,
    ) -> Result<Self, PaperRuntimeError> {
        if attempts.is_empty() || attempts.len() > MAX_PAPER_RUNTIME_ATTEMPTS {
            return Err(PaperRuntimeError::AttemptCountOutOfRange);
        }
        if observations.len() > MAX_PAPER_RUNTIME_OBSERVATIONS {
            return Err(PaperRuntimeError::ObservationCountOutOfRange);
        }
        if observations
            .iter()
            .any(|observation| observation.instrument_id() != facts.instrument_id())
            || facts.execution_rules.instrument_id() != facts.instrument_id()
            || facts
                .provider_state()
                .active_trade_plans()
                .iter()
                .any(|fact| fact.instrument_id() != facts.instrument_id())
        {
            return Err(PaperRuntimeError::InstrumentMismatch);
        }
        if !facts.managed_positions.is_empty()
            && facts.provider_state().active_trade_plans().is_empty()
        {
            return Err(PaperRuntimeError::MissingManagedPlanState);
        }
        if observations
            .windows(2)
            .any(|pair| pair[0].observed_at_unix_millis() >= pair[1].observed_at_unix_millis())
        {
            return Err(PaperRuntimeError::ObservationOutOfOrder);
        }
        let mut observation_ids = BTreeSet::new();
        if observations.iter().any(|observation| {
            observation.source_generated_at_unix_millis() <= facts.as_of_unix_millis
                || !observation_ids.insert(observation.observation_id())
        }) {
            return Err(PaperRuntimeError::DecisionFactReuseOrDuplicateObservation);
        }
        let mut action_ids = BTreeSet::new();
        if attempts.iter().any(|attempt| {
            attempt.recorded_at_unix_millis < started_at_unix_millis
                || attempt.submitted_at_unix_millis < attempt.recorded_at_unix_millis
                || !action_ids.insert(attempt.action_id)
        }) {
            return Err(PaperRuntimeError::InvalidAttempt);
        }
        Ok(Self {
            cycle_id,
            started_at_unix_millis,
            facts,
            attempts,
            observations,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeAiGeneration {
    raw_response: AiRawResponse,
    plan: AiTradingPlan,
    evidence: Option<DeepSeekAttemptEvidence>,
}

impl RuntimeAiGeneration {
    #[must_use]
    pub const fn recorded(raw_response: AiRawResponse, plan: AiTradingPlan) -> Self {
        Self {
            raw_response,
            plan,
            evidence: None,
        }
    }

    fn from_deepseek(value: &DeepSeekPlanGeneration) -> Self {
        Self {
            raw_response: value.raw_response().clone(),
            plan: value.plan().clone(),
            evidence: Some(value.evidence().clone()),
        }
    }
}

pub trait PaperRuntimeProviderError: std::error::Error + Send + Sync {
    fn code(&self) -> &'static str;
    fn evidence(&self) -> Option<&DeepSeekAttemptEvidence>;
}

pub trait PaperRuntimeAiProvider: Send + Sync {
    type Error: PaperRuntimeProviderError;

    fn generate<'a>(
        &'a self,
        context: &'a AiDecisionContext,
        runtime_state: &'a AiTradingRuntimeState,
    ) -> PaperRuntimeProviderFuture<'a, RuntimeAiGeneration, Self::Error>;

    fn replan<'a>(
        &'a self,
        context: &'a AiDecisionContext,
        runtime_state: &'a AiTradingRuntimeState,
        rejected_plan: &'a AiTradingPlan,
        reasons: Vec<Box<str>>,
    ) -> PaperRuntimeProviderFuture<'a, RuntimeAiGeneration, Self::Error>;
}

impl PaperRuntimeAiProvider for DeepSeekAiTradingPlanProvider {
    type Error = DeepSeekProviderError;

    fn generate<'a>(
        &'a self,
        context: &'a AiDecisionContext,
        runtime_state: &'a AiTradingRuntimeState,
    ) -> PaperRuntimeProviderFuture<'a, RuntimeAiGeneration, Self::Error> {
        Box::pin(async move {
            self.generate_runtime_plan(context, runtime_state)
                .await
                .map(|generation| RuntimeAiGeneration::from_deepseek(&generation))
        })
    }

    fn replan<'a>(
        &'a self,
        context: &'a AiDecisionContext,
        runtime_state: &'a AiTradingRuntimeState,
        rejected_plan: &'a AiTradingPlan,
        reasons: Vec<Box<str>>,
    ) -> PaperRuntimeProviderFuture<'a, RuntimeAiGeneration, Self::Error> {
        Box::pin(async move {
            self.replan_runtime_after_rejection(context, runtime_state, rejected_plan, reasons)
                .await
                .map(|generation| RuntimeAiGeneration::from_deepseek(&generation))
        })
    }
}

impl PaperRuntimeProviderError for DeepSeekProviderError {
    fn code(&self) -> &'static str {
        match self.kind() {
            DeepSeekProviderErrorKind::InvalidConfiguration => "INVALID_CONFIGURATION",
            DeepSeekProviderErrorKind::InvalidPrompt => "INVALID_PROMPT",
            DeepSeekProviderErrorKind::ExpiredContext => "EXPIRED_CONTEXT",
            DeepSeekProviderErrorKind::ConcurrencyExhausted => "CONCURRENCY_EXHAUSTED",
            DeepSeekProviderErrorKind::CallBudgetExhausted => "CALL_BUDGET_EXHAUSTED",
            DeepSeekProviderErrorKind::TokenBudgetExhausted => "TOKEN_BUDGET_EXHAUSTED",
            DeepSeekProviderErrorKind::CostBudgetExhausted => "COST_BUDGET_EXHAUSTED",
            DeepSeekProviderErrorKind::ReplanLimitExceeded => "REPLAN_LIMIT_EXCEEDED",
            DeepSeekProviderErrorKind::ReplanProvenanceMismatch => "REPLAN_PROVENANCE_MISMATCH",
            DeepSeekProviderErrorKind::Timeout => "TIMEOUT",
            DeepSeekProviderErrorKind::Transport => "TRANSPORT",
            DeepSeekProviderErrorKind::Http => "HTTP",
            DeepSeekProviderErrorKind::ResponseTooLarge => "RESPONSE_TOO_LARGE",
            DeepSeekProviderErrorKind::InvalidResponse => "INVALID_RESPONSE",
            DeepSeekProviderErrorKind::EmptyOutput => "EMPTY_OUTPUT",
            DeepSeekProviderErrorKind::TruncatedOutput => "TRUNCATED_OUTPUT",
            DeepSeekProviderErrorKind::ProviderRefusal => "PROVIDER_REFUSAL",
            DeepSeekProviderErrorKind::InvalidPlan => "INVALID_PLAN",
            DeepSeekProviderErrorKind::BudgetAccounting => "BUDGET_ACCOUNTING",
            DeepSeekProviderErrorKind::Clock => "CLOCK",
        }
    }

    fn evidence(&self) -> Option<&DeepSeekAttemptEvidence> {
        self.evidence()
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PaperRuntimeOutcome {
    Executed,
    NoTrade,
    Hold,
    ValidationRejected,
    ProviderNoAction,
    ContextRejected,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PaperRuntimeEffect {
    Applied,
    DuplicateNoEffect,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PaperRuntimeCycleReport {
    schema_version: Box<str>,
    cycle_id: Box<str>,
    instrument_id: Box<str>,
    outcome: PaperRuntimeOutcome,
    effect: PaperRuntimeEffect,
    context_hash: Option<Box<str>>,
    runtime_state_hash: Box<str>,
    plan_hash: Option<Box<str>>,
    validation_hash: Option<Box<str>>,
    execution_request_hash: Option<Box<str>>,
    action: Option<Box<str>>,
    failure_code: Option<Box<str>>,
    fill_ids: Vec<Box<str>>,
    provider_attempts: u8,
    validation_attempts: u8,
    trace_events: u32,
    local_parameter_mutations: u8,
}

impl PaperRuntimeCycleReport {
    #[must_use]
    pub const fn outcome(&self) -> PaperRuntimeOutcome {
        self.outcome
    }

    #[must_use]
    pub const fn effect(&self) -> PaperRuntimeEffect {
        self.effect
    }

    #[must_use]
    pub fn context_hash(&self) -> Option<&str> {
        self.context_hash.as_deref()
    }

    #[must_use]
    pub fn runtime_state_hash(&self) -> &str {
        &self.runtime_state_hash
    }

    #[must_use]
    pub fn plan_hash(&self) -> Option<&str> {
        self.plan_hash.as_deref()
    }

    #[must_use]
    pub fn validation_hash(&self) -> Option<&str> {
        self.validation_hash.as_deref()
    }

    #[must_use]
    pub fn execution_request_hash(&self) -> Option<&str> {
        self.execution_request_hash.as_deref()
    }

    #[must_use]
    pub fn action(&self) -> Option<&str> {
        self.action.as_deref()
    }

    #[must_use]
    pub fn failure_code(&self) -> Option<&str> {
        self.failure_code.as_deref()
    }

    #[must_use]
    pub fn fill_ids(&self) -> &[Box<str>] {
        &self.fill_ids
    }

    #[must_use]
    pub const fn provider_attempts(&self) -> u8 {
        self.provider_attempts
    }

    #[must_use]
    pub const fn validation_attempts(&self) -> u8 {
        self.validation_attempts
    }

    #[must_use]
    pub const fn trace_events(&self) -> u32 {
        self.trace_events
    }

    #[must_use]
    pub const fn local_parameter_mutations(&self) -> u8 {
        self.local_parameter_mutations
    }

    #[must_use]
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("paper runtime report must serialize")
    }

    fn duplicate(mut self) -> Self {
        self.effect = PaperRuntimeEffect::DuplicateNoEffect;
        self
    }
}

pub struct SqliteAiPaperRuntime<'a, P> {
    repository: &'a SqliteRepository,
    owner_id: RuntimeInstanceId,
    provider: &'a P,
    paper_policy: PaperExecutionPolicy,
}

impl<'a, P> SqliteAiPaperRuntime<'a, P>
where
    P: PaperRuntimeAiProvider,
{
    #[must_use]
    pub const fn new(
        repository: &'a SqliteRepository,
        owner_id: RuntimeInstanceId,
        provider: &'a P,
        paper_policy: PaperExecutionPolicy,
    ) -> Self {
        Self {
            repository,
            owner_id,
            provider,
            paper_policy,
        }
    }

    pub async fn run_cycle(
        &self,
        input: &PaperRuntimeCycleInput,
    ) -> Result<PaperRuntimeCycleReport, PaperRuntimeError> {
        if let Some(report) = self.completed_report(input.cycle_id).await? {
            return Ok(report.duplicate());
        }
        if self.cycle_exists(input.cycle_id).await? {
            return Err(PaperRuntimeError::RecoveryRequired);
        }

        let context = match input.facts.build_context() {
            Ok(context) => context,
            Err(error) => {
                let mut sequence = 0;
                self.append_event(
                    input,
                    &mut sequence,
                    "CONTEXT_REJECTED",
                    input.started_at_unix_millis,
                    None,
                    json!({"error": error.to_string()}),
                )
                .await?;
                let report = base_report(
                    input,
                    PaperRuntimeOutcome::ContextRejected,
                    None,
                    Some("CONTEXT_REJECTED"),
                    0,
                    0,
                    sequence + 1,
                );
                self.complete(input, &mut sequence, input.started_at_unix_millis, &report)
                    .await?;
                return Ok(report);
            }
        };

        let mut sequence = 0;
        self.append_event(
            input,
            &mut sequence,
            "CONTEXT_BUILT",
            input.started_at_unix_millis,
            Some(&context),
            json!({
                "context_hash": context.context_hash().to_string(),
                "runtime_state_hash": input.facts.provider_state().state_hash().to_string(),
                "runtime_state": serde_json::from_str::<Value>(
                    input.facts.provider_state().to_json()
                ).expect("validated runtime state must serialize")
            }),
        )
        .await?;

        let mut rejected_plan = None;
        let mut rejection_reasons = Vec::new();
        let mut provider_attempts = 0_u8;
        let mut validation_attempts = 0_u8;

        for (index, attempt) in input.attempts.iter().enumerate() {
            provider_attempts = provider_attempts.saturating_add(1);
            let generation = if index == 0 {
                self.provider
                    .generate(&context, input.facts.provider_state())
                    .await
            } else {
                self.provider
                    .replan(
                        &context,
                        input.facts.provider_state(),
                        rejected_plan
                            .as_ref()
                            .expect("a replan attempt requires a rejected plan"),
                        rejection_reasons.clone(),
                    )
                    .await
            };
            let generation = match generation {
                Ok(generation) => generation,
                Err(error) => {
                    if let Some(evidence) = error.evidence() {
                        self.persist_provider_evidence(&context, evidence).await?;
                    }
                    self.append_event(
                        input,
                        &mut sequence,
                        "PROVIDER_NO_ACTION",
                        attempt.recorded_at_unix_millis,
                        Some(&context),
                        json!({"failure_code": error.code()}),
                    )
                    .await?;
                    let report = base_report(
                        input,
                        PaperRuntimeOutcome::ProviderNoAction,
                        Some(&context),
                        Some(error.code()),
                        provider_attempts,
                        validation_attempts,
                        sequence + 1,
                    );
                    self.complete(
                        input,
                        &mut sequence,
                        attempt.recorded_at_unix_millis,
                        &report,
                    )
                    .await?;
                    return Ok(report);
                }
            };
            if let Some(evidence) = &generation.evidence {
                self.persist_provider_evidence(&context, evidence).await?;
            }

            let entry = AiTradePlanLedgerEntry::new(
                context.clone(),
                generation.raw_response,
                generation.plan,
                attempt.trade_plan_id,
                attempt.action_id,
                attempt.recorded_at_unix_millis,
            )?;
            self.repository
                .persist_ai_trade_plan_ledger(
                    self.owner_id,
                    &entry,
                    &ledger_audit(input.cycle_id, &entry)?,
                )
                .await?;
            self.append_event(
                input,
                &mut sequence,
                "AI_PLAN_RECORDED",
                attempt.recorded_at_unix_millis,
                Some(&context),
                json!({
                    "action_id": entry.action_id().to_string(),
                    "trade_plan_id": entry.trade_plan_id().to_string(),
                    "plan_hash": entry.plan().plan_hash().to_string(),
                    "response_hash": entry.response().response_hash().to_string()
                }),
            )
            .await?;

            validation_attempts = validation_attempts.saturating_add(1);
            let decision = attempt.validation_facts.validate(&entry);
            self.repository
                .persist_execution_validation(
                    self.owner_id,
                    &decision,
                    &validation_audit(input.cycle_id, &decision)?,
                )
                .await?;
            self.append_event(
                input,
                &mut sequence,
                decision.outcome().as_str(),
                decision.validated_at_unix_millis(),
                Some(&context),
                json!({
                    "validation_hash": decision.validation_hash().to_string(),
                    "rejections": decision.rejections().iter().map(|reason| reason.code()).collect::<Vec<_>>()
                }),
            )
            .await?;

            if decision.outcome() == ExecutionValidationOutcome::Reject {
                rejected_plan = Some(entry.plan().clone());
                rejection_reasons = decision
                    .rejections()
                    .iter()
                    .map(|reason| {
                        format!("{}: {}", reason.code(), reason.feedback()).into_boxed_str()
                    })
                    .collect();
                if index + 1 < input.attempts.len() {
                    continue;
                }
                let mut report = base_report(
                    input,
                    PaperRuntimeOutcome::ValidationRejected,
                    Some(&context),
                    Some("VALIDATION_REJECTED"),
                    provider_attempts,
                    validation_attempts,
                    sequence + 1,
                );
                attach_plan_and_validation(&mut report, &entry, &decision);
                self.complete(
                    input,
                    &mut sequence,
                    decision.validated_at_unix_millis(),
                    &report,
                )
                .await?;
                return Ok(report);
            }

            let action = entry.plan().action();
            if matches!(action, AiTradingAction::NoTrade | AiTradingAction::Hold) {
                let mut report = base_report(
                    input,
                    if action == AiTradingAction::NoTrade {
                        PaperRuntimeOutcome::NoTrade
                    } else {
                        PaperRuntimeOutcome::Hold
                    },
                    Some(&context),
                    None,
                    provider_attempts,
                    validation_attempts,
                    sequence + 1,
                );
                attach_plan_and_validation(&mut report, &entry, &decision);
                self.complete(
                    input,
                    &mut sequence,
                    decision.validated_at_unix_millis(),
                    &report,
                )
                .await?;
                return Ok(report);
            }

            let request = SpotExecutionRequest::from_accepted_plan(
                &context,
                &decision,
                entry.plan(),
                attempt.execution_order_ids.clone(),
                attempt.submitted_at_unix_millis,
            )?;
            let paper =
                SqlitePaperExecutionPort::new(self.repository, self.owner_id, self.paper_policy);
            paper.submit(&request).await?;
            self.append_event(
                input,
                &mut sequence,
                "EXECUTION_SUBMITTED",
                attempt.submitted_at_unix_millis,
                Some(&context),
                json!({"execution_request_hash": request.request_hash().to_string()}),
            )
            .await?;

            let mut fill_ids = Vec::new();
            for observation in &input.observations {
                let paper_report = paper
                    .process_observation(observation, &input.facts.execution_rules)
                    .await?;
                let observation_fill_ids = paper_report
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
                self.append_event(
                    input,
                    &mut sequence,
                    "PAPER_OBSERVATION_APPLIED",
                    observation.observed_at_unix_millis(),
                    Some(&context),
                    json!({
                        "observation_id": observation.observation_id().to_string(),
                        "fill_ids": observation_fill_ids
                    }),
                )
                .await?;
            }
            let mut report = base_report(
                input,
                PaperRuntimeOutcome::Executed,
                Some(&context),
                None,
                provider_attempts,
                validation_attempts,
                sequence + 1,
            );
            attach_plan_and_validation(&mut report, &entry, &decision);
            report.execution_request_hash =
                Some(request.request_hash().to_string().into_boxed_str());
            report.fill_ids = fill_ids;
            let completed_at = input
                .observations
                .last()
                .map_or(attempt.submitted_at_unix_millis, |observation| {
                    observation.observed_at_unix_millis()
                });
            self.complete(input, &mut sequence, completed_at, &report)
                .await?;
            return Ok(report);
        }
        unreachable!("paper runtime input requires at least one attempt")
    }

    async fn complete(
        &self,
        input: &PaperRuntimeCycleInput,
        sequence: &mut u32,
        occurred_at: u64,
        report: &PaperRuntimeCycleReport,
    ) -> Result<(), PaperRuntimeError> {
        self.append_event(
            input,
            sequence,
            "COMPLETED",
            occurred_at,
            None,
            serde_json::to_value(report).expect("paper runtime report must serialize"),
        )
        .await
    }

    async fn persist_provider_evidence(
        &self,
        context: &AiDecisionContext,
        evidence: &DeepSeekAttemptEvidence,
    ) -> Result<(), PaperRuntimeError> {
        self.repository
            .persist_ai_provider_attempt(
                self.owner_id,
                context,
                evidence,
                &provider_audit(evidence)?,
            )
            .await?;
        Ok(())
    }

    async fn cycle_exists(&self, cycle_id: PaperRuntimeCycleId) -> Result<bool, PaperRuntimeError> {
        let exists: i64 = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM paper_runtime_events WHERE cycle_id = ?)",
        )
        .bind(cycle_id.to_string())
        .fetch_one(&self.repository.pool)
        .await?;
        Ok(exists == 1)
    }

    async fn completed_report(
        &self,
        cycle_id: PaperRuntimeCycleId,
    ) -> Result<Option<PaperRuntimeCycleReport>, PaperRuntimeError> {
        let payload: Option<String> = sqlx::query_scalar(
            "
            SELECT payload_json
            FROM paper_runtime_events
            WHERE cycle_id = ? AND event_type = 'COMPLETED'
            ORDER BY sequence DESC
            LIMIT 1
            ",
        )
        .bind(cycle_id.to_string())
        .fetch_optional(&self.repository.pool)
        .await?;
        payload
            .map(|payload| {
                let report: PaperRuntimeCycleReport = serde_json::from_str(&payload)
                    .map_err(|_| PaperRuntimeError::InvalidStoredReport)?;
                if report.schema_version.as_ref() != PAPER_RUNTIME_VERSION_V1
                    || report.cycle_id.as_ref() != cycle_id.to_string()
                {
                    return Err(PaperRuntimeError::InvalidStoredReport);
                }
                Ok(report)
            })
            .transpose()
    }

    #[allow(clippy::too_many_arguments)]
    async fn append_event(
        &self,
        input: &PaperRuntimeCycleInput,
        sequence: &mut u32,
        event_type: &str,
        occurred_at: u64,
        context: Option<&AiDecisionContext>,
        payload: Value,
    ) -> Result<(), PaperRuntimeError> {
        let payload_json =
            serde_json::to_string(&payload).expect("paper runtime event payload must serialize");
        let event_id = stable_uuid(
            "paper-runtime-event",
            &format!("{}:{sequence}", input.cycle_id),
        );
        let audit = AuditEntry::new(
            stable_audit_id(
                "paper-runtime-event",
                &format!("{}:{sequence}", input.cycle_id),
            )?,
            unix_millis(occurred_at)?,
            "PAPER_RUNTIME_EVENT",
            Some(event_id.to_string()),
            json!({
                "cycle_id": input.cycle_id.to_string(),
                "sequence": sequence,
                "event_type": event_type,
                "payload": payload
            }),
        )
        .map_err(PaperRuntimeError::InvalidAudit)?;
        let _write_guard = self.repository.write_gate.lock().await;
        let mut transaction = self.repository.pool.begin().await?;
        ensure_instance_lease(
            &mut transaction,
            self.owner_id,
            domain_timestamp(occurred_at)?,
        )
        .await?;
        sqlx::query(
            "
            INSERT INTO paper_runtime_events(
                event_id, cycle_id, sequence, instrument_id, context_id,
                event_type, occurred_at, payload_json
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            ",
        )
        .bind(event_id.to_string())
        .bind(input.cycle_id.to_string())
        .bind(i64::from(*sequence))
        .bind(input.facts.instrument_id().to_string())
        .bind(context.map(|context| context.context_id().to_string()))
        .bind(event_type)
        .bind(domain_timestamp(occurred_at)?)
        .bind(payload_json)
        .execute(&mut *transaction)
        .await?;
        insert_audit(&mut transaction, &audit).await?;
        transaction.commit().await?;
        *sequence = sequence.saturating_add(1);
        Ok(())
    }
}

fn base_report(
    input: &PaperRuntimeCycleInput,
    outcome: PaperRuntimeOutcome,
    context: Option<&AiDecisionContext>,
    failure_code: Option<&str>,
    provider_attempts: u8,
    validation_attempts: u8,
    trace_events: u32,
) -> PaperRuntimeCycleReport {
    PaperRuntimeCycleReport {
        schema_version: PAPER_RUNTIME_VERSION_V1.into(),
        cycle_id: input.cycle_id.to_string().into_boxed_str(),
        instrument_id: input.facts.instrument_id().to_string().into_boxed_str(),
        outcome,
        effect: PaperRuntimeEffect::Applied,
        context_hash: context.map(|context| context.context_hash().to_string().into_boxed_str()),
        runtime_state_hash: input
            .facts
            .provider_state()
            .state_hash()
            .to_string()
            .into_boxed_str(),
        plan_hash: None,
        validation_hash: None,
        execution_request_hash: None,
        action: None,
        failure_code: failure_code.map(Into::into),
        fill_ids: Vec::new(),
        provider_attempts,
        validation_attempts,
        trace_events,
        local_parameter_mutations: 0,
    }
}

fn attach_plan_and_validation(
    report: &mut PaperRuntimeCycleReport,
    entry: &AiTradePlanLedgerEntry,
    decision: &ironpilot_application::ExecutionValidationDecision,
) {
    report.plan_hash = Some(entry.plan().plan_hash().to_string().into_boxed_str());
    report.validation_hash = Some(decision.validation_hash().to_string().into_boxed_str());
    report.action = Some(entry.plan().action().as_str().into());
}

fn ledger_audit(
    cycle_id: PaperRuntimeCycleId,
    entry: &AiTradePlanLedgerEntry,
) -> Result<AuditEntry, PaperRuntimeError> {
    AuditEntry::new(
        stable_audit_id(
            "paper-runtime-ledger",
            &format!("{cycle_id}:{}", entry.action_id()),
        )?,
        unix_millis(entry.recorded_at_unix_millis())?,
        "AI_TRADE_PLAN_RECORDED",
        Some(entry.plan().plan_id().to_string()),
        entry.trace_json(),
    )
    .map_err(PaperRuntimeError::InvalidAudit)
}

fn validation_audit(
    cycle_id: PaperRuntimeCycleId,
    decision: &ironpilot_application::ExecutionValidationDecision,
) -> Result<AuditEntry, PaperRuntimeError> {
    AuditEntry::new(
        stable_audit_id(
            "paper-runtime-validation",
            &format!("{cycle_id}:{}", decision.action_id()),
        )?,
        unix_millis(decision.validated_at_unix_millis())?,
        "EXECUTION_VALIDATION_RECORDED",
        Some(decision.action_id().to_string()),
        serde_json::from_str(decision.evidence_json()).expect("validation evidence must be JSON"),
    )
    .map_err(PaperRuntimeError::InvalidAudit)
}

fn provider_audit(evidence: &DeepSeekAttemptEvidence) -> Result<AuditEntry, PaperRuntimeError> {
    let occurred_at = evidence
        .received_at_unix_millis()
        .unwrap_or(evidence.requested_at_unix_millis());
    AuditEntry::new(
        stable_audit_id("paper-runtime-provider", &evidence.attempt_id().to_string())?,
        unix_millis(occurred_at)?,
        "AI_PROVIDER_ATTEMPT_RECORDED",
        Some(evidence.attempt_id().to_string()),
        json!({
            "attempt_id": evidence.attempt_id().to_string(),
            "context_id": evidence.context_id().to_string(),
            "outcome": evidence.outcome().as_str(),
            "prompt_hash": evidence.prompt_hash()
        }),
    )
    .map_err(PaperRuntimeError::InvalidAudit)
}

fn stable_audit_id(
    namespace: &str,
    value: &str,
) -> Result<ironpilot_domain::AuditEntryId, PaperRuntimeError> {
    ironpilot_domain::AuditEntryId::new(stable_uuid(namespace, value))
        .map_err(|_| PaperRuntimeError::InvalidStableId)
}

fn stable_uuid(namespace: &str, value: &str) -> Uuid {
    let digest = Sha256::digest(format!("{namespace}:{value}").as_bytes());
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

fn unix_millis(value: u64) -> Result<UnixMillis, PaperRuntimeError> {
    let value = i64::try_from(value).map_err(|_| PaperRuntimeError::InvalidTimestamp)?;
    UnixMillis::new(value).map_err(PaperRuntimeError::InvalidAudit)
}

#[derive(Debug)]
pub enum PaperRuntimeError {
    InvalidCycleId,
    AttemptCountOutOfRange,
    ObservationCountOutOfRange,
    InstrumentMismatch,
    ObservationOutOfOrder,
    DecisionFactReuseOrDuplicateObservation,
    InvalidAttempt,
    MissingManagedPlanState,
    RecoveryRequired,
    InvalidStoredReport,
    InvalidStableId,
    InvalidTimestamp,
    Context(DecisionContextError),
    ExecutionRequest(SpotExecutionRequestError),
    InvalidAudit(PersistenceValidationError),
    Storage(StorageError),
    Paper(PaperExecutionAdapterError),
    Sqlx(sqlx::Error),
}

impl fmt::Display for PaperRuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidCycleId => formatter.write_str("paper runtime cycle ID is nil"),
            Self::AttemptCountOutOfRange => write!(
                formatter,
                "paper runtime attempt count is outside 1..={MAX_PAPER_RUNTIME_ATTEMPTS}"
            ),
            Self::ObservationCountOutOfRange => write!(
                formatter,
                "paper runtime observation count exceeds {MAX_PAPER_RUNTIME_OBSERVATIONS}"
            ),
            Self::InstrumentMismatch => {
                formatter.write_str("paper runtime input instruments do not match")
            }
            Self::ObservationOutOfOrder => {
                formatter.write_str("paper runtime observations are not strictly ordered")
            }
            Self::DecisionFactReuseOrDuplicateObservation => formatter.write_str(
                "paper runtime observations reuse decision facts or duplicate an observation ID",
            ),
            Self::InvalidAttempt => {
                formatter.write_str("paper runtime attempt IDs or timestamps are invalid")
            }
            Self::MissingManagedPlanState => formatter.write_str(
                "managed positions require the active TradePlan state in the provider input",
            ),
            Self::RecoveryRequired => formatter.write_str(
                "paper runtime cycle is incomplete; restore persisted facts before new AI work",
            ),
            Self::InvalidStoredReport => {
                formatter.write_str("stored paper runtime report is invalid")
            }
            Self::InvalidStableId => formatter.write_str("paper runtime stable ID is invalid"),
            Self::InvalidTimestamp => {
                formatter.write_str("paper runtime timestamp exceeds persistence range")
            }
            Self::Context(error) => write!(formatter, "{error}"),
            Self::ExecutionRequest(error) => write!(formatter, "{error}"),
            Self::InvalidAudit(error) => write!(formatter, "{error}"),
            Self::Storage(error) => write!(formatter, "{error}"),
            Self::Paper(error) => write!(formatter, "{error}"),
            Self::Sqlx(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for PaperRuntimeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Context(error) => Some(error),
            Self::ExecutionRequest(error) => Some(error),
            Self::InvalidAudit(error) => Some(error),
            Self::Storage(error) => Some(error),
            Self::Paper(error) => Some(error),
            Self::Sqlx(error) => Some(error),
            _ => None,
        }
    }
}

impl From<DecisionContextError> for PaperRuntimeError {
    fn from(error: DecisionContextError) -> Self {
        Self::Context(error)
    }
}

impl From<SpotExecutionRequestError> for PaperRuntimeError {
    fn from(error: SpotExecutionRequestError) -> Self {
        Self::ExecutionRequest(error)
    }
}

impl From<StorageError> for PaperRuntimeError {
    fn from(error: StorageError) -> Self {
        Self::Storage(error)
    }
}

impl From<PaperExecutionAdapterError> for PaperRuntimeError {
    fn from(error: PaperExecutionAdapterError) -> Self {
        Self::Paper(error)
    }
}

impl From<sqlx::Error> for PaperRuntimeError {
    fn from(error: sqlx::Error) -> Self {
        Self::Sqlx(error)
    }
}
