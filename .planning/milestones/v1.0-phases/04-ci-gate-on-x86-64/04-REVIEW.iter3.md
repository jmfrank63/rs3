---
phase: 04-ci-gate-on-x86-64
reviewed: 2026-10-20T22:37:56Z
depth: standard
iteration: 2
files_reviewed: 5
files_reviewed_list:
  - .github/workflows/ci.yml
  - scripts/check-workflow.py
  - scripts/ci-guards.sh
  - scripts/ci-smoke.sh
  - scripts/test-ci-guards.sh
findings:
  critical: 0
  warning: 2
  info: 10
  total: 12
status: issues_found
---

# Phase 4: Code Review Report (re-review after fix iteration 1)

**Reviewed:** 2026-10-20T22:37:56Z
**Depth:** standard
**Files Reviewed:** 5
**Status:** issues_found

## Summary

This re-review covers the tree after fix commits fcbd7f3..5940638 and the operator commit
fd8f43f. Moving the runner to ubuntu-26.04 is the deliberate D-01 amendment, so it is not
reported. Also not reported, as stated in the brief: tag/branch action refs (T-04-01), no
push, and no pip install or actionlint download.

Everything was run locally. The scripts resolve to GNU grep 3.11, bash 5.2 and Python 3.12.

- `scripts/ci-guards.sh` printed "all guards passed", both locally and with `CI=true`.
- `scripts/test-ci-guards.sh` reported 76 cases, 0 missed, in about 42 s.
- `python3 scripts/check-workflow.py` exited 0.
- `bash scripts/ci-smoke.sh` printed OK against the current `target/release/rs3`.
- `bash -n` passed on all three shell scripts.

All eight warnings from the previous review are really fixed. I checked each fix against
the code and re-ran its planted cases. The workflow runs the right gates in the right order
and suppresses no errors. Nothing found makes the current CI wrong.

There are two new warnings, both reproduced in a scratch copy under
`target/tmp/review-iter3` (since deleted):

1. `ml_grep`, the scanner behind guards (a), (b), (d) and (h), crashes with Python's exit
   status 1. That is the same status it uses for "no hits", so the guards fail open on any
   unexpected exception. A one-character regex typo turns guard (a) green while
   `#[allow(dead_code)]` is planted.
2. The grep layer that guard (g) relies on in CI when PyYAML is missing does not recognise
   YAML flow mappings. `with: { ref: main }`, `pull_request: { paths-ignore: [...] }`,
   `permissions: { contents: write }` and `push: { branches: [main] }` all pass it with exit 0.

The rest are info items: three new ones and seven carried over from the previous review.

Hypotheses I checked and rejected:

- `#[cfg_attr(rustfmt, rustfmt_skip)]` gets past guard (h) and `cargo fmt --check`, but
  clippy `-D warnings` rejects it with `deprecated_cfg_attr`, so the CI gate still holds.
- `kill -0` on a crashed server stays false, because bash reaps the child. The fixer's
  139-path verification stands.
- `cargo tree -e features,normal` does not leak dev-dependency features.
- `check-workflow.py` allow-lists job, step and `with:` keys correctly, and requires the run
  strings to match exactly and in order.

## Warnings

### WR-01: `ml_grep` fails open, so guards (a), (b), (d) and (h) pass if the scanner throws anything but OSError

**File:** `scripts/ci-guards.sh:37, 55-67` (consumers: `:20-25` grep_guard, `:89-95` (b), `:164-170` (d), `:255-260` (h))
**Issue:** The helper's contract (comment at `:31-33`) is "exits 0 on hits, 1 on none, 2 on an
error ... (grep_guard fails closed on that)". But `re.compile(sys.argv[1])` sits outside the
`try`, and the `try` catches only `OSError`. Any other exception (`re.error`, a
`UnicodeEncodeError` when printing, and so on) ends the interpreter with Python's default
exit status **1**. Every consumer reads 1 as "no hits" and prints `ok`. The traceback goes
into `hits`/`b_src`/`d_src`/`h_src` and is discarded.

I reproduced this in a scratch copy. I planted `#[allow(dead_code)] fn planted() {}` in
`src/lib.rs` and dropped one `)` from guard (a)'s pattern (`(?:allow|expect` instead of
`(?:allow|expect)`). Result: `ci-guards: ok   [a] no #[allow]/...`. Run directly, the helper
exits 1 with `re.error: missing ), unterminated subpattern`, and also on a variable-width
look-behind.

