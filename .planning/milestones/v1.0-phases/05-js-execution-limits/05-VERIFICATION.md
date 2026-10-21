---
phase: 05-js-execution-limits
verified: 2026-10-21T11:53:49Z
status: passed
score: 38/38 must-haves verified
covered_files:
  - ".github/workflows/ci.yml"
  - ".planning/phases/05-js-execution-limits/05-01-PLAN.md"
  - ".planning/phases/05-js-execution-limits/05-01-SUMMARY.md"
  - ".planning/phases/05-js-execution-limits/05-02-PLAN.md"
  - ".planning/phases/05-js-execution-limits/05-02-SUMMARY.md"
  - ".planning/phases/05-js-execution-limits/05-03-PLAN.md"
  - ".planning/phases/05-js-execution-limits/05-03-SUMMARY.md"
  - ".planning/phases/05-js-execution-limits/05-UAT.md"
  - "Cargo.lock"
  - "Cargo.toml"
  - "docs/js-ops.md"
  - "scripts/ci-smoke.sh"
  - "src/config.rs"
  - "src/http/error.rs"
  - "src/http/kv.rs"
  - "src/http/mod.rs"
  - "src/js/executor.rs"
  - "src/js/mod.rs"
  - "src/js/ops.rs"
  - "src/js/runtime.rs"
  - "src/main.rs"
covered_digest: "v3:sha256:37c702abf0aabfcc533d54b6ac616c69f48bc8fa0986a46104a2310fb39ef7ce"
behavior_unverified: 0
overrides_applied: 1
overrides:
  - must_have: "Termination group (05-02, D-06): a runtime created after the job timed out is terminated at registration"
    reason: "Carried from the operator's UAT disposition (05-UAT.md test 2, result skipped, 'Deferred follow-up: accepted risk'). The register-after-timeout branch (src/js/mod.rs:129-136) is present and wired through Registered::new in run_fresh (src/js/runtime.rs:84), and the watchdog's 50 ms re-fire (JobState::fire terminates every registered handle while TIMED_OUT) covers a late-registered isolate anyway; the re-fire is exercised by timeout_throwing_op_loop_is_timeout and timeout_survives_caught_nested_termination. A dedicated test is a deferred follow-up (05-UAT.md Deferred Follow-Ups, test 2)."
    accepted_by: "operator (05-UAT.md test 2)"
    accepted_at: "2026-10-21T09:30:17Z"
re_verification:
  previous_status: passed
  previous_score: 35/38
  reason: "Report went stale: Phase 6 changed covered files Cargo.toml (license = \"MIT\"), docs/js-ops.md (IN-02/IN-03/IN-07 wording, stack-overflow stdout line) and a comment in src/js/executor.rs (IN-05)"
  gaps_closed:
    - "SC-4 CI half: GitHub Actions run 39527188207 at 40afeb7 green on ubuntu-26.04 x86_64 (05-UAT.md test 1); src differs from HEAD only by one comment and Cargo.toml license"
    - "02-9 client-disconnect clause: operator observed on the release binary (05-UAT.md test 3)"
    - "02-2 register-after-timeout branch: accepted risk by the operator (05-UAT.md test 2), recorded as an override"
    - "Four judgment-tier prohibitions: operator accepted all four (05-UAT.md test 4)"
  gaps_remaining: []
  regressions: []
---

# Phase 5: JS Execution Limits Verification Report

**Phase Goal:** As a server operator running rs3, I want to rely on a nesting depth limit and an execution timeout for stored scripts, so that a self-patching or never-ending script can neither crash the server nor starve other requests.
**Verified:** 2026-10-21T11:53:49Z
**Status:** passed
**Re-verification:** Yes. The earlier report (status `passed` after operator UAT) went stale because Phase 6 touched three covered files. This run re-checks every must-have on the final tree, with full scrutiny on the changed files.
**Mode:** mvp. User-story goal format unchanged since the initial verification, where `user-story.validate` returned valid.

