CREATE TABLE system_state (
    singleton_id INTEGER PRIMARY KEY CHECK (singleton_id = 1),
    state TEXT NOT NULL CHECK (
        state IN (
            'STARTING',
            'RECOVERING',
            'OBSERVING',
            'ENTRY_ENABLED',
            'REDUCE_ONLY',
            'HALTED'
        )
    ),
    updated_at INTEGER NOT NULL CHECK (updated_at >= 0)
) STRICT;

CREATE TABLE runtime_instance_lease (
    lock_name TEXT PRIMARY KEY CHECK (lock_name = 'trading-runtime'),
    owner_id TEXT NOT NULL,
    acquired_at INTEGER NOT NULL CHECK (acquired_at >= 0),
    expires_at INTEGER NOT NULL CHECK (expires_at > acquired_at)
) STRICT;

CREATE TABLE market_snapshots (
    snapshot_id TEXT PRIMARY KEY,
    instrument_id TEXT NOT NULL,
    captured_at INTEGER NOT NULL CHECK (captured_at >= 0),
    feature_version TEXT NOT NULL,
    payload_json TEXT NOT NULL CHECK (json_valid(payload_json))
) STRICT;

CREATE TABLE eligibility_events (
    event_id TEXT PRIMARY KEY,
    snapshot_id TEXT NOT NULL REFERENCES market_snapshots(snapshot_id),
    event_type TEXT NOT NULL,
    occurred_at INTEGER NOT NULL CHECK (occurred_at >= 0),
    expires_at INTEGER NOT NULL CHECK (expires_at >= occurred_at),
    payload_json TEXT NOT NULL CHECK (json_valid(payload_json))
) STRICT;

CREATE TABLE strategy_intents (
    decision_id TEXT PRIMARY KEY,
    event_id TEXT NOT NULL REFERENCES eligibility_events(event_id),
    schema_version TEXT NOT NULL,
    strategy_space_version TEXT NOT NULL,
    decided_at INTEGER NOT NULL CHECK (decided_at >= 0),
    expires_at INTEGER NOT NULL CHECK (expires_at >= decided_at),
    payload_json TEXT NOT NULL CHECK (json_valid(payload_json))
) STRICT;

CREATE TABLE materialized_trade_parameters (
    decision_id TEXT PRIMARY KEY REFERENCES strategy_intents(decision_id),
    algorithm_version TEXT NOT NULL,
    materialized_at INTEGER NOT NULL CHECK (materialized_at >= 0),
    payload_json TEXT NOT NULL CHECK (json_valid(payload_json))
) STRICT;

CREATE TABLE risk_decisions (
    risk_decision_id TEXT PRIMARY KEY,
    decision_id TEXT NOT NULL REFERENCES strategy_intents(decision_id),
    rules_version TEXT NOT NULL,
    outcome TEXT NOT NULL,
    decided_at INTEGER NOT NULL CHECK (decided_at >= 0),
    payload_json TEXT NOT NULL CHECK (json_valid(payload_json))
) STRICT;

CREATE TABLE trade_plans (
    trade_plan_id TEXT PRIMARY KEY,
    instrument_id TEXT NOT NULL,
    state TEXT NOT NULL,
    created_at INTEGER NOT NULL CHECK (created_at >= 0),
    updated_at INTEGER NOT NULL CHECK (updated_at >= created_at),
    payload_json TEXT NOT NULL CHECK (json_valid(payload_json))
) STRICT;

CREATE UNIQUE INDEX one_active_trade_plan_per_instrument
ON trade_plans(instrument_id)
WHERE state NOT IN ('REJECTED', 'CANCELLED', 'CLOSED');

CREATE TABLE trade_plan_actions (
    action_id TEXT PRIMARY KEY,
    trade_plan_id TEXT NOT NULL REFERENCES trade_plans(trade_plan_id),
    action_type TEXT NOT NULL,
    state TEXT NOT NULL,
    created_at INTEGER NOT NULL CHECK (created_at >= 0),
    expires_at INTEGER NOT NULL CHECK (expires_at >= created_at),
    payload_json TEXT NOT NULL CHECK (json_valid(payload_json))
) STRICT;

