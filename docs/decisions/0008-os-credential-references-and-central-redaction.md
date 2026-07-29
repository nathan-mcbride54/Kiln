# ADR 0008: OS credential references and central redaction

- Status: accepted
- Date: 2026-07-29
- Review at: H2 export and crash-recovery work

## Context

Provider keys previously lived in Svelte state and crossed the normal test and
chat command boundary. Session-only handling avoided SQLite persistence but did
not survive restart and made every frontend request part of the secret trust
boundary. Provider error bodies can also echo supplied credentials.

## Decision

Store provider secrets in Windows Credential Manager or the Linux Secret
Service. Persist and transport only opaque, random profile references. Resolve
those references in the Rust desktop boundary, verify their provider binding,
and attach an ephemeral, non-serializable credential only for provider I/O.

Use one core redactor for dynamic resolved values and structured credential
patterns. Apply it before provider errors cross into application commands and
reuse its detection in the durable event-store gate. Zeroize Rust secret
buffers on drop where the language and upstream APIs permit it.

## Consequences

The desktop can restore provider profiles without putting secrets in SQLite,
browser storage, events, exports, or ordinary command payloads. A compromised
frontend can request use of a configured provider profile but cannot read its
secret or bind it to another provider. OS-vault availability is now required
to save or resolve cloud credentials.

Rust-owned buffers receive best-effort zeroization. JavaScript, HTTP-library,
and operating-system internals may copy immutable values, so Kiln does not
claim perfect process-memory erasure. Structural redaction is defense in depth,
not permission to log raw request or response bodies.
