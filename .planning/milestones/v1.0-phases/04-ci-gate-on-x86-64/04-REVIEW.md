---
phase: 04-ci-gate-on-x86-64
reviewed: 2026-10-20T22:49:38Z
depth: standard
iteration: 3
files_reviewed: 5
files_reviewed_list:
  - .github/workflows/ci.yml
  - scripts/check-workflow.py
  - scripts/ci-guards.sh
  - scripts/ci-smoke.sh
  - scripts/test-ci-guards.sh
findings:
  critical: 0
  warning: 1
  info: 10
  total: 11
status: issues_found
---

# Phase 4: Code Review Report (re-review after fix iteration 2)

**Reviewed:** 2026-10-20T22:49:38Z
**Depth:** standard
**Files Reviewed:** 5
**Status:** issues_found

## Summary

This re-review covers the tree after fix commits c25a66a (`ml_grep` fails closed), 7ada864
(flow mappings and the brace ban in guard (g)'s grep layer) and 16d6739 (concurrency group
pinned). The following were stated as deliberate in the brief and are not reported:

- the ubuntu-26.04 runner (D-01 amendment)
- tag and branch action refs
- no push
- no pip install and no actionlint download
- the ban on braces in the workflow text

Local runs (the scripts use GNU grep 3.11 at `/usr/bin/grep`, bash 5.2.21, Python 3 with PyYAML 6.0.1):

- `bash -n` passed on all three shell scripts.
- `bash scripts/ci-guards.sh` and `CI=true bash scripts/ci-guards.sh` both printed "all guards passed".
- `python3 scripts/check-workflow.py` exited 0.
- `scripts/test-ci-guards.sh` reported 86 cases, 0 missed, in 48 s.

All three fixes from the last round hold:

- **WR-01.** `re.compile` now runs inside the `try`, and the handler catches `BaseException` and exits 2. All four consumers (`grep_guard`, (b), (d), (h)) treat exit 2 or higher as a failure. The new self-test case breaks guard (a)'s regex and expects `ml_grep failed (exit 2)`.
- **WR-02.** Keys now match after indentation, `{`, `[` or `,`, quoted or not. Any brace left after the `${{ }}` strip fails. `sed` runs on its own, so a `sed` failure cannot be masked.
- **IN-02.** `check-workflow.py` and the grep layer both pin `ci-${{ github.ref }}` exactly.

The workflow runs the right gates in the right order and suppresses no errors. Nothing found
makes the current CI wrong.

There is one warning. It is reproduced, not hypothetical. The grep layer is the only workflow check in CI when PyYAML is missing. That layer is a block-list, and a block-list still lets through the most likely ways to weaken a gate:

- `if: false` on a step
- `|| true` appended to a run line
- `-D warnings` dropped from clippy
- a deleted smoke step
- a deleted `permissions:` block

`check-workflow.py` rejects every one of these, so the gap only matters on the path where PyYAML is missing. Whether the ubuntu-26.04 image ships PyYAML is still unverified.

There are 10 info items. Two are new (IN-01, IN-02); IN-02 includes one false positive I confirmed. The other eight carry over from the last review. IN-02 of iteration 2 is fixed and dropped.

Scratch work went under `target/tmp/review-iter4`, which has been deleted. No tracked file was modified.

Hypotheses I checked and rejected:

- **The `ml_grep` handler catching `BaseException`.** `SystemExit` is never raised inside the `try`. A `KeyboardInterrupt` or a failed stdout flush at shutdown still ends with an exit status of 2 or higher, so the guard fails closed.
- **The brace strip `sed 's/\${{[^}]*}}//g'`.** In BRE, `{` is a literal, so the command does what it says. An expression that contains `}` leaves braces behind and fails, which is the safe direction.
- **The key-position bracket `[[:space:]{,[]`.** GNU grep parses it correctly. The `  group: ci-${{ github.ref }}` line does not trip the `ref` pattern, because there is no colon after `ref`.
- **The self-test's `local -a GUARD_ENV=(CI=true)` in `expect_fail_ci`.** It reaches `expect_fail` through bash's dynamic scoping, as intended. The WR-01 case's `sed` really produces `(?:allow|expect\s*\(|cfg_attr`, and the case's own `grep -qF` checks that.

## Warnings

### WR-01: In CI without PyYAML, the grep layer of guard (g) does not stand on its own

**File:** `scripts/ci-guards.sh:192-202` (comment), `:215-252` (grep layer), `:253-259` (skip branch); `scripts/test-ci-guards.sh:310-366`
**Issue:** The header says the content grep "is the only workflow check when PyYAML is missing in CI, so it stands on its own". It doesn't. The grep layer rejects a fixed list of substrings and keys. It checks nothing positively except the concurrency group line. Each edit below was planted in a scratch copy and then checked two ways:

