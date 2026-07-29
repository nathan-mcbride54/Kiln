# Provider tool turns

**Status:** H1-008 codec and core orchestration implemented; live integration pending
**Canonical repository schemas:** `kiln-core::repository_tool_definitions`
**Transient provider boundary:** `kiln-providers::ToolTurnCodec`

## Purpose

Kiln must accept structured repository proposals from OpenAI Responses,
Anthropic Messages, and compatible Chat Completions without allowing any
provider protocol to become the application event contract. The tool-turn
codec is the narrow boundary between untrusted provider bytes and Kiln's
typed repository requests.

The codec does not execute tools. It emits transient tool calls that are parsed
into the four allowlisted `RepositoryToolRequest` variants and routed through
the workspace permission boundary by `kiln-orchestrator`.

## Stable repository catalog

`kiln-core` owns one strict JSON schema for each repository tool:

- `read_file`
- `search_files`
- `search_text`
- `write_file`

Unknown tool names, unknown fields, non-object arguments, control characters,
invalid limits, invalid hashes, and oversized arguments fail locally before
policy evaluation. Provider adapters render that same catalog into each
protocol's required envelope.

## Stream normalization

`ToolTurnCodec` accepts one provider SSE `data:` value at a time and
normalizes fragmented calls in provider order:

| Protocol | Stream forms |
|---|---|
| OpenAI Responses | output-item start, argument deltas/final arguments, response completion |
| Anthropic Messages | tool-use block start, input JSON deltas, block stop, message stop |
| Compatible Chat Completions | indexed tool-call deltas and terminal finish reason |

A tool-only response is valid when it contains one or more complete calls and
a valid terminal event; it does not need artificial assistant text.

## Transient handle boundary

Provider call IDs are held by `ProviderToolCallHandle`. The handle and raw
arguments deliberately implement no serialization contract, and debug output
redacts the handle and reports only argument size. They cannot enter Tauri IPC,
SQLite events, crash reports, or exports through the typed application
contract.

The orchestrator retains the opaque handle only long enough to return a typed
success or failure for the next provider step. It mints its own internal tool
and approval IDs for durable causality.

## Continuations

`RepositoryToolOutcome` represents success or a stable failure code such as
`denied`, `approval_declined`, `cancelled`, `invalid_request`, `conflict`, or
`execution_failed`. `kiln-providers` encodes that outcome as:

- an OpenAI `function_call_output`;
- an Anthropic `tool_result`; or
- a compatible `tool` message.

Provider-native continuation payloads remain inside `kiln-providers`.

## Bounds and failure rules

- at most 16 tool calls per turn;
- at most 320 KiB of JSON arguments per call;
- at most 2 MiB of decoded tool-turn event data;
- tool names and provider handles are bounded and reject control characters;
- missing, duplicate, oversized, malformed, ambiguous, and out-of-order calls
  fail before execution; and
- completion with unfinished calls is invalid.

Provider diagnostics now use the same tool-call accumulator as production
decoding. A diagnostic pass therefore proves the selected model can satisfy
the shared stream grammar, while still executing no repository action.

## Rust-owned task loop

`kiln-orchestrator` now owns task identity, causality, persistence order,
policy preflight, approval pauses, cancellation, budgets, workspace execution,
transient provider continuations, usage aggregation, and the single terminal
receipt. Its recorded fixtures run real read-edit-review work across all three
provider protocols and prove denied and cancelled writes have no side effect.

See [Rust task orchestration](ORCHESTRATION.md) and
[ADR 0011](decisions/0011-durable-rust-task-loop.md).

## Remaining H1-008 work

The next slice implements real provider HTTP sessions on the orchestrator
trait, resolves credentials at the trusted boundary, exposes a thin Tauri
command, and changes Svelte to project the persisted stream instead of
synthesizing live tool lifecycle events.
