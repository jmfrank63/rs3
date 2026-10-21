# Phase 2: HTTP API Port & Error Handling - Pattern Map

**Mapped:** 2026-10-20
**Files analyzed:** 11 (2 new source, 1 new config, 8 modified)
**Analogs found:** 10 / 11 (all analogs are git-tracked; verified with `git ls-files src`)

Orchestrator resolutions applied: `AppState` lives in `src/state.rs` at the crate root. A BOM-prefixed `.env` fails at startup, which is pinned by a unit test and documented in Phase 6. The startup error keeps the raw `.env` line text.

## File Classification

| New/Modified File | Role | Data Flow | Closest Analog | Match Quality |
|---|---|---|---|---|
| `src/state.rs` (NEW) | model / provider (shared state) | injected state | `src/store/mod.rs` (`Store` Clone-newtype over `Arc`) + `src/config.rs` (`Settings`) | role-match |
| `src/http/error.rs` (NEW) | error type / response mapper | request-response | `src/store/mod.rs:10-17` (`StoreError` thiserror) + `src/http/mod.rs:31-37` (current JSON 501 body) | partial (no `ResponseError` impl exists yet) |
| `clippy.toml` (NEW) | config | — | none (`Cargo.toml` `[lints]` table is the nearest) | no analog |
| `src/http/mod.rs` (MOD) | route table + controller | request-response | itself (`configure`, `status` with `web::Data`) | exact |
| `src/http/kv.rs` (MOD) | controller | CRUD | itself; `status` in `src/http/mod.rs:26-29` for `web::Data` injection | exact |
| `src/store/mod.rs` (MOD) | service / store | CRUD | itself (`insert_many` → `insert_all`, add `MalformedJson`) | exact |
| `src/store/legacy.rs` (MOD) | utility (codec) | transform | itself (`parse_insert_body`, `format_insert_echo`) | exact |
| `src/config.rs` (MOD) | config loader | file-I/O + transform | itself (`from_map`, test `vars()` helper) | exact |
| `src/main.rs` (MOD) | entrypoint | startup | itself | exact |
| `src/lib.rs` (MOD) | module root | — | itself | exact |
| `Cargo.toml` (MOD) | config | — | itself (`[lints.rust]` table) | exact |

## Pattern Assignments

### `src/state.rs` (NEW, shared state)

**Analog:** `src/store/mod.rs` lines 19-31 (a cheap `Clone` handle with an `Arc` inside and a `new()` constructor), plus `src/config.rs` lines 16-20 (`Settings` derives `Debug, Clone`).

```rust
/// Shared handle to the key/value map. Clones share the same map.
#[derive(Clone, Default, Debug)]
pub struct Store {
    map: Arc<papaya::HashMap<String, String>>,
}
impl Store {
    /// An empty store.
    pub fn new() -> Self { Self::default() }
```

Apply it as `#[derive(Clone, Debug)] pub struct AppState { pub store: Store, pub settings: Arc<Settings> }` plus `AppState::new(settings: Settings) -> Self`. Keep the module doc in the style of `src/store/mod.rs:1-5`. Do not import actix here, because `main` and Phase 3 `js` will use it as well. Register it as `pub mod state;` in `src/lib.rs` next to the existing `pub mod` lines 5-8.

---

### `src/http/error.rs` (NEW, ApiError + ResponseError)

**Analog for the thiserror style:** `src/store/mod.rs` lines 10-17.
```rust
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum StoreError {
    #[error("key not found: {0}")]
    NotFound(String),
    #[error("invalid input: {0}")]
    InvalidInput(String),
}
```
**Analog for the JSON error body:** `src/http/mod.rs` lines 32-37. This is the current 501 handler, and it builds a JSON object body through `HttpResponse::...().json(...)`. Replace it with `HttpResponse::build(self.status_code()).json(ErrorBody { error: self.code(), message: self.to_string() })`.

Copy the full shape from RESEARCH.md Pattern 1 (lines 223-270): `#[error(transparent)] Store(#[from] StoreError)`, `NotImplemented(String)`, `Internal`, `code()`, `status_code()`, `error_response()`. The type must stay `pub`, inside `pub mod error;` declared in `src/http/mod.rs`. If it is not `pub`, the unused `Internal` variant triggers the dead_code lint, and `#[allow]` is forbidden.

---

### `src/http/mod.rs` (MOD, route table, `/`, `/ip`, PATCH)

**Analog:** itself.

**Imports** (line 2-4). Extend this to `HttpResponse, Responder, get, patch, web` plus `crate::state::AppState` and `error::ApiError`. Remove the `crate::config::Settings` import.

**Current configure** (lines 8-14). It becomes `configure(state: AppState) -> impl Fn(&mut web::ServiceConfig) + Clone + Send + 'static`, as in RESEARCH Pattern 2 (lines 284-293). Keep the same `.service(index).service(status).service(patch_key).configure(kv::configure)` chain, prefixed by `cfg.app_data(data.clone())`.
```rust
pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(index)
        .service(status)
        .service(patch_key)
        .configure(kv::configure);
}
```

