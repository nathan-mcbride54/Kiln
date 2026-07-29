# Safe workspace editing

Kiln edits one UTF-8 file at a time through the typed `write_file` contract.
The contract is deliberately narrower than a general filesystem or shell tool.

## Edit lifecycle

1. `read_file` returns the complete bounded contents and a SHA-256 version.
2. `write_file` supplies the replacement contents and that expected version.
3. Rust canonicalizes the target or its existing parent, rejects traversal,
   symbolic-link replacement, missing parents, binary files, and `.git`.
4. The central permission engine evaluates the tool and exact absolute path as
   `ask`. An allow-once grant is bound to both the action identifier and the
   approved resource.
5. The desktop displays a native operating-system confirmation naming the
   relative path. Cancelling the dialog produces no filesystem side effect.
6. Kiln writes and synchronizes a same-directory temporary file, checks
   cancellation once more, and atomically replaces the target.
7. The result contains before/after hashes and a unified diff. The desktop
   displays the full diff transiently and records approval, a safe tool summary,
   and diff artifact metadata in causal order.

Existing files cannot be written without the hash from a prior read. If another
process changes the file in between, the edit fails and must be rebased on a new
read. New files require an existing workspace-contained parent.

## Bounds and non-goals

- Replacement contents are limited to 256 KiB of valid UTF-8.
- This phase does not delete or rename files.
- This phase does not stage or commit Git changes.
- Kiln does not edit through symbolic links or touch Git metadata.
- Raw replacement contents and full diffs never enter durable task events.
- The displayed diff is evidence of the exact before/after text, not permission
  to accept a task or merge a branch.

The atomic replacement is platform-specific: Unix uses `rename`, while Windows
uses `MoveFileExW` with replace-existing and write-through flags.
