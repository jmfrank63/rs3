---
phase: 04-ci-gate-on-x86-64
plan: 02
subsystem: infra
tags: [ci, x86_64, v8, smoke, bash, curl, uat-handoff]

requires:
  - phase: 04-ci-gate-on-x86-64
    provides: ci.yml, scripts/ci-guards.sh, scripts/check-workflow.py, scripts/test-ci-guards.sh, scripts/ci-smoke.sh frame (plan 04-01)
  - phase: 03-javascript-runtime-port
    provides: PATCH /key/{key} on the JS pool, ApiError::JsError -> 422 js_error, release binary on the protected V8 platform
provides:
  - scripts/ci-smoke.sh drives a stored script through PATCH on the release binary (protected V8 platform) and asserts the exact 422 js_error body
  - proven smoke failure modes (missing binary, exit status 1, SIGSEGV status 139, busy port)
  - 04-VALIDATION.md per-task map green, Wave 0 complete
  - D-09 pending human UAT item (remote x86_64 run, with the D-07 fallback) for 04-UAT.md
affects: [phase-5-js-limits, phase-6-docker-docs, OPS-03, 04-UAT.md]

actuals:
  tokens: 1548
  tasks: 2
  commits: 2
plan_head_before: 1d13da675f13d9eff4f6b75fae66b2736e392ee4
plan_head_after: 37f7a5b4868350cf2dd088e7cf746e2fa7ad8bdc

tech-stack:
  added: []
  patterns:
    - "Smoke assertions compare the exact status code and exact body bytes with [[ == ]]; on failure they print the request, code and body"
    - "req METHOD PATH [BODY] passes the optional body through a bash array, so a body containing spaces stays one curl argument"

key-files:
  created: []
  modified:
    - scripts/ci-smoke.sh
    - .planning/phases/04-ci-gate-on-x86-64/04-VALIDATION.md

key-decisions:
  - "req() builds the optional --data-binary argument as a bash array instead of the research prototype's unquoted ${3:+...} expansion, so the insert body (which contains spaces) cannot be word-split"
  - "The remote x86_64 run is handed to the operator as a pending UAT item (D-09). It was not observed, and CI-03 remote confirmation stays open"

patterns-established:
  - "ci-smoke.sh ok line format: 'ci-smoke: ok   <request> -> <expected>'; five ok lines then 'ci-smoke: OK (<bin>, port <port>)'"

requirements-completed: [CI-02, CI-03]

coverage:
  - id: D1
    description: "Release smoke inserts m=6*20 and a throwing script, gets PATCH /key/m 200 \"120\" and PATCH /key/boom 422 {\"error\":\"js_error\",\"message\":\"Uncaught Error: boom\"}, then GET / 200 with the server alive. Five ok lines, two back-to-back passes, no rs3 process left"
    requirement: CI-03
    verification:
      - kind: integration
        ref: "cargo build --release --locked && bash scripts/ci-smoke.sh (twice; pgrep -cx rs3 unchanged)"
        status: pass
    human_judgment: false
  - id: D2
    description: "Smoke fails fast and loudly: missing binary, /bin/false (status 1), SIGSEGV shim (status 139, the x86_64 PKU symptom) and a busy port all exit non-zero with their lines"
    requirement: CI-03
    verification:
      - kind: other
        ref: "Task 1 verify blocks 2 and 3 (RS3_BIN=target/tmp/no-such-rs3, /bin/false, target/tmp/smoke-segv; RS3_SMOKE_PORT held by a python3 listener)"
        status: pass
    human_judgment: false
  - id: D3
    description: "Whole CI gate replays green locally in workflow order (guards, fmt, clippy, test, release build, release smoke), plus D-08 checks, guard self-test and TOOL-03; workflow still carries no server config"
    requirement: CI-02
    verification:
      - kind: integration
        ref: "Task 2 verify block 2: replay every ci.yml run: line from the repo root"
        status: pass
      - kind: other
        ref: "bash scripts/test-ci-guards.sh (32 cases, 0 missed); bash scripts/ci-guards.sh (all guards passed)"
        status: pass
      - kind: unit
        ref: "cargo build --locked && cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check (171 passed)"
        status: pass
    human_judgment: false
  - id: D4
    description: "Remote CI run on the GitHub ubuntu-24.04 x86_64 runner, including the protected V8 platform behaviour in the Release smoke step"
    requirement: CI-03
    verification: []
    human_judgment: true
    rationale: "Only a push can produce the remote run, and pushing is forbidden in this phase (operator constraint, D-09). Not observed."

duration: 2min
completed: 2026-10-20
status: complete
---

# Phase 4 Plan 02: x86_64 V8 Evidence Slice Summary

