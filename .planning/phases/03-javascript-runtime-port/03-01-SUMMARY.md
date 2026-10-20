---
phase: 03-javascript-runtime-port
plan: 01
subsystem: js-runtime
tags: [deno_core, v8, op2, extension, tokio, thread-pool, catch_unwind]

requires:
  - phase: 02-http-api-port-error-handling
    provides: "Store facade (owned values, re-entrancy-safe guards), legacy codec (parse_insert_body, format_insert_echo, format_list), lint gate"
provides:
  - "src/js/ops.rs: ExecCtx, five #[op2] store ops (op_rs3_patch reentrant), StrArg string-argument coercion, rs3_ext extension with the op_panic-disabling middleware"
  - "src/js/runtime.rs: run_script(ctx, name, source) on a fresh JsRuntime::try_new, result read through deno_core::scope!"
  - "src/js/executor.rs: JsExecutor bounded rs3-js-* pool (16 MiB stacks, tokio current_thread per worker, catch_unwind per job, oneshot replies, closed() for tests)"
  - "src/js/mod.rs: ExecError {Script, Unavailable, Internal}, JsSettings (in code), cfg(test) test_executor/block_on/run_js"
  - "Cargo.toml: direct tokio (rt, sync); dev-only deno_core unsafe_use_unprotected_platform"
affects: [03-03 PATCH wiring, 03-02 docs/js-ops.md, phase 4 CI (x86 platform), phase 5 depth limit and timeout]

actuals:
  tokens: 10644
  tasks: 3
  commits: 7
plan_head_before: bc4eac8b75509a9468bae14d12d58c2f8d6d6ee4
plan_head_after: ad6da4c4fde106f63c988be97bdbf0b956f50155

tech-stack:
  added: ["tokio 1.54.0 (direct; already locked via deno_core)"]
  patterns:
    - "One fresh JsRuntime per script, created with try_new and dropped before returning; only String/ExecError cross threads"
    - "Per-job ExecCtx injected into OpState via extension! options/state; no global store"
    - "extension! middleware disables dangerous deno_core built-in ops (op_panic)"
    - "FromV8 newtype (StrArg) for string op arguments instead of #[string] String"
    - "Bounded tokio mpsc + Arc<tokio::Mutex<Receiver>> + oneshot pool with a startup ready handshake"

key-files:
  created: [src/js/ops.rs, src/js/runtime.rs, src/js/executor.rs, .planning/phases/03-javascript-runtime-port/tdd-evidence/03-01-task1-red.json, .planning/phases/03-javascript-runtime-port/tdd-evidence/03-01-task1-red.junit.xml, .planning/phases/03-javascript-runtime-port/tdd-evidence/03-01-task3-red.json, .planning/phases/03-javascript-runtime-port/tdd-evidence/03-01-task3-red.junit.xml]
  modified: [Cargo.toml, Cargo.lock, src/js/mod.rs]

key-decisions:
  - "D-04 holds on deno_core 0.412: a nested JsRuntime inside #[op2(reentrant)] op_rs3_patch works on the same thread under the debug test profile; no Rule 4 stop"
  - "String op arguments use a FromV8 newtype StrArg: strings unchanged, other primitives via String(..) (no arg -> \"undefined\", 5 -> \"5\"), objects and symbols throw TypeError without running user toString; deno_core's #[string] String maps every non-string to \"\" (runtime/ops.rs:247), contradicting research S12"
  - "Missing key -> JsErrorBox::generic (Error), bad insert -> JsErrorBox::type_error (TypeError); no custom classes"
  - "Nested script errors rethrow as Error with one leading 'Uncaught ' stripped, no key prefix"
  - "js module docs say 'free of actix-web' (crate name) so the layer grep gate on the identifier actix_web stays meaningful"

patterns-established:
  - "Every string-taking op uses #[scoped] StrArg"
  - "Tests reach JS only through crate::js::run_js / block_on(test_executor()...), i.e. through the shared pool"

requirements-completed: [JSRT-01, JSRT-02, JSRT-03, JSRT-04, ROBU-04]

