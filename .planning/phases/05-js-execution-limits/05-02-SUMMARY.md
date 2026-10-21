---
phase: 05-js-execution-limits
plan: 02
subsystem: js-runtime
tags: [deno_core, v8, terminate_execution, watchdog, timeout, actix-web, tokio, tdd]

# Dependency graph
requires:
  - phase: 05-js-execution-limits
    provides: "05-01 JobState (host fault, depth stop, log sink), ExecCtx { job, chain }, classify(), ApiError::JsDepthLimit"
  - phase: 03-javascript-runtime-port
    provides: JsExecutor pool (rs3-js-N workers, bounded queue), run_script/run_fresh, ApiError mapping
provides:
  - ExecError::Timeout { limit_ms } and ApiError::JsTimeout -> 504 js_timeout
  - JsSettings.timeout_ms (default 5000, built in code, clamped to at least 1)
  - JobState phase CAS (Running -> Finished | TimedOut) and isolate registry (register, unregister, fire, finish, timed_out) with the Registered guard
  - every runtime (top-level and nested) joins its job's termination group in run_fresh
  - one rs3-js-N-wd watchdog thread per worker (Arm/Disarm, recv_timeout, 50 ms re-fire)
  - classify in worker_loop with host fault > timeout > depth > result
  - silent ops after a timeout; op_rs3_patch post-run timeout check
  - JsExecutor::run_job(job, label, f) with the D-07 backstop (timeout_ms + 2 s from pickup)
  - cfg(test) dedicated_executor(settings) helper
affects: [05-03 docs (Limits section, residuals), 05-03 release smoke on x86 (assumption A1), phase 6 README]

# Actuals (#2632)
actuals:
  tokens: 15283
  tasks: 3
  commits: 6
plan_head_before: aed4b9f1379ddee8188f827f83ac29d84f300be3
plan_head_after: fd45bf6d328390155d5c3c931a07fe9fd233ae52

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Per-job termination group: JobState holds the IsolateHandles of every live runtime; Registered (RAII) registers right after JsRuntime::try_new and unregisters on drop"
    - "Per-worker watchdog thread spawned from inside the worker closure; worker arms at pickup, finish() CAS decides the outcome, then Disarm before the next job"
    - "Never throw into a terminating script: ops return Ok(\"\") once the job timed out"
    - "Backstop counted from a started signal at pickup, never from enqueue"

key-files:
  created:
    - .planning/phases/05-js-execution-limits/tdd-evidence/05-02-task1-red.json
    - .planning/phases/05-js-execution-limits/tdd-evidence/05-02-task1-red.junit.xml
    - .planning/phases/05-js-execution-limits/tdd-evidence/05-02-task2-red.json
    - .planning/phases/05-js-execution-limits/tdd-evidence/05-02-task2-red.junit.xml
    - .planning/phases/05-js-execution-limits/tdd-evidence/05-02-task3-red.json
    - .planning/phases/05-js-execution-limits/tdd-evidence/05-02-task3-red.junit.xml
  modified:
    - Cargo.toml
    - src/js/mod.rs
    - src/js/runtime.rs
    - src/js/executor.rs
    - src/js/ops.rs
    - src/http/error.rs
    - src/http/mod.rs

key-decisions:
  - "Outcome by the phase CAS only: finish() returning true means Timeout even if the script computed a value; nothing matches on the 'execution terminated' text"
  - "The watchdog goes idle after a fire that loses to finish(), instead of spinning with a zero recv_timeout until Disarm arrives"
  - "A deadline that does not fit in Instant (absurd timeout_ms) runs the job unarmed instead of panicking the worker; the backstop limit uses saturating_add"
  - "Workers format their own ready errors, so a watchdog spawn failure is not reported as a runtime build failure"
  - "The backstop log line names the full wait (timeout_ms + 2000 ms), e.g. 'error: JS worker did not reply within 2100 ms' for a 100 ms limit"
  - "The 504 message comes from ExecError::Timeout Display; From<ExecError> adds no second log line"

patterns-established:
  - "Pattern: armed test helpers (run_ctx_on, dedicated_executor) for every script that can loop; run_ctx stays for scripts that always finish"
  - "Pattern: per-worker helper threads are spawned from inside the worker closure (V8 thread ancestry, research Pitfall 10)"

requirements-completed: [ROBU-06, TEST-04]

