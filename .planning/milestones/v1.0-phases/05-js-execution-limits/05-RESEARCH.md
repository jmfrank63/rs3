# Phase 5: JS Execution Limits - Research

**Researched:** 2026-10-20
**Domain:** V8 isolate termination (deno_core 0.412.0 / v8 150.4.0), nested-runtime depth limiting, watchdog design on a dedicated JS thread pool, actix-web error mapping
**Confidence:** HIGH. Every load-bearing claim comes from a compiled and run spike against the project's exact lockfile, or from source in `~/.cargo/registry`.

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

#### Nesting depth limit (ROBU-05)
- **D-01:** `JsSettings` gains `max_patch_depth` (default **4**), built in code like the other pool settings (Phase 3 D-06). Phase 2 D-16 allowlists only `server.host`/`server.port` from the environment, so there is no env var. The 16 MiB worker stacks leave ample headroom: Phase 3 research showed hundreds of levels before V8's stack guard.
- **D-02:** `ExecCtx` carries the key chain (the keys from the outer PATCH down to the current level) alongside `depth`. `op_rs3_patch` checks the limit **before** creating the nested runtime. If the next level would exceed `max_patch_depth`, it sets a **shared per-job `depth_exceeded` flag** (the same shared-flag pattern as the Phase 3 `host_fault`), throws a JS error, and does not create a runtime.
- **D-03:** Exceeding the depth limit is a **hard stop**. Once the flag is set:
  - the whole request ends with **422** `{"error":"js_depth_limit","message":"PATCH nesting depth limit (4) exceeded: a -> b -> a -> b -> a"}`, even if the script caught the thrown error;
  - all store ops refuse after the flag is set, matching the Phase 3 post-fault refusal;
  - classification uses the flag, not the message string (research flag);
  - writes made before the limit was hit are kept, since there are no transactions, and this is documented.
- **D-04:** A host fault and a depth limit can both happen in one job. The precedence is host fault (500) over timeout (504) over depth limit (422) over script error (422 `js_error`). Classification happens once, in one place (`run_script`/executor → `ExecError` → `ApiError`).

#### Execution timeout (ROBU-06)
- **D-05:** `JsSettings` gains `timeout_ms` (default **5000**), built in code. Tests build an executor with a short timeout, around 200–300 ms, with no env vars (ROADMAP SC-3).
- **D-06:** Each job has a **termination group**, a shared registry of the `v8::IsolateHandle`s of every runtime the job creates, nested ones included, registered on creation and removed on drop. A per-job watchdog fires after `timeout_ms`. It sets the job's `timed_out` flag **first** and then calls `terminate_execution()` on every registered isolate. The worker classifies the outcome by the flag as `ExecError::Timeout`, never by message. The runtimes are dropped afterwards (fresh runtime per job), so `cancel_terminate_execution` is not needed. The watchdog can be a tokio timer on the worker's current_thread runtime or a separate timer thread, whichever is simplest. It must be cancelled when the job finishes normally, and a finished job must never be terminated late.
- **D-07:** The HTTP handler also awaits the reply with `timeout_ms` plus a grace period (e.g. +2 s) as a **backstop**, answering 504 if the worker never replies. This covers stalls inside Rust op code, which `terminate_execution` cannot interrupt. The backstop must not leak the job: the worker still finishes or terminates and drops the reply.
- **D-08:** A timed-out request answers **504** `{"error":"js_timeout","message":"script exceeded the 5000 ms execution limit"}` (with the configured value). `ApiError` gets `JsTimeout` (504) and `JsDepthLimit` (422), keeping the exhaustive, no-wildcard mapping.
- **D-09:** Isolation from other requests:
  - Actix workers never run V8 (Phase 3), so non-PATCH routes are always answered.
  - A `while(true){}` holds one JS worker only until the timeout. Other PATCHes run on other workers, or wait in the bounded queue and run once a worker frees up.
  - Tests prove both: a pool with ≥2 workers runs an infinite loop alongside a normal PATCH that completes, plus a non-JS route answered during the loop.
  - The timed-out worker serves the next job normally.

#### Tests, docs, carry-overs
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

### Deferred Ideas (OUT OF SCOPE)
- Heap limit / near-heap-limit callback (JSHD-01), cancel on disconnect (JSHD-02), 503 backpressure (JSHD-03), snapshots (JSHD-04): v2.
- Env-configurable timeout and depth (`SERVER.*` allowlist extension): OPS-02 (v2).
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| ROBU-05 | A script that recursively PATCHes itself (directly or via a cycle) is stopped by a nesting depth limit and returns 422 instead of crashing the server | Pattern 4 (depth check before `store.get`, chain in `ExecCtx`, `OnceLock` stop record). The native stack per level was measured at 12.6 KiB debug / 3.7 KiB release (spike S9). The limit semantics match the D-03 example message (Open Question 1). Finite-chain RED strategy is in Pitfall 8 |
| ROBU-06 | A non-terminating script (e.g. `while(true){}`) is terminated after a configurable timeout and returns 504; the server keeps serving other requests | Patterns 1–3 and 5–6. **Critical finding S4/S5:** a slow op that returns `Err` while its isolate is terminating cancels the termination, so ops must go silent once the job timed out, and the watchdog must re-fire until the job is disarmed. Backstop with a "started" signal (Pattern 6) |
| TEST-04 | Each error path (404, 400, 422 insert, 422 JS exception, depth limit, timeout) has a test | The 404/400/422 tests already exist (names in Validation Architecture). New depth and timeout tests are mapped with timing measured under 8-thread load (spike S12) |
</phase_requirements>

## Project Constraints (from CLAUDE.md)

- Toolchain Rust 1.99.1, edition 2024, `rust-version = "1.99"`. Released crates.io dependencies only (no git, no beta).
- `#![forbid(unsafe_code)]` (src/lib.rs:1) and `unsafe_code = "forbid"` in Cargo.toml. Every API this phase needs (`thread_safe_handle`, `IsolateHandle::terminate_execution`) is a safe fn. The spike used no `unsafe`.
- `[lints.clippy] unwrap_used = "deny"`, `expect_used = "deny"` (tests are allowed through clippy.toml). Non-test code must not `.unwrap()` a `Mutex::lock()`. Use `lock().unwrap_or_else(std::sync::PoisonError::into_inner)`.
- Never depend on `v8` directly. Use `deno_core::v8` (`deno_core::v8::IsolateHandle`).
- Single binary crate with modular layers, dependencies pointing http → js → store. The JS module stays free of actix-web.
- Work on branch `modernize-toolchain` and commit per plan. Git identity comes from env vars only. Never push.
- GSD workflow enforcement: edits happen through `/gsd-execute-phase`.
- CI guards (scripts/ci-guards.sh): no `#[allow]`/`#[expect]`, no `#[ignore]`, the unprotected-platform feature stays dev-only, a single v8, no legacy op syntax, and ci.yml is pinned by canonical lines (so `ci.yml` must not be edited; `scripts/ci-smoke.sh` may be).
- `/tmp` is ~96% full. Scratch work goes in `target/tmp/`. A project hook blocks naming the dotenv file, so smokes use `env -i SERVER.HOST=… SERVER.PORT=…`.

## Summary

The phase needs no new crates. Both limits are built from deno_core 0.412 primitives that already exist in the tree: `JsRuntime::v8_isolate().thread_safe_handle()` returns a `Send + Sync` `IsolateHandle`, and its `terminate_execution()` works from any thread and returns `false` on a disposed isolate. All spikes compiled and ran on this host against the project's `Cargo.lock`. A terminated `execute_script` returns `Err(JsError)` with `exception_message == "Uncaught Error: execution terminated"` about 1 ms after the deadline (debug and release, also with 8 V8 threads on 4 cores). The next job on the same thread is unaffected.

**The research flag uncovered one critical hazard the existing design docs miss.** When a *slow* op such as `op_rs3_patch` or `op_rs3_get` returns `Err` while its isolate is being terminated, deno_core builds the JS error by calling a JS callback (`to_v8_error`, error.rs:395-447). That call fails under termination, and deno_core then calls `TryCatch::reset()`, which per the v8 docs "cancels the scheduled exception". **The termination is silently cancelled** and the message is thrown as a plain string. Observed effects:

- (a) `try { op_rs3_patch('loop') } catch (e) {} while(true){}` runs forever even though both isolates were terminated (spike S4).
- (b) A loop calling a throwing op, `for(;;){ try { op() } catch(e){} }`, survived a single termination **7 of 20 runs** (spike S5).

