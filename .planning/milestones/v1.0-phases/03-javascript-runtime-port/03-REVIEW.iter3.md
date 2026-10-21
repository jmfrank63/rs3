---
phase: 03-javascript-runtime-port
reviewed: 2026-10-20T22:05:00Z
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
  warning: 2
  info: 6
  total: 8
status: issues_found
---

# Phase 3: Code Review Report

**Reviewed:** 2026-10-20T22:05:00Z
**Depth:** standard
**Files Reviewed:** 23
**Status:** issues_found

## Summary

This re-review follows fix iteration 1: b90b284 (CR-01), b047f5a (WR-01), 420d286 (WR-02) and
75e74a5 (WR-03). I re-read every file in scope, focusing on the fix diff (`cafc8e3..HEAD`). At
`75e74a5`, `cargo test --locked` passes: 166 lib tests, 1 doc test ignored.

**Prior findings:**
- **CR-01 is fixed.** `stringify` runs `ToString` under `v8::tc_scope!` and special-cases symbols.
  I probed edge cases with a throwaway release crate under `target/`, deleted afterwards. A thrown
  symbol inside `toString` gives `Script("Uncaught Symbol(x)")`. An exception object whose
  `message` and `stack` getters throw gives `Script("Uncaught #<Object>")`, and so does a Proxy
  exception whose `get` trap throws. None of these crash.
- **WR-02 is fixed.** The `op_print` override is in effect: `js stderr: "x\nerror: JS host failure: forged"` appears on one line.
- **WR-03 is fixed.** The test is now deterministic and does check backpressure.
- **WR-01 is fixed for the status code**, but the shared `host_fault` flag it introduced has two
  side effects (WR-01 and WR-02 below). I confirmed WR-01 at runtime without a self-recursive
  script. The probe called `op_rs3_patch('leaf')` once at the JS stack limit, which caused a real
  runtime-creation failure, and once more at shallow depth. The second call succeeded, yet it was
  logged as `error: nested JS host failure in "leaf" ... a nested JS runtime failed while running "leaf"`
  and threw `internal error` into the script.

**Accepted risks, not re-reported:** no depth limit or timeout until Phase 5; only `op_panic`
disabled; a missing key throws a generic `Error`; Promise results stringify as
`[object Promise]`; the 422 body carries only the message; the unprotected platform is dev-only;
store writes made before a nested host fault are kept.

**Still open:** the three Info items from iteration 1 were out of fix scope and are unchanged.
They are carried forward as IN-04 to IN-06.

## Narrative Findings (AI reviewer)

## Warnings

### WR-01: A host fault poisons every later `op_rs3_patch` call in the same job, so successful nested runs are blamed for the fault and throw

**File:** `src/js/runtime.rs:23-33`, `src/js/ops.rs:165-181`

**Issue:** `run_script` decides whether *this* run hit a host fault by reading the job-wide flag
after the run:

```rust
let watch = ctx.clone();
let result = run_fresh(ctx, name, source);
match result {
    Err(ExecError::Internal(_)) => result,
    _ if watch.host_fault() => Err(ExecError::Internal(...)),
    _ => result,
}
```

The flag is shared across the whole job (`ExecCtx::nested` clones the `Arc`) and is never reset.
Once one nested run has faulted and the script has caught the error, every later
`op_rs3_patch` call does the following, even when its nested script completes normally:

1. Its nested `run_fresh` returns `Ok(..)`.
2. `run_script` sees the stale flag and returns `Internal("a nested JS runtime failed while running \"<key>\"")`.
3. `op_rs3_patch` takes the `other` arm. It logs `error: nested JS host failure in "<key>" at depth N`,
   naming a key that did not fault, and throws `Error: internal error` into the caller.

I reproduced this at runtime (see Summary). The second, healthy `op_rs3_patch('leaf')` produced
a second host-failure log line blaming `"leaf"`, and the script's `catch` received
`internal error`. The final HTTP status is still 500, so the client contract holds. What breaks:
- **Server logs are wrong.** They report extra host faults, on keys that ran correctly. That
  misleads whoever is debugging the real 500.
- **Valid results are lost.** A nested script that already ran, including its store writes,
  turns into a thrown error, so script code that branches on success or failure takes the
  wrong branch.

**Fix:** Attribute a fault to a run only if the flag changed during that run:

```rust
pub fn run_script(ctx: ExecCtx, name: &str, source: String) -> Result<String, ExecError> {
    let watch = ctx.clone();
    let before = watch.host_fault();
    let result = run_fresh(ctx, name, source);
    match result {
        Err(ExecError::Internal(_)) => result,
        _ if !before && watch.host_fault() => Err(ExecError::Internal(format!(
            "a nested JS runtime failed while running {name:?}"
        ))),
        _ => result,
    }
}
```

The top level always starts with `before == false`, so it still answers `Internal`. Better still,
fail fast once the flag is set (see WR-02), so later patches never start. Add a cfg(test)
regression test, for example `try { patch('bad') } catch {}; patch('good')` with
`fail_runtime_at_depth`. It needs to inject a failure for one key only, not by depth, so the
hook would need a key filter. The test should assert that `good`'s result reaches the script and
that the top level is still `Internal`.

### WR-02: After a host fault the script keeps running, and its later writes are committed even though the request answers 500

**File:** `src/js/ops.rs:172-181`, `docs/js-ops.md:68-73`

**Issue:** When a nested host fault occurs, `op_rs3_patch` sets the flag and throws a catchable
`Error`. Nothing stops the script after that. It can catch the error and go on to call
`op_rs3_insert` / `op_rs3_delete` and further `op_rs3_patch` calls (which are poisoned, see
WR-01). Every one of those writes is committed, and then the request answers
`500 {"error":"internal","message":"internal error"}`.

