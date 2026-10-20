---
phase: 03-javascript-runtime-port
reviewed: 2026-10-20T20:33:17Z
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
  critical: 1
  warning: 3
  info: 3
  total: 7
status: issues_found
---

# Phase 3: Code Review Report

**Reviewed:** 2026-10-20T20:33:17Z
**Depth:** standard
**Files Reviewed:** 23
**Status:** issues_found

## Summary

I reviewed the JS runtime port: the op surface (`src/js/ops.rs`), the per-job runner
(`src/js/runtime.rs`), the dedicated pool (`src/js/executor.rs`), the PATCH handler and
error mapping, the migrated JS artefacts and `docs/js-ops.md`. I left the accepted risks
alone: no depth limit, no timeout, only `op_panic` disabled, missing key as `generic`,
Promise stringification, a 422 body carrying only the message, and the unprotected
platform in dev-dependencies only.

The pool design is sound. Workers survive panicking jobs, start-up failure tears down
cleanly, and stores stay isolated per job. The artefact migration is complete: all four
`.txt` payloads are valid JSON, and the grep gate finds no `opSync`, no `Deno.core.ops()`
and no un-prefixed `rs3_` calls.

The main defect is in how the completion value is turned into a string. `run_script` reads
it with `Value::to_rust_string_lossy` outside any `TryCatch`. That call runs user code and
quietly turns any exception into an empty string. Scripts that should get a 422 get a 200
with `""` instead. The secondary defects are a host fault in a nested runtime surfacing as
a 422 that contains host details, and incomplete protection against forged log lines.

**Probe evidence.** I built a throwaway crate under `target/` against the `rs3` library
(release, 1 worker, 16 MiB stack), ran it, then deleted it. It ran no self-recursive
PATCH. Results:
- `Symbol('x')`, `({toString(){ throw new Error('x') }})`, `Object.create(null)` and
  `({[Symbol.toPrimitive](){ return Symbol() }})` each returned `Ok("")`.
- `'[' + Deno.core.ops.op_rs3_patch('inner') + ']'`, with `inner = "Symbol('x')"`,
  returned `Ok("[]")`.
- A chain of 40 distinct keys, each recursing deep and then calling `op_rs3_patch` on the
  next key, did not crash. It ended in
  `Err(Script("Uncaught Error: Error: Error: JS host failure: cannot create JS runtime: Failed to parse ext:core/00_primordials.js"))`.

I had suspected that stacking nested isolates could overflow the 16 MiB native stack and
kill the process. The chain probe disproved that, so it is not reported.

## Narrative Findings (AI reviewer)

## Critical Issues

### CR-01: An exception while stringifying the completion value becomes 200 `""` instead of 422

**File:** `src/js/runtime.rs:31-33` (also reached through `src/js/ops.rs:133` for nested PATCH)

**Issue:** After `execute_script` succeeds, the result is read like this:

```rust
deno_core::scope!(scope, &mut rt);
let local = v8::Local::new(scope, global);
Ok(local.to_rust_string_lossy(scope))
```

In v8 150.4.0, `Value::to_rust_string_lossy` is
`self.to_string(scope).map_or_else(String::new, ...)`. See
`v8-150.4.0/src/value.rs:578-585`. `ToString` on an object runs user code: `toString`,
`valueOf`, `Symbol.toPrimitive`, or Proxy traps. On a Symbol it throws. `deno_core::scope!`
(`runtime/jsruntime.rs:702-709`) installs no `TryCatch`. Any exception at this point is
therefore reported to stderr by V8 (`t:14: Uncaught Error: x`) and swallowed, and the
caller gets `Ok("")`. This was confirmed at runtime for all four probe scripts above.

What this breaks:
- **Wrong status (D-10).** A script whose completion value throws during conversion
  answers **200 `""`** instead of **422 js_error**. The client cannot tell this apart from a
  script that really returned `''`.
- **Nested runs lose the error silently.** `op_rs3_patch` returns `""` to the calling
  script instead of throwing, as `Ok("[]")` above shows.
