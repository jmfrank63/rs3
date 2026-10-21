---
phase: "03"
slug: "javascript-runtime-port"
# status lifecycle: draft (seeded by plan-phase) → validated (set by validate-phase §6)
# audit-milestone §5.5 distinguishes NOT-VALIDATED (draft) from PARTIAL (validated + nyquist_compliant: false) (#2117)
status: validated
nyquist_compliant: true
wave_0_complete: true
created: "2026-10-20"
---

# Phase 03 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.
> Source: `03-RESEARCH.md` § Validation Architecture (requirement → test map, live smokes, Wave 0 gaps).

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust libtest + `#[actix_web::test]` (actix-web 4.17.0); in-crate `#[cfg(test)]` modules; shared `#[cfg(test)]` JS test executor |
| **Config file** | `clippy.toml` (test allowances), `Cargo.toml` `[lints]`; new `[dev-dependencies]` deno_core `unsafe_use_unprotected_platform` feature |
| **Quick run command** | `cargo test --lib js:: && cargo test --lib http::` |
| **Full suite command** | `cargo build --locked && cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check` |
| **Estimated runtime** | ~10 seconds warm (quick), ~90 seconds warm (full gate) |

---

## Sampling Rate

- **After every task commit:** Run `cargo test --lib` + `cargo clippy --all-targets -- -D warnings`
- **After every plan wave:** Run the full suite command + the grep gates from the research map
- **Before `/gsd-verify-work`:** Full suite green + `env -i PATH="$PATH" HOME="$HOME" cargo test` + `cargo test -- --test-threads=1` + 5× repeat + live smokes (ports 18500–18519)
- **Max feedback latency:** 90 seconds

---

## Per-Task Verification Map

Filled by the planner/executor as tasks are created; requirement → command map lives in `03-RESEARCH.md` § Validation Architecture (HTTP-07, JSRT-01..06, ROBU-03, ROBU-04, TEST-03, TEST-05, TEST-06, D-07, D-09, D-11, D-12, D-17).

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| 03-01-01 | 01 | 1 | JSRT-01, JSRT-02 (patch), ROBU-04 | T-03 op_panic abort | Nested reentrant patch runs inner script; op_panic disabled (catchable) | unit (TDD) | `cargo test --lib js::ops::tests::nested_patch && cargo test --lib op_panic` | ✅ | ✅ green |
| 03-01-02 | 01 | 1 | JSRT-04 | worker panic isolation | Jobs run on `rs3-js-*` threads; worker survives panicking job; closed executor → unavailable | unit | `cargo test --lib js::executor` | ✅ | ✅ green |
| 03-01-03 | 01 | 1 | JSRT-02, JSRT-03, ROBU-04 | partial insert / op errors | list/insert/delete over shared codec; missing key → catchable `Error`; bad insert → `TypeError`, nothing written; object/symbol args → `TypeError` | unit (TDD) | `cargo test --lib js::ops` | ✅ | ✅ green |
| 03-02-01 | 02 | 1 | JSRT-05, JSRT-06 | — | fac migrated (js + txt); `docs/js-ops.md` mapping incl. "key not found" | static + node stub | `test -f docs/js-ops.md && grep -q 'key not found' docs/js-ops.md` | ✅ | ✅ green |
| 03-02-02 | 02 | 1 | JSRT-05 | — | sum/bin migrated, still compute 10 / tree | static + node stub | `! grep -rnE 'opSync\|Deno\.core\.ops\(\)' requests src` | ✅ | ✅ green |
| 03-02-03 | 02 | 1 | JSRT-05 | — | store, .http walkthroughs, example.js migrated; .txt payloads valid JSON | static | `for f in requests/*.txt; do python3 -c 'import json,sys;json.load(open(sys.argv[1]))' "$f"; done` | ✅ | ✅ green |
| 03-03-01 | 03 | 2 | HTTP-07, JSRT-04 | — | PATCH runs on pool; main order init_platform → JsExecutor::start → HttpServer::new | HTTP + static + live | `cargo test --lib test_patch_runs_script_200` + main-order awk gate + live `"120"` smoke | ✅ | ✅ green |
| 03-03-02 | 03 | 2 | ROBU-03, ROBU-04 | DoS via script error | throw/syntax/uncaught-op-error → 422 `js_error`; op_panic over HTTP → 422, server keeps serving; closed pool → 500 `internal` | HTTP (TDD) | `cargo test --lib test_patch_throw_is_422_js_error test_patch_executor_down_is_500_internal` | ✅ | ✅ green |
| 03-03-03 | 03 | 2 | TEST-03, TEST-05, TEST-06, JSRT-05 | — | fac/sum/bin/store fixtures; PATCH op_rs3_list == GET /list; all 7 routes byte-exact; IN-04/IN-05 | HTTP + full gate | `cargo test --lib fixture_ && cargo test --lib test_patch_body_is_listing` + TOOL-03 gate | ✅ | ✅ green |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [x] `src/js/*` test modules (op semantics, op errors, nested patch, op_panic guard, thread names, worker panic survival)
- [x] `#[cfg(test)]` shared JS test executor wired into `test_state()`; `#[cfg(test)]` closed-executor constructor for the 500 test
- [x] `Cargo.toml`: direct `tokio` dependency (rt, sync) + dev-dependency `deno_core` feature line; `Cargo.lock` stays on released versions
- [x] TDD RED evidence under `.planning/phases/03-javascript-runtime-port/tdd-evidence/`

---

## Manual-Only Verifications

All phase behaviors have automated verification (x86_64 platform behaviour is verified by Phase 4 CI).

---

## Validation Sign-Off

- [x] All tasks have `<automated>` verify or Wave 0 dependencies
- [x] Sampling continuity: no 3 consecutive tasks without automated verify
- [x] Wave 0 covers all MISSING references
- [x] No watch-mode flags
- [x] Feedback latency < 90s
- [x] `nyquist_compliant: true` set in frontmatter

**Approval:** approved 2026-10-20 (validate-phase audit: 12/12 requirements COVERED; 154 tests green, plus env-free, serial and 5x runs and the static gates; x86_64 deferred to Phase 4 CI)

## Validation Audit 2026-10-20

| Metric | Count |
|---|---|
| Gaps found | 0 |
| Resolved | 0 |
| Escalated | 0 |
