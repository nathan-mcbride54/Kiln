# ADR 0011: Make the Rust task loop durable before side effects

**Status:** Accepted
**Date:** 2026-07-29

## Context

Kiln already had provider codecs, SQLite events, workspace policy, native
approval, and cancellation, but the live desktop assembled their lifecycle.
That split let interface code mint causality, request approvals, invoke tools,
and decide when a turn was complete.

Provider-driven work needs one reusable owner for those transitions before
live HTTP continuations are added.

## Decision

Use a Tauri-free `kiln-orchestrator` crate for provider task execution. The
runner mints internal application identities, appends causal events before
each externally visible or side-effecting transition, preflights and executes
through `WorkspaceToolService`, waits on an injected approval gate, shares one
cancellation token, and emits exactly one terminal receipt.

Provider sessions receive only transient typed continuations. Approval
interfaces receive only bounded prompts. Neither provider handles nor approval
UI details become application event identities.

## Consequences

- Desktop and future headless transports share the same task semantics.
- Approval denial and cancellation can be proven to have no write side effect.
- SQLite failure stops the loop instead of exposing unrecorded progress.
- Provider and repository loop budgets are explicit.
- The live desktop remains on its legacy path until a provider HTTP session
  and thin command adapter are connected to the runner.
- H2 recovery must resume from durable receipts and incomplete streams rather
  than serializing provider-native session state into application events.
