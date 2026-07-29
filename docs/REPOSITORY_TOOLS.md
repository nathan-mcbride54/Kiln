# Read-only repository tools

**Status:** Implemented bounded inspection slice
**Roadmap:** H1-003
**Canonical schemas:** `crates/kiln-core/src/tools.rs`
**Workspace host:** `crates/kiln-workspace/src/tools.rs`

## Tool contract

Kiln exposes three provider-independent, tagged request/result pairs:

| Tool | Typed input | Typed result |
|---|---|---|
| `read_file` | Workspace-relative path, first line, line count | UTF-8 content, returned range, truncation marker |
| `search_files` | Path pattern and result limit | Matching Git workspace paths and truncation marker |
| `search_text` | Query, optional workspace-relative scope, case mode, result limit | File, line, column, preview, scanned-file count, truncation marker |

Defaults and hard maximums are part of the Rust contract. The TypeScript mirror
is checked with the rest of the desktop transport surface.

## Workspace containment

The selected repository's canonical root is registered after Git inspection.
Every requested scope must be relative to that root. Kiln rejects absolute
paths, parent traversal, missing paths, and any canonical target outside the
workspace. Symlinks are never followed by text search; a file read can follow a
symlink only when its canonical target remains inside the workspace.

File and text search enumerate Git's tracked and untracked, non-ignored files
with built-in `git ls-files`. This avoids searching `.git`, ignored build
outputs, dependency trees, or secret files that the repository deliberately
ignores. Git execution retains the repository inspector's disabled hooks,
prompts, optional locks, pagers, color, and filesystem monitors.

Reads and searches are bounded:

- file reads accept at most 1,000 lines and return at most 256 KiB;
- one readable file can be at most 16 MiB;
- text search scans files no larger than 1 MiB and at most 64 MiB total;
- search results default to 100 and cannot exceed 500;
- binary and non-UTF-8 files are not exposed as text;
- Git file enumeration retains the 15-second timeout and bounded output;
- cancellation is checked before and throughout file and text processing.

## Permission boundary

The workspace host creates project-scoped rules for the three read-only tool
names plus `read` and `search` path operations under the canonical workspace
root. Each call first executes the named tool proposal through
`PermissionEngine::execute`, then executes the filesystem or Git operation
through a second guarded canonical-path proposal.

A deny or unresolved approval therefore reaches neither the file operation nor
the Git file enumeration. The desktop cannot construct an unrestricted
filesystem operation around this boundary.

## Visibility and data retention

The H1-008 Rust orchestrator persists `tool_proposed`, any approval transition,
and `tool_started` before it invokes the workspace service. It then persists a
bounded `tool_output` summary and `tool_completed` result in order. These
events project into the Activity panel, including failures. Write results also
publish diff metadata while the full diff stays transient.

The live desktop manual-tool path still assembles equivalent events in Svelte
until the real provider task command is connected.

Raw file contents, text-search queries, and matching previews remain transient
typed tool results. They are not copied into the immutable application event
log. Durable summaries contain only the tool name, safe path/range metadata,
and aggregate result counts, preserving the event log's secret-minimization
contract ahead of H1-006's central redaction pipeline.

## Validation

```powershell
cargo test -p kiln-core -p kiln-workspace -p kiln-tauri --offline --locked
cargo clippy -p kiln-core -p kiln-workspace -p kiln-tauri --all-targets --offline --locked -- -D warnings
npm run desktop:contract-check
node --test tests/desktop-events.test.ts
npm run check --prefix desktop
npm run build --prefix desktop
```

Tests cover typed serialization, hard request bounds, path traversal, policy
denial, cancellation, Git-ignore behavior, line ranges, file patterns, text
matches, safe durable summaries, and ordered Activity projection.
