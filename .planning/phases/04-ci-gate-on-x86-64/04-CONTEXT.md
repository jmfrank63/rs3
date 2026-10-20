# Phase 4: CI Gate on x86_64 - Context

**Gathered:** 2026-10-20
**Status:** Ready for planning
**Mode:** Smart discuss (autonomous, non-interactive — every grey area auto-accepted with the Recommended answer)

<domain>
## Phase Boundary

A single GitHub Actions workflow gates every push and pull request on an x86_64 Linux runner.
It runs `cargo fmt --check`, `cargo clippy --all-targets --locked -- -D warnings` and
`cargo test --locked`; any failure fails the build. The archived `actions-rs` workflow is
deleted, so the new workflow is the only file in `.github/workflows`. The workflow also wires
the repo-level guard checks the operator asked for: no lint suppression, plus the
Phase 3 test-tier prohibitions. It adds a release-binary smoke so the production V8
platform path is exercised on x86_64.

**Operator constraint (this autonomous run): do NOT `git push`.** Success criterion 1
("pushing … finishes green on an x86_64 runner") cannot be observed here. Implement the
workflow, validate it locally (YAML/structure lint, every command run locally), and record
the remote CI run as a **pending human UAT item**.

Not in this phase: aarch64 runner, cargo-deny, coverage (v2 OPS-03); Docker image build (Phase 6).

Requirements: CI-01, CI-02, CI-03.

</domain>

<decisions>
## Implementation Decisions

### Workflow shape
- **D-01:** One workflow file `.github/workflows/ci.yml` with a single job `ci` on `runs-on: ubuntu-24.04` (x86_64; a pinned image rather than `ubuntu-latest`, for reproducibility). A single job compiles and links V8 once instead of three times.
- **D-02:** Actions by tag, per the research stack: `actions/checkout@v7`, `dtolnay/rust-toolchain@1.99.1` with `components: rustfmt, clippy` (the ref is the toolchain, matching `rust-toolchain.toml`), and `Swatinem/rust-cache@v2` (default key hashes `rust-toolchain.toml`, `Cargo.lock`, `Cargo.toml`). SHA pinning is deferred (v2 OPS-03). Document the residual supply-chain risk in the threat model.
- **D-03:** Triggers are `push` (all branches) and `pull_request`. Hygiene: `permissions: contents: read`, a `concurrency` group per ref with `cancel-in-progress: true`, `timeout-minutes: 45` on the job, and `CARGO_TERM_COLOR: always` as the only workflow env. **No `continue-on-error` anywhere and no `SERVER.*` environment in the workflow** (CI-02; tests are env-free since Phase 2).
- **D-04:** Step order: checkout → toolchain → rust-cache → guards (D-05) → `cargo fmt --check` → `cargo clippy --all-targets --locked -- -D warnings` → `cargo test --locked` (default test-thread count, which exercises the V8-backed JS tests in parallel; CI-03) → `cargo build --release --locked` → release smoke (D-06). Each is its own named step, so the failing gate is obvious.

### Guards and x86_64 V8 evidence
- **D-05:** Add `scripts/ci-guards.sh` (bash, `set -euo pipefail`, runnable locally from the repo root and by CI). It exits non-zero with a clear message on:
  - (a) any lint-suppression attribute in `src/`: `#[allow(`, `#![allow(`, `#[cfg_attr(…allow(…))]`, or `#[expect(`. This is the operator directive from the Phase 1 UAT.
  - (b) any `#[ignore` in `src/`.
  - (c) `unwrap_used`/`expect_used` not at `deny` in `Cargo.toml`, or a `[lints]` entry set to `allow`.
  - (d) `unsafe_use_unprotected_platform` reaching deno_core's normal-edge feature set (`cargo tree -e features,normal -i deno_core`).
  - (e) a duplicate `v8` in `cargo tree -d`.
  - (f) legacy JS op syntax (`opSync`, `Deno.core.ops()`, un-prefixed `rs3_*` op names) in `requests/` or `src/`.
  - (g) more than one workflow file, or `continue-on-error` / `SERVER.` in `.github/workflows/`.
  - (h) a `.cargo/config*` that could lower lints.

  This turns the Phase 1 and Phase 3 "unverified prohibition" UAT items into wired checks.
- **D-06:** Add `scripts/ci-smoke.sh`. CI calls it after the release build, and it runs locally too. It starts `target/release/rs3` (the protected V8 platform path, with no dev-dependency features) with inline `env SERVER.HOST=127.0.0.1 SERVER.PORT=<port>` inside the script, never as workflow env. It waits for readiness with curl retries and asserts:
  - `GET /` → `"Welcome to rs3"`
  - POST `/insert` a script key
  - `PATCH` a key holding `6*20` → `"120"`
  - `PATCH` a throwing script → 422 `js_error`
  - `GET /` still 200 afterwards

  Then it kills the server (with a trap for cleanup). This is the x86_64 evidence for the production platform and PKU behaviour.
- **D-07:** Document the x86 crash fallback in a workflow comment and in the UAT item: if V8 platform init segfaults on the runner, move `unsafe_use_unprotected_platform` from `[dev-dependencies]` to `[dependencies]` (03-RESEARCH resolved Q1), and relax guard (d) accordingly in the same change.

