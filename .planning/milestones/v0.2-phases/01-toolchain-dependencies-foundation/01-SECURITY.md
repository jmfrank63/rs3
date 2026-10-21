---
phase: "1"
slug: "toolchain-dependencies-foundation"
status: verified
# threats_open = count of OPEN threats at or above workflow.security_block_on severity (the blocking gate)
threats_open: 0
asvs_level: 1
created: "2026-10-20"
---

# Phase 1 — Security

> Per-phase security contract: threat register, accepted risks, and audit trail.
> Register authored at plan time (01-01/02/03 `<threat_model>`); verified at ASVS L1 (grep + automated gate depth) against merged tree `d865917`.

---

## Trust Boundaries

| Boundary | Description | Data Crossing |
|----------|-------------|---------------|
| crates.io / GitHub releases → build | Dependency sources and the V8 prebuilt static lib fetched by the `v8` build script | Build inputs (supply chain) |
| process environment / dotenv file → config | `SERVER.HOST` / `SERVER.PORT` read once at startup | Operator config (low sensitivity) |
| HTTP client → actix handlers | Untrusted bodies and path segments reach `/insert`, `/key/{key}`, `/list`, `/ip`, `PATCH` | Untrusted user input |
| test harness → process state | Parallel libtest threads share one process | Test-only |

---

## Threat Register

| Threat ID | Category | Component | Severity | Disposition | Mitigation | Status |
|-----------|----------|-----------|----------|-------------|------------|--------|
| T-01-01 | Tampering | Cargo.toml / Cargo.lock | high | mitigate | Released crates only; validation rows DEPS-01 green (no git/path/branch, 0 active pre-release, no `git+` source); `v8x` lock entry inert and disclosed (D-21) | closed |
| T-01-02 | Tampering | V8 linkage | medium | mitigate | DEPS-02 green: one `name = "v8"` lock entry, no duplicate root, `cargo tree -i v8` via `deno_v8`, no direct `v8`/`rusty_v8` dep | closed |
| T-01-03 | Tampering | v8 build.rs prebuilt download | medium | accept | See accepted risk AR-01 | closed |
| T-01-04 | Denial of Service | src/config.rs / process env | medium | mitigate | `#![forbid(unsafe_code)]` in src/lib.rs and `[lints.rust] unsafe_code = "forbid"` in Cargo.toml; `Settings::load` called once in main | closed |
| T-01-05 | Denial of Service | src/http/kv.rs handler unwraps | medium | accept | See AR-02 (Phase 2 ROBU-01/02) | closed |
| T-01-06 | Information Disclosure | startup error output | low | accept | See AR-03 | closed |
| T-01-07 | Spoofing | bind address selection (src/main.rs) | medium | mitigate | No default host/port; missing/invalid config exits 1 (ARCH-04 smokes green) | closed |
| T-01-08 | Denial of Service | src/http/kv.rs delete_key | medium | accept | See AR-04 (Phase 2 ROBU-01) | closed |
| T-01-09 | Tampering | src/store/legacy.rs format_list | low | accept | See AR-05 (Phase 2 HTTP-04) | closed |
| T-01-10 | Denial of Service | src/store/mod.rs remove/list | low | mitigate | Per-call `pin()` guards, owned return values; no `static`/`LazyLock` in src/store | closed |
| T-01-11 | Information Disclosure | GET /ip | low | accept | See AR-06 | closed |
| T-01-12 | Denial of Service | src/config.rs tests | medium | mitigate | Tests use `Settings::from_map`; env mutation uncompilable under `forbid(unsafe_code)` | closed |
| T-01-13 | Repudiation | TOOL-03 gate integrity | medium | mitigate | Exactly 2 sanctioned `#[ignore]` attributes; no `#[allow(..)]` / `#![allow(..)]` in src; clippy `-D warnings` green | closed |
| T-01-14 | Denial of Service | src/main.rs startup paths | medium | mitigate | Invalid port, busy port, missing config each exit 1 with an `error:` line, no panic (validation smokes green) | closed |
| T-01-SC | Tampering | cargo dependency installs (phase-wide) | high | mitigate | Only 01-01 added/resolved packages (legitimacy audit in 01-RESEARCH.md; `deno_error = "0.7"` hand-pinned; lock regenerated and gated); Cargo.toml/Cargo.lock unchanged by 01-02/01-03 (`git diff b807333 d865917 -- Cargo.toml Cargo.lock` empty) | closed |

*Status: open · closed · open — below high threshold (non-blocking)*
*Severity: critical > high > medium > low — only open threats at or above workflow.security_block_on count toward threats_open*
*Disposition: mitigate (implementation required) · accept (documented risk) · transfer (third-party)*

---

## Accepted Risks Log

| Risk ID | Threat Ref | Rationale | Accepted By | Date |
|---------|------------|-----------|-------------|------|
| AR-01 | T-01-03 | V8 prebuilt fetched over HTTPS from denoland/rusty_v8 releases for the version pinned in Cargo.lock; `RUSTY_V8_ARCHIVE` allows pinned/offline builds; not a runtime trust boundary | Plan 01-01 threat model (auto mode) | 2026-10-20 |
| AR-02 | T-01-05 | Legacy handler unwraps on malformed `/insert` / missing key kept per locked D-10; actix restarts the worker; replaced by 400/404/422 in Phase 2 (ROBU-01/02) | Plan 01-01 threat model (auto mode) | 2026-10-20 |
| AR-03 | T-01-06 | Config errors name only the `SERVER.*` key and offending value; no environment dump | Plan 01-01 threat model (auto mode) | 2026-10-20 |
| AR-04 | T-01-08 | DELETE of a missing key still panics as legacy did (D-10); Phase 2 ROBU-01 maps it to 404 | Plan 01-02 threat model (auto mode) | 2026-10-20 |
| AR-05 | T-01-09 | `/list` keeps the legacy unescaped shape for byte parity (D-08); Phase 2 HTTP-04 adds serde_json escaping | Plan 01-02 threat model (auto mode) | 2026-10-20 |
| AR-06 | T-01-11 | `GET /ip` returning the configured host is required legacy behaviour (HTTP-02) | Plan 01-03 threat model (auto mode) | 2026-10-20 |

*Accepted risks do not resurface in future audit runs.*

---

## Security Audit Trail

| Audit Date | Threats Total | Closed | Open | Run By |
|------------|---------------|--------|------|--------|
| 2026-10-20 | 15 | 15 | 0 | /gsd-secure-phase (orchestrator, ASVS L1 short-circuit) |

---

## Sign-Off

- [x] All threats have a disposition (mitigate / accept / transfer)
- [x] Accepted risks documented in Accepted Risks Log
- [x] `threats_open: 0` confirmed
- [x] `status: verified` set in frontmatter

**Approval:** verified 2026-10-20
