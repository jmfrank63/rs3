---
phase: 04-ci-gate-on-x86-64
plan: 01
subsystem: infra
tags: [github-actions, ci, x86_64, bash, pyyaml, tomllib, cargo-tree, v8]

requires:
  - phase: 03-javascript-runtime-port
    provides: dev-only unsafe_use_unprotected_platform edge, op_rs3_* op names, release binary on the protected V8 platform
provides:
  - .github/workflows/ci.yml, the single CI workflow (one ci job on ubuntu-24.04, timeout 45)
  - scripts/check-workflow.py, a PyYAML structure check of ci.yml (D-08)
  - scripts/ci-guards.sh, repo policy guards (a) to (h) (D-05)
  - scripts/ci-smoke.sh, the release-binary smoke frame (free port, inline env config, fail-fast startup, GET /)
  - scripts/test-ci-guards.sh, a planted-violation self-test proving that each guard fails loudly
affects: [04-02, phase-5-js-limits, phase-6-docker-docs, OPS-03]

actuals:
  tokens: 6400
  tasks: 2
  commits: 3
plan_head_before: b5dc3e66da2378d2edc32fb67e0d52877cba453f
plan_head_after: 7eea3df51f8f3f1efaafab6080f6925c00fc9dce

tech-stack:
  added: []
  patterns:
    - "The workflow only sequences scripts. Every run: line is a single command that also runs locally from the repo root."
    - "Guards collect every failure, print ok/FAIL [tag] in fixed a-to-h order, then exit 1 if any failed."
    - "Planted-violation self-test: a throwaway copy under target/tmp, one mutation per case, and the copied guard script must exit non-zero with FAIL [tag]."

key-files:
  created:
    - .github/workflows/ci.yml
    - scripts/check-workflow.py
    - scripts/ci-guards.sh
    - scripts/ci-smoke.sh
    - scripts/test-ci-guards.sh
  modified: []
  deleted:
    - .github/workflows/actions-rs.yml

key-decisions:
  - "check-workflow.py requires the six run strings to be exactly REQUIRED_RUNS (no extra run steps), in addition to the prototype's presence and order checks"
  - "Guards (a), (b) and (f) treat a grep error (exit 2, e.g. a missing directory) as a FAIL, and (d)/(e) report cargo tree's stderr as a FAIL, so set -e never aborts without a FAIL line"
  - "Guard (g) uses PyYAML only if it is already importable. In CI it skips just the structure check with a printed skip line, and locally a missing PyYAML fails (g). Nothing is pip-installed."
  - "TDD commits for Task 2 use test(04-01)/feat(04-01) instead of the plan's single ci(04-01) commit, so the RED/GREEN gate commits are visible"

patterns-established:
  - "ci-guards.sh line format: 'ci-guards: ok   [x] msg' on stdout, 'ci-guards: FAIL [x] msg' on stderr, final 'all guards passed' or 'N guard(s) failed'"
  - "ci-smoke.sh frame: plan 04-02 inserts the JS assertions between the GET / check and the final OK line"

requirements-completed: [CI-01, CI-02, CI-03]

