---
phase: 06
review: 06-REVIEW.md
titles: json
findings:
  - id: WR-01
    severity: warning
    disposition: open
    title: "The documented V8 stack-overflow stdout line has the wrong text and the wrong timing"
  - id: WR-02
    severity: warning
    disposition: open
    title: "The README says CI runs `scripts/test-ci-guards.sh`, but that script is local-only"
  - id: WR-03
    severity: warning
    disposition: open
    title: "The security note treats \"loopback\" as private, but loopback is reachable by every local account and by browsers on the host"
  - id: WR-04
    severity: warning
    disposition: open
    title: "The \"hardened\" unit has no memory ceiling, although the docs admit there is no heap limit"
  - id: IN-01
    severity: info
    disposition: open
    title: "The start limit turns client-triggerable crashes into a permanent outage"
  - id: IN-02
    severity: info
    disposition: open
    title: "\"The environment wins over such a file anyway\" overstates how harmless `/.env` is under the unit"
  - id: IN-03
    severity: info
    disposition: open
    title: "The Update section does not cover a changed unit or environment file"
open: 7
total: 7
recorded: 2026-10-21T11:17:39.264Z
---

# Phase 06: Code Review Disposition

| Finding | Severity | Disposition | Source |
|---------|----------|-------------|--------|
| WR-01 | warning | open | - |
| WR-02 | warning | open | - |
| WR-03 | warning | open | - |
| WR-04 | warning | open | - |
| IN-01 | info | open | - |
| IN-02 | info | open | - |
| IN-03 | info | open | - |

Dispositions: `open` (recorded, not yet triaged), `fixed`, `skipped`, `deferred`.
Set `deferred` by hand and put the reason in the Source cell; both are preserved. A `|` in the reason is kept as prose and escaped on the next run.
Re-running the gate keeps every row it can. A row the current review no longer reports is kept and its Source cell flagged, so a finding does not leave this record silently. ONE exception: when a finding id is REUSED by a different finding, the earlier decision cannot keep a row — the id is taken — and it is dropped. A RECORDED decision (anything but `open`) is named on the console when that happens; a row still at `open` is replaced silently, because `open` records no decision to lose.
