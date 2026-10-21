---
phase: 03-javascript-runtime-port
plan: 03
subsystem: api
tags: [actix-web, deno_core, patch, js_error, fixtures, thiserror]

requires:
  - phase: 03-javascript-runtime-port
    provides: "03-01 JsExecutor pool (start/execute/closed), ExecError {Script, Unavailable, Internal}, init_platform, test_executor"
  - phase: 03-javascript-runtime-port
    provides: "03-02 migrated requests/{fac,sum,bin,store}.{js,txt} and docs/js-ops.md with the asserted error messages"
provides:
  - "PATCH /key/{key} runs the stored value on the rs3-js pool: 404 before queueing, 200 with the JSON string of String(completion value)"
  - "main: init_platform, then JsExecutor::start(&JsSettings::default()), then AppState::new(settings, js), then HttpServer::new; pool start failure exits 1"
  - "AppState.js: JsExecutor; test_state() shares the cfg(test) executor"
  - "ApiError::JsError(String) -> js_error / 422; From<ExecError> (Script -> JsError, Unavailable/Internal -> Internal 500 with fixed body); NotImplemented removed"
  - "PATCH fixture, listing-equality, stringification, adjacency and error tests; Phase 2 review IN-01, IN-04 and IN-05 resolved"
affects: [phase 4 CI (x86_64 PKU), phase 5 depth limit and timeout (422/504 on PATCH), phase 6 README]

actuals:
  tokens: 5437
  tasks: 3
  commits: 5
plan_head_before: 893900abd51ea4befbfbc858e1d59386399b5467
plan_head_after: 537afe087422cf280fdc5a58181947c4f0e584ef

tech-stack:
  added: []
  patterns:
    - "HTTP handlers only await JsExecutor::execute; src/http has no deno_core import"
    - "Script errors carry only the JS exception message (422); host faults log to stderr and answer a fixed 500 body"
    - "Struct update syntax AppState { js: JsExecutor::closed(), ..test_state() } for host-fault tests"
    - "HTTP fixture tests include_str! the shipped requests/*.js samples"

key-files:
  created:
    - .planning/phases/03-javascript-runtime-port/tdd-evidence/03-03-task2-red.json
    - .planning/phases/03-javascript-runtime-port/tdd-evidence/03-03-task2-red.junit.xml
  modified:
    - src/state.rs
    - src/main.rs
    - src/http/mod.rs
    - src/http/error.rs
    - src/http/kv.rs
    - src/store/legacy.rs

key-decisions:
  - "Task 1 mapped every ExecError to 500 on purpose so Task 2's 422 tests had a genuine RED (500 vs 422 on the status assertion)"
  - "From<ExecError> logs only host faults (error: JS host failure: ...); script failures are not logged again because run_script already wrote the stack to stderr"
  - "HTTP test helpers return TestRequest and call .to_request() at the call site, because actix-web 4.17 does not re-export actix_http::Request and actix-http is not a direct dependency"

patterns-established:
  - "Every PATCH test runs on its own test_state() app over the one shared test executor"
  - "Response bodies are compared as exact bytes (web::Bytes::from_static) or as a whole serde_json::Value, never by substring"

requirements-completed: [HTTP-07, ROBU-03, ROBU-04, TEST-03, TEST-05, TEST-06, JSRT-04, JSRT-05]

