# Phase 5: JS Execution Limits - Pattern Map

**Mapped:** 2026-10-20
**Files analyzed:** 11 (1 new, 10 modified)
**Analogs found:** 11 / 11 (every analog is a git-tracked file in this repo)

## File Classification

| New/Modified File | Role | Data Flow | Closest Analog | Match Quality |
|-------------------|------|-----------|----------------|---------------|
| `src/js/job.rs` (NEW: `JobState`, `DepthStop`, `Registered` guard, watchdog `Cmd`/loop) | service (per-job shared state + timer thread) | event-driven | `src/js/ops.rs` `ExecCtx.host_fault` (lines 15-88) + `src/js/executor.rs` worker spawn (lines 43-61) | role-match (generalises an existing pattern) |
| `src/js/mod.rs` | model / config | transform | itself: `ExecError` (14-28), `JsSettings` (30-56), test helpers (58-88) | exact |
| `src/js/ops.rs` | service (ops) | request-response (reentrant) | itself: `live_ctx` (136-147), `op_rs3_patch` (188-239), `op_rs3_print` (241-256) | exact |
| `src/js/runtime.rs` | service (runner) | request-response | itself: `run_script` (30-41), `run_fresh` (50-91) | exact |
| `src/js/executor.rs` | service (thread pool) | event-driven / batch | itself: `start` (33-84), `run` (89-102), `worker_loop` (126-143) | exact |
| `src/http/error.rs` | utility (error mapping) | transform | itself: `ApiError` + exhaustive `code()`/`status_code()` (14-77) | exact |
| `src/http/mod.rs` (tests: depth/timeout/isolation/IN-05 parity) | test | request-response | `test_patch_executor_down_is_500_internal` (336-352), `patch_seeded` (203-214), fixture tests (406-475) | exact |
| `src/state.rs` (optional test helper for dedicated-executor AppState) | config/test helper | — | `test_state()` (33-47) | exact |
| `Cargo.toml` | config | — | line 22-25 (`tokio` features, `[dev-dependencies]`) | exact |
| `docs/js-ops.md` | docs | — | "Errors" (35-78) + "Sandbox and known limits" (153-184) | exact |
| `scripts/ci-smoke.sh` | test (smoke) | request-response | itself: `req` helper (98-107) + assertions (109-125) | exact |

`src/main.rs` stays unchanged (it already calls `JsSettings::default()` at line 17; new fields get defaults there).

## Pattern Assignments

### `src/js/job.rs` (NEW; service, event-driven)

**Analog A - shared per-job flag:** `src/js/ops.rs` lines 22-25, 45, 61, 71-79
```rust
/// Set when a nested runtime hit a host fault. Shared by every level of
/// one job, so the top level answers a host fault even if a script caught
/// the error `op_rs3_patch` threw.
host_fault: Arc<AtomicBool>,
...
host_fault: Arc::new(AtomicBool::new(false)),     // ExecCtx::new
host_fault: Arc::clone(&self.host_fault),          // ExecCtx::nested
...
pub(crate) fn note_host_fault(&self) { self.host_fault.store(true, Ordering::SeqCst); }
pub(crate) fn host_fault(&self) -> bool { self.host_fault.load(Ordering::SeqCst) }
```
Move `note_host_fault`/`host_fault` onto `JobState` unchanged (SeqCst). `ExecCtx` swaps `host_fault: Arc<AtomicBool>` for `job: Arc<JobState>`. Add `phase: AtomicU8` CAS, `depth_stop: OnceLock<DepthStop>`, `isolates: Mutex<Vec<(u64, v8::IsolateHandle)>>` per RESEARCH Pattern 1 (lines 227-278). Lock with `.lock().unwrap_or_else(PoisonError::into_inner)` (clippy `unwrap_used`/`expect_used` deny).

**Analog B - thread spawn without panicking:** `src/js/executor.rs` lines 43-61 (`std::thread::Builder::new().name(..).stack_size(..).spawn(..).map_err(..)?`). The watchdog thread is spawned *inside* the worker closure (Pitfall 10), named e.g. `rs3-js-{i}-wd`, and its spawn failure goes back through the existing `ready.send(Err(e))` path (line 58), so `start()` reports it like a runtime build error.

