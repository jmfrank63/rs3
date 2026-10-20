# Phase 2: HTTP API Port & Error Handling - Research

**Researched:** 2026-10-20
**Domain:** actix-web 4.17 handler/state/error wiring, serde_json codec, dotenvy 0.15.7 config loading, clippy lint gating (Rust 1.99.1, edition 2024)
**Confidence:** HIGH. Every API below was read in `~/.cargo/registry` at the exact `Cargo.lock` versions. The critical shapes were also compiled, tested, clippy-linted and run live in a throwaway spike crate (deleted afterwards) on this host.

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

#### Error contract (status codes and body)
- **D-01:** Every error response is a real JSON object (not a double-encoded string) with content type `application/json`: `{"error": "<machine code>", "message": "<human-readable detail>"}`. Codes used in this phase: `not_found`, `malformed_json`, `invalid_input`, `not_implemented`, `internal`. Phase 3 and Phase 5 add `js_error`, `js_depth_limit` and `js_timeout` in the same shape. — **Reversibility:** reversible (no client depends on error bodies; errors used to be panics)
- **D-02:** Errors are typed per layer. `StoreError` stays in `store`. A new `http::error::ApiError` (thiserror) implements `actix_web::ResponseError` (status code + JSON body) and has `From<StoreError>`. Handlers return `Result<HttpResponse, ApiError>`. Status map: `NotFound` → 404, malformed JSON → 400, wrong shape / non-string value → 422, PATCH-not-yet → 501, host fault → 500.
- **D-03:** `POST /insert` reads the raw body (`web::Bytes`, not `web::Json` — REQUIREMENTS out-of-scope). Invalid UTF-8, an empty body, or JSON that fails to parse (serde_json syntax/EOF/IO) → **400**. Valid JSON that is not an object, or an object with any non-string value → **422**, and the message names the offending key. Validation completes before any write, so nothing is inserted on either error. `{}` → 200 with body `"{}"` (no-op). The codec must distinguish malformed from wrong-shape input, e.g. a `StoreError::MalformedJson` variant (400) next to `InvalidInput` (422); exact naming is at Claude's discretion.
- **D-04:** `GET /key/{key}` and `DELETE /key/{key}` on a missing key → **404** JSON error, also on an empty store (the `"[]"` quirk and the is-empty-then-get race are removed: a single `Store::get` / `Store::remove` call decides). `PATCH /key/{key}` on a missing key → **404** now (ROADMAP SC-2). An existing key still answers **501** `not_implemented` until Phase 3.
- **D-05:** Request bodies over actix's default 256 KiB payload limit get actix's built-in **413**. This is documented (README in Phase 6), not converted to JSON.

#### `/list` output and escaping
- **D-06:** `GET /list` keeps the legacy envelope and shape: a JSON string containing `[{"k" : "v"},{"k2" : "v2"}]`. Keep the legacy `" : "` spacing and `,` separators, but serialise every key and value with serde_json string escaping. Output is byte-identical to the legacy output for any data that needs no escaping, and the inner text is always valid JSON (a value containing `"` or `\` round-trips). — **Reversibility:** reversible
- **D-07:** Entries stay sorted by key (Rust `String` byte order). This already shipped in Phase 1 (review IN-04), so Phase 2 golden tests must not treat sorting as new.
- **D-08:** An empty store still answers `"[]"`.
- **D-09:** Drop the legacy `.replace(",]", "]")` quirk, since it corrupted any value containing `,]`. Replace the `format_list_keeps_legacy_replace_quirk` test with escaping and round-trip tests: parse the inner text back with serde_json and compare it with the stored pairs. `/list` and Phase 3's `op_rs3_list` keep sharing this one codec function.

#### State injection and test harness
- **D-10:** Introduce `AppState { store: Store, settings: Arc<Settings> }` (cheap `Clone`; Phase 3 adds a `js` executor field). `http::configure(state: AppState)` returns a closure that registers `web::Data<AppState>` (created once and cloned) plus every route. `main` and the tests call the same helper. Exact signature (`impl Fn(&mut web::ServiceConfig) + Clone` or similar) is at Claude's discretion; it must be callable once per actix worker.
- **D-11:** `main` loads `Settings` and creates the `Store` and `AppState` exactly once, outside the `HttpServer::new` factory closure. The temporary `static STORE: LazyLock<Store>` in `http::kv` and its `TODO(phase-2)` are deleted, and no global map remains anywhere (ARCH-02). `GET /ip` reads the host from the injected settings and never re-reads the environment (ARCH-03). A test builds two separate `init_service` apps from one `AppState` (simulating two workers) and checks that a key inserted through one is visible through the other.
- **D-12:** Each test builds its own `AppState` in code (fresh `Store::new()` + `Settings::from_map(...)`), with no `SERVER.*` environment variables, no `thread::sleep` and no shared global. The suite must pass repeatedly under default parallel `cargo test`. The two parked PATCH tests (`#[ignore]` until Phase 3) are re-pointed at a per-test `AppState` now, so that they are not flaky when re-enabled (review IN-05).
- **D-13:** Turn on `[lints.clippy] unwrap_used = "deny"` and `expect_used = "deny"` in `Cargo.toml` for non-test code. Add a `clippy.toml` with `allow-unwrap-in-tests = true` and `allow-expect-in-tests = true` so tests keep using them. **No `#[allow(...)]` / `#[cfg_attr(..., allow(...))]` attributes anywhere** (operator: no gate-greening by suppression; Phase 4 adds a CI grep for it). The handler insert path becomes unwrap-free (review IN-03: an infallible write after validation, or `insert_many` taking validated pairs; at Claude's discretion). Document that concurrent readers may see a partially written batch.

#### Config loading hardening (operator directive: review WR-01 / WR-02)
- **D-14 (WR-02):** `Settings::load()` must not mutate the process environment. Read the dotenv file with `dotenvy::dotenv_iter()`, which uses the same lookup as before (current directory, then its ancestors), and collect its pairs into a map. Overlay the real process environment on top: process env wins, which matches dotenvy's old "never override" semantics. Feed the result through the existing `Settings::from_map` builder path. Read the env with `std::env::vars_os()` and skip non-UTF-8 entries rather than panicking. With this change, `load()` is sound regardless of thread count, and the ordering comment in `main` stops being load-bearing.
- **D-15 (WR-01):** A missing dotenv file is ignored, but any other dotenv error (parse error at line N, unreadable file) becomes a startup error: `error: invalid configuration: .env: <detail>`, exit code 1. Because the file is collected into a map before anything is applied, a malformed file is never half-loaded. Add unit tests for the merge and error logic that do not touch the real process env or cwd. For example, factor the merge into a pure function over a dotenv iterator/reader plus an env map, then test it with in-memory content.
- **D-16 (review IN-07):** Only keys whose lowercased name starts with `server.` enter the config map, so an unrelated `SERVER` or other variable can no longer collide with the `server` table. The accepted names do not change (still dotted `SERVER.HOST` / `SERVER.PORT`), and there are still no defaults and no `SERVER__HOST` alias (OPS-02, v2).
- **D-17 (review IN-06):** Print the `Starting Http server at host address: ...` banner only after `bind` succeeds and before `run`, so a bind failure prints only the `error:` line.

#### Golden / success tests
- **D-18:** Carry over golden body-byte tests for `GET /` (`"Welcome to rs3"`), `GET /ip` (`"{'IP' : '<host>'}"`), `POST /insert` (echo of the object in input order), `GET /key/{key}` (`"<value>"`) and `DELETE /key/{key}` (`"<old value>"`), all through `configure(state)`. Add `/list` golden and escaping tests and an error-path test for every new status (404 GET/DELETE/PATCH, including the empty store; 400 malformed / empty / invalid UTF-8; 422 non-object and non-string value, with the store unchanged afterwards).

### Claude's Discretion
- Exact module and file layout (`http/error.rs`, `http/state.rs` or `state.rs` at crate root), helper and test names, and error message wording beyond the fixed `error` codes.
- Whether `insert_many` changes signature or an `insert_all` is added (D-13).
- Whether the PATCH 501/404 logic lives in `http/mod.rs` or moves into `http/kv.rs`.
- Commit granularity within plans (TDD RED/GREEN pairs are welcome, as in Phase 1).

### Deferred Ideas (OUT OF SCOPE)
- RFC 9457 `application/problem+json` error bodies: v2 (APIV).
- JSON body for actix's built-in 413 and for unknown-route 404s: v2.
- `SERVER__HOST` alias and config defaults: OPS-02 (v2).
- Request logging / tracing: OPS-01 (v2).
- Versioned API without the double-encoded string envelope: APIV-01 (v2).
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| HTTP-01 | `GET /` returns 200 with body `"Welcome to rs3"` | Unchanged handler. Golden test goes through `configure(state)` (Pattern 2). |
| HTTP-02 | `GET /ip` returns 200 with the configured host in the existing body format | Handler reads `state.settings.server.host` from `web::Data<AppState>` (Pattern 2). Golden `"{'IP' : 'test-host'}"`. |
| HTTP-03 | `POST /insert` inserts every pair and echoes the object in the existing body format | `web::Bytes` → `str::from_utf8` → `parse_insert_body` → `insert_all` (Pattern 4). Echo through the existing `format_insert_echo` (byte-identical, `preserve_order`). Spike: `{"k":"v w"}` → `"{\"k\":\"v w\"}"`. |
| HTTP-04 | `GET /list` returns all entries in the existing shape, correctly JSON-escaped | `format!("{{{} : {}}}", Value::from(k), Value::from(v))` codec (Pattern 5). Spike round-trip with `"`, `\`, `,]`, `\n`, `\t`, non-ASCII passed. |
| HTTP-05 | `GET /key/{key}` returns the value for an existing key | A single `store.get(&key)?` call. Missing key maps through `From<StoreError>` to 404. |
| HTTP-06 | `DELETE /key/{key}` removes an existing key and returns its old value | A single `store.remove(&key)?` call. |
| HTTP-08 | Handlers use the current actix-web 4.x API | `#[actix_web::test]`, `atest::init_service`/`call_service`/`read_body`/`call_and_read_body`, `Result<HttpResponse, ApiError>` returns. All compiled in the spike. |
| ROBU-01 | GET/DELETE/PATCH on a missing key → 404 JSON (no `"[]"`) | `ApiError` + `ResponseError` (Pattern 3). Spike: 404, `content-type: application/json`, `{"error":"not_found","message":"key not found: nope"}`. |
| ROBU-02 | Malformed JSON → 400; non-object / non-string → 422; nothing inserted | `serde_json::Error::classify()` plus a shape check before any write (Pattern 4). Spike verified 12 inputs and the HTTP statuses. |
| ARCH-02 | Store created once in `main`, injected via `web::Data`; no global map | `AppState` built before `HttpServer::new`. `configure(state)` captures one `web::Data<AppState>` and clones it per worker (Pattern 2). Spike: two `init_service` apps share state, and a 4-worker live server returned 8/8 hits. |
| ARCH-03 | Config loaded once at startup and injected; handlers do not re-read env | `Arc<Settings>` in `AppState`. `Settings::load()` is called only in `main` (grep gate). |
| TEST-01 | Isolated store per test, no `thread::sleep`, deterministic under parallel `cargo test` | A per-test `test_state()` helper. No statics. Repeat-run gate. |
| TEST-02 | Tests do not depend on process env | `Settings::from_map` in tests. The pure `merge_dotenv_env` function is tested with `dotenvy::from_read_iter(&[u8])` (Pattern 6). Gate: `env -i` cargo test. |
</phase_requirements>