coverage:
  - id: D1
    description: "PATCH /key/{key} runs the stored script on the rs3-js pool and answers 200 with the JSON string of the completion value; a missing key answers 404 before queueing"
    requirement: HTTP-07
    verification:
      - kind: unit
        ref: "src/http/mod.rs#test_patch_runs_script_200, test_patch_existing_runs_script, test_patch_missing_is_404_json, test_patch_is_ok, test_patch_body_is_listing"
        status: pass
      - kind: e2e
        ref: "live smoke 18500 (PATCH 6*20 -> \"120\" 200, ghost -> 404)"
        status: pass
    human_judgment: false
  - id: D2
    description: "Pool started once in main after init_platform on the main thread; actix workers never run V8; rs3-js thread count equals nproc"
    requirement: JSRT-04
    verification:
      - kind: other
        ref: "Task 1 verify block 2 and phase static gate (startup order awk, no deno_core in src/http)"
        status: pass
      - kind: e2e
        ref: "live smoke 18500 (/proc/<pid>/task/*/comm: 4 rs3-js threads, nproc 4)"
        status: pass
    human_judgment: false
  - id: D3
    description: "Script throws, syntax errors, non-Error throws, uncaught op errors and op_panic answer 422 js_error with only the JS message; the server keeps serving"
    requirement: ROBU-03
    verification:
      - kind: unit
        ref: "src/http/mod.rs#test_patch_throw_is_422_js_error, test_patch_syntax_error_is_422_js_error, test_patch_non_error_throw_is_422_js_error, test_patch_uncaught_op_error_is_422_js_error, test_patch_script_error_is_repeatable, test_patch_error_message_keeps_non_ascii"
        status: pass
      - kind: e2e
        ref: "live smoke 18501"
        status: pass
    human_judgment: false
  - id: D4
    description: "op_panic over HTTP and a closed executor cannot take the process down; host faults answer the fixed 500 internal body; ApiError::Internal response tested (IN-01)"
    requirement: ROBU-04
    verification:
      - kind: unit
        ref: "src/http/mod.rs#test_patch_op_panic_is_422_and_server_keeps_serving, test_patch_executor_down_is_500_internal, test_internal_error_response_is_500_json"
        status: pass
      - kind: e2e
        ref: "live smoke 18501 (GET / and PATCH 1+1 answered after op_panic)"
        status: pass
    human_judgment: false
  - id: D5
    description: "fac, sum, bin and store samples run as PATCH fixtures in tests and on the live binary; sum/bin/sum share one store without top-level collisions"
    requirement: TEST-05
    verification:
      - kind: unit
        ref: "src/http/mod.rs#fixture_fac_is_120, fixture_sum_is_10, fixture_bin_is_tree_json, fixture_store_writes_res, fixtures_share_one_store_in_any_order"
        status: pass
      - kind: e2e
        ref: "live smokes 18502 (migrated .txt payloads) and 18503 (32 parallel PATCHes, all 200)"
        status: pass
    human_judgment: false
  - id: D6
    description: "PATCH of op_rs3_list is byte-equal to GET /list across escapes, byte order and the single-entry case"
    requirement: TEST-06
    verification:
      - kind: unit
        ref: "src/http/mod.rs#test_patch_listing_matches_list_with_escapes_and_order, test_patch_body_is_listing"
        status: pass
      - kind: e2e
        ref: "live smoke 18504 (PT == LS)"
        status: pass
    human_judgment: false
  - id: D7
    description: "All seven routes have byte-exact success tests; stringification and GET/PATCH/DELETE adjacency pinned"
    requirement: TEST-03
    verification:
      - kind: unit
        ref: "test_index_body_is_welcome, test_ip_body_is_ip, test_insert_is_ok, test_get_is_ok, test_delete_is_ok, list_golden_sorted, test_patch_runs_script_200, test_patch_stringifies_completion_values, test_patch_get_delete_on_one_key_hit_their_own_handlers"
        status: pass
      - kind: e2e
        ref: "live smoke 18504 (all-route goldens)"
        status: pass
    human_judgment: false
  - id: D8
    description: "Phase 2 review IN-04 (problem-order doc plus pinning test) and IN-05 (renamed truncated-input tests, HTTP 422 for valid deep nesting)"
    verification:
      - kind: unit
        ref: "src/store/legacy.rs#parse_unrepresentable_value_beats_earlier_non_string, parse_rejects_truncated_deep_nesting; src/http/kv.rs#insert_truncated_deep_nesting_is_400_json, insert_valid_deep_nesting_is_422_json"
        status: pass
    human_judgment: false
  - id: D9
    description: "Phase gate on the merged tree: TOOL-03, env -i, serial, 5x, static/ARCH gate, lockfile one tokio line"
    requirement: JSRT-05
    verification:
      - kind: other
        ref: "Task 3 verify blocks 3-5 (cargo build --locked, test, clippy -D warnings, fmt --check; env -i; --test-threads=1; 5x; static gate)"
        status: pass
    human_judgment: false

duration: 6min
completed: 2026-10-20
status: complete
---

# Phase 3 Plan 03: PATCH over HTTP Summary

**PATCH /key/{key} now runs the stored script on the rs3-js pool. main starts the pool once after the V8 platform, the handler only awaits it, and the result comes back as a 200 JSON string. Script failures answer 422 js_error carrying only the JS message, host faults answer a fixed 500 internal body, and the four bundled samples run as fixtures in tests and on the live binary.**

## Performance

- **Duration:** about 6 min
- **Started:** 2026-10-20T20:18:28Z
- **Completed:** 2026-10-20T20:24:30Z
- **Tasks:** 3
- **Files modified:** 8 (6 source, 2 evidence)

## Accomplishments

