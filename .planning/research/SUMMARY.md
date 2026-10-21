# Project Research Summary

**Project:** rs3
**Domain:** Brownfield modernization of a Rust actix-web key/value store with embedded V8 (deno_core), from Rust 1.56 / edition 2018 / git+beta deps to Rust 1.99 / edition 2024 / released crates, Linux aarch64
**Researched:** 2026-10-20
**Confidence:** HIGH (stack verified by a compiled, tested, clippy-clean spike; API facts read from exact crate sources). MEDIUM on threading/limits design and status-code choices.

## Executive Summary

rs3 is a ~390-LOC actix-web server whose distinguishing feature is `PATCH /key/{k}`: it runs the stored value as JavaScript in a bare deno_core `JsRuntime`, and that JS can call back into the store through five ops. The code currently cannot build on any modern toolchain (deno_core from git main with the removed `op_sync` API, actix-web 4 beta, Rust 1.56). This is not an incremental upgrade: `cargo fix --edition` is impossible because nothing compiles, so the port is effectively a rewrite of the store, JS and HTTP layers on a fresh `Cargo.toml`, keeping the HTTP routes and success-body bytes intact.

The recommended approach: pin Rust 1.99.1, edition 2024, `deno_core 0.412` (use `deno_core::v8` only, never a direct `v8` dep), `actix-web 4.17` (no direct tokio/actix-rt), `papaya` behind an owned-value `Store` facade, `config 0.15` builder + `dotenvy`, `thiserror` per-layer errors. Restructure into `store` / `js` / `http` / `config` modules behind a `lib.rs` with a thin `main.rs` (still one package/crate). Inject the store via `web::Data` (HTTP) and `OpState` (JS); no globals. Run JS on a small, bounded pool of dedicated, large-stack threads with a fresh `JsRuntime` per job.

The key risks are process-killing failures unique to embedded V8: a panic inside an op aborts the whole server (ops are `extern "C"`), self-recursive `op_rs3_patch` overflows the native stack (SIGSEGV), deno_core's debug-only re-entrancy check aborts nested-patch tests, and V8's protected platform segfaults on PKU-capable x86 CI runners when V8 is initialised on the wrong thread ("works on aarch64, crashes in CI"). All are preventable with known measures (fallible ops returning `JsErrorBox`, depth limit + big-stack threads, `#[op2(reentrant)]`, platform init in `main` before any thread spawns), but each must be tested explicitly, and CI on x86 must land soon after the JS port to surface the PKU issue early.

## Key Findings

### Recommended Stack

Spike-verified dependency set (compiled, `cargo test` and `clippy --all-targets -D warnings` green on 1.99.0 aarch64; also builds on 1.95.0). See STACK.md for the exact `Cargo.toml`.

**Core technologies:**
- **Rust 1.99.1, edition 2024**: `rust-toolchain.toml` with `channel = "1.99.1"`, `rustfmt` + `clippy`, `profile = "minimal"`. Delete the legacy `rust-toolchain` file in the same commit, because it takes precedence over the new one.
- **deno_core 0.412.0**: the only maintained V8 embedding with an op system. Uses the `deno_v8 0.4` facade over `v8 150.4`. Never depend on `v8` directly (newest is 152, which is incompatible and would cause a duplicate V8). Don't disable default features.
- **deno_error "0.7"**: write it by hand. deno_core pins `=0.7.1`, and `cargo add` picks 0.7.3, which fails to resolve.
- **actix-web 4.17.0** (`default-features = false, features = ["macros"]`): `#[actix_web::main]` / `#[actix_web::test]` replace direct actix-rt/tokio deps.
- **papaya 0.2.5**: lock-free successor to flurry (same `seize` base). It cannot deadlock under JS re-entrancy.
- **config 0.15.27** (no default features) + **dotenvy 0.15.7**: builder API. Dotted `SERVER.HOST` env names still nest correctly (verified).
- **serde / serde_json (enable `preserve_order`) / thiserror 2**: typed errors and correct `/list` escaping.
- **CI/Docker**: `actions/checkout@v7`, `dtolnay/rust-toolchain@1.99.1`, `Swatinem/rust-cache@v2`. Builder `lukemathwalker/cargo-chef:latest-rust-1.99.1-trixie` (not `-slim`: the v8 build.rs needs curl), runtime `gcr.io/distroless/cc-debian13:nonroot` (same Debian release as the builder).

