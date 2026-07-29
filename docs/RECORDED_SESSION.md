# Kiln recorded session

**Fixture version:** 1
**Status:** H0 integration gate
**Canonical source:** `fixtures/sessions/complete-task-v1.json`

## Purpose

The recorded session is Kiln's provider-free vertical slice. It lets the
conversation, activity timeline, approval history, tool state, artifact
inspector, and completion receipt evolve together without network access or
provider credentials.

The recording deliberately includes:

1. project, workspace, task, session, and turn identity;
2. two assistant message deltas followed by an authoritative completion;
3. a published plan;
4. a proposed workspace write and an allow-once approval;
5. structured edit output and a completed diff artifact;
6. a second tool with CRLF test output;
7. test and command-output artifacts;
8. a terminal turn receipt.

The paths contain spaces and Unicode characters. The output contains CRLF
boundaries. These values are inert contract fixtures: replay never touches the
represented path or executes the represented tool.

## One source, two projection stacks

`scripts/render-recorded-session.mjs` validates the canonical JSON and
generates `desktop/src/lib/recorded-session.generated.ts`. The generated module
is the desktop's browser-preview stream.

Rust deserializes the same JSON directly. Its complete domain projection is
compared with `fixtures/sessions/complete-task-v1.expected.json`. TypeScript
tests independently assert the user-facing task, activity, and inspector
projection.

This is intentional duplication of projector implementations, not fixture
data. Rust remains the canonical domain contract; Svelte owns presentation
labels and times.

## Determinism rule

For one ordered recording:

- repeated Rust replay must serialize to identical bytes;
- repeated TypeScript replay must be deeply equal and serialize identically;
- sequence gaps, mixed streams, or a breaking contract version must fail;
- `turn_receipt` must remain the final event.

## Editing the recording

After changing the source fixture:

```powershell
npm run fixtures:render
npm run fixtures:check
npm run desktop:contract-check
node --test tests/desktop-events.test.ts
cargo test -p kiln-core --all-targets --offline
```

Commit the JSON source, generated TypeScript, and expected Rust projection
together. The normal `npm test` freshness gate rejects drift.
