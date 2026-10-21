---
phase: "06"
slug: "native-deployment-documentation"
status: verified
# threats_open = count of OPEN threats at or above workflow.security_block_on severity (the blocking gate)
threats_open: 0
asvs_level: 1
created: "2026-10-21"
---

# Phase 06 — Security

> Per-phase security contract: threat register, accepted risks, and audit trail.
> The register was authored at plan time (`<threat_model>` in 06-01 and 06-02) and verified at ASVS L1 against tree `eec6de5`. Depth: anchored greps of `deploy/rs3.service`, `deploy/rs3.conf`, `Readme.md` and `docs/js-ops.md` (whitespace-flattened where wording wraps), plus the green user-manager live run (`unit-live.sh bridge`: `NoNewPrivs: 1 Seccomp: 2`) and the README replay (130 checks), both re-run on the final tree. The short-circuit rule applied (threats_open 0, register authored at plan time, ASVS 1), so no auditor was spawned. Neither SUMMARY carries a `## Threat Flags` section.

---

## Trust Boundaries

| Boundary | Description | Data Crossing |
|----------|-------------|---------------|
| network → rs3 port | Any client that reaches the port can store data and run JavaScript (no authentication by design) | Untrusted requests and code |
| V8 sandbox → host | Stored scripts run in-process; the systemd sandbox of the example unit is the outer wall | Untrusted code |
| /etc/rs3/rs3.conf → PID 1 → rs3 | Root-owned configuration read by the service manager before the unprivileged process starts | Listen address and port |
| repository → git index | Removing an ignore rule makes previously ignored local files addable | Possible local secrets |
| stored script → server stdout/journal | `Deno.core.print` output reaches operator logs unescaped on stdout | Untrusted text |
| README → operator/developer actions | Documentation drives how operators expose the port and how clients handle errors | Operational guidance |

---

## Threat Register

| Threat ID | Category | Component | Severity | Disposition | Mitigation | Status |
|-----------|----------|-----------|----------|-------------|------------|--------|
| T-06-01 | Elevation of Privilege | `deploy/rs3.service` sandbox around V8 | high | mitigate | `DynamicUser=yes` + `User=rs3`, empty `CapabilityBoundingSet=`/`AmbientCapabilities=`, `NoNewPrivileges=yes`, `ProtectSystem=strict`, `ProtectHome=yes`, `PrivateTmp=yes`, `PrivateDevices=yes`, `ProtectKernel*`, `RestrictNamespaces=yes`, `RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6`, `SystemCallFilter=@system-service @pkey` (all present, anchored greps); `systemd-analyze verify` clean, offline exposure 1.5; live `NoNewPrivs: 1 Seccomp: 2`. Runtime effect of the root-only directives: operator UAT | closed |
| T-06-02 | Elevation of Privilege | remote JS execution by anyone reaching the port (`deploy/rs3.conf`, README Deployment) | high | mitigate | `RS3_HOST=127.0.0.1` in `deploy/rs3.conf`; README `### Security note` ("rs3 has no authentication", reverse proxy with auth/TLS) | closed |
| T-06-03 | Denial of Service | restart policy (bad config or crash loop) | medium | mitigate | `StartLimitIntervalSec=60`, `StartLimitBurst=5`, `Restart=on-failure`; proven by the `startlimit` and `nofile` live modes (failed after 5 restarts) | closed |
| T-06-04 | Denial of Service | sandbox directives breaking V8 | medium | mitigate | No `MemoryDenyWriteExecute` line (anchored grep; it crashes V8 at the first PATCH), `@pkey` explicit, `SystemCallErrorNumber=EPERM`; seccomp kill-action run (nested PATCH, 422, 504) had no SIGSYS. x86_64 PKU: operator UAT | closed |
| T-06-05 | Tampering | `/etc/rs3/rs3.conf` | low | mitigate | README installs it with `sudo install -D -m 0644` (root-owned); `EnvironmentFile=/etc/rs3/rs3.conf` has no `-` prefix, so a missing file fails loudly (`nofile` mode) | closed |
| T-06-06 | Elevation of Privilege | binding a privileged port | low | mitigate | No `CAP_NET_BIND_SERVICE` in the unit's directives; README documents it as an explicit opt-in | closed |
| T-06-07 | Information Disclosure | removal of the `/docker_rs3/.secrets/` ignore line | medium | mitigate | `docker_rs3/` no longer exists, `git status --porcelain --ignored -- docker_rs3` is empty, and `.gitignore` no longer names it | closed |
| T-06-08 | Repudiation | `Deno.core.print` raw stdout forging rs3-looking journal lines | low | mitigate | `docs/js-ops.md` states stdout is "raw, unescaped" (IN-03/IN-07 rewrite); README `### Output` says scripts and their output are untrusted; structured logging deferred (OPS-01) | closed |
| T-06-09 | Elevation of Privilege | README run examples exposing an unauthenticated port | medium | mitigate | Quick start binds `127.0.0.1`; no `0.0.0.0` anywhere in `Readme.md` | closed |
| T-06-10 | Tampering | misdocumented error contract | low | mitigate | `readme-replay.sh` asserts every documented status and body byte for byte against the release binary (130 checks green); code-derived exceptions are labelled in the README | closed |
| T-06-11 | Denial of Service | no heap limit: an allocating script can abort the process | medium | accept | Documented in README Limits ("There is no heap limit yet") and `docs/js-ops.md`; under the example unit, `Restart=on-failure` and the start limit bound the impact; JSHD-01 is v2 | closed |
| T-06-12 | Tampering | a dotenv file at the filesystem root read under the unit | low | accept | Documented in README "Configuration under systemd"; the environment always wins over the file; `/` is root-owned | closed |
| T-06-SC | Tampering | npm/pip/cargo installs | low | accept | The phase installs no packages; `Cargo.lock` is byte-identical to `90d06ec` | closed |

