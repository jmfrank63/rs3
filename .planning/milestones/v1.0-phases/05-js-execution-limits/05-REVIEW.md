---
phase: 05-js-execution-limits
reviewed: 2026-10-21T02:00:00Z
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
  warning: 0
  info: 8
  total: 8
status: issues_found
---

# Phase 5: Code Review Report (re-review after fix iteration 1)

**Reviewed:** 2026-10-21T02:00:00Z
**Depth:** standard
**Files Reviewed:** 9
**Status:** issues_found (info only)

## Summary

This is an `--auto` re-review of the full Phase 5 scope after fix iteration 1: 82d11a3 (WR-01), 1edc2a8 (WR-02, which also fixed IN-04) and dfd1120 (WR-03). The previous report is `05-REVIEW.iter2.md`. I re-read all nine files in full and traced each fix through `run_script`, `run_fresh`, `op_rs3_patch`, `classify`, `worker_loop` and the smoke.

I ran the five new or affected tests in the main checkout: `host_fault_during_the_timeout_beats_it_and_is_logged`, `nested_boot_without_stack_is_a_catchable_script_error`, `uncaught_nested_boot_without_stack_is_a_script_error`, `only_a_nested_js_boot_failure_is_blamed_on_the_stack` and `patch_after_a_caught_fault_is_refused_without_running`. All five passed. The build went into the gitignored `target/`, and no tracked file was changed. I did not run any unarmed loop or unbounded self-patching script.

**The previous warnings are resolved:**
- **WR-01 (smoke):** fixed. The window `4900 <= LOOP_MS < 6500` now ends below the backstop, which answers at about 7000 ms. `worker_loop` writes `error: js timeout in "loop" after 5000 ms` through `JobState::log_error` (an unbuffered `eprintln!`). It does so after `job.finish()` and before `reply.send`, at `executor.rs:363-374`. So when curl has the 504, the line is already in `$LOG`. The grep therefore has no race, and it proves that the worker which timed out was freed.
- **WR-02 (host fault during a timeout):** fixed. `op_rs3_patch` now logs and records a nested `Internal`/`Unavailable` before the `timed_out()` early return (`ops.rs:322-342`). The `!ctx.host_fault()` guard still makes sure the fault is logged only once. `run_script`'s synthesized `Internal("a nested JS runtime failed ...")` is produced only when the flag is already set, so it is never logged a second time. `classify` then answers 500 ahead of 504. The `map_err` now lists every variant (IN-04 is resolved).
- **WR-03 (nested boot without stack):** fixed. `boot_failed_for_lack_of_stack` matches on `CoreErrorKind` and requires `depth > 0`. The nested isolate is registered only after `try_new` returns (`runtime.rs:79-84`), so watchdog termination cannot cause a Parse/Execute/Js boot failure. The boot scripts are fixed and run at depth 0 on every request. That leaves the caller's stack as the only realistic cause, so the classification holds. In my run the failure was `Failed to parse ext:core/00_primordials.js`. It was classified as a script error and caught, and the job's later ops still worked. A boot failure that happens after a timeout is not logged, and the op returns `""`, so nothing is thrown into a terminating isolate.

I found no new blocker or warning. Six info items carry over unchanged from iteration 2 (IN-04 dropped). Two new info items come from the WR-03 path, and I observed both in the test output.

## Info

### IN-01: The backstop's log line has no job label, and the timeout is logged twice (carry-over)

**File:** `src/js/executor.rs:235-239`, `src/http/error.rs:40-42`
**Issue:** `error: JS worker did not reply within N ms` does not say which key stalled. When the stalled worker finally returns, `worker_loop` also logs `error: js timeout in "<key>" ...`. One request therefore produces two timeout lines, which contradicts the `From<ExecError>` doc comment ("the JS worker or the backstop already logged it once").
**Fix:** Clone `label` before moving it into the `Envelope`, log it in the backstop branch as `{label:?}`, and reword the doc comment to say the timeout can be logged twice.

### IN-02: The docs' ops table and limits section are incomplete or inaccurate (carry-over)

**File:** `docs/js-ops.md:25-28`, `:210-212`, `:223-225`
**Issue:**
- **Ops table.** The "Throws when" cells for `op_rs3_list`, `op_rs3_get`, `op_rs3_insert` and `op_rs3_delete` mention only the nested host fault. They leave out the depth-stop error and the empty-string answer after a timeout. Only the intro and the Errors section cover those.
- **"Every op returns an empty string" after a timeout.** This is not true for argument-type errors. `StrArg::from_v8` runs before `live_ctx`, so `op_rs3_get({})` still throws `TypeError`.
- **"At most 4 scripts run in one PATCH chain".** This reads like a total, but only the nesting depth is bounded. Sibling `op_rs3_patch` calls are bounded only by the timeout.
- **"Other PATCH requests keep being answered meanwhile".** This holds only while a JS worker is free.
**Fix:** Add the depth-stop and timeout behaviour to each table row, note the argument-type exception, and say "nesting depth" and "when a JS worker is free".

