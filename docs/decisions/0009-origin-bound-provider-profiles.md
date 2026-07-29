# ADR 0009: Origin-bound provider profiles

- Status: accepted
- Date: 2026-07-29
- Implemented: H1-007, revision 1.6
- Review at: H3 provider-profile expansion

## Context

Kiln now resolves provider-bound credential references inside Rust, but a
provider category alone is not a complete credential destination. In
particular, a stored token for a local compatible server could otherwise follow
a later base-URL change without a new security decision. H1-007 also needs a
clear policy before it expands provider diagnostics or gateway support.

## Decision

Keep the first-party OpenAI and Anthropic profiles pinned to their official
origins. Do not add arbitrary base URLs to those profiles.

Represent a custom compatible gateway as a separate profile whose credential
reference is bound to the exact normalized origin. Changing that origin makes
the prior binding unusable until the user sees the destination change and
explicitly rebinds or saves a credential for the new origin.

The normalized origin is the URL scheme, host, and effective non-default port;
the API path is not part of the credential destination. Reject credential
transport over cleartext non-loopback HTTP and never follow a credential-bearing
redirect. Compatible endpoints on the literal loopback interface or
`localhost` remain available for local development.

## Consequences

Provider flexibility remains available without making credential routing
ambiguous. The current H1 store keeps one active profile per provider. Its
version-2 alias records both the opaque reference and normalized origin, and
resolution refuses a different destination before loading the secret. A local
endpoint edit shows the old and new origins and requires a new save.

Legacy first-party aliases are usable only at their pinned official origin.
Legacy local aliases remain visible and removable but are marked
`rebind_required` and cannot be resolved until the user saves a credential for
an explicit destination.

Native Anthropic-compatible gateways require a dedicated compatible profile
rather than masquerading as first-party Anthropic.

This policy deliberately favors a visible extra setup step over silently
following endpoint edits. Broader gateway templates can be reconsidered during
H3 provider-profile expansion without weakening the destination binding.

The implemented transport and diagnostic controls are specified in
[Provider diagnostics](../PROVIDER_DIAGNOSTICS.md).
