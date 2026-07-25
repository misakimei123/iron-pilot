# DeepSeek AI Trading Plan Provider v1

`P3-04` implements the provider boundary that turns an immutable
`AiDecisionContext` into one unmodified `AITradingPlan v3`. The implementation
follows `DEVELOPMENT_PLAN v3.1.0`'s open-source-first policy without changing
task dependencies, phase order, or AI trading authority.

## Authority and prompt

`ironpilot-deepseek-trading-prompt-v1` gives DeepSeek:

- the complete 120-candle raw 15m and 1h windows from the source Context;
- all derived indicators, pattern observations, top-of-book facts, timestamps,
  and hashes;
- Spot instrument rules, exchange time, and exact decimal constraints;
- the reconciled account, managed positions, and open orders;
- the user-authorized maximum loss;
- the exact seven-action `AITradingPlan v3` output contract.

The prompt contains no Strategy Space, anchor, risk tier, local entry rule, or
parameter materialization. DeepSeek independently selects the action and every
normal trading parameter. The provider does not have an exchange adapter,
filesystem tool, shell, or account credential.

The request uses DeepSeek's OpenAI-compatible `/chat/completions` endpoint,
thinking mode, non-streaming JSON Output, a bounded output-token limit, and an
explicit instruction to return one JSON object. `async-openai 0.41.1` owns the
OpenAI-compatible base URL, authentication, chat-completion path, JSON request
serialization, HTTP execution contract, and response decoding. Its BYOT surface
keeps the DeepSeek-specific `thinking` and cache-usage fields without forking or
reimplementing the general protocol. The API key is accepted through
`IRONPILOT_DEEPSEEK_API_KEY`, held by the SDK's secret-aware configuration, and
never copied into request evidence or YAML.

IronPilot supplies one narrow Tower service around the SDK transport because
the project has stricter evidence and resource semantics than the general SDK:
it rejects response bodies above 128 KiB while streaming, captures the exact
provider body before SDK deserialization, and reconstructs the same bounded
response for the SDK. Installing this service also replaces `async-openai`'s
default retry executor, so one budgeted provider attempt always causes at most
one HTTP request. Replanning remains the separate, explicit, once-per-Context
operation described below.

## Strict result handling

The provider accepts exactly one completed choice with `finish_reason=stop`.
The content must:

- be non-empty and non-truncated;
- parse directly through the strict `AiTradingPlan::from_json` contract;
- contain no unknown fields or floating-point trading values;
- retain the source Context ID and instrument;
- remain valid when received.

Empty output, truncation, provider refusal, unknown fields, malformed plans,
model mismatch, HTTP failure, oversized response, timeout, future/expired
Context, or inconsistent usage returns an error with no locally generated
fallback plan. Because this module has no execution dependency, those outcomes
cannot produce an order.

## Usage, cost, latency, and budget

Every completed response validates:

- prompt token total = cache-hit + cache-miss tokens;
- total token count = prompt + completion tokens;
- exact cache-hit, cache-miss, and output cost;
- request and receipt timestamps;
- monotonic request latency;
- prompt version/hash, raw request, raw response, vendor response ID, model,
  finish reason, outcome, and whether the attempt was a replan.

The model price snapshot is explicit and injectable because provider prices can
change. The checked-in defaults correspond to the DeepSeek V4 Flash/Pro USD
prices published on 2026-07-25. Call, token, cost, response-size, timeout, and
concurrency budgets are bounded. A request reserves a conservative upper bound
before network I/O; exhausted budgets fail before an HTTP request is sent.

Migration `0004_p3_04_deepseek_provider.sql` stores provider-attempt evidence.
`SqliteRepository::persist_ai_provider_attempt` atomically writes the immutable
Context, attempt evidence, and audit record under the single-instance lease.
Identical repeats have zero effect, conflicting IDs fail closed, and the
database enforces at most one replan attempt per Context.

## Bounded rejection replan

`replan_after_rejection` accepts the unchanged rejected AI plan plus bounded
validator/authorization rejection reasons. It rebuilds a versioned prompt over
the same Context and asks DeepSeek for a complete replacement plan. The provider
allows exactly one such attempt per Context across all clones. A second attempt
is rejected before network I/O. It never patches, rounds, resizes, or otherwise
repairs the rejected plan locally.

## Deferred boundaries

- Execution validation and user authorization belong to `P3-13`.
- TradePlan/OrderIntent and Paper execution belong to `P3-05`.
- Runtime trigger and end-to-end orchestration belong to `P3-06`.
- No live DeepSeek request is part of the deterministic repository gate; the
  protocol suite uses a bounded local HTTP server and records the exact wire
  request/response shapes.
