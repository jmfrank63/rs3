---
phase: 05-js-execution-limits
fixed_at: 2026-10-21T01:11:41Z
review_path: .planning/phases/05-js-execution-limits/05-REVIEW.md
iteration: 1
findings_in_scope: 3
fixed: 3
skipped: 0
status: all_fixed
---

# Phase 5: Code Review Fix Report

**Fixed at:** 2026-10-21
**Source review:** .planning/phases/05-js-execution-limits/05-REVIEW.md
**Iteration:** 1

**Summary:**
- Findings in scope: 3 (fix scope: critical_warning)
- Fixed: 3
- Skipped: 0
- Info item fixed alongside a warning: IN-04 (adjacent to WR-02, same `map_err`)

## Fixed Issues

### WR-01: The smoke's timeout window includes the backstop, so a broken watchdog still passes

**Files modified:** `scripts/ci-smoke.sh`
**Commit:** 82d11a3
**Applied fix:** The loop window is now `4900 <= LOOP_MS < 6500`, which ends below the backstop's reply at about 7000 ms (timeout plus the 2 s grace). The smoke also requires the worker's own line `error: js timeout in "loop" after 5000 ms` in the server log. The worker writes that line only after the terminated job has returned and before it replies, which proves the worker was freed. The log must also not contain the backstop line `JS worker did not reply`. The header comment now explains why. The smoke still binds only to loopback, cleanup still runs on its trap, and `ci.yml` is unchanged.

### WR-02: A nested host fault is dropped when the job times out during the nested run

**Files modified:** `src/js/ops.rs`, `src/js/runtime.rs`
**Commit:** 1edc2a8
**Status:** fixed: requires human verification (precedence / state-handling logic)
**Applied fix:** `op_rs3_patch` now logs and records a nested `Internal`/`Unavailable` result (still only when `!ctx.host_fault()`, so it is logged once) before the "never throw into a terminating script" early return. Host fault (500) therefore beats timeout (504) even when the two overlap at that level. The `map_err` lists every `ExecError` variant (`Internal(_) | Unavailable => internal_error()`), which resolves IN-04: a future variant fails to compile instead of being treated as a host fault without notice. The doc comment is updated. A new cfg(test) hook, `ExecCtx::fail_runtime_after_timeout`, makes an injected runtime failure wait until the job timed out (polling every 5 ms, giving up after 5 s) under the armed 300 ms timeout. The new test `host_fault_during_the_timeout_beats_it_and_is_logged` asserts that the result is `Internal`, that the job both timed out and has a host fault, and that exactly 2 error lines were logged (the nested host failure and the worker's timeout). With the old ordering restored temporarily, the test fails; the file was then restored.

### WR-03: A client can make a request answer 500 "host fault" by exhausting the JS stack before nesting

**Files modified:** `src/js/runtime.rs`, `src/js/ops.rs`, `src/js/executor.rs`, `src/js/mod.rs`, `docs/js-ops.md`
**Commit:** dfd1120
**Status:** fixed: requires human verification (classification logic)
**Applied fix:** `run_fresh` sends a failed `JsRuntime::try_new` through `creation_error`. The pure predicate `boot_failed_for_lack_of_stack(depth, &CoreError)` matches on `CoreErrorKind`, not on message text. At depth > 0, the kinds `Parse`, `Execute` and `Js` (JavaScript failed while deno_core's built-in boot scripts ran) count as the caller's doing. They become `ExecError::Script` carrying `RangeError: not enough stack to start a nested script`. The failure is logged like a script error while the job is not stopped, with no host details sent to the client. `op_rs3_patch` rethrows it into the caller, so the request answers 422 `js_error` when the error is uncaught. Every other kind, and any failure at depth 0, stays `ExecError::Internal` (500).

Findings while reproducing:
- The real failure kind was `Parse` ("Failed to parse ext:core/00_primordials.js"), not only `Js` as the review expected. It happened already at nesting depth 1, on both the 16 MiB and the 2 MiB stack.
- When the stack is that exhausted, deno_core's error builder can overflow too. It then throws the bare message string instead of an `Error` object, so an uncaught failure reads `Uncaught RangeError: not enough stack to start a nested script`. The docs and tests allow both forms.

Tests:
- `only_a_nested_js_boot_failure_is_blamed_on_the_stack` is a pure test of both paths: `Parse`/`Execute` at depth 0 are host faults, at depth > 0 they are stack, and `Io`/`TLA` are host faults at any depth.
- `nested_boot_without_stack_is_a_catchable_script_error` covers one level with the error caught: no host fault, and later ops still work. It also covers a two-key chain.
- `uncaught_nested_boot_without_stack_is_a_script_error` runs on the default stack and on the 2 MiB floor.

All recursion is bounded by V8's stack guard. No test runs an unbounded self-patching script. The "as a 500" wording in `executor.rs`, `mod.rs` and `docs/js-ops.md` (ops table, Errors, PATCH results) is updated.

## Verification

All gates ran in the **isolated worktree** (`.claude/worktrees/rf-05-*`, on the temp branch `gsd-reviewfix/05-*`), with its own `CARGO_TARGET_DIR` inside the worktree. That tree is identical to `modernize-toolchain` after the fast-forward, so the results can be reproduced from the main checkout.

- `bash scripts/ci-guards.sh`: all guards passed
- `cargo fmt --check`: clean
- `cargo clippy --all-targets --locked -- -D warnings`: clean
- `cargo test --locked`: 216 passed. With `-- --test-threads=1`: 216 passed.
- `cargo build --release --locked && bash scripts/ci-smoke.sh` (`RS3_BIN` set to the worktree's release binary, `TMPDIR` under `target/tmp`, deleted afterwards): OK. `PATCH /key/loop` answered 504 after 5009 ms and was terminated by the watchdog.
- `Cargo.lock` and `.github/workflows/ci.yml`: byte-identical to 189f96d.
- No `#[allow]`/`#[ignore]` was added, and `#![forbid(unsafe_code)]` was respected.

---

_Fixed: 2026-10-21_
_Fixer: Claude (gsd-code-fixer)_
_Iteration: 1_