coverage:
  - id: D1
    description: "Nested op_rs3_patch runs the target key's script in a reentrant nested runtime (own scope, same store, catchable errors without doubled Uncaught)"
    requirement: JSRT-02
    verification:
      - kind: unit
        ref: "src/js/ops.rs#nested_patch_returns_inner_result, nested_patch_has_own_top_level_scope, nested_throw_is_catchable_without_uncaught_prefix, nested_patch_of_missing_key_throws, exec_ctx_nested_increments_depth_and_shares_store"
        status: pass
    human_judgment: false
  - id: D2
    description: "op_panic is disabled and can no longer abort the process"
    requirement: ROBU-04
    verification:
      - kind: unit
        ref: "src/js/ops.rs#op_panic_is_disabled_and_catchable, uncaught_op_panic_is_a_script_error"
        status: pass
    human_judgment: false
  - id: D3
    description: "Five #[op2] ops registered through extension! rs3_ext with per-job ExecCtx; list/get/insert/delete with legacy semantics and catchable Error/TypeError"
    requirement: JSRT-01
    verification:
      - kind: unit
        ref: "src/js/ops.rs#list_*, get_*, insert_*, delete_returns_old_value_then_throws, numeric_key_never_aliases_the_empty_key, object_and_symbol_arguments_throw_type_error"
        status: pass
    human_judgment: false
  - id: D4
    description: "Missing keys and invalid input surface as catchable JS exceptions"
    requirement: JSRT-03
    verification:
      - kind: unit
        ref: "src/js/ops.rs#get_missing_key_throws_catchable_error, insert_malformed_throws_type_error_and_writes_nothing, insert_wrong_shape_throws_type_error_and_writes_nothing"
        status: pass
    human_judgment: false
  - id: D5
    description: "Scripts run on a bounded pool of named 16 MiB rs3-js threads with fresh runtimes, panic isolation, store isolation, waiting on a full queue, Unavailable when closed"
    requirement: JSRT-04
    verification:
      - kind: unit
        ref: "src/js/executor.rs#tests (9 tests)"
        status: pass
    human_judgment: false
  - id: D6
    description: "run_script stringifies completion values and maps thrown/syntax/op errors to ExecError::Script"
    verification:
      - kind: unit
        ref: "src/js/runtime.rs#tests (5 tests)"
        status: pass
    human_judgment: false
  - id: D7
    description: "Dependency and platform gates: one lockfile line, tokio 1.54.0, unprotected platform only on dev edges, single v8, cargo build --locked"
    verification:
      - kind: other
        ref: "Task 1 verify block 3 (cargo tree -e features,normal / -e features / -d; git diff 9209f76 -- Cargo.lock)"
        status: pass
    human_judgment: false

duration: 9min
completed: 2026-10-20
status: complete
---

# Phase 3 Plan 01: JS Engine Slice Summary

**deno_core 0.412 engine for rs3: five #[op2] store ops in one extension! with per-job ExecCtx, a reentrant op_rs3_patch running nested runtimes, op_panic disabled by op middleware, and a bounded rs3-js thread pool (16 MiB stacks, tokio current_thread per worker, catch_unwind per job)**

## Performance

- **Duration:** 9 min
- **Started:** 2026-10-20T20:02:35Z
- **Completed:** 2026-10-20T20:11:44Z
- **Tasks:** 3
- **Files modified:** 10 (6 source/config, 4 evidence)

## Accomplishments

- The riskiest assumption (D-04) holds in-repo. A `#[op2(reentrant)]` `op_rs3_patch` creates, runs and drops a nested `JsRuntime` on the same thread under the default debug test profile. With `num=5` and `inner` set to the `op_rs3_get('num')` call, the outer script gives `"5"`. Outer and inner `let q` do not collide (`"71"`). A nested `RangeError('inner boom')` is caught as `true|RangeError: inner boom`. `op_rs3_patch('ghost')` throws `key not found: ghost`.
- `op_panic` is disabled through `rs3_ext` middleware. Caught, it gives `true|op is disabled`. Uncaught, it gives `ExecError::Script("Uncaught Error: op is disabled")`, and the process keeps running.
- All five ops work against the injected per-job store:
  - `op_rs3_list` returns exactly `legacy::format_list(&store.list())`.
  - `op_rs3_insert` validates the whole body before `insert_all`; malformed and wrong-shape bodies throw a `TypeError` and write nothing.
  - `op_rs3_get` and `op_rs3_delete` throw `Error: key not found: <key>` on a missing key.
