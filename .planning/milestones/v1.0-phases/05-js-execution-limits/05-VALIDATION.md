---
phase: "05"
slug: "js-execution-limits"
# status lifecycle: draft (seeded by plan-phase) → validated (set by validate-phase §6)
# audit-milestone §5.5 distinguishes NOT-VALIDATED (draft) from PARTIAL (validated + nyquist_compliant: false) (#2117)
status: validated
nyquist_compliant: true
wave_0_complete: true
created: "2026-10-20"
---

# Phase 05 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.
> Source: `05-RESEARCH.md` § Validation Architecture (requirement → test map, Wave 0 gaps, live smoke).

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust libtest (in-crate `#[cfg(test)]`), `#[actix_web::test]`, shared `test_executor()` + dedicated short-timeout executors |
| **Config file** | none (Cargo.toml dev-dependency platform feature; clippy.toml test allowances) |
| **Quick run command** | `cargo test --locked -- depth timeout watchdog backstop` |
| **Full suite command** | `bash scripts/ci-guards.sh && cargo fmt --check && cargo clippy --all-targets --locked -- -D warnings && cargo test --locked && cargo build --release --locked && bash scripts/ci-smoke.sh` |
| **Estimated runtime** | ~10 seconds (cargo test after this phase), plus ~10 s for the release smoke |

---

## Sampling Rate

- **After every task commit:** Run the task's `cargo test --locked -- <filters>` + `cargo clippy --all-targets --locked -- -D warnings`
- **After every plan wave:** Run `cargo test --locked` and `cargo test --locked -- --test-threads=1`
- **Before `/gsd-verify-work`:** Full suite + 5× repeat + `env -i` run + extended release smoke (ports 18530–18549)
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

Rows filled by the planner. Plan 05-03 Task 3 flips each Status cell to green (and File Exists to ✅) once its commands have passed. The requirement-to-command map is in `05-RESEARCH.md` § Validation Architecture (ROBU-05, ROBU-06, TEST-04, D-01..D-14).

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| 05-01-01 | 01 | 1 | ROBU-05, TEST-04 | T-05-01, T-05-03 | over-deep, self-patching and cyclic PATCH chains answer 422 js_depth_limit with the exact chain; checked before the store read | TDD (RED/GREEN) + HTTP | `cargo test --locked --lib -- depth_limit_ exec_ctx_nested` | ✅ | ✅ green |
| 05-01-02 | 01 | 1 | ROBU-05, TEST-04 | T-05-02, T-05-03 | hard stop: ops refuse after the stop, a caught stop still answers 422, earlier writes stay, one log line; host fault beats depth | TDD + unit | `cargo test --locked --lib -- depth_ every_op_throws logged` | ✅ | ✅ green |
| 05-01-03 | 01 | 1 | ROBU-05 | T-05-01 | max_patch_depth from settings, clamped to 1..=64; maximum depth with deep leaf recursion stays catchable | TDD + unit + plan gate | `cargo test --locked --lib -- max_patch_depth max_depth_leaf default_settings depth_limit_is_repeatable` and the plan gate | ✅ | ✅ green |
| 05-02-01 | 02 | 2 | ROBU-06, TEST-04 | T-05-04, T-05-06, T-05-07 | runaway script answers 504 js_timeout; every runtime in the job's termination group; per-worker watchdog; the worker serves the next job | TDD (RED/GREEN) + HTTP | `cargo test --locked --lib -- timeout job_state default_settings` | ✅ | ✅ green |
| 05-02-02 | 02 | 2 | ROBU-06, TEST-04 | T-05-05, T-05-06 | ops silent after a timeout; caught nested termination, throwing-op loop and looping toString end as Timeout; no write after the stop; late watchdog never hits the next job | TDD + unit | `cargo test --locked --lib -- timeout late_watchdog timed_out_worker classify_precedence` | ✅ | ✅ green |
| 05-02-03 | 02 | 2 | ROBU-06, TEST-04 | T-05-08, T-05-09 | backstop at timeout + 2 s from pickup; GET / and another PATCH answered during a loop; a queued PATCH runs once the worker frees up | TDD + HTTP + plan gate | `cargo test --locked --lib -- backstop timeout_does_not_block timeout_queued timeout_nested_loop_is_504`, `git diff --exit-code 449aaf2 -- Cargo.lock` and the plan gate | ✅ | ✅ green |
| 05-03-01 | 03 | 3 | ROBU-05, ROBU-06 | T-05-04, T-05-14 | release binary (protected platform): 422 js_depth_limit, 504 js_timeout, other requests answered meanwhile, nothing left running | live smoke | `cargo build --release --locked && RS3_SMOKE_PORT=18533 bash scripts/ci-smoke.sh` | ✅ | ✅ green |
| 05-03-02 | 03 | 3 | TEST-04 | T-05-12, T-05-13 | op_print retries on WouldBlock; 2 MiB stack floor; accurate catch_unwind doc; .txt payload parity | TDD + unit + HTTP | `cargo test --locked --lib -- print_ stack_bytes minimum_stack txt_payload` | ✅ | ✅ green |
| 05-03-03 | 03 | 3 | ROBU-05, ROBU-06, TEST-04 | T-05-10, T-05-11, T-05-15 | docs match behaviour; 13 error-path tests pass by name; serial, 5x and env -i runs; no fabricated remote result | full phase gate | the full suite command above, `--test-threads=1`, five repeats, `env -i`, and the exact TEST-04 run | ✅ | ✅ green |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [x] Compiling stubs: `JsSettings.{timeout_ms, max_patch_depth}`, `ExecError::{Timeout, DepthLimit}`, `ApiError::{JsTimeout, JsDepthLimit}`
- [x] Test helpers: dedicated executor with settings overrides; `AppState` with a dedicated executor
- [x] RED tests that never allocate unbounded recursion or hang (finite chain k1..k6; `Date.now()`-bounded busy loop)

---

## Manual-Only Verifications

All phase behaviors have automated verification (x86_64 production-platform behaviour via the CI release smoke — remote run pending operator push).

---

## Validation Sign-Off

- [x] All tasks have `<automated>` verify or Wave 0 dependencies
- [x] Sampling continuity: no 3 consecutive tasks without automated verify
- [x] Wave 0 covers all MISSING references
- [x] No watch-mode flags
- [x] Feedback latency < 30s
- [x] `nyquist_compliant: true` set in frontmatter

**Approval:** approved 2026-10-21 (validate-phase audit: ROBU-05/ROBU-06/TEST-04 COVERED; 212 tests green incl. 13 named error-path tests; env-free and serial runs green; release smoke covers 422/504; x86_64 remote run pending operator push)

## Validation Audit 2026-10-21

| Metric | Count |
|---|---|
| Gaps found | 0 |
| Resolved | 0 |
| Escalated | 0 |
