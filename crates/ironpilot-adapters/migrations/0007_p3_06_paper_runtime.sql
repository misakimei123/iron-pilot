CREATE TABLE paper_runtime_events (
    event_id TEXT PRIMARY KEY,
    cycle_id TEXT NOT NULL,
    sequence INTEGER NOT NULL CHECK (sequence >= 0),
    instrument_id TEXT NOT NULL,
    context_id TEXT,
    event_type TEXT NOT NULL,
    occurred_at INTEGER NOT NULL CHECK (occurred_at >= 0),
    payload_json TEXT NOT NULL CHECK (json_valid(payload_json)),
    UNIQUE(cycle_id, sequence)
) STRICT;

CREATE INDEX paper_runtime_events_by_cycle
ON paper_runtime_events(cycle_id, sequence);

CREATE TRIGGER paper_runtime_events_forbid_update
BEFORE UPDATE ON paper_runtime_events
BEGIN
    SELECT RAISE(ABORT, 'paper_runtime_events is append-only');
END;

CREATE TRIGGER paper_runtime_events_forbid_delete
BEFORE DELETE ON paper_runtime_events
BEGIN
    SELECT RAISE(ABORT, 'paper_runtime_events is append-only');
END;