**Module doc header convention** (copy from `src/js/executor.rs` lines 1-2):
```rust
//! A bounded pool of dedicated JS threads, so actix workers never run V8.
//! This module is free of actix-web.
```
Register in `src/js/mod.rs` alongside `pub mod executor; pub mod ops; pub mod runtime;` (lines 5-7). Use `deno_core::v8::IsolateHandle`, never a direct `v8` dep.

---

### `src/js/mod.rs` (model/config)

**ExecError pattern** (lines 14-28) - add variants in the same doc-commented thiserror style:
```rust
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ExecError {
    /// ... HTTP maps this to 422 js_error.
    #[error("{message}")]
    Script { message: String },
    #[error("JS executor unavailable")]
    Unavailable,
    #[error("JS host failure: {0}")]
    Internal(String),
}
```
New: `#[error("script exceeded the {limit_ms} ms execution limit")] Timeout { limit_ms: u64 }` and `#[error("PATCH nesting depth limit ({limit}) exceeded: {}", chain.join(" -> "))] DepthLimit { limit: u32, chain: Vec<String> }`. `PartialEq, Eq` must still derive.

**JsSettings pattern** (lines 30-56):
```rust
/// Built in code and never read from the environment: the Phase 2 config
/// allowlist admits only server.host and server.port. ...
#[derive(Clone, Debug)]
pub struct JsSettings { pub workers: usize, pub queue_capacity: usize, pub stack_bytes: usize }

impl Default for JsSettings {
    fn default() -> Self {
        let workers = std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get);
        Self { workers, queue_capacity: workers * 16, stack_bytes: 16 * 1024 * 1024 }
    }
}
```
Add `pub timeout_ms: u64` (5000) and `pub max_patch_depth: u32` (4); update the `Default` doc line.

**Test helper pattern** (lines 61-71, 73-88): `test_executor()` uses `LazyLock` + `init_platform()` + `.expect(..)`. A new `#[cfg(test)] pub(crate) fn dedicated_executor(overrides: JsSettings)` (or `with_timeout(ms)`) must call `init_platform()` first and use `..JsSettings::default()` (Pitfall 7/9). `run_js` (85-88) is the template for a `run_js_on(exec, store, src)` variant.

---

### `src/js/ops.rs` (service, reentrant request-response)

**live_ctx gate** (lines 136-147) - becomes three-way per RESEARCH Pattern 3:
```rust
fn live_ctx(state: &OpState) -> Result<&ExecCtx, JsErrorBox> {
    let ctx = state.borrow::<ExecCtx>();
    if ctx.host_fault() { Err(internal_error()) } else { Ok(ctx) }
}
```
New return `Result<Option<&ExecCtx>, JsErrorBox>`: timed_out -> `Ok(None)` (checked first, never `Err`), host_fault -> `Err(internal_error())`, depth_stop -> `Err(depth_error(stop))`. Each op (`op_rs3_list` 152-156, `op_rs3_get` 160-164, `op_rs3_insert` 169-178, `op_rs3_delete` 182-186) becomes `let Some(ctx) = live_ctx(state)? else { return Ok(String::new()) };`.

**Error constructor pattern** (lines 131-134) - copy for `depth_error`:
```rust
/// The error thrown into a script for a host fault. It carries no host details.
fn internal_error() -> JsErrorBox { JsErrorBox::generic("internal error") }
```
Only `generic`/`type_error` classes (lines 120-129 explain why custom classes break).

**op_rs3_patch** (lines 202-239) - insert depth check before `ctx.store.get(&key)` (line 208), post-run `timed_out` check before the `map_err`, and keep the existing exhaustive arm structure. The single-log guard at lines 216-235 is the template for "log once where first set":
```rust
other => {
    if !ctx.host_fault() {
        let detail = match other { super::ExecError::Internal(m) => m, e => e.to_string() };
        ctx.log_error(format_args!("error: nested JS host failure in {key:?} at depth {}: {detail:?}", ctx.depth + 1));
        ctx.note_host_fault();
    }
    internal_error()
}
```
The `other =>` arm must become explicit: `Internal(_) | Unavailable` -> host-fault path; `Timeout{..} | DepthLimit{..}` listed separately and NOT recorded as host faults (RESEARCH Pattern 4). Remove the "Phase 5 DoS item" comment (lines 198-201).

