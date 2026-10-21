---
phase: 03-javascript-runtime-port
fixed_at: 2026-10-20T21:30:00Z
review_path: .planning/phases/03-javascript-runtime-port/03-REVIEW.md
iteration: 1
findings_in_scope: 4
fixed: 4
skipped: 0
status: all_fixed
---

# Phase 3: Code Review Fix Report

**Fixed at:** 2026-10-20T21:30:00Z
**Source review:** .planning/phases/03-javascript-runtime-port/03-REVIEW.md
**Iteration:** 1

**Summary:**
- Findings in scope: 4 (fix scope critical_warning: CR-01, WR-01, WR-02, WR-03)
- Fixed: 4
- Skipped: 0
- Out of scope (Info, not attempted): IN-01, IN-02, IN-03

## Fixed Issues

### CR-01: An exception while stringifying the completion value becomes 200 `""` instead of 422

**Files modified:** `src/js/runtime.rs`, `src/js/ops.rs` (tests), `src/http/mod.rs` (tests), `docs/js-ops.md`
**Commit:** b90b284
**Status:** fixed: requires human verification (logic change)
**Applied fix:** Added `stringify()` in `src/js/runtime.rs`. A Symbol completion value is rendered as `Symbol(desc)`, the same as JS `String(..)` (`Symbol()` when there is no description). Every other value is converted with `to_string` inside `v8::tc_scope!`. A caught exception becomes a `JsError` through `JsError::from_v8_exception`, then `ExecError::Script` (422 js_error). Its message has the same `Uncaught ...` form as an exception thrown by `execute_script`. `op_rs3_patch` goes through `run_script`, so a nested stringify failure now throws into the calling script instead of returning `""`. Regression tests cover:
- `Symbol('x')`, `Symbol()`, `Symbol('')` and `Symbol.iterator`.
- A throwing `toString`, `Object.create(null)`, `Symbol.toPrimitive` returning a symbol, and `Object(Symbol('x'))`.
- Store writes made by a user `toString` during conversion are kept.
- The nested `op_rs3_patch` cases: `"[Symbol(x)]"`, a catchable `Error: x` or `TypeError`, and uncaught `Uncaught Error: Error: x`.
- HTTP: a throwing `toString` or `Object.create(null)` gives 422, and a symbol gives 200 `"Symbol(x)"`.

The docs add a `Symbol('x')` row and explain that converting the result can run user code.

### WR-01: A host fault in a nested runtime turns into a catchable JS `Error` and a 422 whose body contains host details

**Files modified:** `src/js/ops.rs`, `src/js/runtime.rs`, `docs/js-ops.md`
**Commit:** b047f5a
**Status:** fixed: requires human verification (logic change)
**Applied fix:**
- `ExecCtx` gained a private `host_fault: Arc<AtomicBool>`. `nested()` shares it across every level of one job.
- When the nested run returns a non-Script error, `op_rs3_patch` does three things. It logs the detail to stderr, Debug-escaped and including the key and depth. It sets the flag. It throws a fixed `Error: internal error` that carries no host detail.
- `run_script` is now a thin wrapper around `run_fresh`. If the flag is set when the run ends, it returns `ExecError::Internal`, so the request answers 500 with the fixed body even when the script caught the error. A top-level `Internal` error from runtime creation is passed through unchanged.
- A `#[cfg(test)]` field, `fail_runtime_at_depth`, injects a runtime-creation failure at a chosen depth, so no self-recursive script is needed.
- Tests cover a failure at the top level, a nested failure, a caught nested failure (still `Internal`, and the caught `e.message` is `internal error`), a two-level failure, and the same chain succeeding when no failure is injected.
- The docs update the op table row, add a "Nested host faults" bullet, extend the "Host fault: 500" paragraph, and note in "Unbounded nesting" that running out of stack while creating a nested runtime answers 500.

Note: store writes the script made before the fault are kept. There is no transaction, and the docs say so.

### WR-02: Log forging is still possible through exception messages and `Deno.core.print(.., true)`

**Files modified:** `src/js/runtime.rs`, `src/js/ops.rs`, `docs/js-ops.md`
**Commit:** 420d286
**Applied fix:**
- Both JS-error log lines in `run_script` (execution failure and stringify failure) now print `{:?}` of `e.to_string()`. The script-controlled message and stack are written on one escaped line.
- The `rs3_ext` middleware now replaces the implementation of the built-in `op_print` with `op_rs3_print`, using `with_implementation_from`. The signature is identical: `#[op2(fast)]`, `(&str, bool) -> Result<(), io::Error>`.
  - stdout output is unchanged.
  - stderr output (`Deno.core.print(text, true)`) is written as one line per call, `js stderr: "<escaped>"`.
- Test: `print_to_either_stream_keeps_running`. With `--nocapture` it shows `js stderr: "x\nerror: JS host failure: forged"` on a single line.
- `docs/js-ops.md` now describes both streams, the prefix and escaping, and says that script output is untrusted.

### WR-03: `full_queue_makes_callers_wait` does not test what its name says

**Files modified:** `src/js/executor.rs`
**Commit:** 75e74a5
**Applied fix:** I rewrote the test as follows:
1. Block the single worker.
2. Fill the single queue slot with a direct `try_send`.
3. Assert that a second `try_send` returns `TrySendError::Full`.
4. Call `run()` under a 50 ms `tokio::time::timeout`. It must time out. That job sets a flag, and because the single worker runs jobs in FIFO order, a later job completing proves the flag was never set. The job was therefore never queued, and the caller was waiting in `send`.
5. Start a waiting submitter and release the worker. Check that jobs 0, 1 and 2 all complete, and that a later job 3 runs while the abandoned job never runs.

The gate channel is created inside the thread scope, so a failed assertion drops the sender and the test fails instead of hanging. A mutation check (queue capacity `+64`) made the test fail in 0.01 s. The test then passed 4 times in a row under parallel `cargo test`.

## Verification

- Every fix passed these checks before it was committed: re-reading the edited file, `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check`.
- **Where the gates ran:**
  - Per-fix gates ran in the isolated worktree (`.claude/worktrees/rf-03-...`) with `CARGO_TARGET_DIR` set to the project's `target/`.
  - After the fast-forward and worktree removal, the full gate ran again in the main checkout at `75e74a5`: `cargo build --locked && cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check`.
  - Result: build OK, 166 tests passed (plus 1 pre-existing ignored doc test), clippy clean, fmt clean.
- No self-recursive PATCH or `op_rs3_patch` script was run. WR-01 uses the injected cfg(test) failure instead.
- No `#[allow]` attributes were added. Non-test code has no `unwrap` or `expect`.

---

_Fixed: 2026-10-20T21:30:00Z_
_Fixer: Claude (gsd-code-fixer)_
_Iteration: 1_
