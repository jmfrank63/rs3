---
gsd_state_version: "1.0"
current_phase: 6
current_phase_name: Native Deployment & Documentation
status: verifying
stopped_at: Completed 06-02-PLAN.md
last_updated: "2026-10-21T11:05:15.009Z"
last_activity: 2026-10-21
last_activity_desc: Phase 6 execution started
state_head: a9e720b8673a669cb3376b6bccd04c03f39e9f8a
progress:
  total_phases: 6
  completed_phases: 2
  total_plans: 16
  completed_plans: 16
  percent: 33
---

# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-10-21)

**Core value:** rs3 builds, tests and lints cleanly on current stable Rust (edition 2024) with only released dependencies, while every existing HTTP route and the JS-executes-against-the-store behaviour keep working with the same semantics.
**Current focus:** Phase 6 — Native Deployment & Documentation

## Current Position

Phase: 6 (Native Deployment & Documentation) — EXECUTING
Plan: 2 of 2
Status: Phase complete — ready for verification
Last activity: 2026-10-21 — Phase 6 execution started

Progress: [███░░░░░░░] 33%

## Performance Metrics

**Velocity:**
- Total plans completed: 6
- Average duration: -
- Total execution time: 0.0 hours

**By Phase:**

| Phase | Plans | Total | Avg/Plan |
|-------|-------|-------|----------|
| 1 | 3 | - | - |
| 5 | 3 | - | - |

**Recent Trend:**
- Last 5 plans: -
- Trend: -

*Updated after each plan completion*
**Per-Plan Metrics:**

| Plan | Duration | Tasks | Files |
|------|----------|-------|-------|
| Phase 02 P01 | 5 min | 2 tasks | 6 files |
| Phase 02 P02 | 5 min | 2 tasks | 3 files |
| Phase 02 P03 | 5 min | 3 tasks | 6 files |
| Phase 03 P01 | 9 min | 3 tasks | 10 files |
| Phase 03 P02 | 3 min | 3 tasks | 12 files |
| Phase 03 P03 | 6 min | 3 tasks | 8 files |
| Phase 04 P01 | 5 min | 2 tasks | 6 files |
| Phase 04 P02 | 2 min | 2 tasks | 2 files |
| Phase 05 P01 | 8 min | 3 tasks | 6 files |
| Phase 05 P02 | 11 min | 3 tasks | 13 files |
| Phase 05 P03 | 8 min | 3 tasks | 9 files |
| Phase 06 P01 | 11 min | 3 tasks | 20 files |
| Phase 06 P02 | 11 min | 3 tasks | 4 files |

## Accumulated Context

### Decisions

Decisions are logged in the PROJECT.md Key Decisions table.
Recent decisions affecting current work:

