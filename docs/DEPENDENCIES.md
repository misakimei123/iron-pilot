# Dependency Governance

This document records dependencies introduced by implementation tasks. It does
not authorize future dependencies or redefine task scope.

## Cargo dependencies

Direct dependencies are owned by the task and crate that require them. Default
features are disabled; only the listed features are enabled.

| Dependency | Pin | Scope / owner | Purpose | License | Maintenance status | Features / resource impact | Exit plan |
|---|---|---|---|---|---|---|---|
| `rust_decimal` | `1.42.1` | Runtime / `ironpilot-domain`, `ironpilot-application`, `ironpilot-adapters` | Exact decimal values for all domain amounts and the mature historical-metrics adapter | MIT | Maintained upstream | Direct crates request `std`; `quant-metrics` also activates its upstream `serde`/default set; bounded CPU and 16-byte value representation | Replace with an audited fixed-scale integer type while preserving string wire encoding and historical metric equivalence |
| `serde` | `1.0.229` | Runtime / `ironpilot-domain`, `ironpilot-application`, `ironpilot-adapters` | Closed domain/configuration contracts and private Bybit DTO decoding | Apache-2.0 / MIT | Maintained upstream | `derive`, `std`; compile-time derive cost, no I/O or permissions | Replace derives with reviewed manual codecs if the wire layer changes |
| `uuid` | `1.24.0` | Runtime / `ironpilot-domain`, `ironpilot-adapters` | Typed stable identifiers and unpredictable in-memory Telegram Emergency challenge nonces | Apache-2.0 / MIT | Maintained upstream | Domain enables `serde`, `std`; adapters enable `std`, `v4`; randomness is used only for bounded Emergency challenge creation | Replace with an audited identifier and CSPRNG boundary while preserving canonical UUID text and one-time challenge semantics |
| `noyalib` | `0.0.16` | Runtime / `ironpilot-adapters` | Strict YAML startup configuration parsing | Apache-2.0 / MIT | Maintained upstream; pre-1.0 API is version-pinned | `minimal` only; pure Rust, configuration input capped at 64 KiB | Replace with another reviewed Serde YAML decoder or a schema-specific parser |
| `serde_json` | `1.0.151` | Runtime / `ironpilot-application`, `ironpilot-adapters`; Development / `ironpilot-domain` | Structured audit/outbox payloads, bounded provider/Bybit JSON decoding, and exact raw JSON evidence | Apache-2.0 / MIT | Maintained upstream | `raw_value`, `std`; adapter response bodies are bounded before decoding | Replace runtime values with versioned schema-specific codecs |
| `reqwest` | `0.13.4` | Runtime / `ironpilot-adapters` | Bybit V5 public REST transport and the HTTP transport supplied to `async-openai` | Apache-2.0 / MIT | Maintained upstream | `rustls`; redirects disabled; bounded connect/request timeouts and adapter-owned 128/256 KiB response caps | Replace behind adapter/SDK boundaries while preserving DTO, timeout, evidence and error-classification contracts |
| `reqwest` (`teloxide-reqwest`) | `0.12.28` | Runtime / `ironpilot-adapters` | Version-compatible HTTP client supplied to `teloxide-core` so IronPilot can enforce timeout and redirect policy without implementing Telegram protocol logic | Apache-2.0 / MIT | Maintained upstream; version line is selected by `teloxide-core 0.13.0` and locked | Defaults disabled; TLS is owned by `teloxide-core`'s `rustls` feature; adapter configures bounded timeouts and no redirects | Remove the direct alias when the Telegram SDK exposes equivalent client-policy configuration without it |
| `async-openai` | `0.41.1` | Runtime / `ironpilot-adapters` | Mature OpenAI-compatible client for the DeepSeek chat-completion protocol | MIT | Maintained upstream; version and feature set locked | `byot`, `chat-completion`, `middleware`, `rustls`; defaults disabled; BYOT carries DeepSeek fields; one bounded Tower service disables hidden retries and captures at most 128 KiB | Replace with another maintained OpenAI-compatible client while preserving the Prompt, raw evidence, budget and strict-plan contracts |
| `teloxide-core` | `0.13.0` | Runtime / `ironpilot-adapters` | Mature Telegram Rust SDK for Bot API methods, request/response types, response envelope decoding and long-poll update models | MIT | Maintained upstream in the widely adopted Teloxide ecosystem; version and feature set locked | Defaults disabled; `rustls` only; one bounded `getUpdates` call at a time, no adapter retry loop, batches capped by the Telegram policy | Replace with another maintained Telegram SDK only if it preserves SDK-owned protocol behavior, identity evidence and bounded polling |
| `quant-metrics` | `0.7.0` | Runtime / `ironpilot-application` offline evaluation | Mature pure-math Rust library for exact-decimal total return, maximum drawdown and trade expectancy | MIT | Maintained upstream in the Quant Core workspace; version locked | No I/O, async, model or execution surface; upstream has no feature flags and brings exact-reviewed Chrono/Serde/quant-indicators defaults, pinned in `deny.toml`; evaluation records capped at 100,000 | Replace only with a maintained exact-decimal metrics library and retain independent reference tie-out vectors |
| `http` | `1.4.2` | Runtime / `ironpilot-adapters` | Rebuild the already bounded provider response passed from IronPilot's evidence middleware back into `async-openai` | Apache-2.0 / MIT | Maintained upstream with the Hyper ecosystem | No direct features; response metadata/body only, no network access | Remove when the provider SDK exposes a public bounded raw-response hook |
| `tower-service` | `0.3.3` | Runtime / `ironpilot-adapters` | Implement the minimal SDK transport service boundary without importing Tower utility layers | MIT | Stable, maintained ecosystem interface | No features; one cloneable service with no queue or retry layer | Remove when the provider SDK exposes an equivalent bounded raw-response hook |
| `futures-util` | `0.3.33` | Runtime / `ironpilot-adapters` | Minimal async stream/sink extensions for the public WebSocket transport | Apache-2.0 / MIT | Maintained upstream | `sink`, `std`; no executor or unbounded channel features | Replace with direct poll adapters if the WebSocket transport changes |
| `sha2` | `0.10.9` | Runtime / `ironpilot-domain`, `ironpilot-adapters` | Stable SHA-256 hashes for versioned market snapshots/events and dynamic Spot instrument rules | Apache-2.0 / MIT | Maintained upstream | No default features; hashes only bounded canonical feature/event inputs and the bounded 1–3 instrument rule set | Replace with a reviewed digest while publishing new hash schema versions |
| `sqlx` | `0.9.0` | Runtime / `ironpilot-adapters` | SQLite pool, transactions, embedded migrations and typed error boundary | Apache-2.0 / MIT | Maintained upstream | `macros`, `migrate`, `runtime-tokio`, `sqlite-bundled`; pool capped at 4 connections and writes serialized | Replace with a reviewed SQLite adapter while preserving migrations, transaction and repository contracts |
| `tokio` | `1.53.1` | Runtime and Development / `ironpilot-application`, `ironpilot-adapters` | Bounded task supervision, cancellation, channels, network I/O, shutdown deadlines, SQLx synchronization and async tests | MIT | Maintained upstream | Runtime `macros`, `net`, `rt`, `sync`, `time`; tests add `rt-multi-thread`; tasks and channels are bounded by P1-05/P2-02 contracts | Replace with another reviewed runtime while preserving bounded queues, cancellation and forced-shutdown behavior |
| `tokio-tungstenite` | `0.30.0` | Runtime / `ironpilot-adapters` | Bybit Spot public WebSocket transport | MIT | Maintained upstream | `connect`, `rustls-tls-webpki-roots`; one connection, at most 9 topics, 256 KiB messages, 64 KiB frames/write buffer | Replace behind the market-stream adapter while preserving subscription, recovery and bounded-buffer contracts |
| `sysinfo` | `0.39.6` | Runtime / `ironpilot-adapters` | Targeted current-process RSS and CPU sampling for runtime health | MIT | Maintained upstream | `system` only; refreshes the current process rather than the full system; no write or network permissions | Replace with reviewed platform-native process metrics behind the same resource-sample contract |
| `proptest` | `1.11.0` | Development / `ironpilot-domain` | Property tests for state-machine fail-closed behavior | Apache-2.0 / MIT | Maintained upstream | `std`; test-only CPU and memory, no production binary impact | Replace with exhaustive transition tables plus deterministic fuzz tests |