- **The documentation is wrong.** `docs/js-ops.md:73-75` and the op table (line 25) promise
  `String(completion value)`. But `String(Symbol('x'))` is `"Symbol(x)"`, and rs3 returns
  `""`.
- **User code runs after the script has "completed."** That code can call ops,
  `op_rs3_insert` included, and nothing reports its failures.

**Fix:** Do the conversion under a `TryCatch`, map any exception to
`ExecError::Script`, and special-case symbols so the result matches JS `String(..)`:

```rust
deno_core::scope!(scope, &mut rt);
let local = v8::Local::new(scope, global);
if local.is_symbol() {
    // String(sym) === sym.description-based "Symbol(desc)"; ToString would throw.
    let sym = local.cast::<v8::Symbol>();
    let desc = sym.description(scope);
    let d = if desc.is_undefined() { String::new() } else { desc.to_rust_string_lossy(scope) };
    return Ok(format!("Symbol({d})"));
}
v8::tc_scope!(let tc, scope);
match local.to_string(tc) {
    Some(s) => Ok(s.to_rust_string_lossy(tc)),
    None => {
        let msg = tc
            .exception()
            .map(|e| e.to_rust_string_lossy(tc))
            .unwrap_or_else(|| "exception while converting the result to a string".into());
        eprintln!("js error in {name:?} while stringifying the result: {msg:?}");
        Err(ExecError::Script { message: format!("Uncaught {msg}") })
    }
}
```

(Adapt this to the pinned-scope `TryCatch` API in v8 150.) Add regression tests: PATCH of
`({toString(){throw new Error('x')}})` and of `Object.create(null)` gives 422, PATCH of
`Symbol('x')` gives 200 `"Symbol(x)"`, and the same cases through `op_rs3_patch`.

## Warnings

### WR-01: A host fault in a nested runtime turns into a catchable JS `Error` and a 422 whose body contains host details

**File:** `src/js/ops.rs:133-141`

**Issue:** `op_rs3_patch` maps every non-`Script` `ExecError` into a JS error:

```rust
other => JsErrorBox::generic(other.to_string()),
```

`ExecError::Internal` displays as `JS host failure: cannot create JS runtime: <deno_core detail>`.
That message becomes a catchable JS `Error`. If the script does not catch it, the request
answers **422 js_error** with the host text in the body. The probe produced
`Uncaught Error: Error: Error: JS host failure: cannot create JS runtime: Failed to parse ext:core/00_primordials.js`.
This breaks three stated contracts:
- D-11 says host faults, including a runtime creation failure, are 500.
- `docs/js-ops.md:96-98` says "If ... a runtime cannot be created, the answer is
  `{"error":"internal","message":"internal error"}`. The details go to the server log only."
- `src/http/error.rs:28-31` says host details never reach the client.

The failure is easy to hit, and not only through runaway recursion. V8's stack budget is
shared by nested isolates on a thread: the probe showed the usable depth shrinking
7759 → 3365 → 1443 frames per level. Any script that calls `op_rs3_patch` from moderately
deep recursion can therefore fail runtime bootstrap. A script can also `catch` the host
fault and carry on, which hides it completely.

**Fix:** Do not hand host faults to JS as ordinary errors. At minimum, log the detail and
throw a fixed, generic message:

```rust
other => {
    eprintln!("error: nested JS host failure for {key:?}: {other}");
    JsErrorBox::generic("internal error in nested script")
}
```

Better: also record a "host fault" flag in `OpState` (or in a field shared through
`ExecCtx`). `run_script` can then turn the top-level result into `ExecError::Internal`
(500) even if the script caught the error. Either way, update `docs/js-ops.md` lines 25
and 96-98, and the "Unbounded nesting" paragraph, to match the behaviour you choose.

### WR-02: Log forging is still possible through exception messages and `Deno.core.print(.., true)`

**File:** `src/js/runtime.rs:24-25`

