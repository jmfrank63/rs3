---
phase: 04
review: 04-REVIEW.md
titles: json
findings:
  - id: WR-01
    severity: warning
    disposition: fixed
    title: "In CI without PyYAML, the grep layer of guard (g) does not stand on its own"
  - id: IN-03
    severity: info
    disposition: open
    title: "Guard (d) source scan misses `Platform::new_custom(.., true, ..)` passed through `RuntimeOptions::v8_platform` (carry-over, iteration-2 IN-01)"
  - id: IN-04
    severity: info
    disposition: open
    title: "test-ci-guards.sh refuses to run when `target/` is a symlink, and blames an unset variable (carry-over, iteration-2 IN-03)"
  - id: IN-05
    severity: info
    disposition: open
    title: "Guard (e) can false-positive on a same-version v8 (carry-over)"
  - id: IN-06
    severity: info
    disposition: open
    title: "Guard (f) misses whitespace variants and does not scan `tests/` (carry-over)"
  - id: IN-07
    severity: info
    disposition: open
    title: "The Rust-source text greps reject harmless comments and strings, and `[lints] workspace = true` fails (c) (carry-over)"
  - id: IN-08
    severity: info
    disposition: open
    title: "Smoke port TOCTOU and `RS3_BIN` lookup mismatch (carry-over)"
  - id: IN-09
    severity: info
    disposition: open
    title: "check-workflow.py's `continue-on-error`/`env` handling is never exercised directly, and the actions-rs check is unreachable (carry-over, iteration-2 IN-08 + IN-09)"
  - id: IN-10
    severity: info
    disposition: open
    title: "`cancel-in-progress` cancels superseded runs on `main`, and same-repo PR branches run twice (carry-over)"
  - id: WR-02
    severity: warning
    disposition: fixed
    title: "Guard (g)'s grep-only layer (CI without PyYAML) misses YAML flow mappings"
  - id: WR-03
    severity: warning
    disposition: fixed
    title: "Guard (d) checks only the Cargo feature; source code can select the unprotected platform directly"
  - id: WR-04
    severity: warning
    disposition: fixed
    title: "check-workflow.py accepts keys that redirect or neuter every gate"
  - id: WR-05
    severity: warning
    disposition: fixed
    title: "In CI the forbidden-content grep is the only workflow check, and it is case-insensitive and incomplete"
  - id: WR-06
    severity: warning
    disposition: fixed
    title: "Nothing guards the `cargo fmt --check` gate against being disabled by configuration"
  - id: WR-07
    severity: warning
    disposition: fixed
    title: "The smoke never reports the PKU crash it exists to detect; it surfaces as a bare \"transport error\""
  - id: WR-08
    severity: warning
    disposition: fixed
    title: "test-ci-guards.sh runs `rm -rf` on an unvalidated, environment-supplied path"
  - id: IN-01
    severity: info
    disposition: open
    title: "Guard (e) can false-positive on a same-version v8"
  - id: IN-02
    severity: info
    disposition: open
    title: "Guard (g) treats grep errors as \"no hits\" and reports only the first failing layer"
open: 10
total: 18
unparsed: 2
recorded: 2026-10-20T23:01:50.164Z
---

# Phase 04: Code Review Disposition

| Finding | Severity | Disposition | Source |
|---------|----------|-------------|--------|
| WR-01 | warning | fixed | 04-REVIEW-FIX.md |
| IN-03 | info | open | - |
| IN-04 | info | open | - |
| IN-05 | info | open | - |
| IN-06 | info | open | - |
| IN-07 | info | open | - |
| IN-08 | info | open | - |
| IN-09 | info | open | - |
| IN-10 | info | open | - |
| WR-02 | warning | fixed | 04-REVIEW-FIX.iter3.md (not in the current review) |
| WR-03 | warning | fixed | 04-REVIEW-FIX.iter2.md (not in the current review) |
| WR-04 | warning | fixed | 04-REVIEW-FIX.iter2.md (not in the current review) |
| WR-05 | warning | fixed | 04-REVIEW-FIX.iter2.md (not in the current review) |
| WR-06 | warning | fixed | 04-REVIEW-FIX.iter2.md (not in the current review) |
| WR-07 | warning | fixed | 04-REVIEW-FIX.iter2.md (not in the current review) |
| WR-08 | warning | fixed | 04-REVIEW-FIX.iter2.md (not in the current review) |
| IN-01 | info | open | - (not in the current review) |
| IN-02 | info | open | - (not in the current review) |

Dispositions: `open` (recorded, not yet triaged), `fixed`, `skipped`, `deferred`.
Set `deferred` by hand and put the reason in the Source cell; both are preserved. A `|` in the reason is kept as prose and escaped on the next run.
Re-running the gate keeps every row it can. A row the current review no longer reports is kept and its Source cell flagged, so a finding does not leave this record silently. ONE exception: when a finding id is REUSED by a different finding, the earlier decision cannot keep a row — the id is taken — and it is dropped. A RECORDED decision (anything but `open`) is named on the console when that happens; a row still at `open` is replaced silently, because `open` records no decision to lose.