## What changed since the last verification

`git diff 2ecb06d HEAD` over all covered implementation files touches only three files:

| File | Change | Effect on Phase 5 |
|------|--------|-------------------|
| `Cargo.toml` | `license = "MIT"` added | None. No dependency or feature change; `Cargo.lock` and `.github/workflows/ci.yml` are still byte-identical to `449aaf2` (`git diff --stat 449aaf2 HEAD` lists only `scripts/ci-smoke.sh`, the Phase 5 smoke extension) |
| `src/js/executor.rs` | Comment at :147-149 only ("main thread that initialised V8 (main calls init_platform), through the worker") | None. The comment is accurate: `src/main.rs:16` calls `rs3::js::init_platform()` before `JsExecutor::start` |
| `docs/js-ops.md` | Op table "Throws when" cells now list the depth-stop and timeout behaviour; TypeError-on-object/symbol-after-timeout note (three places); sibling `op_rs3_patch` calls not counted by depth; PATCHes during a loop answered "while a JS worker is free"; `Deno.core.print` and stack-overflow stdout wording | Checked below. No softening or contradiction of the limits |

### docs/js-ops.md: limits still stated as the contract requires

| Required statement | Where in the final tree | Status |
|--------------------|-------------------------|--------|
| Depth limit 4, counting the PATCHed key | `:95-97`, `:222-224` ("at most 4 scripts, counting the PATCHed key (`max_patch_depth` in `JsSettings`)") | ✓ |
| 422 `js_depth_limit` with the key chain | `:97-99`, `:151-156` (exact body `...exceeded: a -> b -> a -> b -> a`), `:228-230` | ✓ |
| Timeout 5000 ms -> 504 `js_timeout` | `:102-107`, `:157-161` (exact body), `:233-237` | ✓ |
| Hard stops (catching does not help) | Depth: `:98-101`, `:156`, `:230-232`; timeout: `:103` "Nothing is thrown that the script could catch", `:159-161` | ✓ |
| Not runtime-configurable | `:246-247` "Both limits are set in code (`JsSettings`) and cannot be set through the environment" | ✓ |
| No rollback | `:101`, `:108`, `:248-249` "Neither stop rolls anything back. There are no transactions" | ✓ |

New Phase 6 wording, checked against the code:

- "An object or symbol argument still throws its `TypeError`, because arguments are converted before the timeout is checked." True: `StrArg::from_v8` (`src/js/ops.rs:115-135`) rejects objects and symbols during op argument conversion, before the op body calls `live_ctx` (`:177`). This adds a precise exception; it does not weaken the timeout (no write happens, the job still answers 504 by flag in `classify`).
- "Sibling `op_rs3_patch` calls at the same level are not counted; only the timeout bounds them." True: `ExecCtx::nested` appends to a per-level clone of `chain` (`src/js/ops.rs:70-85`), so siblings share their parent's chain length. This matches the depth definition (nesting, not call count) and the ROADMAP SC-1 cases (self-patch and cycles are nesting).
- "Other routes keep being answered, and other PATCH requests too while a JS worker is free (there is one worker per available CPU); otherwise they wait in the queue." True: `JsSettings::default()` sets `workers` from `available_parallelism` (`src/js/mod.rs:245-247`). This is narrower than the Phase 5 wording but matches the code and the existing 1-CPU skip in `scripts/ci-smoke.sh:175-185`. See Anti-Patterns (Info).

## User Flow Coverage

User story: «As a server operator running rs3, I want to rely on a nesting depth limit and an execution timeout for stored scripts, so that a self-patching or never-ending script can neither crash the server nor starve other requests.»

