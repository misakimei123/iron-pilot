use core::fmt;

use ironpilot_application::{
    PAPER_SOAK_EVIDENCE_VERSION_V1, PaperSoakEvaluator, PaperSoakEvidenceError,
    PaperSoakFaultEvidence, PaperSoakFaultKind, PaperSoakManifest, PaperSoakObservation,
    PaperSoakQualificationReport,
};
use sqlx::Row;

use crate::SqliteRepository;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaperSoakPersistenceEffect {
    Created,
    DuplicateNoEffect,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SqliteDatabaseGrowthEvidence {
    allocated_bytes: u64,
    used_bytes: u64,
    tracked_business_rows: u64,
}

impl SqliteDatabaseGrowthEvidence {
    #[must_use]
    pub const fn allocated_bytes(self) -> u64 {
        self.allocated_bytes
    }

    #[must_use]
    pub const fn used_bytes(self) -> u64 {
        self.used_bytes
    }

    #[must_use]
    pub const fn tracked_business_rows(self) -> u64 {
        self.tracked_business_rows
    }
}

pub struct SqlitePaperSoakEvidence<'repository> {
    repository: &'repository SqliteRepository,
}

impl<'repository> SqlitePaperSoakEvidence<'repository> {
    #[must_use]
    pub const fn new(repository: &'repository SqliteRepository) -> Self {
        Self { repository }
    }

    pub async fn start_run(
        &self,
        manifest: &PaperSoakManifest,
    ) -> Result<PaperSoakPersistenceEffect, PaperSoakStorageError> {
        manifest.validate()?;
        let payload_json = serde_json::to_string(manifest)?;
        let evidence_hash = manifest.evidence_hash()?;
        let started_at = to_i64(manifest.started_at_unix_millis())?;
        let _write_guard = self.repository.write_gate.lock().await;
        let inserted = sqlx::query(
            "
            INSERT INTO paper_soak_runs(
                run_id, schema_version, started_at, manifest_hash, manifest_json
            )
            VALUES (?, ?, ?, ?, ?)
            ON CONFLICT DO NOTHING
            ",
        )
        .bind(manifest.run_id())
        .bind(PAPER_SOAK_EVIDENCE_VERSION_V1)
        .bind(started_at)
        .bind(evidence_hash.as_ref())
        .bind(&payload_json)
        .execute(&self.repository.pool)
        .await?;
        if inserted.rows_affected() == 1 {
            return Ok(PaperSoakPersistenceEffect::Created);
        }

        let stored_hash: Option<String> =
            sqlx::query_scalar("SELECT manifest_hash FROM paper_soak_runs WHERE run_id = ?")
                .bind(manifest.run_id())
                .fetch_optional(&self.repository.pool)
                .await?;
        if stored_hash.as_deref() == Some(evidence_hash.as_ref()) {
            Ok(PaperSoakPersistenceEffect::DuplicateNoEffect)
        } else {
            Err(PaperSoakStorageError::EvidenceConflict)
        }
    }

    pub async fn append_observation(
        &self,
        observation: &PaperSoakObservation,
    ) -> Result<PaperSoakPersistenceEffect, PaperSoakStorageError> {
        observation.validate()?;
        let payload_json = serde_json::to_string(observation)?;
        let evidence_hash = observation.evidence_hash()?;
        let observed_at = to_i64(observation.observed_at_unix_millis())?;
        let _write_guard = self.repository.write_gate.lock().await;
        let inserted = sqlx::query(
            "
            INSERT INTO paper_soak_observations(
                observation_id, run_id, observed_at, evidence_hash, payload_json
            )
            VALUES (?, ?, ?, ?, ?)
            ON CONFLICT DO NOTHING
            ",
        )
        .bind(observation.observation_id())
        .bind(observation.run_id())
        .bind(observed_at)
        .bind(evidence_hash.as_ref())
        .bind(&payload_json)
        .execute(&self.repository.pool)
        .await?;
        if inserted.rows_affected() == 1 {
            return Ok(PaperSoakPersistenceEffect::Created);
        }

        let stored_hash: Option<String> = sqlx::query_scalar(
            "
            SELECT evidence_hash
            FROM paper_soak_observations
            WHERE observation_id = ? OR (run_id = ? AND observed_at = ?)
            ",
        )
        .bind(observation.observation_id())
        .bind(observation.run_id())
        .bind(observed_at)
        .fetch_optional(&self.repository.pool)
        .await?;
        if stored_hash.as_deref() == Some(evidence_hash.as_ref()) {
            Ok(PaperSoakPersistenceEffect::DuplicateNoEffect)
        } else {
            Err(PaperSoakStorageError::EvidenceConflict)
        }
    }

