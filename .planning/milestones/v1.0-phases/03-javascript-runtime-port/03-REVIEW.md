---
phase: 03-javascript-runtime-port
reviewed: 2026-10-20T20:58:10Z
depth: standard
files_reviewed: 23
files_reviewed_list:
  - Cargo.toml
  - docs/js-ops.md
  - requests/bin.js
  - requests/bin.txt
  - requests/fac.js
  - requests/fac.txt
  - requests/rs3_http_localhost_5000.http
  - requests/rs3_https_rs3_cwe.http
  - requests/store.js
  - requests/store.txt
  - requests/sum.js
  - requests/sum.txt
  - src/example.js
  - src/http/error.rs
  - src/http/kv.rs
  - src/http/mod.rs
  - src/js/executor.rs
  - src/js/mod.rs
  - src/js/ops.rs
  - src/js/runtime.rs
  - src/main.rs
  - src/state.rs
  - src/store/legacy.rs
findings:
  critical: 0
  warning: 1
  info: 7
  total: 8
status: issues_found
---

# Phase 3: Code Review Report

**Reviewed:** 2026-10-20T20:58:10Z
**Depth:** standard
**Files Reviewed:** 23
**Status:** issues_found

## Summary

This re-review follows fix iteration 2: 5a9756b (WR-01), cf12e7b (WR-02) and 163cfbd (IN-01). I
re-read every file in scope and focused on the fix diff (`75e74a5..HEAD`). At `163cfbd` all gates
pass: `cargo test --locked` (169 lib tests passed, 1 doc test ignored), `cargo clippy --locked
--all-targets -- -D warnings` and `cargo fmt --check`. I built no scratch crate, and `target/`
holds only cargo's own output. I ran no self-recursive script.

**Prior findings:**
- **WR-02 (iter3) is fixed.** `live_ctx` (`src/js/ops.rs:122-129`) is the first thing all five
  ops call. A job runs on one thread, so no fault can be recorded between the check and the
  write in `op_rs3_insert`/`op_rs3_delete`. The flag is set by the innermost `op_rs3_patch` as
  soon as the nested run returns, before any further JS runs. This holds at every level,
  stringification user code included, because all levels share one `Arc<AtomicBool>`. I found
  no store access path that skips the check. The docs (`docs/js-ops.md:10-11,69-78,114-119`)
  describe the contract accurately.
- **WR-01 (iter3) is fixed**, though largely made redundant by WR-02 (see IN-01).
- **IN-01 (iter3) is fixed**: the prefix is no longer repeated.

**New in this pass:** WR-01 below. It is older than this iteration, but it is now the main
source of misleading host-fault logs. I confirmed it at runtime with the existing test
`deeper_nested_runtime_failure_is_internal` run with `--nocapture`.

**Accepted risks (not reported):** no depth limit or timeout until Phase 5; only `op_panic`
disabled; a missing key throws a generic `Error` and bad input a `TypeError`; Promise results
become `[object Promise]`; the 422 body carries only the message; the unprotected platform is
dev-only; writes before a nested host fault are kept and writes after it are refused.

**Carried over:** iter3 IN-02 to IN-06 were out of fix scope. They are unchanged and appear below
as IN-03 to IN-07.

## Narrative Findings (AI reviewer)

## Warnings

### WR-01: One nested host fault is logged once per ancestor level, and each extra line blames a key whose runtime did not fail

**File:** `src/js/ops.rs:198-211`, `src/js/runtime.rs:31-36`, `src/js/runtime.rs:53-56`

**Issue:** A fault at depth N travels up the chain as follows:
1. The innermost `op_rs3_patch` gets the runtime-creation error. It logs it correctly and sets
   the flag.
2. Each ancestor script (depth N-1 down to 1) gets `Error: internal error`. If it does not catch
   it, `run_fresh` logs `js error in "<ancestor>": "Error: internal error\n ..."`.
3. That ancestor's `run_script` sees `!faulted_before && host_fault()` and returns
   `Internal("a nested JS runtime failed while running \"<ancestor>\"")`.
4. The `op_rs3_patch` one level up takes the `other` arm and logs again,
   `error: nested JS host failure in "<ancestor>" at depth k: ...`, then calls
   `note_host_fault()` on a flag that is already set.

Captured output of `deeper_nested_runtime_failure_is_internal` (top → `mid` → `inner`, failure
injected at depth 2):

```
error: nested JS host failure in "inner" at depth 2: "cannot create JS runtime: injected test failure"
js error in "mid": "Error: internal error\n    at mid:1:15"
error: nested JS host failure in "mid" at depth 1: "a nested JS runtime failed while running \"mid\""
```

Over HTTP, `From<ExecError>` (`src/http/error.rs:41`) adds a fourth line for the top-level key.
So one fault produces about 2N+1 error lines. Every line except the first says a host failure
happened "in" a key whose runtime was created and ran normally. The `\"mid\"` double escaping
comes from `{detail:?}` applied to a detail that already contains `{name:?}`. The docs say that
running out of stack at depth (`docs/js-ops.md:175-177`, hundreds of levels) is a reachable
host fault. In that case any client that can insert and PATCH can turn one request into hundreds
of misleading `error:` lines. This is the same kind of log misattribution as iter3 WR-01, which
was fixed only for runs that start after a fault, not for runs that fail as a result of one.

