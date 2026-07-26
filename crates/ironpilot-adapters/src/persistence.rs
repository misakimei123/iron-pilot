use core::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use ironpilot_application::{
    AuditEntry, EXECUTION_VALIDATOR_VERSION_V1, ExecutionValidationDecision,
    ExecutionValidationOutcome, OutboxMessage, PersistedSystemState, SystemStateChange, UnixMillis,
};
use ironpilot_domain::{
    AI_DECISION_CONTEXT_SCHEMA_VERSION_V1, AiDecisionContext, AiRawResponse,
    AiTradePlanLedgerEntry, AssetCode, DomainDecimal, InstrumentId, ManagedPosition, PortfolioFill,
    PortfolioFillSide, PortfolioReconciliationStatus, PortfolioSnapshot, ReconciliationRunId,
    RuntimeInstanceId, SystemState, TradePlanActionId, TradePlanId, TradePlanLedgerDisposition,
};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::migrate::Migrator;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{Row, SqlitePool};
use tokio::sync::Mutex;

use crate::deepseek::{
    DEEPSEEK_PROVIDER_NAME, DeepSeekAttemptEvidence, DeepSeekAttemptOutcome, DeepSeekUsage,
};

static MIGRATOR: Migrator = sqlx::migrate!("./migrations");
const TRADING_RUNTIME_LOCK: &str = "trading-runtime";
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

pub struct SqliteRepository {
    pub(crate) pool: SqlitePool,
    database_path: PathBuf,
    pub(crate) write_gate: Mutex<()>,
}

impl SqliteRepository {
    pub async fn connect(
        database_path: impl AsRef<Path>,
        max_connections: u8,
    ) -> Result<Self, StorageError> {
        if max_connections == 0 || max_connections > 4 {
            return Err(StorageError::InvalidConnectionLimit { max_connections });
        }
        let database_path = database_path.as_ref().to_path_buf();
        if database_path.as_os_str().is_empty() {
            return Err(StorageError::InvalidDatabasePath);
        }

        let options = connection_options(&database_path, true, false);
        let pool = SqlitePoolOptions::new()
            .max_connections(u32::from(max_connections))
            .connect_with(options)
            .await?;
        MIGRATOR.run(&pool).await?;

        let repository = Self {
            pool,
            database_path,
            write_gate: Mutex::new(()),
        };
        repository.verify_runtime_pragmas().await?;
        Ok(repository)
    }

    #[must_use]
    pub fn database_path(&self) -> &Path {
        &self.database_path
    }

    pub async fn close(self) {
        self.pool.close().await;
    }

    pub async fn acquire_instance_lease(
        &self,
        owner_id: RuntimeInstanceId,
        acquired_at: UnixMillis,
        lease_duration: Duration,
    ) -> Result<InstanceLease, LeaseAcquireError> {
        let expires_at = checked_expiry(acquired_at, lease_duration)?;
        let _write_guard = self.write_gate.lock().await;
        let result = sqlx::query(
            "
            INSERT INTO runtime_instance_lease (
                lock_name, owner_id, acquired_at, expires_at
            )
            VALUES (?, ?, ?, ?)
            ON CONFLICT(lock_name) DO UPDATE SET
                owner_id = excluded.owner_id,
                acquired_at = excluded.acquired_at,
                expires_at = excluded.expires_at
            WHERE runtime_instance_lease.owner_id = excluded.owner_id
               OR runtime_instance_lease.expires_at <= excluded.acquired_at
            ",
        )
        .bind(TRADING_RUNTIME_LOCK)
        .bind(owner_id.to_string())
        .bind(acquired_at.get())
        .bind(expires_at.get())
        .execute(&self.pool)
        .await
        .map_err(StorageError::from)?;

        if result.rows_affected() == 1 {
            return Ok(InstanceLease {
                owner_id,
                acquired_at,
                expires_at,
            });
        }

        let holder = sqlx::query(
            "
            SELECT owner_id, expires_at
            FROM runtime_instance_lease
            WHERE lock_name = ?
            ",
        )
        .bind(TRADING_RUNTIME_LOCK)
        .fetch_one(&self.pool)
        .await
        .map_err(StorageError::from)?;
        Err(LeaseAcquireError::Held {
            owner_id: holder.try_get("owner_id").map_err(StorageError::from)?,
            expires_at: UnixMillis::new(holder.try_get("expires_at").map_err(StorageError::from)?)
                .map_err(|_| StorageError::InvalidStoredTimestamp)?,
        })
    }

    pub async fn renew_instance_lease(
        &self,
        lease: &InstanceLease,
        renewed_at: UnixMillis,
        lease_duration: Duration,
    ) -> Result<InstanceLease, StorageError> {
        let expires_at = checked_expiry(renewed_at, lease_duration).map_err(StorageError::from)?;
        let _write_guard = self.write_gate.lock().await;
        let result = sqlx::query(
            "
            UPDATE runtime_instance_lease
            SET acquired_at = ?, expires_at = ?
            WHERE lock_name = ?
              AND owner_id = ?
              AND expires_at > ?
            ",
        )
        .bind(renewed_at.get())
        .bind(expires_at.get())
        .bind(TRADING_RUNTIME_LOCK)
        .bind(lease.owner_id.to_string())
        .bind(renewed_at.get())
        .execute(&self.pool)
        .await?;
        if result.rows_affected() != 1 {
            return Err(StorageError::InstanceLeaseNotHeld);
        }
        Ok(InstanceLease {
            owner_id: lease.owner_id,
            acquired_at: renewed_at,
            expires_at,
        })
    }

    pub async fn release_instance_lease(&self, lease: &InstanceLease) -> Result<(), StorageError> {
        let _write_guard = self.write_gate.lock().await;
        let result = sqlx::query(
            "
            DELETE FROM runtime_instance_lease
            WHERE lock_name = ? AND owner_id = ?
            ",
        )
        .bind(TRADING_RUNTIME_LOCK)
        .bind(lease.owner_id.to_string())
        .execute(&self.pool)
        .await?;
        if result.rows_affected() != 1 {
            return Err(StorageError::InstanceLeaseNotHeld);
        }
        Ok(())
    }

    pub async fn persist_system_state_change(
        &self,
        owner_id: RuntimeInstanceId,
        change: &SystemStateChange,
    ) -> Result<(), StorageError> {
        let _write_guard = self.write_gate.lock().await;
        let mut transaction = self.pool.begin().await?;

        let lease_is_valid: i64 = sqlx::query_scalar(
            "
            SELECT EXISTS (
                SELECT 1
                FROM runtime_instance_lease
                WHERE lock_name = ?
                  AND owner_id = ?
                  AND expires_at > ?
            )
            ",
        )
        .bind(TRADING_RUNTIME_LOCK)
        .bind(owner_id.to_string())
        .bind(change.changed_at().get())
        .fetch_one(&mut *transaction)
        .await?;
        if lease_is_valid != 1 {
            return Err(StorageError::InstanceLeaseNotHeld);
        }

        let current: Option<String> = sqlx::query_scalar(
            "
            SELECT state
            FROM system_state
            WHERE singleton_id = 1
            ",
        )
        .fetch_optional(&mut *transaction)
        .await?;
        let current = current.as_deref().map(parse_system_state).transpose()?;
        if current != change.expected() {
            return Err(StorageError::SystemStateConflict {
                expected: change.expected(),
                actual: current,
            });
        }

        sqlx::query(
            "
            INSERT INTO system_state(singleton_id, state, updated_at)
            VALUES (1, ?, ?)
            ON CONFLICT(singleton_id) DO UPDATE SET
                state = excluded.state,
                updated_at = excluded.updated_at
            ",
        )
        .bind(system_state_text(change.next()))
        .bind(change.changed_at().get())
        .execute(&mut *transaction)
        .await?;

        insert_audit(&mut transaction, change.audit()).await?;
        if let Some(message) = change.outbox() {
            insert_outbox(&mut transaction, message).await?;
        }
        transaction.commit().await?;
        Ok(())
    }

    pub async fn system_state(&self) -> Result<Option<PersistedSystemState>, StorageError> {
        let row = sqlx::query(
            "
            SELECT state, updated_at
            FROM system_state
            WHERE singleton_id = 1
            ",
        )
        .fetch_optional(&self.pool)
        .await?;
        row.map(|row| {
            let state: String = row.try_get("state")?;
            let updated_at: i64 = row.try_get("updated_at")?;
            Ok(PersistedSystemState::new(
                parse_system_state(&state)?,
                UnixMillis::new(updated_at).map_err(|_| StorageError::InvalidStoredTimestamp)?,
            ))
        })
        .transpose()
    }