## Project Constraints (from CLAUDE.md)

- Toolchain is Rust **1.99.1**, edition 2024 (`rust-toolchain.toml` pinned; host has cargo 1.99.1 and clippy 0.1.99). [VERIFIED: `cargo --version` / `cargo clippy --version` this session]
- Released crates.io dependencies only (no git, no beta). This phase adds **no** dependencies.
- Single binary crate with modular store / JS runtime / HTTP layers and **injected state**.
- Keep the same HTTP routes and semantics, and keep response bodies compatible where tests assert them.
- Work on branch `modernize-toolchain` and commit per plan with clear messages. Git identity comes from env vars (`GIT_AUTHOR_NAME="Johannes Maria Frank"` etc.). Never modify git config, never `git push`, and end commits with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- `target/` stays inside the project dir. Do **not** set `CARGO_TARGET_DIR` to `/tmp` (about 96% full).
- Use `deno_core::v8` and never a direct `v8` dependency (irrelevant here, but do not add one).
- `use actix_web::test` shadows `#[test]`, so import it as `test as atest` (the current code already does this).
- Do not use `actix_web::body::Body`. Compare bodies against `Bytes` / `&[u8]`.
- Use `actix-rt`/`tokio` only through `#[actix_web::main]` / `#[actix_web::test]`, never as direct deps.
- No `lazy_static`. `LazyLock` only for genuinely process-wide immutable things, and **the store must not be global**.
- GSD workflow: edits happen inside `/gsd-execute-phase`.
- `#![forbid(unsafe_code)]` plus `[lints.rust] unsafe_code = "forbid"` stay in force [VERIFIED: src/lib.rs:1, Cargo.toml:23-24].
- The TOOL-03 gate (`cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`) must pass at the end of every plan (CONTEXT specifics).

## Summary

This phase is mostly wiring. All crates are already in `Cargo.lock` at the right versions (actix-web 4.17.0, actix-http 3.19.0, serde_json 1.0.154 with `preserve_order`, dotenvy 0.15.7, config 0.15.27, thiserror 2.0.21, papaya 0.2.5), so **no `cargo add` is needed** [VERIFIED: Cargo.lock grep this session]. A throwaway spike crate built against a copy of the project's `Cargo.lock` (offline, Rust 1.99.1) compiled and passed every load-bearing shape. Tests, clippy `-D warnings` with the D-13 lints and a live 4-worker server smoke all ran: an `ApiError` with a `ResponseError` JSON body, a `configure(state) -> impl Fn(&mut ServiceConfig) + Clone + Send + 'static` closure shared by `HttpServer::new` and `init_service`, a `web::Bytes` insert path with a 400/422 split through `serde_json::Error::classify()`, an escaped `/list` codec, a dotenvy `dotenv_iter`/`from_read_iter` merge, and the 413 default.

The spike turned up five non-obvious facts the planner must design around:
1. **Iterating `dotenvy::Iter` yourself changes two semantics compared with `dotenv()`.** Duplicate keys become last-wins unless you use `entry().or_insert`, and a UTF-8 BOM is no longer stripped, so a BOM-prefixed `.env` becomes a parse error.
2. **dotenvy's `LineParse` error reports a character index within the line, not a line number.** It also echoes the raw line text, and for an unbalanced quote that is the entire rest of the file.
3. **Parsing into `serde_json::Value` never produces `Category::Data`.** All 422s come from the shape check, so `classify()` only separates "syntax/EOF/IO → 400" from the defensive "Data → 422".
4. **`clippy.toml` is mandatory the moment `[lints.clippy] unwrap_used/expect_used = "deny"` lands.** Without it, every `unwrap`/`expect`/`unwrap_err` in `#[actix_web::test]` and `#[cfg(test)]` code fails `-D warnings` (14 errors in the spike). With it, test functions, `#[cfg(test)] mod` helpers and `#[cfg(test)] fn` helpers are all exempt, and non-test code is still denied.
5. **actix's 413 is `text/plain` "payload reached size limit".** It is not JSON, which is correct per D-05, and tests must not assert a JSON body for it.

**Primary recommendation:** Build `AppState` once in `main`. Have `http::configure(state)` create one `web::Data<AppState>` and return a `Clone` `Fn` closure that registers it plus all routes. Make handlers return `Result<HttpResponse, ApiError>`, where `ApiError` wraps `StoreError` and owns the status, code and JSON body mapping. Do all parsing and validation before a single infallible `Store::insert_all`. Make `Settings::load()` a thin shell over a pure, unit-tested `merge_dotenv_env(dotenv_iter, env_vars)` function.

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| Route table, extractors, status/JSON error mapping | API (`src/http/`) | — | Only the HTTP layer knows about actix. `ApiError: ResponseError` lives here (D-02). |
| Insert body codec (parse, 400/422 classification), echo, `/list` text | Store codec (`src/store/legacy.rs`) | API (UTF-8 decode of `Bytes`) | Shared with Phase 3 ops (D-09), so it must stay actix-free. ARCH-01 gate: `! grep -rnE 'actix_web\|deno_core' src/store/`. |
| Key/value persistence and batch write | Store (`src/store/mod.rs`) | — | papaya map behind owned-value methods. Gets the infallible `insert_all`. |
| Config load (dotenv file plus env merge, `server.` filter) | Config (`src/config.rs`) | `main` (calls `load()` once) | Pure merge function, so tests never touch the process env (D-14..D-16). |
| Shared-state wiring (`AppState`) | Crate root (`src/state.rs`) or `src/http/state.rs` | `main` constructs it | Created once outside the worker factory (D-11). Phase 3 adds a `js` field. |
| Startup banner, bind and exit codes | Binary (`src/main.rs`) | — | D-17 ordering. Bind failure gives exit 1 with only the `error:` line. |
| Payload size limit (413) | actix framework | — | Default `PayloadConfig` of 262_144 bytes. No code needed (D-05). |

## Standard Stack

### Core (all already in Cargo.toml / Cargo.lock; no installs)

