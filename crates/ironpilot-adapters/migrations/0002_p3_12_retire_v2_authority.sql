-- P3-12 preserves v2 evidence while making every legacy authority table
-- immutable. The v3 runtime has no write path to these tables.

CREATE TRIGGER legacy_strategy_intents_forbid_insert
BEFORE INSERT ON strategy_intents
BEGIN
    SELECT RAISE(ABORT, 'strategy_intents is retired v2 evidence');
END;

CREATE TRIGGER legacy_strategy_intents_forbid_update
BEFORE UPDATE ON strategy_intents
BEGIN
    SELECT RAISE(ABORT, 'strategy_intents is retired v2 evidence');
END;

CREATE TRIGGER legacy_strategy_intents_forbid_delete
BEFORE DELETE ON strategy_intents
BEGIN
    SELECT RAISE(ABORT, 'strategy_intents is retired v2 evidence');
END;

CREATE TRIGGER legacy_materialized_trade_parameters_forbid_insert
BEFORE INSERT ON materialized_trade_parameters
BEGIN
    SELECT RAISE(ABORT, 'materialized_trade_parameters is retired v2 evidence');
END;

CREATE TRIGGER legacy_materialized_trade_parameters_forbid_update
BEFORE UPDATE ON materialized_trade_parameters
BEGIN
    SELECT RAISE(ABORT, 'materialized_trade_parameters is retired v2 evidence');
END;

CREATE TRIGGER legacy_materialized_trade_parameters_forbid_delete
BEFORE DELETE ON materialized_trade_parameters
BEGIN
    SELECT RAISE(ABORT, 'materialized_trade_parameters is retired v2 evidence');
END;

CREATE TRIGGER legacy_risk_decisions_forbid_insert
BEFORE INSERT ON risk_decisions
BEGIN
    SELECT RAISE(ABORT, 'risk_decisions is retired v2 evidence');
END;

CREATE TRIGGER legacy_risk_decisions_forbid_update
BEFORE UPDATE ON risk_decisions
BEGIN
    SELECT RAISE(ABORT, 'risk_decisions is retired v2 evidence');
END;

CREATE TRIGGER legacy_risk_decisions_forbid_delete
BEFORE DELETE ON risk_decisions
BEGIN
    SELECT RAISE(ABORT, 'risk_decisions is retired v2 evidence');
END;
