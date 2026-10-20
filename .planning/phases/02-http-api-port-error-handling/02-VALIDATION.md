---
phase: "02"
slug: "http-api-port-error-handling"
# status lifecycle: draft (seeded by plan-phase) → validated (set by validate-phase §6)
# audit-milestone §5.5 distinguishes NOT-VALIDATED (draft) from PARTIAL (validated + nyquist_compliant: false) (#2117)
status: draft
nyquist_compliant: false
wave_0_complete: false
created: "2026-10-20"
---

# Phase 02 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.
> Source: `02-RESEARCH.md` § Validation Architecture (requirement → test map, Wave 0 gaps).

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust libtest + `#[actix_web::test]` (actix-web 4.17.0 test utilities) |
| **Config file** | none (Cargo defaults); new `clippy.toml` (Wave 0) |
| **Quick run command** | `cargo test --lib` |
| **Full suite command** | `cargo build --locked && cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check` |
| **Estimated runtime** | ~5 seconds warm (quick), ~60 seconds warm (full gate) |

---

## Sampling Rate

- **After every task commit:** Run `cargo test --lib` + `cargo clippy --all-targets -- -D warnings`
- **After every plan wave:** Run the full suite command + the static greps from the research map
- **Before `/gsd-verify-work`:** Full suite green, plus `env -i PATH="$PATH" HOME="$HOME" cargo test`, `cargo test -- --test-threads=1`, a 5× repeat run, and all live smokes
- **Max feedback latency:** 60 seconds

---

## Per-Task Verification Map

Filled in by the planner and executor as tasks are created. The requirement → command map is in `02-RESEARCH.md` § Validation Architecture → Phase Requirements → Test Map (HTTP-01..06, HTTP-08, ROBU-01, ROBU-02, ARCH-02, ARCH-03, TEST-01, TEST-02, D-13..D-17).

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| 02-xx-xx | — | — | (see research map) | — | — | — | — | — | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `clippy.toml` with `allow-unwrap-in-tests = true` and `allow-expect-in-tests = true`, in the same commit as `[lints.clippy] unwrap_used = "deny"` and `expect_used = "deny"`
- [ ] `test_state()` helper (per-test `AppState` built in code)
- [ ] New tests: the 404 set, the 400 set, the 422 set (assert the store is unchanged), the `{}` no-op, the escaping round-trip (codec and HTTP), the two-app shared state, and the config merge unit tests
- [ ] Rewrites: `test_patch_is_501` (insert the key first), `parse_rejects_malformed_json` (expect a malformed variant), remove `format_list_keeps_legacy_replace_quirk`, re-point the parked PATCH tests at `test_state()`

---

## Manual-Only Verifications

All phase behaviors have automated verification. Live-binary smokes (curl / `env -i`) run in the executor's shell.

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 60s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
