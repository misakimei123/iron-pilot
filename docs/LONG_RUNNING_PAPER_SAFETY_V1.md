# Long-running Paper Safety Evidence v1

## Status boundary

`ironpilot-paper-soak-evidence-v1` is the evidence and qualification contract
for P3-11. Implementing this contract does not complete the Long-running Paper
Safety Gate. The Gate still requires a real, continuous 30-day Paper run.
Virtual time, generated fixtures, deterministic unit tests, and a shorter soak
cannot replace that elapsed-time evidence.

## Immutable run manifest

Each run binds:

- a stable run ID and deployment fingerprint;
- the real start time;
- Paper Runtime, Context, Prompt, Model, AI Plan, Validator, Execution, and
  Emergency versions;
- the fixed 30-day duration and five-minute maximum observation gap;
- memory and CPU ceilings;
- both bounded queue capacities;
- initial, maximum, and maximum daily SQLite growth;
- daily LLM calls, tokens, exact cost, and the one-replan ceiling.

Changing the duration or observation-gap constants makes the manifest
unsupported. The manifest has a canonical SHA-256 evidence hash.

## Periodic observations

Every observation binds its run, ID, timestamp, process and Emergency
availability, and the following cumulative or point-in-time evidence:

- RSS and CPU;
- market and critical queue depth/high-watermark;
- SQLite allocated/used bytes and tracked business-row count;
- UTC-day LLM calls, tokens, exact cost, and maximum replans observed for one
  Context;
- unexplained state divergence, unmanaged sell, duplicate business effect,
  audit gap, and local AI-plan mutation counts;
- managed-position reviews, AI management actions, and unanswered reviews.

Counters cannot regress. Same-run observation timestamps are unique and no
adjacent samples may be more than five minutes apart.

## Required failure drills

The run must contain independently persisted evidence for:

1. model timeout;
2. invalid model output;
3. market disconnect;
4. process restart;
5. resource pressure;
6. Emergency independence from AI.

Every drill must prove fail-closed behavior, recovery, zero unauthorized order
effects, zero unmanaged sells, zero duplicate business effects, zero audit
gaps, zero local AI-plan mutations, and Emergency availability without AI.

## Qualification

The evaluator returns one of:

- `collecting`: no safety violation is present, but 30 days or required
  evidence is incomplete;
- `disqualified`: a continuity, safety, resource, budget, database-growth, or
  fault-drill violation exists;
- `qualified`: the full continuous duration, every drill, AI-managed position
  evidence, and all zero-tolerance invariants are satisfied.

Profit or apparent strategy performance is not an input and cannot offset a
safety violation.

## Persistence and restart

Migration `0009_p3_11_paper_soak_evidence.sql` adds:

- `paper_soak_runs`;
- `paper_soak_observations`;
- `paper_soak_fault_evidence`.

All three tables reject UPDATE and DELETE. Replaying identical evidence has
zero effect; reusing an ID or run/timestamp for different evidence fails
closed. A report is reconstructed from stored JSON and re-evaluated after
restart rather than trusting a mutable cached verdict.

SQLite page count, page size, freelist count, and tracked business rows are
read through SQLx to provide database-growth evidence.

## Open-source reuse

No new protocol client, wire DTO, polling implementation, retry loop, or
general metrics engine is introduced. The implementation reuses the existing
Tokio runtime, SQLx SQLite/migrations, sysinfo process sampler, Serde JSON,
SHA-256, and exact Decimal dependencies. Project-owned code is limited to the
IronPilot-specific evidence contract, fail-closed qualification rules,
append-only persistence, and recovery semantics.

## Operational completion requirements

Before P3-11 can be marked `DONE`, an operator-controlled Paper environment
must:

1. run the frozen build and configuration continuously for at least 30 days;
2. inject a real DeepSeek credential and provide the required public market
   connectivity without enabling Testnet or Live writes;
3. persist observations at least every five minutes;
4. execute and preserve all six failure drills;
5. include normal AI-managed position review evidence;
6. finish with a `qualified` report and independently review its evidence hash.

Until those conditions are met, P3-11 remains incomplete regardless of unit or
integration test results.
