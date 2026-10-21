---
phase: 05-js-execution-limits
plan: 03
subsystem: js-runtime
tags: [deno_core, v8, release-smoke, ci, op_print, stack, docs, tdd]

# Dependency graph
requires:
  - phase: 05-js-execution-limits
    provides: "05-01 depth limit (422 js_depth_limit, JobState, classify) and 05-02 timeout (504 js_timeout, watchdog, backstop, dedicated_executor)"
  - phase: 04-ci-gate-on-x86-64
    provides: scripts/ci-smoke.sh (release smoke frame) and scripts/ci-guards.sh
provides:
  - release smoke asserting 422 js_depth_limit, 504 js_timeout and other requests answered while a script loops
  - write_all_retrying / flush_retrying behind op_rs3_print (IN-04)
  - MIN_STACK_BYTES (2 MiB) and effective_stack_bytes for worker stacks (IN-07)
  - accurate worker_loop catch_unwind doc (IN-06)
  - requests/*.txt payload parity tests (IN-05)
  - docs/js-ops.md limits documentation (D-12)
  - 05-VALIDATION.md nine rows green
affects: [phase 6 README, 05-UAT (remote x86_64 CI run), verify-work for phase 5]

# Actuals (#2632)
actuals:
  tokens: 10237
  tasks: 3
  commits: 4
plan_head_before: 1d8947b8bad773d7ab2097d9a8422641f39511d8
plan_head_after: 00c3428080b09fb5e4bc407c5b48b61f44024761

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Release smoke runs a background request (LOOP_PID + mktemp output) that the EXIT trap kills, reaps and deletes"
    - "Write and flush through retry helpers that mirror upstream op_print, with Ok(0) as WriteZero and no unwrap"
    - "Every JsSettings value the executor uses is clamped: workers, queue capacity, stack bytes, depth, timeout"

key-files:
  created:
    - .planning/phases/05-js-execution-limits/tdd-evidence/05-03-task2-red.json
    - .planning/phases/05-js-execution-limits/tdd-evidence/05-03-task2-red.junit.xml
  modified:
    - scripts/ci-smoke.sh
    - src/js/ops.rs
    - src/js/executor.rs
    - src/js/mod.rs
    - src/http/mod.rs
    - docs/js-ops.md
    - .planning/phases/05-js-execution-limits/05-VALIDATION.md

key-decisions:
  - "The smoke records the loop's start time right after launching curl (the stricter lower bound) and also requires LOOP_OUT to be empty next to kill -0 when PATCH /key/m returns, so a reaped-zombie race cannot pass"
  - "op_rs3_print formats the stderr line into a String first, then writes and flushes it through the retry helpers; output stays one Debug-escaped js stderr: line per call"
  - "A misbehaving writer that reports more bytes than it was given ends the write loop (bytes.get(n..).unwrap_or_default()) instead of panicking"
  - "Docs state precedence host fault 500 > timeout 504 > depth 422 js_depth_limit > script 422 js_error, matching classify"

patterns-established:
  - "Pattern: pure clamp test first, runtime test on the clamped floor only after the clamp exists (research Pitfall 6)"

requirements-completed: [ROBU-05, ROBU-06, TEST-04]

coverage:
  - id: D1
    description: "The release binary (protected V8 platform) answers 422 js_depth_limit for a self-patching key and 504 js_timeout after about 5 s for while(true){}, while GET / and another PATCH are answered, and nothing is left running"
    requirement: ROBU-06
    verification:
      - kind: e2e
        ref: "cargo build --release --locked && RS3_SMOKE_PORT=18533 bash scripts/ci-smoke.sh"
        status: pass
      - kind: e2e
        ref: "RS3_SMOKE_PORT=18534 bash scripts/ci-smoke.sh (phase gate)"
        status: pass
    human_judgment: false
  - id: D2
    description: "op_print retries Interrupted and WouldBlock, turns Ok(0) into WriteZero, returns other errors and never unwraps (IN-04)"
    requirement: TEST-04
    verification:
      - kind: unit
        ref: "src/js/ops.rs#print_retries_would_block"
        status: pass
      - kind: unit
        ref: "src/js/ops.rs#print_gives_up_on_other_errors"
        status: pass
      - kind: unit
        ref: "src/js/ops.rs#print_to_either_stream_keeps_running"
        status: pass
    human_judgment: false
  - id: D3
    description: "Worker stacks are clamped to at least 2 MiB and that floor runs a maximum-depth chain with deep leaf recursion (IN-07)"
    requirement: TEST-04
    verification:
      - kind: unit
        ref: "src/js/executor.rs#stack_bytes_are_clamped"
        status: pass
      - kind: unit
        ref: "src/js/executor.rs#minimum_stack_runs_the_max_depth_chain"
        status: pass
    human_judgment: false
  - id: D4
    description: "Each requests/*.txt payload inserted through POST /insert gives the same body as its .js fixture (IN-05)"
    requirement: TEST-04
    verification:
      - kind: integration
        ref: "src/http/mod.rs#txt_payload_fac_is_120"
        status: pass
      - kind: integration
        ref: "src/http/mod.rs#txt_payload_sum_is_10"
        status: pass
      - kind: integration
        ref: "src/http/mod.rs#txt_payload_bin_is_tree_json"
        status: pass
      - kind: integration
        ref: "src/http/mod.rs#txt_payload_store_writes_res"
        status: pass
    human_judgment: false
  - id: D5
    description: "Every TEST-04 error path has a named passing test; the suite passes at default threads, serially, five times and under env -i in under 60 s per run; the full CI gate is green locally"
    requirement: TEST-04
    verification:
      - kind: other
        ref: "cargo test --locked --lib -- --exact <the 13 TEST-04 tests> (13 passed)"
        status: pass
      - kind: other
        ref: "bash scripts/ci-guards.sh && cargo fmt --check && cargo clippy --all-targets --locked -- -D warnings && cargo test --locked && cargo build --release --locked && RS3_SMOKE_PORT=18534 bash scripts/ci-smoke.sh"
        status: pass
      - kind: other
        ref: "cargo test --locked -- --test-threads=1; 5x cargo test --locked; env -i PATH HOME cargo test --locked"
        status: pass
    human_judgment: false
  - id: D6
    description: "docs/js-ops.md documents both limits, their answers, precedence, kept writes, the backstop residual and the uncovered heap exhaustion, and that both are set in code (IN-06 doc and D-12)"
    requirement: ROBU-05
    verification:
      - kind: other
        ref: "docs grep checks from Task 3 verify (bodies, setting names, heap note, no phase mentions)"
        status: pass
    human_judgment: true
    rationale: "Doc accuracy and the no-rollback / not-from-environment prohibitions are judgment-tier; greps prove presence, not that the prose is correct and clear"
  - id: D7
    description: "The remote x86_64 CI run shows the watchdog terminating isolates on the protected PKU platform (assumption A1)"
    requirement: ROBU-06
    verification: []
    human_judgment: true
    rationale: "Only observable on the x86_64 runner after an operator push; nothing was pushed in this run (pending UAT item below)"

# Metrics
duration: 8min
completed: 2026-10-21
status: complete
---

# Phase 5 Plan 03: Release Smoke, Carry-overs and Limits Docs Summary

**The release smoke now proves both limits on the protected V8 platform: 422 `js_depth_limit` for a self-patching key, and 504 `js_timeout` 5009 ms after a `while(true){}` PATCH was sent, while GET / and another PATCH are answered. op_print retries on WouldBlock, worker stacks have a 2 MiB floor, the catch_unwind doc is accurate, the `.txt` payloads match their fixtures, and docs/js-ops.md documents both limits as built.**

## Performance

- **Duration:** about 8 min
- **Started:** 2026-10-21T00:38:53Z
- **Completed:** 2026-10-21T00:47:12Z
- **Tasks:** 3 (Task 1 tracer, Task 2 TDD RED then GREEN, Task 3 docs and gate)
- **Files modified:** 7, plus 2 evidence files created

## Accomplishments

- `scripts/ci-smoke.sh` asserts exact status codes and exact body bytes for the depth stop, the timeout and the requests answered during the loop. It tracks the background curl in `LOOP_PID`, and its EXIT trap kills and reaps it and deletes its temp file. `ci.yml` is unchanged.
- IN-04: `write_all_retrying` and `flush_retrying` sit behind both op_print streams.
- IN-07: `effective_stack_bytes` clamps worker stacks to `MIN_STACK_BYTES` (2 MiB).
- IN-06: the `worker_loop` doc says what catch_unwind really covers.
- IN-05: four `txt_payload_*` tests send the shipped payloads through POST /insert.
- docs/js-ops.md: "Sandbox and known limits", "Errors", "PATCH results" (with precedence), the op_rs3_patch row and the intro describe the depth limit and the timeout. The page no longer mentions phases.
- 05-VALIDATION.md: all nine rows are green and `wave_0_complete: true`. `status: draft` and `nyquist_compliant: false` are unchanged.

## Task Commits

1. **Task 1 (tracer): release smoke asserts 422 and 504 while other requests are answered**: `01c8c3a` (ci)
2. **Task 2: Phase 3 carry-overs (D-13)**: RED `f8f764b` (test), GREEN `9bb6833` (fix)
3. **Task 3: limits docs, phase gate, validation map**: `00c3428` (docs)

**Tracer feedback gate:** auto mode was active. After the Task 1 commit I re-ran its verify end to end. It passed (504 after 5009 ms, rs3 and curl process counts unchanged), so expansion continued.

## Observed release smoke (phase gate run, port 18534)

```
ci-smoke: ok   GET / -> "Welcome to rs3"
ci-smoke: ok   POST /insert -> 200
ci-smoke: ok   PATCH /key/m -> 200 "120"
ci-smoke: ok   PATCH /key/boom -> 422 js_error
ci-smoke: ok   GET / after JS error -> 200, server alive
ci-smoke: ok   POST /insert me, loop -> 200
ci-smoke: ok   PATCH /key/me -> 422 js_depth_limit
ci-smoke: ok   GET / during a runaway PATCH -> 200
ci-smoke: ok   PATCH /key/m during a runaway PATCH -> 200 "120"
ci-smoke: ok   PATCH /key/loop -> 504 js_timeout after 5010 ms
ci-smoke: ok   PATCH /key/m after the timeout -> 200 "120", server alive
ci-smoke: OK (target/release/rs3, port 18534)
```

- **504 delay:** measured from the moment curl was launched until it exited. Task 1 run and tracer gate (port 18533): 5009 ms both times. Phase gate (port 18534): 5010 ms.
- **CPUs:** the host has 4, so the concurrent PATCH /key/m check ran instead of printing its skip line.
- **Failure path:** `RS3_BIN=/bin/false` still fails with "server exited during startup (status 1)".
- **Cleanup:** the rs3 and curl process counts were 0 before and after each run.

## Suite

| Run | Result | Time |
|-----|--------|------|
| `cargo test --locked` (default threads) | 212 passed, 0 failed | 4.00 s in the harness, 4.6 s wall |
| `cargo test --locked -- --test-threads=1` | 212 passed | 14.32 s |
| 5 repeats of `cargo test --locked` | 212 passed each time | 4.5 to 4.6 s wall each |
| `env -i PATH="$PATH" HOME="$HOME" cargo test --locked` | 212 passed | 4.6 s wall |
| The plan's chained verify (timed run < 60 s, serial, 5x, env -i) | exit 0 | "suite 4s" |
| The 13 TEST-04 tests run by exact name | 13 passed | 1.01 s |

- The full gate passed: `bash scripts/ci-guards.sh` ("all guards passed"), `cargo fmt --check`, `cargo clippy --all-targets --locked -- -D warnings`, `cargo test --locked`, `cargo build --release --locked` and `RS3_SMOKE_PORT=18534 bash scripts/ci-smoke.sh`.
- The one "ignored" doctest is the deno_core `extension!` macro doc for `rs3_ext`. It predates this phase and is not an `#[ignore]` in rs3 code.

## TEST-04 error-path tests

| Error path | Status | Test | File |
|------------|--------|------|------|
| 404 GET of a missing key | pass | `http::kv::tests::get_missing_is_404_json` | src/http/kv.rs |
| 404 DELETE of a missing key | pass | `http::kv::tests::delete_missing_is_404_json` | src/http/kv.rs |
| 404 PATCH of a missing key | pass | `http::tests::test_patch_missing_is_404_json` | src/http/mod.rs |
| 400 malformed insert | pass | `http::kv::tests::insert_malformed_is_400_json` | src/http/kv.rs |
| 422 non-object insert | pass | `http::kv::tests::insert_non_object_is_422_json` | src/http/kv.rs |
| 422 non-string value (nothing inserted) | pass | `http::kv::tests::insert_non_string_value_is_422_and_inserts_nothing` | src/http/kv.rs |
| 422 JS exception | pass | `http::tests::test_patch_throw_is_422_js_error` | src/http/mod.rs |
| 422 depth limit, direct self-patch | pass | `http::tests::depth_limit_self_patch_is_422` | src/http/mod.rs |
| 422 depth limit, A -> B -> A cycle | pass | `http::tests::depth_limit_cycle_is_422` | src/http/mod.rs |
| 422 depth limit, caught but still 422 | pass | `http::tests::depth_limit_caught_is_still_422` | src/http/mod.rs |
| 504 timeout, `while(true){}` | pass | `http::tests::timeout_infinite_loop_is_504_then_worker_serves_next` | src/http/mod.rs |
| 504 timeout, nested infinite loop | pass | `http::tests::timeout_nested_loop_is_504` | src/http/mod.rs |
| Timed-out worker serves the next job | pass | `js::executor::tests::timed_out_worker_serves_next_job` | src/js/executor.rs |

Running exactly these 13 tests reports `test result: ok. 13 passed`.

### Other new Phase 5 tests

| Plan | Test | File | What it pins |
|------|------|------|--------------|
| 05-01 | `depth_limit_finite_chain_is_422` | src/http/mod.rs | k1..k6 chain answers the exact 422 body naming `k1 -> .. -> k5` |
| 05-01 | `depth_limit_allows_exactly_the_max_levels` | src/http/mod.rs | a 4-level chain answers 200 `"4"` |
| 05-01 | `depth_limit_is_repeatable_and_server_keeps_serving` | src/http/mod.rs | byte-identical 422 bodies, then GET / and 6*20 still answer |
| 05-01 | `depth_stop_refuses_later_ops_and_keeps_earlier_writes` | src/js/ops.rs | DepthLimit; earlier write stays; later write refused |
| 05-01 | `every_op_throws_the_depth_error_after_the_stop` | src/js/ops.rs | all five ops throw the depth error; store unchanged |
| 05-01 | `depth_limit_chain_names_a_missing_key` | src/js/ops.rs | refused before any store read; chain ends with the missing key |
| 05-01 | `depth_stop_is_logged_once` | src/js/ops.rs | caught and uncaught stops each log one line |
| 05-01 | `classify_precedence_host_fault_then_depth_then_result` | src/js/executor.rs | host fault > depth > result |
| 05-01 | `max_patch_depth_is_clamped` | src/js/executor.rs | 0 -> 1, 65 -> 64, u32::MAX -> 64 |
| 05-01 | `max_patch_depth_from_settings` | src/js/executor.rs | limits 2, 1 and 0 refuse at the right level |
| 05-01 | `max_depth_leaf_recursion_is_a_catchable_range_error` | src/js/executor.rs | 4 levels plus deep leaf recursion give Ok("true") on 16 MiB |
| 05-02 | `timeout_bounded_busy_loop_is_504` | src/http/mod.rs | a bounded 1500 ms loop answers 504 under a 300 ms limit |
| 05-02 | `timeout_does_not_block_other_requests` | src/http/mod.rs | GET / and PATCH m complete before the loop's 504 |
| 05-02 | `timeout_queued_patch_runs_once_the_worker_frees_up` | src/http/mod.rs | queue wait does not count against the queued PATCH |
| 05-02 | `job_state_phase_changes_exactly_once` | src/js/mod.rs | fire/finish CAS decides the outcome once |
| 05-02 | `timeout_refuses_writes_after_stop` | src/js/ops.rs | no write after a caught terminated nested PATCH |
| 05-02 | `timeout_nested_loop_is_timeout` | src/js/ops.rs | nested loop gives Timeout { 300 } |
| 05-02 | `timeout_survives_caught_nested_termination` | src/js/ops.rs | catch then loop still times out |
| 05-02 | `timeout_throwing_op_loop_is_timeout` | src/js/ops.rs | a loop over throwing ops still times out |
| 05-02 | `timeout_during_stringify_is_timeout` | src/js/ops.rs | a looping toString times out |
| 05-02 | `timeout_is_logged_once` | src/js/ops.rs | one log line per timed-out job |
| 05-02 | `timeout_beats_depth_limit` | src/js/ops.rs | Timeout wins over a recorded depth stop |
| 05-02 | `host_fault_beats_timeout` | src/js/ops.rs | Internal wins over Timeout |
| 05-02 | `late_watchdog_never_hits_next_job` | src/js/executor.rs | 12 near-deadline rounds; every canary Ok("2") |
| 05-02 | `classify_precedence_with_timeout` | src/js/executor.rs | host fault > timeout > depth > result |
| 05-02 | `backstop_answers_timeout_when_the_worker_never_replies` | src/js/executor.rs | a Rust stall answers Timeout at about timeout + 2 s |
| 05-02 | `timeout_ms_is_clamped` | src/js/executor.rs | 0 -> 1, 5000 -> 5000 |
| 05-03 | `print_retries_would_block` | src/js/ops.rs | Interrupted, WouldBlock x2 and a WouldBlock flush are retried; bytes written once; flush attempted twice |
| 05-03 | `print_gives_up_on_other_errors` | src/js/ops.rs | BrokenPipe on write or flush is returned; Ok(0) is WriteZero; no loop |
| 05-03 | `stack_bytes_are_clamped` | src/js/executor.rs | 0 -> 2 MiB, 2 MiB -> 2 MiB, 16 MiB -> 16 MiB (pure) |
| 05-03 | `minimum_stack_runs_the_max_depth_chain` | src/js/executor.rs | stack_bytes 0 (clamped to 2 MiB) runs 4 levels plus deep leaf recursion: Ok("true") |
| 05-03 | `txt_payload_fac_is_120` | src/http/mod.rs | requests/fac.txt via POST /insert, PATCH fac: `"120"` |
| 05-03 | `txt_payload_sum_is_10` | src/http/mod.rs | requests/sum.txt: `"10"` |
| 05-03 | `txt_payload_bin_is_tree_json` | src/http/mod.rs | requests/bin.txt: the binary-tree JSON body |
| 05-03 | `txt_payload_store_writes_res` | src/http/mod.rs | requests/store.txt: `{"res":"1111"}`, then GET /key/res `"1111"` |

## D-13 carry-over mapping

| Item | Change | Commit | Tests |
|------|--------|--------|-------|
| IN-04 | `write_all_retrying` / `flush_retrying` behind both op_print streams. `Interrupted` retries, `WouldBlock` yields and retries, `Ok(0)` is `WriteZero`, other errors are returned, and nothing unwraps. The signature and middleware registration are unchanged. | `f8f764b` (RED), `9bb6833` | `print_retries_would_block`, `print_gives_up_on_other_errors`, `print_to_either_stream_keeps_running` |
| IN-05 | The `.txt` payloads are inserted through POST /insert and PATCHed. Their bodies match the `.js` fixtures. | `9bb6833` | `txt_payload_{fac_is_120,sum_is_10,bin_is_tree_json,store_writes_res}` |
| IN-06 | The `worker_loop` doc now says that catch_unwind covers job code outside V8 callbacks, and that a panic inside an op callback aborts the process. | `9bb6833` | doc only (acceptance grep: the old sentence is gone) |
| IN-07 | `MIN_STACK_BYTES` (2 MiB) and `effective_stack_bytes`, documented in `start()` and on `JsSettings::stack_bytes`. | `9bb6833` | `stack_bytes_are_clamped`, `minimum_stack_runs_the_max_depth_chain` |

## TDD (Task 2)

- **RED:** `js::ops::tests::print_retries_would_block` against stand-in helpers that delegate to std. It failed on its planned Ok assertion with `write gave up: Err(Kind(WouldBlock))`, because std `write_all` retried `Interrupted` and gave up on `WouldBlock`.
  - JUnit run of the single target: tests=1, failures=1, exit 101.
  - `gsd-tools check tdd-red-evidence` answered **RED_EVIDENCE_OK** (`target_test_failed`).
  - Semantic assessment: the target compiled, ran and failed for the intended IN-04 reason. There was no setup or compile fault.
  - Evidence: `tdd-evidence/05-03-task2-red.{junit.xml,json}`.
- **GREEN:** the retry loops, the stack clamp, the doc fix and the parity tests. The target suite went to 9 passed and the full suite to 212.
- **REFACTOR:** none needed.

## TDD Gate Compliance

The RED gate commit `test(05-03)` `f8f764b` is present. The GREEN commit is `fix(05-03)` `9bb6833`, not `feat(05-03)`, because the plan's action names that commit type for the carry-over fixes. A `feat(05-03)` grep will therefore find nothing. The RED-then-GREEN order is intact.

## Files Created/Modified

- `scripts/ci-smoke.sh`:
  - new depth, timeout and concurrent-request assertions
  - `LOOP_OUT` / `LOOP_PID`, with cleanup in the EXIT trap
  - nproc guard and new header comment
- `src/js/ops.rs`:
  - `write_all_retrying` and `flush_retrying`; `op_rs3_print` writes and flushes through them, and its doc names the undrained-pipe residual
  - test sinks `FlakySink` and `ZeroSink`, plus two new tests
- `src/js/executor.rs`:
  - `MIN_STACK_BYTES` and `effective_stack_bytes`; the spawn uses the clamp, and the `start()` doc names the floor
  - corrected `worker_loop` doc
  - two new tests
- `src/js/mod.rs`: the `JsSettings::stack_bytes` doc names the 2 MiB floor.
- `src/http/mod.rs`: `*_TXT` constants, `insert_txt_req`, `patch_txt_payload` and four parity tests.
- `docs/js-ops.md`: limits, errors, PATCH results with precedence, the op table row and the intro.
- `.planning/phases/05-js-execution-limits/05-VALIDATION.md`: nine rows green, `wave_0_complete: true`, Wave 0 boxes ticked.
- `.planning/phases/05-js-execution-limits/tdd-evidence/05-03-task2-red.{junit.xml,json}`: RED evidence.

## Decisions Made

- The smoke records the loop's start time right after it launches curl. Next to `kill -0 "$LOOP_PID"`, it also requires `LOOP_OUT` to be empty when PATCH /key/m returns. That is stricter than the plan, never weaker.
- The stderr print formats one `js stderr: <Debug>` line plus a newline into a String, then writes and flushes it through the helpers. `print_to_either_stream_keeps_running` showed that the output is unchanged.
- `write_all_retrying` advances with `bytes.get(n..).unwrap_or_default()`. A writer that claims more bytes than it was given ends the loop instead of panicking the worker.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 2 - Robustness] The smoke also checks that the loop's output file is empty during the concurrent PATCH**
- **Found during:** Task 1
- **Issue:** `kill -0` succeeds on an exited child that bash has not reaped yet, so on its own it could miss a loop that had already answered.
- **Fix:** The check also requires `[[ ! -s "$LOOP_OUT" ]]`, because curl writes its output only when the response arrives.
- **Files modified:** scripts/ci-smoke.sh
- **Verification:** the smoke passed three times (ports 18533, 18533 and 18534).
- **Committed in:** `01c8c3a`

**2. [Rule 3 - Minor] The RED stand-ins were already wired into op_rs3_print's stdout path**
- **Found during:** Task 2 (RED)
- **Issue:** Helpers that only tests use would be dead code in the non-test build.
- **Fix:** In the RED commit, the stdout path called the stand-ins, which kept std behaviour. GREEN then replaced their bodies and moved stderr onto them too.
- **Files modified:** src/js/ops.rs
- **Committed in:** `f8f764b`

**3. [Rule 3 - Minor] minimum_stack_runs_the_max_depth_chain sets queue_capacity 4**
- **Found during:** Task 2
- **Issue:** The behaviour block shows `workers: 1, stack_bytes: 0, ..JsSettings::default()`, and the default queue capacity is sized for every core.
- **Fix:** Set `queue_capacity: 4` and keep `..JsSettings::default()` for the rest, the same sizing as plans 05-01 and 05-02. The assertions are unchanged.
- **Files modified:** src/js/executor.rs
- **Committed in:** `9bb6833`

---

**Total deviations:** 3 auto-fixed (1 robustness, 2 minor)
**Impact on plan:** None on contracts. Every planned body, status and assertion is as specified.

## Issues Encountered

None.

## Threat Mitigations

- **T-05-11:** the docs state that writes made before either stop stay, that both limits are set in code, the exact defaults and bodies, and the backstop and heap residuals.
- **T-05-12:** the retry helpers, pinned by `print_retries_would_block` and `print_gives_up_on_other_errors`. The undrained-pipe stall is documented as a residual.
- **T-05-13:** the 2 MiB clamp, pinned by `stack_bytes_are_clamped` and `minimum_stack_runs_the_max_depth_chain`.
- **T-05-14:** the server binds `SERVER.HOST=127.0.0.1`. The EXIT trap kills and reaps both the server and `LOOP_PID` and removes both temp files. The rs3 and curl process counts were unchanged after each run.
- **T-05-15:** the remote run is a pending item. No 05-UAT.md was written, no git config was changed and nothing was pushed.
- **T-05-SC:** no crate was added. Cargo.toml is untouched in this plan, `git diff --exit-code 449aaf2 -- Cargo.lock` is clean, and every cargo call used `--locked`.

## Pending human UAT (for 05-UAT.md)

### 1. Remote CI run of the Phase 5 HEAD on the x86_64 runner (ROBU-05, ROBU-06, TEST-04; assumption A1)
expected: After this phase the operator pushes modernize-toolchain (no push happens in this run) and confirms that the CI job is green on the x86_64 runner. The Release smoke must show the lines "PATCH /key/me -> 422 js_depth_limit", "GET / during a runaway PATCH -> 200" and "PATCH /key/loop -> 504 js_timeout after N ms" on the protected V8 platform. That run proves assumption A1: the watchdog thread can terminate isolates under PKU. If the smoke reports "server died (status 139)", the D-07 fallback from Phase 4 applies.
result: [pending]

The remote x86_64 run was not observed in this phase: nothing was pushed, so the protected-platform confirmation stays open until this item passes. The same push also satisfies 04-UAT item 1 (re-run CI on the hardened HEAD).

## Requirements

ROBU-05, ROBU-06 and TEST-04 are complete locally: depth limit, timeout, and every error path has a named passing test under every thread setting. The remote x86_64 confirmation is pending UAT (above).

## User Setup Required

None. No external service configuration is required.

## Next Phase Readiness

- Phase 5 is complete. Its plans are done, the local gate is green and the validation map is green. Next steps are verify-work and validate-phase for Phase 5, then Phase 6.
- Open: the remote x86_64 CI run (assumption A1) waits for an operator push.

---
*Phase: 05-js-execution-limits*
*Completed: 2026-10-21*

## Self-Check: PASSED

- All 7 modified files and both evidence files exist.
- Commits 01c8c3a, f8f764b, 9bb6833 and 00c3428 are ancestors of HEAD, and evaluation-scope resolves the plan's commits on this branch.
- The acceptance criteria of all three tasks, the phase gate, the serial run, the five repeats, the env -i run, the 13-test TEST-04 run and the scope checks were re-run and passed. The RED record answers RED_EVIDENCE_OK.
