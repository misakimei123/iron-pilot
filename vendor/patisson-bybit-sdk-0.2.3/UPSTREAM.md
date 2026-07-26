# Upstream provenance

- Crate: `patisson-bybit-sdk 0.2.3`
- License: MIT
- Repository: <https://github.com/yurii-musolov/patisson-bybit-sdk>
- Published VCS revision: `8a88b11ba9ee8af33468acb5b501c1c01f33dab3`
- Crate archive SHA-256:
  `a83e09b48a6d20d0703663c7a591e4a5eab8057aa1b7c26b94b97aa97d05e57e`

IronPilot retains the published `Cargo.toml.orig` for review and applies two
bounded changes:

1. `Cargo.toml` is a dependency-only overlay that disables unused default
   features, selects Rustls, and aligns Tokio/Tokio-Tungstenite with the
   versions already audited by the workspace.
2. `src/ws/stream.rs` awaits the bounded event sender instead of using
   `try_send`, so a full private-event queue applies backpressure rather than
   silently dropping exchange facts.

All other files under `src/` are byte-for-byte identical to the published
crate.