### Local validation (no push)
- **D-08:** actionlint, yamllint and Docker are not installed on this host, so do not download unvetted binaries. Validate locally with:
  - (1) a Python `yaml.safe_load` parse plus structural assertions. Cover the trigger keys, job `runs-on: ubuntu-24.04`, the exact action refs, the three cargo commands with `--locked` where required, no `continue-on-error`, no `SERVER.` env, the `permissions` block, and that the guard and smoke scripts are referenced. This can live in a small checked-in `scripts/check-workflow.py` that `ci-guards.sh` invokes, or it can be part of the plan's verify commands; Claude's discretion.
  - (2) running `scripts/ci-guards.sh` and every workflow `run:` command locally on aarch64.
  - (3) `bash -n` on the scripts.

  If actionlint becomes available, run it, but it is not required.
- **D-09:** Record the remote run as a pending human UAT item in the phase UAT file: push `modernize-toolchain` to `origin` (github.com/jmfrank63/rs3), confirm the `ci` workflow is green on the x86_64 runner, note the V8 platform behaviour, and apply D-07 if it crashes.
- **D-10:** Delete `.github/workflows/actions-rs.yml` in this phase, so `ci.yml` is the only workflow (CI-01, success criterion 3).

### Claude's Discretion
- Exact script names and structure (one guards script or several), the smoke port, the workflow step names, and whether the workflow-structure check is Python or pure bash.
- Whether to add a `workflow_dispatch` trigger (harmless; allowed).

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Project scope
- `.planning/REQUIREMENTS.md`: CI-01, CI-02, CI-03
- `.planning/ROADMAP.md` §Phase 4: success criteria and notes (push needed to verify; PKU fallback; optional `cargo tree -d` v8 check)
- `.claude/CLAUDE.md` / `.planning/research/STACK.md`: CI tool versions (`actions/checkout@v7`, `dtolnay/rust-toolchain@1.99.1`, `Swatinem/rust-cache@v2`), "do not use actions-rs"
- `.planning/research/PITFALLS.md`: x86 PKU/protected-platform crash, cold-cache V8 download, cache size

### Prior phases
- `.planning/phases/01-toolchain-dependencies-foundation/01-UAT.md`: test 4, the operator directive for a CI suppression grep
- `.planning/phases/02-http-api-port-error-handling/02-UAT.md`: test 4 (same prohibition)
- `.planning/phases/03-javascript-runtime-port/03-UAT.md`: test 1 (Phase 3 test-tier prohibitions to wire)
- `.planning/phases/03-javascript-runtime-port/03-RESEARCH.md`: Open Questions (RESOLVED) Q1, the x86 fallback
- `.planning/phases/03-javascript-runtime-port/03-VALIDATION.md`: the feature-edge check command

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- `rust-toolchain.toml` pins 1.99.1 with rustfmt and clippy; `Cargo.lock` is committed; the build uses `--locked` everywhere.
- Phase 2/3 plans already used the exact gate commands and grep gates (`! grep -rnE '#!?\[(allow|cfg_attr\([^]]*allow)' src/`, `cargo tree -e features,normal -i deno_core | grep -c unsafe_use_unprotected_platform`, the legacy-syntax grep). Reuse them in `scripts/ci-guards.sh`.
- The live-smoke pattern (`env -i PATH=… SERVER.HOST=… SERVER.PORT=… target/debug/rs3 & … curl --retry 30 --retry-connrefused …`) comes from Phases 1–3.

### Established Patterns
- Tests are env-free and parallel-safe (Phase 2 TEST-01/02); the V8 test platform is the dev-only unprotected feature (Phase 3 D-07).
- No `#[allow]` anywhere; clippy `unwrap_used`/`expect_used` = deny with `clippy.toml` test allowances.

### Integration Points
- `.github/workflows/actions-rs.yml` (to delete) → `.github/workflows/ci.yml` (new).
- `scripts/` (new directory): `ci-guards.sh`, `ci-smoke.sh` (+ optional `check-workflow.py`).

</code_context>

<specifics>
## Specific Ideas

- **Git identity (user instruction):** commit with env vars `GIT_AUTHOR_NAME="Johannes Maria Frank"`, `GIT_AUTHOR_EMAIL="jmfrank63@gmail.com"`, `GIT_COMMITTER_NAME="Johannes Maria Frank"`, `GIT_COMMITTER_EMAIL="jmfrank63@gmail.com"`. **Never modify git config.** Commit messages end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- **Never `git push`.** Stay on `modernize-toolchain`.
- The TOOL-03 gate must still pass at the end of every plan.
- Keep `target/` inside the project dir. A project hook blocks naming the dotenv file, so use env-var config in the smoke.

</specifics>

<deferred>
## Deferred Ideas

- SHA-pinned actions, an aarch64 runner job, cargo-deny, coverage: v2 OPS-03.
- Docker image build/smoke in CI: Phase 6 (might add a CI job there).

</deferred>

---

*Phase: 04-ci-gate-on-x86-64*
*Context gathered: 2026-10-20 (smart discuss, auto-accepted recommendations)*