Regex edits to these guards are the routine maintenance path. In CI, a bad edit silently
disables the guard. `test-ci-guards.sh` would notice, but it is local only and not a CI step.
**Fix:** Compile inside the `try` and map every exception to exit 2:
```python
try:
    pat = re.compile(sys.argv[1])
    for root in sys.argv[2:]:
        for p in files(root):
            ...
except Exception as e:  # any scanner failure must fail the guard, never pass it
    print(f"ml_grep: {type(e).__name__}: {e}", file=sys.stderr)
    sys.exit(2)
sys.exit(0 if hits else 1)
```
Optionally add a self-test case that runs a copy of `ci-guards.sh` with a broken pattern and
expects `FAIL [a] ml_grep failed (exit 2)`.

### WR-02: Guard (g)'s grep-only layer (CI without PyYAML) misses YAML flow mappings

**File:** `scripts/ci-guards.sh:207-210`
**Issue:** These patterns are anchored to block-style YAML:
- `^[[:space:]]+(ref|repository)[[:space:]]*:`
- `^[[:space:]]+(branches|...|types)[[:space:]]*:`
- `^[[:space:]]+permissions[[:space:]]*:`
- `...:[[:space:]]*write[[:space:]]*(#.*)?$`

Flow mappings are ordinary workflow syntax. `with: { fetch-depth: 0 }` is common, and the
previous review itself wrote `with: { ref: main }`. Each of these passed `ci-guards.sh` with
`CI=true` and PyYAML hidden (exit 0, no `FAIL [g]`):

| Planted line | Effect |
|---|---|
| `        with: { ref: main }` (on checkout) | every gate runs against `main`, not the pushed commit |
| `  pull_request: { paths-ignore: ['**'] }` | PRs never run CI |
| `permissions: { contents: write }` | defeats D-03 |
| `  push: { branches: [main] }` | branch pushes stop running CI |

`check-workflow.py` rejected all four (rc=1), so this only matters on the path the code
explicitly supports: CI with no PyYAML. On that path the grep is the only workflow check, and
the self-test's `expect_fail_ci` cases cover only the block-style spelling of the same
violations. Whether the ubuntu-26.04 image ships PyYAML is still unverified.
**Fix:** Anchor on a key position instead of the line start, so `{` and `,` count as key
separators:
```bash
k='(^|[[:space:]{,])'
g_pat+="|${k}(ref|repository)[[:space:]]*:"
g_pat+="|${k}(branches|branches-ignore|tags|tags-ignore|paths|paths-ignore|types)[[:space:]]*:"
g_pat+="|^[[:space:]]+permissions[[:space:]]*:|${k}permissions[[:space:]]*:[[:space:]]*\{"
g_pat+="|${k}[a-z-]+[[:space:]]*:[[:space:]]*write([[:space:]]*[,}#]|[[:space:]]*$)"
```
Check that `  group: ci-${{ github.ref }}` still passes: `ref }}` has no colon. Then add
`expect_fail_ci` cases for the four flow-style lines above. The alternative is to reject `{`
in the workflow altogether, except on the group line.

## Info

### IN-01: Guard (d) source scan misses `Platform::new_custom(.., true, ..)` passed through `RuntimeOptions::v8_platform`

**File:** `scripts/ci-guards.sh:164`
**Issue:** The scan catches the method form of the unprotected constructor
(`Platform::new_unprotected`), but not the method form of the custom one. v8 150.4.0 has
`pub fn Platform::new_custom(thread_pool_size, idle_task_support, unprotected: bool, impl PlatformImpl)`
(`src/platform.rs:450`). `PlatformImpl` has default method bodies, so `struct P; impl v8::PlatformImpl for P {}`
is enough. deno_core 0.412 also takes a platform through `RuntimeOptions::v8_platform`
(`runtime/jsruntime.rs:606, 756`), which never goes through `init_platform`. This snippet gets
no hits from the guard's regex:

`v8_platform: Some(deno_core::v8::Platform::new_custom(0, false, true, P).make_shared())`

I verified the regex, but did not compile the snippet. It is unlikely to be written by
accident, which is why this is info.
**Fix:** Use `\bnew_custom\w*` instead of `\bnew_custom_platform\b`, and add
`\bv8_platform\s*:\s*Some\b`.

### IN-02: check-workflow.py accepts any constant concurrency group