The lockfile pins the resolved transitive graph. Any future direct Cargo
dependency must be added only by the task that needs it and must record:

- the concrete requirement and owning module;
- license and source;
- maintenance and security status;
- disabled default features and the exact enabled feature set;
- resource impact;
- replacement or removal plan.

The global default-feature ban remains enabled. `deny.toml` contains exact,
version-pinned exceptions for reviewed proc-macro, test, YAML, provider SDK,
TLS and SQLx/SQLite transitive feature sets. Temporary duplicate versions from
the SDK, WebSocket and SQLx graphs are pinned with removal reasons; dependency
or feature drift fails the supply-chain gate.

### P2-01 Bybit client choice

The P2-01 review found no official Bybit Rust SDK in the official V5 examples
or the Bybit SDK organization listing. The maintained official examples cover
Python, Java, Go, and Node.js. A third-party trading SDK would add private-order
surface and domain types that P2-01 does not need, so the implementation uses a
thin `reqwest` adapter limited to `GET /v5/market/time` and
`GET /v5/market/instruments-info`. Bybit wire DTOs remain private to
`ironpilot-adapters`.

### P3-04 DeepSeek client choice

The P3-04 refactor compared a direct HTTP implementation with `async-openai`,
`genai`, and the broader Rig framework. `async-openai 0.41.1` was selected
because it provides the focused OpenAI-compatible chat client, custom base URL,
BYOT provider extensions and a public Tower transport boundary without adding
agent, tool, RAG, workflow, or multi-provider orchestration semantics.