- `JsExecutor` runs every job on `rs3-js-*` threads. Panicking jobs give `Internal("js worker panicked")` and the pool keeps working. Stores stay isolated under 8 parallel submitters. A full queue (1 worker, capacity 1, 8 jobs) makes callers wait and every job returns Ok. A closed executor answers `Unavailable`.
- Final lib test count: **132 passed, 2 ignored**. The 2 ignored tests are the parked PATCH tests, kept for plan 03-03. 37 of the passing tests are in `js::`. A warm full `cargo test` takes 0.75 s wall (lib suite 0.22 s). The first `cargo test` after the Cargo.toml edit, including the dev-feature deno_core rebuild, took 19.9 s.

## Task Commits

1. **Task 1: tracer, nested op_rs3_patch and op_panic guard**
   - `7b6d0b4` feat: seam (`run_script`, `op_rs3_get`, `ExecCtx`, Cargo.toml/lock)
   - `4c58df0` test: RED spike tests plus evidence
   - `4c38399` feat: GREEN reentrant `op_rs3_patch` and `op_panic` middleware
   - `5188b33` fix: doc wording (deviation 1)
2. **Task 2: bounded rs3-js pool**: `4975492` (feat)
3. **Task 3: list/insert/delete and plan gate**
   - `6bbf120` test: RED plus evidence
   - `ad6da4c` feat: GREEN, including the StrArg fix (deviation 2)

## TDD Evidence

- **RED Task 1:** `js::ops::tests::nested_patch_returns_inner_result` returned `Err(Script { message: "Uncaught TypeError: Deno.core.ops.op_rs3_patch is not a function" })` instead of `Ok("5")`. Exit 101, JUnit `failures="1"`. Classifier verdict: **RED_EVIDENCE_OK**. Semantic assessment: the target compiled, ran and failed on its planned assertion because the op was not registered yet. The seam test passed in the same build.
- **op_panic abort at RED (before the middleware):** running `js::ops::tests::op_panic_is_disabled_and_catchable --exact` exited with **134** (SIGABRT). Stderr showed `JS PANIC: x`, then `panicked at .../deno_core-0.412.0/ops_builtin.rs:155:3`, then `panic in a function that cannot unwind`. This is not assertion evidence, so it was not sent to the classifier.
- **RED Task 3:** `js::ops::tests::list_matches_shared_codec_sorted` failed on the `run_js` assertion with `op_rs3_list is not a function`. Its first assertion, that the expected text equals `format_list`, passed. Classifier verdict: **RED_EVIDENCE_OK**.
- **GREEN:** `4c38399` and `ad6da4c`. No REFACTOR commit was needed.
- The gate commits `test(03-01)` and `feat(03-01)` are both present and in order.

## Files Created/Modified

- `src/js/ops.rs`: ExecCtx, StrArg, `to_js`, the five ops, `rs3_ext` (middleware and state hook), 26 tests
- `src/js/runtime.rs`: `run_script` (`try_new`, `execute_script`, `scope!`, stack to stderr only), 5 tests
- `src/js/executor.rs`: JsExecutor (`start`, `run`, `execute`, `closed`), worker loop, 9 tests
- `src/js/mod.rs`: module wiring, ExecError, JsSettings, cfg(test) helpers
- `Cargo.toml`: `tokio = { version = "1.54.0", features = ["rt", "sync"] }` and `[dev-dependencies] deno_core` with `unsafe_use_unprotected_platform`
- `Cargo.lock`: compared with 9209f76, exactly one changed line, `+ "tokio",`, in the rs3 package entry

## Decisions Made

