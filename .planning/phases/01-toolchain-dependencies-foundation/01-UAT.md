---
status: testing
phase: 01-toolchain-dependencies-foundation
source: [01-VERIFICATION.md]
started: 2026-10-20T17:15:00Z
updated: 2026-10-20T17:15:00Z
---

## Current Test

number: 1
name: Dotenv-file config source (DEPS-04 / SC-4)
expected: |
  Run the binary from a directory holding a dotenv file with SERVER.HOST=127.0.0.1 and SERVER.PORT=18461,
  under `env -i PATH=$PATH`, then `curl http://127.0.0.1:18461/ip` → 200 with body "{'IP' : '127.0.0.1'}";
  the server starts without SERVER.* in the process environment.
awaiting: user response

## Tests

### 1. Dotenv-file config source (DEPS-04 / SC-4)
expected: Binary started via `env -i PATH=$PATH target/debug/rs3` from a directory containing a dotenv file (SERVER.HOST=127.0.0.1, SERVER.PORT=18461) serves `/ip` with 200 and body "{'IP' : '127.0.0.1'}".
result: [pending]

### 2. rustup auto-install of the 1.99.1 pin is idempotent under concurrency (TOOL-01, backstop)
expected: On a machine without 1.99.1 installed, `cargo build & cargo clippy --all-targets & wait` in the repo — both auto-install or reuse 1.99.1 without a rustup error, both report 1.99.1, one prints "Blocking waiting for file lock".
result: [pending]

### 3. v8x pre-release lock entry disclosure (01-01 judgment-tier prohibition, D-21)
expected: 01-01-SUMMARY.md (lines ~63 and ~197-204) discloses the never-built optional `v8x 149.4.0-rc.4` Cargo.lock entry and states DEPS-01 is verified on the manifest + active build graph. Verifier verdict: satisfied — confirm adequate.
result: [pending]

### 4. No gate-greening by suppression (01-03 test-tier prohibition)
expected: Confirm the verifier's grep evidence (no allow/cfg_attr in src/ or Cargo.toml, exactly two PATCH #[ignore]s, no weakened legacy assertions) is sufficient, or request a wired CI grep check in Phase 4.
result: [pending]

## Summary

total: 4
passed: 0
issues: 0
pending: 4
skipped: 0
blocked: 0

## Gaps
