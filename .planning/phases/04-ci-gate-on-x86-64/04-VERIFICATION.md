---
phase: 04-ci-gate-on-x86-64
verified: 2026-10-21T11:52:51Z
status: passed
score: 29/29 must-haves verified
covered_files: [".github/workflows/ci.yml", ".planning/phases/04-ci-gate-on-x86-64/04-01-PLAN.md", ".planning/phases/04-ci-gate-on-x86-64/04-01-SUMMARY.md", ".planning/phases/04-ci-gate-on-x86-64/04-02-PLAN.md", ".planning/phases/04-ci-gate-on-x86-64/04-02-SUMMARY.md", "Cargo.toml", "rust-toolchain.toml", "scripts/check-workflow.py", "scripts/ci-guards.sh", "scripts/ci-smoke.sh", "scripts/test-ci-guards.sh"]
covered_digest: "v3:sha256:c725c4d78cd0e40f6521ff9929902b869c090fe40fdd21c34354b20eb021fd09"
behavior_unverified: 0
overrides_applied: 3
overrides:
  - must_have: "ci.yml defines exactly one job, ci, with runs-on: ubuntu-24.04 (a pinned x86_64 image) and timeout-minutes: 45"
    reason: "D-01 amended by operator commit fd8f43f ('ci: run on ubuntu-26.04 runner'): the pinned x86_64 image is ubuntu-26.04. ci.yml, the CANONICAL line pin and the --structure check in check-workflow.py, and the planted -arm case in test-ci-guards.sh were all moved together. The intent (one job, a pinned x86_64 image, timeout 45) is met: ci.yml line 23 'runs-on: ubuntu-26.04', and remote runs 39507141932 and 39527188207 ran on Image ubuntu-26.04, host x86_64-unknown-linux-gnu. The same amendment covers the 04-01 artifact pattern 'contains: runs-on: ubuntu-24.04'."
    accepted_by: "operator (jmfrank63), commit fd8f43f; recorded in 04-CONTEXT.md D-01"
    accepted_at: "2026-10-20T22:17:49Z"
  - must_have: "04-02-SUMMARY.md Pending human UAT D-09 item: confirm that the CI workflow's single job is green on ubuntu-24.04 x86_64; the SUMMARY states plainly that the remote run was not observed"
    reason: "Superseded by the same D-01 amendment and by events after the SUMMARY was written (21:56). The orchestrator persisted the D-09 item into 04-UAT.md (test 1) with the runner corrected to ubuntu-26.04 and the observed runs recorded. The item was then resolved 'pass' by the operator. The SUMMARY's 'not observed' statement was true when it was written."
    accepted_by: "operator (jmfrank63), 04-UAT.md test 1"
    accepted_at: "2026-10-21T09:29:31Z"
  - must_have: "No git push happens and git config is untouched: no refs/remotes/origin/modernize-toolchain ref exists"
    reason: "The executors never pushed. The remote ref exists because the operator pushed fd8f43f after both plans had finished (22:18 vs 21:56), and the operator confirmed this in 04-UAT.md test 2. The executor-scoped intent holds: no executor push, no git identity in git config, and no Phase 4 commit touched src/, requests/, docs/, Cargo.toml, Cargo.lock, clippy.toml or rust-toolchain.toml."
    accepted_by: "operator (jmfrank63), 04-UAT.md test 2"
    accepted_at: "2026-10-21T09:29:31Z"
re_verification:
  previous_status: human_needed
  previous_score: 29/29
  gaps_closed: []
  gaps_remaining: []
  regressions: []
  human_items_resolved:
    - "Green CI on hardened scripts: run 39527188207 at 40afeb7 (scripts/ and .github/ byte-identical to HEAD) green on ubuntu-26.04 x86_64, 216 tests, 11 smoke ok lines (04-UAT test 1)"
    - "D-09 item persisted with the corrected runner, and REQUIREMENTS.md CI-03 now cites run 39507141932 (04-UAT test 1)"
    - "fd8f43f push confirmed as the operator's (04-UAT test 2)"
    - "Judgment-tier prohibition (no unobserved green claim): operator pass (04-UAT test 3)"
    - "Test-tier smoke-weakening prohibition: operator review pass (04-UAT test 4); verifier re-read ci-smoke.sh at HEAD and every assertion is still an exact [[ == ]] comparison"
