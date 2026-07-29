# Provider tool turns

**Status:** H1-008 codec foundation implemented; orchestration loop pending
**Canonical repository schemas:** `kiln-core::repository_tool_definitions`
**Transient provider boundary:** `kiln-providers::ToolTurnCodec`

## Purpose

Kiln must accept structured repository proposals from OpenAI Responses,
Anthropic Messages, and compatible Chat Completions without allowing any
provider protocol to become the application event contract. The tool-turn
codec is the narrow boundary between untrusted provider bytes and Kiln's
typed repository requests.

The codec does not execute tools. It emits transient tool calls that must be
parsed into the four allowlisted `RepositoryToolRequest` variants and routed
through the workspace permission boundary by the H1-008 orchestrator.

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

The future orchestrator retains the opaque handle only long enough to encode a
typed success or failure for the next provider step. It mints its own internal
tool and approval IDs for durable causality.

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

## Remaining H1-008 work

The next slice is a Tauri-free Rust orchestrator that owns task identity,
causality, persistence order, policy evaluation, approvals, cancellation,
budgets, workspace execution, provider continuation, usage aggregation, and
the single terminal receipt. Svelte will project persisted events instead of
synthesizing live tool lifecycle events.
