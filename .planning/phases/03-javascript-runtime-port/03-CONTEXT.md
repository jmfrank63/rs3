# Phase 3: JavaScript Runtime Port - Context

**Gathered:** 2026-10-20
**Status:** Ready for planning
**Mode:** Smart discuss (autonomous, non-interactive — every grey area auto-accepted with the Recommended answer)

<domain>
## Phase Boundary

Bring back `PATCH /key/{key}`: it runs the stored value as JavaScript on current released
`deno_core` (0.412) and returns the stringified completion value. JS can call the five store
ops `Deno.core.ops.op_rs3_list/get/insert/delete/patch`, defined with `#[op2]` and registered
through `extension!`. The ops work against the same injected `Store` the HTTP handlers use.
JS runs on a bounded pool of dedicated JS threads (not actix workers), and script errors
become 422s instead of panics. Every JS artefact is migrated to the new op syntax, and the
old→new mapping is documented.

Not in this phase: the nesting-depth limit and its 422, and the execution timeout and its 504
(Phase 5: ROBU-05/06, TEST-04). The `depth` counter is plumbed through `ExecCtx` now. Also not
here: CI (Phase 4), Docker and README (Phase 6), heap limits, cancel-on-disconnect, 503
backpressure and snapshots (v2 JSHD-*), and the `rs3` JS facade or Promise results (v2 APIV-*).

Requirements: HTTP-07, JSRT-01, JSRT-02, JSRT-03, JSRT-04, JSRT-05, JSRT-06, ROBU-03, ROBU-04,
TEST-03, TEST-05, TEST-06.

</domain>

<decisions>
## Implementation Decisions

### Op surface and error semantics
- **D-01:** Five ops in `src/js/ops.rs` (or similar), defined with `#[op2]` and registered through `deno_core::extension!(rs3_ext, ops = [...], options = { ctx: ExecCtx }, state = ...)`. They are called from JS as `Deno.core.ops.op_rs3_list()`, `op_rs3_get(key)`, `op_rs3_insert(json)`, `op_rs3_delete(key)` and `op_rs3_patch(key)`. Return values are strings:
  - `op_rs3_list` returns the inner `/list` payload `[{"k" : "v"},...]` through the shared Phase 2 codec `store::legacy::format_list`, so its bytes equal the inner text of `GET /list`.
  - `op_rs3_get` returns the value.
  - `op_rs3_insert` takes a JSON object string, as legacy did, and returns the echo `format_insert_echo`. It validates with `parse_insert_body` before `insert_all`, so it is all-or-nothing.
  - `op_rs3_delete` returns the old value.
  - `op_rs3_patch` returns `String(result)` of the nested script.
  - An extra argument (legacy passed `""` to list) is ignored.
- **D-02:** A missing key or invalid input inside an op becomes a catchable JS exception. Ops return `Result<_, deno_error::JsErrorBox>`: a missing key is a not-found-class error with message `key not found: <key>`, and malformed or wrong-shape insert JSON is a type-class error with the codec's message. No op ever panics: no `unwrap`/`expect` on request data (ROBU-04, and the clippy deny gate already enforces this). An uncaught op error fails the script, and PATCH returns 422 (D-10). The exact JS class/name mapping (`JsErrorBox::not_found` vs a generic `Error` with a name) is Claude's discretion after research. It must be `try/catch`-able and documented in the mapping table.
- **D-03:** Ops reach the store through `ExecCtx { store: Store, depth: u32 }` (plus later limits), placed in `OpState` per job via the extension's `options`/`state` hook. There is no global store (ARCH-02). The JS executor is store-agnostic: each job carries the `Store` handle of the `AppState` that issued it, so isolated per-test stores work with a shared pool.
- **D-04:** `op_rs3_patch` is `#[op2(reentrant)]`. Inside the op, on the same JS thread, it creates a fresh nested `JsRuntime` with `ExecCtx { depth: depth + 1, same store }`, runs the target key's script, converts the result to an owned `String`, and drops the runtime before returning (LIFO drop order). **Spike this first** (research flag): if a nested runtime inside a reentrant op does not work on deno_core 0.412, stop and surface it. Do not silently switch to dispatching onto another pool thread, which can deadlock when the pool is saturated. The depth counter is incremented and carried now. Enforcing the limit (422 naming the key chain) is Phase 5, and until then a self-patching script still overflows the stack, as legacy did. Record that as a known Phase 5 item.

