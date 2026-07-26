CREATE TABLE paper_execution_submissions (
    action_id TEXT PRIMARY KEY REFERENCES execution_validations(action_id),
    trade_plan_id TEXT NOT NULL REFERENCES trade_plans(trade_plan_id),
    venue TEXT NOT NULL CHECK (venue IN ('PAPER', 'BACKTEST', 'TESTNET')),
    command TEXT NOT NULL CHECK (
        command IN ('OPEN_LONG', 'CANCEL_ENTRY', 'MODIFY_PROTECTION', 'REDUCE', 'EXIT')
    ),
    validation_hash TEXT NOT NULL,
    source_plan_hash TEXT NOT NULL,
    request_hash TEXT NOT NULL UNIQUE,
    created_at INTEGER NOT NULL CHECK (created_at >= 0),
    payload_json TEXT NOT NULL CHECK (json_valid(payload_json))
) STRICT;

CREATE TABLE paper_order_specs (
    order_id TEXT PRIMARY KEY REFERENCES paper_orders(order_id),
    action_id TEXT NOT NULL REFERENCES paper_execution_submissions(action_id),
    trade_plan_id TEXT NOT NULL REFERENCES trade_plans(trade_plan_id),
    instrument_id TEXT NOT NULL,
    role TEXT NOT NULL CHECK (
        role IN ('ENTRY', 'PROTECTIVE_STOP', 'TAKE_PROFIT', 'REDUCTION', 'EXIT')
    ),
    take_profit_index INTEGER,
    side TEXT NOT NULL CHECK (side IN ('BUY', 'SELL')),
    order_type TEXT NOT NULL CHECK (order_type IN ('LIMIT', 'MARKET')),
    quantity TEXT,
    limit_price TEXT,
    trigger_price TEXT,
    time_in_force TEXT CHECK (
        time_in_force IS NULL OR time_in_force IN ('GTC', 'IOC', 'FOK')
    ),
    expires_at INTEGER NOT NULL CHECK (expires_at >= 0),
    max_slippage_quote TEXT NOT NULL,
    decision_as_of INTEGER NOT NULL CHECK (decision_as_of >= 0),
    submitted_at INTEGER NOT NULL CHECK (submitted_at >= decision_as_of),
    filled_quantity TEXT NOT NULL DEFAULT '0',
    accumulated_quote TEXT NOT NULL DEFAULT '0',
    accumulated_fee_quote TEXT NOT NULL DEFAULT '0'
) STRICT;

CREATE INDEX paper_order_specs_by_instrument_role
ON paper_order_specs(instrument_id, role, order_id);

CREATE TABLE paper_market_observations (
    observation_id TEXT PRIMARY KEY,
    instrument_id TEXT NOT NULL,
    source_generated_at INTEGER NOT NULL CHECK (source_generated_at >= 0),
    observed_at INTEGER NOT NULL CHECK (observed_at >= source_generated_at),
    observation_hash TEXT NOT NULL UNIQUE,
    payload_json TEXT NOT NULL CHECK (json_valid(payload_json)),
    effect_json TEXT NOT NULL CHECK (json_valid(effect_json))
) STRICT;