coverage:
  - id: D1
    description: "A runaway top-level script answers 504 js_timeout with the exact body after the configured limit, and the timed-out worker serves the next job"
    requirement: ROBU-06
    verification:
      - kind: integration
        ref: "src/http/mod.rs#timeout_bounded_busy_loop_is_504"
        status: pass
      - kind: integration
        ref: "src/http/mod.rs#timeout_infinite_loop_is_504_then_worker_serves_next"
        status: pass
      - kind: unit
        ref: "src/js/executor.rs#timed_out_worker_serves_next_job"
        status: pass
    human_judgment: false
  - id: D2
    description: "Termination cannot be dodged: caught nested termination, throwing-op loop, nested loop and looping toString all end as Timeout; no write after the stop; one log line"
    requirement: ROBU-06
    verification:
      - kind: unit
        ref: "src/js/ops.rs#timeout_refuses_writes_after_stop"
        status: pass
      - kind: unit
        ref: "src/js/ops.rs#timeout_survives_caught_nested_termination"
        status: pass
      - kind: unit
        ref: "src/js/ops.rs#timeout_throwing_op_loop_is_timeout"
        status: pass
      - kind: unit
        ref: "src/js/ops.rs#timeout_nested_loop_is_timeout"
        status: pass
      - kind: unit
        ref: "src/js/ops.rs#timeout_during_stringify_is_timeout"
        status: pass
      - kind: unit
        ref: "src/js/ops.rs#timeout_is_logged_once"
        status: pass
    human_judgment: false
  - id: D3
    description: "Classification once by flag with host fault > timeout > depth > result; the phase changes exactly once and a late watchdog never hits the next job"
    requirement: ROBU-06
    verification:
      - kind: unit
        ref: "src/js/executor.rs#classify_precedence_with_timeout"
        status: pass
      - kind: unit
        ref: "src/js/ops.rs#timeout_beats_depth_limit"
        status: pass
      - kind: unit
        ref: "src/js/ops.rs#host_fault_beats_timeout"
        status: pass
      - kind: unit
        ref: "src/js/mod.rs#job_state_phase_changes_exactly_once"
        status: pass
      - kind: unit
        ref: "src/js/executor.rs#late_watchdog_never_hits_next_job"
        status: pass
    human_judgment: false
  - id: D4
    description: "Other requests keep being answered while a script loops; a queued PATCH runs once the worker frees up; a nested loop answers 504 over HTTP; the backstop answers 504 for a worker that never replies"
    requirement: ROBU-06
    verification:
      - kind: integration
        ref: "src/http/mod.rs#timeout_does_not_block_other_requests"
        status: pass
      - kind: integration
        ref: "src/http/mod.rs#timeout_queued_patch_runs_once_the_worker_frees_up"
        status: pass
      - kind: integration
        ref: "src/http/mod.rs#timeout_nested_loop_is_504"
        status: pass
      - kind: unit
        ref: "src/js/executor.rs#backstop_answers_timeout_when_the_worker_never_replies"
        status: pass
    human_judgment: false
  - id: D5
    description: "timeout_ms comes from JsSettings in code (default 5000, clamped to at least 1); the suite passes at default threads, on one thread and under env -i; Cargo.lock byte-identical; full CI gate and release smoke green"
    requirement: TEST-04
    verification:
      - kind: unit
        ref: "src/js/executor.rs#timeout_ms_is_clamped"
        status: pass
      - kind: unit
        ref: "src/js/executor.rs#default_settings_follow_d06"
        status: pass
      - kind: other
        ref: "bash scripts/ci-guards.sh && cargo fmt --check && cargo clippy --all-targets --locked -- -D warnings && cargo test --locked && cargo build --release --locked && RS3_SMOKE_PORT=18532 bash scripts/ci-smoke.sh"
        status: pass
      - kind: other
        ref: "cargo test --locked -- --test-threads=1"
        status: pass
      - kind: other
        ref: "git diff --exit-code 449aaf2 -- Cargo.lock"
        status: pass
    human_judgment: false
  - id: D6
    description: "No test, RED step or smoke ran an un-armed infinite loop (TEST-04 safety prohibition)"
    requirement: TEST-04
    verification: []
    human_judgment: true
    rationale: "Judgment-tier prohibition: the commit order is the evidence (the RED tests in 66a5c39, 025a987 and 3e2a16e use a Date.now()-bounded loop, straight-line code after a catch and a bounded Rust sleep; while(true){} appears only from 7d704f7 on, through execute, HTTP and run_ctx_on), but no automated check can prove the absence of an earlier unsafe run"
  - id: D7
    description: "terminate_execution from the watchdog thread is safe on the protected PKU platform on x86_64 (assumption A1)"
    requirement: ROBU-06
    verification: []
    human_judgment: true
    rationale: "Only provable by the x86_64 release smoke that plan 05-03 extends and hands to the operator; this aarch64 host runs the release smoke without the loop scenario"

