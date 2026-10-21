---
phase: 04-ci-gate-on-x86-64
reviewed: 2026-10-20T22:06:15Z
depth: standard
files_reviewed: 5
files_reviewed_list:
  - .github/workflows/ci.yml
  - scripts/check-workflow.py
  - scripts/ci-guards.sh
  - scripts/ci-smoke.sh
  - scripts/test-ci-guards.sh
findings:
  critical: 0
  warning: 8
  info: 8
  total: 16
status: issues_found
---

# Phase 4: Code Review Report

**Reviewed:** 2026-10-20T22:06:15Z
**Depth:** standard
**Files Reviewed:** 5
**Status:** issues_found

## Summary

I reviewed the CI workflow, the guard script and its planted-violation self-test, the
workflow structure checker, and the release smoke. I ran all of them locally on aarch64.
The scripts get GNU grep 3.11 and bash 5.2, which is what ubuntu-24.04 has. Results:
`ci-guards.sh` printed "all guards passed", `test-ci-guards.sh` reported "32 cases, 0 missed",
and `ci-smoke.sh` printed OK. The workflow as it stands runs the right gates, in the right
order, without suppressing any errors. I found no defect that makes the current CI wrong.

I then planted more violations in a scratch copy under `target/tmp` (since deleted). Many of
them get past the guards with exit 0 and no FAIL line. Each one below was confirmed by
running the copied `ci-guards.sh`. Where it mattered, I also confirmed with `rustc`,
`rustdoc` or `rustfmt` that the planted code compiles, is honoured, and survives formatting:

- A multi-line `#[cfg_attr(..., allow(...))]`. This is rustfmt's own layout for long
  attributes, so `cargo fmt --check` keeps it as is.
- `#[cfg_attr(..., ignore)]` on a test, a `~~~ignore` doc fence, and a ```` ```rust ignore ````
  doc fence. rustdoc reports each one as "ignored".
- `[lib] test = false` / `doctest = false` in Cargo.toml.
- A lint suppression in a symlinked module file. `grep -r` does not follow symlinks.
- `rustfmt.toml` with `disable_all_formatting = true`. rustfmt 1.99 then exits 0 on
  unformatted code.
- In ci.yml: `actions/checkout` with `with: ref: main`, `working-directory: experiments/hashring`
  on the cargo test step (that crate has its own Cargo.lock, so `--locked` passes), job-level
  `permissions: write-all`, `pull_request: paths-ignore: ['**']`, and `types: [closed]`.
- In the CI path without PyYAML: a lowercase `server.host` workflow env (the config loader
  accepts any case) and `${{ github.head_ref }}` interpolation.

There is also a diagnostics problem in the smoke. V8 isolates are created lazily on the
`rs3-js-*` workers, so a PKU crash would show up at the first `PATCH`, not at startup. The
script would report a bare "transport error" and never print the documented "status 139"
trigger for D-07.

The deliberate items from the brief are not reported: tag refs instead of SHA pins, no push,
skipping the structure check when PyYAML is missing in CI, and the deleted actions-rs.yml.
WR-05 covers only how weak the grep layer that remains in that skip path is.

## Warnings

### WR-01: Guard (a) misses multi-line `cfg_attr` suppression, symlinked files and whitespace-split attributes

**File:** `scripts/ci-guards.sh:31-34` (also `:38-39`, `:92-94`, which use the same `-r` scan)
**Issue:** The regex is applied one line at a time, so this gets through:
```rust
#[cfg_attr(
    all(test, feature = "a_rather_long_feature_name_for_this_example"),
    allow(clippy::unwrap_used, clippy::expect_used, dead_code, unused_variables)
)]
fn y() {}
```
This is rustfmt's canonical output for an attribute longer than 100 columns, so the later
`cargo fmt --check` step does not reject it. I verified it: guards rc=0, and rustfmt left the
attribute unchanged. Separately, `grep -r` skips symlinks it finds while recursing.
`src/z.rs -> ../outside/z.rs` holding `#[allow(dead_code)]` is compiled by rustc but never
scanned (verified: rc=0). The pattern `#!?\[` also rejects valid token spacing such as
`# [allow(..)]` / `# ! [allow(..)]`. rustfmt normalises most of these, but not inside
`#[rustfmt::skip]` items or macro bodies it cannot parse.
**Fix:** Scan with `grep -R` (follow symlinks). Match whole files in multi-line mode and allow
whitespace between the tokens, for example with `grep -Pzo` or a small Python pass over every
`*.rs` file:
```bash
grep_guard a "..." "lint-suppression attribute found:" \
  -RnzP '#\s*!?\s*\[\s*(allow|expect)\s*\(|cfg_attr\s*\((?:[^()]|\((?:[^()]|\([^()]*\))*\))*\b(allow|expect)\s*\(' \
  "${rs_roots[@]}"
```
Alternatively, strip newlines inside `#[...]` groups with Python before matching.

