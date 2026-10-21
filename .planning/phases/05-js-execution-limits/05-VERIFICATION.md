---
phase: 05-js-execution-limits
verified: 2026-10-21T01:20:52Z
status: human_needed
score: 35/38 must-haves verified
covered_files:
  - ".github/workflows/ci.yml"
  - ".planning/phases/05-js-execution-limits/05-01-PLAN.md"
  - ".planning/phases/05-js-execution-limits/05-01-SUMMARY.md"
  - ".planning/phases/05-js-execution-limits/05-02-PLAN.md"
  - ".planning/phases/05-js-execution-limits/05-02-SUMMARY.md"
  - ".planning/phases/05-js-execution-limits/05-03-PLAN.md"
  - ".planning/phases/05-js-execution-limits/05-03-SUMMARY.md"
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
covered_digest: "v3:sha256:ffeb771133019b9d4fb43befc46502dee71c412347bc1c43858e83ff4741bb6f"
behavior_unverified: 2
overrides_applied: 0
behavior_unverified_items:
  - truth: "Termination group (05-02, D-06): a runtime created after the job timed out is terminated at registration"
    test: "Force a nested JsRuntime::try_new to complete after the watchdog has fired (e.g. a test hook that delays runtime creation until job.timed_out()), then run a script in it"
    expected: "JobState::register sees TIMED_OUT under the registry lock and calls terminate_execution, so the new runtime's first script fails and the job answers Timeout within one REFIRE (50 ms)"
    why_human: "The branch at src/js/mod.rs:132-134 is present and wired (Registered::new in run_fresh), but no test reaches it: after a timeout live_ctx stops op_rs3_patch before any runtime is created, so only a race inside try_new hits it. The 50 ms re-fire is a second line of defence, but neither path is exercised for a late-registered isolate"
  - truth: "Edge ROBU-06/concurrency (05-02): a client disconnect does not stop termination, because the watchdog lives on the worker side"
    test: "Start the release binary, insert loop=while(true){} and m=6*20, send PATCH /key/loop with curl --max-time 1 (the client disconnects), wait about 6 s, then PATCH /key/m"
    expected: "The server log holds 'error: js timeout in \"loop\" after 5000 ms', no 'JS worker did not reply' line, and PATCH /key/m answers 200 \"120\""
    why_human: "worker_loop (src/js/executor.rs:350-356) arms the watchdog without consulting the caller and ignores a failed started/reply send, so the code supports the claim, but no test drops the request future after pickup"
human_verification:
  - test: "Push modernize-toolchain at this HEAD and confirm the GitHub Actions CI job is green on the ubuntu-26.04 x86_64 runner"
    expected: "cargo test --locked passes, including the 13 named TEST-04 tests. The Release smoke prints 'PATCH /key/me -> 422 js_depth_limit', 'GET / during a runaway PATCH -> 200' and 'PATCH /key/loop -> 504 js_timeout after N ms, terminated by the watchdog' on the protected V8 platform (PKU). No 'server died (status 139)'"
    why_human: "ROADMAP SC-4 requires the error-path tests to pass in CI too. The last observed remote run (39507141932, fd8f43f) is Phase 4's state, and this run never pushes, so the CI run of the Phase 5 HEAD and the watchdog under x86 PKU (assumption A1) have not been observed"
  - test: "Termination group: a runtime created after the timeout is terminated at registration (see behavior_unverified_items[0])"
    expected: "A late-registered nested isolate is terminated at once and the job answers 504"
    why_human: "Present and wired; no test exercises the register-after-timeout branch"
  - test: "Client disconnect during a runaway PATCH (see behavior_unverified_items[1])"
    expected: "The watchdog still terminates the script and the worker serves the next job"
    why_human: "Present and wired; no test drops the request after pickup"
  - test: "Review the four judgment-tier prohibitions (non-authoritative verifier verdicts below)"
    expected: "Agree that each holds: (1) 05-01 ROBU-05 no unbounded self-patch before the depth check; (2) 05-02 TEST-04 no un-armed infinite loop; (3) 05-03 ROBU-05 docs never imply rollback or env-configurable limits; (4) 05-03 ROBU-06 no claim that remote CI or the PKU watchdog was verified"
    why_human: "Judgment-tier prohibitions are never silently passed in autonomous mode; each is flagged 'unverified-prohibition, human review recommended'"
