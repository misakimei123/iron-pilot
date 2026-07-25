CREATE TABLE ai_provider_attempts (
    attempt_id TEXT PRIMARY KEY,
    context_id TEXT NOT NULL REFERENCES ai_decision_contexts(context_id),
    provider TEXT NOT NULL,
    model TEXT NOT NULL,
    prompt_version TEXT NOT NULL,
    prompt_hash TEXT NOT NULL,
    is_replan INTEGER NOT NULL CHECK (is_replan IN (0, 1)),
    requested_at INTEGER NOT NULL CHECK (requested_at >= 0),
    received_at INTEGER CHECK (received_at IS NULL OR received_at >= requested_at),
    latency_millis INTEGER NOT NULL CHECK (latency_millis >= 0),
    raw_request TEXT NOT NULL CHECK (json_valid(raw_request)),
    raw_response TEXT,
    vendor_response_id TEXT,
    finish_reason TEXT,
    prompt_tokens INTEGER CHECK (prompt_tokens IS NULL OR prompt_tokens >= 0),
    completion_tokens INTEGER CHECK (completion_tokens IS NULL OR completion_tokens >= 0),
    cache_hit_tokens INTEGER CHECK (cache_hit_tokens IS NULL OR cache_hit_tokens >= 0),
    cache_miss_tokens INTEGER CHECK (cache_miss_tokens IS NULL OR cache_miss_tokens >= 0),
    total_tokens INTEGER CHECK (total_tokens IS NULL OR total_tokens >= 0),
    cost_usd TEXT,
    outcome TEXT NOT NULL,
    CHECK (
        (prompt_tokens IS NULL AND completion_tokens IS NULL
            AND cache_hit_tokens IS NULL AND cache_miss_tokens IS NULL
            AND total_tokens IS NULL AND cost_usd IS NULL)
        OR
        (prompt_tokens IS NOT NULL AND completion_tokens IS NOT NULL
            AND cache_hit_tokens IS NOT NULL AND cache_miss_tokens IS NOT NULL
            AND total_tokens IS NOT NULL AND cost_usd IS NOT NULL)
    )
) STRICT;

CREATE INDEX ai_provider_attempts_by_context_time
ON ai_provider_attempts(context_id, requested_at, attempt_id);

CREATE UNIQUE INDEX one_replan_per_ai_context
ON ai_provider_attempts(context_id)
WHERE is_replan = 1;
