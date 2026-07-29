# Kiln architecture

## System shape

```text
Svelte desktop interface
  ↕ versioned commands, queries, and event stream
Tauri application boundary / future headless transport
  ↕
Rust orchestration core
  ├─ task and session state machines
  ├─ permission and policy engine
  ├─ normalized event log and projections
  ├─ context manager and agent loop
  └─ job supervision and receipts
  ↕                              ↕
Provider and agent adapters      Workspace host
  ├─ OpenAI Responses              ├─ files and search
  ├─ Anthropic Messages            ├─ shell and PTY
  ├─ local compatible              ├─ Git and worktrees
  └─ ACP                           └─ platform isolation
  ↕
SQLite events + OS credential service
```

The deployed React preview demonstrates the product and provider onboarding.
The production desktop interface is Svelte. Both are clients of equivalent
application boundaries; neither owns orchestration rules.

## Rust workspace

```text
kiln-core       normalized commands, events, errors, and domain types
  ↑          ↑              ↑
  │          │              └─ kiln-workspace  safe Git discovery, inspection, and atomic editing
  │          └─ kiln-storage  SQLite event log and replay
  └─ kiln-providers  OpenAI, Anthropic, and local HTTP adapters

kiln-platform   clock, paths, and future OS service traits
       ↖       ↗
      kiln-tauri  desktop transport and application startup only
```

`kiln-core`, `kiln-providers`, `kiln-platform`, and `kiln-workspace` compile and
test without Tauri. The desktop crate injects provider, storage, and repository
services into Tauri state and exposes thin commands; it contains no provider
parsing or Git discovery rules.

## Boundary rules

1. Svelte does not call providers or launch processes.
2. Tauri exposes the application API but does not own business rules.
3. Desktop and future headless modes use the same core.
4. Provider adapters emit events; they cannot mutate interface state.
5. Every durable transition originates from an immutable domain event.
6. Read models can be rebuilt from those events.
7. Per-task queues preserve causal order.
8. Explicit receipts mark turn quiescence, checkpoint completion, and finalized
   diff availability.
9. Raw and normalized provider events remain distinguishable.
10. Platform behavior lives behind Rust traits.
11. Remote operation is another transport, not another architecture.

## Core domain vocabulary

| Type | Responsibility |
|---|---|
| Project | Repository identity, instructions, defaults, verification profiles |
| Workspace | Direct checkout or isolated task worktree |
| Task | User-visible unit of work, lifecycle, and branch |
| Session | Conversation with one agent runtime |
| Turn | One user request and resulting activity |
| Event | Immutable normalized state transition |
| Provider profile | Endpoint, credential reference, and capability report |
| Tool call | Proposed/completed action and structured result |
| Approval | Decision, scope, rationale, and persistence |
| Checkpoint | Recoverable task state and Git/file snapshot |
| Artifact | Diff, file, plan, command output, diagnostic, or test result |

## Provider abstraction

The initial Rust abstraction separates transport-specific requests from a
normalized application contract:

```rust
#[async_trait]
pub trait Provider: Send + Sync {
    fn id(&self) -> ProviderId;
    fn capabilities(&self) -> ProviderCapabilities;
    async fn test_connection(&self) -> Result<ConnectionReport>;
    async fn send(&self, request: ChatRequest) -> Result<ChatResponse>;
}
```

The user-facing turn path streams through normalized provider events and an
ordered Tauri channel without changing the UI contract:

```text
turn.started
message.delta*
tool.proposed
approval.requested?
tool.started
tool.output*
tool.completed
message.completed
turn.receipt
```

Normalized errors include authentication, rate limit, invalid request,
unsupported capability, timeout, cancelled, unavailable, malformed response,
and internal adapter failure. Provider payloads remain metadata, never the
application's state model.

## Provider transports

### OpenAI

- `GET /v1/models` for connection diagnostics where permitted.
- `POST /v1/responses` for turns.
- `store: false` by default.
- Extract text from the convenience `output_text` field or typed output blocks.

### Anthropic

- `GET /v1/models` for diagnostics where permitted.
- `POST /v1/messages` with a pinned `anthropic-version`.
- Preserve typed content blocks while normalizing visible text and tool use.

### Local compatible server

- Base URL is user controlled and normalized once.
- `GET /v1/models` probes reachability and model discovery.
- `POST /v1/chat/completions` handles the initial chat path.
- Tokens are optional.
- Future diagnostics probe streaming and tool calls separately so a partially
  compatible server remains useful.

## Application API