---

# Phase 4: CI Gate on x86_64 Verification Report

**Phase goal:** As a maintainer of rs3, I want to gate every push with a GitHub Actions workflow on an x86_64 runner that fails on any fmt, clippy or test failure, so that regressions and x86-only V8 crashes are caught before they land.
**Verified:** 2026-10-21T11:52:51Z
**Status:** passed
**Re-verification:** Yes. The previous report (human_needed, 29/29) went stale because covered files moved later: Phase 5 extended `scripts/ci-smoke.sh` and Phase 6 added `license = "MIT"` to `Cargo.toml`. This run re-judges the report against the final milestone tree (HEAD `a4a8069`).

## Evidence base

- **Code read at HEAD:** `.github/workflows/ci.yml`, `scripts/{check-workflow.py,ci-guards.sh,ci-smoke.sh,test-ci-guards.sh}`, `Cargo.toml`, `rust-toolchain.toml`.
- **Read-only checks run by this verifier:**
  - `python3 -I scripts/check-workflow.py` returned rc 0.
  - `bash -n` passed on all three shell scripts.
  - `git diff --quiet 90d06ec -- .github scripts` and `git diff --quiet 40afeb7 -- .github scripts` both returned rc 0.
  - `git ls-files -s scripts/` shows four `100755` entries.
  - `find` lists exactly one file under any `.github/`: `.github/workflows/ci.yml`.
  - No phase-4 commit (`--grep='(04'`, 37 commits) touches src, requests, docs, Cargo.toml, Cargo.lock, clippy.toml or rust-toolchain.toml.
  - `pgrep -a rs3` shows no rs3 processes left behind.
- **Remote CI, checked read-only with `gh`:**
  - Run **39527188207** at `40afeb7` (an ancestor of HEAD) finished `success`, with `Image: ubuntu-26.04` and host `x86_64-unknown-linux-gnu`.
    - All 9 steps succeeded.
    - Guards printed `ok [a]..[h]`, and `[g]` reported "structure ok", so PyYAML was present.
    - `cargo test` passed 216 tests with no thread flag.
    - Release smoke printed 11 ok lines and `ci-smoke: OK`. That includes `PATCH /key/m -> 200 "120"` and `422 js_error` on the protected V8 platform, so there was no PKU crash.
    - `.github/` and `scripts/` at `40afeb7` are byte-identical to HEAD.
  - Run **39507141932** at `fd8f43f` finished `success` and is the first remote x86_64 run.
- **Shared gate evidence on HEAD `a4a8069`** (not re-run, per the orchestrator; log `target/tmp/d17-a4a8069.log`):
  - ci-guards: a to h ok.
  - check-workflow OK.
  - test-ci-guards: 99 cases, 0 missed.
  - fmt, clippy `-D warnings`, `cargo test --locked` (216 passed), release build: all OK.
  - ci-smoke: OK.

## Goal Achievement

### Observable Truths

**ROADMAP success criteria (the contract)**

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| SC1 | Pushing modernize-toolchain runs a workflow on an x86_64 Linux runner with checkout v4+, dtolnay/rust-toolchain and rust-cache, and it finishes green with the V8 JS tests at the default thread count | ✓ VERIFIED | ci.yml lines 27-35 use `actions/checkout@v7`, `dtolnay/rust-toolchain@1.99.1` and `Swatinem/rust-cache@v2`. Line 47 is `cargo test --locked` with no thread flag. Remote runs 39507141932 and 39527188207 were green on ubuntu-26.04 / x86_64-unknown-linux-gnu. 39527188207 used CI scripts identical to HEAD |
| SC2 | The workflow runs `cargo fmt --check`, `cargo clippy --all-targets --locked -- -D warnings` and `cargo test --locked`, and any failure fails the build (no continue-on-error, no SERVER.* env) | ✓ VERIFIED | ci.yml lines 41/44/47 hold exactly these run strings as separate steps. There is no `continue-on-error`, `if`, `shell` or job/step `env`; the only env is workflow-level `CARGO_TERM_COLOR`. `check-workflow.py` enforces this: `--lines` pins the CANONICAL lines and bans `continue-on-error\|server\.` case-insensitively, and `--structure` checks FORBIDDEN_KEYS. Guard (g) runs it as CI step 1 |
| SC3 | The archived actions-rs workflow is deleted, and ci.yml is the only workflow | ✓ VERIFIED | `find` shows only `.github/workflows/ci.yml`. `git ls-files` has no actions-rs file. Guard (g) checks that the workflow file is the only one. The planted `other.yaml` case is caught (test-ci-guards.sh line 268) |

