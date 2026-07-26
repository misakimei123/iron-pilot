CREATE TABLE bybit_order_acks (
    order_link_id TEXT PRIMARY KEY,
    exchange_order_id TEXT NOT NULL,
    acknowledged_at INTEGER NOT NULL CHECK (acknowledged_at > 0),
    payload_hash TEXT NOT NULL,
    payload_json TEXT NOT NULL CHECK (json_valid(payload_json))
) STRICT;

CREATE TRIGGER bybit_order_acks_forbid_update
BEFORE UPDATE ON bybit_order_acks
BEGIN
    SELECT RAISE(ABORT, 'bybit_order_acks is append-only');
END;

CREATE TRIGGER bybit_order_acks_forbid_delete
BEFORE DELETE ON bybit_order_acks
BEGIN
    SELECT RAISE(ABORT, 'bybit_order_acks is append-only');
END;

CREATE TABLE bybit_private_events (
    event_key TEXT PRIMARY KEY,
    event_kind TEXT NOT NULL CHECK (event_kind IN ('ORDER', 'EXECUTION', 'WALLET')),
    source_message_id TEXT NOT NULL,
    occurred_at INTEGER NOT NULL CHECK (occurred_at > 0),
    payload_hash TEXT NOT NULL,
    payload_json TEXT NOT NULL CHECK (json_valid(payload_json))
) STRICT;

CREATE INDEX bybit_private_events_by_time
ON bybit_private_events(occurred_at, event_key);

CREATE TRIGGER bybit_private_events_forbid_update
BEFORE UPDATE ON bybit_private_events
BEGIN
    SELECT RAISE(ABORT, 'bybit_private_events is append-only');
END;

CREATE TRIGGER bybit_private_events_forbid_delete
BEFORE DELETE ON bybit_private_events
BEGIN
    SELECT RAISE(ABORT, 'bybit_private_events is append-only');
END;

CREATE TABLE bybit_order_facts (
    exchange_order_id TEXT PRIMARY KEY,
    order_link_id TEXT,
    instrument_id TEXT NOT NULL,
    side TEXT NOT NULL CHECK (side IN ('BUY', 'SELL')),
    order_type TEXT NOT NULL CHECK (order_type IN ('LIMIT', 'MARKET')),
    limit_price TEXT,
    original_quantity TEXT NOT NULL,
    filled_quantity TEXT NOT NULL,
    status TEXT NOT NULL CHECK (
        status IN (
            'NEW',
            'PARTIALLY_FILLED',
            'PENDING_CANCEL',
            'FILLED',
            'CANCELLED',
            'REJECTED'
        )
    ),
    updated_at INTEGER NOT NULL CHECK (updated_at > 0),
    payload_hash TEXT NOT NULL,
    payload_json TEXT NOT NULL CHECK (json_valid(payload_json))
) STRICT;

CREATE UNIQUE INDEX bybit_order_facts_by_link_id
ON bybit_order_facts(order_link_id)
WHERE order_link_id IS NOT NULL;

CREATE TABLE bybit_execution_facts (
    execution_id TEXT PRIMARY KEY,
    exchange_order_id TEXT NOT NULL,
    order_link_id TEXT,
    instrument_id TEXT NOT NULL,
    side TEXT NOT NULL CHECK (side IN ('BUY', 'SELL')),
    quantity TEXT NOT NULL,
    price TEXT NOT NULL,
    fee_quantity TEXT NOT NULL,
    fee_asset TEXT NOT NULL,
    occurred_at INTEGER NOT NULL CHECK (occurred_at > 0),
    payload_hash TEXT NOT NULL,
    payload_json TEXT NOT NULL CHECK (json_valid(payload_json))
) STRICT;

CREATE INDEX bybit_execution_facts_by_order
ON bybit_execution_facts(exchange_order_id, occurred_at, execution_id);

CREATE TRIGGER bybit_execution_facts_forbid_update
BEFORE UPDATE ON bybit_execution_facts
BEGIN
    SELECT RAISE(ABORT, 'bybit_execution_facts is append-only');
END;

CREATE TRIGGER bybit_execution_facts_forbid_delete
BEFORE DELETE ON bybit_execution_facts
BEGIN
    SELECT RAISE(ABORT, 'bybit_execution_facts is append-only');
END;

CREATE TABLE bybit_wallet_facts (
    asset TEXT PRIMARY KEY,
    wallet_quantity TEXT NOT NULL,
    locked_quantity TEXT NOT NULL,
    observed_at INTEGER NOT NULL CHECK (observed_at > 0),
    payload_hash TEXT NOT NULL,
    payload_json TEXT NOT NULL CHECK (json_valid(payload_json))
) STRICT;

CREATE TABLE bybit_private_sync_state (
    singleton_id INTEGER PRIMARY KEY CHECK (singleton_id = 1),
    state TEXT NOT NULL CHECK (state IN ('LIVE', 'RECOVERY_REQUIRED', 'RECONCILED')),
    generation INTEGER NOT NULL CHECK (generation >= 0),
    updated_at INTEGER NOT NULL CHECK (updated_at > 0),
    payload_json TEXT NOT NULL CHECK (json_valid(payload_json))
) STRICT;