# Metrics
duration: 11min
completed: 2026-10-21
status: complete
---

# Phase 5 Plan 02: JS Execution Timeout Summary

**A per-job termination group, one `rs3-js-N-wd` watchdog thread per worker with a 50 ms re-fire, silent ops after the stop and a phase compare-and-swap turn `while(true){}` (top-level, nested, caught or looping over throwing ops) into a 504 `js_timeout` after `timeout_ms` (default 5000, built in code). A backstop of timeout + 2 s, counted from pickup, answers for workers stuck in Rust, and other requests keep being answered the whole time.**

## Performance

- **Duration:** about 11 min
- **Started:** 2026-10-21T00:24:54Z
- **Completed:** 2026-10-21T00:35:46Z
- **Tasks:** 3 (each RED then GREEN)
- **Files modified:** 7 source and manifest files, plus 6 evidence files created

## Accomplishments

- `run_fresh` registers every runtime's `IsolateHandle` in its job's `JobState` right after `JsRuntime::try_new`, through the `Registered` RAII guard. A runtime created after the job timed out is terminated at registration.
- Each worker spawns its own `rs3-js-N-wd` watchdog from inside the worker closure. The worker arms it at pickup (deadline = pickup + `timeout_ms`), runs the job, calls `finish()` and disarms the watchdog before taking the next job. On the deadline the watchdog CASes Running to TimedOut first and then terminates every registered isolate. It terminates them again every 50 ms until it is disarmed.
- `classify` moved into `worker_loop`, with precedence host fault > timeout > depth > result, decided by `finish()`. A computed value does not survive a stop. One timed-out job writes exactly one log line.
- Once the job timed out, every rs3 op returns `""` and touches nothing, and never throws. `op_rs3_patch` also returns `""` when the job timed out during its nested run. This closes the research S4/S5 escape, where an op's `Err` made deno_core cancel the termination.
- `run_job` awaits a `started` signal and then waits for the reply for at most `timeout_ms + BACKSTOP_GRACE` (2 s). Queue wait is not timed.
- `ApiError::JsTimeout` maps to 504 `js_timeout` in an exhaustive match with no wildcard.

## Task Commits

| Task | RED (test) | GREEN (feat) | RED verdict |
|------|------------|--------------|-------------|
| 1 (tracer): a runaway top-level script answers 504 end to end; the worker serves the next job | `66a5c39` | `7d704f7` | RED_EVIDENCE_OK (`http::tests#timeout_bounded_busy_loop_is_504`, 200 vs 504) |
| 2: termination cannot be dodged; ops go silent; a late watchdog never hits the next job | `025a987` | `b844f7a` | RED_EVIDENCE_OK (`js::ops::tests#timeout_refuses_writes_after_stop`, write-after-stop assertion) |
| 3: backstop; other requests keep being answered | `3e2a16e` (includes the Cargo.toml feature declarations) | `fd45bf6` | RED_EVIDENCE_OK (`js::executor::tests#backstop_answers_timeout_when_the_worker_never_replies`, elapsed 2.40 s) |

No REFACTOR commits were needed. The tracer feedback gate ran in auto mode: Task 1's verify was re-run after the GREEN commit, passed (5 passed), and expansion continued.

## New timeout tests

