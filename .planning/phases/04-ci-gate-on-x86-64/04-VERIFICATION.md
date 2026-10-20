---
phase: 04-ci-gate-on-x86-64
verified: 2026-10-20T23:20:00Z
status: human_needed
score: 29/29 must-haves verified
covered_files: [".github/workflows/ci.yml", ".planning/phases/04-ci-gate-on-x86-64/04-01-PLAN.md", ".planning/phases/04-ci-gate-on-x86-64/04-01-SUMMARY.md", ".planning/phases/04-ci-gate-on-x86-64/04-02-PLAN.md", ".planning/phases/04-ci-gate-on-x86-64/04-02-SUMMARY.md", "Cargo.toml", "rust-toolchain.toml", "scripts/check-workflow.py", "scripts/ci-guards.sh", "scripts/ci-smoke.sh", "scripts/test-ci-guards.sh"]
covered_digest: "v3:sha256:00032c569837e2b6d66c5748be412d33e76b784896acd2a4da31da197bf3f264"
behavior_unverified: 0
overrides_applied: 0
human_verification:
  - test: "Push the current HEAD of modernize-toolchain (or let the next push happen) and confirm the CI run is green on ubuntu-26.04 x86_64"
    expected: "Single job 'fmt, clippy, test, release smoke (x86_64)' green: Repo guards print ok [a]..[h] (the [g] line should read 'lines match the canonical list, structure ok', since the observed run shows PyYAML is present on the image), fmt, clippy, test (171 passed), release build, Release smoke with five ok lines and 'ci-smoke: OK'"
    why_human: "The only remote run observed (run 39507141932, success) is for fd8f43f. The code-review fixes after it (fcbd7f3..5f0f10e) changed the scripts CI runs by +682/-76 lines (ml_grep scanner, canonical-lines --lines check, smoke crash reporting). They are green locally on aarch64 but have not run on the runner. Pushing is forbidden to this run."
  - test: "Persist the D-09 item from 04-02-SUMMARY.md into 04-UAT.md, with the runner corrected to ubuntu-26.04 and the observed run recorded"
    expected: "The item names ubuntu-26.04 (D-01 amended by fd8f43f), not ubuntu-24.04, and records run https://github.com/jmfrank63/rs3/actions/runs/39507141932 (fd8f43f, success, 1m58s, protected V8 platform without a crash in Release smoke). REQUIREMENTS.md's CI-03 suffix 'the remote x86_64 run is not yet observed' is updated to match"
    why_human: "Record-keeping decision. The SUMMARY text was correct when it was written (21:56) but went stale when the operator pushed at 22:18"
  - test: "Confirm that the push of fd8f43f to origin/modernize-toolchain was made by the operator"
    expected: "refs/remotes/origin/modernize-toolchain = fd8f43f (reflog: 'fetch -q origin: storing head' at 2026-10-20 22:18:33 +0000). It points at the operator commit 'ci: run on ubuntu-26.04 runner' (22:17:49), and no later agent commit is on the remote"
    why_human: "Both plans assert that no push happens and the operator forbade git push for this run, yet the remote ref exists. The timeline points to an operator action after both plans had finished, but the verifier cannot tell who ran the push"
  - test: "Flagged prohibition, judgment tier (04-02, CI-03 transparency): no claim of a remote green result while it was unobserved. Review the non-authoritative verdict"
    expected: "Verifier verdict (non-authoritative): satisfied. 04-02-SUMMARY says plainly that the run was not observed and keeps result: [pending]. That was true at 21:56. The green run that exists now (22:18) was observed by this verifier through gh and was not claimed by the executor"
    why_human: "unverified-prohibition — human review recommended (judgment-tier prohibitions are never auto-passed in autonomous mode)"
  - test: "Flagged prohibition, test tier, with no wired enforcement (04-02, CI-03): the smoke must not be weakened. Review scripts/ci-smoke.sh"
    expected: "Every assertion compares the exact status code and the exact body bytes with [[ == ]], with no status class, substring or retry. The verifier read the code and found this true at HEAD. No repo-resident check would catch a future weakening of ci-smoke.sh: test-ci-guards.sh and the CANONICAL pin cover ci.yml and the guards only"
    why_human: "Fail-closed: a test-tier prohibition with no enforcement evidence is flagged unverified, never green"
---

# Phase 4: CI Gate on x86_64 Verification Report