**Plan 04-01 truths**

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | ci.yml is the only workflow; actions-rs.yml was removed with git rm | ✓ VERIFIED | As SC3. Commit 4d00cd8 deleted it |
| 2 | One job `ci` on `ubuntu-24.04`, timeout 45 | PASSED (override) | Override: D-01 was amended to `ubuntu-26.04` by operator commit fd8f43f. ci.yml lines 21-24 have one job `ci`, `runs-on: ubuntu-26.04` and `timeout-minutes: 45`. check-workflow.py line 184 asserts `ubuntu-26.04` |
| 3 | uses: are exactly checkout@v7, rust-toolchain@1.99.1 (rustfmt, clippy), rust-cache@v2 in order, with no actions-rs | ✓ VERIFIED | ci.yml lines 27-35. `EXPECTED_USES` with an order check (check-workflow.py line 214). The planted `checkout@v2` case is caught |
| 4 | Triggers push (unfiltered) / pull_request / workflow_dispatch; contents: read; per-ref concurrency with cancel; only CARGO_TERM_COLOR env; no continue-on-error/env/if/shell/defaults | ✓ VERIFIED | ci.yml lines 5-18. `TRIGGERS` and `FORBIDDEN_KEYS` in check-workflow.py. The planted `push.branches` case is caught |
| 5 | Six named run steps in the order guards, fmt, clippy, test (no thread flag), release build, smoke | ✓ VERIFIED | ci.yml lines 37-59. `REQUIRED_RUNS` with exact-equality checks (lines 223-230). The planted `--test-threads=1` case is caught (line 283) |
| 6 | Replaying every run: line locally in order passes; the smoke prints ci-smoke: OK | ✓ VERIFIED | The shared gate log on a4a8069 shows all six steps OK |
| 7 | check-workflow.py exits 0; bash -n passes on every shell script | ✓ VERIFIED | Run by this verifier: rc 0, and bash -n ok ×3 |
| 8 | ci-guards.sh prints ok [a]..[h] in order, and on failure prints every FAIL before exiting 1 | ✓ VERIFIED | The shared log and the remote run log both show a to h in order. The fail-accumulate design is exercised by the self-test |
| 9 | test-ci-guards.sh: the control passes, then every planted violation (a)-(h) fails with FAIL [tag] | ✓ VERIFIED | Shared log: `99 cases, 0 missed`. 92 `expect_fail` call sites |
| 10 | Guard (g): with CI set and no PyYAML, only the structure check is skipped; outside CI a missing PyYAML fails; github.event interpolation fails | ✓ VERIFIED | Self-test cases for the skip and for `github.event.head_commit.message` (line 292), counted in the 99/0 result |
| 11 | D-07 fallback documented in a ci.yml comment and at guard (d) | ✓ VERIFIED | ci.yml lines 52-57. ci-guards.sh has the `# D-07 fallback:` comment at guard (d) |
| 12 | Every scripts/ file is 100755 | ✓ VERIFIED | `git ls-files -s scripts/` |
| 13 | Edge adjacency: a clippy warning fails; continue-on-error, if:, changed run strings and `\|\| true` are rejected | ✓ VERIFIED | `-D warnings` on line 44. CANONICAL exact-line pin. Planted cases in the self-test |
| 14 | Edge empty: zero workflows fail with `found: none`; bare `push:` accepted; push.branches rejected | ✓ VERIFIED | The real tree passes with a bare `push:`. The planted cases are in the 99/0 self-test |
| 15 | Edge ordering: run and uses order enforced; guards report a to h | ✓ VERIFIED | check-workflow.py lines 214-230. The guard output order is observed both locally and remotely |
| 16 | No crate/pip/npm/binary added; src, requests, docs, Cargo.toml, Cargo.lock, clippy.toml, rust-toolchain.toml unchanged in Phase 4; TOOL-03 passes | ✓ VERIFIED | No phase-4 commit touches these paths. The later Cargo.toml change (`license = "MIT"`) is Phase 6 (9b9e0ea). TOOL-03 is green in the shared log |

