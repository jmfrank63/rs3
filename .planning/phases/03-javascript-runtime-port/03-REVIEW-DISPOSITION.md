---
phase: 03
review: 03-REVIEW.md
titles: json
findings:
  - id: CR-01
    severity: critical
    disposition: open
    title: "An exception while stringifying the completion value becomes 200 `\"\"` instead of 422"
  - id: WR-01
    severity: warning
    disposition: open
    title: "A host fault in a nested runtime turns into a catchable JS `Error` and a 422 whose body contains host details"
  - id: WR-02
    severity: warning
    disposition: open
    title: "Log forging is still possible through exception messages and `Deno.core.print(.., true)`"
  - id: WR-03
    severity: warning
    disposition: open
    title: "`full_queue_makes_callers_wait` does not test what its name says"
  - id: IN-01
    severity: info
    disposition: open
    title: "The `.txt` payloads the REST walkthrough posts are separate, untested copies of the `.js` fixtures"
  - id: IN-02
    severity: info
    disposition: open
    title: "The `catch_unwind` comment overstates what it protects"
  - id: IN-03
    severity: info
    disposition: open
    title: "`stack_bytes` is not clamped, unlike `workers` and `queue_capacity`"
open: 7
total: 7
recorded: 2026-10-20T20:34:23.692Z
---

# Phase 03: Code Review Disposition

| Finding | Severity | Disposition | Source |
|---------|----------|-------------|--------|
| CR-01 | critical | open | - |
| WR-01 | warning | open | - |
| WR-02 | warning | open | - |
| WR-03 | warning | open | - |
| IN-01 | info | open | - |
| IN-02 | info | open | - |
| IN-03 | info | open | - |

Dispositions: `open` (recorded, not yet triaged), `fixed`, `skipped`, `deferred`.
Set `deferred` by hand and put the reason in the Source cell; both are preserved. A `|` in the reason is kept as prose and escaped on the next run.
Re-running the gate keeps every row it can. A row the current review no longer reports is kept and its Source cell flagged, so a finding does not leave this record silently. ONE exception: when a finding id is REUSED by a different finding, the earlier decision cannot keep a row — the id is taken — and it is dropped. A RECORDED decision (anything but `open`) is named on the console when that happens; a row still at `open` is replaced silently, because `open` records no decision to lose.
