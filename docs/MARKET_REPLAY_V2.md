# IronPilot Market Replay v2

`P3-12` migrates the deterministic no-trade replay evidence from the historical
v2 Strategy Space binding to v3 AI authority versions.

## Version bindings

- Manifest schema: `ironpilot-market-replay-v2`
- Report schema: `ironpilot-market-replay-report-v2`
- Feature engine: `ironpilot-market-features-v1`
- AI Decision Context schema: `ironpilot-ai-decision-context-v1`
- AITradingPlan schema: `3.0`
- Clock step: closed 15-minute candle boundaries
- Randomness: fixed v1 deterministic seed

The manifest and report hashes cover the Context and AITradingPlan version
bindings. Neither JSON artifact contains a Strategy Space, Materializer, or Risk
Engine version.

This replay still produces only reproducible Market Features and trigger facts.
It does not call an LLM or calculate trading performance. `P3-10A` will add the
recorded AI plan, validation, TradePlan, and Paper execution harness.
