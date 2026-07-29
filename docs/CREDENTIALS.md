# Credential storage and redaction

Kiln keeps provider secrets outside application data. The desktop accepts a
secret only through `save_provider_credential`; normal connection tests and
chat commands carry an opaque `cred_<32 lowercase hex characters>` reference.
The trusted Rust transport resolves that reference immediately before provider
I/O and drops the resolved value afterward.

## Platform backends

| Platform | Backend |
|---|---|
| Windows | Windows Credential Manager |
| Linux | the desktop Secret Service over D-Bus, shared with libsecret clients |

`kiln-platform` owns this boundary. Calls run on a blocking worker and are
serialized because the platform APIs are synchronous and Windows credential
operations require deterministic ordering.

H1 keeps one active credential profile per provider. Each provider has one
versioned alias entry containing the opaque profile reference and its normalized
destination origin. A separate entry keyed by that reference contains the
provider secret. Listing profiles reads aliases only. Resolution checks both
the selected provider and the requested origin before loading the secret, so an
OpenAI reference cannot be attached to a local or Anthropic request and a local
credential cannot silently follow an endpoint edit.

The SQLite event log, remembered projects, Svelte state after saving, and
provider command payloads contain only the opaque reference. The desktop
bridge's browser-only fallback keeps reference-only profiles in memory and
discards the supplied secret after the simulated save. The hosted product
preview keeps a supplied key only in the current page session; it does not
persist that key or claim the desktop OS-vault boundary.

## In-memory handling

Rust secret values:

- do not implement serialization;
- redact their `Debug` representation;
- are cleared with `zeroize` when dropped;
- remain scoped to the save or provider request that needs them.

JavaScript strings cannot be reliably zeroized. The desktop therefore clears
the API-key input immediately after a successful native save and never sends a
raw key through connection-test or chat commands. A typed value can still
exist temporarily in the browser process and IPC machinery during the save
operation; the OS vault is the durable boundary, not a claim of secure browser
memory.

## Central redaction

`SensitiveDataRedactor` is the one text scrubber used at output boundaries. It
removes:

- exact resolved secrets and custom-header values;
- authorization, API-key, and cookie header values;
- common JSON credential fields;
- secret-bearing URL query parameters; and
- recognizable OpenAI and Anthropic token forms.

Provider errors are scrubbed before becoming `CommandError`. The SQLite
sensitive-data gate uses the same structured detection before an event can be
committed. Diagnostics, exports, and crash-report integrations must call this
redactor before emitting text; central tests cover all four output categories.

Redaction is defense in depth. New secret formats may not match a structural
pattern, so code must still avoid logging provider bodies, request headers, raw
tool output, or `SecretString::expose_secret()` values.

## Failure behavior

Credential-store errors cross the desktop boundary as stable, secret-free
messages. Backend error strings and entry contents are never forwarded.
Missing, provider-mismatched, destination-mismatched, and unbound legacy
references fail before any provider network request. Replacing a provider
credential writes the new profile and alias before deleting the previous
profile.

## Destination binding

OpenAI credentials are fixed to `https://api.openai.com`; Anthropic credentials
are fixed to `https://api.anthropic.com`. Those first-party profiles reject a
custom base URL in the trusted Rust provider boundary.

A compatible profile binds its credential reference to the exact normalized
origin: scheme, host, and effective non-default port. URL paths do not
participate in that identity, so `https://example.test/v1` and
`https://example.test/compatible/v1` share the origin
`https://example.test`. User information, query strings, fragments, non-HTTP
schemes, and missing hosts are rejected.

Editing a compatible endpoint to a different normalized origin makes the
stored reference unusable. The desktop shows the old and new destinations and
requires the user to save a credential for the new origin or remove the older
profile. Credential-bearing traffic also requires HTTPS unless the destination
is the literal loopback interface or `localhost`; redirects are never followed
with credentials.

The version-2 alias envelope records `{ v, credentialRef, origin }` within the
OS vault. A legacy OpenAI or Anthropic alias is interpreted only at its pinned
official origin. A legacy local alias remains visible and removable but enters
`rebind_required`; it cannot be resolved until the user saves a credential for
an explicit origin. See
[ADR 0009](decisions/0009-origin-bound-provider-profiles.md).

## Validation

The workspace tests prove:

- opaque save/list/resolve/delete behavior;
- cross-provider reference rejection and replacement cleanup;
- exact-origin resolution and destination-mismatch rejection;
- fixed first-party origins, compatible-origin normalization, and legacy
  `rebind_required` handling;
- raw credentials are ignored on request deserialization and excluded from
  request serialization;
- explicit zeroization and redacted debug output;
- dynamic provider-error scrubbing; and
- structured secret rejection before SQLite persistence;
- insecure non-loopback credential destinations are rejected; and
- credential-bearing diagnostics do not follow redirects.

Windows and Ubuntu CI compile and run the platform adapters on every pull
request. Live vault access is exercised by the desktop onboarding flow rather
than CI so automated jobs never create credentials in a user profile.

The five-part connection contract and its network bounds are documented in
[Provider diagnostics](PROVIDER_DIAGNOSTICS.md).
