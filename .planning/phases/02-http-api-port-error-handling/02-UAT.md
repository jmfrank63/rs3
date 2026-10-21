---
status: testing
phase: 02-http-api-port-error-handling
source: [02-VERIFICATION.md]
started: 2026-10-20T19:35:00Z
updated: 2026-10-21T09:45:00Z
---

## Current Test

number: 2
name: Valid dotenv file with stray keys configures /ip
expected: |
  Binary started under `env -i PATH="$PATH"` from a directory whose dotenv file holds SERVER.HOST=127.0.0.1,
  SERVER.PORT=18469, SERVER=x and OTHER=1 answers `curl -s http://127.0.0.1:18469/ip` with 200 and
  body `"{'IP' : '127.0.0.1'}"`.
awaiting: user response

## Tests

### 1. Malformed dotenv file stops startup (live binary)
expected: In an empty directory under /home/t3code, create a .env with `SERVER.HOST=127.0.0.1` and the broken line `SERVER.PORT 5000`, then run `env -i PATH="$PATH" /home/t3code/rs3/Projects/rs3/target/debug/rs3` from it. Expect exit code 1, exactly one stderr line starting `error: invalid configuration: .env: Error parsing line:`, no startup banner, no panic.
result: pass
source: operator
evidence: "Operator run reported 2026-10-21: binary under `env -i` in a directory whose .env contained the broken line `SERVER.PORT 5000`. Output was exactly one line, `error: invalid configuration: .env: Error parsing line: 'SERVER.PORT 5000', error at line index: 12`, exit 1, no banner, no panic."

### 2. Valid dotenv file with stray keys configures /ip
expected: Use a .env with `SERVER.HOST=127.0.0.1`, `SERVER.PORT=18469`, `SERVER=x` and `OTHER=1`. Run the binary under `env -i PATH="$PATH"`, then `curl -s http://127.0.0.1:18469/ip`. Expect 200 with body `"{'IP' : '127.0.0.1'}"`.
result: [pending]

### 3. Process env overrides the dotenv file
expected: Use the same file but with the host line `SERVER.HOST=10.255.255.1`. Start with `env -i PATH="$PATH" SERVER.HOST=127.0.0.1 .../rs3` and curl /ip on the file's port. /ip reports 127.0.0.1 (the environment wins over the file).
result: [pending]

### 4. No gate-greening by suppression (test-tier prohibition, all three plans)
expected: Review `git diff 0928a96..HEAD -- src Cargo.toml clippy.toml`. Expect no suppression attributes, unwrap_used/expect_used still at deny, exactly two #[ignore] (http::tests::test_patch_is_ok and test_patch_body_is_listing), and the legacy goldens intact. The verifier's grep at this commit found the tree clean. Phase 4 adds a wired CI grep for this (operator directive from Phase 1 UAT).
result: pass
source: operator + automated
evidence: "Covered on the current tree by the operator's re-run reported 2026-10-21 under 03-UAT item 1 (no allow/expect/cfg_attr/#[ignore] in src/), and by guards (a) and (c) in scripts/ci-guards.sh, which wire this check into CI (green in run 39527188207 at 40afeb7; src, Cargo.toml and scripts are unchanged since). A re-run of the grep on 2026-10-21 found 0 hits, and unwrap_used/expect_used are still deny. The two PATCH #[ignore]s this item allowed were removed in Phase 3, so none remain."

## Summary

total: 4
passed: 2
issues: 0
pending: 2
skipped: 0
blocked: 0

## Gaps
