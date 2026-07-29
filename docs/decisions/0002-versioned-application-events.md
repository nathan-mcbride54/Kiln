# ADR 0002: Versioned application events as the interface boundary

- **Status:** Accepted
- **Date:** 2026-07-28
- **Roadmap:** H0-007

## Context

Kiln must support Svelte/Tauri today and CLI, headless, and remote transports
later. OpenAI, Anthropic, and compatible local servers expose different
payloads and lifecycle semantics. If the interface projects those responses
directly, provider changes leak into product state, replay becomes unreliable,
and every transport needs its own orchestration behavior.

## Decision

Kiln represents user-visible transitions as immutable, schema-versioned
application events.

- Events are strictly ordered within a named stream.
- Task events carry task identity plus causation and correlation identifiers.
- Explicit receipts mark terminal turn outcomes.
- Provider responses are normalized before the Svelte projection sees them.
- Live and replayed events use the same deterministic projector.
- Unknown additive fields are tolerated within a major version.
- Unsupported major versions and invalid ordering fail before projection.
- Raw provider payloads are diagnostic metadata, never the state model.

Rust in `kiln-core` is the canonical contract. The TypeScript mirror is checked
against deterministic replay fixtures until cross-language contract generation
lands.

## Consequences

Positive:

- Svelte is independent of provider-specific payloads.
- SQLite replay can rebuild the same state used during live execution.
- CLI and headless transports can share semantics with Tauri.
- Ordering failures become explicit rather than subtle UI corruption.
- Streaming extends the message lifecycle without replacing it.

Costs:

- Every new transition needs a contract and projection decision.
- Rust and TypeScript definitions are temporarily duplicated.
- Migrations and compatibility tests become mandatory once events are durable.
- Event payload redaction must be centralized before repository tools land.

## Rejected alternatives

### Store provider responses directly

This couples history and UI behavior to vendor schemas and makes cross-provider
replay unstable.

### Keep state only in Svelte stores

This cannot provide durable restart, audit, headless operation, or deterministic
rebuilds.

### Send unversioned Tauri events

This hides breaking changes until runtime and provides no migration boundary.