**Plan 04-02 truths**

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 17 | The release smoke (protected platform) asserts GET / welcome, POST /insert 200, PATCH m 200 `"120"`, PATCH boom 422 exact js_error body, GET / 200 with the server alive, then ci-smoke: OK | ✓ VERIFIED | ci-smoke.sh lines 122-152 make these five assertions with exact `[[ == ]]` comparisons. Phase 5 (01c8c3a) appended six more checks: depth limit 422, timeout 504, and requests answered during a loop. That extends the smoke without weakening it. Remote run 39527188207 printed all of them |
| 18 | Server config only via inline `env -i ... SERVER.HOST/PORT`; none in the workflow; guard (g) ok | ✓ VERIFIED | ci-smoke.sh line 103. ci.yml has no `SERVER.` (banned by `D05_BANNED`) |
| 19 | Dead or crashing binaries fail fast with their status (missing, status 1, 139, busy port) | ✓ VERIFIED | ci-smoke.sh lines 98-99 and 113-116, plus `server_exit_note`/`status_msg` (the 139 hint). The behaviour was exercised by the previous verifier with /bin/false, a SEGV shim and a busy port. Those code paths are unchanged since (the Phase 5 edits added the loop logic only) |
| 20 | Binds 127.0.0.1 on a free port; the EXIT trap kills and reaps the server and deletes the log | ✓ VERIFIED | Lines 43, 52-66 (TERM, then KILL after 5 s, wait, rm; Phase 5 also reaps `LOOP_PID`). `pgrep -a rs3` shows no processes left after the shared gate run |
| 21 | Edge repeat: two back-to-back runs pass on fresh ports | ✓ VERIFIED | The port is picked fresh for each run (line 43). Two back-to-back passes were observed in the previous verification. Remote and shared-log runs used ports 60003 and 39599 |
| 22 | The full local suite is green | ✓ VERIFIED | Shared gate log on a4a8069 |
| 23 | 04-02-SUMMARY D-09 item (ubuntu-24.04, result: [pending], "remote run not observed") | PASSED (override) | Override: superseded by the D-01 amendment and by the observed runs. The item was persisted with corrections as 04-UAT test 1 and resolved `pass` |
| 24 | The SUMMARY maps prior UAT items to guards (01-4 to a; 02-4 to a, b, c; 03-1 to a, b, c, d, f) | ✓ VERIFIED | 04-02-SUMMARY section present. The guards exist and pass |
| 25 | 04-VALIDATION per-task rows green; wave_0_complete true | ✓ VERIFIED | Unchanged since the previous verification |
| 26 | No push, no git config, no production change in Phase 4 | PASSED (override) | Override: the executors did not push. The remote ref comes from the operator's push of fd8f43f (04-UAT test 2). No git identity is configured, and no phase-4 commit touched the protected paths |

**Score:** 29/29 (26 VERIFIED + 3 PASSED (override)), 0 present-but-behavior-unverified.

### Prohibitions (ADR-550)

