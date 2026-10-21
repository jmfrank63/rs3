---
phase: 03-javascript-runtime-port
verified: 2026-10-21T11:54:15Z
status: passed
score: 26/26 must-haves verified
covered_files:
  - ".github/workflows/ci.yml"
  - ".planning/phases/03-javascript-runtime-port/03-01-PLAN.md"
  - ".planning/phases/03-javascript-runtime-port/03-01-SUMMARY.md"
  - ".planning/phases/03-javascript-runtime-port/03-02-PLAN.md"
  - ".planning/phases/03-javascript-runtime-port/03-02-SUMMARY.md"
  - ".planning/phases/03-javascript-runtime-port/03-03-PLAN.md"
  - ".planning/phases/03-javascript-runtime-port/03-03-SUMMARY.md"
  - "Cargo.lock"
  - "Cargo.toml"
  - "clippy.toml"
  - "docs/js-ops.md"
  - "requests/bin.js"
  - "requests/bin.txt"
  - "requests/fac.js"
  - "requests/fac.txt"
  - "requests/rs3_http_localhost_5000.http"
  - "requests/rs3_https_rs3_cwe.http"
  - "requests/store.js"
  - "requests/store.txt"
  - "requests/sum.js"
  - "requests/sum.txt"
  - "scripts/ci-guards.sh"
  - "scripts/ci-smoke.sh"
  - "src/example.js"
  - "src/http/error.rs"
  - "src/http/kv.rs"
  - "src/http/mod.rs"
  - "src/js/executor.rs"
  - "src/js/mod.rs"
  - "src/js/ops.rs"
  - "src/js/runtime.rs"
  - "src/main.rs"
  - "src/state.rs"
  - "src/store/legacy.rs"
covered_digest: "v3:sha256:20b6b0f6e0f673c0b7423eed701ed3393f77d37654188a4c8667f666d4bf563b"
behavior_unverified: 0
overrides_applied: 4
overrides:
  - must_have: "docs/js-ops.md discloses the sandbox and the known limits: until Phase 5, nesting through op_rs3_patch is unbounded (about 235 debug / 778 release levels, about 800 MB, 2.7 s), and a non-terminating script such as while(true){} occupies a JS worker until the Phase 5 timeout exists"
    reason: "Superseded by Phase 5 decision D-12 (05-CONTEXT.md): rewrite the docs/js-ops.md limits section with no more 'until Phase 5'. Phase 5 added the depth limit (4, commit 92c6198) and the 5000 ms timeout (commit 7d704f7). The current docs state both limits, the 422 js_depth_limit and 504 js_timeout bodies, that writes made before a stop are kept, and the remaining heap-exhaustion limit (v2 JSHD-01). The intent of the must-have (no undisclosed limit) still holds, and operator UAT 03-UAT.md test 1 confirmed the limits are disclosed without being softened."
    accepted_by: "gsd-verifier (orchestrator-directed re-verification; Phase 5 D-12)"
    accepted_at: "2026-10-21T11:54:15Z"
  - must_have: "actix workers never run V8: the live binary has as many threads named rs3-js-* as nproc reports"
    reason: "Superseded in its thread-count detail by Phase 5 D-06 and commit 7d704f7: each worker rs3-js-{i} now spawns a watchdog thread rs3-js-{i}-wd, so a rs3-js-* glob matches 2 x nproc threads. There is still exactly one rs3-js-{i} worker per available_parallelism (JsSettings::default, JsExecutor::start). The substance (V8 runs only on the pool and patch_key only awaits the executor) is verified in truth SC4."
    accepted_by: "gsd-verifier (orchestrator-directed re-verification; Phase 5 D-06)"
    accepted_at: "2026-10-21T11:54:15Z"
  - must_have: "tokio 1.54.0 with features rt and sync is the only new direct dependency and resolves to the tokio already in Cargo.lock; compared with commit 9209f76 the only lockfile change is one added tokio line"
    reason: "Feature set superseded by Phase 5 D-07 (the handler backstop uses tokio::time::timeout; 'time' feature added in commit 3e2a16e) and a dev-only 'macros' feature for tokio::join! concurrency tests. tokio is still the only new direct dependency, still 1.54.0, and `git diff 9209f76 HEAD -- Cargo.lock` is still exactly the one line `+ \"tokio\",` (re-checked at HEAD)."
    accepted_by: "gsd-verifier (orchestrator-directed re-verification; Phase 5 D-07)"
    accepted_at: "2026-10-21T11:54:15Z"
  - must_have: "MUST NOT store or PATCH a self-patching (unboundedly recursive) script in any test, fixture or live smoke in this phase (03-01 JSRT-02, 03-03 HTTP-07 prohibitions)"
    reason: "Scoped to Phase 3 by its own wording. Phase 5 D-11 replaced it once the depth limit existed: self-patching and cyclic scripts are now tested deliberately on the depth-limited path (src/http/mod.rs depth_limit_self_patch_is_422, depth_limit_cycle_is_422; scripts/ci-smoke.sh PATCH /key/me -> 422 js_depth_limit). For the Phase 3 window the operator accepted the verifier's verdict (03-UAT.md test 2, pass)."
    accepted_by: "gsd-verifier (orchestrator-directed re-verification; Phase 5 D-11)"
    accepted_at: "2026-10-21T11:54:15Z"