The fix is two cheap rules, verified to terminate every case:

1. Once the job is timed out, every rs3 op returns `Ok(String::new())` without touching the store and never `Err`. That includes `op_rs3_patch` after a terminated nested run.
2. The watchdog re-terminates all registered isolates every 50 ms until the worker disarms it. With re-fire, the 20-run test hung 0 of 20 times.

Classification must be **by flag, never by result**. A terminated outer script whose remaining code is straight-line (no loop back-edge and no function call after the op returns) completes with `Ok(..)` even though it was terminated (spikes S4b and S6). In 63 of 300 near-deadline jobs, the watchdog won the race after `execute_script` had already returned `Ok`. A single `AtomicU8` phase (`Running → Finished | TimedOut`, set by compare-and-swap) makes the outcome linearizable. A late timer for job N can never touch job N+1, because each job owns its own `Arc<JobState>` and isolate list: 0 canary failures in 900 race trials.

**Primary recommendation:** Add a per-job `JobState` holding the phase CAS, `host_fault`, a depth-stop record and the registered isolate handles. Replace `ExecCtx.host_fault` with it. Run one watchdog thread per JS worker, spawned by the worker so it descends from the V8-initialising thread, with an `Arm`/`Disarm` mpsc protocol and 50 ms re-fire. Rs3 ops go silent after a timeout. The depth check in `op_rs3_patch` happens before the store read. Classify once, in the executor, with precedence host fault > timeout > depth > result. Add an executor-level backstop of `timeout + 2 s` that starts when a worker picks the job up, not at enqueue.

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| Depth limit check + key chain | JS ops (`src/js/ops.rs`, `op_rs3_patch`) | `ExecCtx` (chain, max depth) | Only the op knows it is about to create a nested runtime. D-02 puts the check before runtime creation |
| Termination group (isolate handle registry) | JS runtime (`src/js/runtime.rs`, `run_fresh`) | `JobState` (new, js layer) | `run_fresh` is the single place where every runtime, outer and nested, is created and dropped |
| Watchdog timer | JS executor (`src/js/executor.rs`, per-worker thread) | — | It must be independent of the HTTP future (a disconnected client drops it) and of the worker's tokio runtime (not driven while V8 runs) |
| Outcome classification (precedence) | JS executor (job completion) | `ExecError` | D-04 asks for one place. Nested `run_script` calls must not classify, or `op_rs3_patch` would mistake them for host faults |
| Backstop (worker never replies) | JS executor (`run`/`execute` awaiting the reply) | HTTP handler (unchanged; maps `ExecError`) | Testable without HTTP, and keeps the handler thin. It satisfies D-07 because the handler awaits it |
| Status/body mapping (504 / 422) | HTTP (`src/http/error.rs`) | — | Exhaustive `ApiError` mapping, no wildcard |
| Settings (`timeout_ms`, `max_patch_depth`) | `JsSettings` (`src/js/mod.rs`), built in code | `main.rs` / test executors | Phase 2 D-16 allowlist. No env |

## Spike Results (compiled and run 2026-10-20, deno_core 0.412.0 / v8 150.4.0, aarch64, project Cargo.lock, scratch crate deleted afterwards)

| # | Question | Result | Evidence |
|---|----------|--------|----------|
| S1 | `execute_script` after `terminate_execution` from another thread | `Err(Box<JsError>)` with `exception_message = "Uncaught Error: execution terminated"`. Elapsed 201.4 ms for a 200 ms deadline. The next job on the same thread (a fresh runtime) returns `Ok` | [VERIFIED: spike A; deno_core error.rs:1676-1682 builds `new Error("execution terminated")`] |
| S2 | Nested `while(true){}` inside `op_rs3_patch`, both isolates terminated, uncaught | Outer `Err`, 202 ms. But the outer error is `"Uncaught Uncaught Error: execution terminated"`, which is the op's thrown error and not a termination (see S4) | [VERIFIED: spike B] |
| S3 | Only the nested isolate terminated (outer not registered) | The outer catches and **continues**: `Ok("outer continued after: …")`. Every isolate of the job must be registered and terminated | [VERIFIED: spike D] |
| S4 | Op returns `Err` while its isolate is terminating | **The termination is cancelled.** `try { op_rs3_patch('loop') } catch (e) {} while(true){}` hung until the 120 s kill switch. The same happened with a plain non-nested op that sleeps then throws, so nesting is not the cause. The caught value is the *string* `"Uncaught Error: execution terminated"`. Re-terminating inside the op before returning `Err` does **not** help (the cancel happens after the op returns). An op that returns `Ok` keeps the termination (outer terminated, 202 ms) | [VERIFIED: spikes C1/T0/T1/T2/T4/T5; root cause deno_core error.rs:395-447 (`to_v8_error` → `tc_scope.reset()` on callback failure) + v8 scope.rs:1268-1271 ("Cancels the scheduled exception if it is caught")] |
| S4b | Terminated outer, the op returns Ok, then straight-line code | `try{…}catch{}; 'done:' + m` completes **`Ok("done:none")`** with phase TimedOut. V8 only acts on termination at stack checks (loop back-edges, function entry). Hence classification by flag | [VERIFIED: spikes C2/Se/Sf] |
| S5 | Loop over a throwing op, single termination | Unchecked op: **hung 7/20**. With a 50 ms re-fire: hung 0/20 (5/20 needed a second fire). An op that checks the timed-out flag and returns `Ok`: hung 0/20 even without re-fire | [VERIFIED: spike S, 20-run loops] |
| S6 | Fixed design (silent ops + re-fire) on: caught nested then loop; retry loop of nested loops; three nested levels each catching and looping | All terminate at 201–203 ms (debug and release). The top-level result is `Err` or `Ok("")`, and the phase is always TimedOut | [VERIFIED: spikes Sc/Sd/Sf debug + release] |
| S7 | `terminate_execution` on an **idle** isolate, then `execute_script` | Even `1+1` fails with `"Uncaught Error: execution terminated"` (~0.25 ms). So registering a handle *after* the watchdog fired and terminating immediately is effective | [VERIFIED: spike E] |
| S8 | Infinite loop inside `toString` during result stringification | `to_string` returns `None`, `has_terminated = true`, exception `null`. Phase TimedOut. The existing `stringify` already maps this to `Err`, so classify by flag | [VERIFIED: spike F] |
| S9 | Native stack per nesting level | **12,928 bytes (debug), 3,776 bytes (release)**, constant per level (measured as the address delta between op invocations at depth 0..63). Depth 64 took 692 ms debug / 629 ms release, about 10 ms per fresh runtime | [VERIFIED: spike G] |
| S10 | Minimum worker stack for a depth-4 chain plus a leaf `f(n){return f(n+1)+1}` RangeError | 256 KiB and 512 KiB: **"thread has overflowed its stack" abort (exit 134)**, debug and release. 1 MiB: OK. 2 MiB: OK even at depth 64. **A 2 MiB floor (IN-07) gives ≥2× headroom at depth 4** | [VERIFIED: spike Z] |
| S11 | Late timer / next-job safety | 900 jobs with a deadline close to the job length, each followed by a canary job: **0 canary failures**. 63/300 jobs had an `Ok` result but lost the CAS (TimedOut), so the policy must be "the phase CAS decides" | [VERIFIED: spike H, to=11/14/18 ms] |
| S12 | Timing under load (4 cores) | 8 threads: 4 run `while(true){}` with a 250 ms timeout, 4 run normal jobs. Loops end at 250–251 ms. Normal jobs take 21–80 ms (first one ~75 ms). Fresh runtime + `1+1`: median 10.5 ms debug, 9.1 ms release. Termination overshoot ~1 ms | [VERIFIED: spikes P, cost, K] |
| S13 | Op stall in Rust (`sleep 600 ms`), 200 ms timeout | `terminate_execution` cannot interrupt Rust. The job returns after 613 ms. If JS continues with a loop it is terminated right after (J2). If straight-line, it completes `Ok` (J), and the flag still says TimedOut. This is the D-07 backstop case | [VERIFIED: spikes J/J2] |
| S14 | Baseline suite | 171 tests pass in 0.42 s (`cargo test --locked --offline`) | [VERIFIED: run this session] |

## Standard Stack

