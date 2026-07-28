# Kiln

Kiln is a local-first desktop workbench for directing coding agents across
OpenAI, Anthropic, and OpenAI-compatible local servers. It keeps the agent's
plan, tool activity, approvals, commands, diffs, and verification evidence in
one calm, inspectable surface.

This repository contains two complementary deliverables:

- `app/` — the deployable interactive product preview and provider onboarding
  experience.
- `desktop/` — the native Svelte + Tauri application foundation with a Rust
  provider layer.

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
npm install
npm run tauri dev
```

The desktop provider adapters use:

- OpenAI Responses API
- Anthropic Messages API
- OpenAI-compatible Chat Completions for a user-configured local endpoint

Credentials are accepted as ephemeral session values in the current
foundation. OS credential-store persistence is a gated roadmap item and must
land before production release.

## Product documentation

- [Product specification](docs/SPECIFICATION.md)
- [Architecture](docs/ARCHITECTURE.md)
- [Evolving roadmap](ROADMAP.md)
- [Local-first control-plane decision](docs/decisions/0001-local-first-control-plane.md)

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

cd desktop
npm run build
cargo test --manifest-path src-tauri/Cargo.toml
```

Kiln is currently an alpha foundation. It does not yet execute repository tools
or claim to sandbox an agent.
