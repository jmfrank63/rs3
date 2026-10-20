---
phase: 03-javascript-runtime-port
verified: 2026-10-20T21:08:05Z
status: human_needed
score: 22/22 must-haves verified
covered_files:
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
covered_digest: "v3:sha256:f6c0e3e0444ee19d504091db60dc42f94184d275a526bdf764cbc6a8c3577871"
behavior_unverified: 0
overrides_applied: 0
human_verification:
  - test: "unverified-prohibition — human review recommended. Test-tier prohibitions with no repo-resident enforcement: (a) 03-01 ROBU-04 and 03-03 TEST-03 'MUST NOT make a gate green by suppression' (no allow/expect/cfg_attr lint attribute in src/, no lowered lint, no #[ignore], no weakened golden, no production code under cfg(test)); (b) 03-01 JSRT-04 'unsafe_use_unprotected_platform only on the [dev-dependencies] deno_core entry'; (c) 03-02 JSRT-05 'no compatibility shim, alias or wrapper for the legacy op API'; (d) 03-02 JSRT-06 'docs/js-ops.md must not omit or soften the known limits'. Review `git diff 9209f76..HEAD -- src Cargo.toml clippy.toml docs requests` and confirm."
    expected: "Zero suppression attributes and zero #[ignore] in src/; unwrap_used/expect_used stay at deny; `cargo tree -e features,normal -i deno_core` shows no unsafe_use_unprotected_platform while `cargo tree -e features -i deno_core` shows it (dev edge only); no opSync/Deno.core.ops()/rs3_* alias anywhere outside the migration table in docs/js-ops.md; docs/js-ops.md lists op_panic disabled, unbounded nesting (235/778 levels, ~800 MB, 2.7 s), non-terminating scripts, unawaited Promises and Deno.core.print output."
    why_human: "Fail-closed rule: no wired, repo-resident test or CI check enforces these yet (the Phase 4 CI grep does not exist), so they cannot be marked green automatically. The verifier's own checks at this commit are all clean: grep found no suppression or #[ignore], the normal-edge feature count is 0 and the all-edge count is 1, the legacy-syntax grep hits only docs/js-ops.md migration rows, and every limit is stated in docs/js-ops.md 'Sandbox and known limits'."
  - test: "unverified-prohibition — human review recommended. Judgment-tier prohibition (03-01 JSRT-02, 03-03 HTTP-07): 'MUST NOT store or PATCH a self-patching (unboundedly recursive) script in any test, fixture or live smoke in this phase.' Review every op_rs3_patch use in src/ (`grep -n op_rs3_patch src/js/*.rs src/http/*.rs`)."
    expected: "Every nested chain ends at a key that does not patch (inner, mid->inner, a->b->c with c='1', throws, bad/good, ghost); no key patches itself directly or through a cycle."
    why_human: "Judgment-tier prohibitions get only a non-authoritative LLM verdict. Verifier verdict: satisfied. The verifier inspected all 20 op_rs3_patch call sites and its own live smokes used only the non-recursive outer->inner chain."
  - test: "Read docs/js-ops.md as a script author migrating a legacy rs3 script."
    expected: "The mapping table, error rules, PATCH results and limits are clear enough to migrate fac/sum/bin/store-style scripts without reading the Rust source."
    why_human: "03-02-SUMMARY coverage D4 flags this as human_judgment: the docs gate proves the required statements exist and the verifier confirmed every documented message against the code and live binary, but readability for a user is a human call."
---

# Phase 3: JavaScript Runtime Port Verification Report

**Phase Goal:** As a user storing scripts in rs3, I want to PATCH a key and have its JavaScript run on current deno_core with the five store ops, so that the JS-executes-against-the-store behaviour works again and no script or op error can abort the server.
**Verified:** 2026-10-20T21:08:05Z
**Status:** human_needed
**Re-verification:** No — initial verification
**Mode:** mvp (user-story format validated: `user-story.validate` → true)

## User Flow Coverage