### Core (all already locked; no new crates)

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `deno_core` | 0.412.0 | `JsRuntime::v8_isolate()` (jsruntime.rs:1420), `execute_script`, `#[op2(reentrant)]` | Already the project runtime |
| `deno_core::v8::IsolateHandle` (v8 150.4.0 via deno_v8 0.4.0) | 150.4.0 | `terminate_execution()`: "can be used by any thread", "Returns false if Isolate was already destroyed" (v8 isolate.rs:2237-2251), mutex-guarded against disposal (isolate.rs:2201-2222) | The only termination API. Safe fn |
| `std::sync::mpsc` + `std::thread` | std | Per-worker watchdog thread with `recv_timeout` | No timer crate needed. Works while the worker is blocked in V8 |
| `std::sync::{Mutex, OnceLock, atomic::AtomicU8/AtomicBool}` | std | `JobState` phase CAS, isolate registry, first-wins depth stop record | std only |
| `tokio` | 1.54.0 | `tokio::time::timeout` for the backstop; `oneshot` started/reply signals | Already a dependency (`features = ["rt", "sync"]`). `time` is already enabled in the graph through actix-rt/deno_core (verified: `cargo tree -e features -i tokio` lists `tokio feature "time"`). **Declare `"time"` explicitly** in `[dependencies]` because production code now uses it |

### Supporting

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `tokio` `macros` (dev edge) | 1.54.0 | `tokio::join!` in the D-09 concurrency test | Already enabled in the graph (`tokio feature "macros"`). Declare it in `[dev-dependencies]` if `join!` is used. This is not the guarded unprotected-platform feature, so guard (d) is unaffected |

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| Per-worker watchdog thread | tokio timer on the worker's current_thread runtime | **Does not work.** The worker runs the job outside `block_on` (`let _enter = rt.enter()`, executor.rs:136), so that runtime is not polled while V8 runs and its timers never fire [VERIFIED: src/js/executor.rs:129-143 read this session] |
| Per-worker watchdog thread | Timer on the HTTP side (`tokio::time::timeout` around the reply, ARCHITECTURE Pattern 5) | Dies with the request future. A client disconnect drops it, and the `while(true)` then pins a worker forever. Keep it only as the D-07 backstop |
| Per-worker watchdog thread | Thread per job | Fewer lines, but adds spawn-failure handling and thread churn per PATCH |
| Per-worker watchdog thread | One global watchdog with a deadline heap + Condvar | More code (heap, wakeups) for no gain at ≤ cores workers |
| Silent ops after timeout | Re-terminate from inside the op before `Err` | **Does not work** (spike T4/T5) |

**Installation:** none. Cargo.toml edit only: `tokio = { version = "1.54.0", features = ["rt", "sync", "time"] }`, plus optionally a dev-dependency `tokio = { version = "1.54.0", features = ["macros"] }`. Features are not recorded in Cargo.lock, so `cargo build --locked` must still pass with an unchanged lock. The plan must verify this with `git diff --exit-code Cargo.lock`.

## Package Legitimacy Audit

No external packages are installed or added in this phase. Every crate used is already in `Cargo.lock` (deno_core 0.412.0, tokio 1.54.0 and std).

**Packages removed due to [SLOP] verdict:** none
**Packages flagged as suspicious [SUS]:** none

## Architecture Patterns

### System Architecture Diagram

```
PATCH /key/{k} ──► actix worker: patch_key
                     │ store.get(k)?  (404 before queueing)
                     ▼
               JsExecutor::execute(store, k, src)
                     │ JobState::new()  ── ExecCtx{store, job, depth 0, chain [k], max_depth}
                     │ send Envelope{run, job, started_tx, reply_tx}  (waits while queue full)
                     │ await started_rx ─────────────┐ (queue wait is not timed)
                     │ timeout(timeout + 2s, reply_rx)  ── elapsed ──► log + ExecError::Timeout (BACKSTOP)
                     ▼                               │
   rs3-js-N worker: dequeue ──► started_tx.send() ───┘
                     │ watchdog.send(Arm(job, now+timeout))
                     │ catch_unwind(run) ─► run_script ─► run_fresh:
                     │        JsRuntime::try_new ─► job.register(handle)  (terminate now if TimedOut)
                     │        execute_script ──► JS ──► op_rs3_patch(key)
                     │            live_ctx: TimedOut → Ok("") | host_fault → Err(internal) | depth stop → Err(depth)
                     │            depth check (chain.len()+1 > max) → job.stop_depth(chain), Err(depth msg)
                     │            store.get(key) → run_script(ctx.nested(key)) ─► (same run_fresh, registered)
                     │            after nested: TimedOut → Ok("") (never Err: Err cancels termination)
                     │        drop runtime + unregister
                     │ job.finish()  (CAS Running→Finished; loses if TimedOut)
                     │ watchdog.send(Disarm)
                     │ classify(job, result): Internal > TimedOut > DepthStop > result
                     ▼
               reply_tx.send(result)  ──► ApiError::from ──► 200 / 422 js_error / 422 js_depth_limit / 504 js_timeout / 500

   watchdog thread (one per worker, spawned by the worker):
     idle ──Arm──► armed ──deadline──► job.fire(): CAS Running→TimedOut, terminate all registered
                     ▲                    │ every 50 ms while still armed: re-terminate all registered
                     └──────Disarm────────┘
```

### Recommended Project Structure (files touched)

```
src/js/
├── mod.rs        # ExecError::{Timeout{limit_ms}, DepthLimit{limit, chain}}; JsSettings.{timeout_ms, max_patch_depth}; test helpers
├── job.rs        # NEW: JobState (phase CAS, host_fault, depth stop, isolate registry), Registration guard, Watchdog
├── ops.rs        # ExecCtx{job, depth, chain, max_patch_depth}; live_ctx gate; depth check; silent-after-timeout; op_print retry
├── runtime.rs    # run_fresh registers/unregisters the isolate; log suppression uses job.stopped()
└── executor.rs   # Envelope{job, started}; watchdog per worker; classify(); backstop; stack clamp; catch_unwind comment
src/http/error.rs # ApiError::{JsTimeout(u64), JsDepthLimit{..}} → 504 js_timeout / 422 js_depth_limit
src/main.rs       # unchanged (JsSettings::default() gains defaults)
docs/js-ops.md    # Limits section rewrite (D-12)
scripts/ci-smoke.sh # optional: release-binary depth + timeout smoke (protected platform on x86 CI)
```
A separate `job.rs` is a recommendation. It keeps ops.rs (already ~800 lines with tests) from growing further. Putting the code in executor.rs is also acceptable.

### Pattern 1: `JobState` (termination group plus all per-job flags)

**What:** One `Arc<JobState>` per PATCH job, shared by every level through `ExecCtx`. It replaces `host_fault: Arc<AtomicBool>` (src/js/ops.rs:25) and generalises the Phase 3 shared-flag pattern.

```rust
// Pattern verified in spike (names are discretion). No unwrap: clippy denies it outside tests.
use std::sync::{Mutex, OnceLock, PoisonError, atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering}};
use deno_core::v8;

const RUNNING: u8 = 0;
const FINISHED: u8 = 1;
const TIMED_OUT: u8 = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DepthStop { pub limit: u32, pub chain: Vec<String> }

#[derive(Debug, Default)]
pub struct JobState {
    phase: AtomicU8,                                   // RUNNING → FINISHED | TIMED_OUT (CAS, exactly once)
    host_fault: AtomicBool,
    depth_stop: OnceLock<DepthStop>,                   // first stop wins; its chain goes in the 422 body
    isolates: Mutex<Vec<(u64, v8::IsolateHandle)>>,
    next_id: AtomicU64,
}

impl JobState {
    fn handles(&self) -> std::sync::MutexGuard<'_, Vec<(u64, v8::IsolateHandle)>> {
        self.isolates.lock().unwrap_or_else(PoisonError::into_inner)
    }
    /// Register a freshly created runtime. Checked under the lock, so a concurrent
    /// fire() either sees this handle or this call sees TIMED_OUT (spike S7: an idle,
    /// pre-terminated isolate fails its first script).
    pub fn register(&self, handle: v8::IsolateHandle) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let mut list = self.handles();
        if self.timed_out() { handle.terminate_execution(); }
        list.push((id, handle));
        id
    }
    pub fn unregister(&self, id: u64) { self.handles().retain(|(i, _)| *i != id); }
    /// Watchdog only. First call wins the CAS; later calls (re-fire) re-terminate.
    /// A FINISHED job is never touched.
    pub fn fire(&self) {
        let won = self.phase.compare_exchange(RUNNING, TIMED_OUT, Ordering::SeqCst, Ordering::SeqCst).is_ok();
        if won || self.timed_out() {
            for (_, h) in self.handles().iter() { h.terminate_execution(); } // false on disposed isolates: harmless
        }
    }
    /// Worker only, after run_script returned. Returns true if the job timed out.
    pub fn finish(&self) -> bool {
        self.phase.compare_exchange(RUNNING, FINISHED, Ordering::SeqCst, Ordering::SeqCst).is_err()
    }
    pub fn timed_out(&self) -> bool { self.phase.load(Ordering::SeqCst) == TIMED_OUT }
    // host_fault()/note_host_fault() move here unchanged; stop_depth(DepthStop) -> (&DepthStop, bool newly_set)
    pub fn stopped(&self) -> bool { self.timed_out() || self.host_fault() || self.depth_stop.get().is_some() }
}
```
Registration in `run_fresh` (src/js/runtime.rs:62) goes right after `JsRuntime::try_new(..)?`: `let id = watch.job.register(rt.v8_isolate().thread_safe_handle());`. A small guard struct (`struct Registered<'a>(&'a JobState, u64)` with `Drop` → `unregister`) guarantees removal on every exit path, including unwinding. Drop order relative to `rt` does not matter, because `terminate_execution` on a disposed isolate returns `false` [VERIFIED: v8 isolate.rs:2243-2251].

