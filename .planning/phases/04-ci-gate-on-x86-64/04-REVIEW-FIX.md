---
phase: 04-ci-gate-on-x86-64
fixed_at: 2026-10-20T22:59:23Z
review_path: .planning/phases/04-ci-gate-on-x86-64/04-REVIEW.md
iteration: 3
findings_in_scope: 1
fixed: 1
skipped: 0
status: all_fixed
---

# Phase 4: Code Review Fix Report

**Fixed at:** 2026-10-20T22:59:23Z
**Source review:** .planning/phases/04-ci-gate-on-x86-64/04-REVIEW.md
**Iteration:** 3

**Summary:**
- Findings in scope: 1 (fix scope: Critical and Warning)
- Fixed: 1
- Skipped: 0
- Also resolved, outside the scope: IN-02 (Info). The WR-01 fix removed the case-insensitive block-list grep that caused it.

## Fixed Issues

### WR-01: In CI without PyYAML, the grep layer of guard (g) does not stand on its own

**Files modified:** `scripts/check-workflow.py`, `scripts/ci-guards.sh`, `scripts/test-ci-guards.sh`, `.github/workflows/ci.yml` (one comment only)
**Commits:** 3fe55ef (the fix), 5f0f10e (follow-up: a literal U+2028 had slipped into a `check-workflow.py` comment; it is now written as the `\u2028` escape, so the scripts are pure ASCII again)
**Applied fix:** The block-list was replaced with an allow-list, as the orchestrator recommended. The operator's decisions still hold: nothing is pip-installed, PyYAML is skipped only in CI, and the runner stays `ubuntu-26.04`.

`scripts/check-workflow.py` now has two modes. With no argument it runs both.
- **`--lines`** uses only the standard library. PyYAML is imported lazily, so this mode runs under `python3 -S`.
  - ci.yml must be printable ASCII. A tab, a CR, `\x85`, `\u2028` or any other control or non-ASCII character fails, because YAML and Python disagree about which characters break lines.
  - The file is split on `\n` only. Every non-blank, non-comment line must equal the new `CANONICAL` list exactly and in order. Trailing spaces are ignored.
  - On a mismatch it prints a unified diff against `CANONICAL`.
  - `continue-on-error` and `SERVER.` (any case) stay banned on functional lines, which keeps D-05 (g) in force by name.
  - Skipping lines that start with `#` is safe. No canonical line opens a block scalar or a quoted scalar, so every such line really is a YAML comment.
- **`--structure`** is the existing PyYAML policy check, unchanged. It also guards `CANONICAL` itself: a gate-weakening edit made to ci.yml and `CANONICAL` together passes `--lines` but fails here.

Guard (g) in `scripts/ci-guards.sh` was reworked to use these modes.
- It now runs `check-workflow.py --lines` every time, with or without PyYAML.
- It runs `--structure` when PyYAML is present. That check is still mandatory locally. In CI it is skipped with the same `skip [g-structure] PyYAML unavailable` line as before.
- The single-workflow check stays.
- The block-list was removed: the case-insensitive content grep, the brace ban, the `${{` filter and the concurrency-line grep. The exact-lines check covers all of them.
- The guard header, the `check-workflow.py` docstring and a new two-line comment at the top of ci.yml all state that **editing ci.yml means updating CANONICAL in the same change**. Comments and blank lines are not pinned.

`scripts/test-ci-guards.sh` grew from 86 to 99 cases.
- The review's reproduced bypasses are now `expect_fail_ci` cases, which run with CI=true and PyYAML hidden:
  - `if: false`
  - a dropped `-D warnings`
  - `|| true`
  - `--test-threads=1`
  - `shell: sh`
  - a deleted smoke step
  - a deleted `permissions:` block
  - the explicit `? ref` / `: main` key
  - a deleted guards run line
- Two more `expect_fail_ci` cases check the D-05 ban by name: `continue-on-error` on the job, and a lowercase `server.host`.
- The 15 older CI cases are kept. They now expect the allow-list's message. The tab case expects the non-printable message.
- A new `expect_pass` helper adds a comment that mentions "release server." and "write access" to ci.yml. Guards must pass both locally and in CI, which shows IN-02 is gone.
- A new local case plants `if: false` in ci.yml and in `CANONICAL` together. Its setup asserts that `--lines` passes. The guards must then fail with `step 5: if is forbidden`.

Reproduction: I ran the new self-test against the guard scripts from 16d6739, in a scratch copy under `target/tmp`. All nine reproduced bypasses came back MISSED, and so did both D-05 cases, because the old guard passed them in CI mode. Both comment-only pass cases failed under the old grep. The other old-message cases showed as MISSED only because their expected message text changed.

