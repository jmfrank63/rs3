---
phase: 04
review: 04-REVIEW.md
titles: json
findings:
  - id: WR-01
    severity: warning
    disposition: open
    title: "Guard (a) misses multi-line `cfg_attr` suppression, symlinked files and whitespace-split attributes"
  - id: WR-02
    severity: warning
    disposition: open
    title: "Guard (b) misses common ways to ignore or disable tests"
  - id: WR-03
    severity: warning
    disposition: open
    title: "Guard (d) checks only the Cargo feature; source code can select the unprotected platform directly"
  - id: WR-04
    severity: warning
    disposition: open
    title: "check-workflow.py accepts keys that redirect or neuter every gate"
  - id: WR-05
    severity: warning
    disposition: open
    title: "In CI the forbidden-content grep is the only workflow check, and it is case-sensitive and incomplete"
  - id: WR-06
    severity: warning
    disposition: open
    title: "Nothing guards the `cargo fmt --check` gate against being disabled by configuration"
  - id: WR-07
    severity: warning
    disposition: open
    title: "The smoke never reports the PKU crash it exists to detect; it surfaces as a bare \"transport error\""
  - id: WR-08
    severity: warning
    disposition: open
    title: "test-ci-guards.sh runs `rm -rf` on an unvalidated, environment-supplied path"
  - id: IN-01
    severity: info
    disposition: open
    title: "Guard (e) can false-positive on a same-version v8"
  - id: IN-02
    severity: info
    disposition: open
    title: "Guard (g) treats grep errors as \"no hits\" and reports only the first failing layer"
  - id: IN-03
    severity: info
    disposition: open
    title: "Guard (f) does not cover whitespace variants or `tests/`"
  - id: IN-04
    severity: info
    disposition: open
    title: "Strict greps reject harmless text, and workspace lints are rejected"
  - id: IN-05
    severity: info
    disposition: open
    title: "Smoke robustness nits"
  - id: IN-06
    severity: info
    disposition: open
    title: "Self-test gaps"
  - id: IN-07
    severity: info
    disposition: open
    title: "Unreachable actions-rs check"
  - id: IN-08
    severity: info
    disposition: open
    title: "Concurrency cancels superseded runs on main too"
open: 16
total: 16
recorded: 2026-10-20T22:08:08.066Z
---

# Phase 04: Code Review Disposition

| Finding | Severity | Disposition | Source |
|---------|----------|-------------|--------|
| WR-01 | warning | open | - |
| WR-02 | warning | open | - |
| WR-03 | warning | open | - |
| WR-04 | warning | open | - |
| WR-05 | warning | open | - |
| WR-06 | warning | open | - |
| WR-07 | warning | open | - |
| WR-08 | warning | open | - |
| IN-01 | info | open | - |
| IN-02 | info | open | - |
| IN-03 | info | open | - |
| IN-04 | info | open | - |
| IN-05 | info | open | - |
| IN-06 | info | open | - |
| IN-07 | info | open | - |
| IN-08 | info | open | - |

Dispositions: `open` (recorded, not yet triaged), `fixed`, `skipped`, `deferred`.
Set `deferred` by hand and put the reason in the Source cell; both are preserved. A `|` in the reason is kept as prose and escaped on the next run.
Re-running the gate keeps every row it can. A row the current review no longer reports is kept and its Source cell flagged, so a finding does not leave this record silently. ONE exception: when a finding id is REUSED by a different finding, the earlier decision cannot keep a row — the id is taken — and it is dropped. A RECORDED decision (anything but `open`) is named on the console when that happens; a row still at `open` is replaced silently, because `open` records no decision to lose.