### Pattern 2: One watchdog thread per worker (Arm / Disarm / 50 ms re-fire)

**What:** Each `rs3-js-N` worker spawns its own watchdog thread *from inside the worker closure* before reporting ready. The watchdog thread then descends from the main thread that called `init_platform`, which keeps the PKU thread-ancestry rule safe on the protected release platform.

```rust
// Verified protocol (spike); restructured as a state machine so it can never panic.
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};
const REFIRE: Duration = Duration::from_millis(50);

pub enum Cmd { Arm(std::sync::Arc<JobState>, Instant), Disarm }

pub fn watchdog_loop(rx: Receiver<Cmd>) {
    let mut armed: Option<(std::sync::Arc<JobState>, Instant)> = None;
    loop {
        let msg = match &armed {
            None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
            Some((job, deadline)) => rx.recv_timeout(if job.timed_out() {
                REFIRE
            } else {
                deadline.saturating_duration_since(Instant::now())
            }),
        };
        match msg {
            Ok(Cmd::Arm(job, deadline)) => armed = Some((job, deadline)),
            Ok(Cmd::Disarm) => armed = None,
            Err(RecvTimeoutError::Timeout) => if let Some((job, _)) = &armed { job.fire() },
            Err(RecvTimeoutError::Disconnected) => return, // worker gone
        }
    }
}
```
Worker side, per job:
1. `started.send(())`.
2. `wd.send(Arm(job.clone(), Instant::now() + timeout))`.
3. Run under `catch_unwind`.
4. `let timed_out = job.finish();`
5. `wd.send(Disarm)`.
6. `classify(..)`.
7. Reply.

A failed `wd.send` (watchdog gone, which cannot happen without a panic) must not panic. Log once and rely on the backstop. **Why re-fire:** see Pitfall 1. A thrown op error, including deno_core *built-in* ops that rs3 cannot gate, cancels a termination that lands during error construction. Re-terminating every 50 ms makes the eventual termination overwhelmingly likely (spike S5: 0/20 hangs).

### Pattern 3: Ops go silent once the job timed out (never `Err`)

**What:** `live_ctx` (src/js/ops.rs:140-147) becomes a three-way gate. While timed out, ops answer `Ok(String::new())` and do not touch the store. Returning `Err` would cancel the termination (spike S4).

```rust
/// None = the job timed out: the caller returns Ok(String::new()) without touching the
/// store. Never throw then: a slow op's Err makes deno_core cancel the pending V8
/// termination (to_v8_error → TryCatch::reset), and the script would keep running.
fn live_ctx(state: &OpState) -> Result<Option<&ExecCtx>, JsErrorBox> {
    let ctx = state.borrow::<ExecCtx>();
    if ctx.job.timed_out() { return Ok(None); }                       // checked first
    if ctx.job.host_fault() { return Err(internal_error()); }
    if let Some(stop) = ctx.job.depth_stop() { return Err(depth_error(stop)); }
    Ok(Some(ctx))
}
// in each op:  let Some(ctx) = live_ctx(state)? else { return Ok(String::new()) };
```
`op_rs3_patch` additionally checks `ctx.job.timed_out()` **after** the nested `run_script` returns and before mapping its error, and returns `Ok(String::new())` in that case (spike Sc/Sd/Sf).

### Pattern 4: Depth check and key chain in `op_rs3_patch`

```rust
#[op2(reentrant)]
#[string]
fn op_rs3_patch(state: &mut OpState, #[scoped] key: StrArg) -> Result<String, JsErrorBox> {
    let StrArg(key) = key;
    let Some(ctx) = live_ctx(state)? else { return Ok(String::new()) };
    let ctx = ctx.clone();                       // no OpState borrow across the nested run
    // chain[0] is the PATCHed key (depth 0). The limit counts levels including the top:
    // with max 4, a(0) b(1) a(2) b(3) run, and b's call for `a` is refused with
    // "a -> b -> a -> b -> a" (the D-03 example). See Open Question 1.
    if ctx.chain.len() as u32 + 1 > ctx.max_patch_depth {
        let mut chain = ctx.chain.clone();
        chain.push(key);
        let (stop, first) = ctx.job.stop_depth(DepthStop { limit: ctx.max_patch_depth, chain });
        if first { ctx.log_error(format_args!("js depth limit: {:?}", stop.chain.join(" -> "))); }
        return Err(depth_error(stop));           // a TypeError/Error is fine: no termination involved
    }
    let source = ctx.store.get(&key).map_err(to_js)?;
    let result = super::runtime::run_script(ctx.nested(&key), &key, source);
    if ctx.job.timed_out() { return Ok(String::new()); }     // Pattern 3
    result.map_err(|e| match e { /* Script → generic minus "Uncaught "; Internal/Unavailable → existing
        single-log host-fault path; Timeout/DepthLimit are never produced by nested run_script, but
        must be listed explicitly (no wildcard) and must NOT be treated as host faults */ })
}
```
- The depth check comes **before** `store.get`, so no store read happens past the limit, and the chain names the attempted key even when it is missing.
- `ExecCtx::nested(&self, key: &str)` appends `key` to `chain` and increments `depth`.
- `ExecCtx::new` must receive the root name (`execute()` passes the PATCHed key).
- `ExecCtx.chain` is a `Vec<String>` of at most `max_patch_depth` entries. The cloning cost is negligible.

### Pattern 5: Classify once, in the executor

```rust
/// D-04 precedence: host fault > timeout > depth limit > the script's own result.
fn classify(job: &JobState, timed_out: bool, result: Reply, limits: &Limits) -> Reply {
    match result {
        Err(ExecError::Internal(_)) => result,                 // panics + runtime faults (run_script already
        _ if job.host_fault() => Err(ExecError::Internal(..)), //  maps nested faults to Internal)
        _ if timed_out => Err(ExecError::Timeout { limit_ms: limits.timeout_ms }),
        _ => match job.depth_stop() {
            Some(stop) => Err(ExecError::DepthLimit { limit: stop.limit, chain: stop.chain.clone() }),
            None => result,
        },
    }
}
```
`timed_out` is the result of `job.finish()` (the CAS), so a job the watchdog lost to is `Ok`, and a job it won is `Timeout`, even if the value was computed (spike S11). That is consistent with D-03's "hard stop even when caught". `run_script` stays the per-level runner and does not learn about timeout or depth classification.

### Pattern 6: Backstop that starts when a worker picks the job up

```rust
const BACKSTOP_GRACE: Duration = Duration::from_secs(2);
// in run/execute:
let (started_tx, started_rx) = oneshot::channel();
let (reply_tx, reply_rx) = oneshot::channel();
self.tx.send(Envelope { run, job, started: started_tx, reply: reply_tx }).await.map_err(|_| ExecError::Unavailable)?;
// Queue wait is not limited (same as Phase 3). An Err here means the worker dropped the envelope;
// reply_rx then fails too and maps to Unavailable below.
let _ = started_rx.await;
match tokio::time::timeout(self.limits.timeout + BACKSTOP_GRACE, reply_rx).await {
    Ok(Ok(r)) => r,
    Ok(Err(_)) => Err(ExecError::Unavailable),
    Err(_) => { eprintln!("error: JS worker did not reply within {} ms", ..); Err(ExecError::Timeout { limit_ms }) }
}
```
If the deadline started at enqueue, it would produce false 504s for jobs waiting behind busy workers (queue = workers × 16). Those jobs would still run later and write. The worker side needs no change for "no leak": it finishes or is terminated, and then `reply.send` fails silently (executor.rs:141, `let _ = env.reply.send(result);`).

