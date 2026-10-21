---
phase: 03-javascript-runtime-port
fixed_at: 2026-10-20T21:02:30Z
review_path: .planning/phases/03-javascript-runtime-port/03-REVIEW.md
iteration: 3
findings_in_scope: 1
fixed: 1
skipped: 0
status: all_fixed
---

# Phase 3: Code Review Fix Report

**Fixed at:** 2026-10-20T21:02:30Z
**Source review:** .planning/phases/03-javascript-runtime-port/03-REVIEW.md
**Iteration:** 3

**Summary:**
- Findings in scope: 1 (fix scope critical_warning: WR-01)
- Fixed: 1
- Skipped: 0
- Also fixed (optional Info items the orchestrator asked for, adjacent and safe): IN-01, IN-02
- Out of scope (Info, not attempted): IN-03, IN-04, IN-05, IN-06, IN-07

## Fixed Issues

### WR-01: One nested host fault is logged once per ancestor level, and each extra line blames a key whose runtime did not fail

**Files modified:** `src/js/ops.rs`, `src/js/runtime.rs`
**Commit:** 912d942
**Status:** fixed: requires human verification (logic change to logging)
**Applied fix:**
- In the `op_rs3_patch` host-fault arm, the op now logs and calls `note_host_fault()` only
  when `ctx.host_fault()` is still false. `live_ctx` has already checked that the flag was
  clear when the op started, so if the flag is set when the nested run returns, a deeper
  level has already logged the fault. The op then only throws `Error: internal error`. The
  fault is logged once, at the innermost level. Ancestors still propagate `Internal`, so the
  request still answers 500.
- I also applied the reviewer's optional suggestion. `run_fresh` now gets a `watch: &ExecCtx`
  that shares the job's flag, and it skips both `js error in ..` lines (execute and stringify)
  once the job has faulted. Those errors are the `Error: internal error` that the fault
  caused. Script errors in a job with no fault are logged as before.
- I added `ExecCtx::log_error(fmt::Arguments)`, which calls `eprintln!` once. Under
  `cfg(test)` it also increments an `error_logs: Arc<AtomicU32>` counter on `ExecCtx`, and
  `nested()` shares that counter across levels. All three JS-layer error log sites now go
  through it, so the tests count the lines that are actually written.

With this fix, one fault produces one `error: nested JS host failure ..` line plus the
existing top-level `error: JS host failure: ..` line from `src/http/error.rs`. Before, it
produced about 2N+1 lines.

New tests:
- `one_nested_host_fault_is_logged_once_at_any_depth`: a chain top → a → b → c with the
  failure injected at depth 3. It runs three cases: uncaught at every level, caught at the
  top, and a direct depth-1 failure. Each asserts `ExecError::Internal` and exactly 1 logged
  line. I mutation-checked it: with the guard removed, the first case logs 3 lines and the
  test fails.
- `script_errors_without_a_host_fault_are_still_logged`: a nested `throw` with no fault still
  logs 2 lines (the inner error and the uncaught rethrow at the top). This shows the
  suppression applies only after a fault.

No self-recursive script was used.

### IN-01 (optional): The `faulted_before` attribution branch cannot be reached in production

**Files modified:** `src/js/runtime.rs`, `src/js/ops.rs`
**Commit:** a1a5faa
**Applied fix:** I kept the branch as defence in depth, as the review allowed. The
`run_script` doc comment now says this case cannot be reached while every op refuses after a
fault, and that the check stays so attribution remains correct if that rule is relaxed. The
two tests that pre-set the flag (`every_op_throws_internal_error_once_the_job_faulted` and
`a_fault_recorded_before_a_run_is_not_blamed_on_it`) now say that the pre-set is test-only.
Only comments changed.

### IN-02 (optional): The migration table lists the post-fault refusal for `op_rs3_list` and `op_rs3_patch` only

**Files modified:** `docs/js-ops.md`
**Commit:** e6b7324
**Applied fix:** I appended "; after a nested host fault: `Error: internal error` (see
Errors)" to the "Throws when" cells of the `op_rs3_get`, `op_rs3_insert` and
`op_rs3_delete` rows. Every op row now states the rule.

## Verification

Where the gates ran: the gates first ran inside the isolated worktree
`.claude/worktrees/rf-03-3588803-1792529963` (branch `gsd-reviewfix/03-3588803`) after each
commit. `CARGO_TARGET_DIR` pointed at the main checkout's `target/`. The worktree was
fast-forwarded into `modernize-toolchain` and then removed, and the temp branch was deleted.
I then ran the full gate again in the main checkout at `a1a5faa`:
- `cargo build --locked`: ok
- `cargo test`: 171 lib tests passed (169 before this iteration, plus 2 new), 1 doc test ignored
- `cargo clippy --all-targets -- -D warnings`: clean
- `cargo fmt --check`: clean

No `#[allow(..)]` was added. No `unwrap`/`expect` was added in non-test code. No
self-recursive PATCH or `op_rs3_patch` script was run. The pre-existing uncommitted files
(`.planning/config.json`, `.planning/state.json`, `.planning/milestone.lock`, the `*.iter*.md`
backups) were not touched.

## Previous iterations

### Iteration 1 (backed up at `03-REVIEW-FIX.iter2.md`)

All 4 in-scope findings were fixed:
- **CR-01** (an exception while stringifying the completion value gave 200 `""`): b90b284.
  The completion value is now stringified under a TryCatch.
- **WR-01** (a nested runtime host fault became a catchable 422 with host details): b047f5a.
  A nested host fault is now treated as a 500.
- **WR-02** (log forging through exception messages and `Deno.core.print(.., true)`): 420d286.
  Script-controlled text is now escaped in server logs.
- **WR-03** (the full-queue test did not prove that callers wait): 75e74a5. The test is now
  deterministic.

### Iteration 2 (backed up at `03-REVIEW-FIX.iter3.md`)

Both in-scope findings and one optional Info item were fixed:
- **WR-01** (a host fault poisoned later `op_rs3_patch` calls in the same job): 5a9756b.
  `run_script` now blames only a fault that was recorded during its own run.
- **WR-02** (the script kept running and writing after a host fault): cf12e7b. All five ops
  now refuse with `Error: internal error` once the job has faulted.
- **IN-01** (the host-failure log prefix was repeated): 163cfbd. The bare `Internal` detail
  is logged.

---

_Fixed: 2026-10-20T21:02:30Z_
_Fixer: Claude (gsd-code-fixer)_
_Iteration: 3_