- `ci-guards.sh` with `CI=true` and PyYAML hidden (the same `python3 -S` shim the self-test uses) exited 0 with `ok [g] ... (structure check skipped: PyYAML unavailable in CI)` every time.
- `check-workflow.py` exited 1 on every edit.

| Planted edit | Effect on the gate |
|---|---|
| `if: false` on the `cargo clippy` step | clippy never runs (CI-03) |
| `run: cargo clippy --all-targets --locked`, with `-- -D warnings` dropped | clippy warnings no longer fail CI |
| `run: cargo test --locked \|\| true` | test failures are swallowed |
| `run: cargo test --locked -- --test-threads=1` | defeats the parallel-test gate (the case RESEARCH used to justify exact run strings) |
| `shell: sh` on the fmt step | changes the shell the step runs in (also rejected in PyYAML mode) |
| the `Release smoke` step deleted | the protected-platform smoke (D-06) is gone |
| the top-level `permissions:` / `contents: read` lines deleted | the token falls back to the repo default (D-03) |
| `with:` / `? ref` / `: main` (YAML explicit-key syntax) on checkout | gates run against `main` (obscure, but valid YAML) |

The guards step itself must stay intact for any of these to be checked. That holds in every row, because only other lines were edited. On a runner without PyYAML, these edits therefore turn the CI gate green while gutting it.

The self-test's `expect_fail_ci` cases cover only the block-list entries. The `if: false`, `shell: sh`, run-string and deleted-step cases exist only in PyYAML mode (`expect_fail` with `-u CI`, `:254-265`), so the gap stays invisible.

The last two review rounds each found a new hole in this block-list, and each was closed one pattern at a time. A positive check would end that cycle.

**Fix:** Keep the operator's decision (no pip install, skip only when PyYAML is missing), but give the PyYAML-less path a stdlib-only allow-list instead of more block patterns. ci.yml is block style and fully pinned, so a canonical-lines comparison is cheap. One way is a fallback in `check-workflow.py` that imports PyYAML lazily:

```python
CANONICAL = [  # every non-blank, non-comment line of ci.yml, exactly
    "name: CI", "on:", "  push:", "  pull_request:", "  workflow_dispatch:",
    "permissions:", "  contents: read",
    "concurrency:", "  group: ci-${{ github.ref }}", "  cancel-in-progress: true",
    # ... through "        run: bash scripts/ci-smoke.sh"
]
def lines_check(text):
    got = [l.rstrip() for l in text.splitlines() if l.strip() and not l.lstrip().startswith("#")]
    return [] if got == CANONICAL else [f"workflow lines differ from the canonical ci.yml: {got!r}"]
try:
    import yaml
except ImportError:
    yaml = None   # main(): if yaml is None and CI is set, run lines_check and return
```

Then make `ci-guards.sh` call this mode instead of printing `skip [g-structure]`.

A smaller fix closes the realistic rows: in the skip branch, require each of the six run lines with `grep -qxF '        run: <cmd>'`, and also require the `permissions:` and `  contents: read` lines. Add `if|shell|env|defaults` (indented) to the key-position pattern. Under either fix, port the `if: false`, `|| true`, missing `-D warnings` and deleted-smoke cases to `expect_fail_ci`.

## Info

### IN-01 (new): check-workflow.py silently accepts the forbidden keys and `defaults` in scopes it never reports them for

**File:** `scripts/check-workflow.py:47-49, 81-82, 88-89, 102-116`
**Issue:** `unexpected()` always removes `FORBIDDEN_KEYS` and `"defaults"` from its result, because they are "reported separately". In fact they are reported separately only for the job and its steps (`FORBIDDEN_KEYS`), and for the workflow and the job (`defaults`). I verified with PyYAML present that `check-workflow.py` exits 0 for each of these:

- `if: false` under `concurrency:`
- a top-level `continue-on-error: true`
- a step-level `defaults: x`

GitHub rejects all three as schema errors, so the workflow would not run at all. A broken run shows up there, so this is not a bypass. Still, the checker's diagnostics do not match its own allow-list comment (`:31-33`).

**Fix:** Pass the separately reported keys explicitly, for example `unexpected(obj, allowed, reported=())`. Use `reported=FORBIDDEN_KEYS + ("defaults",)` only for the job, `("defaults",)` only for the workflow, and `FORBIDDEN_KEYS` only for steps.

### IN-02 (new, extends the carry-over text-grep note): guard (g) fails on ordinary English in ci.yml comments

**File:** `scripts/ci-guards.sh:215-220`
**Issue:** The case-insensitive pattern includes `server\.`, `rustflags`, `working-directory` and `<key>: write`. I verified that adding the comment line `# Starts the release server.` above the smoke step makes guard (g) fail with "forbidden workflow content", while `check-workflow.py` exits 0. A step named `name: Write ...` would fail too, because `-i` also matches `write`. This fails closed, and the error message prints the offending line, so the impact is limited to confusion.

