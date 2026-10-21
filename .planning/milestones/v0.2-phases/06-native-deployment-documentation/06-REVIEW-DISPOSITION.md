---
phase: 06
review: 06-REVIEW.md
titles: json
findings:
  - id: WR-01
    severity: warning
    disposition: fixed
    title: "The new `MemoryMax=` sizing rule underestimates memory by about 4× the store per concurrent `/list` or `op_rs3_list`; an operator who follows it gets an OOM kill and loses every key under normal traffic"
  - id: IN-01
    severity: info
    disposition: fixed
    title: "The `MemoryMax=` paragraph states the kill-and-empty consequence twice"
  - id: WR-02
    severity: warning
    disposition: fixed
    title: "The README says CI runs `scripts/test-ci-guards.sh`, but that script is local-only"
  - id: WR-03
    severity: warning
    disposition: fixed
    title: "The security note treats \"loopback\" as private"
  - id: WR-04
    severity: warning
    disposition: fixed
    title: "The \"hardened\" unit has no memory ceiling"
  - id: IN-02
    severity: info
    disposition: fixed
    title: "\"The environment wins over such a file anyway\" overstates how harmless `/.env` is"
  - id: IN-03
    severity: info
    disposition: fixed
    title: "The Update section does not cover a changed unit or environment file"
open: 0
total: 7
recorded: 2026-10-21T11:35:54.482Z
---

# Phase 06: Code Review Disposition

| Finding | Severity | Disposition | Source |
|---------|----------|-------------|--------|
| WR-01 | warning | fixed | 06-REVIEW-FIX.md (9a1e3b2; reconciled by hand: the fix report shortens the finding title, so the title check did not match) |
| IN-01 | info | fixed | 06-REVIEW-FIX.md |
| WR-02 | warning | fixed | 06-REVIEW-FIX.iter2.md (not in the current review) |
| WR-03 | warning | fixed | 06-REVIEW-FIX.iter2.md (not in the current review) |
| WR-04 | warning | fixed | 06-REVIEW-FIX.iter2.md (not in the current review) |
| IN-02 | info | fixed | 06-REVIEW-FIX.iter2.md (not in the current review) |
| IN-03 | info | fixed | 06-REVIEW-FIX.iter2.md (not in the current review) |

Dispositions: `open` (recorded, not yet triaged), `fixed`, `skipped`, `deferred`.
Set `deferred` by hand and put the reason in the Source cell; both are preserved. A `|` in the reason is kept as prose and escaped on the next run.
Re-running the gate keeps every row it can. A row the current review no longer reports is kept and its Source cell flagged, so a finding does not leave this record silently. ONE exception: when a finding id is REUSED by a different finding, the earlier decision cannot keep a row — the id is taken — and it is dropped. A RECORDED decision (anything but `open`) is named on the console when that happens; a row still at `open` is replaced silently, because `open` records no decision to lose.