- [Roadmap]: 6 phases (standard granularity). Research phases 1 (bootstrap) and 2 (store & config) are merged, because the tree can't compile on the new deps until flurry, dotenv and config are replaced anyway.
- [Roadmap]: CI is Phase 4, right after the JS port, to surface x86-only V8/PKU crashes early. The JS limits (Phase 5) are verified under that gate.
- [Roadmap]: Every phase is in MVP mode, with user-story goals.
- [Research]: Stack is Rust 1.99.1, deno_core 0.412 (no direct `v8`), actix-web 4.17, papaya 0.2.5, config 0.15 + dotenvy, thiserror 2.
- [Research]: Status codes are 404 missing key, 400 malformed JSON, 422 bad shape / JS error / depth limit, 504 timeout. Ops throw on a missing key.
- [Phase 1]: papaya `Store` facade (owned `String`s), lib + thin main, sorted `/list` already shipped, PATCH answers 501 until Phase 3 (two parked PATCH tests).
- [Phase 1 UAT]: dotenv path verified by operator; rustup auto-install concurrency accepted as risk (override); v8x lock-entry disclosure accepted.
- [Phase 02]: [02-01] AppState { store, Arc<Settings> } lives at crate root (src/state.rs), built once in main; http::configure(state) creates the single web::Data and returns a Clone closure cloned per worker
- [Phase 02]: [02-01] ApiError (pub, in pub mod http::error) wraps StoreError via #[from]; exhaustive no-wildcard status/code match; error body {error, message} application/json
- [Phase 02]: [02-02] Settings::load() reads dotenv via dotenvy::dotenv_iter() and env via std::env::vars_os(), never writes the environment; env wins, first duplicate wins in the file
- [Phase 02]: [02-02] Missing dotenv ignored; any other dotenv error is 'error: invalid configuration: .env: <detail>' exit 1, never half-loaded; raw line kept (T-02-10)
- [Phase 02]: [02-02] Only keys whose lowercased name starts with 'server.' enter the config map; BOM-prefixed .env is now a startup error (Phase 6 README must document)
- [Phase 02]: [02-03] POST /insert reads web::Bytes; serde_json classify() splits 400 malformed_json (Syntax/Eof/Io, invalid UTF-8) from 422 invalid_input (shape checks); nothing written on either
- [Phase 02]: [02-03] Store::insert_all (infallible, one guard, returns count) replaces insert_many; partial-batch visibility documented; Phase 3 op_rs3_insert reuses parse-then-insert_all
- [Phase 02]: [02-03] /list escapes keys and values via serde_json Value Display in the legacy envelope; ,] quirk dropped; codec shared with op_rs3_list
- [Phase 02]: [02-03] [lints.clippy] unwrap_used/expect_used = deny with clippy.toml test allowances; no suppression attributes
- [Phase 05]: depth limit (max_patch_depth 4, clamp 1..=64) = hard stop → 422 js_depth_limit with key chain; timeout (timeout_ms 5000) via per-worker watchdog thread + per-job termination group + JobState CAS, ops silent after timeout (closes termination-cancellation escape) → 504 js_timeout; backstop at +2 s; precedence host fault > timeout > depth > result; nested boot stack exhaustion → 422
- [Phase 04]: single `ci` job on ubuntu-26.04 (operator amendment of D-01) — guards (a)–(h) via scripts/ci-guards.sh (+ canonical-lines allowlist of ci.yml, PyYAML structure check), fmt, clippy --locked -D warnings, test --locked, release build, release smoke (protected V8 platform); actions-rs workflow deleted; self-test with 99 planted cases
- [Phase 03]: JS on a bounded `rs3-js-N` pool (16 MiB stacks, tokio current_thread, fresh JsRuntime per job via try_new, catch_unwind); five #[op2] ops in rs3_ext with op_panic disabled; missing key → generic Error, bad input → TypeError; PATCH 200 String(result) / 422 js_error / 500 internal; nested host fault → 500 and store ops refuse afterwards; dev-only unprotected V8 platform
- [Phase 02 review-fix]: config keys lowercased before precedence (env always wins over .env); config map restricted to exactly server.host/server.port; /insert: syntax/EOF/invalid-UTF-8/unpaired-surrogate-escape → 400, out-of-range number / valid over-deep nesting → 422 naming the key
- [Phase 03]: [03-01] D-04 holds on deno_core 0.412: nested JsRuntime inside #[op2(reentrant)] op_rs3_patch works on the same thread (debug profile); op_panic disabled via rs3_ext middleware
- [Phase 03]: [03-01] String op args use FromV8 newtype StrArg: strings unchanged, primitives via String(..) (no arg -> undefined, 5 -> 5), objects/symbols throw TypeError; deno_core #[string] maps non-strings to empty string
- [Phase 03]: [03-01] JsExecutor: bounded rs3-js-* pool (available_parallelism workers, x16 queue, 16 MiB), tokio current_thread per worker, catch_unwind per job, Unavailable when closed; shared cfg(test) LazyLock executor
- [Phase 03]: [03-02] docs/js-ops.md documents the as-built StrArg rule: primitives coerced like String(..), objects/symbols throw TypeError without running toString; insert objects via JSON.stringify
- [Phase 03]: [03-02] All JS artefacts migrated by deleting the bootstrap call and renaming the op call only; JSRT-05 stays open until 03-03 (shared-ID gate), JSRT-06 marked complete
- [Phase 03]: [03-03] PATCH /key/{key}: 404 before queueing, then await state.js.execute; 200 JSON string of String(completion value); actix workers never run V8
- [Phase 03]: [03-03] ApiError::JsError -> js_error/422 with only the JS message; ExecError Unavailable/Internal -> fixed 500 internal body, detail to stderr; NotImplemented removed
- [Phase 03]: [03-03] main: init_platform -> JsExecutor::start(&JsSettings::default()) -> AppState::new(settings, js) -> HttpServer::new; pool start failure exits 1
- [Phase 04]: [04-01] Single ci.yml on ubuntu-24.04 sequences scripts that also run locally (guards, fmt, clippy, test, release build, release smoke); check-workflow.py requires the exact ordered uses/run lists and forbids continue-on-error/env/if/shell/defaults
- [Phase 04]: [04-01] ci-guards.sh enforces D-05 (a)-(h); grep/cargo-tree errors fail the guard; PyYAML skipped only in CI (structure check mandatory locally, never pip-installed); event-context interpolation rejected; test-ci-guards.sh proves 31 planted cases + CI skip
- [Phase 04]: [04-02] Release smoke drives JS through the protected V8 platform on the release binary: PATCH 6*20 -> "120", throwing script -> exact 422 js_error body, server alive; startup crash reported as exit status (139 = x86_64 PKU symptom, D-07 fallback)
- [Phase 04]: [04-02] Remote x86_64 CI run NOT observed (no push); handed off as pending human UAT in 04-02-SUMMARY.md for 04-UAT.md, with the D-07 fallback; CI-03 remote confirmation open
- [Phase 05]: [05-01] Depth limit counts the PATCHed key as level 1: op_rs3_patch refuses when chain.len() >= max_patch_depth, before the store read and before any runtime; the 422 chain ends with the refused key
- [Phase 05]: [05-01] Per-job Arc<JobState> (src/js/mod.rs) replaces ExecCtx's host_fault Arc: host fault, first-wins depth stop, stopped(), and the job's one error log sink
- [Phase 05]: [05-01] JsExecutor::execute classifies once via classify(): Err(Internal) passes, host fault -> Internal, depth stop -> DepthLimit (422 js_depth_limit), else the result; nested run_script never classifies
- [Phase 05]: [05-01] max_patch_depth in JsSettings (default 4) clamped to 1..=64 in Limits::from_settings; after a stop every op throws the depth error and writes nothing; one stop logs one line
- [Phase 05]: 05-02: outcome decided by the JobState phase CAS (finish() true = Timeout even with a computed value); never by the 'execution terminated' text
- [Phase 05]: 05-02: one rs3-js-N-wd watchdog per worker, spawned inside the worker closure; Arm at pickup, Disarm before the next job, 50 ms re-fire
- [Phase 05]: 05-02: rs3 ops return an empty string and never throw once the job timed out (an op Err cancels V8 termination, research S4)
- [Phase 05]: 05-02: D-07 backstop in JsExecutor::run_job: timeout_ms + 2 s counted from a started signal at pickup; queue wait untimed
- [Phase 05]: 05-03: The release smoke asserts 422 js_depth_limit and 504 js_timeout (at least 4900 ms after sending) with GET / and a second PATCH answered during the loop; it also requires the loop's output file to be empty next to kill -0, and its EXIT trap reaps the background curl
- [Phase 05]: 05-03: op_print writes and flushes through write_all_retrying / flush_retrying (Interrupted retries, WouldBlock yields, Ok(0) is WriteZero, no unwrap); an undrained pipe stalls the worker behind the backstop 504 (documented residual)
- [Phase 05]: 05-03: Worker stacks are clamped to MIN_STACK_BYTES (2 MiB) via effective_stack_bytes, like workers, queue capacity, depth and timeout
- [Phase 05]: 05-03: Remote x86_64 CI confirmation of the watchdog on the protected PKU platform (A1) is a pending UAT item; nothing was pushed
- [Operator UAT 2026-10-21]: CI run 39527188207 at 40afeb7 (ubuntu-26.04 x86_64) green with 0 annotations and 216 tests, so the hardened guards, the watchdog and the release smoke hold on the protected V8 platform. 03/04/05 UAT complete (late-registration path an accepted-risk follow-up; docs/js-ops.md readability a deferred user review); 02 items 1 and 4 pass.
- [Phase 6 re-scope 2026-10-21]: Docker dropped. rs3 ships as a native binary on a single Linux server (release build plus an example hardened systemd unit); DOCK-01..03 replaced by DEPL-01..03, DOCS-04 adds the MIT license. Stale experiments/web_scrape_s3_api/Cargo.lock removed (5c28dde).
- [Phase 06]: 06-01: deploy/rs3.service ships the verified research unit unchanged (env(1) bridge RS3_* -> SERVER.*, DynamicUser, no capabilities, @system-service @pkey, start limit 5/60 s, SyslogIdentifier=rs3); verify clean, exposure 1.5
- [Phase 06]: 06-01: seccomp proven under the user manager (bridge + kill-action run with nested PATCH, 422, 504, no SIGSYS); root-only directives and x86_64 PKU are pending operator UAT in 06-01-SUMMARY.md
- [Phase 06]: 06-01: LICENSE not created on this branch (arrives from main 9d1596b with the merge); README reset-failed recovery is cited from systemd.unit 255, not live-run
- [Phase 06]: 06-01: Dockerfile and docker_rs3/ deleted (2021 setup at tag msc), brief item 9 re-scoped; Vagrantfile keeps its dangling references (ruling 5)
- [Phase 06]: 06-02: README complete in D-12 order; every body/status/startup line replayed byte for byte against the release binary (readme-replay, 130 checks); only 500 internal and the dotenv-file rules are code-derived and labelled
- [Phase 06]: 06-02: 413/262144 limit and any Content-Type documented as unchanged since 2021 in HTTP API, not under Behaviour changes; body must be UTF-8
- [Phase 06]: 06-02: docs/js-ops.md says stdout is raw, unescaped (IN-03) with the V8 RangeError exception (IN-07); IN-02 table/limits gaps closed; IN-05 executor comment fixed (comment only)

