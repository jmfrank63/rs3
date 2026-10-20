---
status: testing
phase: 03-javascript-runtime-port
source: [03-VERIFICATION.md]
started: 2026-10-20T21:20:00Z
updated: 2026-10-20T21:20:00Z
---

## Current Test

number: 1
name: Test-tier prohibitions without repo-resident enforcement (suppression, dev-only platform, no op shim, limits disclosed)
expected: |
  Zero suppression attributes and zero #[ignore] in src/; unwrap_used/expect_used stay at deny;
  `cargo tree -e features,normal -i deno_core` shows no unsafe_use_unprotected_platform (dev edge only);
  no opSync / Deno.core.ops() / rs3_* alias outside the migration table in docs/js-ops.md;
  docs/js-ops.md lists op_panic disabled and the Phase 5 limits without softening them.
awaiting: user response

## Tests

### 1. Test-tier prohibitions without repo-resident enforcement
expected: Review `git diff 9209f76..HEAD -- src Cargo.toml docs requests`. Expect:
- zero suppression attributes and zero `#[ignore]` in src/, with unwrap_used/expect_used still at deny;
- the unprotected V8 platform feature on the dev edge only (`cargo tree -e features,normal -i deno_core` shows none);
- no legacy op API shim and no `opSync` / `Deno.core.ops()` / `rs3_*` aliases outside the docs migration table;
- the docs disclose the Phase 5 limits without softening them.

The verifier's checks were clean. Phase 4's CI grep gate is meant to wire these.
result: [pending]

### 2. No self-patching script in tests, fixtures or smokes (judgment-tier prohibition)
expected: Review every `op_rs3_patch` use (`grep -n op_rs3_patch src/js/*.rs src/http/*.rs`). Every nested chain should end at a key that does not patch, and no key should patch itself directly or through a cycle. The verifier checked all 20 call sites and found them clean.
result: [pending]

### 3. docs/js-ops.md readability for a script author
expected: Read docs/js-ops.md as someone migrating a legacy rs3 script. The mapping table, error rules, PATCH result rules and limits should be clear enough to migrate fac/sum/bin/store-style scripts without reading the Rust source. The verifier confirmed every documented message matches the code and the live binary.
result: [pending]

## Summary

total: 3
passed: 0
issues: 0
pending: 3
skipped: 0
blocked: 0

## Gaps