*Status: open · closed · open — below high threshold (non-blocking)*
*Severity: critical > high > medium > low — only open threats at or above workflow.security_block_on count toward threats_open*
*Disposition: mitigate (implementation required) · accept (documented risk) · transfer (third-party)*

---

## Accepted Risks Log

| Risk ID | Threat Ref | Rationale | Accepted By | Date |
|---------|------------|-----------|-------------|------|
| AR-06-01 | T-06-11 | No V8 heap limit yet; v2 requirement JSHD-01. Documented for operators; the example unit restarts the service and stops at the start limit | planner (06-02 threat model), confirmed at audit | 2026-10-21 |
| AR-06-02 | T-06-12 | A dotenv file at `/` would be read under the unit; `/` is root-owned and the unit's environment always wins | planner (06-02 threat model), confirmed at audit | 2026-10-21 |
| AR-06-03 | T-06-SC | No package installs in this phase; the lockfile is unchanged | planner (06-01/06-02 threat models), confirmed at audit | 2026-10-21 |

*Accepted risks do not resurface in future audit runs.*

---

## Security Audit Trail

| Audit Date | Threats Total | Closed | Open | Run By |
|------------|---------------|--------|------|--------|
| 2026-10-21 | 13 | 13 | 0 | /gsd-secure-phase 6 (orchestrator, L1 short-circuit) |

Pending operator UAT (runtime effect, not a register gap): on a real systemd host, confirm the root-only directives apply, the main PID is not root, and V8 runs under the seccomp filter on x86_64 with PKU (see 06-VALIDATION.md Manual-Only Verifications).

---

## Sign-Off

- [x] All threats have a disposition (mitigate / accept / transfer)
- [x] Accepted risks documented in Accepted Risks Log
- [x] `threats_open: 0` confirmed
- [x] `status: verified` set in frontmatter

**Approval:** verified 2026-10-21
