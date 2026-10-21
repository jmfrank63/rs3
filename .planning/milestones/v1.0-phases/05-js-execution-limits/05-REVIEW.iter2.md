---
phase: 05-js-execution-limits
reviewed: 2026-10-21T01:00:00Z
depth: standard
files_reviewed: 9
files_reviewed_list:
  - Cargo.toml
  - docs/js-ops.md
  - scripts/ci-smoke.sh
  - src/http/error.rs
  - src/http/mod.rs
  - src/js/executor.rs
  - src/js/mod.rs
  - src/js/ops.rs
  - src/js/runtime.rs
findings:
  critical: 0
  warning: 3
  info: 7
  total: 10
status: issues_found
---

# Phase 5: Code Review Report

**Reviewed:** 2026-10-21T01:00:00Z
**Depth:** standard
**Files Reviewed:** 9
**Status:** issues_found

## Summary

I reviewed the Phase 5 watchdog, termination group, classification, depth limit, backstop, docs and release smoke against D-01..D-14 and the resolved research questions. Where a claim could be checked, I checked it in a throwaway copy of the crate under `target/tmp`. That copy has been deleted, and no tracked file was changed.

**Verified sound (no finding):**
- **JobState CAS.** `fire()` and `finish()` share one `AtomicU8`, so a job ends either finished or timed out, never both.
- **No late kill.** Each job has its own `Arc<JobState>` and isolate list. A late `fire()` on job N cannot reach job N+1. The FIFO `Arm`/`Disarm` channel also keeps the watchdog's commands in order.
- **register vs fire.** `register()` checks `timed_out()` while holding the registry lock. In every interleaving with `fire()`, either `fire()` sees the handle or `register()` terminates it.
- **Drop order.** `_registered` is declared after `rt` in `run_fresh`, so it is dropped first. The isolate leaves the group before it is disposed.
- **Locks.** There is no lock-order inversion between the `isolates` mutex and V8's handle mutex, and a poisoned lock is recovered with `into_inner`.
- **Depth limit.** The `chain.len() >= max_patch_depth` check matches the documented contract: 4 scripts counting the PATCHed key, chain `me -> me -> me -> me -> me`. The clamp to 1 means no nested PATCH.
- **Precedence.** `classify` applies host fault > timeout > depth > result.
- **Production code never unwraps.**

**Tested empirically in the scratch copy:**
- A tight loop over a throwing deno_core built-in op (`op_close(987654)`) never escaped termination in 10 of 10 runs. Neither did a loop over a throwing argument conversion (`op_rs3_get({})`), also 10 of 10. Both throw through deno_core's native error builder, and the 50 ms re-fire covered the few late cases (reply at 351 to 419 ms against a 300 ms limit). The worker served a canary job right after every run.
- The full suite passed 3 times in a row and 4 times in parallel, at about 5.5 s per run.

**Problems found:**
- The release smoke cannot tell a working watchdog from the backstop.
- A nested host fault that coincides with a timeout is silently dropped: it is not logged, and the request answers 504 instead of 500.
- A client can trigger a "host fault" 500 by exhausting the JS stack before calling `op_rs3_patch`. I reproduced this.

## Warnings

### WR-01: The smoke's timeout window includes the backstop, so a broken watchdog still passes

**File:** `scripts/ci-smoke.sh:189-197` (also the header comment at `:23-26`)
**Issue:** The header says this smoke is "the only place where the timeout watchdog terminates a script under the protected V8 platform". Its assertions cannot prove that.

- **Timing window.** The loop check accepts a 504 with the exact `js_timeout` body anywhere in `4900 <= LOOP_MS < 9000`. The backstop (`run_job`, `executor.rs:229-241`) answers the same status and the same body at about `timeout_ms + BACKSTOP_GRACE = 7000 ms`, which is inside that window.
- **Follow-up request.** The later `PATCH /key/m after the timeout` check runs on a pool with one worker per core. On any runner with `nproc >= 2` (the CI runner included), another worker serves it.

**Consequence:** Suppose `terminate_execution()` from the `-wd` thread silently did nothing on x86_64 with the protected platform (a PKU or thread-ancestry problem, which is exactly what this check exists to catch). The smoke would still see a 504 at about 7 s and a 200 for `m`, and would print `OK`. Meanwhile the server would have a JS worker spinning forever.