| # | Step of the user story | Expected | Evidence in codebase / live binary | Status |
|---|------------------------|----------|------------------------------------|--------|
| 1 | User stores a script with `POST /insert` | key holds the source | live 18520: `{"m":"6*20"}` echoed; the four `requests/*.txt` payloads inserted unchanged | ✓ |
| 2 | User PATCHes the key | 200, JSON string of `String(completion value)` | `src/http/mod.rs` `patch_key` → `state.js.execute(..)`; live: `PATCH /key/m` → `200 "120"` | ✓ |
| 3 | Script runs on current deno_core | deno_core 0.412.0 from crates.io, `#[op2]` + `extension!` | `Cargo.toml` `deno_core = "0.412.0"`; `src/js/ops.rs` `deno_core::extension!(rs3_ext, ops = [...5 ops])`; single v8 150.4.0 through deno_v8 | ✓ |
| 4 | Script uses the five store ops against the same store | list/get/insert/delete/patch reach the injected store | live: fac `"120"`, sum `"10"`, bin tree JSON, store echo then `GET /key/res` → `"1111"`; nested `outer` → `"11"`; JS `op_rs3_delete("num")` → `"5"` then `GET /key/num` → 404; PATCH of `op_rs3_list().toString()` equals `GET /list` byte for byte | ✓ |
| 5 | Outcome: "no script or op error can abort the server" | script/op errors → 422 or caught; server keeps serving | live: op_panic, throw, syntax error, uncaught op error → 422 js_error; caught op error → 200; `GET /` still 200 after; 32 parallel PATCHes all 200; process alive. Verifier probe: all 98 other exposed built-in ops (every op in `Deno.core.ops` except op_panic and the five rs3 ops) called with 12 argument patterns each, plus callback-setting probes (format-exception, promise hooks, rejection handler, wasm streaming) → no abort, no panic | ✓ |

## Goal Achievement

### Observable Truths

