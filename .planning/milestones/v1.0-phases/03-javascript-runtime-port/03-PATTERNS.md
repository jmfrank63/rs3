# Phase 3: JavaScript Runtime Port - Pattern Map

**Mapped:** 2026-10-20
**Files analyzed:** 22 (new + modified)
**Analogs found:** 18 / 22 (the JS-specific files have no in-repo analog; use the verified RESEARCH.md Code Examples)

All analog paths below are git-tracked (`git ls-files` checked).

## File Classification

| New/Modified File | Role | Data Flow | Closest Analog | Match Quality |
|-------------------|------|-----------|----------------|---------------|
| `src/js/mod.rs` (modify) | module root / error types / test singleton | n/a | `src/js/mod.rs` itself + `src/state.rs` (`test_state`) + `src/store/mod.rs` (`StoreError`) | role-match |
| `src/js/ops.rs` (new) | service adapter (ops) | request-response (sync, store CRUD) | `src/http/kv.rs` handlers (same store calls + codec) | data-flow match |
| `src/js/runtime.rs` (new) | service | transform (script -> String) | none | no analog (RESEARCH "runtime.rs") |
| `src/js/executor.rs` (new) | worker pool | event-driven / queue | none | no analog (RESEARCH "executor.rs") |
| `src/state.rs` (modify) | state/provider | n/a | itself | exact |
| `src/main.rs` (modify) | entry point | startup | itself (Settings::load error/exit pattern) | exact |
| `src/http/error.rs` (modify) | error mapping | request-response | itself | exact |
| `src/http/mod.rs` (modify: `patch_key` + tests) | controller | request-response | `src/http/kv.rs::get_key` / `insert` | exact |
| `src/store/legacy.rs` (modify: IN-04 doc, IN-05 rename) | utility | transform | itself | exact |
| `src/http/kv.rs` (modify: IN-05 rename) | test | n/a | itself | exact |
| `Cargo.toml` (modify) | config | n/a | itself | exact |
| `Cargo.lock` (modify: one `"tokio",` line) | config | n/a | — | generated |
| `requests/{fac,sum,bin,store}.js` | fixture | n/a | `src/http/mod.rs:324` `LIST_SCRIPT` (already new syntax) | role-match |
| `requests/{fac,sum,bin,store}.txt` | fixture (JSON-escaped) | n/a | same as above | role-match |
| `requests/rs3_http_localhost_5000.http`, `requests/rs3_https_rs3_cwe.http` | fixture | n/a | same | role-match |
| `src/example.js` | example | n/a | same | role-match |
| `docs/js-ops.md` (new) | docs | n/a | none in repo | no analog (RESEARCH mapping table) |
| JS unit tests inside `src/js/*.rs` | test | n/a | `src/store/mod.rs` `#[cfg(test)] mod tests` (line 98) | role-match |
| HTTP PATCH tests in `src/http/mod.rs` | test | request-response | `src/http/mod.rs` tests 278-354, `kv.rs` `assert_insert_400` | exact |

## Pattern Assignments

### `src/js/ops.rs` (service adapter, sync store CRUD)

**Analog:** `src/http/kv.rs` lines 1-58 — the HTTP handlers do exactly what the ops do, through the same codec. Ops must mirror them byte-for-byte (TEST-06).

Imports convention (kv.rs lines 1-6) — crate paths via `crate::store::{StoreError, legacy}`:
```rust
use super::error::ApiError;
use crate::state::AppState;
use crate::store::{StoreError, legacy};
```

Insert: validate-all then write (kv.rs lines 29-32) — `op_rs3_insert` copies this order:
```rust
let entries = legacy::parse_insert_body(text)?;
let echo = legacy::format_insert_echo(&entries);
state.store.insert_all(entries);
Ok(HttpResponse::Ok().json(echo))
```

List (kv.rs line 57) — `op_rs3_list` must call the identical function:
```rust
HttpResponse::Ok().json(legacy::format_list(&state.store.list()))
```

Get/delete owned-value calls (kv.rs lines 42, 51): `state.store.get(&key)?`, `state.store.remove(&key)?`.

Error source (`src/store/mod.rs` lines 11-19) — map in `to_js`, exhaustive match, no wildcard:
```rust
pub enum StoreError {
    #[error("key not found: {0}")] NotFound(String),
    #[error("malformed JSON: {0}")] MalformedJson(String),
    #[error("invalid input: {0}")] InvalidInput(String),
}
```
NotFound -> `JsErrorBox::generic(e.to_string())`; MalformedJson/InvalidInput -> `JsErrorBox::type_error(e.to_string())`. Never `JsErrorBox::new("NotFound", ..)` (throws `undefined`).

Re-entrancy safety already documented in `src/store/mod.rs` lines 27-28 (each method takes and drops its own `pin()` guard) — nothing to add.

Op bodies, `#[op2(reentrant)]` on `op_rs3_patch`, `extension!` with `options`/`middleware` (disable `op_panic`)/`state`: copy RESEARCH.md "ops.rs" verbatim. Nested error: strip leading `"Uncaught "` (RESOLVED Q3).

---

