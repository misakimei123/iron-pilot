use std::path::{Path, PathBuf};

use ironpilot_adapters::{
    PaperSoakPersistenceEffect, PaperSoakStorageError, SqlitePaperSoakEvidence, SqliteRepository,
};
use ironpilot_application::{
    PAPER_SOAK_MAX_OBSERVATION_GAP_MILLIS, PaperSoakFaultEvidence, PaperSoakFaultKind,
    PaperSoakLimits, PaperSoakLlmEvidence, PaperSoakManifest, PaperSoakObservation,
    PaperSoakQualificationStatus, PaperSoakResourceEvidence, PaperSoakSafetyCounters,
    PaperSoakVersions,
};
use ironpilot_domain::DomainDecimal;
use sqlx::{Connection, Executor, SqliteConnection};
use uuid::Uuid;

const START: u64 = 1_800_000_000_000;
const DAY: u64 = 24 * 60 * 60 * 1_000;

fn decimal(value: &str) -> DomainDecimal {
    value.parse().expect("fixture decimal should parse")
}

fn manifest() -> PaperSoakManifest {
    PaperSoakManifest::new(
        "paper-soak-adapter-test",
        "paper-test-a",
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

fn observation(id: &str, observed_at: u64) -> PaperSoakObservation {
    PaperSoakObservation::new(
        id,
        "paper-soak-adapter-test",
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
            1_000_000,
            900_000,
            100,
        ),
        PaperSoakLlmEvidence::new(observed_at / DAY, 10, 10_000, decimal("0.10"), 1),
        PaperSoakSafetyCounters::new(0, 0, 0, 0, 0, 1, 1, 0),
    )
    .expect("fixture observation should be valid")
}

fn fault(index: usize, kind: PaperSoakFaultKind) -> PaperSoakFaultEvidence {
    let injected_at = START + u64::try_from(index + 1).expect("fixture index fits u64") * 30_000;
    PaperSoakFaultEvidence::new(
        format!("fault-{index}"),
        "paper-soak-adapter-test",
        kind,
        injected_at,
        injected_at + 1_000,
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
}

#[tokio::test]
async fn evidence_is_append_only_idempotent_restartable_and_reports_database_growth() {
    let database_path = temporary_database_path();
    let repository = SqliteRepository::connect(&database_path, 1)
        .await
        .expect("repository should connect");
    let evidence = SqlitePaperSoakEvidence::new(&repository);

    assert_eq!(
        evidence
            .start_run(&manifest())
            .await
            .expect("run should start"),
        PaperSoakPersistenceEffect::Created
    );
    assert_eq!(
        evidence
            .start_run(&manifest())
            .await
            .expect("same run should be idempotent"),
        PaperSoakPersistenceEffect::DuplicateNoEffect
    );

    let first = observation("observation-0", START);
    let second = observation(
        "observation-1",
        START + PAPER_SOAK_MAX_OBSERVATION_GAP_MILLIS,
    );
    for observation in [&first, &second] {
        assert_eq!(
            evidence
                .append_observation(observation)
                .await
                .expect("observation should append"),
            PaperSoakPersistenceEffect::Created
        );
        assert_eq!(
            evidence
                .append_observation(observation)
                .await
                .expect("same observation should be idempotent"),
            PaperSoakPersistenceEffect::DuplicateNoEffect
        );
    }
    for (index, kind) in PaperSoakFaultKind::ALL.into_iter().enumerate() {
        let fault = fault(index, kind);
        assert_eq!(
            evidence
                .append_fault_evidence(&fault)
                .await
                .expect("fault evidence should append"),
            PaperSoakPersistenceEffect::Created
        );
    }

    let report = evidence
        .qualification_report("paper-soak-adapter-test")
        .await
        .expect("partial report should load");
    assert_eq!(report.status(), PaperSoakQualificationStatus::Collecting);
    assert_eq!(report.observation_count(), 2);
    assert_eq!(report.fault_evidence_count(), 6);

    let growth = evidence
        .sample_database_growth()
        .await
        .expect("database growth should be measurable");
    assert!(growth.allocated_bytes() >= growth.used_bytes());
    assert!(growth.used_bytes() > 0);
    assert!(growth.tracked_business_rows() >= 8);

    let conflict = observation("observation-0", START + 1);
    assert!(matches!(
        evidence.append_observation(&conflict).await,
        Err(PaperSoakStorageError::EvidenceConflict)
    ));

    let report_hash = report.evidence_hash().to_owned();
    repository.close().await;
    let repository = SqliteRepository::connect(&database_path, 1)
        .await
        .expect("repository should reconnect");
    let evidence = SqlitePaperSoakEvidence::new(&repository);
    let recovered = evidence
        .qualification_report("paper-soak-adapter-test")
        .await
        .expect("report should survive restart");
    assert_eq!(recovered.evidence_hash(), report_hash);
    repository.close().await;

    let mut connection = SqliteConnection::connect(&sqlite_url(&database_path))
        .await
        .expect("direct verification connection should open");
    let error = connection
        .execute("UPDATE paper_soak_runs SET schema_version = 'tampered'")
        .await
        .expect_err("append-only run evidence must reject update");
    assert!(error.to_string().contains("paper_soak_runs is append-only"));
    connection.close().await.expect("connection should close");
    remove_database_files(&database_path);
}

fn temporary_database_path() -> PathBuf {
    std::env::temp_dir().join(format!("ironpilot-paper-soak-{}.sqlite3", Uuid::new_v4()))
}

fn sqlite_url(path: &Path) -> String {
    format!("sqlite://{}", path.to_string_lossy().replace('\\', "/"))
}

fn remove_database_files(path: &Path) {
    for candidate in [
        path.to_path_buf(),
        PathBuf::from(format!("{}-wal", path.display())),
        PathBuf::from(format!("{}-shm", path.display())),
    ] {
        if candidate.exists() {
            std::fs::remove_file(&candidate).expect("temporary database file should be removable");
        }
    }
}