coverage:
  - id: D1
    description: "ci.yml is the only workflow (actions-rs.yml deleted). It has one ci job on ubuntu-24.04 with checkout@v7, rust-toolchain@1.99.1 (rustfmt, clippy), rust-cache@v2 and the six gate steps in D-04 order"
    requirement: CI-01
    verification:
      - kind: other
        ref: "python3 -I scripts/check-workflow.py && [ \"$(ls -A .github/workflows)\" = ci.yml ]"
        status: pass
    human_judgment: false
  - id: D2
    description: "fmt, clippy (-D warnings) and test (--locked) are separate steps with no suppression key, env, if or shell, and every workflow run: line passes locally in order, ending with ci-smoke: OK"
    requirement: CI-02
    verification:
      - kind: integration
        ref: "Task 1 verify block 2: replay every run: line of ci.yml (yaml.safe_load) from the repo root"
        status: pass
      - kind: other
        ref: "bash scripts/test-ci-guards.sh (g cases: continue-on-error, if:, shell:, thread-count flag, clippy without --locked)"
        status: pass
    human_judgment: false
  - id: D3
    description: "Static half of CI-03: x86_64 ubuntu-24.04 image, default test-thread count, release build and release smoke step. The remote x86_64 observation is pending human UAT (handed off by plan 04-02)"
    requirement: CI-03
    verification:
      - kind: other
        ref: "grep -qxF '    runs-on: ubuntu-24.04' .github/workflows/ci.yml && ! grep -q test-threads .github/workflows/ci.yml"
        status: pass
    human_judgment: true
    rationale: "The real x86_64 run (V8 PKU behaviour of the protected platform) can only be observed by pushing, which is forbidden in this phase (D-09)."
  - id: D4
    description: "Guards (a) to (h) pass on the real tree and each one catches its planted violations (31 caught cases plus the CI PyYAML skip case)"
    verification:
      - kind: other
        ref: "bash scripts/ci-guards.sh (ok lines abcdefgh)"
        status: pass
      - kind: other
        ref: "bash scripts/test-ci-guards.sh (32 cases, 0 missed)"
        status: pass
    human_judgment: false

duration: 5min
completed: 2026-10-20
status: complete
---

# Phase 4 Plan 01: CI Gate Slice Summary

**A single x86_64 `ci.yml` (checkout@v7, rust-toolchain@1.99.1, rust-cache@v2) now runs repo guards, fmt, clippy, test, the release build and a release-binary smoke. It replaces the archived actions-rs workflow, and all eight D-05 policy guards are proven to fail on planted violations.**

## Performance

- **Duration:** about 5 min
- **Started:** 2026-10-20T21:47:07Z
- **Completed:** 2026-10-20T21:52:31Z
- **Tasks:** 2 (a tracer, then a TDD task)
- **Files modified:** 6 (5 created, 1 deleted)

## Accomplishments

- `.github/workflows/ci.yml` is the only workflow. It has one job, `ci`, on `ubuntu-24.04` with `timeout-minutes: 45`, `permissions: contents: read`, per-ref concurrency with cancellation, and `CARGO_TERM_COLOR` as the only env. The archived `actions-rs.yml` (four step-level error suppressions and the server env) is removed with `git rm`.
- `scripts/check-workflow.py` (D-08) checks the structure: triggers, permissions, concurrency, env, a single job, the runner, the timeout, and the exact ordered `uses:`/`run:` lists. It forbids `continue-on-error`, `env`, `if` and `shell` on the job and on every step, and `defaults` at both levels.
- `scripts/ci-guards.sh` enforces D-05 guards (a) to (h). Guard (g) includes the resolved research open questions: event-context interpolation is rejected, and PyYAML is skipped only in CI.
- `scripts/ci-smoke.sh` is the release-binary smoke frame. It picks a free port, configures the server with inline `env -i` (hermetic), fails fast on early exit (the status appears, and 139 means SIGSEGV), and asserts the exact `GET /` body.
- `scripts/test-ci-guards.sh` proves that each guard has teeth. RED and GREEN are both recorded below.

## ci.yml step names (exact)

1. Checkout
2. Install Rust 1.99.1
3. Cache cargo and target
4. Repo guards
5. cargo fmt --check
6. cargo clippy
7. cargo test
8. cargo build --release
9. Release smoke

## Workflow replay on this aarch64 host (D-08 item 2)

Each `run:` line was extracted with `yaml.safe_load` and run in order from the repo root. These are the final timings after Task 2, on a warm `target/` (incremental, so they are not cold-runner numbers):

| Step | Duration |
|------|----------|
| bash scripts/ci-guards.sh | 0.4s |
| cargo fmt --check | 0.1s |
| cargo clippy --all-targets --locked -- -D warnings | 0.2s |
| cargo test --locked | 1.0s |
| cargo build --release --locked | 0.2s |
| bash scripts/ci-smoke.sh | 0.9s (`ci-smoke: OK (target/release/rs3, port <free>)`) |