**Fix:** Log only the original fault. `live_ctx` guarantees the flag was clear when this op
started. If the flag is set when the nested run returns, a deeper level has already logged the
fault and only the 500 needs to propagate:

```rust
other => {
    // A deeper level already logged and recorded this fault; just propagate it.
    if !ctx.host_fault() {
        let detail = match other {
            super::ExecError::Internal(m) => m,
            e => e.to_string(),
        };
        eprintln!(
            "error: nested JS host failure in {key:?} at depth {}: {detail:?}",
            ctx.depth + 1
        );
        ctx.note_host_fault();
    }
    internal_error()
}
```

Optionally, skip the `js error in ...` line in `run_fresh` when `ctx.host_fault()` is set (keep
a clone of the flag before moving `ctx` into the extension). Add a test that captures the
fault-site count, or at least asserts that the 3-level chain still answers `Internal` while
only the innermost level reports.

## Info

### IN-01: The `faulted_before` attribution branch cannot be reached in production

**File:** `src/js/runtime.rs:20-36`, `src/js/ops.rs:409-414`

**Issue:** Since cf12e7b, `op_rs3_patch` calls `live_ctx` before it starts a nested run. A nested
`run_script` therefore always starts with the flag clear, and the top level always starts with a
fresh `ExecCtx`. As a result `faulted_before` is always `false` outside tests. The doc comment
("a fault that was already recorded when the run started belongs to an earlier run of the same
job") describes a situation that can no longer occur. `a_fault_recorded_before_a_run_is_not_blamed_on_it`
and the first half of `every_op_throws_internal_error_once_the_job_faulted` pin behaviour that
only exists when a test pre-sets the flag.

**Fix:** Keep it as defence in depth, but say so in the comment: "unreachable while every op
refuses after a fault; kept so attribution stays correct if that rule is relaxed". Or remove the
branch and its test.

### IN-02: The migration table lists the post-fault refusal for `op_rs3_list` and `op_rs3_patch` only

**File:** `docs/js-ops.md:23-25`

**Issue:** The "Throws when" cells for `op_rs3_get`, `op_rs3_insert` and `op_rs3_delete` do not
mention that these ops also throw `Error: internal error` after a nested host fault. The
`op_rs3_list` and `op_rs3_patch` rows do. The intro (line 10) and the Errors section cover the
rule, but the table, which is the per-op reference, is inconsistent.

**Fix:** Append "; after a nested host fault: `Error: internal error` (see Errors)" to the three
rows.

### IN-03 (carried over from iter3 IN-02): The docs say a script cannot fake server log lines, but stdout is still written raw

**File:** `docs/js-ops.md:157-164`, `src/js/ops.rs:226-230`, `src/main.rs:38`

**Issue:** `Deno.core.print(text)` writes unescaped text to stdout, where the server prints its
own `Starting Http server at ...` line. "so a script cannot fake server log lines" is true only
for stderr.

**Fix:** Narrow the sentence to stderr, or prefix and escape stdout output as well.

### IN-04 (carried over from iter3 IN-03): The `op_print` replacement no longer retries on `WouldBlock`

**File:** `src/js/ops.rs:221-231`

**Issue:** Upstream `op_print` loops on partial or `WouldBlock` writes because the fd may be
non-blocking. `write_all`/`writeln!` fail instead, so `Deno.core.print` would throw (and turn a
good script into a 422) when rs3 inherits non-blocking pipes.

**Fix:** Copy upstream's retry loop, or document the difference in the source.

### IN-05 (carried over from iter3 IN-04): The `.txt` payloads posted by the REST walkthrough are untested copies of the `.js` fixtures

**File:** `requests/bin.txt:2`, `requests/fac.txt:2`, `requests/sum.txt:2`, `requests/store.txt:2`

**Issue:** The tests use `include_str!` on the `.js` files only (`src/http/mod.rs:406-409`). The
hand-escaped `.txt` copies posted by the `.http` walkthrough and `docs/js-ops.md:206` are never
exercised, and they already differ (`bin.txt` uses `t`/`s` where `bin.js` uses `tree`).

**Fix:** For each `.txt` file, add a test that runs it through `parse_insert_body`, PATCHes the
key and asserts the same body as the matching `.js` fixture.

### IN-06 (carried over from iter3 IN-05): The `catch_unwind` comment overstates what it protects

**File:** `src/js/executor.rs:126-128`

**Issue:** "a panicking job cannot kill the worker" is not true for a panic inside an op callback,
`op_rs3_patch` and its nested runtime included. That panic unwinds across V8 frames and aborts
the process.

**Fix:** Change the comment to "a panic in job code outside V8 callbacks" and point to the
ops' no-panic rule.

### IN-07 (carried over from iter3 IN-06): `stack_bytes` is not clamped, unlike `workers` and `queue_capacity`

**File:** `src/js/executor.rs:33-35,45`

**Issue:** A zero or tiny `stack_bytes` goes straight to `stack_size`, and V8 bootstrap then
overflows the native stack, which aborts the process. The value is set in code today, but it
becomes a hazard once it is configurable.

**Fix:** Use `.stack_size(settings.stack_bytes.max(2 * 1024 * 1024))` and document the floor next
to the other clamps.

---

_Reviewed: 2026-10-20T20:58:10Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
