# ADR 0001: Local-first Rust control plane with a Svelte desktop client

- **Status:** Accepted
- **Date:** 2026-07-28

## Context

Kiln needs to coordinate providers, tools, terminals, Git worktrees, approvals,
and recoverable task state across Windows and Linux. The user prefers Rust and
Svelte. A future headless daemon and remote supervision mode should not require
a rewrite.

## Decision

Use a pure Rust orchestration core behind a versioned application API. Expose it
through Tauri to a Svelte desktop interface. Keep provider adapters, workspace
hosting, permissions, event persistence, and platform behavior outside the UI.
Treat a future headless service as another transport over the same API.

The product preview is a separate deployable web client used to validate
information architecture and onboarding; it is not the orchestration core.

## Consequences

- Core behavior can be tested without a browser or desktop runtime.
- The interface remains capability-driven and provider independent.
- Windows/Linux implementation details can be isolated behind traits.
- Remote operation can reuse commands and events.
- More up-front contract design is required.
- Streaming, cancellation, and event ordering must be modeled explicitly rather
  than delegated to component state.

## Rejected alternatives

- **Provider calls directly from Svelte:** weak secret and process boundary,
  duplicated transport logic, and difficult headless reuse.
- **Tauri commands containing business logic:** tightly couples the core to one
  delivery surface.
- **Terminal-output parsing as the primary protocol:** fragile ordering and
  capability inference.
- **Cloud-first orchestration:** conflicts with repository ownership, local
  credentials, and offline/local-provider goals.