**Fix:** Bound the window below the backstop, and assert which path answered:
```bash
(( LOOP_MS >= 4900 && LOOP_MS < 6500 )) || fail "PATCH /key/loop -> 504 after ${LOOP_MS} ms (expected 4900 to 6499 ms; ~7000 means the backstop answered, not the watchdog)"
grep -q 'error: js timeout in "loop" after 5000 ms' "$LOG" || fail "watchdog did not log the timeout"
! grep -q 'JS worker did not reply' "$LOG" || fail "the backstop answered PATCH /key/loop; the watchdog did not terminate it"
```
The `error: js timeout in "loop"` line is written by the worker only after the job returns. If it is present before the final `PATCH /key/m`, the timed-out worker really was freed.

### WR-02: A nested host fault is dropped when the job times out during the nested run

**File:** `src/js/ops.rs:298-303`
**Issue:** `op_rs3_patch` returns `Ok(String::new())` as soon as `ctx.job.timed_out()` is true, before it looks at `result`. Suppose the nested `run_script` returned `ExecError::Internal`, for example because the nested `JsRuntime::try_new` failed (see WR-03 for a client-triggerable cause), and the watchdog fired while that level was running. Then:
- `note_host_fault()` is never called, so `classify` answers 504 `js_timeout` instead of 500 `internal`. This violates D-04: a host fault must beat a timeout.
- The fault is logged nowhere. The `log_error` in the `other` arm is skipped, and `ApiError::from(Timeout)` does not log. The only host failure of the request leaves no trace in stderr.

The window is narrow (the fault and the timeout have to overlap at the level where the fault happens), but the precedence contract is stated as unconditional, and the test `host_fault_beats_timeout` covers only a fault that happens before the timeout.

**Fix:** Record a host fault before applying the "never throw into a terminating script" rule:
```rust
let result = super::runtime::run_script(ctx.nested(&key), &key, source);
if let Err(e @ (ExecError::Internal(_) | ExecError::Unavailable)) = &result
    && !ctx.host_fault()
{
    let detail = match e { ExecError::Internal(m) => m.clone(), e => e.to_string() };
    ctx.log_error(format_args!(
        "error: nested JS host failure in {key:?} at depth {}: {detail:?}", ctx.depth + 1));
    ctx.note_host_fault();
}
// Never throw into a terminating script: an Err here would cancel its termination.
if ctx.job.timed_out() {
    return Ok(String::new());
}
```
Then simplify the `other` arm so it only returns `internal_error()`.

### WR-03: A client can make a request answer 500 "host fault" by exhausting the JS stack before nesting

**File:** `src/js/runtime.rs:68-72`, `src/js/ops.rs:316-336`. Documented as intended in `src/js/executor.rs:39-41` and `src/js/mod.rs:228-230` ("V8's own stack guard would end the chain first, as a 500").
**Issue:** I reproduced this in the scratch copy with the default settings (16 MiB stack, depth 4). Each stored key recurses until it is just short of V8's stack limit and then calls `op_rs3_patch` on the next key. The third runtime cannot be created (`cannot create JS runtime: Failed to execute ext:core/00_primordials.js`). The same happens on the 2 MiB floor, already at depth 1. The failure takes the host-fault path:
- the server logs `error: nested JS host failure in "k3" at depth 2`;
- the job is poisoned, so every later op throws `Error: internal error`;
- the request answers **500 `internal`**, even if the script catches the error.

The process did not crash, and the worker served canary jobs normally afterwards, so this is not an availability issue. It is a misclassification: a condition the script itself causes (and which is repeatable from two stored keys) is reported to the client and to operators as a server fault. That undermines the D-04/500 contract ("a runtime cannot be created" means the host is broken), and it creates false alarms in the logs. The path predates this phase, but this phase writes the "as a 500" behaviour into the limits documentation.

**Fix:** Tell a bootstrap that failed under JS stack pressure apart from a real host failure. `JsRuntime::try_new` returns a `CoreError`; when the error is a JS exception during bootstrap (the `CoreErrorKind::Js(..)` variant) in a nested level (`ctx.depth > 0`), map it to a script error that `op_rs3_patch` rethrows as a catchable `Error` (for example `RangeError: not enough stack to start a nested script`). The request then answers 422 `js_error` when the script does not catch it. Keep `ExecError::Internal` for non-JS creation failures and for depth 0. Add a test that uses a two-key deep-recursion chain like the one above, and update the "as a 500" wording in `executor.rs`, `mod.rs` and `docs/js-ops.md`.

## Info

### IN-01: The backstop's log line has no job label, and the timeout is logged twice

