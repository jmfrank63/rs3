---
phase: 02
review: 02-REVIEW.md
titles: json
findings:
  - id: CR-01
    severity: critical
    disposition: open
    title: "Env-over-dotenv precedence is non-deterministic when key case differs"
  - id: WR-01
    severity: warning
    disposition: open
    title: "`server.` prefix filter still admits keys that collide with the server table"
  - id: WR-02
    severity: warning
    disposition: open
    title: "Valid JSON with an out-of-range number answers 400 `malformed_json` instead of 422"
  - id: IN-01
    severity: info
    disposition: open
    title: "`ApiError::Internal` is never constructed and the `internal` code is untested"
  - id: IN-02
    severity: info
    disposition: open
    title: "Startup banner prints the configured address, not the bound one"
  - id: IN-03
    severity: info
    disposition: open
    title: "`/insert` accepts keys that can never be read or deleted over HTTP"
  - id: IN-04
    severity: info
    disposition: open
    title: "Non-string values are discarded silently when a later duplicate key is a string"
open: 7
total: 7
recorded: 2026-10-20T18:41:59.762Z
---

# Phase 02: Code Review Disposition

| Finding | Severity | Disposition | Source |
|---------|----------|-------------|--------|
| CR-01 | critical | open | - |
| WR-01 | warning | open | - |
| WR-02 | warning | open | - |
| IN-01 | info | open | - |
| IN-02 | info | open | - |
| IN-03 | info | open | - |
| IN-04 | info | open | - |

Dispositions: `open` (recorded, not yet triaged), `fixed`, `skipped`, `deferred`.
Set `deferred` by hand and put the reason in the Source cell; both are preserved. A `|` in the reason is kept as prose and escaped on the next run.
Re-running the gate keeps every row it can. A row the current review no longer reports is kept and its Source cell flagged, so a finding does not leave this record silently. ONE exception: when a finding id is REUSED by a different finding, the earlier decision cannot keep a row — the id is taken — and it is dropped. A RECORDED decision (anything but `open`) is named on the console when that happens; a row still at `open` is replaced silently, because `open` records no decision to lose.