Smoke failure paths were also checked by hand: `/bin/false` gives `server exited during startup (status 1)`, a SIGSEGV shim gives `(status 139)`, a missing binary gives `... missing; run cargo build --release --locked first`, and the busy port 18081 gives `port 18081 already in use`.

## TDD (Task 2)

- **RED:** `c9cadc2`. Run against the Task 1 script, which had guard (g) only: `test-ci-guards: 32 cases, 16 missed`, exit 1. The control passed, all 15 (g) cases were caught, `ok g-structure-skip` passed, and every one of the 16 (a)-(f) and (h) cases was MISSED, with the guard output showing `all guards passed` and zero `setup failed` lines. Semantic assessment: each target case failed for the intended reason (its guard did not exist yet), not because of a setup or syntax fault. The output is bespoke bash, a format the gsd `tdd-red-evidence` classifier has no adapter for. `workflow.tdd_mode` is not enabled, so the classifier gate was not run.
- **GREEN:** `7eea3df`. `test-ci-guards: 32 cases, 0 missed` (31 `caught` lines plus `ok g-structure-skip`), exit 0, and the scratch directory was removed. The real tree prints ok lines `abcdefgh` and `ci-guards: all guards passed`.
- **REFACTOR:** none needed.

## Prior UAT items now wired

- **01-UAT test 4** (no suppression attributes): guard (a).
- **02-UAT test 4** (no gate-greening by suppression): guards (a), (b) and (c), plus `cargo test --locked` for the legacy goldens.
- **03-UAT test 1** (test-tier prohibitions): guards (a), (b), (c), (d) and (f). The "docs disclose the Phase 5 limits without softening them" part (docs/js-ops.md) stays a judgment item.

## Action SHAs as of 2026-10-20 (threat T-04-01, for OPS-03 pinning)

- `actions/checkout@v7`: `3d3c42e5aac5ba805825da76410c181273ba90b1`
- `dtolnay/rust-toolchain@1.99.1`: `d124f86f28ff42855784ff3319f48ac182c248f8`
- `Swatinem/rust-cache@v2`: `e9d7b9f1e587218970671bac74e5511ed9506e6a`