Status: fixed. This changes how the check works, not its logic. Every case listed above is exercised by the self-test.

## Not addressed (Info, outside the scope)

The fix scope was `critical_warning`, so IN-01 and IN-03 through IN-10 were left as they are. IN-09 is affected in one way: the content grep no longer runs in front of the structure check, so the masking it describes no longer applies within guard (g). The two direct `expect_wf_fail` cases it asks for were not added. IN-10's note that a second `${{` expression is forbidden still holds, now through `CANONICAL` and `--structure`.

## Verification

Where it ran:
- **Per-fix checks** ran in the isolated worktree (`.claude/worktrees/rf-04-3728749-*`, now removed):
  - `ast.parse` and `bash -n`
  - `check-workflow.py` in every mode, including under `python3 -S`, plus a bad-argument run
  - `ci-guards.sh`, both locally and with CI=true
  - the full `test-ci-guards.sh`, twice: 99 cases, 0 missed
- **The final gate set** ran in the **main checkout** after the fast-forward to 3fe55ef, so the results can be reproduced from the current tree:

| Gate | Result |
|---|---|
| `bash -n` on the three scripts | ok |
| `bash scripts/ci-guards.sh` | all guards passed (`[g] single ci.yml, lines match the canonical list, structure ok`) |
| `CI=true bash scripts/ci-guards.sh` | all guards passed |
| `python3 scripts/check-workflow.py` | exit 0 |
| `bash scripts/test-ci-guards.sh` | 99 cases, 0 missed (was 86); re-run after 5f0f10e along with both guard runs and `check-workflow.py`, all green |
| `cargo build --release --locked && bash scripts/ci-smoke.sh` | `ci-smoke: OK (target/release/rs3, port 55159)` |
| TOOL-03 (`cargo build --locked && cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check`) | green, 171 passed, 0 failed |

Scratch copies (`target/tmp/wr01-old` and the self-test's `guard-selftest`) were deleted. Nothing was written to `/tmp`. The other entries already in `target/tmp` belong to earlier plans and were not touched.

## Previous iterations

### Iteration 2 (2026-10-20T22:44:48Z): 2 of 2 in scope fixed (plus IN-02), status all_fixed

The full report is backed up at `04-REVIEW-FIX.iter3.md`. These were the fixes for the second review (now `04-REVIEW.iter3.md`):

| Finding | Commit | Summary |
|---|---|---|
| WR-01 | c25a66a | `ml_grep` fails closed: `re.compile` runs inside the `try`, and any exception exits 2 |
| WR-02 | 7ada864 | Guard (g)'s grep layer matches keys in flow and quoted positions and bans braces |
| IN-02 | 16d6739 | The concurrency group must be exactly `ci-${{ github.ref }}`, in check-workflow.py and the grep layer |

### Iteration 1 (2026-10-20T22:27:03Z): 8 of 8 in scope fixed, status all_fixed

The full report is backed up at `04-REVIEW-FIX.iter2.md`. These were the fixes for the first review (now `04-REVIEW.iter2.md`):

| Finding | Commit | Summary |
|---|---|---|
| WR-01 | fcbd7f3 | Guard (a) catches multi-line `cfg_attr`, symlinked files and whitespace-split attributes (`ml_grep` introduced) |
| WR-02 | 2317756 | Guard (b) catches `cfg_attr(.., ignore)`, `~~~`/comma/space ignore fences, `cfg(any())`/`cfg(FALSE)`, and Cargo.toml `test`/`doctest`/`harness`/`auto*` = false |
| WR-03 | 1b158fb | Guard (d) also scans the source for unprotected or custom platform constructors and for non-`None` `init_platform` |
| WR-04 | f645c87 | check-workflow.py allow-lists workflow, trigger, concurrency, job, step and `with:` keys |
| WR-05 | c0713ce | Guard (g) content grep is case-insensitive and rejects working-directory, permissions, ref/repository, trigger filters and stray `${{` |
| WR-06 | 52620d4 | Guard (h) rejects rustfmt config files and `rustfmt::skip` |
| WR-07 | 7222c0d | Smoke reports a server crash during any request with its exit status (PKU status 139 path) |
| WR-08 | beb9789 | test-ci-guards.sh validates the scratch path under `target/tmp` and cleans up on exit |
| IN-05 (part) | 5940638 | Smoke readiness uses a real 30 s deadline, and cleanup escalates to SIGKILL |

---

_Fixed: 2026-10-20T22:59:23Z_
_Fixer: Claude (gsd-code-fixer)_
_Iteration: 3_
