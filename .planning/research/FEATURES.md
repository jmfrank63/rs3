# Feature Research

**Domain:** Brownfield modernization of a small Rust HTTP key/value store (actix-web 4) with an embedded V8 JavaScript runtime (deno_core) whose JS can call back into the store
**Researched:** 2026-10-20
**Confidence:** HIGH for current behaviour (read from `src/`, `requests/`) and for the deno_core API (read from the deno_core 0.412.0 crate source). MEDIUM for the status-code choices: they follow RFC 9110 and actix conventions, but they are design decisions, not facts.

> **What "features" means for this milestone.** No new product features ship. The "features" are
> (a) the HTTP API, which must keep the same routes and semantics, (b) the five JS store ops,
> (c) robustness, (d) deterministic tests, (e) CI, (f) Docker and (g) docs. Each feature below
> says what a well-built modern Rust service does, and whether each existing quirk should be
> **preserved** or **fixed**.

---

## Current Behaviour Inventory (ground truth from the code)

| Route | Handler path | Current success body (raw bytes) | Current failure behaviour | Asserted by a test? |
|-------|--------------|----------------------------------|---------------------------|---------------------|
| `GET /` | `services::index` | `"Welcome to rs3"` (a JSON string) | none | **Yes**, exact body |
| `GET /ip` | `services::status` | `"{'IP' : '<host>'}"`: a JSON string holding pseudo-JSON with single quotes | `Config::from_env().unwrap()` on **every request**, so it panics if the env is missing | **Yes**, exact body (reads env in the test, so it is not deterministic) |
| `POST /insert` | `bindings::insert` | JSON string of the re-serialized object, e.g. `"{\"insert_test\":\"John Doe\"}"` | `unwrap` panics on bad JSON, non-object and non-string values. Inserts happen **while iterating**, so a panic midway leaves a partial insert | **Yes**, exact body + 200 |
| `GET /list` | `bindings::list` | JSON string holding a hand-built array `[{"k" : "v"},...]`. Nothing is escaped, so a value with `"` (stored JS!) gives invalid inner JSON | none | Status only, plus `patch body == list body` |
| `GET /key/{k}` | `bindings::get` | JSON string of the value | Empty map: returns `"[]"` for **any** key. Non-empty map with a missing key: `unwrap` panic | Status only |
| `DELETE /key/{k}` | `bindings::delete` | JSON string of the removed value | Missing key: `unwrap` panic | Status + key removed |
| `PATCH /key/{k}` | `bindings::patch` | JSON string of `String(completionValue)` (`to_rust_string_lossy`) | Missing key: panic (via get). JS exception: `unwrap` panic. Reads the result in a **new** V8 context (a hack) | Status + body equals `/list` body |

Every success body is `HttpResponse::Ok().json(String)`, so **every response is a JSON-encoded string** with `Content-Type: application/json`, even when the string itself contains JSON (double encoding). Clients that work today parse twice.

JS ops today: `Deno.core.ops();` (syncs the op ids) and then `Deno.core.opSync("rs3_<name>", arg)`. Names: `rs3_list`, `rs3_get`, `rs3_insert`, `rs3_delete`, `rs3_patch`. `src/example.js` calls `op_list`, which was **never registered**, so the sample is already broken. The old syntax also appears in **`requests/*.txt` and `requests/*.http` payloads**, not only in `*.js`. All of them need updating.

---

## Feature Landscape

### Table Stakes (the milestone fails without these)

