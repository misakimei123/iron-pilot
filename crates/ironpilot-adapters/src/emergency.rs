use core::fmt;
use core::str::FromStr;
use std::collections::{BTreeMap, BTreeSet};

use ironpilot_application::{
    AuditEntry, AuthorizedEmergencyCommand, EmergencyActionState, EmergencyEffect,
    MAX_EMERGENCY_OBSERVATIONS, PaperExecutionError, PaperExecutionPolicy, PaperMarketObservation,
    PaperMatchingEngine, PaperOpenOrder, PaperOrderEvaluation, PlannedSpotOrder, UnixMillis,
};
use ironpilot_domain::{
    AccountOrderSide, AiOrderType, AiTimeInForce, AuditEntryId, DomainDecimal, EmergencyActionId,
    InstrumentId, OrderId, OrderIntentId, RuntimeInstanceId, TradePlanId,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::SqliteRepository;
use crate::persistence::{
    StorageError, consume_managed_lots_by_plan, domain_timestamp, ensure_instance_lease,
    insert_audit, normalized_decimal,
};

pub const EMERGENCY_CORE_VERSION_V1: &str = "ironpilot-emergency-core-v1";
pub const MAX_EMERGENCY_OBSERVATION_AGE_MILLIS: u64 = 10_000;

pub struct SqlitePaperEmergencyController<'a> {
    repository: &'a SqliteRepository,
    owner_id: RuntimeInstanceId,
    policy: PaperExecutionPolicy,
    max_slippage_quote: DomainDecimal,
}

impl<'a> SqlitePaperEmergencyController<'a> {
    pub fn new(
        repository: &'a SqliteRepository,
        owner_id: RuntimeInstanceId,
        policy: PaperExecutionPolicy,
        max_slippage_quote: DomainDecimal,
    ) -> Result<Self, EmergencyAdapterError> {
        if max_slippage_quote < DomainDecimal::ZERO {
            return Err(EmergencyAdapterError::InvalidPolicy);
        }
        Ok(Self {
            repository,
            owner_id,
            policy,
            max_slippage_quote,
        })
    }

