# Phase 2: HTTP API Port & Error Handling - Context

**Gathered:** 2026-10-20
**Status:** Ready for planning
**Mode:** Smart discuss (autonomous, non-interactive — every grey area auto-accepted with the Recommended answer)

<domain>
## Phase Boundary

Bring the six non-JS routes (`GET /`, `GET /ip`, `POST /insert`, `GET /key/{key}`,
`DELETE /key/{key}`, `GET /list`) onto injected state with typed errors: same success bodies
as before, 404 / 400 / 422 JSON errors instead of panics or the `"[]"` quirk, a correctly
escaped `/list`, no global store, config loaded once and injected, and isolated,
env-free, sleep-free tests that use the same `configure(state)` helper as `main`. Also fixes
the Phase 1 review warnings WR-01/WR-02 in the config loader (operator directive).

Not in this phase: running JavaScript / the five ops (Phase 3 — PATCH keeps answering 501
for an existing key), CI (Phase 4), JS limits (Phase 5), Docker/README (Phase 6).

Requirements: HTTP-01, HTTP-02, HTTP-03, HTTP-04, HTTP-05, HTTP-06, HTTP-08, ROBU-01,
ROBU-02, ARCH-02, ARCH-03, TEST-01, TEST-02.

</domain>

<decisions>
## Implementation Decisions

### Error contract (status codes and body)
- **D-01:** Every error response is a real JSON object (not a double-encoded string) with content type `application/json`: `{"error": "<machine code>", "message": "<human-readable detail>"}`. Codes used in this phase: `not_found`, `malformed_json`, `invalid_input`, `not_implemented`, `internal`. Phase 3 and Phase 5 add `js_error`, `js_depth_limit` and `js_timeout` in the same shape. — **Reversibility:** reversible (no client depends on error bodies; errors used to be panics)
- **D-02:** Errors are typed per layer. `StoreError` stays in `store`. A new `http::error::ApiError` (thiserror) implements `actix_web::ResponseError` (status code + JSON body) and has `From<StoreError>`. Handlers return `Result<HttpResponse, ApiError>`. Status map: `NotFound` → 404, malformed JSON → 400, wrong shape / non-string value → 422, PATCH-not-yet → 501, host fault → 500.
- **D-03:** `POST /insert` reads the raw body (`web::Bytes`, not `web::Json` — REQUIREMENTS out-of-scope). Invalid UTF-8, an empty body, or JSON that fails to parse (serde_json syntax/EOF/IO) → **400**. Valid JSON that is not an object, or an object with any non-string value → **422**, and the message names the offending key. Validation completes before any write, so nothing is inserted on either error. `{}` → 200 with body `"{}"` (no-op). The codec must distinguish malformed from wrong-shape input, e.g. a `StoreError::MalformedJson` variant (400) next to `InvalidInput` (422); exact naming is at Claude's discretion.
- **D-04:** `GET /key/{key}` and `DELETE /key/{key}` on a missing key → **404** JSON error, also on an empty store (the `"[]"` quirk and the is-empty-then-get race are removed: a single `Store::get` / `Store::remove` call decides). `PATCH /key/{key}` on a missing key → **404** now (ROADMAP SC-2). An existing key still answers **501** `not_implemented` until Phase 3.
- **D-05:** Request bodies over actix's default 256 KiB payload limit get actix's built-in **413**. This is documented (README in Phase 6), not converted to JSON.

