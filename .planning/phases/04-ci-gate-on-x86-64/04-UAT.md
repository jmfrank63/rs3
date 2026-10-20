---
status: testing
phase: 04-ci-gate-on-x86-64
source: [04-VERIFICATION.md, 04-02-SUMMARY.md]
started: 2026-10-20T23:10:00Z
updated: 2026-10-20T23:10:00Z
---

## Current Test

number: 1
name: Remote CI run on the GitHub x86_64 runner for the hardened HEAD (CI-03, ROADMAP SC-1; D-09)
expected: |
  After pushing modernize-toolchain (HEAD includes the review-fix hardening of scripts/ci-guards.sh,
  scripts/check-workflow.py, scripts/ci-smoke.sh), the CI workflow's single job on ubuntu-26.04 is green:
  guards a–h ok, fmt, clippy, cargo test (default threads), release build, release smoke "ci-smoke: OK".
awaiting: user response

## Tests

### 1. Remote CI run on the GitHub x86_64 runner for the hardened HEAD (CI-03, ROADMAP SC-1; D-09)
expected: From the dev host, run `git push origin modernize-toolchain` (origin is github.com/jmfrank63/rs3). In the Actions tab, the `CI` workflow's single job `fmt, clippy, test, release smoke (x86_64)` on `ubuntu-26.04` should be green at every step:
- repo guards: ok lines a–h, and `[g]` should report structure ok;
- `cargo fmt --check`;
- `cargo clippy`;
- `cargo test`, which runs the V8-backed JS tests at the default thread count;
- `cargo build --release`;
- release smoke: five ok lines plus `ci-smoke: OK`.

If the release smoke reports `server died (status 139)` or `server exited during startup (status 139)`, apply the D-07 fallback. That means moving `features = ["unsafe_use_unprotected_platform"]` from the `[dev-dependencies]` deno_core entry to the `[dependencies]` entry, and relaxing guard (d)'s feature check (only the feature check, not the source scan) and its self-test case in the same commit.

Prior evidence: GitHub Actions run 39507141932 (push of operator commit `fd8f43f`, the 04-01/02 scripts before the review-fix hardening) was green on `ubuntu-26.04` / `x86_64-unknown-linux-gnu` in 1m58s cold. All guards were ok (PyYAML present), 171 tests passed, and the release smoke passed (`PATCH /key/m -> 200 "120"`, `PATCH /key/boom -> 422 js_error`), so there was no PKU crash on the protected V8 platform. The hardened scripts at HEAD (+682/-76 lines) have not run remotely yet.
result: [pending]

### 2. Confirm the push of fd8f43f was the operator's own
expected: The autonomous run never pushed. `refs/remotes/origin/modernize-toolchain` was updated by a `fetch -q origin` at 22:18:33, after the operator session's commit `fd8f43f` (ubuntu-26.04 runner). Confirm that the push came from the operator's session.
result: [pending]

### 3. Judgment-tier prohibition (04-02): no green remote run claimed before it was observed
expected: The phase records claimed no remote result until run 39507141932 was observed read-only by the verifier (`gh run view`). The verifier's non-authoritative verdict is that this holds.
result: [pending]

### 4. Test-tier prohibition (04-02): the release smoke must not be weakened
expected: `scripts/ci-smoke.sh` compares exact status codes and exact body bytes, including the 422 `js_error` body. No repo-resident check would catch a future weakening, so review any later change to the smoke script.
result: [pending]

## Summary

total: 4
passed: 0
issues: 0
pending: 4
skipped: 0
blocked: 0

## Gaps
