# ADR 0009: Origin-bound provider profiles

- Status: accepted
- Date: 2026-07-29
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

## Consequences

Provider flexibility remains available without making credential routing
ambiguous. The current `local` category remains useful for compatible servers,
but H1-007 must replace its provider-only credential binding with an
origin-bound profile before claiming the gateway warning acceptance criterion.
Native Anthropic-compatible gateways require a dedicated compatible profile
rather than masquerading as first-party Anthropic.

This policy deliberately favors a visible extra setup step over silently
following endpoint edits. Broader gateway templates can be reconsidered during
H3 provider-profile expansion without weakening the destination binding.