| Test | File | Settings (workers, timeout_ms) | What it pins |
|------|------|--------------------------------|--------------|
| `timeout_bounded_busy_loop_is_504` | src/http/mod.rs | 1, 300 | RED test: a Date.now()-bounded 1500 ms loop answers 504 with the exact body, application/json, in under 1500 ms |
| `timeout_infinite_loop_is_504_then_worker_serves_next` | src/http/mod.rs | 1, 300 | `while(true){}` answers 504 with the exact body in 300 to 1300 ms; then `1+1` on the same app answers 200 "2" |
| `timeout_does_not_block_other_requests` | src/http/mod.rs | 2, 1000 | with the loop in flight, GET / (200 "Welcome to rs3") and PATCH m (200 "120") complete before the loop's response and under 1000 ms; the loop answers 504 at 1000 ms or later |
| `timeout_queued_patch_runs_once_the_worker_frees_up` | src/http/mod.rs | 1, 500 | the loop answers 504 (500 ms body); the queued PATCH m answers 200 "120" after it, at 500 ms or later (queue wait does not count against its timeout) |
| `timeout_nested_loop_is_504` | src/http/mod.rs | 1, 300 | `outer -> loop` answers 504 with the exact 300 ms body |
| `job_state_phase_changes_exactly_once` | src/js/mod.rs | pure | fire then finish reports a timeout and the job stays timed out; finish then fire leaves the job untouched (not timed out, not stopped) |
| `timeout_refuses_writes_after_stop` | src/js/ops.rs | 1, 300 | RED test: a caught terminated nested PATCH followed by an insert gives Timeout, and `after` is never written |
| `timeout_nested_loop_is_timeout` | src/js/ops.rs | 1, 300 | `op_rs3_patch('loop')` gives Timeout { 300 } |
| `timeout_survives_caught_nested_termination` | src/js/ops.rs | 1, 300 | S4: catch then `while (true) {}` gives Timeout in under 1300 ms |
| `timeout_throwing_op_loop_is_timeout` | src/js/ops.rs | 1, 300 | S5: `for(;;){ try { op_rs3_get('missing') } catch {} }` gives Timeout in under 1300 ms, three runs on one executor |
| `timeout_during_stringify_is_timeout` | src/js/ops.rs | 1, 300 | S8: a looping `toString` gives Timeout |
| `timeout_is_logged_once` | src/js/ops.rs | 1, 300 | the uncaught nested loop and the caught variant each give `error_log_count() == 1` |
| `timeout_beats_depth_limit` | src/js/ops.rs | 1, 300 | a self-patch caught and then a loop gives Timeout, not DepthLimit (the depth stop is still recorded) |
| `host_fault_beats_timeout` | src/js/ops.rs | 1, 300 | an injected nested runtime failure, caught and then a loop, gives Internal (the job is timed out too) |
| `timed_out_worker_serves_next_job` | src/js/executor.rs | 1, 300 | three rounds: `while(true){}` gives Timeout { 300 }, then `1+1` gives Ok("2") |
| `late_watchdog_never_hits_next_job` | src/js/executor.rs | 1, 250 | 12 near-deadline rounds (235/245/250/255/265 ms): each gives Ok("undefined") or Timeout { 250 } and nothing else; every canary is Ok("2") |
| `classify_precedence_with_timeout` | src/js/executor.rs | pure | host fault + timed out gives Internal; timed out + depth stop gives Timeout; timed out with Ok("x") gives Timeout; Internal passes through |
| `backstop_answers_timeout_when_the_worker_never_replies` | src/js/executor.rs | 1, 100 | RED test: a 2400 ms Rust sleep gives Timeout { 100 } in 2100 to 2400 ms; the next `run` gives Ok("next") |
| `timeout_ms_is_clamped` | src/js/executor.rs | pure | 0 -> 1, 1 -> 1, 5000 -> 5000 |
| `default_settings_follow_d06` (extended) | src/js/executor.rs | pure | `timeout_ms == 5000` |
| `classify_precedence_host_fault_then_depth_then_result` (updated) | src/js/executor.rs | pure | the plan 05-01 assertions with the new signature (timed_out false, 5000) |

## Observed 504 bodies and elapsed times

- `{"error":"js_timeout","message":"script exceeded the 300 ms execution limit"}` (busy loop, infinite loop, nested loop)
- `{"error":"js_timeout","message":"script exceeded the 1000 ms execution limit"}` (two-worker isolation test)
- `{"error":"js_timeout","message":"script exceeded the 500 ms execution limit"}` (queued PATCH test)
- Per-test wall time, each test run alone with `--report-time`:
  - bounded busy loop: 0.315 s
  - infinite loop plus the follow-up `1+1`: 0.324 s
  - nested loop over HTTP: 0.314 s
  - two-worker isolation: 1.011 s
  - queued PATCH: 0.526 s
  - throwing-op loop x3: 0.915 s
  - backstop: 2.402 s (the backstop answered at about 2.1 s; the rest is the stalled worker finishing before the follow-up job)
  - late watchdog, 12 rounds: 3.146 s
- Termination overshoot is a few ms past the limit, in line with research S1/S12.

## Log line formats

