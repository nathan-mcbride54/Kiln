# Kiln evolving roadmap

> Generated from `product/roadmap.json`. Edit the structured source, then run
> `npm run roadmap:render`. Use `npm run roadmap:check` in CI.

| Revision | Last reviewed | Current horizon | Launch platforms | Later platforms |
|---|---|---|---|---|
| 1.5 | 2026-07-29 | H1 — Connected vertical slice | Windows and Linux | macOS |

Kiln measures progress through complete, reliable user journeys. An item is
done only when every acceptance criterion passes on its target platforms.

## Current focus

| ID | Priority | Status | Outcome |
|---|---:|---|---|
| H1-007 | P0 | `planned` | A connection test explains exactly which behaviors a model endpoint supports. |
| H1-008 | P0 | `planned` | A provider can inspect, edit, and finish one real repository task through the same policy-checked loop. |

**Next review trigger:** H1 exit-gate review or any change to provider, event, permission, or platform contracts.

## Horizon overview

| Horizon | Status | Progress | Outcome |
|---|---|---:|---|
| H0 — Product foundation | Complete | 100% | A coherent product shell with stable contracts, deterministic replay, and clean Windows/Linux build gates. |
| H1 — Connected vertical slice | In progress | 75% | One genuine repository task completes safely through each required provider type. |
| H2 — Reliable task workspace | Planned | 0% | Kiln supports concurrent daily work that survives restarts and preserves user changes. |
| H3 — Open agent ecosystem | Planned | 0% | Existing agents and external tools operate through Kiln's task, event, and permission surfaces. |
| H4 — Windows and Linux beta | Planned | 0% | A polished, installable daily driver for regular personal use. |
| H5 — Secure autonomy and remote operation | Deferred | 0% | Long-running tasks gain stronger containment and remote supervision without weakening the local trust model. |
| H6 — macOS release | Later | 0% | Supported daily-driver workflows reach signed and notarized macOS parity. |
| H7 — Advanced orchestration | Discovery | 0% | Kiln differentiates through understandable, policy-aware coordination rather than agent count. |

## H0 — Product foundation

**Lane:** now · **Status:** Complete · **Timeframe:** Current

**Outcome:** A coherent product shell with stable contracts, deterministic replay, and clean Windows/Linux build gates.

### Work items

| ID | Priority | Status | Platforms | Dependencies | Deliverable |
|---|---:|---|---|---|---|
| H0-001 | P0 | `done` | Web, Windows, Linux | — | Product language and interactive preview |
| H0-002 | P0 | `done` | Windows, Linux | — | Typed provider boundary |
| H0-003 | P0 | `done` | All | — | Product specification and architecture |
| H0-004 | P0 | `done` | Windows, Linux | — | Reproducible Svelte/Tauri desktop build |
| H0-005 | P0 | `done` | All | H0-003 | Living roadmap automation |
| H0-006 | P0 | `done` | Windows, Linux, macOS-ready | H0-002 | Extract the UI-independent Rust core |
| H0-007 | P0 | `done` | All | H0-006 | Versioned command and event contract |
| H0-008 | P0 | `done` | Windows, Linux | H0-007 | SQLite event log and deterministic projections |
| H0-009 | P0 | `done` | Windows, Linux | H0-007, H0-008 | Recorded fake-session replay |
| H0-010 | P0 | `done` | Windows, Linux | H0-004, H0-006 | Windows and Linux continuous integration |

### Acceptance criteria

#### H0-001 — Product language and interactive preview

A user can understand Kiln's task, provider, review, and roadmap model before repository execution exists.

- Workbench, provider onboarding, diff review, and roadmap views are interactive.
- Web production build, lint, and rendered-output tests pass.
- No starter branding or unused starter surface remains.

Last reviewed 2026-07-28. Completed in the initial Kiln foundation.

#### H0-002 — Typed provider boundary

OpenAI, Anthropic, and a local compatible server share one normalized Rust command surface.

- OpenAI Responses, Anthropic Messages, and local Chat Completions adapters compile.
- Connection tests and normalized chat responses use typed request and error models.
- Recorded parsing, URL-safety, and secret-redaction tests pass for all provider types.

Last reviewed 2026-07-28. Non-streaming foundation complete; streaming remains H1.

#### H0-003 — Product specification and architecture

Product boundaries, safety guarantees, platform claims, and non-goals are explicit and reviewable.

- The product specification defines launch workflows and permission semantics.
- The architecture separates interface, orchestration, providers, workspace host, and storage.
- Local-first control-plane decisions are recorded.

Last reviewed 2026-07-28. Completed in revision 0.1.

#### H0-004 — Reproducible Svelte/Tauri desktop build

A contributor can install, check, build, and launch the native shell from a clean checkout.

