---
phase: 04-ci-gate-on-x86-64
fixed_at: 2026-10-20T22:27:03Z
review_path: .planning/phases/04-ci-gate-on-x86-64/04-REVIEW.md
iteration: 1
findings_in_scope: 8
fixed: 8
skipped: 0
status: all_fixed
---

# Phase 4: Code Review Fix Report

**Fixed at:** 2026-10-20T22:27:03Z
**Source review:** .planning/phases/04-ci-gate-on-x86-64/04-REVIEW.md
**Iteration:** 1

**Summary:**
- Findings in scope: 8 (fix scope critical_warning: WR-01..WR-08)
- Fixed: 8
- Skipped: 0
- Info items handled because they sat next to a warning fix (not counted above): IN-05 (partly), IN-02 (inside WR-05), IN-06 (partly, inside WR-04 and WR-08)

Each guard extension has planted cases in `scripts/test-ci-guards.sh`. The self-test went from 32 cases to 76, with 0 missed, and the real tree still passes every guard.

## Fixed Issues

### WR-01: Guard (a) misses multi-line `cfg_attr` suppression, symlinked files and whitespace-split attributes

**Files modified:** `scripts/ci-guards.sh`, `scripts/test-ci-guards.sh`
**Commit:** fcbd7f3
**Applied fix:** Added an `ml_grep` helper. It is a small `python3` pass that matches a regex against whole files, follows symlinked files and directories the way `grep -R` does, skips symlink loops, and fails closed (exit 2) on a dangling link or missing path. Guard (a) now matches `#\s*!?\s*\[\s*(allow|expect)\s*\(`, and `cfg_attr(` up to the attribute's closing `]` containing `allow(`/`expect(`. That means `\s` spans newlines and `# ! [` spacing is accepted. `grep_guard` now takes a command (`grep ...` or `ml_grep ...`), and guards (b) and (f) use `grep -R`. New planted cases: the rustfmt-layout multi-line `cfg_attr`, `# [allow]`, `# ! [allow]`, a symlinked module file, and a symlinked module directory.

### WR-02: Guard (b) misses common ways to ignore or disable tests

**Files modified:** `scripts/ci-guards.sh`, `scripts/test-ci-guards.sh`
**Commit:** 2317756
**Applied fix:** Guard (b) now uses `ml_grep` to catch:
- `#[ignore]`
- `cfg_attr(.., ignore)`, including when it spans several lines
- doc fences opened with ```` ``` ```` or `~~~` whose lang string contains `ignore`, whether separated by commas or spaces
- `#[cfg(any())]` / `#[cfg(FALSE)]`

A `tomllib` check of Cargo.toml was added to the same tag. It rejects `test`/`doctest`/`harness = false` on any `[lib]`/`[[bin]]`/`[[test]]`/`[[bench]]`/`[[example]]` target, and `autolib`/`autobins`/`autotests = false`. 10 planted cases were added.

### WR-03: Guard (d) checks only the Cargo feature; source code can select the unprotected platform directly

**Files modified:** `scripts/ci-guards.sh`, `scripts/test-ci-guards.sh`
**Commit:** 1b158fb
**Applied fix:** This is folded into tag (d), so the [a]..[h] mapping to D-05 still holds. Guard (d) now also scans the Rust roots and rejects:
- `new_unprotected*` (`v8::new_unprotected_default_platform`, `Platform::new_unprotected`)
- `new_custom_platform`
- a direct `initialize_platform`
- any `init_platform(` argument other than `None` or empty. A literal `Some(..)` and a variable are both rejected.

A comment notes that the D-07 fallback relaxes only the feature check, never this scan. 6 planted cases were added, including a multi-line `Some(..)` and the variable form.

### WR-04: check-workflow.py accepts keys that redirect or neuter every gate

