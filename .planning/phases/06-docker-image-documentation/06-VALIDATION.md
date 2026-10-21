---
phase: "06"
slug: "docker-image-documentation"
# status lifecycle: draft (seeded by plan-phase) → validated (set by validate-phase §6)
# audit-milestone §5.5 distinguishes NOT-VALIDATED (draft) from PARTIAL (validated + nyquist_compliant: false) (#2117)
status: draft
nyquist_compliant: false
wave_0_complete: false
created: "2026-10-21"
---

# Phase 06 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.
> Source: `06-RESEARCH.md` § Validation Architecture (requirement → command map, simulated-context build, planted cases).

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | bash guard scripts + Python checker + `cargo test`; no Docker on the host (image build/run verified by the CI `docker` job on push) |
| **Config file** | none |
| **Quick run command** | `bash scripts/ci-guards.sh && python3 scripts/check-workflow.py && bash -n scripts/docker-smoke.sh` |
| **Full suite command** | `cargo fmt --check && cargo clippy --all-targets --locked -- -D warnings && cargo test --locked && cargo build --release --locked && bash scripts/ci-smoke.sh && bash scripts/ci-guards.sh && bash scripts/test-ci-guards.sh` |
| **Estimated runtime** | ~5 seconds quick; a few minutes full (plus ~80 s for the simulated-context release build) |

---

## Sampling Rate

- **After every task commit:** Run the quick run command
- **After every plan wave:** Run the full suite command
- **Before `/gsd-verify-work`:** Full suite green + simulated `.dockerignore` context build + README command checks
- **Max feedback latency:** 10 seconds (quick)

---

## Per-Task Verification Map

Filled by the planner/executor as tasks are created; requirement → command map lives in `06-RESEARCH.md` § Validation Architecture (DOCK-01..03, DOCS-01..03, D-01..D-12).

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| 06-xx-xx | — | — | (see research map) | — | — | — | — | — | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `Dockerfile` (rewrite), `.dockerignore` (allowlist), `scripts/docker-smoke.sh`, guard (i) + planted cases
- [ ] CI `docker` job + `check-workflow.py` CANONICAL/structure updates + self-test cases
- [ ] `Readme.md` rewrite; `docs/js-ops.md` carry-overs

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| `docker build` + `docker run` + `curl /` → `"Welcome to rs3"`, non-root | DOCK-01, DOCK-02 | Docker not installed on the dev host; CI `docker` job needs a push (forbidden in this run) | Push `modernize-toolchain` and confirm the `docker` job is green, or locally: `docker build -t rs3 . && docker run --rm -p 127.0.0.1:8080:8080 -e SERVER.HOST=0.0.0.0 rs3` then `curl 127.0.0.1:8080/` |
| README walkthrough by a new developer | DOCS-01 | Readability judgment | Follow only `Readme.md` on a fresh machine |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 10s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
