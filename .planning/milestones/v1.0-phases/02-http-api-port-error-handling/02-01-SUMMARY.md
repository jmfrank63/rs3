---
phase: 02-http-api-port-error-handling
plan: 01
subsystem: api
tags: [actix-web, web-data, appstate, thiserror, response-error, json-errors, test-isolation]

requires:
  - phase: 01-toolchain-dependencies-foundation
    provides: "papaya Store facade (get/remove return StoreError::NotFound), Settings::from_map, lib + thin main, actix-web 4.17 test idioms"
provides:
  - "src/state.rs: AppState { store: Store, settings: Arc<Settings> } built once in main, plus the #[cfg(test)] test_state() helper"
  - "src/http/error.rs: ApiError (Store(#[from] StoreError) | NotImplemented | Internal) with a ResponseError JSON body {error, message}"
  - "http::configure(state) -> impl Fn(&mut ServiceConfig) + Clone + Send + 'static, creating one web::Data<AppState> shared by every worker and test app"
  - "404 not_found JSON for GET/DELETE/PATCH on a missing key (including the empty store); 501 not_implemented JSON for PATCH on an existing key"
  - "crate::http::tests::assert_json_error(resp, status, code) test helper for 02-03"
  - "Startup banner printed only after a successful bind"
affects: [02-02, 02-03, 03-js-runtime-port]

actuals:
  tokens: 5083
  tasks: 2
  commits: 3
plan_head_before: 0928a96de95826648ff5b51e37c27c67b9f79a5f
plan_head_after: f93e0aaa0ac903bca9126f8eb540746d464df259

tech-stack:
  added: []
  patterns:
    - "Injected state: configure(state) creates the web::Data once and returns a Clone closure; HttpServer::new clones the closure per worker"
    - "Typed HTTP errors: handlers return Result<HttpResponse, ApiError>; ? converts StoreError; status/code matches are exhaustive over StoreError (no wildcard)"
    - "Per-test state: every test builds test_state() and App::new().configure(crate::http::configure(state.clone())); no globals, env or sleeps"

key-files:
  created:
    - src/state.rs
    - src/http/error.rs
    - .planning/phases/02-http-api-port-error-handling/tdd-evidence/02-01-task2-red.json
    - .planning/phases/02-http-api-port-error-handling/tdd-evidence/02-01-task2-red.junit.xml
  modified:
    - src/lib.rs
    - src/http/mod.rs
    - src/http/kv.rs
    - src/main.rs

key-decisions:
  - "AppState lives at the crate root (src/state.rs) with no actix types, so Phase 3 can add a js field without js depending on http"
  - "ApiError stays pub inside pub mod http::error so the not-yet-constructed Internal variant is reachable public API and needs no dead_code suppression"
  - "assert_json_error takes the expected status as `expected`, not `status`: a parameter named status collides with the #[get(\"/ip\")] status handler unit struct inside http::tests"
  - "kv tests drop `use super::*` (they only need crate::http::configure and the test_state helper), which avoids the kv::configure name clash entirely"

patterns-established:
  - "Error body contract: {\"error\": <machine code>, \"message\": <Display>} as application/json, built only in ApiError::error_response"
  - "Missing-key decision is one Store::get / Store::remove call; no handler checks emptiness or length first"

requirements-completed: [HTTP-01, HTTP-02, HTTP-03, HTTP-05, HTTP-06, HTTP-08, ROBU-01, ARCH-02, ARCH-03, TEST-01, TEST-02]

