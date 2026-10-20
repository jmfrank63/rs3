# Phase 3: JavaScript Runtime Port - Research

**Researched:** 2026-10-20
**Domain:** Embedding V8 through deno_core 0.412 (`#[op2]` ops, `extension!`, nested runtimes) behind an actix-web 4.17 handler, using a bounded pool of dedicated JS threads
**Confidence:** HIGH. Every research flag was resolved by compiling and running a throwaway spike against the pinned crates (deno_core 0.412.0, deno_error 0.7.1, tokio 1.54.0, papaya 0.2.5) with the project's own `Cargo.lock`, offline, on this aarch64 host, in debug and release. The spike was deleted afterwards.

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

#### Op surface and error semantics
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

#### Execution model and threading
- **D-05:** JS runs on a bounded pool of dedicated named OS threads (`rs3-js-{i}`, explicit stack size, default 16 MiB). Each thread builds and enters a tokio `current_thread` runtime (`enable_all`). deno_core 0.412 aborts if a `JsRuntime` is created outside a tokio context and V8 later posts a delayed task. Each thread creates a **fresh `JsRuntime` per job** (no reuse: top-level `let` collisions, state leaks), runs `execute_script`, converts the completion value to an owned `String` on the worker, drops the runtime, and replies over a oneshot channel. The actix handler only awaits the reply, so actix workers never run V8 (JSRT-04). The phase must end on this pool. `web::block` is acceptable only as a stepping stone inside the phase.
- **D-06:** A `JsSettings` (or `JsLimits`) struct is built **in code**, not read from the environment, because D-16 of Phase 2 allowlists only `server.host`/`server.port`. Defaults: workers = `std::thread::available_parallelism()` (min 1), queue capacity = workers × 16, thread stack = 16 MiB. When the queue is full, callers wait (bounded channel send); 503 backpressure is v2 (JSHD-03). `AppState` gains a `js` executor handle field (cheap `Clone`). `main` starts the pool once, after `init_platform()` and before `HttpServer` (D-07).
- **D-07:** The V8 platform is initialised exactly once on the main thread before any other thread spawns. Production keeps `JsRuntime::init_platform(None)` in `main` (Phase 1 D-23). Tests enable `deno_core`'s `unsafe_use_unprotected_platform` feature **only in `[dev-dependencies]`**, plus a process-wide `Once`/`LazyLock` test init, which solves the libtest sibling-thread problem without changing the release binary. This must be verified on x86_64 in Phase 4 CI. The documented fallback, if CI crashes, is `new_unprotected_default_platform` in a `Once`. Adding the dev-dependency feature line must keep the lockfile on released versions only (`--locked` builds), since it is the same `deno_core` version.
- **D-08:** Tests share one process-wide executor (a `LazyLock` in test support code is allowed: it holds an executor, not a store), and stores stay per-test through `ExecCtx`. The full suite must pass under default parallel `cargo test`, `--test-threads=1`, `env -i` and a 5× repeat.
- **D-09:** Dependencies: only released crates. A direct `tokio` dependency (features limited to what is needed, e.g. `rt`, `sync`, `macros` only if required) and/or a small channel crate (e.g. `async-channel`) are acceptable if research shows they are needed. Prefer the fewest new crates, and versions must resolve to what is already in `Cargo.lock` where possible. No direct `v8` dependency. Use `deno_core::v8`. `deno_error = "0.7"` is already present.

#### PATCH results and errors
- **D-10:** `PATCH /key/{key}`:
  - **Missing key:** 404 `not_found` (unchanged from Phase 2).
  - **Existing key:** run its value as a script named after the key. Success is 200 with the JSON-string envelope of `String(completion value)`, read with `deno_core::scope!` + `v8::Local::new` + `to_rust_string_lossy`. Do not create a new `v8::Context` (that was the legacy hack). Numbers become `"120"`, `undefined` becomes `"undefined"`, objects become `"[object Object]"`, and a Promise becomes `"[object Promise]"` (document this; awaiting promises is v2 APIV-02).
  - **Script errors:** a thrown exception, a syntax error, or an op error the script did not catch gives **422** `{"error":"js_error","message":"<exception message>"}` (D-01 shape from Phase 2). The message is the JS exception message (e.g. `Uncaught Error: boom`); the stack is logged server-side (stderr) and not returned.
- **D-11:** Host faults (runtime creation failure, executor channel closed, worker panic) are **500** `{"error":"internal",...}`. This finally constructs `ApiError::Internal` (resolves Phase 2 review IN-01) and gets a test. A JS worker thread must survive a failing job: `catch_unwind` around a job, or an equivalent design, is Claude's discretion, as long as one bad job cannot take the pool down.
- **D-12:** Re-enable the two parked PATCH tests (`test_patch_is_ok`, `test_patch_body_is_listing`) on a per-test `AppState`, removing their `#[ignore]` (the total `#[ignore]` count drops to 0). Replace `test_patch_existing_is_501` with real success and error tests. TEST-06: a PATCH whose script returns `Deno.core.ops.op_rs3_list()` yields the same body as `GET /list`.

#### JS artefact migration and documentation
- **D-13:** Migrate every JS artefact to `Deno.core.ops.op_rs3_*(...)` and remove every `Deno.core.ops();` call. That covers `requests/*.js` (bin, fac, store, sum), the embedded payloads in `requests/*.txt` and `requests/*.http`, and `src/example.js` (also fix its unregistered `op_list` → `op_rs3_list`). Keep each script's behaviour. A grep gate proves no `opSync`, `Deno.core.ops()` or `rs3_` without the `op_` prefix remains in JS artefacts.
- **D-14:** `fac.js`, `sum.js` and `bin.js` run as test fixtures via `include_str!("../../requests/<name>.js")` (or a path from the crate root), against seeded keys: `num="5"` gives `"120"`; `numbers="1 2 3 4"` gives `"10"`; `tree="5 3 8"` gives the JSON tree string. Each test asserts the exact body (TEST-05). `store.js` gets an op-level test (insert via JS, then `GET /key/res` returns `"1111"`).
- **D-15:** Document the old→new op mapping in a new `docs/js-ops.md` (JSRT-06): a table of old call → new call → argument → return → throws-when, plus the "missing key throws" rule, the `Deno.core.ops();` removal, the PATCH result-stringification rules and the Promise caveat. The Phase 6 README links to and summarises it.
- **D-16:** Every route now has a success test asserting status and preserved body bytes (TEST-03). PATCH is the last one, the other six exist from Phase 2.

#### Phase 2 review housekeeping (operator-routed follow-ups)
- **D-17:** Fold in the deferred Phase 2 review info items:
  - **IN-01:** real `ApiError::Internal` path plus a test (D-11).
  - **IN-04:** correct the "first problem in body order decides" doc sentence in `parse_insert_body`.
  - **IN-05:** rename `parse_rejects_deep_nesting` and `insert_deeply_nested_is_400_json` to truncated-input names.

### Claude's Discretion
- Module layout inside `src/js/` (`mod.rs`, `ops.rs`, `executor.rs`, `runtime.rs`), type names (`JsExecutor`, `ExecError`, `ScriptJob`), and channel primitives.
- How the worker survives a panicking job (D-11).
- Whether `ExecCtx` already carries a placeholder `limits` field for Phase 5.
- Test helper names and fixture seeding helpers. Exact JS error class names (documented).

