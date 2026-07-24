# Dependency Governance

This document records dependencies introduced by `P1-01`. It does not authorize
future dependencies or redefine task scope.

## Runtime dependencies

The Cargo workspace has no third-party runtime, build, or development
dependencies. All four workspace crates currently use only the Rust standard
library and contain no optional or default Cargo features.

Any future direct Cargo dependency must be added only by the task that needs it
and must record:

- the concrete requirement and owning module;
- license and source;
- maintenance and security status;
- disabled default features and the exact enabled feature set;
- resource impact;
- replacement or removal plan.

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