**Files modified:** `scripts/check-workflow.py`, `scripts/test-ci-guards.sh`
**Commit:** f645c87
**Applied fix:** The checker now uses allow-lists:
- workflow keys
- triggers (`push`, `pull_request`, `workflow_dispatch`, all unfiltered)
- `concurrency` keys
- job keys (`name`, `runs-on`, `timeout-minutes`, `steps`)
- step keys (`name`, `uses`, `with`, `run`)
- `with:` inputs for each action: none for checkout and rust-cache, `components` for rust-toolchain

Each step must also have exactly one of `uses`/`run`, and a non-mapping `with:` no longer crashes the components check. The existing `FORBIDDEN_KEYS` messages stay. A new `expect_wf_fail` helper runs `check-workflow.py` directly (IN-06), with 9 cases:
- checkout `ref:` and `repository:`
- `toolchain: nightly`
- `working-directory`
- job `permissions: write-all`
- job `container:`
- `paths-ignore`
- `types: [closed]`
- an extra `pull_request_target` trigger

There is also one case that runs through the guards.

### WR-05: In CI the forbidden-content grep is the only workflow check, and it is case-insensitive and incomplete

**Files modified:** `scripts/ci-guards.sh`, `scripts/test-ci-guards.sh`
**Commit:** c0713ce
**Applied fix:** The content grep in guard (g) is now case-insensitive (`grep -inE`). On top of the old terms it rejects:
- `working-directory`
- `write-all`
- an indented (job-level) `permissions:`
- any `key: write` value
- an indented `ref:`/`repository:`
- trigger filter keys: `branches`, `tags`, `paths` with their `-ignore` forms, and `types`

A second grep rejects every line containing `${{` except the exact `  group: ci-${{ github.ref }}` concurrency line. That covers `github.head_ref`, multi-line `run: |` blocks and tab-spaced expressions. Grep errors now fail closed, and every layer runs, so forbidden content no longer hides structure errors (IN-02). A new `expect_fail_ci` helper (CI=true with PyYAML hidden) checks that the grep layer alone catches 8 cases:
- lowercase `server.host`
- `github.head_ref`
- a tab-spaced `github.event` in a multi-line run block
- `working-directory`
- job `permissions`
- `contents: write`
- checkout `ref:`
- `paths-ignore`

### WR-06: Nothing guards the `cargo fmt --check` gate against being disabled by configuration

**Files modified:** `scripts/ci-guards.sh`, `scripts/test-ci-guards.sh`
**Commit:** 52620d4
**Applied fix:** Guard (h) now also fails on any `rustfmt.toml` / `.rustfmt.toml`, whatever it contains, because the project uses rustfmt defaults. It checks the repo root and the source roots, where rustfmt also looks (it follows symlinks, and a `find` error fails closed). It also fails on any `rustfmt::skip` attribute in the Rust roots. 5 planted cases were added: `disable_all_formatting`, `ignore = [...]`, a config inside `src/http/`, `#![rustfmt::skip]`, and an item-level `#[rustfmt::skip]`.

### WR-07: The smoke never reports the PKU crash it exists to detect; it surfaces as a bare "transport error"

**Files modified:** `scripts/ci-smoke.sh`, `scripts/ci-guards.sh`, `.github/workflows/ci.yml` (comments only)
**Commit:** 7222c0d
**Applied fix:** Every `fail` now runs `server_exit_note` in the main shell. If the server process is gone (it allows up to 2 s for a dying process), it reaps it and appends `; server died (status N)`. For 139 it adds `; SIGSEGV is the x86_64 V8 PKU symptom: apply the D-07 fallback`, and the startup message gets the same hint. The literal `(status 139)` text is unchanged, so the existing greps still match. The smoke header, the readiness comment, the guard (d) D-07 comment and the workflow comment now say the crash shows up at the first script execution (PATCH /key/m), not at startup.

Verified with shims under `target/tmp` (since deleted):
- A python stand-in that serves GET/POST and SIGSEGVs itself on PATCH prints `PATCH /key/m: transport error; server died (status 139); SIGSEGV ...`.
- `kill -SEGV $$` at startup prints `server exited during startup (status 139); SIGSEGV ...`.
- `/bin/false` prints `(status 1)`.
- A missing binary prints `missing`.