### Pending Todos

- [Phase 6]: docs wording — stdout prints are raw (03 review IN-03); README must document BOM/malformed .env startup failures, 400/422 inserts, escaped /list, 404s, op mapping (docs/js-ops.md).

### Blockers/Concerns

- [Phase 6]: systemd (255 on the dev host) ignores dotted names such as `SERVER.HOST` in `Environment=` / `EnvironmentFile=`, so the example unit must bridge them (see the ROADMAP Phase 6 notes).
- [Env]: `/tmp` is about 96% full. Keep `target/` under the project dir, since a debug build with V8 is about 2.6 GB.
- [Open]: The user should confirm the behaviour changes (`/list` escaping + sort order — legacy `" : "` whitespace is now preserved; 404 replacing `"[]"`; 400/422 inserts; BOM/malformed .env now fail startup; the op rename) and have them recorded in PROJECT.md Key Decisions.

### Roadmap Evolution

- Phase 6 edited: re-scoped from Docker Image & Documentation to Native Deployment & Documentation (user decision 2026-10-21); edited fields: title, goal, requirements (DOCK-01..03 dropped; DEPL-01..03 and DOCS-04 added), success_criteria, notes

## Deferred Verification

| Phase | State | Resume |
|-------|-------|--------|
| 02 | verification_deferred_human | /gsd-verify-work 02 |