**ExecCtx::new / nested** (lines 39-69): `new(store)` gains the root key (`new(store, root: &str, settings)`), `nested(&self)` becomes `nested(&self, key: &str)` pushing onto `chain: Vec<String>`. Keep the `#[cfg(test)]` hook fields copied through as now. Update call sites: `executor.rs` line 112 and the test helpers `failing_at`/`failing_for` (ops.rs 363-373) and `exec_ctx_nested_increments_depth_and_shares_store` (535).

**log_error** (lines 81-87) is the single sink; it bumps `error_logs` in tests. Every new error line (depth stop, timeout, backstop where reachable from ctx) goes through it.

**op_rs3_print** (lines 241-256) - IN-04: keep the exact signature `(#[string] msg: &str, is_err: bool) -> Result<(), std::io::Error>` (required by `with_implementation_from`, line 267). Factor a `fn write_all_retrying(w: &mut impl Write, bytes: &[u8]) -> io::Result<()>` (+flush) with `Interrupted` -> continue, `WouldBlock` -> `std::thread::yield_now()` + continue, so it can be unit-tested with a test `Write` impl.

**Test patterns to copy** (ops.rs tests):
- `run_ctx` (357-361) runs a crafted `ExecCtx` on the shared executor; timeout tests need a variant on a dedicated short-timeout executor via `exec.run(..)` or `exec.execute(..)`.
- `run_counting` (379-386) + `one_nested_host_fault_is_logged_once_at_any_depth` (492-511): copy for `depth_stop_logged_once` / `timeout_logged_once`.
- `caught_nested_runtime_failure_is_still_internal_and_refuses_later_writes` (401-419): exact template for `depth_stop_refuses_later_ops_and_keeps_earlier_writes` and the caught-but-422 test.
- `every_op_throws_internal_error_once_the_job_faulted` (421-443): template for "every op throws the depth error after the stop" (the `g(() => ...)` join trick).

---

### `src/js/runtime.rs` (service, request-response)

**Registration point** (lines 62-66):
```rust
let mut rt = JsRuntime::try_new(RuntimeOptions {
    extensions: vec![rs3_ext::init(ctx)],
    ..Default::default()
})
.map_err(|e| ExecError::Internal(format!("cannot create JS runtime: {e}")))?;
```
Immediately after: `let _registered = Registered::new(&watch.job, rt.v8_isolate().thread_safe_handle());` (RESEARCH Code Examples, line 506).

**Log suppression** (lines 70 and 81): replace `if !watch.host_fault()` with `if !watch.job.stopped()`.

**run_script** (30-41) keeps its `faulted_before` host-fault attribution; it must NOT classify Timeout/Depth (classification is in the executor, RESEARCH Pattern 5 / anti-patterns). Update its doc comment (lines 20-29).

**Test injection hooks** (56-61) show how `#[cfg(test)]` hooks short-circuit inside `run_fresh` - reuse this style if a test-only hook is needed (e.g. small depth is via settings instead, preferred).

---

### `src/js/executor.rs` (service, event-driven pool)

**Clamping pattern** (lines 34-35, IN-07 target at line 45):
```rust
let workers = settings.workers.max(1);
let capacity = settings.queue_capacity.max(1);
...
.stack_size(settings.stack_bytes)          // line 45 -> effective_stack_bytes(settings)
```
Add pure fns `effective_stack_bytes` (`.max(2 MiB)`), `max_patch_depth.clamp(1, 64)`, `timeout_ms.max(1)`; document in the `start` doc comment (lines 28-32) which already says "Zero workers or zero queue capacity are clamped to 1".

**Envelope** (lines 15-18) gains `job: Arc<JobState>` and `started: oneshot::Sender<()>`. Note the test `full_queue_makes_callers_wait` builds `Envelope { run, reply }` literally (lines 261-271) - must be updated.

**run()** (lines 89-102) - backstop template:
```rust
let (reply, reply_rx) = oneshot::channel();
self.tx.send(Envelope { run: Box::new(job), reply }).await.map_err(|_| ExecError::Unavailable)?;
reply_rx.await.map_err(|_| ExecError::Unavailable)?
```
Becomes: await `started_rx`, then `tokio::time::timeout(timeout + BACKSTOP_GRACE, reply_rx)` (RESEARCH Pattern 6). Requires `tokio` feature `time` in Cargo.toml.