- JavaScript dependencies are locked.
- Svelte type checking and production build pass.
- Tauri launches the generated frontend through the typed Rust commands.
- A clean Windows build passes; a clean Linux build passes in CI.

Last reviewed 2026-07-29. Completed in revision 1.2 after clean hosted Windows and Ubuntu installs, Svelte checks, production builds, and complete Rust workspace builds passed.

#### H0-005 — Living roadmap automation

Repository documentation and both in-product roadmap views are generated from one reviewed source.

- Stable roadmap data is stored in product/roadmap.json.
- ROADMAP.md and web/desktop summary modules are generated from that source.
- A freshness check fails when generated outputs drift.

Last reviewed 2026-07-28. Added in revision 0.2 to eliminate roadmap duplication.

#### H0-006 — Extract the UI-independent Rust core

Provider, task, policy, and event behavior can run and test without Tauri.

- kiln-core, kiln-providers, and kiln-platform crates have no Tauri dependency.
- kiln-tauri contains transport and application wiring only.
- Provider fixture tests run against the extracted crates.

Last reviewed 2026-07-28. Completed in revision 0.3 with a Cargo workspace, Tauri-free core/provider/platform crates, and 15 passing extracted-crate tests.

#### H0-007 — Versioned command and event contract

Every task transition has a stable, serializable, causally ordered representation.

- Project, workspace, task, session, turn, approval, tool, artifact, and receipt events are versioned.
- Per-task sequence and causation identifiers are defined.
- Unknown additive event fields are tolerated and breaking versions fail explicitly.
- The Svelte interface consumes application events rather than provider events.

Last reviewed 2026-07-28. Completed in revision 0.4: Rust and TypeScript contracts, deterministic Svelte projections, provider-result normalization, compatibility tests, contract documentation, and ADR 0002.

#### H0-008 — SQLite event log and deterministic projections

Task state survives restart and can be rebuilt from an auditable local history.

- Events append transactionally with schema and migration versions.
- Secrets and raw environment snapshots are forbidden from durable payloads.
- Projection rebuilds produce stable snapshot fixtures.
- Migration forward and rollback tests pass.

Last reviewed 2026-07-28. Completed in revision 0.6 with transactional SQLite append/replay, migrations, secret rejection, close/reopen recovery, versioned projection snapshots, Tauri application-data initialization, typed storage commands, durable-before-visible Svelte batches, and startup restore.

#### H0-009 — Recorded fake-session replay

The complete task interface can be developed and tested without a live provider.

- A fixture includes streaming text, a plan, a proposed tool, an approval, output, a diff, tests, and a completion receipt.
- Replay produces the expected task, activity, and inspector projections.
- Repeated replay is deterministic.

Last reviewed 2026-07-28. Completed in revision 0.7 with one generated 25-event recording, rich Rust and TypeScript projection snapshots, visible desktop artifacts, and deterministic replay tests.

#### H0-010 — Windows and Linux continuous integration

Cross-platform claims are proved on every change.

- Rust format, check, unit tests, and frontend freshness/build checks run on both platforms.
- Fixtures cover spaces, Unicode paths, line endings, and process cancellation.
- Release-blocking failures are visible before merge.

Last reviewed 2026-07-29. Completed in revision 1.2 with green hosted Windows and Ubuntu jobs, portable generated-file and projection fixtures, and both quality jobs required by main-branch protection.

### Exit gates

- The Svelte interface contains no provider transport or orchestration business logic.
- The Rust core runs in tests without Tauri.
- A recorded fake session replays into the complete task interface.
- Database projections rebuild deterministically.
- Clean Windows and Linux builds pass.
- Architecture changes have decision records and roadmap change notes.

## H1 — Connected vertical slice

**Lane:** now · **Status:** In progress · **Timeframe:** Parallel with H0 infrastructure gates

**Outcome:** One genuine repository task completes safely through each required provider type.

### Work items

| ID | Priority | Status | Platforms | Dependencies | Deliverable |
|---|---:|---|---|---|---|
| H1-001 | P0 | `done` | Windows, Linux | H0-007, H0-008 | Normalized streaming and cancellation |
| H1-002 | P0 | `done` | Windows, Linux | H0-006, H0-008 | Open and remember a Git repository |
| H1-003 | P0 | `done` | Windows, Linux | H1-002, H1-004 | Read-only file and search tools |
| H1-004 | P0 | `done` | All | H0-007 | Central permission engine |
| H1-005 | P0 | `done` | Windows, Linux | H1-003, H1-004 | File editing and real diff review |
| H1-006 | P0 | `done` | Windows, Linux | H0-006 | OS credential storage and redaction pipeline |
| H1-007 | P0 | `planned` | Windows, Linux | H1-001, H1-006 | Provider diagnostics and capability discovery |
| H1-008 | P0 | `planned` | Windows, Linux | H1-003, H1-004, H1-005, H1-006, H1-007 | Provider-driven repository task loop |

