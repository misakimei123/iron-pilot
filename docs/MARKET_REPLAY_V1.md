# IronPilot Market Replay v1

> Historical v2 evidence: `P3-12` replaced this active contract with
> `docs/MARKET_REPLAY_V2.md`, whose manifest binds versioned AI Decision Context
> and `AITradingPlan v3` evidence. This document remains unchanged in scope so
> the original P2-04 evidence is understandable.

This implementation contract records the deterministic, no-trade replay
boundary delivered by `P2-04`. It does not change task scope, dependencies, or
any phase Gate in `docs/DEVELOPMENT_PLAN.md`.

## Version bindings

- Manifest schema: `ironpilot-market-replay-v1`
- Report schema: `ironpilot-market-replay-report-v1`
- Feature engine: `ironpilot-market-features-v1`
- Vertical Slice Strategy Space: `strategy-space-v1-vs`
- Clock step: closed 15-minute candle boundaries
- Randomness: the manifest records the fixed v1 seed; runtime entropy and wall
  clock time are not inputs

The manifest hash covers every binding above, the inclusive clock range, the
ordered instrument set, and the immutable dataset hash. The dataset hash covers
all closed 15-minute and 1-hour candles plus all top-of-book observations using
length-delimited canonical fields and normalized exact decimals.

## Replay rules

1. A dataset contains one to three unique Bybit Spot instruments.
2. Candle series are strictly ordered, contiguous, instrument-scoped, and
   timeframe-scoped. Book observations are strictly ordered.
3. The replay clock advances only by its fixed 15-minute step.
4. At each clock instant, the runner exposes only candles whose close time and
   books whose observation time are less than or equal to that instant.
5. The existing `MarketFeatureEngine` computes every snapshot with the
   `Replay` source. The existing `EligibilityEventEngine` evaluates it under the
   Vertical Slice default policy.
6. A dataset or manifest hash mismatch fails closed before replay.
7. Re-running the same manifest and dataset starts from fresh deterministic
   eligibility state and produces the same report and output hash.

## Report boundary

The canonical JSON report records the manifest hash, fixed version bindings,
fixed seed, clock instant, instrument, latest visible candle closes, snapshot
hash, and either the eligibility event hash/kinds or deterministic rejection
reasons.

The report is reproduction evidence only. It contains no order execution,
position accounting, performance metrics, PnL conclusion, external narrative,
or news dependency. Those concerns belong to later tasks and cannot be inferred
from this artifact.