| Library | Locked Version | Purpose in this phase | Why Standard |
|---------|---------------|-----------------------|--------------|
| actix-web | 4.17.0 | `ResponseError`, `web::Bytes`, `web::Data`, `ServiceConfig`, test utilities | Project stack [VERIFIED: Cargo.lock; sources read] |
| actix-http | 3.19.0 (transitive) | `PayloadError::Overflow` → 413 | [VERIFIED: actix-web-4.17.0/src/error/response_error.rs:119-126] |
| serde_json | 1.0.154, `preserve_order` | Parse the insert body, `Error::classify()`, string escaping through `Value` Display | [VERIFIED: Cargo.toml:20; serde_json-1.0.154/src/error.rs:54-81] |
| thiserror | 2.0.21 | `ApiError`, `StoreError` derives (`#[error(transparent)]`, `#[from]`) | [VERIFIED: compiled in spike] |
| dotenvy | 0.15.7 | `dotenv_iter()`, `from_read_iter()`, `Error::not_found()` | [VERIFIED: dotenvy-0.15.7/src/lib.rs:303,370; errors.rs:10-22] |
| config | 0.15.27 (`default-features = false`) | Existing `Settings::from_map` → `Environment::source(Some(map))` | [VERIFIED: config-0.15.27/src/env.rs:227-230] |
| papaya | 0.2.5 | Existing `Store` backend | [VERIFIED: Cargo.lock] |

### Supporting
None. No new crates.

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| `web::Bytes` + manual parse | `web::Json<Map>` | **Forbidden** (REQUIREMENTS Out of Scope). It needs a JSON content type and collapses 400/422. |
| `web::Bytes` | `String` extractor (current code) | `String` returns a **text/plain 400 "Can not decode body"** on invalid UTF-8 and rejects unknown `charset=` params, so neither error is JSON. `Bytes` accepts any content type and charset [VERIFIED: actix-web-4.17.0/src/types/payload.rs:209-230, actix-http-3.19.0/src/http_message.rs:61-75; spike: `charset=bogus` → 200 with Bytes]. |
| `Value::from(s)` Display for escaping | `serde_json::to_string(&s)?` | `to_string` returns `Result`, which forces `format_list` to be fallible or to use an `unwrap` (denied). `Value` Display is infallible from our side and produces identical bytes (spike asserted equality). |
| Wrap `StoreError` inside `ApiError` | A flat `ApiError` with a hand-written `From<StoreError>` | Both compile. Wrapping (`#[error(transparent)] Store(#[from] StoreError)`) makes the status/code `match` exhaustive over `StoreError`, so a new store variant forces an HTTP mapping decision. Recommended. |

**Installation:** none.

**Version verification:** versions taken from `Cargo.lock` (authoritative for this build) and the sources read under `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/`.

## Package Legitimacy Audit

This phase installs **no** external packages. All crates used are already locked and were audited in Phase 1.

| Package | Registry | Age | Downloads | Source Repo | Verdict | Disposition |
|---------|----------|-----|-----------|-------------|---------|-------------|
| (none new) | — | — | — | — | — | — |

**Packages removed due to [SLOP] verdict:** none
**Packages flagged as suspicious [SUS]:** none

## Architecture Patterns

### System Architecture Diagram

```
startup (main thread)
  Settings::load()
     ├─ dotenvy::dotenv_iter()  ──► NotFound? → no file pairs
     │                          └─► other Err → ConfigError ".env: …" → "error: invalid configuration: …" exit 1
     ├─ merge_dotenv_env(file pairs [first-wins], env::vars_os() [UTF-8 only, overrides])
     ├─ retain keys where lowercase starts_with "server."
     └─ Settings::from_map(map) → config::Environment::source → try_deserialize
  rs3::js::init_platform()
  AppState { store: Store::new(), settings: Arc::new(settings) }
  cfg = http::configure(state)      // creates ONE web::Data<AppState>, returns Clone+Fn closure
  HttpServer::new(move || App::new().configure(cfg.clone())).bind(..)
     ├─ Err → "error: cannot bind …" exit 1 (no banner)
     └─ Ok  → println!("Starting Http server …") → run()

request (any of N workers; all hold clones of the same Data<AppState> → same Arc'd map)
  HTTP ─► actix router ─► extractor
            ├─ web::Bytes (> 262_144 B → 413 text/plain, handler not called)
            ├─ web::Path<String>
            └─ web::Data<AppState> (missing → 500 text/plain; impossible via configure)
        ─► handler -> Result<HttpResponse, ApiError>
            POST /insert: from_utf8 ─✗→ MalformedJson(400)
                          parse_insert_body ─✗→ MalformedJson(400) | InvalidInput(422)
                          format_insert_echo → store.insert_all(pairs) → 200 json(echo)
            GET/DELETE /key/{k}: store.get/remove ─✗→ NotFound(404)
            PATCH /key/{k}: store.get ─✗→ 404 ; else NotImplemented(501)
            GET /list: legacy::format_list(store.list()) → 200 json(text)
            GET /ip:   state.settings.server.host → 200
        ─► Err(ApiError) ─► ResponseError::error_response
                            → status + application/json {"error": code, "message": Display}
```

### Recommended Project Structure

```
src/
├── lib.rs            # pub mod config; pub mod http; pub mod js; pub mod store; pub mod state (+ pub use state::AppState)
├── main.rs           # load → init_platform → AppState → configure(state) → bind → banner → run
├── config.rs         # Settings::load (thin), pub(crate)/pub fn merge_dotenv_env (pure), from_map
├── state.rs          # AppState { store: Store, settings: Arc<Settings> } + AppState::new(settings)
├── http/
│   ├── mod.rs        # configure(state) closure, index, status(/ip), patch_key
│   ├── error.rs      # ApiError + ResponseError + ErrorBody
│   └── kv.rs         # insert, get_key, delete_key, list (no statics)
├── store/
│   ├── mod.rs        # Store (+ insert_all), StoreError { NotFound, MalformedJson, InvalidInput }
│   └── legacy.rs     # parse_insert_body (400/422 split), format_insert_echo, format_list (escaped)
└── js/mod.rs         # unchanged (init_platform)
clippy.toml           # allow-unwrap-in-tests = true, allow-expect-in-tests = true
```

`src/state.rs` at the crate root is recommended over `http/state.rs`. `main` constructs it, and Phase 3 adds a `js` executor field that should not make `js` look like it depends on `http`. Either placement passes the ARCH-01 gate (`store` must not import `actix_web`/`deno_core`). This is at Claude's discretion.

### Pattern 1: `ApiError` implementing `ResponseError` with a JSON body
**What:** One error type per layer. The HTTP layer owns status, machine code and JSON body.
**When to use:** Every handler that can fail returns `Result<HttpResponse, ApiError>`. Infallible handlers (`/`, `/ip`, `/list`) may keep `impl Responder` or `HttpResponse`.
**Verified facts:**
- `ResponseError: fmt::Debug + fmt::Display` with overridable `fn status_code(&self) -> StatusCode` and `fn error_response(&self) -> HttpResponse<BoxBody>`. The default `error_response` is **text/plain**, so it must be overridden for D-01 [VERIFIED: actix-web-4.17.0/src/error/response_error.rs:25-48].
- `impl<T: ResponseError + 'static> From<T> for Error` makes `Result<_, ApiError>` a valid handler return [VERIFIED: actix-web-4.17.0/src/error/error.rs:116].
- `HttpResponseBuilder::json` sets `Content-Type: application/json` when none is set [VERIFIED: actix-web-4.17.0/src/response/builder.rs:355-372].
- **Dead-code trap:** `Internal` is not constructed in Phase 2. It does not warn while `ApiError` is reachable as public API (`pub mod http` → `pub mod error` → `pub enum ApiError`). The spike's unused `Internal` passed `-D warnings` this way. If the module or type is made `pub(crate)`, the unused variant warns and the gate fails. **Do not** silence it with `#[allow(dead_code)]` (D-13). Keep it `pub`.

```rust
// src/http/error.rs. Shape compiled and tested in the spike (actix-web 4.17.0, thiserror 2.0.21).
use actix_web::{HttpResponse, ResponseError, http::StatusCode};
use crate::store::StoreError;

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("not implemented: {0}")]
    NotImplemented(String),
    #[error("internal error")]
    Internal,
}

#[derive(serde::Serialize)]
struct ErrorBody<'a> {
    error: &'a str,
    message: String,
}

impl ApiError {
    pub fn code(&self) -> &'static str {
        match self {
            ApiError::Store(StoreError::NotFound(_)) => "not_found",
            ApiError::Store(StoreError::MalformedJson(_)) => "malformed_json",
            ApiError::Store(StoreError::InvalidInput(_)) => "invalid_input",
            ApiError::NotImplemented(_) => "not_implemented",
            ApiError::Internal => "internal",
        }
    }
}

impl ResponseError for ApiError {
    fn status_code(&self) -> StatusCode {
        match self {
            ApiError::Store(StoreError::NotFound(_)) => StatusCode::NOT_FOUND,
            ApiError::Store(StoreError::MalformedJson(_)) => StatusCode::BAD_REQUEST,
            ApiError::Store(StoreError::InvalidInput(_)) => StatusCode::UNPROCESSABLE_ENTITY,
            ApiError::NotImplemented(_) => StatusCode::NOT_IMPLEMENTED,
            ApiError::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
    fn error_response(&self) -> HttpResponse {
        HttpResponse::build(self.status_code()).json(ErrorBody { error: self.code(), message: self.to_string() })
    }
}
```
Spike output: `404 ct=application/json body={"error":"not_found","message":"key not found: nope"}` [VERIFIED: spike run].

