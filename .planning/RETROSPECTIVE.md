# Project Retrospective

*A living document updated after each milestone. Lessons feed forward into future planning.*

## Milestone: v0.2 — rs3 toolchain modernization

**Shipped:** 2026-10-21 (override closeout: phases 2 and 6 carry pending human UAT)
**Phases:** 6 | **Plans:** 16 | **Sessions:** 5

### What Was Built
- **Toolchain and dependencies:** Rust 1.99.1 / edition 2024 on released crates only (deno_core 0.412, actix-web 4.17, papaya 0.2.5, config 0.15 + dotenvy). The crate is a `lib.rs` plus a thin `main.rs`, with the store and config injected rather than global.
- **HTTP errors:** a JSON error contract replaces the panics: 404, 400 and 422 on inserts, 422 `js_error`, 500 `internal`. `/list` is escaped and sorted.
- **JS engine:** five `#[op2]` ops (`Deno.core.ops.op_rs3_*`) on a bounded JS worker pool, with reentrant nested PATCH. The four MSc samples run as fixtures.
- **JS limits:** the depth limit answers 422 `js_depth_limit` and the timeout 504 `js_timeout`. The server keeps answering other requests while a script loops.
- **CI:** one x86_64 `ubuntu-26.04` job that mirrors the local gate (guards, fmt, clippy, 216 tests, release build, release smoke). It is green remotely with 0 annotations.
- **Deployment and docs:**
  - native deployment: `cargo build --release --locked` plus the example hardened `deploy/rs3.service` (exposure 1.5);
  - `Readme.md` rewritten and replayed against the release binary (130 checks);
  - the MIT license.

### What Worked
- **Tracer-first plans.** Each plan led with an end-to-end slice, so every phase had something running early. The riskiest piece was spiked first in Phase 3: a nested `JsRuntime` inside the reentrant `op_rs3_patch`.
- **A local gate that mirrors CI.** The same scripts run locally and in CI (`ci-guards.sh`, `check-workflow.py`, `ci-smoke.sh`), and a self-test plants violations to prove every guard can fail. That made "green locally" a reliable predictor of the remote run.
- **Documentation proven by replay.** Every README body, status and startup line was diffed against the release binary. Code-derived statements were labelled instead of guessed.
- **Live proofs before handing off.** The systemd user manager runs proved the `env(1)` bridge, the seccomp filter (with a kill-action run) and the start limit. Only the genuinely root-only checks went to the operator.
- **Precise operator UAT feedback.** Run IDs, exact outputs and timings closed phases 3–5 without guesswork.

### What Was Inefficient
- **Mid-milestone re-scope.** Phase 6 changed from a Docker image to native deployment, so the first Docker-oriented plan and its artefacts were discarded.
- **Stale verification reports.** Fingerprinted reports went stale whenever a later phase touched their covered files, so phases 01–05 had to be re-verified against the final tree before the audit.
- **Three review rounds on the `MemoryMax=` guidance.** Round 2 added the swap cap and a sizing rule, and round 3 corrected the rule. The verifier still measured a small undercount (about 20× against 17×), which is left as a UAT decision.
- **Tooling friction:**
  - the commit helper skipped a deleted directory;
  - the UAT predicate needed a "Deferred follow-up:" prefix;
  - the decision-coverage gate could not parse bold labels ending in a period;
  - the isolation guard needed an explicit `dispatch-isolation`;
  - some generated commits lacked the attribution trailer;
  - a ledger title mismatch had to be reconciled by hand.
- **Subagent wrote to user-level config.** A research subagent wrote `auto_install` into `~/.rustup/settings.toml`, and it had to be reverted.
- **Checks the agent could not run itself.** Remote CI (no push), root-only systemd behaviour (no sudo, user namespaces restricted) and live dotenv-file checks (the secret-read hook) all waited on the operator.

### Patterns Established
- **Error contract:** one JSON shape, `{"error","message"}`, with stable codes: `not_found`, `malformed_json`, `invalid_input`, `js_error`, `js_depth_limit`, `js_timeout`, `internal`.
- **Classification:** outcomes are decided by reason (the job-phase CAS and typed `ExecError`), never by matching message text.
- **Limits:** set in code (`JsSettings`) and clamped. Tests use short limits without environment variables.
- **Workflow pinning:** an allow-list script (`check-workflow.py`) pins the CI workflow, and a planted-violation self-test proves each guard can fail.
- **Documentation claims:** checked by replaying them against the release binary.
- **Working rules:**
  - scratch goes under `target/tmp`;
  - commands never name the dotenv file;
  - commits use the env-var identity plus the Co-Authored-By trailer;
  - the operator pushes.

### Key Lessons
1. Budget a re-verification pass before the milestone audit whenever later phases edit files covered by earlier reports, or keep cross-phase edits to those files minimal.
2. Some checks can't be observed from the agent's environment: remote CI, root systemd, PKU hardware, secret files. Hand them off as UAT items with exact commands and expected output, and never claim them.
3. Measure resource-sizing guidance (memory caps, concurrency bounds) under realistic load before writing it into docs. A plausible rule can be off by a large factor.
4. Record the deliberate deviations (the `env(1)` bridge, no `MemoryDenyWriteExecute=`, `@pkey`) in CONTEXT and RESEARCH, so reviewers don't re-flag them in every iteration.
5. Keep subagents out of user- and global-level configuration. Review any write outside the repo.

### Cost Observations
- Model mix: subagents inherit the orchestrator's model (`model_profile: inherit`). This closing session ran on Opus 5.5, and no per-model split was recorded.
- Sessions: 5 Claude Code sessions over 2 days (2026-10-20 → 2026-10-21).
- Notable: the cold V8 build took about 1.5 min on aarch64 thanks to the prebuilt static library. Keeping `target/` (about 2.6 GB in debug) under the project directory avoided the nearly full `/tmp`.

---

## Cross-Milestone Trends

### Process Evolution

| Milestone | Sessions | Phases | Key Change |
|-----------|----------|--------|------------|
| v0.2 | 5 | 6 | First GSD milestone: tracer-first plans, a local gate that mirrors CI, operator UAT handoffs, override closeout for root-only and remote checks |

### Cumulative Quality

| Milestone | Tests | Coverage | Zero-Dep Additions |
|-----------|-------|----------|-------------------|
| v0.2 | 216 | not measured (coverage is v2 OPS-03) | — (the dependency set was replaced) |

### Top Lessons (Verified Across Milestones)

1. Not yet cross-validated: v0.2 is the first milestone. Re-check the v0.2 lessons at the next close.