### Acceptance criteria

#### H1-001 — Normalized streaming and cancellation

Users see responsive provider output and can stop a turn reliably.

- All three adapters emit normalized message deltas and completion events.
- Cancellation propagates through provider HTTP and active tool jobs.
- Late provider events cannot mutate a cancelled turn.

Last reviewed 2026-07-28. Completed in revision 0.9 with normalized SSE adapters, a shared cancellation domain, ordered Tauri channels, durable desktop deltas, a stop control, late-event guards, fixtures, and ADR 0005.

#### H1-002 — Open and remember a Git repository

A user can select a real project and start a task against a truthful workspace state.

- Repository identity, root, branch, status, and project defaults are projected.
- Invalid or unsafe repository selections fail with actionable messages.
- Remembered projects contain no credentials.

Last reviewed 2026-07-28. Completed in revision 1.0 with a Tauri-free bounded Git inspector, typed project/workspace projections, immutable remembered-project events, live startup revalidation, actionable selection errors, credential-free defaults, a desktop project picker, and cross-language tests.

#### H1-003 — Read-only file and search tools

An agent can inspect a repository through typed, visible, policy-checked tools.

- File read, file search, and text search use typed schemas.
- Paths are constrained to the selected workspace.
- Tool proposals and results appear in the ordered activity timeline.

Last reviewed 2026-07-28. Completed in revision 1.1 with tagged Rust and TypeScript schemas, canonical workspace containment, Git-aware bounded search, central tool-and-path policy checks, cancellation, transient raw results, durable activity summaries, desktop inspection controls, tests, and ADR 0006.

#### H1-004 — Central permission engine

Every agent action is evaluated as allow, ask, or deny against a named resource.

- Policies scope tool, command, path, network host, and extension resources.
- Allow-once decisions never persist silently.
- Denied actions execute no provider, filesystem, process, or network side effect.
- Extensions cannot bypass policy evaluation.

Last reviewed 2026-07-28. Completed in revision 0.8 with typed resource and origin rules, deterministic precedence, ephemeral allow-once grants, guarded execution, extension enforcement, tests, and ADR 0004.

#### H1-005 — File editing and real diff review

Users can inspect every agent change before accepting it.

- Writes are workspace-bound, policy-checked, and recorded as artifacts.
- Unified diff shows real changed files and hunks.
- An interrupted write cannot leave an unreported partial state.

Last reviewed 2026-07-29. Completed in revision 1.3 with bounded whole-file replacement, read-version preconditions, resource-bound allow-once policy, native confirmation, atomic Windows/Unix replacement, durable approval and safe diff metadata, and a transient real desktop diff view.

#### H1-006 — OS credential storage and redaction pipeline

Provider credentials survive safely without entering application data.

- Credential Manager and the libsecret-compatible Linux Secret Service store opaque profile references.
- Secret values are zeroized where practical.
- Provider errors, diagnostics, exports, and crash reports pass central redaction tests.

Last reviewed 2026-07-29. Completed in revision 1.4 with provider-bound opaque references, Windows Credential Manager and Linux Secret Service adapters, non-serializable zeroizing Rust secrets, central provider-error and persistence redaction, secure desktop onboarding, and cross-platform tests.

#### H1-007 — Provider diagnostics and capability discovery

A connection test explains exactly which behaviors a model endpoint supports.

- Reachability, authentication, model discovery, streaming, and tool compatibility report separately.
- The UI follows advertised capabilities instead of provider-name conditions.
- First-party cloud profiles remain pinned to official origins; compatible custom profiles bind credentials to an exact normalized origin and warn before that destination changes.

Last reviewed 2026-07-29. Promoted to P0 in revision 1.5 because the provider-driven task loop needs truthful tool capability reports and origin-bound credential routing.

#### H1-008 — Provider-driven repository task loop

A provider can inspect, edit, and finish one real repository task through the same policy-checked loop.

- OpenAI, Anthropic, and local compatible adapters normalize tool proposals and tool results without exposing provider payloads to the interface.
- The Rust orchestration loop routes every proposed tool through the central permission engine and returns results to the provider in causal order.
- One recorded read-edit-review fixture completes through all three providers, while denied and cancelled calls produce no side effect.

Last reviewed 2026-07-29. Added in revision 1.5 after the progress review found that H1's provider-driven exit gate had no implementation item beyond manually invoked workspace tools.

### Exit gates

- The same fixture task completes through OpenAI, Anthropic, and a local compatible server.
- Tool input and results appear in causal order.
- Cancellation stops generation and execution within a documented bound.
- Denied tools produce no side effect.
- Every changed file is inspectable before acceptance.
- Credentials never appear in events, logs, exports, or crash reports.

## H2 — Reliable task workspace

