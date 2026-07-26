# AI-Dominant Paper Runtime v1

## Scope

`ironpilot-ai-paper-runtime-v1` is the P3-06 application runtime for one bounded decision cycle:

```text
complete market/account facts
→ AiDecisionContext
→ DeepSeek or a recorded provider
→ AITradingPlan v3
→ immutable AI plan ledger
→ ExecutionValidator ACCEPT/REJECT
→ unchanged SpotExecutionRequest
→ SQLite Paper execution
→ fills/orders/managed-position facts
→ a later AI review cycle (HOLD/MODIFY_PROTECTION/REDUCE/EXIT)
```

Triggering is external to the cycle. A trigger may represent a closed candle, meaningful fact
change, order/fill transition, scheduled review, recovery or explicit re-evaluation. It decides
when to call AI, not whether a trade exists.

## Authority boundary

- The provider supplies every normal order, entry, quantity, stop, target, slippage, expiry and
  review parameter.
- The runtime creates IDs, timestamps, Contexts, audit records and protocol envelopes only.
- Validation returns `ACCEPT` or `REJECT`; it never edits the AI plan.
- An accepted executable action is mapped only by
  `SpotExecutionRequest::from_accepted_plan` and sent to the shared Paper adapter.
- `NO_TRADE` and `HOLD` are persisted and complete with no execution request.
- A rejected plan may consume at most one provider replan for the same Context. The runtime never
  repairs or substitutes the rejected parameters.
- Provider failure, invalid output or exhausted budget produces `ProviderNoAction`; there is no
  local fallback plan.

The active path contains no news node, Strategy Materializer or strategy-style Risk Engine.

## Facts and review

`PaperRuntimeFacts` owns the complete reproducible inputs required to construct
`AiDecisionContext`: closed 15m/1h candles, market features, top of book, current instrument
rules, Portfolio, managed positions, active orders and the user maximum-loss authorization.

The provider input uses Runtime Prompt v2. It binds the Context to a bounded, hashed runtime
state containing the active TradePlan ID, its original `AITradingPlan`, and the latest execution
result. This gives the model the exact target and prior intent needed for
`HOLD`/`MODIFY_PROTECTION`/`REDUCE`/`EXIT`. Initial entry cycles carry an empty runtime state.
The existing Prompt v1 path remains available and structurally unchanged for non-runtime callers.

Each cycle validates a fresh owned execution-fact snapshot. If rules, Portfolio, managed
positions, orders, authorization or time evidence changed after Context construction, validation
fails closed. Paper fills and order changes are not silently projected into the next Context; the
caller must supply the newly observed facts for the next review cycle. This makes AI the authority
for normal HOLD, protection changes, reductions and exits.

## Trace and recovery

`paper_runtime_events` is an append-only, gap-free per-cycle event ledger. Events bind the cycle to
Context, provider result, raw/parsed AI plan, validation, exact execution request, Paper
observations, fills and terminal report. Existing provider-attempt, AI-plan, validation, execution,
order, fill, ManagedLot and audit tables retain the detailed evidence.

- A completed cycle is restart-idempotent: the stored terminal report is returned with
  `DuplicateNoEffect`, without another AI call or order.
- An incomplete cycle returns `RecoveryRequired`. New AI work is blocked until persisted Context,
  plan, orders and account facts are restored; startup never automatically opens a position.
- Context-construction failures, provider failures and validation rejections also receive terminal
  runtime reports and event evidence.

The runtime permits at most two provider attempts (initial plus one replan) and at most 10,000
Paper observations per cycle. It does not authorize Testnet or live exchange writes and does not
approve any phase Gate.
