---
phase: 05-js-execution-limits
plan: 01
subsystem: js-runtime
tags: [deno_core, v8, op2, depth-limit, actix-web, thiserror, tdd]

# Dependency graph
requires:
  - phase: 03-javascript-runtime-port
    provides: ExecCtx with depth, the shared host_fault flag with live_ctx refusal, the single-log rule, JsExecutor and the ApiError mapping
  - phase: 04-ci-gate-on-x86-64
    provides: scripts/ci-guards.sh and scripts/ci-smoke.sh (the end-of-plan gate)
provides:
  - ExecError::DepthLimit { limit, chain } and ApiError::JsDepthLimit -> 422 js_depth_limit
  - JsSettings.max_patch_depth (default 4, clamped 1..=64 in Limits::from_settings)
  - DepthStop and the per-job JobState (host fault, first-wins depth stop, stopped(), the job's one log sink)
  - ExecCtx { job, chain, max_patch_depth }, ExecCtx::new(store, job, root, max_patch_depth), ExecCtx::nested(key)
  - depth check in op_rs3_patch before the store read; live_ctx refusal in D-04 order
  - executor classify(): the single classification point (host fault > depth stop > result)
affects: [05-02 timeout (adds timed_out/termination group to JobState and a Timeout arm to classify), 05-03 docs and stack clamp, phase 6 README]

# Actuals (#2632)
actuals:
  tokens: 14634
  tasks: 3
  commits: 6
plan_head_before: 39dbcb22976991bb84af6f1411e08fb21d3ec06a
plan_head_after: 992e8f8367e8f2033e3334857f5599c0df34f1bf

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Per-job Arc<JobState> shared by every PATCH level; flags (never messages) decide the outcome"
    - "Classify once, after the run, in JsExecutor::execute (classify); nested run_script never classifies"
    - "First-wins record through OnceLock::get_or_init with a local newly-set flag; log only when newly set"
    - "One error sink per job (JobState::log_error); per-level script errors suppressed once the job is stopped"

key-files:
  created:
    - .planning/phases/05-js-execution-limits/tdd-evidence/05-01-task1-red.json
    - .planning/phases/05-js-execution-limits/tdd-evidence/05-01-task1-red.junit.xml
    - .planning/phases/05-js-execution-limits/tdd-evidence/05-01-task2-red.json
    - .planning/phases/05-js-execution-limits/tdd-evidence/05-01-task2-red.junit.xml
    - .planning/phases/05-js-execution-limits/tdd-evidence/05-01-task3-red.json
    - .planning/phases/05-js-execution-limits/tdd-evidence/05-01-task3-red.junit.xml
  modified:
    - src/js/mod.rs
    - src/js/ops.rs
    - src/js/runtime.rs
    - src/js/executor.rs
    - src/http/error.rs
    - src/http/mod.rs

key-decisions:
  - "Depth counting: the PATCHed key is level 1; op_rs3_patch refuses when chain.len() >= max_patch_depth (usize compare), and the 422 chain ends with the refused key even when it does not exist"
  - "JobState lives in src/js/mod.rs next to ExecError/JsSettings (not a new job.rs); it replaces ExecCtx's host_fault Arc and owns the test error counter"
  - "classify() in the executor is the only classification point: Err(Internal) passes, host fault -> Internal, depth stop -> DepthLimit, else the result"
  - "max_patch_depth clamp 1..=64 in Limits::from_settings; dedicated test executors use ..JsSettings::default()"
  - "The JS message and the 422 message come from one format: depth_error() builds JsErrorBox::generic from the ExecError::DepthLimit Display"

patterns-established:
  - "Pattern: shared per-job state object (JobState) instead of individual Arc flags on ExecCtx"
  - "Pattern: RED evidence via a JUnit run of the test binary with RUSTC_BOOTSTRAP scoped to that one run, checked by gsd-tools check tdd-red-evidence"

requirements-completed: [ROBU-05, TEST-04]

coverage:
  - id: D1
    description: "Over-deep, self-patching and cyclic PATCH chains answer 422 js_depth_limit with the exact key chain; a chain of exactly 4 levels answers 200"
    requirement: ROBU-05
    verification:
      - kind: integration
        ref: "src/http/mod.rs#depth_limit_finite_chain_is_422"
        status: pass
      - kind: integration
        ref: "src/http/mod.rs#depth_limit_self_patch_is_422"
        status: pass
      - kind: integration
        ref: "src/http/mod.rs#depth_limit_cycle_is_422"
        status: pass
      - kind: integration
        ref: "src/http/mod.rs#depth_limit_allows_exactly_the_max_levels"
        status: pass
      - kind: unit
        ref: "src/js/ops.rs#depth_limit_chain_names_a_missing_key"
        status: pass
    human_judgment: false
  - id: D2
    description: "The depth stop is a hard stop: a caught stop still answers 422, every op refuses afterwards, earlier writes stay, one stop logs one line"
    requirement: ROBU-05
    verification:
      - kind: integration
        ref: "src/http/mod.rs#depth_limit_caught_is_still_422"
        status: pass
      - kind: unit
        ref: "src/js/ops.rs#depth_stop_refuses_later_ops_and_keeps_earlier_writes"
        status: pass
      - kind: unit
        ref: "src/js/ops.rs#every_op_throws_the_depth_error_after_the_stop"
        status: pass
      - kind: unit
        ref: "src/js/ops.rs#depth_stop_is_logged_once"
        status: pass
    human_judgment: false
  - id: D3
    description: "Classification happens once by flag with host fault > depth stop > script result"
    requirement: ROBU-05
    verification:
      - kind: unit
        ref: "src/js/executor.rs#classify_precedence_host_fault_then_depth_then_result"
        status: pass
    human_judgment: false
  - id: D4
    description: "max_patch_depth comes from JsSettings (default 4), clamped to 1..=64; limits 2, 1 and 0 refuse at the right level; the maximum depth with deep leaf recursion is a catchable RangeError"
    requirement: ROBU-05
    verification:
      - kind: unit
        ref: "src/js/executor.rs#max_patch_depth_is_clamped"
        status: pass
      - kind: unit
        ref: "src/js/executor.rs#max_patch_depth_from_settings"
        status: pass
      - kind: unit
        ref: "src/js/executor.rs#default_settings_follow_d06"
        status: pass
      - kind: unit
        ref: "src/js/executor.rs#max_depth_leaf_recursion_is_a_catchable_range_error"
        status: pass
    human_judgment: false
  - id: D5
    description: "Repeated depth stops answer byte-identical 422 bodies and the server keeps serving GET / and normal PATCHes (TEST-04 depth rows)"
    requirement: TEST-04
    verification:
      - kind: integration
        ref: "src/http/mod.rs#depth_limit_is_repeatable_and_server_keeps_serving"
        status: pass
      - kind: other
        ref: "bash scripts/ci-guards.sh && cargo fmt --check && cargo clippy --all-targets --locked -- -D warnings && cargo test --locked && cargo build --release --locked && RS3_SMOKE_PORT=18531 bash scripts/ci-smoke.sh"
        status: pass
    human_judgment: false
  - id: D6
    description: "No test ran an unbounded self-patching or cyclic script before the depth check existed (D-11 safety prohibition)"
    requirement: ROBU-05
    verification: []
    human_judgment: true
    rationale: "Judgment-tier prohibition: the commit order (finite acyclic RED in dc5ff52, self-patch and cycle tests only in GREEN 92c6198) is the evidence, but no automated check can prove an absence of an earlier unsafe run"

# Metrics
duration: 8min
completed: 2026-10-21
status: complete
---

# Phase 5 Plan 01: PATCH Nesting Depth Limit Summary

**A per-job JobState plus a depth check in op_rs3_patch (default 4, clamped 1..=64) turn self-patching, cyclic and over-deep PATCH chains into a 422 `js_depth_limit` naming the exact key chain. It is a hard stop even when caught, it is logged once, and it is classified once in the executor.**

## Performance

- **Duration:** about 8 min
- **Started:** 2026-10-21T00:14:26Z
- **Completed:** 2026-10-21T00:22:30Z
- **Tasks:** 3 (each RED then GREEN)
- **Files modified:** 6 source files, plus 6 evidence files created

## Accomplishments

- `op_rs3_patch` refuses the next level before it reads the store or creates a runtime. It records the refused chain first-wins in the shared `JobState`, logs one line and throws the depth error.
- `JsExecutor::execute` creates one `JobState` per job and classifies the result once through `classify()`: host fault (500) beats depth stop (422 `js_depth_limit`), which beats the script's own result. Nothing matches on message text.
- After a stop, every op (`op_rs3_list`, `op_rs3_get`, `op_rs3_insert`, `op_rs3_delete`, `op_rs3_patch`) throws the same depth error and touches nothing. Writes made before the stop stay.
- The single-log rule covers depth stops: `run_fresh` writes no per-level `js error` line once the job is stopped.
- `max_patch_depth` lives in `JsSettings` (built in code, default 4). `Limits::from_settings` clamps it to 1..=64.

## Task Commits

| Task | RED (test) | GREEN (feat) | RED verdict |
|------|------------|--------------|-------------|
| 1 (tracer): over-deep chain answers 422 end to end; self-patch and cycle stopped | `dc5ff52` | `92c6198` | RED_EVIDENCE_OK (`http::tests#depth_limit_finite_chain_is_422`, 200 vs 422) |
| 2: hard stop, ops refuse, one log line, precedence pinned | `5748b2b` | `b18511c` | RED_EVIDENCE_OK (`js::ops::tests#depth_stop_refuses_later_ops_and_keeps_earlier_writes`, after-write assertion) |
| 3: max_patch_depth from settings, clamped 1..=64 | `f836292` | `992e8f8` | RED_EVIDENCE_OK (`js::executor::tests#max_patch_depth_is_clamped`, left 0 right 1) |

No REFACTOR commits were needed. The tracer feedback gate ran in auto mode: Task 1's verify was re-run end to end, passed, and expansion continued.

## New depth tests

| Test | File | What it pins |
|------|------|--------------|
| `depth_limit_finite_chain_is_422` | src/http/mod.rs | k1..k6 chain answers exactly 422 `{"error":"js_depth_limit","message":"... (4) exceeded: k1 -> k2 -> k3 -> k4 -> k5"}` (RED test, finite and acyclic) |
| `depth_limit_allows_exactly_the_max_levels` | src/http/mod.rs | k1 -> k2 -> k3 -> k4 (k4 = `4`) answers 200 `"4"` (boundary) |
| `depth_limit_self_patch_is_422` | src/http/mod.rs | `me -> me -> me -> me -> me`, then GET / is 200 "Welcome to rs3" and a 6*20 PATCH is 200 "120" |
| `depth_limit_cycle_is_422` | src/http/mod.rs | a <-> b gives `a -> b -> a -> b -> a` |
| `depth_limit_caught_is_still_422` | src/http/mod.rs | a caught stop still answers 422 with `t -> me -> me -> me -> me` |
| `depth_limit_is_repeatable_and_server_keeps_serving` | src/http/mod.rs | two PATCHes give byte-identical 422 bodies, then GET / is 200 and 6*20 is 200 "120" |
| `depth_stop_refuses_later_ops_and_keeps_earlier_writes` | src/js/ops.rs | DepthLimit result; before=1 stays; after is not written; keep survives delete |
| `every_op_throws_the_depth_error_after_the_stop` | src/js/ops.rs | all five ops throw `Error: PATCH nesting depth limit (4) exceeded: a -> b`; the store is unchanged |
| `depth_limit_chain_names_a_missing_key` | src/js/ops.rs | chain `test, k1, k2, k3, ghost`: refused before any store read |
| `depth_stop_is_logged_once` | src/js/ops.rs | uncaught and caught stops each write exactly 1 error line; the job holds the 5-key chain |
| `exec_ctx_nested_increments_depth_and_shares_store` (updated) | src/js/ops.rs | nested("k"): depth 1, chain [test, k], same limit, same job Arc, shared store |
| `classify_precedence_host_fault_then_depth_then_result` | src/js/executor.rs | host fault > depth > result; Internal passes through; no flag leaves the result unchanged |
| `max_patch_depth_is_clamped` | src/js/executor.rs | 0 -> 1, 1 -> 1, 4 -> 4, 64 -> 64, 65 -> 64, u32::MAX -> 64 (pure) |
| `max_patch_depth_from_settings` | src/js/executor.rs | limit 2 refuses `k1, k2, k3`; limits 1 and 0 refuse `t, inner` with limit 1 |
| `max_depth_leaf_recursion_is_a_catchable_range_error` | src/js/executor.rs | 4 levels plus a leaf recursing to V8's stack guard gives Ok("true") on the default 16 MiB stack |
| `default_settings_follow_d06` (extended) | src/js/executor.rs | `max_patch_depth == 4` |

## Observed 422 bodies

- `{"error":"js_depth_limit","message":"PATCH nesting depth limit (4) exceeded: k1 -> k2 -> k3 -> k4 -> k5"}`
- `{"error":"js_depth_limit","message":"PATCH nesting depth limit (4) exceeded: me -> me -> me -> me -> me"}`
- `{"error":"js_depth_limit","message":"PATCH nesting depth limit (4) exceeded: a -> b -> a -> b -> a"}`
- `{"error":"js_depth_limit","message":"PATCH nesting depth limit (4) exceeded: t -> me -> me -> me -> me"}`

## Depth-stop log line

Each stop writes exactly one line to stderr, with the joined chain Debug-formatted so keys cannot forge log lines:

```
error: JS depth limit (4) exceeded: "test -> me -> me -> me -> me"
```

## Suite

- `cargo test --locked`: 185 passed (lib), 0 failed, in 0.57 s at the default thread count
- `cargo test --locked -- --test-threads=1`: 185 passed in 1.96 s
- The full gate passed: `bash scripts/ci-guards.sh` ("all guards passed"), `cargo fmt --check`, `cargo clippy --all-targets --locked -- -D warnings`, `cargo test --locked`, `cargo build --release --locked`, `RS3_SMOKE_PORT=18531 bash scripts/ci-smoke.sh` ("ci-smoke: OK").
- The one "ignored" doctest is the deno_core `extension!` macro doc for `rs3_ext`. It existed before this plan and is not a `#[ignore]` in rs3 code.
- `Cargo.toml` and `Cargo.lock` are unchanged (`git diff --quiet 449aaf2 -- Cargo.toml Cargo.lock ...` exits 0). No `#[allow]`, `#[expect]` or `#[ignore]` was added. No push, and no git config change.

## Files Created/Modified

- `src/js/mod.rs`: `ExecError::DepthLimit`, `JsSettings.max_patch_depth`, `DepthStop`, `JobState` (`note_host_fault`, `host_fault`, `stop_depth`, `depth_stop`, `stopped`, `log_error`, test-only `error_log_count`)
- `src/js/ops.rs`: `ExecCtx { job, chain, max_patch_depth }`, new `new`/`nested(key)` signatures, `depth_error`, the depth check in `op_rs3_patch`, and the `live_ctx` refusal in D-04 order. The test counter moved to the job. Docs updated.
- `src/js/runtime.rs`: both `run_fresh` log guards use `watch.job.stopped()`
- `src/js/executor.rs`: `Limits` (clamp 1..=64), `JsExecutor.limits`, per-job `JobState` in `execute`, `classify`
- `src/http/error.rs`: `ApiError::JsDepthLimit` -> 422 `js_depth_limit`, with no stderr line in `From<ExecError>`
- `src/http/mod.rs`: six HTTP depth tests and a `depth_body` helper
- `.planning/phases/05-js-execution-limits/tdd-evidence/05-01-task{1,2,3}-red.{json,junit.xml}`: RED evidence

## Decisions Made

- I followed resolved research question 1: the PATCHed key counts as level 1, the check is `chain.len() >= max_patch_depth as usize` (no u32 truncation), and the message chain includes the refused key.
- `JobState` sits in `src/js/mod.rs` as the plan flags. It is `pub` because `ExecCtx::new` takes `Arc<JobState>`.
- The nested `run_script` error map got an explicit `ExecError::DepthLimit { .. }` arm that converts to a generic Error without logging or flagging a fault. It cannot be reached today, but it avoids a wildcard.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Doc accuracy] Clamp sentence on `JsSettings.max_patch_depth` added in Task 3, not Task 1**
- **Found during:** Task 1
- **Issue:** The plan asked for the field doc to state the 1..=64 clamp in Task 1, but the clamp only exists from Task 3. Writing it in Task 1 would have documented behaviour the code did not have yet.
- **Fix:** Task 1's doc covers the counting rule and says the value never comes from the environment. Task 3 adds the clamp sentence together with the clamp.
- **Files modified:** src/js/mod.rs
- **Committed in:** 992e8f8