| Step | Expected | Evidence | Status |
|------|----------|----------|--------|
| Operator starts rs3 with defaults | Depth limit 4 and timeout 5000 ms active without configuration | `src/main.rs:16-17` (`init_platform`, then `JsExecutor::start(&JsSettings::default())`); `src/js/mod.rs:250-251`; test `default_settings_follow_d06` (`executor.rs:782-783`) | ✓ |
| A key PATCHes itself or forms an A -> B -> A cycle | 422 JSON naming the limit and chain; server stays up | Depth check `src/js/ops.rs:296` before `store.get` at `:313`; `classify` (`executor.rs:301-317`); `ApiError::JsDepthLimit` -> 422 (`http/error.rs:48,92`); tests `depth_limit_self_patch_is_422`, `depth_limit_cycle_is_422`; release smoke exact-body check (`ci-smoke.sh:160-162`) | ✓ |
| A key holds `while(true){}` and is PATCHed | Terminated after the timeout, 504 JSON | Watchdog (`executor.rs:86-111`), armed at pickup (`:351-356`), `JobState::fire` (`mod.rs:148-157`), `JsTimeout` -> 504 (`error.rs:49,93`); shared gate log: `PATCH /key/loop -> 504 js_timeout after 5006 ms, terminated by the watchdog` | ✓ |
| Meanwhile other requests arrive | Answered while the loop runs | Test `timeout_does_not_block_other_requests`; smoke `GET / during a runaway PATCH -> 200` and `PATCH /key/m during a runaway PATCH` (nproc >= 2), both asserted before the final `ci-smoke: OK` | ✓ |
| After the runaway script | Worker serves the next job; process alive | Gate log: `PATCH /key/m after the timeout -> 200 "120", server alive`; tests `timed_out_worker_serves_next_job`, `late_watchdog_never_hits_next_job` | ✓ |
| Outcome on the x86_64 PKU runner | Same on the protected V8 platform | CI run 39527188207 at 40afeb7 green on ubuntu-26.04 (05-UAT.md test 1); src identical to HEAD except one comment | ✓ |

## Goal Achievement

### Observable Truths

Roadmap success criteria (the contract):

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| SC-1 | Direct or cyclic self-PATCH returns 422 JSON naming the depth limit and key chain; process stays up | ✓ VERIFIED | `depth_limit_self_patch_is_422` (`http/mod.rs:268`, asserts `depth_body("me -> me -> me -> me -> me")`, then GET / and PATCH m), `depth_limit_cycle_is_422`; release smoke exact body; in the 216-test green run on a4a8069 |
| SC-2 | `while(true){}` terminated after the configured timeout, 504; other requests including PATCHes answered | ✓ VERIFIED | `timeout_infinite_loop_is_504_then_worker_serves_next`, `timeout_does_not_block_other_requests`, `timeout_queued_patch_runs_once_the_worker_frees_up`; gate log smoke 504 after 5006 ms by the watchdog |
| SC-3 | Timeout comes from settings; tests set a short timeout in code without env vars | ✓ VERIFIED | `JsSettings.timeout_ms` (`mod.rs:237`), clamped in `Limits::from_settings` (`executor.rs:48`); config allowlist `SERVER_KEYS = ["server.host", "server.port"]` (`config.rs:16,75`); `timeout_exec(timeout_ms)` builds settings in code (`executor.rs:704-708`) |
| SC-4 | Every error path has a passing test (404, 400, 422 insert, 422 JS exception, 422 depth, 504), locally and in CI | ✓ VERIFIED | All 13 named tests exist on the final tree (grep: `get_missing_is_404_json`, `delete_missing_is_404_json`, `test_patch_missing_is_404_json`, `insert_malformed_is_400_json`, `insert_non_object_is_422_json`, `insert_non_string_value_is_422_and_inserts_nothing`, `test_patch_throw_is_422_js_error`, `depth_limit_self_patch_is_422`, `depth_limit_cycle_is_422`, `depth_limit_caught_is_still_422`, `timeout_infinite_loop_is_504_then_worker_serves_next`, `timeout_nested_loop_is_504`, `timed_out_worker_serves_next_job`). Locally: `cargo test --locked` 216 passed, 0 failed on a4a8069 (shared gate log). CI: run 39527188207 at 40afeb7, 216 tests, green (05-UAT.md test 1); `ci.yml:46-47,59` unchanged |

