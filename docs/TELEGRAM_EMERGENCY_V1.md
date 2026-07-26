# Telegram Emergency Adapter v1

## Boundary

`ironpilot-telegram-emergency-v1` is a protected authorization adapter for
`CLOSE_ALL_MANAGED_EXPOSURE`. It uses the existing `teloxide-core 0.13.0`
`Bot`, `Requester::get_updates`, `Requester::send_message`, `Update`,
`Message`, and `User` contracts. IronPilot does not define Telegram wire DTOs,
response envelopes, Bot API paths, polling retries, or protocol decoding.

The adapter does not cancel an order, sell an asset, or call AI. A successful
confirmation produces the same `AuthorizedEmergencyCommand` consumed by the
Emergency Core. The caller must hand that command to the core before committing
the Telegram update cursor. Actual progress and completion are communicated
from committed audit events.

## Authorization flow

1. The Telegram chat must be in the existing chat allowlist.
2. The SDK-decoded Telegram user ID must be in the emergency operator allowlist.
3. `/emergency_close_all` creates a random UUID v4 nonce and a bounded pending
   challenge for that exact `(chat_id, user_id)` pair.
4. The adapter sends the nonce using SDK `sendMessage` with
   `protect_content=true`.
5. The same chat and user must send
   `/confirm_emergency_close_all <nonce>` before the 10–120 second confirmation
   TTL expires.
6. The first confirmation attempt consumes the challenge. A bad nonce,
   malformed command, missing SDK user identity, expired challenge, replay, or
   unauthorized user produces no `AuthorizedEmergencyCommand`.

Only the SHA-256 nonce hash enters the command. The raw nonce is not persisted
or logged. Authorization evidence binds the adapter version, chat ID, user ID,
Telegram update ID, emergency action ID, and issue time.

Pending challenges are intentionally process-local and bounded to 16. A service
restart invalidates every unconfirmed challenge; the operator must request a
new nonce. This fail-closed behavior prevents a deployment or restart from
silently confirming an old emergency request.

## Command and cursor semantics

The resulting command uses a separately bounded 10–300 second TTL and a stable
action ID. Telegram confirmation has zero direct trading-table effect. Replayed
or already-consumed confirmations produce no command. The poll result exposes
the next SDK update offset to the caller, but the caller owns persistence of
that offset and must not advance it before handing every returned command to
the idempotent Emergency Core.