### Execution model and threading
- **D-05:** JS runs on a bounded pool of dedicated named OS threads (`rs3-js-{i}`, explicit stack size, default 16 MiB). Each thread builds and enters a tokio `current_thread` runtime (`enable_all`). deno_core 0.412 aborts if a `JsRuntime` is created outside a tokio context and V8 later posts a delayed task. Each thread creates a **fresh `JsRuntime` per job** (no reuse: top-level `let` collisions, state leaks), runs `execute_script`, converts the completion value to an owned `String` on the worker, drops the runtime, and replies over a oneshot channel. The actix handler only awaits the reply, so actix workers never run V8 (JSRT-04). The phase must end on this pool. `web::block` is acceptable only as a stepping stone inside the phase.
- **D-06:** A `JsSettings` (or `JsLimits`) struct is built **in code**, not read from the environment, because D-16 of Phase 2 allowlists only `server.host`/`server.port`. Defaults: workers = `std::thread::available_parallelism()` (min 1), queue capacity = workers × 16, thread stack = 16 MiB. When the queue is full, callers wait (bounded channel send); 503 backpressure is v2 (JSHD-03). `AppState` gains a `js` executor handle field (cheap `Clone`). `main` starts the pool once, after `init_platform()` and before `HttpServer` (D-07).
- **D-07:** The V8 platform is initialised exactly once on the main thread before any other thread spawns. Production keeps `JsRuntime::init_platform(None)` in `main` (Phase 1 D-23). Tests enable `deno_core`'s `unsafe_use_unprotected_platform` feature **only in `[dev-dependencies]`**, plus a process-wide `Once`/`LazyLock` test init, which solves the libtest sibling-thread problem without changing the release binary. This must be verified on x86_64 in Phase 4 CI. The documented fallback, if CI crashes, is `new_unprotected_default_platform` in a `Once`. Adding the dev-dependency feature line must keep the lockfile on released versions only (`--locked` builds), since it is the same `deno_core` version.
- **D-08:** Tests share one process-wide executor (a `LazyLock` in test support code is allowed: it holds an executor, not a store), and stores stay per-test through `ExecCtx`. The full suite must pass under default parallel `cargo test`, `--test-threads=1`, `env -i` and a 5× repeat.
- **D-09:** Dependencies: only released crates. A direct `tokio` dependency (features limited to what is needed, e.g. `rt`, `sync`, `macros` only if required) and/or a small channel crate (e.g. `async-channel`) are acceptable if research shows they are needed. Prefer the fewest new crates, and versions must resolve to what is already in `Cargo.lock` where possible. No direct `v8` dependency. Use `deno_core::v8`. `deno_error = "0.7"` is already present.

### PATCH results and errors
- **D-10:** `PATCH /key/{key}`:
  - **Missing key:** 404 `not_found` (unchanged from Phase 2).
  - **Existing key:** run its value as a script named after the key. Success is 200 with the JSON-string envelope of `String(completion value)`, read with `deno_core::scope!` + `v8::Local::new` + `to_rust_string_lossy`. Do not create a new `v8::Context` (that was the legacy hack). Numbers become `"120"`, `undefined` becomes `"undefined"`, objects become `"[object Object]"`, and a Promise becomes `"[object Promise]"` (document this; awaiting promises is v2 APIV-02).
  - **Script errors:** a thrown exception, a syntax error, or an op error the script did not catch gives **422** `{"error":"js_error","message":"<exception message>"}` (D-01 shape from Phase 2). The message is the JS exception message (e.g. `Uncaught Error: boom`); the stack is logged server-side (stderr) and not returned.
