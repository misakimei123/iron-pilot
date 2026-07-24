# Dependency Governance

This document records dependencies introduced by `P1-01`. It does not authorize
future dependencies or redefine task scope.

## Cargo dependencies

Direct dependencies are owned by the task and crate that require them. Default
features are disabled; only the listed features are enabled.

| Dependency | Pin | Scope / owner | Purpose | License | Maintenance status | Features / resource impact | Exit plan |
|---|---|---|---|---|---|---|---|
| `rust_decimal` | `1.42.1` | Runtime / `ironpilot-domain` | Exact decimal values for all domain amounts | MIT | Maintained upstream | `std`; bounded CPU and 16-byte value representation | Replace with an audited fixed-scale integer type while preserving string wire encoding |
| `serde` | `1.0.229` | Runtime / `ironpilot-domain` | Closed, explicit domain wire contracts | Apache-2.0 / MIT | Maintained upstream | `derive`, `std`; compile-time derive cost, no I/O or permissions | Replace derives with reviewed manual codecs if the wire layer changes |
| `uuid` | `1.24.0` | Runtime / `ironpilot-domain` | Typed stable identifiers | Apache-2.0 / MIT | Maintained upstream | `serde`, `std`; generation features are disabled, no randomness or I/O | Replace with an internal 16-byte identifier while preserving canonical UUID text |
| `proptest` | `1.11.0` | Development / `ironpilot-domain` | Property tests for state-machine fail-closed behavior | Apache-2.0 / MIT | Maintained upstream | `std`; test-only CPU and memory, no production binary impact | Replace with exhaustive transition tables plus deterministic fuzz tests |
| `serde_json` | `1.0.151` | Development / `ironpilot-domain` | JSON contract and unknown-field rejection tests | Apache-2.0 / MIT | Maintained upstream | `std`; test-only allocation, no production binary impact | Replace with `serde_test` or reviewed fixture decoding |

The lockfile pins the resolved transitive graph. Any future direct Cargo
dependency must be added only by the task that needs it and must record:

- the concrete requirement and owning module;
- license and source;
- maintenance and security status;
- disabled default features and the exact enabled feature set;
- resource impact;
- replacement or removal plan.

The global default-feature ban remains enabled. `deny.toml` contains exact,
version-pinned exceptions only for `serde_derive`'s proc-macro default feature
and the test-only Unicode parser feature set pulled in by `proptest`.

## Workspace boundaries

| Crate | Purpose | Allowed inward dependencies |
|---|---|---|
| `ironpilot-domain` | Pure domain contracts | None |
| `ironpilot-application` | Use-case orchestration | `ironpilot-domain` |
| `ironpilot-adapters` | External I/O and interface adapters | `ironpilot-application`, `ironpilot-domain` |
| `ironpilot` | Composition root and process lifecycle | All workspace crates |

P1-01 creates the compile boundaries only. It does not add path dependencies
until a real contract is implemented and consumed.

## Tooling dependencies

| Tool | Pin | Purpose | License | Maintenance status | Features / permissions | Exit plan |
|---|---|---|---|---|---|---|
| Rust | `1.97.1` | Build, test, format and lint | Apache-2.0 / MIT | Maintained by the Rust project | Minimal rustup profile plus `rustfmt` and `clippy` | Change the single `rust-toolchain.toml` pin after CI verification |
| `actions/checkout` | `v7` commit `3d3c42e5aac5ba805825da76410c181273ba90b1` | Checkout in CI | MIT | Maintained by GitHub | `contents: read`; credentials are not persisted | Replace with a reviewed newer commit or explicit Git commands |
| `cargo-deny` | `0.19.4` | License, advisory, duplicate, feature and source policy | Apache-2.0 / MIT | Maintained; version pinned | CI-only; no application features or credentials | Replace with `cargo-audit` plus an equivalent license/source policy |
| Gitleaks | `8.30.1` commit `83d9cd684c87d95d656c1458ef04895a7f1cbd8e` | Repository secret scan | MIT | Security-fix maintenance; feature-frozen upstream | CI-only; scans the checked-out Git history with redacted output | Replace with another reviewed history-aware secret scanner |

Tool versions and checksums are pinned in CI. Tooling is not linked into the
application and has no runtime credentials or trading permissions.