**Phase goal:** As a maintainer of rs3, I want to gate every push with a GitHub Actions workflow on an x86_64 runner that fails on any fmt, clippy or test failure, so that regressions and x86-only V8 crashes are caught before they land.
**Verified:** 2026-10-20T23:20:00Z
**Status:** human_needed
**Re-verification:** No. This is the initial verification.

## Key finding: the remote x86_64 run was observed (at fd8f43f)

The orchestrator expected success criterion 1 to be unobservable. That turned out to be wrong. `refs/remotes/origin/modernize-toolchain` exists at `fd8f43f`, the operator's runner-amendment commit. A read-only `gh run view 39507141932 --repo jmfrank63/rs3` shows the following:

- Run `CI`, push event, headSha `fd8f43f1ef4c0f8bb7e9183eae2b5ab92ab592b3`, **conclusion: success**, 22:18:33Z to 22:20:31Z (1m58s, cold).
- Runner image `ubuntu-26.04` (version 20261018.168.1, Ubuntu 26.04.1 LTS, Azure). The toolchain target is `x86_64-unknown-linux-gnu`.
- Every step succeeded: Checkout, Install Rust 1.99.1, Cache, Repo guards (ok [a]..[h], `[g] ... structure ok`, so PyYAML is present and no skip happened), fmt, clippy, test, release build, Release smoke.
- cargo test: `running 171 tests ... 171 passed; 0 failed`, at the default thread count (there is no thread flag in ci.yml).
- Release smoke on x86_64, using the protected V8 platform: `GET / -> "Welcome to rs3"`, `POST /insert -> 200`, `PATCH /key/m -> 200 "120"`, `PATCH /key/boom -> 422 js_error`, `GET / after JS error -> 200, server alive`, `ci-smoke: OK`. **No PKU crash, so D-07 is not needed.**

Caveat: that run covers `fd8f43f`, not HEAD (`357e57d`). Since then ci.yml has changed only in comments: it gained a header and a reworded D-07 comment, and its functional lines are byte-identical. The scripts CI runs, however, changed substantially in the review-fix loop. HEAD is therefore locally proven but not yet remote-proven. That is human item 1.

## Goal Achievement

### Observable Truths

Runner note: operator commit `fd8f43f` amended D-01 from `ubuntu-24.04` to `ubuntu-26.04` (recorded in 04-CONTEXT.md). Every plan must-have that names `ubuntu-24.04` was verified against `ubuntu-26.04`, which is still x86_64. The planted `-arm` case is now `ubuntu-26.04-arm`.

**ROADMAP success criteria (the contract)**

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| SC1 | Pushing modernize-toolchain runs a workflow on an x86_64 Linux runner with checkout v4+, dtolnay/rust-toolchain and rust-cache, and it finishes green with the V8 JS tests at the default thread count | ✓ VERIFIED (at fd8f43f) | Run 39507141932: success on ubuntu-26.04 / x86_64-unknown-linux-gnu; checkout@v7, rust-toolchain@1.99.1, rust-cache@v2; 171 tests passed with no thread flag. A re-run at HEAD is human item 1 |
| SC2 | The workflow runs `cargo fmt --check`, `cargo clippy --all-targets --locked -- -D warnings` and `cargo test --locked`, and any failure fails the build (no continue-on-error, no SERVER.* env) | ✓ VERIFIED | ci.yml lines 36-43 hold exactly these run strings as separate steps. No `continue-on-error`, `if`, `env` (except CARGO_TERM_COLOR) or `shell` appears anywhere. `check-workflow.py --lines` pins every functional line, and `--structure` forbids the bypass keys. Self-test cases catch `continue-on-error`, `if: false`, `|| true`, dropped `-D warnings`, `SERVER.HOST`/`server.host` |
| SC3 | The archived actions-rs workflow is deleted, and ci.yml is the only workflow | ✓ VERIFIED | `ls .github/workflows` shows only `ci.yml`. Commit 4d00cd8 has `D .github/workflows/actions-rs.yml`. Guard (g) checks for a single file, and the planted `other.yaml` and zero-workflow cases are caught |

