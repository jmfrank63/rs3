---
phase: "06"
slug: "native-deployment-documentation"
# status lifecycle: draft (seeded by plan-phase) → validated (set by validate-phase §6)
# audit-milestone §5.5 distinguishes NOT-VALIDATED (draft) from PARTIAL (validated + nyquist_compliant: false) (#2117)
status: validated
nyquist_compliant: true
wave_0_complete: true
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
| 06-01-01 | 01 | 1 | DEPL-03, DEPL-02, DOCS-04 | T-06-01, T-06-02, T-06-05 | the committed unit runs the release binary through the env(1) bridge with NoNewPrivs 1 and Seccomp 2; loopback default; license MIT with Cargo.lock unchanged | live (user manager) + static | `bash target/tmp/p6/unit-live.sh bridge`; `systemd-analyze verify --recursive-errors=no deploy/rs3.service` (empty, exit 0); `systemd-analyze security --offline=true --threshold=15 deploy/rs3.service`; install dry-run into target/tmp/p6/root | ✅ | ✅ green |
| 06-01-02 | 01 | 1 | DEPL-03, DEPL-02 | T-06-03, T-06-04 | V8 survives the seccomp kill action (nested PATCH, 422, 504, clean stop); a broken or missing env file stops at the start limit; install replaces a running binary | live (user manager) | `bash target/tmp/p6/unit-live.sh kill`, then the modes `startlimit`, `nofile` and `update` | ✅ | ✅ green |
| 06-01-03 | 01 | 1 | DEPL-01 | T-06-07 | no ignored or untracked file under the removed path becomes addable; the container artefacts and references are gone | static + full suite | `git grep -n -i docker -- . ':!.planning' ':!aws' ':!Vagrantfile' ':!experiments'` prints nothing; the D-17 full suite | n/a | ✅ green |
| 06-02-01 | 02 | 2 | DOCS-01, DOCS-03 | T-06-09, T-06-10 | every documented route body, error body and startup line equals the release binary's live output | replay | `bash target/tmp/p6/readme-replay.sh` (30 or more checks) | ✅ | ✅ green |
| 06-02-02 | 02 | 2 | DOCS-02, DOCS-03 | T-06-08, T-06-11 | raw stdout, the limits and the behaviour changes are documented truthfully | replay + static | `bash target/tmp/p6/readme-replay.sh` (45 or more checks); whitespace-flattened greps on docs/js-ops.md | ✅ | ✅ green |
| 06-02-03 | 02 | 2 | DOCS-01, DEPL-01 | T-06-SC | CI unchanged and green on the final tree; the README is complete in D-12 order | full suite + replay + live | the D-17 full suite, then `bash target/tmp/p6/readme-replay.sh` and `bash target/tmp/p6/unit-live.sh bridge` | ✅ | ✅ green |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [x] `target/tmp/p6/unit-live.sh` (created by 06-01 Task 1, extended by Task 2): a user-manager live run derived from the committed `deploy/rs3.service` and `deploy/rs3.conf`. Modes: `bridge` (banner tagged rs3, `GET /`, PATCH through V8, the env(1) bridge in the main PID's environment, `NoNewPrivs: 1 Seccomp: 2`), `kill` (seccomp kill action: nested PATCH, 422, 504, clean stop, no SIGSYS), `startlimit`, `nofile` and `update`. Not committed.
- [x] `target/tmp/p6/readme-replay.sh` (created by 06-02 Task 1, extended by Task 2): starts `target/release/rs3` with `env -i PATH="$PATH" SERVER.HOST=127.0.0.1 SERVER.PORT=<free port>`, diffs every documented command, body and startup line, and checks that each appears verbatim in Readme.md. Not committed. Never name the dotenv file in a command.
- No cargo test changes are needed, because the phase changes no behaviour.

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Root-only unit directives (`DynamicUser`, `ProtectSystem`/`ProtectHome`, `PrivateTmp`/`PrivateDevices`, `ProtectKernel*`, capability bounding, `ProtectProc`) take effect, the service runs as a non-root user, and V8 runs under the seccomp filter on the target CPU (including PKU `pkey_*` on x86_64) | DEPL-03 | No sudo on the dev host. Ubuntu 24.04 blocks unprivileged user namespaces, so a user-manager unit cannot apply these directives. | On a Linux server with systemd, install per the README Deployment section. Then: `systemctl status rs3` is active; `curl -s http://127.0.0.1:5000/` returns `"Welcome to rs3"`; insert `{"m":"6*20"}` and `curl -s -X PATCH http://127.0.0.1:5000/key/m` returns `"120"`; `ps -o user= -p $(systemctl show -p MainPID --value rs3)` is not `root`; record `systemd-analyze security rs3` |

---

## Validation Sign-Off

- [x] All tasks have `<automated>` verify or Wave 0 dependencies
- [x] Sampling continuity: no 3 consecutive tasks without automated verify
- [x] Wave 0 covers all MISSING references
- [x] No watch-mode flags
- [x] Feedback latency < 240s (quick run command: about 1 s; full suite about 4 min warm, run per wave)
- [x] `nyquist_compliant: true` set in frontmatter

**Approval:** approved 2026-10-21 (validate-phase audit: 0 automatable gaps; `unit-live.sh bridge` and `readme-replay.sh` (130 checks) re-run green on the final tree; DEPL-03 root-only directives stay manual-only)

## Validation Audit 2026-10-21

| Metric | Count |
|---|---|
| Gaps found | 0 |
| Resolved | 0 |
| Escalated | 0 |
| Manual-only (pre-existing) | 1 |