coverage:
  - id: D1
    description: "One AppState built in main and injected through configure(state); no process-wide store; settings loaded once"
    requirement: ARCH-02
    verification:
      - kind: unit
        ref: "src/http/mod.rs#http::tests::insert_on_one_app_visible_on_another"
        status: pass
      - kind: e2e
        ref: "live smoke on 127.0.0.1:18461: 20/20 fresh-connection GET /key/mw after one insert (4 workers)"
        status: pass
      - kind: other
        ref: "grep gates: no LazyLock/static Store/TODO(phase-2) in src/; web::Data::new(state) only in non-test src/http/mod.rs; Settings::load only in src/main.rs"
        status: pass
    human_judgment: false
  - id: D2
    description: "GET /ip reads the host from the injected Arc<Settings>; body byte-identical to legacy"
    requirement: ARCH-03
    verification:
      - kind: unit
        ref: "src/http/mod.rs#http::tests::test_ip_body_is_ip"
        status: pass
      - kind: e2e
        ref: "live smoke on 18461: GET /ip == \"{'IP' : '127.0.0.1'}\""
        status: pass
    human_judgment: false
  - id: D3
    description: "GET/DELETE/PATCH on a missing key (including an empty store) answer 404 application/json {error: not_found, message}"
    requirement: ROBU-01
    verification:
      - kind: unit
        ref: "src/http/kv.rs#http::kv::tests::get_missing_is_404_json, get_on_empty_store_is_404_json, get_missing_twice_is_404_and_creates_nothing, delete_missing_is_404_json, delete_twice_is_200_then_404"
        status: pass
      - kind: unit
        ref: "src/http/mod.rs#http::tests::test_patch_missing_is_404_json"
        status: pass
      - kind: e2e
        ref: "live smoke on 18460: GET /key/nope -> 404 application/json {\"error\":\"not_found\",\"message\":\"key not found: nope\"}; smoke on 18461: PATCH /key/ghost -> 404"
        status: pass
    human_judgment: false
  - id: D4
    description: "PATCH on an existing key answers 501 not_implemented JSON until Phase 3"
    requirement: ROBU-01
    verification:
      - kind: unit
        ref: "src/http/mod.rs#http::tests::test_patch_existing_is_501"
        status: pass
      - kind: e2e
        ref: "live smoke on 18461: PATCH /key/mw -> 501"
        status: pass
    human_judgment: false
  - id: D5
    description: "Golden success bodies for /, /ip, insert (body order), get and delete preserved through configure(test_state())"
    requirement: HTTP-03
    verification:
      - kind: unit
        ref: "src/http/mod.rs#test_index_body_is_welcome, test_ip_body_is_ip; src/http/kv.rs#test_insert_is_ok, test_insert_map_entry_is_ok, test_insert_echo_preserves_body_order, test_insert_same_body_twice_is_idempotent, test_get_is_ok, test_delete_is_ok"
        status: pass
      - kind: e2e
        ref: "live smoke on 18460: GET / == \"Welcome to rs3\", POST /insert {\"k\":\"v\"} echo, GET /key/k == \"v\""
        status: pass
    human_judgment: false
  - id: D6
    description: "Isolated, env-free, sleep-free tests that pass in parallel, serially and five times in a row"
    requirement: TEST-01
    verification:
      - kind: other
        ref: "env -i PATH HOME cargo test; cargo test -- --test-threads=1; 5x cargo test -q (44 passed, 2 ignored each)"
        status: pass
    human_judgment: false
  - id: D7
    description: "Startup banner printed only after bind; a bind failure prints only the error line and exits 1 without panic"
    requirement: ARCH-02
    verification:
      - kind: e2e
        ref: "bind-failure smoke on 127.0.0.1:18462 (port pre-held): rc=1, stdout='', stderr='error: cannot bind 127.0.0.1:18462: Address already in use (os error 98)'"
        status: pass
    human_judgment: false

duration: 5min
completed: 2026-10-20
status: complete
---

# Phase 2 Plan 01: Injected AppState and JSON Error Contract Summary

**One `AppState { store, Arc<Settings> }` built in `main` and shared by every actix worker through `http::configure(state)`. `ApiError: ResponseError` answers `{"error","message"}` JSON, so GET, DELETE and PATCH on a missing key now return 404 `not_found` instead of panicking or answering `"[]"`. PATCH on an existing key returns 501 `not_implemented`, and every test runs on its own `test_state()`.**

## Performance

- **Duration:** about 5 min
- **Started:** 2026-10-20T18:13:19Z
- **Completed:** 2026-10-20T18:17:45Z
- **Tasks:** 2 (1 tracer, 1 TDD)
- **Files modified:** 6 source files (2 created, 4 modified) plus 2 RED-evidence files

## Accomplishments

