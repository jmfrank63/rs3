---
phase: "03"
slug: "javascript-runtime-port"
# status lifecycle: draft (seeded by plan-phase) → validated (set by validate-phase §6)
# audit-milestone §5.5 distinguishes NOT-VALIDATED (draft) from PARTIAL (validated + nyquist_compliant: false) (#2117)
status: draft
nyquist_compliant: false
wave_0_complete: false
created: "2026-10-20"
---

# Phase 03 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.
> Source: `03-RESEARCH.md` § Validation Architecture (requirement → test map, live smokes, Wave 0 gaps).

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust libtest + `#[actix_web::test]` (actix-web 4.17.0); in-crate `#[cfg(test)]` modules; shared `#[cfg(test)]` JS test executor |
| **Config file** | `clippy.toml` (test allowances), `Cargo.toml` `[lints]`; new `[dev-dependencies]` deno_core `unsafe_use_unprotected_platform` feature |
| **Quick run command** | `cargo test --lib js:: && cargo test --lib http::` |
| **Full suite command** | `cargo build --locked && cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check` |
| **Estimated runtime** | ~10 seconds warm (quick), ~90 seconds warm (full gate) |

---

## Sampling Rate

- **After every task commit:** Run `cargo test --lib` + `cargo clippy --all-targets -- -D warnings`
- **After every plan wave:** Run the full suite command + the grep gates from the research map
- **Before `/gsd-verify-work`:** Full suite green + `env -i PATH="$PATH" HOME="$HOME" cargo test` + `cargo test -- --test-threads=1` + 5× repeat + live smokes (ports 18500–18519)
- **Max feedback latency:** 90 seconds

---

## Per-Task Verification Map

Filled by the planner/executor as tasks are created; requirement → command map lives in `03-RESEARCH.md` § Validation Architecture (HTTP-07, JSRT-01..06, ROBU-03, ROBU-04, TEST-03, TEST-05, TEST-06, D-07, D-09, D-11, D-12, D-17).

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| 03-xx-xx | — | — | (see research map) | — | — | — | — | — | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `src/js/*` test modules (op semantics, op errors, nested patch, op_panic guard, thread names, worker panic survival)
- [ ] `#[cfg(test)]` shared JS test executor wired into `test_state()`; `#[cfg(test)]` closed-executor constructor for the 500 test
- [ ] `Cargo.toml`: direct `tokio` dependency (rt, sync) + dev-dependency `deno_core` feature line; `Cargo.lock` stays on released versions
- [ ] TDD RED evidence under `.planning/phases/03-javascript-runtime-port/tdd-evidence/`

---

## Manual-Only Verifications

All phase behaviors have automated verification (x86_64 platform behaviour is verified by Phase 4 CI).

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 90s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