### Deferred Ideas (OUT OF SCOPE)
- Nesting-depth limit (422 with key chain) and execution timeout (504, configurable via settings): Phase 5.
- Heap limit / near-heap-limit callback, cancel on client disconnect, 503 backpressure, V8 snapshots: v2 (JSHD-01..04).
- `rs3` JS facade object, awaiting Promise results, a versioned API: v2 (APIV-01..03).
- Env-configurable JS pool settings: revisit with Phase 5's timeout setting or OPS-02.
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| HTTP-07 | `PATCH /key/{key}` executes the stored value as JavaScript and returns the stringified completion value | Spike S5: `execute_script` + `deno_core::scope!` + `to_rust_string_lossy` gives `"120"`, `"undefined"`, `"[object Object]"`, `"[object Promise]"`, `"null"`. Pattern 2 and Code Example "run_script" |
| JSRT-01 | Current released `deno_core` with ops via `#[op2]`, registered through `extension!` | Spike S4: exact signatures compile under `#![forbid(unsafe_code)]` and clippy `-D warnings`. Pattern 1 |
| JSRT-02 | Five store ops with legacy semantics | Spike S4/S6: all five ops ran against an injected store. `op_rs3_list('')` ignores the extra argument. Nested `op_rs3_patch` works (S1) |
| JSRT-03 | Missing key / invalid input → catchable JS exception, no abort | Spike S6: `JsErrorBox::generic` → `Error`, `JsErrorBox::type_error` → `TypeError`, both `instanceof Error` and catchable. **`JsErrorBox::new("NotFound", ..)` throws `undefined`, so don't use it** (Pitfall 2). **`Deno.core.ops.op_panic` aborts the process and must be disabled** (Pitfall 1) |
| JSRT-04 | JS off actix workers on dedicated threads; V8 platform initialised once in `main` first | Spike S8: tokio-only pool (mpsc + shared receiver + oneshot), named threads, `catch_unwind` survival, 64-job burst OK. Pattern 3 |
| JSRT-05 | All JS artefacts use `Deno.core.ops.op_rs3_*` with no `Deno.core.ops();` | Spike: `Deno.core.ops();` → `Uncaught TypeError: Deno.core.ops is not a function`; `opSync` → `... is not a function`. Migrated `fac.js`/`sum.js`/`bin.js` produced `120`/`10`/tree JSON. Artefact inventory below |
| JSRT-06 | Old→new op mapping documented | Mapping table with verified throw behaviour (Code Examples → `docs/js-ops.md` content) |
| ROBU-03 | Throw / syntax error → 422 JSON with the JS message | Spike S7: `JsError.exception_message` = `Uncaught Error: boom`, `Uncaught SyntaxError: Unexpected token ';'`, `Uncaught plain` (non-Error throw) |
| ROBU-04 | No `unwrap`/`expect` on request data; ops return errors to JS | Ops return `Result<String, JsErrorBox>`. Use `JsRuntime::try_new`, not `new` (which panics). Clippy deny gate already active. The `op_panic` middleware closes the remaining abort path |
| TEST-03 | Every route has a success test with status and body bytes | PATCH success test (`"120"` envelope) completes the set. The other six exist (Phase 2) |
| TEST-05 | `fac.js`, `sum.js`, `bin.js` run as fixtures with expected results | Verified outputs: `120`, `10`, `{"value":5,"left":{"value":3},"right":{"value":8}}` |
| TEST-06 | PATCH running `op_rs3_list` == `GET /list` | `op_rs3_list` → `legacy::format_list(&store.list())`, the same function `GET /list` uses. Both go through `HttpResponse::Ok().json(String)` |
</phase_requirements>

## Project Constraints (from CLAUDE.md)

- Toolchain Rust 1.99.1, edition 2024. Single binary crate (lib + thin main, one package).
- Released crates.io versions only: no git, path or pre-release dependencies. `cargo build --locked` must pass.
- **No direct `v8` dependency.** Use `deno_core::v8`. Never `cargo add deno_error`; keep `deno_error = "0.7"`.
- Same HTTP routes and semantics; keep response bodies compatible where tests assert them.
- Store injected (`web::Data<AppState>` for HTTP, `OpState` for JS); no global store. `LazyLock` only for genuinely process-wide things (the test executor qualifies).
- `#![forbid(unsafe_code)]` plus `[lints.rust] unsafe_code = "forbid"`. Clippy `unwrap_used`/`expect_used` deny (test allowances via `clippy.toml`). **No `#[allow]` attributes** (a Phase 4 CI grep gate will enforce this).
- TOOL-03 gate at the end of every plan: `cargo build --locked`, `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`.
- Work on branch `modernize-toolchain`; commit per plan. Git identity comes from `GIT_AUTHOR_*`/`GIT_COMMITTER_*` env vars only (never `git config`). Messages end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. Never push.
- Keep `target/` in the project dir (`/tmp` is about 96% full). A project hook blocks naming the dotenv file, so use env-var config for live smokes.
- GSD workflow: edits only through `/gsd-execute-phase` plans.

## Summary

The phase's highest-risk assumption holds on deno_core 0.412. A `#[op2(reentrant)]` op can create, run and drop a fresh nested `JsRuntime` on the same thread, in debug and release, with a tokio `current_thread` runtime entered. Without `reentrant`, debug builds abort (exit 134) as soon as the nested runtime bootstraps. The `reentrant` flag is therefore mandatory, not cosmetic. All five ops, the `extension!` `options`/`state` injection of a per-job `ExecCtx`, result reading through `deno_core::scope!`, and JS-error extraction compile cleanly under the project's `#![forbid(unsafe_code)]` and clippy `-D warnings`. The bundled samples `fac.js`, `sum.js` and `bin.js` produce `120`, `10` and the expected tree JSON once their op calls are migrated.

The spike found two problems the milestone research did not anticipate. First, bare deno_core exposes about 100 built-in ops on `Deno.core.ops`, including **`op_panic`**: a stored script calling `Deno.core.ops.op_panic('x')` kills the whole server (SIGABRT, exit 134, debug and release). That directly violates success criterion 3. deno_core has a supported fix: an `extension!` `middleware` that rewrites `op_panic` with `OpDecl::disable()`. After that, the call throws a catchable `Error: op is disabled`. This was verified. Second, D-02's "not-found-class" error cannot use a `"NotFound"` class. Bare deno_core only registers `Error`, `RangeError`, `ReferenceError`, `SyntaxError`, `TypeError` and `URIError`, and an unregistered class makes the op throw a bare `undefined` with no message. Use `JsErrorBox::generic` (a JS `Error`) for a missing key and `JsErrorBox::type_error` for bad insert input.

Threading needs **no new crates**. A direct `tokio = { version = "1.54.0", features = ["rt", "sync"] }` resolves to the tokio already in `Cargo.lock`; the only lockfile change is one line, `"tokio",`, in rs3's own dependency list. tokio's `mpsc` (bounded, async `send`) with a receiver shared behind `Arc<tokio::sync::Mutex<_>>` gives the MPMC queue, and `tokio::sync::oneshot` carries the reply. `async-channel` is not in the lockfile and is unnecessary. The dev-dependency `unsafe_use_unprotected_platform` feature leaves `Cargo.lock` byte-identical (`cargo metadata --locked` passes). Cargo builds deno_core with the feature only for test, `--all-targets` and clippy builds, never for `cargo build`/`--release`; build fingerprints confirmed this. Per-request cost is about 9–10 ms per fresh runtime (debug and release are close, because V8 is prebuilt), plus about 8 ms per nested `op_rs3_patch` level.

**Primary recommendation:** Build `src/js/{mod,ops,runtime,executor}.rs` exactly as in the Code Examples: five `Result<String, JsErrorBox>` ops plus an `op_panic`-disabling middleware in one `extension!`, a `run_script(ctx, name, source) -> Result<String, ExecError>` that uses `JsRuntime::try_new`, and a tokio-only bounded pool with `catch_unwind` per job. Go straight to the pool and skip the `web::block` stepping stone, since it is proven.

## Spike Results (research flags resolved)

All results below came from a scratch crate under `target/` that copied the project's `Cargo.lock` and `clippy.toml`, built `--offline` with Rust 1.99.1 on aarch64 Linux (4 cores), then deleted. `[VERIFIED: spike run]` means the output quoted was produced this session.