### `src/js/runtime.rs` and `src/js/executor.rs` (no analog)

Use RESEARCH.md Code Examples "runtime.rs" (`JsRuntime::try_new`, `execute_script(name.to_owned(), ..)`, `deno_core::scope!`, `to_rust_string_lossy`, `eprintln!` stack) and "executor.rs" (tokio mpsc + `Arc<tokio::sync::Mutex<Receiver>>` + oneshot, `rs3-js-{i}` threads, `rt.enter()` after `block_on(recv)`, `catch_unwind(AssertUnwindSafe(..))`). Module doc-comment style to copy: `src/store/mod.rs` lines 1-5 (`//!` summary stating what the module is free of — here "free of actix_web").

---

### `src/js/mod.rs` (module root)

**Analog:** current file (lines 1-7) — keep `init_platform()` unchanged; add `pub mod ops; pub mod runtime; pub mod executor;`, `ExecError`, `JsSettings`.

`ExecError` style copies `StoreError` (`src/store/mod.rs` 10-19): `#[derive(Debug, thiserror::Error)]`, one `#[error(..)]` per variant.

`test_executor()` copies `test_state()` (`src/state.rs` lines 35-45): `#[cfg(test)] pub(crate) fn`, `expect` allowed only because it is under `#[cfg(test)]` (Pitfall 6):
```rust
#[cfg(test)]
pub(crate) fn test_state() -> AppState {
    ...
    AppState::new(Settings::from_map(map).expect("valid test settings"))
}
```

---

### `src/state.rs` (modify)

**Analog:** itself, lines 18-33.
```rust
#[derive(Clone, Debug)]
pub struct AppState {
    pub store: Store,
    pub settings: Arc<Settings>,
}
impl AppState {
    pub fn new(settings: Settings) -> Self { Self { store: Store::new(), settings: Arc::new(settings) } }
}
```
Add `pub js: JsExecutor` (must be `Clone + Debug` for the derive). `new(settings, js)`; `test_state()` passes `crate::js::test_executor()`. Update the module doc (lines 1-6) — "from Phase 3" is now true.

---

### `src/main.rs` (modify)

**Analog:** itself, lines 8-15 (startup failure -> `eprintln!("error: ...")` + `ExitCode::FAILURE`):
```rust
let settings = match Settings::load() {
    Ok(s) => s,
    Err(e) => { eprintln!("error: invalid configuration: {e}"); return ExitCode::FAILURE; }
};
rs3::js::init_platform();
```
Insert after `init_platform()` and before `AppState::new`: `match rs3::js::executor::JsExecutor::start(&JsSettings::default()) { Ok(j)=>j, Err(e)=>{ eprintln!("error: cannot start JS runtime: {e}"); return ExitCode::FAILURE; } }`. Order gate: `init_platform()` < `JsExecutor::start` < `HttpServer::new` (awk gate in RESEARCH Validation).

---

### `src/http/error.rs` (modify)

**Analog:** itself. Add variant and keep both matches exhaustive (lines 11-18, 28-37, 41-49):
```rust
pub enum ApiError {
    #[error(transparent)] Store(#[from] StoreError),
    #[error("not implemented: {0}")] NotImplemented(String),   // remove with test_patch_existing_is_501
    #[error("internal error")] Internal,
}
...
ApiError::Internal => "internal",            // code()
ApiError::Internal => StatusCode::INTERNAL_SERVER_ERROR,   // status_code()
```
Add `#[error("{0}")] JsError(String)` -> `"js_error"` / `StatusCode::UNPROCESSABLE_ENTITY`. Add `impl From<ExecError> for ApiError` (Script -> JsError(message); Unavailable/Internal -> `eprintln!` detail then `ApiError::Internal`). `error_response` (lines 51-56) needs no change. Note `http` may import `crate::js::ExecError` (direction http -> js -> store).

---

### `src/http/mod.rs` `patch_key` (controller, request-response)

**Analog:** `src/http/kv.rs::get_key` lines 36-43 (`?` on store, JSON-string envelope) and current `patch_key` lines 41-53:
```rust
#[patch("/key/{key}")]
pub async fn patch_key(state: web::Data<AppState>, path: web::Path<String>) -> Result<HttpResponse, ApiError> {
    state.store.get(&path.into_inner())?;   // keep: 404 before queueing
    ...
}
```
Replace body with RESEARCH "PATCH handler" (`let source = state.store.get(&key)?; let out = state.js.execute(state.store.clone(), key, source).await?; Ok(HttpResponse::Ok().json(out))`). Update the doc comment (lines 41-43).

---

### Tests in `src/http/mod.rs`