re_verification:
  previous_status: human_needed
  previous_score: 22/22
  gaps_closed: []
  gaps_remaining: []
  regressions: []
  reason: "Previous report went stale (covered files changed by Phases 5 and 6). The three human items it raised are resolved in 03-UAT.md: test 1 pass, test 2 pass, test 3 skipped as a deferred follow-up (docs readability, not a blocker)."
---

# Phase 3: JavaScript Runtime Port Verification Report

**Phase Goal:** As a user storing scripts in rs3, I want to PATCH a key and have its JavaScript run on current deno_core with the five store ops, so that the JS-executes-against-the-store behaviour works again and no script or op error can abort the server.
**Verified:** 2026-10-21T11:54:15Z
**Status:** passed
**Re-verification:** Yes. The previous report (2026-10-20, human_needed, 22/22) went stale after Phases 5 and 6 changed the covered files. This run judges the final milestone tree (HEAD a4a8069).
**Mode:** mvp (`user-story.validate` returned true)

## Evidence basis

- **Shared gate run (not re-run here, per orchestrator):** on HEAD a4a8069 (source identical to the final tree) `bash scripts/ci-guards.sh` passed guards a to h, and `check-workflow.py`, `test-ci-guards.sh` (99 cases, 0 missed), `cargo fmt --check` and `cargo clippy --all-targets --locked -- -D warnings` all passed. `cargo test --locked` gave 216 passed, 0 failed, 0 ignored. The one ignored doctest is the one deno_core's `extension!` macro generates. `cargo build --release --locked` and `bash scripts/ci-smoke.sh` both succeeded. The log is `target/tmp/d17-a4a8069.log`. Remote CI run 39527188207 passed on ubuntu-26.04 x86_64.
- **This verifier's own read-only checks:**
  - Source reads of every covered Rust file, the samples and docs/js-ops.md.
  - Grep checks for every plan-named test, every key-link pattern, layering, unwrap/expect outside tests, legacy syntax, `#[ignore]` and debt markers.
  - `cargo tree --offline` for the feature edges and the v8 count.
  - `git diff 9209f76 HEAD -- Cargo.lock`.
  - `verify.artifacts` for all three plans.
- **Operator UAT (03-UAT.md):**
  - Test 1 (the test-tier prohibitions, re-run by the operator) passed.
  - Test 2 (no self-patching script in the Phase 3 window) passed.
  - Test 3 (docs readability) was skipped as a deferred follow-up the user will review later. It is not a blocker and is not raised again here.

## User Flow Coverage

| # | Step of the user story | Expected | Evidence in the final tree | Status |
|---|------------------------|----------|----------------------------|--------|
| 1 | User stores a script with `POST /insert` | The key holds the source | `test_insert_is_ok` (exact echo bytes). `txt_payload_*` tests insert the four `requests/*.txt` payloads unchanged | ✓ |
| 2 | User PATCHes the key | 200 with the JSON string of `String(completion value)` | `patch_key` (src/http/mod.rs:46-55): `store.get` (404 before queueing), then `state.js.execute(..).await`. `test_patch_runs_script_200` gives `"120"`. ci-smoke on the release binary also gives `PATCH /key/m -> 200 "120"` | ✓ |
| 3 | Script runs on current deno_core | deno_core 0.412.0 from crates.io, `#[op2]` and `extension!` | Cargo.toml `deno_core = "0.412.0"`. Single `v8 v150.4.0` reached only through deno_v8 and deno_core. `deno_core::extension!(rs3_ext, ops = [5 ops], ..)` | ✓ |
| 4 | Script uses the five store ops against the same store | list, get, insert, delete and patch reach the injected store | `ExecCtx.store` is the `state.store.clone()` passed by `patch_key`. Fixture tests fac `"120"`, sum `"10"`, bin tree JSON, store echo then `GET /key/res` `"1111"`. TEST-06 byte equality holds | ✓ |
| 5 | Outcome: no script or op error can abort the server | Errors give 422 or are caught, and the server keeps serving | op_panic disabled by middleware. Throw, syntax and uncaught op errors give 422 `js_error` (tests). `test_patch_op_panic_is_422_and_server_keeps_serving` then answers `GET /` 200 and another PATCH 200. Panicking jobs are isolated by catch_unwind. Clippy denies unwrap/expect. ci-smoke shows 422 `js_error` and the server alive | ✓ |