**2. [Rule 3 - Minor] `max_patch_depth_from_settings` dedicated executors use `workers: 1, queue_capacity: 4`**
- **Found during:** Task 3
- **Issue:** The behaviour block shows `..JsSettings::default()`, which spawns one worker per core for each of the three short-lived executors.
- **Fix:** I set workers 1 and queue 4 and kept `..JsSettings::default()` for the remaining fields, so research Pitfall 7/9 still holds. The assertions are unchanged.
- **Files modified:** src/js/executor.rs
- **Committed in:** 992e8f8

---

**Total deviations:** 2 auto-fixed (1 doc accuracy, 1 minor test sizing)
**Impact on plan:** None on behaviour or contracts. Every planned assertion and body is exactly as specified.

## Issues Encountered

None. The Task 2 RED run showed the per-level log amplification (three extra `js error in "me"` lines) that Task 2's single-log change then removed. `depth_stop_is_logged_once` pins this.

## Threat Mitigations

- T-05-01 (DoS by nesting): the check runs before the store read and before runtime creation, with default 4 and a clamp of 1..=64. Covered by the finite, self-patch and cycle tests and by max-depth leaf recursion.
- T-05-02 (writes after a caught stop): `live_ctx` refuses every op, and classify answers 422 even when the stop was caught.
- T-05-03 (disclosure / log forging): the body goes through serde `ErrorBody`, the log line Debug-formats the chain, and one stop writes one line.
- T-05-SC: no crate was added. Cargo.toml and Cargo.lock are byte-identical, and every cargo call used `--locked`.

## User Setup Required

None. No external service configuration is required.

## Next Phase Readiness

- Ready for 05-02 (timeout). `JobState` is the place for the termination group and the `timed_out` phase, and `classify` gets the Timeout arm between host fault and depth. `live_ctx` becomes the three-way gate from research Pattern 3.
- 05-03 still has to rewrite the docs/js-ops.md Limits section (D-12), which this plan does not own.

## Self-Check: PASSED

- All 6 modified source files and all 6 evidence files exist.
- Commits dc5ff52, 92c6198, 5748b2b, b18511c, f836292 and 992e8f8 are ancestors of HEAD. evaluation-scope resolves 6 plan commits.
- The acceptance criteria of all three tasks, the plan gate and the serial run were re-run and passed.

---
*Phase: 05-js-execution-limits*
*Completed: 2026-10-21*
