# Kiln desktop

The native Kiln foundation pairs a Svelte 5 interface with a Tauri 2 boundary
and Rust provider adapters.

## Run

Install the current Tauri platform prerequisites, then:

```powershell
npm install
npm run tauri dev
```

Use `npm run dev` for the browser-safe interface preview. In that mode, provider
actions return explicit fixture responses; no credential leaves the browser.

## Desktop commands

- `list_provider_capabilities`
- `test_connection`
- `send_chat_request`

The Svelte layer calls only these typed commands. Provider HTTP and secret
handling stay behind the Rust boundary.

## Current provider paths

- OpenAI: Responses API under `https://api.openai.com/v1`
- Anthropic: Messages API under `https://api.anthropic.com/v1`
- Local: OpenAI-compatible Chat Completions under a user-selected base URL

Keys are ephemeral request values in this foundation. Production persistence
must use an OS credential service; it must not place keys in Svelte storage,
SQLite, logs, or exported transcripts.

## Validation

```powershell
npm run check
npm run build
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml
```

Windows and Linux are the launch platforms. macOS remains a later release gate,
not a current support claim.
