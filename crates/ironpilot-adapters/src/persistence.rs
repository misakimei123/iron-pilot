use core::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use ironpilot_application::{
    AuditEntry, OutboxMessage, PersistedSystemState, SystemStateChange, UnixMillis,
};
use ironpilot_domain::{RuntimeInstanceId, SystemState};
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
        }
    }
}

impl std::error::Error for StorageError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Sqlx(error) => Some(error),
            Self::Migration(error) => Some(error),
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

    use super::{LeaseAcquireError, SqliteRepository, StorageError};
    use ironpilot_application::{AuditEntry, OutboxMessage, SystemStateChange, UnixMillis};
    use ironpilot_domain::{AuditEntryId, OutboxMessageId, RuntimeInstanceId, SystemState};
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

    fn uuid_text(value: u128) -> String {
        format!("{value:032x}")
    }
}
