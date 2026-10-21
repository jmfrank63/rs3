---
phase: "06"
slug: "native-deployment-documentation"
# status lifecycle: draft (seeded by plan-phase) → validated (set by validate-phase §6)
# audit-milestone §5.5 distinguishes NOT-VALIDATED (draft) from PARTIAL (validated + nyquist_compliant: false) (#2117)
status: draft
nyquist_compliant: false
wave_0_complete: false
created: "2026-10-21"
---

# Phase 06 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | cargo test (Rust 1.99.1 built-in harness; 216 tests in the latest CI run), shell gates (`scripts/*.sh`), `systemd-analyze` (systemd 255), a user-manager live run under `target/tmp` |
| **Config file** | none (`Cargo.toml`; `clippy.toml` holds the test allowances) |
| **Quick run command** | `bash scripts/ci-guards.sh && python3 scripts/check-workflow.py && systemd-analyze verify --recursive-errors=no deploy/rs3.service` |
| **Full suite command** | `bash scripts/ci-guards.sh && python3 scripts/check-workflow.py && bash scripts/test-ci-guards.sh && cargo fmt --check && cargo clippy --all-targets --locked -- -D warnings && cargo test --locked && cargo build --release --locked && bash scripts/ci-smoke.sh` |
| **Estimated runtime** | ~240 seconds when warm (cargo test and the release smoke dominate; the release smoke includes the 5 s timeout case) |

---

## Sampling Rate

- **After every task commit:** Run the quick run command (once `deploy/rs3.service` exists; before that, the guards and check-workflow).
- **After every plan wave:** Run the full suite command. Rebuild the release binary before any live run once `Cargo.toml` changes.
- **Before `/gsd-verify-work`:** The full suite, the unit-derived user-manager live run and the README command replay must all be green.
- **Max feedback latency:** 240 seconds

---

## Per-Task Verification Map

Filled by the planner/executor as tasks are created. The requirement → command map lives in 06-RESEARCH.md §Validation Architecture.

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| 06-xx-xx | — | — | (see research map) | — | — | — | — | — | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `target/tmp/…/unit-live.sh`: a user-manager live run derived from the committed `deploy/rs3.service` and `deploy/rs3.conf` (bridge, banner, `GET /`, PATCH through V8, `Seccomp: 2`; also a kill-action seccomp variant that runs the 504 timeout). Not committed.
- [ ] A README replay under `target/tmp`: start `target/release/rs3` with `env SERVER.HOST=127.0.0.1 SERVER.PORT=<free port>` and diff every documented command and body. Not committed. Never name the dotenv file in a command.
- No cargo test changes are needed, because the phase changes no behaviour.

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Root-only unit directives (`DynamicUser`, `ProtectSystem`/`ProtectHome`, `PrivateTmp`/`PrivateDevices`, `ProtectKernel*`, capability bounding, `ProtectProc`) take effect, the service runs as a non-root user, and V8 runs under the seccomp filter on the target CPU (including PKU `pkey_*` on x86_64) | DEPL-03 | No sudo on the dev host. Ubuntu 24.04 blocks unprivileged user namespaces, so a user-manager unit cannot apply these directives. | On a Linux server with systemd, install per the README Deployment section. Then: `systemctl status rs3` is active; `curl -s http://127.0.0.1:5000/` returns `"Welcome to rs3"`; insert `{"m":"6*20"}` and `curl -s -X PATCH http://127.0.0.1:5000/key/m` returns `"120"`; `ps -o user= -p $(systemctl show -p MainPID --value rs3)` is not `root`; record `systemd-analyze security rs3` |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 240s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