- PATCH is wired end to end. A missing key gets a 404 before anything is queued. Otherwise the handler awaits `state.js.execute(store, key, source)` and answers `HttpResponse::Ok().json(out)`.
- Startup order in `main` (46 lines) is `Settings::load`, `init_platform`, `JsExecutor::start`, `AppState::new(settings, js)`, `HttpServer::new`. If the pool fails to start, main prints `error: cannot start JS runtime: ...` and exits 1.
- `ApiError::JsError` maps to 422 `js_error`. `ExecError::Unavailable` and `ExecError::Internal` map to a 500 with the fixed `internal error` message. Both matches are exhaustive with no wildcard arm. `NotImplemented` and the 501 path are gone. Phase 2 IN-01 is resolved: `Internal` is now constructed and tested.
- The two parked PATCH tests run again, and `src/` has no `#[ignore]` left. Final lib test count: **154 passed, 0 ignored**. The Doc-tests line reads `0 passed; 0 failed; 1 ignored`; that ignored test is the `extension!` macro's `rust,ignore` doctest (Pitfall 7).
- Timing: a warm full `cargo test` takes **0.86 s** wall, and the lib suite reports 0.31 s.

## Live Smoke Results (protected-platform binary)

- **18500:**
  - `GET /` answers `"Welcome to rs3"`.
  - The insert echo is `"{\"m\":\"6*20\"}"`.
  - `PATCH /key/m` answers `"120" 200`, and `PATCH /key/ghost` answers `404`.
  - **rs3-js thread count is 4, and nproc is 4.**
- **18501 (422 bodies):**
  - throw: `{"error":"js_error","message":"Uncaught Error: boom"}`, status `422 application/json`
  - syntax error: `{"error":"js_error","message":"Uncaught SyntaxError: Unexpected token ';'"}`
  - op_panic: `{"error":"js_error","message":"Uncaught Error: op is disabled"}`, status 422
  - uncaught op error: `{"error":"js_error","message":"Uncaught Error: key not found: nope"}`
  - After all of these, `PATCH ok` (1+1) still answers `"2"` and `GET /` answers `"Welcome to rs3" 200`, so the process survived.
- **500 body:** `{"error":"internal","message":"internal error"}`. This is asserted on the closed-executor app in `test_patch_executor_down_is_500_internal`. A live executor cannot be closed from outside, so this body is not observed on the live binary.
- **18502 (migrated `requests/*.txt` payloads):**
  - fac gives `"120"` and sum gives `"10"`.
  - bin gives `"{\"value\":5,\"left\":{\"value\":3},\"right\":{\"value\":8}}"`.
  - store gives `"{\"res\":\"1111\"}"`, and then `GET /key/res` gives `"1111"`.
- **18503 (burst):** 32 parallel PATCHes of fac, 16 at a time, gave `32 200`. `GET /list` afterwards answered 200.
- **18504 (all routes):**
  - The `/`, `/ip`, insert echo, GET, `/list` and DELETE goldens all match.
  - The PATCH of the `op_rs3_list` script is byte-equal to `GET /list`: `"[{\"code\" : \"Deno.core.ops.op_rs3_list().toString();\"},{\"insert_test\" : \"John Doe\"}]"`.
  - PATCH on the deleted key gives 404.
- No listener was left on 18500 to 18504 after the smokes.

## Fixture Outputs (tests)

These match the live results above:
- `fixture_fac_is_120` gives `"120"`, `fixture_sum_is_10` gives `"10"`, and `fixture_bin_is_tree_json` gives the exact tree JSON string.
- `fixture_store_writes_res` gives the echo `"{\"res\":\"1111\"}"`, and `GET /key/res` then gives `"1111"`.
- `fixtures_share_one_store_in_any_order`: sum, bin and sum all succeed with their expected bodies.

## Task Commits

1. **Task 1 (tracer): PATCH on the pool**: `a4c70a3` (feat). Auto mode is active, so the tracer gate re-ran the verify (including the 18500 smoke) green before expanding, and no checkpoint was synthesised.
2. **Task 2 (TDD): 422 js_error and 500 internal**
   - `f26f568` test (RED plus evidence)
   - `8de354e` feat (GREEN)
3. **Task 3: fixtures, equality, routes, housekeeping, gate**
   - `c69e904` test
   - `537afe0` docs (IN-04/IN-05)

## TDD Evidence

- **RED:** `http::tests::test_patch_throw_is_422_js_error` failed on the status assertion in `assert_json_error` (`src/http/mod.rs:70`): left 500, right 422. Exit 101, JUnit `failures="1"`. Stderr showed the script running on the pool (`js error in "t": Error: boom` and `error: JS execution failed: Uncaught Error: boom`). The classifier verdict was **RED_EVIDENCE_OK** (`target_test_failed`).
  - **Semantic assessment:** the target compiled, ran and failed on the planned 500-vs-422 status assertion, because the js_error variant and the Script mapping did not exist yet. There was no build, load or fixture failure.
  - At RED, the other six 422 tests failed the same way. The two 500 tests passed, as the plan expected.