**worker_loop** (lines 126-143) - insert Arm/finish/Disarm/classify around the existing catch_unwind:
```rust
let result = {
    let _enter = rt.enter();
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(env.run))
        .unwrap_or_else(|_| Err(ExecError::Internal("js worker panicked".into())))
};
let _ = env.reply.send(result);
```
IN-06: rewrite the doc comment at lines 126-128 ("a panicking job cannot kill the worker") per RESEARCH Pattern 7. `classify()` (RESEARCH Pattern 5) lives here, once.

**closed()** (lines 118-123) - closed-executor test constructor; keep working with the new Envelope (no change needed beyond fields).

**Test patterns:**
- `dedicated()` (160-167) builds a struct literal - change to `JsSettings { workers, queue_capacity, stack_bytes: MIB16, ..JsSettings::default() }`; add a `short_timeout(workers, ms)` sibling.
- `worker_survives_panicking_jobs` (178-196): template for `timed_out_worker_serves_next_job` (loop Timeout x3, then `Ok`).
- `zero_settings_are_clamped` (324-332) calls `init_platform()` first - required for any dedicated executor.
- `default_settings_follow_d06` (346-352): extend with `timeout_ms == 5000`, `max_patch_depth == 4`.
- Backstop test: `exec.run(|| { std::thread::sleep(..); Ok(..) })` on a 100 ms executor, like `full_queue_makes_callers_wait` uses plain closures.

---

### `src/http/error.rs` (utility, transform)

**Variants + exhaustive mapping** (lines 14-25, 27-46, 55-66, 68-77):
```rust
pub enum ApiError {
    #[error(transparent)] Store(#[from] StoreError),
    #[error("{0}")] JsError(String),
    #[error("internal error")] Internal,
}
impl From<ExecError> for ApiError {
    fn from(e: ExecError) -> Self {
        match e {
            ExecError::Script { message } => ApiError::JsError(message),
            ExecError::Unavailable | ExecError::Internal(_) => { ...; eprintln!("error: JS host failure: {detail}"); ApiError::Internal }
        }
    }
}
pub fn code(&self) -> &'static str { match self { ..., ApiError::JsError(_) => "js_error", ApiError::Internal => "internal" } }
fn status_code(&self) -> StatusCode { match self { ..., ApiError::JsError(_) => StatusCode::UNPROCESSABLE_ENTITY, ... } }
```
Add `#[error("{0}")] JsTimeout(String)` -> `"js_timeout"` / `GATEWAY_TIMEOUT`, `#[error("{0}")] JsDepthLimit(String)` -> `"js_depth_limit"` / `UNPROCESSABLE_ENTITY`; `From` maps `Timeout`/`DepthLimit` via `e.to_string()`. No wildcard arms. Update the module doc header (lines 1-7) listing codes. Body serialization via existing `ErrorBody` (serde escapes keys in the chain).

---

### `src/http/mod.rs` (tests)

**Dedicated-executor AppState** (lines 336-352):
```rust
let state = AppState { js: crate::js::executor::JsExecutor::closed(), ..test_state() };
state.store.insert("k".into(), "1".into());
let app = atest::init_service(App::new().configure(configure(state.clone()))).await;
let resp = atest::call_service(&app, atest::TestRequest::patch().uri("/key/k").to_request()).await;
let body = assert_json_error(resp, StatusCode::INTERNAL_SERVER_ERROR, "internal").await;
assert_eq!(body, serde_json::json!({"error": "internal", "message": "internal error"}));
```
Copy with `js: short_timeout_executor(..)` for `timeout_infinite_loop_is_504` (exact body `script exceeded the 300 ms execution limit`). Depth tests can use `patch_seeded` (203-214) on the default executor + `assert_json_error(.., UNPROCESSABLE_ENTITY, "js_depth_limit")` (helper at 65-82) with exact `serde_json::json!` bodies, as in `test_patch_throw_is_422_js_error` (216-224).

**IN-05 parity**: copy fixture constants/tests (406-475: `include_str!("../../requests/fac.js")`, `seeded`, `send`, `patch_req`, `TREE_BODY`) using `requests/*.txt` loaded through `POST /insert` then PATCH; assert the same bytes.

**D-09 isolation**: run the loop PATCH and `GET /` + a normal PATCH concurrently (`tokio::join!`, dev-dep `macros`), workers=2.

`patch_key` (lines 46-54) itself needs no change - backstop lives in `JsExecutor::run`.

