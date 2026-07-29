# Git projects and direct workspaces

**Status:** Implemented direct-workspace slice
**Roadmap:** H1-002
**Canonical inspector:** `crates/kiln-workspace`

## Purpose

Kiln opens a real Git working tree before a task can start. The selected folder
is resolved to the repository root and projected into a provider-independent
snapshot containing:

- a path-derived project identity and display name;
- the canonical repository root;
- the current branch or an explicit detached-HEAD state;
- the current commit when one exists;
- staged, modified, untracked, conflicted, ahead, and behind counts;
- safe project defaults for provider, model, and verification profile.

Selecting a nested folder resolves to the same root and identity. Worktrees and
per-task branches remain H2; H1 uses the direct checkout and marks it as
non-isolated.

## Safety boundary

Repository discovery is Tauri-free and invokes built-in, read-only Git
commands. Kiln:

- requires an absolute, existing directory;
- keeps Git ownership and `safe.directory` checks enabled;
- rejects bare repositories and entire filesystem roots;
- disables repository hooks, filesystem monitors, terminal prompts, pagers,
  color output, and optional Git locks during inspection;
- stops inspection after 15 seconds and bounds captured Git output;
- never reads a remote URL into the project model;
- returns curated error messages rather than raw Git stderr;
- rejects paths that cannot be represented safely by the desktop contract.

The selected repository is not a sandbox. H1-003 read-only tools constrain
every file operation to the canonical workspace root and the central permission
engine. Later writes and shell execution still need their own permission and
containment boundaries.

## Remembering and recovery

Opening a repository appends `project_opened` and `workspace_ready` events to
the immutable `project:<id>` stream before the interface treats it as active.
The recent-project view is derived from the latest `project_opened` event per
stream; there is no mutable recent-project side table.

At startup, Kiln re-inspects each recent root. Available entries receive fresh
branch and status data. Missing, inaccessible, non-Git, bare, or ownership-
rejected entries remain visible with an actionable reason and can be relocated
through the project picker.

Remembered project events have no credential-shaped fields. Provider defaults
store only a provider identifier and model name. API keys, headers, tokens,
remote URLs, and process environments are not part of the project contract and
the SQLite persistence gate still rejects secret markers.

## Identity limits

The current project identity hashes the canonical root path. It is stable when
the same repository or one of its nested folders is reopened at that location,
without requiring Kiln to write an identifier into the repository or retain a
possibly credential-bearing remote URL. Moving the repository creates a new
identity until an explicit relocation contract is introduced.

## Validation

```powershell
cargo test -p kiln-core -p kiln-storage -p kiln-workspace -p kiln-tauri --locked
npm run desktop:contract-check
node --test tests/desktop-events.test.ts
npm run check --prefix desktop
npm run build --prefix desktop
```

The workspace tests create real temporary Git repositories and cover nested
selection, stable path identity, branch projection, staged/modified/untracked
status, invalid paths, non-repositories, and missing Git. Storage and transport
tests prove recent projects are event-derived and contain no credential fields.