Commands are versioned, serializable, and transport-independent. The first
desktop slice exposes provider descriptors, connection tests, and chat sends.
Contract version 1 defines command envelopes and immutable application events
with stream sequence, causation, correlation, and optional task identifiers.
Additive JSON fields are tolerated; unsupported major versions fail validation
explicitly.
The complete event catalog, compatibility policy, envelope example, and
projection rules are documented in
[the application contract](APPLICATION_CONTRACT.md). The architectural choice
is recorded in
[ADR 0002](decisions/0002-versioned-application-events.md).
Provider framing, shared cancellation, the terminal mutation barrier, and the
IPC choice are documented in [the streaming guide](STREAMING.md) and
[ADR 0005](decisions/0005-ordered-provider-streaming.md).
Later commands cover:

- task create, cancel, fork, retry, archive, and resume;
- approval decide and policy persist;
- artifact and diff queries;
- checkpoint create and restore;
- provider, model, and capability discovery;
- verification profile execution.

Command inputs never contain durable secret values after onboarding. They carry
opaque credential references resolved by the trusted Rust boundary.

## Event storage

SQLite schema version 1 now contains:

- immutable event envelopes;
- schema version and migration history;
- task/session/turn identifiers;
- sequence and causation identifiers;
- redacted payloads;
- rebuildable projections;
- checkpoint and artifact references.

Secrets, full OS environment snapshots, and unredacted request headers are
forbidden. Large artifacts use content-addressed files with transactional
references rather than oversized event rows.

The initial store enforces one-stream transactions, the durable sequence tail,
unique event IDs, and a persistence-time sensitive-data gate. See
[the storage specification](STORAGE.md) and
[ADR 0003](decisions/0003-sqlite-immutable-event-log.md). A checked-in
projection snapshot verifies SQLite rebuilds. Tauri opens the application-data
database, and the Svelte durability coordinator commits before projecting and
restores the stream at startup.

Project opens use the same durability boundary. A direct Git workspace is
inspected through the Tauri-free `kiln-workspace` crate, then its
`project_opened` and `workspace_ready` events are appended before the desktop
activates it. Recent projects are derived from latest immutable events and
revalidated against Git on startup. See [the project guide](PROJECTS.md).

Read-only repository tools register that canonical project root, evaluate both
the tool name and canonical path through the permission engine, then perform
bounded file reads or Git-aware searches. Raw source and search matches remain
transient while safe proposal/result summaries enter the task event stream. See
[the repository-tool guide](REPOSITORY_TOOLS.md) and
[ADR 0006](decisions/0006-bounded-repository-inspection.md).

Workspace edits use the same host and policy engine, but `write_file` is an
`ask` action rather than a default allow. Existing files require the SHA-256
returned by a complete read, Rust owns the native confirmation dialog, and the
same-directory replacement is atomic. Approval, safe activity summary, and the
diff artifact metadata are appended in causal order, while the full diff remains
transient so edited secrets cannot enter durable task history. See
[the safe-editing guide](SAFE_EDITING.md) and
[ADR 0007](decisions/0007-native-confirmed-atomic-editing.md).

The provider-free integration gate replays one canonical 25-event recording
through both Rust and TypeScript. It covers message deltas, planning, approval,
tool state, output, diff and test artifacts, and the terminal receipt. See
[the recorded-session specification](RECORDED_SESSION.md).

## Safety model

The permission engine evaluates action plus resource before execution. Provider
adapters and MCP/ACP extensions submit proposed actions through the same
boundary.

The implemented core contract names tool, command, path, network-host, and
extension resources; applies task/project/provider/global rules; keeps
allow-once grants in memory; and accepts the side-effecting operation only
through a guarded executor. Exact precedence and the platform-canonicalization
boundary are documented in [the permission guide](PERMISSIONS.md) and
[ADR 0004](decisions/0004-central-permission-engine.md).

Worktree isolation protects task changes from each other but does not constrain
process capabilities. Stronger filesystem, process, and network containment is
an explicit later security horizon.

## Cross-platform host

Rust traits cover:

- path and filesystem semantics;
- shell selection and argument quoting;
- PTY lifecycle;
- child process trees/groups;
- OS credential storage;
- configuration directories;
- notifications, startup, packaging, and updates.

Platform adapters are tested with fixtures containing spaces, Unicode, unusual
line endings, long paths, cancellation, orphaned children, and restart.

## Testing strategy

- Provider adapters use recorded response and stream fixtures.
- Core state machines use deterministic clocks and IDs.
- Event replay compares stable projection snapshots.
- Permission tests prove denied actions produce no side effect.
- Fault injection covers cancellation, timeout, malformed events, process
  death, disk interruption, and restart.
- Windows and Linux run clean-build, quoting, path, Git, PTY, and packaging
  suites.
- macOS compile checks begin before it becomes a supported release platform.

The initial Windows/Ubuntu matrix and its platform fixtures are specified in
[the continuous-integration guide](CONTINUOUS_INTEGRATION.md).