- Worker, once per timed-out job, written through the job's log sink: `error: js timeout in "loop" after 300 ms`. The label is Debug-formatted: the PATCHed key for `execute`, `"job"` for `run`, `"test"` for `run_ctx_on`.
- Backstop, caller side, plain stderr: `error: JS worker did not reply within 2100 ms` for a 100 ms limit. It names the full wait, `timeout_ms + 2000`.
- A timed-out job writes no per-level `js error in ...` lines, because `stopped()` now includes the timeout. `timeout_is_logged_once` pins this.

## Thread names seen

While `timeout_does_not_block_other_requests` ran, `/proc/<pid>/task/*/comm` listed `rs3-js-0` .. `rs3-js-3` and `rs3-js-0-wd` .. `rs3-js-3-wd` for the shared test executor (4 cores). The dedicated two-worker executor added a second `rs3-js-0`/`rs3-js-0-wd` and `rs3-js-1`/`rs3-js-1-wd` pair. Every worker had exactly one watchdog.

## Suite

- `cargo test --locked`: 204 passed (lib), 0 failed, in 4.0 s at the default thread count (about 9 s wall including the build check). It was repeated 3 times, all green, plus once under `env -i PATH=... HOME=...`.
- `cargo test --locked -- --test-threads=1`: 204 passed in 14.19 s.
- The full gate passed: `bash scripts/ci-guards.sh` ("all guards passed"), `cargo fmt --check`, `cargo clippy --all-targets --locked -- -D warnings`, `cargo test --locked`, `cargo build --release --locked`, `RS3_SMOKE_PORT=18532 bash scripts/ci-smoke.sh` ("ci-smoke: OK").
- `git diff --exit-code 449aaf2 -- Cargo.lock` printed nothing and exited 0, so Cargo.lock is byte-identical. Cargo.toml changed only in the two tokio feature declarations (`["rt", "sync", "time"]` in `[dependencies]`, `["macros"]` in `[dev-dependencies]`). Guard (d) still passes.
- The one "ignored" doctest is the deno_core `extension!` macro doc for `rs3_ext`, which predates this plan. No `#[allow]`, `#[expect]` or `#[ignore]` was added. No push, and no git config change.

## Files Created/Modified

- `src/js/mod.rs`:
  - `ExecError::Timeout { limit_ms }`
  - `JsSettings.timeout_ms` (default 5000)
  - `JobState` gains `phase`, `isolates`, `next_id`, `register`, `unregister`, `fire`, `finish` and `timed_out`; `stopped()` now includes the timeout
  - `Registered` guard
  - test helper `dedicated_executor`
  - tests module with `job_state_phase_changes_exactly_once`
- `src/js/runtime.rs`: `run_fresh` registers each runtime in the job's termination group. Docs updated.
- `src/js/executor.rs`:
  - `Envelope { job, label, started }`, `Limits.timeout_ms`, `Cmd`, `REFIRE`, `BACKSTOP_GRACE`, `watchdog_loop`
  - `rs3-js-N-wd` threads
  - `run_job` with the backstop; `run` delegates to it; `execute` owns its job
  - four-argument `classify` in `worker_loop`
  - new tests
- `src/js/ops.rs`: `live_ctx` returns `Option` (silent after a timeout). All five ops open with the let-else gate. `op_rs3_patch` gains the post-run timeout check and an explicit `Timeout` arm. New test helpers `run_ctx_on` and `timeout_exec`, plus the timeout tests.
- `src/http/error.rs`: `ApiError::JsTimeout` -> 504 `js_timeout`. Module doc updated.
- `src/http/mod.rs`: helpers `timeout_state` and `timeout_body`, plus five HTTP timeout tests.
- `Cargo.toml`: tokio feature declarations only.
- `.planning/phases/05-js-execution-limits/tdd-evidence/05-02-task{1,2,3}-red.{json,junit.xml}`: RED evidence.

## Decisions Made

- I followed the interfaces block. `JobState` stays in `src/js/mod.rs` next to the plan 05-01 state, so no `job.rs` was created.
- `run_ctx_on` runs a context as its own armed, classified job. The existing `run_ctx` on `run()` stays for scripts that always finish and whose raw completion value a test inspects.
- The `BACKSTOP_GRACE` and `REFIRE` constants are private, as research question 3 resolved.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] The watchdog goes idle after a fire that loses to `finish()`**
- **Found during:** Task 1
- **Issue:** In research Pattern 2, after the deadline fires on a job that already finished (phase Finished), `timed_out()` stays false, so the next `recv_timeout` gets a zero duration. The watchdog would spin until the worker's `Disarm` arrives.
- **Fix:** After `fire()`, the watchdog drops the armed job when it is not timed out, so nothing is left to terminate.
- **Files modified:** src/js/executor.rs
- **Verification:** `late_watchdog_never_hits_next_job` (12 near-deadline rounds, every canary Ok("2")) and the full suite.
- **Committed in:** 7d704f7