## Goal Achievement

### Observable Truths

The table lists the roadmap success criteria (the contract) first, then the plan truths grouped by concern, then the superseded plan items carried by overrides.

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| SC1 | PATCH runs the stored script on released deno_core and returns 200 with the stringified completion value. fac, sum and bin run as fixtures. Each of the seven routes has a success test asserting status and body bytes | ✓ VERIFIED | `patch_key` (src/http/mod.rs:46-55). `fixture_fac_is_120`, `fixture_sum_is_10`, `fixture_bin_is_tree_json` and `fixture_store_writes_res` use `include_str!("../../requests/*.js")`. Route tests, each asserting status and exact bytes: `test_index_body_is_welcome`, `test_ip_body_is_ip`, `test_insert_is_ok`, `test_get_is_ok`, `test_delete_is_ok`, `list_golden_sorted` (status checked in `list_body`) and `test_patch_runs_script_200`. The suite passed (216/0) |
| SC2 | Scripts call `Deno.core.ops.op_rs3_list/get/insert/delete/patch` (`#[op2]`, `extension!`) against the same injected store, and PATCH of `op_rs3_list()` equals `GET /list` | ✓ VERIFIED | src/js/ops.rs:194-358 has five `#[op2]` fns, and `op_rs3_patch` is `#[op2(reentrant)]`. `extension!(rs3_ext, options = { ctx: ExecCtx }, state = put(ctx))` at :416-431. `op_rs3_list` = `legacy::format_list(&ctx.store.list())`. `test_patch_body_is_listing` and `test_patch_listing_matches_list_with_escapes_and_order` check byte equality including `"` and `\`, B<a<b order, and that the listing contains the script key |
| SC3 | A throw or syntax error gives 422 JSON with the JS message. An op on a missing key or with invalid input raises a catchable exception. The server keeps serving. No unwrap/expect on request data | ✓ VERIFIED | `From<ExecError> for ApiError` maps Script to `JsError`, which is 422 `js_error`. Tests: `test_patch_throw_is_422_js_error` (exact body), the syntax, non-Error-throw and uncaught-op-error 422 tests, `test_patch_op_panic_is_422_and_server_keeps_serving`, `test_patch_script_error_is_repeatable`, `get_missing_key_throws_catchable_error` and `insert_malformed_throws_type_error_and_writes_nothing`. Cargo.toml `unwrap_used`/`expect_used = "deny"` with clippy `-D warnings` green, and ci-guards (c) enforces it. The verifier grep found every non-test `.unwrap(`/`.expect(` hit inside `#[cfg(test)]` items |
| SC4 | JS runs on dedicated JS threads, not actix workers. The V8 platform is initialised once in main before other threads spawn. The full suite passes under default parallel `cargo test` | ✓ VERIFIED | src/main.rs runs `Settings::load` (no threads), then `rs3::js::init_platform()`, then `JsExecutor::start`, then `AppState::new(settings, js)`, then `HttpServer::new`. src/http imports no deno_core, and `patch_key` only awaits the executor future. `job_runs_on_a_named_js_thread` asserts the `rs3-js-` prefix. `cargo test --locked` passed 216/0 in the default parallel run, and CI is green on x86_64. ci-guards (d) pins `init_platform(None)` and the protected platform in the release build |
| SC5 | Every JS artefact uses `Deno.core.ops.op_rs3_*`, with no `Deno.core.ops();` or `opSync` left, and the old-to-new mapping (including "missing key throws") is documented | ✓ VERIFIED | All 4 `.js`, 4 `.txt` and 2 `.http` files and src/example.js use `op_rs3_*` (read in full). ci-guards (f) found no `opSync`, `Deno.core.ops()` or unprefixed `rs3_*` in requests/ or src/. The docs/js-ops.md mapping table has a bootstrap row and one row per op, and documents `key not found: <key>` |
| 6 | Nested `op_rs3_patch`: reentrant nested JsRuntime on the same thread, its own top-level scope, the same store, nested errors catchable with one `Uncaught ` stripped, a missing key throws | ✓ VERIFIED | ops.rs:285-358 runs `run_script(ctx.nested(&key), ..)` and does `strip_prefix("Uncaught ")`. Tests: `nested_patch_returns_inner_result`, `nested_patch_has_own_top_level_scope`, `nested_throw_is_catchable_without_uncaught_prefix`, `nested_patch_of_missing_key_throws` and `exec_ctx_nested_increments_depth_and_shares_store`. `nested(&key)` now also extends the key chain (Phase 5 D-02). Depth + 1 and the same store are unchanged |
| 7 | `op_panic` is disabled by extension middleware: catchable `op is disabled`; uncaught it gives 422 `Uncaught Error: op is disabled`; the process keeps running | ✓ VERIFIED | `middleware = \|op\| match op.name { "op_panic" => op.disable(), .. }` (ops.rs:423-427). Tests: `op_panic_is_disabled_and_catchable`, `uncaught_op_panic_is_a_script_error` and the HTTP test from SC3 |
| 8 | Op semantics. list uses the shared codec: sorted, `[]` when empty, extra arguments ignored. get/delete on a missing key throw `Error` `key not found: <k>`. insert validates the whole body, throws TypeError `malformed JSON:` or `invalid input:`, and writes nothing on error. String coercion. Idempotency | ✓ VERIFIED | `to_js` is an exhaustive match. `StrArg` coerces primitives and rejects objects and symbols. All plan-named op tests exist: `list_matches_shared_codec_sorted`, `list_on_empty_store_is_brackets`, `get_coerces_its_argument`, the `insert_*` tests, `delete_returns_old_value_then_throws` and others. The suite is green |
| 9 | `run_script` uses only `JsRuntime::try_new`, reads the result with `deno_core::scope!` in the runtime's own context, and stringifies per D-10 | ✓ VERIFIED | runtime.rs:79-108. There is no `JsRuntime::new(`, `Context::new` or `HandleScope::new` in src/. `stringify` handles symbols and catches exceptions thrown during ToString. Tests: `completion_values_are_stringified`, `symbol_completion_values_match_js_string`, `exception_while_stringifying_is_a_script_error` and the HTTP test `test_patch_stringifies_completion_values` |
| 10 | Script errors carry `JsError.exception_message`, non-ASCII text survives, and the stack goes only to stderr, escaped | ✓ VERIFIED | `ExecError::Script { message: e.exception_message }`, with `log_error` using Debug-escaped name and text. Tests: `thrown_errors_become_script_errors`, `error_messages_keep_non_ascii` and `test_patch_error_message_keeps_non_ascii` |
| 11 | JsExecutor: a bounded pool of `rs3-js-{i}` threads with an explicit stack size; a tokio current_thread runtime per worker with runtimes created inside it; a fresh runtime per job; a full queue makes callers wait; zero settings are clamped; JsSettings is built in code | ✓ VERIFIED | executor.rs:131-196 and `worker_loop` (`rt.enter()` before the job). `JsSettings::default()` gives available_parallelism workers, ×16 queue, 16 MiB stack. Phase 5 added a 2 MiB floor. main uses `JsSettings::default()` and reads nothing from the environment. Tests: `job_runs_on_a_named_js_thread`, `every_job_gets_a_fresh_runtime`, `full_queue_makes_callers_wait`, `zero_settings_are_clamped` and `default_settings_follow_d06` (all present, suite green) |
| 12 | Panic isolation and a closed pool: a panicking job gives `ExecError::Internal` and workers survive; a closed executor gives `Unavailable` | ✓ VERIFIED | `catch_unwind(AssertUnwindSafe(run))` at executor.rs:361. Tests `worker_survives_panicking_jobs` and `closed_executor_is_unavailable`. No `panic = "abort"` profile |
| 13 | Per-job store isolation on the shared executor, also in parallel | ✓ VERIFIED | `stores_on_one_executor_are_isolated` (8 scoped threads). Every HTTP test uses its own `test_state()` store on `test_executor()` |
| 14 | Dependencies: the lockfile differs from 9209f76 by one line, the unprotected platform is on dev edges only, and there is a single v8 | ✓ VERIFIED | `git diff 9209f76 HEAD -- Cargo.lock` shows only `+ "tokio",`. `cargo tree -e features,normal -i deno_core` finds `unsafe_use_unprotected_platform` 0 times, and `-e features` finds it once (the dev edge). `cargo tree -i v8` shows only v8 150.4.0, via deno_v8 and deno_core. ci-guards (d) and (e) enforce this in CI. The tokio feature-set detail is superseded (override 3) |
| 15 | The samples are migrated and keep their behaviour. The `.txt` payloads are the same scripts. The REST-client payloads are `op_rs3_list().toString();`. example.js lists and prints | ✓ VERIFIED | Fixture tests run on the `.js` files. `txt_payload_fac_is_120`, `txt_payload_sum_is_10`, `txt_payload_bin_is_tree_json` and `txt_payload_store_writes_res` (added in Phase 5) automate the former live `.txt` check. `fixtures_share_one_store_in_any_order` checks there is no top-level collision. example.js calls `Deno.core.ops.op_rs3_list()` and `Deno.core.print` |
| 16 | docs/js-ops.md: mapping table (old call, new call, argument, returns, throws when), missing-key and insert error rules, no NotFound class, PATCH results, nested errors and host faults, runtime model, argument coercion and encoding, sandbox, re-insert after upgrade | ✓ VERIFIED | Read in full on the final tree. Every section is present. The exact strings the tests assert are in the doc: `op is disabled`, `key not found: nope`, `{"error":"js_error","message":"Uncaught Error: boom"}`, `internal error` and `TypeError: Deno.core.ops is not a function`. The limits detail is superseded (override 1) |
| 17 | AppState has `pub js: JsExecutor`. `test_state()` uses `js::test_executor()`. main exits 1 with `error: cannot start JS runtime:` when the pool fails to start. main.rs is at most 60 lines | ✓ VERIFIED | src/state.rs and src/main.rs (46 lines). `eprintln!("error: cannot start JS runtime: {e}"); return ExitCode::FAILURE` |
| 18 | ApiError: `JsError(String)` gives js_error/422 and `Internal` gives 500 with a fixed body. `From<ExecError>` logs host faults only. NotImplemented and the 501 test are removed. The matches are exhaustive with no wildcard | ✓ VERIFIED | src/http/error.rs. Phase 5 added `JsDepthLimit`/`JsTimeout`, and the matches are still exhaustive with no `_ =>`. No `NotImplemented`/`501` in src/. Tests `test_internal_error_response_is_500_json` and `test_patch_executor_down_is_500_internal` (store unchanged) |
| 19 | Host faults (a closed executor, or a runtime creation failure at any depth, even when caught) answer 500. Ops refuse after a fault, and the fault is logged once | ✓ VERIFIED | `JobState.host_fault` and the `live_ctx` refusal. Tests `caught_nested_runtime_failure_is_still_internal_and_refuses_later_writes`, `every_op_throws_internal_error_once_the_job_faulted` and the others named in the plan (all present) |
| 20 | Script errors are repeatable and do not poison the app. The parked PATCH tests run again. There is no `#[ignore]` in src/. GET, PATCH and DELETE on one key path reach their own handlers. An empty script is a 200 | ✓ VERIFIED | `test_patch_script_error_is_repeatable`, `test_patch_is_ok`, `test_patch_body_is_listing` and `test_patch_get_delete_on_one_key_hit_their_own_handlers`. The `s5` empty script gives `"undefined"`. `grep '#[ignore' src` finds 0, and ci-guards (b) enforces it |
| 21 | D-17 housekeeping: the parse_insert_body problem-order doc is corrected and pinned, the truncated-input tests are renamed, and an HTTP test gives 422 for valid deep nesting | ✓ VERIFIED | src/store/legacy.rs:29-31. `parse_unrepresentable_value_beats_earlier_non_string`, `parse_rejects_truncated_deep_nesting` and `insert_valid_deep_nesting_is_422_json` |
| 22 | Phase gate (TOOL-03) and static gates: no suppression, http→js→store direction, deno_core only in src/js, no panicking constructor, no legacy syntax | ✓ VERIFIED | Shared gate run: fmt, clippy `-D warnings`, `test --locked` 216/0 and `build --release --locked` all green, and ci-guards a to h passed. The verifier grep found no deno_core/deno_error outside src/js and no actix_web or `crate::(http\|config\|state)` in src/js. The `env -i`, `--test-threads=1` and five-runs variants were verified at the Phase 3 commit (previous report) and were not re-run here, per the shared-evidence instruction |
| O1 | (03-02) docs disclose "until Phase 5" unbounded nesting (235/778 levels, about 800 MB, 2.7 s) and a non-terminating script holding a worker | PASSED (override) | Superseded by Phase 5 D-12. The docs now state the depth limit of 4, the 5000 ms timeout, and the heap-exhaustion limit (v2) |
| O2 | (03-03) the live binary has exactly nproc threads matching `rs3-js-*` | PASSED (override) | Superseded by Phase 5 D-06 (watchdog threads `rs3-js-{i}-wd`). There is still one worker per available CPU |
| O3 | (03-01) tokio `rt, sync` is the only new direct dependency, and the lockfile delta is one line | PASSED (override) | Phase 5 D-07 added `time` (backstop) and a dev-only `macros`. The lockfile delta is still exactly one line |
| O4 | (03-01 JSRT-02 / 03-03 HTTP-07 prohibition) no self-patching script in tests or smokes in this phase | PASSED (override) | Scoped to Phase 3, then replaced by Phase 5 D-11 (depth-limited self-patch and cycle tests). 03-UAT test 2 passed for the Phase 3 window |

**Score:** 26/26 truths verified (22 VERIFIED, 4 PASSED by override as superseded; 0 present but behavior-unverified)

Behavior-dependent truths (11, 12, 13, 19, 20) each name a test that exercises the transition or invariant: queue wait, panic survival, store isolation, fault refusal and repeatability. Every such test exists in src/ and passed in the shared 216/0 run, so none stays PRESENT_BEHAVIOR_UNVERIFIED.

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `src/js/ops.rs` | ExecCtx, five `#[op2]` ops, `to_js`, `rs3_ext` with op_panic middleware | ✓ VERIFIED | contains `op2(reentrant)`. Used by runtime.rs and executor.rs |
| `src/js/runtime.rs` | `run_script` on a fresh `JsRuntime::try_new` | ✓ VERIFIED | Used by executor.rs and `op_rs3_patch` |
| `src/js/executor.rs` | bounded pool, `catch_unwind`, `execute`, `closed()` | ✓ VERIFIED | Used by main.rs, state.rs and http/mod.rs |
| `src/js/mod.rs` | `ExecError`, `JsSettings`, `init_platform`, cfg(test) helpers | ✓ VERIFIED | Used by error.rs and main.rs |
| `Cargo.toml` | direct tokio; dev-only `unsafe_use_unprotected_platform` | ✓ VERIFIED | Feature on the `[dev-dependencies]` deno_core only |
| `docs/js-ops.md` | JSRT-06 mapping and rules | ✓ VERIFIED | contains `Deno.core.ops.op_rs3_patch(` |
| `requests/{fac,sum,bin,store}.js`, `src/example.js` | migrated samples | ✓ VERIFIED | Pulled in by `include_str!` in 4 fixture tests |
| `src/http/mod.rs` | `patch_key` and the PATCH tests | ✓ VERIFIED | contains `state.js.execute(` |
| `src/http/error.rs` | `JsError`, `From<ExecError>`, `Internal` | ✓ VERIFIED | contains `js_error` |
| `src/state.rs` | `pub js: JsExecutor` | ✓ VERIFIED | |
| `src/main.rs` | init, then start, then AppState, then HttpServer | ✓ VERIFIED | contains `JsExecutor::start` |
| `src/store/legacy.rs`, `src/http/kv.rs` | IN-04 and IN-05 | ✓ VERIFIED | |

`verify.artifacts`: 03-01 5/5, 03-02 6/6 and 03-03 6/6, all with exit 0.

### Key Link Verification

`verify.key-links` cannot parse `from:` fields that name a symbol after the path. It verified 2 of 13 links automatically, and the verifier checked every declared pattern by hand.

| From | To | Via | Status | Details |
|------|----|-----|--------|---------|
| runtime.rs `run_script` | ops.rs `rs3_ext` | `rs3_ext::init(` | WIRED | 1 match |
| ops.rs `op_rs3_patch` | runtime.rs `run_script` | `run_script(ctx.nested(&key), ..)` | WIRED | Match found, and the nested tests pass |
| ops.rs `op_rs3_list` | legacy.rs `format_list` | `legacy::format_list(` | WIRED | TEST-06 byte equality holds |
| ops.rs `op_rs3_insert` | legacy.rs `parse_insert_body` | `parse_insert_body(` before `insert_all` | WIRED | |
| executor.rs `execute` | runtime.rs `run_script` | job closure under catch_unwind | WIRED | |
| http/mod.rs `patch_key` | executor.rs `execute` | `state.js.execute(` after `store.get` | WIRED | 404 is answered before queueing |
| http/error.rs | js/mod.rs `ExecError` | `impl From<ExecError> for ApiError` | WIRED | |
| main.rs | state.rs `AppState::new` | `AppState::new(settings, js)` | WIRED | |
| state.rs `test_state` | js/mod.rs `test_executor` | `crate::js::test_executor()` | WIRED | |
| http/mod.rs fixture tests | requests/*.js | `include_str!("../../requests/..")` | WIRED | 8 matches (4 `.js`, 4 `.txt`) |
| requests/*.txt | requests/*.js | same op calls and results | WIRED | Each of the 4 payloads calls `op_rs3_get`/`op_rs3_insert`. `txt_payload_*` tests assert the same results |
| docs/js-ops.md | src/js/ops.rs | documented names and messages | WIRED | 30 op-name matches. The asserted messages are present verbatim |

### Data-Flow Trace (Level 4)

| Artifact | Data Variable | Source | Produces Real Data | Status |
|----------|---------------|--------|--------------------|--------|
| `patch_key` response | `out` | `state.store.get(&key)`, then `JsExecutor::execute`, then the V8 completion value | Yes (fixture results, ci-smoke `"120"`) | ✓ FLOWING |
| `op_rs3_*` results | store values | `OpState`, then `ExecCtx.store` (the same `Store` that `patch_key` passes in) | Yes (`fixture_store_writes_res`: a JS insert is visible to `GET /key/res`) | ✓ FLOWING |

### Behavioral Spot-Checks

The orchestrator supplied the evidence. The verifier did not run cargo build or test, the smoke, or the binary.

| Behavior | Command | Result | Status |
|----------|---------|--------|--------|
| Full gate | ci-guards, check-workflow, test-ci-guards, fmt, clippy, `cargo test --locked`, release build | All OK. 216 passed, 0 failed, 0 ignored | ✓ PASS |
| Release binary PATCH | `scripts/ci-smoke.sh` | `200 "120"`, 422 js_error, 422 js_depth_limit, 504 js_timeout, then `200 "120"` with the server alive | ✓ PASS |
| Feature edges | `cargo tree --offline -e features[,normal] -i deno_core` | normal 0, all 1 | ✓ PASS |
| Single v8 | `cargo tree --offline -i v8` | v8 150.4.0 only | ✓ PASS |
| Lockfile delta | `git diff 9209f76 HEAD -- Cargo.lock` | one line `+ "tokio",` | ✓ PASS |
| Plan-named tests exist | grep for each `fn <name>` across all three plans | 71 of 71 present (the remaining 4 hits are prose words, the intentionally removed `test_patch_existing_is_501`, and the renamed `parse_rejects_deep_nesting`) | ✓ PASS |

### Probe Execution

Step 7c does not apply. There are no `scripts/*/tests/probe-*.sh` files, and no plan declares one.

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|-------------|-------------|--------|----------|
| HTTP-07 | 03-03 | PATCH executes the stored value as JS and returns the stringified completion value | ✓ SATISFIED | SC1, truths 9 and 20 |
| JSRT-01 | 03-01 | current released deno_core, `#[op2]` and `extension!` | ✓ SATISFIED | SC2, truth 14 |
| JSRT-02 | 03-01 | five ops with legacy semantics | ✓ SATISFIED | SC2, truths 6 and 8 |
| JSRT-03 | 03-01 | a missing key or invalid input gives a catchable JS exception | ✓ SATISFIED | SC3, truth 8 |
| JSRT-04 | 03-01, 03-03 | dedicated JS threads, platform initialised once in main first | ✓ SATISFIED | SC4, truths 11 and 17 |
| JSRT-05 | 03-02, 03-03 | every JS artefact uses `Deno.core.ops.op_rs3_*` | ✓ SATISFIED | SC5, truth 15, ci-guards (f) |
| JSRT-06 | 03-02 | the old-to-new mapping is documented | ✓ SATISFIED | SC5, truth 16 |
| ROBU-03 | 03-03 | a throw or syntax error gives 422 JSON with the JS message | ✓ SATISFIED | SC3, truth 10 |
| ROBU-04 | 03-01, 03-03 | no unwrap/expect on request data; ops never panic | ✓ SATISFIED | SC3, truths 7, 12 and 19; clippy deny; ci-guards (c) |
| TEST-03 | 03-03 | every route has a success test with status and bytes | ✓ SATISFIED | SC1 (seven named tests) |
| TEST-05 | 03-03 | fac, sum and bin run as fixtures with the expected results | ✓ SATISFIED | SC1, truth 15 |
| TEST-06 | 03-03 | PATCH running `op_rs3_list` equals `GET /list` | ✓ SATISFIED | SC2 |

Each of the 12 phase IDs is claimed by at least one plan. REQUIREMENTS.md maps exactly these 12 to Phase 3, so none is orphaned.

### Prohibitions

| Plan / Req | Statement (short) | Tier | Disposition |
|------------|-------------------|------|-------------|
| 03-01 ROBU-04 | no script-reachable abort path | test | ✓ Enforced by `op_panic_is_disabled_and_catchable`, `uncaught_op_panic_is_a_script_error`, `worker_survives_panicking_jobs`, clippy deny and ci-guards (c). There is no `JsRuntime::new(` in src/ |
| 03-03 ROBU-04 | no request takes the server down | test | ✓ Enforced by `test_patch_op_panic_is_422_and_server_keeps_serving`, `test_patch_executor_down_is_500_internal`, `test_patch_script_error_is_repeatable` and ci-smoke (server alive after 422 and 504) |
| 03-01 JSRT-04 | unprotected platform on dev edges only | test | ✓ Now enforced by a wired check: ci-guards (d) runs in the CI "Repo guards" step. It was flagged in the previous report because this enforcement did not exist yet. The operator also re-ran it (03-UAT test 1, pass) |
| 03-01 ROBU-04 / 03-03 TEST-03 | no gate made green by suppression | test | ✓ Now enforced by ci-guards (a) (no allow/expect attribute, including cfg_attr), (b) (no `#[ignore]` or disabled targets) and (c) (lints at deny), all wired in CI |
| 03-02 JSRT-05 | no legacy-API shim | test | ✓ Enforced by ci-guards (f) (no `opSync`, `Deno.core.ops()` or unprefixed `rs3_*` in requests/ or src/). The docs state there is no shim |
| 03-02 JSRT-06 | limits are not omitted or softened in the docs | test | ✓ Human-resolved: no automated guard exists, and operator UAT 03-UAT test 1 passed. The verifier re-read the current docs: op_panic disabled, unawaited Promises, print to stdout, the depth limit, the timeout and heap exhaustion are all disclosed. The specific Phase 3 numbers were superseded by Phase 5 D-12 (override 1) |
| 03-01 JSRT-02 / 03-03 HTTP-07 | no self-patching script in tests or smokes | judgment | PASSED (override 4): superseded by Phase 5 D-11. 03-UAT test 2 passed for the Phase 3 window |

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| src/store/legacy.rs | 18, 39, 71 | `XXX` match | ℹ️ Info | False positive: the `\uXXXX` escape notation in doc comments, not a debt marker |
| src/js/runtime.rs, src/js/executor.rs | 71, 760 | `thread::sleep` | ℹ️ Info | The 03-01 Task 2 verify grep (`! grep thread::sleep src/`) no longer holds. Both hits are test-only, bounded and added in Phase 5: the `cfg(test)` fault-after-timeout hook and the backstop test. This is a plan-level acceptance check, not a must-have, and production code has no sleep |
| docs/js-ops.md | Heap exhaustion bullet | known limit | ℹ️ Info | A script that allocates without bound can exhaust V8's heap and abort the process. This is resource exhaustion, not a script or op error. It is disclosed in the docs and scoped to v2 JSHD-01 (REQUIREMENTS.md:93; ROADMAP Phase 5 notes). It is not a Phase 3 success criterion |

There are no TODO/FIXME/HACK/placeholder markers, no stub returns, and no unwrap/expect outside `#[cfg(test)]`.

### Human Verification Required

None pending. The items from the previous report are resolved in 03-UAT.md:
- Test 1 passed: the operator re-ran the test-tier prohibition checks.
- Test 2 passed: there is no self-patching script in the Phase 3 window.
- Test 3, docs/js-ops.md readability, was skipped by the operator as a **deferred follow-up** that the user will review later. It is not a blocker. It is recorded under "Deferred Follow-Ups" in 03-UAT.md and is not raised again here.

### Gaps Summary

There are no gaps. All 5 roadmap success criteria hold on the final milestone tree, and so do the 17 grouped plan truths. The verifier read the code for each one, every plan-named test exists, and the shared gate run passed: 216 tests passed with 0 failed or ignored, ci-guards a to h passed, and the release ci-smoke on the protected V8 platform passed. All 12 requirement IDs are satisfied, and none is orphaned.

Four plan-level details were changed on purpose by Phase 5 decisions. Each is recorded as an override with its citation, not as a gap:
- the docs limits wording (D-12)
- the watchdog threads that share the `rs3-js-` name prefix (D-06)
- the tokio `time` feature for the backstop (D-07)
- the Phase-3-only ban on self-patching scripts, now tested on the depth-limited path (D-11)

Compared with the previous report, the five test-tier prohibitions that were flagged for lack of enforcement now have repo-resident enforcement: ci-guards (a) to (f), run in CI. The one remaining unautomated item, JSRT-06 docs limits, was resolved by the operator in 03-UAT.

---

_Verified: 2026-10-21T11:54:15Z_
_Verifier: Claude (gsd-verifier)_
