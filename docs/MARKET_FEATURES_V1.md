# IronPilot Market Features v1 Contract

> Implementation contract: `ironpilot-market-features-v1`
>
> This document freezes implementation semantics for `P2-03`. It does not alter
> task scope, dependencies, acceptance criteria, or any phase Gate in
> `docs/DEVELOPMENT_PLAN.md`.

## 1. Authority Boundary

Market features and pattern observations are deterministic, read-only market
facts. Eligibility events only authorize a later AI decision attempt. Neither
component may choose:

- trade direction;
- strategy family;
- entry, stop, target, quantity, or execution policy;
- a fallback trade when data or budget checks fail.

No news input, private exchange stream, account state mutation, or trading
write operation belongs to this contract.

## 2. Canonical Input

Each snapshot is bound to one `bybit:spot:*` instrument and contains:

- the last 120 continuous, confirmed 15-minute candles;
- the last 120 continuous, confirmed 1-hour candles;
- a non-crossed positive best bid and ask observed no more than 30 seconds ago;
- an explicit evaluation timestamp and source provenance.

Candles must use exact decimal OHLCV/turnover values, be aligned to their
timeframe, strictly ordered, unique, gap-free, non-future, and fresh. The
latest 1-hour close must equal the hour boundary at or before the latest
15-minute close.

Any future, stale, gap, duplicate, out-of-order, mismatched, unclosed, invalid
OHLC, or insufficient warm-up input fails closed. No partial snapshot is
published.

## 3. Frozen Numeric Semantics

All calculations use exact base-10 decimal arithmetic. Derived indicator and
spread values use half-even rounding to eight decimal places.

| Field | Frozen definition |
|---|---|
| Donchian upper | Maximum high of the 20 candles before the current candle |
| Donchian lower | Minimum low of the 10 candles before the current candle |
| ATR | Wilder ATR(20), seeded by the simple mean of the first 20 true ranges |
| EMA fast | EMA(20), seeded by the simple mean of the first 20 closes |
| EMA slow | EMA(50), seeded by the simple mean of the first 50 closes |
| Volume ratio | Current volume divided by the mean volume of the previous 20 candles |
| RSI | Wilder RSI(14) |
| ADX | Wilder ADX(14), requiring at least 28 candles |
| Spread | `(ask - bid) / midpoint * 10000` basis points |

RSI is `100` when loss is zero and gain is positive, and `0` when gain is zero
and loss is positive. Flat gain/loss, zero ATR, invalid ADX true range, or zero
volume baseline is unavailable and fails the complete snapshot.

EMA alignment uses current close, EMA(20), EMA(50), and ATR(20):

- `StrongBullish` / `Bullish` when `close > fast > slow`;
- `StrongBearish` / `Bearish` when `close < fast < slow`;
- `Mixed` otherwise;
- strong separation requires `abs(fast - slow) >= 0.5 * ATR`.

## 4. Key Location and Patterns

Key-location tolerance is `0.25 * ATR`.

- Donchian lower and an EMA(50) below the close are support candidates.
- Donchian upper and an EMA(50) above the close are resistance candidates.
- Distance uses the minimum distance from the level to the current close and
  the relevant current-candle extreme.
- If support and resistance are both within tolerance, the nearer one wins;
  an exact tie produces no key location.

Patterns are emitted only at a valid support or resistance location. The
controlled set contains exactly:

1. `BullishEngulfing`
2. `BearishEngulfing`
3. `BullishHarami`
4. `BearishHarami`
5. `BigBullish`
6. `BigBearish`
7. `Hammer`
8. `HangingMan`
9. `ShootingStar`
10. `InvertedHammer`
11. `Doji`

That order is also the deterministic conflict priority. Pattern names and
their controlled semantics remain observations; they do not carry trading
authority.

## 5. Snapshot and Event Hashes

Snapshot hashes use length-delimited canonical fields and SHA-256. They bind:

- feature version and instrument;
- a canonical input hash over both active candle windows and best-book values;
- both timeframe feature sets and candle boundaries;
- exact normalized decimal outputs;
- controlled alignment, key-location, and pattern values;
- best bid, ask, and spread.

Transport provenance is deliberately excluded, so canonical REST bootstrap,
WebSocket live input, and replay input produce the same snapshot hash when
their market facts are equal.

Event hashes bind the feature version, instrument, snapshot hash, and sorted
controlled event kinds. Emission time is excluded so restart recovery yields
the same hash for the same canonical event.

## 6. Eligibility/Event Prefilter

Controlled event kinds are:

- `StructureChanged`
- `KeyLocationReached`
- `VolatilityExpanded`
- `VolumeAnomaly`
- `BreakoutAttempt`
- `RetestEvent`
- `PositionReviewDue`
- `InvalidationRiskIncreased`

The default policy checks:

- snapshot TTL and complete data quality;
- system and instrument eligibility;
- active TradePlan/review state;
- latest quote turnover of at least `10000`;
- spread no wider than `50` basis points;
- information delta, duplicate event, and 15-minute cooldown;
- one concurrent LLM call;
- daily limits of 40 calls, 200,000 tokens, and USD `2.00`;
- a per-attempt reservation of 4,000 tokens and USD `0.05`.

Every rejection returns one or more stable reason codes. The prefilter does not
optimize for a target rejection percentage.

Deduplication storage is capped at 1,024 entries. Per-instrument cooldown state
is capped at three instruments. Capacity excess fails closed; no unbounded
buffer or cache is permitted.
