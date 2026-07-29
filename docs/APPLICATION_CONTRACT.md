# Kiln application contract

**Contract version:** 1
**Status:** Implemented foundation
**Canonical implementation:** `crates/kiln-core/src/events.rs`

## Purpose

Kiln's application contract is the stable boundary between orchestration and
every interface. Provider payloads, Tauri commands, and future headless
transports may differ, but the task interface consumes the same ordered
application events.

The Rust types are canonical. The desktop TypeScript mirror lives in
`desktop/src/lib/events.ts`; cross-language generation remains H1-007. Until
then, Rust serialization tests, TypeScript type checks, and replay tests guard
the shared field and variant names.

## Envelope

Every event uses this shape:

```json
{
  "schemaVersion": 1,
  "eventId": "event-42",
  "streamId": "task:provider-router",
  "taskId": "provider-router",
  "sequence": 42,
  "occurredAtMs": 1753731600000,
  "causationId": "command:turn-7",
  "correlationId": "turn-7",
  "payload": {
    "type": "turn_receipt",
    "data": {
      "turnId": "turn-7",
      "outcome": "completed",
      "summary": "Provider response completed."
    }
  }
}
```

| Field | Rule |
|---|---|
| `schemaVersion` | Major compatibility version. Version 1 is currently supported. |
| `eventId` | Globally unique immutable event identity. |
| `streamId` | Identity of the causally ordered stream. Task streams use `task:<id>`. |
| `taskId` | Present for task-scoped events; optional for application/project streams. |
| `sequence` | Starts at 1 and increases by exactly one within a stream. |
| `occurredAtMs` | Milliseconds since Unix epoch supplied by the trusted application boundary. |
| `causationId` | Command or event that directly caused this transition. |
| `correlationId` | Groups all events belonging to one user-visible operation or turn. |
| `payload` | Tagged, normalized application event. Never a raw provider response. |

Commands use the same `schemaVersion`, a `commandId`, issue time, optional task
identity, and typed payload.

## Event catalog

| Family | Version 1 events | Projection responsibility |
|---|---|---|
| Project | `project_opened` | Identity, canonical root, branch, commit, status, safe defaults, and recent-project views |
| Workspace | `workspace_ready` | Checkout/worktree identity and isolation |
| Task | `task_created`, `task_status_changed` | Task list, status, and lifecycle |
| Session | `session_started` | Provider/model attribution |
| Turn | `turn_started`, `turn_receipt` | Running state and terminal outcome |
| Message | `message_added`, `message_delta`, `message_completed` | Conversation and streaming text |
| Approval | `approval_requested`, `approval_decided` | Blocking trust decision and receipt |
| Tool | `tool_proposed`, `tool_started`, `tool_output`, `tool_completed` | Ordered activity and diagnostics |
| Artifact | `artifact_published` | Diff, plan, test, file, and diagnostic surfaces |

`turn_receipt` is the authoritative quiescence marker. A missing receipt means
the turn is incomplete even if a message or tool result exists.

## Ordering and replay

1. An event is validated before projection.
2. A projector accepts only its expected stream and next sequence.
3. Projection functions are deterministic and side-effect free.
4. Replaying the same ordered events must produce the same read model.
5. Gaps, duplicates, cross-stream events, and unsupported versions fail before
   state changes.
6. Live delivery, SQLite recovery, and recorded-session replay use the same
   projector path.

The desktop commits event batches to SQLite before making their projection
visible and rebuilds the ordered stream from disk during startup. The canonical
provider-free integration recording is documented in
[`RECORDED_SESSION.md`](RECORDED_SESSION.md).

Project streams use the same rules. `project_opened` carries a path-derived
project identity, canonical root, optional branch and commit, typed working-tree
status counts, and provider/model/verification defaults. `workspace_ready`
identifies the direct non-isolated checkout. Older version-1 project events
without the additive status/default fields deserialize to empty defaults.
Recent-project views select the latest project event per stream and revalidate
its root before activation.

## Compatibility policy

Compatible within version 1:

- adding optional envelope or payload fields;
- adding an event variant that older consumers can deliberately ignore at an
  external transport boundary;
- adding enum values only when consumers have an explicit unknown fallback.

Requires a new major version:

- removing or renaming a field or event;
- changing a field's meaning or type;
- changing sequence, causation, or receipt semantics;
- making an optional field required;
- reusing an existing event name for a different transition.

Serde ignores unknown additive object fields. Both Rust and TypeScript reject
unsupported major versions explicitly. Database migrations do not silently
rewrite old event meaning.

## Provider boundary

Provider adapters return normalized stream events inside `kiln-providers`.
The desktop bridge adds application identity and converts deltas, completion,
cancellation, or a structured error into the application vocabulary. Tauri
commits each event batch to SQLite before `App.svelte` renders the deterministic
projection.

At startup, Tauri reloads the same stream from SQLite and Svelte rebuilds the
same projection. `message_delta` events are followed by `message_completed` and
an authoritative receipt. After a cancelled receipt, late provider mutations
remain in sequence but cannot change the projection. See
[`STREAMING.md`](STREAMING.md).

## Data safety

Forbidden in event payloads:

- API keys, authorization headers, cookies, and credential-store values;
- complete process environments;
- unredacted provider request or response bodies;
- unrestricted command output without the redaction pipeline;
- opaque binary or large artifact bodies.

Durable events contain redacted summaries and content-addressed artifact
references. Raw diagnostic retention, when implemented, is separate,
explicitly enabled, and subject to redaction.

## Validation

```powershell
npm run desktop:contract-check
node --test tests/desktop-events.test.ts

$kilnTarget = Join-Path ([System.IO.Path]::GetTempPath()) "kiln-cargo-target"
$env:CARGO_TARGET_DIR = $kilnTarget
cargo test -p kiln-core -p kiln-platform -p kiln-providers --offline
```

The default `npm test` also runs the TypeScript contract check and replay tests.
