---
status: testing
phase: 02-http-api-port-error-handling
source: [02-VERIFICATION.md]
started: 2026-10-20T19:35:00Z
updated: 2026-10-20T19:35:00Z
---

## Current Test

number: 1
name: Malformed dotenv file stops startup (live binary)
expected: |
  Exit code 1, exactly one stderr line starting `error: invalid configuration: .env: Error parsing line:`,
  no startup banner, no panic.
awaiting: user response

## Tests

### 1. Malformed dotenv file stops startup (live binary)
expected: In an empty directory under /home/t3code, create a .env with `SERVER.HOST=127.0.0.1` and the broken line `SERVER.PORT 5000`, then run `env -i PATH="$PATH" /home/t3code/rs3/Projects/rs3/target/debug/rs3` from it. Expect exit code 1, exactly one stderr line starting `error: invalid configuration: .env: Error parsing line:`, no startup banner, no panic.
result: [pending]

### 2. Valid dotenv file with stray keys configures /ip
expected: Use a .env with `SERVER.HOST=127.0.0.1`, `SERVER.PORT=18469`, `SERVER=x` and `OTHER=1`. Run the binary under `env -i PATH="$PATH"`, then `curl -s http://127.0.0.1:18469/ip`. Expect 200 with body `"{'IP' : '127.0.0.1'}"`.
result: [pending]

### 3. Process env overrides the dotenv file
expected: Use the same file but with the host line `SERVER.HOST=10.255.255.1`. Start with `env -i PATH="$PATH" SERVER.HOST=127.0.0.1 .../rs3` and curl /ip on the file's port. /ip reports 127.0.0.1 (the environment wins over the file).
result: [pending]

### 4. No gate-greening by suppression (test-tier prohibition, all three plans)
expected: Review `git diff 0928a96..HEAD -- src Cargo.toml clippy.toml`. Expect no suppression attributes, unwrap_used/expect_used still at deny, exactly two #[ignore] (http::tests::test_patch_is_ok and test_patch_body_is_listing), and the legacy goldens intact. The verifier's grep at this commit found the tree clean. Phase 4 adds a wired CI grep for this (operator directive from Phase 1 UAT).
result: [pending]

## Summary

total: 4
passed: 0
issues: 0
pending: 4
skipped: 0
blocked: 0

## Gaps
