# Bybit Testnet Protocol Smoke v1

## Scope

`P4-02A` is a bounded protocol and state-synchronization smoke for Bybit
Testnet Spot. It does not authorize Mainnet, real funds, derivatives,
qualification, release, or any stage Gate.

The executable is intentionally fixed to:

- the SDK-owned Bybit Testnet REST and private WebSocket endpoints;
- `BTCUSDT` and `Category::Spot`;
- non-leveraged orders with `marketUnit=baseCoin`;
- an `ip4-` project ownership prefix and a maximum 36-character
  `orderLinkId`;
- at most 10 USDT reference notional for every new order.

The run requires all three secret environment variables. The write
authorization value must exactly equal
`P4-02A:BYBIT-TESTNET:SPOT:WRITE`. Missing or different authorization fails
before the SDK client is used.

## Protocol sequence

The runner performs one bounded sequence:

1. query server time, API-key permissions, Testnet wallet, instrument rules,
   and ticker;
2. authenticate and subscribe to the SDK private order, execution, and wallet
   topics;
3. submit one below-book limit buy, persist its REST acknowledgement, query it,
   prove a repeated local submission is `DUPLICATE_NO_EFFECT`, and cancel it;
4. submit one market buy whose reference notional is above the exchange
   minimum but no greater than 10 USDT;
5. derive the managed quantity only from the private execution fact;
6. accept a bounded `AuthorizedEmergencyCommand`, cancel only open `ip4-`
   orders, and market-sell only the managed smoke quantity;
7. disconnect, reopen the same SQLite database, fetch an SDK-typed REST
   snapshot, and require reconciliation with no open project-owned order.

REST acknowledgement is persisted separately from private order and execution
facts. It never creates a fill. The append-only intent table binds each
`orderLinkId` to both the source `PlannedSpotOrder` payload hash and serialized
SDK request hash. Reuse with different fields fails closed; identical reuse
does not issue another exchange request.

## Field fidelity and Emergency ownership

The production mapping accepts an immutable `PlannedSpotOrder` and copies
side, type, quantity, limit price, time in force, and project ID into the SDK
request. It adds only venue envelope fields fixed by this smoke: Spot category,
symbol, no leverage, and base-coin quantity units. It does not round, resize,
or reprice an accepted order.

Dynamic minimum-size probe construction is isolated inside the protocol-smoke
runner and is not a production trading decision path. Normal runtime orders
must still originate in an accepted `AITradingPlan`. The mapping contract test
proves exact field preservation; the online evidence stores the exact source
and SDK request hashes.

Emergency never liquidates the account wallet. The exit quantity is the sum of
the smoke buy's private execution quantities, less a base-asset fee when
applicable, rounded down only to the exchange base precision. Pre-existing BTC
is outside the managed quantity and cannot be sold by this runner.

## Credentials and operation

On Windows, create the current-user DPAPI file without echoing secrets:

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\set-bybit-testnet-credential.ps1
```

Run the smoke:

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\run-bybit-testnet-smoke.ps1
```

The wrapper imports the DPAPI file only in its process, clears the environment
variables in `finally`, and accepts a configured Windows proxy only when its
endpoint is loopback. Secrets, signatures, and authorization nonces are never
written to the repository or evidence database.

REST uses the loopback HTTP proxy selected by the operator. Private WebSocket
transport uses the mature `tokio-socks 0.5.3` crate so DNS resolution occurs
inside the same loopback SOCKS5 route while TLS still validates the original
official Bybit host. The audited Bybit SDK overlay also treats the JSON
`retCode` envelope as authoritative when an intermediary strips the optional
response header, and accepts the documented Spot empty/missing fields in
current private order and execution messages.

If local DNS or routing blocks Bybit, stop before writes. Do not replace the
official endpoint, disable TLS verification, add a Mainnet fallback, or use a
browser/UI trade as evidence for this protocol smoke. If any step fails after a
write, the runner cancels only open `ip4-` orders and derives any recovery sell
strictly from REST executions belonging to `ip4-` order IDs. It never sells
pre-existing wallet BTC.

## Deterministic gates

The repository gates cover:

- exact domain-to-SDK field mapping;
- rejection of non-`BTCUSDT` symbols and foreign order IDs;
- an independent 10 USDT cap;
- current Testnet Spot order/execution DTO compatibility and REST envelope
  behavior behind a proxy;
- append-only migrations for intent and smoke evidence;
- strict Clippy, full workspace tests, dependency policy, secret scanning, and
  formatting.

Online evidence is required before `P4-02A` may be marked `DONE`. A local
network timeout or missing private event is a failed run, not qualifying
evidence. Only the user or an authorized reviewer may approve a later stage
Gate.