---

# Phase 5: JS Execution Limits Verification Report

**Phase Goal:** As a server operator running rs3, I want to rely on a nesting depth limit and an execution timeout for stored scripts, so that a self-patching or never-ending script can neither crash the server nor starve other requests.
**Verified:** 2026-10-21T01:20:52Z
**Status:** human_needed
**Re-verification:** No. This is the initial verification.
**Mode:** mvp. The user-story format passed `user-story.validate` (valid: true).

## User Flow Coverage

User story: «As a server operator running rs3, I want to rely on a nesting depth limit and an execution timeout for stored scripts, so that a self-patching or never-ending script can neither crash the server nor starve other requests.»

| Step | Expected | Evidence | Status |
|------|----------|----------|--------|
| Operator starts rs3 with defaults | Depth limit 4 and timeout 5000 ms are active without any configuration | `src/main.rs:17` starts `JsExecutor` with `JsSettings::default()`; `src/js/mod.rs:244-253` sets `max_patch_depth: 4` and `timeout_ms: 5000`; `default_settings_follow_d06` passes | ✓ |
| A user stores a key that PATCHes itself (or an A -> B -> A cycle) and PATCHes it | 422 JSON naming the limit and the key chain; the server stays up | `op_rs3_patch` depth check before the store read (`src/js/ops.rs:296-312`); `classify` (`src/js/executor.rs:300-315`); `ApiError::JsDepthLimit` -> 422 (`src/http/error.rs:48,92`). The release smoke observed `PATCH /key/me -> 422 js_depth_limit` with the exact body, then the server served `PATCH /key/m` and `GET /` | ✓ |
| A user stores `while(true){}` and PATCHes it | Terminated after the timeout, 504 JSON | Per-worker watchdog (`src/js/executor.rs:86-112, 350-366`); termination group (`src/js/runtime.rs:84`, `src/js/mod.rs:129-158`). The release smoke observed `504 js_timeout after 5009 ms, terminated by the watchdog` (worker log line present, no backstop line) | ✓ |
| Meanwhile other users call GET / and PATCH other keys | Answered while the loop runs | Smoke: `GET / during a runaway PATCH -> 200`, `PATCH /key/m during a runaway PATCH -> 200 "120"` before the loop answered (nproc 4); test `timeout_does_not_block_other_requests` passes | ✓ |
| After the runaway script, the server keeps working | The timed-out worker serves the next job; the process is alive | Smoke: `PATCH /key/m after the timeout -> 200 "120", server alive`; tests `timed_out_worker_serves_next_job` and `late_watchdog_never_hits_next_job` pass | ✓ |
| Outcome: neither crash nor starvation | The process survives both failure modes, and other requests are served | All of the above, observed on the release binary (protected V8 platform, aarch64). Not covered on purpose: heap exhaustion (T-05-10, documented in `docs/js-ops.md:233-236`). The x86_64 PKU run is pending (human item 1) | ✓ (local) |

## Goal Achievement

### Observable Truths

