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

Each provider has one alias entry whose value is an opaque profile reference.
A separate entry keyed by that reference contains the provider secret. Listing
profiles reads aliases only. Resolution checks that the reference belongs to
the selected provider before loading its secret, so an OpenAI reference cannot
be attached to a local or Anthropic request.

The SQLite event log, remembered projects, Svelte state after saving, and
provider command payloads contain only the opaque reference. The web preview
keeps reference-only profiles in memory and never retains the supplied secret.

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
Missing or provider-mismatched references fail before any provider network
request. Replacing a provider credential writes the new profile and alias
before deleting the previous profile.

## Validation

The workspace tests prove:

- opaque save/list/resolve/delete behavior;
- cross-provider reference rejection and replacement cleanup;
- raw credentials are ignored on request deserialization and excluded from
  request serialization;
- explicit zeroization and redacted debug output;
- dynamic provider-error scrubbing; and
- structured secret rejection before SQLite persistence.

Windows and Ubuntu CI compile and run the platform adapters on every pull
request. Live vault access is exercised by the desktop onboarding flow rather
than CI so automated jobs never create credentials in a user profile.