    pub async fn append_fault_evidence(
        &self,
        evidence: &PaperSoakFaultEvidence,
    ) -> Result<PaperSoakPersistenceEffect, PaperSoakStorageError> {
        evidence.validate()?;
        let payload_json = serde_json::to_string(evidence)?;
        let evidence_hash = evidence.evidence_hash()?;
        let injected_at = to_i64(evidence.injected_at_unix_millis())?;
        let observed_at = to_i64(evidence.observed_at_unix_millis())?;
        let _write_guard = self.repository.write_gate.lock().await;
        let inserted = sqlx::query(
            "
            INSERT INTO paper_soak_fault_evidence(
                fault_id, run_id, kind, injected_at, observed_at, evidence_hash, payload_json
            )
            VALUES (?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT DO NOTHING
            ",
        )
        .bind(evidence.fault_id())
        .bind(evidence.run_id())
        .bind(fault_kind_text(evidence.kind()))
        .bind(injected_at)
        .bind(observed_at)
        .bind(evidence_hash.as_ref())
        .bind(&payload_json)
        .execute(&self.repository.pool)
        .await?;
        if inserted.rows_affected() == 1 {
            return Ok(PaperSoakPersistenceEffect::Created);
        }

        let stored_hash: Option<String> = sqlx::query_scalar(
            "SELECT evidence_hash FROM paper_soak_fault_evidence WHERE fault_id = ?",
        )
        .bind(evidence.fault_id())
        .fetch_optional(&self.repository.pool)
        .await?;
        if stored_hash.as_deref() == Some(evidence_hash.as_ref()) {
            Ok(PaperSoakPersistenceEffect::DuplicateNoEffect)
        } else {
            Err(PaperSoakStorageError::EvidenceConflict)
        }
    }

    pub async fn qualification_report(
        &self,
        run_id: &str,
    ) -> Result<PaperSoakQualificationReport, PaperSoakStorageError> {
        let manifest_row = sqlx::query(
            "SELECT manifest_hash, manifest_json FROM paper_soak_runs WHERE run_id = ?",
        )
        .bind(run_id)
        .fetch_optional(&self.repository.pool)
        .await?
        .ok_or(PaperSoakStorageError::RunNotFound)?;
        let manifest_hash: String = manifest_row.try_get("manifest_hash")?;
        let manifest_json: String = manifest_row.try_get("manifest_json")?;
        let manifest: PaperSoakManifest = serde_json::from_str(&manifest_json)?;
        if manifest.evidence_hash()?.as_ref() != manifest_hash {
            return Err(PaperSoakStorageError::EvidenceHashMismatch);
        }

        let observation_rows = sqlx::query(
            "
            SELECT evidence_hash, payload_json
            FROM paper_soak_observations
            WHERE run_id = ?
            ORDER BY observed_at, observation_id
            ",
        )
        .bind(run_id)
        .fetch_all(&self.repository.pool)
        .await?;
        let mut observations = Vec::with_capacity(observation_rows.len());
        for row in observation_rows {
            let stored_hash: String = row.try_get("evidence_hash")?;
            let payload_json: String = row.try_get("payload_json")?;
            let observation: PaperSoakObservation = serde_json::from_str(&payload_json)?;
            if observation.evidence_hash()?.as_ref() != stored_hash {
                return Err(PaperSoakStorageError::EvidenceHashMismatch);
            }
            observations.push(observation);
        }

        let fault_rows = sqlx::query(
            "
            SELECT evidence_hash, payload_json
            FROM paper_soak_fault_evidence
            WHERE run_id = ?
            ORDER BY kind, injected_at, fault_id
            ",
        )
        .bind(run_id)
        .fetch_all(&self.repository.pool)
        .await?;
        let mut faults = Vec::with_capacity(fault_rows.len());
        for row in fault_rows {
            let stored_hash: String = row.try_get("evidence_hash")?;
            let payload_json: String = row.try_get("payload_json")?;
            let evidence: PaperSoakFaultEvidence = serde_json::from_str(&payload_json)?;
            if evidence.evidence_hash()?.as_ref() != stored_hash {
                return Err(PaperSoakStorageError::EvidenceHashMismatch);
            }
            faults.push(evidence);
        }

        PaperSoakEvaluator::evaluate(&manifest, &observations, &faults)
            .map_err(PaperSoakStorageError::from)
    }

    pub async fn sample_database_growth(
        &self,
    ) -> Result<SqliteDatabaseGrowthEvidence, PaperSoakStorageError> {
        let page_count: i64 = sqlx::query_scalar("PRAGMA page_count")
            .fetch_one(&self.repository.pool)
            .await?;
        let page_size: i64 = sqlx::query_scalar("PRAGMA page_size")
            .fetch_one(&self.repository.pool)
            .await?;
        let freelist_count: i64 = sqlx::query_scalar("PRAGMA freelist_count")
            .fetch_one(&self.repository.pool)
            .await?;
        let row = sqlx::query(
            "
            SELECT
                (SELECT COUNT(*) FROM ai_decision_contexts)
              + (SELECT COUNT(*) FROM ai_provider_attempts)
              + (SELECT COUNT(*) FROM ai_provider_responses)
              + (SELECT COUNT(*) FROM ai_trade_plan_ledger)
              + (SELECT COUNT(*) FROM ai_trading_plans)
              + (SELECT COUNT(*) FROM audit_log)
              + (SELECT COUNT(*) FROM outbox)
              + (SELECT COUNT(*) FROM execution_validations)
              + (SELECT COUNT(*) FROM order_intents)
              + (SELECT COUNT(*) FROM paper_orders)
              + (SELECT COUNT(*) FROM fills)
              + (SELECT COUNT(*) FROM paper_market_observations)
              + (SELECT COUNT(*) FROM paper_runtime_events)
              + (SELECT COUNT(*) FROM reconciliation_runs)
              + (SELECT COUNT(*) FROM emergency_action_steps)
              + (SELECT COUNT(*) FROM emergency_fills)
              + (SELECT COUNT(*) FROM paper_soak_observations)
              + (SELECT COUNT(*) FROM paper_soak_fault_evidence)
                AS tracked_rows
            ",
        )
        .fetch_one(&self.repository.pool)
        .await?;
        let tracked_rows: i64 = row.try_get("tracked_rows")?;

        let page_count = nonnegative_u64(page_count)?;
        let page_size = nonnegative_u64(page_size)?;
        let freelist_count = nonnegative_u64(freelist_count)?;
        let tracked_business_rows = nonnegative_u64(tracked_rows)?;
        let allocated_bytes = page_count
            .checked_mul(page_size)
            .ok_or(PaperSoakStorageError::IntegerOutOfRange)?;
        let used_pages = page_count
            .checked_sub(freelist_count)
            .ok_or(PaperSoakStorageError::IntegerOutOfRange)?;
        let used_bytes = used_pages
            .checked_mul(page_size)
            .ok_or(PaperSoakStorageError::IntegerOutOfRange)?;
        Ok(SqliteDatabaseGrowthEvidence {
            allocated_bytes,
            used_bytes,
            tracked_business_rows,
        })
    }
}

const fn fault_kind_text(kind: PaperSoakFaultKind) -> &'static str {
    match kind {
        PaperSoakFaultKind::ModelTimeout => "model_timeout",
        PaperSoakFaultKind::InvalidModelOutput => "invalid_model_output",
        PaperSoakFaultKind::MarketDisconnect => "market_disconnect",
        PaperSoakFaultKind::Restart => "restart",
        PaperSoakFaultKind::ResourcePressure => "resource_pressure",
        PaperSoakFaultKind::EmergencyIndependence => "emergency_independence",
    }
}

