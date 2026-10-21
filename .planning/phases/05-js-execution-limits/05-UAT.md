---
status: complete
phase: 05-js-execution-limits
source: [05-VERIFICATION.md, 05-03-SUMMARY.md]
started: 2026-10-21T02:00:00Z
updated: 2026-10-21T09:30:17.553Z
---

## Current Test

[testing complete]

## Tests

### 1. CI run of this HEAD on the GitHub x86_64 runner (ROADMAP SC-4 "in CI")
expected: Run `git push origin modernize-toolchain`. The `CI` workflow's job on `ubuntu-26.04` should be green at every step. In the release smoke step, look for:
- `PATCH /key/me -> 422 js_depth_limit`;
- `GET /` and `PATCH /key/m` answered while the loop runs;
- `PATCH /key/loop -> 504 js_timeout after ~5010 ms, terminated by the watchdog`;
- no `server died (status 139)`.

This is the only place the watchdog's `terminate_execution` runs under the protected V8 platform on x86_64 (research assumption A1). The same push also covers 04-UAT item 1.
result: pass
source: operator
evidence: "Operator reported 2026-10-21: GitHub Actions run 39527188207 at 40afeb7 succeeded with 0 annotations and 216 tests passing. The release smoke asserts the 422 js_depth_limit, the requests answered during the loop and the watchdog 504 exactly, so a green run means all of them held on the protected V8 platform on x86_64."

### 2. Runtime started after the timeout is terminated immediately (present, not exercised by a test)
expected: Review `src/js/mod.rs:132-134`. When a nested runtime registers after the job has already timed out, it is terminated straight away. No test reaches this path, so confirm by review, or add a test later.
result: skipped
source: operator
reason: "Deferred follow-up: accepted risk (operator, 2026-10-21). The watchdog's 50 ms re-fire covers a runtime that registers after the timeout; a test for this path can be added later."

### 3. Client disconnect during a runaway script (present, not exercised by a test)
expected: Review `src/js/executor.rs:350-356`. The watchdog is armed at worker pickup, independent of the HTTP future, so a dropped request still has its script terminated after `timeout_ms`. No test drops a request mid-job.
result: pass
source: operator
evidence: "Operator test on the release binary, reported 2026-10-21. `PATCH /key/loop` (while(true){}) was sent with curl --max-time 1, and the client gave up with exit 28. After a 6 s wait, `PATCH /key/m` (6*20) returned \"120\" HTTP 200 in 13 ms. PATCH /key/loop again returned 504 {\"error\":\"js_timeout\",\"message\":\"script exceeded the 5000 ms execution limit\"}."

### 4. Judgment-tier prohibitions (flagged by policy in an autonomous run)
expected: The verifier's non-authoritative review says all four hold:
- no unbounded self-patching ran before the depth check existed;
- no infinite loop ran without an armed timeout;
- docs/js-ops.md implies neither rollback nor env-configurable limits;
- nothing claims the remote CI run was verified for HEAD.
result: pass
source: operator
evidence: "Operator accepted the verifier verdicts for all four prohibitions, reported 2026-10-21."

## Summary

total: 4
passed: 3
issues: 0
pending: 0
skipped: 1
blocked: 0

## Gaps

## Deferred Follow-Ups

- test: 2
  idea: "Optional test for a nested runtime that registers after the job timed out (src/js/mod.rs); accepted risk, the 50 ms watchdog re-fire covers it."
  deferred_at: 2026-10-21
