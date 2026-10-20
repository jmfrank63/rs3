---
phase: "04"
slug: "ci-gate-on-x86-64"
# status lifecycle: draft (seeded by plan-phase) → validated (set by validate-phase §6)
# audit-milestone §5.5 distinguishes NOT-VALIDATED (draft) from PARTIAL (validated + nyquist_compliant: false) (#2117)
status: draft
nyquist_compliant: false
wave_0_complete: false
created: "2026-10-20"
---

# Phase 04 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.
> Source: `04-RESEARCH.md` § Validation Architecture (requirement → command map, planted-violation recipe).

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | bash scripts (`scripts/ci-guards.sh`, `scripts/ci-smoke.sh`) + Python structure check (`scripts/check-workflow.py`, PyYAML 6.0.1) + cargo test harness |
| **Config file** | none |
| **Quick run command** | `bash scripts/ci-guards.sh` |
| **Full suite command** | `bash scripts/ci-guards.sh && cargo fmt --check && cargo clippy --all-targets --locked -- -D warnings && cargo test --locked && cargo build --release --locked && bash scripts/ci-smoke.sh` |
| **Estimated runtime** | ~1 second (quick), a few minutes (full suite, release build) |

---

## Sampling Rate

- **After every task commit:** Run `bash scripts/ci-guards.sh` + `bash -n scripts/*.sh`
- **After every plan wave:** Run the full suite command
- **Before `/gsd-verify-work`:** Full suite green locally; remote x86_64 run recorded as pending UAT (D-09)
- **Max feedback latency:** 5 seconds for guards (release build excluded)

---

## Per-Task Verification Map

Rows filled by the planner. Plan 04-02 Task 2 flips each Status cell to green once its commands have passed. The requirement-to-command map is in `04-RESEARCH.md` § Validation Architecture (CI-01, CI-02, CI-03, D-05, D-06, D-08, D-09).

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| 04-01-01 | 01 | 1 | CI-01, CI-02, CI-03 | T-04-02, T-04-03 | single workflow, read-only token, no event-context interpolation | static + local workflow replay | `python3 -I scripts/check-workflow.py` and a replay of every ci.yml `run:` line in order | ❌ W0 | ⬜ pending |
| 04-01-02 | 01 | 1 | CI-02 | T-04-04, T-04-05 | the gate cannot be greened by suppression; the unprotected platform stays dev-only | planted-violation self-test | `bash scripts/test-ci-guards.sh` and `bash scripts/ci-guards.sh` | ❌ W0 | ⬜ pending |
| 04-02-01 | 02 | 2 | CI-03 | T-04-08, T-04-09, T-04-11 | loopback-only smoke with trap cleanup; a crash is reported as status 139 | live smoke + failure modes | `cargo build --release --locked && bash scripts/ci-smoke.sh` | ❌ W0 | ⬜ pending |
| 04-02-02 | 02 | 2 | CI-01, CI-02, CI-03 | T-04-10 | no fabricated remote result; no push | full local suite | the full suite command above, plus TOOL-03 | ✅ | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `scripts/ci-guards.sh`, `scripts/check-workflow.py`, `scripts/ci-smoke.sh`
- [ ] `.github/workflows/ci.yml` (new); `.github/workflows/actions-rs.yml` deleted
- [ ] `04-UAT.md` pending item for the remote x86_64 run (D-09) incl. the D-07 fallback

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Workflow runs green on the GitHub x86_64 runner | CI-03 (SC-1) | Requires `git push`, forbidden in this autonomous run | Push `modernize-toolchain` to origin; confirm the `ci` workflow is green; if V8 platform init crashes, apply D-07 |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 5s (guards)
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
