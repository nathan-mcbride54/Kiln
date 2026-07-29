# ADR 0004: One guarded permission engine for every action

- **Status:** Accepted
- **Date:** 2026-07-28
- **Roadmap:** H1-004

## Context

Kiln will accept actions from native provider loops, MCP tools, ACP agents, and
future headless clients. Separate permission implementations would create
inconsistent prompts and let an extension choose a weaker execution path.
Worktree isolation does not prevent filesystem, process, credential, or
network side effects.

## Decision

Use one transport-independent permission engine in `kiln-core`.

- Every action has a trusted origin, typed named resource, rationale, and
  unique action ID.
- Rules evaluate to `allow`, `ask`, or `deny` in task, project,
  provider-profile, or global context.
- Specificity and tie precedence are deterministic and documented.
- No matching policy defaults to `ask`.
- Allow-once grants remain ephemeral and are consumed by one execution attempt.
- Side effects are supplied as a closure to the guarded executor and cannot run
  for `ask`, `deny`, or invalid input.
- Extension adapters assign extension origins before evaluation; extensions do
  not receive an alternate executor.

## Consequences

Positive:

- Native and extension tools share one auditable policy language.
- Denied actions have a directly testable no-side-effect boundary.
- Allow-once cannot be serialized accidentally with durable rules.
- The engine is reusable by Tauri, CLI, and future headless transports.

Costs:

- Workspace and platform adapters must produce canonical resource names.
- Every new side-effecting subsystem must integrate with the guarded executor.
- Durable session/project grants require explicit event and storage work.
- A single engine becomes security-critical and needs adversarial fixtures.

## Rejected alternatives

### Permission checks inside each tool

This duplicates precedence and persistence logic and makes bypasses difficult
to audit.

### Trust extensions to enforce their own policy

An extension may be buggy or hostile and cannot define Kiln's user-facing
security boundary.

### Treat worktrees as containment

Worktrees separate Git changes but do not constrain processes, network access,
credentials, or paths outside the checkout.