### Expected Features

No new product features ship. The "features" are API preservation, robustness, tests, CI, Docker and docs.

**Must have (table stakes):**
- T1: All 7 routes keep path, method and success-body bytes, including the double-encoded JSON-string envelope and `/ip`'s single-quoted pseudo-JSON.
- T2–T5: Typed `AppError`→status mapping with no `unwrap` in handlers, store or ops. Missing key → 404 (also fixes the empty-map `"[]"` quirk). `/insert` validates everything before writing anything (400 malformed JSON, 422 wrong shape or non-string value). JS exception → 422 with a JSON error body.
- T6: PATCH result stays `String(completionValue)`, read via `deno_core::scope!` (drop the "new Context" hack).
- T7–T8: Five ops as `#[op2]` + `extension!` (`op_rs3_list/get/insert/delete/patch`), callable as `Deno.core.ops.op_rs3_*`. Migrate every JS artefact (`requests/*.js`, `*.txt`, `*.http`, `src/example.js`, test scripts) and remove `Deno.core.ops();`.
- T9–T12: Injected `Store` and `Config`. Isolated deterministic tests (no sleep, no env, no global). Error-path tests and op-level tests.
- T13–T14: `main` returns `Result`, so startup errors don't panic. Dotted env config still works.
- T15–T18: Gating CI (fmt / clippy `--all-targets -D warnings` / test, all `--locked`, no `continue-on-error`, no `SERVER.*` env). cargo-chef Dockerfile + `.dockerignore`, binding 0.0.0.0. README with op mapping, status codes and a "Behaviour changes" section.
- D4 (escaping part): `/list` built with serde_json, sorted by key, same shape.

**Should have (recommended in this milestone):**
- D2 `op_rs3_patch` depth limit (**in scope**: a self-patching key otherwise crashes the process).
- D1 off-worker execution + execution timeout (**in scope**: `while(true){}` otherwise pins a worker forever).
- D3 `rs3` JS facade, D7 sample scripts as test fixtures, D5 logging, D6 `SERVER__HOST` alias + defaults, D8 release profile, D10 non-root image.

**Defer (v2+ / later milestones):**
- Versioned API without the double encoding, Promise/async results, Store trait + git backend, V8 startup snapshot, cargo-deny/arm CI jobs, coverage, musl/Alpine.
- Anti-features: `web::Json` for `/insert` (breaks `curl -d` and the 400/422 split), reusing one JsRuntime across requests, adding deno_runtime/fs/net, back-compat shim for `opSync`, MSRV job.

### Architecture Approach

A layered single crate: `main.rs` (≈25 lines of wiring) over `lib.rs` exposing `config`, `store`, `js`, `state`, `http`. Dependencies point strictly downward (`http → state → js → store`). `store` knows nothing about actix or V8. `js` knows nothing about actix. An `AppState { store, js, settings }` made of Arc-backed fields is shared by every transport, which leaves seams for future LSP, git-over-HTTP and TypeScript layers. `GET /list` and `op_rs3_list` share one codec function (`store/legacy.rs`), so their bytes are identical by construction.

**Major components:**
1. **config**: `Settings { server, js }`, loaded once in `main` with defaults. Tests build it in code.
2. **store**: a cheap-clone `Store` with a sync, fallible, owned-value API (`get/insert/insert_many/remove/list`), `StoreError`, and the legacy wire-format codec. papaya inside. No guards leak out.
3. **js**: `init_platform`, `#[op2]` ops reading `ExecCtx { store, depth, limits }` from `OpState`, `run_script`, and `JsExecutor` (a bounded pool of dedicated threads), plus limits.
4. **http**: actix handlers, `configure(state)` used by both `main` and tests, `ApiError: ResponseError`.