**The release smoke now runs JavaScript through the protected V8 platform on the release binary. It checks that PATCH of a stored `6*20` returns `"120"`, that a throwing script returns the exact 422 `js_error` body, and that the server keeps serving afterwards. A startup crash fails fast with its exit status (139 for SIGSEGV, the x86_64 PKU symptom). The full CI gate replays green locally. The remote x86_64 run is handed to the operator as a pending UAT item, together with the D-07 fallback.**

## Performance

- **Duration:** about 2 min
- **Started:** 2026-10-20T21:54:48Z
- **Completed:** 2026-10-20T21:56:38Z
- **Tasks:** 2
- **Files modified:** 2

## Accomplishments

- `scripts/ci-smoke.sh` has a `req` helper and four new exact-match assertions between the `GET /` check and the final OK line. Everything from plan 04-01 is unchanged: the inline `env -i` loopback config, the free-port pick, the busy-port refusal, the fail-fast readiness loop and the EXIT trap.
- The failure modes are proven. A scratch copy with a wrong expected body (`"121"`, or a truncated js_error message) also fails with the request, code and body, so the assertions have teeth.
- `04-VALIDATION.md` shows all four Phase 4 task rows green and `wave_0_complete: true`. `status: draft` and `nyquist_compliant: false` are unchanged.

## Observed smoke ok lines (release binary, this aarch64 host)

```
ci-smoke: ok   GET / -> "Welcome to rs3"
ci-smoke: ok   POST /insert -> 200
ci-smoke: ok   PATCH /key/m -> 200 "120"
ci-smoke: ok   PATCH /key/boom -> 422 js_error
ci-smoke: ok   GET / after JS error -> 200, server alive
ci-smoke: OK (target/release/rs3, port 57041)
```

The second back-to-back run passed on port 47475. `pgrep -cx rs3` was 0 before and after.

## Failure-mode runs

| Case | Exit | Output |
|------|------|--------|
| Missing binary (`RS3_BIN=target/tmp/no-such-rs3`) | 1 | `ci-smoke: FAIL: target/tmp/no-such-rs3 missing; run cargo build --release --locked first` |
| Exiting binary (`RS3_BIN=/bin/false`) | 1 | `ci-smoke: FAIL: server exited during startup (status 1)` |
| SIGSEGV shim (`kill -SEGV $$`) | 1 | bash's `Segmentation fault (core dumped)` notice, then `ci-smoke: FAIL: server exited during startup (status 139)` |
| Busy port (python3 listener on 59171) | 1 | `ci-smoke: FAIL: port 59171 already in use` |

No core file landed in the repo. The host's core_pattern pipes cores to apport.

## Workflow replay (D-08, warm incremental target/, aarch64)

| Step | Duration |
|------|----------|
| bash scripts/ci-guards.sh | 0.4s |
| cargo fmt --check | 0.1s |
| cargo clippy --all-targets --locked -- -D warnings | 0.2s |
| cargo test --locked | 1.0s |
| cargo build --release --locked | 0.2s |
| bash scripts/ci-smoke.sh | 0.9s (five ok lines, `ci-smoke: OK (target/release/rs3, port 38087)`) |

Other D-08 checks: `yaml.safe_load` of ci.yml, `python3 -I scripts/check-workflow.py`, the ast parse of check-workflow.py (no `scripts/__pycache__`), and `bash -n` on all three shell scripts all pass. actionlint is not installed, so it did not run, and it was not downloaded.

- Guard self-test: `test-ci-guards: 32 cases, 0 missed`.
- Guards: ok lines a to h and `ci-guards: all guards passed`.
- TOOL-03: `cargo build --locked && cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check` is green. The tests report 171 passed, 0 failed. One ignored item sits in a 0-test harness; it existed before this plan, and guard (a)/(b) still passes.

## Pending human UAT (for 04-UAT.md)

### 1. Remote CI run on the GitHub x86_64 runner (CI-03, ROADMAP success criterion 1; D-09)
expected: From the dev host run `git push -u origin modernize-toolchain` (origin is github.com/jmfrank63/rs3). In the repository's Actions tab, the `CI` workflow starts for that push. Its single job, `fmt, clippy, test, release smoke (x86_64)`, runs on `ubuntu-24.04`, and every step is green:
- Repo guards: ok lines a to h. A `skip [g-structure] PyYAML unavailable` line is acceptable in CI.
- cargo fmt --check.
- cargo clippy.
- cargo test: the V8-backed JS tests at the default thread count.
- cargo build --release.
- Release smoke: five ok lines and `ci-smoke: OK`.
Record the V8 platform behaviour seen in the Release smoke step (the protected platform; expected: no crash) and the duration of the cold run.
If the Release smoke reports `server exited during startup (status 139)`, or V8 platform init segfaults anywhere, apply the D-07 fallback: in Cargo.toml, move `features = ["unsafe_use_unprotected_platform"]` from the `[dev-dependencies]` deno_core entry to the `[dependencies]` deno_core entry. In the same commit, relax guard (d) in scripts/ci-guards.sh together with its (d) case in scripts/test-ci-guards.sh, then re-run the workflow.
result: [pending]

