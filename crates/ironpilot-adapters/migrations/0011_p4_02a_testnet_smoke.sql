CREATE TABLE bybit_testnet_order_intents (
    order_link_id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL,
    purpose TEXT NOT NULL CHECK (purpose IN ('CANCEL_PROBE', 'FILL_PROBE', 'EMERGENCY_EXIT')),
    source_order_payload_hash TEXT NOT NULL,
    sdk_request_payload_hash TEXT NOT NULL,
    sdk_request_payload_json TEXT NOT NULL CHECK (json_valid(sdk_request_payload_json)),
    exchange_order_id TEXT NOT NULL,
    acknowledged_at INTEGER NOT NULL CHECK (acknowledged_at > 0)
) STRICT;

CREATE INDEX bybit_testnet_order_intents_by_run
ON bybit_testnet_order_intents(run_id, acknowledged_at, order_link_id);

CREATE TRIGGER bybit_testnet_order_intents_forbid_update
BEFORE UPDATE ON bybit_testnet_order_intents
BEGIN
    SELECT RAISE(ABORT, 'bybit_testnet_order_intents is append-only');
END;

CREATE TRIGGER bybit_testnet_order_intents_forbid_delete
BEFORE DELETE ON bybit_testnet_order_intents
BEGIN
    SELECT RAISE(ABORT, 'bybit_testnet_order_intents is append-only');
END;

CREATE TABLE bybit_testnet_smoke_evidence (
    evidence_id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL,
    evidence_kind TEXT NOT NULL,
    observed_at INTEGER NOT NULL CHECK (observed_at > 0),
    payload_hash TEXT NOT NULL,
    payload_json TEXT NOT NULL CHECK (json_valid(payload_json))
) STRICT;

CREATE INDEX bybit_testnet_smoke_evidence_by_run
ON bybit_testnet_smoke_evidence(run_id, observed_at, evidence_id);

CREATE TRIGGER bybit_testnet_smoke_evidence_forbid_update
BEFORE UPDATE ON bybit_testnet_smoke_evidence
BEGIN
    SELECT RAISE(ABORT, 'bybit_testnet_smoke_evidence is append-only');
END;

CREATE TRIGGER bybit_testnet_smoke_evidence_forbid_delete
BEFORE DELETE ON bybit_testnet_smoke_evidence
BEGIN
    SELECT RAISE(ABORT, 'bybit_testnet_smoke_evidence is append-only');
END;
