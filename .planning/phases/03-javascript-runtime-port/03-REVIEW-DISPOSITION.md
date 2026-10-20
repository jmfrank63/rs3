---
phase: 03
review: 03-REVIEW.md
titles: json
findings:
  - id: WR-01
    severity: warning
    disposition: fixed
    title: "One nested host fault is logged once per ancestor level, and each extra line blames a key whose runtime did not fail"
  - id: IN-01
    severity: info
    disposition: open
    title: "The `faulted_before` attribution branch cannot be reached in production"
  - id: IN-02
    severity: info
    disposition: open
    title: "The migration table lists the post-fault refusal for `op_rs3_list` and `op_rs3_patch` only"
  - id: CR-01
    severity: critical
    disposition: fixed
    title: "An exception while stringifying the completion value becomes 200 `\"\"` instead of 422"
  - id: WR-02
    severity: warning
    disposition: fixed
    title: "After a host fault the script keeps running, and its later writes are committed even though the request answers 500"
  - id: WR-03
    severity: warning
    disposition: fixed
    title: "`full_queue_makes_callers_wait` does not test what its name says"
  - id: IN-03
    severity: info
    disposition: open
    title: "`stack_bytes` is not clamped, unlike `workers` and `queue_capacity`"
open: 3
total: 7
unparsed: 5
recorded: 2026-10-20T21:03:15.182Z
---

# Phase 03: Code Review Disposition

| Finding | Severity | Disposition | Source |
|---------|----------|-------------|--------|
| WR-01 | warning | fixed | 03-REVIEW-FIX.md |
| IN-01 | info | open | - |
| IN-02 | info | open | - |
| CR-01 | critical | fixed | 03-REVIEW-FIX.iter2.md (not in the current review) |
| WR-02 | warning | fixed | 03-REVIEW-FIX.iter3.md (not in the current review) |
| WR-03 | warning | fixed | 03-REVIEW-FIX.iter2.md (not in the current review) |
| IN-03 | info | open | - (not in the current review) |

Dispositions: `open` (recorded, not yet triaged), `fixed`, `skipped`, `deferred`.
Set `deferred` by hand and put the reason in the Source cell; both are preserved. A `|` in the reason is kept as prose and escaped on the next run.
Re-running the gate keeps every row it can. A row the current review no longer reports is kept and its Source cell flagged, so a finding does not leave this record silently. ONE exception: when a finding id is REUSED by a different finding, the earlier decision cannot keep a row — the id is taken — and it is dropped. A RECORDED decision (anything but `open`) is named on the console when that happens; a row still at `open` is replaced silently, because `open` records no decision to lose.