**Plan 04-01 truths**

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | ci.yml is the only workflow, and actions-rs.yml was removed with git rm | ✓ VERIFIED | As SC3 |
| 2 | One job `ci`, an x86_64 runner (now ubuntu-26.04, superseded from 24.04), timeout-minutes 45 | ✓ VERIFIED | ci.yml lines 22-25; CANONICAL; structure check `runs-on must be ubuntu-26.04` |
| 3 | uses: steps are exactly checkout@v7, rust-toolchain@1.99.1 (rustfmt, clippy), rust-cache@v2, in order, with no actions-rs | ✓ VERIFIED | ci.yml lines 27-35; EXPECTED_USES plus ALLOWED_WITH; planted checkout@v2, `ref:`, `repository:` and `toolchain: nightly` are all caught |
| 4 | Triggers push (unfiltered) / pull_request / workflow_dispatch; permissions contents: read; per-ref concurrency with cancel; only env CARGO_TERM_COLOR; no continue-on-error/env/if/shell/defaults | ✓ VERIFIED | ci.yml; structure check (TRIGGERS, permissions, `CONCURRENCY_GROUP`, FORBIDDEN_KEYS, defaults); planted cases caught |
| 5 | Six separately named run steps in the exact order guards, fmt, clippy, test (no thread flag), release build, smoke | ✓ VERIFIED | The YAML extraction yields exactly the 6 REQUIRED_RUNS in order |
| 6 | Replaying every run: line locally in order passes, and the last step prints ci-smoke: OK | ✓ VERIFIED | Verifier replay: all 6 steps ok; `ci-smoke: OK (target/release/rs3, port 44091)`; rs3 process count 0 before and after |
| 7 | check-workflow.py exits 0; bash -n passes on every shell script | ✓ VERIFIED | `python3 -I scripts/check-workflow.py` rc 0; `bash -n` ok on 3 scripts; ast parse ok; no `scripts/__pycache__` |
| 8 | ci-guards.sh prints ok [a]..[h] in order; on failure it prints every FAIL line before exiting 1 | ✓ VERIFIED | Verifier run: ok a b c d e f g h, `all guards passed`. `fail()` accumulates and the script exits at the end; the multi-tag check is in the self-test |
| 9 | test-ci-guards.sh: the control passes, then every planted violation (a)-(h) exits non-zero with FAIL [tag] | ✓ VERIFIED | Verifier run: `test-ci-guards: 99 cases, 0 missed`, rc 0, 96 caught, ok control, 2 ok pass, ok g-structure-skip, scratch removed |
| 10 | Guard (g): CI with no PyYAML skips only the structure check (still enforces the file and token checks); missing PyYAML locally fails; github.event interpolation fails | ✓ VERIFIED | `ok g-structure-skip`; 25+ `expect_fail_ci` cases caught via `--lines` alone; `PyYAML unavailable outside CI` caught; github.event / github.head_ref cases caught. Info: comments are now exempt from the line pin, and an expression in a YAML comment is inert |
| 11 | D-07 fallback documented in a ci.yml comment and at guard (d) ("D-07 fallback") | ✓ VERIFIED | ci.yml lines 52-57 name unsafe_use_unprotected_platform, [dependencies] and guard (d); ci-guards.sh has the `# D-07 fallback:` comment |
| 12 | Every scripts/ file is 100755 | ✓ VERIFIED | `git ls-files -s scripts/` shows four 100755 entries |
| 13 | Edge adjacency: a clippy warning fails the build; continue-on-error, if:, changed run strings and `|| true` are rejected | ✓ VERIFIED | `-D warnings`; planted cases caught in both the local and the CI (no-PyYAML) modes |
| 14 | Edge empty: zero workflows fail with `found: none`; bare `push:` accepted; push.branches rejected | ✓ VERIFIED | Self-test `ci.yml deleted (zero workflows)` (extra 'found: none'), `push.branches filter`, `push: { branches: [main] }` caught; the real tree passes with bare `push:` |
| 15 | Edge ordering: run and uses order enforced; guards report a to h in fixed order | ✓ VERIFIED | `run steps out of order` / `uses must be exactly ... in order`, plus CANONICAL order; guard output order observed |
| 16 | No crate/pip/npm/binary added; src, requests, docs, Cargo.toml, Cargo.lock, clippy.toml, rust-toolchain.toml unchanged; TOOL-03 passes | ✓ VERIFIED | `git diff --quiet 8924cac -- ...` rc 0; TOOL-03 rc 0 (171 passed, 0 failed) |

