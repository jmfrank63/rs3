---
phase: 02-http-api-port-error-handling
verified: 2026-10-21T11:53:27Z
status: human_needed
score: 20/20 must-haves verified
covered_files:
  - ".github/workflows/ci.yml"
  - ".planning/phases/02-http-api-port-error-handling/02-01-PLAN.md"
  - ".planning/phases/02-http-api-port-error-handling/02-01-SUMMARY.md"
  - ".planning/phases/02-http-api-port-error-handling/02-02-PLAN.md"
  - ".planning/phases/02-http-api-port-error-handling/02-02-SUMMARY.md"
  - ".planning/phases/02-http-api-port-error-handling/02-03-PLAN.md"
  - ".planning/phases/02-http-api-port-error-handling/02-03-SUMMARY.md"
  - "Cargo.toml"
  - "clippy.toml"
  - "scripts/ci-guards.sh"
  - "src/config.rs"
  - "src/http/error.rs"
  - "src/http/kv.rs"
  - "src/http/mod.rs"
  - "src/lib.rs"
  - "src/main.rs"
  - "src/state.rs"
  - "src/store/legacy.rs"
  - "src/store/mod.rs"
covered_digest: "v3:sha256:b8911b07ad72a2246003fedea006b1b7629a710fe4200093d06622ad13c19db1"
behavior_unverified: 0
overrides_applied: 3
overrides:
  - must_have: "PATCH /key/{key} returns 404 not_found JSON for a missing key and 501 not_implemented JSON for an existing key until Phase 3 (ROBU-01, D-04)."
    reason: "Superseded on purpose by Phase 3 D-12 (03-CONTEXT.md) and the 03-03-PLAN truth 'NotImplemented and test_patch_existing_is_501 are removed'. PATCH now runs the stored JavaScript (HTTP-07), so the 501 interim answer is gone. The 404 half of this must-have still holds: patch_key calls state.store.get(&key)? before queueing, enforced by http::tests::test_patch_missing_is_404_json."
    accepted_by: "phase-03 plan decision D-12 (recorded by the verifier on the final-tree re-verification)"
    accepted_at: "2026-10-21T11:53:27Z"
  - must_have: "Exactly the two parked PATCH tests (Phase 1 D-17) are ignored, keeping their 're-enabled in Phase 3 (JS runtime port)' reason"
    reason: "Superseded on purpose by Phase 3 D-12: the two #[ignore] lines were removed and test_patch_is_ok / test_patch_body_is_listing run again (src/http/mod.rs). There are now zero #[ignore] in src/, which is stricter, and CI guard (b) in scripts/ci-guards.sh forbids any ignored test."
    accepted_by: "phase-03 plan decision D-12 (recorded by the verifier on the final-tree re-verification)"
    accepted_at: "2026-10-21T11:53:27Z"
  - must_have: "Insert bodies whose nesting goes past serde_json's recursion limit return 400 malformed_json (02-03 truth 1 sub-clause)"
    reason: "Superseded inside Phase 2 by review fix WR-02 (commit 3514062, 02-REVIEW-FIX.md): *valid* JSON nested past the recursion limit, like an out-of-range number, is valid JSON that cannot be represented. It now answers 422 invalid_input, which matches ROADMAP SC-2 ('422 for a non-object or non-string values'). Truncated deep input is still 400. Pinned by kv::tests::insert_valid_deep_nesting_is_422_json and insert_truncated_deep_nesting_is_400_json."
    accepted_by: "phase-02 code-review disposition WR-02 (recorded by the verifier on the final-tree re-verification)"
    accepted_at: "2026-10-21T11:53:27Z"
re_verification:
  previous_status: human_needed
  previous_score: 18/18
  gaps_closed: []
  gaps_remaining: []
  regressions: []