### Pattern 7: Carry-overs (D-13)

- **IN-04 `op_print` retry:** mirror upstream deno_core 0.412 `op_print` (ops_builtin.rs:233-261). Write in a manual loop, `continue` on `Interrupted`, `std::thread::yield_now()` and continue on `WouldBlock`. Upstream ends with `out.flush().unwrap()`, which rs3 must **not** copy (clippy deny, and an op panic aborts). Retry the flush with the same `WouldBlock`/`Interrupted` rule and return other errors. Keep the stderr path's single Debug-escaped line (`js stderr: {msg:?}`): format it into a `String` first, then write the bytes through the same retry helper. The signature must stay exactly `(#[string] msg: &str, is_err: bool) -> Result<(), std::io::Error>`, because `with_implementation_from` requires it (src/js/ops.rs:246-256, 265-268).
- **IN-06 comment:** executor.rs:126-128 says "a panicking job cannot kill the worker". Change it to say it catches panics in job code outside V8 callbacks, and that a panic inside an op callback (any `op_rs3_*`, including the nested runtime run by `op_rs3_patch`) unwinds across V8 frames and aborts the process, which is why ops never panic (ROBU-04).
- **IN-07 clamp:** executor.rs:45 `.stack_size(settings.stack_bytes)` becomes `.stack_size(settings.stack_bytes.max(MIN_STACK_BYTES))` with `MIN_STACK_BYTES = 2 * 1024 * 1024` (spike S10: 512 KiB aborts, 1 MiB is the minimum at depth 4, 2 MiB is fine to depth 64). Document it next to the `workers`/`queue_capacity` clamps (executor.rs:30-32).
- **IN-05 parity:** the `.txt` scripts are **not** textually equal to the `.js` files (whitespace differs, and `bin.txt` uses `t`/`s` where `bin.js` uses `tree`) [VERIFIED: requests/*.txt read this session]. Test "same results": `POST /insert` with `include_str!("../../requests/{fac,sum,bin,store}.txt")`, seed the inputs (`num=5`, `numbers="1 2 3 4"`, `tree="5 3 8"`), PATCH the key, and assert the same body bytes as the `.js` fixture tests (`"120"`, `"10"`, `TREE_BODY`, `"{\"res\":\"1111\"}"`).

### Anti-Patterns to Avoid
- **Classifying by message** (`"execution terminated"`, `"depth limit"`). Strings change between levels (`Uncaught Uncaught …`, S2), and a caught termination becomes a string value (S4). Use the flags.
- **Returning `Err` from an rs3 op while the job is timed out.** It cancels the termination (S4).
- **Firing the watchdog once.** A termination that lands during op error construction is lost (S5).
- **A tokio timer on the worker runtime.** It never fires during the job (executor.rs:136 uses `rt.enter()`, not `block_on`).
- **Terminating only the top isolate, or only the innermost.** The other keeps running (S3).
- **Classifying inside `run_script`.** `op_rs3_patch`'s non-Script arm would then log and record a host fault for a timeout or depth stop.
- **A backstop timed from enqueue.** It gives false 504s under queue load.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Interrupting running JS | Polling a flag from JS, or `Isolate::request_interrupt` callbacks | `IsolateHandle::terminate_execution()` | Uncatchable by JS and thread-safe. Polling is defeatable by user code |
| Detecting a disposed isolate | Your own liveness tracking | The `IsolateHandle` mutex (returns `false` after dispose) | Already race-free in v8 (isolate.rs:2201-2222) |
| Timer wheel / deadline heap | Custom scheduler | `std::sync::mpsc::Receiver::recv_timeout` per worker | One armed job per worker at a time |
| Job-identity race protection | Generation counters | Per-job `Arc<JobState>` + one `AtomicU8` CAS | The watchdog only ever holds the current job's Arc. A late fire on a finished job is a no-op (S11) |
| JSON escaping of the key chain | Manual escaping | `serde` via the existing `ErrorBody` | Keys are arbitrary UTF-8. Debug-format (`{:?}`) for log lines |

**Key insight:** V8 termination is cooperative, has a short lifetime, and can be cancelled in two hidden ways (op errors via `TryCatch::reset`, and deno_core's own `exception_to_err` cancel/resume). Robustness comes from (1) never throwing after the stop, (2) re-terminating until disarmed, and (3) deciding the outcome from host-side flags.

## Common Pitfalls

### Pitfall 1: A thrown op error cancels the V8 termination
**What goes wrong:** A timed-out script keeps running forever and pins a JS worker. The request still gets 504 from the backstop, but the worker is lost.
**Why it happens:** For slow ops, deno_core's `to_v8_error` calls the JS custom-error builder. Under termination the call returns `None`, and deno_core runs `tc_scope.reset()`, which "cancels the scheduled exception", then throws the message string [VERIFIED: deno_core error.rs:395-447; v8 scope.rs:1268-1271; spike S4/S5].
**How to avoid:** Pattern 3 (silent ops after the timeout) plus Pattern 2 (50 ms re-fire until disarm).
**Warning signs:** A test whose loop catches an op error hangs, or the caught value in a script is a string `"Uncaught Error: execution terminated"`.

### Pitfall 2: A terminated script can still complete `Ok`
**What goes wrong:** A 200 response for a timed-out script, or flaky tests.
**Why it happens:** V8 only acts on termination at stack checks. Straight-line code after an op returns can finish (S4b, S6, S13).
**How to avoid:** Classify by the phase CAS (Pattern 5). Ops refuse writes silently after the timeout, so the work done after the stop is nil.

### Pitfall 3: A late timer kills the next job
**How to avoid:** Use a per-job `Arc<JobState>`, `fire()` only CASes from `RUNNING`, the isolate list belongs to the job, and the worker always sends `Disarm` before taking the next job. Verified with 0/900 canary failures (S11).

### Pitfall 4: Registration race (watchdog fires during `JsRuntime::try_new`)
**What goes wrong:** A nested runtime created after the fire is never terminated, so a nested `while(true)` runs forever.
**How to avoid:** `register()` checks `timed_out()` under the registry lock and terminates the new handle immediately. A pre-terminated idle isolate fails its first script (S7). Re-fire also covers it.

### Pitfall 5: Log amplification (single-log rule from Phase 3 WR-01)
**What goes wrong:** One timeout or depth stop logs `js error in …` once per level.
**How to avoid:** In `run_fresh`, replace `if !watch.host_fault()` (runtime.rs:70, 81) with `if !watch.job.stopped()`. Log the depth stop once where it is first set (Pattern 4, `first`). Log the timeout once in the executor (`error: js timeout in {name:?} after {ms} ms`) and the backstop once. Extend the `error_logs` test counter assertions to the new paths.

### Pitfall 6: Tiny-stack RED test crashes the whole test binary
**What goes wrong:** A RED test that spawns a worker with `stack_bytes: 0` before the clamp exists overflows the native stack. That is a process abort (exit 134, S10), not an assertion failure, so the whole `cargo test` run dies.
**How to avoid:** Unit-test the clamp as a pure function (`effective_stack_bytes(&settings) == 2 MiB` for 0). Do not spawn tiny-stack threads in tests.

### Pitfall 7: `JsSettings` struct literals break
**What goes wrong:** `dedicated()` in executor tests (executor.rs:160-167) builds `JsSettings { workers, queue_capacity, stack_bytes }` and stops compiling once fields are added.
**How to avoid:** Use `..JsSettings::default()` there and in every new test constructor.

### Pitfall 8: RED tests that hang or allocate 800 MB (D-11)
**What goes wrong:** Before the limits exist, a RED depth test with a self-patching cycle recurses unboundedly (~800 MB, Phase 3 S1c). A RED timeout test with `while(true){}` never returns.
**How to avoid:**
- **Depth RED:** use a *finite, acyclic* over-deep chain, for example `k1 → k2 → … → k6` with `k6 = "6"`. Without the limit it returns 200 `"6"` and the RED test fails on the assertion. With the limit it is 422 naming `k1 -> k2 -> k3 -> k4 -> k5`. Add the cyclic tests (`a ↔ b`, `me → me`, caught-but-422) only in or after the GREEN commit.
- **Timeout RED:** use a *bounded* busy loop, `const end = Date.now() + 1500; while (Date.now() < end) {}`, on a 300 ms executor. Without the watchdog it returns 200 after 1.5 s (assertion fails). With it, 504 after ~300 ms. Add `while(true){}` tests only in or after GREEN.
- Add the `JsSettings`/`ExecError` fields and the `ApiError` variants as compiling stubs first, so RED fails on assertions and not on compilation.

### Pitfall 9: Short-timeout executors running normal work
**What goes wrong:** False 504s under parallel `cargo test` or slow CI.
**How to avoid:** Use dedicated short-timeout executors only for scripts that loop, plus trivial follow-ups (`1+1`). Normal jobs take 21–80 ms debug under 8-thread load (S12), so keep timeouts at **≥300 ms** where a follow-up job must succeed on the same executor. The shared `test_executor()` keeps the 5000 ms default. Call `crate::js::init_platform()` (idempotent) before `JsExecutor::start` in any dedicated test executor, as `zero_settings_are_clamped` does (executor.rs:325-326).

### Pitfall 10: The watchdog thread must descend from the V8-initialising thread
**What goes wrong:** On PKU x86 with the protected platform, V8 requires threads that use V8 to descend from the `v8::Initialize` thread [CITED: v8 platform.rs:311-314]. `terminate_execution` only sets an interrupt on the isolate. It is documented as callable from any thread (isolate.rs:2240-2241), so it is likely safe even from an unrelated thread [ASSUMED].
**How to avoid:** Spawn each watchdog thread from inside its worker closure. Workers descend from `main`, which calls `init_platform` first (main.rs). Prove it on x86 with the release smoke (Validation Architecture).

### Pitfall 11: The worker is still occupied after a backstop 504
**What goes wrong:** Under D-07 the client gets 504 while the worker is stuck in Rust (for example the IN-04 `WouldBlock` retry loop on a stdout pipe nobody drains). The worker stays busy until the stall ends.
**How to avoid:** Accept and document it as residual (JSHD-* v2). The in-memory store never stalls.

### Pitfall 12: OOM is not covered by the timeout
**What goes wrong:** `let a=[]; while(true) a.push(1)` can hit V8's heap limit before 5 s, and a V8 fatal OOM aborts the process.
**How to avoid:** Out of scope (JSHD-01, v2). Document it in the Limits section of docs/js-ops.md. **Never test or smoke it.**

## Code Examples

### Registering every runtime (outer and nested) in `run_fresh`
```rust
// Source: spike (deno_core 0.412.0 API: jsruntime.rs:1420 v8_isolate(); v8 isolate.rs:982 thread_safe_handle())
let mut rt = JsRuntime::try_new(RuntimeOptions {
    extensions: vec![rs3_ext::init(ctx)],
    ..Default::default()
})
.map_err(|e| ExecError::Internal(format!("cannot create JS runtime: {e}")))?;
let _registered = Registered::new(&watch.job, rt.v8_isolate().thread_safe_handle()); // Drop → unregister
let global = rt.execute_script(name.to_owned(), source).map_err(|e| { /* log only if !watch.job.stopped() */ })?;
```

### `ApiError` mapping additions (exhaustive, no wildcard)
```rust
// ExecError (src/js/mod.rs) gains:
//   #[error("script exceeded the {limit_ms} ms execution limit")] Timeout { limit_ms: u64 },
//   #[error("PATCH nesting depth limit ({limit}) exceeded: {}", chain.join(" -> "))] DepthLimit { limit: u32, chain: Vec<String> },
// From<ExecError> for ApiError: Timeout → ApiError::JsTimeout(msg), DepthLimit → ApiError::JsDepthLimit(msg)
// code():        JsTimeout => "js_timeout",              JsDepthLimit => "js_depth_limit"
// status_code(): JsTimeout => StatusCode::GATEWAY_TIMEOUT, JsDepthLimit => StatusCode::UNPROCESSABLE_ENTITY
```
Existing codes, quoted verbatim from src/http/error.rs:59-63 [VERIFIED: read this session]: `"not_found"`, `"malformed_json"`, `"invalid_input"`, `"js_error"`, `"internal"`. The two new codes `js_timeout` and `js_depth_limit` come from CONTEXT D-03/D-08.

### Existing in-repo values the plan builds on (verbatim)
- `ExecError` (src/js/mod.rs:16-28): `Script { message: String }`, `Unavailable`, `Internal(String)`, with `#[derive(Debug, thiserror::Error, PartialEq, Eq)]` [VERIFIED].
- `JsSettings` (src/js/mod.rs:36-55): fields `pub workers: usize`, `pub queue_capacity: usize`, `pub stack_bytes: usize`; defaults `queue_capacity: workers * 16`, `stack_bytes: 16 * 1024 * 1024` [VERIFIED].
- `ExecCtx` (src/js/ops.rs:17-37): `pub store: Store`, `pub depth: u32`, `host_fault: Arc<AtomicBool>`, test hooks `fail_runtime_at_depth: Option<u32>`, `fail_runtime_for_key: Option<&'static str>`, `error_logs: Arc<AtomicU32>` [VERIFIED].
- Extension middleware (src/js/ops.rs:265-269): `"op_panic" => op.disable()`, `"op_print" => op.with_implementation_from(&op_rs3_print())` [VERIFIED].