| # | Feature | Why Expected | Complexity | Notes |
|---|---------|--------------|------------|-------|
| T1 | **All seven routes keep path, method and success body shape** | Core Value: "same semantics". Tests assert exact bodies for `/`, `/ip`, `/insert` | LOW | Keep the double-encoded JSON-string envelope on every 2xx response this milestone (see the Quirks table). Use `HttpResponse::Ok().json(s)` exactly as today |
| T2 | **Typed error type → HTTP status (no `unwrap`/`expect` in handlers, store or ops)** | Brief item 5. A handler panic is caught by actix (500 plus a dropped connection). An **op panic crosses the V8 FFI boundary and aborts the whole process** | MEDIUM | One `AppError` enum (`thiserror`) implementing `actix_web::ResponseError`. The status map is below. `clippy::unwrap_used` can be denied in non-test code to lock this in |
| T3 | **Missing key returns 404 on GET, DELETE and PATCH** | Standard REST. Today it is a panic or the bogus `"[]"` | LOW | **Fix** the empty-map `"[]"` quirk: return 404 on an empty map too. The old behaviour was inconsistent (`"[]"` when empty, panic when non-empty) and made a stored value of `[]` impossible to tell apart from a miss |
| T4 | **`POST /insert` validates first and is all-or-nothing** | A partial insert on bad input is data corruption | LOW | Parse the whole body into an ordered map of `String→String` **before** touching the store. Malformed JSON (syntax/EOF) → **400**. Well-formed but not an object, or any non-string value → **422**, naming the offending key. `{}` → 200, no-op, body `"{}"`. Body over the actix 256 KiB default `PayloadConfig` limit → **413** (actix gives this for free) |
| T5 | **`PATCH` surfaces JS exceptions as 422 with a JSON error body** | A stored script that throws or fails to parse is a fault in the client's content, not the server's | MEDIUM | `execute_script` returns `Result<v8::Global<v8::Value>, Box<JsError>>` (verified, deno_core 0.412). Map `Err(JsError)` → 422 with `name`, `message`, and optionally `stack`. Use 500 **only** for host faults (runtime init failure, blocking-task join error) |
| T6 | **PATCH result semantics preserved: `String(completionValue)`** | `test_patch_body_is_listing` and every sample in `requests/` depend on it | LOW | Read the result with `deno_core::scope!(scope, runtime)` + `v8::Local::new(scope, global)` + `to_rust_string_lossy`. **Drop** the "new Context" hack. Numbers become `"120"`, `undefined` becomes `"undefined"`, objects become `"[object Object]"` (samples call `JSON.stringify` themselves). A Promise becomes `"[object Promise]"`; document that, don't change it |
| T7 | **Five store ops ported to `#[op2]` + `extension!`, callable as `Deno.core.ops.op_rs3_*`** | Brief item 3 | MEDIUM | Mapping table below. The store reaches ops through `OpState` (`state.borrow::<Store>()`, put there via the extension's `options`/`state` closure). Ops return `Result<String, JsErrorBox>` so failures become **thrown JS errors**, never panics. `Deno.core.ops` and `Deno.core.print` still exist in a bare `JsRuntime` (verified in `01_core.js`) |
| T8 | **All JS artefacts migrated to the new syntax** | Broken samples are broken docs | LOW | `requests/*.js`, **`requests/*.txt`, `requests/*.http`** (embedded payloads), `src/example.js` (also fix its `op_list` bug), and test scripts. Remove every `Deno.core.ops();` preamble: `Deno.core.ops` is now an **object**, so calling it throws `TypeError` |
| T9 | **Injected shared state; no global `MAP`** | Brief items 6, 7 and the Key Decision. Required for test isolation and for future git/TS layers | MEDIUM | A cheap-to-clone `Store` (an `Arc` inside) handed to `App` via `web::Data<Store>` and to each `JsRuntime` via `OpState`. The API returns **owned `String`s only**, never guards or refs (see Pitfall note). Config is also injected (`web::Data<Config>`) so `/ip` stops re-reading the env per request |
| T10 | **Deterministic, isolated tests** | Brief item 6. `thread::sleep` + `MAP.clear()` + a shared global is a race | LOW–MEDIUM | Each test builds a fresh `Store` and `Config` and calls `test::init_service(App::new().app_data(...).configure(routes))`. No sleeps, no clears, no env reads. Use `#[actix_web::test]` and `test::call_and_read_body` / `test::call_service`. Tests run in parallel safely because nothing is shared |
| T11 | **Tests for every error path** | Robustness that isn't tested regresses | LOW | 404 on GET/DELETE/PATCH, 400 bad JSON, 422 non-object, 422 non-string value (and the store unchanged afterwards), 422 JS throw, 422 JS syntax error, JS-side op error (missing key inside `op_rs3_get`) → 422 |
| T12 | **Op-level tests independent of HTTP** | JS-runtime layer separation; faster diagnosis | LOW | Build a runtime with a seeded `Store`, run a script, assert the result and the store contents. Covers all five ops, including `op_rs3_patch` (nested execution) |
| T13 | **Startup errors are reported, not panicked** | `Config::from_env().unwrap()` in `main` gives a confusing panic message | LOW | `main` returns `Result`. A missing or invalid `SERVER.HOST`/`SERVER.PORT` prints a clear message and exits non-zero |
| T14 | **`SERVER.HOST` / `SERVER.PORT` via `.env`/env still work** | Validated requirement | LOW | Verified in config 0.15.27: the `Environment` source lowercases keys and `set_value` parses `server.host` as a path, so dotted names still nest. Tests must **not** mutate the env (`std::env::set_var` is `unsafe` in edition 2024). Use `Environment::default().source(Some(map))` for config-loading tests |
| T15 | **CI that actually gates** | Brief item 8 | LOW | See the CI spec below. fmt, clippy (`-D warnings`, `--all-targets`) and test, all with `--locked`, no `continue-on-error`. Drop the `SERVER.HOST` env from the workflow: once tests stop needing it, that proves isolation |
| T16 | **Dockerfile: current `rust` builder, cargo-chef, slim glibc runtime, binds 0.0.0.0** | Brief item 9 | LOW–MEDIUM | See the Docker spec below. **Must** set `SERVER.HOST=0.0.0.0` (localhost inside a container is unreachable). **Must** add a `.dockerignore` that excludes `target/`, which grows to GBs with V8 artefacts |
| T17 | **README rewrite** | Brief item 10 | LOW | Toolchain, first-build note (the V8 prebuilt download is large and slow), `.env` setup, endpoint table **with status codes**, op mapping table, curl examples, Docker build/run, `cargo test`/`clippy`/`fmt` |
| T18 | **Documented behaviour changes** | Users of the old API need to know what moved | LOW | A "Behaviour changes" section in the README (or `CHANGELOG.md`): `"[]"`→404, panics→4xx, op rename + no `Deno.core.ops()`, `/list` escaping, `/list` key order |

### Differentiators (worth doing if cheap; not required to call the port done)

| # | Feature | Value Proposition | Complexity | Notes |
|---|---------|-------------------|------------|-------|
| D1 | **Run JS off the actix worker (`web::block`) + execution timeout** | `while(true){}` in a stored script hangs an actix worker thread forever today. With N workers, N such PATCHes take down the server | MEDIUM | Create **and drop** the `JsRuntime` inside the `web::block` closure (it is `!Send`, which is fine because it never leaves the thread). A watchdog calls `runtime.v8_isolate().thread_safe_handle().terminate_execution()` after e.g. 5 s (API verified in deno_core tests). Map a timeout to 422 `{"error":"js_timeout"}`. **Highest-value differentiator; strongly recommended** |
| D2 | **Recursion-depth limit for `op_rs3_patch`** | A script that PATCHes its own key recurses, creating isolates until stack overflow, which **aborts the process** (not catchable) | LOW | Keep a depth counter in `OpState` (or a thread-local), passed into the nested runtime. Throw a JS error at depth > 8 |
| D3 | **Stable `rs3` JS facade** (`rs3.get(k)`, `rs3.list()`, `rs3.insert(obj)`, ...) loaded via the extension's `js`/`esm` files | Insulates stored scripts from deno_core internals (`Deno.core.ops.*` is an internal API that has churned twice). Gives the future TypeScript layer a typed surface (`rs3.d.ts`) | LOW | ~15 lines of JS. Keep `Deno.core.ops.op_rs3_*` working as well (the brief requires that syntax). `rs3.insert` can accept an object and `JSON.stringify` it |
| D4 | **Sorted, properly escaped `/list`** | Deterministic output for tests and diffing. Fixes invalid JSON when values contain quotes | LOW | Build the inner array with `serde_json` (escapes correctly), sorted by key. Shape **preserved** (`[{"k":"v"},...]` wrapped in a JSON string). Only whitespace changes (`" : "` → `":"`). Treat as near-table-stakes: the escaping fix is part of T2 robustness |
| D5 | **Request logging (`middleware::Logger` + `env_logger` or `tracing`)** | Basic operability in Docker | LOW | Off by default, controlled by `RUST_LOG` |
| D6 | **Shell-friendly env aliases** (`SERVER__HOST`) | `export SERVER.HOST=...` is invalid in bash/zsh, so today the vars can only come from `.env`, `env`, Docker or CI | LOW | `Environment::default().separator("__")`. Dotted names keep working. Also consider defaults (`127.0.0.1:8080`) so `cargo run` works without a `.env` |
| D7 | **Sample scripts as test fixtures** (`include_str!("../requests/fac.js")`) | Docs and samples can never rot again | LOW | One test per sample: seed inputs, run, assert (e.g. fac(5) → `"120"`, sum → `"55"`) |
| D8 | **Release profile tuning** (`strip = true`, `lto = "thin"`, `codegen-units = 1`) | V8 makes the binary large. Stripping saves tens of MB in the image | LOW | Longer release link time; dev builds unaffected |
| D9 | **CI extras:** Docker build job (no push), `cargo-deny` (advisories + licenses; V8 pulls many), Dependabot for `cargo` + `github-actions`, an `ubuntu-24.04-arm` job matching the aarch64 target | Catches Dockerfile rot and supply-chain issues. Exercises the real platform | LOW each | Run the Docker job only on `main`/PR with a GHA cache backend. It is slow because of V8 |
| D10 | **Non-root runtime user** | Container hardening at zero cost | LOW | `gcr.io/distroless/cc-debian13:nonroot`, or `USER 65532` on `debian:trixie-slim` |
| D11 | **Error body using RFC 9457 field names** (`type`/`title`/`status`/`detail`, `application/problem+json`) | Standard, self-describing errors | LOW | Same effort as an ad-hoc `{error,message}`. Choose one shape and use it everywhere. Ad-hoc `{"error","message"}` is acceptable for this milestone |
| D12 | **lib + bin targets in the single package** (`src/lib.rs` + thin `src/main.rs`) | Enables `tests/` integration tests and doc-tests. Still one crate, so it satisfies the "single binary crate" constraint in spirit | LOW | Optional. `#[cfg(test)]` modules are enough for this milestone |

### Anti-Features (deliberately do NOT build this milestone)

| Feature | Why Requested | Why Problematic | Alternative |
|---------|---------------|-----------------|-------------|
| **"Fixing" the double-encoded JSON-string envelope** (returning real JSON objects from `/list`, `/insert`, `/ip`) | It is the obvious wart | Breaks asserted test bodies and every existing client. It is a semantic change, not modernization | Preserve it. Document it as a known quirk. Fix in a future versioned API (`/v2/...` or `Accept` negotiation) |
| **Changing `/list` to a single object `{"k":"v",...}`** | Nicer shape | Same break as above. `patch == list` comparisons in scripts would change | Keep the array of single-entry objects. Fix escaping only (D4) |
| **Coercing non-string insert values** (`{"num": 5}` → `"5"`) | Friendlier | Silent semantic change. Ambiguous for objects and arrays (stringify how?) | Reject with 422 naming the key. Revisit as an explicit feature later |
| **Back-compat shim for `Deno.core.opSync("rs3_get", ...)` / callable `Deno.core.ops()`** | Old scripts keep working | The store is in-memory, so no legacy scripts survive a restart. The shim would perpetuate a dead internal API, and `Deno.core.ops` cannot be made callable without hacks | Clean break + documented mapping + optional `rs3` facade (D3) |
| **Using `web::Json<T>` for `/insert`** | Idiomatic actix | Enforces `Content-Type: application/json`. `curl -d` sends `application/x-www-form-urlencoded` and would start getting errors (actix maps content-type errors to **400**, verified). That is a compat break. It also maps all deserialization errors to 400, losing the 400/422 distinction | Keep the `String` extractor + explicit `serde_json` parsing |
| **Adding `deno_runtime`, fetch, fs, timers or a module loader to the JS runtime** | "Real Deno" | Turns arbitrary stored JS into network/filesystem access on the server. The bare `JsRuntime` has none of that, which is its security property | Keep the bare `JsRuntime` + the five ops (+ `Deno.core.print`) |
| **Reusing one long-lived JsRuntime/isolate across requests** | Faster | Leaks globals between scripts (one script's `let x` breaks the next one with a redeclaration error). `!Send` lifetime headaches with actix workers | A fresh runtime per PATCH, as today. Snapshotting is a later optimization |
| **Persistence, transactions, or new store operations** | "While we're here" | Explicitly out of scope (PROJECT.md) | Keep the `Store` API small and owned-value-based so a git-backed store can slot in later |
| **Premature `trait Store` + `dyn` plumbing** | Future git backend | YAGNI. Generics/dyn churn through handlers, ops and tests | A concrete `Store` struct with a narrow owned-value API. Extract a trait when the second backend arrives |
| **MSRV job / lower `rust-version` than the pinned toolchain** | "Best practice" | deno_core tracks recent stable (edition 2024, no declared MSRV). An older MSRV would be fiction and a maintenance tax | `rust-version` = the pinned toolchain (1.99). No separate MSRV job |
| **CI matrix over beta/nightly, macOS, Windows** | Coverage | Each V8-linked build is slow and heavy. The deployment target is Linux only | One Linux job (optionally an arm64 job, D9) |
| **Multi-arch Docker via QEMU emulation** | One image for all architectures | Building V8-linked Rust under emulation takes hours | Native build per arch (native arm64 runner) if ever needed |
| **Alpine/musl or `scratch` runtime image** | Smallest image | The V8 prebuilt static libs are glibc-only. musl means building V8 from source | `gcr.io/distroless/cc-debian13` or `debian:trixie-slim` |
| **Code coverage, release automation, registry publishing** | Polish | Low value now. Coverage instrumentation with V8 is slow | Later milestone |
| **Adding curl to the runtime image for `HEALTHCHECK`** | Docker healthchecks | Bloats a distroless image. `GET /` already serves as a liveness probe for an orchestrator | Document `GET /` as the health probe |
| **Renaming PATCH-executes to `POST /key/{k}/exec` or adding 405 handling** | Semantically cleaner | Route change = API break | Keep `PATCH`. Revisit with a versioned API |

---

## Expected Behaviour Specifications

### HTTP status code contract (recommended)

| Situation | Status | Body | Preserve/Fix |
|-----------|--------|------|--------------|
| `GET /` | 200 | `"Welcome to rs3"` | Preserve |
| `GET /ip` | 200 | `"{'IP' : '<host>'}"` (from injected config) | Preserve the body. Fix the per-request env read and unwrap |
| `POST /insert` valid object of strings | 200 | JSON string of the echoed object, keys in **input order** | Preserve (see the `preserve_order` note) |
| `POST /insert` `{}` | 200 | `"{}"` | New explicit case (no-op) |
| `POST /insert` malformed JSON / empty body / invalid UTF-8 | **400** | error JSON | Fix (was a panic) |
| `POST /insert` valid JSON but not an object (array, string, number) | **422** | error JSON | Fix (was a panic) |
| `POST /insert` object with a non-string value | **422**, nothing inserted | error JSON naming the key | Fix (was a panic + partial insert) |
| `POST /insert` body > 256 KiB | 413 | actix default | Free; document |
| `GET /list`, empty store | 200 | `"[]"` | Preserve |
| `GET /list` | 200 | JSON string of `[{"k":"v"},...]`, escaped, sorted by key | Fix escaping + ordering; whitespace changes |
| `GET /key/{k}` hit | 200 | `"<value>"` | Preserve |
| `GET /key/{k}` miss (including an empty store) | **404** | error JSON | **Fix** the `"[]"` quirk and the panic |
| `DELETE /key/{k}` hit | 200 | `"<old value>"` | Preserve (not 204, because the body is part of the contract) |
| `DELETE /key/{k}` miss | **404** | error JSON | Fix |
| `PATCH /key/{k}` success | 200 | `"<String(result)>"` | Preserve |
| `PATCH /key/{k}` miss | **404** | error JSON | Fix |
| `PATCH` script throws / syntax error / op error not caught by the script | **422** | error JSON with JS `name`/`message` (+`stack`) | Fix (was a panic) |
| `PATCH` timeout (D1) / recursion limit (D2) | **422** | error JSON (`js_timeout`, `js_recursion_limit`) | New |
| Runtime creation failure, `web::block` join error, any other host fault | 500 | error JSON, details logged | New |

**Rationale for 400 vs 422:** RFC 9110 §15.5.21 defines 422 (Unprocessable Content) as "syntax correct, but unable to process the contained instructions". Malformed JSON is a syntax problem (400). The wrong shape or type, or a script that fails, is a semantic problem (422). `serde_json::Error::classify()` distinguishes `Syntax`/`Eof` (→400) from `Data` (→422) for free. Errors are **real JSON objects** (not double-encoded strings): no existing client depends on error bodies because errors used to be panics.

### JS op mapping (old → new)

| Old call (2021) | New call | Rust fn | Arg | Returns | Throws (JS Error) when |
|-----------------|----------|---------|-----|---------|------------------------|
| `Deno.core.ops();` | *(delete the line)* | n/a | n/a | n/a | Calling it now throws `TypeError`, because `ops` is an object |
| `Deno.core.opSync("rs3_list", "")` | `Deno.core.ops.op_rs3_list()` | `op_rs3_list(state)` | none (an extra arg is ignored) | string `[{"k":"v"},...]` (same as the inner `/list` payload) | never |
| `Deno.core.opSync("rs3_get", key)` | `Deno.core.ops.op_rs3_get(key)` | `op_rs3_get(state, #[string] key)` | key | value string | key missing (same as HTTP 404) |
| `Deno.core.opSync("rs3_insert", json)` | `Deno.core.ops.op_rs3_insert(json)` | `op_rs3_insert(state, #[string] body)` | JSON object **string** | echoed JSON string | bad JSON / non-object / non-string values (`TypeError`) |
| `Deno.core.opSync("rs3_delete", key)` | `Deno.core.ops.op_rs3_delete(key)` | `op_rs3_delete(state, #[string] key)` | key | old value | key missing |
| `Deno.core.opSync("rs3_patch", key)` | `Deno.core.ops.op_rs3_patch(key)` | `op_rs3_patch(state, #[string] key)` | key | `String(result)` of the nested script | key missing, nested JS error, depth limit (D2) |
| `Deno.core.print(s)` | unchanged (`Deno.core.print(s)`) | built-in | n/a | n/a | n/a |

Conventions: the `op_` prefix plus a `rs3_` namespace (valid Rust identifiers that cannot collide with deno_core built-ins such as `op_print`). Ops are sync, return `Result<String, JsErrorBox>` and are annotated `#[string]` on returns and args. Use `deno_core`'s re-exports (`deno_core::v8`, `deno_core::serde_json`). deno_core pins `deno_error = "=0.7.1"` exactly, so if rs3 names `deno_error` directly it must use the same version. **Missing key throws rather than returning `null`.** That keeps "a miss is an error" identical across HTTP and JS, and the samples call `.toString()` on the result anyway. Scripts can `try/catch`.

### Quirks: preserve vs fix (decision table)

| Quirk | Decision | Why |
|-------|----------|-----|
| Every 2xx body is a JSON-encoded string (double encoding) | **Preserve** | Asserted by tests; client compatibility |
| `/ip` body uses single-quoted pseudo-JSON | **Preserve** | Asserted by a test |
| `/list` hand-built JSON without escaping | **Fix** (serde_json, same shape) | Produces invalid JSON for any value containing `"` or `\`, which includes stored JS |
| `/list` key order = hash-map iteration order | **Fix** (sort by key) | Determinism; free |
| `GET /key` on an empty map returns `"[]"` for any key | **Fix** → 404 | A bug, inconsistent with the non-empty case; nothing asserts it |
| `/insert` echo key order | **Preserve input order, explicitly** | deno_core 0.412 enables `serde_json/preserve_order` (verified in its Cargo.toml). Feature unification therefore already makes `serde_json::Map` insertion-ordered in rs3. Enable `preserve_order` on rs3's own `serde_json` dependency so this does not hinge on a transitive feature |
| PATCH ignores any request body | **Preserve** | No reason to change |
| PATCH returns `"[object Promise]"` for async scripts | **Preserve + document** | Resolving promises needs the event loop; future milestone |
| `example.js` calls the unregistered `op_list` | **Fix** → `op_rs3_list` | Already broken |
| Keys containing `/` are unaddressable (single path segment) | **Preserve + document** | A tail-match route would be an API change |

### Test isolation pattern (what "good" looks like)

- **No process-global state.** `Store::default()` per test, plus `Config { server: ServerConfig { host: "test-host".into(), port: 0 } }` built in code. The `/ip` test asserts `"{'IP' : 'test-host'}"` without touching the env.
- **One app builder shared by `main` and tests:** `fn routes(cfg: &mut web::ServiceConfig)` plus `App::new().app_data(web::Data::new(store.clone())).app_data(web::Data::new(config)).configure(routes)`, so tests exercise exactly the production wiring.
- **Assert on the store directly** after HTTP calls (`store.get("k")`), since the test holds a clone of the same `Arc`.
- **Bodies:** `let body = test::call_and_read_body(&app, req).await; assert_eq!(body, web::Bytes::from_static(br#""Welcome to rs3""#));`. The old `Body::from(...)` comparisons no longer compile in actix-web 4.x final.
- **`#[actix_web::test]`**, not `#[actix_rt::test]`, which lets the direct `actix-rt` dependency go.
- **Three layers of tests:** store unit tests (pure Rust), JS runtime tests (runtime + seeded store, no HTTP), HTTP tests (`init_service`). Plus error-path tests (T11) and sample fixtures (D7).
- **Parallel-safe by construction.** `cargo test`'s default multi-threading is fine: each test owns its store, and multiple V8 isolates on different threads are supported (deno_core initializes the platform once).
- **No `thread::sleep`, no `MAP.clear()`, no ordering assumptions between tests.**

### CI pipeline (what a modern Rust CI includes)

Table stakes (T15):
- Triggers: `push` + `pull_request`. `permissions: contents: read`. `concurrency: { group: ${{ github.workflow }}-${{ github.ref }}, cancel-in-progress: true }`.
- `actions/checkout` at a current major (v5+. v7 appears to be the latest; MEDIUM confidence, so check at implementation time).
- `dtolnay/rust-toolchain` with **the same toolchain as `rust-toolchain.toml`** (1.99) and `components: rustfmt, clippy`. A mismatch means rustup silently switches to the toml's toolchain.
- `Swatinem/rust-cache@v2`, which caches `target/` including the downloaded V8 static lib (~100+ MB). Without it every run re-downloads V8.
- Jobs (no `continue-on-error`):
  1. `fmt`: `cargo fmt --all --check` (no cache needed, fast fail).
  2. `clippy`: `cargo clippy --all-targets --locked -- -D warnings` (this subsumes `cargo check`; keep a separate `check` step only if the brief is read literally).
  3. `test`: `cargo test --locked`.
- Use `-D warnings` on the clippy command line, **not** `RUSTFLAGS`, which would invalidate the cache between jobs.
- No `SERVER.*` env in the workflow (proves test isolation).

Differentiators (D9): Docker build job, `cargo-deny`, Dependabot (cargo + actions), an `ubuntu-24.04-arm` job. Anti-features: MSRV job, beta/nightly/OS matrix, coverage.

### Docker (best practice for a V8-linked binary)

- **Builder:** an official `rust:1.99-slim-trixie` (or a matching `lukemathwalker/cargo-chef:*-rust-1.99*` image). Stages: `chef` (install cargo-chef once) → `planner` (`cargo chef prepare`) → `builder` (`cargo chef cook --release --locked`, then `COPY . .` and `cargo build --release --locked`). The cook layer caches deps **including the V8 prebuilt download**, which is the expensive part.
- **Runtime:** `gcr.io/distroless/cc-debian13:nonroot` (glibc + libgcc; the only supported distroless cc track, verified) or `debian:trixie-slim`. deno_core's default `v8_use_custom_libcxx` feature statically links V8's own libc++, so **no libstdc++ is needed**, only glibc, libgcc_s, libm and libpthread. **Builder and runtime must use the same Debian release** (glibc compatibility). No CA certs and no ICU data are needed (no outbound TLS, and `include_icu_data` stays off).
- **Config:** `ENV SERVER.HOST=0.0.0.0 SERVER.PORT=8080` (Docker accepts dotted env names), `EXPOSE 8080`, exec-form `ENTRYPOINT ["/usr/local/bin/rs3"]` so SIGTERM reaches actix for graceful shutdown. Never `COPY .env` into the image.
- **`.dockerignore`:** `target/`, `.git/`, `.env`, `.planning/`, `experiments/`, `aws/`, `docker_rs3/`.
- Drop the old `ARG DISTRO`/`FROM ${DISTRO}` pattern, or give it a default. As written it fails without `--build-arg`.

---

## Feature Dependencies

```
Toolchain 1.99 + edition 2024 + released deps
    └──requires──> deno_core op2 port (T7) ──requires──> Store injected via OpState (T9)
    └──requires──> actix-web 4.x final port ──requires──> Store/Config via web::Data (T9)

AppError → status mapping (T2)
    └──required by──> 404s (T3), insert validation (T4), JS error surfacing (T5), startup errors (T13)

Store layer with owned-value API (T9)
    └──enables──> isolated tests (T10) ──enables──> error-path tests (T11), op tests (T12)
    └──enables──> sorted/escaped /list (D4)

Config injection (T9) ──enables──> deterministic /ip test (T10) ──enables──> CI without SERVER.* env (T15)

Op port (T7) ──requires──> JS artefact migration (T8) ──feeds──> README op table (T17), sample fixtures (D7)

web::block execution (D1) ──enables──> timeout watchdog (D1)
OpState depth counter (D2) ──requires──> runtime factory taking (Store, depth)

Working build + tests (all T*) ──required by──> CI gating (T15), Docker (T16)

rs3 JS facade (D3) ──enhances──> op mapping (T7); keep both
Double-encoded envelope (preserved) ──conflicts──> "return real JSON" (anti-feature)
Strict 422 on non-string values (T4) ──conflicts──> value coercion (anti-feature)
web::Json extractor (anti-feature) ──conflicts──> 400/422 split + curl -d compatibility
```

### Dependency Notes

- **Store injection must come before or alongside the deno_core port.** The new ops get their data from `OpState`, so porting ops against the old global `MAP` and then re-porting would be wasted work. Recommended order: Store + AppError layer → op port → handlers → tests → CI/Docker/docs.
- **Error mapping (T2) must precede error-path tests (T11)**, and both are needed before clippy `-D warnings` with `unwrap_used` lints can gate CI.
- **Config injection unblocks CI simplification:** once the `/ip` test stops reading the env, the workflow's `SERVER.*` env block can go, which is a visible signal that isolation works.
- **The PATCH handler must clone the value and release any store lock/guard before running JS.** The script may call `op_rs3_insert`/`op_rs3_delete` on the same key. Holding a `DashMap` `Ref` or an `RwLock` read guard across execution is a guaranteed deadlock. Hence "owned values only" in the Store API.

## MVP Definition

### Launch With (milestone done)

- [ ] T1–T6: routes preserved, typed errors, 404/400/422/413 mapping, atomic insert, JS errors → 422, `String(result)` semantics
- [ ] T7–T8: five `op_rs3_*` ops via `#[op2]` + `extension!`, all JS artefacts (`.js`, `.txt`, `.http`, `example.js`) migrated
- [ ] T9–T12: injected `Store`/`Config`, isolated deterministic tests, error-path + op-level tests
- [ ] T13–T14: clean startup errors, dotted env config still works
- [ ] T15–T16: gating CI (fmt/clippy/test, cache, `--locked`), cargo-chef Dockerfile with a glibc slim runtime + `.dockerignore`
- [ ] T17–T18: README + documented behaviour changes and op mapping
- [ ] D4 escaping part of `/list` (correctness, effectively table stakes)

### Add After Validation (same milestone if time allows)

- [ ] D1 `web::Block` + timeout: first thing to add, because it removes the "one PATCH hangs a worker" failure mode
- [ ] D2 recursion limit: tiny, prevents a process abort
- [ ] D3 `rs3` JS facade: groundwork for the TypeScript milestone
- [ ] D7 sample fixtures, D8 release profile, D10 non-root, D5 logging

### Future Consideration (later milestones)

- [ ] Versioned API with real JSON bodies (`/v2`) and no double encoding
- [ ] Promise/async script results via the event loop (needed for TS/async ops anyway)
- [ ] Store trait + git-backed backend
- [ ] V8 startup snapshot for faster per-request runtimes

## Feature Prioritization Matrix

| Feature | User Value | Implementation Cost | Priority |
|---------|------------|---------------------|----------|
| T1 route/body preservation | HIGH | LOW | P1 |
| T2–T5 error mapping, 404, atomic insert, JS 422 | HIGH | MEDIUM | P1 |
| T6 PATCH result semantics | HIGH | LOW | P1 |
| T7 op2 port | HIGH | MEDIUM | P1 |
| T8 JS artefact migration | MEDIUM | LOW | P1 |
| T9 injected Store/Config | HIGH | MEDIUM | P1 |
| T10–T12 tests | HIGH | LOW–MEDIUM | P1 |
| T13–T14 config/startup | MEDIUM | LOW | P1 |
| T15 CI | HIGH | LOW | P1 |
| T16 Docker | MEDIUM | LOW–MEDIUM | P1 |
| T17–T18 docs | MEDIUM | LOW | P1 |
| D4 escaped + sorted `/list` | MEDIUM | LOW | P1 (escaping) / P2 (sorting) |
| D1 off-worker execution + timeout | HIGH | MEDIUM | P2 |
| D2 recursion limit | MEDIUM | LOW | P2 |
| D3 `rs3` JS facade | MEDIUM | LOW | P2 |
| D7 sample fixtures | MEDIUM | LOW | P2 |
| D5, D6, D8, D10 | LOW–MEDIUM | LOW | P2 |
| D9 CI extras | LOW–MEDIUM | LOW | P3 |
| D11 RFC 9457 errors | LOW | LOW | P3 |
| D12 lib+bin split | LOW | LOW | P3 |

## Reference Patterns

| Concern | Common practice elsewhere | rs3 approach |
|---------|---------------------------|--------------|
| Missing resource | REST APIs: 404 with an error object | 404 + JSON error |
| Malformed vs invalid payload | GitHub/Rails APIs: 400 for parse errors, 422 for validation errors. actix's `web::Json` uses 400 for everything except 413/415-like overflow cases | 400 for syntax, 422 for shape/type (via `serde_json::Error::classify`) |
| User-code failure in an execution API | FaaS platforms flag function errors separately from platform errors (e.g. a Lambda invoke returns a function-error marker; platform faults are 5xx) | 422 for script faults, 500 reserved for host faults |
| Embedding deno_core | deno_core's `eval_js_value` example: `execute_script` → `deno_core::scope!` → `v8::Local::new` → convert | Same, with `to_rust_string_lossy` to preserve `String(result)` semantics |
| State in ops | deno_core `extension!(..., options = {..}, state = \|state, options\| state.put(..))` + `OpState::borrow` | `Store` put into `OpState` per runtime |
| Rust service images | cargo-chef + distroless cc / debian-slim, same distro in both stages | Same, Debian 13 (trixie) |

## Sources

- Local code: `/home/t3code/rs3/Projects/rs3/src/{main.rs,bindings/mod.rs,services/mod.rs,config/mod.rs,example.js}`, `/home/t3code/rs3/Projects/rs3/requests/*` (HIGH)
- deno_core 0.412.0 crate source (crates.io, latest stable as of 2026-09-16): `Cargo.toml` (serde_json `preserve_order`, `deno_error = "=0.7.1"`, default `v8_use_custom_libcxx`, `deno_v8` facade), `runtime/jsruntime.rs` (`execute_script` signature, the `scope!` macro), `01_core.js` (`Deno.core.ops`, `Deno.core.print`), `examples/eval_js_value.rs`, `examples/op2.rs`, `runtime/tests/misc.rs` (`terminate_execution`) (HIGH)
- deno_error 0.7.1 README: `JsErrorBox::generic`/`type_error`, `#[class(...)]` (HIGH)
- config 0.15.27 crate source `src/env.rs` / `src/source.rs`: key lowercasing, path-parsed keys, `Environment::source(Some(map))` for tests (HIGH)
- actix-web `actix-web/src/error/mod.rs` (master): `JsonPayloadError` status mapping (413 for overflow, 400 otherwise, including content-type) (HIGH)
- crates.io API: actix-web 4.17.0, deno_core 0.412.0, config 0.15.27, dotenvy 0.15.7, dashmap 6.2.1, papaya 0.2.5, serde_json 1.0.154, cargo-chef 0.1.78 (HIGH)
- GoogleContainerTools/distroless README: `cc-debian13` is the supported cc track with `nonroot` tags (MEDIUM-HIGH)
- actions/checkout releases page: v7 latest (MEDIUM; confirm at implementation time)
- RFC 9110 §15.5.21 (422 Unprocessable Content) (HIGH, standard)

---
*Feature research for: rs3 toolchain modernization (actix-web + embedded deno_core KV store)*
*Researched: 2026-10-20*
