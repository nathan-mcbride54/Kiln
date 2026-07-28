# Kiln evolving roadmap

**Revision:** 0.1  
**Last reviewed:** 2026-07-28  
**Launch platforms:** Windows and Linux  
**Later platform:** macOS

Kiln's roadmap is measured in complete, reliable user journeys. A feature is
not done because code exists; it is done when its acceptance gates pass on its
target platforms.

## Current horizon

### H0 — Product foundation

**Status:** `in_progress`  
**Outcome:** A coherent product shell and stable technical contracts.

Scope:

- [x] Product language, visual system, and interactive workbench preview.
- [x] Provider onboarding for OpenAI, Anthropic, and local compatible servers.
- [x] Rust provider abstraction and a Svelte/Tauri desktop foundation.
- [x] Versioned product specification, architecture, and roadmap.
- [ ] Versioned command/event contract shared by desktop and headless modes.
- [ ] SQLite event schema, migrations, projections, and replay fixtures.
- [ ] Windows and Linux continuous integration.

Exit gates:

- The Svelte UI contains no provider-specific transport logic.
- The Rust core runs in tests without Tauri.
- A recorded fake session can replay into the complete task UI.
- Database projections rebuild deterministically.
- Clean Windows and Linux builds pass.

## Committed horizons

| ID | Horizon | Status | User outcome |
|---|---|---|---|
| H0 | Product foundation | `in_progress` | Stable shell and contracts |
| H1 | Connected vertical slice | `planned` | One real task through each required provider |
| H2 | Reliable task workspace | `planned` | Concurrent daily work that survives restarts |
| H3 | Open agent ecosystem | `planned` | ACP agents and MCP tools in one task UI |
| H4 | Windows + Linux beta | `planned` | Installable, accessible daily driver |
| H5 | Secure autonomy | `deferred` | Stronger isolation and remote supervision |
| H6 | macOS release | `later` | Signed, notarized feature parity |
| H7 | Advanced orchestration | `discovery` | Coordinated specialist-agent workflows |

### H1 — Connected vertical slice

Deliverables:

- Streaming OpenAI Responses adapter.
- Streaming Anthropic Messages adapter.
- Streaming and non-streaming local Chat Completions adapter.
- Provider discovery, capability diagnostics, and manual model fallback.
- Native file read/write, search, and command tools.
- Ordered activity timeline, cancellation, approval flow, and diff review.

Exit gates:

- The same fixture task completes through all three provider types.
- Tool input and results appear in causal order.
- Cancellation stops generation and tool execution within a documented bound.
- A denied tool never executes.
- Every changed file can be reviewed before acceptance.
- Credentials never appear in the database, logs, exports, or crash reports.

### H2 — Reliable task workspace

Deliverables:

- Per-task Git branches and worktrees.
- Concurrent task supervision with ordered per-task queues.
- Checkpoints, retry, resume, fork, archive, and targeted restore.
- Crash-resumable sessions and explicit turn-completion receipts.
- Terminal inspector, verification profiles, and context compaction.

Exit gates:

- Two tasks can change one repository without sharing state.
- Restarting mid-turn produces a truthful, recoverable task.
- Restoring one checkpoint preserves unrelated user work.
- Child processes and worktrees are cleaned up after cancellation or restart.
- Completed tasks retain verification evidence and a finalized diff.

### H3 — Open agent ecosystem

Deliverables:

- MCP clients for local stdio and remote HTTP servers.
- ACP client with capability negotiation.
- Goose and OpenCode connection profiles.
- Extension health, lifecycle, and per-tool permissions.
- Sanitized session import and export.

Exit gates:

- Goose and OpenCode sessions run through the same task surface.
- Unsupported features are disabled with an explanation.
- MCP tools cannot bypass the permission engine.
- A failed adapter can restart without restarting Kiln.
- Exports can omit secrets, source content, and sensitive command output.

### H4 — Windows + Linux beta

Deliverables:

- Signed installers and safe updater path.
- Command palette and complete keyboard navigation.
- Accessibility, reduced-motion, and compact-density passes.
- Long-session performance work.
- Redaction-preview diagnostics.
- Cost, token, and latency reporting when advertised.
- Explicit commit-preparation flow and backup controls.

Exit gates:

- All core flows are keyboard accessible.
- A 10,000-event session remains within the agreed responsiveness budget.
- A 24-hour task leaks neither processes nor unbounded memory.
- Upgrades and rollback preserve sessions and policies.
- Windows and Linux release smoke suites pass.

## Deferred and later horizons

### H5 — Secure autonomy and remote operation

Starts only after the local product is reliable. Candidate scope includes
platform sandboxing, network egress policy, resource budgets, a headless daemon,
authenticated pairing, and reconnectable event streaming.

Security gates must explicitly cover filesystem, network, CPU, memory,
execution-time, pairing revocation, and disconnected-client behavior.

### H6 — macOS release

Requires native PTY and Keychain behavior, physical Apple Silicon validation,
signing, notarization, hardened runtime, updater validation, and parity across
provider, Git, terminal, permission, recovery, and migration suites.

### H7 — Advanced orchestration

Discovery candidates:

- Parallel specialist agents with a visible dependency graph.
- Compare models on one task using recorded evaluation fixtures.
- Policy-aware automatic reviewer.
- Reusable task recipes.
- Local semantic index.
- Pull-request provider integrations.
- Shared team policy and audit export.

A candidate becomes committed only after its user problem, success metric, and
security model are defined.

## Cross-cutting quality gates

Every horizon must add or retain:

- Unit tests for state machines and policy decisions.
- Recorded-stream tests for provider normalization.
- End-to-end happy, denied, cancelled, and restarted paths.
- Secret-redaction coverage.
- Windows and Linux path, quoting, shell, PTY, and process-tree coverage.
- Forward and rollback migration coverage.
- Accessibility checks for new user-visible workflows.
- Honest degradation when a provider lacks a capability.

## Roadmap record format

Every item carries:

- Stable ID and user outcome.
- Status: `discovery`, `planned`, `in_progress`, `blocked`, `beta`, `done`, or
  `deferred`.
- Target platforms and dependencies.
- Acceptance criteria.
- Decision links.
- Owner when applicable.
- Last-reviewed date and a scope-change note.

Roadmap changes are reviewed at milestone boundaries and versioned with
releases. Provider-specific behavior must update the capability matrix;
architectural changes require a short decision record.

## Initial success metrics

- Median time from opening a repository to starting a task.
- Successful completion rate by provider profile.
- Approval prompts per completed task.
- Cancellation success rate and latency.
- Crash-recovery success rate.
- Tasks completed without an external terminal.
- Completed tasks with verification evidence.
- Provider-normalization errors per 1,000 events.
- User-restored changes per completed task.
- Installer and update success by platform.