- **D-11:** Host faults (runtime creation failure, executor channel closed, worker panic) are **500** `{"error":"internal",...}`. This finally constructs `ApiError::Internal` (resolves Phase 2 review IN-01) and gets a test. A JS worker thread must survive a failing job: `catch_unwind` around a job, or an equivalent design, is Claude's discretion, as long as one bad job cannot take the pool down.
- **D-12:** Re-enable the two parked PATCH tests (`test_patch_is_ok`, `test_patch_body_is_listing`) on a per-test `AppState`, removing their `#[ignore]` (the total `#[ignore]` count drops to 0). Replace `test_patch_existing_is_501` with real success and error tests. TEST-06: a PATCH whose script returns `Deno.core.ops.op_rs3_list()` yields the same body as `GET /list`.

### JS artefact migration and documentation
- **D-13:** Migrate every JS artefact to `Deno.core.ops.op_rs3_*(...)` and remove every `Deno.core.ops();` call. That covers `requests/*.js` (bin, fac, store, sum), the embedded payloads in `requests/*.txt` and `requests/*.http`, and `src/example.js` (also fix its unregistered `op_list` → `op_rs3_list`). Keep each script's behaviour. A grep gate proves no `opSync`, `Deno.core.ops()` or `rs3_` without the `op_` prefix remains in JS artefacts.
- **D-14:** `fac.js`, `sum.js` and `bin.js` run as test fixtures via `include_str!("../../requests/<name>.js")` (or a path from the crate root), against seeded keys: `num="5"` gives `"120"`; `numbers="1 2 3 4"` gives `"10"`; `tree="5 3 8"` gives the JSON tree string. Each test asserts the exact body (TEST-05). `store.js` gets an op-level test (insert via JS, then `GET /key/res` returns `"1111"`).
- **D-15:** Document the old→new op mapping in a new `docs/js-ops.md` (JSRT-06): a table of old call → new call → argument → return → throws-when, plus the "missing key throws" rule, the `Deno.core.ops();` removal, the PATCH result-stringification rules and the Promise caveat. The Phase 6 README links to and summarises it.
- **D-16:** Every route now has a success test asserting status and preserved body bytes (TEST-03). PATCH is the last one, the other six exist from Phase 2.

### Phase 2 review housekeeping (operator-routed follow-ups)
- **D-17:** Fold in the deferred Phase 2 review info items:
  - **IN-01:** real `ApiError::Internal` path plus a test (D-11).
  - **IN-04:** correct the "first problem in body order decides" doc sentence in `parse_insert_body`.
  - **IN-05:** rename `parse_rejects_deep_nesting` and `insert_deeply_nested_is_400_json` to truncated-input names.

### Claude's Discretion
- Module layout inside `src/js/` (`mod.rs`, `ops.rs`, `executor.rs`, `runtime.rs`), type names (`JsExecutor`, `ExecError`, `ScriptJob`), and channel primitives.
- How the worker survives a panicking job (D-11).
- Whether `ExecCtx` already carries a placeholder `limits` field for Phase 5.
- Test helper names and fixture seeding helpers. Exact JS error class names (documented).

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Project scope
- `.planning/PROJECT.md`: core value, constraints, key decisions
- `.planning/REQUIREMENTS.md`: HTTP-07, JSRT-01..06, ROBU-03, ROBU-04, TEST-03, TEST-05, TEST-06 (this phase), plus Out of Scope (reusing one `JsRuntime`, `deno_runtime`, an `opSync` shim)
- `.planning/ROADMAP.md` §Phase 3: success criteria and notes (spike list)

### Prior phases
- `.planning/phases/02-http-api-port-error-handling/02-CONTEXT.md`: error body D-01, ApiError D-02, AppState/configure D-10/D-11, lint gate D-13, config allowlist D-16
- `.planning/phases/02-http-api-port-error-handling/02-0{1,2,3}-SUMMARY.md`: what was built (AppState, ApiError, insert_all, escaped codec)
- `.planning/phases/02-http-api-port-error-handling/02-REVIEW-DISPOSITION.md`: IN-01, IN-04, IN-05 routed here
- `.planning/phases/01-toolchain-dependencies-foundation/01-CONTEXT.md`: D-05 (deno_core added), D-17 (parked PATCH tests), D-23 (init_platform in main)