**Plan 04-02 truths**

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 17 | The smoke on the release binary (protected platform) prints five ok lines: GET / welcome, POST /insert 200, PATCH m 200 "120", PATCH boom 422 exact js_error body, GET / 200 with the server alive | ✓ VERIFIED | Verifier replay output (all five lines). Also observed on x86_64 in run 39507141932 |
| 18 | Server config only via inline `env -i ... SERVER.HOST/PORT` in the smoke; none in the workflow; guard (g) ok | ✓ VERIFIED | ci-smoke.sh line 55; ci.yml has no SERVER.; D05_BANNED plus CANONICAL |
| 19 | Dead or crashing binaries fail fast with their status: missing, /bin/false status 1, SIGSEGV status 139, busy port | ✓ VERIFIED | Verifier runs: all exit 1 with `missing`, `(status 1)`, `(status 139); SIGSEGV is the x86_64 V8 PKU symptom`, `port 35859 already in use`. Also a crash during a request (a Python shim that SIGSEGVs on PATCH): `PATCH /key/m: transport error; server died (status 139)` |
| 20 | Binds 127.0.0.1 on a free port; the EXIT trap kills and reaps the server and deletes the log | ✓ VERIFIED | `pgrep -cx rs3` 0 before and after the replay; cleanup does TERM, then KILL after 5 s, then wait, then `rm -f "$LOG"`; no 0.0.0.0 |
| 21 | Edge repeat: two back-to-back runs pass on fresh ports | ✓ VERIFIED | Verifier ran two back-to-back passes: `ci-smoke: OK` on ports 42779 and 33209; `pgrep -cx rs3` 0 before and after |
| 22 | The full local suite is green (replay, self-test, check-workflow, safe_load, bash -n, TOOL-03; actionlint only if present) | ✓ VERIFIED | All rerun by the verifier; actionlint, shellcheck and yamllint are not installed, and none was downloaded |
| 23 | 04-02-SUMMARY has a "Pending human UAT (for 04-UAT.md)" D-09 item with result: [pending] and the D-07 fallback, and states that the remote run was not observed | ✓ VERIFIED (wording stale) | Section present at SUMMARY lines 149-163. WARNING: its runner text still says `ubuntu-24.04` (superseded by the D-01 amendment), and its "not observed" statement was overtaken by run 39507141932. See human item 2 |
| 24 | The SUMMARY maps prior UAT items to guards (01-4 to a; 02-4 to a, b, c plus goldens; 03-1 to a, b, c, d, f, with the docs disclosure left as judgment) | ✓ VERIFIED | SUMMARY lines 165-171; wiring confirmed below |
| 25 | 04-VALIDATION per-task rows green; wave_0_complete true | ✓ VERIFIED | Four rows `✅ green`, `wave_0_complete: true`. status/nyquist have since been set to validated/true by validate-phase, which owns them (superseded, expected) |
| 26 | No git push by the executors, no git config identity, no production source or manifest change | ✓ VERIFIED (with note) | No `user.email`/`user.name` in git config; 8924cac diff empty. NOTE: `origin/modernize-toolchain` exists at the operator commit fd8f43f, created 22:18:33 after both plans finished (04-02 ended 21:56). Human item 3 |

**Score:** 29/29 truths verified (0 present-but-behavior-unverified)

### Prohibitions (ADR-550)

