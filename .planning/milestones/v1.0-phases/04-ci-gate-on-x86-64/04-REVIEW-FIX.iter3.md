---
phase: 04-ci-gate-on-x86-64
fixed_at: 2026-10-20T22:44:48Z
review_path: .planning/phases/04-ci-gate-on-x86-64/04-REVIEW.md
iteration: 2
findings_in_scope: 2
fixed: 2
skipped: 0
status: all_fixed
---

# Phase 4: Code Review Fix Report

**Fixed at:** 2026-10-20T22:44:48Z
**Source review:** .planning/phases/04-ci-gate-on-x86-64/04-REVIEW.md
**Iteration:** 2

**Summary:**
- Findings in scope: 2 (fix scope: Critical and Warning)
- Fixed: 2
- Skipped: 0
- Also fixed, outside the scope: IN-02 (Info). The orchestrator flagged it as trivially adjacent to WR-02.

## Fixed Issues

### WR-01: `ml_grep` fails open, so guards (a), (b), (d) and (h) pass if the scanner throws anything but OSError

**Files modified:** `scripts/ci-guards.sh`, `scripts/test-ci-guards.sh`
**Commit:** c25a66a
**Applied fix:**
- `re.compile(sys.argv[1])` now runs inside the `try`.
- The handler catches `BaseException` (previously only `OSError`), prints `ml_grep: <Type>: <msg>` to stderr and exits 2.
- The helper's header comment now states that no exception may escape. Python's own exit status for an uncaught exception is 1, which the consumers read as "no hits".
- All four consumers (`grep_guard`, (b), (d), (h)) already fail on exit 2 or higher, so they did not need changes.

New self-test case: it drops the `)` from guard (a)'s pattern in the copied `ci-guards.sh`, plants `#[allow(dead_code)]`, and expects `FAIL [a]` with `ml_grep failed (exit 2)`. I checked the case both ways. With the old helper (`git show HEAD:scripts/ci-guards.sh` in a scratch copy) it reports `ok [a]`, which reproduces the bug. With the fix it reports `FAIL [a] ml_grep failed (exit 2): ml_grep: error: missing ), unterminated subpattern`.

### WR-02: Guard (g)'s grep-only layer (CI without PyYAML) misses YAML flow mappings

**Files modified:** `scripts/ci-guards.sh`, `scripts/test-ci-guards.sh`
**Commit:** 7ada864
**Applied fix:** the change has two layers.
1. **Key-position anchoring**, following the review's suggestion:
   - A forbidden key now matches after indentation, `{`, `[` or `,`, not only at the start of a line. It may also be quoted with `"` or `'`.
   - This covers `ref|repository`, the trigger filters (`branches`/`tags`/`paths` with their `-ignore` forms, and `types`), and `<key>: write`.
   - The `write` value may be quoted, so `contents: 'write'` was a block-style gap that is now closed.
   - Job-level `permissions:` still matches only when indented, so the required top-level `permissions:` line passes.
2. **No flow mappings at all.** The guard strips `${{ ... }}` expressions with `sed`, then fails on any remaining `{` or `}` and prints "YAML flow mapping (or stray brace) in the workflow".
   - ci.yml is block style throughout, so this costs nothing today.
   - `sed` runs on its own rather than in a pipe. Otherwise, under `pipefail`, grep's "no match" (exit 1) would hide a `sed` failure.
   - Trade-off, documented in the guard header: a brace inside a `run:` string, such as shell `${VAR}`, also fails. Every run string is pinned by `check-workflow.py` anyway.

Seven new `expect_fail_ci` cases (CI=true, PyYAML hidden):
- The four flow-style lines from the review: `with: { ref: main }`, `pull_request: { paths-ignore: ['**'] }`, `permissions: { contents: write }` and `push: { branches: [main] }`. Each must produce `forbidden workflow content`, so the key patterns catch them on their own, not only the brace check.
- A quoted key, `"ref": main`.
- A quoted value, `contents: 'write'`.
- A flow mapping with no forbidden key, `with: { components: rustfmt }`, which must produce `YAML flow mapping`.

I checked that `  group: ci-${{ github.ref }}` still passes.

### IN-02 (Info, outside the scope): check-workflow.py accepts any constant concurrency group

**Files modified:** `scripts/check-workflow.py`, `scripts/ci-guards.sh`, `scripts/test-ci-guards.sh`
**Commit:** 16d6739
**Applied fix:**
- `check-workflow.py` now requires `concurrency.group == "ci-${{ github.ref }}"`, the per-ref group from D-03. The constant is `CONCURRENCY_GROUP`.
- The guard (g) grep layer (`grep -qxF '  group: ci-${{ github.ref }}'`) now requires the same line, so the CI path without PyYAML is covered too.
- Two new cases test `group: ci`: one through `expect_wf_fail` and one through `expect_fail_ci`.

## Not addressed (Info, outside the scope)

IN-01 and IN-03 through IN-10 were left as they are, because the fix scope was `critical_warning`. The runner stays `ubuntu-26.04` (operator commit fd8f43f, the D-01 amendment).

## Verification

Where it ran:
- **Per-fix checks** ran in the isolated worktree (`.claude/worktrees/rf-04-*`, since removed): `bash -n`, `ast.parse`, `ci-guards.sh`, and the full `test-ci-guards.sh` after WR-02 and after IN-02.
- **The final gate set** ran in the **main checkout** after the fast-forward to 16d6739, so it can be reproduced from the current tree:

| Gate | Result |
|---|---|
| `bash scripts/ci-guards.sh` | all guards passed |
| `CI=true bash scripts/ci-guards.sh` | all guards passed |
| `python3 scripts/check-workflow.py` | exit 0 |
| `bash scripts/test-ci-guards.sh` | 86 cases, 0 missed (was 76: +1 WR-01, +7 WR-02, +2 IN-02) |
| `cargo build --release --locked && bash scripts/ci-smoke.sh` | `ci-smoke: OK (target/release/rs3, ...)` |
| TOOL-03 (`cargo build --locked && cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check`) | green, 171 passed, 0 failed |

Scratch directories went under `target/tmp` (`wr01`, `wr01b`, `guard-selftest`) and were deleted. Nothing was written to `/tmp`.

## Previous iterations

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

_Fixed: 2026-10-20T22:44:48Z_
_Fixer: Claude (gsd-code-fixer)_
_Iteration: 2_