**Injected-data handler** (lines 26-29). Switch to `state: web::Data<AppState>` and read `state.settings.server.host`. The body format stays byte-identical.
```rust
#[get("/ip")]
pub async fn status(settings: web::Data<Settings>) -> impl Responder {
    HttpResponse::Ok().json(format!("{{'IP' : '{}'}}", settings.server.host))
}
```

**PATCH** (lines 32-37). Replace it with RESEARCH Pattern 3 (lines 337-341): `state.store.get(&key)?` gives a 404 for a missing key, otherwise `Err(ApiError::NotImplemented(..))` gives 501.

**Test idioms to keep** (lines 40-57, 59-120): `use actix_web::{App, http::StatusCode, test as atest};`, `atest::init_service`, `call_service`, `read_body(resp).await == r#"..."#.as_bytes()`, and the JSON body check `serde_json::from_slice::<serde_json::Value>(..)`, then `body["error"].is_string()` (lines 116-119).

**Test helper to convert** (lines 44-51). `test_settings()` becomes a `test_state() -> AppState` helper that wraps the same `Settings::from_map` map (`SERVER.HOST=test-host`, `SERVER.PORT=8080`) with `Store::new()`. Every test then calls `App::new().configure(configure(test_state()))`.
```rust
fn test_settings() -> web::Data<Settings> {
    let map: ::config::Map<String, String> =
        [("SERVER.HOST", "test-host"), ("SERVER.PORT", "8080")]
            .into_iter().map(|(k, v)| (k.to_owned(), v.to_owned())).collect();
    web::Data::new(Settings::from_map(map).expect("valid test settings"))
}
```
Put `test_state()` somewhere `kv.rs` tests can reach it, for example as a `#[cfg(test)] pub(crate) fn` in `src/state.rs` or `src/http/mod.rs`, so it is not duplicated.

**Tests whose semantics change.** `test_patch_is_501` (lines 108-120) must insert the key into `state.store` first, and a new 404 test is needed. `test_ip_without_settings_is_500` (lines 101-106) builds an app without Data; keep it or adapt it, since it still yields a text/plain 500. The parked tests (lines 124-150) replace `crate::http::kv::STORE.insert(..)` with `state.store.insert(..)` on a per-test `AppState` and keep `#[ignore = "re-enabled in Phase 3 (JS runtime port)"]`. Add the two-apps-one-state test from RESEARCH lines 306-319.

---

### `src/http/kv.rs` (MOD, CRUD controller)

**Analog:** itself, with `web::Data` injection copied from `status` (`src/http/mod.rs:27`).

**Delete:** lines 2, 8-9 (`LazyLock`, the `TODO(phase-2)` comment and `static STORE`).

**Keep:** `kv::configure` (lines 12-17) unchanged as a plain `fn`, with routes only and no Data.

**Handler conversions** (replace lines 22-56). The non-test unwraps at lines 24, 26, 40 and 49 must all go.
- `insert(body: String)` becomes `insert(state: web::Data<AppState>, body: web::Bytes) -> Result<HttpResponse, ApiError>`, following RESEARCH Pattern 4 lines 386-394: `from_utf8` → `MalformedJson`, then `parse_insert_body?`, `format_insert_echo`, `state.store.insert_all(entries)`, `Ok(HttpResponse::Ok().json(echo))`.
- `get_key` drops the `is_empty` branch (lines 37-41) and becomes a single `state.store.get(&key)?`.
- `delete_key` becomes `state.store.remove(&key)?`.
- `list` becomes `HttpResponse::Ok().json(legacy::format_list(&state.store.list()))`. It is infallible, so it may stay `impl Responder`.

**Test idioms** (lines 64-69, 71-84). Keep the `insert_request(payload)` builder and the golden `web::Bytes::from_static(br#""{\"insert_test\":\"John Doe\"}""#)` assertions. Replace every `STORE.insert/get` (lines 102, 115, 127, 134, 158) with `state.store`, using a per-test `test_state()` cloned into `configure(state.clone())`. Replace `App::new().service(insert)` with `App::new().configure(configure_http(state.clone()))`. Note that the name clashes with `kv::configure`, so refer to it as `crate::http::configure`.

---

### `src/store/mod.rs` (MOD, store)

**Analog:** itself.
- Add the `MalformedJson(String)` variant to `StoreError` (lines 11-17) in the same `#[error("...: {0}")]` style, for example `#[error("malformed JSON: {0}")]`.
- Replace `insert_many` (lines 47-60) with an infallible `insert_all`. Keep the pin-once loop.
```rust
let map = self.map.pin();
for (key, value) in validated { map.insert(key, value); }
```
  Its doc comment must state that the write is not atomic for concurrent readers (D-13). The test idiom is the `kv()` helper with plain `#[test]` (lines 99-101). Delete or rewrite `insert_many_writes_all` and `insert_many_all_or_nothing` (from line 128).

---

### `src/store/legacy.rs` (MOD, codec)