### Release smoke additions (scripts/ci-smoke.sh; ci.yml unchanged)
```bash
# After the existing PATCH checks; default 5000 ms timeout (no env knob exists by design).
req POST /insert '{"me":"Deno.core.ops.op_rs3_patch(\"me\")","loop":"while(true){}"}'
req PATCH /key/me
[[ "$CODE" == "422" && "$BODY" == '{"error":"js_depth_limit","message":"PATCH nesting depth limit (4) exceeded: me -> me -> me -> me -> me"}' ]] || fail "PATCH /key/me -> ${CODE} ${BODY}"
curl -sS --max-time 30 -o /dev/null -w '%{http_code}' -X PATCH "$BASE/key/loop" > "$TMPD/loop.code" &   # in flight
sleep 0.5; req GET /;            [[ "$CODE" == "200" ]] || fail "GET / during loop -> ${CODE}"
req PATCH /key/m;                [[ "$CODE" == "200" && "$BODY" == '"120"' ]] || fail "PATCH m during loop"
wait; [[ "$(cat "$TMPD/loop.code")" == "504" ]] || fail "PATCH /key/loop -> $(cat "$TMPD/loop.code")"
```
`PATCH /key/m` succeeds during the loop only if the pool has ≥2 workers. The CI runner has 4 vCPU, so `available_parallelism ≥ 2`. Keep a guard: skip that line when `nproc` is 1. Where a temp dir is needed, use a `mktemp -d` under the existing log location, not `/tmp`.

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| HTTP-side `tokio::time::timeout` + `terminate_all` (milestone ARCHITECTURE Pattern 5) | Worker-side watchdog thread, with the HTTP/executor timeout only as a backstop | This research | Survives client disconnect. Termination starts at dequeue |
| "A terminated script surfaces as `Uncaught Error: execution terminated`; map by `group.reason()`" (Pattern 5) | Also true, **but** an op `Err` during termination cancels it (S4). Silent ops + re-fire required | This research (deno_core 0.412 `to_v8_error` fallback) | Without this, `try{patch}catch{}` + loop defeats the timeout |
| `thread_stack_bytes ≈ (max_patch_depth + 1) × 2 MiB` (Pattern 4, MEDIUM) | Measured 12.6 KiB/level debug, 3.7 KiB release. 1 MiB is enough at depth 4 | This research (S9/S10) | The 16 MiB default is generous. A 2 MiB floor is safe |
| `JsErrorBox::range_error("rs3_patch: max nesting depth 4 exceeded")` (Pattern 4) | Flag + chain record; 422 `js_depth_limit` body built from the record | CONTEXT D-02/D-03 | The JS-visible error type is discretion (an `Error` is fine) |

**Deprecated/outdated:** reading termination from `is_execution_terminating()` after `execute_script` returns. deno_core's `exception_to_err` cancels and then resumes termination (error.rs:1662, 1699-1702), and the flag is not authoritative across nested runtimes.

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | `IsolateHandle::terminate_execution` from the watchdog thread is safe on the protected PKU platform on x86. Mitigated by spawning the watchdog from the worker (descendant) | Pitfall 10 | The release smoke on x86 CI crashes (status 139). The D-07 fallback from Phase 4 applies |
| A2 | Declaring tokio `time` (and dev `macros`) leaves Cargo.lock unchanged, because features are not stored in the lock | Standard Stack | `--locked` fails. The plan verifies with `git diff --exit-code Cargo.lock` |
| A3 | Spike timings (aarch64, 4 cores) transfer to the GitHub x86 runner within ~3×. Normal debug jobs stay < 100 ms, so a 300 ms test timeout has margin | Pitfall 9 | Flaky timeout tests in CI. Remedy: raise test timeouts to 500 ms |
| A4 | Stack per level on x86 is in the same range as aarch64 (no deep native frame differences) | Spike S9/S10 | The 2 MiB floor margin shrinks. The 16 MiB default is unaffected |
| A5 | The intended meaning of "depth limit 4" is the D-03 example message (5 keys including the refused one, i.e. at most 4 levels including the top-level PATCH) | Pattern 4 / Open Question 1 | The off-by-one changes the chain length in exact-body tests |