    pub async fn execute(
        &self,
        command: &AuthorizedEmergencyCommand,
        now_unix_millis: u64,
        observations: &[PaperMarketObservation],
    ) -> Result<EmergencyExecutionReport, EmergencyAdapterError> {
        validate_observations(command, now_unix_millis, observations)?;
        let now = domain_timestamp(now_unix_millis)?;
        let _write_guard = self.repository.write_gate.lock().await;
        let mut transaction = self.repository.pool.begin().await?;
        ensure_instance_lease(&mut transaction, self.owner_id, now).await?;

        let existing: Option<(String, String)> = sqlx::query_as(
            "SELECT state, payload_json FROM emergency_actions WHERE emergency_action_id = ?",
        )
        .bind(command.action_id().to_string())
        .fetch_optional(&mut *transaction)
        .await?;
        let was_existing = existing.is_some();
        let previous_state = if let Some((state, payload)) = existing {
            if payload != command.payload_json() {
                return Err(StorageError::IdempotencyConflict.into());
            }
            let state = parse_state(&state)?;
            if state == EmergencyActionState::Completed {
                let remaining = remaining_managed_plan_count(&mut transaction).await?;
                transaction.commit().await?;
                return Ok(EmergencyExecutionReport {
                    action_id: command.action_id(),
                    state,
                    effect: EmergencyEffect::DuplicateNoEffect,
                    cancelled_orders: 0,
                    fill_ids: Vec::new(),
                    remaining_managed_plans: remaining,
                });
            }
            Some(state)
        } else {
            if !command.is_valid_at(now_unix_millis) {
                return Err(EmergencyAdapterError::CommandExpiredOrNotYetValid);
            }
            sqlx::query(
                "
                INSERT INTO emergency_actions(
                    emergency_action_id, state, requested_at, updated_at, payload_json
                )
                VALUES (?, 'REQUESTED', ?, ?, ?)
                ",
            )
            .bind(command.action_id().to_string())
            .bind(domain_timestamp(command.issued_at_unix_millis())?)
            .bind(now)
            .bind(command.payload_json())
            .execute(&mut *transaction)
            .await?;
            append_step(
                &mut transaction,
                command.action_id(),
                EmergencyActionState::Requested,
                now,
                &command.command_hash().to_string(),
                json!({"command_hash": command.command_hash().to_string()}),
            )
            .await?;
            None
        };

        let mut changed = !was_existing;
        if previous_state.is_none_or(|state| state == EmergencyActionState::Requested) {
            sqlx::query(
                "
                INSERT INTO system_state(singleton_id, state, updated_at)
                VALUES (1, 'HALTED', ?)
                ON CONFLICT(singleton_id) DO UPDATE SET
                    state = 'HALTED',
                    updated_at = MAX(system_state.updated_at, excluded.updated_at)
                ",
            )
            .bind(now)
            .execute(&mut *transaction)
            .await?;
            set_action_state(
                &mut transaction,
                command.action_id(),
                EmergencyActionState::EntryDisabled,
                now,
            )
            .await?;
            append_step(
                &mut transaction,
                command.action_id(),
                EmergencyActionState::EntryDisabled,
                now,
                &command.command_hash().to_string(),
                json!({"system_state": "HALTED", "automatic_resume": false}),
            )
            .await?;
            changed = true;
        }

        let state = current_state(&mut transaction, command.action_id()).await?;
        let mut cancelled_orders = 0;
        if matches!(
            state,
            EmergencyActionState::Requested | EmergencyActionState::EntryDisabled
        ) {
            let cancelled = sqlx::query(
                "
                UPDATE paper_orders
                SET state = 'CANCELLED', updated_at = ?
                WHERE order_id IN (SELECT order_id FROM paper_order_specs)
                  AND state IN ('STAGED', 'OPEN', 'ACTIVE', 'PARTIALLY_FILLED')
                ",
            )
            .bind(now)
            .execute(&mut *transaction)
            .await?;
            cancelled_orders = cancelled.rows_affected();
            sqlx::query(
                "
                UPDATE order_intents
                SET state = 'CANCELLED'
                WHERE order_intent_id IN (
                    SELECT paper_orders.order_intent_id
                    FROM paper_orders
                    JOIN paper_order_specs USING(order_id)
                    WHERE paper_orders.state = 'CANCELLED'
                )
                  AND state NOT IN ('FILLED', 'CANCELLED', 'REJECTED')
                ",
            )
            .execute(&mut *transaction)
            .await?;
            sqlx::query(
                "
                UPDATE trade_plan_actions
                SET state = 'RECOVERY_REQUIRED'
                WHERE action_id IN (SELECT action_id FROM paper_execution_submissions)
                  AND state NOT IN ('EXECUTED', 'REJECTED', 'CANCELLED', 'EXPIRED')
                ",
            )
            .execute(&mut *transaction)
            .await?;
            sqlx::query(
                "
                UPDATE trade_plans
                SET state = 'RECOVERY_REQUIRED', updated_at = ?
                WHERE state NOT IN ('REJECTED', 'CANCELLED', 'CLOSED')
                  AND EXISTS (
                    SELECT 1 FROM managed_lots
                    WHERE managed_lots.trade_plan_id = trade_plans.trade_plan_id
                      AND managed_lots.closed_at IS NULL
                  )
                ",
            )
            .bind(now)
            .execute(&mut *transaction)
            .await?;
            set_action_state(
                &mut transaction,
                command.action_id(),
                EmergencyActionState::OrdersCancelled,
                now,
            )
            .await?;
            append_step(
                &mut transaction,
                command.action_id(),
                EmergencyActionState::OrdersCancelled,
                now,
                &command.command_hash().to_string(),
                json!({"cancelled_owned_order_count": cancelled_orders}),
            )
            .await?;
            changed = true;
        }

        let mut fill_ids = Vec::new();
        let mut liquidity_by_instrument: BTreeMap<InstrumentId, DomainDecimal> = observations
            .iter()
            .map(|observation| {
                (
                    observation.instrument_id().clone(),
                    observation.available_base_liquidity(),
                )
            })
            .collect();
        let observations_by_instrument: BTreeMap<InstrumentId, &PaperMarketObservation> =
            observations
                .iter()
                .map(|observation| (observation.instrument_id().clone(), observation))
                .collect();
        let managed_plans = managed_plans(&mut transaction).await?;
        for plan in managed_plans {
            let Some(observation) = observations_by_instrument.get(&plan.instrument_id) else {
                continue;
            };
            let available_liquidity = liquidity_by_instrument
                .get(&plan.instrument_id)
                .copied()
                .unwrap_or(DomainDecimal::ZERO);
            if available_liquidity <= DomainDecimal::ZERO {
                continue;
            }
            let ids = emergency_order_ids(
                command.action_id(),
                plan.trade_plan_id,
                observation.observation_id(),
            )?;
            let expires_at = observation
                .observed_at_unix_millis()
                .checked_add(1)
                .ok_or(EmergencyAdapterError::InvalidObservation)?;
            let planned = PlannedSpotOrder::from_persisted(
                ids,
                ironpilot_application::ExecutionOrderRole::Exit,
                AccountOrderSide::Sell,
                AiOrderType::Market,
                Some(plan.quantity),
                None,
                None,
                Some(AiTimeInForce::Ioc),
                expires_at,
                self.max_slippage_quote,
            )?;
            let open = PaperOpenOrder::new(
                planned,
                plan.instrument_id.clone(),
                command.issued_at_unix_millis(),
                command.issued_at_unix_millis(),
                plan.quantity,
            )?;
            let matched = match PaperMatchingEngine::evaluate(
                &open,
                observation,
                available_liquidity,
                self.policy,
            ) {
                Ok(PaperOrderEvaluation::Fill(matched)) => matched,
                Ok(
                    PaperOrderEvaluation::NoFill
                    | PaperOrderEvaluation::Expired
                    | PaperOrderEvaluation::SlippageLimitExceeded,
                )
                | Err(PaperExecutionError::SlippageLimitExceeded) => continue,
                Err(error) => return Err(error.into()),
            };
            let fill_id = stable_uuid(
                "emergency-fill",
                &format!(
                    "{}:{}:{}",
                    command.action_id(),
                    plan.trade_plan_id,
                    observation.observation_id()
                ),
            );
            let payload = json!({
                "schema_version": EMERGENCY_CORE_VERSION_V1,
                "emergency_action_id": command.action_id().to_string(),
                "trade_plan_id": plan.trade_plan_id.to_string(),
                "instrument_id": plan.instrument_id.to_string(),
                "observation_id": observation.observation_id().to_string(),
                "side": "SELL",
                "base_quantity": normalized_decimal(matched.base_quantity()),
                "execution_price": normalized_decimal(matched.execution_price()),
                "quote_quantity": normalized_decimal(matched.quote_quantity()),
                "fee_quote": normalized_decimal(matched.fee_quote())
            });
            let insert = sqlx::query(
                "
                INSERT INTO emergency_fills(
                    emergency_fill_id, emergency_action_id, trade_plan_id, instrument_id,
                    observation_id, occurred_at, base_quantity, execution_price,
                    quote_quantity, fee_quote, payload_json
                )
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                ON CONFLICT(emergency_action_id, trade_plan_id, observation_id) DO NOTHING
                ",
            )
            .bind(fill_id.to_string())
            .bind(command.action_id().to_string())
            .bind(plan.trade_plan_id.to_string())
            .bind(plan.instrument_id.to_string())
            .bind(observation.observation_id().to_string())
            .bind(domain_timestamp(matched.occurred_at_unix_millis())?)
            .bind(normalized_decimal(matched.base_quantity()))
            .bind(normalized_decimal(matched.execution_price()))
            .bind(normalized_decimal(matched.quote_quantity()))
            .bind(normalized_decimal(matched.fee_quote()))
            .bind(payload.to_string())
            .execute(&mut *transaction)
            .await?;
            if insert.rows_affected() == 0 {
                continue;
            }
            consume_managed_lots_by_plan(
                &mut transaction,
                plan.trade_plan_id,
                matched.base_quantity(),
                domain_timestamp(matched.occurred_at_unix_millis())?,
            )
            .await?;
            let remaining = managed_quantity_by_plan(&mut transaction, plan.trade_plan_id).await?;
            if remaining == DomainDecimal::ZERO {
                sqlx::query(
                    "
                    UPDATE trade_plans
                    SET state = 'CLOSED', updated_at = ?
                    WHERE trade_plan_id = ?
                    ",
                )
                .bind(domain_timestamp(matched.occurred_at_unix_millis())?)
                .bind(plan.trade_plan_id.to_string())
                .execute(&mut *transaction)
                .await?;
            }
            liquidity_by_instrument.insert(
                plan.instrument_id,
                available_liquidity
                    .checked_sub(matched.base_quantity())
                    .ok_or(EmergencyAdapterError::ArithmeticFailure)?,
            );
            fill_ids.push(fill_id.to_string());
            changed = true;
        }

        let remaining_managed_plans = remaining_managed_plan_count(&mut transaction).await?;
        let final_state = if remaining_managed_plans == 0 {
            EmergencyActionState::Completed
        } else {
            EmergencyActionState::ExposureReducing
        };
        let state_before_finalize = current_state(&mut transaction, command.action_id()).await?;
        if state_before_finalize != final_state || !fill_ids.is_empty() {
            set_action_state(&mut transaction, command.action_id(), final_state, now).await?;
            let evidence = emergency_effect_hash(command, observations, remaining_managed_plans);
            append_step(
                &mut transaction,
                command.action_id(),
                final_state,
                now,
                &evidence,
                json!({
                    "fill_ids": fill_ids,
                    "remaining_managed_plans": remaining_managed_plans
                }),
            )
            .await?;
            changed = true;
        }
        transaction.commit().await?;
        Ok(EmergencyExecutionReport {
            action_id: command.action_id(),
            state: final_state,
            effect: if !changed {
                EmergencyEffect::DuplicateNoEffect
            } else if was_existing {
                EmergencyEffect::Resumed
            } else {
                EmergencyEffect::Applied
            },
            cancelled_orders,
            fill_ids,
            remaining_managed_plans,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmergencyExecutionReport {
    action_id: EmergencyActionId,
    state: EmergencyActionState,
    effect: EmergencyEffect,
    cancelled_orders: u64,
    fill_ids: Vec<String>,
    remaining_managed_plans: u64,
}

impl EmergencyExecutionReport {
    #[must_use]
    pub const fn action_id(&self) -> EmergencyActionId {
        self.action_id
    }

    #[must_use]
    pub const fn state(&self) -> EmergencyActionState {
        self.state
    }

    #[must_use]
    pub const fn effect(&self) -> EmergencyEffect {
        self.effect
    }

    #[must_use]
    pub const fn cancelled_orders(&self) -> u64 {
        self.cancelled_orders
    }

    #[must_use]
    pub fn fill_ids(&self) -> &[String] {
        &self.fill_ids
    }

    #[must_use]
    pub const fn remaining_managed_plans(&self) -> u64 {
        self.remaining_managed_plans
    }
}

struct ManagedPlan {
    trade_plan_id: TradePlanId,
    instrument_id: InstrumentId,
    quantity: DomainDecimal,
}

async fn managed_plans(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
) -> Result<Vec<ManagedPlan>, EmergencyAdapterError> {
    let rows: Vec<(String, String, String)> = sqlx::query_as(
        "
        SELECT trade_plan_id, instrument_id,
               json_extract(payload_json, '$.remaining_quantity')
        FROM managed_lots
        WHERE closed_at IS NULL
        ORDER BY instrument_id, trade_plan_id
        ",
    )
    .fetch_all(&mut **transaction)
    .await?;
    let mut totals: BTreeMap<(InstrumentId, TradePlanId), DomainDecimal> = BTreeMap::new();
    for (trade_plan_id, instrument_id, quantity) in rows {
        let trade_plan_id = TradePlanId::from_str(&trade_plan_id)
            .map_err(|_| EmergencyAdapterError::InvalidStoredState)?;
        let instrument_id = InstrumentId::from_str(&instrument_id)
            .map_err(|_| EmergencyAdapterError::InvalidStoredState)?;
        let quantity = DomainDecimal::from_str(&quantity)
            .map_err(|_| EmergencyAdapterError::InvalidStoredState)?;
        let total = totals
            .entry((instrument_id, trade_plan_id))
            .or_insert(DomainDecimal::ZERO);
        *total = total
            .checked_add(quantity)
            .ok_or(EmergencyAdapterError::ArithmeticFailure)?;
    }
    Ok(totals
        .into_iter()
        .map(|((instrument_id, trade_plan_id), quantity)| ManagedPlan {
            trade_plan_id,
            instrument_id,
            quantity,
        })
        .collect())
}

async fn managed_quantity_by_plan(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    trade_plan_id: TradePlanId,
) -> Result<DomainDecimal, EmergencyAdapterError> {
    let values: Vec<String> = sqlx::query_scalar(
        "
        SELECT json_extract(payload_json, '$.remaining_quantity')
        FROM managed_lots
        WHERE trade_plan_id = ? AND closed_at IS NULL
        ",
    )
    .bind(trade_plan_id.to_string())
    .fetch_all(&mut **transaction)
    .await?;
    values
        .into_iter()
        .try_fold(DomainDecimal::ZERO, |total, value| {
            total
                .checked_add(
                    DomainDecimal::from_str(&value)
                        .map_err(|_| EmergencyAdapterError::InvalidStoredState)?,
                )
                .ok_or(EmergencyAdapterError::ArithmeticFailure)
        })
}

async fn remaining_managed_plan_count(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
) -> Result<u64, EmergencyAdapterError> {
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(DISTINCT trade_plan_id) FROM managed_lots WHERE closed_at IS NULL",
    )
    .fetch_one(&mut **transaction)
    .await?;
    u64::try_from(count).map_err(|_| EmergencyAdapterError::InvalidStoredState)
}

async fn current_state(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    action_id: EmergencyActionId,
) -> Result<EmergencyActionState, EmergencyAdapterError> {
    let value: String =
        sqlx::query_scalar("SELECT state FROM emergency_actions WHERE emergency_action_id = ?")
            .bind(action_id.to_string())
            .fetch_one(&mut **transaction)
            .await?;
    parse_state(&value)
}

fn parse_state(value: &str) -> Result<EmergencyActionState, EmergencyAdapterError> {
    match value {
        "REQUESTED" => Ok(EmergencyActionState::Requested),
        "ENTRY_DISABLED" => Ok(EmergencyActionState::EntryDisabled),
        "ORDERS_CANCELLED" => Ok(EmergencyActionState::OrdersCancelled),
        "EXPOSURE_REDUCING" => Ok(EmergencyActionState::ExposureReducing),
        "COMPLETED" => Ok(EmergencyActionState::Completed),
        _ => Err(EmergencyAdapterError::InvalidStoredState),
    }
}

async fn set_action_state(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    action_id: EmergencyActionId,
    state: EmergencyActionState,
    now: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE emergency_actions SET state = ?, updated_at = MAX(updated_at, ?) WHERE emergency_action_id = ?",
    )
    .bind(state.as_str())
    .bind(now)
    .bind(action_id.to_string())
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn append_step(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    action_id: EmergencyActionId,
    state: EmergencyActionState,
    occurred_at: i64,
    evidence_hash: &str,
    payload: Value,
) -> Result<(), EmergencyAdapterError> {
    let insert = sqlx::query(
        "
        INSERT INTO emergency_action_steps(
            emergency_action_id, step, occurred_at, evidence_hash, payload_json
        )
        VALUES (?, ?, ?, ?, ?)
        ON CONFLICT(emergency_action_id, step, evidence_hash) DO NOTHING
        ",
    )
    .bind(action_id.to_string())
    .bind(state.as_str())
    .bind(occurred_at)
    .bind(evidence_hash)
    .bind(payload.to_string())
    .execute(&mut **transaction)
    .await?;
    if insert.rows_affected() == 1 {
        let audit_id = AuditEntryId::new(stable_uuid(
            "emergency-audit",
            &format!("{action_id}:{}:{evidence_hash}", state.as_str()),
        ))
        .map_err(|_| EmergencyAdapterError::InvalidStoredState)?;
        let audit = AuditEntry::new(
            audit_id,
            UnixMillis::new(occurred_at).map_err(|_| EmergencyAdapterError::InvalidStoredState)?,
            "emergency_core_step",
            Some(action_id.to_string()),
            json!({
                "core_version": EMERGENCY_CORE_VERSION_V1,
                "state": state.as_str(),
                "evidence_hash": evidence_hash,
                "effect": payload
            }),
        )
        .map_err(|_| EmergencyAdapterError::InvalidStoredState)?;
        insert_audit(transaction, &audit).await?;
    }
    Ok(())
}

fn validate_observations(
    command: &AuthorizedEmergencyCommand,
    now: u64,
    observations: &[PaperMarketObservation],
) -> Result<(), EmergencyAdapterError> {
    if observations.len() > MAX_EMERGENCY_OBSERVATIONS {
        return Err(EmergencyAdapterError::TooManyObservations);
    }
    let mut instruments = BTreeSet::new();
    for observation in observations {
        if !instruments.insert(observation.instrument_id().clone())
            || observation.observed_at_unix_millis() > now
            || now.saturating_sub(observation.observed_at_unix_millis())
                > MAX_EMERGENCY_OBSERVATION_AGE_MILLIS
            || observation.source_generated_at_unix_millis() <= command.issued_at_unix_millis()
        {
            return Err(EmergencyAdapterError::InvalidObservation);
        }
    }
    Ok(())
}

fn emergency_order_ids(
    action_id: EmergencyActionId,
    trade_plan_id: TradePlanId,
    observation_id: ironpilot_domain::SnapshotId,
) -> Result<ironpilot_application::ExecutionOrderIds, EmergencyAdapterError> {
    let seed = format!("{action_id}:{trade_plan_id}:{observation_id}");
    let intent = OrderIntentId::new(stable_uuid("emergency-intent", &seed))
        .map_err(|_| EmergencyAdapterError::InvalidStoredState)?;
    let order = OrderId::new(stable_uuid("emergency-order", &seed))
        .map_err(|_| EmergencyAdapterError::InvalidStoredState)?;
    Ok(ironpilot_application::ExecutionOrderIds::new(intent, order))
}

fn stable_uuid(namespace: &str, value: &str) -> Uuid {
    let mut hasher = Sha256::new();
    hasher.update(namespace.as_bytes());
    hasher.update([0]);
    hasher.update(value.as_bytes());
    let digest: [u8; 32] = hasher.finalize().into();
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

fn emergency_effect_hash(
    command: &AuthorizedEmergencyCommand,
    observations: &[PaperMarketObservation],
    remaining: u64,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(command.command_hash().to_string());
    for observation in observations {
        hasher.update(observation.observation_id().to_string());
    }
    hasher.update(remaining.to_be_bytes());
    let digest: [u8; 32] = hasher.finalize().into();
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[derive(Debug)]
pub enum EmergencyAdapterError {
    Storage(StorageError),
    Sqlx(sqlx::Error),
    Paper(PaperExecutionError),
    InvalidPolicy,
    CommandExpiredOrNotYetValid,
    TooManyObservations,
    InvalidObservation,
    InvalidStoredState,
    ArithmeticFailure,
}

impl fmt::Display for EmergencyAdapterError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Storage(error) => error.fmt(formatter),
            Self::Sqlx(error) => error.fmt(formatter),
            Self::Paper(error) => error.fmt(formatter),
            Self::InvalidPolicy => formatter.write_str("emergency execution policy is invalid"),
            Self::CommandExpiredOrNotYetValid => {
                formatter.write_str("new emergency command is expired or not yet valid")
            }
            Self::TooManyObservations => {
                formatter.write_str("emergency observation batch exceeds the fixed bound")
            }
            Self::InvalidObservation => {
                formatter.write_str("emergency market observation is stale, reused, or invalid")
            }
            Self::InvalidStoredState => {
                formatter.write_str("emergency persistence contains invalid state")
            }
            Self::ArithmeticFailure => formatter.write_str("emergency decimal arithmetic failed"),
        }
    }
}

impl std::error::Error for EmergencyAdapterError {}

impl From<StorageError> for EmergencyAdapterError {
    fn from(value: StorageError) -> Self {
        Self::Storage(value)
    }
}

impl From<sqlx::Error> for EmergencyAdapterError {
    fn from(value: sqlx::Error) -> Self {
        Self::Sqlx(value)
    }
}

impl From<PaperExecutionError> for EmergencyAdapterError {
    fn from(value: PaperExecutionError) -> Self {
        Self::Paper(value)
    }
}

#[cfg(test)]
mod tests {
    use core::str::FromStr;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    use ironpilot_application::{
        AuthorizedEmergencyCommand, EmergencyCommandKind, EmergencyEffect, PaperExecutionPolicy,
        PaperMarketObservation, UnixMillis,
    };
    use ironpilot_domain::{
        DomainDecimal, EmergencyActionId, InstrumentId, RuntimeInstanceId, SnapshotId,
    };

    use super::{EmergencyActionState, EmergencyAdapterError, SqlitePaperEmergencyController};
    use crate::{SqliteRepository, StorageError};

    static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);
    const OWNER: &str = "00000000-0000-0000-0000-000000000010";
    const PLAN: &str = "00000000-0000-0000-0000-000000000020";
    const ACTION: &str = "00000000-0000-0000-0000-000000000021";
    const OWNED_INTENT: &str = "00000000-0000-0000-0000-000000000022";
    const OWNED_ORDER: &str = "00000000-0000-0000-0000-000000000023";
    const UNOWNED_INTENT: &str = "00000000-0000-0000-0000-000000000024";
    const UNOWNED_ORDER: &str = "00000000-0000-0000-0000-000000000025";
    const LOT: &str = "00000000-0000-0000-0000-000000000026";
    const CONTEXT: &str = "00000000-0000-0000-0000-000000000027";
    const RESPONSE: &str = "00000000-0000-0000-0000-000000000028";
    const AI_PLAN: &str = "00000000-0000-0000-0000-000000000029";
    const EMERGENCY: &str = "00000000-0000-0000-0000-000000000030";

    struct Fixture {
        repository: SqliteRepository,
        path: PathBuf,
    }

    impl Fixture {
        async fn new() -> Self {
            let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir()
                .join(format!("ironpilot-p3-08-{}-{sequence}", std::process::id()));
            std::fs::create_dir(&path).expect("temp directory");
            let repository = SqliteRepository::connect(path.join("db.sqlite3"), 1)
                .await
                .expect("repository");
            repository
                .acquire_instance_lease(
                    RuntimeInstanceId::from_str(OWNER).expect("owner"),
                    UnixMillis::new(1_000).expect("timestamp"),
                    Duration::from_secs(10),
                )
                .await
                .expect("lease");
            Self { repository, path }
        }

        async fn seed_managed_plan_and_orders(&self) {
            let pool = &self.repository.pool;
            sqlx::query(
                "INSERT INTO trade_plans VALUES (?, 'bybit:spot:BTCUSDT', 'ACTIVE', 1000, 1000, '{}')",
            )
            .bind(PLAN)
            .execute(pool)
            .await
            .expect("plan");
            sqlx::query(
                "INSERT INTO trade_plan_actions VALUES (?, ?, 'OPEN_LONG', 'EXECUTED', 1000, 9000, '{}')",
            )
            .bind(ACTION)
            .bind(PLAN)
            .execute(pool)
            .await
            .expect("action");
            sqlx::query(
                "INSERT INTO ai_decision_contexts VALUES (?, 'v1', 'bybit:spot:BTCUSDT', 900, 9000, '10', 'context-hash', '{}')",
            )
            .bind(CONTEXT)
            .execute(pool)
            .await
            .expect("context");
            sqlx::query(
                "INSERT INTO ai_provider_responses VALUES (?, ?, 'provider', 'model', 950, 'response-hash', '{}')",
            )
            .bind(RESPONSE)
            .bind(CONTEXT)
            .execute(pool)
            .await
            .expect("response");
            sqlx::query(
                "INSERT INTO ai_trading_plans VALUES (?, ?, ?, 'v3', 'bybit:spot:BTCUSDT', 'OPEN_LONG', 1000, 9000, 'plan-hash', '{}')",
            )
            .bind(AI_PLAN)
            .bind(CONTEXT)
            .bind(RESPONSE)
            .execute(pool)
            .await
            .expect("ai plan");
            sqlx::query(
                "INSERT INTO execution_validations VALUES (?, ?, ?, 'v1', 'ACCEPT', 'context-hash', 'plan-hash', '5', '10', 1000, 'validation-hash', '{}')",
            )
            .bind(ACTION)
            .bind(PLAN)
            .bind(AI_PLAN)
            .execute(pool)
            .await
            .expect("validation");
            sqlx::query(
                "INSERT INTO paper_execution_submissions VALUES (?, ?, 'PAPER', 'OPEN_LONG', 'validation-hash', 'plan-hash', 'request-hash', 1000, '{}')",
            )
            .bind(ACTION)
            .bind(PLAN)
            .execute(pool)
            .await
            .expect("submission");
            for (intent, order) in [(OWNED_INTENT, OWNED_ORDER), (UNOWNED_INTENT, UNOWNED_ORDER)] {
                sqlx::query("INSERT INTO order_intents VALUES (?, ?, 'OPEN', 1000, '{}')")
                    .bind(intent)
                    .bind(ACTION)
                    .execute(pool)
                    .await
                    .expect("intent");
                sqlx::query("INSERT INTO paper_orders VALUES (?, ?, 'OPEN', 1000, 1000, '{}')")
                    .bind(order)
                    .bind(intent)
                    .execute(pool)
                    .await
                    .expect("order");
            }
            sqlx::query(
                "
                INSERT INTO paper_order_specs VALUES (
                    ?, ?, ?, 'bybit:spot:BTCUSDT', 'PROTECTIVE_STOP', NULL, 'SELL',
                    'MARKET', NULL, NULL, '190', NULL, 9000, '0', 900, 1000, '0', '0', '0'
                )
                ",
            )
            .bind(OWNED_ORDER)
            .bind(ACTION)
            .bind(PLAN)
            .execute(pool)
            .await
            .expect("owned order spec");
            sqlx::query(
                "
                INSERT INTO managed_lots VALUES (
                    ?, ?, 'bybit:spot:BTCUSDT', 1000, NULL,
                    '{\"schema_version\":\"ironpilot-managed-lot-v1\",\"base_asset\":\"BTC\",\"initial_quantity\":\"1\",\"remaining_quantity\":\"1\",\"source_fill_id\":\"source\"}'
                )
                ",
            )
            .bind(LOT)
            .bind(PLAN)
            .execute(pool)
            .await
            .expect("managed lot");
        }

        async fn close(self) {
            self.repository.close().await;
            std::fs::remove_dir_all(self.path).expect("remove temp directory");
        }
    }

    fn decimal(value: &str) -> DomainDecimal {
        DomainDecimal::from_str(value).expect("decimal")
    }

    fn command(action_id: &str, nonce: u8) -> AuthorizedEmergencyCommand {
        AuthorizedEmergencyCommand::new(
            EmergencyActionId::from_str(action_id).expect("action ID"),
            EmergencyCommandKind::CloseAllManagedExposure,
            "telegram-chat:42",
            [1; 32],
            [nonce; 32],
            1_500,
            2_000,
        )
        .expect("command")
    }

    fn observation(id: u128, at: u64, liquidity: &str) -> PaperMarketObservation {
        PaperMarketObservation::new(
            SnapshotId::from_str(&format!("00000000-0000-0000-0000-{id:012x}")).expect("snapshot"),
            InstrumentId::from_str("bybit:spot:BTCUSDT").expect("instrument"),
            at - 1,
            at,
            decimal("200"),
            decimal("201"),
            decimal("199"),
            decimal("202"),
            decimal(liquidity),
        )
        .expect("observation")
    }

    fn controller(repository: &SqliteRepository) -> SqlitePaperEmergencyController<'_> {
        SqlitePaperEmergencyController::new(
            repository,
            RuntimeInstanceId::from_str(OWNER).expect("owner"),
            PaperExecutionPolicy::new(decimal("0.001"), decimal("0.002"), decimal("0.001"))
                .expect("policy"),
            decimal("100"),
        )
        .expect("controller")
    }