Plan 05-01 (depth limit):

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 01-1 | Tracer: k1..k6 chain answers 422 with exact body `k1 -> k2 -> k3 -> k4 -> k5` | ✓ VERIFIED | `depth_limit_finite_chain_is_422`; in green suite |
| 01-3 | Check before store read and runtime creation; refuses when `chain.len() >= max`; chain names a missing refused key | ✓ VERIFIED | `ops.rs:296` before `:313`; `depth_limit_chain_names_a_missing_key` |
| 01-4 | Hard stop: caught stop still 422; every op throws the depth error; earlier writes stay | ✓ VERIFIED | `depth_limit_caught_is_still_422`, `every_op_throws_the_depth_error_after_the_stop`, `depth_stop_refuses_later_ops_and_keeps_earlier_writes` |
| 01-5 | Classification once, by flag: host fault > depth > result | ✓ VERIFIED | `classify` (`executor.rs:301-317`) reads only `job.host_fault()` / `timed_out` / `job.depth_stop()`; `classify_precedence_host_fault_then_depth_then_result` |
| 01-6 | One log line per depth stop | ✓ VERIFIED | `newly_set` gate (`ops.rs:303-310`); `depth_stop_is_logged_once` |
| 01-7 | `max_patch_depth` in code, default 4, clamped 1..=64 | ✓ VERIFIED | `mod.rs:250`, `executor.rs:47`; `max_patch_depth_is_clamped` |
| 01-8 | Boundary: exactly max levels gives 200 "4"; max 2 refuses with 3 keys | ✓ VERIFIED | `depth_limit_allows_exactly_the_max_levels`, `max_patch_depth_from_settings` |
| 01-9 | Precision: 0 -> 1, 64 -> 64, 65/u32::MAX -> 64; usize comparison | ✓ VERIFIED | `max_patch_depth_is_clamped`; `ops.rs:296` casts to usize |
| 01-10 | Chain in call order joined by " -> " | ✓ VERIFIED | `ExecError::DepthLimit` Display (`mod.rs:33-37`); exact-body tests |
| 01-11 | Max 1 (or 0) refuses any nested PATCH, chain `[root, refused]` | ✓ VERIFIED | `max_patch_depth_from_settings` (`executor.rs:810-837`) |
| 01-12 | Byte-identical 422s, then GET / 200 and 6*20 -> 200 "120" | ✓ VERIFIED | `depth_limit_is_repeatable_and_server_keeps_serving` |
| 01-13 | D-11: no self-patching script before the depth check | ✓ VERIFIED | Historical git order unchanged: `dc5ff52` (finite chain) -> `92c6198` (check) -> `5748b2b` (first self-patch); operator accepted (05-UAT.md test 4) |
| 01-14 | D-14 plan gate | ✓ VERIFIED | Shared gate log on a4a8069: guards, check-workflow, test-ci-guards (99 cases, 0 missed), fmt, clippy, test, release build, smoke all OK |