CREATE TABLE order_intents (
    order_intent_id TEXT PRIMARY KEY,
    action_id TEXT NOT NULL REFERENCES trade_plan_actions(action_id),
    state TEXT NOT NULL,
    created_at INTEGER NOT NULL CHECK (created_at >= 0),
    payload_json TEXT NOT NULL CHECK (json_valid(payload_json))
) STRICT;

CREATE TABLE paper_orders (
    order_id TEXT PRIMARY KEY,
    order_intent_id TEXT NOT NULL UNIQUE REFERENCES order_intents(order_intent_id),
    state TEXT NOT NULL,
    created_at INTEGER NOT NULL CHECK (created_at >= 0),
    updated_at INTEGER NOT NULL CHECK (updated_at >= created_at),
    payload_json TEXT NOT NULL CHECK (json_valid(payload_json))
) STRICT;

CREATE TABLE fills (
    fill_id TEXT PRIMARY KEY,
    order_id TEXT NOT NULL REFERENCES paper_orders(order_id),
    occurred_at INTEGER NOT NULL CHECK (occurred_at >= 0),
    payload_json TEXT NOT NULL CHECK (json_valid(payload_json))
) STRICT;

CREATE TABLE managed_lots (
    managed_lot_id TEXT PRIMARY KEY,
    trade_plan_id TEXT NOT NULL REFERENCES trade_plans(trade_plan_id),
    instrument_id TEXT NOT NULL,
    opened_at INTEGER NOT NULL CHECK (opened_at >= 0),
    closed_at INTEGER CHECK (closed_at IS NULL OR closed_at >= opened_at),
    payload_json TEXT NOT NULL CHECK (json_valid(payload_json))
) STRICT;

CREATE TABLE reconciliation_runs (
    reconciliation_run_id TEXT PRIMARY KEY,
    started_at INTEGER NOT NULL CHECK (started_at >= 0),
    completed_at INTEGER CHECK (completed_at IS NULL OR completed_at >= started_at),
    outcome TEXT,
    payload_json TEXT NOT NULL CHECK (json_valid(payload_json))
) STRICT;

CREATE TABLE emergency_actions (
    emergency_action_id TEXT PRIMARY KEY,
    state TEXT NOT NULL,
    requested_at INTEGER NOT NULL CHECK (requested_at >= 0),
    updated_at INTEGER NOT NULL CHECK (updated_at >= requested_at),
    payload_json TEXT NOT NULL CHECK (json_valid(payload_json))
) STRICT;

CREATE TABLE audit_log (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    audit_entry_id TEXT NOT NULL UNIQUE,
    occurred_at INTEGER NOT NULL CHECK (occurred_at >= 0),
    category TEXT NOT NULL,
    subject_id TEXT,
    payload_json TEXT NOT NULL CHECK (json_valid(payload_json))
) STRICT;

CREATE TRIGGER audit_log_forbid_update
BEFORE UPDATE ON audit_log
BEGIN
    SELECT RAISE(ABORT, 'audit_log is append-only');
END;

CREATE TRIGGER audit_log_forbid_delete
BEFORE DELETE ON audit_log
BEGIN
    SELECT RAISE(ABORT, 'audit_log is append-only');
END;

CREATE TABLE outbox (
    outbox_message_id TEXT PRIMARY KEY,
    topic TEXT NOT NULL,
    payload_json TEXT NOT NULL CHECK (json_valid(payload_json)),
    created_at INTEGER NOT NULL CHECK (created_at >= 0),
    published_at INTEGER CHECK (published_at IS NULL OR published_at >= created_at),
    attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0)
) STRICT;

CREATE INDEX pending_outbox_by_creation
ON outbox(created_at, outbox_message_id)
WHERE published_at IS NULL;
