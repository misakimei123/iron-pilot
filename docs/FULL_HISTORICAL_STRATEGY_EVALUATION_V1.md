# Full Historical Strategy Evaluation v1

## Scope

`ironpilot-full-historical-evaluation-v1` is the P3-10B offline comparison and
evidence contract built on top of the P3-10A execution ledger. It compares
exactly three arms:

1. Rule-only Baseline;
2. Deterministic AI Plan Stub;
3. recorded `AITradingPlan v3`.

Every comparison ID must contain one record from each arm with the same market
fact hash, fact cutoff and decision time. All three arms bind the same Context,
Validator, Spot Execution, Paper matcher, user maximum-loss authorization, fee
rate and slippage rate through one immutable manifest. Every record also carries
an immutable validation/execution evidence hash.

Rule-only is an offline reference only. The report marks every arm and the
Rule-only surface as not production-eligible; this module is not exported to the
process composition root and cannot create a production trading decision.

## Immutable manifest

`HistoricalEvaluationManifest` binds:

- dataset hash;
- `AiDecisionContext` schema;
- Prompt and model versions;
- `AITradingPlan v3` schema plus deterministic-stub and recorded plan-set hashes;
- Execution Validator, Spot Execution and Paper matcher versions;
- `quant-metrics 0.7.0` standard-metrics library version;
- evaluation and out-of-sample time boundaries;
- starting equity and user maximum-loss authorization;
- exact base fee and slippage rates;
- one to eight named fee/slippage stress scenarios.

The canonical manifest payload has a stable SHA-256 hash. Invalid hashes,
missing stress scenarios, invalid sample splits, non-positive capital or
authorization and unbounded labels fail before evaluation.

The evaluator independently recomputes the comparison-dataset hash from every
comparison ID, arm, market-fact hash and decision cutoff. It also recomputes
the deterministic-stub and recorded plan-set hashes from AI plan provenance.
A syntactically valid manifest with different evidence bindings is rejected.

## Comparable records and fail-closed rules

The evaluator accepts at most 100,000 bounded records. Facts after the decision
instant are rejected as future data. A comparison fails unless all three arms
have exactly one record over the same immutable market facts and decision
instant.

Recorded and stub AI arms require an immutable AI plan hash. Rule-only must not
claim AI plan provenance. A non-zero local parameter-mutation count fails the
whole evaluation. Rejected records require bounded rejection reasons and cannot
claim PnL or cost; `NO_TRADE` records likewise have zero execution effect.

Any safety-invariant failure rejects the complete report before profitability
is considered. Consequently positive PnL can never hide an unauthorized sale,
duplicate business effect, trace gap or other recorded safety failure.
Executed records must settle strictly after the decision instant, preserving
the P3-10A prohibition on same-decision-fact execution.

## Report

Standard total-return percentage, maximum-drawdown percentage and per-trade
expectancy are delegated to the mature, pure-math `quant-metrics 0.7.0` crate,
which operates on `rust_decimal::Decimal`. IronPilot does not duplicate those
formulas. It retains exact quote-denominated PnL, cost and drawdown amounts
because they are part of the project evidence contract.

For every arm, the deterministic report contains full-sample and out-of-sample:

- gross and net PnL;
- total-return percentage;
- maximum drawdown as both quote amount and positive percentage magnitude;
- expectancy;
- decision and executed-trade counts;
- `NO_TRADE` and rejection counts;
- total fee and slippage cost;
- rejection-reason counts.

Each stress scenario recomputes net PnL, total cost and maximum drawdown with
exact decimal fee/slippage multipliers. Per-comparison rows show every arm's
outcome, AI plan provenance where applicable, execution evidence hash, net PnL
and recorded-AI deltas against the other two arms. AI contribution also
includes aggregate net-PnL deltas and decision-divergence counts.

An independently produced reference artifact must provide exact net PnL,
maximum drawdown, trade count and out-of-sample net PnL for all three arms.
Mismatch fails closed. A successful report records the reference source/hash,
sets `tie_out=true`, and receives its own stable SHA-256 report hash.

## Boundaries

This module performs no parameter search, optimization, model call, exchange
call or production strategy selection. It adds no Materializer, strategy-style
Risk Engine or news node. The P3-10A harness remains responsible for producing
the shared Validator/TradePlan/Paper execution ledgers; P3-10B only evaluates
immutable evidence from comparable runs.