| # | Flag | Result | Evidence |
|---|------|--------|----------|
| S1 | Nested `JsRuntime` inside `#[op2(reentrant)]` op, same thread, tokio current_thread entered | **Works**, debug and release. Inner script sees its own `ExecCtx` (depth 1), outer keeps depth 0. Top-level `let num` in outer and inner do not collide (separate runtimes). A nested throw is catchable by the outer script | `[nested-patch] OK "5\|1\|outer-depth=0"`, `[nested-let-collision] OK "71"`, `[nested-throw-caught] OK "Error\|Uncaught RangeError: inner boom"` [VERIFIED: spike run] |
| S1b | Same op **without** `reentrant` | **Process abort in debug** (exit 134): `op op_bad_patch was not marked as #[op2(reentrant)], but re-entrantly invoked op op_get_extras_binding_object` then `panic in a function that cannot unwind`. Release does not check this (`[badpatch] OK "x"`) | [VERIFIED: spike run; deno_core-0.412.0/ops.rs:47-60] |
| S1c | Unbounded self-recursion (`me` → `op_rs3_patch('me')`), pre-Phase-5 | **No SIGSEGV** on 16 MiB *or* 2 MiB threads. It ends with a catchable chain once nested runtime bootstrap hits V8's stack guard (`runtime creation failed: Failed to execute ext:core/00_primordials.js`). Depth reached: 235 (debug), 778 (release). **Cost: about 800 MB peak RSS and 2.7 s in release** (about 1 MB per live nested isolate). The 422 message is huge (`Uncaught Error: ` × depth) | [VERIFIED: spike run, `/proc/<pid>/status` VmHWM = 808052 kB] |
| S1d | Nesting × deep JS recursion | 4/50/200 nested levels + `f(1e6)` in the leaf gives a catchable `RangeError: Maximum call stack size exceeded` on 2 MiB and 16 MiB stacks. No crash. `fac(100000)` → RangeError, no crash | [VERIFIED: spike run] |
| S2 | Test platform: `unsafe_use_unprotected_platform` as dev-dep feature | Lockfile unchanged (`cargo metadata --locked` exit 0, `diff` empty). Release fingerprint features `["default","reactor-tokio","v8","v8_use_custom_libcxx"]`; test-build fingerprint adds `"unsafe_use_unprotected_platform"`. 8 libtest threads each creating runtimes directly, plus 4 tests through a shared `LazyLock` pool: **12/12 pass** in parallel | [VERIFIED: spike run; deno_core-0.412.0/runtime/setup.rs:200-202] |
| S3 | `JsRuntime::new` + execute cost | **Debug:** first 15 ms, steady 10.5 ms/job, 19.4 ms with one nested patch. **Release:** first 41 ms (cold), steady 9.2 ms/job, 17.3 ms with one nested patch. Pool burst of 64 jobs on 4 workers: 714 ms debug, 614 ms release | [VERIFIED: spike run, n=50 debug / n=200 release] |
| S4 | Exact `#[op2]` / `extension!` syntax | Compiles under `#![forbid(unsafe_code)]` + `[lints.rust] unsafe_code="forbid"` + clippy `-D warnings` (lib). See Pattern 1 | [VERIFIED: spike build + clippy] |
| S5 | Reading the completion value | `deno_core::scope!(scope, &mut rt); v8::Local::new(scope, global).to_rust_string_lossy(scope)`: `6*20`→`"120"`, `let a=1;`→`"undefined"`, `({a:1})`→`"[object Object]"`, `Promise.resolve(1)`→`"[object Promise]"`, `null`→`"null"`, `'hi'`→`"hi"` | [VERIFIED: spike run] |
| S6 | JsErrorBox classes in bare deno_core | `generic` → `Error` (`instanceof Error` true, `name` "Error"); `type_error` → `TypeError`; **`new("NotFound", ..)` → throws `undefined`** (`typeof` "undefined", `instanceof Error` false, uncaught message `Uncaught undefined`) | [VERIFIED: spike run; deno_core-0.412.0/00_infra.js:94-131] |
| S7 | `JsError` → 422 message | Use `JsError.exception_message`: `Uncaught Error: boom`, `Uncaught SyntaxError: Unexpected token ';'`, `Uncaught ReferenceError: nope is not defined`, `Uncaught plain` (string throw; `name`/`message`/`stack` are `None`). `stack` holds the multi-line trace for stderr | [VERIFIED: spike run; deno_core-0.412.0/error.rs:495-512] |
| S8 | Pool primitives, panic survival | tokio only: `mpsc::channel(cap)` + `Arc<tokio::sync::Mutex<Receiver>>` + `oneshot`. A Rust panic in a job (with a live `JsRuntime` on the stack, no V8 frames) is caught by `catch_unwind(AssertUnwindSafe(..))`. The worker continues: `[pool-panic] OTHER-ERR internal: js worker panicked`, then `[pool-after-panic] OK "42"`. A panic **inside an op** (V8 frames on the stack) cannot be caught; it aborts | [VERIFIED: spike run] |
| S9 | Built-in op attack surface | `Deno.core.ops` has 106 ops in bare deno_core (core + ours). `op_panic` aborts the process, **so disable it**. Probed and safe (throw or no-op): `op_import_sync`, `op_import_sync_with_source`, `op_current_user_call_site`, the wasm streaming ops, `op_lazy_load_esm`, `op_load_ext_script`, `op_eval_context`, `op_serialize`/`op_deserialize`/`op_structured_clone`, `op_dispatch_exception`, `op_read_sync`/`op_write_sync` (`BadResource`, empty resource table), timers. `fetch`, `require`, `setTimeout` are undefined. `Deno.core.print` writes to server stdout | [VERIFIED: spike run; deno_core-0.412.0/ops_builtin.rs:149-156] |
| S10 | Non-ASCII script names | `execute_script("ключ/🔑".to_owned(), ..)` works. The doc says names should be 7-bit ASCII, but owned `String` names are accepted | [VERIFIED: spike run] |
| S11 | `op_panic` fix | `extension!(.., middleware = \|op\| match op.name { "op_panic" => op.disable(), _ => op }, ..)` makes the call `Uncaught Error: op is disabled`, catchable, exit 0. Samples and nested patch still work | [VERIFIED: spike run; deno_core-0.412.0/extensions.rs:297-309, 472; extension_set.rs init_ops applies middleware to deno_core's own ops] |
| S12 | Op string-argument coercion | `#[string]` args coerce: `op_rs3_get()` looks up key `"undefined"`, `op_rs3_get(5)` looks up `"5"`. No TypeError | [VERIFIED: spike run] |

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| PATCH routing, 404-before-queue, status mapping (`ApiError`) | API / Backend (`src/http`) | — | HTTP owns transport and status codes. It never touches V8 |
| Job queueing, worker threads, reply channel | API / Backend (`src/js/executor.rs`) | — | Store-agnostic engine; owned data crosses the boundary in both directions |
| Runtime creation, script execution, result stringification | API / Backend (`src/js/runtime.rs`, on `rs3-js-*` threads) | — | `JsRuntime` is `!Send` and must live and die on one worker |
| Store ops, op error mapping, `op_panic` disable | API / Backend (`src/js/ops.rs`) | Database / Storage (`src/store`) | Ops translate `StoreError` → `JsErrorBox`. The store stays free of `deno_*` |
| Wire format shared by `/list` and `op_rs3_list`, `/insert` and `op_rs3_insert` | Database / Storage (`src/store/legacy.rs`) | — | One codec guarantees TEST-06 byte equality |
| V8 platform init | Process (`src/main.rs` → `js::init_platform`) | Test support (`LazyLock`) | Must run once, before the JS threads exist |

## Standard Stack

### Core

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `deno_core` | 0.412.0 (already a dependency) | `#[op2]`, `extension!`, `JsRuntime`, `deno_core::scope!`, `deno_core::v8` | Required by the project. All APIs used here were compiled this session [VERIFIED: spike build] |
| `deno_error` | `"0.7"` → 0.7.1 (already a dependency) | `JsErrorBox::{generic, type_error}` for fallible ops | deno_core pins `=0.7.1`. Only built-in class names map to JS classes [VERIFIED: deno_error-0.7.1/src/lib.rs:168-179, 689-723] |
| `tokio` | **1.54.0** (new *direct* dependency; already in `Cargo.lock` via deno_core with feature `full`) | `runtime::Builder::new_current_thread` on each JS worker; `sync::{mpsc, oneshot, Mutex}` for queue and replies | Zero new crates. The lockfile change is one line [VERIFIED: lockfile diff in spike]. Features: `["rt", "sync"]` (`enable_all` is not feature-gated [VERIFIED: tokio-1.54.0/src/runtime/builder.rs:398]; the time driver deno_core needs is compiled in via deno_core's `full`) |

### Supporting

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `deno_core` (dev-dependency, same version) | `{ version = "0.412.0", features = ["unsafe_use_unprotected_platform"] }` | Unprotected V8 platform in test builds only | D-07. Test, clippy `--all-targets` and doc builds only [VERIFIED: fingerprints] |
| `std::sync::LazyLock`, `std::panic::catch_unwind`, `std::thread::Builder` | std | Test executor singleton, job panic isolation, named threads with 16 MiB stack | Always. No crate needed |

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| tokio `mpsc` + `Arc<tokio::sync::Mutex<Receiver>>` | `async-channel` (true MPMC) | Not in `Cargo.lock`. It would add `async-channel`, `concurrent-queue`, `event-listener*` and more. The shared-receiver pattern is proven in the spike |
| tokio `mpsc` | `std::sync::mpsc::sync_channel` | Its `send` **blocks the actix thread** when the queue is full. D-06 needs an async wait |
| Per-job boxed closure or enum `Job` | — | Recommend a boxed closure (`Box<dyn FnOnce() -> Result<String, ExecError> + Send>`), so tests can submit a panicking job or read `thread::current().name()` without test-only variants in production code |
| `web::block` stepping stone | — | Skip it. The pool is proven, and `web::block`'s 2 MiB unbounded threads are the anti-pattern D-05 rules out |

**Installation (Cargo.toml edits; do not use `cargo add`):**
```toml
[dependencies]
tokio = { version = "1.54.0", features = ["rt", "sync"] }

[dev-dependencies]
deno_core = { version = "0.412.0", features = ["unsafe_use_unprotected_platform"] }
```
Then run `cargo build` once, without `--locked`, to record `"tokio",` in rs3's lock entry, and commit `Cargo.lock` in the same commit. After that, `cargo build --locked` passes.

**Version verification:** tokio 1.54.0 is the version in `Cargo.lock` [VERIFIED: Cargo.lock line 1880-1881 `name = "tokio"` / `version = "1.54.0"`]. No other new package.

## Package Legitimacy Audit

| Package | Registry | Age | Downloads | Source Repo | Verdict | Disposition |
|---------|----------|-----|-----------|-------------|---------|-------------|
| tokio | crates.io | first published 2016-07-01 | about 20.7M/week | github.com/tokio-rs/tokio | OK | Approved. Already a transitive dependency at the same version |

**Packages removed due to [SLOP] verdict:** none
**Packages flagged as suspicious [SUS]:** none
`async-channel` was considered and is **not recommended** (not needed), so it was not installed or audited.

## Architecture Patterns

### System Architecture Diagram

```
 client ──PATCH /key/K──► actix worker (async; never runs V8)
                              │
                              ├─ store.get(K) ── missing ──► 404 not_found
                              │
                              ├─ js.execute(ScriptJob{name:K, source, store}).await
                              │      │ tokio::mpsc bounded send (waits when full)
                              │      ▼
                              │  ┌──────── shared queue ───────────┐
                              │  └──┬───────────┬───────────────┬──┘   Arc<tokio::Mutex<Receiver>>
                              │     ▼           ▼               ▼
                              │  rs3-js-0    rs3-js-1   ...  rs3-js-N-1     (16 MiB stack, tokio current_thread entered)
                              │     │ catch_unwind {
                              │     │   JsRuntime::try_new(extensions=[rs3_ext::init(ExecCtx{store,depth:0})])
                              │     │      middleware: op_panic → disabled
                              │     │   execute_script(K, source)
                              │     │      ├─ op_rs3_get/list/insert/delete ──► Store (papaya, owned values)
                              │     │      └─ op_rs3_patch(K2) [reentrant] ──► nested JsRuntime (depth+1) ──► String, dropped
                              │     │   scope! → Local::new → to_rust_string_lossy ──► String
                              │     │ }  runtime dropped on the worker
                              │     └── oneshot reply: Ok(String) | Err(ExecError)
                              ▼
          Ok(s) ──► 200 json(s)      ExecError::Script ──► 422 js_error     Unavailable / Internal / panic ──► 500 internal
```

### Recommended Project Structure

```
src/
├── main.rs            # Settings::load → js::init_platform → JsExecutor::start → AppState::new(settings, js) → HttpServer
├── state.rs           # AppState { store, settings, js: JsExecutor }; test_state() uses js::test_executor()
├── js/
│   ├── mod.rs         # init_platform(), pub use; ExecError; JsSettings; #[cfg(test)] test_executor()
│   ├── ops.rs         # ExecCtx, five #[op2] ops, to_js(StoreError), extension!(rs3_ext, .., middleware, state)
│   ├── runtime.rs     # run_script(ctx, name, source) -> Result<String, ExecError>
│   └── executor.rs    # JsExecutor (Clone + Debug), worker loop, catch_unwind
├── http/
│   ├── mod.rs         # patch_key now awaits state.js.execute(..)
│   └── error.rs       # ApiError gains JsError(String) → 422 "js_error"; Internal → 500 is now constructed
docs/
└── js-ops.md          # JSRT-06 mapping (new)
```
Dependency direction stays `http → js → store`. `js` must not import `actix_web`.

### Pattern 1: Ops + extension with per-job state and the `op_panic` guard
**What:** Each op borrows `ExecCtx` from `OpState`, calls the owned-value `Store`, and maps `StoreError` to a JS-catchable `JsErrorBox`. The extension's `options`/`state` hook injects the per-job `ExecCtx`. Its `middleware` disables `op_panic`; middleware also applies to deno_core's built-in op table.
**When to use:** For every op in this phase.
**Example:** see Code Examples → "ops.rs". It compiled and ran in the spike.

### Pattern 2: `run_script`, one fresh runtime per call
**What:** `JsRuntime::try_new` (never `new`, which panics) → `execute_script(name.to_owned(), source)` → map `Box<JsError>` to `ExecError::Script { message: exception_message }` (log `stack` to stderr) → `deno_core::scope!` → `Local::new(scope, global).to_rust_string_lossy(scope)`. The worker and the nested `op_rs3_patch` both use it.
**When to use:** For every script execution, top-level and nested.

### Pattern 3: Tokio-only bounded pool with panic isolation
**What:** `JsExecutor { tx: mpsc::Sender<Job> }` (Clone). `start(&JsSettings)` spawns `workers` threads named `rs3-js-{i}` with `stack_size`. Each builds a `current_thread` runtime (`enable_all`), loops `rt.block_on(async { rx.lock().await.recv().await })`, and for each job takes `let _enter = rt.enter();`, then `catch_unwind(AssertUnwindSafe(job))`, then `reply.send(..)`. When every worker exits, the last `Arc<Mutex<Receiver>>` drops. `send` then fails, which maps to 500, and pending oneshots fail, which also maps to 500. No request hangs forever.
**When to use:** Production and tests (one process-wide executor in tests).
**Startup handshake (recommended):** `start` returns `io::Result<JsExecutor>`. Report a thread-spawn error directly, and report a runtime-build failure over a `std::sync::mpsc` ready channel. `main` prints `error: cannot start JS runtime: ..` and exits 1 (ARCH-04 style).

### Pattern 4: Nested patch inside a reentrant op
**What:** `#[op2(reentrant)]` `op_rs3_patch` clones `ExecCtx` out of `OpState`, fetches the source (missing → not-found error), and calls `run_script(ExecCtx { depth: depth + 1, .. }, &key, source)`. The nested runtime is created and dropped inside the op body (LIFO). Map nested `ExecError::Script { message }` to `JsErrorBox::generic(message without the leading "Uncaught ")`, so a top-level failure reads `Uncaught Error: RangeError: inner boom` rather than `Uncaught Error: Uncaught RangeError: inner boom`. This is a discretion item; document it.

### Anti-Patterns to Avoid
- **`JsErrorBox::new("NotFound", ..)` (or any unregistered class):** throws `undefined` in bare deno_core [VERIFIED: S6]. Use `generic`/`type_error`/`range_error`.
- **`JsRuntime::new`:** it panics on bootstrap failure (`Failed to initialize a JsRuntime`) [VERIFIED: deno_core-0.412.0/runtime/jsruntime.rs:739-750]. Deep nesting makes bootstrap fail for real (S1c). Use `try_new` and map the error.
- **Non-reentrant `op_rs3_patch`:** debug abort (S1b).
- **`unwrap`/`expect` in a `LazyLock` initialiser or helper outside `#[cfg(test)]`:** clippy's `allow-expect-in-tests` only covers `#[test]` fns and `#[cfg(test)]` items. The spike's `tests/*.rs` helpers failed `-D clippy::expect-used` [VERIFIED: spike clippy]. Keep the test executor inside a `#[cfg(test)]` item in `src/`.
- **Creating V8 objects or runtimes on actix threads, or holding a `JsRuntime`/`v8::Global` across `.await`:** only `String`/`ExecError` cross threads.
- **Reusing a runtime between jobs:** top-level `let` collisions. Two fresh runtimes running `let q = ...` both succeed (S1).
- **Replacing the platform with `v8::new_unprotected_default_platform()`** (D-07's documented fallback): it drops deno_core's private `DenoPlatformImpl`, which routes foreground and delayed tasks [VERIFIED: setup.rs:116-212]. The better Phase 4 fallback is the same `unsafe_use_unprotected_platform` feature in `[dependencies]`, which keeps `DenoPlatformImpl` (see Open Questions).

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| MPMC job queue | custom condvar queue | `tokio::sync::mpsc` + `Arc<tokio::sync::Mutex<Receiver>>` | Async bounded send (D-06), runtime-agnostic, proven in spike |
| Reply channel | `Arc<Mutex<Option<..>>>` polling | `tokio::sync::oneshot` | Dropping it on any failure turns into a clean `RecvError` → 500 |
| Blocking dangerous built-in ops | JS prelude `delete Deno.core.ops.op_panic` | `extension!` `middleware` + `OpDecl::disable()` | A JS delete only removes the property. Middleware swaps the native function before bootstrap, so wrappers cannot reach it either |
| Result stringification | a new `v8::Context` + `HandleScope` (legacy) | `deno_core::scope!` + `to_rust_string_lossy` | The legacy pattern no longer compiles (pinned scopes) and read results in the wrong realm |
| List/insert wire format inside ops | hand-built strings | `store::legacy::{format_list, parse_insert_body, format_insert_echo}` | TEST-06 byte equality, escaping already solved in Phase 2 |
| Error class mapping | custom JS error registry | `JsErrorBox::generic` / `type_error` | Built-in classes are pre-registered in `00_infra.js`. Custom ones would need an ESM file in the extension |

**Key insight:** In deno_core every Rust panic reachable from JS is a process abort (extern "C" op boundary). The only defences are "no panics in our ops" (types plus clippy) and "no panicking built-in ops reachable" (middleware).

## Runtime State Inventory

Not a rename/refactor phase in the data sense, but the op rename (`rs3_*` → `op_rs3_*`) has one runtime-state aspect:

| Category | Items Found | Action Required |
|----------|-------------|------------------|
| Stored data | The store is in-memory and starts empty on every boot, so no persisted scripts exist. Any script a user stored in a running legacy server is lost on restart anyway | None. Document the rename in `docs/js-ops.md` (users must re-insert migrated scripts) |
| Live service config | None. No external services. `docker_rs3/` playbooks are out of scope and contain no JS | None |
| OS-registered state | None | None |
| Secrets/env vars | None. JS settings are in code (D-06) | None |
| Build artifacts | `target/debug` gains a second `deno_core` build (with the feature) on the first `cargo test`. That is expected; disk has 48 GB free on `/home` | None |

## Common Pitfalls

### Pitfall 1: `Deno.core.ops.op_panic` aborts the whole server
**What goes wrong:** A stored script `Deno.core.ops.op_panic('x')` gives `JS PANIC: x` and then the process exits with SIGABRT (134), in debug and release.
**Why it happens:** `op_panic` is a built-in op (`panic!("JS PANIC: {}", message)`), and a panic inside an op cannot unwind through V8.
**How to avoid:** Add `middleware = |op| match op.name { "op_panic" => op.disable(), _ => op }` in `rs3_ext`, plus a regression test: PATCH of that script → 422 `js_error` with message `Uncaught Error: op is disabled`, then a later request still succeeds.
**Warning signs:** The test binary dies with `panic in a function that cannot unwind`.

### Pitfall 2: Unregistered JsErrorBox class throws `undefined`
**What goes wrong:** `JsErrorBox::new("NotFound", "key not found: x")` → JS sees `undefined`; the 422 message becomes `Uncaught undefined`.
**How to avoid:** missing key → `JsErrorBox::generic(StoreError::NotFound(..).to_string())` (JS `Error`, message `key not found: <key>`). Invalid insert → `JsErrorBox::type_error(e.to_string())`. Pin both with `instanceof`/`name` assertions in a JS test.

### Pitfall 3: Forgetting `reentrant` (debug-only abort)
`cargo test` (debug) aborts and `--release` "works". Test nested patch in the default `cargo test` profile.

### Pitfall 4: `JsRuntime::new` panics
Bootstrap can fail (deep nesting reproduces it). `new` panics, and inside an op that is an abort. Always use `try_new` → `ExecError::Internal` (top level → 500) or `JsErrorBox` (nested → script error).

### Pitfall 5: Tokio context ordering on the worker
Create every `JsRuntime` while `rt.enter()` is held (deno_core registers `Handle::try_current()` and aborts on a delayed task without one [VERIFIED: setup.rs:88-110]). Take the guard *after* `rt.block_on(recv)` returns, and drop it before the next `block_on`.

### Pitfall 6: Clippy test allowance scope
`expect()` in a `LazyLock` or helper outside `#[cfg(test)]` fails `cargo clippy --all-targets -- -D warnings`. Put `test_executor()` in `#[cfg(test)]` (like the existing `test_state()`), or return `Result` and handle it.

### Pitfall 7: The doctest summary shows "1 ignored"
`extension!` expands to a doc comment with a ```` ```rust,ignore ```` example [VERIFIED: extensions.rs:486]. `cargo test` will report `1 ignored` under **Doc-tests**. D-12's "#[ignore] count drops to 0" must be checked with `grep -rn '#\[ignore' src` (expect 0) and the **lib** test summary (`0 ignored`), not the doctest line.

### Pitfall 8: Nested error message growth and memory before Phase 5
Self-recursion is not a crash (S1c), but it is a 422 with a message of about 12 KB or more, about 800 MB of RSS, and 2.7 s of one worker (release). This is a DoS vector until ROBU-05. Record it as a known Phase 5 item (it corrects the D-04 expectation "still overflows the stack"), and do **not** add a self-recursion test or smoke in Phase 3.

### Pitfall 9: String arguments coerce
`op_rs3_get()` reads key `"undefined"` and `op_rs3_get(5)` reads `"5"`. Document this in `docs/js-ops.md`. It is consistent with legacy, where any value was stringified.

### Pitfall 10: Tests don't exercise the production (protected) platform
With the dev feature, `cargo test` runs the *unprotected* platform. Only the binary (`cargo build`, no dev-deps) uses the protected platform. The live-binary PATCH smokes are the Phase 3 evidence for the production path, and x86 CI (Phase 4) is the PKU evidence.

### Pitfall 11: Script names and the "7-bit ASCII" doc
Non-ASCII key names worked (S10). Pass `key.clone()` (owned `String`); never `ascii_str!`/`from_ascii_static_unchecked` on user data.

### Pitfall 12: Microtasks/Promises don't run
`execute_script` doesn't pump the event loop (`MicrotasksPolicy::Explicit`). `Promise.resolve(1)` returns `"[object Promise]"` and `.then` callbacks never run. Document this (APIV-02 is v2).

## Code Examples

These are verified patterns: compiled, clippy-clean and run in the spike against deno_core 0.412.0. Names follow the recommended layout.

### Verified in-repo anchors (quoted verbatim)
- `StoreError` [VERIFIED: src/store/mod.rs:12-19]: `#[error("key not found: {0}")] NotFound(String)`, `#[error("malformed JSON: {0}")] MalformedJson(String)`, `#[error("invalid input: {0}")] InvalidInput(String)`
- `Store` methods [VERIFIED: src/store/mod.rs:36,45,55,66,75 via grep + read]: `pub fn get(&self, key: &str) -> Result<String, StoreError>`, `pub fn insert(&self, key: String, value: String) -> Option<String>`, `pub fn insert_all<I: IntoIterator<Item = (String, String)>>(&self, entries: I) -> usize`, `pub fn remove(&self, key: &str) -> Result<String, StoreError>`, `pub fn list(&self) -> Vec<(String, String)>`
- Codec [VERIFIED: src/store/legacy.rs:34,132,148]: `pub fn parse_insert_body(body: &str) -> Result<Vec<(String, String)>, StoreError>`, `pub fn format_insert_echo(entries: &[(String, String)]) -> String`, `pub fn format_list(entries: &[(String, String)]) -> String`
- `ApiError` [VERIFIED: src/http/error.rs:11-18, 30-36, 42-48]: variants `Store(#[from] StoreError)`, `NotImplemented(String)`, `#[error("internal error")] Internal`; codes `"not_found"`, `"malformed_json"`, `"invalid_input"`, `"not_implemented"`, `"internal"`; body `ErrorBody { error: self.code(), message: self.to_string() }`
- `AppState` [VERIFIED: src/state.rs:13-16, 20, 31]: `pub store: Store`, `pub settings: Arc<Settings>`, `pub fn new(settings: Settings) -> Self`, `#[cfg(test)] pub(crate) fn test_state() -> AppState`
- `init_platform` [VERIFIED: src/js/mod.rs:5-7]: `pub fn init_platform() { JsRuntime::init_platform(None); }`
- main order today [VERIFIED: src/main.rs:8-21]: `Settings::load()` → `rs3::js::init_platform();` → `rs3::state::AppState::new(settings)` → `rs3::http::configure(state)` → `HttpServer::new(..)`
- deno_core: `extension!` accepts `options = { .. }`, `middleware = $middleware_fn`, `state = $state_fn` in that order [VERIFIED: extensions.rs:471-473]. `pub fn disable(self) -> Self` on `OpDecl` [VERIFIED: extensions.rs:299]. Unprotected platform when `cfg!(any(test, feature = "unsafe_use_unprotected_platform"))` [VERIFIED: setup.rs:201-202]. `JsError` fields `name`, `message`, `stack`, `exception_message: String` [VERIFIED: error.rs:495-500]

### ops.rs
```rust
// Source: spike (deno_core 0.412.0), adapted to the rs3 Store/codec signatures quoted above
use deno_core::{OpState, op2};
use deno_error::JsErrorBox;

use crate::store::{Store, StoreError, legacy};

#[derive(Clone, Debug)]
pub struct ExecCtx {
    pub store: Store,
    pub depth: u32,
}

fn to_js(e: StoreError) -> JsErrorBox {
    match e {
        StoreError::NotFound(_) => JsErrorBox::generic(e.to_string()), // "key not found: <key>"
        StoreError::MalformedJson(_) | StoreError::InvalidInput(_) => JsErrorBox::type_error(e.to_string()),
    }
}

#[op2]
#[string]
fn op_rs3_list(state: &mut OpState) -> String {
    legacy::format_list(&state.borrow::<ExecCtx>().store.list())
}

#[op2]
#[string]
fn op_rs3_get(state: &mut OpState, #[string] key: String) -> Result<String, JsErrorBox> {
    state.borrow::<ExecCtx>().store.get(&key).map_err(to_js)
}

#[op2]
#[string]
fn op_rs3_insert(state: &mut OpState, #[string] body: String) -> Result<String, JsErrorBox> {
    let entries = legacy::parse_insert_body(&body).map_err(to_js)?;
    let echo = legacy::format_insert_echo(&entries);
    state.borrow::<ExecCtx>().store.insert_all(entries);
    Ok(echo)
}

#[op2]
#[string]
fn op_rs3_delete(state: &mut OpState, #[string] key: String) -> Result<String, JsErrorBox> {
    state.borrow::<ExecCtx>().store.remove(&key).map_err(to_js)
}

#[op2(reentrant)] // mandatory: a nested runtime calls ops while this op is on the stack
#[string]
fn op_rs3_patch(state: &mut OpState, #[string] key: String) -> Result<String, JsErrorBox> {
    let ctx = state.borrow::<ExecCtx>().clone();
    let source = ctx.store.get(&key).map_err(to_js)?;
    let nested = ExecCtx { store: ctx.store, depth: ctx.depth + 1 };
    super::runtime::run_script(nested, &key, source).map_err(|e| match e {
        super::ExecError::Script { message } => {
            JsErrorBox::generic(message.strip_prefix("Uncaught ").unwrap_or(&message).to_owned())
        }
        other => JsErrorBox::generic(other.to_string()),
    })
}

deno_core::extension!(
    rs3_ext,
    ops = [op_rs3_list, op_rs3_get, op_rs3_insert, op_rs3_delete, op_rs3_patch],
    options = { ctx: ExecCtx },
    // op_panic is a deno_core built-in that panics -> process abort. Disable it.
    middleware = |op| match op.name {
        "op_panic" => op.disable(),
        _ => op,
    },
    state = |state, options| {
        state.put(options.ctx);
    },
);
```
(The spike's list op used an equivalent inline formatter; `format_list`'s signature is quoted above. `ExecError::Script { message }` assumes the recommended `ExecError` shape below.)

### runtime.rs
```rust
// Source: spike (deno_core 0.412.0); pattern from deno_core examples/eval_js_value.rs
use deno_core::{JsRuntime, RuntimeOptions, v8};

use super::{ExecError, ops::{ExecCtx, rs3_ext}};

pub fn run_script(ctx: ExecCtx, name: &str, source: String) -> Result<String, ExecError> {
    let mut rt = JsRuntime::try_new(RuntimeOptions {
        extensions: vec![rs3_ext::init(ctx)],
        ..Default::default()
    })
    .map_err(|e| ExecError::Internal(format!("cannot create JS runtime: {e}")))?;
    let global = rt.execute_script(name.to_owned(), source).map_err(|e| {
        eprintln!("js error in {name}: {e}"); // Display = stack when multi-line
        ExecError::Script { message: e.exception_message.clone() }
    })?;
    deno_core::scope!(scope, &mut rt);
    let local = v8::Local::new(scope, global);
    Ok(local.to_rust_string_lossy(scope))
}
```

### executor.rs (worker loop core, as run in the spike)
```rust
// Source: spike; tokio 1.54 sync primitives + std threads
fn worker(rx: Arc<tokio::sync::Mutex<tokio::sync::mpsc::Receiver<Job>>>, rt: tokio::runtime::Runtime) {
    loop {
        let Some(job) = rt.block_on(async { rx.lock().await.recv().await }) else { return };
        let _enter = rt.enter(); // tokio context for JsRuntime::try_new (delayed V8 tasks)
        let Job { run, reply } = job; // run: Box<dyn FnOnce() -> Result<String, ExecError> + Send>
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(run))
            .unwrap_or_else(|_| Err(ExecError::Internal("js worker panicked".into())));
        let _ = reply.send(result); // receiver gone (client left) is fine
    }
}
// spawn: std::thread::Builder::new().name(format!("rs3-js-{i}")).stack_size(settings.stack_bytes).spawn(..)
// execute: self.tx.send(job).await.map_err(|_| ExecError::Unavailable)?; reply_rx.await.map_err(|_| ExecError::Unavailable)?
```

### Recommended error types
```rust
// src/js/mod.rs
#[derive(Debug, thiserror::Error)]
pub enum ExecError {
    #[error("{message}")]
    Script { message: String },          // -> 422 js_error (message = JsError.exception_message)
    #[error("JS executor unavailable")]
    Unavailable,                         // channel closed / reply dropped -> 500
    #[error("JS host failure: {0}")]
    Internal(String),                    // try_new failed, worker panicked -> 500
}
// src/http/error.rs: add `#[error("{0}")] JsError(String)` -> code "js_error", 422.
// impl From<ExecError> for ApiError: Script{message} -> JsError(message);
//   Unavailable | Internal(_) -> eprintln!(detail) then ApiError::Internal (body message stays "internal error").
// Keep the code()/status_code() matches exhaustive (no wildcard). Remove NotImplemented (dead after this phase)
// together with test_patch_existing_is_501.
```

### Test executor (inside `#[cfg(test)]`, so `expect` is allowed)
```rust
#[cfg(test)]
pub(crate) fn test_executor() -> JsExecutor {
    static EXEC: std::sync::LazyLock<JsExecutor> = std::sync::LazyLock::new(|| {
        init_platform(); // unprotected in test builds via the dev-dependency feature
        JsExecutor::start(&JsSettings::default()).expect("test JS executor")
    });
    EXEC.clone()
}
```

### PATCH handler
```rust
#[patch("/key/{key}")]
pub async fn patch_key(state: web::Data<AppState>, path: web::Path<String>) -> Result<HttpResponse, ApiError> {
    let key = path.into_inner();
    let source = state.store.get(&key)?;            // 404 before queueing
    let out = state.js.execute(state.store.clone(), key, source).await?; // From<ExecError>
    Ok(HttpResponse::Ok().json(out))
}
```

### docs/js-ops.md mapping (content verified against spike behaviour)

| Old call | New call | Argument | Returns | Throws when |
|----------|----------|----------|---------|-------------|
| `Deno.core.ops();` | *(delete the line)* | — | — | calling it now throws `TypeError: Deno.core.ops is not a function` |
| `Deno.core.opSync("rs3_list", "")` | `Deno.core.ops.op_rs3_list()` | none (extra args ignored) | `/list` inner text `[{"k" : "v"},...]` | never |
| `Deno.core.opSync("rs3_get", key)` | `Deno.core.ops.op_rs3_get(key)` | key (coerced to string) | value | missing key: `Error: key not found: <key>` (legacy: server panic) |
| `Deno.core.opSync("rs3_insert", json)` | `Deno.core.ops.op_rs3_insert(json)` | JSON object of strings, as a string | compact echo `{"k":"v"}` | malformed JSON: `TypeError: malformed JSON: ...`; wrong shape: `TypeError: invalid input: ...`; nothing written on error |
| `Deno.core.opSync("rs3_delete", key)` | `Deno.core.ops.op_rs3_delete(key)` | key | old value | missing key: `Error: key not found: <key>` |
| `Deno.core.opSync("rs3_patch", key)` | `Deno.core.ops.op_rs3_patch(key)` | key | `String(completion value)` of that key's script, run in a fresh runtime | missing key; any uncaught error in the nested script (`Error: <nested message>`) |

Also document: PATCH stringification (`120`→`"120"`, `undefined`, `[object Object]`, `[object Promise]`), 422 `{"error":"js_error","message":"Uncaught ..."}`, `Deno.core.print` still works, `op_panic` is disabled, and nesting has no limit until Phase 5.

### JS artefact inventory (D-13), current state [VERIFIED: files read this session]
- `requests/fac.js`, `sum.js`, `bin.js`: `Deno.core.ops();` + `Deno.core.opSync("rs3_get", "<key>")`. Replace with `Deno.core.ops.op_rs3_get("<key>")` and delete the `Deno.core.ops();` line. With that change the spike produced `120` / `10` / `{"value":5,"left":{"value":3},"right":{"value":8}}`.
- `requests/store.js`: `Deno.core.opSync("rs3_insert", "{\"res\":\"" + p + "\"}")` → `Deno.core.ops.op_rs3_insert(...)`.
- `requests/{fac,sum,bin,store}.txt`: the same code JSON-escaped inside `/insert` bodies (`{"fac": "..."}`). Edit while keeping the JSON valid. Post-edit check: `python3 -c 'import json,sys;json.load(open(sys.argv[1]))' requests/fac.txt`.
- `requests/rs3_http_localhost_5000.http:13`, `requests/rs3_https_rs3_cwe.http:6`: `"code": "Deno.core.ops();Deno.core.opSync(\"rs3_list\",\"\").toString();"` → `"Deno.core.ops.op_rs3_list().toString();"`.
- `src/example.js`: `Deno.core.ops();` + `Deno.core.opSync('op_list', '')` → `Deno.core.ops.op_rs3_list()`. `Deno.core.print` exists (`typeof` → `function`).
- `src/http/mod.rs:184` test script `LIST_SCRIPT` already uses the new syntax.

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `register_op` + `op_sync` + `sync_ops_cache` | `#[op2]` + `extension!` + `rs3_ext::init(..)` | deno_core 0.2xx–0.3xx | Op name = Rust fn name |
| `Deno.core.opSync("name", arg)` / `Deno.core.ops()` | `Deno.core.ops.op_name(arg)` (plain object) | — | All artefacts break until migrated |
| `HandleScope::new` + `Context::new` | `deno_core::scope!` (pinned scopes) | v8 ≥ about 140 | Reads the result in the right realm |
| `rusty_v8` crate | `deno_core::v8` (deno_v8 facade over v8 150.4) | — | No direct v8 dependency |
| `anyhow` errors from ops | `Result<T, deno_error::JsErrorBox>` | deno_core 0.3xx | Only built-in classes map to JS classes in bare deno_core |

**Deprecated/outdated:** `web::block` for JS (unbounded, 2 MiB stacks); `JsRuntime::new` in fallible paths; the `denoland/deno_core` repo (archived; crate now published from `denoland/deno` `libs/core`).

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | Behaviour on x86_64 with PKU (protected platform in the binary, unprotected in tests) matches aarch64 | Pitfall 10, D-07 | Segfaults in Phase 4 CI. The fallback is ready (Open Question 1) |
| A2 | The nesting/stack results (no SIGSEGV on 2 MiB, about 1 MB RSS per nested isolate) hold on x86_64 | Spike S1c/S1d | A self-recursive PATCH could crash rather than 422 before Phase 5. 16 MiB stacks remain the mitigation |
| A3 | No other built-in op panics on adversarial input beyond `op_panic` (about 25 probed, not all 101) | Pitfall 1, Security | An unprobed op could still abort the server. Mitigation: grow the middleware denylist; full audit deferred |
| A4 | Stripping the leading `"Uncaught "` from nested error messages is acceptable UX | Pattern 4 | Cosmetic only (discretion item) |
| A5 | Removing `ApiError::NotImplemented` is acceptable (nothing constructs it after this phase) | Code Examples | None functionally. Keeping it is also fine |

## Open Questions

1. **D-07 fallback choice for Phase 4.**
   - What we know: `new_unprotected_default_platform()` would replace deno_core's private `DenoPlatformImpl` (foreground/delayed task routing) [VERIFIED: setup.rs:116-212]. The `unsafe_use_unprotected_platform` feature keeps it and only flips the `unprotected` flag.
   - Recommendation: if x86 CI crashes, move the feature from `[dev-dependencies]` to `[dependencies]`, not `new_unprotected_default_platform`. No Phase 3 action; record it for Phase 4.
2. **How much of the built-in op surface to deny.**
   - Recommendation: Phase 3 disables `op_panic` only (the one proven abort) and adds its regression test. A broader allowlist risks breaking bootstrap (core JS calls many ops). Revisit with JSHD-* hardening.
3. **Nested error message format** (discretion): recommended strip-`Uncaught` mapping. The planner may choose to prefix the key (`op_rs3_patch <key>: ...`).

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| Rust toolchain | build | ✓ | rustc 1.99.1, cargo 1.99.1 | — |
| V8 prebuilt static lib | deno_core build | ✓ (already compiled in `target/debug`) | v8 150.4.0 | — |
| tokio 1.54.0 sources | new direct dependency | ✓ (`~/.cargo/registry`, in `Cargo.lock`) | 1.54.0 | — |
| Disk | debug build plus a second deno_core | ✓ | `/home` 48 GB free; `target/` 2.9 GB | — |
| curl | live smokes | ✓ (used in Phase 2) | — | — |
| `/usr/bin/time` | RSS measurement | ✗ | — | `/proc/<pid>/status` VmHWM (used) |
| x86_64 host | PKU verification | ✗ | — | Phase 4 CI (CI-03) |

**Missing dependencies with no fallback:** none for Phase 3.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust libtest + `#[actix_web::test]` (actix-web 4.17.0), unit tests in `src/**` (`#[cfg(test)]`) |
| Config file | `clippy.toml` (test allowances), `Cargo.toml` `[lints]`. New: `[dev-dependencies]` deno_core feature |
| Quick run command | `cargo test --lib js:: && cargo test --lib http::` |
| Full suite command | `cargo build --locked && cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check` |
| Phase gate extras | `env -i PATH="$PATH" HOME="$HOME" cargo test`; `cargo test -- --test-threads=1`; `for i in 1 2 3 4 5; do cargo test -q || exit 1; done` |
| Baseline | 97 lib tests (95 pass, 2 ignored) today [VERIFIED: cargo test run] |

### Phase Requirements → Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| JSRT-01/02 | five ops round-trip on an injected store; `op_rs3_list('')` ignores the arg | unit (run_script on the test executor) | `cargo test --lib js::` | ❌ Wave 0 (`src/js/*` tests) |
| JSRT-02 (patch) | nested `op_rs3_patch` sees depth+1, same store; inner `op_rs3_get` works in debug | unit | `cargo test --lib js::tests::nested_patch` | ❌ |
| JSRT-03 | missing key → `Error` `key not found: k`, `instanceof Error`; bad insert → `TypeError`; both catchable; store unchanged on insert error | unit | `cargo test --lib js::tests::op_errors` | ❌ |
| JSRT-03 / ROBU-04 | `Deno.core.ops.op_panic('x')` → `Uncaught Error: op is disabled`, process alive | unit + HTTP | `cargo test --lib op_panic` | ❌ |
| JSRT-04 | job runs on a thread named `rs3-js-*` (submit a closure returning `thread::current().name()`); worker survives a panicking job; next job OK | unit | `cargo test --lib js::executor` | ❌ |
| JSRT-04 (static) | in `main.rs`, `init_platform()` precedes `JsExecutor::start` precedes `HttpServer::new` | grep gate | `awk '/init_platform\(\)/{a=NR}/JsExecutor::start/{b=NR}/HttpServer::new/{c=NR}END{exit !(a&&b&&c&&a<b&&b<c)}' src/main.rs` | ✅ file exists |
| JSRT-04 (live) | binary has N `rs3-js-*` threads | live smoke | `cat /proc/$PID/task/*/comm \| grep -c '^rs3-js-'` equals `nproc` | live |
| HTTP-07 | PATCH existing → 200 `"120"` (`6*20` script) | HTTP | `cargo test --lib http::tests::test_patch` | partial (parked tests) |
| HTTP-07 | stringification `undefined`/`[object Object]`/`[object Promise]` | unit or HTTP | `cargo test --lib patch_stringifies` | ❌ |
| ROBU-03 | throw → 422 `js_error`, message `Uncaught Error: boom`; syntax error → 422 `Uncaught SyntaxError: ...`; uncaught op error → 422 `Uncaught Error: key not found: x`; later request 200 | HTTP | `cargo test --lib http::tests::test_patch_throw` | ❌ |
| D-11 / IN-01 | closed executor → 500 `{"error":"internal"}` | HTTP (`#[cfg(test)]` closed-executor constructor) | `cargo test --lib patch_executor_down_is_500` | ❌ |
| TEST-03 | PATCH success asserts status + bytes | HTTP | `cargo test --lib test_patch_is_ok` | ✅ (un-ignore) |
| TEST-05 | `fac.js`→`"120"`, `sum.js`→`"10"`, `bin.js`→`"{\"value\":5,...}"`; `store.js` then GET `/key/res` → `"1111"` | HTTP via `include_str!("../../requests/*.js")` | `cargo test --lib fixture_` | ❌ |
| TEST-06 | PATCH of `LIST_SCRIPT` == GET `/list` bytes | HTTP | `cargo test --lib test_patch_body_is_listing` | ✅ (un-ignore) |
| D-12 | no `#[ignore]` left | grep gate | `test "$(grep -rn '#\[ignore' src \| wc -l)" -eq 0` | ✅ |
| JSRT-05 | no old syntax in artefacts | grep gate | `! grep -rnE 'opSync\|Deno\.core\.ops\(\)' requests src && ! grep -rnE '(^\|[^_])rs3_(list\|get\|insert\|delete\|patch)' requests src/example.js` | ✅ |
| JSRT-05 | `.txt` payloads still valid JSON | static | `for f in requests/*.txt; do python3 -c 'import json,sys;json.load(open(sys.argv[1]))' "$f" \|\| exit 1; done` | ✅ |
| JSRT-06 | mapping doc present | static | `test -f docs/js-ops.md && grep -c 'op_rs3_' docs/js-ops.md` (≥ 5) `&& grep -q 'key not found' docs/js-ops.md` | ❌ |
| DEPS (D-09) | lock on released versions, single V8, tokio 1.54.0 | static | `cargo build --locked && cargo tree -d -e normal \| grep -c '^v8 '` (0 duplicates) `&& ! grep -q 'async-channel' Cargo.lock` | ✅ |
| D-07 | release build lacks the unprotected feature | static | `cargo tree -e features,normal -i deno_core \| grep -c unsafe_use_unprotected_platform` → 0 (checked today: 0). Authoritative: after `cargo build --release`, `target/release/.fingerprint/deno_core-*/lib-deno_core.json` lists no `unsafe_use_unprotected_platform` | ✅ |
| D-17 | IN-04 sentence fixed; IN-05 names gone | grep gate | `! grep -n 'first one in body' src/store/legacy.rs && ! grep -rn 'parse_rejects_deep_nesting\|insert_deeply_nested_is_400_json' src` | ✅ |

### Live-binary smokes (ports 18500–18519; env-var config only)
```bash
cargo build && B=target/debug/rs3
env 'SERVER.HOST=127.0.0.1' 'SERVER.PORT=18500' $B & PID=$!; sleep 1
curl -s -X POST localhost:18500/insert -d '{"num":"5","numbers":"1 2 3 4","tree":"5 3 8"}'
curl -s -X POST localhost:18500/insert --data-binary @requests/fac.txt   # migrated payload
curl -s -w ' %{http_code}\n' -X PATCH localhost:18500/key/fac            # "120" 200
curl -s -X POST localhost:18500/insert -d '{"t":"throw new Error(\"boom\")","p":"Deno.core.ops.op_panic(\"x\")"}'
curl -s -w ' %{http_code}\n' -X PATCH localhost:18500/key/t              # {"error":"js_error","message":"Uncaught Error: boom"} 422
curl -s -w ' %{http_code}\n' -X PATCH localhost:18500/key/p              # 422 "Uncaught Error: op is disabled"
curl -s -w ' %{http_code}\n' localhost:18500/                            # still 200 (process alive)
cat /proc/$PID/task/*/comm | grep -c '^rs3-js-'                          # == nproc
seq 1 32 | xargs -P 16 -I{} curl -s -o /dev/null -w '%{http_code}\n' -X PATCH localhost:18500/key/fac | sort | uniq -c   # 32 x 200
kill $PID
```
This binary uses the **protected** platform (no dev-deps), so it is the Phase 3 evidence for the production platform path. **Do not** smoke self-recursion (800 MB; Phase 5).

### Sampling Rate
- **Per task commit:** `cargo test --lib` + `cargo clippy --all-targets -- -D warnings`
- **Per wave merge:** full suite command + grep gates above
- **Phase gate:** full suite + `env -i` / `--test-threads=1` / 5× repeat + live smokes before `/gsd-verify-work`

### Wave 0 Gaps
- [ ] `src/js/{ops,runtime,executor}.rs` test modules (op semantics, errors, nested patch, op_panic, thread name, panic survival)
- [ ] `#[cfg(test)] js::test_executor()` and `test_state()` wired to it; a `#[cfg(test)]` closed-executor constructor for the 500 test
- [ ] Cargo.toml: `tokio` direct dependency + dev-dependency feature line; `Cargo.lock` updated (one line)
- [ ] TDD RED evidence per Phase 1/2 method (JUnit via `RUSTC_BOOTSTRAP=1 <test-bin> <name> --exact -Z unstable-options --format junit`) under `.planning/phases/03-javascript-runtime-port/tdd-evidence/`

### Suggested plan split (for the planner; no file overlap within a wave)
- **Wave 1, Plan 01 (JS core):** Task 1 is an in-repo RED test reproducing S1 (nested reentrant patch) and the op_panic abort guard, so a dead end surfaces first (CONTEXT specifics). Files: Cargo.toml, Cargo.lock, src/js/*.
- **Wave 1, Plan 02 (artefacts + docs + housekeeping):** requests/*, src/example.js, docs/js-ops.md, IN-04 (legacy.rs doc), IN-05 (test renames in legacy.rs and kv.rs). No code dependency on Plan 01.
- **Wave 2, Plan 03 (HTTP wiring):** state.rs, main.rs, http/mod.rs, http/error.rs: PATCH handler, ApiError `js_error`/`Internal`, un-ignore parked tests, fixture tests (`include_str!` the migrated scripts from Plan 02), TEST-06, live smokes, full phase gate.

## Security Domain

`security_enforcement` is enabled (ASVS L1, block on high).

### Applicable ASVS Categories

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | no | — (no auth in rs3) |
| V3 Session Management | no | — |
| V4 Access Control | no (single-tenant store) | — |
| V5 Input Validation | yes | Op inputs validated through `legacy::parse_insert_body`; string args coerced; JS source executed in the bare deno_core sandbox (no fs/net/timers) |
| V6 Cryptography | no | — |
| V7 Error Handling and Logging | yes | 422 body has only `exception_message`; stack goes to stderr; 500 body is the fixed `internal error` and detail is logged |
| V12/V14 (resources/config) | yes | Bounded pool and queue; 16 MiB stacks; JS settings in code |

### Known Threat Patterns for deno_core + actix

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| Script calls `Deno.core.ops.op_panic` → process abort | Denial of Service | `extension!` middleware `op.disable()` + regression test (**high, must fix in Phase 3**) |
| Op panics on bad input → abort | DoS | `Result<_, JsErrorBox>`, no `unwrap`/`expect` (clippy deny), `try_new` |
| Self-recursive `op_rs3_patch` → about 800 MB RSS per request, huge 422 body | DoS | Accepted residual until ROBU-05 (Phase 5 depth limit). Do not smoke |
| `while(true){}` pins a JS worker; N such requests saturate the pool (PATCH requests queue and wait; other routes unaffected because actix workers never run V8) | DoS | Accepted residual until ROBU-06 (Phase 5 timeout) |
| Stack trace / internals leaked in responses | Information Disclosure | Only `exception_message` returned; stack to stderr |
| `Deno.core.print` spams or injects into server stdout | Tampering (logs) | Accepted (legacy feature used by `src/example.js`). Note in docs |
| Unprobed built-in op with an internal `unwrap` | DoS | About 25 probed safe. Denylist extensible (A3) |
| Cross-test or cross-store leakage via a shared pool | Information Disclosure | Store travels per job in `ExecCtx` (D-03); fresh runtime per job |

## Sources

### Primary (HIGH confidence)
- Spike crate built and run this session (deno_core 0.412.0, deno_error 0.7.1, tokio 1.54.0, papaya 0.2.5; Rust 1.99.1; aarch64; debug + release; project `Cargo.lock` offline). Outputs quoted in Spike Results
- deno_core 0.412.0 source (`~/.cargo/registry`): `runtime/setup.rs` (88-110 delayed-task abort, 116-212 DenoPlatformImpl/unprotected flag), `runtime/jsruntime.rs` (702-737 `scope!`/`init_platform`, 739-750 `new` panics, 2018-2029 `execute_script`), `ops.rs:44-61` (reentrancy check), `extensions.rs` (297-309 `disable`, 456-477 `extension!` grammar, 486 `rust,ignore` doc), `extension_set.rs` (`init_ops` applies middleware to core ops), `error.rs` (395-450 custom error builder, 495-512 `JsError`), `00_infra.js:85-145` (registered classes), `ops_builtin.rs:149-156` (`op_panic`), `Cargo.toml` features
- deno_error 0.7.1 `src/lib.rs` (168-179 builtin classes, 689-723 constructors)
- deno_ops 0.288.0 `op2/README.md`, `valid_args.md`, `valid_retvals.md` (`#[string]`, `&mut OpState`, `reentrant`, fast rules)
- tokio 1.54.0 `src/runtime/builder.rs:398` (`enable_all` ungated)
- Project files read this session: `src/{main,state,lib}.rs`, `src/js/mod.rs`, `src/http/{mod,error,kv}.rs`, `src/store/{mod,legacy}.rs`, `Cargo.toml`, `clippy.toml`, `requests/*`, `src/example.js`, legacy `main:src/bindings/mod.rs` and `main:src/services/mod.rs` (git)
- `gsd-tools query package-legitimacy check --ecosystem crates tokio` → OK

### Secondary (MEDIUM confidence)
- `.planning/research/{ARCHITECTURE,PITFALLS,STACK,SUMMARY}.md` (milestone research). Corrected here on: stack-overflow expectation (S1c), `NotFound` class (S6), `op_panic` (S9), `async-channel` not needed (S8)

### Tertiary (LOW confidence)
- None used for decisions.

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH. Compiled and lockfile-diffed this session
- Architecture: HIGH. The pool, nested reentrant runtime and error flow ran end-to-end in the spike (actix integration not spiked, but it only awaits runtime-agnostic tokio sync primitives, which the spike exercised from separate per-test runtimes)
- Pitfalls: HIGH for the observed ones (op_panic, NotFound, reentrant, clippy scope, doctest). MEDIUM for x86 transfer (A1/A2)

**Research date:** 2026-10-20
**Valid until:** 2026-11-20 (deno_core releases weekly, but the version is pinned in `Cargo.lock`, so findings stay valid while 0.412.0 is pinned)