## Open Questions (RESOLVED)

1. **Exact off-by-one of `max_patch_depth`.** — RESOLVED (orchestrator, Recommended): follow the CONTEXT example — the limit counts PATCH levels including the top-level request; refuse when `chain.len() + 1 > max_patch_depth`; the message chain includes the refused key.
   - What we know: D-03's example `a -> b -> a -> b -> a` for limit 4 has 5 keys. Milestone ARCHITECTURE Pattern 4 said "if depth >= max", where depth is the caller's 0-based depth, which would allow 5 levels and show 6 keys.
   - Recommendation: **follow the CONTEXT example.** The limit counts PATCH levels including the top-level request. Refuse when `chain.len() + 1 > max_patch_depth`. The chain in the message includes the refused key: `me -> me -> me -> me -> me` and `a -> b -> a -> b -> a`. Existing nested tests reach at most depth 3 (`failing_at(&store, 3)` with test→a→b→c), so they still pass. Document it in docs/js-ops.md ("at most 4 scripts deep, counting the PATCHed key").
2. **Clamp range for `max_patch_depth` and `timeout_ms`.** — RESOLVED (orchestrator, Recommended): `max_patch_depth.clamp(1, 64)`, `timeout_ms.max(1)`, tested as pure functions.
   - Recommendation: `max_patch_depth.clamp(1, 64)`. 1 means no nested PATCH. 64 was verified on the 2 MiB stack floor in S10, and above ~200 nested runtimes V8's stack guard turns the limit into a 500. Use `timeout_ms.max(1)`, consistent with the existing `.max(1)` clamps. Test both as pure functions.
3. **Backstop grace as a constant or a setting.** — RESOLVED (orchestrator, Recommended): private constant `BACKSTOP_GRACE = 2 s`.
   - Recommendation: a private constant `BACKSTOP_GRACE = 2 s` (discretion per CONTEXT). The single backstop test (a Rust closure sleeping past `timeout + grace` on a dedicated 100 ms executor) then takes ~2.1 s, which is acceptable. A public knob nobody asked for is avoided.
4. **Should every `run()` job be armed, or only `execute()` jobs?** — RESOLVED (orchestrator, Recommended): arm every job.
   - Recommendation: arm every job (the Envelope always carries a `JobState`). A plain Rust closure that overruns is then classified `Timeout`, which is uniform and harmless for the existing tests (all finish in milliseconds; job 0 in `full_queue_makes_callers_wait` waits on a gate the test releases quickly). The alternative `Option<Arc<JobState>>` is also acceptable.
5. **Should the release smoke cover depth and timeout?** — RESOLVED (orchestrator, Recommended): yes, extend `scripts/ci-smoke.sh`; ci.yml unchanged (guard (g) canonical lines).
   - Recommendation: yes, extend `scripts/ci-smoke.sh`. It is the only place the watchdog runs under the protected platform on x86 (A1). It costs ~5 s (default timeout). ci.yml stays untouched (guard g).

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| Rust toolchain | build/test | ✓ | rustc 1.99.1 / cargo 1.99.1 | — |
| Compiled V8 deps in `target/` | fast offline builds | ✓ | target/ 4.8 GB (debug + release) | Cold build ~1 min with network |
| curl, python3, bash | live smokes, ci-smoke.sh, guards | ✓ | (used by existing scripts) | — |
| 4 CPU cores | parallel tests | ✓ | nproc = 4 | — |
| Free disk | builds | ✓ (`/home` 45 GB free); ✗ `/tmp` (2.1 GB, 96%) | — | Keep scratch under `target/tmp/` |
| Docker | — (Phase 6) | ✗ | — | Not needed this phase |

**Missing dependencies with no fallback:** none.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust libtest (in-crate `#[cfg(test)]` modules), `#[actix_web::test]` for HTTP, shared `test_executor()` + dedicated short-timeout executors |
| Config file | none (Cargo.toml `[dev-dependencies]` deno_core unprotected-platform feature; clippy.toml test allowances) |
| Quick run command | `cargo test --locked -- depth timeout watchdog backstop` (substring filters) |
| Full suite command | `cargo test --locked` (baseline 171 tests in 0.42 s; expected under ~10 s after this phase) |
| Phase gate | `bash scripts/ci-guards.sh && cargo fmt --check && cargo clippy --all-targets --locked -- -D warnings && cargo test --locked && cargo build --release --locked && bash scripts/ci-smoke.sh` |

### Phase Requirements → Test Map
| Req / Decision | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| TEST-04 404 | GET/DELETE/PATCH on a missing key → 404 `not_found` | unit (HTTP) | `cargo test --locked -- get_missing_is_404_json delete_missing_is_404_json test_patch_missing_is_404_json` | ✅ src/http/kv.rs, src/http/mod.rs |
| TEST-04 400 | malformed insert → 400 `malformed_json` | unit (HTTP) | `cargo test --locked -- insert_malformed_is_400_json` | ✅ src/http/kv.rs |
| TEST-04 422 insert | non-object / non-string → 422 `invalid_input` | unit (HTTP) | `cargo test --locked -- insert_non_object_is_422_json insert_non_string_value_is_422_and_inserts_nothing` | ✅ src/http/kv.rs |
| TEST-04 422 JS | throw → 422 `js_error` | unit (HTTP) | `cargo test --locked -- test_patch_throw_is_422_js_error` | ✅ src/http/mod.rs |
| ROBU-05 / D-03 (RED) | finite chain k1..k6 → 422 naming `k1 -> … -> k5` | unit (HTTP) | `cargo test --locked -- depth_limit_finite_chain` | ❌ Wave 0 (proposed name) |
| ROBU-05 direct | `me → me` → exact 422 body `…(4) exceeded: me -> me -> me -> me -> me` | unit (HTTP) | `cargo test --locked -- depth_limit_self_patch_is_422` | ❌ |
| ROBU-05 cycle | `a ↔ b` → exact 422 body `a -> b -> a -> b -> a` | unit (HTTP) | `cargo test --locked -- depth_limit_cycle_is_422` | ❌ |
| D-03 caught | `try { patch('b') } catch {} 'ok'` → still 422 | unit (js + HTTP) | `cargo test --locked -- depth_limit_caught_is_still_422` | ❌ |
| D-03 refusal + writes kept | write before the stop stays; insert/get/delete/list/patch after the stop throw the depth error and write nothing | unit (js) | `cargo test --locked -- depth_stop_refuses_later_ops_and_keeps_earlier_writes` | ❌ |
| D-01 config | `max_patch_depth: 2` → chain of 3 keys; clamp(1,64) pure fn | unit | `cargo test --locked -- max_patch_depth` | ❌ |
| D-04 precedence | host fault + depth → 500; timeout + depth (`try{patch('me')}catch{} while(true){}`) → 504; depth + script throw → 422 depth | unit (js) | `cargo test --locked -- precedence` | ❌ |
| ROBU-06 (RED) | bounded `Date.now()+1500` busy loop on a 300 ms executor → `ExecError::Timeout` | unit (js) | `cargo test --locked -- timeout_bounded_busy_loop` | ❌ |
| ROBU-06 top | `while(true){}` → 504 exact body `script exceeded the 300 ms execution limit` | unit (HTTP, dedicated AppState.js) | `cargo test --locked -- timeout_infinite_loop_is_504` | ❌ |
| ROBU-06 nested | patched key loops → 504 | unit (js) | `cargo test --locked -- timeout_nested_loop` | ❌ |
| S4 regression | `try { patch('loop') } catch(e) {} while(true){}` → 504 within timeout + 1 s | unit (js) | `cargo test --locked -- timeout_survives_caught_nested_termination` | ❌ |
| S5 regression | `for(;;){ try { op_rs3_get('missing') } catch(e){} }` → 504 | unit (js) | `cargo test --locked -- timeout_throwing_op_loop` | ❌ |
| S4b policy | `try{patch('loop')}catch{}; op_rs3_insert('{"after":"1"}'); 'done'` → Timeout and `after` not written | unit (js) | `cargo test --locked -- timeout_refuses_writes_after_stop` | ❌ |
| S8 | `({toString(){while(true){}}})` → Timeout | unit (js) | `cargo test --locked -- timeout_during_stringify` | ❌ |
| D-09 next job | workers=1: loop → Timeout, then `1+1` → `"2"` (×3) | unit (executor) | `cargo test --locked -- timed_out_worker_serves_next_job` | ❌ |
| D-09 isolation | workers=2, timeout 1000: loop PATCH in flight while `GET /` and a normal PATCH complete first (`tokio::join!`, compare completion instants) | unit (HTTP) | `cargo test --locked -- timeout_does_not_block_other_requests` | ❌ |
| D-06 no late kill | many near-deadline jobs + canary jobs on workers=1, short timeout: canaries never Timeout | unit (executor) | `cargo test --locked -- late_watchdog_never_hits_next_job` | ❌ |
| D-07 backstop | `run(|| sleep(timeout + 2.3s))` on a 100 ms executor → Timeout at ≥ timeout + 2 s and < sleep | unit (executor) | `cargo test --locked -- backstop` | ❌ |
| D-05/SC-3 | timeout from `JsSettings` in code, `env -i` | suite | `env -i PATH="$PATH" HOME="$HOME" cargo test --locked` | n/a |
| Single-log | one depth stop / one timeout → exactly one error line (`error_logs` counter) | unit (js) | `cargo test --locked -- logged_once` | ❌ (extend existing counter pattern) |
| IN-04 | `op_rs3_print` write/flush retry helper: WouldBlock then Ok via a test `Write` impl | unit | `cargo test --locked -- print_retries_would_block` | ❌ |
| IN-05 | `.txt` payloads via POST /insert + PATCH equal the `.js` results | unit (HTTP) | `cargo test --locked -- txt_payload` | ❌ |
| IN-07 | `effective_stack_bytes(0) == 2 MiB` (pure fn, no tiny-stack thread) | unit | `cargo test --locked -- stack_bytes_are_clamped` | ❌ |
| D-14 | guards, fmt, clippy, tests, release, smoke | gate | phase gate command above | ✅ scripts |
| Live smoke | release binary, ports 18530–18549: depth 422 exact body, loop 504 after ~5 s with GET / + PATCH m answered meanwhile | smoke | `RS3_SMOKE_PORT=18531 bash scripts/ci-smoke.sh` (after extending) | ✅ (extend) |