fn to_i64(value: u64) -> Result<i64, PaperSoakStorageError> {
    i64::try_from(value).map_err(|_| PaperSoakStorageError::IntegerOutOfRange)
}

fn nonnegative_u64(value: i64) -> Result<u64, PaperSoakStorageError> {
    u64::try_from(value).map_err(|_| PaperSoakStorageError::IntegerOutOfRange)
}

#[derive(Debug)]
pub enum PaperSoakStorageError {
    Sqlx(sqlx::Error),
    Json(serde_json::Error),
    Evidence(PaperSoakEvidenceError),
    RunNotFound,
    EvidenceConflict,
    EvidenceHashMismatch,
    IntegerOutOfRange,
}

impl fmt::Display for PaperSoakStorageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sqlx(error) => write!(formatter, "paper soak SQLite operation failed: {error}"),
            Self::Json(error) => write!(formatter, "paper soak JSON operation failed: {error}"),
            Self::Evidence(error) => write!(formatter, "paper soak evidence is invalid: {error}"),
            Self::RunNotFound => formatter.write_str("paper soak run was not found"),
            Self::EvidenceConflict => {
                formatter.write_str("paper soak evidence ID conflicts with stored evidence")
            }
            Self::EvidenceHashMismatch => {
                formatter.write_str("paper soak evidence hash does not match stored payload")
            }
            Self::IntegerOutOfRange => {
                formatter.write_str("paper soak integer is outside the supported range")
            }
        }
    }
}

impl std::error::Error for PaperSoakStorageError {}

impl From<sqlx::Error> for PaperSoakStorageError {
    fn from(value: sqlx::Error) -> Self {
        Self::Sqlx(value)
    }
}

impl From<serde_json::Error> for PaperSoakStorageError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

impl From<PaperSoakEvidenceError> for PaperSoakStorageError {
    fn from(value: PaperSoakEvidenceError) -> Self {
        Self::Evidence(value)
    }
}
