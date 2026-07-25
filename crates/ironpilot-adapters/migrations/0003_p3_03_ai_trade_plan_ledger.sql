CREATE TABLE ai_decision_contexts (
    context_id TEXT PRIMARY KEY,
    schema_version TEXT NOT NULL,
    instrument_id TEXT NOT NULL,
    as_of INTEGER NOT NULL CHECK (as_of >= 0),
    valid_until INTEGER NOT NULL CHECK (valid_until > as_of),
    maximum_loss_quote TEXT NOT NULL,
    context_hash TEXT NOT NULL UNIQUE,
    payload_json TEXT NOT NULL CHECK (json_valid(payload_json))
) STRICT;

CREATE TABLE ai_provider_responses (
    response_id TEXT PRIMARY KEY,
    context_id TEXT NOT NULL REFERENCES ai_decision_contexts(context_id),
    provider TEXT NOT NULL,
    model TEXT NOT NULL,
    received_at INTEGER NOT NULL CHECK (received_at >= 0),
    response_hash TEXT NOT NULL UNIQUE,
    raw_response TEXT NOT NULL
) STRICT;

CREATE TABLE ai_trading_plans (
    ai_plan_id TEXT PRIMARY KEY,
    context_id TEXT NOT NULL REFERENCES ai_decision_contexts(context_id),
    response_id TEXT NOT NULL UNIQUE REFERENCES ai_provider_responses(response_id),
    schema_version TEXT NOT NULL,
    instrument_id TEXT NOT NULL,
    action TEXT NOT NULL,
    created_at INTEGER NOT NULL CHECK (created_at >= 0),
    valid_until INTEGER NOT NULL CHECK (valid_until > created_at),
    plan_hash TEXT NOT NULL UNIQUE,
    payload_json TEXT NOT NULL CHECK (json_valid(payload_json))
) STRICT;

CREATE TABLE ai_trade_plan_ledger (
    action_id TEXT PRIMARY KEY REFERENCES trade_plan_actions(action_id),
    trade_plan_id TEXT NOT NULL REFERENCES trade_plans(trade_plan_id),
    context_id TEXT NOT NULL REFERENCES ai_decision_contexts(context_id),
    response_id TEXT NOT NULL REFERENCES ai_provider_responses(response_id),
    ai_plan_id TEXT NOT NULL UNIQUE REFERENCES ai_trading_plans(ai_plan_id),
    context_hash TEXT NOT NULL,
    response_hash TEXT NOT NULL,
    plan_hash TEXT NOT NULL,
    recorded_at INTEGER NOT NULL CHECK (recorded_at >= 0)
) STRICT;

CREATE INDEX ai_contexts_by_instrument_time
ON ai_decision_contexts(instrument_id, as_of, context_id);

CREATE INDEX ai_ledger_by_trade_plan_time
ON ai_trade_plan_ledger(trade_plan_id, recorded_at, action_id);
