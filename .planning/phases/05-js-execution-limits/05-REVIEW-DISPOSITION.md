---
phase: 05
review: 05-REVIEW.md
titles: json
findings:
  - id: WR-01
    severity: warning
    disposition: open
    title: "The smoke's timeout window includes the backstop, so a broken watchdog still passes"
  - id: WR-02
    severity: warning
    disposition: open
    title: "A nested host fault is dropped when the job times out during the nested run"
  - id: WR-03
    severity: warning
    disposition: open
    title: "A client can make a request answer 500 \"host fault\" by exhausting the JS stack before nesting"
  - id: IN-01
    severity: info
    disposition: open
    title: "The backstop's log line has no job label, and the timeout is logged twice"
  - id: IN-02
    severity: info
    disposition: open
    title: "The ops table and the timeout section in the docs are incomplete or inaccurate"
  - id: IN-03
    severity: info
    disposition: open
    title: "`ExecCtx::depth` duplicates `chain.len() - 1`"
  - id: IN-04
    severity: info
    disposition: open
    title: "`op_rs3_patch`'s `map_err` claims to need no wildcard but ends with one"
  - id: IN-05
    severity: info
    disposition: open
    title: "The smoke compares `nproc` with a pool sized by `available_parallelism`"
  - id: IN-06
    severity: info
    disposition: open
    title: "The watchdog spawn comment names the wrong thread as the V8 initialiser"
  - id: IN-07
    severity: info
    disposition: open
    title: "The timing assertions have tight upper bounds"
open: 10
total: 10
recorded: 2026-10-21T01:01:10.020Z
---

# Phase 05: Code Review Disposition

| Finding | Severity | Disposition | Source |
|---------|----------|-------------|--------|
| WR-01 | warning | open | - |
| WR-02 | warning | open | - |
| WR-03 | warning | open | - |
| IN-01 | info | open | - |
| IN-02 | info | open | - |
| IN-03 | info | open | - |
| IN-04 | info | open | - |
| IN-05 | info | open | - |
| IN-06 | info | open | - |
| IN-07 | info | open | - |

Dispositions: `open` (recorded, not yet triaged), `fixed`, `skipped`, `deferred`.
Set `deferred` by hand and put the reason in the Source cell; both are preserved. A `|` in the reason is kept as prose and escaped on the next run.
Re-running the gate keeps every row it can. A row the current review no longer reports is kept and its Source cell flagged, so a finding does not leave this record silently. ONE exception: when a finding id is REUSED by a different finding, the earlier decision cannot keep a row — the id is taken — and it is dropped. A RECORDED decision (anything but `open`) is named on the console when that happens; a row still at `open` is replaced silently, because `open` records no decision to lose.
