---
phase: "05"
slug: "js-execution-limits"
status: verified
# threats_open = count of OPEN threats at or above workflow.security_block_on severity (the blocking gate)
threats_open: 0
asvs_level: 1
created: "2026-10-21"
---

# Phase 05 — Security

> Per-phase security contract: threat register, accepted risks, and audit trail.
> The register was authored at plan time (`<threat_model>` in 05-01/02/03) and verified at ASVS L1 (grep plus automated test-gate depth, 28 limit-specific tests, the release smoke) against tree `b25a576`. The short-circuit rule applied (threats_open 0, register authored at plan time, ASVS 1), so no auditor was spawned.

---

## Trust Boundaries

| Boundary | Description | Data Crossing |
|----------|-------------|---------------|
| HTTP client → PATCH | Any client can store and execute scripts (self-patching, cyclic, non-terminating) | Untrusted code |
| JS script → store ops after a stop | Scripts may catch a depth/timeout stop and continue | Untrusted control flow |
| Watchdog thread → JS isolates | A per-worker thread terminates isolates across jobs | In-process concurrency |
| Script output → server logs | `op_print` and error messages reach stdout/stderr | Untrusted text |

---

## Threat Register

| Threat ID | Category | Component | Severity | Disposition | Mitigation | Status |
|-----------|----------|-----------|----------|-------------|------------|--------|
| T-05-01 | Denial of Service | unbounded `op_rs3_patch` nesting | high | mitigate | Depth check in `op_rs3_patch` before store read / runtime creation, under `max_patch_depth` (ops.rs:30–52); 422 `js_depth_limit` tests (self, cycle, chain) | closed |
| T-05-02 | Tampering | writes after a caught depth stop | medium | mitigate | Shared per-job stop; `live_ctx` refuses every op (ops.rs:169); caught stop still classified 422 | closed |
| T-05-03 | Information Disclosure | keys in 422 body / log forging | low | mitigate | Body serialised by serde (`ErrorBody`, JSON-escaped); stderr lines escaped (Phase 3 WR-02) | closed |
| T-05-04 | Denial of Service | worker pinned by `while(true){}` | high | mitigate | Per-worker watchdog → `terminate_execution()` on every registered isolate (mod.rs:133–155); `timeout_infinite_loop_is_504…` | closed |
| T-05-05 | Denial of Service | termination cancelled via caught nested PATCH / throwing-op loop (research S4/S5) | high | mitigate | Ops go silent after timeout and never throw (ops.rs:171, 301); watchdog re-fires every 50 ms until disarmed; regression tests | closed |
| T-05-06 | Tampering | late watchdog hits the next job | medium | mitigate | Per-job `Arc<JobState>` with CAS `RUNNING → TIMED_OUT/FINISHED` (mod.rs:148–164); `late_watchdog_never_hits_next_job` | closed |
| T-05-07 | Denial of Service | client disconnect leaves runaway script | medium | mitigate | Watchdog armed worker-side at pickup, independent of the HTTP future | closed |
| T-05-08 | Denial of Service | stall in Rust op code that termination cannot interrupt | medium | mitigate | Backstop 504 at `timeout_ms + BACKSTOP_GRACE` (2 s) from pickup (executor.rs:78, 229); `backstop` test | closed |
| T-05-09 | Denial of Service | concurrent runaway scripts occupying all workers | medium | mitigate | Each holds a worker ≤ `timeout_ms`; queue drains; isolation tests (GET / and other PATCHes answered during a loop) | closed |
| T-05-10 | Denial of Service | heap exhaustion (`a.push` loop) aborting the process | medium | accept | Deferred to v2 JSHD-01 (heap limit + near-heap-limit callback); disclosed in docs/js-ops.md:220 | closed |
| T-05-11 | Repudiation | docs claiming rollback or wrong limits | low | mitigate | docs/js-ops.md documents writes-before-stop kept, exact defaults/bodies, heap and backstop residuals | closed |
| T-05-12 | Denial of Service | `op_print` on non-blocking pipes | low | mitigate | `write_all_retrying`/`flush_retrying` retry Interrupted/WouldBlock (ops.rs:367–377); `print_retries_would_block` | closed |
| T-05-13 | Denial of Service | tiny configured worker stack | low | mitigate | `effective_stack_bytes` floor 2 MiB (executor.rs:58); `stack_bytes_are_clamped` | closed |
| T-05-14 | Denial of Service | smoke leaves processes running / non-loopback | low | mitigate | 127.0.0.1 only; `trap cleanup EXIT` kills server and background loop request | closed |
| T-05-15 | Repudiation | fabricated remote x86_64 result | medium | mitigate | Remote run recorded `result: [pending]`; origin still at operator commit `fd8f43f`; nothing pushed | closed |
| T-05-SC | Tampering | package installs (phase-wide) | high | mitigate | No crate added; only tokio feature declarations already enabled in the graph; `Cargo.lock` byte-identical to 449aaf2; `--locked` builds | closed |

*Status: open · closed · open — below high threshold (non-blocking)*
*Severity: critical > high > medium > low — only open threats at or above workflow.security_block_on count toward threats_open*
*Disposition: mitigate (implementation required) · accept (documented risk) · transfer (third-party)*

---

## Accepted Risks Log

| Risk ID | Threat Ref | Rationale | Accepted By | Date |
|---------|------------|-----------|-------------|------|
| AR-05-01 | T-05-10 | V8 heap exhaustion can abort the process before the timeout; heap limit + near-heap-limit callback is v2 scope (JSHD-01) per CONTEXT deferred ideas; disclosed in docs | plan-time threat model (05-02), autonomous run | 2026-10-21 |

*Accepted risks do not resurface in future audit runs.*

---

## Security Audit Trail

| Audit Date | Threats Total | Closed | Open | Run By |
|------------|---------------|--------|------|--------|
| 2026-10-21 | 16 | 16 | 0 | gsd-secure-phase (orchestrator, L1 grep + test-gate short-circuit) |

---

## Sign-Off

- [x] All threats have a disposition (mitigate / accept / transfer)
- [x] Accepted risks documented in Accepted Risks Log
- [x] `threats_open: 0` confirmed
- [x] `status: verified` set in frontmatter

**Approval:** verified 2026-10-21
