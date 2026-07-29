# ADR 0007: Native-confirmed atomic workspace editing

- Status: accepted
- Date: 2026-07-29
- Review at: H2 worktree isolation

## Context

H1 needs real file changes without introducing a general shell, trusting a
frontend-supplied approval flag, or allowing stale reads to overwrite newer
user work. A failed or cancelled write must not leave a partially truncated
file, and every successful change must be reviewable.

## Decision

Use a typed whole-file replacement tool with:

- a required SHA-256 precondition for existing files;
- central `ask` policies for the `write_file` tool and exact write path;
- allow-once grants bound to the approved origin and resource;
- a native Rust-owned confirmation dialog that names the path;
- same-directory synchronized temporary files and atomic replacement; and
- durable approval, activity, and diff artifact metadata, with the full diff
  kept transient.

## Consequences

This is intentionally less expressive than arbitrary patch or shell execution.
Large files, binary files, symbolic links, missing parent directories, and Git
metadata are not editable. Whole-file replacement keeps the contract portable
and deterministic now. Keeping raw diffs out of task events prevents edited
credentials from bypassing the durable-storage secret boundary. Later worktree
isolation may add multi-file
transactions, deletion, rename, and targeted restore without weakening this
approval boundary.