**Lane:** next · **Status:** Planned · **Timeframe:** After the H1 slice

**Outcome:** Kiln supports concurrent daily work that survives restarts and preserves user changes.

### Work items

| ID | Priority | Status | Platforms | Dependencies | Deliverable |
|---|---:|---|---|---|---|
| H2-001 | P0 | `planned` | Windows, Linux | H1-002, H1-005 | Per-task Git worktrees and branches |
| H2-002 | P0 | `planned` | Windows, Linux | H2-001, H0-007 | Concurrent task supervision |
| H2-003 | P0 | `planned` | Windows, Linux | H2-001, H0-008 | Checkpoints, retry, fork, and targeted restore |
| H2-004 | P0 | `planned` | Windows, Linux | H0-008, H1-001 | Crash-resumable sessions |
| H2-005 | P1 | `planned` | Windows, Linux | H1-004 | Terminal and verification profiles |
| H2-006 | P1 | `planned` | All | H0-008, H1-001 | Context compaction and usage visibility |

### Acceptance criteria

#### H2-001 — Per-task Git worktrees and branches

Concurrent task changes remain isolated without claiming a security sandbox.

- Create, recover, inspect, and clean task worktrees without deleting unrelated user data.

Last reviewed 2026-07-28. Retained after direct-workspace H1 validation.

#### H2-002 — Concurrent task supervision

Multiple tasks run without interleaving events, approvals, output, or paths.

- Ordered per-task queues and job ownership pass concurrency and fault-injection tests.

Last reviewed 2026-07-28. No scope change.

#### H2-003 — Checkpoints, retry, fork, and targeted restore

Users can recover choices without rewriting unrelated work.

- Checkpoint restore is auditable, scoped, and preserves unrelated user changes.

Last reviewed 2026-07-28. No scope change.

#### H2-004 — Crash-resumable sessions

Application or adapter failure never fabricates completion or loses the durable task state.

- Fault-injection restart tests produce explicit interrupted, recoverable, or completed receipts.

Last reviewed 2026-07-28. No scope change.

#### H2-005 — Terminal and verification profiles

Tasks run explicit project checks with inspectable command output.

- Shell selection, quoting, PTY lifecycle, process-tree cancellation, and verification receipts pass platform fixtures.

Last reviewed 2026-07-28. No scope change.

#### H2-006 — Context compaction and usage visibility

Long sessions remain understandable and within provider limits.

- Compaction boundaries are auditable and usage information degrades honestly by provider capability.

Last reviewed 2026-07-28. No scope change.

### Exit gates

- Two tasks can change one repository without sharing workspace or event state.
- Restart during a turn produces a truthful recoverable task.
- Restoring one checkpoint preserves unrelated user work.
- Child processes and worktrees clean up after cancellation and restart.
- Completed tasks retain verification evidence and a finalized diff.

## H3 — Open agent ecosystem

**Lane:** later · **Status:** Planned · **Timeframe:** After workspace reliability

**Outcome:** Existing agents and external tools operate through Kiln's task, event, and permission surfaces.

### Work items

| ID | Priority | Status | Platforms | Dependencies | Deliverable |
|---|---:|---|---|---|---|
| H3-001 | P1 | `planned` | Windows, Linux | H1-004, H2-002 | MCP stdio and HTTP clients |
| H3-002 | P1 | `planned` | Windows, Linux | H2-002 | ACP client and capability negotiation |
| H3-003 | P1 | `planned` | Windows, Linux | H3-002 | Goose and OpenCode profiles |
| H3-004 | P2 | `planned` | All | H0-008, H1-006 | Sanitized session import and export |

### Acceptance criteria

#### H3-001 — MCP stdio and HTTP clients

Users can add typed tools without weakening central policy.

- MCP lifecycle, health, schemas, calls, and approvals are represented as domain events.

Last reviewed 2026-07-28. Begins only after the native permission path is proven.

#### H3-002 — ACP client and capability negotiation

Kiln can supervise compatible agents without provider-specific UI integrations.

- Sessions, plans, tools, files, terminals, cancellation, and receipts map into Kiln events.

Last reviewed 2026-07-28. Delayed until cancellation and recovery contracts are stable.

#### H3-003 — Goose and OpenCode profiles

Users can connect established agent runtimes through guided configuration.

- One Goose and one OpenCode fixture complete through the common task interface.

Last reviewed 2026-07-28. No bespoke task UI is permitted.

#### H3-004 — Sanitized session import and export

Users can move or share task evidence with explicit content controls.

- A preview can omit secrets, source, command output, and provider payloads before export.

Last reviewed 2026-07-28. No scope change.

### Exit gates

- Goose and OpenCode sessions run through the same task interface.
- Unsupported capabilities disable with an explanation.
- MCP tools pass through the central permission engine.
- A failed adapter restarts without restarting Kiln.
- Exports can omit secrets and sensitive content.

