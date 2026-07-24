# IronPilot

Constrained autonomous trading with deterministic risk and execution boundaries.

The current repository contains the Phase A domain and startup configuration
kernel. The executable validates configuration and exits without starting
trading behavior.

## Workspace

- `ironpilot-domain`: pure domain boundary.
- `ironpilot-application`: use-case orchestration boundary.
- `ironpilot-adapters`: external I/O and interface adapter boundary.
- `ironpilot`: composition root and process entry point.

The dependency direction is documented in
[`docs/DEPENDENCIES.md`](docs/DEPENDENCIES.md). A fail-closed YAML example is
available at [`config/ironpilot.example.yaml`](config/ironpilot.example.yaml).

## Validate startup configuration

PowerShell:

```powershell
$env:IRONPILOT_CONFIG_PATH="config/ironpilot.example.yaml"
$env:IRONPILOT_ENVIRONMENT="development"
$env:IRONPILOT_ENVIRONMENT_FINGERPRINT="development-paper-local"
cargo run --locked
```

The environment name and fingerprint must match the YAML document. Runtime
limits, enabled Spot instruments, execution permission and semantic versions
are validated before later tasks can initialize side effects.

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
