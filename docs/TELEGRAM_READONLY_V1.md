# Telegram Read-only Adapter v1

## Scope

`ironpilot-telegram-readonly-v1` exposes confirmed IronPilot facts through the Telegram Bot API:

- append-only audit events;
- runtime status;
- AI plan summaries and the recorded raw AI plan;
- Execution Validation outcomes, authorization amounts, evidence and rejection reasons;
- managed positions;
- Paper orders and fills;
- the latest Decision Context user maximum-loss authorization.

The supported commands are `/status`, `/events`, `/plans`, `/plan`, `/validations`,
`/validation`, `/positions`, `/orders`, `/trades`, `/authorization` and `/help`. Query row
counts, update batches, notification batches, response bodies and Telegram text are all bounded.

This adapter has no strategy or emergency command type and performs no trading write. Pause,
resume, cancel and emergency commands are deliberately unsupported. Emergency authorization and
execution belong to P3-08 and P3-07B.

## Protocol and secrets

The adapter uses the official HTTP Bot API `getUpdates` long-poll contract and advances the caller
cursor to the highest processed `update_id + 1`. It uses `sendMessage` with plain text and
`protect_content=true`; messages are capped at Telegram's 4,096-character limit.

The bot token is loaded from `IRONPILOT_TELEGRAM_BOT_TOKEN` or passed directly to the secret-bearing
constructor. It is never accepted in YAML, included in response text or retained in errors.
Production traffic is fixed to the HTTPS `api.telegram.org` origin, redirects are disabled, and
only bounded bodies are decoded.

Official protocol references:

- <https://core.telegram.org/bots/api#getupdates>
- <https://core.telegram.org/bots/api#sendmessage>

## Authorization and delivery semantics

Every inbound command and outbound notification is restricted to a bounded configured chat
allowlist. Non-message updates, ordinary text and commands from other chats receive no reply.

Notifications are generated only from committed rows in the append-only `audit_log`. The caller
supplies the last completed audit sequence and advances it only after the batch succeeds. Telegram
does not provide an application idempotency key for `sendMessage`, so a crash after remote
acceptance but before cursor persistence can repeat a notification; this is explicit at-least-once
delivery, never a network exactly-once claim. Repeated queries and notifications have zero trading
business effect.

P3-07A does not authorize Testnet or live exchange writes and does not approve any phase Gate.
