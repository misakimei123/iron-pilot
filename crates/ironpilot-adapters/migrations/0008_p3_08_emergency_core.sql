CREATE TABLE emergency_action_steps (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    emergency_action_id TEXT NOT NULL REFERENCES emergency_actions(emergency_action_id),
    step TEXT NOT NULL CHECK (
        step IN ('REQUESTED', 'ENTRY_DISABLED', 'ORDERS_CANCELLED', 'EXPOSURE_REDUCING', 'COMPLETED')
    ),
    occurred_at INTEGER NOT NULL CHECK (occurred_at >= 0),
    evidence_hash TEXT NOT NULL,
    payload_json TEXT NOT NULL CHECK (json_valid(payload_json)),
    UNIQUE(emergency_action_id, step, evidence_hash)
) STRICT;

CREATE TRIGGER emergency_action_steps_forbid_update
BEFORE UPDATE ON emergency_action_steps
BEGIN
    SELECT RAISE(ABORT, 'emergency_action_steps is append-only');
END;

CREATE TRIGGER emergency_action_steps_forbid_delete
BEFORE DELETE ON emergency_action_steps
BEGIN
    SELECT RAISE(ABORT, 'emergency_action_steps is append-only');
END;

CREATE TABLE emergency_fills (
    emergency_fill_id TEXT PRIMARY KEY,
    emergency_action_id TEXT NOT NULL REFERENCES emergency_actions(emergency_action_id),
    trade_plan_id TEXT NOT NULL REFERENCES trade_plans(trade_plan_id),
    instrument_id TEXT NOT NULL,
    observation_id TEXT NOT NULL,
    occurred_at INTEGER NOT NULL CHECK (occurred_at >= 0),
    base_quantity TEXT NOT NULL,
    execution_price TEXT NOT NULL,
    quote_quantity TEXT NOT NULL,
    fee_quote TEXT NOT NULL,
    payload_json TEXT NOT NULL CHECK (json_valid(payload_json)),
    UNIQUE(emergency_action_id, trade_plan_id, observation_id)
) STRICT;

CREATE TRIGGER emergency_fills_forbid_update
BEFORE UPDATE ON emergency_fills
BEGIN
    SELECT RAISE(ABORT, 'emergency_fills is append-only');
END;

CREATE TRIGGER emergency_fills_forbid_delete
BEFORE DELETE ON emergency_fills
BEGIN
    SELECT RAISE(ABORT, 'emergency_fills is append-only');
END;
