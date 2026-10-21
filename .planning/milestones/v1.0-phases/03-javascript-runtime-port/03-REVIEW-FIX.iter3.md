---
phase: 03-javascript-runtime-port
fixed_at: 2026-10-20T21:10:00Z
review_path: .planning/phases/03-javascript-runtime-port/03-REVIEW.md
iteration: 2
findings_in_scope: 2
fixed: 2
skipped: 0
status: all_fixed
---

# Phase 3: Code Review Fix Report

**Fixed at:** 2026-10-20T21:10:00Z
**Source review:** .planning/phases/03-javascript-runtime-port/03-REVIEW.md
**Iteration:** 2

**Summary:**
- Findings in scope: 2 (fix scope critical_warning: WR-01, WR-02)
- Fixed: 2
- Skipped: 0
- Also fixed (optional Info, adjacent and safe): IN-01
- Out of scope (Info, not attempted): IN-02, IN-03, IN-04, IN-05, IN-06

## Fixed Issues

### WR-01: A host fault poisons every later `op_rs3_patch` call in the same job, so successful nested runs are blamed for the fault and throw

**Files modified:** `src/js/runtime.rs`, `src/js/ops.rs`
**Commit:** 5a9756b
**Status:** fixed: requires human verification (logic change)
**Applied fix:** `run_script` now reads the job's `host_fault` flag before the run
(`faulted_before`) and maps an otherwise successful or script-failed run to
`ExecError::Internal` only when the flag was clear at the start and set at the end. A fault
that was already recorded belongs to an earlier run, so it is no longer blamed on this one and
no longer produces an extra log line naming a healthy key. The top level always starts with a
fresh flag, so it still answers `Internal` once any level has faulted. Added a cfg(test) hook
`ExecCtx::fail_runtime_for_key` (fails runtime creation for one script name at any depth;
copied through `nested()`) next to the existing depth hook. Regression tests:
`a_fault_recorded_before_a_run_is_not_blamed_on_it` (pre-set flag, `6*7` returns `42`) and a
script-level test showing a healthy `op_rs3_patch('good')` after a caught fault in
`'bad'` returns its result. Both failed with the fix reverted and passed with it. No
self-recursive script was used. The script-level test was replaced in the WR-02 commit,
because WR-02 makes post-fault patches refuse. The direct `run_script` test still guards the
attribution rule.

### WR-02: After a host fault the script keeps running, and its later writes are committed even though the request answers 500

**Files modified:** `src/js/ops.rs`, `docs/js-ops.md`
**Commit:** cf12e7b
**Status:** fixed: requires human verification (logic change, behaviour contract change)
**Applied fix:** Fail fast. A new helper `live_ctx(&OpState)` returns the job's `ExecCtx`, or
`internal_error()` (`JsErrorBox::generic("internal error")`, the same constant the fault path
throws) once `host_fault` is set. All five ops (`op_rs3_list`, `op_rs3_get`, `op_rs3_insert`,
`op_rs3_delete`, `op_rs3_patch`) use it first, so after a fault they refuse without reading or
changing the store. Refusing reads too keeps the rule simple, as the orchestrator suggested.
`op_rs3_list` now returns `Result<String, JsErrorBox>`. Its doc says it throws only after a
host fault. Tests:
- `caught_nested_runtime_failure_is_still_internal_and_hides_details` was renamed to
  `..._and_refuses_later_writes`. It now asserts that a write made before the fault stays,
  and that a post-fault insert and delete are refused.
- New: `every_op_throws_internal_error_once_the_job_faulted` (all five ops answer
  `Error: internal error`, nothing written or deleted).
- New: `patch_after_a_caught_fault_is_refused_without_running` (the later patch's script does
  not run; the same chain runs it when no failure is injected).

`docs/js-ops.md` was updated in the intro, the `op_rs3_list` and `op_rs3_patch` table rows,
the "Nested host faults" error bullet and the "Host fault: 500" result bullet. They now say
that writes made before the fault stay (no transactions) and ops called after it are refused.

### IN-01 (optional): The host-fault log line repeats its prefix

**Files modified:** `src/http/error.rs`, `src/js/ops.rs`
**Commit:** 163cfbd
**Applied fix:** Both log sites now log the bare `ExecError::Internal` detail instead of its
Display text, which already starts with `JS host failure:`. `Unavailable` keeps its Display
text, so its log line is unchanged. The nested log line stays Debug-escaped (`{detail:?}`).

## Verification

Where the gates ran: inside the isolated worktree
`.claude/worktrees/rf-03-3579272-1792529480` (branch `gsd-reviewfix/03-3579272`, removed after
it was fast-forwarded into `modernize-toolchain`). `CARGO_TARGET_DIR` pointed at the main
checkout's `target/`, so the build directory stayed inside the project. The gates ran on the
same tree that was fast-forwarded, so you can reproduce the results from the main checkout at
`163cfbd`.

After each commit (WR-01, WR-02, IN-01) the full gate passed:
- `cargo build --locked`: ok
- `cargo test --locked`: 169 lib tests passed at HEAD (168 after WR-01), 1 doc test ignored
- `cargo clippy --locked --all-targets -- -D warnings`: clean
- `cargo fmt --check`: clean

No `#[allow(..)]` was added. No `unwrap`/`expect` was added in non-test code. No
self-recursive PATCH or `op_rs3_patch` script was run.

## Previous iterations

### Iteration 1 (backed up at `03-REVIEW-FIX.iter2.md`)

All 4 in-scope findings were fixed:
- **CR-01** (exception while stringifying the completion value gave 200 `""`): b90b284.
  The completion value is stringified under a TryCatch.
- **WR-01** (nested runtime host fault became a catchable 422 with host details): b047f5a.
  A nested host fault is treated as a 500.
- **WR-02** (log forging through exception messages and `Deno.core.print(.., true)`): 420d286.
  Script-controlled text is escaped in server logs.
- **WR-03** (full-queue test did not prove callers wait): 75e74a5. The test was made
  deterministic.

---

_Fixed: 2026-10-20T21:10:00Z_
_Fixer: Claude (gsd-code-fixer)_
_Iteration: 2_