human_verification:
  - test: "02-UAT item 2. Valid dotenv file with stray keys configures /ip. In an empty directory under /home/t3code, create the dotenv file with SERVER.HOST=127.0.0.1, SERVER.PORT=18469, SERVER=x and OTHER=1. Run `env -i PATH=\"$PATH\" /home/t3code/rs3/Projects/rs3/target/release/rs3` from that directory, then `curl -s http://127.0.0.1:18469/ip`."
    expected: "200 with body `\"{'IP' : '127.0.0.1'}\"`."
    why_human: "A project hook stops agents from creating or naming the dotenv file. The rule is covered in code by config::tests::merge_keeps_only_server_keys, bare_server_var_does_not_break_settings, nested_server_keys_do_not_break_settings and dotenv_source_configures_settings, but dotenvy::dotenv_iter()'s real file lookup only runs on the live binary. Still PENDING with the operator in 02-UAT.md."
  - test: "02-UAT item 3. The process environment overrides the dotenv file. Use the same file but change the host line to SERVER.HOST=10.255.255.1. Start with `env -i PATH=\"$PATH\" SERVER.HOST=127.0.0.1 .../rs3` and curl /ip on port 18469."
    expected: "/ip reports 127.0.0.1, because the environment wins over the file."
    why_human: "The same dotenv-file hook applies. Precedence is unit-tested (config::tests::env_overrides_dotenv, env_overrides_dotenv_across_case), but it has not been checked on the live binary with a real file. Still PENDING with the operator in 02-UAT.md."
---

# Phase 2: HTTP API Port & Error Handling Verification Report

**Phase Goal:** As a client of the rs3 HTTP API, I want to call every non-JavaScript route and get the same success bodies as before plus clear 4xx errors for missing keys and bad input, so that existing integrations keep working and bad requests can no longer crash the server.
**Verified:** 2026-10-21T11:53:27Z
**Status:** human_needed
**Re-verification:** Yes. The previous report went `stale` because covered files changed in Phases 3 to 6. This run checks the final milestone tree at HEAD a4a8069 (clean working tree, apart from the untracked `.planning/milestone.lock`).
**Mode:** mvp (the goal is in user-story form)