Phase 01 (roadmap-complete): VERIFICATION.md reads `stale` because Phase 02 intentionally evolved files it covered (STORE bridge → injected AppState, `dotenv()` → `dotenv_iter()`, legacy codec). Re-verification of every stale earlier phase is scheduled once, against the final tree, immediately before the milestone audit (`/gsd-execute-phase N` resumes at the verification gates).

Phase 02: all automated must-haves verified (18/18, 13/13 requirements). Operator UAT 2026-10-21: item 1 (malformed dotenv stops startup) and item 4 (no suppression, now guard-wired) pass. Items 2 and 3, the live dotenv-file checks (stray keys; env overrides the file), are still pending with the operator, because agents never create or name the dotenv file. Report is also `stale`, so it is re-verified with the others.

Phase 03: UAT complete (operator 2026-10-21: items 1 and 2 pass, item 3 docs/js-ops.md readability deferred to a later user review). VERIFICATION.md reads `stale` (later phases evolved covered files); re-verified before the audit.

Phase 04: UAT complete (operator 2026-10-21: all 4 pass; CI runs 39507141932 and 39527188207 green on ubuntu-26.04 x86_64 with 0 annotations). VERIFICATION.md reads `stale` (Phase 5 evolved Cargo.toml and scripts/ci-smoke.sh); re-verified before the audit.

Phase 05: complete (2026-10-21). UAT complete (3 pass, late-registration path an accepted-risk follow-up) and VERIFICATION.md canonicalized to `passed`.

## Deferred Items

Items acknowledged and deferred at milestone close, most recent first:

| Category | Item | Status | Deferred At | Milestone |
|----------|------|--------|-------------|-----------|
| *(none)* | | | | |

## Session Continuity

Last session: 2026-10-21T11:05:14.796Z
Stopped at: Completed 06-02-PLAN.md
Resume file: None