| Plan | Statement (short) | Tier | Disposition |
|------|-------------------|------|-------------|
| 04-01 / CI-02 | Do not green the gate by weakening it (workflow keys, `|| true`, thread flag, RUSTFLAGS, cap-lints, attributes, ignores, lint levels, repo cargo config) | test | VERIFIED: wired enforcement. Guards (a), (b), (c), (g), (h) run as CI step 1 (green on the runner); CANONICAL allow-list; 99-case self-test |
| 04-01 / CI-01 | No second or weaker workflow, no actions-rs | test | VERIFIED: guard (g) single-file check plus EXPECTED_USES; planted cases caught |
| 04-01 / CI-03 | Do not apply D-07 early; unprotected platform stays dev-only | test | VERIFIED: Cargo.toml line 13 is plain `deno_core = "0.412.0"`, the feature is only on line 28 `[dev-dependencies]`; guard (d) (cargo tree plus source scan) with 7 planted cases |
| 04-02 / CI-03 | Do not claim the remote run green while unobserved | judgment | FLAGGED: non-authoritative verdict "satisfied" (human item 4) |
| 04-02 / CI-03 | Do not weaken the smoke (exact codes and bytes) | test | FLAGGED unverified: true by code reading at HEAD, but no wired enforcement against future weakening (human item 5) |
| 04-02 / CI-02 | No push, no git config change, no server config in the workflow | test | Workflow part VERIFIED (guard (g)); git config VERIFIED; push: executors did not push, and the remote ref is from an operator action (human item 3) |

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `.github/workflows/ci.yml` | Single CI workflow, one x86_64 job | ✓ VERIFIED | 58 lines, contains `runs-on: ubuntu-26.04` (plan pattern `ubuntu-24.04` superseded) |
| `scripts/check-workflow.py` | D-08 structure check | ✓ VERIFIED | EXPECTED_USES, REQUIRED_RUNS, CANONICAL, `--lines`/`--structure`; called by guard (g) |
| `scripts/ci-guards.sh` | Guards (a)-(h) | ✓ VERIFIED | Contains `check-workflow.py`, tomllib, `--prefix none`, rs_roots; CI step 1 |
| `scripts/ci-smoke.sh` | Release smoke with JS assertions | ✓ VERIFIED | Contains `env -i PATH=` and `Uncaught Error: boom`; CI step 9 |
| `scripts/test-ci-guards.sh` | Planted-violation self-test | ✓ VERIFIED | Contains `FAIL [`; local only by design (not a CI step, per D-04) |
| `.planning/.../04-VALIDATION.md` | Map green, wave 0 complete | ✓ VERIFIED | `wave_0_complete: true` |
| `.github/workflows/actions-rs.yml` | Deleted | ✓ VERIFIED | Absent |

### Key Link Verification

| From | To | Via | Status |
|------|----|-----|--------|
| ci.yml | scripts/ci-guards.sh | `run: bash scripts/ci-guards.sh` (before fmt) | ✓ WIRED (ran remotely) |
| ci.yml | scripts/ci-smoke.sh | `run: bash scripts/ci-smoke.sh` after the release build | ✓ WIRED (ran remotely) |
| ci-guards.sh | check-workflow.py | `python3 scripts/check-workflow.py --lines` / `--structure` | ✓ WIRED (plan pattern `python3 scripts/check-workflow\.py` matches as a prefix) |
| ci.yml | rust-toolchain.toml | `dtolnay/rust-toolchain@1.99.1` = `channel = "1.99.1"` | ✓ WIRED |
| guard (d) | Cargo.toml dev-dep deno_core | `cargo tree --locked --target all -e features,normal -i deno_core` | ✓ WIRED |
| test-ci-guards.sh | ci-guards.sh | `bash scripts/ci-guards.sh` inside each scratch copy | ✓ WIRED |
| ci-smoke.sh | src/http/error.rs | exact `{"error":"js_error",...}` body | ✓ WIRED (asserted live) |
| ci-smoke.sh | src/http/mod.rs | `"Welcome to rs3"`, PATCH `"120"` | ✓ WIRED (asserted live) |
| 04-02-SUMMARY UAT section | 04-UAT.md | orchestrator persists the item | PENDING: 04-UAT.md does not exist yet (by design; human item 2) |

### Data-Flow Trace (Level 4)

Not applicable: no rendered dynamic data. The smoke asserts real server responses from the release binary, not fixtures.

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
|----------|---------|--------|--------|
| Structure check | `python3 -I scripts/check-workflow.py` | rc 0 | ✓ PASS |
| Guards on the real tree | `bash scripts/ci-guards.sh` | ok a-h, all guards passed | ✓ PASS |
| Guards have teeth | `bash scripts/test-ci-guards.sh` | 99 cases, 0 missed (57 s) | ✓ PASS |
| Workflow replay | YAML-extracted run lines, in order | 6/6 ok, smoke OK | ✓ PASS |
| TOOL-03 | build, test, clippy -D warnings, fmt --check | rc 0, 171 passed | ✓ PASS |
| Smoke failure modes | missing, /bin/false, SEGV shim, busy port, crash during a request | all rc 1 with the expected lines; status 139 reported in both the startup and the request path | ✓ PASS |
| Remote x86_64 run | `gh run view 39507141932` (read-only) | success, ubuntu-26.04, x86_64, smoke OK | ✓ PASS (at fd8f43f) |

### Probe Execution