**File:** `src/js/executor.rs:233-237`, `src/http/error.rs:40-42`
**Issue:** `error: JS worker did not reply within N ms` does not say which key stalled. When the stalled worker finally returns, `worker_loop` also logs `error: js timeout in "<key>" ...`, because the watchdog fired. One request therefore produces two timeout lines, which contradicts the `From<ExecError>` doc ("the JS worker or the backstop already logged it once").
**Fix:** Pass the label into the backstop branch (`label` is moved into the `Envelope`, so clone it first) and log `{label:?}`. Either reword the doc comment, or skip the worker's line when the reply send fails.

### IN-02: The ops table and the timeout section in the docs are incomplete or inaccurate

**File:** `docs/js-ops.md:25-28`, `:89-92`, `:197-199`, `:210-212`
**Issue:**
- **Ops table.** The "Throws when" column for `op_rs3_list` says "only after a nested host fault". The `op_rs3_get`/`op_rs3_insert`/`op_rs3_delete` rows also leave out the depth-stop error and the empty-string answer after a timeout. Those are covered only in the intro and the Errors section.
- **"Every op returns an empty string".** This is not true for argument-type errors. `StrArg::from_v8` runs before `live_ctx`, so `op_rs3_get({})` still throws a `TypeError` after the timeout. This is harmless for termination: it goes through deno_core's native error builder, and I verified that termination is not lost.
- **"At most 4 scripts run in one PATCH chain".** This can be read as a total. Only the nesting depth is bounded. Sibling `op_rs3_patch` calls at the same level are bounded only by the timeout.
- **"Other PATCH requests keep being answered meanwhile".** This holds only while a worker is free; otherwise they queue.
**Fix:** Add the depth-stop and timeout behaviour to each table row, note the argument-type exception, and say "nesting depth" and "when a JS worker is free".

### IN-03: `ExecCtx::depth` duplicates `chain.len() - 1`

**File:** `src/js/ops.rs:24-28`, `:64-78`, `:280`, `:332`
**Issue:** The depth check uses `chain.len()`, while the log line and the test hook use `depth`. Two sources of truth have to be kept in step by every constructor.
**Fix:** Drop `depth` and derive it with `fn depth(&self) -> u32 { (self.chain.len() - 1) as u32 }`, or add a `debug_assert_eq!` in `nested`.

### IN-04: `op_rs3_patch`'s `map_err` claims to need no wildcard but ends with one

**File:** `src/js/ops.rs:304-338`
**Issue:** The comment says the arms are listed "so no wildcard is needed", but `other =>` catches every remaining variant. A future `ExecError` variant would silently be treated as a host fault (logged, then poisoning the job).
**Fix:** Spell out `ExecError::Internal(m)` and `ExecError::Unavailable` as arms, so the compiler flags any new variant.

### IN-05: The smoke compares `nproc` with a pool sized by `available_parallelism`

**File:** `scripts/ci-smoke.sh:169-179`
**Issue:** The pool is sized by Rust's `available_parallelism()`, which honours cgroup CPU quotas and affinity. Older coreutils `nproc` ignores cgroup quotas. In a CPU-limited container (`--cpus=1` on a multi-core host), the smoke would expect a second worker that does not exist. `PATCH /key/m` would then wait behind the loop, and the "loop still running" assertion would fail falsely.
**Fix:** Gate the check on an explicit opt-in such as `RS3_SMOKE_CONCURRENCY=1`, or treat "m answered after the loop" as a skip with a note instead of a failure.

### IN-06: The watchdog spawn comment names the wrong thread as the V8 initialiser

**File:** `src/js/executor.rs:145-146`
**Issue:** "the watchdog descends from the thread that initialised V8, like the worker itself". `init_platform` runs on the main thread (`src/main.rs:16`), not on the worker. The descendant chain main -> worker -> watchdog is still correct.
**Fix:** "so the watchdog descends from the main thread that initialised V8, through the worker".

### IN-07: The timing assertions have tight upper bounds

**File:** `src/js/executor.rs:762-765` (2100 to 2400 ms window), `src/js/ops.rs:571-574`, `:586-589`, `src/http/mod.rs:403-406`
**Issue:** The backstop test fails if the backstop reply is delayed by more than 300 ms (the worker's own reply would then arrive first). The suite was stable here (3 runs in a row and 4 in parallel), but a heavily loaded 2-vCPU CI runner leaves little margin.
**Fix:** Sleep longer in the backstop test (for example 3500 ms, with an upper bound of 3400 ms), or assert that the reply came from the backstop through a flag rather than through elapsed time.

---

_Reviewed: 2026-10-21T01:00:00Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
