# ADR 0010: Keep provider tool turns transient behind strict Rust codecs

**Status:** Accepted
**Date:** 2026-07-29

## Context

Provider tool-call protocols expose different streaming fragments, identifiers,
argument envelopes, and continuation formats. Passing those values through IPC
or persisting them as application events would couple Kiln's state model to
upstream protocols and let untrusted provider identifiers become approval or
causality keys.

H1-008 needs one cross-provider repository loop without weakening the existing
permission, credential, event, or redaction boundaries.

## Decision

`kiln-core` owns the strict allowlisted repository schemas and typed
success/failure outcomes. `kiln-providers` owns bounded protocol-specific
decoding and continuation encoding.

Provider-native call handles, raw arguments, and continuation payloads remain
transient, non-serializable values inside the Rust provider boundary. The
orchestrator converts a complete call into `RepositoryToolRequest`, mints
internal durable IDs, routes it through policy and workspace services, and
returns a typed outcome through the opaque provider handle.

Diagnostics and production decoding share the same stream accumulator.

## Consequences

- Provider protocol changes cannot silently change the application event
  contract.
- Provider IDs cannot become approval IDs or durable causality keys.
- Malformed and oversized calls fail before policy evaluation or execution.
- Tool-only provider responses no longer require fabricated message text.
- The orchestrator must keep a short-lived provider session alive across
  provider and tool steps.
- Recorded fixtures are required for every supported provider protocol change.