### `/list` output and escaping
- **D-06:** `GET /list` keeps the legacy envelope and shape: a JSON string containing `[{"k" : "v"},{"k2" : "v2"}]`. Keep the legacy `" : "` spacing and `,` separators, but serialise every key and value with serde_json string escaping. Output is byte-identical to the legacy output for any data that needs no escaping, and the inner text is always valid JSON (a value containing `"` or `\` round-trips). — **Reversibility:** reversible
- **D-07:** Entries stay sorted by key (Rust `String` byte order). This already shipped in Phase 1 (review IN-04), so Phase 2 golden tests must not treat sorting as new.
- **D-08:** An empty store still answers `"[]"`.
- **D-09:** Drop the legacy `.replace(",]", "]")` quirk, since it corrupted any value containing `,]`. Replace the `format_list_keeps_legacy_replace_quirk` test with escaping and round-trip tests: parse the inner text back with serde_json and compare it with the stored pairs. `/list` and Phase 3's `op_rs3_list` keep sharing this one codec function.

### State injection and test harness
- **D-10:** Introduce `AppState { store: Store, settings: Arc<Settings> }` (cheap `Clone`; Phase 3 adds a `js` executor field). `http::configure(state: AppState)` returns a closure that registers `web::Data<AppState>` (created once and cloned) plus every route. `main` and the tests call the same helper. Exact signature (`impl Fn(&mut web::ServiceConfig) + Clone` or similar) is at Claude's discretion; it must be callable once per actix worker.
- **D-11:** `main` loads `Settings` and creates the `Store` and `AppState` exactly once, outside the `HttpServer::new` factory closure. The temporary `static STORE: LazyLock<Store>` in `http::kv` and its `TODO(phase-2)` are deleted, and no global map remains anywhere (ARCH-02). `GET /ip` reads the host from the injected settings and never re-reads the environment (ARCH-03). A test builds two separate `init_service` apps from one `AppState` (simulating two workers) and checks that a key inserted through one is visible through the other.
- **D-12:** Each test builds its own `AppState` in code (fresh `Store::new()` + `Settings::from_map(...)`), with no `SERVER.*` environment variables, no `thread::sleep` and no shared global. The suite must pass repeatedly under default parallel `cargo test`. The two parked PATCH tests (`#[ignore]` until Phase 3) are re-pointed at a per-test `AppState` now, so that they are not flaky when re-enabled (review IN-05).
- **D-13:** Turn on `[lints.clippy] unwrap_used = "deny"` and `expect_used = "deny"` in `Cargo.toml` for non-test code. Add a `clippy.toml` with `allow-unwrap-in-tests = true` and `allow-expect-in-tests = true` so tests keep using them. **No `#[allow(...)]` / `#[cfg_attr(..., allow(...))]` attributes anywhere** (operator: no gate-greening by suppression; Phase 4 adds a CI grep for it). The handler insert path becomes unwrap-free (review IN-03: an infallible write after validation, or `insert_many` taking validated pairs; at Claude's discretion). Document that concurrent readers may see a partially written batch.

### Config loading hardening (operator directive: review WR-01 / WR-02)
- **D-14 (WR-02):** `Settings::load()` must not mutate the process environment. Read the dotenv file with `dotenvy::dotenv_iter()`, which uses the same lookup as before (current directory, then its ancestors), and collect its pairs into a map. Overlay the real process environment on top: process env wins, which matches dotenvy's old "never override" semantics. Feed the result through the existing `Settings::from_map` builder path. Read the env with `std::env::vars_os()` and skip non-UTF-8 entries rather than panicking. With this change, `load()` is sound regardless of thread count, and the ordering comment in `main` stops being load-bearing.
- **D-15 (WR-01):** A missing dotenv file is ignored, but any other dotenv error (parse error at line N, unreadable file) becomes a startup error: `error: invalid configuration: .env: <detail>`, exit code 1. Because the file is collected into a map before anything is applied, a malformed file is never half-loaded. Add unit tests for the merge and error logic that do not touch the real process env or cwd. For example, factor the merge into a pure function over a dotenv iterator/reader plus an env map, then test it with in-memory content.
- **D-16 (review IN-07):** Only keys whose lowercased name starts with `server.` enter the config map, so an unrelated `SERVER` or other variable can no longer collide with the `server` table. The accepted names do not change (still dotted `SERVER.HOST` / `SERVER.PORT`), and there are still no defaults and no `SERVER__HOST` alias (OPS-02, v2).
- **D-17 (review IN-06):** Print the `Starting Http server at host address: ...` banner only after `bind` succeeds and before `run`, so a bind failure prints only the `error:` line.

### Golden / success tests
- **D-18:** Carry over golden body-byte tests for `GET /` (`"Welcome to rs3"`), `GET /ip` (`"{'IP' : '<host>'}"`), `POST /insert` (echo of the object in input order), `GET /key/{key}` (`"<value>"`) and `DELETE /key/{key}` (`"<old value>"`), all through `configure(state)`. Add `/list` golden and escaping tests and an error-path test for every new status (404 GET/DELETE/PATCH, including the empty store; 400 malformed / empty / invalid UTF-8; 422 non-object and non-string value, with the store unchanged afterwards).

### Claude's Discretion
- Exact module and file layout (`http/error.rs`, `http/state.rs` or `state.rs` at crate root), helper and test names, and error message wording beyond the fixed `error` codes.
- Whether `insert_many` changes signature or an `insert_all` is added (D-13).
- Whether the PATCH 501/404 logic lives in `http/mod.rs` or moves into `http/kv.rs`.
- Commit granularity within plans (TDD RED/GREEN pairs are welcome, as in Phase 1).

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Project scope
- `.planning/PROJECT.md`: core value, constraints, key decisions
- `.planning/REQUIREMENTS.md`: HTTP-01..06, HTTP-08, ROBU-01, ROBU-02, ARCH-02, ARCH-03, TEST-01, TEST-02 (this phase), plus the Out of Scope table (`web::Json` for `/insert`)
- `.planning/ROADMAP.md` §Phase 2: success criteria and notes

### Prior phase
- `.planning/phases/01-toolchain-dependencies-foundation/01-CONTEXT.md`: locked Phase 1 decisions (D-06..D-13, D-16, D-19, D-22)
- `.planning/phases/01-toolchain-dependencies-foundation/01-REVIEW.md`: WR-01, WR-02, IN-01..IN-07 with suggested fixes
- `.planning/phases/01-toolchain-dependencies-foundation/01-REVIEW-DISPOSITION.md`: which findings are routed to Phase 2

### Research
- `.planning/research/SUMMARY.md`: resolved decisions (c), (f), (h), (i)
- `.planning/research/FEATURES.md`: HTTP status code contract table, T1–T4, D4
- `.planning/research/ARCHITECTURE.md`: `AppState`, `configure(state)`, `ApiError` / `ResponseError` sketch, layer error flow
- `.planning/research/PITFALLS.md`: state created inside the `HttpServer::new` closure, edition-2024 `set_var`, test isolation
- `.planning/research/STACK.md`: actix-web 4.17 test utilities (`init_service`, `call_service`, `read_body`, `call_and_read_body`), `test as atest` import

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- `src/store/mod.rs` `Store`: papaya newtype with `get/insert/insert_many/remove/list/len/is_empty`, plus `StoreError::{NotFound, InvalidInput}`. Already owned-value and guard-free.
- `src/store/legacy.rs`: `parse_insert_body` (needs the 400/422 split), `format_insert_echo` (byte-identical echo, keep), `format_list` (rewrite per D-06..D-09).
- `src/config.rs` `Settings::from_map`: the in-code construction path that tests and the new `load()` both go through.
- Existing actix 4.17 test idioms in `src/http/mod.rs` and `src/http/kv.rs` (`atest::init_service`, `call_service`, `read_body`, `call_and_read_body`, `web::Bytes::from_static`).

### Established Patterns
- Success responses are JSON-encoded strings via `HttpResponse::Ok().json(String)`. This double-encoded envelope is preserved.
- Routes use actix attribute macros (`#[get]`, `#[post]`, `#[delete]`, `#[patch]`) and are registered from `configure` functions.
- `#![forbid(unsafe_code)]` crate-wide, plus `[lints.rust] unsafe_code = "forbid"`.
- `main` returns `ExitCode` and prints a one-line `error: ...` on startup failure.

### Integration Points
- `src/main.rs`: `Settings::load()` → `rs3::js::init_platform()` → `HttpServer::new(move || App::new().app_data(..).configure(..))`. Switch to `configure(state)`.
- `src/http/kv.rs` `STORE` global: delete it and switch handlers to `web::Data<AppState>`.
- `src/http/mod.rs` parked PATCH tests reference `crate::http::kv::STORE`: re-point them at a per-test `AppState`.
- Phase 3 will add `AppState.js` and wire PATCH to the JS executor; the `op_rs3_*` ops will reach the same `Store` through `OpState`.

</code_context>

<specifics>
## Specific Ideas

- **Git identity (user instruction):** the host has no global git identity. Commit with env vars `GIT_AUTHOR_NAME="Johannes Maria Frank"`, `GIT_AUTHOR_EMAIL="jmfrank63@gmail.com"`, `GIT_COMMITTER_NAME="Johannes Maria Frank"`, `GIT_COMMITTER_EMAIL="jmfrank63@gmail.com"`. **Do not modify git config.** End commit messages with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- **Never `git push`** (operator directive for this autonomous run). Stay on branch `modernize-toolchain`.
- Keep `target/` inside the project dir (`/tmp` is about 96% full). The V8 build is already cached in `target/`.
- The TOOL-03 gate (`cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`) must pass at the end of every plan.
- `/list` byte parity for simple data is a deliberate choice: the only visible `/list` changes compared with legacy are escaping, the dropped `,]` quirk, and sorting (Phase 1).

</specifics>

<deferred>
## Deferred Ideas

- RFC 9457 `application/problem+json` error bodies: v2 (APIV).
- JSON body for actix's built-in 413 and for unknown-route 404s: v2.
- `SERVER__HOST` alias and config defaults: OPS-02 (v2).
- Request logging / tracing: OPS-01 (v2).
- Versioned API without the double-encoded string envelope: APIV-01 (v2).

</deferred>

---

*Phase: 02-http-api-port-error-handling*
*Context gathered: 2026-10-20 (smart discuss, auto-accepted recommendations)*
