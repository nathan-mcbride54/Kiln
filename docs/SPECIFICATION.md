# Kiln product specification

**Revision:** 0.1  
**Updated:** 2026-07-28  
**Stage:** Product foundation

## Vision

Kiln is a beautiful, local-first workspace for directing coding agents across
multiple repositories and models. It makes an agent's intent, activity,
permissions, code changes, and outcomes visible without requiring the user to
supervise raw terminal output.

> Open a repository, choose any configured model, describe an outcome, and
> safely review the complete journey from plan to tested diff.

Kiln begins as a personal desktop application. The core remains suitable for a
headless daemon and remote client later.

## Inspiration translated into product choices

| Project | What Kiln keeps |
|---|---|
| T3 Code | A task-centered multi-agent control surface, isolated worktrees, ordered domain events, checkpoints, and explicit completion receipts |
| OpenCode | Headless-capable sessions, fork/revert/abort, integrated diffs, granular permissions, and provider/model freedom |
| Goose | A Rust-native agent loop, MCP extensions, context revision, concurrent agents, and ACP interoperability |
| Rig | Provider-independent traits, typed tools, streaming, structured outputs, tracing, and deterministic adapter tests |

Kiln does not copy their interfaces. It combines the strongest architectural
ideas around a calmer, capability-driven desktop workflow.

## Product principles

1. **Local-first ownership.** Repositories, transcripts, policies, and
   credentials remain on the user's machine unless a selected provider requires
   data transfer.
2. **Provider freedom.** OpenAI, Anthropic, and local OpenAI-compatible servers
   are first-class peers.
3. **Visible agency.** Plans, calls, commands, approvals, failures, diffs, and
   checkpoints remain inspectable.
4. **Safe autonomy.** Permissions attach to tools and resources, not vague trust
   in a model vendor.
5. **Recoverable by design.** Sessions survive restart; user choices can be
   inspected and restored.
6. **Protocol over parsing.** Prefer typed APIs, ACP, and MCP to inferred
   terminal state.
7. **Capability-driven interface.** Controls follow advertised support rather
   than provider-name conditionals.
8. **Cross-platform first.** Paths, quoting, shells, processes, signals, and
   credentials have platform abstractions from the start.
9. **Calm density.** Show rich information with progressive disclosure.
10. **Core/interface separation.** Rust orchestration does not depend on Tauri
    or Svelte.

## Primary experience

### Workspace

- A project rail lists repositories, active tasks, status, and approvals.
- A task canvas combines conversation, plan, streaming answer, and grouped
  activity.
- An inspector switches among diff, file, plan, terminal, diagnostics,
  checkpoint, and roadmap views.
- A command palette exposes projects, providers, models, policies, sessions,
  and safe Git actions.
- A status bar shows model, context, latency, cost when available, branch,
  working state, and data destination.

### Interaction requirements

- Streaming must not cause disruptive scroll jumps.
- Routine activity is summarized but expands to exact inputs and outputs.
- Risky actions remain visually prominent and name the affected resource.
- Diff review supports file and hunk inspection.
- All core actions are keyboard accessible.
- Light, dark, reduced-motion, and compact-density modes are planned.
- No state is communicated by color alone.

## Launch workflows

### Projects and tasks

- Open and remember local Git repositories.
- Create an isolated branch and worktree per task.
- Offer direct-workspace mode only with a clear warning.
- Supervise multiple tasks without mixing state.
- Rename, archive, resume, fork, cancel, retry, and restore tasks.
- Distinguish blocked, awaiting approval, running, failed, and completed states.

### Agent turns

- Support chat, plan-only, approval, smart-approval, and autonomous modes.
- Stream text, structured plans, calls, command output, and diagnostics.
- Attach files and selected code ranges.
- Show context sources supplied to the model.
- Cancel active generation and execution promptly.
- Resume after application or provider-process restart.
- Compact long context behind an auditable summary boundary.

### Code and Git

- Search files and workspace symbols.
- Render syntax-aware files and unified or side-by-side diffs.
- Show the working tree and changed-file summary.
- Create per-turn checkpoints.
- Restore a checkpoint without rewriting unrelated user changes.
- Run explicit verification profiles.
- Prepare a commit; push, pull request, merge, and destructive cleanup remain
  explicit user actions.