The current `StoreError` variants are exactly `NotFound(String)` and `InvalidInput(String)` [VERIFIED: src/store/mod.rs:11-17 — `#[error("key not found: {0}")] NotFound(String),` / `#[error("invalid input: {0}")] InvalidInput(String),`]. **`MalformedJson(String)` is new** and must be added in this phase (D-03, name at discretion).

### Pattern 2: `configure(state)` closure shared by `main` (per worker) and tests
**What:** `configure` creates the `web::Data<AppState>` **once** and returns a closure that is `Fn + Clone + Send + 'static`. `HttpServer::new`'s factory clones it per worker. Tests call `App::new().configure(configure(state.clone()))`, or clone one `cfg` into two apps to simulate two workers.
**Verified facts:**
- `App::configure<F: FnOnce(&mut ServiceConfig)>`. `ServiceConfig::app_data` is merged into the **app-level** extensions (`self.extensions.extend(cfg.app_data)`), so `cfg.app_data(data.clone())` inside configure is visible to every route [VERIFIED: actix-web-4.17.0/src/app.rs:191-201, src/config.rs:347-350].
- `HttpServer::new` requires `F: Fn() -> I + Send + Clone + 'static` [VERIFIED: actix-web-4.17.0/src/server.rs:97-98]. A `move` closure capturing a `Clone + Send` closure satisfies this.
- `web::Data<T>` extraction looks up `req.app_data::<Data<T>>()`. When it is missing, the handler gets a **text/plain 500** "Requested application data is not configured correctly" [VERIFIED: actix-web-4.17.0/src/data.rs:162-183]. The type must match exactly: `Data<AppState>` ≠ `Data<Arc<AppState>>`.

```rust
// src/http/mod.rs. Compiled and tested in the spike; the live 4-worker server returned the inserted key on 8/8 GETs.
pub fn configure(state: AppState) -> impl Fn(&mut web::ServiceConfig) + Clone + Send + 'static {
    let data = web::Data::new(state);            // created once
    move |cfg: &mut web::ServiceConfig| {
        cfg.app_data(data.clone())               // cloned per worker / per app
            .service(index)
            .service(status)
            .service(patch_key)
            .configure(kv::configure);
    }
}

// src/main.rs
let cfg = rs3::http::configure(state);
let server = match HttpServer::new(move || App::new().configure(cfg.clone())).bind((host.as_str(), port)) {
    Ok(s) => s,
    Err(e) => { eprintln!("error: cannot bind {host}:{port}: {e}"); return ExitCode::FAILURE; }
};
println!("Starting Http server at host address: {host}, with port: {port}!");   // D-17: after bind
```

```rust
// two "workers" sharing one AppState (D-11). Passed in the spike.
#[actix_web::test]
async fn insert_on_one_app_visible_on_another() {
    let state = test_state();
    let cfg = configure(state.clone());
    let a = atest::init_service(App::new().configure(cfg.clone())).await;
    let b = atest::init_service(App::new().configure(cfg)).await;
    let body = atest::call_and_read_body(&a,
        atest::TestRequest::post().uri("/insert").set_payload(r#"{"k":"v"}"#).to_request()).await;
    assert_eq!(body, web::Bytes::from_static(br#""{\"k\":\"v\"}""#));
    let resp = atest::call_service(&b, atest::TestRequest::get().uri("/key/k").to_request()).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(atest::read_body(resp).await, web::Bytes::from_static(br#""v""#));
    assert_eq!(state.store.get("k"), Ok("v".to_owned()));   // test holds the same Arc'd store
}
```

Note: `kv::configure` stays a plain `fn(&mut ServiceConfig)` that registers only routes. The Data is registered once by the outer closure.

### Pattern 3: Handlers on injected state

```rust
#[get("/key/{key}")]
pub async fn get_key(state: web::Data<AppState>, path: web::Path<String>) -> Result<HttpResponse, ApiError> {
    Ok(HttpResponse::Ok().json(state.store.get(&path.into_inner())?))   // NotFound → 404 via From
}

#[delete("/key/{key}")]
pub async fn delete_key(state: web::Data<AppState>, path: web::Path<String>) -> Result<HttpResponse, ApiError> {
    Ok(HttpResponse::Ok().json(state.store.remove(&path.into_inner())?))
}

#[patch("/key/{key}")]
pub async fn patch_key(state: web::Data<AppState>, path: web::Path<String>) -> Result<HttpResponse, ApiError> {
    state.store.get(&path.into_inner())?;                               // missing → 404 (D-04)
    Err(ApiError::NotImplemented("PATCH /key/{key} returns in Phase 3 (JavaScript runtime)".into()))
}
```
`Store::get`/`remove` already return `Result<String, StoreError>` with `NotFound(key)` [VERIFIED: src/store/mod.rs:34-40, 63-69].

### Pattern 4: `/insert` via raw `web::Bytes` with a 400/422 split before any write

**Verified facts:**
- `web::Bytes` extraction applies `PayloadConfig::from_req(req)`. It falls back to `DEFAULT_CONFIG` with `limit: DEFAULT_CONFIG_LIMIT`, where `const DEFAULT_CONFIG_LIMIT: usize = 262_144; // 2^18 bytes (~256kB)`, and `mimetype: None` (no content-type check) [VERIFIED: actix-web-4.17.0/src/types/payload.rs:161-178, 324-339].
- Over-limit gives `PayloadError::Overflow`, then `StatusCode::PAYLOAD_TOO_LARGE`, with the default text/plain body `payload reached size limit`. This happens for both a declared `Content-Length` and chunked bodies [VERIFIED: response_error.rs:119-126; payload.rs:367-437; spike live curl: 300 000-byte body → `413 Payload Too Large`, `content-type: text/plain; charset=utf-8`, also with `Transfer-Encoding: chunked`].
- `serde_json::Error::classify()` maps `ErrorCode::Message(_)` → `Data`, `Io` → `Io`, `EofWhileParsing*` → `Eof`, and every syntax code including `TrailingCharacters`, `TrailingComma`, `RecursionLimitExceeded`, `InvalidUnicodeCodePoint` and `NumberOutOfRange` → `Syntax` [VERIFIED: serde_json-1.0.154/src/error.rs:54-81, enum `Category { Io, Syntax, Data, Eof }` at 166-185].
- **Deserializing into `Value` cannot produce `Data`.** Every 422 therefore comes from the explicit shape check, which is also what lets the message name the offending key.

Spike classification results (`parse_insert_body` with the split) [VERIFIED: spike run]:

| Input | Result | HTTP |
|-------|--------|------|
| `""` | `MalformedJson("EOF while parsing a value at line 1 column 0")` | 400 |
| `"   "` | `MalformedJson("EOF while parsing a value at line 1 column 3")` | 400 |
| `{"a":` | `MalformedJson("EOF while parsing a value …")` | 400 |
| `{"a":"1"} trailing` | `MalformedJson("trailing characters …")` | 400 |
| `{"a":"1",}` | `MalformedJson("trailing comma …")` | 400 |
| `{bad` | `MalformedJson("key must be a string at line 1 column 2")` | 400 |
| bytes `FF FE` | UTF-8 decode error → `MalformedJson` | 400 |
| `"["` × 200 | `MalformedJson("recursion limit exceeded at line 1 column 128")` | 400 |
| `[1,2]`, `"x"`, `3`, `null` | `InvalidInput("insert body must be a JSON object")` | 422 |
| `{"a":1}`, `{"a":null}` | `InvalidInput("value for key a must be a string")` | 422 |
| `{}` | `Ok([])` → body `"{}"` | 200 |
| `{"a":"1","a":"2"}` | `Ok([("a","2")])` (duplicate: last value wins, at the first position) | 200 |

