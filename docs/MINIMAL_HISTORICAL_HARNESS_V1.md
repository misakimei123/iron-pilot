# Minimal Historical Harness v1

## Scope

`ironpilot-minimal-historical-harness-v1` is the P3-10A deterministic proof path:

```text
recorded AiDecisionContext + recorded AiTradingPlan
→ ExecutionValidator
→ accepted TradePlan action
→ SpotExecutionRequest
→ SQLite Paper execution
→ ordered historical market observations
→ fills, fees, managed lots and protection state
```

The harness deliberately accepts an `AiTradePlanLedgerEntry`; it has no AI provider, HTTP client,
prompt, retry, token budget or live-LLM path. It is not a strategy generator and does not contain a
Materializer or post-AI Risk Engine.

## Deterministic input

`MinimalHistoricalReplayInput` freezes:

- the complete recorded Context, raw provider response and parsed `AITradingPlan v3`;
- all facts consumed by the production `ExecutionValidator`;
- stable order-intent and order IDs;
- exact submission time and Spot instrument rules;
- an ordered finite list of `PaperMarketObservation` values.

`HistoricalValidationFacts` calls the same `ExecutionValidator` used by Paper runtime. The accepted
plan is mapped with the same `SpotExecutionRequest::from_accepted_plan` contract. Matching,
maker/taker fees, market slippage, partial fills, protection and ManagedLot accounting use the same
`SqlitePaperExecutionPort` and `PaperMatchingEngine` as Paper execution.

## No-look-ahead boundary

Before any ledger write, the harness rejects the entire input when:

- an observation belongs to another instrument;
- an observation reuses a source fact whose generation time is not later than Context `as_of`;
- observation times are not strictly increasing;
- an observation ID repeats.
- the observation list is empty or exceeds the fixed 10,000-observation bound.

The underlying Paper adapter repeats the decision-fact check inside its own transaction.

## Reproducibility evidence

Every successful run emits a `MinimalHistoricalReplayReport` with:

- Context, plan, validation and execution-request hashes;
- one decision record followed by one record per observation;
- stable fill IDs;
- a cumulative ledger hash after every record.

The cumulative hash is append-only. Replaying identical input produces identical report and
canonical SQLite ledger rows. Appending later observations preserves every existing record and
cumulative prefix hash. The report is evidence for the execution chain only; P3-10A does not build
a performance engine, optimizer, parameter search, portfolio analytics platform or complete
historical strategy evaluation.
