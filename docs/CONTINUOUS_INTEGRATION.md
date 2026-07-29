# Kiln continuous integration

**Status:** Implemented locally; first hosted Windows/Linux run pending
**Workflow:** `.github/workflows/ci.yml`

## Purpose

Kiln treats Windows and Linux support as a tested product contract. Every pull
request and push to `main` runs the same release-blocking quality job on
`windows-latest` and `ubuntu-22.04`.

The Linux baseline is intentionally explicit. Ubuntu 22.04 provides the
WebKitGTK 4.1 packages required by Tauri 2 while keeping the generated binary
compatible with a useful range of distributions.

## Required checks

Each operating system proves:

1. the roadmap and recorded-session generated files are fresh;
2. TypeScript contracts, browser preview tests, and platform fixtures pass;
3. the web preview builds and the repository lints cleanly;
4. the Svelte application type-checks and builds from its lockfile;
5. Rust formatting and strict Clippy checks pass;
6. every Rust crate, including the Tauri shell, compiles and tests.

The platform fixtures deliberately exercise:

- a temporary path containing spaces and Unicode characters;
- exact CRLF and LF round trips plus explicit normalization;
- cancellation of a long-running child process within five seconds.

These small tests catch common cross-platform assumptions before real workspace
and process tools arrive in H1.

## Merge enforcement

After the workflow has completed once in the hosted repository, configure the
default-branch ruleset to require both unique checks:

- `Quality (windows-latest)`
- `Quality (ubuntu-22.04)`

GitHub can only select a required check after it has reported in that
repository. Until those first hosted runs and the ruleset are confirmed,
H0-010 remains `in_progress`.

## Local equivalent

Run the following before handing off a change:

```powershell
npm ci
npm ci --prefix desktop
npm test
npm run lint
npm run check --prefix desktop
npm run build --prefix desktop
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --all-targets --locked
```

Linux additionally needs the Tauri 2 system prerequisites listed in the
workflow. Windows uses the WebView2 environment supplied by the hosted runner.