**2. [Rule 2 - Robustness] Deadline overflow cannot panic a worker**
- **Found during:** Task 1 and Task 3
- **Issue:** `Instant::now() + Duration::from_millis(timeout_ms)` panics for an absurd `timeout_ms`, and that panic would happen outside `catch_unwind` in the worker.
- **Fix:** `checked_add`: a deadline that does not fit runs the job unarmed (in effect, no limit). The backstop limit uses `saturating_add`.
- **Files modified:** src/js/executor.rs
- **Committed in:** 7d704f7, fd45bf6

**3. [Rule 1 - Accuracy] Workers format their own start errors**
- **Found during:** Task 1
- **Issue:** `start()` wrapped every worker-reported error as "cannot build JS worker runtime", which would mislabel a watchdog spawn failure.
- **Fix:** The worker sends `cannot spawn JS watchdog rs3-js-N-wd: ..` or `cannot build JS worker runtime: ..`, and `start()` passes it through. No test asserted the old text.
- **Files modified:** src/js/executor.rs
- **Committed in:** 7d704f7

**4. [Rule 3 - Minor] Test sizing and the backstop log value**
- **Found during:** Task 1 and Task 3
- **Issue and fix:**
  - `timeout_state` and `timeout_exec` set `queue_capacity` explicitly (16 for HTTP, 4 for js tests) and keep `..JsSettings::default()` for the rest.
  - The interfaces block's backstop line `within <ms> ms` is filled with the actual wait (`timeout_ms + 2000`), which is the time the caller really waited.
- **Files modified:** src/http/mod.rs, src/js/ops.rs, src/js/executor.rs
- **Committed in:** 7d704f7, b844f7a, fd45bf6

---

**Total deviations:** 4 auto-fixed (2 bug or accuracy fixes, 1 robustness fix, 1 minor sizing and wording choice)
**Impact on plan:** None on contracts. Every planned body, status, precedence and test assertion is as specified.

## Issues Encountered

None. Each RED failed on its planned assertion on the first run:
- Task 1: 200 vs 504.
- Task 2: the after write landed in all three probe runs.
- Task 3: elapsed was 2.40 s.

## Threat Mitigations

- T-05-04: the per-worker watchdog plus the termination group. Pinned by `timeout_infinite_loop_is_504_then_worker_serves_next` and `timed_out_worker_serves_next_job`.
- T-05-05: silent ops, the post-run check in `op_rs3_patch` and the 50 ms re-fire. Pinned by the S4, S5 and S8 tests.
- T-05-06: a per-job `Arc<JobState>`, `fire()` never touches a Finished job, and the worker disarms before the next job. Pinned by `late_watchdog_never_hits_next_job` and `job_state_phase_changes_exactly_once`.
- T-05-07: the watchdog is worker-side and armed at pickup, independent of the request future.
- T-05-08: the backstop at timeout + 2 s from pickup. Residual worker occupancy is documented for 05-03.
- T-05-09: the queued-PATCH test.
- T-05-SC: no crate was added and Cargo.lock is byte-identical.

## User Setup Required

None. No external service configuration is required.

## Next Phase Readiness

- Ready for 05-03:
  - rewrite the docs/js-ops.md Limits section (timeout 5000 ms / 504, depth, residuals: built-in throwing ops, Rust stalls, heap exhaustion)
  - extend `scripts/ci-smoke.sh` with the depth and loop scenarios (proves A1 on x86_64)
  - fold in the IN-04/05/06/07 carry-overs, including correcting the catch_unwind sentence in the `worker_loop` doc, which is unchanged on purpose
- Open: A1 (PKU on x86_64) is unproven until the x86 release smoke runs the loop scenario.

## Self-Check: PASSED

- All 7 modified source and manifest files and all 6 evidence files exist.
- Commits 66a5c39, 7d704f7, 025a987, b844f7a, 3e2a16e and fd45bf6 are ancestors of HEAD. evaluation-scope resolves the plan's commits on this branch.
- All three RED records answer RED_EVIDENCE_OK.
- The acceptance criteria of all three tasks, the plan gate, the serial run and the Cargo.lock diff were re-run and passed.

---
*Phase: 05-js-execution-limits*
*Completed: 2026-10-21*