### Critical Pitfalls

1. **Op panic aborts the process**: op2 slow ops are `extern "C"` with no `catch_unwind`. Fix: store returns `Result`, ops return `Result<_, JsErrorBox>`, `#![deny(clippy::unwrap_used)]` in non-test code, and a test that calls an op on a missing key and checks the process survives.
2. **Nested patch: re-entrancy abort in debug, stack overflow on recursion**: mark `op_rs3_patch` `#[op2(reentrant)]`. Create and drop the nested `JsRuntime` inside the op body (LIFO drop order). Cap depth (default 4, error includes the key chain). Run JS on threads with an explicit ~16 MiB stack.
3. **V8 platform / PKU segfault on x86 CI only**: initialise the platform in `main` before actix and the JS pool spawn threads. Tests need a matching strategy (see decision (b)). Verify on an x86 runner with default `--test-threads`.
4. **Version coupling and stale toolchain**: delete the legacy `rust-toolchain` file and `Cargo.lock` first. Don't use `cargo fix --edition`. No direct `v8`. `deno_error = "0.7"`. `rust-version = "1.99"` (a lower value makes resolver 3 choose stale deps). CI check: `cargo tree -d` must show no duplicate `v8`.
5. **State and test isolation traps**: a store created inside the `HttpServer::new` closure gives one map per worker (tests pass, the server is broken). `set_var` is `unsafe` in edition 2024. dashmap-style guards held across JS deadlock. Fix: create state outside the closure, use one `configure(state)` helper, load config once, use an owned-value store API, and `#![forbid(unsafe_code)]`.
6. **Docker**: a slim builder has no curl, so the V8 download fails. trixie/bookworm glibc mismatch. `--locked` missing. No `.dockerignore`, so multi-GB `target/` goes into the context. Smoke-test with `docker run` + `curl /`.

## Resolved Decisions (researcher disagreements)