**Analog:** same file lines 195-354.
- Module header (lines 196-199): `pub(crate) mod tests { use super::*; use crate::state::test_state; use actix_web::{App, dev::ServiceResponse, http::StatusCode, test as atest}; }`
- Error assertion helper `assert_json_error(resp, StatusCode::UNPROCESSABLE_ENTITY, "js_error")` (lines 203-220) — returns body, then assert `body["message"] == "Uncaught Error: boom"`.
- Seed-then-patch shape (current `test_patch_existing_is_501`, lines 286-296):
```rust
let state = test_state();
state.store.insert("p".into(), "1".into());
let app = atest::init_service(App::new().configure(configure(state.clone()))).await;
let req = atest::TestRequest::patch().uri("/key/p").to_request();
let resp = atest::call_service(&app, req).await;
```
- Body bytes assertion: `assert_eq!(atest::read_body(resp).await, web::Bytes::from_static(br#""120""#));` (pattern lines 313-320).
- Un-ignore `test_patch_is_ok` / `test_patch_body_is_listing` (lines 326-354): delete only the `#[ignore = ...]` lines.
- Fixtures: `state.store.insert("fac".into(), include_str!("../../requests/fac.js").into())` plus seeded `num="5"`.
- 500 test needs a `#[cfg(test)]` closed-executor constructor on `JsExecutor`.

### JS unit tests in `src/js/*.rs`

**Analog:** `src/store/mod.rs` line 98 `#[cfg(test)] mod tests` with plain `#[test]` fns (sync; `run_script` is sync but must run on the executor/with tokio context — submit via `test_executor()` and `block_on`, or use `#[actix_web::test]`). Do NOT `use actix_web::test;` unaliased (shadows `#[test]`); `js` must not import actix in non-test code.

---

### `src/store/legacy.rs` / `src/http/kv.rs` (housekeeping D-17)

- IN-04: rewrite the sentence at `legacy.rs` lines 29-30 ("When valid JSON grammar has more than one such problem, the first one in body order decides.").
- IN-05: rename `parse_rejects_deep_nesting` (`legacy.rs:273`, body `"[".repeat(200)`) and `insert_deeply_nested_is_400_json` (`kv.rs:290`) to truncated-input names (e.g. `parse_rejects_truncated_open_brackets`, `insert_truncated_open_brackets_is_400_json`). Leave `parse_valid_deep_nesting_is_invalid_input` (line 294) as is.

---

### `Cargo.toml`

**Analog:** itself — comment-above-dependency convention (lines 13-15):
```toml
# Must stay "0.7": deno_core pins deno_error =0.7.1. Never `cargo add` it.
deno_error = "0.7"
```
Add `tokio = { version = "1.54.0", features = ["rt", "sync"] }` under `[dependencies]` and a new `[dev-dependencies]` with `deno_core = { version = "0.412.0", features = ["unsafe_use_unprotected_platform"] }`, each with a one-line why-comment (note Phase 4 fallback: move feature to `[dependencies]`). Hand-edit, no `cargo add`.

---

### JS artefacts (`requests/*`, `src/example.js`)

**Analog:** `src/http/mod.rs:324` `const LIST_SCRIPT: &str = "Deno.core.ops.op_rs3_list().toString();";`
Current old form (`requests/fac.js` last lines):
```js
Deno.core.ops();
let num = Number(Deno.core.opSync("rs3_get", "num").toString());
```
-> delete `Deno.core.ops();`, use `Deno.core.ops.op_rs3_get("num")`. `store.js`: `Deno.core.ops.op_rs3_insert("{\"res\":\"" + p + "\"}")`. `example.js`: `Deno.core.ops.op_rs3_list()` (keep `Deno.core.print`). `.txt`/`.http`: same edits inside JSON strings; validate with python json check. Exact list in RESEARCH "JS artefact inventory".

## Shared Patterns

### Error bodies
**Source:** `src/http/error.rs` lines 22-27, 51-56 — every error `{"error": code, "message": to_string()}`. Apply to new `JsError` and `Internal` paths; no handler builds error JSON itself.

### Success envelope
**Source:** `src/http/kv.rs` line 42/57 — `HttpResponse::Ok().json(String)`. PATCH success uses the same.

### No panics / lint gate
**Source:** `Cargo.toml` `[lints]` + `clippy.toml` (`allow-*-in-tests`), `src/lib.rs` line 1 `#![forbid(unsafe_code)]`. No `unwrap`/`expect` outside `#[cfg(test)]`, no `#[allow]`. Use `JsRuntime::try_new`, never `new`.

### Injected state, no globals
**Source:** `src/state.rs` + `src/http/mod.rs::configure` lines 15-24 (one `web::Data` created once, cloned per worker). Only allowed `LazyLock`: the `#[cfg(test)]` executor.

### Test isolation
**Source:** `test_state()` per test (`src/state.rs` 35-45); per-test stores, shared executor.

## No Analog Found

| File | Role | Data Flow | Reason |
|------|------|-----------|--------|
| `src/js/runtime.rs` | service | transform | First V8 code; use RESEARCH "runtime.rs" |
| `src/js/executor.rs` | worker pool | event-driven | No threads/channels in repo; use RESEARCH "executor.rs" |
| `src/js/ops.rs` (op/extension macros) | adapter | request-response | Store-call logic mirrors kv.rs; macro syntax from RESEARCH "ops.rs" |
| `docs/js-ops.md` | docs | — | No `docs/` dir yet; use RESEARCH mapping table |

## Metadata

**Analog search scope:** `src/**`, `requests/`, `Cargo.toml`, `clippy.toml`
**Files scanned:** 14
**Pattern extraction date:** 2026-10-20