Roadmap success criteria (the contract) first, then plan-specific truths grouped by concern.

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| SC1 | PATCH runs the stored script on released deno_core and returns 200 with the stringified completion value; fac/sum/bin run as fixtures; every one of the seven routes has a success test asserting status and body bytes | ✓ VERIFIED | `patch_key` (src/http/mod.rs:46-55); tests `fixture_fac_is_120`, `fixture_sum_is_10`, `fixture_bin_is_tree_json`, `fixture_store_writes_res` use `include_str!("../../requests/*.js")`; route tests `test_index_body_is_welcome`, `test_ip_body_is_ip`, `test_insert_is_ok`, `test_get_is_ok`, `test_delete_is_ok`, `list_golden_sorted`, `test_patch_runs_script_200` all assert status plus exact bytes; live smoke matches |
| SC2 | Scripts call `Deno.core.ops.op_rs3_list/get/insert/delete/patch` (`#[op2]`, `extension!`) against the same injected store; PATCH of `op_rs3_list()` yields the same content as `GET /list` | ✓ VERIFIED | src/js/ops.rs: five `#[op2]` fns, `op_rs3_patch` is `#[op2(reentrant)]`; `extension!(rs3_ext, options = { ctx: ExecCtx }, state = put(ctx))`; ExecCtx carries `state.store.clone()` from `patch_key`; `op_rs3_list` = `legacy::format_list(&store.list())`; tests `test_patch_body_is_listing`, `test_patch_listing_matches_list_with_escapes_and_order` (byte equality including `"` and `\`, B<a<b order, script key included); live `LIST EQUAL` |
| SC3 | Throw / syntax error → 422 JSON with the JS message; op on missing key / invalid input raises a catchable exception; server keeps serving; no unwrap/expect on request data | ✓ VERIFIED | `From<ExecError> for ApiError` (Script → JsError 422); tests `test_patch_throw_is_422_js_error`, `..._syntax_error_...`, `..._non_error_throw_...`, `..._uncaught_op_error_...`, `test_patch_op_panic_is_422_and_server_keeps_serving`, `get_missing_key_throws_catchable_error`, `insert_malformed_throws_type_error_and_writes_nothing`; clippy `unwrap_used`/`expect_used = deny` passes with `-D warnings`; verifier scan: every `unwrap()`/`expect(` in src/js, src/http, src/store, src/state.rs is inside `#[cfg(test)]`; built-in op probe found no abort path |
| SC4 | JS on dedicated JS threads, V8 platform initialised once in main before other threads spawn; full suite passes under default parallel `cargo test` | ✓ VERIFIED | src/main.rs: `Settings::load` → `rs3::js::init_platform()` → `JsExecutor::start` → `AppState::new(settings, js)` → `HttpServer::new` (sequential code, nothing spawns before line 16); `#[actix_web::main]` builds a current-thread runtime; src/http has no deno_core import; live: 4 `rs3-js-*` threads, nproc 4; `cargo test`: 171 passed, 0 failed, 0 ignored (parallel); also 171 passed under `env -i ... cargo test -- --test-threads=1` |
| SC5 | Every JS artefact uses `Deno.core.ops.op_rs3_*(...)`, no `Deno.core.ops();` / `opSync`; old→new mapping (incl. "missing key throws") documented | ✓ VERIFIED | grep over requests/ and src/: zero `opSync`, `Deno.core.ops()` or unprefixed `rs3_*` calls; all 4 `.txt` payloads and both `.http` files use `op_rs3_*`; src/example.js uses `op_rs3_list()`; docs/js-ops.md mapping table has one row per op plus the bootstrap row, and states `key not found: <key>` |
| 6 | Nested `op_rs3_patch`: reentrant nested JsRuntime on the same thread, own top-level scope, same store, nested errors catchable with one `Uncaught ` stripped, missing key throws | ✓ VERIFIED | tests `nested_patch_returns_inner_result`, `nested_patch_has_own_top_level_scope` (71), `nested_throw_is_catchable_without_uncaught_prefix` (`true\|RangeError: inner boom`), `nested_patch_of_missing_key_throws`, `exec_ctx_nested_increments_depth_and_shares_store`; live outer→inner `"11"` |
| 7 | `op_panic` disabled by extension middleware: catchable `op is disabled`; uncaught → 422 `Uncaught Error: op is disabled`; process keeps running | ✓ VERIFIED | `middleware = \|op\| match op.name { "op_panic" => op.disable(), ... }`; tests `op_panic_is_disabled_and_catchable`, `uncaught_op_panic_is_a_script_error`, HTTP test above; live 422 then `GET /` 200 |
| 8 | Op semantics: list uses the shared codec (sorted, `[]` empty, extra args ignored); get/delete missing → `Error` `key not found: <k>`; insert validates whole body, TypeError `malformed JSON:` / `invalid input:`, nothing written; string coercion (`undefined`, `5`); idempotency | ✓ VERIFIED | src/js/ops.rs `to_js`, `StrArg`; tests `list_matches_shared_codec_sorted`, `list_on_empty_store_is_brackets`, `list_ignores_an_extra_argument`, `get_coerces_its_argument`, `numeric_key_never_aliases_the_empty_key`, `object_and_symbol_arguments_throw_type_error`, `insert_*`, `delete_returns_old_value_then_throws`, `get_twice_returns_the_same_value`; live caught insert error → `TypeError\|malformed JSON: ...` |
| 9 | `run_script`: `JsRuntime::try_new` only, `deno_core::scope!` in the runtime's own context, stringification per D-10 (incl. Symbol and exceptions during ToString → 422, CR-01 fix) | ✓ VERIFIED | src/js/runtime.rs: no `JsRuntime::new(` in src/; `stringify` under `v8::tc_scope!`; tests `completion_values_are_stringified` (120, undefined, [object Object], [object Promise], null, "", undefined), `symbol_completion_values_match_js_string`, `exception_while_stringifying_is_a_script_error`, HTTP `test_patch_stringifies_completion_values`, `test_patch_stringify_exception_is_422_js_error`, `test_patch_symbol_result_is_200` |
| 10 | Script errors carry `JsError.exception_message`; non-ASCII preserved; stack only on stderr (escaped) | ✓ VERIFIED | `ExecError::Script { message: e.exception_message }`; `log_error` with Debug-escaped name and text; tests `thrown_errors_become_script_errors`, `error_messages_keep_non_ascii`, `test_patch_error_message_keeps_non_ascii`; live stderr shows `js error in "t": "Error: boom\n    at t:1:7"` and the 422 body has no stack |
| 11 | JsExecutor: bounded pool of `rs3-js-{i}` threads, explicit 16 MiB stack, tokio current_thread per worker with runtimes created inside it, fresh runtime per job, full queue makes callers wait, zero settings clamped, JsSettings built in code | ✓ VERIFIED | src/js/executor.rs `start`, `worker_loop` (`rt.enter()` before the job); tests `job_runs_on_a_named_js_thread`, `every_job_gets_a_fresh_runtime`, `full_queue_makes_callers_wait`, `zero_settings_are_clamped`, `default_settings_follow_d06` — all pass in the run above (behavior-dependent invariants exercised by tests) |
| 12 | Panic isolation and closed pool: panicking job → `ExecError::Internal`, workers survive more panics than workers; closed executor → `Unavailable` | ✓ VERIFIED | `catch_unwind(AssertUnwindSafe(env.run))`; tests `worker_survives_panicking_jobs`, `closed_executor_is_unavailable`; no `panic = "abort"` profile in Cargo.toml |
| 13 | Per-job store isolation on the shared executor, also in parallel | ✓ VERIFIED | test `stores_on_one_executor_are_isolated`; every HTTP test uses its own `test_state()` store over `test_executor()` |
| 14 | Dependencies: tokio 1.54.0 (rt, sync) is the only new direct dep; lockfile differs from 9209f76 by one line; unprotected platform only on dev edges; single v8 | ✓ VERIFIED | `git diff 9209f76 -- Cargo.lock` = one `+ "tokio",` line; `cargo tree -e features,normal -i deno_core` count 0, `-e features` count 1; `cargo tree -i v8` → v8 150.4.0 only via deno_v8 ← deno_core |
| 15 | Samples migrated and keep behaviour (fac 120, sum 10, bin tree JSON, store writes res=1111); `.txt` payloads are the same scripts; REST-client payloads `op_rs3_list().toString();`; example.js lists and prints | ✓ VERIFIED | HTTP fixture tests on the `.js` files; live 18520 ran the four unmodified `.txt` payloads through `/insert` + PATCH: `"120"`, `"10"`, tree JSON, echo, `GET /key/res` → `"1111"`; `fixtures_share_one_store_in_any_order` (sum/bin/sum, no top-level collision) |
| 16 | docs/js-ops.md: mapping table (old call/new call/argument/returns/throws), missing-key and insert error rules, no NotFound class, PATCH results (200/422/500/404), nested errors and host faults, runtime model, argument coercion and encoding, sandbox and Phase 5 limits, re-insert after upgrade | ✓ VERIFIED | Read in full; every message (`op is disabled`, `key not found: nope`, `{"error":"js_error","message":"Uncaught Error: boom"}`, `internal error`) matches the code and the live binary; limits section states 235/778 levels, ~800 MB, 2.7 s, `while(true){}`, Phase 5 422/504 |
| 17 | AppState gains `pub js: JsExecutor`; `test_state()` uses `js::test_executor()`; main exits 1 with `error: cannot start JS runtime:` on pool failure; main.rs ≤ 60 lines | ✓ VERIFIED | src/state.rs; src/main.rs (46 lines, `eprintln!("error: cannot start JS runtime: {e}"); return ExitCode::FAILURE`) |
| 18 | ApiError: `JsError(String)` → js_error/422, `Internal` → 500 fixed body, `From<ExecError>` logs host faults only, NotImplemented and the 501 test removed, matches exhaustive without wildcard | ✓ VERIFIED | src/http/error.rs; grep finds no `NotImplemented`/`501` in src/; tests `test_internal_error_response_is_500_json` (Phase 2 IN-01), `test_patch_executor_down_is_500_internal` (store unchanged) |
| 19 | Host faults (closed executor, runtime creation failure at top level or any nested depth, even if caught) answer 500; ops refuse after a nested fault; fault logged once | ✓ VERIFIED | tests `top_level_runtime_failure_is_internal`, `nested_runtime_failure_is_internal`, `deeper_nested_runtime_failure_is_internal`, `caught_nested_runtime_failure_is_still_internal_and_refuses_later_writes`, `every_op_throws_internal_error_once_the_job_faulted`, `patch_after_a_caught_fault_is_refused_without_running`, `one_nested_host_fault_is_logged_once_at_any_depth`, `a_fault_recorded_before_a_run_is_not_blamed_on_it` |
| 20 | Script errors are repeatable and do not poison the app; parked PATCH tests run again; no `#[ignore]` in src/; GET/PATCH/DELETE on one key path hit their own handlers; empty script is a 200 | ✓ VERIFIED | `test_patch_script_error_is_repeatable`, `test_patch_is_ok`, `test_patch_body_is_listing` (not ignored; 0 ignored lib tests), `test_patch_get_delete_on_one_key_hit_their_own_handlers`, `test_patch_stringifies_completion_values` (s5 empty → `"undefined"`) |
| 21 | D-17 housekeeping: parse_insert_body problem-order doc corrected and pinned (`{"a":1,"b":1e400}` → invalid input naming b); truncated-input tests renamed; HTTP 422 for valid deep nesting | ✓ VERIFIED | src/store/legacy.rs:29-31 doc; `parse_unrepresentable_value_beats_earlier_non_string`; `parse_rejects_truncated_deep_nesting`; src/http/kv.rs `insert_valid_deep_nesting_is_422_json` — all pass |
| 22 | Phase gate (TOOL-03): `cargo build --locked`, `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check` exit 0; static gates (no suppression, http→js→store direction, deno_core only in src/js, no panicking constructor, no legacy syntax) | ✓ VERIFIED | Verifier ran all four: build ok, test 171/0/0, clippy clean, fmt exit 0; grep: no `allow(`/`expect(` attributes, no deno_core/deno_error outside src/js, no `JsRuntime::new(`. The "five times in a row" repetition was not re-run (verifier runs the suite at most once per configuration); parallel and env-free serial runs both passed |

**Score:** 22/22 truths verified (0 present, behavior-unverified)

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `src/js/ops.rs` | ExecCtx, five `#[op2]` ops, `to_js`, `rs3_ext` with op_panic middleware | ✓ VERIFIED | 734 lines; contains `op2(reentrant)`; used by runtime.rs |
| `src/js/runtime.rs` | `run_script` on a fresh `JsRuntime::try_new` | ✓ VERIFIED | used by executor.rs and op_rs3_patch |
| `src/js/executor.rs` | bounded pool, `catch_unwind`, `execute`, `closed()` | ✓ VERIFIED | used by main.rs, state.rs, http/mod.rs |
| `src/js/mod.rs` | `ExecError`, `JsSettings`, `init_platform`, cfg(test) helpers | ✓ VERIFIED | used by error.rs, main.rs |
| `Cargo.toml` | direct tokio; dev-only `unsafe_use_unprotected_platform` | ✓ VERIFIED | as stated |
| `docs/js-ops.md` | JSRT-06 mapping and rules | ✓ VERIFIED | contains `Deno.core.ops.op_rs3_patch(` |
| `requests/{fac,sum,bin,store}.js`, `src/example.js` | migrated samples | ✓ VERIFIED | included by HTTP fixture tests |
| `src/http/mod.rs` | `patch_key` + PATCH tests | ✓ VERIFIED | contains `state.js.execute(` |
| `src/http/error.rs` | `JsError`, `From<ExecError>`, `Internal` | ✓ VERIFIED | contains `js_error` |
| `src/state.rs` | `pub js: JsExecutor` | ✓ VERIFIED | |
| `src/main.rs` | init → start → AppState → HttpServer | ✓ VERIFIED | contains `JsExecutor::start` |
| `src/store/legacy.rs`, `src/http/kv.rs` | IN-04/IN-05 | ✓ VERIFIED | contain `parse_rejects_truncated_deep_nesting`, `insert_valid_deep_nesting_is_422_json` |

`verify.artifacts`: 03-01 5/5, 03-02 6/6, 03-03 6/6.

### Key Link Verification

`verify.key-links` could not parse the plans' `from:` fields (they name symbols, e.g. "src/js/ops.rs op_rs3_patch"), so every link was checked by hand with its declared pattern.

| From | To | Via | Status | Details |
|------|----|-----|--------|---------|
| runtime.rs `run_script` | ops.rs `rs3_ext` | `rs3_ext::init(ctx)` | WIRED | 1 match |
| ops.rs `op_rs3_patch` | runtime.rs `run_script` | `run_script(ctx.nested(), ..)` | WIRED | match + nested tests pass |
| ops.rs `op_rs3_list` | legacy.rs `format_list` | `legacy::format_list(` | WIRED | TEST-06 byte equality holds |
| ops.rs `op_rs3_insert` | legacy.rs `parse_insert_body` | `parse_insert_body(` before `insert_all` | WIRED | |
| executor.rs `execute` | runtime.rs `run_script` | job closure under catch_unwind | WIRED | |
| http/mod.rs `patch_key` | executor.rs `execute` | `state.js.execute(` after `store.get` | WIRED | 404 before queueing (live ghost → 404) |
| http/error.rs | js/mod.rs `ExecError` | `impl From<ExecError> for ApiError` | WIRED | |
| main.rs | state.rs `AppState::new` | `AppState::new(settings, js)` | WIRED | |
| state.rs `test_state` | js/mod.rs `test_executor` | `crate::js::test_executor()` | WIRED | |
| http/mod.rs fixture tests | requests/*.js | `include_str!("../../requests/..")` | WIRED | 4 matches |
| requests/*.txt | requests/*.js | same op calls and results | WIRED | 4/4 payloads contain `op_rs3_get`/`op_rs3_insert`; live results identical |
| docs/js-ops.md | src/js/ops.rs | documented names and messages | WIRED | names and messages match code and tests |

### Data-Flow Trace (Level 4)

| Artifact | Data Variable | Source | Produces Real Data | Status |
|----------|---------------|--------|--------------------|--------|
| `patch_key` response | `out` | `state.store.get(&key)` → `JsExecutor::execute` → V8 completion value | Yes (live `"120"`, fixture results) | ✓ FLOWING |
| `op_rs3_*` results | store values | `OpState` → `ExecCtx.store` (the same `Store` handed in by `patch_key`) | Yes (JS insert visible to `GET /key/res`; JS delete visible to `GET /key/num` 404) | ✓ FLOWING |

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
|----------|---------|--------|--------|
| Gate | `cargo build --locked`; `cargo test`; `cargo clippy --all-targets -- -D warnings`; `cargo fmt --check` | all exit 0; 171 passed, 0 ignored (lib) | ✓ PASS |
| Env-free serial suite | `env -i PATH HOME cargo test -- --test-threads=1` | 171 passed | ✓ PASS |
| PATCH success / missing | live 18520 `PATCH /key/m`, `/key/ghost` | `200 "120"`, `404 not_found` | ✓ PASS |
| Samples via `.txt` payloads | live 18520 insert + PATCH fac/sum/bin/store | `"120"`, `"10"`, tree JSON, echo; `GET /key/res` `"1111"` | ✓ PASS |
| Errors keep server alive | live op_panic / throw / syntax / uncaught op error / caught op error, then `GET /` | 422 ×4, 200, 200 | ✓ PASS |
| TEST-06 live | `PATCH /key/code` vs `GET /list` | byte-equal | ✓ PASS |
| Concurrency | 32 parallel `PATCH /key/fac` | 32 × 200 | ✓ PASS |
| JS threads | `/proc/<pid>/task/*/comm` | 4 `rs3-js-*`, nproc 4 | ✓ PASS |
| Nested + print | live 18521 outer→inner, caught insert error, `Deno.core.print`, JS delete | `"11"`, `TypeError\|malformed JSON: ...`, `hello` on stdout, `"5"` then 404 | ✓ PASS |
| Built-in op abort probe | live 18524: each of 98 exposed built-in ops × 12 arg patterns in its own PATCH | no crash, no hang, no panic in stderr (only `op_dispatch_exception` → 422) | ✓ PASS |
| Callback-op probe | live 18525: throwing format-exception callback, throwing promise hooks / rejection handler, microtasks, timer schedule, wasm streaming callback + compileStreaming | 422/200, process alive | ✓ PASS |

### Probe Execution

Step 7c: not applicable. No `scripts/*/tests/probe-*.sh` exist, and no plan declares one.

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|-------------|-------------|--------|----------|
| HTTP-07 | 03-03 | PATCH executes the stored value as JS, returns stringified completion value | ✓ SATISFIED | SC1, truths 9, 20; live |
| JSRT-01 | 03-01 | current released deno_core, `#[op2]` + `extension!` | ✓ SATISFIED | SC2; deno_core 0.412.0 |
| JSRT-02 | 03-01 | five ops with legacy semantics | ✓ SATISFIED | SC2, truths 6, 8 |
| JSRT-03 | 03-01 | missing key / invalid input → catchable JS exception | ✓ SATISFIED | SC3, truth 8 |
| JSRT-04 | 03-01, 03-03 | dedicated JS threads, platform initialised once in main first | ✓ SATISFIED | SC4, truths 11, 17 |
| JSRT-05 | 03-02, 03-03 | all JS artefacts use `Deno.core.ops.op_rs3_*` | ✓ SATISFIED | SC5, truth 15 |
| JSRT-06 | 03-02 | old→new mapping documented | ✓ SATISFIED | SC5, truth 16 |
| ROBU-03 | 03-03 | throw / syntax error → 422 JSON with the JS message | ✓ SATISFIED | SC3, truth 10 |
| ROBU-04 | 03-01, 03-03 | no unwrap/expect on request data; ops never panic | ✓ SATISFIED | SC3, truths 7, 12, 19; clippy deny gate; op probe |
| TEST-03 | 03-03 | every route has a success test with status and bytes | ✓ SATISFIED | SC1 (seven named tests) |
| TEST-05 | 03-03 | fac/sum/bin run as fixtures with expected results | ✓ SATISFIED | SC1, truth 15 |
| TEST-06 | 03-03 | PATCH running `op_rs3_list` equals `GET /list` | ✓ SATISFIED | SC2 |

All 12 phase IDs are claimed by at least one plan. REQUIREMENTS.md maps exactly these 12 to Phase 3, so none is orphaned.

### Prohibitions

| Plan / Req | Statement (short) | Tier | Disposition |
|------------|-------------------|------|-------------|
| 03-01 ROBU-04 | no script-reachable abort path (op_panic disabled, no unwrap/expect/panic on request data, no panicking constructor) | test | ✓ enforced: `op_panic_is_disabled_and_catchable`, `uncaught_op_panic_is_a_script_error`, `worker_survives_panicking_jobs`, clippy `unwrap_used`/`expect_used = deny`. The "no `JsRuntime::new`" part is grep-only (clean). The verifier's op probe found no other abort path |
| 03-03 ROBU-04 | no request takes the server down | test | ✓ enforced: `test_patch_op_panic_is_422_and_server_keeps_serving`, `test_patch_executor_down_is_500_internal`, `test_patch_script_error_is_repeatable` |
| 03-01 JSRT-04 | unprotected platform only on dev edges | test | ⚠️ unverified-prohibition, flagged: no repo-resident enforcement. Verifier `cargo tree` evidence clean (human item 1) |
| 03-01 ROBU-04 / 03-03 TEST-03 | no gate-greening by suppression | test | ⚠️ unverified-prohibition, flagged: no repo-resident enforcement (Phase 4 CI grep pending). Verifier grep clean (human item 1) |
| 03-02 JSRT-05 | no legacy-API shim | test | ⚠️ unverified-prohibition, flagged. Grep clean (human item 1) |
| 03-02 JSRT-06 | limits not omitted or softened | test | ⚠️ unverified-prohibition, flagged. Docs contain every limit (human item 1) |
| 03-01 JSRT-02 / 03-03 HTTP-07 | no self-patching script in tests/smokes | judgment | ⚠️ flagged. Non-authoritative verdict: satisfied (human item 2) |

### Decision Coverage

All trackable CONTEXT.md decisions are honored by shipped artifacts (17/17, `check.decision-coverage-verify`).

### Test Quality Audit

| Test File | Linked Req | Active | Skipped | Circular | Assertion Level | Verdict |
|-----------|-----------|--------|---------|----------|-----------------|---------|
| src/http/mod.rs | HTTP-07, ROBU-03, ROBU-04, TEST-03, TEST-05, TEST-06 | all | 0 | no | Value (exact bytes or whole JSON body) | ✓ |
| src/http/kv.rs | TEST-03 | all | 0 | no | Value (golden bytes) | ✓ |
| src/js/ops.rs | JSRT-01..03, ROBU-04 | all | 0 | no | Value / Behavioral | ✓ |
| src/js/runtime.rs | HTTP-07, ROBU-03 | all | 0 | no | Value | ✓ |
| src/js/executor.rs | JSRT-04, ROBU-04 | all | 0 | no | Behavioral (thread name, panic survival, queue wait, isolation) | ✓ |

TEST-06 compares PATCH output against `GET /list`. Both go through `format_list`, which the requirement intends. `/list` itself is pinned independently by `list_golden_sorted` (legacy golden bytes), so the check is not circular. Fixture expectations (120, 10, the 5/3/8 tree) are independent oracles.

**Disabled tests on requirements:** 0. **Circular patterns detected:** 0. **Insufficient assertions:** 0. The single "ignored" doctest is generated inside deno_core's `extension!` macro (deno_core extensions.rs:486), not by this crate.

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| src/store/legacy.rs | 18, 39, 71 | `XXX` match | ℹ️ Info | false positive: `\uXXXX` escape notation in doc comments, not a debt marker |

No TODO/FIXME/HACK/placeholder, no stub returns, no unwrap/expect outside `#[cfg(test)]`.

Remaining review info items IN-03..IN-07 (iter-3) are triaged in 03-REVIEW-DISPOSITION.md and assigned to Phases 5/6. None blocks this phase's goal. IN-06 says a panic inside an op would still abort the process. The verifier confirmed that no rs3 op can panic (lint gate) and that none of the 98 other exposed built-in ops could be made to abort.

### Human Verification Required

#### 1. Test-tier prohibitions without repo-resident enforcement

**Test:** Review `git diff 9209f76..HEAD -- src Cargo.toml clippy.toml docs requests` for: lint suppression or `#[ignore]`; the unprotected-platform feature outside `[dev-dependencies]`; any legacy-API shim; any softened limit in docs/js-ops.md.
**Expected:** None present. The verifier's evidence is clean: no suppression or ignore found by grep; the normal-edge feature count is 0 and the dev-edge count is 1; legacy syntax appears only in the docs migration rows; all limits are stated.
**Why human:** These are test-tier prohibitions, but no wired test or CI check enforces them yet, so the fail-closed default cannot mark them green.

#### 2. Judgment-tier prohibition: no self-patching script

**Test:** Inspect every `op_rs3_patch` use in src/.
**Expected:** Every chain ends at a non-patching key. No key patches itself or forms a cycle.
**Why human:** Judgment-tier items get only a non-authoritative verdict. The verifier's verdict is "satisfied".

#### 3. docs/js-ops.md readability

**Test:** Read the guide as a script author migrating a legacy script.
**Expected:** Clear enough to migrate without reading the Rust source.
**Why human:** Prose clarity. The content's accuracy was verified against the code and the live binary.

### Gaps Summary

No gaps. All 5 roadmap success criteria and 17 grouped plan truths hold in the codebase. The verifier confirmed them with its own four-command gate run, an env-free serial run, and live-binary smokes on ports 18520-18525:

- PATCH success, missing key, and the four migrated `.txt` payloads.
- Every script-error class answering 422 while the server stays up.
- TEST-06 byte equality.
- 32 parallel PATCHes.
- The thread count.

The verifier also ran an adversarial probe beyond the plans: all 98 other built-in ops exposed in `Deno.core.ops`, plus callback-setting paths, could not abort the process. This backs the "no script or op error can abort the server" clause beyond the op_panic case the phase targeted. All 12 requirement IDs are satisfied, and none is orphaned.

The status is `human_needed` only because of policy-flagged items:

- Five test-tier prohibitions have no repo-resident enforcement yet. Phase 4 CI is expected to add the grep gates.
- One judgment-tier prohibition gets a non-authoritative "satisfied" verdict.
- One docs-readability judgment carried over from 03-02-SUMMARY.

Two items are deliberate and are not gaps: the nesting-depth limit and the execution timeout. Both are scheduled for Phase 5 and disclosed in docs/js-ops.md.

---

_Verified: 2026-10-20T21:08:05Z_
_Verifier: Claude (gsd-verifier)_