Evidence policy for this run: per the orchestrator, cargo, the smokes and the binary were **not** re-run. Gate results come from the shared log `target/tmp/d17-a4a8069.log`, run on source identical to HEAD: guards a–h ok, check-workflow OK, test-ci-guards 99 cases with 0 missed, fmt OK, clippy `-D warnings` OK, `cargo test --locked` 216 passed with 0 failed (the 1 ignored test is the doctest generated by deno_core's `extension!`), release build OK, ci-smoke OK. Route and error bodies come from Phase 6's README replay, `target/tmp/p6/readme-replay.sh` → `replay-final.out`: 130 `ok` lines, `OK (130 checks)`, run on `target/release/rs3`. The remote CI run was 39527188207 at 40afeb7. Everything else was checked by reading the code and grepping it on the final tree.

## User Flow Coverage

| # | Step of the user story | Expected | Evidence on the final tree | Status |
|---|------------------------|----------|----------------------------|--------|
| 1 | Client calls `GET /` | 200 `"Welcome to rs3"` | `src/http/mod.rs:27-30`; golden `test_index_body_is_welcome`; README replay | ✓ |
| 2 | Client calls `GET /ip` | 200 `"{'IP' : '<host>'}"` from the startup config | `status` reads `state.settings.server.host` (`mod.rs:36-39`); golden `test_ip_body_is_ip` (`test-host`); replay line 137 `"{'IP' : '127.0.0.1'}"` | ✓ |
| 3 | Client posts `/insert` (curl -d, no JSON content type) | 200 echo in body order | `kv::insert(body: web::Bytes)` (`kv.rs:23-34`); `insert_without_content_type_is_ok`, `test_insert_echo_preserves_body_order` | ✓ |
| 4 | Client reads `GET /key/{k}`, lists `GET /list`, deletes `DELETE /key/{k}` | legacy bodies; `/list` escaped | goldens `test_get_is_ok`, `test_delete_is_ok`, `list_golden_sorted`; replay lines 150-172, including an escaped `\"hi\"\n` value | ✓ |
| 5 | Client hits a missing key (GET, DELETE or PATCH, also on an empty store) | 404 JSON `{error,message}` | one `Store::get`/`remove` + `?` → `ApiError::Store(NotFound)`; replay lines 139-143 assert the exact body `{"error":"not_found","message":"key not found: greeting"}` and `application/json` on all three methods on an empty store | ✓ |
| 6 | Client sends bad input | 400 for malformed input, 422 for the wrong shape, nothing inserted | `parse_insert_body` two-pass classifier; 10+ HTTP tests that assert `store.len()==0` / `is_empty()` afterwards; replay lines 176-182 compare the exact 400/422 bodies | ✓ |
| 7 | Outcome: "bad requests can no longer crash the server" | the server keeps serving | No `.unwrap(`/`.expect(` before `#[cfg(test)]` in main/state/http/store/config (grep); `unwrap_used`/`expect_used = "deny"` with clippy green; the replay keeps one server process alive through every error path and ends `SIGTERM -> exit 0` | ✓ |

## Goal Achievement

### Observable Truths

The ROADMAP success criteria (the contract) come first, followed by the plan-specific truths that add detail. Rows marked PASSED (override) were superseded on purpose by a recorded later decision; see the frontmatter `overrides`.

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | SC-1: `GET /`, `GET /ip`, `POST /insert`, `GET /key/{key}`, `DELETE /key/{key}` return 200 with the legacy body bytes (golden tests carried over). `GET /list` keeps its shape with keys and values JSON-escaped, and a value containing `"` or `\` round-trips | ✓ VERIFIED | The goldens match the legacy `main:src/main.rs` / `main:src/services/mod.rs` bytes: `"Welcome to rs3"`, `"{'IP' : '<host>'}"`, `"{\"insert_test\":\"John Doe\"}"` (`test_insert_is_ok`, `test_insert_map_entry_is_ok`), `"John Doe"` for get and delete (the new tests now also assert the bodies, which the legacy tests only status-checked). `format_list` (`legacy.rs:150-162`) uses `Value::from(..)` escaping with the legacy `" : "` spacing. `list_escapes_quotes_and_backslashes` parses the inner text back to `a"b\c`; `format_list_round_trips` and `format_list_cannot_forge_entries`. Replay line 159 shows an escaped value on the live binary |
| 2 | SC-2: GET, DELETE or PATCH on a missing key → 404 JSON, also on an empty store (no `"[]"`). `/insert` → 400 for malformed input, 422 for a non-object or non-string values, nothing inserted on either | ✓ VERIFIED | `get_key`/`delete_key` (`kv.rs:37-52`) and `patch_key` (`mod.rs:52`, before the JS queue) each decide with one store call + `?`. Tests: `get_on_empty_store_is_404_json` (asserts body ≠ `"[]"`), `get_missing_is_404_json`, `delete_missing_is_404_json`, `test_patch_missing_is_404_json`, `insert_non_object_is_422_json`, `insert_non_string_value_is_422_and_inserts_nothing` (asserts `a` absent), `assert_insert_400` (asserts `len()==0` for each 400). Replay lines 139-143 and 176-182 |
| 3 | SC-3: store and config are created once in `main` outside `HttpServer::new` and injected through `web::Data`; a key is visible on any worker; `/ip` uses the startup host; no global map remains | ✓ VERIFIED | `src/main.rs:8,26-29`: `Settings::load()` and `AppState::new(settings, js)` + `rs3::http::configure(state)` before `HttpServer::new(move \|\| App::new().configure(cfg.clone()))`. `web::Data::new` appears exactly once in src/ (`http/mod.rs:16`). `Settings::load` is called only in main; there is no `env::var`/`vars_os`/`dotenvy` outside `src/config.rs`. The only `static`/`LazyLock` in src/ is the `#[cfg(test)]` shared JS executor in `src/js/mod.rs:261-268`, which holds an executor, not a map (Phase 3 design). `insert_on_one_app_visible_on_another` (two apps from one configure closure) passes in the 216-pass run |
| 4 | SC-4: HTTP tests use `#[actix_web::test]` and current test utilities through the same `configure(state)` as main; each test builds its own store and config in code; no `thread::sleep`, no `SERVER.*` env; passes repeatedly under parallel `cargo test` | ✓ VERIFIED | Every HTTP test builds `test_state()` (`state.rs:36-47`, `Settings::from_map` in code) and `App::new().configure(configure(state))`. There is no `SERVER.` in `src/http/`, no `set_var`/`remove_var` in src/, and no `thread::sleep` in `src/http/`. `actix_rt`/`body::Body`/`web::Json` are absent. Suite: 216 passed / 0 failed (shared log), and CI green on x86_64. See Info on the Phase 5 async staggers |
| 5 | `ApiError` is the single error producer: a JSON object with exactly the `error` and `message` strings, `application/json`, and an exhaustive match over `StoreError` (`#[from]`) | ✓ VERIFIED | `src/http/error.rs`: `Store(#[from] StoreError)`; `code()`/`status_code()` have no wildcard arm (NotFound 404, MalformedJson 400, InvalidInput 422, plus the Phase 3/5 JS variants); `ErrorBody { error, message }` via `.json(..)`. `assert_json_error` checks the content type and both fields |
| 6 | PATCH on an existing key → 501 `not_implemented` until Phase 3 | PASSED (override) | Override: superseded by Phase 3 D-12. PATCH now runs the script (`test_patch_existing_runs_script`, `test_patch_runs_script_200`). The 404 half still holds (`test_patch_missing_is_404_json`) |
| 7 | The banner prints only after bind; a bind failure prints only `error: cannot bind` and exits 1, with no panic (D-17) | ✓ VERIFIED | `src/main.rs:29-37`: `println!` sits after the `bind` match whose `Err` arm returns `ExitCode::FAILURE`. Live evidence: the README replay `cfgline` checks (lines 236-241) compare the whole output for a busy port (`Address already in use (os error 98)`), port 80 and an empty host, and all are ok in `replay-final.out` |
| 8 | `Settings::load()` never writes the environment: `dotenv_iter()` + `vars_os()` → pure `merge_dotenv_env` → `from_map`. A missing file is ignored; other dotenv errors → a `.env: ` ConfigError, never half-loaded | ✓ VERIFIED | `config.rs:86-107`; `merge_dotenv_env` returns at the first `item.map_err(dotenv_error)?` (`:66`). Tests: `malformed_dotenv_is_error_with_env_prefix`, `unreadable_dotenv_is_error`, `missing_dotenv_file_uses_env_only`, `dotenv_source_configures_settings`. The live malformed-file check is 02-UAT item 1, which the operator passed (`.env: Error parsing line: 'SERVER.PORT 5000'`, exit 1) |
| 9 | The env wins over the dotenv file (any key case); the first file occurrence wins; only server host/port are accepted; non-UTF-8 env entries are skipped; a BOM is an error; no defaults and no `__` alias | ✓ VERIFIED | `SERVER_KEYS` exact allowlist (`config.rs:16,75`). This is stricter than the plan's `server.` prefix after review fix bd2137e and still meets the D-16 intent. Tests: `env_overrides_dotenv`, `env_overrides_dotenv_across_case`, `dotenv_first_occurrence_wins(_across_case)`, `merge_keeps_only_server_keys`, `nested_server_keys_do_not_break_settings`, `bom_prefixed_dotenv_is_error`, `underscore_names_are_not_mapped`. Replay: the `SERVER_HOST/SERVER_PORT` names are not read; lowercase or mixed-case names start the server. The live file paths are UAT items 2 and 3 (human) |
| 10 | Config tests use in-memory dotenv content and explicit env lists; none reads or mutates the process env or cwd (TEST-02) | ✓ VERIFIED | Config tests feed `dotenvy::from_read_iter(bytes)` / constructed errors plus a `Vec<(OsString, OsString)>`; none calls `load()`. No `set_var`/`remove_var` in src/ |
| 11 | `/insert` reads raw `web::Bytes` with any or no Content-Type or charset; strict UTF-8; `{}` → 200 `"{}"` no-op; >256 KiB → actix's built-in 413 | ✓ VERIFIED | `kv.rs:24-33`; no `PayloadConfig` override anywhere, so actix's default 262_144-byte limit applies. Tests: `insert_without_content_type_is_ok`, `insert_with_bogus_charset_is_ok`, `insert_invalid_utf8_is_400_json` (FF FE), `insert_empty_object_is_noop`, `insert_over_256_kib_is_413` |
| 12 | 400/422 classification (current contract): syntax, EOF, empty, trailing characters, invalid UTF-8 or an unpaired surrogate escape → 400 `malformed_json`; valid JSON that is not an object, has a non-string value, an out-of-range number or valid over-deep nesting → 422 `invalid_input` naming the key | ✓ VERIFIED | `legacy.rs:36-69`: pass 1 `IgnoredAny` + `classify()` (Syntax/Eof/Io → MalformedJson); pass 2 surrogate → 400, otherwise `unrepresentable` → 422 via `FindBadKey`. More than 25 `parse_*` unit tests, plus HTTP tests `insert_out_of_range_number_is_422_naming_key`, `insert_unpaired_surrogate_escape_is_400_json`, `insert_valid_deep_nesting_is_422_json`, `insert_truncated_deep_nesting_is_400_json` |
| 13 | (02-03 plan text) Nesting past the recursion limit → 400 | PASSED (override) | Override: superseded inside Phase 2 by review fix WR-02 (3514062). Valid deep nesting is now 422, which matches SC-2; truncated deep input stays 400 |
| 14 | Infallible `Store::insert_all` under one guard, validate-then-write; the doc says concurrent readers may see a partial batch (D-13) | ✓ VERIFIED | `store/mod.rs:49-63` (one `pin()`, returns `usize`, caveat in the doc). `kv::insert` parses the whole body before `insert_all`. Tests: `insert_all_writes_all_and_counts`, `concurrent_inserts_are_all_visible` |
| 15 | `/list` keeps the legacy envelope sorted by key byte order (B before a); empty → `"[]"`; the `,]` rewrite is removed; one shared `legacy::format_list` codec | ✓ VERIFIED | `kv.rs:55-58` calls `legacy::format_list(&state.store.list())`; no `replace(",]"` in src/. Tests: `list_golden_sorted` `"[{\"B\" : \"3\"},{\"a\" : \"1\"},{\"b\" : \"2\"}]"`, `list_empty_is_brackets_json`. Phase 3 `op_rs3_list` reuses the codec (`test_patch_listing_matches_list_with_escapes_and_order`) |
| 16 | Lint gate: `[lints.clippy] unwrap_used = "deny"`, `expect_used = "deny"`; `clippy.toml` test allowances; no suppression attribute; no non-test unwrap/expect | ✓ VERIFIED | Confirmed in `Cargo.toml` and `clippy.toml`. Grep: 0 `#[allow(`/`#[expect(`/`cfg_attr(..allow/expect)` in src/; an awk scan of the code before `#[cfg(test)]` in main/state/http/store/config finds no `.unwrap(`/`.expect(`. Clippy `-D warnings` OK and CI guards [a] and [c] ok (shared log) |
| 17 | Phase gate: build, test, clippy `-D warnings` and `fmt --check` all exit 0 | ✓ VERIFIED | Shared log `d17-a4a8069.log`: `GATE fmt OK`, `GATE clippy OK`, `216 passed; 0 failed`, `GATE release build OK`; remote CI run 39527188207 green on x86_64 |
| 18 | (Plan text) Exactly the two parked PATCH tests are `#[ignore]`d | PASSED (override) | Override: superseded by Phase 3 D-12. 0 `#[ignore]` in src/ (grep), both tests are active (`mod.rs:643-675`), and CI guard [b] forbids any ignored test |
| 19 | Module direction (ARCH-01 kept): store and config import no web, JS or other rs3 module; state imports no web or JS-engine crate; http never imports deno | ✓ VERIFIED | Grep: no `crate::` in `src/store/*` or `src/config.rs`; no `actix` in store, config, state or js; no `deno`/`v8` in `src/http/*`, `state.rs`, store or config. `state.rs` now imports `crate::js::executor::JsExecutor` (an rs3 handle type, Phase 3 D-06), not deno_core |
| 20 | Edge behaviours: the echo keeps body order; the same insert twice is idempotent; for a duplicate key the last value wins; inserts overwrite; non-ASCII is byte-exact; DELETE gives 200 then 404; a repeated missing GET creates nothing; racing removes → exactly one success; a repeated rejected body is stable | ✓ VERIFIED | Tests `test_insert_echo_preserves_body_order`, `test_insert_same_body_twice_is_idempotent`, `insert_duplicate_key_last_value_wins`, `insert_overwrites_existing_key`, `insert_non_ascii_round_trips`, `delete_twice_is_200_then_404`, `get_missing_twice_is_404_and_creates_nothing`, `concurrent_remove_of_one_key_succeeds_once`, `insert_rejected_twice_leaves_store_unchanged`, all in the 216-pass run |

**Score:** 20/20 truths verified (17 VERIFIED + 3 PASSED (override); 0 present-but-behavior-unverified)

### Superseded Plan-Level Must-Haves (recorded, not gaps)

| Phase 2 must-have | Superseded by | Citation | What still holds |
|-------------------|---------------|----------|------------------|
| PATCH existing → 501 `not_implemented` (02-01) | Phase 3 D-12 / 03-03 plan truth | `.planning/phases/03-javascript-runtime-port/03-CONTEXT.md:56`; `03-03-PLAN.md:35` | 404 for a missing key before queueing |
| Exactly two `#[ignore]` PATCH tests (02-01, 02-03) | Phase 3 D-12 | `03-CONTEXT.md:56`; `03-03-PLAN.md:36` | Zero ignores (stricter); CI guard [b] |
| Over-deep nesting → 400 (02-03 truth 1) | Phase 2 review WR-02 | `02-REVIEW-FIX.md:129`, commit 3514062 | Truncated deep input still 400; valid deep → 422 per SC-2 |
| `AppState { store, settings }` (02-01) | Phase 3 D-06/D-08 added `js: JsExecutor` | `03-03-PLAN.md:117` | Additive only. Still one AppState built once in main; counted as VERIFIED in truth 3, no override needed |

### Prohibitions

| Plan | Prohibition | Tier | Disposition |
|------|-------------|------|-------------|
| 02-02 | MUST NOT fall back silently to a default, partial or guessed configuration | test | ✓ VERIFIED. Wired enforcement: `missing_server_is_error`, `missing_port_is_error`, `invalid_port_is_error`, `underscore_names_are_not_mapped`, `malformed_dotenv_is_error_with_env_prefix`, `unreadable_dotenv_is_error`, plus the live replay `cfgline` checks (host only, port only, `abc`, `70000`, empty → exit 1 with the exact error line) |
| 02-01, 02-02, 02-03 | MUST NOT make the gate green by suppression | test | ✓ VERIFIED. This is now wired, repo-resident enforcement: `scripts/ci-guards.sh` guards [a] (no allow/expect/cfg_attr), [b] (no ignored tests) and [c] (unwrap_used/expect_used deny, no allow), run by `.github/workflows/ci.yml` and self-tested by `scripts/test-ci-guards.sh` (99 cases). All ok in the shared log. The previous run's flag (02-UAT item 4) is closed: the operator passed it |

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `src/state.rs` | `pub struct AppState`, `AppState::new`, `test_state()` | ✓ VERIFIED | 47 lines; used by main.rs and every HTTP test |
| `src/http/error.rs` | `ResponseError for ApiError` | ✓ VERIFIED | Exhaustive maps; `malformed_json` present |
| `src/http/mod.rs` | `pub fn configure(state: AppState)`, index, status, patch_key | ✓ VERIFIED | Called from main.rs:27 and all tests |
| `src/http/kv.rs` | handlers on `web::Data<AppState>`, `body: web::Bytes` | ✓ VERIFIED | Registered through `.configure(kv::configure)` |
| `src/main.rs` | `AppState::new`, configure per worker, banner after bind | ✓ VERIFIED | 46 lines |
| `src/config.rs` | `pub fn merge_dotenv_env`, `from_sources`, `load` | ✓ VERIFIED | 19 config tests |
| `src/store/mod.rs` | `pub fn insert_all`, `StoreError::MalformedJson` | ✓ VERIFIED | Used by `kv::insert` |
| `src/store/legacy.rs` | `classify()`, escaped `format_list` | ✓ VERIFIED | Used by `kv::insert`, `kv::list` and the Phase 3 ops |
| `Cargo.toml` | `[lints.clippy]` deny | ✓ VERIFIED | `unwrap_used`/`expect_used = "deny"` |
| `clippy.toml` | `allow-unwrap-in-tests = true` | ✓ VERIFIED | Both allowances present |

### Key Link Verification

| From | To | Via | Status |
|------|----|-----|--------|
| `src/main.rs` | `src/http/mod.rs` | `rs3::http::configure(state)`, then `cfg.clone()` per worker | ✓ WIRED |
| `src/http/mod.rs` | `src/state.rs` | `web::Data::new(state)` once, `app_data(data.clone())` | ✓ WIRED |
| `src/http/kv.rs` | `src/http/error.rs` | `Result<HttpResponse, ApiError>` + `?` | ✓ WIRED |
| `src/http/error.rs` | `src/store/mod.rs` | `Store(#[from] StoreError)` | ✓ WIRED |
| `Settings::load` | `dotenvy::dotenv_iter()` / `std::env::vars_os()` | `from_sources(..)` | ✓ WIRED |
| `Settings::from_sources` | `merge_dotenv_env` → `from_map` | `Self::from_map(merge_dotenv_env(file, env)?)` | ✓ WIRED |
| `src/main.rs` | `src/config.rs` | `"error: invalid configuration: {e}"` arm, exit 1 | ✓ WIRED |
| `kv::insert` | `legacy::parse_insert_body` | `from_utf8` → `parse_insert_body(text)?` | ✓ WIRED |
| `parse_insert_body` | `StoreError` | `e.classify()` | ✓ WIRED |
| `kv::list` | `legacy::format_list` | `legacy::format_list(&state.store.list())` | ✓ WIRED |
| `Cargo.toml [lints.clippy]` | `clippy.toml` | test allowances | ✓ WIRED (clippy green) |

### Data-Flow Trace (Level 4)

| Artifact | Data Variable | Source | Produces Real Data | Status |
|----------|---------------|--------|--------------------|--------|
| `GET /ip` | `state.settings.server.host` | `Settings::load()` in main → `Arc<Settings>` | Yes (replay: the env-supplied `127.0.0.1`) | ✓ FLOWING |
| `GET /key`, `/list`, `DELETE` | store values | one `papaya::HashMap` in `AppState.store`, written by `insert_all` | Yes (replay insert → list → get round trip) | ✓ FLOWING |

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
|----------|---------|--------|--------|
| Full gate | shared log `target/tmp/d17-a4a8069.log` (not re-run, per the orchestrator) | guards a–h ok; fmt, clippy, test (216/0), release build and ci-smoke OK | ✓ PASS (shared evidence) |
| Routes + error bodies on the release binary | `target/tmp/p6/replay-final.out` | 130 ok, `OK (130 checks)` | ✓ PASS (shared evidence) |
| Golden / 404 / 400 / 422 tests exist | read `src/http/{mod,kv}.rs` | every named test present, with no `#[ignore]` | ✓ PASS |
| Live dotenv-file paths | — | blocked by a project hook | ? SKIP → human (UAT 2, 3) |

### Probe Execution

No `scripts/*/tests/probe-*.sh` exists and no Phase 2 plan declares a probe. Not applicable.

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|-------------|-------------|--------|----------|
| HTTP-01 | 02-01 | `GET /` 200 `"Welcome to rs3"` | ✓ SATISFIED | Truth 1 |
| HTTP-02 | 02-01 | `GET /ip` configured host, legacy format | ✓ SATISFIED | Truths 1, 3 |
| HTTP-03 | 02-01, 02-03 | `/insert` inserts every pair, echoes | ✓ SATISFIED | Truths 1, 11, 14, 20 |
| HTTP-04 | 02-03 | `/list` legacy shape, escaped | ✓ SATISFIED | Truths 1, 15 |
| HTTP-05 | 02-01 | `GET /key/{key}` | ✓ SATISFIED | Truth 1 |
| HTTP-06 | 02-01 | `DELETE /key/{key}` returns the old value | ✓ SATISFIED | Truths 1, 20 |
| HTTP-08 | 02-01, 02-03 | actix-web 4.x API | ✓ SATISFIED | Truths 4, 16; actix-web 4.17.0, no actix-rt/`web::Json`/`body::Body` |
| ROBU-01 | 02-01 | 404 JSON on a missing key (GET/DELETE/PATCH) | ✓ SATISFIED | Truths 2, 5 |
| ROBU-02 | 02-03 | 400 malformed / 422 wrong shape, nothing inserted | ✓ SATISFIED | Truths 2, 12 |
| ARCH-02 | 02-01 | store created once, injected, no global | ✓ SATISFIED | Truth 3 (the OpState half was delivered in Phase 3 through `ExecCtx`) |
| ARCH-03 | 02-01, 02-02 | config loaded once, injected | ✓ SATISFIED | Truths 3, 8 |
| TEST-01 | 02-01, 02-03 | isolated store per test, no sleep, parallel-deterministic | ✓ SATISFIED | Truths 4, 17 |
| TEST-02 | 02-01, 02-02, 02-03 | tests independent of the process env | ✓ SATISFIED | Truths 4, 10 |

All 13 phase requirement IDs appear in at least one plan's `requirements` field. REQUIREMENTS.md maps no further IDs to Phase 2, so there are no orphaned requirements.

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| src/store/legacy.rs | 18, 39, 71 | `XXX` match | ℹ️ Info | False positive: the `\uXXXX` escape notation in comments, not a debt marker |
| src/http/mod.rs | 425, 459 | `tokio::time::sleep(50 ms)` in the Phase 5 timeout tests | ℹ️ Info | An async stagger that orders a second request behind a running loop, not `thread::sleep`. It is not used for state sync, the tests use dedicated per-test state, and the margins are wide (the asserted limits are 500–1000 ms). Owned and reviewed in Phase 5; SC-4 is not violated |
| src/js/mod.rs | 261-268 | `static LazyLock` | ℹ️ Info | `#[cfg(test)]` shared JS executor (Phase 3 design). It holds no store, so SC-3/TEST-01 are unaffected |

There are no TBD/FIXME/TODO/HACK/PLACEHOLDER markers in any covered file, no stubs, no empty handlers and no hardcoded empty data paths.

### Human Verification Required

These are the two items still PENDING in 02-UAT.md. Items 1 (malformed dotenv stops startup) and 4 (no suppression) were passed by the operator and are not raised again.

### 1. Valid dotenv file with stray keys configures /ip (02-UAT item 2)

**Test:** In an empty directory under /home/t3code, create the dotenv file with `SERVER.HOST=127.0.0.1`, `SERVER.PORT=18469`, `SERVER=x` and `OTHER=1`. Run `env -i PATH="$PATH" /home/t3code/rs3/Projects/rs3/target/release/rs3` from there, then run `curl -s http://127.0.0.1:18469/ip`.
**Expected:** 200 with body `"{'IP' : '127.0.0.1'}"`.
**Why human:** A project hook stops agents from creating or naming the dotenv file. The in-code coverage is `merge_keeps_only_server_keys`, `bare_server_var_does_not_break_settings` and `dotenv_source_configures_settings`.

### 2. The process environment overrides the dotenv file (02-UAT item 3)

**Test:** Change the host line to `SERVER.HOST=10.255.255.1`. Start with `env -i PATH="$PATH" SERVER.HOST=127.0.0.1 .../rs3` and curl `/ip` on port 18469.
**Expected:** `/ip` reports `127.0.0.1`.
**Why human:** The same hook applies. The in-code coverage is `env_overrides_dotenv` and `env_overrides_dotenv_across_case`.

### Gaps Summary

There are no gaps. On the final milestone tree, all four ROADMAP Phase 2 success criteria hold:
- Every non-JS route has golden bodies that match the legacy bytes.
- Missing keys answer a JSON 404 on GET, DELETE and PATCH, including on an empty store.
- Bad insert bodies answer 400 or 422 and write nothing.
- The store and settings are built once in main and injected through a single `web::Data`, with no global map.
- Tests are per-state and env-free on actix-web 4.17 utilities.

Three plan-level must-haves were superseded on purpose and are recorded as overrides with citations, not as gaps: the PATCH-501 interim answer and the two parked `#[ignore]`s (both Phase 3 D-12), and over-deep nesting answering 400 (Phase 2 review WR-02). The "no suppression" prohibition, flagged last time, now has wired CI enforcement (guards a/b/c). Status is `human_needed` only because the two live dotenv-file checks (02-UAT items 2 and 3) are still pending with the operator. Agents cannot carry them out under the project hook.

---

_Verified: 2026-10-21T11:53:27Z_
_Verifier: Claude (gsd-verifier)_