Plan 05-02 (timeout):

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 02-1 | Tracer: 300 ms timeout gives exact 504 in [300, 1300) ms, then 1+1 -> 200 "2" | ✓ VERIFIED | `timeout_infinite_loop_is_504_then_worker_serves_next` |
| 02-2 | Termination group: every runtime registers its IsolateHandle and unregisters on drop; a runtime created after the timeout is terminated at registration | ✓ PASSED (override) | Registration of nested isolates is exercised (`timeout_nested_loop_is_504` passes only if the nested isolate is in the group). The register-after-timeout branch (`mod.rs:129-136`) has no test. Override: accepted risk by the operator (05-UAT.md test 2), with the 50 ms re-fire as the covering path |
| 02-3 | Per-worker watchdog `rs3-js-N-wd`, armed at pickup, CAS Running -> TimedOut, re-fires every 50 ms | ✓ VERIFIED | `executor.rs:142-151` (spawned from the worker), `:351-356` (arm at pickup), `REFIRE` (`:75`); `job_state_phase_changes_exactly_once`; re-fire exercised by `timeout_throwing_op_loop_is_timeout`, `timeout_survives_caught_nested_termination` |
| 02-4 | `finish()` CAS; precedence host fault > timeout > depth > result | ✓ VERIFIED | `classify_precedence_with_timeout`, `timeout_beats_depth_limit`, `host_fault_beats_timeout`, `host_fault_during_the_timeout_beats_it_and_is_logged` |
| 02-5 | Termination cannot be dodged; no write after the stop | ✓ VERIFIED | `timeout_survives_caught_nested_termination`, `timeout_throwing_op_loop_is_timeout`, `timeout_during_stringify_is_timeout`, `timeout_refuses_writes_after_stop`, `timeout_nested_loop_is_timeout` |
| 02-6 | Backstop at timeout + 2 s from pickup; worker serves next job | ✓ VERIFIED | `run_job` awaits `started_rx` (`executor.rs:231`) before `tokio::time::timeout` (`:232-245`); `backstop_answers_timeout_when_the_worker_never_replies` |
| 02-9 | Concurrency: worker held only until the timeout; other routes answered; client disconnect does not stop termination; late watchdog never hits job N+1 | ✓ VERIFIED | Clauses 1, 2, 4 tested (`timeout_does_not_block_other_requests`, `late_watchdog_never_hits_next_job`). Client-disconnect clause: code arms the watchdog independent of the caller and ignores failed `started`/`reply` sends (`executor.rs:351-375`), and the operator observed it on the release binary (05-UAT.md test 3: `curl --max-time 1` exit 28, then PATCH m 200 "120" in 13 ms) |
| 02-10 | Fast script 200; past the limit 504 within about 1 s; exactly one outcome at the deadline | ✓ VERIFIED | `timeout_bounded_busy_loop_is_504`, `late_watchdog_never_hits_next_job` |
| 02-11 | CAS lets exactly one win; never 200 with post-stop writes; never 504 for the next job | ✓ VERIFIED | `job_state_phase_changes_exactly_once`; canaries in `late_watchdog_never_hits_next_job` |
| 02-12 | Timeout 0 -> 1; message echoes the effective value; monotonic Instant | ✓ VERIFIED | `timeout_ms_is_clamped`; `Instant` deadline (`executor.rs:353`) |
| 02-13 | tokio `time`/`macros` only; Cargo.lock identical to 449aaf2; gate passes | ✓ VERIFIED | `git diff --stat 449aaf2 HEAD -- Cargo.lock .github/workflows/ci.yml` empty; the only later Cargo.toml change is `license` |

