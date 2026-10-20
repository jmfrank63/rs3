---
phase: 02
review: 02-REVIEW.md
titles: json
findings:
  - id: IN-01
    severity: info
    disposition: deferred
    title: "`ApiError::Internal` is never constructed and the `internal` code is untested (carried over)"
  - id: IN-02
    severity: info
    disposition: deferred
    title: "Startup banner prints the configured address, not the bound one (carried over)"
  - id: IN-03
    severity: info
    disposition: deferred
    title: "`/insert` accepts keys that can never be read or deleted over HTTP (carried over)"
  - id: IN-04
    severity: info
    disposition: deferred
    title: "Doc claim \"the first one in body order decides\" is false when a non-string value precedes an unrepresentable one"
  - id: IN-05
    severity: info
    disposition: deferred
    title: "Tests named \"deep nesting is 400\" exercise a truncated body, which contradicts the 422 decision for valid deep nesting"
  - id: CR-01
    severity: critical
    disposition: fixed
    title: "Env-over-dotenv precedence is non-deterministic when key case differs"
  - id: WR-01
    severity: warning
    disposition: fixed
    title: "Lone-surrogate escapes now answer 422 with false error messages (regression from the WR-02 fix)"
  - id: WR-02
    severity: warning
    disposition: fixed
    title: "Valid JSON with an out-of-range number answers 400 `malformed_json` instead of 422"
open: 0
total: 8
recorded: 2026-10-20T18:58:52.555Z
---

# Phase 02: Code Review Disposition

| Finding | Severity | Disposition | Source |
|---------|----------|-------------|--------|
| IN-01 | info | deferred | Phase 3: ApiError::Internal becomes the JS host-fault path (runtime init / join errors -> 500) and gets a test there |
| IN-02 | info | deferred | Accepted: banner reports configured host/port (legacy wording); SERVER.PORT=0 is not a supported config. Revisit with OPS-01 logging (v2) |
| IN-03 | info | deferred | Deferred v2 (APIV): empty key is stored but unreachable via /key/{key}; legacy parity, no data loss |
| IN-04 | info | deferred | Phase 3 housekeeping: correct the 'first problem in body order decides' doc sentence in parse_insert_body (doc-only) |
| IN-05 | info | deferred | Phase 3 housekeeping: rename parse_rejects_deep_nesting / insert_deeply_nested_is_400_json to truncated-input names (test names only) |
| CR-01 | critical | fixed | 02-REVIEW-FIX.iter2.md (not in the current review) |
| WR-01 | warning | fixed | 02-REVIEW-FIX.md (not in the current review) |
| WR-02 | warning | fixed | 02-REVIEW-FIX.iter2.md (not in the current review) |

Dispositions: `open` (recorded, not yet triaged), `fixed`, `skipped`, `deferred`.
Set `deferred` by hand and put the reason in the Source cell; both are preserved. A `|` in the reason is kept as prose and escaped on the next run.
Re-running the gate keeps every row it can. A row the current review no longer reports is kept and its Source cell flagged, so a finding does not leave this record silently. ONE exception: when a finding id is REUSED by a different finding, the earlier decision cannot keep a row — the id is taken — and it is dropped. A RECORDED decision (anything but `open`) is named on the console when that happens; a row still at `open` is replaced silently, because `open` records no decision to lose.
