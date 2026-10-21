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
    disposition: fixed
    title: "The `faulted_before` attribution branch cannot be reached in production"
  - id: IN-02
    severity: info
    disposition: fixed
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
    disposition: deferred
    title: "`stack_bytes` is not clamped, unlike `workers` and `queue_capacity`"
open: 0
total: 7
unparsed: 5
recorded: 2026-10-20T21:03:15.182Z
---

# Phase 03: Code Review Disposition

| Finding | Severity | Disposition | Source |
|---------|----------|-------------|--------|
| WR-01 | warning | fixed | 03-REVIEW-FIX.md |
| IN-01 | info | fixed | a1a5faa (iter 3): unreachable pre-run fault branch documented as defence in depth |
| IN-02 | info | fixed | e6b7324 (iter 3): migration table notes post-fault refusal for get/insert/delete |
| CR-01 | critical | fixed | 03-REVIEW-FIX.iter2.md (not in the current review) |
| WR-02 | warning | fixed | 03-REVIEW-FIX.iter3.md (not in the current review) |
| WR-03 | warning | fixed | 03-REVIEW-FIX.iter2.md (not in the current review) |
| IN-03 | info | deferred | Carry-over info items from the iter-3 review (IN-03..IN-07) are triaged below |

Dispositions: `open` (recorded, not yet triaged), `fixed`, `skipped`, `deferred`.
Set `deferred` by hand and put the reason in the Source cell; both are preserved. A `|` in the reason is kept as prose and escaped on the next run.
Re-running the gate keeps every row it can. A row the current review no longer reports is kept and its Source cell flagged, so a finding does not leave this record silently. ONE exception: when a finding id is REUSED by a different finding, the earlier decision cannot keep a row — the id is taken — and it is dropped. A RECORDED decision (anything but `open`) is named on the console when that happens; a row still at `open` is replaced silently, because `open` records no decision to lose.

## Orchestrator triage of remaining info findings (iter-3 review, 2026-10-20)

| Iter-3 id | Finding | Disposition | Target |
|-----------|---------|-------------|--------|
| IN-03 | docs overclaim that stdout output cannot fake log lines (stdout is raw) | deferred | Phase 6 docs pass (README / js-ops.md wording) |
| IN-04 | `op_print` replacement does not retry on `WouldBlock` (non-blocking pipes) | deferred | Phase 5 (JS runtime hardening) |
| IN-05 | `requests/*.txt` payload copies are not exercised by tests | deferred | Phase 5/6 (fixture parity test) |
| IN-06 | `catch_unwind` comment overstates coverage (a panic inside an op still aborts) | deferred | Phase 5 (comment fix while touching executor) |
| IN-07 | `stack_bytes` is not clamped like `workers` / `queue_capacity` | deferred | Phase 5 (stack sizing is a Phase 5 research flag) |

WR-01 (iter 3, log amplification) was fixed in 912d942 after the final permitted re-review; the 3-iteration cap was reached, so the fix is covered by its regression tests (`one_nested_host_fault_is_logged_once_at_any_depth`) and the phase verifier rather than a 4th review.