Plan 05-03 (release and hardening):

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 03-1 | Release smoke asserts 422 depth, GET/PATCH during the loop, 504 at >= 4900 ms, then 200 "120" | ✓ VERIFIED | `ci-smoke.sh:160-212` under `set -euo pipefail` with `fail` exiting 1 (`:39,90-95`), so `ci-smoke: OK` in the gate log implies every earlier check passed (the log keeps only the tail lines: 504 after 5006 ms by the watchdog, server alive, OK) |
| 03-2 | Smoke uses inline `env -i`, binds 127.0.0.1, EXIT trap; ci.yml unchanged; guard (g) ok | ✓ VERIFIED | ci.yml unchanged since 449aaf2; gate log `[g] ... ok` |
| 03-3 | IN-04: op_print retry loop | ✓ VERIFIED | `write_all_retrying` (`ops.rs:389`); print tests in green suite |
| 03-4 | IN-06: worker_loop doc about panics in op callbacks | ✓ VERIFIED | `executor.rs:319-325` |
| 03-5 | IN-07: 2 MiB stack floor; stack 0 runs the max-depth chain | ✓ VERIFIED | `MIN_STACK_BYTES` (`executor.rs:56`); `stack_bytes_are_clamped`, `minimum_stack_runs_the_max_depth_chain` |
| 03-6 | IN-05: .txt payload parity | ✓ VERIFIED | `txt_payload_*` tests in green suite |
| 03-7 | D-12: docs describe both limits, answers, in-code settings, kept writes, heap not covered | ✓ VERIFIED | Re-checked after Phase 6 edits: `docs/js-ops.md:95-108,151-168,222-253`; table above |
| 03-9 | Suite passes at default threads, serially, 5x, under `env -i`, in < 60 s | ✓ VERIFIED | Default-thread run 216 passed in 4.16 s (gate log). Serial/5x/env -i runs recorded in 05-03-SUMMARY and VALIDATION row 05-03-03 at the Phase 5 HEAD; test code unchanged since (only a comment in executor.rs) |
| 03-10 | D-14 phase gate; Cargo.lock identical; ci.yml unchanged; no allow/expect/ignore | ✓ VERIFIED | Gate log; `grep -rnE '#\[(allow\|expect\|ignore)' src` empty; guards [a]/[b] ok |
| 03-11 | 05-VALIDATION.md nine rows green, `wave_0_complete: true` | ✓ VERIFIED | Unchanged since the initial verification |

**Score:** 38/38 truths verified (37 VERIFIED, 1 PASSED (override); 0 present but behavior-unverified).

### Prohibitions

| Prohibition | Tier | Disposition | Evidence |
|-------------|------|-------------|----------|
| 05-01 ROBU-05: no unbounded self-patch or cycle before the depth check (D-11) | judgment | Resolved by human: PASS (05-UAT.md test 4) | Commit order unchanged |
| 05-01 TEST-04: depth tests not weakened | test | VERIFIED | Guards [a]/[b] ok; depth tests compare full JSON bodies |
| 05-02 ROBU-06: no 504 while the script can still write | test | VERIFIED | `timeout_refuses_writes_after_stop` |
| 05-02 ROBU-06: no cross-job timeout or misclassification | test | VERIFIED | `late_watchdog_never_hits_next_job`, `timeout_queued_patch_runs_once_the_worker_frees_up`; Disarm before next job (`executor.rs:364-367`) |
| 05-02 TEST-04: no un-armed infinite loop | judgment | Resolved by human: PASS (05-UAT.md test 4) | Unchanged test code |
| 05-03 ROBU-05: docs never imply rollback or env-set limits | judgment | Resolved by human: PASS (05-UAT.md test 4); re-checked on the Phase 6 docs: still holds | `docs/js-ops.md:246-249` |
| 05-03 ROBU-06: remote CI and PKU watchdog not claimed as verified | judgment | Resolved by human: PASS (05-UAT.md test 4); remote CI now actually observed (run 39527188207) | 05-UAT.md test 1 |
| 05-03 TEST-04: no push or git config change; smoke not weakened | test | VERIFIED | `ci-smoke.sh` unchanged since Phase 5 completion; exact code/body comparisons |

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `src/js/mod.rs` | ExecError::DepthLimit/Timeout, JsSettings limits, JobState, termination group | ✓ VERIFIED | Unchanged since Phase 5; used by ops/runtime/executor |
| `src/js/ops.rs` | Depth check, live_ctx, silent ops after timeout, print retry | ✓ VERIFIED | Unchanged since Phase 5 |
| `src/js/runtime.rs` | Isolate registration, single-log suppression | ✓ VERIFIED | `Registered::new(.., thread_safe_handle())` at :84 |
| `src/js/executor.rs` | Limits, watchdog, classify, BACKSTOP_GRACE, MIN_STACK_BYTES | ✓ VERIFIED | Phase 6 diff is a comment only |
| `src/http/error.rs` | js_depth_limit 422, js_timeout 504 | ✓ VERIFIED | `:48-49, :78-79, :92-93` |
| `src/http/mod.rs`, `src/http/kv.rs` | HTTP depth/timeout/error-path tests | ✓ VERIFIED | All 13 TEST-04 tests present |
| `scripts/ci-smoke.sh` | Release depth and timeout smoke | ✓ VERIFIED | Green in shared gate |
| `docs/js-ops.md` | Limits documentation | ✓ VERIFIED | Phase 6 edits checked, no softening |
| `Cargo.toml` | tokio `time`/`macros` | ✓ VERIFIED | Only `license` added later |

