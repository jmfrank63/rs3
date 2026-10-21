---
phase: 05
review: 05-REVIEW.md
titles: json
findings:
  - id: IN-01
    severity: info
    disposition: deferred
    title: "The backstop's log line has no job label, and the timeout is logged twice (carry-over)"
  - id: IN-02
    severity: info
    disposition: deferred
    title: "The docs' ops table and limits section are incomplete or inaccurate (carry-over)"
  - id: IN-03
    severity: info
    disposition: deferred
    title: "`ExecCtx::depth` duplicates `chain.len() - 1`, and now drives a classification (carry-over, widened)"
  - id: IN-04
    severity: info
    disposition: deferred
    title: "The smoke compares `nproc` with a pool sized by `available_parallelism` (carry-over)"
  - id: IN-05
    severity: info
    disposition: deferred
    title: "The watchdog spawn comment names the wrong thread as the V8 initialiser (carry-over)"
  - id: IN-06
    severity: info
    disposition: deferred
    title: "The timing assertions have tight upper bounds (carry-over)"
  - id: IN-07
    severity: info
    disposition: deferred
    title: "Every boot that runs out of stack writes an unprefixed V8 line to the server's stdout (new)"
  - id: IN-08
    severity: info
    disposition: deferred
    title: "The stack-exhaustion log line blames the nested key, not the script that used up the stack (new)"
  - id: WR-01
    severity: warning
    disposition: fixed
    title: "The smoke's timeout window includes the backstop, so a broken watchdog still passes"
  - id: WR-02
    severity: warning
    disposition: fixed
    title: "A nested host fault is dropped when the job times out during the nested run"
  - id: WR-03
    severity: warning
    disposition: fixed
    title: "A client can make a request answer 500 \"host fault\" by exhausting the JS stack before nesting"
open: 0
total: 11
recorded: 2026-10-21T01:17:02.474Z
---

# Phase 05: Code Review Disposition

| Finding | Severity | Disposition | Source |
|---------|----------|-------------|--------|
| IN-01 | info | deferred | Info: backstop log lacks key label / double timeout log — deferred to v2 OPS-01 (structured logging) |
| IN-02 | info | deferred | Info: docs/js-ops.md ops table + limits wording gaps — Phase 6 docs pass |
| IN-03 | info | deferred | Info: depth field duplicates chain length — deferred (refactor; behaviour correct and tested) |
| IN-04 | info | deferred | Info: smoke nproc vs available_parallelism — deferred v2 OPS-03 (only matters in CPU-quota containers) |
| IN-05 | info | deferred | Info: comment names wrong V8 init thread — Phase 6 docs/comments pass |
| IN-06 | info | deferred | Info: tight timing upper bounds in tests — watch in CI; loosen if flaky (v2 OPS-03) |
| IN-07 | info | deferred | Info: V8 prints stack-overflow line to stdout on nested boot failure — deferred v2 OPS-01 (logging) ; documented |
| IN-08 | info | deferred | Info: log names nested key instead of caller — deferred v2 OPS-01 (logging) |
| WR-01 | warning | fixed | 05-REVIEW-FIX.md (not in the current review) |
| WR-02 | warning | fixed | 05-REVIEW-FIX.md (not in the current review) |
| WR-03 | warning | fixed | 05-REVIEW-FIX.md (not in the current review) |

Dispositions: `open` (recorded, not yet triaged), `fixed`, `skipped`, `deferred`.
Set `deferred` by hand and put the reason in the Source cell; both are preserved. A `|` in the reason is kept as prose and escaped on the next run.
Re-running the gate keeps every row it can. A row the current review no longer reports is kept and its Source cell flagged, so a finding does not leave this record silently. ONE exception: when a finding id is REUSED by a different finding, the earlier decision cannot keep a row — the id is taken — and it is dropped. A RECORDED decision (anything but `open`) is named on the console when that happens; a row still at `open` is replaced silently, because `open` records no decision to lose.