### WR-02: Guard (b) misses common ways to ignore or disable tests

**File:** `scripts/ci-guards.sh:36-39`
**Issue:** `#\[[[:space:]]*ignore|```[[:alnum:]_,]*ignore` misses all of the following. Each
was verified with guards rc=0 and rustdoc/libtest reporting "ignored":
- `#[cfg_attr(not(feature = "never"), ignore)]` on a `#[test]`
- a `~~~ignore` doc fence (CommonMark tilde fence)
- ```` ```rust ignore ```` (rustdoc splits lang strings on spaces as well as commas)
- `[lib] test = false`, `doctest = false`, `autotests = false` or `harness = false` in
  Cargo.toml, which silently drop whole test targets (verified: rc=0)
- `#[cfg(any())]` / `#[cfg(FALSE)]` on a test module

CI-03 relies on `cargo test --locked` running the V8-backed tests, so each of these quietly
weakens that gate.
**Fix:** Extend the regex and add a Cargo.toml check to the existing tomllib block (c):
```bash
-RnE '#\[[[:space:]]*ignore|cfg_attr[[:space:]]*\(.*\bignore\b|(```|~~~)[^`~]*\bignore\b|#\[[[:space:]]*cfg[[:space:]]*\([[:space:]]*(any[[:space:]]*\([[:space:]]*\)|FALSE)'
```
```python
data = tomllib.load(f)
for sect in ("lib", "bin", "test", "bench", "example"):
    for t in ([data[sect]] if isinstance(data.get(sect), dict) else data.get(sect, [])):
        for k in ("test", "doctest", "harness"):
            if t.get(k) is False: errs.append(f"[{sect}] {k} = false")
if data.get("package", {}).get("autotests") is False: errs.append("autotests = false")
```

### WR-03: Guard (d) checks only the Cargo feature; source code can select the unprotected platform directly

**File:** `scripts/ci-guards.sh:66-78`
**Issue:** The invariant is "the release binary keeps the protected V8 platform". deno_core
0.412 picks the unprotected platform only when it is passed `None`
(`runtime/setup.rs:200-202`, `cfg!(any(test, feature = "unsafe_use_unprotected_platform"))`).
`src/js/mod.rs:11` could just as well call
`JsRuntime::init_platform(Some(deno_core::v8::new_unprotected_default_platform(0, false).make_shared()))`.
`new_unprotected_default_platform` is a safe `pub fn` (`v8-150.4.0/src/platform.rs:328`), so
`unsafe_code = "forbid"` does not block it. The release binary would then run unprotected with
guard (d) green, and the smoke would pass, so the D-07 evidence would be falsified.
**Fix:** Add a source grep to guard (d), and relax it together with the feature move in the
D-07 fallback:
```bash
grep_guard d2 "no direct unprotected platform in src" "unprotected V8 platform selected in source:" \
  -RnE 'new_unprotected_default_platform|unprotected_platform' "${rs_roots[@]}"
```
Folding it into tag `d` keeps the "[a]..[h] one-to-one" contract.

### WR-04: check-workflow.py accepts keys that redirect or neuter every gate

**File:** `scripts/check-workflow.py:30, 46-49, 77-83`
**Issue:** `FORBIDDEN_KEYS` covers only `continue-on-error`, `env`, `if` and `shell`, and only the
push trigger is checked for filters. All of the following passed the full structure check
(PyYAML present, guards rc=0):
- `actions/checkout@v7` with `with: { ref: main }` (or `repository: other/repo`). Every gate then
  runs against a different commit than the one pushed. The PR is "green" without testing its code.
- `working-directory: experiments/hashring` on the `cargo test` step. That crate has a
  committed Cargo.lock, so `cargo test --locked` succeeds against unrelated code.
- Job-level `permissions: write-all`. This overrides the checked workflow-level
  `{contents: read}` and defeats D-03.
- `pull_request: { paths-ignore: ['**'] }` or `types: [closed]`. PRs then never run CI.
- Job-level `container:` (any image) and `services:`.
**Fix:** Make job and step keys allow-lists instead of a deny-list, and validate `with:` and the
PR trigger:
```python
JOB_KEYS = {"name", "runs-on", "timeout-minutes", "steps"}
STEP_KEYS = {"name", "uses", "with", "run"}
ALLOWED_WITH = {"actions/checkout@v7": set(), "dtolnay/rust-toolchain@1.99.1": {"components"},
                "Swatinem/rust-cache@v2": set()}
if set(job) - JOB_KEYS: errs.append(f"job: unexpected keys {sorted(set(job) - JOB_KEYS)}")
for i, s in enumerate(steps):
    if set(s) - STEP_KEYS: errs.append(f"step {i}: unexpected keys {sorted(set(s) - STEP_KEYS)}")
    if "uses" in s and set(s.get("with") or {}) - ALLOWED_WITH.get(s["uses"], set()):
        errs.append(f"step {i}: unexpected with: inputs for {s['uses']}")
if triggers.get("pull_request") not in (None, {}):
    errs.append("pull_request trigger must not be filtered")
```

### WR-05: In CI the forbidden-content grep is the only workflow check, and it is case-sensitive and incomplete

**File:** `scripts/ci-guards.sh:105` (with `:107-110`)
**Issue:** With `CI` set and no PyYAML, guard (g) relies on
`grep -nE 'continue-on-error|SERVER\.|RUSTFLAGS|cap-lints|\$\{\{ *github\.event\.'`. Skipping the
structure check is deliberate, but this remaining layer does not cover what it claims to:
- `src/config.rs:7-8,16,72-77` lowercases every env key and accepts `SERVER.HOST` in any case.
  `server.host: 0.0.0.0` under workflow `env:` therefore passes (verified: rc=0 with `CI=true`
  and the PyYAML shim) and breaks CI-02.
- `${{ github.head_ref }}`, the classic branch-name script-injection source, is not matched
  (verified: rc=0). Neither are `github.event_path`-style variants or
  `${{github.event` written with a tab.
- `working-directory`, `permissions` and checkout `ref:` (WR-04) are also unchecked.

I could not verify from this host whether the ubuntu-24.04 image's `/usr/bin/python3` ships
PyYAML. If it does not, this weaker layer is the only workflow check on every CI run.
**Fix:** Use `grep -inE` and widen the pattern, for example
`'continue-on-error|server\.|rustflags|cap-lints|working-directory|permissions:.*write|\$\{\{[[:space:]]*github\.(event|head_ref)'`.
Allow the single top-level `permissions:` line explicitly. Better still, make the structure
check independent of PyYAML (JSON via `python3 -c` cannot parse YAML, but the workflow is
small enough for a strict line-based allow-list).

### WR-06: Nothing guards the `cargo fmt --check` gate against being disabled by configuration

**File:** `scripts/ci-guards.sh:118-123`
**Issue:** Guard (h) blocks a `.cargo/config*` that could weaken clippy, but the fmt gate has
an equivalent bypass. A repo-root `rustfmt.toml` / `.rustfmt.toml` containing
`disable_all_formatting = true` makes `cargo fmt --check` exit 0 on unformatted code. I verified
this with rustfmt from 1.99.1: rc=1 without the file, rc=0 with it, and the guards rc=0. The
same applies to item-level `#[rustfmt::skip]` sprinkled over source. This bypass mirrors the
lint-suppression directive the guards exist to enforce.
**Fix:**
```bash
shopt -s nullglob; fmtcfg=(rustfmt.toml .rustfmt.toml); shopt -u nullglob
for f in "${fmtcfg[@]}"; do [[ -e $f ]] && grep -qE '^[[:space:]]*(disable_all_formatting|ignore)[[:space:]]*=' "$f" \
  && fail h "rustfmt config disables formatting: $f"; done
```
Add `rustfmt::skip` to the guard (a) pattern, or to a new tag.

### WR-07: The smoke never reports the PKU crash it exists to detect; it surfaces as a bare "transport error"

**File:** `scripts/ci-smoke.sh:41-53, 59-67, 73-74` (comments at `:13-15`, `.github/workflows/ci.yml:50-53`, `scripts/ci-guards.sh:68-70`)
**Issue:** `src/main.rs:16` initialises the platform, and
`JsExecutor::start` (`src/js/executor.rs:33-80`) spawns workers that build only a tokio runtime.
No `JsRuntime`/isolate exists until the first job, and readiness uses `GET /`, which never
touches V8. A protected-platform PKU fault on the x86_64 runner would therefore kill the
process during `PATCH /key/m`. The status-139 check sits only in the readiness loop, so the
script prints `PATCH /key/m: transport error` (curl: empty reply). The server's exit status is
then swallowed by `cleanup`'s `wait ... || true`. The workflow comment, the smoke header and
guard (d)'s comment all tell the operator to look for "status 139 at startup". That never
appears, so the D-07 decision has to be made from a misleading message. The gate still fails,
so CI behaviour is correct, but the diagnosis is wrong.
**Fix:** On any request failure, report whether the server died and with what status:
```bash
server_status() {
  if ! kill -0 "$PID" 2>/dev/null; then local st=0; wait "$PID" || st=$?; PID=""
    echo " (server exited with status ${st}${st:+; 139 = SIGSEGV, see D-07})"; fi
}
out=$(curl ...) || fail "$1 $2: transport error$(server_status)"
```
Also update the comments to say "at the first script execution", not "at startup".

### WR-08: test-ci-guards.sh runs `rm -rf` on an unvalidated, environment-supplied path

**File:** `scripts/test-ci-guards.sh:12, 24, 74, 187` (and `:32`)
**Issue:** `SCRATCH="${CI_GUARDS_SCRATCH:-...}"` goes straight into `rm -rf "$SCRATCH"`.
`CI_GUARDS_SCRATCH=.` (relative to the repo root, after `cd "$ROOT"`), `CI_GUARDS_SCRATCH=..` or a
mistyped `$HOME` deletes the working tree or more before anything is copied. A relative value
also breaks the shims. `run_guards` does `cd "$SCRATCH"` and then prepends the still-relative
`$SCRATCH/shim` to PATH, which now resolves under the new cwd. The cargo and python3 shims are
then not found, and the (e) and PyYAML cases fail for the wrong reason. There is no trap, so an
interrupted run leaves the scratch copy behind.
**Fix:**
```bash
SCRATCH="$(realpath -m "${CI_GUARDS_SCRATCH:-$ROOT/target/tmp/guard-selftest}")"
case "$SCRATCH" in "$ROOT"/target/*) ;; *) echo "CI_GUARDS_SCRATCH must be under $ROOT/target" >&2; exit 2;; esac
trap 'rm -rf "$SCRATCH"' EXIT
```

## Info

### IN-01: Guard (e) can false-positive on a same-version v8

**File:** `scripts/ci-guards.sh:80-88`
**Issue:** `cargo tree -d --target all -e normal,build,dev` lists same-version duplicates. Here it
currently prints `actix-router v0.5.4`, `libc v0.2.191`, `memchr v2.8.3` and others twice. Any
`v8` line fails the guard, so a single v8 version built twice (host/target or a feature split)
would be reported as "duplicate v8".
**Fix:** Count distinct versions:
`grep -E '^(v8|rusty_v8) v' <<<"$dups" | sort -u | wc -l` and fail only when the count is greater than 1.

### IN-02: Guard (g) treats grep errors as "no hits" and reports only the first failing layer

**File:** `scripts/ci-guards.sh:105-116`
**Issue:** `elif hits=$(grep ...)` treats exit 2 (a read error) the same as exit 1, so it does not
fail closed the way `grep_guard` does. The `elif` chain also stops at the first problem. That
contradicts the header claim that "one run shows every violation", because forbidden content
hides structure errors.
**Fix:** Capture `rc` as `grep_guard` does, and run the structure check even when the content grep hits.

### IN-03: Guard (f) does not cover whitespace variants or `tests/`

**File:** `scripts/ci-guards.sh:90-94`
**Issue:** `Deno.core.ops ( );` passes (verified). `tests/` (already in `rs_roots` when it
exists) is not scanned, although integration tests would embed JS strings.
**Fix:** Use `Deno\.core\.ops[[:space:]]*\([[:space:]]*\)` and scan `requests "${rs_roots[@]}"`.

### IN-04: Strict greps reject harmless text, and workspace lints are rejected

**File:** `scripts/ci-guards.sh:32-39, 43-64, 92-94`
**Issue:** A doc comment such as "never write `#[allow(..)]`" or a string containing `rs3_get`
fails (a), (b) or (f). In (c), `[lints] workspace = true` is reported as "not a table" and
fails the unwrap/expect check. This is acceptable for a single-crate repo, but it is not
documented.
**Fix:** Document this in the header, or skip `//` comment lines in (a) and (b).

### IN-05: Smoke robustness nits

**File:** `scripts/ci-smoke.sh:20, 26, 33/38, 45-54`
**Issue:**
- (`:20`) Picking a port by bind-then-close is a TOCTOU race.
- (`:45-54`) The message says "after 30s", but the worst case is 60 × (2 s curl timeout +
  0.5 s) ≈ 150 s.
- (`:26`) `cleanup` sends SIGTERM and then waits with no deadline. actix's graceful shutdown can
  take up to 30 s, and a hung server blocks until the 45-minute job timeout.
- (`:33` vs `:38`) `[[ -x "$BIN" ]]` checks the cwd, but `env` searches PATH when `RS3_BIN` has no slash.
**Fix:** Retry on a bind failure, or let the server bind port 0 and report its port. Use a
deadline (`SECONDS`) for readiness. Escalate to `kill -9` after a few seconds. Normalise
`BIN` to a path (`[[ $BIN == */* ]] || BIN=./$BIN`).

### IN-06: Self-test gaps

**File:** `scripts/test-ci-guards.sh:23-27, 32, 136-158`
**Issue:**
- `fresh()` ignores `cp` failures and never copies `tests/ benches/ examples/ build.rs`, so the
  control can diverge from the real tree.
- `run_guards` does not unset an inherited `CI_GUARDS_ROOT`. If it is set, the copied guards
  check the real tree, every planted case is MISSED, and the control still passes.
- The `continue-on-error`, `SERVER.HOST` and `github.event` cases are all caught by the
  content grep before `check-workflow.py` runs, so the checker's own handling of those keys is
  never exercised.
**Fix:** Use `cp ... || exit 1`, copy the optional roots when they exist, add
`-u CI_GUARDS_ROOT` to `run_guards`, and add cases that invoke `python3 scripts/check-workflow.py`
directly.

### IN-07: Unreachable actions-rs check

**File:** `scripts/check-workflow.py:98-99`
**Issue:** `uses` must already equal `EXPECTED_USES` exactly (line 82), so an `actions-rs/*` ref
always produces the earlier error. This check is dead code.
**Fix:** Remove it, or keep it as an explicit message but note that it is redundant.

### IN-08: Concurrency cancels superseded runs on main too

**File:** `.github/workflows/ci.yml:11-13`
**Issue:** `cancel-in-progress: true` for every ref means rapid pushes to `main` leave earlier
main commits with no CI verdict. A same-repo PR branch also runs twice, once for `push` and
once for `pull_request`, because the refs differ.
**Fix:** Consider `cancel-in-progress: ${{ github.ref != 'refs/heads/main' }}`, and update
check-workflow.py to match. Accepting the trade-off is also fine if it is documented.

---

_Reviewed: 2026-10-20T22:06:15Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
