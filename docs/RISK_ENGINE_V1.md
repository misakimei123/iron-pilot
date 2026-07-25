# Deterministic Risk Engine v1

> Superseded implementation evidence: DEVELOPMENT_PLAN v3 and ADR-0006 remove
> this v2 Risk Engine from the active trading path. Keep this document and its
> commit as historical evidence only. `P3-12` moved the source and tests under
> `crates/ironpilot-domain/legacy/v2`, removed them from the compiled public
> domain, and made the legacy SQLite evidence tables immutable.

`P3-02` implements the pure-domain `ironpilot-risk-rules-v1` contract. This
document records the implemented interface and evidence; it does not amend
`docs/DEVELOPMENT_PLAN.md`, redefine task dependencies, or approve a phase gate.

## Accepted input

The engine accepts `MaterializedRiskInput`, which must bind:

- a locally validated `StrategyIntent v2`;
- the executable `strategy-space-v1-vs` provenance;
- the original decision and instrument identity;
- a bounded materialization algorithm version;
- an immutable materialization hash;
- a positive requested quantity and a non-negative deterministic maximum.

For `P3-02`, the materialized entry-risk boundary is intentionally limited to
`OPEN_LONG`. `NO_TRADE`, `HOLD`, `EXIT`, non-Spot instruments, future Strategy
Space versions, and unvalidated intents cannot enter this entry-risk input.
Materialization algorithms, entry/stop/target prices, and order construction
remain owned by later tasks.

## Deterministic outcomes

The only outcomes are:

- `APPROVE`;
- `ADJUST_DOWN`;
- `REJECT`;
- `REDUCE_ONLY`;
- `HALT_SYMBOL`;
- `HALT_SYSTEM`.

The precedence is conservative: global invariant breach or halt, symbol halt,
non-entry system/symbol state, unreconciled portfolio, active TradePlan limit,
zero allowance, quantity tightening, then approval.

Only `APPROVE` and `ADJUST_DOWN` carry a `RiskAuthorization`. Its constructor is
private to the risk module, and it binds the risk decision ID, original strategy
decision ID, original instrument/action, materialization hash, and approved
quantity. Rejected or degraded decisions therefore cannot provide the token
that later execution-domain inputs must require.

`ADJUST_DOWN` can only set the approved quantity to the materialized maximum,
which is strictly below the requested quantity. Risk never creates strategy,
entry, stop, target, direction, or quantity increases.

## Portfolio and resource boundary

- Any `PortfolioSnapshot` balance difference rejects a new entry.
- Reaching the configured active TradePlan limit rejects a new entry.
- More than the frozen two-plan hard bound is treated as an invariant breach
  and halts the system.
- The configured limit itself must remain within `1..=2`.

## Traceability

Every decision binds the rules version, source decision/instrument/action,
materialization version and hash, portfolio snapshot hash, outcome/reason,
requested/approved quantity, timestamp, and a deterministic decision hash.
Persistence and audit-before-action integration remains part of the consuming
TradePlan/application boundary; this pure-domain task creates no external side
effect or execution order.
