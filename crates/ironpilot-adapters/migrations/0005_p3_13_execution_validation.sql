CREATE TABLE execution_validations (
    action_id TEXT PRIMARY KEY REFERENCES trade_plan_actions(action_id),
    trade_plan_id TEXT NOT NULL REFERENCES trade_plans(trade_plan_id),
    ai_plan_id TEXT NOT NULL UNIQUE REFERENCES ai_trading_plans(ai_plan_id),
    validator_version TEXT NOT NULL,
    outcome TEXT NOT NULL CHECK (outcome IN ('ACCEPT', 'REJECT')),
    context_hash TEXT NOT NULL,
    plan_hash TEXT NOT NULL,
    recalculated_maximum_loss_quote TEXT,
    authorized_maximum_loss_quote TEXT NOT NULL,
    validated_at INTEGER NOT NULL CHECK (validated_at >= 0),
    validation_hash TEXT NOT NULL UNIQUE,
    evidence_json TEXT NOT NULL CHECK (json_valid(evidence_json))
) STRICT;

CREATE INDEX execution_validations_by_outcome_time
ON execution_validations(outcome, validated_at, action_id);