**Fix:** Run the content grep on the comment-stripped text, which the brace check already builds with `sed`. Add `s/^[[:space:]]*#.*//` to the `sed` script, keeping line numbers. Alternatively, note in the header that workflow comments must avoid these words.

### IN-03: Guard (d) source scan misses `Platform::new_custom(.., true, ..)` passed through `RuntimeOptions::v8_platform` (carry-over, iteration-2 IN-01)

**File:** `scripts/ci-guards.sh:166`
**Issue:** The current regex matches `\bnew_custom_platform\b`, but not the method `v8::Platform::new_custom(thread_pool_size, idle_task_support, unprotected: bool, impl PlatformImpl)`. It also does not match a platform passed through `RuntimeOptions { v8_platform: Some(..) }`, which never calls `init_platform`. This is unlikely to happen by accident.

**Fix:** Use `\bnew_custom\w*` and add `\bv8_platform\s*:\s*Some\b`.

### IN-04: test-ci-guards.sh refuses to run when `target/` is a symlink, and blames an unset variable (carry-over, iteration-2 IN-03)

**File:** `scripts/test-ci-guards.sh:14-19`
**Issue:** `realpath -m` resolves a `target -> elsewhere` symlink, so the default scratch path fails the `"$ROOT"/target/tmp/?*` check. The script then reports "CI_GUARDS_SCRATCH must resolve ..." even though the variable is unset. This fails safe.

**Fix:** Compare against `"$(realpath -m -- "$ROOT/target/tmp")"/?*`, and name the default path in the message when the variable is unset.

### IN-05: Guard (e) can false-positive on a same-version v8 (carry-over)

**File:** `scripts/ci-guards.sh:178-181`
**Issue:** `cargo tree -d` can list a crate twice at the same version. When it does, any `v8` line fails the guard.

**Fix:** Fail only when `grep -E '^(v8|rusty_v8) v' | sort -u | wc -l` is greater than 1.

### IN-06: Guard (f) misses whitespace variants and does not scan `tests/` (carry-over)

**File:** `scripts/ci-guards.sh:188-190`
**Issue:** `Deno.core.ops ( )` passes the check. Only `requests` and `src` are scanned, even though `rs_roots` would include `tests/`.

**Fix:** Use `Deno\.core\.ops[[:space:]]*\([[:space:]]*\)` and scan `requests "${rs_roots[@]}"`.

### IN-07: The Rust-source text greps reject harmless comments and strings, and `[lints] workspace = true` fails (c) (carry-over)

**File:** `scripts/ci-guards.sh:80-82, 91, 129-133, 188-190`
**Issue:** A doc comment that mentions `#[allow(..)]`, or a string that contains `rs3_get`, fails guard (a) or (f). `[lints] workspace = true` is reported as "not a table". This is acceptable for a single-crate repo, but it is undocumented.

**Fix:** Note it in the header.

### IN-08: Smoke port TOCTOU and `RS3_BIN` lookup mismatch (carry-over)

**File:** `scripts/ci-smoke.sh:23, 71, 76`
**Issue:** The port is chosen by binding and then closing a socket, which is racy. `[[ -x "$BIN" ]]` checks a path relative to the current directory, but `env` searches `PATH` when `RS3_BIN` has no slash.

**Fix:** Use `[[ $BIN == */* ]] || BIN=./$BIN`, and retry once if the bind fails.

### IN-09: check-workflow.py's `continue-on-error`/`env` handling is never exercised directly, and the actions-rs check is unreachable (carry-over, iteration-2 IN-08 + IN-09)

**File:** `scripts/test-ci-guards.sh:244-247`; `scripts/check-workflow.py:145-146`
**Issue:** The content grep catches the step-level `continue-on-error` and `env` cases before the checker runs, so a regression in the checker's own handling would go unnoticed. Separately, `uses` must already equal `EXPECTED_USES`, so the `actions-rs/` check is dead code.

**Fix:** Add two `expect_wf_fail` cases (step `continue-on-error: true`, and step `env:` with one key in block style). Remove the actions-rs check, or comment that it is redundant on purpose.

### IN-10: `cancel-in-progress` cancels superseded runs on `main`, and same-repo PR branches run twice (carry-over)

**File:** `.github/workflows/ci.yml:11-13`
**Issue:** Rapid pushes to `main` leave earlier `main` commits with no verdict. A same-repo PR runs CI twice, once for `push` and once for `pull_request`, because the two events have different refs.

**Fix:** Accept and document the trade-off. Changing it would need a second `${{` expression, which the grep layer and `check-workflow.py` now forbid.

---

_Reviewed: 2026-10-20T22:49:38Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