| # | Question | Decision | Reasoning |
|---|----------|----------|-----------|
| a | JS execution model: per-request `web::block` (STACK) vs fixed pool of dedicated threads `JsExecutor` (ARCHITECTURE) | **Fixed, bounded pool of dedicated named threads** (default `available_parallelism`, explicit ~16 MiB stack, each with an entered tokio `current_thread` runtime). Fresh `JsRuntime` per job. Reply via oneshot. Queue full → 503. | `web::block` concurrency is unbounded (up to 512 threads × V8 heap), its 2 MiB stack is too small for nested isolates, and its threads don't share a single ancestry for V8. deno_core 0.412 aborts if a runtime is created without a tokio context and V8 posts a delayed task, so the per-thread runtime handles that too. Cost is ~150 LOC. The `web::block` approach is an acceptable stepping stone inside the JS-port phase if it helps get PATCH green first, but the milestone should end on the pool. |
| b | V8 platform init: `init_platform(new_unprotected_default_platform)` in a `Once` (STACK) vs `init_platform(None)` in `main` + `unsafe_use_unprotected_platform` only as a dev-dep feature (PITFALLS/ARCH) | **Production: `JsRuntime::init_platform(None)` as the first step in `main`**, before actix or the JS pool spawn threads. This keeps deno_core's own default platform. **Tests: enable `deno_core` feature `unsafe_use_unprotected_platform` in `[dev-dependencies]` only.** | The default platform keeps deno's custom foreground-task handling and memory protection in prod. With a single init point and a pool spawned from the main thread, the ancestry rule holds. The dev-dep feature removes the libtest sibling-thread problem without leaking into the release binary (resolver 2/3). Fallback if x86 CI still crashes: the spike-proven `new_unprotected_default_platform(0, false)` in an `ensure_v8()` `Once`. **Must be verified on an x86 runner.** |
| c | papaya vs dashmap | **papaya 0.2.5** behind a `Store` facade returning owned `String`s. | Lock-free reads cannot deadlock when JS ops re-enter the store during PATCH. dashmap would work too given owned returns, but it needs discipline that papaya doesn't. |
| d | `rust-version` 1.99 vs ~1.95 effective MSRV | **Toolchain pinned `1.99.1`** (current stable, per the brief). **`rust-version = "1.99"`.** | Declare only what CI tests. A lower value makes resolver 3 prefer older deps (config/actix need 1.88), which contradicts "latest released versions". Note: the host has 1.99.0, so rustup will auto-install 1.99.1 on the first cargo call. The real floor (~1.95, set by deno_core) is documentation only. |
| e | Hardening scope (timeout, heap limit, patch depth, cancel-on-drop, busy 503) | **In scope:** patch depth limit (+ `reentrant` + big stacks) and execution timeout via `IsolateHandle::terminate_execution`. **Optional / stretch:** heap limit + near-heap-limit callback, cancel-on-client-disconnect, 503 backpressure tuning. | Self-patch kills the process and `while(true)` pins a worker, so both are robustness bugs, not features. Heap OOM is real but rarer; add it if time allows. |
| f | Status codes for timeout and bad insert (ARCH 504/400 vs FEATURES 422/400+422) | **400** malformed JSON (`serde_json::Error::classify` Syntax/Eof). **422** wrong shape, non-string value, JS exception, depth limit, memory limit. **504** timeout. **503** busy. **500** host faults. Errors are real JSON objects `{"error": ..., "message": ...}`. | RFC 9110 semantics. Timeout is a server-side execution deadline (gateway-style), not a client fault. |
| g | `op_rs3_get`/`op_rs3_delete` on missing key: `null` (ARCH) vs throw (FEATURES) | **Throw a JS Error** (matching HTTP 404). Scripts can `try/catch`. | Keeps "a miss is an error" consistent across HTTP and JS. Document it in the op mapping table. |
| h | `Store` as `Arc<dyn StoreBackend>` trait (ARCH) vs concrete struct (FEATURES: premature trait is an anti-feature) | **Concrete `Store` newtype now** with a narrow owned-value, fallible API. Extract the trait when the git backend arrives. | YAGNI. The newtype hides the internals, so extracting the trait later is cheap. |
| i | lib target | **Add `src/lib.rs` + thin `src/main.rs` in the same package.** Record as a Key Decision: still a single crate/package (no workspace), satisfying the "single binary crate" constraint. Prefer in-crate `#[cfg(test)]` modules or a single `tests/it/main.rs`. | Each extra test binary links the ~150–200 MB V8 static lib. Enables reuse by future LSP/bin targets. |

**Environment note:** the root filesystem (`/tmp`) is ~96% full, while `/home` has ~51 GB free. Keep `target/` inside the project dir (the default), and don't point `CARGO_TARGET_DIR` or `TMPDIR`-heavy build steps at `/tmp`. Debug `target/` is ~2.6 GB with V8.

## Implications for Roadmap

Suggested phase structure (7 phases):

### Phase 1: Toolchain & Dependency Bootstrap
**Rationale:** Everything depends on a compiling tree. The legacy toolchain file and git lockfile must go first.
**Delivers:** `rust-toolchain.toml` (1.99.1), legacy `rust-toolchain` and old `Cargo.lock` deleted, edition 2024, `rust-version = "1.99"`, new `[dependencies]` (STACK set), lib.rs/main.rs skeleton, a compiling build (PATCH may temporarily return 501), a dedicated `cargo fmt` commit, `[lints]` config.
**Addresses:** Active reqs: toolchain, released deps.
**Avoids:** Pitfalls 5, 6, 21, 22 (legacy file precedence, `cargo fix` trap, duplicate v8, deno_error pin, resolver-3 stale deps).