---

### `src/state.rs` (optional test helper)

`test_state()` (lines 33-47) - if a reusable `test_state_with(js: JsExecutor)` is wanted, wrap `AppState::new(Settings::from_map(map)..., js)`; otherwise struct-update `AppState { js, ..test_state() }` inline (preferred, matches existing test).

---

### `Cargo.toml`

Line 22-23: `tokio = { version = "1.54.0", features = ["rt", "sync"] }` -> add `"time"`. Optional `[dev-dependencies]` (line 25) `tokio = { version = "1.54.0", features = ["macros"] }`. Verify `git diff --exit-code Cargo.lock`.

---

### `docs/js-ops.md`

Rewrite lines 168-184 (the two "until Phase 5" bullets + the Phase 5 bullet) in "Sandbox and known limits". Add a depth-limit and timeout bullet to "Errors" in the same style as "Nested host faults" (lines 69-78: hard stop even when caught, ops refuse afterwards, writes before the stop stay). Mention heap OOM as residual (Pitfall 12) and the backstop worker-occupancy residual (Pitfall 11).

---

### `scripts/ci-smoke.sh`

**req helper + assertion style** (lines 98-120):
```bash
req PATCH /key/boom
[[ "$CODE" == "422" && "$BODY" == '{"error":"js_error","message":"Uncaught Error: boom"}' ]] \
  || fail "PATCH /key/boom -> ${CODE} ${BODY}"
echo "ci-smoke: ok   PATCH /key/boom -> 422 js_error"
```
Insert depth + loop checks between line 120 and line 122 (RESEARCH lines 529-537). Update header comment (lines 7-12). Use `mktemp` like `LOG` (line 25) and remove it in `cleanup` (line 37); not `/tmp` paths by name. Guard the concurrent `PATCH /key/m` on `nproc` >= 2. Do not touch `.github/workflows/ci.yml` (guard g).

## Shared Patterns

### Shared per-job flags decide classification
**Source:** `src/js/ops.rs` 22-25, 71-79; `src/js/runtime.rs` 30-41
**Apply to:** job.rs, ops.rs, runtime.rs, executor.rs. Never match on message strings.

### Single-log rule
**Source:** `ExecCtx::log_error` (`src/js/ops.rs` 81-87) + guard at 222-235; runtime.rs 70/81
**Apply to:** depth stop (log where first set), timeout (log once in executor), backstop (log once). Debug-format (`{:?}`) script-controlled text. Extend `error_logs` counter tests.

### No panics / no unwrap outside tests
**Source:** executor.rs 43-61 (`map_err` on spawn), 140-141 (`let _ = ready.send(..)`), 141 (`let _ = env.reply.send(result)`)
**Apply to:** watchdog sends, mutex locks (`unwrap_or_else(PoisonError::into_inner)`). No `#[allow]`, `#[expect]`, `#[ignore]` (scripts/ci-guards.sh).

### Exhaustive mapping
**Source:** `src/http/error.rs` 27-77; `op_rs3_patch` match (ops.rs 209-238)
**Apply to:** every `match` on `ExecError` gains explicit `Timeout`/`DepthLimit` arms.

### Test executors
**Source:** `src/js/mod.rs` 61-88 (`test_executor`, `block_on`, `run_js`), executor.rs 160-167 / 324-332
**Apply to:** all new timeout tests: dedicated executors, `init_platform()` first, `..JsSettings::default()`, timeouts >= 300 ms when follow-ups must succeed; shared executor keeps 5000 ms.

## No Analog Found

| File | Role | Data Flow | Reason |
|------|------|-----------|--------|
| watchdog loop inside `src/js/job.rs` | timer thread | event-driven | No `recv_timeout` state machine exists yet; use RESEARCH Pattern 2 (lines 286-312) verbatim. Thread-spawn shape still from executor.rs 43-61. |
| `IsolateHandle` registry / `Registered` Drop guard | RAII | — | No Drop guards in the codebase; use RESEARCH Pattern 1 (lines 249-280). |

## Metadata

**Analog search scope:** `src/js/`, `src/http/`, `src/state.rs`, `src/main.rs`, `Cargo.toml`, `scripts/ci-smoke.sh`, `docs/js-ops.md`
**Files scanned:** 11
**Pattern extraction date:** 2026-10-20
