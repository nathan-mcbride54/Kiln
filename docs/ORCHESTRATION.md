# Rust task orchestration

**Status:** H1-008 core runner implemented; live provider and desktop integration pending
**Canonical implementation:** `crates/kiln-orchestrator`

## Purpose

`kiln-orchestrator` is the transport-independent owner of one
provider-driven repository task. It sits between normalized provider turns,
SQLite application events, the central workspace permission boundary, an
injected approval gate, and the shared cancellation domain.

The runner deliberately has no Tauri dependency. Desktop, CLI, tests, and
future headless transports can supply different approval interfaces and
provider sessions without changing task causality or workspace authorization.

## Durable-before-action order

For every repository call, the runner:

1. mints an internal tool identity unrelated to the provider call handle;
2. appends `tool_proposed`;
3. preflights the central workspace policy;
4. appends `approval_requested` before waiting when policy returns `ask`;
5. appends `approval_decided` before granting one ephemeral approval;
6. appends `tool_started` before invoking the workspace host;
7. appends a bounded `tool_output`, `tool_completed`, and optional diff
   artifact; and
8. returns the transient typed result to the provider session.

Provider call handles, raw arguments, file contents, search terms, previews,
and full diffs never enter these application events.

## Provider-session boundary

`ProviderTaskSession` accepts zero or more transient
`ProviderToolContinuation` values and returns one normalized
`ProviderTaskTurn`. The session owns protocol-specific HTTP history and uses
`kiln-providers` to decode calls and encode their continuations.

The core runner accepts tool-only provider turns, executes multiple calls in
provider order, aggregates token usage, and stops only at one terminal
`turn_receipt`.

The current production adapters do not yet implement this session boundary.
That adapter work, credential resolution, and the thin desktop command are the
remaining live-integration slice.

## Safety and bounds

- at most 32 provider steps per task turn;
- at most 4,096 normalized events in one provider step;
- at most 16 repository calls in one provider response;
- at most 64 repository calls across the task turn;
- at most 1 MiB of normalized message text per provider step;
- canonical `task:<task-id>` streams and bounded internal identifiers;
- blocking workspace work runs outside the async executor;
- approval denial returns a typed failure to the provider without a write;
- cancellation wins provider waits and approval waits, reaches workspace
  execution, and produces one terminal cancelled receipt; and
- terminal receipts clear any projected pending approval.

Storage failure remains an explicit hard error because Kiln cannot claim a
terminal state that it failed to persist.

## Fixtures

The integration suite runs a real temporary Git repository through
read-edit-review for OpenAI Responses, Anthropic Messages, and compatible Chat
Completions. It proves:

- provider results cause the next provider request;
- approved writes are version checked and produce durable diff metadata;
- declined and cancelled writes have no filesystem side effect;
- event sequences and causation are contiguous;
- provider handles and raw repository content are absent from SQLite events;
- token usage is aggregated; and
- exactly one terminal receipt closes the task.

## Remaining H1-008 work

Implement real provider HTTP sessions on this trait, resolve credentials at
the trusted command boundary, expose the runner through a thin Tauri command,
and move the live Svelte task path from synthesized lifecycle events to
projecting the orchestrator's persisted stream.