## H4 — Windows and Linux beta

**Lane:** later · **Status:** Planned · **Timeframe:** After ecosystem hardening

**Outcome:** A polished, installable daily driver for regular personal use.

### Work items

| ID | Priority | Status | Platforms | Dependencies | Deliverable |
|---|---:|---|---|---|---|
| H4-001 | P0 | `planned` | Windows, Linux | H0-010, H2-004 | Installers, signing, and safe updates |
| H4-002 | P0 | `planned` | All | H2-005 | Keyboard, accessibility, and reduced motion |
| H4-003 | P0 | `planned` | Windows, Linux | H2-004, H2-006 | Long-session performance and diagnostics |
| H4-004 | P1 | `planned` | All | H2-006, H0-008 | Usage, cost, backup, and retention controls |

### Acceptance criteria

#### H4-001 — Installers, signing, and safe updates

Users can install, update, roll back, and remove Kiln predictably.

- Signed release artifacts and update/rollback smoke tests pass on launch platforms.

Last reviewed 2026-07-28. No scope change.

#### H4-002 — Keyboard, accessibility, and reduced motion

Core journeys remain operable without a pointer or animation.

- Core workflows meet the documented WCAG 2.2 AA interaction expectations.

Last reviewed 2026-07-28. No scope change.

#### H4-003 — Long-session performance and diagnostics

Large and long-running tasks stay responsive and explain failures safely.

- Performance fixtures and redaction-preview diagnostic bundles pass release budgets.

Last reviewed 2026-07-28. Combined related performance and support work.

#### H4-004 — Usage, cost, backup, and retention controls

Users understand provider consumption and control local history.

- Usage degrades honestly and backup/retention operations are explicit and recoverable.

Last reviewed 2026-07-28. Combined lifecycle controls into one release outcome.

### Exit gates

- All core flows are keyboard accessible.
- A 10,000-event session meets the agreed responsiveness budget.
- A 24-hour task leaks neither processes nor unbounded memory.
- Upgrade and rollback preserve sessions and policies.
- Windows and Linux release smoke suites pass.

## H5 — Secure autonomy and remote operation

**Lane:** later · **Status:** Deferred · **Timeframe:** Deferred until the local product is stable

**Outcome:** Long-running tasks gain stronger containment and remote supervision without weakening the local trust model.

### Work items

| ID | Priority | Status | Platforms | Dependencies | Deliverable |
|---|---:|---|---|---|---|
| H5-001 | P1 | `deferred` | Windows, Linux | H4-003 | Platform process and filesystem containment |
| H5-002 | P2 | `deferred` | Windows, Linux | H5-001 | Headless daemon and authenticated pairing |

### Acceptance criteria

#### H5-001 — Platform process and filesystem containment

Autonomous jobs execute within documented OS-specific limits.

- Containment boundaries and escape limitations have a dedicated security review.

Last reviewed 2026-07-28. Deferred until local execution is reliable.

#### H5-002 — Headless daemon and authenticated pairing

A remote client can supervise without direct filesystem access.

- Pairing, revocation, reconnect, and least-privilege APIs pass a separate threat model.

Last reviewed 2026-07-28. Deferred with secure autonomy.

### Exit gates

- Filesystem, network, CPU, memory, and execution-time limits are enforced and visible.
- Remote pairing expires, revokes, and reconnects safely.
- Security documentation distinguishes guarantees from best-effort controls.

## H6 — macOS release

**Lane:** later · **Status:** Later · **Timeframe:** After launch-platform parity

**Outcome:** Supported daily-driver workflows reach signed and notarized macOS parity.

### Work items

| ID | Priority | Status | Platforms | Dependencies | Deliverable |
|---|---:|---|---|---|---|
| H6-001 | P1 | `deferred` | macOS | H4-001, H4-003 | Native macOS platform adapter and packaging |

### Acceptance criteria

#### H6-001 — Native macOS platform adapter and packaging

Kiln behaves natively across shell, PTY, paths, Keychain, signing, and updates.

- Physical-hardware release gates pass and remaining differences appear in-product.

Last reviewed 2026-07-28. Architecture-compatible now; support claim remains deferred.

### Exit gates

- Provider, Git, terminal, permission, credential, update, and recovery suites pass.
- Packages are signed and notarized.
- Apple Silicon is verified on physical hardware.

## H7 — Advanced orchestration

**Lane:** later · **Status:** Discovery · **Timeframe:** Exploratory

**Outcome:** Kiln differentiates through understandable, policy-aware coordination rather than agent count.

### Work items

| ID | Priority | Status | Platforms | Dependencies | Deliverable |
|---|---:|---|---|---|---|
| H7-001 | P2 | `discovery` | All | H3-002, H4-003 | Parallel specialist task graphs |
| H7-002 | P2 | `discovery` | All | H0-009, H2-001 | Model comparison and evaluation fixtures |
| H7-003 | P2 | `discovery` | All | H2-003, H3-001 | Reusable recipes and policy-aware review |