| Plan | Statement (short) | Tier | Disposition |
|------|-------------------|------|-------------|
| 04-01 / CI-02 | Do not green the gate by weakening it | test | VERIFIED. Wired enforcement: guards (a), (b), (c), (g) and (h) run as CI step 1 and are green remotely. The 99-case self-test proves they catch violations |
| 04-01 / CI-01 | No second or weaker workflow, no actions-rs | test | VERIFIED. Guard (g) single-file check plus EXPECTED_USES |
| 04-01 / CI-03 | Do not apply D-07 early | test | VERIFIED. In Cargo.toml, `[dependencies]` has a plain `deno_core = "0.412.0"`, and the feature appears only under `[dev-dependencies]`. Guard (d) checks both cargo tree and the source, with planted cases (test-ci-guards.sh lines 222-241) |
| 04-02 / CI-03 | Do not claim a remote green result while it was unobserved | judgment | RESOLVED by human: operator pass (04-UAT test 3) |
| 04-02 / CI-03 | Do not weaken the smoke | test | RESOLVED by human: operator pass (04-UAT test 4). The verifier re-read ci-smoke.sh at HEAD. Every check compares the exact code and the exact body, and none uses a substring, a status class or a retry. There is still no repo-resident enforcement, which is noted below |
| 04-02 / CI-02 | No push, no git config, no server config in the workflow | test | Workflow part VERIFIED (guard (g)). git config is clean. The push was the operator's (override 3) |

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `.github/workflows/ci.yml` | Single x86_64 CI workflow | ✓ VERIFIED (pattern via override) | 59 lines. Contains `runs-on: ubuntu-26.04`; the plan pattern `ubuntu-24.04` is superseded by D-01 |
| `scripts/check-workflow.py` | Structure check (`EXPECTED_USES`) | ✓ VERIFIED | 257 lines. Called by guard (g) |
| `scripts/ci-guards.sh` | Guards (a)-(h), contains `check-workflow.py` | ✓ VERIFIED | 255 lines. Runs as CI step "Repo guards" |
| `scripts/ci-smoke.sh` | Release smoke, contains `env -i PATH=` and `Uncaught Error: boom` | ✓ VERIFIED | 212 lines. Runs as CI step "Release smoke" |
| `scripts/test-ci-guards.sh` | Planted-violation self-test, contains `FAIL [` | ✓ VERIFIED | 472 lines. Local only by design (D-04) |
| `04-VALIDATION.md` | `wave_0_complete: true` | ✓ VERIFIED | |
| `.github/workflows/actions-rs.yml` | Deleted | ✓ VERIFIED | Absent |

### Key Link Verification

| From | To | Via | Status |
|------|----|-----|--------|
| ci.yml | scripts/ci-guards.sh | `run: bash scripts/ci-guards.sh` (line 38, before fmt) | ✓ WIRED (ran remotely) |
| ci.yml | scripts/ci-smoke.sh | `run: bash scripts/ci-smoke.sh` (line 59, after the release build) | ✓ WIRED (ran remotely) |
| ci-guards.sh | check-workflow.py | `python3 scripts/check-workflow.py --lines` / `--structure` | ✓ WIRED (remote `[g] ... structure ok`) |
| ci.yml | rust-toolchain.toml | `dtolnay/rust-toolchain@1.99.1` matches `channel = "1.99.1"` | ✓ WIRED (remote log: "overridden by rust-toolchain.toml", 1.99.1) |
| guard (d) | Cargo.toml dev-dep | `cargo tree --locked --target all -e features,normal -i deno_core` | ✓ WIRED |
| test-ci-guards.sh | ci-guards.sh | runs the copied guard script in each scratch copy | ✓ WIRED |
| ci-smoke.sh | src/http/error.rs | exact `{"error":"js_error",...}` body | ✓ WIRED (asserted live, remote) |
| ci-smoke.sh | src/http/mod.rs | `"Welcome to rs3"`, PATCH `"120"` | ✓ WIRED (asserted live, remote) |
| 04-02-SUMMARY UAT section | 04-UAT.md | persisted D-09 item | ✓ WIRED (04-UAT test 1, status complete, 4/4 pass) |

### Data-Flow Trace (Level 4)

