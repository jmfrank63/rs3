---
status: testing
phase: 05-js-execution-limits
source: [05-VERIFICATION.md, 05-03-SUMMARY.md]
started: 2026-10-21T02:00:00Z
updated: 2026-10-21T02:00:00Z
---

## Current Test

number: 1
name: CI run of this HEAD on the GitHub x86_64 runner (ROADMAP SC-4 "in CI")
expected: |
  Push modernize-toolchain; the ci job on ubuntu-26.04 is green; the release smoke shows the depth-limit 422,
  requests answered during the runaway loop, and the 504 ended by the watchdog — no "server died (status 139)".
awaiting: user response

## Tests

### 1. CI run of this HEAD on the GitHub x86_64 runner (ROADMAP SC-4 "in CI")
expected: Run `git push origin modernize-toolchain`. The `CI` workflow's job on `ubuntu-26.04` should be green at every step. In the release smoke step, look for:
- `PATCH /key/me -> 422 js_depth_limit`;
- `GET /` and `PATCH /key/m` answered while the loop runs;
- `PATCH /key/loop -> 504 js_timeout after ~5010 ms, terminated by the watchdog`;
- no `server died (status 139)`.

This is the only place the watchdog's `terminate_execution` runs under the protected V8 platform on x86_64 (research assumption A1). The same push also covers 04-UAT item 1.
result: [pending]

### 2. Runtime started after the timeout is terminated immediately (present, not exercised by a test)
expected: Review `src/js/mod.rs:132-134`. When a nested runtime registers after the job has already timed out, it is terminated straight away. No test reaches this path, so confirm by review, or add a test later.
result: [pending]

### 3. Client disconnect during a runaway script (present, not exercised by a test)
expected: Review `src/js/executor.rs:350-356`. The watchdog is armed at worker pickup, independent of the HTTP future, so a dropped request still has its script terminated after `timeout_ms`. No test drops a request mid-job.
result: [pending]

### 4. Judgment-tier prohibitions (flagged by policy in an autonomous run)
expected: The verifier's non-authoritative review says all four hold:
- no unbounded self-patching ran before the depth check existed;
- no infinite loop ran without an armed timeout;
- docs/js-ops.md implies neither rollback nor env-configurable limits;
- nothing claims the remote CI run was verified for HEAD.
result: [pending]

## Summary

total: 4
passed: 0
issues: 0
pending: 4
skipped: 0
blocked: 0

## Gaps
