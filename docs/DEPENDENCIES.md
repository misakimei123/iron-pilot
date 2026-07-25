# Dependency Governance

This document records dependencies introduced by implementation tasks. It does
not authorize future dependencies or redefine task scope.

## Cargo dependencies

Direct dependencies are owned by the task and crate that require them. Default
features are disabled; only the listed features are enabled.

| Dependency | Pin | Scope / owner | Purpose | License | Maintenance status | Features / resource impact | Exit plan |
|---|---|---|---|---|---|---|---|
| `rust_decimal` | `1.42.1` | Runtime / `ironpilot-domain` | Exact decimal values for all domain amounts | MIT | Maintained upstream | `std`; bounded CPU and 16-byte value representation | Replace with an audited fixed-scale integer type while preserving string wire encoding |
| `serde` | `1.0.229` | Runtime / `ironpilot-domain`, `ironpilot-application`, `ironpilot-adapters` | Closed domain/configuration contracts and private Bybit DTO decoding | Apache-2.0 / MIT | Maintained upstream | `derive`, `std`; compile-time derive cost, no I/O or permissions | Replace derives with reviewed manual codecs if the wire layer changes |
| `uuid` | `1.24.0` | Runtime / `ironpilot-domain` | Typed stable identifiers | Apache-2.0 / MIT | Maintained upstream | `serde`, `std`; generation features are disabled, no randomness or I/O | Replace with an internal 16-byte identifier while preserving canonical UUID text |
| `noyalib` | `0.0.16` | Runtime / `ironpilot-adapters` | Strict YAML startup configuration parsing | Apache-2.0 / MIT | Maintained upstream; pre-1.0 API is version-pinned | `minimal` only; pure Rust, configuration input capped at 64 KiB | Replace with another reviewed Serde YAML decoder or a schema-specific parser |
| `serde_json` | `1.0.151` | Runtime / `ironpilot-application`, `ironpilot-adapters`; Development / `ironpilot-domain` | Structured audit/outbox payloads and bounded Bybit JSON envelope decoding | Apache-2.0 / MIT | Maintained upstream | `std`; adapter response bodies are capped at 256 KiB before decoding | Replace runtime values with versioned schema-specific codecs |
| `reqwest` | `0.12.28` | Runtime / `ironpilot-adapters` | Thin Bybit V5 public REST transport | Apache-2.0 / MIT | Maintained upstream | `rustls-tls-webpki-roots`; no redirects, 5 s connect timeout, 10 s request timeout, 256 KiB response cap; no credentials or private endpoints | Replace behind the adapter boundary while preserving DTO, timeout and error-classification contracts |
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
version-pinned exceptions for reviewed proc-macro, test, YAML and SQLx/SQLite
transitive feature sets. SQLx's temporary `hashbrown` and `syn` duplicate
versions are also pinned with removal reasons; dependency or feature drift
fails the supply-chain gate.

### P2-01 Bybit client choice

The P2-01 review found no official Bybit Rust SDK in the official V5 examples
or the Bybit SDK organization listing. The maintained official examples cover
Python, Java, Go, and Node.js. A third-party trading SDK would add private-order
surface and domain types that P2-01 does not need, so the implementation uses a
thin `reqwest` adapter limited to `GET /v5/market/time` and
`GET /v5/market/instruments-info`. Bybit wire DTOs remain private to
`ironpilot-adapters`.

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
