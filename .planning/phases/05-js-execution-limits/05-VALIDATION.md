---
phase: "05"
slug: "js-execution-limits"
# status lifecycle: draft (seeded by plan-phase) → validated (set by validate-phase §6)
# audit-milestone §5.5 distinguishes NOT-VALIDATED (draft) from PARTIAL (validated + nyquist_compliant: false) (#2117)
status: draft
nyquist_compliant: false
wave_0_complete: false
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

Filled by the planner/executor as tasks are created; requirement → command map lives in `05-RESEARCH.md` § Validation Architecture (ROBU-05, ROBU-06, TEST-04, D-01..D-14).

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| 05-xx-xx | — | — | (see research map) | — | — | — | — | — | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] Compiling stubs: `JsSettings.{timeout_ms, max_patch_depth}`, `ExecError::{Timeout, DepthLimit}`, `ApiError::{JsTimeout, JsDepthLimit}`
- [ ] Test helpers: dedicated executor with settings overrides; `AppState` with a dedicated executor
- [ ] RED tests that never allocate unbounded recursion or hang (finite chain k1..k6; `Date.now()`-bounded busy loop)

---

## Manual-Only Verifications

All phase behaviors have automated verification (x86_64 production-platform behaviour via the CI release smoke — remote run pending operator push).

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
