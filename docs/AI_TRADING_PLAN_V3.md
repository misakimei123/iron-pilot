# AITradingPlan v3

This contract is the active trading-decision authority delivered by `P3-12`.
It records implementation scope and evidence without changing
`docs/DEVELOPMENT_PLAN.md`, task dependencies, or any phase Gate.

## Authority boundary

- The AI supplies the exact action, entry order, quantity, protective stop,
  take-profit quantities and prices, validity, review schedule, declared maximum
  loss, thesis, invalidation, and risks.
- Every price, quantity, amount, and confidence value is an exact decimal JSON
  string. JSON numbers and unit-bearing strings are rejected.
- Unknown fields and unknown enum values are rejected.
- The active Spot actions are `OPEN_LONG`, `NO_TRADE`, `HOLD`,
  `CANCEL_ENTRY`, `MODIFY_PROTECTION`, `REDUCE`, and `EXIT`.
- `OPEN_SHORT`, perpetual instruments, leverage, margin, Strategy Space,
  anchors, risk tiers, materialization versions, and deterministic Risk
  decisions are not part of the active contract.
- The domain parser validates structural completeness only. It does not derive,
  round, resize, rank, or replace an AI trading parameter.

The serialized plan is canonically hashed after parsing. Its private fields and
lack of a public local-strategy constructor keep downstream work anchored to the
strict AI wire contract.

## Action shapes

- `OPEN_LONG` carries an exact order, protective stop, one or more
  take-profits, declared maximum loss, and review schedule.
- `NO_TRADE` carries no execution or target-plan fields.
- `HOLD` references an existing TradePlan and carries a review schedule.
- `CANCEL_ENTRY` references an existing TradePlan and carries no replacement
  order.
- `MODIFY_PROTECTION` references an existing TradePlan and carries an AI
  replacement stop and/or take-profit set, declared maximum loss, and review
  schedule.
- `REDUCE` and `EXIT` reference an existing TradePlan and carry an exact AI
  order plus a review schedule.

Exchange compatibility, current balances, stale context, order conflicts, and
user maximum-loss authorization belong to `P3-13`. That component may only
accept or reject; it cannot rewrite this contract.

## v2 retirement

The v2 Strategy Space and deterministic Risk Engine source and tests remain
under `crates/ironpilot-domain/legacy/v2` as historical evidence, outside the
compiled crate and public API. The legacy SQLite tables remain present so prior
evidence is not deleted, but migration `0002_p3_12_retire_v2_authority.sql`
makes inserts, updates, and deletes fail closed.

The active TradePlan state machine no longer contains `MATERIALIZED` or
`RISK_APPROVED`. It moves from `PROPOSED` to `ACCEPTED` before execution states.
