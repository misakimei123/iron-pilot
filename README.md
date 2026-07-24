# IronPilot

Constrained autonomous trading with deterministic risk and execution boundaries.

The current repository contains the `P1-01` Rust skeleton only. The executable
intentionally performs no work; domain models and trading behavior begin in
their explicitly dependent tasks.

## Workspace

- `ironpilot-domain`: pure domain boundary.
- `ironpilot-application`: use-case orchestration boundary.
- `ironpilot-adapters`: external I/O and interface adapter boundary.
- `ironpilot`: composition root and process entry point.

The dependency direction is documented in
[`docs/DEPENDENCIES.md`](docs/DEPENDENCIES.md). The example configuration is
intentionally empty because `P1-03` owns its schema.

## Local quality gates

Rust `1.97.1`, `rustfmt`, and Clippy are pinned by `rust-toolchain.toml`.

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --all-targets --locked
cargo build --workspace --all-targets --locked
cargo metadata --locked --no-deps --format-version 1
cargo deny check
gitleaks git --redact --no-banner
```

CI runs the same build, test, static-analysis, dependency-policy, advisory, and
secret-history checks.
