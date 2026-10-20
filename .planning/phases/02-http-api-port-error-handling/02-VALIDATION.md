---
phase: "02"
slug: "http-api-port-error-handling"
# status lifecycle: draft (seeded by plan-phase) → validated (set by validate-phase §6)
# audit-milestone §5.5 distinguishes NOT-VALIDATED (draft) from PARTIAL (validated + nyquist_compliant: false) (#2117)
status: draft
nyquist_compliant: false
wave_0_complete: false
created: "2026-10-20"
---

# Phase 02 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.
> Source: `02-RESEARCH.md` § Validation Architecture (requirement → test map, Wave 0 gaps).

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust libtest + `#[actix_web::test]` (actix-web 4.17.0 test utilities) |
| **Config file** | none (Cargo defaults); new `clippy.toml` (Wave 0) |
| **Quick run command** | `cargo test --lib` |
| **Full suite command** | `cargo build --locked && cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check` |
| **Estimated runtime** | ~5 seconds warm (quick), ~60 seconds warm (full gate) |

---

## Sampling Rate

- **After every task commit:** Run `cargo test --lib` + `cargo clippy --all-targets -- -D warnings`
- **After every plan wave:** Run the full suite command + the static greps from the research map
- **Before `/gsd-verify-work`:** Full suite green, plus `env -i PATH="$PATH" HOME="$HOME" cargo test`, `cargo test -- --test-threads=1`, a 5× repeat run, and all live smokes
- **Max feedback latency:** 60 seconds

---

## Per-Task Verification Map

Filled in by the planner and executor as tasks are created. The requirement → command map is in `02-RESEARCH.md` § Validation Architecture → Phase Requirements → Test Map (HTTP-01..06, HTTP-08, ROBU-01, ROBU-02, ARCH-02, ARCH-03, TEST-01, TEST-02, D-13..D-17).

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| 02-01-01 | 01 | 1 | ARCH-02, ARCH-03, ROBU-01, HTTP-02, HTTP-05, HTTP-08 | T-02-01, T-02-02, T-02-05 | One AppState, no global; missing key gives a JSON 404 instead of a panic; banner after bind | unit + live smoke | `cargo test --lib http::` + live smokes on 18460 (insert/get/404) and 18462 (bind failure, no banner) | ✅ (tests re-pointed; `get_missing_is_404_json` new) | ⬜ pending |
| 02-01-02 | 01 | 1 | ROBU-01, HTTP-01, HTTP-03, HTTP-06, ARCH-02, TEST-01, TEST-02 | T-02-01, T-02-02 | PATCH 404/501; two apps share state; goldens; env-free, serial and 5x runs | unit (TDD) + live smoke | test-name list check + `cargo test --lib http::`; multi-worker smoke on 18461; `env -i` / `--test-threads=1` / 5x repeat | ❌ Wave 0 (8 new tests) | ⬜ pending |
| 02-02-01 | 02 | 1 | ARCH-03, TEST-02 | T-02-07, T-02-08 | No env mutation; dotenv errors are config errors; never half-loaded | unit + live smoke | `cargo test --lib config::` (from_sources tests); env-only smoke on 18463; Phase 1 no-config smoke; human-check: malformed dotenv file | ❌ Wave 0 (4 new tests) | ⬜ pending |
| 02-02-02 | 02 | 1 | TEST-02, ARCH-03 | T-02-09, T-02-11 | server.* allow-list; env wins, first wins in file; non-UTF-8 skipped; BOM pinned | unit (TDD) + live smoke | `cargo test --lib config::`; stray SERVER=x smoke on 18464; human-check: valid file and env override | ❌ Wave 0 (7 new tests) | ⬜ pending |
| 02-03-01 | 03 | 2 | ROBU-02, HTTP-03, HTTP-08 | T-02-15, T-02-16 | Malformed, empty, deep or non-UTF-8 body gives 400 malformed_json; any content type accepted | unit + live smoke | test-name list check + `cargo test --lib`; smoke on 18465 | partial (`parse_rejects_malformed_json` rewritten; 10 new) | ⬜ pending |
| 02-03-02 | 03 | 2 | ROBU-02, HTTP-03 | T-02-13, T-02-14, T-02-17 | 422 writes nothing; {} no-op; 413 oversize; insert_all; unwrap/expect denied outside tests | unit + static + live smoke | test-name list check + `cargo test --lib`; lint/clippy.toml gate + `cargo clippy --all-targets -- -D warnings`; smoke on 18466 | ❌ Wave 0 (10 new tests, clippy.toml) | ⬜ pending |
| 02-03-03 | 03 | 2 | HTTP-04, TEST-01, TEST-02, HTTP-08 | T-02-12 | Escaped /list round-trips and cannot be forged; full phase gate | unit (TDD) + full gate + live smokes | test-name list check; TOOL-03 four commands; `env -i` / serial / 5x; combined static + ARCH-01 gate; smokes on 18467 (escaping) and 18468 (all-routes goldens) | ❌ Wave 0 (7 new tests; quirk test replaced) | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `clippy.toml` with `allow-unwrap-in-tests = true` and `allow-expect-in-tests = true`, in the same commit as `[lints.clippy] unwrap_used = "deny"` and `expect_used = "deny"`
- [ ] `test_state()` helper (per-test `AppState` built in code)
- [ ] New tests: the 404 set, the 400 set, the 422 set (assert the store is unchanged), the `{}` no-op, the escaping round-trip (codec and HTTP), the two-app shared state, and the config merge unit tests
- [ ] Rewrites: `test_patch_is_501` (insert the key first), `parse_rejects_malformed_json` (expect a malformed variant), remove `format_list_keeps_legacy_replace_quirk`, re-point the parked PATCH tests at `test_state()`

---

## Manual-Only Verifications

All phase behaviors have automated verification. Live-binary smokes (curl / `env -i`) run in the executor's shell.

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 60s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
