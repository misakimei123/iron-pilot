# Emergency Core v1

## Purpose

`ironpilot-emergency-core-v1` is the internal emergency execution boundary for
`CLOSE_ALL_MANAGED_EXPOSURE`. It accepts an already authenticated and explicitly
confirmed `AuthorizedEmergencyCommand`; Telegram, AI generation, and ordinary
strategy evaluation are not dependencies of the controller.

## Command contract

The command contains:

- a stable `EmergencyActionId`;
- the authorization subject;
- hashes of authorization evidence and the independent confirmation nonce;
- issue and expiry timestamps with a hard five-minute TTL;
- a canonical payload and SHA-256 command hash.

Raw credentials and confirmation nonces are not persisted. A new command is
accepted only inside its half-open validity window. Once the same command has
been persisted, recovery may continue after expiry. Reusing an action ID with a
different canonical payload fails closed.

## Execution and recovery

The controller requires the active runtime lease and persists this monotonic
sequence:

1. `REQUESTED`;
2. `ENTRY_DISABLED` after forcing `system_state` to `HALTED`;
3. `ORDERS_CANCELLED` after cancelling active paper orders that have a
   project-owned `paper_order_specs` row;
4. `EXPOSURE_REDUCING` while managed lots remain;
5. `COMPLETED` after all provable managed lots are closed.

The controller never restores entry permissions. It only reduces quantities in
`managed_lots`; assets without managed-lot provenance are outside the sell set.
Every step and emergency fill is append-only. Replaying the same command and
market observation produces no additional fill or managed-lot mutation.

Paper emergency pricing reuses `PaperMatchingEngine`,
`PaperExecutionPolicy`, exact `DomainDecimal`, Tokio synchronization, SQLx
transactions and SQLite migrations already selected by the project. P3-08
introduces no external protocol client and no replacement infrastructure. A
future venue adapter must use that venue's mature SDK under the repository-wide
SDK reuse rule.

## Bounded market input

Each call accepts at most three observations, one per instrument. Observations
must be newer than the command decision fact, no more than ten seconds old, and
not from the future. Missing or unusable observations leave the action in
`EXPOSURE_REDUCING`; they never trigger a guessed or blind replacement order.