### Acceptance criteria

#### H7-001 — Parallel specialist task graphs

Users can understand dependencies, ownership, and evidence across collaborating agents.

- Discovery defines when specialist graphs outperform one recoverable task loop.

Last reviewed 2026-07-28. Not committed.

#### H7-002 — Model comparison and evaluation fixtures

Users can compare models on reproducible tasks without duplicating side effects.

- Discovery defines isolation, evaluation measures, and cost controls.

Last reviewed 2026-07-28. Not committed.

#### H7-003 — Reusable recipes and policy-aware review

Repeated engineering workflows become inspectable recipes rather than hidden automation.

- Discovery defines authorship, trust, versioning, and rollback semantics.

Last reviewed 2026-07-28. Not committed.

### Exit gates

- Every candidate has a user problem, success measure, and security model before commitment.

## Risk register

| ID | Severity | Status | Risk | Mitigation |
|---|---|---|---|---|
| RISK-001 | high | mitigated | The desktop frontend is not yet proven from a clean dependency install. | Revision 1.2 proves clean hosted Windows and Ubuntu dependency installs, checks, production builds, and complete Rust workspace tests. |
| RISK-002 | medium | mitigated | Future orchestration work could regress into the desktop transport layer. | The workspace now isolates core, providers, platform, and Git workspace contracts from kiln-tauri; keep Tauri-free dependency checks in CI. |
| RISK-003 | high | mitigated | Credentials and upstream payloads may leak without OS storage and centralized redaction. | H1-006 stores provider-bound opaque references in application data, keeps secret values in the OS vault, zeroizes Rust buffers where practical, and applies one tested redactor to provider errors and persistence boundaries. |
| RISK-004 | medium | mitigating | Provider defaults and roadmap content can drift across Rust, web, desktop, and documentation. | H0-005 establishes generated roadmap outputs; provider-contract generation remains H1-007. |
| RISK-005 | medium | mitigated | Windows works locally while Linux behavior remains an architectural claim. | Windows and Ubuntu quality jobs now pass on hosted runners and are both required before main can merge. |
| RISK-006 | medium | mitigated | Concurrent writers or corrupt event order could make restart projections untruthful. | H0-008 uses one transactional SQLite writer, database uniqueness constraints, durable-tail checks, and validated ordered replay. |
| RISK-007 | high | mitigated | Provider bytes arriving after cancellation could rewrite a terminal turn or leave background work active. | H1-001 shares cancellation across HTTP and jobs, gives cancellation race priority, stops the channel at one terminal batch, and blocks late mutations in both projectors. |
| RISK-008 | high | open | Manual workspace tools could be mistaken for a complete provider-driven task loop, causing H1 to close without its stated user journey. | H1-008 now explicitly gates H1 on one policy-checked read-edit-review loop and a common fixture across OpenAI, Anthropic, and a local compatible provider. |

## Decision queue

| ID | Status | Decision | Review point | Reason |
|---|---|---|---|---|
| DEC-001 | accepted | Use a local-first Rust control plane with a Svelte/Tauri desktop client. | H2 exit | The core must support desktop and future headless transports without duplicating orchestration. |
| DEC-002 | accepted | Pin first-party cloud profiles to official origins and require separate origin-bound compatible profiles for custom gateways. | H3 provider-profile expansion | A stored credential must have an unambiguous destination; changing a compatible endpoint requires an explicit warning and credential rebind. |
| DEC-003 | open | Whether Rig becomes the native loop implementation or remains architectural inspiration. | H2 exit | Adoption should follow a stable event, tool, and provider contract. |
| DEC-004 | open | Which Linux package formats receive first-class support. | Before H4-001 | Packaging investment should follow tested user distribution needs. |
| DEC-005 | accepted | Use versioned, causally ordered application events as the interface and replay boundary. | H2 exit | Provider payloads and transport behavior must not become the product state model. |
| DEC-006 | accepted | Use SQLite as an immutable local event log with one H0 writer. | H2 performance review | Kiln needs transactional restart and replay without requiring an external service. |
| DEC-007 | accepted | Route every native and extension action through one guarded permission engine. | H3 extension exit | Allow, ask, deny, and ephemeral approval semantics must not vary by transport or tool source. |
| DEC-008 | accepted | Normalize provider SSE behind ordered Tauri channels and one shared cancellation domain. | H2 crash-resume review | Streaming must remain provider-independent, durable before display, and immune to late cancellation races. |
| DEC-009 | accepted | Require native confirmation and optimistic version checks for atomic direct-workspace edits. | H2 worktree isolation | A frontend assertion must not grant write access, stale reads must not overwrite user work, and completed edits need durable review evidence. |
| DEC-010 | accepted | Store provider secrets in OS credential services and transport only provider-bound opaque references. | H2 export and crash-recovery work | Normal application data and frontend provider commands must not expose durable secret values, and every output boundary needs one tested redaction contract. |

