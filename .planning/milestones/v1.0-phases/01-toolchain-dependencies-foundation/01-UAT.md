---
status: complete
phase: 01-toolchain-dependencies-foundation
source: [01-VERIFICATION.md]
started: 2026-10-20T17:15:00Z
updated: 2026-10-20T17:15:13.982Z
---

## Current Test

[testing complete]

## Tests

### 1. Dotenv-file config source (DEPS-04 / SC-4)
expected: Binary started via `env -i PATH=$PATH target/debug/rs3` from a directory containing a dotenv file (SERVER.HOST=127.0.0.1, SERVER.PORT=18461) serves `/ip` with 200 and body "{'IP' : '127.0.0.1'}".
result: pass
source: operator
evidence: "Verified manually by the user's operator: binary run under `env -i` from a directory containing a .env with SERVER.HOST=127.0.0.1 / SERVER.PORT=18461 served GET /ip -> 200 \"{'IP' : '127.0.0.1'}\"."

### 2. rustup auto-install of the 1.99.1 pin is idempotent under concurrency (TOOL-01, backstop)
expected: On a machine without 1.99.1 installed, `cargo build & cargo clippy --all-targets & wait` in the repo — both auto-install or reuse 1.99.1 without a rustup error, both report 1.99.1, one prints "Blocking waiting for file lock".
result: skipped
source: operator
reason: "Accepted risk — not worth uninstalling the toolchain to exercise the auto-install clause. Toolchain selection and target-lock serialisation were already observed directly by the verifier; CI (Phase 4) installs the pin explicitly via dtolnay/rust-toolchain@1.99.1."

### 3. v8x pre-release lock entry disclosure (01-01 judgment-tier prohibition, D-21)
expected: 01-01-SUMMARY.md (lines ~63 and ~197-204) discloses the never-built optional `v8x 149.4.0-rc.4` Cargo.lock entry and states DEPS-01 is verified on the manifest + active build graph. Verifier verdict: satisfied — confirm adequate.
result: pass
source: operator
evidence: "Operator confirmed the disclosure is adequate."

### 4. No gate-greening by suppression (01-03 test-tier prohibition)
expected: Confirm the verifier's grep evidence (no allow/cfg_attr in src/ or Cargo.toml, exactly two PATCH #[ignore]s, no weakened legacy assertions) is sufficient, or request a wired CI grep check in Phase 4.
result: pass
source: operator
evidence: "Operator confirmed the verifier's grep evidence is sufficient for Phase 1, and requested a wired CI grep check for new #[allow] / #[cfg_attr(.., allow(..))] attributes in Phase 4 (see Deferred Follow-Ups)."

## Summary

total: 4
passed: 3
issues: 0
pending: 0
skipped: 1
blocked: 0

## Gaps

[none]

## Deferred Follow-Ups

- test: 4
  idea: "Add a CI grep check that fails on new #[allow(...)] / #[cfg_attr(..., allow(...))] attributes (suppression guard) — Phase 4 (CI Gate on x86_64)."
  deferred_at: 2026-10-20
  target_phase: 4
- test: review
  idea: "Address Phase 1 code-review advisories WR-01 (malformed .env half-loaded, error swallowed) and WR-02 (dotenvy mutates the process env via set_var under a safe API) — Phase 2 (HTTP API Port & Error Handling)."
  deferred_at: 2026-10-20
  target_phase: 2