### Sampling Rate
- **Per task commit:** `cargo test --locked -- <filters of the task>` plus `cargo clippy --all-targets --locked -- -D warnings`.
- **Per wave merge:** `cargo test --locked` and `cargo test --locked -- --test-threads=1`.
- **Phase gate:** the full gate command, plus `for i in 1 2 3 4 5; do cargo test --locked -q || break; done` (5× repeat), plus `env -i` run, plus `RS3_SMOKE_PORT=1853x bash scripts/ci-smoke.sh` (pick a free port in 18530–18549).

### Wave 0 Gaps
- [ ] Compiling stubs: `JsSettings.{timeout_ms, max_patch_depth}` (default 5000 / 4), `ExecError::{Timeout, DepthLimit}`, `ApiError::{JsTimeout, JsDepthLimit}` with codes/statuses, so RED tests fail on assertions.
- [ ] A test helper for a dedicated executor with settings overrides (`..JsSettings::default()`), calling `init_platform()` first.
- [ ] A test helper building an `AppState` with a dedicated executor (pattern: `AppState { js: ..., ..test_state() }`, as in `test_patch_executor_down_is_500_internal`).
- [ ] Optional dev-dependency `tokio` `macros` for `join!` (verify that Cargo.lock is unchanged).

## Security Domain

### Applicable ASVS Categories

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | no | — (no auth in rs3) |
| V3 Session Management | no | — |
| V4 Access Control | no | — |
| V5 Input Validation | yes | Stored scripts are untrusted input. Depth and time are bounded per job. The key chain is JSON-escaped by serde in the body and Debug-escaped (`{:?}`) in logs |
| V6 Cryptography | no | — |
| V7 Error Handling & Logging | yes | Fixed 504 text (limit only), and a 422 depth message carrying only the user's own keys. Single-log rule per job. No host details in bodies |
| V11/V12 Resources (business logic / DoS) | yes | Watchdog + silent ops + re-fire; depth limit; bounded pool and queue (Phase 3); backstop |

### Known Threat Patterns for rs3 JS execution

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| `while(true){}` pins a worker | DoS | Watchdog `terminate_execution` at `timeout_ms` (S1, S12) |
| Catch-and-continue around a terminated nested PATCH | DoS | Silent ops after timeout; re-fire (S4, S6) |
| Tight loop over a throwing op to dodge termination | DoS | Silent rs3 ops + 50 ms re-fire (S5). Residual: a loop over a deno_core built-in op that throws gets a fresh cancel chance on each fire. Re-fire makes eventual termination near-certain (~35% loss per fire observed) |
| Self-patching / cyclic PATCH | DoS (memory ~1 MB per nested isolate, stack) | Depth limit 4 before runtime creation (S9) |
| Client disconnect leaves a runaway script | DoS | The watchdog is worker-side and does not depend on the request future |
| Many concurrent loops occupy all workers | DoS | Residual: each costs ≤ timeout_ms of one worker; queue backpressure is v2 (JSHD-03) |
| Heap exhaustion (`a.push` loop) aborts the process | DoS | Residual, out of scope (JSHD-01). Documented. Never tested |
| Forged log lines via keys in the depth chain | Tampering/Repudiation | Debug-format the chain in logs |
| Late watchdog terminates another tenant's job | Tampering | Per-job `JobState` + CAS (S11) |

## Sources

### Primary (HIGH confidence)
- Spike crate (scratch, deleted) built offline with the project `Cargo.lock`: deno_core 0.412.0, deno_error 0.7, tokio 1.54.0, debug and release, aarch64, 4 cores. Scenarios cost, A–K, P, S, T, Z, H as cited in the Spike Results table.
- `~/.cargo/registry/src/*/deno_core-0.412.0`: `runtime/jsruntime.rs:1420` (`v8_isolate`), `:2018-2029` (`execute_script`); `runtime/jsrealm.rs:473-506`; `error.rs:267-273, 395-447` (`to_v8_error` + reset fallback), `:1650-1703` (`exception_to_err`, "execution terminated", cancel/resume); `ops_builtin.rs:233-261` (`op_print` retry loop); `runtime/tests/misc.rs:340-380` (upstream terminate tests).
- `~/.cargo/registry/src/*/v8-150.4.0`: `src/isolate.rs:2175-2300` (`IsolateHandle`, `terminate_execution`, disposal mutex), `:982` (`thread_safe_handle`); `src/scope.rs:1268-1281` (`TryCatch::reset` cancels a scheduled exception); `src/platform.rs:300-330` (PKU descendant-thread rule).
- Project files read this session: src/js/{mod,ops,runtime,executor}.rs, src/http/{mod,error}.rs, src/state.rs, src/main.rs, src/lib.rs, Cargo.toml, docs/js-ops.md, requests/*.{js,txt}, scripts/ci-guards.sh (header), scripts/ci-smoke.sh (key lines), .github/workflows/ci.yml, 03-REVIEW-DISPOSITION.md, 03-REVIEW.md (IN-04..07), 03-RESEARCH.md (S1c/S1d/S8), milestone ARCHITECTURE.md Patterns 4/5, PITFALLS.md.

### Secondary (MEDIUM confidence)
- None needed. No web sources were used. All questions were resolved from locked source and runtime experiments.

### Tertiary (LOW confidence)
- A1/A3/A4 (x86 transfer of the PKU safety and timing/stack numbers) need confirmation via the CI release smoke.

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH. No new crates. APIs compiled in the spike against the locked versions.
- Architecture: HIGH. Watchdog, CAS, registry, silent ops and re-fire were all exercised, including adversarial cases and 900 race trials.
- Pitfalls: HIGH for S4/S5 (reproduced, root cause located in source). MEDIUM for the x86 transfer (A1/A3/A4).

**Research date:** 2026-10-20
**Valid until:** 2026-11-20 for the pinned deno_core 0.412 / v8 150.4. Re-verify S4 (the `to_v8_error` reset fallback) on any deno_core bump.