### Phase 2: Store & Config Layers
**Rationale:** Both HTTP and JS depend on `Store`. It is pure Rust and the cheapest place to settle the API.
**Delivers:** `Store` (papaya, owned values, `insert_many` validate-then-write, sorted `list`), `StoreError`, `legacy.rs` codec (serde_json-escaped `/list`, insert echo in input order), `Settings` loaded once with defaults, unit tests (config via `Environment::source(Some(map))`).
**Addresses:** T4, T9, T13, T14, D4, D6 (optional).
**Avoids:** Pitfalls 8, 11, 16, 18.

### Phase 3: HTTP Layer Port & Isolated Tests
**Rationale:** Turns most of the old test suite green before the riskiest (V8) work. Provides `AppState`/`ApiError` that PATCH needs.
**Delivers:** actix-web 4.17 handlers, `configure(state)` shared by main and tests, `ApiError: ResponseError` with the status table, golden characterization tests for preserved bodies, error-path tests, a multi-worker shared-store test.
**Addresses:** T1–T4, T10, T11, T13.
**Avoids:** Pitfalls 7, 17, 18, 11.

### Phase 4: JS Runtime Port (deno_core op2)
**Rationale:** Core value: PATCH and JS→store ops working on modern deno_core.
**Delivers:** `init_platform` in main (+ dev-dep feature for tests), five `#[op2]` ops via `extension!` + `ExecCtx` in OpState, `op_rs3_patch` marked `reentrant` with nested runtime created and dropped in-op, result read via `scope!`, `JsExecutor` pool, PATCH wired, JS errors → 422, all JS artefacts migrated (`.js/.txt/.http/example.js`), op-level and sample-fixture tests.
**Uses:** deno_core 0.412, deno_error 0.7, thiserror.
**Addresses:** T5–T8, T12, D7.
**Avoids:** Pitfalls 1, 2, 4, 9, 10, 20, 23.

### Phase 5: CI Pipeline
**Rationale:** Comes right after the JS port so the x86 PKU risk and the cold V8 download show up before more is built on top. Needs the passing test suite to gate.
**Delivers:** A single-job workflow (checkout@v7, rust-toolchain@1.99.1, rust-cache@v2): fmt --check, clippy --all-targets --locked -D warnings, test --locked. No `continue-on-error`, no `SERVER.*` env. `cargo tree -d` v8 check. Delete the `actions-rs` workflow.
**Addresses:** T15.
**Avoids:** Pitfalls 4 (verification), 15, 25.

### Phase 6: JS Robustness Hardening
**Rationale:** Separated from the port because it is the part most likely to surprise (nested isolates, stack sizing, termination classification) and deserves its own verification.
**Delivers:** Patch depth limit (default 4, `RangeError` with key chain → 422). Thread stack sizing verified by a recursion test. Execution timeout via `terminate_execution` watchdog → 504, classified by reason, not by string. Optional: heap limit + near-heap-limit callback, cancel-on-disconnect, busy → 503.
**Addresses:** D1, D2.
**Avoids:** Pitfalls 3, 19.

### Phase 7: Docker & Documentation
**Rationale:** Needs the final binary and behaviour. Docs describe the final op mapping and status codes.
**Delivers:** cargo-chef Dockerfile (trixie builder → distroless cc-debian13:nonroot, `--locked`, `SERVER.HOST=0.0.0.0`, exec-form entrypoint, `.dockerignore`), a `docker run` + `curl /` smoke check, README rewrite (toolchain, first V8 build note, env forms, endpoint/status table, op mapping, behaviour changes, known break of out-of-scope `docker_rs3/`/`aws/`), optional release profile tuning.
**Addresses:** T16, T17, T18, D8, D10.
**Avoids:** Pitfalls 12, 13, 14, 26.

### Phase Ordering Rationale

- Store before HTTP and JS: both consume it. Injecting it first avoids porting ops against the global `MAP` and then re-porting.
- HTTP before JS: gives `AppState`/`ApiError` for PATCH, and gets the bulk of the tests green early.
- CI immediately after the JS port, not last: the PKU crash and cold-cache V8 behaviour only appear on clean x86 runners, and finding them late would force rework.
- Hardening after CI: limits get verified under the real gate. Depth limit + timeout are in scope because they prevent process death and worker starvation.
- Phases 2 and the config part of 2 could run in parallel with late Phase 1 work. They are small and independent.

