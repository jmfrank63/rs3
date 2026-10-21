# Phase 5: JS Execution Limits - Context

**Gathered:** 2026-10-20
**Status:** Ready for planning
**Mode:** Smart discuss (autonomous, non-interactive — every grey area auto-accepted with the Recommended answer)

<domain>
## Phase Boundary

Stored scripts can no longer crash the server or starve it:
- A PATCH nesting-depth limit stops self-patching scripts, whether they patch themselves directly or through a cycle such as A → B → A. The request answers 422 with a JSON error naming the limit and the key chain.
- An execution timeout terminates non-terminating scripts such as `while(true){}`. The request answers 504, and every other request, including other PATCHes, keeps being answered.
- The timeout comes from settings built in code, so tests use a short value with no environment variables.
- Every error path has a passing test (TEST-04).

Requirements: ROBU-05, ROBU-06, TEST-04.

Not in this phase: V8 heap limit and the near-heap-limit callback, cancel on client disconnect, 503 backpressure and snapshots (v2 JSHD-01..04). An environment-configurable timeout is also out (OPS-02, v2).

</domain>

<decisions>
## Implementation Decisions

### Nesting depth limit (ROBU-05)
- **D-01:** `JsSettings` gains `max_patch_depth` (default **4**), built in code like the other pool settings (Phase 3 D-06). Phase 2 D-16 allowlists only `server.host`/`server.port` from the environment, so there is no env var. The 16 MiB worker stacks leave ample headroom: Phase 3 research showed hundreds of levels before V8's stack guard.
- **D-02:** `ExecCtx` carries the key chain (the keys from the outer PATCH down to the current level) alongside `depth`. `op_rs3_patch` checks the limit **before** creating the nested runtime. If the next level would exceed `max_patch_depth`, it sets a **shared per-job `depth_exceeded` flag** (the same shared-flag pattern as the Phase 3 `host_fault`), throws a JS error, and does not create a runtime.
- **D-03:** Exceeding the depth limit is a **hard stop**. Once the flag is set:
  - the whole request ends with **422** `{"error":"js_depth_limit","message":"PATCH nesting depth limit (4) exceeded: a -> b -> a -> b -> a"}`, even if the script caught the thrown error;
  - all store ops refuse after the flag is set, matching the Phase 3 post-fault refusal;
  - classification uses the flag, not the message string (research flag);
  - writes made before the limit was hit are kept, since there are no transactions, and this is documented.
- **D-04:** A host fault and a depth limit can both happen in one job. The precedence is host fault (500) over timeout (504) over depth limit (422) over script error (422 `js_error`). Classification happens once, in one place (`run_script`/executor → `ExecError` → `ApiError`).

### Execution timeout (ROBU-06)
- **D-05:** `JsSettings` gains `timeout_ms` (default **5000**), built in code. Tests build an executor with a short timeout, around 200–300 ms, with no env vars (ROADMAP SC-3).
- **D-06:** Each job has a **termination group**, a shared registry of the `v8::IsolateHandle`s of every runtime the job creates, nested ones included, registered on creation and removed on drop. A per-job watchdog fires after `timeout_ms`. It sets the job's `timed_out` flag **first** and then calls `terminate_execution()` on every registered isolate. The worker classifies the outcome by the flag as `ExecError::Timeout`, never by message. The runtimes are dropped afterwards (fresh runtime per job), so `cancel_terminate_execution` is not needed. The watchdog can be a tokio timer on the worker's current_thread runtime or a separate timer thread, whichever is simplest. It must be cancelled when the job finishes normally, and a finished job must never be terminated late.
- **D-07:** The HTTP handler also awaits the reply with `timeout_ms` plus a grace period (e.g. +2 s) as a **backstop**, answering 504 if the worker never replies. This covers stalls inside Rust op code, which `terminate_execution` cannot interrupt. The backstop must not leak the job: the worker still finishes or terminates and drops the reply.
- **D-08:** A timed-out request answers **504** `{"error":"js_timeout","message":"script exceeded the 5000 ms execution limit"}` (with the configured value). `ApiError` gets `JsTimeout` (504) and `JsDepthLimit` (422), keeping the exhaustive, no-wildcard mapping.
- **D-09:** Isolation from other requests:
  - Actix workers never run V8 (Phase 3), so non-PATCH routes are always answered.
  - A `while(true){}` holds one JS worker only until the timeout. Other PATCHes run on other workers, or wait in the bounded queue and run once a worker frees up.
  - Tests prove both: a pool with ≥2 workers runs an infinite loop alongside a normal PATCH that completes, plus a non-JS route answered during the loop.
  - The timed-out worker serves the next job normally.

### Tests, docs, carry-overs
- **D-10 (TEST-04):** Every error path has a passing test, and the SUMMARY lists them in one table with test names:
  - 404 missing key, for GET, DELETE and PATCH
  - 400 malformed insert
  - 422 bad insert shape
  - 422 JS exception
  - 422 depth limit: direct self-patch, A→B→A cycle, and caught-but-still-422
  - 504 timeout: `while(true){}`, a nested infinite loop inside a patched key, and the worker serving the next job afterwards

  They must pass locally and in CI (CI runs `cargo test --locked`). Timeout tests use short settings, and the whole suite must stay well under ~2 minutes and pass under `--test-threads=1`, `env -i` and a 5× repeat.