## Permission model

Rules evaluate to `allow`, `ask`, or `deny` and may target:

- Tool category or individual tool.
- Command or executable pattern.
- File or directory path.
- Network host.
- MCP server or MCP tool.
- Task, project, provider profile, or global default.

Approval prompts show the action, exact scope, rationale, and persistence
choice. “Allow once” never becomes durable. External writes, destructive Git
actions, credential access, and expanded network access require explicit policy
coverage.

Git worktrees isolate changes; they are not described as a security sandbox.

## Provider contract

### Shared behavior

- Profiles are separate from model identities.
- Switching models preserves task history.
- Each adapter reports streaming, tool, image, structured-output,
  usage-reporting, and context capabilities.
- Normalized events retain raw provider metadata for diagnostics.
- Credentials use the OS credential service in production and never enter the
  event database or exported transcripts.
- Cancellation, timeout, health, and error classes are consistent across
  adapters.

### OpenAI

- Use the Responses API.
- Support streaming text, function calls, structured output, usage, and finish
  state normalization.
- Allow model discovery plus manual model entry.
- Support optional organization and project headers where applicable.

### Anthropic

- Use the Messages API with a pinned API version.
- Support streaming content blocks, tool use, usage, and stop-reason
  normalization.
- Allow model discovery plus manual model entry.

### Local OpenAI-compatible server

- Accept a base URL, model, optional bearer token, and custom headers.
- Offer loopback onboarding presets.
- Probe `/v1/models`, with manual fallback.
- Support streaming and non-streaming Chat Completions initially.
- Report reachability, authentication, model availability, streaming, and tool
  compatibility as separate diagnostics.
- Include compatibility toggles for reproducible server deviations.
- Always show whether code is sent to a device or a remote service.

## Extensibility

- MCP client support covers local stdio and remote HTTP servers.
- ACP client support connects Goose, OpenCode, Codex adapters, and compatible
  agents.
- A later ACP server lets editors and alternate clients use Kiln's native
  runtime.
- Native tools use typed Rust traits and JSON Schema inputs.
- Extensions cannot bypass the permission engine.

## Data and privacy

- Durable state is an append-only event log with rebuildable projections.
- Provider secrets are referenced by opaque credential IDs.
- Raw provider payload retention is configurable and redacted by default.
- Exports preview every included category and support content omission.
- Telemetry is off unless deliberately enabled.
- Local-provider traffic never traverses Kiln cloud infrastructure.

## Platform requirements

### Windows — launch

- Detect Windows PowerShell and `pwsh`; do not assume Bash.
- Handle quoting, drive-relative and UNC paths, symlinks, spaces, non-ASCII
  characters, and executable suffixes.
- Use ConPTY-compatible terminals and process-tree termination.
- Ship an installer, Start Menu entry, uninstaller, and signed update path.

### Linux — launch

- Select shells explicitly.
- Follow XDG config, cache, state, and data locations.
- Use PTYs and process-group cleanup correctly.
- Smoke-test Wayland and X11.
- Provide one portable package and selected native formats.

### macOS — later

- Keep the architecture continuously compilable where practical.
- Implement native PTY, shell, path, and Keychain behavior.
- Decide universal versus separate architecture packages.
- Pass signing, notarization, hardened-runtime, and updater gates before
  claiming support.

## Accessibility and performance budgets

- Core journeys meet WCAG 2.2 AA interaction expectations.
- Keyboard focus is always visible and follows logical reading order.
- Reduced motion removes nonessential transitions.
- The task canvas virtualizes or progressively renders long sessions.
- A 10,000-event session remains responsive; exact frame, memory, and query
  budgets are set with H2 fixture measurements.

## Explicit first-release non-goals

- Multi-user cloud collaboration.
- Hosting or reselling model access.
- Model training or fine-tuning.
- Replacing a full IDE.
- Automatic push, pull request, merge, or destructive Git cleanup.
- Claiming Git worktrees are a security boundary.
- Supporting untestable local-server quirks.
- A public extension marketplace before adapter contracts stabilize.

See [the evolving roadmap](../ROADMAP.md) for sequencing and acceptance gates.