See key-decisions in the frontmatter. The most significant is StrArg (deviation 2).

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] The plan's module-doc wording tripped the plan's own layer gate**
- **Found during:** Task 1 acceptance check AC3
- **Issue:** The plan asked for module docs saying "free of actix_web". The gate `! grep -rnE 'actix_web|...' src/js/` matches that literal text.
- **Fix:** The docs now say "free of actix-web", the crate name. The gate is unchanged and still catches any real `actix_web` import.
- **Files modified:** src/js/{mod,ops,runtime}.rs (docs only)
- **Committed in:** 5188b33

**2. [Rule 1 - Bug] `#[string] String` op arguments turn every non-string into `""`**
- **Found during:** Task 3 RED. `get_coerces_its_argument` failed with `key not found: ` (empty key).
- **Issue:** deno_core 0.412's `#[string] String` argument conversion (`runtime/ops.rs:247`, `if !string.is_string() { return String::new(); }`) maps `undefined`, `null`, numbers, booleans and objects to `""`. Research S12 and the plan truth say `op_rs3_get()` reads `"undefined"` and `op_rs3_get(5)` reads `"5"`. With the empty-string behaviour, `op_rs3_delete(5)` would silently delete the key `""`, which is a data-confusion bug.
- **Fix:** Added a private `FromV8` newtype, `StrArg`, used with `#[scoped]` on every string-taking op (get, insert, delete, patch):
  - Strings pass through unchanged.
  - Other non-symbol primitives are converted with `String(..)` semantics. This cannot throw or run user code.
  - Objects and symbols throw `TypeError: op argument must be a string or another primitive value`. Converting an object would run user `toString` inside the op, and a `toString` that calls an op would hit the debug reentrancy abort.
- **Added tests:** `numeric_key_never_aliases_the_empty_key` and `object_and_symbol_arguments_throw_type_error`. The second one also asserts that `toString` never ran.
- **Files modified:** src/js/ops.rs
- **Verification:** all 37 js tests pass, plus the full plan gate
- **Committed in:** ad6da4c
- **Follow-up for 03-02/03-03:** `docs/js-ops.md` should say that object and symbol arguments throw a TypeError. The no-argument and number cases match the documented "coerced to string" rule.

---

**Total deviations:** 2 auto-fixed (2 Rule 1).
**Impact on plan:** Both were needed for correctness. Deviation 2 makes the code meet the plan's own coercion truth, which plain `#[string]` cannot. No architectural change and no scope creep.

## Issues Encountered

None beyond the deviations. The D-04 dead-end rule was not triggered.

## Plan Gate Results

- `cargo build --locked`, `cargo test` (132 passed, 2 ignored), `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` all exit 0.
- `env -i PATH HOME cargo test` passes, `--test-threads=1` passes, and 5 of 5 repeated `cargo test -q` runs pass.
- Static gate passes:
  - The ignored list is exactly `test_patch_body_is_listing` and `test_patch_is_ok`.
  - No suppression attributes, no lowered lints, no `.cargo` directory.
  - Layer direction is http -> js -> store, and deno_core appears only in src/js.
  - Only `try_new` is used, with no sleep and no env mutation.
- Feature check: `unsafe_use_unprotected_platform` is absent from `cargo tree -e features,normal -i deno_core` and present in `-e features`. There is no duplicate v8.

## Known Stubs

None.

## Threat Flags

None. The StrArg conversion sits at the JS -> Rust op boundary already covered by T-03-02. It returns `Result` and never panics.

## User Setup Required

None. No external service configuration is required.

## Next Phase Readiness

- Plan 03-03 can wire `PATCH /key/{key}` to `state.js.execute(store, key, source)` and map `ExecError`: Script becomes 422, and Unavailable or Internal becomes 500. `JsExecutor::closed()` covers the 500 test, and `JsExecutor` derives `Clone + Debug` for AppState.
- Known Phase 5 item, unchanged: a self-patching script costs about 800 MB per request until the depth limit exists. No test or smoke exercises it.
- x86_64/PKU platform behaviour still needs to be verified in Phase 4 CI. The fallback is to move the feature to `[dependencies]`.

---
*Phase: 03-javascript-runtime-port*
*Completed: 2026-10-20*
