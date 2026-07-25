# AI Decision Context v1 and TradePlan Ledger

`P3-03` implements the immutable fact boundary that the AI receives and the
atomic ledger that records its original response and plan. This document
records implementation evidence without changing `docs/DEVELOPMENT_PLAN.md`,
task dependencies, or any phase Gate.

## Context contents

`ironpilot-ai-decision-context-v1` contains:

- exactly the feature engine's complete raw closed-candle window for both 15m
  and 1h;
- every derived indicator and pattern in `ironpilot-market-features-v1`;
- top-of-book prices, quantities, source time, and observation time;
- target Spot instrument rules, exchange server time, rule validity, and hash;
- the full reconciled account asset snapshot;
- all supplied managed positions and open account orders;
- the user-authorized maximum loss in quote currency;
- schema versions, fact timestamps, validity, stable IDs, and a canonical
  SHA-256 hash.

The constructor recomputes `MarketFeatureSnapshot` from the supplied candles
and book. A mismatch, incomplete candle window, future candle/book/rules/
portfolio/order, stale market facts, stale rules, duplicate position/order, or
non-positive user authorization fails closed.

The serialized Context has no action, recommendation, Strategy Space,
eligibility direction, risk tier, anchor, or locally generated trade
parameter. Indicators and patterns remain facts for AI interpretation.

## Raw response and provenance

`AiRawResponse` records the provider, model, receipt time, unmodified response,
Context ID, response ID, and response hash. It is intentionally provider-neutral
so `P3-04` can supply DeepSeek request/usage metadata without changing this
provenance boundary.

Every `AiTradePlanLedgerEntry` requires:

- matching Context IDs across Context, raw response, and `AITradingPlan v3`;
- the same instrument in Context and AI plan;
- receipt and recording before both Context and plan expiry;
- the original Context, response, and plan hashes;
- a stable TradePlan ID and action ID;
- an existing matching TradePlan for management actions.

`OPEN_LONG` creates a `PROPOSED` TradePlan. `NO_TRADE` creates a terminal
`CLOSED` trace record. `HOLD`, `CANCEL_ENTRY`, `MODIFY_PROTECTION`, `REDUCE`,
and `EXIT` append to the AI-selected target TradePlan.

## Atomic SQLite ledger

Migration `0003_p3_03_ai_trade_plan_ledger.sql` adds:

- `ai_decision_contexts`;
- `ai_provider_responses`;
- `ai_trading_plans`;
- `ai_trade_plan_ledger`.

A single lease-fenced transaction writes the Context, raw response, parsed AI
plan, TradePlan/action, provenance link, and audit entry. Repeating identical
content has zero business effect; reusing an ID with different content fails
closed. Any conflict or audit failure rolls back the complete transaction.

The existing partial unique index on `trade_plans` enforces at most one
non-terminal TradePlan per instrument. The trace query joins any action back to
its Context, raw response, AI plan, and all three hashes.

## Deferred boundaries

- DeepSeek request construction, API calls, usage, cost, latency, and bounded
  replan belong to `P3-04`.
- Exchange compatibility and user-authorization acceptance/rejection belong to
  `P3-13`.
- OrderIntent and Paper execution belong to `P3-05`.