- The process-wide `static STORE: LazyLock<Store>` and its `TODO(phase-2)` are gone. `main` builds one `AppState` outside the `HttpServer::new` factory, and `configure(state)` creates the single `web::Data<AppState>`. Workers and test apps clone only the closure.
- Added `http::error::ApiError` (thiserror) with `#[error(transparent)] Store(#[from] StoreError)`, `NotImplemented(String)` and `Internal`. Its `ResponseError` impl maps NotFound→404, InvalidInput→422, NotImplemented→501 and Internal→500, and always writes an `application/json` `{error, message}` body. The match covers every `StoreError` variant with no wildcard, so the variant 02-03 adds must get a mapping.
- `get_key`, `delete_key` and `patch_key` each decide with one `Store::get` / `Store::remove` call, which removes the review IN-01 is-empty-then-get race. `insert` returns `Result` and propagates parse and batch errors with `?`. No non-test `unwrap`/`expect` remains in `src/main.rs`, `src/state.rs` or `src/http/`.
- The banner now prints after bind succeeds (D-17). On a bind failure the only output is the `error: cannot bind` line.
- Every existing test now uses a per-test `test_state()`, built in code with `SERVER.HOST=test-host` and `SERVER.PORT=8080` and never read from the environment. The two parked PATCH tests keep their exact Phase 1 D-17 ignore reason. I added 9 tests: `get_missing_is_404_json`, the 8 Task 2 behaviours, and the rename of `test_patch_is_501` to `test_patch_existing_is_501`.

### Observed evidence (as requested by the plan output spec)

- **GET /key/nope (live, port 18460):** `404`, `content-type: application/json`, body `{"error":"not_found","message":"key not found: nope"}`.
- **Multi-worker hit count (live, port 18461, 4 workers = nproc):** 20/20 fresh-connection `GET /key/mw` returned `"1"` after one insert. `/ip` returned `"{'IP' : '127.0.0.1'}"`, `PATCH /key/mw` returned 501 and `PATCH /key/ghost` returned 404.
- **Bind failure (port 18462 pre-held by a Python listener):** exit code 1, stdout `''` (no banner), stderr `error: cannot bind 127.0.0.1:18462: Address already in use (os error 98)`, no panic.
- **RED evidence classification:** `RED_EVIDENCE_OK` (`target_test_failed`). Details are under TDD Gate Compliance.

## Task Commits

1. **Task 1 (tracer): GET /key/{key} served from one injected AppState, JSON 404 for a missing key** - `a030465` (feat)
   - The tracer feedback gate ran in auto mode. After the commit I re-ran all three tracer verifies (cargo test http::, live smoke 18460, bind-failure smoke 18462), they passed, and I proceeded to expansion.
2. **Task 2 (TDD): every key route answers JSON 404 for a missing key, goldens hold, one AppState across workers**
   - RED: `bd48605` (test)
   - GREEN: `f93e0aa` (feat)
   - REFACTOR: none needed

**Plan metadata:** recorded in the follow-up `docs(02-01)` commit.

## TDD Gate Compliance

- **RED** `test(02-01)` `bd48605`: I added the eight behaviour tests. The target `http::tests::test_patch_missing_is_404_json` failed because `patch_key` still answered 501 for every key. `test_patch_existing_is_501` also failed: the hand-built 501 body had no `not_implemented` code. I re-ran the target alone through libtest's JUnit reporter: `RUSTC_BOOTSTRAP=1 target/debug/deps/rs3-d5bd492b7c3dd84a http::tests::test_patch_missing_is_404_json --exact -Z unstable-options --format junit`, exit 101. `RUSTC_BOOTSTRAP=1` was set only on that run of the already-built test binary, never on cargo or rustc. `gsd-tools check tdd-red-evidence` returned **`RED_EVIDENCE_OK`** (`target_test_failed`). **Semantic assessment:** the target compiled, ran and failed on its planned status assertion inside `assert_json_error` (`left: 501`, `right: 404`), because `patch_key` did not consult the store yet. There was no build, load, fixture or unrelated failure. The six kv-side tests and `insert_on_one_app_visible_on_another` already passed at RED, as the plan expected, because Task 1 had already delivered GET/DELETE 404 and the shared state. Evidence is in `tdd-evidence/02-01-task2-red.{json,junit.xml}`.
- **GREEN** `feat(02-01)` `f93e0aa`: `patch_key` runs `state.store.get(&key)?`, then returns `Err(ApiError::NotImplemented(..))`. 44 passed, 2 ignored.
- **REFACTOR:** no changes, so no commit.

## Files Created/Modified