## Success metrics

- Median time from opening a repository to starting a task.
- Successful completion rate by provider profile.
- Approval prompts per completed task.
- Cancellation success rate and latency.
- Crash-recovery success rate.
- Completed tasks with verification evidence.
- Provider normalization errors per 1,000 events.
- User-restored changes per completed task.
- Installer and update success by platform.

## Roadmap policy

- `discovery`: Problem and success measure are being defined.
- `planned`: Accepted scope with dependencies and acceptance criteria.
- `in_progress`: Implementation is active and the item is not yet through every gate.
- `blocked`: No meaningful progress is possible until a named dependency changes.
- `beta`: User journey works but release-quality gates remain.
- `done`: Every acceptance criterion passes on the target platforms.
- `deferred`: Intentionally postponed with the reason kept visible.
- New provider-specific behavior must update the capability contract.
- Architectural changes require a short decision record.
- Deferred work remains visible with its reason.
- Progress is measured by completed user journeys and reliability gates, not feature count.

## Change history

### 2026-07-29 — revision 1.5

Corrected the H1 critical path after the credential-storage milestone review.

- Found that H1's provider-driven repository-task exit gate had no implementation item beyond manually invoked workspace tools.
- Added H1-008 as a P0 provider-driven task loop with normalized tool calls, central policy routing, causal results, and a cross-provider read-edit-review fixture.
- Promoted H1-007 diagnostics to P0, made secure credential profiles a dependency, and kept diagnostics ahead of the task loop.
- Resolved DEC-002 by pinning first-party cloud profiles to official origins and requiring origin-bound compatible profiles plus an explicit destination-change warning.
- Added RISK-008 so H1 cannot be declared complete until the genuine provider-driven user journey passes.

### 2026-07-29 — revision 1.4

Completed OS credential storage and centralized secret redaction.

- Added provider-bound opaque credential profiles backed by Windows Credential Manager and the Linux Secret Service, with serialized blocking access and replacement cleanup.
- Removed raw credentials from normal frontend test and chat payloads, resolved references only inside the trusted Rust desktop boundary, and restored stored profile references at startup.
- Made Rust secrets non-serializable and zeroizing on drop, while documenting the limits of JavaScript and upstream-library memory handling.
- Applied one dynamic and structural redactor to provider errors and the SQLite persistence gate, with tests covering diagnostics, exports, crash text, injected raw credentials, and cross-provider reference misuse.
- Completed H1-006, mitigated the credential-leak risk, accepted ADR 0008, and moved provider diagnostics and capability discovery to the active focus.

### 2026-07-29 — revision 1.3

Completed native-confirmed file editing and real diff review.

- Added typed read hashes and bounded write_file requests with required optimistic concurrency for existing UTF-8 files.
- Bound allow-once grants to the approved origin and exact resource, blocked Git metadata and symbolic-link replacement, and required a Rust-owned native confirmation.
- Implemented synchronized same-directory temporary writes with atomic Windows and Unix replacement plus cancellation before the filesystem transition.
- Recorded approval, safe tool summaries, and diff artifact metadata in causal order while keeping full diffs transient, and added a real desktop edit and diff-review surface.
- Completed H1-005 and moved credential storage and provider diagnostics to the active focus.

### 2026-07-29 — revision 1.2

Closed the cross-platform foundation gates and protected the public main branch.

- Forced byte-checked generated artifacts to stable LF checkout semantics and made the SQLite projection snapshot compare structured JSON instead of platform line endings.
- Passed the complete hosted quality workflow on Windows and Ubuntu, including generated fixtures, web and desktop builds, formatting, Clippy, and all Rust tests.
- Protected main behind pull requests, current-branch checks, resolved review conversations, and force-push and deletion prevention.
- Marked the reproducible desktop build and Windows/Linux CI items complete, closed their platform risks, and moved the active roadmap horizon to H1.

### 2026-07-28 — revision 1.0

Completed real Git project selection and removed the desktop dependency-install blocker.

- Added the Tauri-free kiln-workspace crate with canonical-root discovery, path-derived identity, branch and commit projection, porcelain-v2 status parsing, ownership checks, disabled hooks and filesystem monitors, bounded output, and a 15-second inspection limit.
- Persisted project_opened and workspace_ready events before activation, derived recent projects from immutable SQLite history, and revalidated remembered roots at startup without storing credentials or remote URLs.
- Added typed Rust and TypeScript project projections plus a desktop picker that shows branch, truthful working-tree state, recent availability, and actionable invalid-selection errors while blocking turns without a repository.
- Generated desktop/package-lock.json, aligned the official Svelte Vite plugin with Vite 8, migrated production minification to Oxc, and passed a clean Windows npm ci, Svelte check, and desktop build.
- Added real temporary-Git, storage, transport, projection, and credential-shape tests plus the project/workspace guide; read-only file and search tools are next.

