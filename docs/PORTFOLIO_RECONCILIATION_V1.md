# IronPilot Portfolio and Reconciliation v1

This implementation contract records the `P3-01` portfolio, managed-asset, and
reconciliation boundary. It does not change task scope, dependencies, or any
phase Gate in `docs/DEVELOPMENT_PLAN.md`.

## Version and facts

- Portfolio schema: `ironpilot-portfolio-v1`
- Portfolio fill payload: `ironpilot-portfolio-fill-v1`
- Managed lot payload: `ironpilot-managed-lot-v1`
- Every Portfolio Fill derives its instrument and base/quote assets from a
  validated P2-01 `SpotInstrumentRules` contract.
- Exchange balances are external facts supplied to the pure reconciler.
- Local expected balances and provable managed quantities are separate facts.
- Exact decimal strings are used for all quantities; binary floating point is
  not used.

For each asset, a Portfolio Snapshot records exchange available, locked and
total quantity; local expected quantity; provable managed quantity; unknown
surplus; and local shortfall. Assets are canonically sorted and the complete
snapshot has a stable SHA-256 hash.

## Entry and sell boundaries

A Portfolio Snapshot permits new entries only when every exchange total exactly
matches its local expected balance. Any unknown surplus, missing exchange
quantity, or other balance difference makes `allows_new_entries` false. Unknown
assets remain visible in reconciliation output but never become managed merely
because the exchange reports them.

A sell authorization must have a positive quantity and cannot exceed either:

1. the quantity attributable to IronPilot managed lots for that instrument; or
2. the exchange quantity currently available for the base asset.

The authorization returns the requested quantity unchanged or rejects it. It
cannot promote an unknown exchange balance into sellable managed inventory.

## Fill persistence and idempotency

The SQLite repository applies each Portfolio Fill under the active runtime
lease and in one transaction:

1. insert the stable Fill ID into the existing `fills` ledger;
2. for a buy, create one managed lot tied to its TradePlan and source Fill;
3. for a sell, consume existing managed lots in deterministic
   `(opened_at, managed_lot_id)` order;
4. append the audit record; and
5. commit the transaction.

An insufficient sell rolls back the Fill insertion and every lot mutation. An
identical repeated Fill ID returns `DuplicateNoEffect`: it creates no additional
Fill, lot, quantity change, or audit entry. Reusing an existing ID with
different content fails as an idempotency conflict.

Reconciliation snapshots use the existing `reconciliation_runs` and
append-only audit tables. A repeated identical Reconciliation Run ID likewise
has zero additional business effect.

## Deferred boundaries

This contract does not call private exchange APIs, create orders, simulate
fills, calculate fees, choose risk, or manage a TradePlan lifecycle. Those
belong to later authorized tasks. P3-01 provides the shared safety boundary
that those tasks must consume.
