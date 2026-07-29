# Kiln event storage

**Storage schema:** 1
**Status:** H0 foundation complete
**Implementation:** `crates/kiln-storage`

## Responsibilities

`kiln-storage` persists the immutable envelopes defined by
[the application contract](APPLICATION_CONTRACT.md). It does not interpret
provider payloads, mutate Svelte state, store credentials, or execute tools.

The H0 implementation provides:

- a single ordered SQLite writer;
- transactional event batches;
- durable schema migration history;
- unique event IDs and per-stream sequence constraints;
- ordered replay with contract validation;
- persistence-time sensitive-data rejection;
- forward and rollback migration tests;
- deterministic replay fixtures.

The Tauri shell opens the database in the operating system's application-data
directory. The Svelte client loads its task stream at startup and persists each
new batch before updating the visible projection.

The H1 task-loop core writes provider messages, tool transitions, approval
decisions, artifacts, and the terminal receipt directly through
`SqliteEventStore` before allowing the next side effect. The current live
desktop chat path retains the Svelte-coordinated writer until the provider
session and Tauri command are connected to that runner.

## Database location

The desktop application will resolve the production path through
`kiln-platform`:

```text
<application data>/Kiln/kiln.db
```

Tests use isolated in-memory databases. No repository-local database is a
supported production configuration.

## Schema

### `schema_migrations`

| Column | Purpose |
|---|---|
| `version` | Monotonic storage schema version |
| `name` | Human-readable migration identity |
| `applied_at_ms` | Operational migration timestamp |

### `event_log`

| Column | Constraint |
|---|---|
| `stream_id` | Required; part of the primary key |
| `sequence` | Positive; part of the primary key |
| `event_id` | Required and globally unique |
| `schema_version` | Positive application-contract major version |
| `task_id` | Optional task lookup key |
| `occurred_at_ms` | Non-negative event time |
| `causation_id` | Optional direct cause |
| `correlation_id` | Optional user-operation/turn identity |
| `event_type` | Indexed normalized event name |
| `payload_json` | Serialized tagged application payload |

Indexes support task replay and event-type/time diagnostics. The envelope
columns remain separate from `payload_json` so ordering and identity checks do
not require JSON extraction.

## Append algorithm

1. Validate the contract version, identifiers, and payload.
2. Reject mixed-stream batches, gaps, and duplicate sequence numbers.
3. Reject forbidden sensitive keys and credential markers.
4. Begin one SQLite transaction.
5. Read the durable stream tail inside the transaction.
6. Require the first new event to be exactly the next sequence.
7. Insert every event.
8. Commit all events or none.

SQLite constraints provide a second line of defense against duplicate event
IDs and `(stream_id, sequence)` pairs.

H0 uses one pooled connection, WAL mode, a five-second busy timeout, foreign
keys, and normal synchronous mode. This favors clear causal ordering. H2 may
add supervised read connections after measurement; it will not add concurrent
writers to a task stream.

## Migration policy

- Storage migrations and application-contract versions are independent.
- Every forward migration is transactional.
- Every migration includes an explicit rollback used by migration tests.
- A database newer than the running build fails clearly.
- Event meaning is never rewritten silently by a storage migration.
- Destructive rollback is a development/test operation, not an automatic user
  recovery strategy.

Before release, migrations will be tested against copies of previous fixture
databases and backup/restore behavior.

## Sensitive-data gate

Typed application events exclude credential objects by construction. Before
serialization, storage also scans for forbidden keys such as API keys,
authorization, cookies, passwords, refresh tokens, custom headers, and full
environments. Common bearer-token markers are rejected in string content.
String detection uses the same central redactor that scrubs provider errors,
diagnostics, exports, and crash-report text. This remains defense in depth:
secrets are excluded from typed event contracts and stored in the OS credential
service. See [the credential-storage guide](CREDENTIALS.md).

## Remembered projects

Project metadata uses the event log rather than a mutable side table. Opening a
Git repository appends `project_opened` and `workspace_ready` to a dedicated
`project:<id>` stream. The recent-project query returns the newest
`project_opened` event per stream, and the workspace service revalidates each
root before the interface activates it.

Project payloads contain canonical paths, Git status, and safe defaults only.
Credential-shaped fields still fail the persistence-time sensitive-data scan.

## Recovery model

On startup:

1. Tauri resolves the application-data directory and opens `kiln.db`.
2. The desktop requests its task stream ordered by `sequence`.
3. Every application envelope and causal sequence is revalidated.
4. Svelte restores the same deterministic projector used for live events.
5. The composer becomes available only after restore completes.

Corrupt, gapped, cross-stream, or unsupported events fail before a partial
projection is shown.

For activity on the current chat-only desktop path, Svelte builds and validates
the candidate event batch, asks Tauri to commit it, and only then replaces the
visible projection. A failed write restores the in-memory sequencer to the
durable tail so retrying cannot create a sequence gap. The Rust task-loop core
instead owns its event sequence and appends each transition before continuing
provider, approval, or repository work; a failed append ends the loop without a
false receipt.

## Validation

```powershell
$kilnTarget = Join-Path ([System.IO.Path]::GetTempPath()) "kiln-cargo-target"
$env:CARGO_TARGET_DIR = $kilnTarget

cargo test -p kiln-storage --all-targets --offline
cargo clippy -p kiln-storage --all-targets --offline -- -D warnings
```

Tests cover atomic append/replay, rejected-batch rollback, secret rejection,
schema rollback/reapply, byte-stable repeated replay, a versioned projection
snapshot, and close/reopen recovery from a file database.
