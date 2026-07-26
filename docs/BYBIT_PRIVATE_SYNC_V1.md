# Bybit Private Synchronization v1

`ironpilot-bybit-private-sync-v1` is the P4-01 contract for translating
SDK-decoded Bybit Spot account facts into durable IronPilot state. It does not
authorize Testnet or Live writes and does not approve a stage Gate.

## SDK and transport boundary

`patisson-bybit-sdk 0.2.3` owns private WebSocket authentication and
subscription messages, wire DTOs, decoding, heartbeat, reconnect scheduling,
and REST order/wallet response types. IronPilot starts that SDK with:

- a 32-record command queue and 256-record event queue;
- 20-second ping and 10-second pong timeout;
- five reconnect attempts, from 500 milliseconds up to 8 seconds;
- lossless bounded event backpressure from the audited local SDK patch.

IronPilot owns only Spot-domain validation, idempotency, immutable evidence,
current-fact projections, reconciliation state and Context mapping. It does
not implement Bybit endpoints, signatures, private wire DTOs or retry loops.

## Fact semantics

- A successful REST place-order response records only
  `ACKNOWLEDGED_NOT_FILLED`. It never creates a fill, balance change or
  execution fact.
- Order, execution and wallet facts come from typed SDK private events or an
  authoritative typed SDK REST reconciliation snapshot.
- Exact duplicate acknowledgements and private messages have zero additional
  business effect. An idempotency key with different content fails closed.
- Stale order or wallet updates cannot overwrite newer projections.
- Private event history, executions, acknowledgements and audit evidence are
  append-only; current order/wallet tables are replaceable projections.
- Each incoming batch and reconciliation snapshot is capped at 256 records.

## Disconnect and recovery

Any SDK reconnecting or disconnected event moves the singleton sync state to
`RECOVERY_REQUIRED`. A later connected event does not clear it. While recovery
is required, account facts cannot be loaded for a new AI Context.

Recovery requires a complete SDK-typed REST snapshot of current Spot orders
and wallet balances. Applying the snapshot and its audit evidence is one
SQLite transaction. Only then does the state become `RECONCILED`, terminal
orders disappear from the open-order projection, and Context reads resume.
Reapplying the same snapshot is a no-op; conflicting evidence fails closed.

## Next Context contract

`load_context_facts` reads the reconciled exchange wallet projection and
current non-terminal orders, reconciles them with IronPilot's local managed
balances, and returns:

- the portfolio reconciliation result;
- all current open Spot order facts;
- the latest private execution timestamp.

The caller must use this result when assembling the next AI Context. Missing,
disconnected or unreconciled exchange state returns an error instead of stale
facts.

## Deterministic evidence

Repository fixtures are decoded by the selected SDK and cover Spot order,
execution and wallet messages. Integration tests prove:

1. REST acknowledgement is not a fill.
2. Duplicate acknowledgement/event effects are zero.
3. Disconnect blocks Context reads until a complete snapshot converges.
4. Order, execution and wallet facts are available to the next Context.
5. SDK queues, heartbeat and reconnect budgets are finite and bounded.