Not applicable. Nothing renders dynamic data. The smoke asserts real responses from the release binary.

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
|----------|---------|--------|--------|
| Workflow structure | `python3 -I scripts/check-workflow.py` | rc 0 | ✓ PASS |
| Shell syntax | `bash -n scripts/*.sh` | ok ×3 | ✓ PASS |
| CI inputs unchanged since the last green remote run | `git diff --quiet 40afeb7 -- .github scripts` | rc 0 | ✓ PASS |
| Remote x86_64 gate | `gh run view 39527188207 --json/--log` (read-only) | success; ubuntu-26.04; x86_64; 216 tests; smoke OK | ✓ PASS |
| Remote x86_64 gate (first) | `gh run view 39507141932 --json` (read-only) | success at fd8f43f | ✓ PASS |
| Full local gate on HEAD | shared log `target/tmp/d17-a4a8069.log` (not re-run, per the orchestrator) | all GATE OK | ✓ PASS (shared evidence) |

### Probe Execution

No `scripts/*/tests/probe-*.sh` exist and none is declared. Not applicable.

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|-------------|-------------|--------|----------|
| CI-01 | 04-01 | checkout v4+, dtolnay/rust-toolchain, rust-cache; actions-rs removed | ✓ SATISFIED | SC1, SC3, truths 1 and 3 |
| CI-02 | 04-01, 04-02 | fmt, clippy `--all-targets -D warnings`, tests with `--locked`; any failure fails the build | ✓ SATISFIED | SC2; guard (g) pin; remote step list |
| CI-03 | 04-01, 04-02 | Runs on x86_64 Linux and passes | ✓ SATISFIED | Runs 39507141932 and 39527188207 green on ubuntu-26.04 x86_64; protected-platform smoke passed |

No orphaned requirements. REQUIREMENTS.md maps only CI-01 to CI-03 to Phase 4, and both plans claim them.

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| ci.yml, scripts/*, Cargo.toml, rust-toolchain.toml | - | TBD/FIXME/XXX/TODO/HACK/placeholder | none found | - |
| .planning/REQUIREMENTS.md | 72 | The CI-03 suffix still says "re-run on the hardened HEAD scripts pending as 04-UAT item 1" | ℹ️ Info | Stale. 04-UAT test 1 is resolved (run 39527188207). Record-keeping only |
| scripts/ci-smoke.sh | - | The smoke-weakening prohibition has no repo-resident enforcement | ℹ️ Info | Mitigated by operator review (04-UAT test 4). A future change to the smoke needs review |
| (cargo test output) | - | `1 ignored` doctest | ℹ️ Info | Generated by deno_core's `extension!` macro (documented at ci-guards.sh lines 84-85). Guard (b) scans the source, which has no ignore |

### Human Verification Required

None. All four 04-UAT items are complete (4/4 pass, status complete).

Non-blocking note: commit `9b9e0ea` (Phase 6) changed `src/js/executor.rs` (3 lines), docs and Cargo.toml metadata after the last remote run at `40afeb7`. The CI scripts and workflow are byte-identical, and the full gate is green locally on HEAD. The operator's next push will produce a fresh remote run of the final HEAD. That run confirms the result but is not an unmet must-have.

### Gaps Summary

There are no gaps.

- **The workflow:** one pinned x86_64 workflow (ubuntu-26.04, per the recorded D-01 amendment) gates every push, pull request and manual dispatch. It runs repo guards, `cargo fmt --check`, `cargo clippy --all-targets --locked -- -D warnings`, `cargo test --locked` at default threads, a release build, and a release smoke that drives V8 through the protected platform.
- **Bypasses are blocked:** bypass keys such as `continue-on-error`, `if:` and `|| true`, server config, and a second workflow cannot be added without failing guard (g). The 99-case planted-violation self-test proves that the guards fail loudly.
- **Proven on the real runner:** the workflow has run green twice on GitHub's x86_64 runner, the second time with CI scripts identical to the final tree. No PKU crash occurred, so D-07 was not applied (the prohibition holds).

There are three plan-level deviations. Each is a recorded, operator-accepted evolution and is carried as an override, not a gap: the runner image, the stale D-09 wording, and the operator's push.

---

_Verified: 2026-10-21T11:52:51Z_
_Verifier: Claude (gsd-verifier)_
