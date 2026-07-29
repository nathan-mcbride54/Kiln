# Kiln permission engine

**Status:** Implemented core contract
**Roadmap:** H1-004
**Canonical implementation:** `crates/kiln-core/src/policy.rs`

## Trust boundary

All future provider, filesystem, process, network, and extension actions must
enter the same Rust permission engine before their side-effecting operation is
constructed. The Svelte interface displays decisions and submits approvals; it
does not authorize work.

Extensions are out-of-process and cannot submit a trusted origin value
directly. Their adapter creates an `ActionOrigin::Extension` proposal, and the
guarded executor applies both extension-origin and resource rules.

## Named resources

Version 1 evaluates these typed resource families:

| Resource | Stable name |
|---|---|
| Tool | Tool identifier such as `read_file` |
| Command | Resolved executable name |
| Path | Host-normalized path plus read, search, write, or execute operation |
| Network | Normalized host plus optional port |
| Extension | Extension identity plus capability |

Rules can target global, provider-profile, project, or task context and can
match every origin, the native core, or one extension.

## Decision order

Matching is deterministic:

1. task rules are more specific than project, provider-profile, and global
   rules;
2. an exact origin is more specific than any origin;
3. exact named resources are more specific than path prefixes and catch-all
   rules;
4. at equal specificity, `deny` wins over `ask`, which wins over `allow`.

No matching rule defaults to `ask`.

This allows a deliberate narrow exception to a broad default while preserving
deny precedence for equally specific rules. Changing this order is a contract
change and requires an ADR update plus fixture tests.

## Allow once

An allow-once grant is keyed to one action ID and exists only in the live
`PermissionEngine`. It is not part of the serializable rule list. The guarded
executor consumes the grant before the operation is attempted, even when that
operation later reports an error.

Restarting Kiln therefore cannot silently turn allow-once into a durable rule.
Session and project persistence will use explicit policy events in a later
slice.

## No-side-effect guarantee

Callers use `PermissionEngine::execute`. The operation closure is invoked only
for an allow decision. `ask`, `deny`, and invalid proposals return before the
closure runs.

Tests use a side-effect counter to prove denied native and extension-origin
actions never invoke their operation. H1-003 repository tools now use this
guarded entry point for both the named tool and canonical read/search path.

H1-005 keeps `write_file` and the exact canonical write path at `ask`. Rust
shows a native path-specific confirmation, then inserts allow-once grants bound
to the approved origin and resource. A caller cannot substitute another path
under the same action identifier, and the grant is consumed at execution.

## Current limit

The platform layer must resolve executables and canonicalize paths before
creating a proposal. The core deliberately does not guess OS path aliases,
symlink identity, or executable lookup. Those checks belong at the trusted
workspace host boundary.