Roadmap success criteria (the contract):

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| SC-1 | Direct or cyclic self-PATCH returns 422 JSON naming the depth limit and key chain; the process stays up | ✓ VERIFIED | `depth_limit_self_patch_is_422` (body `me -> me -> me -> me -> me`, then GET / 200 and PATCH m 200), `depth_limit_cycle_is_422` (`a -> b -> a -> b -> a`); release smoke exact body; all pass |
| SC-2 | `while(true){}` is terminated after the configured timeout, PATCH returns 504, and other requests including PATCHes keep being answered | ✓ VERIFIED | `timeout_infinite_loop_is_504_then_worker_serves_next` (300 ms, 504 exact bytes, 300 to 1300 ms window), `timeout_does_not_block_other_requests` (GET / and PATCH m answered before the loop's 504), `timeout_queued_patch_runs_once_the_worker_frees_up`; release smoke |
| SC-3 | The timeout comes from settings; tests set a short timeout in code without environment variables | ✓ VERIFIED | `JsSettings.timeout_ms` (`src/js/mod.rs:237`), clamped in `Limits::from_settings`; the config allowlist is only `server.host`/`server.port` (`src/config.rs:16`); tests build `JsSettings { timeout_ms: 300/500/1000/100, .. }` in code |
| SC-4 | Every error path has a passing test (404, 400, 422 insert, 422 JS exception, 422 depth limit, 504 timeout), locally and in CI | ? UNCERTAIN | Locally VERIFIED: the 13 named tests pass serially under `env -i` (13 passed), and the full suite passes 216/216. CI wiring VERIFIED: `.github/workflows/ci.yml:46-47` runs `cargo test --locked` and `:59` the smoke, and ci.yml is unchanged since 449aaf2. The remote run of this HEAD has not been observed, so this routes to human item 1 |

Plan 05-01 (depth limit), excluding T2 (folded into SC-1):

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 01-1 | Tracer: k1..k6 chain answers 422 with exact body `k1 -> k2 -> k3 -> k4 -> k5` | ✓ VERIFIED | `depth_limit_finite_chain_is_422` compares the full JSON body; passes |
| 01-3 | Check runs before the store read and before runtime creation; refuses when `chain.len() >= max`; the chain names a missing refused key | ✓ VERIFIED | `src/js/ops.rs:296` comes before `store.get` at `:313`; `depth_limit_chain_names_a_missing_key` passes |
| 01-4 | Hard stop: a caught stop still answers 422; every op throws the depth error and touches nothing; earlier writes stay | ✓ VERIFIED | `depth_limit_caught_is_still_422`, `every_op_throws_the_depth_error_after_the_stop`, `depth_stop_refuses_later_ops_and_keeps_earlier_writes` pass |
| 01-5 | Classification once, by flag: host fault > depth > result; no message matching | ✓ VERIFIED | `classify` uses `job.host_fault()`/`depth_stop()` only; `classify_precedence_host_fault_then_depth_then_result` passes |
| 01-6 | Single-log rule: one line per depth stop, caught or uncaught | ✓ VERIFIED | `newly_set` gate (`ops.rs:303`); `run_fresh` suppresses per-level lines once stopped (`runtime.rs:88,99`); `depth_stop_is_logged_once` asserts count == 1 for both forms |
| 01-7 | D-01: `max_patch_depth` set in code, default 4, clamped to 1..=64 | ✓ VERIFIED | `mod.rs:250`, `executor.rs:47`; `max_patch_depth_is_clamped` passes |
| 01-8 | Boundary: exactly max levels gives 200 "4"; with max 2 the refused chain has 3 keys | ✓ VERIFIED | `depth_limit_allows_exactly_the_max_levels`, `max_patch_depth_from_settings` (`k1,k2,k3`) pass |
| 01-9 | Precision: 0 becomes 1, 64 stays 64, 65 and u32::MAX become 64; usize comparison | ✓ VERIFIED | `max_patch_depth_is_clamped` table; `ops.rs:296` casts to usize |
| 01-10 | Ordering: chain in call order joined by " -> " | ✓ VERIFIED | `ExecError::DepthLimit` Display (`mod.rs:36`); exact-body tests |
| 01-11 | Empty: max 1 (or 0) refuses any nested PATCH, chain `[root, refused]` | ✓ VERIFIED | `max_patch_depth_from_settings` loop over `[1, 0]` asserts `["t","inner"]` |
| 01-12 | Idempotency: byte-identical 422s, then GET / 200 and 6*20 gives 200 "120" | ✓ VERIFIED | `depth_limit_is_repeatable_and_server_keeps_serving` |
| 01-13 | D-11: no self-patching or cyclic script before the depth check | ✓ VERIFIED | Git order: RED `dc5ff52` uses the finite k1..k6 chain only; the depth check landed in `92c6198`; the first `op_rs3_patch('me')` arrives in `5748b2b` |
| 01-14 | D-14 plan gate | ✓ VERIFIED | Re-run at HEAD: guards ok, fmt ok, clippy `-D warnings` ok, tests 216 passed, release build ok, smoke OK; `git diff 449aaf2 -- Cargo.lock .github/workflows/ci.yml` is empty; guard [a]/[b] ok |

Plan 05-02 (timeout), excluding the isolation truth (folded into SC-2) and D-05 (folded into SC-3):

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 02-1 | Tracer: 300 ms timeout gives exact 504 body in [300, 1300) ms, then 1+1 gives 200 "2" | ✓ VERIFIED | `timeout_infinite_loop_is_504_then_worker_serves_next` |
| 02-2 | Termination group: every runtime registers its IsolateHandle and unregisters on drop; a runtime created after the timeout is terminated at registration | ⚠️ PRESENT_BEHAVIOR_UNVERIFIED | Registration of nested isolates is exercised: `timeout_nested_loop_is_504` can only pass if the nested isolate is in the group. The register-after-timeout branch (`mod.rs:132-134`) has no test (see behavior_unverified_items) |
| 02-3 | Per-worker watchdog `rs3-js-N-wd`, armed at pickup, CAS Running -> TimedOut, re-fires every 50 ms | ✓ VERIFIED | `executor.rs:143-152` (spawned in the worker closure), `:352-353` (arm at pickup), `REFIRE` 50 ms; `job_state_phase_changes_exactly_once`; re-fire is needed for and exercised by `timeout_throwing_op_loop_is_timeout` and `timeout_survives_caught_nested_termination` |
| 02-4 | Classification: `finish()` CAS; precedence host fault > timeout > depth > result | ✓ VERIFIED | `classify_precedence_with_timeout`, `timeout_beats_depth_limit`, `host_fault_beats_timeout`, `host_fault_during_the_timeout_beats_it_and_is_logged` |
| 02-5 | Termination cannot be dodged: silent ops, caught nested termination, throwing-op loop, looping toString all end as Timeout; no write after the stop | ✓ VERIFIED | `timeout_survives_caught_nested_termination`, `timeout_throwing_op_loop_is_timeout`, `timeout_during_stringify_is_timeout`, `timeout_refuses_writes_after_stop`, `timeout_nested_loop_is_timeout` |
| 02-6 | Backstop at timeout + 2 s from pickup, answers Timeout; the worker serves the next job | ✓ VERIFIED | `run_job` awaits `started_rx` before `tokio::time::timeout` (`executor.rs:230-232`); `backstop_answers_timeout_when_the_worker_never_replies` (2100 to 2400 ms window, then "next") |
| 02-9 | Concurrency: one worker held only until the timeout; other routes answered; client disconnect does not stop termination; a late watchdog never hits job N+1 | ⚠️ PRESENT_BEHAVIOR_UNVERIFIED | Clauses 1, 2 and 4 are tested (`timeout_does_not_block_other_requests`, `late_watchdog_never_hits_next_job`). The client-disconnect clause is supported by the code (`executor.rs:350-356`) but no test drops the request after pickup |
| 02-10 | Boundary: fast script gives 200; past the limit gives 504 within about 1 s; at the deadline exactly one outcome | ✓ VERIFIED | `timeout_bounded_busy_loop_is_504`, `late_watchdog_never_hits_next_job` (235 to 265 ms around 250) |
| 02-11 | Adjacency: CAS lets exactly one win; never 200 with post-stop writes; never 504 for the next job | ✓ VERIFIED | `job_state_phase_changes_exactly_once`; canaries in `late_watchdog_never_hits_next_job` always `Ok("2")` |
| 02-12 | Precision: timeout 0 becomes 1; message echoes the effective value; monotonic Instant | ✓ VERIFIED | `timeout_ms_is_clamped`; `timeout_body(300)` exact; `Instant` in `executor.rs:352` |
| 02-13 | D-14: tokio `time`/`macros` only; Cargo.lock byte-identical to 449aaf2; gate passes | ✓ VERIFIED | Cargo.toml diff shows only those two features; Cargo.lock diff is empty; gate re-run green |

Plan 05-03 (release and hardening), excluding D-10 (folded into SC-4):

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 03-1 | Tracer: release smoke asserts 422 depth, GET/PATCH during the loop, 504 at >= 4900 ms, then 200 "120" | ✓ VERIFIED | Ran `bash scripts/ci-smoke.sh` on the HEAD release build: OK, loop 504 after 5009 ms, watchdog line present, no backstop line |
| 03-2 | Smoke uses inline `env -i`, binds 127.0.0.1, EXIT trap cleans up; ci.yml unchanged; guard (g) ok | ✓ VERIFIED | `ci-smoke.sh:52-66,103`; ci.yml diff is empty; guard [g] ok |
| 03-3 | IN-04: op_print retry loop (Interrupted, WouldBlock, WriteZero, flush retried); signature unchanged | ✓ VERIFIED | `ops.rs:371-414`; print_ tests pass in the suite |
| 03-4 | IN-06: worker_loop doc corrected about panics in op callbacks | ✓ VERIFIED | `executor.rs:317-323` |
| 03-5 | IN-07: 2 MiB stack floor; stack 0 runs the max-depth chain with a RangeError leaf | ✓ VERIFIED | `MIN_STACK_BYTES` (`executor.rs:56`); `stack_bytes_are_clamped`, `minimum_stack_runs_the_max_depth_chain` pass |
| 03-6 | IN-05: .txt payload parity | ✓ VERIFIED | `txt_payload_{fac,sum,bin,store}_*` pass |
| 03-7 | D-12: docs describe both limits, answers, in-code settings, kept writes, heap not covered | ✓ VERIFIED | `docs/js-ops.md:93-103,147-164,210-236`; the op_rs3_patch table row lists 422 and 504 |
| 03-9 | Concurrency: suite passes at default threads, serially, 5x, under `env -i`, in < 60 s | ✓ VERIFIED (partial re-run) | Re-run: full suite in 5 s at default threads; the 13 TEST-04 tests ran with `--test-threads=1` under `env -i`. The 5x and full serial runs were not repeated here (one full run per verification); SUMMARY records them, and VALIDATION row 05-03-03 is green |
| 03-10 | D-14 phase gate; Cargo.lock identical; ci.yml unchanged; no allow/expect/ignore | ✓ VERIFIED | Gate re-run green; guards [a]/[b] ok; grep of the changed files finds no `#[allow`/`#[expect`/`#[ignore` |
| 03-11 | 05-VALIDATION.md: nine Phase 5 rows green, `wave_0_complete: true` | ✓ VERIFIED | 9 rows are `✅ green`, `wave_0_complete: true`. `status`/`nyquist_compliant` were later set to `validated`/`true` by validate-phase, which owns those fields (this is expected and not a deviation) |

**Score:** 35/38 truths verified. 2 are present but behavior-unverified, and 1 is uncertain (SC-4's CI half).

### Prohibitions

| Prohibition | Tier | Disposition | Evidence |
|-------------|------|-------------|----------|
| 05-01 ROBU-05: no unbounded self-patch or cycle before the depth check (D-11) | judgment | flagged: unverified prohibition, human review recommended. Verifier verdict (non-authoritative): holds | Commit order `dc5ff52` (finite chain) -> `92c6198` (check) -> `5748b2b` (first self-patch) |
| 05-01 TEST-04: depth tests are not weakened | test | VERIFIED | Guards [a]/[b] enforce no suppression or ignore in CI; depth tests compare the full JSON body (`depth_body(..)`) |
| 05-02 ROBU-06: no 504 while the script can still write | test | VERIFIED | `timeout_refuses_writes_after_stop` passes; `live_ctx` returns `Ok(None)` first once timed out |
| 05-02 ROBU-06: no cross-job timeout or misclassification | test | VERIFIED | `late_watchdog_never_hits_next_job`, `timeout_queued_patch_runs_once_the_worker_frees_up`; deadline and backstop start at pickup |
| 05-02 TEST-04: no un-armed infinite loop | judgment | flagged: unverified prohibition, human review recommended. Verifier verdict (non-authoritative): holds | Every `while(true)`/`for(;;)` in `src/` runs through `run_ctx_on`, `execute` or HTTP on a dedicated executor with a 300 to 1000 ms timeout; RED `66a5c39` comes before the watchdog in `7d704f7`, and `025a987` (after it) uses `run_ctx_on` |
| 05-03 ROBU-05: docs never imply rollback or env-set limits | judgment | flagged: unverified prohibition, human review recommended. Verifier verdict (non-authoritative): holds | `docs/js-ops.md:99,103,229-232` |
| 05-03 ROBU-06: remote CI and PKU watchdog not claimed as verified | judgment | flagged: unverified prohibition, human review recommended. Verifier verdict (non-authoritative): holds | 05-03-SUMMARY D7 `verification: []`, UAT item `result: [pending]` |
| 05-03 TEST-04: no push or git config change; smoke not weakened | test | VERIFIED | `origin/modernize-toolchain` = `fd8f43f`; no local user/credential config; smoke compares exact codes and bodies |

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `src/js/mod.rs` | ExecError::DepthLimit/Timeout, JsSettings limits, JobState, Registered | ✓ VERIFIED | Substantive (316 lines), used by ops/runtime/executor |
| `src/js/ops.rs` | depth check, live_ctx, silent ops after a timeout, print retry | ✓ VERIFIED | `stop_depth`, `timed_out`, `WouldBlock` present and exercised |
| `src/js/runtime.rs` | isolate registration, single-log suppression, nested stack-boot classification | ✓ VERIFIED | `thread_safe_handle` at :84, `job.stopped()` at :88/:99/:147 |
| `src/js/executor.rs` | Limits, watchdog, classify, BACKSTOP_GRACE, MIN_STACK_BYTES | ✓ VERIFIED | All present and wired into `start`/`worker_loop`/`run_job` |
| `src/http/error.rs` | js_depth_limit 422, js_timeout 504 | ✓ VERIFIED | `:48-49, :78-79, :92-93` |
| `src/http/mod.rs` | HTTP depth/timeout/txt tests | ✓ VERIFIED | `depth_limit_cycle_is_422`, `requests/fac.txt` parity tests present and passing |
| `scripts/ci-smoke.sh` | release depth and timeout smoke | ✓ VERIFIED | Ran green |
| `docs/js-ops.md` | limits documentation | ✓ VERIFIED | `js_depth_limit`, `timeout_ms`, heap note present |
| `Cargo.toml` | tokio `time`/`macros` | ✓ VERIFIED | Diff limited to those features |
| `05-VALIDATION.md` | nine rows green | ✓ VERIFIED | See 03-11 |

### Key Link Verification

| From | To | Via | Status | Details |
|------|----|-----|--------|---------|
| `op_rs3_patch` | `JobState::stop_depth` | first-wins record before the throw | WIRED | `ops.rs:299` |
| `classify` | `ExecError::DepthLimit`/`Timeout` | flags -> error | WIRED | `executor.rs:300-315`, called at `:374` |
| `From<ExecError>` | HTTP 422/504 bodies | `JsDepthLimit`/`JsTimeout` | WIRED | `error.rs:48-49`, `GATEWAY_TIMEOUT` at :93 |
| `run_fresh` | `JobState::stopped` | per-level log suppression | WIRED | `runtime.rs:88,99` |
| `run_fresh` | `JobState::register` | `Registered::new(.., thread_safe_handle())` | WIRED | `runtime.rs:84` |
| `watchdog_loop` | `JobState::fire` | deadline/refire | WIRED | `executor.rs:102` |
| `worker_loop` | `classify` | `job.finish()` | WIRED | `executor.rs:363,374` |
| `run_job` | `ExecError::Timeout` | `tokio::time::timeout(timeout + BACKSTOP_GRACE)` | WIRED | `executor.rs:231-243` |
| `op_rs3_print` | `write_all_retrying` | both streams | WIRED | `ops.rs:376,380` |
| `ci.yml` | `ci-smoke.sh` | `run: bash scripts/ci-smoke.sh` | WIRED | `ci.yml:59` |
| `docs/js-ops.md` | `JsSettings` | documented defaults 4 / 5000 | WIRED | Matches `mod.rs:250-251` |
| `main.rs` | `JsExecutor` with limits | `JsSettings::default()` | WIRED | `main.rs:17` |

### Data-Flow Trace (Level 4)

| Artifact | Data Variable | Source | Produces Real Data | Status |
|----------|---------------|--------|--------------------|--------|
| 422 depth body | `DepthStop.chain` | `ExecCtx.chain` grown by `nested(key)` per real PATCH level | Yes (observed `me -> me -> me -> me -> me` live) | ✓ FLOWING |
| 504 timeout body | `limit_ms` | `Limits.timeout_ms` from `JsSettings` | Yes (observed 5000 live, 300/500/1000 in tests) | ✓ FLOWING |

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
|----------|---------|--------|--------|
| Guards | `bash scripts/ci-guards.sh` | all 8 guards ok | ✓ PASS |
| Guard self-test | `bash scripts/test-ci-guards.sh` | 99 cases, 0 missed | ✓ PASS |
| Format | `cargo fmt --check` | exit 0 | ✓ PASS |
| Lint | `cargo clippy --all-targets --locked -- -D warnings` | exit 0 | ✓ PASS |
| Full suite (once) | `cargo test --locked` | 216 passed, 0 failed, 5 s | ✓ PASS |
| TEST-04 named set | `env -i PATH HOME cargo test --locked --lib -- --exact --test-threads=1 <13 tests>` | 13 passed | ✓ PASS |
| Release build | `cargo build --release --locked` | exit 0 | ✓ PASS |
| Release smoke | `bash scripts/ci-smoke.sh` | OK; 422 depth; GET and PATCH during the loop; 504 after 5009 ms by the watchdog; server alive | ✓ PASS |
| Lock and CI file unchanged | `git diff 449aaf2 -- Cargo.lock .github/workflows/ci.yml` | empty | ✓ PASS |
| Remote CI at HEAD | (requires a push) | not observed | ? SKIP -> human |

### Probe Execution

Step 7c: no `scripts/*/tests/probe-*.sh` exist, and no plan declares a probe. Not applicable.

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|-------------|-------------|--------|----------|
| ROBU-05 | 05-01, 05-03 | Self- or cyclic-PATCH stopped by a depth limit with 422, no crash | ✓ SATISFIED | SC-1, 01-* truths, smoke |
| ROBU-06 | 05-02, 05-03 | `while(true){}` terminated after a configurable timeout with 504; other requests served | ✓ SATISFIED (local) | SC-2, SC-3, 02-* truths, smoke; x86 PKU run pending (human item 1) |
| TEST-04 | 05-01, 05-02, 05-03 | Each error path has a test | ✓ SATISFIED locally / ? CI pending | 13 named tests pass; CI run of HEAD pending |

No orphaned requirements: REQUIREMENTS.md maps exactly ROBU-05, ROBU-06 and TEST-04 to Phase 5, and all three are claimed by plans.

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| (phase-changed files) | - | TBD/FIXME/XXX | - | None found |
| (phase-changed files) | - | TODO/HACK/placeholder, `#[allow]`/`#[ignore]` | - | None found |
| `src/js/executor.rs` | 236-239 | Backstop logs with `eprintln!` rather than the job sink and without a key label; a backstopped job logs twice | ℹ️ Info | Deviates from the "through the job's log sink" wording in 05-02 D-05; review IN-01, deferred to v2 OPS-01. No effect on answers |
| Doc-tests | - | 1 ignored doctest | ℹ️ Info | Comes from upstream deno_core `extension!` macro text (`deno_core-0.412.0/extensions.rs`), not project code; guard [b] passes |
| `scripts/ci-smoke.sh` | 175-185 | Concurrent-PATCH check is skipped on a 1-CPU host | ℹ️ Info | With one JS worker, a PATCH queues behind a runaway script for up to 5 s (answered, not starved indefinitely). Documented; review IN-04 deferred |
| tests | - | Tight timing upper bounds | ℹ️ Info | Review IN-06; watch for flakiness on the CI runner |

### Human Verification Required

#### 1. Remote CI run of the Phase 5 HEAD (x86_64, protected V8 platform)

**Test:** Push `modernize-toolchain` at this HEAD and open the GitHub Actions run on ubuntu-26.04.
**Expected:** `cargo test --locked` is green. The Release smoke shows `PATCH /key/me -> 422 js_depth_limit`, `GET / during a runaway PATCH -> 200` and `PATCH /key/loop -> 504 js_timeout after N ms, terminated by the watchdog`. No `server died (status 139)`.
**Why human:** SC-4 requires CI too, and the watchdog under x86 PKU (assumption A1) can only be observed on the runner. The last observed run (39507141932, fd8f43f) is Phase 4's state.

#### 2. Late-registered isolate is terminated

**Test:** Make a nested runtime finish creation after the watchdog fired (test hook or code review of `JobState::register` under the registry lock).
**Expected:** The isolate is terminated at registration, and the job answers 504.
**Why human:** Present and wired, but no test reaches that branch.

#### 3. Client disconnect during a runaway PATCH

**Test:** Release binary; `curl --max-time 1 -X PATCH /key/loop` (the client leaves); wait about 6 s; `PATCH /key/m`.
**Expected:** The log shows the worker's timeout line and no backstop line; PATCH m answers 200 "120".
**Why human:** Present and wired; no test drops the request after pickup.

#### 4. Judgment-tier prohibitions (4)

**Test:** Review the four flagged prohibitions in the Prohibitions table.
**Expected:** Agree with the non-authoritative "holds" verdicts.
**Why human:** Autonomous verification must not silently pass judgment-tier prohibitions.

### Gaps Summary

No blocking gaps. The phase goal is achieved in the codebase and was observed end to end on the release binary. A self-patching or cyclic script answers 422 `js_depth_limit` with the exact key chain, and the server keeps serving. A `while(true){}` script is terminated by the per-worker watchdog and answers 504 `js_timeout` at about 5 s, while GET / and other PATCHes are answered meanwhile. Afterwards the same worker serves the next job. Both limits come from `JsSettings` built in code. Every TEST-04 error path has a named, passing test, and the whole local gate is green at HEAD with Cargo.lock and ci.yml unchanged.

Three items remain for a human:
- The CI half of SC-4: the x86_64 run of this HEAD, which needs a push.
- Two sub-clauses of 05-02 truths that are present and wired but not exercised by a test: the register-after-timeout branch, and a client disconnect after pickup.
- The four judgment-tier prohibitions, which are flagged by policy (all assessed as holding).

Deliberate exclusions, not gaps: heap exhaustion (T-05-10), writes before a stop are kept, and the limits are settable only in code.

---

_Verified: 2026-10-21T01:20:52Z_
_Verifier: Claude (gsd-verifier)_
