---
phase: "04"
slug: "ci-gate-on-x86-64"
status: verified
# threats_open = count of OPEN threats at or above workflow.security_block_on severity (the blocking gate)
threats_open: 0
asvs_level: 1
created: "2026-10-20"
---

# Phase 04 — Security

> Per-phase security contract: threat register, accepted risks, and audit trail.
> Register authored at plan time (04-01/02 `<threat_model>`). Verified at ASVS L1 (grep, the guard self-test with 32 planted cases, and the release smoke) against tree `dab4b58`. Short-circuit rule applied (threats_open 0, register authored at plan time, ASVS 1), so no auditor was spawned.

---

## Trust Boundaries

| Boundary | Description | Data Crossing |
|----------|-------------|---------------|
| GitHub event → workflow `run:` steps | Push/PR metadata could be interpolated into shell | Untrusted text (branch names, PR titles) |
| Third-party actions → runner | `actions/checkout`, `dtolnay/rust-toolchain`, `Swatinem/rust-cache` execute with the job token | Supply chain |
| Fork PRs → cache / token | PR runs share caches and receive a token | Untrusted contributors |
| Developer → gate configuration | Suppression attributes, lowered lints, thread flags could green the gate | Build integrity |
| CI smoke → network | Release binary started on the runner | Local loopback only |

---

## Threat Register

| Threat ID | Category | Component | Severity | Disposition | Mitigation | Status |
|-----------|----------|-----------|----------|-------------|------------|--------|
| T-04-01 | Tampering | mutable action refs (`checkout@v7`, `rust-toolchain@1.99.1` branch, `rust-cache@v2`) | medium | accept | D-02 (SHA pinning deferred to OPS-03); bounded by `permissions: contents: read` and no secrets | closed |
| T-04-02 | Tampering | `run:` script injection via event context | medium | mitigate | `ci.yml` has 0 `github.event` expressions; guard (g) regex fails on `${{ github.event.` (ci-guards.sh:105), planted case caught | closed |
| T-04-03 | Elevation of Privilege | `GITHUB_TOKEN` scope | medium | mitigate | `permissions: contents: read` (ci.yml:8–9), asserted by `check-workflow.py`; planted removal caught | closed |
| T-04-04 | Tampering | gate greened by suppression / config | high | mitigate | Guards (a) attributes, (b) ignores, (c) lint levels, (g) workflow keys/flags, (h) `.cargo/config*`; 8/8 ok on tree; 32/32 planted violations caught | closed |
| T-04-05 | Elevation of Privilege | unprotected V8 platform in release binary | medium | mitigate | Guard (d) checks normal-edge features via `cargo tree`; planted case caught | closed |
| T-04-06 | Tampering | rust-cache poisoning from fork PRs | low | accept | GitHub scopes PR cache writes to the PR ref; nothing deployed from CI | closed |
| T-04-07 | Tampering | V8 prebuilt download in v8 build.rs (no content hash) | low | accept | HTTPS from denoland/rusty_v8 releases (same trust as local builds); `RUSTY_V8_MIRROR`/`ARCHIVE` available | closed |
| T-04-08 | Denial of Service | smoke server left running / reachable | low | mitigate | Binds 127.0.0.1 only (ci-smoke.sh:38); `trap cleanup EXIT` (ci-smoke.sh:29) | closed |
| T-04-09 | Spoofing | smoke talking to a foreign service on a colliding port | low | mitigate | Free port chosen via python socket (ci-smoke.sh:20); `/dev/tcp` probe refuses an occupied port (ci-smoke.sh:34) | closed |
| T-04-10 | Repudiation | fabricated green remote x86_64 result | medium | mitigate | D-09 item recorded `result: [pending]`; no `refs/remotes/origin/modernize-toolchain`; nothing pushed | closed |
| T-04-11 | Denial of Service | release binary V8 platform init on PKU x86_64 runners | medium | transfer | Detection in-repo (smoke fails fast with `status 139`); remediation (D-07 fallback) transferred to the operator's pending remote-run UAT | closed |
| T-04-SC | Tampering | package / binary installs (phase-wide) | high | mitigate | No crate, pip package or binary added; actionlint not downloaded; `Cargo.toml`/`Cargo.lock` unchanged vs 8924cac | closed |

*Status: open · closed · open — below high threshold (non-blocking)*
*Severity: critical > high > medium > low — only open threats at or above workflow.security_block_on count toward threats_open*
*Disposition: mitigate (implementation required) · accept (documented risk) · transfer (third-party)*

---

## Accepted Risks Log

| Risk ID | Threat Ref | Rationale | Accepted By | Date |
|---------|------------|-----------|-------------|------|
| AR-04-01 | T-04-01 | Tag/branch action refs per D-02; SHA pinning deferred to v2 OPS-03; read-only token, no secrets | plan-time threat model (04-01), autonomous run | 2026-10-20 |
| AR-04-02 | T-04-06 | GitHub's cache scoping for PR refs; no deployment from CI | plan-time threat model (04-01), autonomous run | 2026-10-20 |
| AR-04-03 | T-04-07 | V8 prebuilt fetched over HTTPS from the upstream release, same trust as local builds | plan-time threat model (04-01), autonomous run | 2026-10-20 |
| TR-04-01 | T-04-11 | Transfer: x86 PKU crash remediation (move `unsafe_use_unprotected_platform` to `[dependencies]`) is part of the operator's remote-run UAT (04-UAT.md) | plan-time threat model (04-02), autonomous run | 2026-10-20 |

*Accepted risks do not resurface in future audit runs.*

---

## Security Audit Trail

| Audit Date | Threats Total | Closed | Open | Run By |
|------------|---------------|--------|------|--------|
| 2026-10-20 | 12 | 12 | 0 | gsd-secure-phase (orchestrator, L1 grep + guard self-test + smoke short-circuit) |

---

## Sign-Off

- [x] All threats have a disposition (mitigate / accept / transfer)
- [x] Accepted risks documented in Accepted Risks Log
- [x] `threats_open: 0` confirmed
- [x] `status: verified` set in frontmatter

**Approval:** verified 2026-10-20
