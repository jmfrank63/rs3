---
phase: 01
review: 01-REVIEW.md
titles: json
findings:
  - id: WR-01
    severity: warning
    disposition: deferred
    title: "A malformed `.env` is half-loaded and the error is discarded, so the user gets a misleading config error"
  - id: WR-02
    severity: warning
    disposition: deferred
    title: "`Settings::load()` is a safe public API that calls `std::env::set_var` underneath, despite `forbid(unsafe_code)`"
  - id: IN-01
    severity: info
    disposition: deferred
    title: "Handler panics and the is-empty-then-get race in `GET /key` (documented deferral, D-10)"
  - id: IN-02
    severity: info
    disposition: deferred
    title: "`/list` builds JSON-like text with no escaping, so a stored value can forge entries (documented deferral, D-08)"
  - id: IN-03
    severity: info
    disposition: deferred
    title: "`insert_many`'s `Result`-item API forces an always-`Ok` `unwrap()` in the handler, and batches are not atomic for readers"
  - id: IN-04
    severity: info
    disposition: deferred
    title: "`/list` is already sorted, although D-08 puts the sort change in Phase 2"
  - id: IN-05
    severity: info
    disposition: deferred
    title: "The parked PATCH listing test dropped the legacy store reset and will be flaky when re-enabled"
  - id: IN-06
    severity: info
    disposition: deferred
    title: "The startup banner prints before the bind succeeds"
  - id: IN-07
    severity: info
    disposition: deferred
    title: "The unprefixed `Environment` source reads every process environment variable into the config tree"
open: 0
total: 9
recorded: 2026-10-20T17:05:39.574Z
---

# Phase 01: Code Review Disposition

| Finding | Severity | Disposition | Source |
|---------|----------|-------------|--------|
| WR-01 | warning | deferred | Phase 2 (operator directive 2026-10-20): ignore only not-found; surface other .env errors as config errors |
| WR-02 | warning | deferred | Phase 2 (operator directive 2026-10-20): stop mutating the process env (dotenvy iter -> config source) |
| IN-01 | info | deferred | Phase 2 (D-10, ROBU-01): single get -> 404, no unwrap in handlers |
| IN-02 | info | deferred | Phase 2 (D-08, HTTP-06): serde_json-escaped /list |
| IN-03 | info | deferred | Phase 2: infallible insert path for the handler; document batch visibility |
| IN-04 | info | deferred | Recorded: sorted /list shipped in Phase 1; Phase 2 golden tests must not treat sorting as new |
| IN-05 | info | deferred | Phase 3 (TEST-03/05): re-enable PATCH listing test on an injected per-test Store |
| IN-06 | info | deferred | Phase 2: print the startup banner after a successful bind |
| IN-07 | info | deferred | Deferred to v2 (OPS-02 prefix/alias work); legacy parity, D-12 keeps dotted names |

Dispositions: `open` (recorded, not yet triaged), `fixed`, `skipped`, `deferred`.
Set `deferred` by hand and put the reason in the Source cell; both are preserved. A `|` in the reason is kept as prose and escaped on the next run.
Re-running the gate keeps every row it can. A row the current review no longer reports is kept and its Source cell flagged, so a finding does not leave this record silently. ONE exception: when a finding id is REUSED by a different finding, the earlier decision cannot keep a row — the id is taken — and it is dropped. A RECORDED decision (anything but `open`) is named on the console when that happens; a row still at `open` is replaced silently, because `open` records no decision to lose.