**Issue:** The comment says it Debug-formats the name "so a key with control characters
cannot forge log lines". But the same `eprintln!` prints `{e}` with `Display`. That is
`JsError`'s Display (`deno_core error.rs:1362-1376`), which writes the raw multi-line
`stack` or `exception_message`. Both are fully script-controlled.

A stored script `throw new Error("x\nerror: JS host failure: forged")` writes a separate
stderr line that cannot be told apart from the real host-failure line written by
`src/http/error.rs:36`. Every nested level in `op_rs3_patch` repeats this.

Scripts can also write arbitrary bytes to stderr directly through the built-in
`Deno.core.print(msg, true)` (`op_print`, `is_err = true`, `ops_builtin.rs:234-241`).
`docs/js-ops.md:136-138` says print writes only to stdout, which is incomplete.

**Fix:** Escape what the script controls before logging it. For example, log
`{:?}` of `e.exception_message`, and log the stack as Debug or with each line escaped:

```rust
eprintln!("js error in {name:?}: {:?}", e.to_string());
```

Fix the doc as well: say that `Deno.core.print(text, true)` writes to stderr, and that
script output in either stream is untrusted. If clean server logs matter before v2,
consider overriding `op_print` through the `middleware` hook so script output is
prefixed or escaped.

### WR-03: `full_queue_makes_callers_wait` does not test what its name says

**File:** `src/js/executor.rs:232-263`

**Issue:** The test blocks the single worker and submits 7 more jobs into a queue of
capacity 1. It then checks only that every submitter eventually gets `Ok("job {i}")`.
That outcome is identical with an unbounded queue, or with a capacity large enough that
no caller waits. The D-06 backpressure property ("when the queue is full, callers wait")
is therefore never checked, and a regression to `unbounded_channel` would pass.

**Fix:** While the gate is closed, assert that submissions beyond `capacity` are still
pending. For example, use a `tokio::time::timeout` or a `try_send`-style probe on a
submitter that must not finish, or count submitters whose `run` future has returned
before `gate_tx.send` and check that the count is 0 and that at most `capacity` jobs were
accepted. Then release the gate and check that all of them complete.

## Info

### IN-01: The `.txt` payloads the REST walkthrough posts are separate, untested copies of the `.js` fixtures

**File:** `requests/bin.txt:2`, `requests/fac.txt:2`, `requests/sum.txt:2`, `requests/store.txt:2`

**Issue:** The tests use `include_str!` on the `.js` files (`src/http/mod.rs:383-386`).
The `.http` walkthrough and `docs/js-ops.md:177-179` post the hand-escaped `.txt` copies,
and those already differ from the `.js` files (`bin.txt` uses `t`/`s`, `bin.js` uses
`tree`). A future change could break the documented workflow without any test failing.

**Fix:** Add one test that parses each `requests/*.txt` with `parse_insert_body`, inserts
the result, runs the PATCH and checks the same expected body as the `.js` fixture.

### IN-02: The `catch_unwind` comment overstates what it protects

**File:** `src/js/executor.rs:126-139`

**Issue:** The comment says "a panicking job cannot kill the worker". That is true only
for panics outside V8 callbacks. A panic inside any op callback unwinds across V8's C++
frames and aborts the process. That is the reason `op_panic` is disabled, and
`catch_unwind` cannot help there.

**Fix:** Change the comment to "a panic in job code outside V8 callbacks", and refer to
the op no-panic rule (ROBU-04).

### IN-03: `stack_bytes` is not clamped, unlike `workers` and `queue_capacity`

**File:** `src/js/executor.rs:30-35,45`

**Issue:** `start` clamps a zero worker count and a zero queue capacity to 1, but passes
`settings.stack_bytes` straight to `stack_size`. A zero or tiny value gives a thread with
the platform minimum stack, where V8 bootstrap can overflow natively. Settings are built
in code today, but this becomes a risk once they are env-configurable (deferred).

**Fix:** Clamp to a sane floor, for example `settings.stack_bytes.max(2 * 1024 * 1024)`,
and document it next to the other clamps.

---

_Reviewed: 2026-10-20T20:33:17Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