### Research
- `.planning/research/SUMMARY.md`: resolved decisions (a) the JS execution model, (b) V8 platform init, (g) missing key throws
- `.planning/research/ARCHITECTURE.md`: JsExecutor pool pattern, ExecCtx in OpState, nested patch in a reentrant op, stack sizing, result read via `scope!`, ExecError → status mapping
- `.planning/research/PITFALLS.md`: an op panic aborts the process, re-entrancy aborts in debug, stack overflow on recursion, PKU/protected-platform segfaults, the tokio-context abort
- `.planning/research/STACK.md`: deno_core 0.412 API cheat-sheet (`#[op2]`, `extension!`, `rs3_ext::init`, `execute_script`, `scope!`), deno_error pin
- `.planning/research/FEATURES.md`: T6–T8 (PATCH semantics, op mapping table, artefact migration)

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- `src/js/mod.rs`: `init_platform()` (keeps `JsRuntime::init_platform(None)`). This is the seam Phase 3 fills.
- `src/state.rs` `AppState { store, settings }` + `test_state()`. Add the `js` handle here.
- `src/store/legacy.rs` `parse_insert_body`, `format_insert_echo`, `format_list`: the shared codec for `op_rs3_insert`/`op_rs3_list`.
- `src/store/mod.rs` `Store::{get, insert_all, remove, list}`: owned-value, guard-free, and safe under re-entrant op calls (papaya).
- `src/http/error.rs` `ApiError` (exhaustive mapping, no wildcard). Add `js_error` (422) and use `Internal` (500).
- `src/http/mod.rs` `patch_key` (currently 404/501) plus the two parked PATCH tests and `assert_json_error` helper.

### Established Patterns
- Success bodies use the JSON-string envelope `HttpResponse::Ok().json(String)`; errors are `{"error","message"}` JSON objects.
- `#![forbid(unsafe_code)]`, clippy `unwrap_used`/`expect_used` deny outside tests (clippy.toml test allowances), no `#[allow]`.
- TDD RED/GREEN commit pairs with JUnit evidence under `tdd-evidence/`. Live-binary curl smokes on distinct ports (Phase 2 used 18460–18499).

### Integration Points
- `src/main.rs`: `init_platform()` → start the JS pool → `AppState` with the `js` handle → `HttpServer::new(configure(state))`.
- `requests/`, `src/example.js`: artefact migration (D-13).
- `docs/js-ops.md` (new): the mapping doc (D-15).

</code_context>

<specifics>
## Specific Ideas

- **Git identity (user instruction):** commit with env vars `GIT_AUTHOR_NAME="Johannes Maria Frank"`, `GIT_AUTHOR_EMAIL="jmfrank63@gmail.com"`, `GIT_COMMITTER_NAME="Johannes Maria Frank"`, `GIT_COMMITTER_EMAIL="jmfrank63@gmail.com"`. **Never modify git config.** Commit messages end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- **Never `git push`.** Stay on `modernize-toolchain`.
- The TOOL-03 gate (`cargo build --locked`, `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`) must pass at the end of every plan. No `#[allow]`.
- Keep `target/` inside the project dir (`/tmp` is about 96% full).
- A project hook blocks agents from creating or naming the dotenv file. Use env-var config for live smokes.
- Highest-risk phase: put the nested-runtime/reentrant spike and the test-platform strategy in the first plan's first task, so a dead end surfaces before the pool and the artefact work are built on it.

</specifics>

<deferred>
## Deferred Ideas

- Nesting-depth limit (422 with key chain) and execution timeout (504, configurable via settings): Phase 5.
- Heap limit / near-heap-limit callback, cancel on client disconnect, 503 backpressure, V8 snapshots: v2 (JSHD-01..04).
- `rs3` JS facade object, awaiting Promise results, a versioned API: v2 (APIV-01..03).
- Env-configurable JS pool settings: revisit with Phase 5's timeout setting or OPS-02.

</deferred>

---

*Phase: 03-javascript-runtime-port*
*Context gathered: 2026-10-20 (smart discuss, auto-accepted recommendations)*
