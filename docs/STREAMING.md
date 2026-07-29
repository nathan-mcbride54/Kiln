# Kiln provider streaming and cancellation

**Status:** Implemented vertical slice
**Roadmap:** H1-001
**Canonical provider entry point:** `ProviderService::stream_chat`

## Data path

```text
OpenAI / Anthropic / local compatible SSE
  → provider-specific parser
  → normalized ChatStreamEvent
  → ordered Tauri IPC Channel
  → ApplicationEvent batch
  → SQLite commit
  → Svelte projection
```

Provider frames never enter Svelte directly. Each adapter converts its protocol
to `message_delta`, `message_completed`, or `cancelled`. The desktop bridge
assigns task message and turn identity. Every delta and terminal batch uses the
existing durable-before-visible history coordinator.

Tauri channels are used instead of global events because they preserve order
and are designed for streamed data. See the official
[Tauri channel guide](https://v2.tauri.app/develop/calling-frontend/#channels).

## Provider protocols

OpenAI Responses uses `response.output_text.delta` and
`response.completed`. Anthropic Messages combines `message_start`,
`content_block_delta`, `message_delta`, and `message_stop`. Compatible local
servers use `choices[0].delta.content` plus a finish reason or `[DONE]`.

The SSE framer accepts LF or CRLF boundaries, preserves multi-line `data:`
fields, retains incomplete UTF-8 across HTTP chunks, and rejects invalid UTF-8
without exposing response bodies.

## Cancellation

One cloned cancellation domain spans the connection wait, active HTTP byte
stream, future process and tool-job futures, the Tauri turn registry, and the
desktop stop button. Cancellation wins a simultaneous stream race. The active
network or job future is dropped, one normalized cancellation event is sent,
and the registry entry is removed after forwarding ends.

## Late-event rule

`turn_receipt(cancelled)` is authoritative. Rust and TypeScript projections
still advance their sequence over later envelopes for audit continuity, but
ignore late message and tool mutations until another turn starts. The Tauri
channel consumer also stops accepting provider messages after its first
terminal batch.

## Validation

Fixture and concurrency tests prove normalized output for all three protocols,
split Unicode and CRLF framing, active-job cancellation, cancellation winning
over queued late chunks, Tauri turn cleanup, stream-to-application mapping, and
the cancelled-turn mutation barrier in both projectors.