### Key Link Verification

| From | To | Via | Status | Details |
|------|----|-----|--------|---------|
| `op_rs3_patch` | `JobState::stop_depth` | First-wins record before the throw | WIRED | `ops.rs:299-302` |
| `worker_loop` | `classify` | `job.finish()` then classify | WIRED | `executor.rs:364,375` |
| `From<ExecError>` | HTTP 422/504 | `JsDepthLimit`/`JsTimeout` | WIRED | `error.rs:48-49,92-93` |
| `run_fresh` | `JobState::register` | `Registered::new` | WIRED | `runtime.rs:84` |
| `watchdog_loop` | `JobState::fire` | Deadline and re-fire | WIRED | `executor.rs:100-108` |
| `run_job` | `ExecError::Timeout` | `tokio::time::timeout(timeout + BACKSTOP_GRACE)` | WIRED | `executor.rs:231-245` |
| `main.rs` | `JsExecutor` with limits | `JsSettings::default()` | WIRED | `main.rs:17` |
| `ci.yml` | `cargo test`, `ci-smoke.sh` | run steps | WIRED | `ci.yml:46-47,59` |
| `docs/js-ops.md` | `JsSettings` defaults | 4 / 5000 | WIRED | Matches `mod.rs:250-251`; Readme.md:173-174 agrees |

### Data-Flow Trace (Level 4)

| Artifact | Data Variable | Source | Produces Real Data | Status |
|----------|---------------|--------|--------------------|--------|
| 422 depth body | `DepthStop.chain` | `ExecCtx.chain` grown by `nested(key)` per real PATCH level | Yes (smoke exact body `me -> me -> me -> me -> me`) | ✓ FLOWING |
| 504 timeout body | `limit_ms` | `Limits.timeout_ms` from `JsSettings` | Yes (5000 in smoke, 300/250/100 in tests) | ✓ FLOWING |

### Behavioral Spot-Checks

Per the orchestrator, cargo and the smoke were not re-run (parallel verifiers would contend). Evidence is the shared gate on a4a8069, whose source equals the final tree.

| Behavior | Command | Result | Status |
|----------|---------|--------|--------|
| Guards | `bash scripts/ci-guards.sh` | all 8 guards ok | ✓ PASS (shared log) |
| Workflow check | `python3 scripts/check-workflow.py` | OK | ✓ PASS (shared log) |
| Guard self-test | `bash scripts/test-ci-guards.sh` | 99 cases, 0 missed | ✓ PASS (shared log) |
| Format / lint | `cargo fmt --check`, `cargo clippy --all-targets --locked -- -D warnings` | OK | ✓ PASS (shared log) |
| Full suite | `cargo test --locked` | 216 passed, 0 failed, 4.16 s | ✓ PASS (shared log) |
| Release build and smoke | `cargo build --release --locked`; `bash scripts/ci-smoke.sh` | OK; 504 after 5006 ms by the watchdog; server alive | ✓ PASS (shared log) |
| Remote CI | GitHub Actions run 39527188207 at 40afeb7 | green, 216 tests, 0 annotations | ✓ PASS (operator, 05-UAT.md test 1) |
| Tree diff since CI run | `git diff --stat 40afeb7 HEAD -- src Cargo.toml Cargo.lock scripts .github` | Cargo.toml +1 (license), executor.rs comment | ✓ PASS (read-only, this run) |
| TEST-04 tests exist | grep `fn <name>` in `src/` for all 13 | 13 found | ✓ PASS (read-only, this run) |