The test `caught_nested_runtime_failure_is_still_internal_and_hides_details`
(`src/js/ops.rs:332-341`) shows this. It writes `seen` from the `catch` block *after* the fault
and asserts that the write persisted.

The documentation states a narrower contract: "Store writes the script made **before** that stay"
(`docs/js-ops.md:72-73`). The accepted risk covers writes made before the fault only. Writes made
after the fault are not disclosed. A client that gets 500 `internal error` has no reason to
expect that the script went on to change state after the server had already decided the request
failed.

**Fix:** Pick one of these:
- **Fail fast (preferred).** At the top of `op_rs3_insert`, `op_rs3_delete` and `op_rs3_patch`,
  refuse once the job has faulted:
  ```rust
  let ctx = state.borrow::<ExecCtx>();
  if ctx.host_fault() {
      return Err(JsErrorBox::generic("internal error"));
  }
  ```
  For a stronger guarantee, call `terminate_execution()` on the calling isolate from
  `op_rs3_patch` after `note_host_fault()`. Change the test so it asserts that the
  post-fault write is *refused*.
- **Document it.** Change `docs/js-ops.md:72-73` (and the op table row on line 25) to say that the
  script keeps running after the fault, and that any writes it makes afterwards are also kept,
  even though the request answers 500.

## Info

### IN-01: The host-fault log line repeats its prefix

**File:** `src/http/error.rs:36`, `src/js/mod.rs:248`, `src/js/ops.rs:174-178`

**Issue:** `ExecError::Internal` displays as `JS host failure: {0}`, and both log sites add the
same prefix again. A nested fault reaching the top level is therefore logged as
`error: JS host failure: JS host failure: a nested JS runtime failed while running "probe"`.
The nested site likewise prints `... at depth 1: "JS host failure: cannot create JS runtime: ..."`.

**Fix:** Log `{e:?}`, or match out the inner string, for example
`ExecError::Internal(m) => eprintln!("error: JS host failure: {m}")`. Alternatively, drop the
prefix from the `#[error]` attribute.

### IN-02: The docs say a script cannot fake server log lines, but stdout is still written raw

**File:** `docs/js-ops.md:151-158`, `src/js/ops.rs:195-199`, `src/main.rs:38`

**Issue:** `Deno.core.print(text)`, and the `console.log` that deno_core builds on top of it, write
`text` to stdout unescaped. The server writes its own startup line, `Starting Http server at ...`,
to stdout too. A script can therefore fake that line, or any other stdout line. The doc says
"so a script cannot fake server log lines" right after describing only the stderr escaping. It
does go on to call both streams untrusted, but the earlier sentence overclaims.

**Fix:** Limit the claim to stderr, for example "... so a script cannot fake lines in the server's
stderr log". Alternatively, give stdout output a prefix or escaping as well.

### IN-03: The `op_print` replacement no longer retries on `WouldBlock`

**File:** `src/js/ops.rs:190-200`

**Issue:** deno_core's own `op_print` (`ops_builtin.rs:243-249`) uses a manual write loop, because
"the fd may be in non-blocking mode". `op_rs3_print` uses `write_all` / `writeln!`, which fail
with `WouldBlock` on a non-blocking stdout or stderr. When rs3 is started with inherited
non-blocking pipes, for example by a Node-based parent such as the planned VS Code integration,
`Deno.core.print` would throw and turn an otherwise good script into a 422.

**Fix:** Copy upstream's loop (retry on `ErrorKind::WouldBlock` / `Interrupted`), or add a note in
the source that this is a known difference from upstream.

### IN-04 (carried over from iteration 1 IN-01): The `.txt` payloads posted by the REST walkthrough are untested copies of the `.js` fixtures

**File:** `requests/bin.txt:2`, `requests/fac.txt:2`, `requests/sum.txt:2`, `requests/store.txt:2`

**Issue:** The tests `include_str!` the `.js` files (`src/http/mod.rs:383-386`). The `.http`
walkthrough and `docs/js-ops.md:200-201` post the hand-escaped `.txt` copies instead, and those
already differ from the `.js` files (`bin.txt` uses `t`/`s` where `bin.js` uses `tree`).

**Fix:** For each `requests/*.txt`, add a test that runs it through `parse_insert_body`, inserts
the result, PATCHes it and asserts the same body as the matching `.js` fixture.

### IN-05 (carried over from iteration 1 IN-02): The `catch_unwind` comment overstates what it protects

**File:** `src/js/executor.rs:126-128`

**Issue:** The comment still says "a panicking job cannot kill the worker". A panic inside an op
callback, `op_rs3_patch` and its nested runtime included, unwinds across V8 frames and aborts the
process. `catch_unwind` only covers job code that runs outside V8 callbacks.

**Fix:** Change the comment to "a panic in job code outside V8 callbacks", and add a reference to
the op no-panic rule.

### IN-06 (carried over from iteration 1 IN-03): `stack_bytes` is not clamped, unlike `workers` and `queue_capacity`

**File:** `src/js/executor.rs:33-35,45`

**Issue:** A zero or tiny `stack_bytes` is passed straight to `stack_size`. V8 bootstrap can then
overflow the native stack, and that is a process abort, not an error. The settings are built in
code today, but they become a hazard once they are configurable.

**Fix:** Use `.stack_size(settings.stack_bytes.max(2 * 1024 * 1024))` and document the floor
alongside the other clamps.

---

_Reviewed: 2026-10-20T22:05:00Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
