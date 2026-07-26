CREATE TABLE paper_soak_runs (
    run_id TEXT PRIMARY KEY,
    schema_version TEXT NOT NULL,
    started_at INTEGER NOT NULL CHECK (started_at >= 0),
    manifest_hash TEXT NOT NULL,
    manifest_json TEXT NOT NULL CHECK (json_valid(manifest_json))
) STRICT;

CREATE TABLE paper_soak_observations (
    observation_id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL REFERENCES paper_soak_runs(run_id),
    observed_at INTEGER NOT NULL CHECK (observed_at >= 0),
    evidence_hash TEXT NOT NULL,
    payload_json TEXT NOT NULL CHECK (json_valid(payload_json)),
    UNIQUE(run_id, observed_at)
) STRICT;

CREATE INDEX paper_soak_observations_by_run
ON paper_soak_observations(run_id, observed_at);

CREATE TABLE paper_soak_fault_evidence (
    fault_id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL REFERENCES paper_soak_runs(run_id),
    kind TEXT NOT NULL CHECK (
        kind IN (
            'model_timeout',
            'invalid_model_output',
            'market_disconnect',
            'restart',
            'resource_pressure',
            'emergency_independence'
        )
    ),
    injected_at INTEGER NOT NULL CHECK (injected_at >= 0),
    observed_at INTEGER NOT NULL CHECK (observed_at >= injected_at),
    evidence_hash TEXT NOT NULL,
    payload_json TEXT NOT NULL CHECK (json_valid(payload_json))
) STRICT;

CREATE INDEX paper_soak_fault_evidence_by_run
ON paper_soak_fault_evidence(run_id, kind, injected_at);

CREATE TRIGGER paper_soak_runs_forbid_update
BEFORE UPDATE ON paper_soak_runs
BEGIN
    SELECT RAISE(ABORT, 'paper_soak_runs is append-only');
END;

CREATE TRIGGER paper_soak_runs_forbid_delete
BEFORE DELETE ON paper_soak_runs
BEGIN
    SELECT RAISE(ABORT, 'paper_soak_runs is append-only');
END;

CREATE TRIGGER paper_soak_observations_forbid_update
BEFORE UPDATE ON paper_soak_observations
BEGIN
    SELECT RAISE(ABORT, 'paper_soak_observations is append-only');
END;

CREATE TRIGGER paper_soak_observations_forbid_delete
BEFORE DELETE ON paper_soak_observations
BEGIN
    SELECT RAISE(ABORT, 'paper_soak_observations is append-only');
END;

CREATE TRIGGER paper_soak_fault_evidence_forbid_update
BEFORE UPDATE ON paper_soak_fault_evidence
BEGIN
    SELECT RAISE(ABORT, 'paper_soak_fault_evidence is append-only');
END;

CREATE TRIGGER paper_soak_fault_evidence_forbid_delete
BEFORE DELETE ON paper_soak_fault_evidence
BEGIN
    SELECT RAISE(ABORT, 'paper_soak_fault_evidence is append-only');
END;