No `scripts/*/tests/probe-*.sh` exist and none is declared. Step 7c is not applicable.

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|-------------|-------------|--------|----------|
| CI-01 | 04-01 | checkout v4+, dtolnay/rust-toolchain, rust-cache; actions-rs removed | ✓ SATISFIED | SC3, truths 1 and 3 |
| CI-02 | 04-01, 04-02 | fmt, clippy (`--all-targets -D warnings`), tests with `--locked`; any failure fails the build | ✓ SATISFIED | SC2; remote run steps |
| CI-03 | 04-01, 04-02 | Runs on x86_64 Linux and passes | ✓ SATISFIED (at fd8f43f) | Run 39507141932. REQUIREMENTS.md line 72 still says "remote x86_64 run is not yet observed", which is stale (human item 2) |

No orphaned requirements: REQUIREMENTS.md maps only CI-01 to CI-03 to Phase 4, and every plan claims them.

### Prior "unverified prohibition" UAT items: are they now wired?

| Prior item | Guard(s) | Wired? |
|------------|----------|--------|
| 01-UAT test 4 (operator asked for a CI grep against new allow/cfg_attr(allow)) | (a): multi-line, whitespace-split, symlink-following, fail-closed | YES. 11 planted (a) cases caught; runs as CI step 1 |
| 02-UAT test 4 (no suppression, unwrap/expect deny, ignores, goldens intact) | (a), (b), (c) plus `cargo test --locked` | YES for the code parts. Note: the item's "exactly two #[ignore]" expectation is obsolete, since Phase 3 restored PATCH and guard (b) now requires zero, which the tree meets |
| 03-UAT test 1 (suppression, dev-only platform, no op shim, limits disclosed) | (a), (b), (c), (d), (f) | YES for the code parts. The docs/js-ops.md "limits disclosed without softening" part stays a judgment item (not wired, as planned) |

These items can now be closed on wired evidence, except the docs judgment part of 03-UAT test 1.

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| (all phase files) | - | TBD/FIXME/XXX/TODO/placeholder | none found | - |
| 04-02-SUMMARY.md | 152 | Stale runner name `ubuntu-24.04` in the UAT handoff text | ⚠️ Warning | Would mislead the operator if persisted verbatim into 04-UAT.md |
| .planning/REQUIREMENTS.md | 72 | CI-03 suffix "not yet observed" | ℹ️ Info | Stale after run 39507141932 |
| 04-SECURITY.md | 14 | Verified against `dab4b58` with "32 planted cases" | ℹ️ Info | Predates the review hardening (now 99 cases); no threat reopened |
| scripts/ci-guards.sh | (d) | Deferred IN-03: `Platform::new_custom(..)` passed via `RuntimeOptions::v8_platform` is not matched | ℹ️ Info | Deferred to OPS-03 in 04-REVIEW-DISPOSITION; not a current bypass in the tree |

### Human Verification Required

1. **Confirm green CI at HEAD.** Push and check that the ubuntu-26.04 x86_64 run is green with the post-review scripts. The observed green run is for fd8f43f only.
2. **Persist the D-09 item with corrections.** Write it into 04-UAT.md with `ubuntu-26.04` and the observed run 39507141932 recorded, and update the CI-03 suffix in REQUIREMENTS.md.
3. **Confirm who pushed fd8f43f.** Check that the push to origin/modernize-toolchain was the operator's (the run forbids git push).
4. **Judgment-tier prohibition (flagged).** Review the non-authoritative "satisfied" verdict that no remote green result was fabricated.
5. **Test-tier prohibition with no wired enforcement (flagged).** The smoke uses exact comparisons today, but nothing in the repo prevents it being weakened later.

### Gaps Summary

There are no blocking gaps. The phase goal is achieved in the codebase:

- One x86_64 workflow sequences guards, fmt, clippy (`-D warnings`), test (`--locked`, default threads), the release build and a release smoke that drives V8 through the protected platform.
- Bypass keys are structurally impossible without failing guard (g), and 99 planted violations prove the guards have teeth.
- That workflow has already run green on GitHub's ubuntu-26.04 x86_64 runner at fd8f43f, including the PKU-sensitive PATCH path.

The status is `human_needed` rather than `passed` for two reasons. HEAD's hardened scripts have not run remotely yet. And two prohibitions (one judgment tier, one test tier without enforcement) need a human sign-off under the fail-closed rules. The orchestrator should also correct the stale `ubuntu-24.04` / "not observed" wording before it persists 04-UAT.md.

---

_Verified: 2026-10-20T23:20:00Z_
_Verifier: Claude (gsd-verifier)_