- **GREEN:** `8de354e`. All 51 http tests passed at that point. No REFACTOR commit was needed.
- The gate commits `test(03-03)` (`f26f568`) and `feat(03-03)` (`8de354e`) are present and in order.

## Files Created/Modified

- `src/state.rs`: the `pub js: JsExecutor` field, `AppState::new(settings, js)`, and `test_state()` on `crate::js::test_executor()`.
- `src/main.rs`: starts the pool after `init_platform` and exits 1 if that fails. It also has the D-07 comment.
- `src/http/error.rs`: adds `JsError` and `From<ExecError>`, removes `NotImplemented`, and lists the codes in the module doc.
- `src/http/mod.rs`: the new `patch_key`, plus 19 new or renamed tests and the helpers `patch_seeded`, `send`, `seeded`, `patch_req` and `get_req`.
- `src/http/kv.rs`: renames `insert_truncated_deep_nesting_is_400_json` and adds `insert_valid_deep_nesting_is_422_json`.
- `src/store/legacy.rs`: corrects the IN-04 doc, adds `parse_unrepresentable_value_beats_earlier_non_string`, and renames `parse_rejects_truncated_deep_nesting`.
- `tdd-evidence/03-03-task2-red.{json,junit.xml}`: the RED evidence.

## Decisions Made

See key-decisions in the frontmatter.

## Deviations from Plan

None. The plan was executed exactly as written. No file outside the plan's ownership was touched: the ownership guards on src/js, Cargo.toml/lock, clippy.toml, config.rs, lib.rs, store/mod.rs, requests/, docs/ and example.js all passed.

## Issues Encountered

- Two compile errors in the first draft of the Task 3 test helpers were fixed before the commit:
  - A pattern binding named `status` resolved to the `#[get("/ip")]` handler unit struct `status`. The binding was renamed to `code`.
  - `actix_http::Request` cannot be named, because actix-web does not re-export it. The helpers now return `TestRequest`, and the generic `send<S, R>` calls `atest::call_service`.

## Known Stubs

None.

## Threat Flags

None. No new network endpoint, auth path or trust-boundary surface beyond the plan's threat model:
- T-03-11 is mitigated by the exact-body 422 and 500 tests and by the 18501 smoke.
- T-03-12, T-03-13, T-03-14 and T-03-16 are covered by the static gate, the closed-executor test, the op_panic test and smoke, and the cargo tree feature check.

## Phase Gate

- `cargo build --locked`, `cargo test`, `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` all exit 0.
- `env -i PATH HOME cargo test` passes, `--test-threads=1` passes (154 passed), and 5 of 5 repeated `cargo test -q` runs pass.
- The static gate passes:
  - No `#[ignore]` in src/, and the ignored lib list is empty.
  - No suppression attributes or `.cargo`, and no global store, sleep or env mutation.
  - No `JsRuntime::new(`. The layer direction is http -> js -> store, and deno_core is used only in src/js.
  - `op_rs3_patch` is still `op2(reentrant)`. The startup order is correct, and main.rs is 46 lines.
  - There is no legacy JS syntax and no 501 path.
  - `unsafe_use_unprotected_platform` is absent from deno_core's normal edges.
  - `git diff 9209f76 -- Cargo.lock` is exactly `+ "tokio",`.
- TOOL-01 regression check: there is no `rust-toolchain` file, and the toolchain is `cargo 1.99.1`.

## User Setup Required

None. No external service configuration is required.

## Next Phase Readiness

The phase is complete and ready for phase verification.

**Phase 4 carry-overs:**
- Verify both platform paths on x86_64 with PKU: the protected platform in the binary and the unprotected platform in tests through the dev feature.
- The recorded fallback, if the x86 runner crashes: move the `unsafe_use_unprotected_platform` feature from `[dev-dependencies]` to `[dependencies]`.

**Phase 5 items (unchanged):**
- A self-patching script costs about 800 MB per request until the depth limit exists (ROBU-05).
- Non-terminating scripts pin a worker until the timeout exists (ROBU-06).
- No test or smoke in this phase exercises either one.

---
*Phase: 03-javascript-runtime-port*
*Completed: 2026-10-20*

## Self-Check: PASSED

All 8 key files exist on disk. The commits a4c70a3, f26f568, 8de354e, c69e904 and 537afe0 are on this branch (evaluation-scope resolved them from plan subjects). Every task's acceptance criteria and the plan-level verification (test lists, TOOL-03, env -i, serial, 5x, static gate, five live smokes, TOOL-01, docs drift check) re-ran green on the final tree.
