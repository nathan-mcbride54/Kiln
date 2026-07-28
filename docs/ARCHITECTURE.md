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

The foundation implements non-streaming connection and chat paths first.
Streaming evolves into an event stream without changing the UI contract:

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
Later commands cover:

- project open and remember;
- task create, cancel, fork, retry, archive, and resume;
- approval decide and policy persist;
- artifact and diff queries;
- checkpoint create and restore;
- provider, model, and capability discovery;
- verification profile execution.

Command inputs never contain durable secret values after onboarding. They carry
opaque credential references resolved by the trusted Rust boundary.

## Event storage

SQLite will contain:

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

## Safety model

The permission engine evaluates action plus resource before execution. Provider
adapters and MCP/ACP extensions submit proposed actions through the same
boundary.

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