The remote x86_64 run was not observed in this phase: per the operator constraint nothing was pushed, so CI-03's remote confirmation stays open until this item passes.

## Prior UAT items now wired

- **01-UAT test 4** (no gate-greening by suppression): guard (a).
- **02-UAT test 4** (no gate-greening by suppression, all three plans): guards (a), (b) and (c), plus `cargo test --locked` for the legacy goldens.
- **03-UAT test 1** (test-tier prohibitions): guards (a), (b), (c), (d) and (f). The part about the docs/js-ops.md limits disclosure stays a judgment item.

Evidence the verifier can re-run: `bash scripts/ci-guards.sh` (prints ok lines a to h, then `ci-guards: all guards passed`) and `bash scripts/test-ci-guards.sh` (prints `test-ci-guards: 32 cases, 0 missed`).

## Requirements

- **CI-01:** complete (plan 04-01; the single ci.yml).
- **CI-02:** complete. Every gate passes locally in workflow order, and the workflow carries no server config.
- **CI-03:** implemented and locally evidenced (the release smoke exercises the protected V8 platform, and a crash is reported as status 139). The remote x86_64 run is pending UAT.

## Task Commits

1. **Task 1: release smoke drives JS through the protected V8 platform**: `34b7397` (ci)
2. **Task 2: phase gate replay and validation map**: `37f7a5b` (docs)

## Files Created/Modified

- `scripts/ci-smoke.sh`: the `req` helper, assertions for POST /insert, PATCH /key/m, PATCH /key/boom and GET / after the JS error, the alive check, and a header listing the assertions and the status 139 / D-07 note
- `.planning/phases/04-ci-gate-on-x86-64/04-VALIDATION.md`: four rows green, `wave_0_complete: true`, Wave 0 checkboxes, and the 04-UAT.md handoff note

## Decisions Made

- `req()` passes the optional body as a bash array (`${data[@]+"${data[@]}"}`) instead of the research prototype's `${3:+--data-binary "$3"}`. The insert body contains spaces, and the array form makes it explicit that the body stays a single argument and is safe under `set -u`.

## Deviations from Plan

The plan was executed as written. (The array-based `req` body handling is an implementation detail inside the specified helper, not a behaviour change.) One planning-record adjustment was made at close-out:

**1. [Rule 2 - Transparency] CI-03 requirement line annotated as remote-pending**
- **Found during:** state updates (`requirements.mark-complete CI-02 CI-03`)
- **Issue:** the CI-03 checkbox text reads "CI runs on an x86_64 Linux runner and passes". Ticking it with no note would imply the remote run is green, which the plan's CI-03 transparency prohibition forbids.
- **Fix:** the checkbox and the traceability `Complete` stay as the tool set them, and the CI-03 line in REQUIREMENTS.md gets the suffix "(implemented and locally evidenced; the remote x86_64 run is not yet observed and is a pending human UAT item, see 04-02-SUMMARY.md, D-09)".
- **Files modified:** .planning/REQUIREMENTS.md
- **Committed in:** the plan metadata commit

## Issues Encountered

None.

## Threat Flags

None. The smoke still binds 127.0.0.1 only, cleans up through the EXIT trap, and adds no new surface.

## User Setup Required

None. The remote run is a UAT item, not setup.

## Next Phase Readiness

- Phase 4 is complete locally. The orchestrator should persist the D-09 item above into `04-UAT.md`. Nothing was pushed: `refs/remotes/origin/modernize-toolchain` does not exist, and no git identity is configured.
- If the remote x86_64 run crashes during V8 platform init, apply D-07 (see the UAT item) before Phase 5 builds the JS limits on top.

---
*Phase: 04-ci-gate-on-x86-64*
*Completed: 2026-10-20*

## Self-Check: PASSED

- FOUND: scripts/ci-smoke.sh, .planning/phases/04-ci-gate-on-x86-64/04-VALIDATION.md
- FOUND commits: 34b7397, 37f7a5b
- Plan verification: SUMMARY greps (pending UAT, result: [pending], push command, unsafe_use_unprotected_platform, status 139) pass; replay, D-08, self-test, TOOL-03, scope and no-push checks pass