```rust
// src/store/legacy.rs
pub fn parse_insert_body(body: &str) -> Result<Vec<(String, String)>, StoreError> {
    let value: Value = serde_json::from_str(body).map_err(|e| match e.classify() {
        serde_json::error::Category::Data => StoreError::InvalidInput(e.to_string()), // defensive; unreachable for Value
        _ => StoreError::MalformedJson(e.to_string()),                                // Syntax | Eof | Io
    })?;
    let object = value.as_object()
        .ok_or_else(|| StoreError::InvalidInput("insert body must be a JSON object".into()))?;
    object.iter().map(|(k, v)| match v.as_str() {
        Some(s) => Ok((k.clone(), s.to_owned())),
        None => Err(StoreError::InvalidInput(format!("value for key {k} must be a string"))),
    }).collect()
}

// src/http/kv.rs
#[post("/insert")]
pub async fn insert(state: web::Data<AppState>, body: web::Bytes) -> Result<HttpResponse, ApiError> {
    let text = std::str::from_utf8(&body)
        .map_err(|e| StoreError::MalformedJson(format!("body is not valid UTF-8: {e}")))?;
    let entries = legacy::parse_insert_body(text)?;      // fully validated before any write
    let echo = legacy::format_insert_echo(&entries);
    state.store.insert_all(entries);                      // infallible
    Ok(HttpResponse::Ok().json(echo))
}

// src/store/mod.rs. Replaces insert_many (IN-03).
/// Write every pair; returns the count. Not atomic for concurrent readers:
/// a simultaneous `list()`/`get()` may observe a partially written batch.
pub fn insert_all<I: IntoIterator<Item = (String, String)>>(&self, entries: I) -> usize { … }
```
Recommendation (discretion): **replace** `insert_many` with `insert_all` and delete the two `insert_many_*` store tests. Validation now lives entirely in `parse_insert_body`, and Phase 3's `op_rs3_insert` follows the same parse-then-`insert_all` path. Add an HTTP test that a 422 body (`{"a":"1","b":2}`) leaves `a` absent. That carries the all-or-nothing guarantee.

### Pattern 5: Escaped `/list` codec (D-06..D-09)

serde_json escapes **only** `"` → `\"`, `\` → `\\`, and bytes `0x00..=0x1F` (`\b \t \n \f \r`, others `\u00XX`). `/`, DEL and all non-ASCII UTF-8 pass through unescaped [VERIFIED: serde_json-1.0.154/src/ser.rs:2147-2165 `ESCAPE` table — row 2: `__, __, QU, …` (0x22), row 5: `… BS, …` (0x5C), rows 0–1: `UU`/`BB`/`TT`/`NN`/`FF`/`RR`]. So the output is byte-identical to legacy for any data without those characters (D-06).

```rust
// src/store/legacy.rs. Spike asserted equality with a serde_json::to_string-based variant plus a full round-trip.
pub fn format_list(entries: &[(String, String)]) -> String {
    let items: Vec<String> = entries.iter()
        .map(|(k, v)| format!("{{{} : {}}}", Value::from(k.as_str()), Value::from(v.as_str())))
        .collect();
    format!("[{}]", items.join(","))      // empty → "[]" (D-08); no ",]" replace (D-09)
}
```
Spike output: `[{"a" : "1"},{"q" : "he said \"hi\" \\ back,] \n tab\t ü /"}]`. Parsing it back as `Vec<serde_json::Map<String, Value>>` reproduced the input pairs, and `[("a","1"),("b","2")]` → `[{"a" : "1"},{"b" : "2"}]` (legacy golden) [VERIFIED: spike run]. `Value` Display is infallible from the caller's side (`impl Display for Value`, serde_json-1.0.154/src/value/mod.rs:197-222), so no `unwrap` is needed.

Round-trip test shape for D-09:
```rust
let text = format_list(&pairs);
let parsed: Vec<serde_json::Map<String, serde_json::Value>> = serde_json::from_str(&text).expect("valid JSON");
let back: Vec<(String, String)> = parsed.into_iter()
    .flat_map(|m| m.into_iter().map(|(k, v)| (k, v.as_str().expect("string").to_owned())))
    .collect();
assert_eq!(back, pairs);
```
Also add an HTTP-level test. Insert a value containing `"` and `\` through `/insert`, `GET /list`, decode the outer JSON string (`serde_json::from_slice::<String>(&body)`), then parse the inner text as above.

### Pattern 6: Config load without env mutation (D-14..D-16)

**Verified dotenvy 0.15.7 facts:**
- `pub fn dotenv_iter() -> Result<iter::Iter<File>>` uses `Finder::new().find()`: `.env` in `env::current_dir()`, then parents. A missing file gives `Error::Io(NotFound "path not found")`. A metadata error other than NotFound (for example EACCES on stat) or a `File::open` failure gives `Error::Io(..)` [VERIFIED: dotenvy-0.15.7/src/lib.rs:370-373, src/find.rs:25-60].
- `pub fn from_read_iter<R: io::Read>(reader: R) -> Iter<R>` is the in-memory entry point for unit tests [VERIFIED: lib.rs:303].
- `pub enum Error { LineParse(String, usize), Io(io::Error), EnvVar(env::VarError) }` is `#[non_exhaustive]`. `pub fn not_found(&self) -> bool` is true only for `Io` with `ErrorKind::NotFound`. Display for `LineParse` is `"Error parsing line: '{}', error at line index: {}"` [VERIFIED: errors.rs:8-22, 36-46].
- **First-wins:** `Iter::load` only sets a key `if env::var(&key).is_err()`, so the first file occurrence wins and existing env wins [VERIFIED: iter.rs:29-38]. Collecting the iterator into a map is **last-wins**. Use `entry(k).or_insert(v)` to keep parity (spike: `SERVER.HOST=filehost` then `SERVER.HOST=second` → `filehost`).
- **BOM:** `remove_bom()` is private and called only from `load()`/`load_override()` [VERIFIED: iter.rs:29-30, 47-48, 58-66]. Keys must start with an ASCII letter or `_` [VERIFIED: parse.rs:76-81], so a BOM-prefixed file read through the iterator **fails**. Spike: `Err(".env: Error parsing line: '\u{feff}SERVER.HOST=bomhost', error at line index: 0")`. See Open Question 1.
- **"line N" is not available:** the `usize` in `LineParse` is a character position within the logical line. An unbalanced quote makes the reader swallow the rest of the file into one "line" (spike: the error text contained the following `X=1` line). See Open Question 2.
- `config` lowercases every source key (`let mut key = key.to_lowercase();`) [VERIFIED: config-0.15.27/src/env.rs:269]. So `SERVER.HOST` and `server.host` collide nondeterministically (HashMap order) if both exist. This is a pre-existing edge, unchanged by D-16. `ConfigError::Message(s)` displays as just `s` [VERIFIED: config-0.15.27/src/error.rs:98, 228], so `ConfigError::Message(format!(".env: {e}"))` prints `error: invalid configuration: .env: …` through the existing `main` arm (src/main.rs:11).

```rust
// src/config.rs. The merge logic was compiled and tested in the spike.
use std::ffi::OsString;

/// Pure merge: dotenv pairs (first occurrence wins) overlaid by process env (wins), UTF-8 only,
/// restricted to keys whose lowercase form starts with "server.".
pub fn merge_dotenv_env<I, E>(dotenv: Option<I>, env: E) -> Result<::config::Map<String, String>, ConfigError>
where
    I: IntoIterator<Item = dotenvy::Result<(String, String)>>,
    E: IntoIterator<Item = (OsString, OsString)>,
{
    let mut vars = ::config::Map::new();
    if let Some(items) = dotenv {
        for item in items {
            let (k, v) = item.map_err(|e| ConfigError::Message(format!(".env: {e}")))?;
            vars.entry(k).or_insert(v);
        }
    }
    for (k, v) in env {
        if let (Ok(k), Ok(v)) = (k.into_string(), v.into_string()) {
            vars.insert(k, v);
        }
    }
    vars.retain(|k, _| k.to_lowercase().starts_with("server."));
    Ok(vars)
}

pub fn load() -> Result<Self, ConfigError> {
    let file = match dotenvy::dotenv_iter() {
        Ok(iter) => Some(iter),
        Err(e) if e.not_found() => None,
        Err(e) => return Err(ConfigError::Message(format!(".env: {e}"))),
    };
    Self::from_map(merge_dotenv_env(file, std::env::vars_os())?)
}
```
Unit tests feed `Some(dotenvy::from_read_iter("SERVER.HOST=a\n…".as_bytes()))` and a `Vec<(OsString, OsString)>`. They never touch the process env or cwd. Cover these cases: env overrides file, first-wins in file, a malformed line → `Err` whose message starts with `.env:` (and nothing returned, so no half-load), `SERVER`/`OTHER` filtered out, `server.host` lowercase accepted, a non-UTF-8 `OsString` skipped (build with `std::os::unix::ffi::OsStringExt::from_vec(vec![0xff])`; this is safe code), and `None` file.

`std::env::vars()` **panics** on non-UTF-8 entries, which is why D-14 mandates `vars_os()` [ASSUMED: std docs; consistent with D-14].