### 2026-07-28 — revision 0.9

Completed normalized provider streaming and cancellation from HTTP frames to durable desktop projection.

- Added OpenAI Responses, Anthropic Messages, and compatible local SSE parsers with normalized delta, completion, and cancellation events.
- Added a shared cancellation domain that drops active HTTP and job futures and gives cancellation priority over queued late frames.
- Streamed normalized events through a typed Tauri channel, converted them to application events, and committed each batch before Svelte projection.
- Added a desktop stop control, duplicate-turn protection, terminal channel handling, and late-mutation guards in Rust and TypeScript.
- Added protocol, framing, cancellation, registry, bridge, and projection tests plus the streaming guide and ADR 0005; repository selection is next.

### 2026-07-28 — revision 0.8

Completed the central permission engine before introducing repository tools.

- Added typed tool, command, path, network-host, and extension resources with task, project, provider-profile, and global policy targets.
- Defined deterministic target, origin, resource, and deny/ask/allow precedence with an ask-by-default fallback.
- Kept allow-once grants outside serializable rules and consumed them at the guarded execution boundary.
- Proved denied native and extension-origin actions invoke no side-effect closure, documented the trust boundary, and accepted ADR 0004.
- Started H1-001 streaming and cancellation while H0 desktop dependency and hosted CI gates remain open.

### 2026-07-28 — revision 0.7

Completed the recorded-session integration gate and established the Windows/Linux quality workflow.

- Added one canonical 25-event fake session covering streaming, planning, approval, tool output, a diff, tests, and a final receipt.
- Generated the Svelte recording from JSON and verified deterministic task and inspector projections against a Rust snapshot.
- Added a Windows and Ubuntu GitHub Actions matrix with web, Svelte, Rust, freshness, lint, and build gates.
- Added executable fixtures for spaces, Unicode paths, mixed line endings, and process cancellation; H0-010 remains active until hosted runs and required checks are confirmed.
- Marked H0-004 blocked because the approval service still rejects the registry request needed to create the desktop lockfile.

### 2026-07-28 — revision 0.6

Completed restart-safe SQLite integration from Tauri startup through the Svelte projection.

- Initialized kiln.db under the Tauri application-data directory and exposed typed append/load commands.
- Added a durable desktop history coordinator that commits event batches before projection and resets its sequencer after failed writes.
- Restored the ordered task stream and deterministic projection before enabling the desktop composer.
- Added durability coordinator tests and marked H0-008 complete; recorded fake-session replay and CI are next.

### 2026-07-28 — revision 0.5

Started durable SQLite event storage behind a Tauri-free crate.

- Added storage schema version 1 with indexed immutable event envelopes and explicit migration history.
- Implemented one-stream transactional append, durable-tail sequence checks, and validated replay.
- Added persistence-time credential-marker rejection plus atomicity, rollback, migration, file reopen, and projection-snapshot tests.
- Documented the physical schema, recovery rules, safety boundary, and ADR 0003; H0-008 remains in progress for desktop append/startup integration.

### 2026-07-28 — revision 0.4

Completed the versioned application-event boundary from Rust through the Svelte projection.

- Added message lifecycle events and stable camel-case transport fields to the Rust contract.
- Replaced direct Svelte conversation and activity mutation with deterministic ordered-event projections.
- Converted provider success and failure results into message and receipt events at the desktop bridge.
- Added cross-language contract checks, replay tests, complete contract documentation, and ADR 0002.

### 2026-07-28 — revision 0.3

Extracted the Tauri-free Rust foundation and started the versioned application event contract.

- Created a Cargo workspace with kiln-core, kiln-providers, kiln-platform, and a thin kiln-tauri shell.
- Moved provider fixtures and normalized contracts out of the desktop transport.
- Added versioned command and event envelopes with causal stream ordering and forward-compatible field handling.
- Marked H0-006 done and H0-007 in progress after workspace and isolated-core tests passed.

### 2026-07-28 — revision 0.2

Converted the roadmap into a generated source of truth and incorporated the foundation review.

- Prioritized a reproducible desktop build, Rust core extraction, event contract, SQLite replay, and CI.
- Required permissions before repository tools and streaming before ecosystem integrations.
- Added risk, decision, metric, and change-history records.
- Synchronized web and desktop roadmap summaries from the structured source.

### 2026-07-28 — revision 0.1

Established the initial H0–H7 product roadmap.

- Set Windows and Linux as launch platforms.
- Deferred macOS support and secure remote operation until release gates are met.