### WR-08: test-ci-guards.sh runs `rm -rf` on an unvalidated, environment-supplied path

**Files modified:** `scripts/test-ci-guards.sh`
**Commit:** beb9789
**Applied fix:** ROOT is now the physical path (`pwd -P`). `CI_GUARDS_SCRATCH` is resolved with `realpath -m`, relative to the repo root, and must match `$ROOT/target/tmp/?*`. Otherwise the script exits 2 before deleting anything. SCRATCH is therefore always absolute, and so is the shim path on PATH. EXIT, INT and TERM traps remove the scratch directory.

Also included (IN-06):
- `fresh()` aborts on a failed copy and copies `tests/ benches/ examples/ build.rs` when they exist.
- `run_guards` unsets an inherited `CI_GUARDS_ROOT`.

Verified on a throwaway copy, so a validation bug could not touch the worktree:
- `.`, `..`, `$HOME`, `target/tmp`, `target/tmpx`, `/tmp/x` and `target/tmp/../../src` are all rejected with exit 2, and nothing is deleted.
- A relative `target/tmp/rel-scratch` run from `src/` passes all 76 cases, including the cargo-shim case (e).
- `CI_GUARDS_ROOT=/nonexistent` passes all 76.
- SIGTERM during a run exits 143 and leaves `target/tmp` empty.

### IN-05 (partial, adjacent to WR-07): Smoke robustness nits

**Files modified:** `scripts/ci-smoke.sh`
**Commit:** 5940638
**Applied fix:** Readiness now loops on a real `SECONDS` deadline, so the "after 30s" message is accurate. `cleanup` sends SIGTERM, waits up to 5 s, then sends SIGKILL, so a hung graceful shutdown can no longer block until the 45-minute job timeout. A shim that ignores SIGTERM finished the smoke in about 37 s. The TOCTOU port pick and the `RS3_BIN` path normalisation were left as they are.

## Not addressed (Info, out of scope)

IN-01, IN-03, IN-04, IN-07 and IN-08 are not addressed. Neither are the remaining parts of IN-05 (port TOCTOU, `RS3_BIN` normalisation) and IN-06 (cases for `continue-on-error`/`SERVER.HOST`/`github.event` that call `check-workflow.py` directly).

## Concurrent change on the branch

While this run was in progress, the foreground session committed `fd8f43f ci: run on ubuntu-26.04 runner` to `modernize-toolchain`. That commit changes ci.yml, check-workflow.py and test-ci-guards.sh, and supersedes D-01's `ubuntu-24.04`. The fix branch was rebased onto `fd8f43f` with no conflicts, which is why the hashes above are the rebased ones. All verification below was then re-run on the rebased tip.

## Verification

Where it ran: the isolated worktree (`.claude/worktrees/rf-04-*`, branch `gsd-reviewfix/04-*`), on the rebased tip (fd8f43f plus the 9 fix commits). Cargo used `CARGO_TARGET_DIR=<main checkout>/target`, so the build reused the existing dependency artifacts. Source files are identical to the main checkout after the fast-forward, so these results reproduce there. Note that this rebuilt `target/release/rs3` from the same sources.

- `bash scripts/ci-guards.sh`: all guards passed, both locally and with `CI=true`
- `bash scripts/test-ci-guards.sh`: 76 cases, 0 missed. `target/tmp` was empty afterwards.
- `python3 scripts/check-workflow.py`: exit 0
- `bash -n` on each of the three scripts: ok
- `cargo build --release --locked && bash scripts/ci-smoke.sh`: `ci-smoke: OK`
- TOOL-03: `cargo build --locked` ok. `cargo test`: 171 passed, 0 failed (the `1 ignored` is deno_core's `extension!` doctest, as documented). `cargo clippy --all-targets -- -D warnings`: clean. `cargo fmt --check`: ok.

---

_Fixed: 2026-10-20T22:27:03Z_
_Fixer: Claude (gsd-code-fixer)_
_Iteration: 1_
