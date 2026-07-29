# ADR 0003: SQLite as the immutable local event log

- **Status:** Accepted
- **Date:** 2026-07-28
- **Roadmap:** H0-008

## Context

Kiln needs durable, auditable task history on Windows and Linux without
requiring a service. Sessions must survive restart, projections must be
rebuildable, and task ordering must remain truthful after crashes. Credentials
and large artifacts must not enter the history database.

## Decision

Use SQLite as the local event log behind a Tauri-free `kiln-storage` crate.

- Application events append transactionally and are never updated in place.
- `(stream_id, sequence)` is the causal primary key.
- Event IDs are globally unique.
- Envelope metadata is stored in indexed columns; normalized payloads use JSON.
- Storage migrations have explicit versions and rollback tests.
- H0 uses one writer connection with WAL mode and a busy timeout.
- Loaded events are revalidated before projection.
- Credential-like content is rejected before persistence.
- Large artifacts will use content-addressed files with transactional
  references.

## Consequences

Positive:

- No external database service or daemon is required.
- SQLite behaves consistently across launch platforms.
- Transactions make turn/event batches atomic.
- Ordered replay supports deterministic recovery and diagnostics.
- The storage crate remains reusable by desktop, CLI, and headless transports.

Costs:

- Schema and application-contract migrations require separate discipline.
- One-writer ordering can constrain future throughput.
- JSON payload queries are less strongly typed than dedicated event tables.
- Backups, corruption handling, and projection snapshots still need product
  workflows.

## Rejected alternatives

### JSONL files

Simple append behavior does not provide equivalent transactional migrations,
indexed lookup, uniqueness constraints, or robust concurrent access.

### One relational table per event

This makes every application event addition a storage migration and couples
domain evolution too tightly to the physical schema.

### Embedded key-value store

It would require rebuilding transaction, indexing, migration, and diagnostic
query behavior already provided by SQLite.