**Analog:** itself.
- `parse_insert_body` (lines 11-26). Change only the `map_err` on line 13 to a `classify()` split (RESEARCH lines 372-383). The shape check on lines 14-25 stays as it is.
- `format_insert_echo` (lines 31-37) stays unchanged. Its `Value` usage is the model for escaping.
- `format_list` (lines 40-47). Rewrite it per RESEARCH Pattern 5 (lines 409-414) with `Value::from(k.as_str())` Display, and drop `.replace(",]", "]")`.
- No actix or deno imports here (ARCH-01). The imports stay `use super::StoreError; use serde_json::{Map, Value};`.
- Tests use the `pairs(&[(&str,&str)])` helper (lines 53-58). Replace `format_list_keeps_legacy_replace_quirk` with the round-trip test (RESEARCH lines 419-426). Change `parse_rejects_malformed_json` to expect `MalformedJson`.

---

### `src/config.rs` (MOD, config loader)

**Analog:** itself.
- `load()` (lines 24-27) currently calls `dotenvy::dotenv().ok()`, which mutates the environment and swallows errors. Replace it with the thin shell in RESEARCH lines 467-474 that calls a new pure `merge_dotenv_env(dotenv: Option<I>, env: E)` (RESEARCH lines 446-465) and then the existing `Self::from_map` (lines 30-32, unchanged). Errors use `ConfigError::Message(format!(".env: {e}"))`. That prints the raw line text, which is accepted.
- Update the module doc (lines 1-5) so it describes the merge, process env wins, and the `server.` filter.
- Test idiom: the `vars(&[(&str,&str)])` helper (lines 43-48) and the `unwrap_err()` plus `to_string().contains(..)` checks (lines 69-72). The new tests feed `Some(dotenvy::from_read_iter("...".as_bytes()))` and a `Vec<(OsString, OsString)>`. Add one test for the BOM case (`"\u{feff}SERVER.HOST=x"` → `Err` starting with `.env:`). Also cover first-wins, env overrides the file, the `SERVER`/`OTHER` filter, non-UTF-8 skipped (`OsStringExt::from_vec`), and the `None` file.
- `underscore_names_are_not_mapped` (lines 88-93) stays.

---

### `src/main.rs` (MOD, entrypoint)

**Analog:** itself.
- Keep the error arms (lines 8-14 and 29-33, 35-41). The `error: ...` and `ExitCode::FAILURE` idiom stays.
- Delete lines 15 (the ordering comment is no longer load-bearing), 19 (the banner moves), and 20-26.
- Build `let state = rs3::state::AppState::new(settings);` and `let cfg = rs3::http::configure(state);`, then `HttpServer::new(move || App::new().configure(cfg.clone()))`.
- Move the `println!("Starting Http server at host address: {host}, with port: {port}!")` banner to after the bind `match` and before `server.run()` (D-17, RESEARCH lines 296-301).
- Keep `rs3::js::init_platform()` before the server starts. `web` will no longer be needed in the imports.

---

### `Cargo.toml` (MOD) + `clippy.toml` (NEW)

**Analog:** `Cargo.toml` lines 23-24.
```toml
[lints.rust]
unsafe_code = "forbid"
```
Add `[lints.clippy]` with `unwrap_used = "deny"` and `expect_used = "deny"`. **In the same commit**, add the root `clippy.toml` with `allow-unwrap-in-tests = true` and `allow-expect-in-tests = true` (RESEARCH Pitfall 4). Without it, about 14 test errors appear.

## Shared Patterns

### Error handling (all fallible handlers)
**Source:** new `src/http/error.rs`, with `StoreError` at `src/store/mod.rs:10-17`.
Handlers return `Result<HttpResponse, ApiError>` and use `?` on `Store`/codec results. The `From<StoreError>` derive does the mapping. Use no `unwrap`/`expect` outside tests and no `#[allow(...)]` anywhere.

### State injection
**Source:** `src/http/mod.rs:27` (`web::Data<T>` extractor).
Every handler that touches state takes `state: web::Data<AppState>`. The type must be exactly `Data<AppState>`. `web::Data::new` is called only inside `http::configure`.

### Success envelope
**Source:** `src/http/mod.rs:19,28` and `src/http/kv.rs:27`. Success responses are `HttpResponse::Ok().json(String)`. This double-encoded string is preserved byte-for-byte.

### Test harness
**Source:** `src/http/mod.rs:42` and `src/http/kv.rs:62-69`.
Use `test as atest`, `#[actix_web::test]`, `App::new().configure(crate::http::configure(state.clone()))`, a per-test `test_state()` built with `Settings::from_map` and `Store::new()`, no env vars and no sleeps. Golden bytes are compared with `web::Bytes::from_static(br#"..."#)` or `r#"..."#.as_bytes()`.

## No Analog Found

| File | Role | Reason |
|---|---|---|
| `clippy.toml` | config | There is no clippy config in the repo yet. Use the exact keys from RESEARCH Pitfall 4. |
| (`ResponseError` impl inside `src/http/error.rs`) | error mapper | No `ResponseError` impl exists yet. Use RESEARCH Pattern 1, which was verified in the spike. |

## Metadata

**Analog search scope:** `src/**` (all 9 tracked files), `Cargo.toml`
**Files scanned:** 10
**Pattern extraction date:** 2026-10-20
