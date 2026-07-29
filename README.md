# Kiln

Kiln is a local-first desktop workbench for directing coding agents across
OpenAI, Anthropic, and OpenAI-compatible local servers. It keeps the agent's
plan, tool activity, approvals, commands, diffs, and verification evidence in
one calm, inspectable surface.

This repository contains two complementary deliverables:

- `app/` — the deployable interactive product preview and provider onboarding
  experience.
- `desktop/` — the native Svelte + thin Tauri application shell.
- `crates/` — the Tauri-free Rust core, provider adapters, SQLite storage, Git
  workspace inspector, and platform boundaries shared by every future transport.

## Product principles

- Local-first ownership of projects, policies, credentials, and history.
- Provider freedom through capability-driven adapters.
- Visible agency from intent through tested diff.
- Safe autonomy with scoped `allow`, `ask`, and `deny` policies.
- Recoverable sessions and explicit checkpoints.
- Cross-platform behavior from the first implementation.

## Run the interactive preview

Requires Node.js 22.13 or later.

```powershell
npm install
npm run dev
```

Then open `http://localhost:3000`.

The preview can test real OpenAI and Anthropic credentials without persisting
them. Local-server requests travel directly from the browser to the configured
OpenAI-compatible endpoint, so that server must allow the preview origin.

## Run the desktop foundation

Prerequisites: Node.js, a stable Rust toolchain, and the platform requirements
listed by Tauri.

```powershell
cd desktop
npm ci
npm run tauri dev
```

The desktop provider adapters use:

- OpenAI Responses API
- Anthropic Messages API
- OpenAI-compatible Chat Completions for a user-configured local endpoint

Credentials are accepted as ephemeral session values in the current
foundation. OS credential-store persistence is a gated roadmap item and must
land before production release.

Task events are different from credentials: the desktop stores normalized,
redacted application events in `kiln.db` under the OS application-data
directory. Event batches commit before the Svelte projection changes and are
replayed when the application starts.

Before a task can start, the desktop opens and validates a real Git working
tree. Project identity, canonical root, branch, commit, status counts, and safe
provider/model defaults are remembered as immutable application events. API
keys and remote URLs are never part of remembered project metadata.

## Product documentation

- [Product specification](docs/SPECIFICATION.md)
- [Architecture](docs/ARCHITECTURE.md)
- [Application event contract](docs/APPLICATION_CONTRACT.md)
- [SQLite event storage](docs/STORAGE.md)
- [Recorded provider-free session](docs/RECORDED_SESSION.md)
- [Central permission engine](docs/PERMISSIONS.md)
- [Provider streaming and cancellation](docs/STREAMING.md)
- [Git projects and direct workspaces](docs/PROJECTS.md)
- [Repository inspection tools](docs/REPOSITORY_TOOLS.md)
- [Safe workspace editing](docs/SAFE_EDITING.md)
- [Windows/Linux continuous integration](docs/CONTINUOUS_INTEGRATION.md)
- [Evolving roadmap](ROADMAP.md)
- [Local-first control-plane decision](docs/decisions/0001-local-first-control-plane.md)
- [Versioned event-boundary decision](docs/decisions/0002-versioned-application-events.md)
- [SQLite event-log decision](docs/decisions/0003-sqlite-immutable-event-log.md)
- [Central permission-engine decision](docs/decisions/0004-central-permission-engine.md)
- [Ordered provider-streaming decision](docs/decisions/0005-ordered-provider-streaming.md)
- [Bounded repository-inspection decision](docs/decisions/0006-bounded-repository-inspection.md)
- [Native-confirmed atomic-editing decision](docs/decisions/0007-native-confirmed-atomic-editing.md)

The roadmap is generated from `product/roadmap.json`, which carries stable item
IDs, dependencies, acceptance gates, risks, decisions, and change history.

```powershell
npm run roadmap:render
npm run roadmap:check
npm run fixtures:render
npm run fixtures:check
```

Edit the structured source, render the outputs, and commit the source and
generated files together. The same render also refreshes the web and desktop
roadmap summaries.

## Platform status

| Platform | Status | Target |
|---|---|---|
| Windows | Foundation | First beta |
| Linux | Foundation | First beta |
| macOS | Architecture-compatible | After release-gate parity |

## Validation

```powershell
npm run build
npm test

cargo test --workspace --all-targets --offline

cd desktop
npm run check
npm run build
```

On Windows machines where Application Control blocks executables built inside
the repository, point Cargo at a trusted temporary build directory:

```powershell
$kilnTarget = Join-Path ([System.IO.Path]::GetTempPath()) "kiln-cargo-target"
$env:CARGO_TARGET_DIR = $kilnTarget
cargo test --workspace --all-targets --offline
```

Kiln is currently an alpha foundation. It executes bounded, policy-checked
repository reads, searches, and native-confirmed atomic UTF-8 edits. It does
not yet run general shell commands or claim to sandbox an agent.