### Probe Execution

Step 7c: no `scripts/*/tests/probe-*.sh` exist, and no plan declares a probe. Not applicable.

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|-------------|-------------|--------|----------|
| ROBU-05 | 05-01, 05-03 | Self- or cyclic-PATCH stopped by a depth limit with 422, no crash | ✓ SATISFIED | SC-1, 01-* truths, smoke, CI |
| ROBU-06 | 05-02, 05-03 | `while(true){}` terminated after a configurable timeout with 504; other requests served | ✓ SATISFIED | SC-2, SC-3, 02-* truths, smoke, CI on x86_64 PKU |
| TEST-04 | 05-01, 05-02, 05-03 | Each error path has a test | ✓ SATISFIED | SC-4: 13 named tests present and green locally and in CI |

No orphaned requirements: REQUIREMENTS.md maps exactly ROBU-05, ROBU-06 and TEST-04 to Phase 5 (lines 161, 162, 172), and all three are claimed by plans.

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| (all covered files) | - | TBD/FIXME/XXX/TODO/HACK/PLACEHOLDER | - | None found |
| `src/` | - | `#[allow]`/`#[expect]`/`#[ignore]` | - | None found |
| `docs/js-ops.md` | 239-242 | Phase 6 narrowed "other PATCH requests keep being answered" to "while a JS worker is free ... otherwise they wait in the queue" | ℹ️ Info | Accurate to the code (one worker per CPU). On a 1-CPU host a PATCH queues up to the 5 s timeout behind a runaway script: answered, not starved indefinitely. SC-2 is met on the CI runner and the dev host; the smoke already skips that check on 1 CPU (`ci-smoke.sh:175-185`) |
| `src/js/executor.rs` | 236-240 | Backstop logs with `eprintln!` rather than the job sink | ℹ️ Info | Unchanged; review IN-01, deferred to v2 OPS-01. No effect on answers |
| tests | - | Tight timing upper bounds | ℹ️ Info | Unchanged; green on the local host and the CI runner |

### Human Verification Required

None outstanding. All four items from the initial verification were resolved by the operator in 05-UAT.md (status complete): test 1 PASS (CI run 39527188207), test 2 accepted risk (recorded as the 02-2 override, deferred follow-up), test 3 PASS (client disconnect on the release binary), test 4 PASS (judgment-tier prohibitions). Phase 6's later changes to the covered files are a comment, a license field and documentation wording, none of which alters behavior the UAT observed.

### Gaps Summary

No gaps. The phase goal holds on the final tree. A self-patching or cyclic script answers 422 `js_depth_limit` with the exact key chain, and the server keeps serving. A `while(true){}` script is terminated by the per-worker watchdog and answers 504 `js_timeout` at about 5 s, while other requests are answered. Both limits come from `JsSettings` built in code. Every TEST-04 error path has a named, passing test, locally and in CI on x86_64.

Phase 6's changes do not regress Phase 5. `src/js/executor.rs` changed only in a comment, `Cargo.toml` only gained `license`, and the `docs/js-ops.md` edits still state the contract: depth 4 -> 422 `js_depth_limit` with the chain, 5000 ms -> 504 `js_timeout`, hard stops, settable only in code, no rollback. The new statements (TypeError on object/symbol arguments after a timeout, siblings not counted, PATCHes queue when no worker is free) are accurate to the code.

One must-have passes by override: the register-after-timeout branch of the termination group. The operator accepted it as a risk with a deferred test, and the watchdog's tested 50 ms re-fire covers it.

---

_Verified: 2026-10-21T11:53:49Z_
_Verifier: Claude (gsd-verifier)_