    pub async fn audit_entries(&self) -> Result<Vec<AuditRow>, StorageError> {
        let rows = sqlx::query(
            "
            SELECT sequence, audit_entry_id, occurred_at, category, subject_id, payload_json
            FROM audit_log
            ORDER BY sequence
            ",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                Ok(AuditRow {
                    sequence: row.try_get("sequence")?,
                    audit_entry_id: row.try_get("audit_entry_id")?,
                    occurred_at: UnixMillis::new(row.try_get("occurred_at")?)
                        .map_err(|_| StorageError::InvalidStoredTimestamp)?,
                    category: row.try_get("category")?,
                    subject_id: row.try_get("subject_id")?,
                    payload_json: row.try_get("payload_json")?,
                })
            })
            .collect()
    }

    pub async fn pending_outbox(&self, limit: u16) -> Result<Vec<PendingOutboxRow>, StorageError> {
        if limit == 0 || limit > 256 {
            return Err(StorageError::InvalidOutboxLimit { limit });
        }
        let rows = sqlx::query(
            "
            SELECT outbox_message_id, topic, payload_json, created_at, attempts
            FROM outbox
            WHERE published_at IS NULL
            ORDER BY created_at, outbox_message_id
            LIMIT ?
            ",
        )
        .bind(i64::from(limit))
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                Ok(PendingOutboxRow {
                    outbox_message_id: row.try_get("outbox_message_id")?,
                    topic: row.try_get("topic")?,
                    payload_json: row.try_get("payload_json")?,
                    created_at: UnixMillis::new(row.try_get("created_at")?)
                        .map_err(|_| StorageError::InvalidStoredTimestamp)?,
                    attempts: row.try_get("attempts")?,
                })
            })
            .collect()
    }

    pub async fn apply_portfolio_fill(
        &self,
        owner_id: RuntimeInstanceId,
        fill: &PortfolioFill,
        audit: &AuditEntry,
    ) -> Result<PersistenceEffect, StorageError> {
        let occurred_at = i64::try_from(fill.occurred_at_unix_millis())
            .map_err(|_| StorageError::InvalidStoredTimestamp)?;
        if audit.occurred_at().get() != occurred_at {
            return Err(StorageError::AtomicTimestampMismatch);
        }
        let fill_payload = portfolio_fill_json(fill);
        let fill_payload_text = fill_payload.to_string();
        let _write_guard = self.write_gate.lock().await;
        let mut transaction = self.pool.begin().await?;
        ensure_instance_lease(&mut transaction, owner_id, occurred_at).await?;

        let insert = sqlx::query(
            "
            INSERT INTO fills(fill_id, order_id, occurred_at, payload_json)
            VALUES (?, ?, ?, ?)
            ON CONFLICT(fill_id) DO NOTHING
            ",
        )
        .bind(fill.fill_id().to_string())
        .bind(fill.order_id().to_string())
        .bind(occurred_at)
        .bind(&fill_payload_text)
        .execute(&mut *transaction)
        .await?;
        if insert.rows_affected() == 0 {
            let existing: (String, i64, String) = sqlx::query_as(
                "
                SELECT order_id, occurred_at, payload_json
                FROM fills
                WHERE fill_id = ?
                ",
            )
            .bind(fill.fill_id().to_string())
            .fetch_one(&mut *transaction)
            .await?;
            if existing.0 != fill.order_id().to_string()
                || existing.1 != occurred_at
                || existing.2 != fill_payload_text
            {
                return Err(StorageError::IdempotencyConflict);
            }
            transaction.commit().await?;
            return Ok(PersistenceEffect::DuplicateNoEffect);
        }

        match fill.side() {
            PortfolioFillSide::Buy => {
                insert_managed_lot(&mut transaction, fill, occurred_at).await?;
            }
            PortfolioFillSide::Sell => {
                consume_managed_lots(&mut transaction, fill, occurred_at).await?;
            }
        }
        insert_audit(&mut transaction, audit).await?;
        transaction.commit().await?;
        Ok(PersistenceEffect::Applied)
    }

    pub async fn managed_position(
        &self,
        instrument_id: &InstrumentId,
        base_asset: AssetCode,
    ) -> Result<ManagedPosition, StorageError> {
        let rows: Vec<String> = sqlx::query_scalar(
            "
            SELECT payload_json
            FROM managed_lots
            WHERE instrument_id = ? AND closed_at IS NULL
            ORDER BY opened_at, managed_lot_id
            ",
        )
        .bind(instrument_id.to_string())
        .fetch_all(&self.pool)
        .await?;
        let mut quantity = DomainDecimal::ZERO;
        for payload in rows {
            let lot = parse_managed_lot(&payload)?;
            if lot.base_asset != base_asset.as_str() {
                return Err(StorageError::ManagedAssetMismatch);
            }
            quantity = quantity
                .checked_add(lot.remaining_quantity)
                .ok_or(StorageError::PortfolioArithmeticOverflow)?;
        }
        ManagedPosition::new(instrument_id.clone(), base_asset, quantity)
            .map_err(StorageError::Portfolio)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn persist_portfolio_reconciliation(
        &self,
        owner_id: RuntimeInstanceId,
        reconciliation_run_id: ReconciliationRunId,
        started_at: UnixMillis,
        completed_at: UnixMillis,
        snapshot: &PortfolioSnapshot,
        audit: &AuditEntry,
    ) -> Result<PersistenceEffect, StorageError> {
        if completed_at < started_at
            || audit.occurred_at() != completed_at
            || u64::try_from(completed_at.get()).ok() != Some(snapshot.observed_at_unix_millis())
        {
            return Err(StorageError::AtomicTimestampMismatch);
        }
        let outcome = match snapshot.status() {
            PortfolioReconciliationStatus::Balanced => "BALANCED",
            PortfolioReconciliationStatus::BalanceDifference => "BALANCE_DIFFERENCE",
        };
        let payload = portfolio_snapshot_json(snapshot);
        let _write_guard = self.write_gate.lock().await;
        let mut transaction = self.pool.begin().await?;
        ensure_instance_lease(&mut transaction, owner_id, completed_at.get()).await?;
        let insert = sqlx::query(
            "
            INSERT INTO reconciliation_runs(
                reconciliation_run_id, started_at, completed_at, outcome, payload_json
            )
            VALUES (?, ?, ?, ?, ?)
            ON CONFLICT(reconciliation_run_id) DO NOTHING
            ",
        )
        .bind(reconciliation_run_id.to_string())
        .bind(started_at.get())
        .bind(completed_at.get())
        .bind(outcome)
        .bind(payload.to_string())
        .execute(&mut *transaction)
        .await?;
        if insert.rows_affected() == 0 {
            let existing: (i64, i64, String, String) = sqlx::query_as(
                "
                SELECT started_at, completed_at, outcome, payload_json
                FROM reconciliation_runs
                WHERE reconciliation_run_id = ?
                ",
            )
            .bind(reconciliation_run_id.to_string())
            .fetch_one(&mut *transaction)
            .await?;
            if existing
                != (
                    started_at.get(),
                    completed_at.get(),
                    outcome.to_owned(),
                    payload.to_string(),
                )
            {
                return Err(StorageError::IdempotencyConflict);
            }
            transaction.commit().await?;
            return Ok(PersistenceEffect::DuplicateNoEffect);
        }
        insert_audit(&mut transaction, audit).await?;
        transaction.commit().await?;
        Ok(PersistenceEffect::Applied)
    }

    pub async fn persist_ai_trade_plan_ledger(
        &self,
        owner_id: RuntimeInstanceId,
        entry: &AiTradePlanLedgerEntry,
        audit: &AuditEntry,
    ) -> Result<PersistenceEffect, StorageError> {
        let recorded_at = domain_timestamp(entry.recorded_at_unix_millis())?;
        if audit.occurred_at().get() != recorded_at {
            return Err(StorageError::AtomicTimestampMismatch);
        }
        let _write_guard = self.write_gate.lock().await;
        let mut transaction = self.pool.begin().await?;
        ensure_instance_lease(&mut transaction, owner_id, recorded_at).await?;
        ensure_ai_context(&mut transaction, entry.context()).await?;
        ensure_ai_response(&mut transaction, entry.response()).await?;

        let plan = entry.plan();
        let plan_payload = plan.to_json();
        let plan_insert = sqlx::query(
            "
            INSERT INTO ai_trading_plans(
                ai_plan_id, context_id, response_id, schema_version, instrument_id,
                action, created_at, valid_until, plan_hash, payload_json
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(ai_plan_id) DO NOTHING
            ",
        )
        .bind(plan.plan_id().to_string())
        .bind(plan.context_id().to_string())
        .bind(entry.response().response_id().to_string())
        .bind(plan.schema_version())
        .bind(plan.instrument_id().to_string())
        .bind(plan.action().as_str())
        .bind(recorded_at)
        .bind(domain_timestamp(plan.valid_until_unix_millis())?)
        .bind(plan.plan_hash().to_string())
        .bind(&plan_payload)
        .execute(&mut *transaction)
        .await?;
        if plan_insert.rows_affected() == 0 {
            ensure_existing_ai_ledger_matches(&mut transaction, entry).await?;
            transaction.commit().await?;
            return Ok(PersistenceEffect::DuplicateNoEffect);
        }

        let trace_payload = entry.trace_json().to_string();
        match entry.disposition() {
            TradePlanLedgerDisposition::Create { initial_state } => {
                if initial_state == ironpilot_domain::TradePlanState::Proposed {
                    let existing: Option<String> = sqlx::query_scalar(
                        "
                        SELECT trade_plan_id
                        FROM trade_plans
                        WHERE instrument_id = ?
                          AND state NOT IN ('REJECTED', 'CANCELLED', 'CLOSED')
                        LIMIT 1
                        ",
                    )
                    .bind(plan.instrument_id().to_string())
                    .fetch_optional(&mut *transaction)
                    .await?;
                    if let Some(existing_trade_plan_id) = existing {
                        return Err(StorageError::ActiveTradePlanExists {
                            instrument_id: plan.instrument_id().to_string().into_boxed_str(),
                            trade_plan_id: existing_trade_plan_id.into_boxed_str(),
                        });
                    }
                }
                sqlx::query(
                    "
                    INSERT INTO trade_plans(
                        trade_plan_id, instrument_id, state, created_at, updated_at, payload_json
                    )
                    VALUES (?, ?, ?, ?, ?, ?)
                    ",
                )
                .bind(entry.trade_plan_id().to_string())
                .bind(plan.instrument_id().to_string())
                .bind(initial_state.as_str())
                .bind(recorded_at)
                .bind(recorded_at)
                .bind(&trace_payload)
                .execute(&mut *transaction)
                .await?;
            }
            TradePlanLedgerDisposition::AppendToExisting => {
                let target: Option<(String, String)> = sqlx::query_as(
                    "
                    SELECT instrument_id, state
                    FROM trade_plans
                    WHERE trade_plan_id = ?
                    ",
                )
                .bind(entry.trade_plan_id().to_string())
                .fetch_optional(&mut *transaction)
                .await?;
                let Some((instrument_id, state)) = target else {
                    return Err(StorageError::TargetTradePlanUnavailable {
                        trade_plan_id: entry.trade_plan_id().to_string().into_boxed_str(),
                    });
                };
                if instrument_id != plan.instrument_id().to_string()
                    || matches!(state.as_str(), "REJECTED" | "CANCELLED" | "CLOSED")
                {
                    return Err(StorageError::TargetTradePlanUnavailable {
                        trade_plan_id: entry.trade_plan_id().to_string().into_boxed_str(),
                    });
                }
            }
        }

        sqlx::query(
            "
            INSERT INTO trade_plan_actions(
                action_id, trade_plan_id, action_type, state, created_at, expires_at, payload_json
            )
            VALUES (?, ?, ?, 'RECORDED', ?, ?, ?)
            ",
        )
        .bind(entry.action_id().to_string())
        .bind(entry.trade_plan_id().to_string())
        .bind(plan.action().as_str())
        .bind(recorded_at)
        .bind(domain_timestamp(plan.valid_until_unix_millis())?)
        .bind(&trace_payload)
        .execute(&mut *transaction)
        .await?;

        sqlx::query(
            "
            INSERT INTO ai_trade_plan_ledger(
                action_id, trade_plan_id, context_id, response_id, ai_plan_id,
                context_hash, response_hash, plan_hash, recorded_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            ",
        )
        .bind(entry.action_id().to_string())
        .bind(entry.trade_plan_id().to_string())
        .bind(entry.context().context_id().to_string())
        .bind(entry.response().response_id().to_string())
        .bind(plan.plan_id().to_string())
        .bind(entry.context().context_hash().to_string())
        .bind(entry.response().response_hash().to_string())
        .bind(plan.plan_hash().to_string())
        .bind(recorded_at)
        .execute(&mut *transaction)
        .await?;

        insert_audit(&mut transaction, audit).await?;
        transaction.commit().await?;
        Ok(PersistenceEffect::Applied)
    }

    pub async fn persist_ai_provider_attempt(
        &self,
        owner_id: RuntimeInstanceId,
        context: &AiDecisionContext,
        evidence: &DeepSeekAttemptEvidence,
        audit: &AuditEntry,
    ) -> Result<PersistenceEffect, StorageError> {
        if evidence.context_id() != context.context_id()
            || !matches!(
                evidence.prompt_version(),
                ironpilot_application::AI_TRADING_PROMPT_VERSION_V1
                    | ironpilot_application::AI_TRADING_PROMPT_VERSION_V2
            )
            || evidence.requested_at_unix_millis() < context.as_of_unix_millis()
            || context.is_expired_at(evidence.requested_at_unix_millis())
        {
            return Err(StorageError::InvalidAiProviderEvidence);
        }
        let occurred_at = evidence
            .received_at_unix_millis()
            .unwrap_or(evidence.requested_at_unix_millis());
        if audit.occurred_at().get() != domain_timestamp(occurred_at)? {
            return Err(StorageError::AtomicTimestampMismatch);
        }
        if evidence
            .received_at_unix_millis()
            .is_some_and(|received_at| {
                received_at < evidence.requested_at_unix_millis()
                    || context.is_expired_at(received_at)
            })
        {
            return Err(StorageError::InvalidAiProviderEvidence);
        }
        if evidence.outcome() == DeepSeekAttemptOutcome::Plan
            && (evidence.raw_response().is_none()
                || evidence.usage().is_none()
                || evidence.cost_usd().is_none()
                || evidence.received_at_unix_millis().is_none())
        {
            return Err(StorageError::InvalidAiProviderEvidence);
        }
        serde_json::from_str::<Value>(evidence.raw_request())
            .map_err(|_| StorageError::InvalidAiProviderEvidence)?;

        let _write_guard = self.write_gate.lock().await;
        let mut transaction = self.pool.begin().await?;
        ensure_instance_lease(&mut transaction, owner_id, domain_timestamp(occurred_at)?).await?;
        ensure_ai_context(&mut transaction, context).await?;
        let usage = evidence.usage();
        let insert = sqlx::query(
            "
            INSERT INTO ai_provider_attempts(
                attempt_id, context_id, provider, model, prompt_version, prompt_hash,
                is_replan, requested_at, received_at, latency_millis, raw_request,
                raw_response, vendor_response_id, finish_reason, prompt_tokens,
                completion_tokens, cache_hit_tokens, cache_miss_tokens, total_tokens,
                cost_usd, outcome
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(attempt_id) DO NOTHING
            ",
        )
        .bind(evidence.attempt_id().to_string())
        .bind(evidence.context_id().to_string())
        .bind(DEEPSEEK_PROVIDER_NAME)
        .bind(evidence.model())
        .bind(evidence.prompt_version())
        .bind(evidence.prompt_hash())
        .bind(i64::from(evidence.is_replan()))
        .bind(domain_timestamp(evidence.requested_at_unix_millis())?)
        .bind(
            evidence
                .received_at_unix_millis()
                .map(domain_timestamp)
                .transpose()?,
        )
        .bind(
            i64::try_from(evidence.latency_millis())
                .map_err(|_| StorageError::InvalidStoredTimestamp)?,
        )
        .bind(evidence.raw_request())
        .bind(evidence.raw_response())
        .bind(evidence.vendor_response_id())
        .bind(evidence.finish_reason())
        .bind(
            usage
                .map(DeepSeekUsage::prompt_tokens)
                .map(token_count)
                .transpose()?,
        )
        .bind(
            usage
                .map(DeepSeekUsage::completion_tokens)
                .map(token_count)
                .transpose()?,
        )
        .bind(
            usage
                .map(DeepSeekUsage::prompt_cache_hit_tokens)
                .map(token_count)
                .transpose()?,
        )
        .bind(
            usage
                .map(DeepSeekUsage::prompt_cache_miss_tokens)
                .map(token_count)
                .transpose()?,
        )
        .bind(
            usage
                .map(DeepSeekUsage::total_tokens)
                .map(token_count)
                .transpose()?,
        )
        .bind(evidence.cost_usd().map(|cost| cost.to_string()))
        .bind(evidence.outcome().as_str())
        .execute(&mut *transaction)
        .await?;
        if insert.rows_affected() == 0 {
            let matches: i64 = sqlx::query_scalar(
                "
                SELECT EXISTS(
                    SELECT 1
                    FROM ai_provider_attempts
                    WHERE attempt_id = ?
                      AND context_id = ?
                      AND provider = ?
                      AND model = ?
                      AND prompt_version = ?
                      AND prompt_hash = ?
                      AND is_replan = ?
                      AND requested_at = ?
                      AND received_at IS ?
                      AND latency_millis = ?
                      AND raw_request = ?
                      AND raw_response IS ?
                      AND vendor_response_id IS ?
                      AND finish_reason IS ?
                      AND prompt_tokens IS ?
                      AND completion_tokens IS ?
                      AND cache_hit_tokens IS ?
                      AND cache_miss_tokens IS ?
                      AND total_tokens IS ?
                      AND cost_usd IS ?
                      AND outcome = ?
                )
                ",
            )
            .bind(evidence.attempt_id().to_string())
            .bind(evidence.context_id().to_string())
            .bind(DEEPSEEK_PROVIDER_NAME)
            .bind(evidence.model())
            .bind(evidence.prompt_version())
            .bind(evidence.prompt_hash())
            .bind(i64::from(evidence.is_replan()))
            .bind(domain_timestamp(evidence.requested_at_unix_millis())?)
            .bind(
                evidence
                    .received_at_unix_millis()
                    .map(domain_timestamp)
                    .transpose()?,
            )
            .bind(
                i64::try_from(evidence.latency_millis())
                    .map_err(|_| StorageError::InvalidStoredTimestamp)?,
            )
            .bind(evidence.raw_request())
            .bind(evidence.raw_response())
            .bind(evidence.vendor_response_id())
            .bind(evidence.finish_reason())
            .bind(
                usage
                    .map(DeepSeekUsage::prompt_tokens)
                    .map(token_count)
                    .transpose()?,
            )
            .bind(
                usage
                    .map(DeepSeekUsage::completion_tokens)
                    .map(token_count)
                    .transpose()?,
            )
            .bind(
                usage
                    .map(DeepSeekUsage::prompt_cache_hit_tokens)
                    .map(token_count)
                    .transpose()?,
            )
            .bind(
                usage
                    .map(DeepSeekUsage::prompt_cache_miss_tokens)
                    .map(token_count)
                    .transpose()?,
            )
            .bind(
                usage
                    .map(DeepSeekUsage::total_tokens)
                    .map(token_count)
                    .transpose()?,
            )
            .bind(evidence.cost_usd().map(|cost| cost.to_string()))
            .bind(evidence.outcome().as_str())
            .fetch_one(&mut *transaction)
            .await?;
            if matches != 1 {
                return Err(StorageError::IdempotencyConflict);
            }
            transaction.commit().await?;
            return Ok(PersistenceEffect::DuplicateNoEffect);
        }
        insert_audit(&mut transaction, audit).await?;
        transaction.commit().await?;
        Ok(PersistenceEffect::Applied)
    }

    pub async fn ai_trade_plan_trace(
        &self,
        action_id: TradePlanActionId,
    ) -> Result<Option<AiTradePlanTraceRow>, StorageError> {
        let row = sqlx::query(
            "
            SELECT
                ledger.action_id,
                ledger.trade_plan_id,
                ledger.context_id,
                ledger.response_id,
                ledger.ai_plan_id,
                ledger.context_hash,
                ledger.response_hash,
                ledger.plan_hash,
                ledger.recorded_at,
                plans.action
            FROM ai_trade_plan_ledger AS ledger
            JOIN ai_trading_plans AS plans ON plans.ai_plan_id = ledger.ai_plan_id
            WHERE ledger.action_id = ?
            ",
        )
        .bind(action_id.to_string())
        .fetch_optional(&self.pool)
        .await?;
        row.map(|row| {
            Ok(AiTradePlanTraceRow {
                action_id: row.try_get("action_id")?,
                trade_plan_id: row.try_get("trade_plan_id")?,
                context_id: row.try_get("context_id")?,
                response_id: row.try_get("response_id")?,
                ai_plan_id: row.try_get("ai_plan_id")?,
                context_hash: row.try_get("context_hash")?,
                response_hash: row.try_get("response_hash")?,
                plan_hash: row.try_get("plan_hash")?,
                action: row.try_get("action")?,
                recorded_at: UnixMillis::new(row.try_get("recorded_at")?)
                    .map_err(|_| StorageError::InvalidStoredTimestamp)?,
            })
        })
        .transpose()
    }

    pub async fn persist_execution_validation(
        &self,
        owner_id: RuntimeInstanceId,
        decision: &ExecutionValidationDecision,
        audit: &AuditEntry,
    ) -> Result<PersistenceEffect, StorageError> {
        let validated_at = domain_timestamp(decision.validated_at_unix_millis())?;
        if audit.occurred_at().get() != validated_at {
            return Err(StorageError::AtomicTimestampMismatch);
        }
        let _write_guard = self.write_gate.lock().await;
        let mut transaction = self.pool.begin().await?;
        ensure_instance_lease(&mut transaction, owner_id, validated_at).await?;

        let ledger: Option<(String, String, String, String, String)> = sqlx::query_as(
            "
            SELECT
                ledger.trade_plan_id,
                ledger.context_hash,
                ledger.plan_hash,
                ledger.ai_plan_id,
                plans.action
            FROM ai_trade_plan_ledger AS ledger
            JOIN ai_trading_plans AS plans ON plans.ai_plan_id = ledger.ai_plan_id
            WHERE ledger.action_id = ?
            ",
        )
        .bind(decision.action_id().to_string())
        .fetch_optional(&mut *transaction)
        .await?;
        let Some((trade_plan_id, context_hash, plan_hash, ai_plan_id, action)) = ledger else {
            return Err(StorageError::ValidationTargetUnavailable);
        };
        if trade_plan_id != decision.trade_plan_id().to_string()
            || context_hash != decision.context_hash()
            || plan_hash != decision.plan_hash().to_string()
            || ai_plan_id != decision.plan_id()
        {
            return Err(StorageError::ValidationEvidenceMismatch);
        }

        let insert = sqlx::query(
            "
            INSERT INTO execution_validations(
                action_id, trade_plan_id, ai_plan_id, validator_version, outcome,
                context_hash, plan_hash, recalculated_maximum_loss_quote,
                authorized_maximum_loss_quote, validated_at, validation_hash, evidence_json
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(action_id) DO NOTHING
            ",
        )
        .bind(decision.action_id().to_string())
        .bind(decision.trade_plan_id().to_string())
        .bind(decision.plan_id())
        .bind(EXECUTION_VALIDATOR_VERSION_V1)
        .bind(decision.outcome().as_str())
        .bind(decision.context_hash())
        .bind(decision.plan_hash().to_string())
        .bind(
            decision
                .recalculated_maximum_loss_quote()
                .map(|value| value.to_string()),
        )
        .bind(decision.authorized_maximum_loss_quote().to_string())
        .bind(validated_at)
        .bind(decision.validation_hash().to_string())
        .bind(decision.evidence_json())
        .execute(&mut *transaction)
        .await?;
        if insert.rows_affected() == 0 {
            let matches: i64 = sqlx::query_scalar(
                "
                SELECT EXISTS(
                    SELECT 1
                    FROM execution_validations
                    WHERE action_id = ?
                      AND trade_plan_id = ?
                      AND ai_plan_id = ?
                      AND validator_version = ?
                      AND outcome = ?
                      AND context_hash = ?
                      AND plan_hash = ?
                      AND recalculated_maximum_loss_quote IS ?
                      AND authorized_maximum_loss_quote = ?
                      AND validated_at = ?
                      AND validation_hash = ?
                      AND evidence_json = ?
                )
                ",
            )
            .bind(decision.action_id().to_string())
            .bind(decision.trade_plan_id().to_string())
            .bind(decision.plan_id())
            .bind(EXECUTION_VALIDATOR_VERSION_V1)
            .bind(decision.outcome().as_str())
            .bind(decision.context_hash())
            .bind(decision.plan_hash().to_string())
            .bind(
                decision
                    .recalculated_maximum_loss_quote()
                    .map(|value| value.to_string()),
            )
            .bind(decision.authorized_maximum_loss_quote().to_string())
            .bind(validated_at)
            .bind(decision.validation_hash().to_string())
            .bind(decision.evidence_json())
            .fetch_one(&mut *transaction)
            .await?;
            if matches != 1 {
                return Err(StorageError::IdempotencyConflict);
            }
            transaction.commit().await?;
            return Ok(PersistenceEffect::DuplicateNoEffect);
        }

        let action_update = sqlx::query(
            "
            UPDATE trade_plan_actions
            SET state = ?
            WHERE action_id = ? AND state = 'RECORDED'
            ",
        )
        .bind(match decision.outcome() {
            ExecutionValidationOutcome::Accept => "VALIDATION_ACCEPTED",
            ExecutionValidationOutcome::Reject => "VALIDATION_REJECTED",
        })
        .bind(decision.action_id().to_string())
        .execute(&mut *transaction)
        .await?;
        if action_update.rows_affected() != 1 {
            return Err(StorageError::ValidationTargetUnavailable);
        }

        if action == "OPEN_LONG" {
            let update = sqlx::query(
                "
                UPDATE trade_plans
                SET state = ?, updated_at = ?
                WHERE trade_plan_id = ? AND state = 'PROPOSED'
                ",
            )
            .bind(match decision.outcome() {
                ExecutionValidationOutcome::Accept => "ACCEPTED",
                ExecutionValidationOutcome::Reject => "REJECTED",
            })
            .bind(validated_at)
            .bind(decision.trade_plan_id().to_string())
            .execute(&mut *transaction)
            .await?;
            if update.rows_affected() != 1 {
                return Err(StorageError::ValidationTargetUnavailable);
            }
        }
        insert_audit(&mut transaction, audit).await?;
        transaction.commit().await?;
        Ok(PersistenceEffect::Applied)
    }

    pub async fn execution_validation(
        &self,
        action_id: TradePlanActionId,
    ) -> Result<Option<ExecutionValidationRow>, StorageError> {
        let row = sqlx::query(
            "
            SELECT
                action_id, trade_plan_id, ai_plan_id, validator_version, outcome,
                context_hash, plan_hash, recalculated_maximum_loss_quote,
                authorized_maximum_loss_quote, validated_at, validation_hash, evidence_json
            FROM execution_validations
            WHERE action_id = ?
            ",
        )
        .bind(action_id.to_string())
        .fetch_optional(&self.pool)
        .await?;
        row.map(|row| {
            Ok(ExecutionValidationRow {
                action_id: row.try_get("action_id")?,
                trade_plan_id: row.try_get("trade_plan_id")?,
                ai_plan_id: row.try_get("ai_plan_id")?,
                validator_version: row.try_get("validator_version")?,
                outcome: row.try_get("outcome")?,
                context_hash: row.try_get("context_hash")?,
                plan_hash: row.try_get("plan_hash")?,
                recalculated_maximum_loss_quote: row.try_get("recalculated_maximum_loss_quote")?,
                authorized_maximum_loss_quote: row.try_get("authorized_maximum_loss_quote")?,
                validated_at: UnixMillis::new(row.try_get("validated_at")?)
                    .map_err(|_| StorageError::InvalidStoredTimestamp)?,
                validation_hash: row.try_get("validation_hash")?,
                evidence_json: row.try_get("evidence_json")?,
            })
        })
        .transpose()
    }

    pub async fn backup_to(&self, destination: impl AsRef<Path>) -> Result<(), StorageError> {
        let destination = destination.as_ref();
        if destination.as_os_str().is_empty() || destination == self.database_path {
            return Err(StorageError::InvalidBackupPath);
        }
        if destination.exists() {
            return Err(StorageError::BackupDestinationExists {
                path: destination.to_path_buf(),
            });
        }
        let destination_text = destination
            .to_str()
            .ok_or(StorageError::NonUnicodeBackupPath)?;

        let _write_guard = self.write_gate.lock().await;
        sqlx::query("VACUUM INTO ?")
            .bind(destination_text)
            .execute(&self.pool)
            .await?;
        verify_database_file(destination).await
    }

    async fn verify_runtime_pragmas(&self) -> Result<(), StorageError> {
        let journal_mode: String = sqlx::query_scalar("PRAGMA journal_mode")
            .fetch_one(&self.pool)
            .await?;
        if !journal_mode.eq_ignore_ascii_case("wal") {
            return Err(StorageError::UnexpectedJournalMode {
                actual: journal_mode.into_boxed_str(),
            });
        }
        let foreign_keys: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
            .fetch_one(&self.pool)
            .await?;
        if foreign_keys != 1 {
            return Err(StorageError::ForeignKeysDisabled);
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn pool(&self) -> &SqlitePool {
        &self.pool
    }
}

pub(crate) async fn ensure_instance_lease(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    owner_id: RuntimeInstanceId,
    at: i64,
) -> Result<(), StorageError> {
    let lease_is_valid: i64 = sqlx::query_scalar(
        "
        SELECT EXISTS (
            SELECT 1
            FROM runtime_instance_lease
            WHERE lock_name = ?
              AND owner_id = ?
              AND expires_at > ?
        )
        ",
    )
    .bind(TRADING_RUNTIME_LOCK)
    .bind(owner_id.to_string())
    .bind(at)
    .fetch_one(&mut **transaction)
    .await?;
    if lease_is_valid != 1 {
        return Err(StorageError::InstanceLeaseNotHeld);
    }
    Ok(())
}

async fn ensure_ai_context(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    context: &AiDecisionContext,
) -> Result<(), StorageError> {
    let as_of = domain_timestamp(context.as_of_unix_millis())?;
    let valid_until = domain_timestamp(context.valid_until_unix_millis())?;
    let insert = sqlx::query(
        "
        INSERT INTO ai_decision_contexts(
            context_id, schema_version, instrument_id, as_of, valid_until,
            maximum_loss_quote, context_hash, payload_json
        )
        VALUES (?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(context_id) DO NOTHING
        ",
    )
    .bind(context.context_id().to_string())
    .bind(AI_DECISION_CONTEXT_SCHEMA_VERSION_V1)
    .bind(context.instrument_id().to_string())
    .bind(as_of)
    .bind(valid_until)
    .bind(context.maximum_loss_quote().to_string())
    .bind(context.context_hash().to_string())
    .bind(context.to_json())
    .execute(&mut **transaction)
    .await?;
    if insert.rows_affected() == 0 {
        let existing: (String, String, i64, i64, String, String, String) = sqlx::query_as(
            "
            SELECT schema_version, instrument_id, as_of, valid_until,
                   maximum_loss_quote, context_hash, payload_json
            FROM ai_decision_contexts
            WHERE context_id = ?
            ",
        )
        .bind(context.context_id().to_string())
        .fetch_one(&mut **transaction)
        .await?;
        if existing
            != (
                AI_DECISION_CONTEXT_SCHEMA_VERSION_V1.to_owned(),
                context.instrument_id().to_string(),
                as_of,
                valid_until,
                context.maximum_loss_quote().to_string(),
                context.context_hash().to_string(),
                context.to_json().to_owned(),
            )
        {
            return Err(StorageError::IdempotencyConflict);
        }
    }
    Ok(())
}

async fn ensure_ai_response(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    response: &AiRawResponse,
) -> Result<(), StorageError> {
    let received_at = domain_timestamp(response.received_at_unix_millis())?;
    let insert = sqlx::query(
        "
        INSERT INTO ai_provider_responses(
            response_id, context_id, provider, model, received_at, response_hash, raw_response
        )
        VALUES (?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(response_id) DO NOTHING
        ",
    )
    .bind(response.response_id().to_string())
    .bind(response.context_id().to_string())
    .bind(response.provider())
    .bind(response.model())
    .bind(received_at)
    .bind(response.response_hash().to_string())
    .bind(response.raw_response())
    .execute(&mut **transaction)
    .await?;
    if insert.rows_affected() == 0 {
        let existing: (String, String, String, i64, String, String) = sqlx::query_as(
            "
            SELECT context_id, provider, model, received_at, response_hash, raw_response
            FROM ai_provider_responses
            WHERE response_id = ?
            ",
        )
        .bind(response.response_id().to_string())
        .fetch_one(&mut **transaction)
        .await?;
        if existing
            != (
                response.context_id().to_string(),
                response.provider().to_owned(),
                response.model().to_owned(),
                received_at,
                response.response_hash().to_string(),
                response.raw_response().to_owned(),
            )
        {
            return Err(StorageError::IdempotencyConflict);
        }
    }
    Ok(())
}

async fn ensure_existing_ai_ledger_matches(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    entry: &AiTradePlanLedgerEntry,
) -> Result<(), StorageError> {
    let plan = entry.plan();
    let existing: (
        String,
        String,
        String,
        String,
        String,
        i64,
        i64,
        String,
        String,
        String,
        String,
    ) = sqlx::query_as(
        "
        SELECT
            plans.context_id,
            plans.response_id,
            plans.schema_version,
            plans.instrument_id,
            plans.action,
            plans.created_at,
            plans.valid_until,
            plans.plan_hash,
            plans.payload_json,
            ledger.trade_plan_id,
            ledger.action_id
        FROM ai_trading_plans AS plans
        JOIN ai_trade_plan_ledger AS ledger ON ledger.ai_plan_id = plans.ai_plan_id
        WHERE plans.ai_plan_id = ?
        ",
    )
    .bind(plan.plan_id().to_string())
    .fetch_one(&mut **transaction)
    .await
    .map_err(|error| {
        if matches!(error, sqlx::Error::RowNotFound) {
            StorageError::IdempotencyConflict
        } else {
            StorageError::Sqlx(error)
        }
    })?;
    if existing
        != (
            plan.context_id().to_string(),
            entry.response().response_id().to_string(),
            plan.schema_version().to_owned(),
            plan.instrument_id().to_string(),
            plan.action().as_str().to_owned(),
            domain_timestamp(entry.recorded_at_unix_millis())?,
            domain_timestamp(plan.valid_until_unix_millis())?,
            plan.plan_hash().to_string(),
            plan.to_json(),
            entry.trade_plan_id().to_string(),
            entry.action_id().to_string(),
        )
    {
        return Err(StorageError::IdempotencyConflict);
    }
    Ok(())
}

pub(crate) fn domain_timestamp(value: u64) -> Result<i64, StorageError> {
    i64::try_from(value).map_err(|_| StorageError::InvalidStoredTimestamp)
}

fn token_count(value: u64) -> Result<i64, StorageError> {
    i64::try_from(value).map_err(|_| StorageError::InvalidAiProviderEvidence)
}

pub(crate) async fn insert_managed_lot(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    fill: &PortfolioFill,
    occurred_at: i64,
) -> Result<(), StorageError> {
    let managed_lot_id = fill
        .managed_lot_id()
        .ok_or(StorageError::InvalidPortfolioFill)?;
    let payload = managed_lot_json(
        fill.base_asset(),
        fill.base_quantity(),
        fill.base_quantity(),
        fill.fill_id().to_string(),
    );
    sqlx::query(
        "
        INSERT INTO managed_lots(
            managed_lot_id, trade_plan_id, instrument_id, opened_at, closed_at, payload_json
        )
        VALUES (?, ?, ?, ?, NULL, ?)
        ",
    )
    .bind(managed_lot_id.to_string())
    .bind(fill.trade_plan_id().to_string())
    .bind(fill.instrument_id().to_string())
    .bind(occurred_at)
    .bind(payload.to_string())
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

pub(crate) async fn consume_managed_lots(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    fill: &PortfolioFill,
    occurred_at: i64,
) -> Result<(), StorageError> {
    let rows: Vec<(String, String)> = sqlx::query_as(
        "
        SELECT managed_lot_id, payload_json
        FROM managed_lots
        WHERE instrument_id = ? AND closed_at IS NULL
        ORDER BY opened_at, managed_lot_id
        ",
    )
    .bind(fill.instrument_id().to_string())
    .fetch_all(&mut **transaction)
    .await?;
    let mut lots = Vec::with_capacity(rows.len());
    let mut total = DomainDecimal::ZERO;
    for (managed_lot_id, payload) in rows {
        let lot = parse_managed_lot(&payload)?;
        if lot.base_asset != fill.base_asset().as_str() {
            return Err(StorageError::ManagedAssetMismatch);
        }
        total = total
            .checked_add(lot.remaining_quantity)
            .ok_or(StorageError::PortfolioArithmeticOverflow)?;
        lots.push((managed_lot_id, lot));
    }
    if fill.base_quantity() > total {
        return Err(StorageError::InsufficientManagedQuantity);
    }

    let mut quantity_to_consume = fill.base_quantity();
    for (managed_lot_id, lot) in lots {
        if quantity_to_consume == DomainDecimal::ZERO {
            break;
        }
        let consumed = lot.remaining_quantity.min(quantity_to_consume);
        let remaining_quantity = lot
            .remaining_quantity
            .checked_sub(consumed)
            .ok_or(StorageError::PortfolioArithmeticOverflow)?;
        quantity_to_consume = quantity_to_consume
            .checked_sub(consumed)
            .ok_or(StorageError::PortfolioArithmeticOverflow)?;
        let payload = managed_lot_json(
            fill.base_asset(),
            lot.initial_quantity,
            remaining_quantity,
            lot.source_fill_id,
        );
        sqlx::query(
            "
            UPDATE managed_lots
            SET closed_at = ?, payload_json = ?
            WHERE managed_lot_id = ?
            ",
        )
        .bind((remaining_quantity == DomainDecimal::ZERO).then_some(occurred_at))
        .bind(payload.to_string())
        .bind(managed_lot_id)
        .execute(&mut **transaction)
        .await?;
    }
    Ok(())
}

pub(crate) async fn consume_managed_lots_by_plan(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    trade_plan_id: TradePlanId,
    quantity: DomainDecimal,
    occurred_at: i64,
) -> Result<(), StorageError> {
    if quantity <= DomainDecimal::ZERO {
        return Err(StorageError::InvalidPortfolioFill);
    }
    let rows: Vec<(String, String)> = sqlx::query_as(
        "
        SELECT managed_lot_id, payload_json
        FROM managed_lots
        WHERE trade_plan_id = ? AND closed_at IS NULL
        ORDER BY opened_at, managed_lot_id
        ",
    )
    .bind(trade_plan_id.to_string())
    .fetch_all(&mut **transaction)
    .await?;
    let mut lots = Vec::with_capacity(rows.len());
    let mut total = DomainDecimal::ZERO;
    for (managed_lot_id, payload) in rows {
        let lot = parse_managed_lot(&payload)?;
        total = total
            .checked_add(lot.remaining_quantity)
            .ok_or(StorageError::PortfolioArithmeticOverflow)?;
        lots.push((managed_lot_id, lot));
    }
    if quantity > total {
        return Err(StorageError::InsufficientManagedQuantity);
    }
    let mut quantity_to_consume = quantity;
    for (managed_lot_id, lot) in lots {
        if quantity_to_consume == DomainDecimal::ZERO {
            break;
        }
        let consumed = lot.remaining_quantity.min(quantity_to_consume);
        let remaining_quantity = lot
            .remaining_quantity
            .checked_sub(consumed)
            .ok_or(StorageError::PortfolioArithmeticOverflow)?;
        quantity_to_consume = quantity_to_consume
            .checked_sub(consumed)
            .ok_or(StorageError::PortfolioArithmeticOverflow)?;
        let payload = managed_lot_payload_json(
            &lot.base_asset,
            lot.initial_quantity,
            remaining_quantity,
            lot.source_fill_id,
        );
        sqlx::query(
            "
            UPDATE managed_lots
            SET closed_at = ?, payload_json = ?
            WHERE managed_lot_id = ?
            ",
        )
        .bind((remaining_quantity == DomainDecimal::ZERO).then_some(occurred_at))
        .bind(payload.to_string())
        .bind(managed_lot_id)
        .execute(&mut **transaction)
        .await?;
    }
    Ok(())
}

fn portfolio_fill_json(fill: &PortfolioFill) -> Value {
    json!({
        "schema_version": "ironpilot-portfolio-fill-v1",
        "instrument_id": fill.instrument_id().to_string(),
        "trade_plan_id": fill.trade_plan_id().to_string(),
        "managed_lot_id": fill.managed_lot_id().map(|value| value.to_string()),
        "base_asset": fill.base_asset().as_str(),
        "quote_asset": fill.quote_asset().as_str(),
        "side": match fill.side() {
            PortfolioFillSide::Buy => "buy",
            PortfolioFillSide::Sell => "sell",
        },
        "base_quantity": normalized_decimal(fill.base_quantity()),
        "quote_quantity": normalized_decimal(fill.quote_quantity()),
    })
}

fn managed_lot_json(
    base_asset: &AssetCode,
    initial_quantity: DomainDecimal,
    remaining_quantity: DomainDecimal,
    source_fill_id: String,
) -> Value {
    managed_lot_payload_json(
        base_asset.as_str(),
        initial_quantity,
        remaining_quantity,
        source_fill_id,
    )
}

fn managed_lot_payload_json(
    base_asset: &str,
    initial_quantity: DomainDecimal,
    remaining_quantity: DomainDecimal,
    source_fill_id: String,
) -> Value {
    json!({
        "schema_version": "ironpilot-managed-lot-v1",
        "base_asset": base_asset,
        "initial_quantity": normalized_decimal(initial_quantity),
        "remaining_quantity": normalized_decimal(remaining_quantity),
        "source_fill_id": source_fill_id,
    })
}

fn portfolio_snapshot_json(snapshot: &PortfolioSnapshot) -> Value {
    let assets: Vec<Value> = snapshot
        .assets()
        .iter()
        .map(|asset| {
            json!({
                "asset": asset.asset().as_str(),
                "exchange_available_quantity": normalized_decimal(asset.exchange_available_quantity()),
                "exchange_locked_quantity": normalized_decimal(asset.exchange_locked_quantity()),
                "exchange_total_quantity": normalized_decimal(asset.exchange_total_quantity()),
                "local_expected_quantity": normalized_decimal(asset.local_expected_quantity()),
                "managed_quantity": normalized_decimal(asset.managed_quantity()),
                "unknown_quantity": normalized_decimal(asset.unknown_quantity()),
                "shortfall_quantity": normalized_decimal(asset.shortfall_quantity()),
            })
        })
        .collect();
    json!({
        "schema_version": snapshot.schema_version(),
        "observed_at_unix_millis": snapshot.observed_at_unix_millis(),
        "status": match snapshot.status() {
            PortfolioReconciliationStatus::Balanced => "balanced",
            PortfolioReconciliationStatus::BalanceDifference => "balance_difference",
        },
        "allows_new_entries": snapshot.allows_new_entries(),
        "snapshot_hash": snapshot.snapshot_hash().to_string(),
        "assets": assets,
    })
}

pub(crate) fn normalized_decimal(value: DomainDecimal) -> String {
    value.as_decimal().normalize().to_string()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ManagedLotPayload {
    schema_version: String,
    base_asset: String,
    initial_quantity: String,
    remaining_quantity: String,
    source_fill_id: String,
}

struct ParsedManagedLot {
    base_asset: String,
    initial_quantity: DomainDecimal,
    remaining_quantity: DomainDecimal,
    source_fill_id: String,
}

fn parse_managed_lot(payload: &str) -> Result<ParsedManagedLot, StorageError> {
    let payload: ManagedLotPayload =
        serde_json::from_str(payload).map_err(|_| StorageError::InvalidPortfolioPayload)?;
    if payload.schema_version != "ironpilot-managed-lot-v1" {
        return Err(StorageError::InvalidPortfolioPayload);
    }
    let initial_quantity = payload
        .initial_quantity
        .parse()
        .map_err(|_| StorageError::InvalidPortfolioPayload)?;
    let remaining_quantity = payload
        .remaining_quantity
        .parse()
        .map_err(|_| StorageError::InvalidPortfolioPayload)?;
    if initial_quantity <= DomainDecimal::ZERO
        || remaining_quantity < DomainDecimal::ZERO
        || remaining_quantity > initial_quantity
    {
        return Err(StorageError::InvalidPortfolioPayload);
    }
    Ok(ParsedManagedLot {
        base_asset: payload.base_asset,
        initial_quantity,
        remaining_quantity,
        source_fill_id: payload.source_fill_id,
    })
}

fn connection_options(path: &Path, create: bool, read_only: bool) -> SqliteConnectOptions {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .foreign_keys(true)
        .busy_timeout(BUSY_TIMEOUT);
    if read_only {
        options.read_only(true)
    } else {
        options
            .create_if_missing(create)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Full)
    }
}

pub(crate) async fn insert_audit(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    audit: &AuditEntry,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "
        INSERT INTO audit_log (
            audit_entry_id, occurred_at, category, subject_id, payload_json
        )
        VALUES (?, ?, ?, ?, ?)
        ",
    )
    .bind(audit.id().to_string())
    .bind(audit.occurred_at().get())
    .bind(audit.category())
    .bind(audit.subject_id())
    .bind(audit.payload().to_string())
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn insert_outbox(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    message: &OutboxMessage,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "
        INSERT INTO outbox (
            outbox_message_id, topic, payload_json, created_at
        )
        VALUES (?, ?, ?, ?)
        ",
    )
    .bind(message.id().to_string())
    .bind(message.topic())
    .bind(message.payload().to_string())
    .bind(message.created_at().get())
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn verify_database_file(path: &Path) -> Result<(), StorageError> {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(connection_options(path, false, true))
        .await?;
    let integrity: String = sqlx::query_scalar("PRAGMA integrity_check")
        .fetch_one(&pool)
        .await?;
    if integrity != "ok" {
        return Err(StorageError::BackupIntegrityCheckFailed {
            result: integrity.into_boxed_str(),
        });
    }
    let migration_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations WHERE success = 1")
            .fetch_one(&pool)
            .await?;
    pool.close().await;
    if migration_count == 0 {
        return Err(StorageError::BackupMissingMigrations);
    }
    Ok(())
}

fn checked_expiry(
    acquired_at: UnixMillis,
    lease_duration: Duration,
) -> Result<UnixMillis, LeaseAcquireError> {
    let duration_ms = i64::try_from(lease_duration.as_millis())
        .map_err(|_| LeaseAcquireError::InvalidDuration)?;
    if duration_ms == 0 {
        return Err(LeaseAcquireError::InvalidDuration);
    }
    let expires_at = acquired_at
        .get()
        .checked_add(duration_ms)
        .ok_or(LeaseAcquireError::InvalidDuration)?;
    UnixMillis::new(expires_at).map_err(|_| LeaseAcquireError::InvalidDuration)
}

const fn system_state_text(state: SystemState) -> &'static str {
    match state {
        SystemState::Starting => "STARTING",
        SystemState::Recovering => "RECOVERING",
        SystemState::Observing => "OBSERVING",
        SystemState::EntryEnabled => "ENTRY_ENABLED",
        SystemState::ReduceOnly => "REDUCE_ONLY",
        SystemState::Halted => "HALTED",
    }
}

fn parse_system_state(value: &str) -> Result<SystemState, StorageError> {
    match value {
        "STARTING" => Ok(SystemState::Starting),
        "RECOVERING" => Ok(SystemState::Recovering),
        "OBSERVING" => Ok(SystemState::Observing),
        "ENTRY_ENABLED" => Ok(SystemState::EntryEnabled),
        "REDUCE_ONLY" => Ok(SystemState::ReduceOnly),
        "HALTED" => Ok(SystemState::Halted),
        _ => Err(StorageError::InvalidStoredSystemState {
            value: value.into(),
        }),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InstanceLease {
    owner_id: RuntimeInstanceId,
    acquired_at: UnixMillis,
    expires_at: UnixMillis,
}

impl InstanceLease {
    #[must_use]
    pub const fn owner_id(self) -> RuntimeInstanceId {
        self.owner_id
    }

    #[must_use]
    pub const fn acquired_at(self) -> UnixMillis {
        self.acquired_at
    }

    #[must_use]
    pub const fn expires_at(self) -> UnixMillis {
        self.expires_at
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuditRow {
    pub sequence: i64,
    pub audit_entry_id: String,
    pub occurred_at: UnixMillis,
    pub category: String,
    pub subject_id: Option<String>,
    pub payload_json: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingOutboxRow {
    pub outbox_message_id: String,
    pub topic: String,
    pub payload_json: String,
    pub created_at: UnixMillis,
    pub attempts: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AiTradePlanTraceRow {
    pub action_id: String,
    pub trade_plan_id: String,
    pub context_id: String,
    pub response_id: String,
    pub ai_plan_id: String,
    pub context_hash: String,
    pub response_hash: String,
    pub plan_hash: String,
    pub action: String,
    pub recorded_at: UnixMillis,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExecutionValidationRow {
    pub action_id: String,
    pub trade_plan_id: String,
    pub ai_plan_id: String,
    pub validator_version: String,
    pub outcome: String,
    pub context_hash: String,
    pub plan_hash: String,
    pub recalculated_maximum_loss_quote: Option<String>,
    pub authorized_maximum_loss_quote: String,
    pub validated_at: UnixMillis,
    pub validation_hash: String,
    pub evidence_json: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PersistenceEffect {
    Applied,
    DuplicateNoEffect,
}

#[derive(Debug)]
pub enum LeaseAcquireError {
    InvalidDuration,
    Held {
        owner_id: String,
        expires_at: UnixMillis,
    },
    Storage(StorageError),
}

impl fmt::Display for LeaseAcquireError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDuration => formatter.write_str("instance lease duration is invalid"),
            Self::Held {
                owner_id,
                expires_at,
            } => write!(
                formatter,
                "trading runtime lease is held by {owner_id} until {}",
                expires_at.get()
            ),
            Self::Storage(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for LeaseAcquireError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Storage(error) => Some(error),
            Self::InvalidDuration | Self::Held { .. } => None,
        }
    }
}

impl From<StorageError> for LeaseAcquireError {
    fn from(value: StorageError) -> Self {
        Self::Storage(value)
    }
}

#[derive(Debug)]
pub enum StorageError {
    Sqlx(sqlx::Error),
    Migration(sqlx::migrate::MigrateError),
    InvalidConnectionLimit {
        max_connections: u8,
    },
    InvalidDatabasePath,
    InvalidStoredTimestamp,
    InvalidAiProviderEvidence,
    InvalidStoredSystemState {
        value: Box<str>,
    },
    SystemStateConflict {
        expected: Option<SystemState>,
        actual: Option<SystemState>,
    },
    InstanceLeaseNotHeld,
    InvalidOutboxLimit {
        limit: u16,
    },
    UnexpectedJournalMode {
        actual: Box<str>,
    },
    ForeignKeysDisabled,
    InvalidBackupPath,
    NonUnicodeBackupPath,
    BackupDestinationExists {
        path: PathBuf,
    },
    BackupIntegrityCheckFailed {
        result: Box<str>,
    },
    BackupMissingMigrations,
    AtomicTimestampMismatch,
    IdempotencyConflict,
    InvalidPortfolioFill,
    InvalidPortfolioPayload,
    ManagedAssetMismatch,
    InsufficientManagedQuantity,
    PortfolioArithmeticOverflow,
    ActiveTradePlanExists {
        instrument_id: Box<str>,
        trade_plan_id: Box<str>,
    },
    TargetTradePlanUnavailable {
        trade_plan_id: Box<str>,
    },
    ValidationTargetUnavailable,
    ValidationEvidenceMismatch,
    Portfolio(ironpilot_domain::PortfolioError),
}

impl fmt::Display for StorageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sqlx(error) => write!(formatter, "SQLite operation failed: {error}"),
            Self::Migration(error) => write!(formatter, "SQLite migration failed: {error}"),
            Self::InvalidConnectionLimit { max_connections } => write!(
                formatter,
                "SQLite connection limit {max_connections} is outside 1..=4"
            ),
            Self::InvalidDatabasePath => formatter.write_str("SQLite database path is invalid"),
            Self::InvalidStoredTimestamp => {
                formatter.write_str("SQLite contains an invalid timestamp")
            }
            Self::InvalidAiProviderEvidence => {
                formatter.write_str("AI provider attempt evidence is invalid")
            }
            Self::InvalidStoredSystemState { value } => {
                write!(formatter, "SQLite contains unknown system state {value}")
            }
            Self::SystemStateConflict { expected, actual } => write!(
                formatter,
                "system state compare-and-set failed: expected {expected:?}, actual {actual:?}"
            ),
            Self::InstanceLeaseNotHeld => {
                formatter.write_str("runtime instance does not hold a valid trading lease")
            }
            Self::InvalidOutboxLimit { limit } => {
                write!(formatter, "outbox read limit {limit} is outside 1..=256")
            }
            Self::UnexpectedJournalMode { actual } => {
                write!(formatter, "SQLite journal mode is {actual}, expected WAL")
            }
            Self::ForeignKeysDisabled => formatter.write_str("SQLite foreign keys are disabled"),
            Self::InvalidBackupPath => formatter.write_str("SQLite backup path is invalid"),
            Self::NonUnicodeBackupPath => {
                formatter.write_str("SQLite backup path must be valid Unicode")
            }
            Self::BackupDestinationExists { path } => write!(
                formatter,
                "SQLite backup destination {} already exists",
                path.display()
            ),
            Self::BackupIntegrityCheckFailed { result } => {
                write!(formatter, "SQLite backup integrity check failed: {result}")
            }
            Self::BackupMissingMigrations => {
                formatter.write_str("SQLite backup does not contain applied migrations")
            }
            Self::AtomicTimestampMismatch => {
                formatter.write_str("portfolio state and audit timestamps do not match")
            }
            Self::IdempotencyConflict => {
                formatter.write_str("an idempotency key was reused with different content")
            }
            Self::InvalidPortfolioFill => formatter.write_str("portfolio fill is invalid"),
            Self::InvalidPortfolioPayload => {
                formatter.write_str("SQLite contains an invalid portfolio payload")
            }
            Self::ManagedAssetMismatch => {
                formatter.write_str("managed lot asset does not match the requested asset")
            }
            Self::InsufficientManagedQuantity => {
                formatter.write_str("fill quantity exceeds provable managed quantity")
            }
            Self::PortfolioArithmeticOverflow => {
                formatter.write_str("portfolio persistence arithmetic overflowed")
            }
            Self::ActiveTradePlanExists {
                instrument_id,
                trade_plan_id,
            } => write!(
                formatter,
                "instrument {instrument_id} already has active TradePlan {trade_plan_id}"
            ),
            Self::TargetTradePlanUnavailable { trade_plan_id } => write!(
                formatter,
                "target TradePlan {trade_plan_id} is missing, terminal, or belongs to another instrument"
            ),
            Self::ValidationTargetUnavailable => {
                formatter.write_str("execution validation target is unavailable or not proposed")
            }
            Self::ValidationEvidenceMismatch => formatter.write_str(
                "execution validation evidence does not match the persisted AI plan ledger",
            ),
            Self::Portfolio(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for StorageError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Sqlx(error) => Some(error),
            Self::Migration(error) => Some(error),
            Self::Portfolio(error) => Some(error),
            _ => None,
        }
    }
}

impl From<sqlx::Error> for StorageError {
    fn from(value: sqlx::Error) -> Self {
        Self::Sqlx(value)
    }
}

impl From<sqlx::migrate::MigrateError> for StorageError {
    fn from(value: sqlx::migrate::MigrateError) -> Self {
        Self::Migration(value)
    }
}

impl From<LeaseAcquireError> for StorageError {
    fn from(value: LeaseAcquireError) -> Self {
        match value {
            LeaseAcquireError::Storage(error) => error,
            LeaseAcquireError::InvalidDuration | LeaseAcquireError::Held { .. } => {
                Self::InstanceLeaseNotHeld
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::path::{Path, PathBuf};
    use std::str::FromStr;
    use std::sync::Mutex as StdMutex;
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

    use super::{LeaseAcquireError, PersistenceEffect, SqliteRepository, StorageError};
    use ironpilot_application::{
        AiRuntimeTradePlanFact, AiTradingRuntimeState, AuditEntry, ExecutionAuthorization,
        ExecutionMode, ExecutionOrderIdSet, ExecutionOrderIds, ExecutionValidationDecision,
        ExecutionValidationOutcome, ExecutionValidationPolicy, OutboxMessage, PaperExecutionError,
        PaperExecutionPolicy, PaperMarketObservation, SpotExecutionPort, SpotExecutionRequest,
        SpotOrderPriceLimits, SystemStateChange, UnixMillis,
    };
    use ironpilot_domain::{
        AccountOrderFact, AccountOrderSide, AccountOrderStatus, AiDecisionContext,
        AiDecisionContextId, AiOrderType, AiProviderResponseId, AiRawResponse,
        AiTradePlanLedgerEntry, AiTradingPlan, AiTradingPlanId, AssetCode, AuditEntryId,
        ClosedCandle, DomainDecimal, ExchangeAssetBalance, ExchangeServerTime,
        FEATURE_CANDLE_WINDOW, FillId, InstrumentId, InstrumentRulesSnapshot,
        InstrumentTradingStatus, LocalAssetBalance, ManagedLotId, MarketDataSource,
        MarketFeatureEngine, MarketTimeframe, OrderId, OrderIntentId, OutboxMessageId,
        PortfolioFill, PortfolioFillSide, PortfolioReconciler, ReconciliationRunId, RulesHash,
        RuntimeInstanceId, SnapshotId, SpotInstrumentRules, SystemState, TopOfBook,
        TradePlanActionId, TradePlanId, TradePlanState, validated_spot_instrument_rules,
    };
    use serde_json::json;

    use crate::{
        HistoricalValidationFacts, MinimalHistoricalHarnessError, MinimalHistoricalReplayInput,
        PaperRuntimeActionAttempt, PaperRuntimeAiProvider, PaperRuntimeCycleId,
        PaperRuntimeCycleInput, PaperRuntimeEffect, PaperRuntimeError, PaperRuntimeFacts,
        PaperRuntimeOutcome, PaperRuntimeProviderError, PaperRuntimeProviderFuture,
        RuntimeAiGeneration, SqliteAiPaperRuntime, SqliteMinimalHistoricalHarness,
    };

    static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);
    const AI_END_AT: i64 = 1_800_000_000_000;

    #[tokio::test]
    async fn migrations_enable_wal_and_create_only_the_planned_storage_kernel() {
        let fixture = Fixture::new().await;

        let journal_mode: String = sqlx::query_scalar("PRAGMA journal_mode")
            .fetch_one(fixture.repository.pool())
            .await
            .expect("journal mode should be readable");
        assert_eq!(journal_mode.to_ascii_lowercase(), "wal");

        let table_names: Vec<String> = sqlx::query_scalar(
            "
            SELECT name
            FROM sqlite_master
            WHERE type = 'table' AND name NOT LIKE 'sqlite_%' AND name != '_sqlx_migrations'
            ORDER BY name
            ",
        )
        .fetch_all(fixture.repository.pool())
        .await
        .expect("schema should be readable");
        assert_eq!(
            table_names,
            vec![
                "ai_decision_contexts",
                "ai_provider_attempts",
                "ai_provider_responses",
                "ai_trade_plan_ledger",
                "ai_trading_plans",
                "audit_log",
                "bybit_execution_facts",
                "bybit_order_acks",
                "bybit_order_facts",
                "bybit_private_events",
                "bybit_private_sync_state",
                "bybit_wallet_facts",
                "eligibility_events",
                "emergency_action_steps",
                "emergency_actions",
                "emergency_fills",
                "execution_validations",
                "fills",
                "managed_lots",
                "market_snapshots",
                "materialized_trade_parameters",
                "order_intents",
                "outbox",
                "paper_execution_submissions",
                "paper_market_observations",
                "paper_order_specs",
                "paper_orders",
                "paper_runtime_events",
                "paper_soak_fault_evidence",
                "paper_soak_observations",
                "paper_soak_runs",
                "reconciliation_runs",
                "risk_decisions",
                "runtime_instance_lease",
                "strategy_intents",
                "system_state",
                "trade_plan_actions",
                "trade_plans",
            ]
        );
        fixture.close().await;
    }

    #[tokio::test]
    async fn v2_authority_tables_are_preserved_as_immutable_evidence() {
        let fixture = Fixture::new().await;

        for (table, statement, expected) in [
            (
                "strategy_intents",
                "INSERT INTO strategy_intents(
                    decision_id, event_id, schema_version, strategy_space_version,
                    decided_at, expires_at, payload_json
                 ) VALUES ('decision', 'event', '2.0', 'legacy', 1, 2, '{}')",
                "strategy_intents is retired v2 evidence",
            ),
            (
                "materialized_trade_parameters",
                "INSERT INTO materialized_trade_parameters(
                    decision_id, algorithm_version, materialized_at, payload_json
                 ) VALUES ('decision', 'legacy', 1, '{}')",
                "materialized_trade_parameters is retired v2 evidence",
            ),
            (
                "risk_decisions",
                "INSERT INTO risk_decisions(
                    risk_decision_id, decision_id, rules_version, outcome, decided_at, payload_json
                 ) VALUES ('risk', 'decision', 'legacy', 'REJECT', 1, '{}')",
                "risk_decisions is retired v2 evidence",
            ),
        ] {
            let error = sqlx::query(statement)
                .execute(fixture.repository.pool())
                .await
                .expect_err("retired v2 authority table must reject new writes");
            assert!(
                error.to_string().contains(expected),
                "{table} rejected for an unexpected reason: {error}"
            );
        }

        let trigger_count: i64 = sqlx::query_scalar(
            "
            SELECT COUNT(*)
            FROM sqlite_master
            WHERE type = 'trigger' AND name LIKE 'legacy_%_forbid_%'
            ",
        )
        .fetch_one(fixture.repository.pool())
        .await
        .expect("legacy retirement triggers should be readable");
        assert_eq!(trigger_count, 9);

        fixture.close().await;
    }

    #[tokio::test]
    async fn ai_context_response_plan_and_actions_are_atomic_traceable_and_idempotent() {
        let fixture = Fixture::new().await;
        let owner = runtime_id(700);
        fixture
            .repository
            .acquire_instance_lease(
                owner,
                timestamp(AI_END_AT),
                std::time::Duration::from_secs(60),
            )
            .await
            .expect("runtime lease should be acquired");

        let trade_plan_id = trade_plan_id(700);
        let open = ai_ledger_entry(700, "OPEN_LONG", trade_plan_id);
        let open_audit = ai_ledger_audit(&open, 700);
        assert_eq!(
            fixture
                .repository
                .persist_ai_trade_plan_ledger(owner, &open, &open_audit)
                .await
                .expect("OPEN_LONG ledger should persist"),
            PersistenceEffect::Applied
        );

        let hold = ai_ledger_entry(710, "HOLD", trade_plan_id);
        let hold_audit = ai_ledger_audit(&hold, 710);
        assert_eq!(
            fixture
                .repository
                .persist_ai_trade_plan_ledger(owner, &hold, &hold_audit)
                .await
                .expect("HOLD ledger should append"),
            PersistenceEffect::Applied
        );
        assert_eq!(
            fixture
                .repository
                .persist_ai_trade_plan_ledger(owner, &hold, &hold_audit)
                .await
                .expect("same ledger append should be idempotent"),
            PersistenceEffect::DuplicateNoEffect
        );

        let trace = fixture
            .repository
            .ai_trade_plan_trace(hold.action_id())
            .await
            .expect("trace query should succeed")
            .expect("trace must exist");
        assert_eq!(trace.action_id, hold.action_id().to_string());
        assert_eq!(trace.trade_plan_id, trade_plan_id.to_string());
        assert_eq!(trace.context_id, hold.context().context_id().to_string());
        assert_eq!(trace.response_id, hold.response().response_id().to_string());
        assert_eq!(trace.ai_plan_id, hold.plan().plan_id().to_string());
        assert_eq!(
            trace.context_hash,
            hold.context().context_hash().to_string()
        );
        assert_eq!(
            trace.response_hash,
            hold.response().response_hash().to_string()
        );
        assert_eq!(trace.plan_hash, hold.plan().plan_hash().to_string());
        assert_eq!(trace.action, "HOLD");

        for (table, expected) in [
            ("ai_decision_contexts", 2),
            ("ai_provider_responses", 2),
            ("ai_trading_plans", 2),
            ("ai_trade_plan_ledger", 2),
            ("trade_plans", 1),
            ("trade_plan_actions", 2),
            ("audit_log", 2),
        ] {
            assert_eq!(
                ai_table_count(&fixture.repository, table).await,
                expected,
                "unexpected row count for {table}"
            );
        }

        fixture.close().await;
    }

    #[tokio::test]
    async fn accepted_validation_is_atomic_idempotent_and_creates_no_order() {
        let fixture = Fixture::new().await;
        let owner = runtime_id(715);
        fixture
            .repository
            .acquire_instance_lease(
                owner,
                timestamp(AI_END_AT),
                std::time::Duration::from_secs(60),
            )
            .await
            .expect("runtime lease should be acquired");

        let entry = ai_ledger_entry(715, "OPEN_LONG", trade_plan_id(715));
        fixture
            .repository
            .persist_ai_trade_plan_ledger(owner, &entry, &ai_ledger_audit(&entry, 715))
            .await
            .expect("OPEN_LONG ledger should persist");
        let decision = accepted_execution_validation(&entry);
        let validation_audit = execution_validation_audit(&decision, 715);
        assert_eq!(
            fixture
                .repository
                .persist_execution_validation(owner, &decision, &validation_audit)
                .await
                .expect("validation should persist"),
            PersistenceEffect::Applied
        );
        assert_eq!(
            fixture
                .repository
                .persist_execution_validation(owner, &decision, &validation_audit)
                .await
                .expect("same validation should be idempotent"),
            PersistenceEffect::DuplicateNoEffect
        );

        let row = fixture
            .repository
            .execution_validation(entry.action_id())
            .await
            .expect("validation query should succeed")
            .expect("validation must exist");
        assert_eq!(row.outcome, "ACCEPT");
        assert_eq!(row.plan_hash, entry.plan().plan_hash().to_string());
        assert_eq!(row.validation_hash, decision.validation_hash().to_string());
        let plan_state: String =
            sqlx::query_scalar("SELECT state FROM trade_plans WHERE trade_plan_id = ?")
                .bind(entry.trade_plan_id().to_string())
                .fetch_one(fixture.repository.pool())
                .await
                .expect("TradePlan state should be readable");
        let action_state: String =
            sqlx::query_scalar("SELECT state FROM trade_plan_actions WHERE action_id = ?")
                .bind(entry.action_id().to_string())
                .fetch_one(fixture.repository.pool())
                .await
                .expect("action state should be readable");
        let order_intents: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM order_intents")
            .fetch_one(fixture.repository.pool())
            .await
            .expect("order-intent count should be readable");
        assert_eq!(plan_state, "ACCEPTED");
        assert_eq!(action_state, "VALIDATION_ACCEPTED");
        assert_eq!(order_intents, 0, "P3-13 must never create an order intent");

        fixture.close().await;
    }

    #[tokio::test]
    async fn paper_execution_is_exact_partial_and_idempotent_without_decision_bar_reuse() {
        let fixture = Fixture::new().await;
        let owner = runtime_id(717);
        fixture
            .repository
            .acquire_instance_lease(
                owner,
                timestamp(AI_END_AT),
                std::time::Duration::from_secs(60),
            )
            .await
            .expect("runtime lease should be acquired");
        let entry = ai_ledger_entry(717, "OPEN_LONG", trade_plan_id(717));
        fixture
            .repository
            .persist_ai_trade_plan_ledger(owner, &entry, &ai_ledger_audit(&entry, 717))
            .await
            .expect("OPEN_LONG ledger should persist");
        let decision = accepted_execution_validation(&entry);
        fixture
            .repository
            .persist_execution_validation(
                owner,
                &decision,
                &execution_validation_audit(&decision, 717),
            )
            .await
            .expect("validation should persist");

        let ids = ExecutionOrderIdSet::new(
            Some(ExecutionOrderIds::new(order_intent_id(717), order_id(717))),
            Some(ExecutionOrderIds::new(order_intent_id(718), order_id(718))),
            vec![ExecutionOrderIds::new(order_intent_id(719), order_id(719))],
        )
        .expect("execution IDs should be valid");
        let end_at = u64::try_from(AI_END_AT).expect("timestamp fits u64");
        let request = SpotExecutionRequest::from_accepted_plan(
            entry.context(),
            &decision,
            entry.plan(),
            ids,
            end_at + 5_000,
        )
        .expect("accepted plan should produce an execution request");
        let policy =
            PaperExecutionPolicy::new(decimal("0.001"), decimal("0.002"), decimal("0.001"))
                .expect("paper policy should be valid");
        let port = crate::SqlitePaperExecutionPort::new(&fixture.repository, owner, policy);
        assert_eq!(
            port.submit(&request)
                .await
                .expect("paper submission should succeed")
                .effect(),
            ironpilot_application::ExecutionEffect::Applied
        );
        assert_eq!(
            port.submit(&request)
                .await
                .expect("duplicate paper submission should succeed")
                .effect(),
            ironpilot_application::ExecutionEffect::DuplicateNoEffect
        );

        let reused = PaperMarketObservation::new(
            snapshot_id(717),
            instrument(),
            entry.context().as_of_unix_millis(),
            end_at + 5_001,
            decimal("209.9"),
            decimal("210.1"),
            decimal("209"),
            decimal("211"),
            decimal("0.04"),
        )
        .expect("reused observation fixture should be structurally valid");
        assert!(matches!(
            port.process_observation(&reused, &rules()).await,
            Err(crate::PaperExecutionAdapterError::Paper(
                PaperExecutionError::DecisionBarReuse
            ))
        ));
        let observations: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM paper_market_observations")
                .fetch_one(fixture.repository.pool())
                .await
                .expect("observation count should be readable");
        assert_eq!(observations, 0, "reused fact must roll back atomically");

        let first = PaperMarketObservation::new(
            snapshot_id(718),
            instrument(),
            end_at + 6_000,
            end_at + 6_001,
            decimal("209.9"),
            decimal("210.1"),
            decimal("209"),
            decimal("211"),
            decimal("0.04"),
        )
        .expect("first observation should be valid");
        let first_report = port
            .process_observation(&first, &rules())
            .await
            .expect("partial paper fill should persist");
        assert_eq!(first_report.fill_ids().len(), 1);
        assert_eq!(
            port.process_observation(&first, &rules())
                .await
                .expect("duplicate observation should be idempotent")
                .effect(),
            PersistenceEffect::DuplicateNoEffect
        );
        let partial: (String, String) = sqlx::query_as(
            "
            SELECT orders.state, specs.filled_quantity
            FROM paper_orders AS orders
            JOIN paper_order_specs AS specs ON specs.order_id = orders.order_id
            WHERE orders.order_id = ?
            ",
        )
        .bind(order_id(717).to_string())
        .fetch_one(fixture.repository.pool())
        .await
        .expect("partial entry should be readable");
        assert_eq!(partial, ("PARTIALLY_FILLED".to_owned(), "0.04".to_owned()));

        let second = PaperMarketObservation::new(
            snapshot_id(719),
            instrument(),
            end_at + 7_000,
            end_at + 7_001,
            decimal("209.9"),
            decimal("210.1"),
            decimal("209"),
            decimal("211"),
            decimal("0.06"),
        )
        .expect("second observation should be valid");
        port.process_observation(&second, &rules())
            .await
            .expect("remaining paper fill should persist");
        let (plan_state, entry_state, stop_state, take_profit_state): (
            String,
            String,
            String,
            String,
        ) = sqlx::query_as(
            "
            SELECT
                plans.state,
                entry_order.state,
                stop_order.state,
                take_profit_order.state
            FROM trade_plans AS plans
            JOIN paper_orders AS entry_order ON entry_order.order_id = ?
            JOIN paper_orders AS stop_order ON stop_order.order_id = ?
            JOIN paper_orders AS take_profit_order ON take_profit_order.order_id = ?
            WHERE plans.trade_plan_id = ?
            ",
        )
        .bind(order_id(717).to_string())
        .bind(order_id(718).to_string())
        .bind(order_id(719).to_string())
        .bind(entry.trade_plan_id().to_string())
        .fetch_one(fixture.repository.pool())
        .await
        .expect("paper execution states should be readable");
        assert_eq!(plan_state, "ACTIVE");
        assert_eq!(entry_state, "FILLED");
        assert_eq!(stop_state, "ACTIVE");
        assert_eq!(take_profit_state, "ACTIVE");
        assert_eq!(
            fixture
                .repository
                .managed_position(&instrument(), asset("BTC"))
                .await
                .expect("managed position should be readable")
                .quantity(),
            decimal("0.10")
        );
        let stored_order_payload: String =
            sqlx::query_scalar("SELECT payload_json FROM paper_orders WHERE order_id = ?")
                .bind(order_id(717).to_string())
                .fetch_one(fixture.repository.pool())
                .await
                .expect("stored order payload should be readable");
        assert_eq!(stored_order_payload, request.orders()[0].payload_json());

        let stop_trigger = PaperMarketObservation::new(
            snapshot_id(720),
            instrument(),
            end_at + 8_000,
            end_at + 8_001,
            decimal("199"),
            decimal("199.2"),
            decimal("198"),
            decimal("201"),
            decimal("0.10"),
        )
        .expect("stop observation should be valid");
        let stop_report = port
            .process_observation(&stop_trigger, &rules())
            .await
            .expect("protective stop should persist");
        assert_eq!(stop_report.fill_ids().len(), 1);
        let (closed_plan_state, stop_order_state, cancelled_take_profit): (String, String, String) =
            sqlx::query_as(
                "
            SELECT plans.state, stop_order.state, take_profit_order.state
            FROM trade_plans AS plans
            JOIN paper_orders AS stop_order ON stop_order.order_id = ?
            JOIN paper_orders AS take_profit_order ON take_profit_order.order_id = ?
            WHERE plans.trade_plan_id = ?
            ",
            )
            .bind(order_id(718).to_string())
            .bind(order_id(719).to_string())
            .bind(entry.trade_plan_id().to_string())
            .fetch_one(fixture.repository.pool())
            .await
            .expect("protective close states should be readable");
        assert_eq!(closed_plan_state, "CLOSED");
        assert_eq!(stop_order_state, "FILLED");
        assert_eq!(cancelled_take_profit, "CANCELLED");
        assert_eq!(
            fixture
                .repository
                .managed_position(&instrument(), asset("BTC"))
                .await
                .expect("closed managed position should be readable")
                .quantity(),
            DomainDecimal::ZERO
        );
        let stop_fill_payload: String =
            sqlx::query_scalar("SELECT payload_json FROM fills WHERE fill_id = ?")
                .bind(stop_report.fill_ids()[0].to_string())
                .fetch_one(fixture.repository.pool())
                .await
                .expect("stop fill payload should be readable");
        let stop_fill_payload: serde_json::Value =
            serde_json::from_str(&stop_fill_payload).expect("stop fill payload should be JSON");
        assert_eq!(stop_fill_payload["execution_price"], "199");
        assert_eq!(stop_fill_payload["fee_quote"], "0.0398");

        fixture.close().await;
    }

    #[tokio::test]
    async fn minimal_historical_harness_is_deterministic_and_prefix_stable() {
        let first_fixture = Fixture::new().await;
        let second_fixture = Fixture::new().await;
        let extended_fixture = Fixture::new().await;
        let owner = runtime_id(760);
        for repository in [
            &first_fixture.repository,
            &second_fixture.repository,
            &extended_fixture.repository,
        ] {
            repository
                .acquire_instance_lease(
                    owner,
                    timestamp(AI_END_AT),
                    std::time::Duration::from_secs(60),
                )
                .await
                .expect("historical runtime lease should be acquired");
        }

        let short_input = historical_input(760, false, false);
        let extended_input = historical_input(760, true, false);
        let policy =
            PaperExecutionPolicy::new(decimal("0.001"), decimal("0.002"), decimal("0.001"))
                .expect("historical paper policy should be valid");
        let first_report =
            SqliteMinimalHistoricalHarness::new(&first_fixture.repository, owner, policy)
                .run(&short_input)
                .await
                .expect("first historical replay should succeed");
        let second_report =
            SqliteMinimalHistoricalHarness::new(&second_fixture.repository, owner, policy)
                .run(&short_input)
                .await
                .expect("identical historical replay should succeed");
        let extended_report =
            SqliteMinimalHistoricalHarness::new(&extended_fixture.repository, owner, policy)
                .run(&extended_input)
                .await
                .expect("extended historical replay should succeed");

        assert_eq!(
            first_report, second_report,
            "same recorded Context, AI plan, validation facts, IDs, fees, slippage and observations must produce the same report ledger"
        );
        assert_eq!(
            historical_ledger_rows(&first_fixture.repository).await,
            historical_ledger_rows(&second_fixture.repository).await,
            "same input must persist the same canonical SQLite ledger"
        );
        assert_eq!(
            first_report.records(),
            &extended_report.records()[..first_report.records().len()],
            "adding later observations must not change the existing ledger prefix"
        );
        assert_ne!(
            first_report.ledger_hash(),
            extended_report.ledger_hash(),
            "the appended stop execution must extend the cumulative ledger"
        );
        assert_eq!(first_report.fill_ids().len(), 2);
        assert_eq!(extended_report.fill_ids().len(), 3);

        let first_fill: String = sqlx::query_scalar(
            "SELECT payload_json FROM fills ORDER BY occurred_at, fill_id LIMIT 1",
        )
        .fetch_one(first_fixture.repository.pool())
        .await
        .expect("historical fill should be readable");
        let first_fill: serde_json::Value =
            serde_json::from_str(&first_fill).expect("historical fill should be JSON");
        assert_eq!(first_fill["execution_price"], "210");
        assert_eq!(first_fill["fee_quote"], "0.0084");

        first_fixture.close().await;
        second_fixture.close().await;
        extended_fixture.close().await;
    }

    #[tokio::test]
    async fn minimal_historical_harness_rejects_decision_fact_reuse_before_writes() {
        let fixture = Fixture::new().await;
        let owner = runtime_id(761);
        fixture
            .repository
            .acquire_instance_lease(
                owner,
                timestamp(AI_END_AT),
                std::time::Duration::from_secs(60),
            )
            .await
            .expect("historical runtime lease should be acquired");
        let input = historical_input(761, false, true);
        let policy =
            PaperExecutionPolicy::new(decimal("0.001"), decimal("0.002"), decimal("0.001"))
                .expect("historical paper policy should be valid");
        let result = SqliteMinimalHistoricalHarness::new(&fixture.repository, owner, policy)
            .run(&input)
            .await;
        assert!(matches!(
            result,
            Err(MinimalHistoricalHarnessError::DecisionFactReuse)
        ));
        assert_eq!(
            ai_table_count(&fixture.repository, "ai_decision_contexts").await,
            0,
            "look-ahead rejection must happen before any ledger write"
        );
        fixture.close().await;
    }

    #[tokio::test]
    async fn ai_paper_runtime_opens_reviews_exits_and_restarts_without_duplicate_ai() {
        let fixture = Fixture::new().await;
        let owner = runtime_id(800);
        fixture
            .repository
            .acquire_instance_lease(
                owner,
                timestamp(AI_END_AT),
                std::time::Duration::from_secs(120),
            )
            .await
            .expect("paper runtime lease should be acquired");
        let target_plan = trade_plan_id(800);
        let provider = ScriptedRuntimeProvider::new(vec![
            ScriptedRuntimeStep::plan(800, "OPEN_LONG", None, "2.00"),
            ScriptedRuntimeStep::plan(801, "EXIT", Some(target_plan), "0"),
        ]);
        let policy =
            PaperExecutionPolicy::new(decimal("0.001"), decimal("0.002"), decimal("0.001"))
                .expect("paper runtime policy should be valid");
        let runtime = SqliteAiPaperRuntime::new(&fixture.repository, owner, &provider, policy);

        let (open_facts, open_validation) =
            paper_runtime_fact_bundle(800, "BTCUSDT", "BTC", false, false, false);
        let open_input = PaperRuntimeCycleInput::new(
            cycle_id(800),
            u64::try_from(AI_END_AT).expect("timestamp fits u64") + 1_000,
            open_facts,
            vec![paper_runtime_attempt(
                800,
                target_plan,
                open_validation,
                true,
                0,
            )],
            vec![
                paper_runtime_observation(800, "BTCUSDT", 6_000, "209", "211", "0.04"),
                paper_runtime_observation(801, "BTCUSDT", 7_000, "209", "211", "0.06"),
            ],
        )
        .expect("OPEN_LONG runtime input should be valid");
        let open_report = runtime
            .run_cycle(&open_input)
            .await
            .expect("OPEN_LONG runtime cycle should succeed");
        assert_eq!(open_report.outcome(), PaperRuntimeOutcome::Executed);
        assert_eq!(open_report.action(), Some("OPEN_LONG"));
        assert!(open_report.context_hash().is_some());
        assert!(open_report.plan_hash().is_some());
        assert!(open_report.validation_hash().is_some());
        assert!(open_report.execution_request_hash().is_some());
        assert_eq!(open_report.fill_ids().len(), 2);
        assert_eq!(open_report.local_parameter_mutations(), 0);

        let (exit_facts, exit_validation) =
            paper_runtime_fact_bundle(801, "BTCUSDT", "BTC", true, false, false);
        let exit_input = PaperRuntimeCycleInput::new(
            cycle_id(801),
            u64::try_from(AI_END_AT).expect("timestamp fits u64") + 10_000,
            exit_facts,
            vec![paper_runtime_attempt(
                801,
                target_plan,
                exit_validation,
                false,
                9_000,
            )],
            vec![paper_runtime_observation(
                802, "BTCUSDT", 16_000, "218", "220", "0.10",
            )],
        )
        .expect("EXIT runtime input should be valid");
        let exit_report = runtime
            .run_cycle(&exit_input)
            .await
            .expect("AI review EXIT cycle should succeed");
        assert_eq!(exit_report.outcome(), PaperRuntimeOutcome::Executed);
        assert_eq!(exit_report.action(), Some("EXIT"));
        assert_eq!(exit_report.fill_ids().len(), 1);
        assert_eq!(
            fixture
                .repository
                .managed_position(&runtime_instrument("BTCUSDT"), asset("BTC"))
                .await
                .expect("managed position should be readable")
                .quantity(),
            DomainDecimal::ZERO
        );
        let plan_state: String =
            sqlx::query_scalar("SELECT state FROM trade_plans WHERE trade_plan_id = ?")
                .bind(target_plan.to_string())
                .fetch_one(fixture.repository.pool())
                .await
                .expect("closed TradePlan should be readable");
        assert_eq!(plan_state, "CLOSED");

        let calls_before_restart = provider.calls();
        let replayed = runtime
            .run_cycle(&exit_input)
            .await
            .expect("completed cycle restart should recover the terminal report");
        assert_eq!(replayed.effect(), PaperRuntimeEffect::DuplicateNoEffect);
        assert_eq!(
            provider.calls(),
            calls_before_restart,
            "restart must not call AI or automatically open a position"
        );
        assert_runtime_trace_complete(&fixture.repository, cycle_id(800), true).await;
        assert_runtime_trace_complete(&fixture.repository, cycle_id(801), true).await;
        let runtime_trace_update =
            sqlx::query("UPDATE paper_runtime_events SET event_type = 'TAMPERED'")
                .execute(fixture.repository.pool())
                .await;
        assert!(
            runtime_trace_update.is_err(),
            "paper runtime trace must be append-only"
        );
        fixture.close().await;
    }

    #[tokio::test]
    async fn ai_paper_runtime_failures_are_traced_and_create_zero_orders() {
        let fixture = Fixture::new().await;
        let owner = runtime_id(810);
        fixture
            .repository
            .acquire_instance_lease(
                owner,
                timestamp(AI_END_AT),
                std::time::Duration::from_secs(120),
            )
            .await
            .expect("paper runtime lease should be acquired");
        let provider = ScriptedRuntimeProvider::new(vec![
            ScriptedRuntimeStep::failure("CALL_BUDGET_EXHAUSTED"),
            ScriptedRuntimeStep::failure("INVALID_PLAN"),
            ScriptedRuntimeStep::plan(812, "OPEN_LONG", None, "100.00"),
            ScriptedRuntimeStep::plan(813, "NO_TRADE", None, "0"),
            ScriptedRuntimeStep::plan(815, "OPEN_LONG", None, "2.00"),
        ]);
        let policy =
            PaperExecutionPolicy::new(decimal("0.001"), decimal("0.002"), decimal("0.001"))
                .expect("paper runtime policy should be valid");
        let runtime = SqliteAiPaperRuntime::new(&fixture.repository, owner, &provider, policy);

        for (sequence, expected_code) in [
            (810_u128, "CALL_BUDGET_EXHAUSTED"),
            (811_u128, "INVALID_PLAN"),
        ] {
            let (facts, validation) =
                paper_runtime_fact_bundle(sequence, "BTCUSDT", "BTC", false, false, false);
            let input = PaperRuntimeCycleInput::new(
                cycle_id(sequence),
                u64::try_from(AI_END_AT).expect("timestamp fits u64") + 1_000,
                facts,
                vec![paper_runtime_attempt(
                    sequence,
                    trade_plan_id(sequence),
                    validation,
                    true,
                    0,
                )],
                Vec::new(),
            )
            .expect("provider failure input should be valid");
            let report = runtime
                .run_cycle(&input)
                .await
                .expect("provider failure should fail closed as NO_ACTION");
            assert_eq!(report.outcome(), PaperRuntimeOutcome::ProviderNoAction);
            assert_eq!(report.failure_code(), Some(expected_code));
            assert_runtime_trace_complete(&fixture.repository, cycle_id(sequence), false).await;
        }

        let (facts, first_validation) =
            paper_runtime_fact_bundle(812, "BTCUSDT", "BTC", false, false, false);
        let (_, second_validation) =
            paper_runtime_fact_bundle(813, "BTCUSDT", "BTC", false, false, false);
        let replan_input = PaperRuntimeCycleInput::new(
            cycle_id(812),
            u64::try_from(AI_END_AT).expect("timestamp fits u64") + 1_000,
            facts,
            vec![
                paper_runtime_attempt(812, trade_plan_id(812), first_validation, true, 0),
                paper_runtime_attempt(813, trade_plan_id(813), second_validation, false, 0),
            ],
            Vec::new(),
        )
        .expect("bounded replan input should be valid");
        let replan_report = runtime
            .run_cycle(&replan_input)
            .await
            .expect("over-authorized plan should permit one bounded AI replan");
        assert_eq!(replan_report.outcome(), PaperRuntimeOutcome::NoTrade);
        assert_eq!(replan_report.provider_attempts(), 2);
        assert_eq!(replan_report.validation_attempts(), 2);

        let (stale_facts, stale_validation) =
            paper_runtime_fact_bundle(814, "BTCUSDT", "BTC", false, true, false);
        let stale_input = PaperRuntimeCycleInput::new(
            cycle_id(814),
            u64::try_from(AI_END_AT).expect("timestamp fits u64") + 70_000,
            stale_facts,
            vec![paper_runtime_attempt(
                814,
                trade_plan_id(814),
                stale_validation,
                true,
                69_000,
            )],
            Vec::new(),
        )
        .expect("stale runtime input should be structurally valid");
        let stale_report = runtime
            .run_cycle(&stale_input)
            .await
            .expect("stale facts should produce a traced context rejection");
        assert_eq!(stale_report.outcome(), PaperRuntimeOutcome::ContextRejected);

        let (changed_facts, changed_validation) =
            paper_runtime_fact_bundle(815, "BTCUSDT", "BTC", false, false, true);
        let changed_input = PaperRuntimeCycleInput::new(
            cycle_id(815),
            u64::try_from(AI_END_AT).expect("timestamp fits u64") + 1_000,
            changed_facts,
            vec![paper_runtime_attempt(
                815,
                trade_plan_id(815),
                changed_validation,
                true,
                0,
            )],
            Vec::new(),
        )
        .expect("changed account input should be valid");
        let changed_report = runtime
            .run_cycle(&changed_input)
            .await
            .expect("post-Context order change should be traced");
        assert_eq!(
            changed_report.outcome(),
            PaperRuntimeOutcome::ValidationRejected
        );
        assert_eq!(changed_report.failure_code(), Some("VALIDATION_REJECTED"));

        let (recovery_facts, recovery_validation) =
            paper_runtime_fact_bundle(816, "BTCUSDT", "BTC", false, false, false);
        let recovery_input = PaperRuntimeCycleInput::new(
            cycle_id(816),
            u64::try_from(AI_END_AT).expect("timestamp fits u64") + 1_000,
            recovery_facts,
            vec![paper_runtime_attempt(
                816,
                trade_plan_id(816),
                recovery_validation,
                true,
                0,
            )],
            Vec::new(),
        )
        .expect("recovery input should be valid");
        sqlx::query(
            "
            INSERT INTO paper_runtime_events(
                event_id, cycle_id, sequence, instrument_id, context_id,
                event_type, occurred_at, payload_json
            )
            VALUES (?, ?, 0, ?, NULL, 'CONTEXT_BUILT', ?, '{}')
            ",
        )
        .bind(uuid_text(816 + 90_000))
        .bind(cycle_id(816).to_string())
        .bind(runtime_instrument("BTCUSDT").to_string())
        .bind(AI_END_AT + 1_000)
        .execute(fixture.repository.pool())
        .await
        .expect("incomplete runtime trace fixture should insert");
        let calls_before_recovery = provider.calls();
        assert!(matches!(
            runtime.run_cycle(&recovery_input).await,
            Err(PaperRuntimeError::RecoveryRequired)
        ));
        assert_eq!(
            provider.calls(),
            calls_before_recovery,
            "incomplete restart must restore persisted facts before calling AI"
        );

        let order_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM order_intents")
            .fetch_one(fixture.repository.pool())
            .await
            .expect("order count should be readable");
        assert_eq!(
            order_count, 0,
            "budget, invalid output, over-authorization/replan NO_TRADE and stale facts must create zero orders"
        );
        fixture.close().await;
    }

    #[tokio::test]
    async fn ai_paper_runtime_supports_multiple_spot_instruments_without_shared_state() {
        let fixture = Fixture::new().await;
        let owner = runtime_id(820);
        fixture
            .repository
            .acquire_instance_lease(
                owner,
                timestamp(AI_END_AT),
                std::time::Duration::from_secs(120),
            )
            .await
            .expect("paper runtime lease should be acquired");
        let provider = ScriptedRuntimeProvider::new(vec![
            ScriptedRuntimeStep::plan(820, "OPEN_LONG", None, "2.00"),
            ScriptedRuntimeStep::plan(821, "OPEN_LONG", None, "2.00"),
        ]);
        let policy =
            PaperExecutionPolicy::new(decimal("0.001"), decimal("0.002"), decimal("0.001"))
                .expect("paper runtime policy should be valid");
        let runtime = SqliteAiPaperRuntime::new(&fixture.repository, owner, &provider, policy);

        for (sequence, symbol, base) in [(820_u128, "BTCUSDT", "BTC"), (821_u128, "ETHUSDT", "ETH")]
        {
            let (facts, validation) =
                paper_runtime_fact_bundle(sequence, symbol, base, false, false, false);
            let input = PaperRuntimeCycleInput::new(
                cycle_id(sequence),
                u64::try_from(AI_END_AT).expect("timestamp fits u64") + 1_000,
                facts,
                vec![paper_runtime_attempt(
                    sequence,
                    trade_plan_id(sequence),
                    validation,
                    true,
                    0,
                )],
                Vec::new(),
            )
            .expect("multi-instrument cycle should be valid");
            assert_eq!(
                runtime
                    .run_cycle(&input)
                    .await
                    .expect("multi-instrument cycle should succeed")
                    .outcome(),
                PaperRuntimeOutcome::Executed
            );
        }
        let instruments: Vec<String> =
            sqlx::query_scalar("SELECT instrument_id FROM trade_plans ORDER BY instrument_id")
                .fetch_all(fixture.repository.pool())
                .await
                .expect("runtime TradePlan instruments should be readable");
        assert_eq!(
            instruments,
            vec!["bybit:spot:BTCUSDT", "bybit:spot:ETHUSDT"]
        );
        fixture.close().await;
    }

    #[tokio::test]
    async fn rejected_validation_closes_the_plan_and_illegal_plan_creates_zero_orders() {
        let fixture = Fixture::new().await;
        let owner = runtime_id(716);
        fixture
            .repository
            .acquire_instance_lease(
                owner,
                timestamp(AI_END_AT),
                std::time::Duration::from_secs(60),
            )
            .await
            .expect("runtime lease should be acquired");
        let entry = ai_ledger_entry(716, "OPEN_LONG", trade_plan_id(716));
        fixture
            .repository
            .persist_ai_trade_plan_ledger(owner, &entry, &ai_ledger_audit(&entry, 716))
            .await
            .expect("OPEN_LONG ledger should persist");

        let decision = execution_validation(&entry, ExecutionMode::ObserveOnly);
        assert_eq!(decision.outcome(), ExecutionValidationOutcome::Reject);
        fixture
            .repository
            .persist_execution_validation(
                owner,
                &decision,
                &execution_validation_audit(&decision, 716),
            )
            .await
            .expect("rejected validation should persist");

        let (plan_state, action_state): (String, String) = sqlx::query_as(
            "
            SELECT plans.state, actions.state
            FROM trade_plans AS plans
            JOIN trade_plan_actions AS actions
              ON actions.trade_plan_id = plans.trade_plan_id
            WHERE actions.action_id = ?
            ",
        )
        .bind(entry.action_id().to_string())
        .fetch_one(fixture.repository.pool())
        .await
        .expect("validation states should be readable");
        let order_intents: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM order_intents")
            .fetch_one(fixture.repository.pool())
            .await
            .expect("order-intent count should be readable");
        assert_eq!(plan_state, "REJECTED");
        assert_eq!(action_state, "VALIDATION_REJECTED");
        assert_eq!(order_intents, 0);

        fixture.close().await;
    }

    #[tokio::test]
    async fn second_active_plan_is_rejected_and_all_candidate_rows_roll_back() {
        let fixture = Fixture::new().await;
        let owner = runtime_id(720);
        fixture
            .repository
            .acquire_instance_lease(
                owner,
                timestamp(AI_END_AT),
                std::time::Duration::from_secs(60),
            )
            .await
            .expect("runtime lease should be acquired");

        let first = ai_ledger_entry(720, "OPEN_LONG", trade_plan_id(720));
        fixture
            .repository
            .persist_ai_trade_plan_ledger(owner, &first, &ai_ledger_audit(&first, 720))
            .await
            .expect("first active plan should persist");

        let second = ai_ledger_entry(730, "OPEN_LONG", trade_plan_id(730));
        assert!(matches!(
            fixture
                .repository
                .persist_ai_trade_plan_ledger(owner, &second, &ai_ledger_audit(&second, 730))
                .await,
            Err(StorageError::ActiveTradePlanExists { .. })
        ));
        assert_eq!(ai_table_count(&fixture.repository, "trade_plans").await, 1);
        assert_eq!(
            ai_table_count(&fixture.repository, "ai_decision_contexts").await,
            1
        );
        assert_eq!(
            ai_table_count(&fixture.repository, "ai_provider_responses").await,
            1
        );
        assert_eq!(
            ai_table_count(&fixture.repository, "ai_trading_plans").await,
            1
        );
        assert!(
            fixture
                .repository
                .ai_trade_plan_trace(second.action_id())
                .await
                .expect("trace query should succeed")
                .is_none()
        );

        fixture.close().await;
    }

    #[tokio::test]
    async fn audit_failure_rolls_back_the_complete_ai_ledger_transaction() {
        let fixture = Fixture::new().await;
        let owner = runtime_id(740);
        fixture
            .repository
            .acquire_instance_lease(
                owner,
                timestamp(AI_END_AT),
                std::time::Duration::from_secs(60),
            )
            .await
            .expect("runtime lease should be acquired");

        let first = ai_ledger_entry(740, "NO_TRADE", trade_plan_id(740));
        let first_audit = ai_ledger_audit(&first, 740);
        fixture
            .repository
            .persist_ai_trade_plan_ledger(owner, &first, &first_audit)
            .await
            .expect("first ledger should persist");

        let second = ai_ledger_entry(750, "NO_TRADE", trade_plan_id(750));
        let duplicate_audit = AuditEntry::new(
            first_audit.id(),
            timestamp(
                i64::try_from(second.recorded_at_unix_millis()).expect("test timestamp fits i64"),
            ),
            "AI_TRADE_PLAN_RECORDED",
            Some(second.plan().plan_id().to_string()),
            second.trace_json(),
        )
        .expect("audit fixture is valid");
        assert!(
            fixture
                .repository
                .persist_ai_trade_plan_ledger(owner, &second, &duplicate_audit)
                .await
                .is_err()
        );
        assert_eq!(
            ai_table_count(&fixture.repository, "ai_decision_contexts").await,
            1
        );
        assert_eq!(
            ai_table_count(&fixture.repository, "ai_trade_plan_ledger").await,
            1
        );
        assert!(
            fixture
                .repository
                .ai_trade_plan_trace(second.action_id())
                .await
                .expect("trace query should succeed")
                .is_none()
        );

        fixture.close().await;
    }

    #[tokio::test]
    async fn critical_state_audit_and_outbox_write_is_atomic() {
        let fixture = Fixture::new().await;
        let owner = runtime_id(1);
        fixture
            .repository
            .acquire_instance_lease(owner, timestamp(1_000), std::time::Duration::from_secs(60))
            .await
            .expect("first instance should acquire lease");

        let initial = state_change(
            None,
            SystemState::Starting,
            1_001,
            audit_id(1),
            outbox_id(1),
        );
        fixture
            .repository
            .persist_system_state_change(owner, &initial)
            .await
            .expect("initial write should commit");

        let duplicate_outbox = state_change(
            Some(SystemState::Starting),
            SystemState::Recovering,
            1_002,
            audit_id(2),
            outbox_id(1),
        );
        assert!(
            fixture
                .repository
                .persist_system_state_change(owner, &duplicate_outbox)
                .await
                .is_err(),
            "duplicate outbox key must reject the whole transaction"
        );

        let state = fixture
            .repository
            .system_state()
            .await
            .expect("state should be readable")
            .expect("state should exist");
        assert_eq!(state.state(), SystemState::Starting);
        assert_eq!(
            fixture
                .repository
                .audit_entries()
                .await
                .expect("audit should be readable")
                .len(),
            1
        );
        assert_eq!(
            fixture
                .repository
                .pending_outbox(10)
                .await
                .expect("outbox should be readable")
                .len(),
            1
        );
        fixture.close().await;
    }

    #[tokio::test]
    async fn audit_log_rejects_update_and_delete() {
        let fixture = Fixture::new().await;
        let owner = runtime_id(1);
        fixture
            .repository
            .acquire_instance_lease(owner, timestamp(2_000), std::time::Duration::from_secs(60))
            .await
            .expect("lease should be acquired");
        fixture
            .repository
            .persist_system_state_change(
                owner,
                &state_change(
                    None,
                    SystemState::Starting,
                    2_001,
                    audit_id(1),
                    outbox_id(1),
                ),
            )
            .await
            .expect("initial write should commit");

        let update_error = sqlx::query("UPDATE audit_log SET category = 'tampered'")
            .execute(fixture.repository.pool())
            .await
            .expect_err("audit update must fail");
        assert!(update_error.to_string().contains("append-only"));

        let delete_error = sqlx::query("DELETE FROM audit_log")
            .execute(fixture.repository.pool())
            .await
            .expect_err("audit delete must fail");
        assert!(delete_error.to_string().contains("append-only"));
        fixture.close().await;
    }

    #[tokio::test]
    async fn duplicate_buy_fill_has_zero_business_effect() {
        let fixture = Fixture::new().await;
        let owner = runtime_id(1);
        fixture
            .repository
            .acquire_instance_lease(owner, timestamp(2_500), std::time::Duration::from_secs(60))
            .await
            .expect("lease should be acquired");
        let fill = portfolio_fill(1, 1, 1, Some(1), PortfolioFillSide::Buy, "1", 2_501);
        seed_order(&fixture.repository, &fill, 1).await;
        let audit = portfolio_audit(&fill, 1);

        assert_eq!(
            fixture
                .repository
                .apply_portfolio_fill(owner, &fill, &audit)
                .await
                .expect("first fill must apply"),
            PersistenceEffect::Applied
        );
        assert_eq!(
            fixture
                .repository
                .apply_portfolio_fill(owner, &fill, &audit)
                .await
                .expect("duplicate fill must be idempotent"),
            PersistenceEffect::DuplicateNoEffect
        );
        let conflicting = portfolio_fill(1, 1, 1, Some(1), PortfolioFillSide::Buy, "2", 2_501);
        let conflict = fixture
            .repository
            .apply_portfolio_fill(owner, &conflicting, &audit)
            .await
            .expect_err("same fill ID with different content must fail");
        assert!(matches!(conflict, StorageError::IdempotencyConflict));
        let position = fixture
            .repository
            .managed_position(&instrument(), asset("BTC"))
            .await
            .expect("managed position must be readable");
        assert_eq!(position.quantity(), decimal("1"));
        assert_eq!(table_count(&fixture.repository, "fills").await, 1);
        assert_eq!(table_count(&fixture.repository, "managed_lots").await, 1);
        assert_eq!(table_count(&fixture.repository, "audit_log").await, 1);
        fixture.close().await;
    }

    #[tokio::test]
    async fn sell_fill_cannot_exceed_managed_quantity_and_duplicate_sell_is_zero_effect() {
        let fixture = Fixture::new().await;
        let owner = runtime_id(1);
        fixture
            .repository
            .acquire_instance_lease(owner, timestamp(2_600), std::time::Duration::from_secs(60))
            .await
            .expect("lease should be acquired");
        let buy = portfolio_fill(10, 10, 10, Some(10), PortfolioFillSide::Buy, "1", 2_601);
        seed_order(&fixture.repository, &buy, 10).await;
        fixture
            .repository
            .apply_portfolio_fill(owner, &buy, &portfolio_audit(&buy, 10))
            .await
            .expect("buy fill must apply");

        let oversized_sell = portfolio_fill(
            11,
            11,
            10,
            None,
            PortfolioFillSide::Sell,
            "1.00000001",
            2_602,
        );
        seed_order(&fixture.repository, &oversized_sell, 11).await;
        let error = fixture
            .repository
            .apply_portfolio_fill(
                owner,
                &oversized_sell,
                &portfolio_audit(&oversized_sell, 11),
            )
            .await
            .expect_err("oversized sell must fail");
        assert!(matches!(error, StorageError::InsufficientManagedQuantity));
        assert_eq!(table_count(&fixture.repository, "fills").await, 1);

        let sell = portfolio_fill(12, 12, 10, None, PortfolioFillSide::Sell, "0.4", 2_603);
        seed_order(&fixture.repository, &sell, 12).await;
        let audit = portfolio_audit(&sell, 12);
        assert_eq!(
            fixture
                .repository
                .apply_portfolio_fill(owner, &sell, &audit)
                .await
                .expect("managed sell must apply"),
            PersistenceEffect::Applied
        );
        assert_eq!(
            fixture
                .repository
                .apply_portfolio_fill(owner, &sell, &audit)
                .await
                .expect("duplicate sell must be idempotent"),
            PersistenceEffect::DuplicateNoEffect
        );
        let position = fixture
            .repository
            .managed_position(&instrument(), asset("BTC"))
            .await
            .expect("managed position must be readable");
        assert_eq!(position.quantity(), decimal("0.6"));
        assert_eq!(table_count(&fixture.repository, "fills").await, 2);
        assert_eq!(table_count(&fixture.repository, "audit_log").await, 2);
        fixture.close().await;
    }

    #[tokio::test]
    async fn reconciliation_snapshot_and_audit_are_atomic_and_idempotent() {
        let fixture = Fixture::new().await;
        let owner = runtime_id(1);
        fixture
            .repository
            .acquire_instance_lease(owner, timestamp(2_700), std::time::Duration::from_secs(60))
            .await
            .expect("lease should be acquired");
        let snapshot = PortfolioReconciler::reconcile(
            vec![
                ExchangeAssetBalance::new(asset("BTC"), decimal("1.1"), decimal("0"))
                    .expect("exchange balance must be valid"),
            ],
            vec![
                LocalAssetBalance::new(asset("BTC"), decimal("1"), decimal("0.8"))
                    .expect("local balance must be valid"),
            ],
            2_701,
        )
        .expect("snapshot must be valid");
        assert!(!snapshot.allows_new_entries());
        let run_id = reconciliation_id(1);
        let audit = AuditEntry::new(
            audit_id(21),
            timestamp(2_701),
            "PORTFOLIO_RECONCILED",
            Some(run_id.to_string()),
            json!({"snapshot_hash": snapshot.snapshot_hash().to_string()}),
        )
        .expect("audit must be valid");

        assert_eq!(
            fixture
                .repository
                .persist_portfolio_reconciliation(
                    owner,
                    run_id,
                    timestamp(2_700),
                    timestamp(2_701),
                    &snapshot,
                    &audit,
                )
                .await
                .expect("first reconciliation must apply"),
            PersistenceEffect::Applied
        );
        assert_eq!(
            fixture
                .repository
                .persist_portfolio_reconciliation(
                    owner,
                    run_id,
                    timestamp(2_700),
                    timestamp(2_701),
                    &snapshot,
                    &audit,
                )
                .await
                .expect("duplicate reconciliation must be idempotent"),
            PersistenceEffect::DuplicateNoEffect
        );
        let payload: String = sqlx::query_scalar("SELECT payload_json FROM reconciliation_runs")
            .fetch_one(fixture.repository.pool())
            .await
            .expect("reconciliation payload must be readable");
        let payload: serde_json::Value =
            serde_json::from_str(&payload).expect("payload must be valid JSON");
        assert_eq!(payload["allows_new_entries"], false);
        assert_eq!(payload["assets"][0]["unknown_quantity"], "0.1");
        assert_eq!(
            table_count(&fixture.repository, "reconciliation_runs").await,
            1
        );
        assert_eq!(table_count(&fixture.repository, "audit_log").await, 1);
        fixture.close().await;
    }

    #[tokio::test]
    async fn second_instance_cannot_acquire_lease_or_write_tradable_state() {
        let fixture = Fixture::new().await;
        let first = runtime_id(1);
        let second = runtime_id(2);
        fixture
            .repository
            .acquire_instance_lease(first, timestamp(3_000), std::time::Duration::from_secs(60))
            .await
            .expect("first instance should acquire lease");

        let error = fixture
            .repository
            .acquire_instance_lease(second, timestamp(3_001), std::time::Duration::from_secs(60))
            .await
            .expect_err("second instance must be rejected");
        assert!(matches!(error, LeaseAcquireError::Held { .. }));

        let write_error = fixture
            .repository
            .persist_system_state_change(
                second,
                &state_change(
                    None,
                    SystemState::Starting,
                    3_002,
                    audit_id(1),
                    outbox_id(1),
                ),
            )
            .await
            .expect_err("non-owner must not write runtime state");
        assert!(matches!(write_error, StorageError::InstanceLeaseNotHeld));
        assert_eq!(
            fixture
                .repository
                .system_state()
                .await
                .expect("state should be readable"),
            None
        );
        fixture.close().await;
    }

    #[tokio::test]
    async fn expired_lease_can_be_taken_over_but_old_owner_stays_fenced() {
        let fixture = Fixture::new().await;
        let first = runtime_id(1);
        let second = runtime_id(2);
        fixture
            .repository
            .acquire_instance_lease(
                first,
                timestamp(4_000),
                std::time::Duration::from_millis(10),
            )
            .await
            .expect("first instance should acquire lease");
        fixture
            .repository
            .acquire_instance_lease(second, timestamp(4_010), std::time::Duration::from_secs(60))
            .await
            .expect("expired lease should allow takeover");

        let old_owner_error = fixture
            .repository
            .persist_system_state_change(
                first,
                &state_change(
                    None,
                    SystemState::Starting,
                    4_011,
                    audit_id(1),
                    outbox_id(1),
                ),
            )
            .await
            .expect_err("old owner must remain fenced");
        assert!(matches!(
            old_owner_error,
            StorageError::InstanceLeaseNotHeld
        ));
        fixture.close().await;
    }

    #[tokio::test]
    async fn backup_is_integrity_checked_and_recoverable() {
        let fixture = Fixture::new().await;
        let owner = runtime_id(1);
        fixture
            .repository
            .acquire_instance_lease(owner, timestamp(5_000), std::time::Duration::from_secs(60))
            .await
            .expect("lease should be acquired");
        fixture
            .repository
            .persist_system_state_change(
                owner,
                &state_change(
                    None,
                    SystemState::Starting,
                    5_001,
                    audit_id(1),
                    outbox_id(1),
                ),
            )
            .await
            .expect("initial write should commit");

        let backup_path = fixture.temp_dir.path().join("backup.sqlite3");
        fixture
            .repository
            .backup_to(&backup_path)
            .await
            .expect("backup should succeed and pass integrity checks");

        let recovered = SqliteRepository::connect(&backup_path, 1)
            .await
            .expect("backup should open as a recovery source");
        assert_eq!(
            recovered
                .system_state()
                .await
                .expect("recovered state should be readable")
                .expect("recovered state should exist")
                .state(),
            SystemState::Starting
        );
        assert_eq!(
            recovered
                .audit_entries()
                .await
                .expect("recovered audit should be readable")
                .len(),
            1
        );
        assert_eq!(
            recovered
                .pending_outbox(10)
                .await
                .expect("recovered outbox should be readable")
                .len(),
            1
        );
        recovered.close().await;
        fixture.close().await;
    }

    struct Fixture {
        repository: SqliteRepository,
        temp_dir: TestTempDir,
    }

    impl Fixture {
        async fn new() -> Self {
            let temp_dir = TestTempDir::new();
            let repository =
                SqliteRepository::connect(temp_dir.path().join("ironpilot.sqlite3"), 4)
                    .await
                    .expect("repository should initialize");
            Self {
                temp_dir,
                repository,
            }
        }

        async fn close(self) {
            self.repository.close().await;
            std::fs::remove_dir_all(self.temp_dir.path)
                .expect("temporary directory should be removable after closing SQLite");
        }
    }

    struct TestTempDir {
        path: PathBuf,
    }

    impl TestTempDir {
        fn new() -> Self {
            let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir()
                .join(format!("ironpilot-p1-04-{}-{sequence}", std::process::id()));
            std::fs::create_dir(&path).expect("temporary directory should be created");
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    fn state_change(
        expected: Option<SystemState>,
        next: SystemState,
        at: i64,
        audit_id: AuditEntryId,
        outbox_id: OutboxMessageId,
    ) -> SystemStateChange {
        let at = timestamp(at);
        let audit = AuditEntry::new(
            audit_id,
            at,
            "SYSTEM_STATE_CHANGED",
            Some("system"),
            json!({"next": format!("{next:?}")}),
        )
        .expect("audit should be valid");
        let outbox = OutboxMessage::new(
            outbox_id,
            at,
            "system.state.changed",
            json!({"next": format!("{next:?}")}),
        )
        .expect("outbox should be valid");
        SystemStateChange::new(expected, next, at, audit, Some(outbox))
            .expect("state change should be valid")
    }

    fn timestamp(value: i64) -> UnixMillis {
        UnixMillis::new(value).expect("timestamp should be valid")
    }

    fn runtime_id(value: u128) -> RuntimeInstanceId {
        RuntimeInstanceId::from_str(&uuid_text(value)).expect("runtime ID should be valid")
    }

    fn audit_id(value: u128) -> AuditEntryId {
        AuditEntryId::from_str(&uuid_text(value)).expect("audit ID should be valid")
    }

    fn outbox_id(value: u128) -> OutboxMessageId {
        OutboxMessageId::from_str(&uuid_text(value + 100)).expect("outbox ID should be valid")
    }

    fn fill_id(value: u128) -> FillId {
        FillId::from_str(&uuid_text(value + 200)).expect("fill ID should be valid")
    }

    fn order_id(value: u128) -> OrderId {
        OrderId::from_str(&uuid_text(value + 300)).expect("order ID should be valid")
    }

    fn order_intent_id(value: u128) -> OrderIntentId {
        OrderIntentId::from_str(&uuid_text(value + 350)).expect("order intent ID should be valid")
    }

    fn snapshot_id(value: u128) -> SnapshotId {
        SnapshotId::from_str(&uuid_text(value + 375)).expect("snapshot ID should be valid")
    }

    fn trade_plan_id(value: u128) -> TradePlanId {
        TradePlanId::from_str(&uuid_text(value + 400)).expect("trade plan ID should be valid")
    }

    fn managed_lot_id(value: u128) -> ManagedLotId {
        ManagedLotId::from_str(&uuid_text(value + 500)).expect("managed lot ID should be valid")
    }

    fn reconciliation_id(value: u128) -> ReconciliationRunId {
        ReconciliationRunId::from_str(&uuid_text(value + 600))
            .expect("reconciliation ID should be valid")
    }

    fn decimal(value: &str) -> DomainDecimal {
        DomainDecimal::from_str(value).expect("decimal should be valid")
    }

    fn asset(value: &str) -> AssetCode {
        AssetCode::new(value).expect("asset should be valid")
    }

    fn instrument() -> InstrumentId {
        InstrumentId::from_str("bybit:spot:BTCUSDT").expect("instrument should be valid")
    }

    fn rules() -> SpotInstrumentRules {
        validated_spot_instrument_rules(
            instrument(),
            asset("BTC"),
            asset("USDT"),
            InstrumentTradingStatus::Trading,
            decimal("0.01"),
            decimal("0.00000001"),
            decimal("0.00000001"),
            decimal("5"),
            decimal("10"),
            decimal("10"),
            decimal("10"),
            decimal("0.01"),
            decimal("0.01"),
        )
        .expect("instrument rules should be valid")
    }

    fn portfolio_fill(
        fill: u128,
        order: u128,
        trade_plan: u128,
        managed_lot: Option<u128>,
        side: PortfolioFillSide,
        base_quantity: &str,
        occurred_at: u64,
    ) -> PortfolioFill {
        PortfolioFill::new(
            fill_id(fill),
            order_id(order),
            trade_plan_id(trade_plan),
            managed_lot.map(managed_lot_id),
            &rules(),
            side,
            decimal(base_quantity),
            decimal("100"),
            occurred_at,
        )
        .expect("portfolio fill should be valid")
    }

    fn portfolio_audit(fill: &PortfolioFill, sequence: u128) -> AuditEntry {
        AuditEntry::new(
            audit_id(sequence + 100),
            timestamp(
                i64::try_from(fill.occurred_at_unix_millis())
                    .expect("test timestamp should fit i64"),
            ),
            "PORTFOLIO_FILL_APPLIED",
            Some(fill.fill_id().to_string()),
            json!({"fill_id": fill.fill_id().to_string()}),
        )
        .expect("portfolio audit should be valid")
    }

    fn ai_ledger_entry(
        sequence: u128,
        action: &str,
        trade_plan_id: TradePlanId,
    ) -> AiTradePlanLedgerEntry {
        let end_at = u64::try_from(AI_END_AT).expect("test timestamp fits u64");
        let as_of = end_at + 1_000;
        let primary = ai_candles(MarketTimeframe::FifteenMinutes, end_at);
        let confirmation = ai_candles(MarketTimeframe::OneHour, end_at);
        let book = TopOfBook::new(
            instrument(),
            end_at,
            end_at + 500,
            decimal("218.9"),
            decimal("10"),
            decimal("219.1"),
            decimal("12"),
        )
        .expect("AI context book is valid");
        let features = MarketFeatureEngine::compute(
            &primary,
            &confirmation,
            &book,
            as_of,
            MarketDataSource::WebSocketLive,
        )
        .expect("AI context features are valid");
        let rules_snapshot = InstrumentRulesSnapshot::new(
            vec![rules()],
            ExchangeServerTime::new(end_at / 1_000, end_at * 1_000_000, end_at)
                .expect("exchange server time is valid"),
            end_at,
            end_at + 60_000,
            RulesHash::from_sha256([9; 32]),
        )
        .expect("AI context rules snapshot is valid");
        let is_open = action == "OPEN_LONG";
        let managed_btc = if is_open { "0" } else { "0.4" };
        let portfolio = PortfolioReconciler::reconcile(
            vec![
                ExchangeAssetBalance::new(asset("BTC"), decimal("0.5"), decimal("0"))
                    .expect("BTC balance is valid"),
                ExchangeAssetBalance::new(asset("USDT"), decimal("1000"), decimal("0"))
                    .expect("USDT balance is valid"),
            ],
            vec![
                LocalAssetBalance::new(asset("BTC"), decimal("0.5"), decimal(managed_btc))
                    .expect("BTC local balance is valid"),
                LocalAssetBalance::new(asset("USDT"), decimal("1000"), decimal("0"))
                    .expect("USDT local balance is valid"),
            ],
            end_at,
        )
        .expect("AI context portfolio is valid");
        let managed_positions = if is_open {
            Vec::new()
        } else {
            vec![
                ironpilot_domain::ManagedPosition::new(instrument(), asset("BTC"), decimal("0.4"))
                    .expect("managed position is valid"),
            ]
        };
        let open_orders = if is_open {
            Vec::new()
        } else {
            vec![
                AccountOrderFact::new(
                    format!("exchange-{sequence}"),
                    Some(format!("ironpilot-{sequence}")),
                    instrument(),
                    AccountOrderSide::Buy,
                    AiOrderType::Limit,
                    Some(decimal("210")),
                    decimal("0.10"),
                    decimal("0"),
                    AccountOrderStatus::New,
                    end_at,
                )
                .expect("account order is valid"),
            ]
        };
        let context_id = AiDecisionContextId::from_str(&uuid_text(sequence + 10_000))
            .expect("context ID is valid");
        let context = AiDecisionContext::new(
            context_id,
            as_of,
            primary,
            confirmation,
            book,
            features,
            &rules_snapshot,
            &portfolio,
            managed_positions,
            open_orders,
            decimal("25.00"),
        )
        .expect("AI Decision Context is valid");
        let ai_plan_id =
            AiTradingPlanId::from_str(&uuid_text(sequence + 11_000)).expect("AI plan ID is valid");
        let plan_value = match action {
            "OPEN_LONG" => json!({
                "schema_version": "3.0",
                "plan_id": ai_plan_id.to_string(),
                "context_id": context_id.to_string(),
                "instrument_id": instrument().to_string(),
                "action": "OPEN_LONG",
                "valid_until": end_at + 20_000,
                "order": {
                    "type": "LIMIT",
                    "quantity": "0.10",
                    "limit_price": "210.00",
                    "time_in_force": "GTC",
                    "expires_at": end_at + 20_000,
                    "max_slippage_quote": "1.00"
                },
                "protective_stop": {
                    "trigger_price": "200.00",
                    "order_type": "MARKET"
                },
                "take_profits": [{"price": "230.00", "quantity": "0.10"}],
                "declared_max_loss_quote": "2.00",
                "review": {
                    "next_review_at": end_at + 10_000,
                    "max_holding_until": end_at + 100_000
                },
                "confidence": "0.70",
                "thesis": "Complete facts support this AI-selected entry.",
                "invalidation": "Exit if subsequent facts invalidate the thesis.",
                "risks": ["The market can reverse."]
            }),
            "NO_TRADE" => json!({
                "schema_version": "3.0",
                "plan_id": ai_plan_id.to_string(),
                "context_id": context_id.to_string(),
                "instrument_id": instrument().to_string(),
                "action": "NO_TRADE",
                "valid_until": end_at + 20_000,
                "confidence": "0.60",
                "thesis": "The AI elects not to trade these complete facts.",
                "invalidation": "Re-evaluate when new market or account facts arrive.",
                "risks": []
            }),
            "HOLD" => json!({
                "schema_version": "3.0",
                "plan_id": ai_plan_id.to_string(),
                "context_id": context_id.to_string(),
                "instrument_id": instrument().to_string(),
                "action": "HOLD",
                "target_trade_plan_id": trade_plan_id.to_string(),
                "valid_until": end_at + 20_000,
                "review": {
                    "next_review_at": end_at + 10_000,
                    "max_holding_until": end_at + 100_000
                },
                "confidence": "0.65",
                "thesis": "The AI elects to hold after reviewing the complete facts.",
                "invalidation": "Exit if subsequent facts invalidate the thesis.",
                "risks": ["The existing position can lose value."]
            }),
            _ => panic!("unsupported AI ledger test action"),
        };
        let plan =
            AiTradingPlan::from_json(&plan_value.to_string()).expect("AI plan fixture is valid");
        let response = AiRawResponse::new(
            AiProviderResponseId::from_str(&uuid_text(sequence + 12_000))
                .expect("response ID is valid"),
            context_id,
            "deepseek",
            "deepseek-chat",
            end_at + 2_000,
            plan.to_json(),
        )
        .expect("raw response fixture is valid");
        AiTradePlanLedgerEntry::new(
            context,
            response,
            plan,
            trade_plan_id,
            TradePlanActionId::from_str(&uuid_text(sequence + 13_000)).expect("action ID is valid"),
            end_at + 3_000,
        )
        .expect("AI ledger fixture is valid")
    }

    fn ai_candles(timeframe: MarketTimeframe, end_at: u64) -> Vec<ClosedCandle> {
        let duration = timeframe.duration_millis();
        let first_open =
            end_at - duration * u64::try_from(FEATURE_CANDLE_WINDOW).expect("window fits u64");
        (0..FEATURE_CANDLE_WINDOW)
            .map(|index| {
                let price = 100 + i64::try_from(index).expect("index fits i64");
                ClosedCandle::new(
                    instrument(),
                    timeframe,
                    first_open + duration * u64::try_from(index).expect("index fits u64"),
                    decimal(&price.to_string()),
                    decimal(&(price + 1).to_string()),
                    decimal(&(price - 1).to_string()),
                    decimal(&price.to_string()),
                    decimal("10"),
                    decimal(&(price * 10).to_string()),
                    true,
                )
                .expect("AI context candle is valid")
            })
            .collect()
    }

    fn ai_ledger_audit(entry: &AiTradePlanLedgerEntry, sequence: u128) -> AuditEntry {
        AuditEntry::new(
            audit_id(sequence + 20_000),
            timestamp(
                i64::try_from(entry.recorded_at_unix_millis()).expect("test timestamp fits i64"),
            ),
            "AI_TRADE_PLAN_RECORDED",
            Some(entry.plan().plan_id().to_string()),
            entry.trace_json(),
        )
        .expect("AI ledger audit is valid")
    }

    fn accepted_execution_validation(
        entry: &AiTradePlanLedgerEntry,
    ) -> ExecutionValidationDecision {
        execution_validation(entry, ExecutionMode::Paper)
    }

    fn execution_validation(
        entry: &AiTradePlanLedgerEntry,
        execution_mode: ExecutionMode,
    ) -> ExecutionValidationDecision {
        historical_validation_facts(execution_mode).validate(entry)
    }

    fn historical_validation_facts(execution_mode: ExecutionMode) -> HistoricalValidationFacts {
        let end_at = u64::try_from(AI_END_AT).expect("test timestamp fits u64");
        let rules_snapshot = InstrumentRulesSnapshot::new(
            vec![rules()],
            ExchangeServerTime::new(end_at / 1_000, end_at * 1_000_000, end_at)
                .expect("server time is valid"),
            end_at,
            end_at + 60_000,
            RulesHash::from_sha256([9; 32]),
        )
        .expect("rules snapshot is valid");
        let portfolio = PortfolioReconciler::reconcile(
            vec![
                ExchangeAssetBalance::new(asset("BTC"), decimal("0.5"), decimal("0"))
                    .expect("BTC balance is valid"),
                ExchangeAssetBalance::new(asset("USDT"), decimal("1000"), decimal("0"))
                    .expect("USDT balance is valid"),
            ],
            vec![
                LocalAssetBalance::new(asset("BTC"), decimal("0.5"), decimal("0"))
                    .expect("BTC local balance is valid"),
                LocalAssetBalance::new(asset("USDT"), decimal("1000"), decimal("0"))
                    .expect("USDT local balance is valid"),
            ],
            end_at,
        )
        .expect("portfolio is valid");
        let book = TopOfBook::new(
            instrument(),
            end_at,
            end_at + 500,
            decimal("218.9"),
            decimal("10"),
            decimal("219.1"),
            decimal("12"),
        )
        .expect("book is valid");
        let price_limits =
            SpotOrderPriceLimits::new(instrument(), decimal("220"), decimal("190"), end_at + 2_500)
                .expect("price limits are valid");
        let authorization = ExecutionAuthorization::new(execution_mode, true, vec![instrument()])
            .expect("authorization is valid");
        let policy =
            ExecutionValidationPolicy::new(decimal("0"), 5_000, 5_000).expect("policy is valid");
        HistoricalValidationFacts::new(
            rules_snapshot,
            portfolio,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            book,
            price_limits,
            decimal("25"),
            authorization,
            policy,
            end_at + 4_000,
        )
    }

    fn historical_input(
        sequence: u128,
        include_stop: bool,
        reuse_decision_fact: bool,
    ) -> MinimalHistoricalReplayInput {
        let entry = ai_ledger_entry(sequence, "OPEN_LONG", trade_plan_id(sequence));
        let ids = ExecutionOrderIdSet::new(
            Some(ExecutionOrderIds::new(
                order_intent_id(sequence),
                order_id(sequence),
            )),
            Some(ExecutionOrderIds::new(
                order_intent_id(sequence + 1),
                order_id(sequence + 1),
            )),
            vec![ExecutionOrderIds::new(
                order_intent_id(sequence + 2),
                order_id(sequence + 2),
            )],
        )
        .expect("historical execution IDs should be valid");
        let end_at = u64::try_from(AI_END_AT).expect("test timestamp fits u64");
        let first_source = if reuse_decision_fact {
            entry.context().as_of_unix_millis()
        } else {
            end_at + 6_000
        };
        let mut observations = vec![
            PaperMarketObservation::new(
                snapshot_id(sequence + 1),
                instrument(),
                first_source,
                end_at + 6_001,
                decimal("209.9"),
                decimal("210.1"),
                decimal("209"),
                decimal("211"),
                decimal("0.04"),
            )
            .expect("first historical observation should be valid"),
            PaperMarketObservation::new(
                snapshot_id(sequence + 2),
                instrument(),
                end_at + 7_000,
                end_at + 7_001,
                decimal("209.9"),
                decimal("210.1"),
                decimal("209"),
                decimal("211"),
                decimal("0.06"),
            )
            .expect("second historical observation should be valid"),
        ];
        if include_stop {
            observations.push(
                PaperMarketObservation::new(
                    snapshot_id(sequence + 3),
                    instrument(),
                    end_at + 8_000,
                    end_at + 8_001,
                    decimal("199"),
                    decimal("199.2"),
                    decimal("198"),
                    decimal("201"),
                    decimal("0.10"),
                )
                .expect("historical stop observation should be valid"),
            );
        }
        MinimalHistoricalReplayInput::new(
            entry,
            historical_validation_facts(ExecutionMode::Paper),
            ids,
            end_at + 5_000,
            rules(),
            observations,
        )
    }

    async fn historical_ledger_rows(repository: &SqliteRepository) -> Vec<String> {
        let queries = [
            "SELECT 'context|' || context_hash || '|' || payload_json FROM ai_decision_contexts ORDER BY context_id",
            "SELECT 'response|' || response_hash || '|' || raw_response FROM ai_provider_responses ORDER BY response_id",
            "SELECT 'plan|' || plan_hash || '|' || payload_json FROM ai_trading_plans ORDER BY ai_plan_id",
            "SELECT 'ledger|' || action_id || '|' || trade_plan_id || '|' || context_hash || '|' || response_hash || '|' || plan_hash FROM ai_trade_plan_ledger ORDER BY action_id",
            "SELECT 'validation|' || validation_hash || '|' || evidence_json FROM execution_validations ORDER BY action_id",
            "SELECT 'trade_plan|' || trade_plan_id || '|' || state || '|' || payload_json FROM trade_plans ORDER BY trade_plan_id",
            "SELECT 'action|' || action_id || '|' || state || '|' || payload_json FROM trade_plan_actions ORDER BY action_id",
            "SELECT 'submission|' || request_hash || '|' || payload_json FROM paper_execution_submissions ORDER BY action_id",
            "SELECT 'intent|' || order_intent_id || '|' || state || '|' || payload_json FROM order_intents ORDER BY order_intent_id",
            "SELECT 'order|' || order_id || '|' || state || '|' || payload_json FROM paper_orders ORDER BY order_id",
            "SELECT 'observation|' || observation_hash || '|' || payload_json || '|' || effect_json FROM paper_market_observations ORDER BY observed_at, observation_id",
            "SELECT 'fill|' || fill_id || '|' || payload_json FROM fills ORDER BY occurred_at, fill_id",
            "SELECT 'lot|' || managed_lot_id || '|' || COALESCE(CAST(closed_at AS TEXT), 'OPEN') || '|' || payload_json FROM managed_lots ORDER BY managed_lot_id",
            "SELECT 'audit|' || audit_entry_id || '|' || category || '|' || payload_json FROM audit_log ORDER BY audit_entry_id",
        ];
        let mut rows = Vec::new();
        for query in queries {
            rows.extend(
                sqlx::query_scalar::<_, String>(query)
                    .fetch_all(repository.pool())
                    .await
                    .expect("canonical historical ledger rows should be readable"),
            );
        }
        rows
    }

    #[derive(Clone, Debug)]
    enum ScriptedRuntimeStep {
        Plan {
            sequence: u128,
            action: &'static str,
            target_trade_plan_id: Option<TradePlanId>,
            declared_max_loss_quote: &'static str,
        },
        Failure(&'static str),
    }

    impl ScriptedRuntimeStep {
        const fn plan(
            sequence: u128,
            action: &'static str,
            target_trade_plan_id: Option<TradePlanId>,
            declared_max_loss_quote: &'static str,
        ) -> Self {
            Self::Plan {
                sequence,
                action,
                target_trade_plan_id,
                declared_max_loss_quote,
            }
        }

        const fn failure(code: &'static str) -> Self {
            Self::Failure(code)
        }
    }

    #[derive(Debug)]
    struct ScriptedRuntimeProvider {
        steps: StdMutex<VecDeque<ScriptedRuntimeStep>>,
        calls: AtomicUsize,
    }

    impl ScriptedRuntimeProvider {
        fn new(steps: Vec<ScriptedRuntimeStep>) -> Self {
            Self {
                steps: StdMutex::new(steps.into()),
                calls: AtomicUsize::new(0),
            }
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::Acquire)
        }

        fn next(
            &self,
            context: &AiDecisionContext,
            runtime_state: &AiTradingRuntimeState,
        ) -> Result<RuntimeAiGeneration, ScriptedRuntimeError> {
            self.calls.fetch_add(1, Ordering::AcqRel);
            let step = self
                .steps
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .pop_front()
                .expect("scripted runtime provider must have a response");
            match step {
                ScriptedRuntimeStep::Plan {
                    sequence,
                    action,
                    target_trade_plan_id,
                    declared_max_loss_quote,
                } => {
                    if let Some(target) = target_trade_plan_id {
                        assert!(
                            runtime_state
                                .active_trade_plans()
                                .iter()
                                .any(|fact| fact.trade_plan_id() == target),
                            "managed action target must be present in runtime AI state"
                        );
                    }
                    Ok(scripted_runtime_generation(
                        context,
                        sequence,
                        action,
                        target_trade_plan_id,
                        declared_max_loss_quote,
                    ))
                }
                ScriptedRuntimeStep::Failure(code) => Err(ScriptedRuntimeError(code)),
            }
        }
    }

    impl PaperRuntimeAiProvider for ScriptedRuntimeProvider {
        type Error = ScriptedRuntimeError;

        fn generate<'a>(
            &'a self,
            context: &'a AiDecisionContext,
            runtime_state: &'a AiTradingRuntimeState,
        ) -> PaperRuntimeProviderFuture<'a, RuntimeAiGeneration, Self::Error> {
            Box::pin(async move { self.next(context, runtime_state) })
        }

        fn replan<'a>(
            &'a self,
            context: &'a AiDecisionContext,
            runtime_state: &'a AiTradingRuntimeState,
            _rejected_plan: &'a AiTradingPlan,
            _reasons: Vec<Box<str>>,
        ) -> PaperRuntimeProviderFuture<'a, RuntimeAiGeneration, Self::Error> {
            Box::pin(async move { self.next(context, runtime_state) })
        }
    }

    #[derive(Clone, Copy, Debug)]
    struct ScriptedRuntimeError(&'static str);

    impl std::fmt::Display for ScriptedRuntimeError {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str(self.0)
        }
    }

    impl std::error::Error for ScriptedRuntimeError {}

    impl PaperRuntimeProviderError for ScriptedRuntimeError {
        fn code(&self) -> &'static str {
            self.0
        }

        fn evidence(&self) -> Option<&crate::DeepSeekAttemptEvidence> {
            None
        }
    }

    fn scripted_runtime_generation(
        context: &AiDecisionContext,
        sequence: u128,
        action: &str,
        target_trade_plan_id: Option<TradePlanId>,
        declared_max_loss_quote: &str,
    ) -> RuntimeAiGeneration {
        let plan_id =
            AiTradingPlanId::from_str(&uuid_text(sequence + 40_000)).expect("plan ID is valid");
        let now = context.as_of_unix_millis();
        let valid_until = now + 20_000;
        let mut value = json!({
            "schema_version": "3.0",
            "plan_id": plan_id.to_string(),
            "context_id": context.context_id().to_string(),
            "instrument_id": context.instrument_id().to_string(),
            "action": action,
            "valid_until": valid_until,
            "confidence": "0.70",
            "thesis": "The recorded provider selects this exact action.",
            "invalidation": "Re-evaluate when market or account facts change.",
            "risks": ["The market can move against the position."]
        });
        match action {
            "OPEN_LONG" => {
                value["order"] = json!({
                    "type": "LIMIT",
                    "quantity": "0.10",
                    "limit_price": "210.00",
                    "time_in_force": "GTC",
                    "expires_at": valid_until,
                    "max_slippage_quote": "1.00"
                });
                value["protective_stop"] = json!({
                    "trigger_price": "200.00",
                    "order_type": "MARKET"
                });
                value["take_profits"] = json!([{"price": "230.00", "quantity": "0.10"}]);
                value["declared_max_loss_quote"] = json!(declared_max_loss_quote);
                value["review"] = json!({
                    "next_review_at": now + 8_000,
                    "max_holding_until": now + 18_000
                });
            }
            "EXIT" => {
                value["target_trade_plan_id"] = json!(
                    target_trade_plan_id
                        .expect("EXIT requires a target")
                        .to_string()
                );
                value["order"] = json!({
                    "type": "MARKET",
                    "quantity": "0.10",
                    "time_in_force": "IOC",
                    "expires_at": valid_until,
                    "max_slippage_quote": "1.00"
                });
                value["review"] = json!({
                    "next_review_at": now + 8_000,
                    "max_holding_until": now + 18_000
                });
            }
            "NO_TRADE" => {}
            _ => panic!("unsupported scripted runtime action"),
        }
        let plan =
            AiTradingPlan::from_json(&value.to_string()).expect("scripted plan should be valid");
        let response = AiRawResponse::new(
            AiProviderResponseId::from_str(&uuid_text(sequence + 50_000))
                .expect("response ID is valid"),
            context.context_id(),
            "recorded-stub",
            "deterministic",
            now + 1_000,
            plan.to_json(),
        )
        .expect("scripted response should be valid");
        RuntimeAiGeneration::recorded(response, plan)
    }

    fn paper_runtime_fact_bundle(
        sequence: u128,
        symbol: &str,
        base_asset: &str,
        managed: bool,
        stale: bool,
        account_changed_after_context: bool,
    ) -> (PaperRuntimeFacts, HistoricalValidationFacts) {
        let end_at = u64::try_from(AI_END_AT).expect("timestamp fits u64");
        let offset = if stale {
            69_000
        } else if managed {
            9_000
        } else {
            0
        };
        let as_of = end_at + 1_000 + offset;
        let instrument = runtime_instrument(symbol);
        let rules = runtime_rules(symbol, base_asset);
        let primary = runtime_candles(&instrument, MarketTimeframe::FifteenMinutes, end_at);
        let confirmation = runtime_candles(&instrument, MarketTimeframe::OneHour, end_at);
        let book = TopOfBook::new(
            instrument.clone(),
            as_of - 500,
            as_of - 100,
            decimal("218.9"),
            decimal("10"),
            decimal("219.1"),
            decimal("12"),
        )
        .expect("runtime book should be valid");
        let features = MarketFeatureEngine::compute(
            &primary,
            &confirmation,
            &book,
            as_of,
            MarketDataSource::WebSocketLive,
        )
        .expect("runtime market features should be valid");
        let rules_snapshot = InstrumentRulesSnapshot::new(
            vec![rules.clone()],
            ExchangeServerTime::new(end_at / 1_000, end_at * 1_000_000, end_at)
                .expect("runtime server time should be valid"),
            end_at,
            end_at + 60_000,
            RulesHash::from_sha256([9; 32]),
        )
        .expect("runtime rules snapshot should be valid");
        let base_quantity = if managed { "0.10" } else { "0.50" };
        let managed_quantity = if managed { "0.10" } else { "0" };
        let portfolio = PortfolioReconciler::reconcile(
            vec![
                ExchangeAssetBalance::new(asset(base_asset), decimal(base_quantity), decimal("0"))
                    .expect("runtime base exchange balance should be valid"),
                ExchangeAssetBalance::new(asset("USDT"), decimal("1000"), decimal("0"))
                    .expect("runtime quote exchange balance should be valid"),
            ],
            vec![
                LocalAssetBalance::new(
                    asset(base_asset),
                    decimal(base_quantity),
                    decimal(managed_quantity),
                )
                .expect("runtime base local balance should be valid"),
                LocalAssetBalance::new(asset("USDT"), decimal("1000"), decimal("0"))
                    .expect("runtime quote local balance should be valid"),
            ],
            as_of - 100,
        )
        .expect("runtime portfolio should be valid");
        let managed_positions = if managed {
            vec![
                ironpilot_domain::ManagedPosition::new(
                    instrument.clone(),
                    asset(base_asset),
                    decimal("0.10"),
                )
                .expect("runtime managed position should be valid"),
            ]
        } else {
            Vec::new()
        };
        let context_orders = if account_changed_after_context {
            vec![
                AccountOrderFact::new(
                    format!("runtime-exchange-{sequence}"),
                    Some(format!("runtime-link-{sequence}")),
                    instrument.clone(),
                    AccountOrderSide::Buy,
                    AiOrderType::Limit,
                    Some(decimal("210")),
                    decimal("0.10"),
                    decimal("0"),
                    AccountOrderStatus::New,
                    as_of - 100,
                )
                .expect("runtime account order should be valid"),
            ]
        } else {
            Vec::new()
        };
        let validation_managed = if managed {
            vec![
                ironpilot_application::ManagedPositionExecutionFact::new(
                    trade_plan_id(800),
                    managed_positions[0].clone(),
                    decimal("210"),
                    decimal("200"),
                )
                .expect("runtime validation managed position should be valid"),
            ]
        } else {
            Vec::new()
        };
        let active_plans = if managed {
            vec![ironpilot_application::ActiveTradePlanFact::new(
                trade_plan_id(800),
                instrument.clone(),
                TradePlanState::Active,
            )]
        } else {
            Vec::new()
        };
        let price_limits = SpotOrderPriceLimits::new(
            instrument.clone(),
            decimal("220"),
            decimal("190"),
            as_of + 2_000,
        )
        .expect("runtime price limits should be valid");
        let authorization =
            ExecutionAuthorization::new(ExecutionMode::Paper, true, vec![instrument.clone()])
                .expect("runtime authorization should be valid");
        let policy =
            ExecutionValidationPolicy::new(decimal("0"), 5_000, 5_000).expect("policy is valid");
        let validation = HistoricalValidationFacts::new(
            rules_snapshot.clone(),
            portfolio.clone(),
            validation_managed,
            if account_changed_after_context {
                Vec::new()
            } else {
                context_orders.clone()
            },
            active_plans,
            book.clone(),
            price_limits,
            decimal("25"),
            authorization,
            policy,
            as_of + 3_000,
        );
        let provider_state = if managed {
            let original_plan = runtime_original_open_plan(sequence, &instrument, as_of);
            AiTradingRuntimeState::new(vec![
                AiRuntimeTradePlanFact::new(
                    trade_plan_id(800),
                    original_plan,
                    json!({
                        "status": "FILLED",
                        "filled_quantity": "0.10",
                        "average_price": "210.00",
                        "last_fill_at": as_of - 1_000
                    }),
                )
                .expect("runtime active plan fact should be valid"),
            ])
            .expect("runtime provider state should be valid")
        } else {
            AiTradingRuntimeState::empty()
        };
        (
            PaperRuntimeFacts::new(
                AiDecisionContextId::from_str(&uuid_text(sequence + 60_000))
                    .expect("runtime context ID should be valid"),
                as_of,
                primary,
                confirmation,
                book,
                features,
                rules_snapshot,
                portfolio,
                managed_positions,
                context_orders,
                decimal("25"),
                rules,
                provider_state,
            ),
            validation,
        )
    }

    fn runtime_original_open_plan(
        sequence: u128,
        instrument: &InstrumentId,
        as_of: u64,
    ) -> AiTradingPlan {
        let value = json!({
            "schema_version": "3.0",
            "plan_id": uuid_text(sequence + 70_000),
            "context_id": uuid_text(sequence + 71_000),
            "instrument_id": instrument.to_string(),
            "action": "OPEN_LONG",
            "valid_until": as_of + 20_000,
            "confidence": "0.70",
            "thesis": "The original AI plan opened this managed position.",
            "invalidation": "Exit when the original thesis no longer holds.",
            "risks": ["The market can move against the position."],
            "order": {
                "type": "LIMIT",
                "quantity": "0.10",
                "limit_price": "210.00",
                "time_in_force": "GTC",
                "expires_at": as_of + 20_000,
                "max_slippage_quote": "1.00"
            },
            "protective_stop": {
                "trigger_price": "200.00",
                "order_type": "MARKET"
            },
            "take_profits": [{"price": "230.00", "quantity": "0.10"}],
            "declared_max_loss_quote": "2.00",
            "review": {
                "next_review_at": as_of + 8_000,
                "max_holding_until": as_of + 18_000
            }
        });
        AiTradingPlan::from_json(&value.to_string())
            .expect("runtime original OPEN_LONG plan should be valid")
    }

    fn paper_runtime_attempt(
        sequence: u128,
        trade_plan_id: TradePlanId,
        validation: HistoricalValidationFacts,
        open_long_shape: bool,
        time_offset: u64,
    ) -> PaperRuntimeActionAttempt {
        let id_base = sequence
            .checked_mul(10)
            .expect("runtime test ID base should not overflow");
        let runtime_order_ids = |role: u128| {
            ExecutionOrderIds::new(
                OrderIntentId::from_str(&uuid_text(id_base + 90_000 + role))
                    .expect("runtime order-intent ID should be valid"),
                OrderId::from_str(&uuid_text(id_base + 100_000 + role))
                    .expect("runtime order ID should be valid"),
            )
        };
        let ids = if open_long_shape {
            ExecutionOrderIdSet::new(
                Some(runtime_order_ids(0)),
                Some(runtime_order_ids(1)),
                vec![runtime_order_ids(2)],
            )
        } else {
            ExecutionOrderIdSet::new(Some(runtime_order_ids(0)), None, Vec::new())
        }
        .expect("runtime execution IDs should be valid");
        let end_at = u64::try_from(AI_END_AT).expect("timestamp fits u64");
        PaperRuntimeActionAttempt::new(
            trade_plan_id,
            TradePlanActionId::from_str(&uuid_text(sequence + 70_000))
                .expect("runtime action ID should be valid"),
            ids,
            validation,
            end_at + 3_000 + time_offset,
            end_at + 5_000 + time_offset,
        )
    }

    fn paper_runtime_observation(
        sequence: u128,
        symbol: &str,
        time_offset: u64,
        traded_low: &str,
        traded_high: &str,
        liquidity: &str,
    ) -> PaperMarketObservation {
        let end_at = u64::try_from(AI_END_AT).expect("timestamp fits u64");
        PaperMarketObservation::new(
            snapshot_id(sequence),
            runtime_instrument(symbol),
            end_at + time_offset,
            end_at + time_offset + 1,
            decimal("218"),
            decimal("220"),
            decimal(traded_low),
            decimal(traded_high),
            decimal(liquidity),
        )
        .expect("runtime observation should be valid")
    }

    fn runtime_instrument(symbol: &str) -> InstrumentId {
        InstrumentId::from_str(&format!("bybit:spot:{symbol}"))
            .expect("runtime instrument should be valid")
    }

    fn runtime_rules(symbol: &str, base_asset: &str) -> SpotInstrumentRules {
        validated_spot_instrument_rules(
            runtime_instrument(symbol),
            asset(base_asset),
            asset("USDT"),
            InstrumentTradingStatus::Trading,
            decimal("0.01"),
            decimal("0.00000001"),
            decimal("0.00000001"),
            decimal("5"),
            decimal("10"),
            decimal("10"),
            decimal("10"),
            decimal("0.01"),
            decimal("0.01"),
        )
        .expect("runtime instrument rules should be valid")
    }

    fn runtime_candles(
        instrument: &InstrumentId,
        timeframe: MarketTimeframe,
        end_at: u64,
    ) -> Vec<ClosedCandle> {
        let duration = timeframe.duration_millis();
        let first_open =
            end_at - duration * u64::try_from(FEATURE_CANDLE_WINDOW).expect("window fits u64");
        (0..FEATURE_CANDLE_WINDOW)
            .map(|index| {
                let price = 100 + i64::try_from(index).expect("index fits i64");
                ClosedCandle::new(
                    instrument.clone(),
                    timeframe,
                    first_open + duration * u64::try_from(index).expect("index fits u64"),
                    decimal(&price.to_string()),
                    decimal(&(price + 1).to_string()),
                    decimal(&(price - 1).to_string()),
                    decimal(&price.to_string()),
                    decimal("10"),
                    decimal(&(price * 10).to_string()),
                    true,
                )
                .expect("runtime candle should be valid")
            })
            .collect()
    }

    fn cycle_id(sequence: u128) -> PaperRuntimeCycleId {
        PaperRuntimeCycleId::new(
            uuid::Uuid::parse_str(&uuid_text(sequence + 80_000))
                .expect("runtime cycle UUID should be valid"),
        )
        .expect("runtime cycle ID should be valid")
    }

    async fn assert_runtime_trace_complete(
        repository: &SqliteRepository,
        cycle_id: PaperRuntimeCycleId,
        expect_execution: bool,
    ) {
        let events: Vec<(i64, String, String)> = sqlx::query_as(
            "
            SELECT sequence, event_type, payload_json
            FROM paper_runtime_events
            WHERE cycle_id = ?
            ORDER BY sequence
            ",
        )
        .bind(cycle_id.to_string())
        .fetch_all(repository.pool())
        .await
        .expect("runtime trace should be readable");
        assert!(!events.is_empty());
        assert_eq!(events[0].0, 0);
        assert_eq!(
            events.last().map(|event| event.1.as_str()),
            Some("COMPLETED")
        );
        if let Some(context_event) = events
            .iter()
            .find(|event| event.1.as_str() == "CONTEXT_BUILT")
        {
            let payload: serde_json::Value = serde_json::from_str(&context_event.2)
                .expect("Context event payload should be JSON");
            assert!(payload["context_hash"].is_string());
            assert!(payload["runtime_state_hash"].is_string());
            assert!(
                payload["runtime_state"]["active_trade_plans"].is_array(),
                "trace must retain the complete provider runtime state"
            );
        }
        if expect_execution {
            for required in [
                "CONTEXT_BUILT",
                "AI_PLAN_RECORDED",
                "ACCEPT",
                "EXECUTION_SUBMITTED",
                "COMPLETED",
            ] {
                assert!(
                    events.iter().any(|event| event.1 == required),
                    "successful runtime trace is missing {required}"
                );
            }
        }
        assert!(
            events
                .iter()
                .enumerate()
                .all(|(index, event)| event.0 == i64::try_from(index).expect("index fits i64")),
            "runtime trace sequence must be gap-free"
        );
    }

    fn execution_validation_audit(
        decision: &ExecutionValidationDecision,
        sequence: u128,
    ) -> AuditEntry {
        AuditEntry::new(
            audit_id(sequence + 30_000),
            timestamp(
                i64::try_from(decision.validated_at_unix_millis())
                    .expect("test timestamp fits i64"),
            ),
            "EXECUTION_VALIDATION_RECORDED",
            Some(decision.action_id().to_string()),
            serde_json::from_str(decision.evidence_json())
                .expect("validation evidence must be JSON"),
        )
        .expect("validation audit is valid")
    }

    async fn ai_table_count(repository: &SqliteRepository, table: &str) -> i64 {
        let query = match table {
            "ai_decision_contexts" => "SELECT COUNT(*) FROM ai_decision_contexts",
            "ai_provider_responses" => "SELECT COUNT(*) FROM ai_provider_responses",
            "ai_trading_plans" => "SELECT COUNT(*) FROM ai_trading_plans",
            "ai_trade_plan_ledger" => "SELECT COUNT(*) FROM ai_trade_plan_ledger",
            "execution_validations" => "SELECT COUNT(*) FROM execution_validations",
            "trade_plans" => "SELECT COUNT(*) FROM trade_plans",
            "trade_plan_actions" => "SELECT COUNT(*) FROM trade_plan_actions",
            "audit_log" => "SELECT COUNT(*) FROM audit_log",
            _ => panic!("AI test table must be explicitly allowed"),
        };
        sqlx::query_scalar(query)
            .fetch_one(repository.pool())
            .await
            .expect("AI table count should be readable")
    }

    async fn seed_order(repository: &SqliteRepository, fill: &PortfolioFill, sequence: u128) {
        let occurred_at =
            i64::try_from(fill.occurred_at_unix_millis()).expect("test timestamp should fit i64");
        sqlx::query(
            "
            INSERT INTO trade_plans(
                trade_plan_id, instrument_id, state, created_at, updated_at, payload_json
            )
            VALUES (?, ?, 'OPEN', ?, ?, '{}')
            ON CONFLICT(trade_plan_id) DO NOTHING
            ",
        )
        .bind(fill.trade_plan_id().to_string())
        .bind(fill.instrument_id().to_string())
        .bind(occurred_at)
        .bind(occurred_at)
        .execute(repository.pool())
        .await
        .expect("trade plan fixture should insert");
        let action_id = uuid_text(sequence + 1_000);
        sqlx::query(
            "
            INSERT INTO trade_plan_actions(
                action_id, trade_plan_id, action_type, state, created_at, expires_at, payload_json
            )
            VALUES (?, ?, 'FILL_FIXTURE', 'APPROVED', ?, ?, '{}')
            ",
        )
        .bind(&action_id)
        .bind(fill.trade_plan_id().to_string())
        .bind(occurred_at)
        .bind(occurred_at + 1_000)
        .execute(repository.pool())
        .await
        .expect("trade plan action fixture should insert");
        let order_intent_id = uuid_text(sequence + 2_000);
        sqlx::query(
            "
            INSERT INTO order_intents(
                order_intent_id, action_id, state, created_at, payload_json
            )
            VALUES (?, ?, 'APPROVED', ?, '{}')
            ",
        )
        .bind(&order_intent_id)
        .bind(&action_id)
        .bind(occurred_at)
        .execute(repository.pool())
        .await
        .expect("order intent fixture should insert");
        sqlx::query(
            "
            INSERT INTO paper_orders(
                order_id, order_intent_id, state, created_at, updated_at, payload_json
            )
            VALUES (?, ?, 'FILLED', ?, ?, '{}')
            ",
        )
        .bind(fill.order_id().to_string())
        .bind(order_intent_id)
        .bind(occurred_at)
        .bind(occurred_at)
        .execute(repository.pool())
        .await
        .expect("paper order fixture should insert");
    }

    async fn table_count(repository: &SqliteRepository, table: &str) -> i64 {
        let query = match table {
            "fills" => "SELECT COUNT(*) FROM fills",
            "managed_lots" => "SELECT COUNT(*) FROM managed_lots",
            "audit_log" => "SELECT COUNT(*) FROM audit_log",
            "reconciliation_runs" => "SELECT COUNT(*) FROM reconciliation_runs",
            _ => panic!("test table must be explicitly allowed"),
        };
        sqlx::query_scalar(query)
            .fetch_one(repository.pool())
            .await
            .expect("table count should be readable")
    }

    fn uuid_text(value: u128) -> String {
        format!("{value:032x}")
    }
}
