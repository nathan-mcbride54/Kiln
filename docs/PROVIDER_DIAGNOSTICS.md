# Provider diagnostics

Kiln reports provider behavior as five independent probes. A reachable endpoint
can therefore remain usable when model discovery, streaming, or structured tool
calls are unavailable, and an authentication failure does not masquerade as a
network failure.

## Probe contract

| Probe | What Kiln verifies |
|---|---|
| Reachability | The configured destination returns an HTTP response before the deadline. A redirect counts only as a response; Kiln does not follow it. |
| Authentication | The model-list request accepts the configured credential, explicitly rejects it with `401` or `403`, or leaves the result skipped when the response cannot isolate authentication. |
| Model discovery | A successful, bounded model-list response has a compatible shape and contains valid model identifiers. An absent route can report unsupported without making the whole endpoint unreachable. |
| Streaming | The selected model completes the protocol's expected streamed text event sequence. |
| Tool compatibility | The selected model emits one complete, correctly named structured call with the required synthetic arguments. |

Each slot is `passed`, `failed`, `unsupported`, or `skipped` and may include
latency and HTTP status. The aggregate is:

- `unavailable` when reachability or authentication fails;
- `ready` only when every probe passes; or
- `degraded` for every partially verified or partially compatible result.

`unsupported` is distinct from `failed`: it says that the adapter or endpoint
does not expose the tested behavior, not that a previously supported behavior
malfunctioned.

## Basic and model verification

A basic connection test makes one authenticated `GET` request to the provider's
model-list route. It does not ask a model to generate content. Kiln derives the
reachability, authentication, and model-discovery slots from that one response
and leaves streaming and tool compatibility skipped. If that endpoint cannot
isolate authentication, a successful explicit model probe can later establish
that the configured credential was accepted.

Model verification is explicit. Once the user supplies a model, Kiln makes two
small inference requests:

1. a fixed prompt requesting only `OK`, to verify streamed generation; and
2. a fixed prompt forcing `kiln_capability_probe` with `{ "value": "ok" }`, to
   verify the provider's structured tool-call stream.

These requests can consume provider tokens, incur the provider's normal charge,
or load and warm a local model. Diagnostics never include repository content,
task history, user prompts, or tool results. The synthetic tool is a no-op
schema used only for response validation; Kiln never executes the emitted call.
OpenAI diagnostic requests also set `store: false`.

The hosted preview exposes basic testing and model verification as separate
actions. The native contract follows the same boundary by omitting or supplying
the optional model.

## Credential destination rules

First-party cloud profiles are not generic gateways:

| Profile | Fixed base URL | Credential origin |
|---|---|---|
| OpenAI | `https://api.openai.com/v1` | `https://api.openai.com` |
| Anthropic | `https://api.anthropic.com/v1` | `https://api.anthropic.com` |

The compatible profile accepts a custom HTTP(S) base URL. Kiln derives one
canonical origin from its scheme, host, and effective non-default port. Paths
do not change the credential identity; user information, query strings,
fragments, non-HTTP schemes, and missing hosts are invalid.

A stored compatible credential resolves only when the requested normalized
origin exactly matches its binding. When the destination changes, the desktop
shows both origins and refuses to send the old credential until the user saves
one for the new origin. Credential-bearing requests require HTTPS unless the
host is the literal loopback interface or `localhost`.

H1 keeps one active credential profile per provider. Legacy cloud profiles are
limited to their fixed official origin. A legacy compatible profile is marked
`rebind_required`; it remains removable but cannot be used until the user
explicitly binds a new credential.

## Network and parsing bounds

Diagnostics use deliberately narrow transport behavior:

- redirects are disabled for every provider request, so authorization and
  custom headers cannot be forwarded to an unapproved destination;
- a diagnostic attempt is sent once and is never retried automatically;
- model discovery has a 15-second request deadline;
- native inference probes have a 30-second deadline, while the hosted preview
  allows 45 seconds for the same bounded checks;
- model-list and stream bodies are capped at 256 KiB;
- at most 100 discovered model identifiers are returned, and identifiers are
  bounded to 200 characters; and
- user-facing failures are locally worded and do not echo upstream bodies or
  response headers.

A timeout or bound failure affects its own probe rather than manufacturing
capability evidence for the others.

## What H1-007 does not prove

A passing tool-compatibility probe proves only that one selected model emitted
the required structured call shape. It does not prove that Kiln can execute a
provider-driven task, route a proposed tool through policy, return a causal
result, or stop denied and cancelled calls without side effects.

Those end-to-end guarantees belong to H1-008. Its cross-provider
read-edit-review fixture must exercise the real orchestration loop through
OpenAI, Anthropic, and a compatible local endpoint before the H1 exit gate can
close.

## Implementation and validation

- `kiln-core` owns the normalized probe, status, overall result, capability,
  and origin types.
- `kiln-providers` owns destination validation, bounded requests, and
  protocol-specific stream evidence.
- `kiln-platform` owns versioned origin-bound aliases in the OS credential
  service.
- The Svelte desktop consumes the Rust capability and diagnostic contracts.
  The hosted React preview mirrors the five-probe experience without persisting
  supplied credentials.

Unit and integration tests cover origin normalization, fixed cloud
destinations, legacy migration, destination mismatch, redirect refusal,
bounded model parsing, and complete versus malformed stream/tool events for all
three protocols.
