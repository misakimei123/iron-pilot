# Execution Validation v1

## Purpose

`ironpilot-execution-validator-v1` is the deterministic compatibility and user-authorization boundary between `AITradingPlan v3` and execution. It validates the exact AI plan and returns only `ACCEPT` or `REJECT`.

It is not a strategy Risk Engine. It never rounds, resizes, reprices, moves protection, replaces targets, selects another trade, or returns an adjusted plan.

## Required facts

Every validation binds:

- immutable AI Decision Context ID/hash and exact AI plan ID/hash;
- current instrument rules snapshot and rules hash;
- current Portfolio snapshot, managed positions, open orders, active TradePlans and maximum-loss authorization;
- fresh top-of-book and exact Spot `buyLmt`/`sellLmt` price-limit facts;
- deployment execution mode, AI-plan permission and instrument scope;
- explicit fee rate and freshness limits;
- stable TradePlan action ID and TradePlan ID.

The price-limit fact intentionally uses Bybit's public [Get Order Price Limit](https://bybit-exchange.github.io/docs/v5/market/order-price-limit) result rather than reconstructing the limit from `priceLimitRatioX/Y`: Bybit documents exact Spot `buyLmt` and `sellLmt` values, while the [price-limit formula](https://bybit-exchange.github.io/docs/v5/account/set-price-limit) also depends on index price and average premium and changed in May 2026.

## Validation

Common checks fail closed on:

- schema, Context, instrument or target-TradePlan mismatch;
- stale Context, plan, order, rules, book or price limits;
- changed rules, Portfolio, managed-position, open-order or user-authorization facts;
- disabled AI plans, unauthorized instrument or non-Paper execution effect;
- unavailable target TradePlan or conflicting exchange order.

Order-bearing actions additionally check:

- exact tick and quantity-step multiples with no local rounding;
- order-type quantity maximum, minimum order amount and time-in-force;
- current exact exchange buy/sell price limit;
- quote balance for entry or provable managed and exchange-available base quantity for sell;
- full-exit/reduction quantity semantics.

For `OPEN_LONG`, the independent worst-loss calculation is:

```text
max(entry_notional - stop_notional, 0)
+ taker_fee_rate * (entry_notional + stop_notional)
+ AI max_slippage_quote
```

The declared loss must be at least the recalculated loss, and both must be within the current user maximum-loss authorization. `MODIFY_PROTECTION` uses the current managed quantity and average entry, with the new stop when supplied and the existing stop otherwise.

## No-rewrite and execution binding

The decision contains no order or replacement plan. An accepted decision authorizes execution only when `authorizes_unchanged(plan)` confirms the supplied plan ID and canonical plan hash still match. Any change to entry, quantity, stop, target, expiry or management fields changes the plan hash and invalidates the authorization.

## Persistence and idempotency

`execution_validations` stores the validation outcome, exact Context/plan hashes, loss evidence, rejection codes, timestamp and validation hash. Under the runtime lease, the repository atomically:

1. verifies that evidence matches the persisted AI ledger action;
2. inserts validation evidence;
3. marks the action `VALIDATION_ACCEPTED` or `VALIDATION_REJECTED`;
4. moves an `OPEN_LONG` TradePlan from `PROPOSED` to `ACCEPTED` or `REJECTED`;
5. appends the audit record.

The action ID is the idempotency key. Identical replay has zero business effect; different content under the same key is rejected. P3-13 never inserts `order_intents` or orders.

## Boundaries

- The deterministic gate does not call DeepSeek or Bybit.
- Supplying fresh public/account facts is the runtime/adaptor responsibility.
- Paper order simulation and `OrderIntent` creation belong to P3-05.
- Testnet and Live remain unauthorized by the current configuration contract.
- No phase Gate is approved by this task.