### IN-03: `ExecCtx::depth` duplicates `chain.len() - 1`, and now drives a classification (carry-over, widened)

**File:** `src/js/ops.rs:24-28`, `:70-86`, `:296`, `:334`; `src/js/runtime.rs:78`, `:131-137`
**Issue:** The depth check reads `chain.len()`. The log line, the test hooks and now `boot_failed_for_lack_of_stack` (through `let depth = ctx.depth`) read `depth`. If the two ever drifted apart, a depth-0 boot failure could be reported as a 422 script error instead of a 500 host fault.
**Fix:** Derive the depth with `fn depth(&self) -> u32 { (self.chain.len() - 1) as u32 }`, or at least add `debug_assert_eq!(self.depth as usize + 1, self.chain.len())` in `nested`.

### IN-04: The smoke compares `nproc` with a pool sized by `available_parallelism` (carry-over)

**File:** `scripts/ci-smoke.sh:175-185`
**Issue:** Older coreutils `nproc` ignores cgroup CPU quotas, while `available_parallelism()` honours them. In a container limited to one CPU on a multi-core host, the smoke expects a second worker that does not exist. The "loop still running" assertion then fails even though nothing is wrong.
**Fix:** Gate the concurrency check on an explicit opt-in, or report a skip instead of a failure when `m` answers only after the loop.

### IN-05: The watchdog spawn comment names the wrong thread as the V8 initialiser (carry-over)

**File:** `src/js/executor.rs:147-148`
**Issue:** The comment says the watchdog "descends from the thread that initialised V8, like the worker itself". `init_platform` runs on the main thread, not on the worker.
**Fix:** "so the watchdog descends from the main thread that initialised V8, through the worker".

### IN-06: The timing assertions have tight upper bounds (carry-over)

**File:** `src/js/executor.rs:759-766` (window 2100 to 2400 ms against a 2400 ms sleep), `src/js/ops.rs:590-593`, `:605-608`, `src/http/mod.rs:383`, `:404-406`
**Issue:** The backstop test fails if the backstop reply is more than 300 ms late. A heavily loaded 2-vCPU CI runner leaves little margin.
**Fix:** Sleep longer in the backstop test (for example 3500 ms with an upper bound of 3400 ms), or assert that the reply came from the backstop with a flag rather than through elapsed time.

### IN-07: Every boot that runs out of stack writes an unprefixed V8 line to the server's stdout (new)

**File:** `src/js/runtime.rs:79-83` (the `JsRuntime::try_new` path that WR-03 now treats as a normal script error); `docs/js-ops.md:205-206`
**Issue:** With no message listener installed yet, V8's default message reporter prints `<unknown>:-1: Uncaught RangeError: Maximum call stack size exceeded` to **stdout**, one line per failed boot. I saw 4 lines on stdout and 0 on stderr when running the WR-03 tests. Before WR-03 this happened only on a host fault. Now it is an ordinary, catchable script error that a client can trigger, and a catch-and-retry loop repeats it for up to 5 s. The text is fixed, so it cannot be used to forge log lines. Still, it bypasses rs3's log format, it goes through C stdio buffering (so it can interleave out of order with Rust's stdout), and it contradicts the docs, which say failed-script errors go to stderr in escaped form.
**Fix:** Document the line in "Sandbox and known limits" next to `Deno.core.print`. Alternatively, install a V8 message listener before bootstrap, if deno_core exposes a hook for one.

### IN-08: The stack-exhaustion log line blames the nested key, not the script that used up the stack (new)

**File:** `src/js/runtime.rs:147-153`
**Issue:** `creation_error` logs `js error in "<nested key>": RangeError: not enough stack ...`. For example, `nested_boot_without_stack_is_a_catchable_script_error` logs `js error in "leaf": ...` even though `leaf` (`'leaf'`) never ran. The script that recursed was the caller (`test` or `k1`). An operator would look at the wrong stored script.
**Fix:** Name the caller. The parent key is `watch.chain[watch.chain.len() - 2]` when `depth > 0`. For example: `js error in {caller:?}: {NESTED_STACK_MESSAGE} while starting {name:?} (...)`.

---

_Reviewed: 2026-10-21T02:00:00Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