### Research Flags

Phases likely needing deeper research or a spike during planning:
- **Phase 4:** V8 platform/test strategy (decision b) must be confirmed. Need to verify that a nested `JsRuntime` inside a sync `reentrant` op works on 0.412 (not yet compiled), that `Result<Option<String>, JsErrorBox>`/`Result<String, JsErrorBox>` op returns compile as expected, and to measure `JsRuntime::new` cost in debug and release.
- **Phase 6:** native stack needed per nesting level (MEDIUM reasoning, ~1 MB/level), the exact `heap_limits` / `add_near_heap_limit_callback` signatures on v8 150.4, and termination classification.
- **Phase 5:** whether GitHub x86 runners expose PKU (verify empirically). Cache size with V8.

Phases with standard patterns (skip research):
- **Phase 1, 2, 3, 7:** well documented, and the spike and source reading already cover the APIs.

## Confidence Assessment

| Area | Confidence | Notes |
|------|------------|-------|
| Stack | HIGH | Registry API data + compiled, tested, clippy-clean spike on 1.99.0 (and 1.95.0) |
| Features | HIGH / MEDIUM | Current behaviour read from code (HIGH). Status-code choices are design decisions (MEDIUM) |
| Architecture | MEDIUM-HIGH | API facts from exact crate sources. Pool, nested isolate and stack design are reasoned and not yet compiled |
| Pitfalls | HIGH | Mostly source-inspected (op2 `extern "C"`, reentrancy check, build.rs, setup.rs). PKU-on-runner is MEDIUM |

**Overall confidence:** HIGH for the "what", MEDIUM for the JS threading/limits "how".

### Gaps to Address

- **PKU / platform init on x86:** verify decision (b) in Phase 4/5 on an x86 runner. The fallback is ready.
- **Nested runtime in a reentrant op:** spike early in Phase 4. If it fails, keep the depth limit but revisit the nesting mechanism.
- **Per-request isolate cost:** measure. Snapshotting is deferred unless it is unacceptably slow.
- **Stack per nesting level:** confirm with a recursion test and tune `thread_stack_bytes`.
- **`/list` format change:** whitespace, escaping and sort order change. Confirm with the user (record in PROJECT.md Key Decisions) together with the 404 replacing `"[]"` and the op rename.
- **Disk:** `/tmp` is nearly full. Keep build artefacts under `/home` (the project's `target/`).

## Sources

### Primary (HIGH confidence)
- deno_core 0.412.0, deno_ops 0.288.0, deno_v8 0.4.0, v8 150.4.0, deno_error 0.7.1, actix-web 4.17.0, config 0.15.27, dotenvy 0.15.7, dashmap 6.2.1 crate sources (jsruntime.rs, setup.rs, ops.rs, op2/dispatch_slow.rs, build.rs, platform.rs, isolate.rs, test_utils.rs, env.rs)
- crates.io API (versions, dependency pins, MSRVs), 2026-10-20
- A throwaway spike crate built, tested and linted on aarch64 with Rust 1.99.0 / 1.95.0
- static.rust-lang.org channel manifest (1.99.1), Docker Hub / gcr.io tags, GitHub API (actions, rusty_v8 release assets, deno_core repo archived)
- rustup overrides docs, Rust Edition Guide 2024, RFC 9110
- rs3 repository source (`src/`, `requests/`, Dockerfile, workflow)

### Secondary (MEDIUM confidence)
- actix.rs docs (app state, configure), distroless README, GitHub Actions cache changelog, glibc-mismatch community reports

### Tertiary (LOW confidence)
- deno community Q&A on embedding in tokio. Historical deno bootstrap timing commits. v8-users PKU thread (corroboration only)

---
*Research completed: 2026-10-20*
*Ready for roadmap: yes*