- **D-11:** Never run an **unbounded** self-patching script outside the new depth-limited code paths. Depth tests run only once the limit is in place: RED tests may fail on an assertion but must not allocate 800 MB, so add a test-only small-limit config, or write the RED test against a stub, to avoid the old unbounded path. Never smoke an infinite loop without the timeout.
- **D-12:** Rewrite the "Limits" section of `docs/js-ops.md`: no more "until Phase 5". Document the depth limit (default 4, chain in the message, a hard stop even when caught), the timeout (default 5000 ms, 504), the error codes, and that writes before the stop are kept.
- **D-13:** Fold in the Phase 3 review carry-overs from `03-REVIEW-DISPOSITION.md`:
  - IN-04: the `op_print` replacement retries on `WouldBlock`, like upstream.
  - IN-06: correct the `catch_unwind` comment (a panic inside an op still aborts).
  - IN-07: clamp `stack_bytes` like `workers`/`queue_capacity`.
  - IN-05: a fixture-parity test that the `requests/*.txt` payload scripts equal their `.js` siblings, or produce the same results.
- **D-14:** CI (Phase 4) must stay green. The new tests run inside `cargo test --locked`. If any new test needs a feature, it stays on the dev edge only (guard (d)). No `#[ignore]`, no `#[allow]`; the guards enforce both.

### Claude's Discretion
- Exact type and field names (`TerminationGroup`, `JobFlags`, `ExecError::{Timeout, DepthLimit}`), the watchdog mechanism (tokio timer or thread), and the grace value for the backstop.
- The message wording beyond the fixed error codes, and the separator in the key chain.
- How the small-limit test configuration is exposed (e.g. `JsSettings` builder or a `#[cfg(test)]` constructor).

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Project scope
- `.planning/REQUIREMENTS.md`: ROBU-05, ROBU-06, TEST-04 (and v2 JSHD-* out of scope)
- `.planning/ROADMAP.md` §Phase 5: success criteria; research flag (native stack per level, classify termination by reason)

### Prior phases
- `.planning/phases/03-javascript-runtime-port/03-CONTEXT.md`: D-04 (depth plumbed), D-05/D-06 (pool, JsSettings), D-10/D-11 (422/500 mapping)
- `.planning/phases/03-javascript-runtime-port/03-0{1,2,3}-SUMMARY.md` and `03-REVIEW-FIX.md`: as-built executor, `ExecCtx`, the shared `host_fault` flag pattern, post-fault op refusal, single-log rule
- `.planning/phases/03-javascript-runtime-port/03-RESEARCH.md`: spike results (self-recursion cost ~800 MB, depth 235/778), deno_core APIs
- `.planning/phases/03-javascript-runtime-port/03-REVIEW-DISPOSITION.md`: carry-over IN-04..IN-07 routed here
- `.planning/phases/04-ci-gate-on-x86-64/04-CONTEXT.md` and `scripts/ci-guards.sh`: CI gate and guards that must stay green

### Research
- `.planning/research/ARCHITECTURE.md`: Pattern 4 (nested patch, depth limit, stack sizing), Pattern 5 (timeout via `IsolateHandle::terminate_execution`, TerminationGroup), ExecError → status mapping
- `.planning/research/PITFALLS.md`: stack overflow on recursion, termination classification
- `.planning/research/SUMMARY.md`: resolved decision (e) (hardening scope) and (f) (status codes: 422 depth, 504 timeout)

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- `src/js/ops.rs`: `ExecCtx { store, depth, … }`, `op_rs3_patch` (reentrant, nested runtime), the shared `host_fault` flag with `live_ctx` refusal, and the single-log rule.
- `src/js/runtime.rs`: `run_script` (try_new, TryCatch stringification, error classification).
- `src/js/executor.rs`: `JsExecutor`, `JsSettings` (workers/queue/stack, clamping), worker loop with `catch_unwind`, the closed-executor path.
- `src/js/mod.rs`: `ExecError` (Script / Internal / Unavailable …).
- `src/http/error.rs`: exhaustive `ApiError` mapping. `src/http/mod.rs` `patch_key` awaits `JsExecutor::execute`.
- `docs/js-ops.md`: the Limits section to rewrite.

### Established Patterns
- Shared per-job flags, not message strings, decide classification. Post-fault op refusal. Fixed 500 body. 422 carries only the message.
- TDD RED/GREEN pairs with JUnit evidence. No `#[allow]` and no `#[ignore]` (CI guards). In-crate `#[cfg(test)]` modules with a shared test executor.

### Integration Points
- `JsSettings` construction in `main`/state and in test executors, so a short-timeout test executor needs its own constructor.
- `ApiError` gains two variants. `patch_key` gets the backstop timeout.

</code_context>

<specifics>
## Specific Ideas

- **Git identity (user instruction):** commit with env vars `GIT_AUTHOR_NAME="Johannes Maria Frank"`, `GIT_AUTHOR_EMAIL="jmfrank63@gmail.com"`, `GIT_COMMITTER_NAME="Johannes Maria Frank"`, `GIT_COMMITTER_EMAIL="jmfrank63@gmail.com"`. **Never modify git config.** Commit messages end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- **Never `git push`** (an operator session may push separately). Stay on `modernize-toolchain`. Another session may commit concurrently: rebase or fast-forward cleanly, and never rewrite others' commits.
- The TOOL-03 gate plus `bash scripts/ci-guards.sh` must pass at the end of every plan.
- Keep `target/` inside the project dir. A project hook blocks naming the dotenv file, so use env-var config for smokes.

</specifics>

<deferred>
## Deferred Ideas

- Heap limit / near-heap-limit callback (JSHD-01), cancel on disconnect (JSHD-02), 503 backpressure (JSHD-03), snapshots (JSHD-04): v2.
- Env-configurable timeout and depth (`SERVER.*` allowlist extension): OPS-02 (v2).

</deferred>

---

*Phase: 05-js-execution-limits*
*Context gathered: 2026-10-20 (smart discuss, auto-accepted recommendations)*