The SDK now owns authentication, endpoint construction, standard request
serialization, HTTP execution contract and response decoding. IronPilot owns
only its trading-domain Prompt/types, strict `AITradingPlan` parsing, usage and
cost accounting, explicit once-per-Context replan, and a small evidence
middleware required by project invariants. The middleware streams and caps the
body before deserialization, records the exact response, and replaces the SDK's
default retry executor. Therefore one IronPilot provider attempt is one HTTP
request; no SDK retry can escape the call/token/cost evidence boundary.

### P3-07 Telegram client choice

P3-07 uses `teloxide-core 0.13.0` for Bot API method construction,
Telegram request and response types, response envelope decoding, `getUpdates`
long-poll models, and `sendMessage`. IronPilot owns only the bounded domain
adapter: chat/user allowlists, read-only command routing, Emergency
challenge/confirmation policy, SQLite read models, notification text, offset
handoff, and audit boundaries.

The adapter supplies a version-compatible `reqwest 0.12` client solely to
enforce bounded connect/request timeouts and disabled redirects. It does not
construct Telegram endpoints, wire DTOs, response envelopes, protocol retry
loops, or error decoders. Polling remains one SDK request per adapter call; the
caller persists the returned offset only after processing any confirmed
Emergency command.

### P3-10B historical metrics choice

P3-10B reviewed focused statistics crates and complete backtesting frameworks.
Barter is the most established full Rust trading/backtesting framework found,
but adopting its Strategy, RiskManager, portfolio and execution engine would
duplicate IronPilot's frozen AI authority, Validator and shared Paper matcher.
`bts-rs` likewise owns strategy generation, order/position simulation and
optimization and uses floating-point market values. `trametricks` is a small
`f64` statistics crate and cannot satisfy the exact-decimal invariant.

`quant-metrics 0.7.0` was selected for the narrow standard-math surface it
already implements with `rust_decimal`: total return, maximum drawdown and
expectancy. IronPilot retains only project-specific orchestration and evidence:
the three comparable arms, immutable Context/Prompt/Model/Plan/Validator/
Execution bindings, user authorization, OOS split, stress inputs, rejection
taxonomy, per-trade provenance, safety-failure precedence and independent
reference tie-out.

The crate is pure math with no I/O or async surface. Its upstream manifest does
not expose feature switches and currently brings Chrono, quant-primitives and
quant-indicators defaults even though P3-10B calls only three functions. Those
features are version-pinned and exhaustively listed in `deny.toml`; any
transitive feature expansion fails the supply-chain gate.

## Workspace boundaries

| Crate | Purpose | Allowed inward dependencies |
|---|---|---|
| `ironpilot-domain` | Pure domain contracts | None |
| `ironpilot-application` | Use-case orchestration | `ironpilot-domain` |
| `ironpilot-adapters` | External I/O and interface adapters | `ironpilot-application`, `ironpilot-domain` |
| `ironpilot` | Composition root and process lifecycle | All workspace crates |

Version-pinned path dependencies now enforce the implemented direction:
`ironpilot` → `ironpilot-adapters` → `ironpilot-application` →
`ironpilot-domain`.

## Tooling dependencies

| Tool | Pin | Purpose | License | Maintenance status | Features / permissions | Exit plan |
|---|---|---|---|---|---|---|
| Rust | `1.97.1` | Build, test, format and lint | Apache-2.0 / MIT | Maintained by the Rust project | Minimal rustup profile plus `rustfmt` and `clippy` | Change the single `rust-toolchain.toml` pin after CI verification |
| `actions/checkout` | `v7` commit `3d3c42e5aac5ba805825da76410c181273ba90b1` | Checkout in CI | MIT | Maintained by GitHub | `contents: read`; credentials are not persisted | Replace with a reviewed newer commit or explicit Git commands |
| `cargo-deny` | `0.19.4` | License, advisory, duplicate, feature and source policy | Apache-2.0 / MIT | Maintained; version pinned | CI-only; no application features or credentials | Replace with `cargo-audit` plus an equivalent license/source policy |
| Gitleaks | `8.30.1` commit `83d9cd684c87d95d656c1458ef04895a7f1cbd8e` | Repository secret scan | MIT | Security-fix maintenance; feature-frozen upstream | CI-only; scans the checked-out Git history with redacted output | Replace with another reviewed history-aware secret scanner |

Tool versions and checksums are pinned in CI. Tooling is not linked into the
application and has no runtime credentials or trading permissions.
