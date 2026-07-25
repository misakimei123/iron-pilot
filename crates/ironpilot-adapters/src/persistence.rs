use core::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use ironpilot_application::{
    AuditEntry, OutboxMessage, PersistedSystemState, SystemStateChange, UnixMillis,
};
use ironpilot_domain::{
    AssetCode, DomainDecimal, InstrumentId, ManagedPosition, PortfolioFill, PortfolioFillSide,
    PortfolioReconciliationStatus, PortfolioSnapshot, ReconciliationRunId, RuntimeInstanceId,
    SystemState,
};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::migrate::Migrator;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{Row, SqlitePool};
use tokio::sync::Mutex;

static MIGRATOR: Migrator = sqlx::migrate!("./migrations");
const TRADING_RUNTIME_LOCK: &str = "trading-runtime";
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

pub struct SqliteRepository {
    pool: SqlitePool,
    database_path: PathBuf,
    write_gate: Mutex<()>,
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

async fn ensure_instance_lease(
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

async fn insert_managed_lot(
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

async fn consume_managed_lots(
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
    json!({
        "schema_version": "ironpilot-managed-lot-v1",
        "base_asset": base_asset.as_str(),
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

fn normalized_decimal(value: DomainDecimal) -> String {
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

async fn insert_audit(
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
    use std::path::{Path, PathBuf};
    use std::str::FromStr;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::{LeaseAcquireError, PersistenceEffect, SqliteRepository, StorageError};
    use ironpilot_application::{AuditEntry, OutboxMessage, SystemStateChange, UnixMillis};
    use ironpilot_domain::{
        AssetCode, AuditEntryId, DomainDecimal, ExchangeAssetBalance, FillId, InstrumentId,
        InstrumentTradingStatus, LocalAssetBalance, ManagedLotId, OrderId, OutboxMessageId,
        PortfolioFill, PortfolioFillSide, PortfolioReconciler, ReconciliationRunId,
        RuntimeInstanceId, SpotInstrumentRules, SystemState, TradePlanId,
        validated_spot_instrument_rules,
    };
    use serde_json::json;

    static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

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
                "audit_log",
                "eligibility_events",
                "emergency_actions",
                "fills",
                "managed_lots",
                "market_snapshots",
                "materialized_trade_parameters",
                "order_intents",
                "outbox",
                "paper_orders",
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
