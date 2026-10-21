---
phase: "03"
slug: "javascript-runtime-port"
status: verified
# threats_open = count of OPEN threats at or above workflow.security_block_on severity (the blocking gate)
threats_open: 0
asvs_level: 1
created: "2026-10-20"
---

# Phase 03 — Security

> Per-phase security contract: threat register, accepted risks, and audit trail.
> The register was authored at plan time (`<threat_model>` in 03-01/02/03) and verified at ASVS L1 (grep plus automated test-gate depth) against tree `e6b58db`. The short-circuit rule applied (threats_open 0, register authored at plan time, ASVS 1), so no auditor was spawned.

---

## Trust Boundaries

| Boundary | Description | Data Crossing |
|----------|-------------|---------------|
| HTTP client → PATCH handler | Any client can PATCH any key; the stored value is executed as JavaScript | Untrusted code (stored by any client) |
| JS script → store ops | Scripts call `op_rs3_*` with arbitrary values; ops run Rust on the JS thread | Untrusted arguments |
| JS script → deno_core built-in ops | Bare deno_core exposes internal ops on `Deno.core.ops` (e.g. `op_panic`) | Untrusted calls into host |
| actix workers → JS pool | Jobs cross a bounded queue to `rs3-js-*` threads; replies over oneshot | In-process |
| build → release binary | Test-only V8 platform feature must not reach production | Build integrity |

---

## Threat Register

| Threat ID | Category | Component | Severity | Disposition | Mitigation | Status |
|-----------|----------|-----------|----------|-------------|------------|--------|
| T-03-01 | Denial of Service | built-in `op_panic` (process abort) | high | mitigate | `rs3_ext` middleware `"op_panic" => op.disable()` (ops.rs:151); `op_panic_is_disabled_and_catchable`, `uncaught_op_panic_is_a_script_error` | closed |
| T-03-02 | Denial of Service | op bodies panicking on untrusted input | high | mitigate | All ops return `Result<String, JsErrorBox>` (ops.rs:87–128); no unwrap/expect in non-test code; `JsRuntime::try_new` only (runtime.rs:18); op error tests | closed |
| T-03-03 | Denial of Service | nested op calls without reentrant flag | medium | mitigate | `#[op2(reentrant)]` on `op_rs3_patch` (ops.rs:126); nested tests run in debug profile | closed |
| T-03-04 | Denial of Service | panicking job kills a worker | medium | mitigate | `catch_unwind` per job (executor.rs:137); `worker_survives_panicking_jobs` | closed |
| T-03-05 | Information Disclosure | state shared across stores/requests | medium | mitigate | Per-job `ExecCtx` store + fresh runtime; `stores_on_one_executor_are_isolated`, `every_job_gets_a_fresh_runtime` | closed |
| T-03-06 | Denial of Service | unbounded `op_rs3_patch` self-recursion (~800 MB / 2.7 s) | medium | accept | Legacy crashed; now a catchable error. Phase 5 depth limit (ROBU-05); disclosed in `docs/js-ops.md`; never exercised | closed |
| T-03-07 | Elevation of Privilege | unprotected V8 platform in production | medium | mitigate | Feature only on `[dev-dependencies]` deno_core (Cargo.toml:25–28); normal-edge feature set excludes it (`cargo tree -e features,normal -i deno_core` → 0) | closed |
| T-03-08 | Tampering | `requests/*.txt` payload escaping | low | mitigate | All `.txt` payloads parse as JSON; decoded scripts checked against node stub (03-02) | closed |
| T-03-09 | Repudiation | `docs/js-ops.md` drift from implementation | low | mitigate | Written from as-built 03-01 semantics; 03-03 verification greps exact messages | closed |
| T-03-10 | Denial of Service | users storing self-patching / non-terminating scripts pre-Phase 5 | medium | mitigate | `docs/js-ops.md` discloses 800 MB nesting cost and `while(true){}` worker pinning, with the Phase 5 fix | closed |
| T-03-11 | Information Disclosure | stack traces / internals in 422/500 bodies | medium | mitigate | 422 = `exception_message` only (runtime.rs:27); 500 = fixed `"internal error"` (error.rs:23); exact-body tests | closed |
| T-03-12 | Denial of Service | actix workers running V8 | medium | mitigate | No `deno_core` in `src/http/`; handler only awaits `JsExecutor::execute`; 32-parallel PATCH smoke | closed |
| T-03-13 | Denial of Service | request hanging when pool is down | medium | mitigate | Closed queue / dropped reply → `ExecError::Unavailable` (executor.rs:100–101) → 500; `test_patch_executor_down_is_500_internal` | closed |
| T-03-14 | Denial of Service | `op_panic` / script error over HTTP aborting process | high | mitigate | Middleware + `test_patch_op_panic_is_422_and_server_keeps_serving` + live 18501 smoke on protected-platform binary | closed |
| T-03-15 | Denial of Service | non-terminating / self-patching scripts saturating the pool pre-Phase 5 | medium | accept | Bounded pool and queue; other routes unaffected (no V8 on actix workers). Phase 5 timeout (ROBU-06) + depth limit (ROBU-05); disclosed in docs | closed |
| T-03-16 | Elevation of Privilege | unprotected platform reaching production binary | medium | mitigate | Phase gate re-checks normal-edge features; live smokes ran the protected-platform binary | closed |
| T-03-SC | Tampering | cargo dependency installs (phase-wide) | high | mitigate | Only `tokio` added as direct dep (already locked transitively at 1.54.0); `Cargo.lock` diff vs 9209f76 is exactly `+ "tokio",`; `--locked` builds | closed |

*Status: open · closed · open — below high threshold (non-blocking)*
*Severity: critical > high > medium > low — only open threats at or above workflow.security_block_on count toward threats_open*
*Disposition: mitigate (implementation required) · accept (documented risk) · transfer (third-party)*

---

## Accepted Risks Log

| Risk ID | Threat Ref | Rationale | Accepted By | Date |
|---------|------------|-----------|-------------|------|
| AR-03-01 | T-03-06 | Unbounded nested PATCH now ends in a catchable error (legacy crashed the process); costs ~800 MB / 2.7 s per request until the Phase 5 depth limit; disclosed in docs | plan-time threat model (03-01), autonomous run | 2026-10-20 |
| AR-03-02 | T-03-15 | Non-terminating / self-patching scripts can occupy JS workers until the Phase 5 timeout and depth limit; HTTP routes stay responsive because actix workers never run V8 | plan-time threat model (03-03), autonomous run | 2026-10-20 |

*Accepted risks do not resurface in future audit runs.*

---

## Security Audit Trail

| Audit Date | Threats Total | Closed | Open | Run By |
|------------|---------------|--------|------|--------|
| 2026-10-20 | 17 | 17 | 0 | gsd-secure-phase (orchestrator, L1 grep + test-gate short-circuit) |

---

## Sign-Off

- [x] All threats have a disposition (mitigate / accept / transfer)
- [x] Accepted risks documented in Accepted Risks Log
- [x] `threats_open: 0` confirmed
- [x] `status: verified` set in frontmatter

**Approval:** verified 2026-10-20
