# ADR 0005: Ordered channels and shared cancellation for provider streams

- **Status:** Accepted
- **Date:** 2026-07-28
- **Roadmap:** H1-001

## Context

Kiln needs responsive output from three provider protocols without letting
protocol frames become interface state. Cancellation must stop active network
and tool work, not only hide a spinner. A cancellation race must not allow late
bytes to rewrite terminal task state.

## Decision

- Normalize provider frames to `ChatStreamEvent` in `kiln-providers`.
- Use one shared `kiln-platform` cancellation token for HTTP and job futures.
- Deliver normalized events over a typed, ordered Tauri IPC channel.
- Convert channel messages to application events at the desktop bridge.
- Persist each application-event batch before projecting it.
- Treat a cancelled receipt as a terminal mutation barrier in both projectors.
- Keep the non-streaming path temporarily for diagnostics and compatibility.

## Consequences

Positive:

- Provider protocols remain outside interface and storage contracts.
- Streaming, persistence, and restart replay share one projector.
- Cancellation tears down real work and has deterministic race behavior.
- Tauri channels provide ordered, scoped delivery without a global event bus.

Costs:

- Each protocol needs maintained stream fixtures.
- Persisting each delta creates write pressure; later batching must preserve
  event order.
- Tool executors must adopt the shared cancellation domain.
- Incomplete streams require an explicit error receipt.

## Rejected alternatives

Global Tauri events are less strongly typed and are not the recommended
high-throughput ordered mechanism. Projecting provider frames in Svelte couples
product state to upstream protocols. UI-only cancellation leaves side effects
active and permits late mutation races.