    #[tokio::test]
    async fn emergency_is_owned_only_idempotent_and_restart_recoverable() {
        let fixture = Fixture::new().await;
        fixture.seed_managed_plan_and_orders().await;
        let command = command(EMERGENCY, 2);

        let first = controller(&fixture.repository)
            .execute(&command, 1_700, &[observation(1, 1_700, "0.4")])
            .await
            .expect("partial emergency execution");
        assert_eq!(first.state(), EmergencyActionState::ExposureReducing);
        assert_eq!(first.effect(), EmergencyEffect::Applied);
        assert_eq!(first.cancelled_orders(), 1);
        assert_eq!(first.fill_ids().len(), 1);
        assert_eq!(first.remaining_managed_plans(), 1);
        let system_state: String =
            sqlx::query_scalar("SELECT state FROM system_state WHERE singleton_id = 1")
                .fetch_one(&fixture.repository.pool)
                .await
                .expect("system state");
        assert_eq!(system_state, "HALTED");
        let unowned_state: String =
            sqlx::query_scalar("SELECT state FROM paper_orders WHERE order_id = ?")
                .bind(UNOWNED_ORDER)
                .fetch_one(&fixture.repository.pool)
                .await
                .expect("unowned order");
        assert_eq!(unowned_state, "OPEN");

        let duplicate = controller(&fixture.repository)
            .execute(&command, 1_700, &[observation(1, 1_700, "0.4")])
            .await
            .expect("duplicate");
        assert_eq!(duplicate.effect(), EmergencyEffect::DuplicateNoEffect);
        assert!(duplicate.fill_ids().is_empty());

        let completed = controller(&fixture.repository)
            .execute(&command, 2_500, &[observation(2, 2_500, "1")])
            .await
            .expect("restart after command expiry must resume");
        assert_eq!(completed.effect(), EmergencyEffect::Resumed);
        assert_eq!(completed.state(), EmergencyActionState::Completed);
        assert_eq!(completed.remaining_managed_plans(), 0);
        let lot: (Option<i64>, String) = sqlx::query_as(
            "SELECT closed_at, payload_json FROM managed_lots WHERE managed_lot_id = ?",
        )
        .bind(LOT)
        .fetch_one(&fixture.repository.pool)
        .await
        .expect("lot");
        assert_eq!(lot.0, Some(2_500));
        assert!(lot.1.contains("\"remaining_quantity\":\"0\""));
        let final_system_state: String =
            sqlx::query_scalar("SELECT state FROM system_state WHERE singleton_id = 1")
                .fetch_one(&fixture.repository.pool)
                .await
                .expect("system state");
        assert_eq!(final_system_state, "HALTED");
        assert!(
            sqlx::query("UPDATE emergency_fills SET base_quantity = '99'")
                .execute(&fixture.repository.pool)
                .await
                .is_err(),
            "emergency fills must be append-only"
        );
        assert!(
            sqlx::query("DELETE FROM emergency_action_steps")
                .execute(&fixture.repository.pool)
                .await
                .is_err(),
            "emergency steps must be append-only"
        );
        fixture.close().await;
    }

    #[tokio::test]
    async fn new_expired_commands_and_idempotency_conflicts_fail_closed() {
        let fixture = Fixture::new().await;
        let expired = controller(&fixture.repository)
            .execute(&command(EMERGENCY, 2), 2_500, &[])
            .await;
        assert!(matches!(
            expired,
            Err(EmergencyAdapterError::CommandExpiredOrNotYetValid)
        ));
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM emergency_actions")
            .fetch_one(&fixture.repository.pool)
            .await
            .expect("count");
        assert_eq!(count, 0);

        controller(&fixture.repository)
            .execute(&command(EMERGENCY, 2), 1_700, &[])
            .await
            .expect("first command");
        let conflict = controller(&fixture.repository)
            .execute(&command(EMERGENCY, 3), 1_700, &[])
            .await;
        assert!(matches!(
            conflict,
            Err(EmergencyAdapterError::Storage(
                StorageError::IdempotencyConflict
            ))
        ));
        fixture.close().await;
    }
}