(These come from the plan's threat register and were not re-fetched, since the plan does no network work.)

## Task Commits

1. **Task 1: Tracer, ci.yml replaces actions-rs.yml, plus structure check, guard (g) and smoke frame**: `4d00cd8` (ci)
2. **Task 2 RED: planted-violation self-test**: `c9cadc2` (test)
3. **Task 2 GREEN: guards (a)-(f) and (h)**: `7eea3df` (feat)

## Files Created/Modified

- `.github/workflows/ci.yml`: the single CI workflow, with the D-07 comment above Release smoke
- `.github/workflows/actions-rs.yml`: deleted (D-10)
- `scripts/check-workflow.py`: D-08 structure check (stdlib plus PyYAML), resolves ci.yml from its own location
- `scripts/ci-guards.sh`: D-05 guards (a) to (h), with the `D-07 fallback` comment at guard (d)
- `scripts/ci-smoke.sh`: release smoke frame (plan 04-02 adds the JS assertions)
- `scripts/test-ci-guards.sh`: planted-violation self-test (local only, not a CI step)

All four scripts are tracked as mode 100755. CI calls the shell scripts through `bash`.

## Decisions Made

- check-workflow.py requires the run steps to be exactly `REQUIRED_RUNS`, with no extra `run:` steps. This enforces the "exactly, in this order" truth, which the prototype checked only for presence and order.
- A grep or cargo-tree error inside a guard becomes a FAIL line for that guard instead of a silent pass (grep exit 2) or a `set -e` abort. The cargo tree stderr is captured by a second call that runs only on the failure path, so no temp file is needed.
- ci-smoke.sh sets its EXIT trap right after `mktemp` and uses a `ready` flag, so the log is removed on every failure path and a partial curl body cannot count as readiness.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Self-test setup check matched the D-07 comment**
- **Found during:** Task 2 (first RED run)
- **Issue:** the "guards run line deleted" case confirmed the deletion with `! grep -q 'ci-guards.sh'`, but the D-07 comment in ci.yml also names `scripts/ci-guards.sh`, so the case reported `setup failed`.
- **Fix:** the check now greps for `run: bash scripts/ci-guards.sh`.
- **Files modified:** scripts/test-ci-guards.sh
- **Verification:** the RED re-run caught the case, and GREEN reports 0 missed.
- **Committed in:** c9cadc2 (the RED commit)

**2. [Rule 2 - Correctness hardening] Stricter structure and error handling than the prototypes**
- **Found during:** Tasks 1 and 2
- **Issue:** compared with the prototypes, check-workflow.py let extra `run:` steps through and gave a traceback on a non-mapping or unreadable YAML file. ci-guards.sh treated grep exit 2 as a pass, and a cargo tree failure aborted the script under `set -e` without a FAIL line. ci-smoke.sh leaked its log on early failures.
- **Fix:** an exact run-list check, clean `check-workflow:` errors for load failures, a `grep_guard` helper (0 fails, 1 passes, anything else fails), cargo tree failures reported as `FAIL [d]`/`FAIL [e]`, and an early EXIT trap in the smoke.
- **Files modified:** scripts/check-workflow.py, scripts/ci-guards.sh, scripts/ci-smoke.sh
- **Verification:** a manual multi-violation copy printed both `FAIL [a]` and `FAIL [h]` and then `2 guard(s) failed`. A copy with a broken Cargo.toml printed `FAIL [c]`, `FAIL [d] cargo tree failed`, `FAIL [e] cargo tree failed` and then the summary.
- **Committed in:** 4d00cd8, 7eea3df

**3. [Process] Task 2 split into test/feat commits**
- **Found during:** Task 2
- **Issue:** the plan names one `ci(04-01): repo guards (a)-(h) with a planted-violation self-test` commit, but the task is `tdd="true"`, and the TDD commit-scope contract wants a RED `test(...)` commit followed by a GREEN `feat(...)` commit.
- **Fix:** a RED commit `test(04-01): ...` (c9cadc2), then a GREEN commit `feat(04-01): repo guards (a)-(h) with a planted-violation self-test` (7eea3df).
- **Committed in:** c9cadc2, 7eea3df

---

**Total deviations:** 3 (1 bug, 1 correctness hardening, 1 process)
**Impact on plan:** no scope creep. Every file is in the plan's owned set, and nothing under src/, requests/, docs/, Cargo.toml, Cargo.lock, clippy.toml or rust-toolchain.toml changed (`git diff --quiet 8924cac` passes).

## Issues Encountered

- `/tmp` already holds four zero-byte `tmp.*` files and one directory from the research-phase prototype runs (21:19 and 21:43, before this plan started). They were left alone. The new smoke cleans up its own log.
- actionlint is not installed and was not fetched (D-08). Validation is check-workflow.py plus `bash -n`.

## User Setup Required

None. No external service configuration is required.

## Next Phase Readiness

- Plan 04-02 can insert the JS/V8 assertions into `scripts/ci-smoke.sh` between the `GET /` check and the final `ci-smoke: OK` line, then record the remote x86_64 run as pending human UAT (D-09) together with the D-07 fallback.
- Nothing was pushed. `refs/remotes/origin/modernize-toolchain` does not exist.
- Reviewers should know that `dtolnay/rust-toolchain`'s own composite action contains an internal `continue-on-error` (research Pitfall 6). Guard (g) scans only our `.github/workflows/`.

## Self-Check: PASSED

- FOUND: .github/workflows/ci.yml, scripts/check-workflow.py, scripts/ci-guards.sh, scripts/ci-smoke.sh, scripts/test-ci-guards.sh
- GONE: .github/workflows/actions-rs.yml
- FOUND commits: 4d00cd8, c9cadc2, 7eea3df
- Plan verification: replay green, check-workflow ok, `bash -n` ok on all scripts, guards abcdefgh, self-test 32/0, TOOL-03 green (171 passed), scope and no-push checks pass, all scripts 100755

---
*Phase: 04-ci-gate-on-x86-64*
*Completed: 2026-10-20*