### Anti-Patterns to Avoid
- **`web::Data::new(...)` inside the `HttpServer::new` closure.** It still works here, because `AppState` fields are `Arc`s, but it violates D-10's "created once" and invites Pitfall 7 (a per-worker store) when someone later adds a non-Arc field. Create the Data in `configure`, outside the factory.
- **Keeping `STORE.is_empty()` before `get`.** This is the IN-01 race: papaya `len()` is a relaxed sharded counter. Use one `get`.
- **`Settings::load()` or any `env::var` in `src/http/`.** This violates ARCH-03.
- **`#[allow(clippy::unwrap_used)]` / `#[allow(dead_code)]` to green the gate.** D-13 forbids it, and a Phase 4 CI grep will fail on it.
- **Asserting JSON for 413 or for unknown-route 404.** Both are actix defaults (text/plain or empty). Deferred to v2.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| JSON string escaping in `/list` | Manual `replace('"', "\\\"")` chains | `serde_json::Value::from(&str)` Display | Control characters, `\u` escapes and the exact serde_json escape set are already correct and round-trip-tested. |
| Distinguishing syntax vs shape errors | String matching on error messages | `serde_json::Error::classify()` + `as_object()`/`as_str()` | Stable enum API, not message parsing. |
| Body size limiting | Manual length checks | actix default `PayloadConfig` (262 144 B → 413) | Handles chunked bodies and Content-Length up front. |
| `.env` parsing | A custom KEY=VALUE splitter | `dotenvy::dotenv_iter()` / `from_read_iter()` | Quotes, escapes, `export`, comments and substitution. |
| Error → HTTP response | Per-handler `HttpResponse::NotFound()...` | `impl ResponseError for ApiError` + `?` | One mapping table. Handlers stay three lines. |
| Test isolation | Clearing a global map between tests | A fresh `AppState` per test | Parallel-safe by construction. |

**Key insight:** every behavioural edge in this phase (escaping, 413, UTF-8, error classification) is already implemented correctly in a locked dependency. The work is choosing the right extractor and the right API, not writing parsers.

## Common Pitfalls

