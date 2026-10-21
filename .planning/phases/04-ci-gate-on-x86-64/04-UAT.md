---
status: complete
phase: 04-ci-gate-on-x86-64
source: [04-VERIFICATION.md, 04-02-SUMMARY.md]
started: 2026-10-20T23:10:00Z
updated: 2026-10-21T09:29:31.002Z
---

## Current Test

[testing complete]

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
result: pass
source: operator
evidence: "Operator reported 2026-10-21: run 39507141932 at fd8f43f on ubuntu-26.04 x86_64 succeeded with 0 annotations, 171 tests, guards and release smoke passing. The hardened scripts ran remotely in run 39527188207 at 40afeb7 (reported under 05-UAT item 1): green, 0 annotations, 216 tests. 40afeb7 contains every Phase 4 review-fix commit to scripts/ and .github/, and those paths are unchanged since. The operator moved the runner from ubuntu-24.04 to ubuntu-26.04 and updated check-workflow.py and test-ci-guards.sh to match. No PKU crash, so the D-07 fallback is not needed."

### 2. Confirm the push of fd8f43f was the operator's own
expected: The autonomous run never pushed. `refs/remotes/origin/modernize-toolchain` was updated by a `fetch -q origin` at 22:18:33, after the operator session's commit `fd8f43f` (ubuntu-26.04 runner). Confirm that the push came from the operator's session.
result: pass
source: operator
evidence: "Operator confirmed 2026-10-21: the push of fd8f43f was made by the operator on the user's instruction."

### 3. Judgment-tier prohibition (04-02): no green remote run claimed before it was observed
expected: The phase records claimed no remote result until run 39507141932 was observed read-only by the verifier (`gh run view`). The verifier's non-authoritative verdict is that this holds.
result: pass
source: operator
evidence: "Operator review 2026-10-21: no green result was claimed before it was observed."

### 4. Test-tier prohibition (04-02): the release smoke must not be weakened
expected: `scripts/ci-smoke.sh` compares exact status codes and exact body bytes, including the 422 `js_error` body. No repo-resident check would catch a future weakening, so review any later change to the smoke script.
result: pass
source: operator
evidence: "Operator review 2026-10-21: the smoke was not weakened."

## Summary

total: 4
passed: 4
issues: 0
pending: 0
skipped: 0
blocked: 0

## Gaps