- `src/state.rs` (new): `AppState`, `AppState::new`, `#[cfg(test)] pub(crate) fn test_state()`.
- `src/http/error.rs` (new): `ApiError`, `ApiError::code`, `impl ResponseError for ApiError`, private `ErrorBody`.
- `src/lib.rs`: `pub mod state;`.
- `src/http/mod.rs`: `pub mod error;`, `configure(state)` closure, `status` on `web::Data<AppState>`, `patch_key` 404/501, the test helper `assert_json_error`, re-pointed and new tests.
- `src/http/kv.rs`: no global. Handlers use `web::Data<AppState>`. `get_key`, `delete_key` and `insert` return `Result<HttpResponse, ApiError>`. Re-pointed and new tests.
- `src/main.rs`: `AppState::new` + `rs3::http::configure(state)` outside the factory, banner after bind, and the `web` import and ordering comment removed.

## Decisions Made

- `AppState` lives at the crate root and holds no actix types (RESEARCH Open Question 3, already resolved).
- `ApiError` stays `pub` so `Internal` is reachable public API (RESEARCH Pitfall 7). There is no `#[allow]` anywhere.
- The `assert_json_error` parameter is named `expected`, not `status`. See deviation 1.
- In the kv test module, `use super::*` was removed rather than worked around. See deviation 2.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] `assert_json_error` parameter renamed from `status` to `expected`**
- **Found during:** Task 1
- **Issue:** The plan's helper signature named the parameter `status`. Inside `http::tests`, `use super::*` brings in the `#[get("/ip")] status` handler, which the actix macro generates as a unit struct. A binding named `status` then becomes a pattern for that unit struct and fails to compile (E0308/E0277: "can't compare StatusCode with http::status").
- **Fix:** Renamed the parameter to `expected`. Its type, the position of the arguments, the return type and every call site are unchanged.
- **Files modified:** src/http/mod.rs
- **Verification:** `cargo clippy --all-targets -- -D warnings` is clean and all tests pass.
- **Committed in:** a030465

**2. [Rule 3 - Blocking] Removed the unused `use super::*` in `http::kv::tests`**
- **Found during:** Task 1
- **Issue:** Once every kv test registered routes through `crate::http::configure(state.clone())` (as the plan required), nothing from `super` was used any more. `-D warnings` then failed on `unused_imports`.
- **Fix:** Removed the import, without suppressing the lint. The tests import `crate::state::test_state`, `crate::store::StoreError` and the actix test items explicitly.
- **Files modified:** src/http/kv.rs
- **Verification:** clippy is clean.
- **Committed in:** a030465

**Minor additive test strengthening (not a deviation in behaviour):** `test_insert_map_entry_is_ok` now also checks that the key was stored. `test_insert_same_body_twice_is_idempotent` now also checks `state.store.len() == 1`, the must-have "exactly one value under the key". No golden assertion was removed or weakened.

---

**Total deviations:** 2 auto-fixed (both Rule 3 compile/lint blockers caused by this plan's own changes)
**Impact on plan:** None on behaviour or contracts. The helper's call shape `assert_json_error(resp, StatusCode, code)` is what 02-03 uses.

## Issues Encountered

None.

## User Setup Required

None. No external service configuration is required.

## Next Phase Readiness

- 02-02 (config hardening) can proceed. `main` still calls `Settings::load()` exactly once, and the `error: invalid configuration: ` arm and the `/ip` body are unchanged. This plan did not touch `src/config.rs`, `src/store/`, `Cargo.toml` or `Cargo.lock`, and no `clippy.toml` exists.
- 02-03 can reuse `crate::http::tests::assert_json_error`. Its new `StoreError::MalformedJson` variant will fail to compile until it is mapped in `ApiError::code` and `status_code`, because both matches have no wildcard arm. That is intentional. `insert` still uses the `String` extractor and `insert_many(..map(Ok))?` for 02-03 to replace.
- TOOL-03 gate at the end of this plan: `cargo build --locked`, `cargo test` (44 passed, 2 ignored), `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` all pass.

## Self-Check: PASSED

- All 6 source files and both RED-evidence files exist on disk.
- Commits a030465, bd48605 and f93e0aa are ancestors of HEAD. `check evaluation-scope --plan 02-01 --commits-only` resolved all three.
- Task 1 acceptance criteria (11/11) and Task 2 acceptance criteria (7/7) pass. Every live smoke (18460, 18461, 18462) and the env-free, serial and 5x runs pass.

---
*Phase: 02-http-api-port-error-handling*
*Completed: 2026-10-20*