### Pitfall 1: Collecting `dotenv_iter()` flips duplicate-key semantics
**What goes wrong:** `iter.collect::<Result<HashMap<_,_>,_>>()` (the shape in 01-REVIEW's WR-02 suggestion) makes the **last** duplicate win, while `dotenvy::dotenv()` applied the **first** [VERIFIED: iter.rs:29-38].
**How to avoid:** Use `vars.entry(k).or_insert(v)` for file pairs, then `insert` for env pairs (env wins). Add a unit test for this.
**Warning signs:** A `.env` with a repeated `SERVER.PORT` picks a different port after the change.

### Pitfall 2: BOM-prefixed `.env` turns into a startup error
**What goes wrong:** `dotenv()` stripped a UTF-8 BOM. The public iterator does not, so the first key fails `parse_key` [VERIFIED: spike].
**How to avoid:** Accept and document it (recommended, see Open Question 1). The failure is loud (`error: invalid configuration: .env: Error parsing line: '\u{feff}…', error at line index: 0`), not silent. Add a unit test that pins the behaviour.
**Warning signs:** A Windows-edited `.env` fails with "line index: 0".

### Pitfall 3: Promising "line N" in the `.env` error
**What goes wrong:** D-15's wording "parse error at line N" cannot be met literally. `LineParse(String, usize)` carries the line **text** and a character index [VERIFIED: errors.rs:11, 42].
**How to avoid:** Tests and smokes assert the prefix `error: invalid configuration: .env:` and exit code 1, not a line number.

### Pitfall 4: Turning on `unwrap_used`/`expect_used` without `clippy.toml` breaks every test
**What goes wrong:** `[lints.clippy]` applies to all targets. In the spike, without `clippy.toml`, `cargo clippy --all-targets -- -D warnings` produced 14 errors inside `#[actix_web::test]`, `#[test]` and `#[cfg(test)]` helpers. `unwrap_err()` is covered by `unwrap_used` too [VERIFIED: spike clippy 0.1.99].
**How to avoid:** Land `clippy.toml` (`allow-unwrap-in-tests = true`, `allow-expect-in-tests = true`) in the **same commit** as the `[lints.clippy]` table. With it, the spike showed test fns, `#[cfg(test)] mod` helpers and `#[cfg(test)] fn` helpers exempt, and a non-test `.expect()` still denied (in both the `lib` and `lib test` targets). The `#[actix_web::main]` / `#[actix_web::test]` macro expansions themselves do not trigger the lints [VERIFIED: spike].
**Warning signs:** Clippy errors pointing into `mod tests`.

### Pitfall 5: Remaining non-test `unwrap`s the lint will catch
**What goes wrong:** The current non-test code has three `unwrap()` calls in `src/http/kv.rs` (lines 24, 26, 40) and one in line 49 [VERIFIED: src/http/kv.rs read this session]. All four disappear with Patterns 3 and 4. Any new helper that uses `.unwrap()` or `.expect()` (for example on `serde_json::to_string` or `from_utf8`) fails the gate.
**How to avoid:** Use `?` with `From<StoreError>`, `Value` Display for formatting, and `map_err` for UTF-8.

### Pitfall 6: Existing tests that encode old semantics
- `test_patch_is_501` (src/http/mod.rs:108-120) PATCHes `/key/anything` on an empty store. After D-04 that is **404**. Insert the key first for the 501 test, and add a separate 404 test.
- `format_list_keeps_legacy_replace_quirk` (src/store/legacy.rs:73-76) must be **replaced** (D-09).
- `parse_rejects_malformed_json` expects `InvalidInput` (legacy.rs:94-97). It must expect `MalformedJson`.
- `insert_many_writes_all` / `insert_many_all_or_nothing` (store/mod.rs:128-149) go away or change with `insert_all`.
- The parked PATCH tests reference `crate::http::kv::STORE` (mod.rs:127, 137), which will no longer exist. They **must** be re-pointed to `test_state()` or the crate stops compiling (`#[ignore]` tests are still compiled).
- `test_ip_without_settings_is_500` (mod.rs:101-106) builds `App::new().service(status)` without data. It remains valid actix behaviour (text/plain 500) if `status` keeps the `web::Data<AppState>` extractor. Keep it, or drop it as redundant, since `configure` always registers the data.
- The kv tests use `App::new().service(insert)` without data. All must move to `App::new().configure(configure(test_state()))` (D-18 "all through `configure(state)`").

### Pitfall 7: `dead_code` on the unused `ApiError::Internal` variant
**What goes wrong:** If `http::error` or `ApiError` is not reachable as public API, the never-constructed `Internal` triggers `dead_code`, and `-D warnings` fails.
**How to avoid:** `pub mod error;` inside `pub mod http`, and `pub enum ApiError`. Never `#[allow]`.

### Pitfall 8: Smokes that inherit a developer `.env` or `SERVER.*`
**What goes wrong:** The binary walks up from cwd looking for `.env`. Dotted names cannot be set with bash `export`.
**How to avoid:** Follow the Phase 1 convention: `env -i PATH="$PATH" SERVER.HOST=127.0.0.1 SERVER.PORT=<port> target/debug/rs3`, wait with `curl --retry-connrefused` (no `sleep`), and use `env -i -C "$D"` with `D=$(mktemp -d -p /home/t3code)` for dotenv smokes (Phase 1 01-03-PLAN lines 177, 212).

### Pitfall 9: Partial batch visibility
**What goes wrong:** `insert_all` writes pairs one at a time under one papaya guard. That is not a transaction, so a concurrent `/list` can see part of a batch.
**How to avoid:** Documentation only (D-13). Put it in the `insert_all` doc comment.

## Code Examples

All examples are under Patterns 1–6 above. A test-state helper (one per test module, or one shared `#[cfg(test)] pub(crate) mod test_support`; both are clippy-exempt per the spike):

```rust
#[cfg(test)]
pub(crate) fn test_state() -> AppState {
    let map: ::config::Map<String, String> = [("SERVER.HOST", "test-host"), ("SERVER.PORT", "8080")]
        .into_iter().map(|(k, v)| (k.to_owned(), v.to_owned())).collect();
    AppState { store: Store::new(), settings: Arc::new(Settings::from_map(map).expect("valid test settings")) }
}
```

Error-body assertion helper:
```rust
async fn assert_json_error(resp: ServiceResponse, status: StatusCode, code: &str) {
    assert_eq!(resp.status(), status);
    assert_eq!(resp.headers().get("content-type").map(|v| v.as_bytes()), Some(&b"application/json"[..]));
    let body: serde_json::Value = serde_json::from_slice(&atest::read_body(resp).await).expect("JSON body");
    assert_eq!(body["error"], code);
    assert!(body["message"].is_string());
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `dotenvy::dotenv()` (mutates env via `set_var`) | `dotenv_iter()` → map → `Environment::source` | Edition 2024 made `set_var` unsafe | `load()` sound in any thread context (WR-02) |
| Global `LazyLock<Store>` | `web::Data<AppState>` from `configure(state)` | This phase | ARCH-02 |
| `String` body extractor + `unwrap` | `web::Bytes` + typed `ApiError` | This phase | JSON 400/422 instead of panic or text/plain 400 |
| Hand-built `/list` with `.replace(",]", "]")` | `Value` Display escaping | This phase | Valid inner JSON, no forged entries (IN-02) |

**Deprecated/outdated:** none relevant within the locked versions.

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | `std::env::vars()` panics on non-UTF-8 entries (the reason for `vars_os()`) | Pattern 6 | None. `vars_os()` is correct either way. |
| A2 | No ancestor directory of the repo or of `/home/t3code` contains a `.env` with `SERVER.*` (inferred from Phase 1's empty-dir smoke passing; not re-probed, because reading `.env` paths is blocked by a secret guard) | Pitfall 8 / Validation | A smoke run from the repo could pick up stray config. Mitigated by using `env -i` plus explicit `SERVER.*`, which override the file anyway. |
| A3 | Accepting the BOM regression is acceptable to the operator (Linux-only target, loud failure) | Open Question 1 | If rejected, the planner must read the file manually (deviating from D-14's literal `dotenv_iter()`). |
| A4 | Echoing the raw failing `.env` line to stderr is acceptable | Open Question 2 / Security | Possible leak of a secret value from `.env` into container logs. Low, since `.env` here holds only host and port. |

## Open Questions (RESOLVED)

1. **BOM-prefixed `.env` (behaviour change vs `dotenv()`)** — RESOLVED (orchestrator, Recommended): accept the startup failure, pinned by unit test `bom_prefixed_dotenv_is_error` (02-02); document it in the Phase 6 README.
   - What we know: `dotenv_iter()` gives no BOM stripping, and the failure is explicit at startup.
   - What's unclear: whether the operator wants parity.
   - Recommendation: accept and pin it with a unit test (`from_read_iter("\u{feff}SERVER.HOST=x\n")` → `Err` starting with `.env:`). Note it in the Phase 6 README "Behaviour changes". The alternative would be to locate the file manually (cwd → ancestors), read it to a `String`, strip `\u{feff}`, then `from_read_iter`. It is more code and deviates from D-14's wording.

2. **Should `LineParse` details be redacted?** — RESOLVED (orchestrator, Recommended): no redaction; keep dotenvy's Display in the startup error (02-02).
   - What we know: `Display` includes the full raw line, possibly multiple lines for unbalanced quotes.
   - Recommendation: keep dotenvy's Display (best operator diagnostics, as D-15 asks for `<detail>`). This is a LOW risk. If redaction is wanted, match `dotenvy::Error::LineParse(_, idx)` (the enum is `#[non_exhaustive]`, so add a `_ =>` arm) and print only the index.

3. **`AppState` location** — RESOLVED (orchestrator, Recommended): `src/state.rs` at crate root (02-01). `src/state.rs` (recommended) vs `src/http/state.rs`. Either works. Decide in the plan, and keep `AppState` `pub` (re-exported as `rs3::AppState`) so `main` can build it.

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| Rust toolchain (cargo, clippy, rustfmt) | build/test/lint gate | ✓ | cargo 1.99.1, clippy 0.1.99 | — |
| Cached V8 build in `target/` | fast `cargo test` | ✓ | incremental test build 3.45 s, 36 passed / 2 ignored baseline | — |
| curl | live smokes | ✓ | 8.5.0 | python3 urllib |
| python3 | bind-occupied smoke (Phase 1 pattern) | ✓ | present | — |
| jq | optional JSON assertions in smokes | ✓ | present | python3 -c json |
| CPU cores | multi-worker smoke (actix default workers = cores) | ✓ | 4 | `.workers(n)` not needed |

**Missing dependencies with no fallback:** none.
**Missing dependencies with fallback:** none.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust libtest + `#[actix_web::test]` (actix-web 4.17.0 test utilities) |
| Config file | none (Cargo defaults). New: `clippy.toml` (Wave 0) |
| Quick run command | `cargo test --lib` (about 3–4 s warm) |
| Full suite command | `cargo build --locked && cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check` |

Test names below are **proposed**. The planner may rename them, but the filters in the commands must match whatever names are chosen.

### Phase Requirements → Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| HTTP-01 | `GET /` → 200 `"Welcome to rs3"` via configure | unit (actix) | `cargo test --lib http::tests::test_index_body_is_welcome` | ✅ (re-point to `configure(test_state())`) |
| HTTP-02 | `GET /ip` → `"{'IP' : 'test-host'}"` from injected settings | unit | `cargo test --lib http::tests::test_ip_body_is_ip` | ✅ (re-point) |
| HTTP-02 / ARCH-03 | live: `/ip` reports the startup host | smoke | `cargo build --locked && { env -i PATH="$PATH" SERVER.HOST=127.0.0.1 SERVER.PORT=18460 target/debug/rs3 >/dev/null 2>&1 & P=$!; B=$(curl -s --retry 30 --retry-connrefused --retry-delay 1 http://127.0.0.1:18460/ip); kill $P; test "$B" = "\"{'IP' : '127.0.0.1'}\""; }` | n/a |
| HTTP-03 | insert echo golden, input order | unit | `cargo test --lib http::kv::tests::test_insert_is_ok` | ✅ (re-point) |
| HTTP-03 | `{}` → 200 `"{}"`, store unchanged | unit | `cargo test --lib http::kv::tests::insert_empty_object_is_noop` | ❌ Wave 0 |
| HTTP-03 | insert without JSON content type (`curl -d`) works | smoke | `… curl -s -d '{"k":"v"}' http://127.0.0.1:18461/insert` → `"{\"k\":\"v\"}"` | n/a |
| HTTP-04 | `/list` golden `"[{\"a\" : \"1\"},{\"b\" : \"2\"}]"` and empty `"[]"` | unit | `cargo test --lib list` | partial (`format_list_shape`, `format_list_empty_is_brackets` exist) |
| HTTP-04 | escaping round-trip (`"`, `\`, `,]`, control chars) at codec and HTTP level | unit | `cargo test --lib format_list_round_trips && cargo test --lib http::kv::tests::list_escapes_quotes_and_backslashes` | ❌ Wave 0 (replaces `format_list_keeps_legacy_replace_quirk`) |
| HTTP-05 | GET golden `"John Doe"` | unit | `cargo test --lib http::kv::tests::test_get_is_ok` | ✅ (re-point) |
| HTTP-06 | DELETE golden + key gone | unit | `cargo test --lib http::kv::tests::test_delete_is_ok` | ✅ (re-point) |
| HTTP-08 | actix 4.x API only | static | `grep -rq 'actix_web::test' src/ && ! grep -rnE 'actix_rt\|body::Body\|web::Json' src/` | n/a |
| ROBU-01 | 404 JSON for GET/DELETE/PATCH missing, incl. empty store | unit | `cargo test --lib not_found` (e.g. `get_missing_is_404_json`, `get_on_empty_store_is_404_json`, `delete_missing_is_404_json`, `patch_missing_is_404_json`) | ❌ Wave 0 |
| ROBU-01 | PATCH existing key → 501 `not_implemented` | unit | `cargo test --lib http::tests::test_patch_existing_is_501` | ✅ rewrite (`test_patch_is_501` must insert the key first) |
| ROBU-01 | live 404 is JSON | smoke | `O=$(curl -s -w '\n%{http_code} %{content_type}' http://127.0.0.1:PORT/key/nope); test "$(printf '%s\n' "$O" \| tail -1)" = "404 application/json" && printf '%s\n' "$O" \| head -1 \| jq -e '.error=="not_found"'` | n/a |
| ROBU-02 | 400: malformed, empty, whitespace, trailing, invalid UTF-8 | unit | `cargo test --lib malformed` (codec `parse_rejects_malformed_json` → `MalformedJson`; HTTP `insert_malformed_is_400`, `insert_empty_body_is_400`, `insert_invalid_utf8_is_400`) | partial / ❌ Wave 0 |
| ROBU-02 | 422: non-object, non-string value; store unchanged afterwards | unit | `cargo test --lib invalid_input` (e.g. `insert_non_object_is_422`, `insert_non_string_value_is_422_and_inserts_nothing`) | ❌ Wave 0 |
| ROBU-02 / D-05 | > 256 KiB → 413 (actix default, not JSON) | smoke (or unit: `TestRequest::set_payload(vec![b'a'; 300*1024])` → 413, verified in spike) | `head -c 300000 /dev/zero \| tr '\0' a \| curl -s -o /dev/null -w '%{http_code}' --data-binary @- http://127.0.0.1:PORT/insert` → `413` | ❌ Wave 0 |
| ARCH-02 | no global map; two apps share one state | unit + static | `cargo test --lib insert_on_one_app_visible_on_another && ! grep -rnE 'LazyLock\|lazy_static\|static [A-Z_]+: .*Store' src/ && ! grep -rn 'TODO(phase-2)' src/` | ❌ Wave 0 |
| ARCH-02 | live multi-worker visibility (4 workers) | smoke | start the binary; `curl -s -d '{"mw":"1"}' …/insert`; `for i in $(seq 20); do test "$(curl -s …/key/mw)" = '"1"' \|\| exit 1; done` | n/a |
| ARCH-03 | handlers never read env / load config | static | `! grep -rnE 'Settings::load\|env::var\|vars_os\|dotenvy' src/http/ && test "$(grep -rn 'Settings::load' src/ \| grep -v 'src/config.rs' \| wc -l)" = 1` (only `main`) | n/a |
| TEST-01 | no sleep / no shared global; repeatable in parallel and serial | static + run | `! grep -rn 'thread::sleep' src/ && for i in 1 2 3 4 5; do cargo test -q 2>&1 \| grep -q 'test result: ok' \|\| exit 1; done && cargo test -- --test-threads=1 2>&1 \| grep -E 'test result: ok\. [1-9][0-9]* passed'` | n/a |
| TEST-02 | tests independent of process env | run + static | `env -i PATH="$PATH" HOME="$HOME" cargo test 2>&1 \| grep -E 'test result: ok\. [1-9][0-9]* passed' && ! grep -rnE 'set_var\|remove_var' src/` | n/a |
| D-14 (WR-02) | no env mutation; `dotenv_iter` + `vars_os` | static | `! grep -rnE 'dotenvy::dotenv\(\)\|set_var\|from_path\(' src/ && grep -q 'dotenv_iter' src/config.rs && grep -q 'vars_os' src/config.rs` | n/a |
| D-14/15/16 | merge: env wins, file first-wins, malformed → Err `.env:`, `server.` filter, non-UTF-8 skipped, BOM pinned | unit | `cargo test --lib config::` | ❌ Wave 0 (add to the existing `config::tests`) |
| D-15 (WR-01) | malformed `.env` → exit 1 with `error: invalid configuration: .env:` | smoke | `D=$(mktemp -d -p /home/t3code) && printf 'SERVER.HOST=127.0.0.1\nSERVER.PORT 5000\n' > "$D/.env" && timeout 30 env -i -C "$D" PATH="$PATH" "$PWD/target/debug/rs3" 2>"$D/err"; C=$?; cat "$D/err"; grep -q '^error: invalid configuration: .env:' "$D/err" && test "$C" = 1 && ! grep -q panicked "$D/err"; R=$?; rm -rf "$D"; [ "$R" = 0 ]` | n/a |
| D-17 | bind failure prints no banner | smoke | Phase 1 python bind-occupied smoke, additionally asserting `"Starting Http server" not in r.stdout` | n/a |
| D-13 | lints on, clippy.toml present, no suppression attrs | static | `grep -q 'unwrap_used = "deny"' Cargo.toml && grep -q 'expect_used = "deny"' Cargo.toml && grep -q 'allow-unwrap-in-tests = true' clippy.toml && grep -q 'allow-expect-in-tests = true' clippy.toml && ! grep -rnE '#!?\[(allow\|cfg_attr\([^]]*allow)' src/ && cargo clippy --all-targets -- -D warnings` | ❌ Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test --lib` plus `cargo clippy --all-targets -- -D warnings`
- **Per wave merge:** the full TOOL-03 gate plus the static greps above
- **Phase gate:** the full gate, the `env -i` and `--test-threads=1` runs, the 5× repeat run and all live smokes green before `/gsd-verify-work`

### Wave 0 Gaps
- [ ] `clippy.toml` with both allowances, landing in the same commit as `[lints.clippy] unwrap_used = "deny"`, `expect_used = "deny"` in `Cargo.toml`
- [ ] `test_state()` helper (per module or a shared `#[cfg(test)]` module)
- [ ] New tests: 404 set, 400 set, 422 set (store-unchanged assertion), `{}` no-op, escaping round-trip (codec plus HTTP), two-app shared state, config merge unit tests
- [ ] Rewrites: `test_patch_is_501` (insert the key first), `parse_rejects_malformed_json` (expect `MalformedJson`), remove `format_list_keeps_legacy_replace_quirk`, `insert_many_*` → `insert_all`, parked PATCH tests → `test_state()`

## Security Domain

`security_enforcement: true`, ASVS level 1.

### Applicable ASVS Categories

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | no | No auth in rs3 (unchanged scope) |
| V3 Session Management | no | Stateless API |
| V4 Access Control | no | No users or roles (out of scope this milestone) |
| V5 Input Validation | **yes** | Raw-bytes UTF-8 check, serde_json parse, shape validation before write, 256 KiB payload cap (actix default), serde_json output encoding for `/list` |
| V6 Cryptography | no | None used |
| V7 Error Handling & Logging | **yes** | Typed `ApiError`: no panics on request data, no stack traces or internals in bodies, fixed machine codes |
| V14 Configuration | **yes** | No env mutation (`set_var` race removed), malformed config fails closed (exit 1), `server.` key allow-list |

### Known Threat Patterns for actix-web + serde_json

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| Entry forgery in `/list` via an unescaped value (`x"},{"admin" : "true`) | Tampering / Spoofing | serde_json escaping (Pattern 5) plus a round-trip test using exactly this payload |
| Panic on a malformed body or missing key (worker-thread crash / DoS) | Denial of Service | `?` + `ApiError`, `unwrap_used`/`expect_used` deny |
| Oversized body memory exhaustion | Denial of Service | actix default 262 144 B limit → 413 (D-05) |
| Deeply nested JSON stack exhaustion | Denial of Service | serde_json recursion limit (128) → `RecursionLimitExceeded` → 400 [VERIFIED: spike] |
| Partial insert on invalid batch | Tampering (integrity) | Validate everything before the first write (Pattern 4) |
| Env-mutation data race (UB) at startup | Tampering / DoS | `dotenv_iter` + map, no `set_var` (D-14) |
| Unrelated env var colliding with the config tree | Tampering | `server.` prefix allow-list (D-16) |
| Error message echoes the user-supplied key | Information disclosure (minimal) | Acceptable: it echoes the caller's own input. The JSON encoder escapes it, so there is no injection into the body. |
| `.env` parse error echoes the raw line to stderr | Information disclosure (LOW) | Accept (Open Question 2) or redact the `LineParse` text |

## Sources

### Primary (HIGH confidence)
- `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/actix-web-4.17.0/`: `src/error/response_error.rs` (25-48, 119-126), `src/error/error.rs:116`, `src/types/payload.rs` (161-230, 273-339, 355-437), `src/app.rs:191-201`, `src/config.rs:347-380`, `src/server.rs:97-108`, `src/data.rs:162-183`, `src/response/builder.rs:355-372`
- `actix-http-3.19.0/src/error.rs:266-268` (`Overflow` display "payload reached size limit"), `src/http_message.rs:61-75` (charset check)
- `serde_json-1.0.154/src/error.rs:54-81, 164-185`, `src/ser.rs:2147-2165`, `src/value/mod.rs:197-222`
- `dotenvy-0.15.7/src/errors.rs:8-46`, `src/iter.rs:29-66`, `src/lib.rs:303, 328-331, 370-373`, `src/find.rs:25-60`, `src/parse.rs:76-95, 260-274`
- `config-0.15.27/src/env.rs:227-230, 269`, `src/error.rs:98, 228`, `src/map.rs`
- Project sources read this session: `Cargo.toml`, `src/{lib,main,config}.rs`, `src/http/{mod,kv}.rs`, `src/store/{mod,legacy}.rs`, `src/js/mod.rs`
- Throwaway spike crate (offline, project `Cargo.lock`, Rust 1.99.1, deleted after the run): 6 tests passed, clippy `-D warnings` with/without `clippy.toml`, live 4-worker server plus curl (insert without JSON content type, 8/8 cross-worker GETs, 404 JSON, 413 Content-Length and chunked, bogus charset accepted)

### Secondary (MEDIUM confidence)
- `.planning/research/{ARCHITECTURE,FEATURES,PITFALLS}.md` (milestone research, reused); `01-REVIEW.md` WR-01/WR-02/IN-01..07

### Tertiary (LOW confidence)
- None.

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH. No new deps, and versions come from the lockfile.
- Architecture: HIGH. Every pattern was compiled, tested and run live on the locked versions.
- Pitfalls: HIGH. The dotenvy semantics, clippy behaviour and 413 behaviour were observed directly in the spike.

**Research date:** 2026-10-20
**Valid until:** 2026-11-20 (locked versions; only invalidated by a `Cargo.lock` bump)