**File:** `scripts/check-workflow.py:76-80`
**Issue:** The group is only checked for truthiness. `group: ci` passed both the structure check
and the `${{` grep (verified). With `cancel-in-progress: true`, every push or PR on any ref then
cancels the in-progress run on every other ref, including `main`.
**Fix:** `if conc.get("group") != "ci-${{ github.ref }}": errs.append(...)`.

### IN-03: test-ci-guards.sh refuses to run when `target/` is a symlink, and blames an unset variable

**File:** `scripts/test-ci-guards.sh:14-19`
**Issue:** `realpath -m` resolves the `target` symlink, which is common with shared or large
target directories. The default scratch path then no longer matches `"$ROOT"/target/tmp/?*`,
and the script exits 2 with "CI_GUARDS_SCRATCH must resolve ..." even though the variable is
unset (verified with `target -> elsewhere`). This fails safe, but the message is misleading.
**Fix:** Compare against `"$(realpath -m -- "$ROOT/target/tmp")"/?*`, and name the default path
in the message when `CI_GUARDS_SCRATCH` is unset.

### IN-04: Guard (e) can false-positive on a same-version v8 (carry-over, previous IN-01)

**File:** `scripts/ci-guards.sh:176-179`
**Issue:** `cargo tree -d` lists same-version duplicates. Any `v8` line fails, even when only one
version is present.
**Fix:** Fail only when `grep -E '^(v8|rusty_v8) v' | sort -u | wc -l` is greater than 1.

### IN-05: Guard (f) misses whitespace variants and does not scan `tests/` (carry-over, previous IN-03)

**File:** `scripts/ci-guards.sh:186-188`
**Issue:** `Deno.core.ops ( )` passes. Only `requests` and `src` are scanned, although
integration tests in `tests/` (already in `rs_roots` when it exists) would embed JS strings.
**Fix:** Use `Deno\.core\.ops[[:space:]]*\([[:space:]]*\)` and scan `requests "${rs_roots[@]}"`.

### IN-06: The text greps reject harmless comments and strings, and `[lints] workspace = true` fails (c) (carry-over, previous IN-04)

**File:** `scripts/ci-guards.sh:78-80, 89, 128-130, 186-188`
**Issue:** A doc comment mentioning `#[allow(..)]`, or a string containing `rs3_get`, fails (a)
or (f). Under (c), `[lints] workspace = true` is reported as "not a table". This is acceptable
for a single-crate repo, but it is undocumented.
**Fix:** Note it in the header.

### IN-07: Smoke port TOCTOU and `RS3_BIN` lookup mismatch (carry-over, rest of previous IN-05)

**File:** `scripts/ci-smoke.sh:23, 71, 76`
**Issue:** The port is chosen by bind-then-close, which is racy. `[[ -x "$BIN" ]]` checks a path
relative to the cwd, while `env` searches `PATH` when `RS3_BIN` has no slash.
**Fix:** `[[ $BIN == */* ]] || BIN=./$BIN`, and retry once if the bind fails.

### IN-08: check-workflow.py's FORBIDDEN_KEYS handling of `continue-on-error`/`env` is never exercised directly (carry-over, rest of previous IN-06)

**File:** `scripts/test-ci-guards.sh:236-239`
**Issue:** The content grep catches these two cases before `check-workflow.py` runs, so a
regression in the checker's own handling would go unnoticed. The `if:` and `shell:` cases do
reach the checker.
**Fix:** Add two `expect_wf_fail` cases, for example step-level `continue-on-error: true` and
step `env: {A: b}`.

### IN-09: The actions-rs check is unreachable (carry-over, previous IN-07)

**File:** `scripts/check-workflow.py:140-141`
**Issue:** `uses` must already equal `EXPECTED_USES` exactly, so this check is dead code.
**Fix:** Remove it, or comment that it is redundant on purpose.

### IN-10: `cancel-in-progress` cancels superseded runs on `main`, and same-repo PR branches run twice (carry-over, previous IN-08)

**File:** `.github/workflows/ci.yml:11-13`
**Issue:** Rapid pushes to `main` leave earlier `main` commits with no verdict. `push` and
`pull_request` use different refs, so a same-repo PR runs CI twice.
**Fix:** Accept and document the trade-off, or use
`cancel-in-progress: ${{ github.ref != 'refs/heads/main' }}` together with matching updates to
check-workflow.py and the `${{` grep.

---

_Reviewed: 2026-10-20T22:37:56Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
