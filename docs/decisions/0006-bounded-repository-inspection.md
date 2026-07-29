# ADR 0006: Bound repository inspection at the workspace host

- **Status:** Accepted
- **Date:** 2026-07-28
- **Roadmap:** H1-003

## Context

Repository inspection must work across provider types without giving a model an
unrestricted path or process primitive. File content is also more sensitive
than ordinary application history: persisting every read or search match would
quietly copy source code and possible secrets into the event database.

Search needs predictable behavior on Windows and Linux, including ignored
files, large workspaces, unusual paths, cancellation, and symlinks.

## Decision

Kiln defines tagged read-file, file-search, and text-search schemas in
`kiln-core`. `kiln-workspace` implements them against a registered canonical Git
root.

Every operation passes both a named-tool proposal and a canonical-path proposal
through the central permission engine. Requests accept only workspace-relative
paths, canonical targets must remain under the root, and searches do not follow
symlinks.

File enumeration uses a bounded, noninteractive `git ls-files` invocation so
tracked and untracked files are searchable while Git-ignored files and
repository internals remain excluded. File reading and text matching happen in
Rust with explicit byte, line, result, and cancellation bounds.

Typed raw results are transient. The immutable activity stream stores the
ordered proposal, start, bounded aggregate result summary, and completion—not
file contents, queries, or matching previews.

## Consequences

- Providers, desktop commands, and future headless transports can share one
  stable repository-tool contract.
- Path traversal, absolute paths, and escaping symlinks fail before content is
  read.
- Denied operations cannot reach Git or the filesystem.
- Activity remains truthful without turning the event database into an
  accidental source-code or secret archive.
- Ignored files are unavailable to search by default. A future explicit
  sensitive-file workflow must add its own policy and retention design.
- The first text search is intentionally bounded and literal. Regex,
  ignore-override, and indexed search can be additive future tools.
