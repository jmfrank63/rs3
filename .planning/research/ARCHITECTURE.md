# Architecture Research

**Domain:** Rust HTTP key/value store with an embedded V8 JavaScript runtime (actix-web 4 + deno_core), brownfield modernization of rs3
**Researched:** 2026-10-20
**Confidence:** MEDIUM-HIGH overall. API facts were checked against the published source of the exact crate versions (deno_core 0.412.0, deno_ops 0.288.0, deno_error 0.7.1, rusty_v8 `main`) and the actix-web 4.17 docs, so those are HIGH. The threading and limit design is reasoned from those sources and is MEDIUM. Performance numbers were not benchmarked and are LOW.

> **Version context (checked on crates.io 2026-10-20):** deno_core 0.412.0 (edition 2024; it uses the `deno_v8` 0.4 facade over `v8` ^150.4), actix-web 4.17.0, thiserror 2.0.21, config 0.15.27, dotenvy 0.15.7, dashmap 6.2.1, papaya 0.2.5, async-channel 2.5.0, tokio 1.54.0.
> deno_core pins `deno_error = "=0.7.1"` and does **not** re-export it. rs3 must declare `deno_error = "0.7"` itself. Writing `"0.7.3"` would make dependency resolution fail.

---

## Standard Architecture

### System Overview

```
                 ┌──────────────────────────── main.rs (thin binary) ────────────────────────────┐
                 │ 1. Settings::load()  2. js::init_platform() on MAIN thread                     │
                 │ 3. AppState::new(settings)  4. HttpServer::new(|| App::configure(http::configure(state.clone()))) │
                 └───────────────────────────────────────────┬───────────────────────────────────┘
                                                             │ (everything below lives in lib.rs: `rs3::*`)
┌────────────────────────────────────────────────────────────┴────────────────────────────────────────────┐
│ TRANSPORT / ADAPTER LAYER                                                                                │
│  ┌───────────────────────────────┐   (future)  ┌──────────────────┐   (future)  ┌───────────────────┐    │
│  │ http/  (actix-web handlers)   │             │ lsp/ (VS Code)   │             │ git_http/ (smart  │    │
│  │ routes, ApiError:ResponseError│             │ tower-lsp/stdio  │             │ HTTP git serving) │    │
│  └──────────────┬────────────────┘             └────────┬─────────┘             └─────────┬─────────┘    │
├─────────────────┼───────────────────────────────────────┼─────────────────────────────────┼──────────────┤
│ APPLICATION STATE   AppState { store: Store, js: JsExecutor, settings: Arc<Settings> }  (Clone, all Arc) │
├─────────────────┼───────────────────────────────────────┼─────────────────────────────────┼──────────────┤
│ DOMAIN / ENGINE LAYER                                                                                    │
│  ┌──────────────▼──────────────────────────┐      ┌──────────────────────────────────────────────────┐  │
│  │ js/  JsExecutor (Send+Sync handle)      │      │ store/  Store = Arc<dyn StoreBackend> (Clone)    │  │
│  │  ─ bounded MPMC job queue ─────────────►│      │  get / insert / insert_many / remove / list      │  │
│  │  N dedicated OS threads ("rs3-js-N")    │─────►│  StoreError (thiserror, no HTTP/JS deps)         │  │
│  │   each: tokio current_thread rt         │ ops  │  legacy.rs: shared JSON codec for /list,/insert  │  │
│  │   per job: fresh JsRuntime + rs3_ext    │      │  memory.rs: in-memory backend                    │  │
│  │   ops (#[op2]) read ExecCtx from OpState│      │  (future) git.rs: git-backed backend             │  │
│  │  TerminationGroup: timeout/cancel/heap  │      └──────────────────────────────────────────────────┘  │
│  └─────────────────────────────────────────┘                                                             │
├──────────────────────────────────────────────────────────────────────────────────────────────────────────┤
│ CROSS-CUTTING:  config.rs (Settings: server + js limits)   ·   per-layer error enums (thiserror)         │
└──────────────────────────────────────────────────────────────────────────────────────────────────────────┘
```

**Dependency direction (strict, enforced by module visibility):**
`main → lib{ http → state → (js → store), config }`. `store` depends on nothing in the crate. `js` depends only on `store` and `config`. Nothing depends on `http`. Future adapters (LSP, git-over-HTTP) sit beside `http` and reuse `AppState` unchanged.

### Component Responsibilities

| Component | Owns | Must NOT | Implementation |
|-----------|------|----------|----------------|
| `config` | `Settings { server: ServerSettings, js: JsSettings }`, loading from `.env` and env vars, defaults | read env per request; know about actix/V8 | `config` 0.15 + `dotenvy`. Load once in `main`. `Settings::for_tests()` for tests |
| `store` | Key/value semantics, the backend abstraction, `StoreError`, the legacy JSON codec shared by HTTP and JS | depend on actix, deno_core or deno_error; hold a map guard across a call | `Store(Arc<dyn StoreBackend>)` newtype with an in-memory backend (dashmap or papaya, STACK.md decides). Returns owned values only |
| `js` | V8 platform init, worker pool, per-job `JsRuntime`, ops + `extension!`, limits (timeout, heap, nesting depth), `ExecError` | depend on actix; let a `JsRuntime` or `v8::Global` leave a worker thread | deno_core 0.412 `#[op2]` and `extension!`, `async-channel` MPMC queue, `tokio::sync::oneshot` replies |
| `state` | `AppState` aggregate (`Clone`, all fields Arc-backed) | contain logic | Plain struct shared by every transport |
| `http` | Routes, request parsing, legacy response shapes, `ApiError` → status codes | contain store or JS logic; create `JsRuntime` | actix-web 4.17; `pub fn configure(state) -> impl FnOnce(&mut ServiceConfig)` |
| `main.rs` | Process wiring only | contain anything a test needs | About 25 lines |

---

## Recommended Project Structure

```
src/
├── main.rs              # thin: Settings::load → js::init_platform → AppState::new → HttpServer
├── lib.rs               # pub mod config, store, js, state, http;  pub use state::AppState;
├── config.rs            # Settings, ServerSettings, JsSettings (+ defaults, for_tests())
├── state.rs             # AppState { store, js, settings }
├── store/
│   ├── mod.rs           # Store (Arc<dyn StoreBackend>), trait StoreBackend, StoreError
│   ├── memory.rs        # MemoryBackend (concurrent map)
│   └── legacy.rs        # parse_insert_body(), format_list(), format_insert_echo() — rs3 v0 wire format
├── js/
│   ├── mod.rs           # pub JsExecutor, ScriptRequest, ExecError, init_platform()
│   ├── executor.rs      # worker threads, bounded queue, timeout + cancel-on-drop guard
│   ├── runtime.rs       # build_runtime(ctx), run_script(ctx, name, src) -> Result<String, ExecError>
│   ├── ops.rs           # #[op2] op_rs3_{list,get,insert,delete,patch}; extension!(rs3_ext); ExecCtx
│   └── limits.rs        # Limits, TerminationGroup (IsolateHandle registry + cancelled flag)
└── http/
    ├── mod.rs           # pub fn configure(state) ; route table
    ├── handlers.rs      # index, ip, insert, list, get, delete, patch
    └── error.rs         # ApiError + impl ResponseError + From<StoreError> + From<ExecError>
tests/
├── common/mod.rs        # test_state() -> AppState (fresh Store + shared process-wide JsExecutor)
├── http_api.rs          # route tests, now isolated per test
└── js_ops.rs            # op round-trips, nested patch, timeout, depth limit, heap limit
```

### Structure Rationale

- **`lib.rs` + thin `main.rs`:** Integration tests in `tests/` can only reach a library target. The split also lets a later `src/bin/rs3-lsp.rs` reuse everything. It is still **one package and no workspace**, so it fits the "single binary crate" constraint in spirit. It does add a lib target, so the roadmap should record this as a decision.
- **Rename `bindings/` → `js/` and `services/` → `http/`:** The old names hide which layer the code belongs to. `bindings` mixed store logic with V8 code. That is the coupling this milestone removes.
- **`store/legacy.rs` is in the store layer, not `http`:** `GET /list` and `op_rs3_list` must produce **byte-identical** output (`test_patch_body_is_listing` asserts this). One codec function used by both is the only reliable way to guarantee it. It also fixes the unescaped hand-built JSON by building it through `serde_json`.
- **Errors live in each layer:** `StoreError` in `store`, `ExecError` in `js`, `ApiError` in `http`. A single crate-wide `Error` enum would force `store` to know about HTTP status codes and V8.

---

## Architectural Patterns

### Pattern 1: Cheap-clone `Store` handle over a backend trait (injected, not global)

**What:** `Store` is a newtype over `Arc<dyn StoreBackend>`. Its methods are **synchronous and fallible** and take and return **owned** values. One instance is created in `main` (or per test) and passed to `AppState` (→ `web::Data`) and into each JS job (→ `OpState`).
**When:** Now. It replaces `lazy_static! MAP`. The trait seam is where the git backend plugs in later.
**Why sync:** deno ops that run inside `execute_script` are synchronous (async ops would need the event loop to be driven). Git libraries (gix, git2) are blocking anyway. An HTTP handler calls the in-memory store directly. When a slow backend arrives, handlers wrap calls in `web::block`.
**Why `dyn` rather than generics:** A generic `AppState<S>` would spread through every handler signature and need `parameters = [S: StoreBackend]` on `extension!`. The vtable cost is irrelevant at this scale.
**Trade-offs:** `Option<String>`/`String` values only. Byte or metadata values (git blobs) will need a later trait revision. That is acceptable, because the newtype hides the trait from callers.

```rust
// src/store/mod.rs
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("key not found: {0}")]
    NotFound(String),
    #[error("invalid input: {0}")]
    InvalidInput(String),          // bad JSON, non-object, non-string value
    #[error("backend failure: {0}")]
    Backend(String),               // future: git/io errors
}

pub trait StoreBackend: Send + Sync + 'static {
    fn get(&self, key: &str) -> Result<Option<String>, StoreError>;
    fn insert(&self, key: String, value: String) -> Result<Option<String>, StoreError>;
    fn remove(&self, key: &str) -> Result<Option<String>, StoreError>;
    /// Snapshot, sorted by key -> deterministic /list and op_rs3_list output.
    fn list(&self) -> Result<Vec<(String, String)>, StoreError>;
}

#[derive(Clone)]
pub struct Store(std::sync::Arc<dyn StoreBackend>);

impl Store {
    pub fn in_memory() -> Self { Self(std::sync::Arc::new(memory::MemoryBackend::default())) }
    /// Validate ALL entries first, then insert (no partial inserts on bad input).
    pub fn insert_many(&self, entries: Vec<(String, String)>) -> Result<(), StoreError> { /* ... */ }
    pub fn require(&self, key: &str) -> Result<String, StoreError> {
        self.0.get(key)?.ok_or_else(|| StoreError::NotFound(key.to_owned()))
    }
    // get / remove / list delegate to self.0
}
```

### Pattern 2: Dedicated JS worker pool behind a `Send + Sync` async handle (recommended threading model)

**What:** `JsExecutor` is a cloneable handle. It holds the sender of a **bounded MPMC queue** (`async_channel`) and the limits. `N` dedicated OS threads (`std::thread::Builder::new().name("rs3-js-{i}").stack_size(cfg.js.thread_stack_bytes)`) each:

1. build a `tokio` **current_thread** runtime (`enable_all`) and drive it with `rt.block_on(rx.recv())` between jobs,
2. for each job **create a fresh `JsRuntime`** with `rs3_ext::init(ExecCtx { store, depth: 0, limits, group })` while that runtime is entered,
3. run `execute_script`, convert the result to an owned `String` **on the worker**, drop the runtime, and send `Result<String, ExecError>` back over a `oneshot`.

The HTTP handler only `await`s the oneshot, so actix workers never block.

**Why this and not the alternatives** (each point checked against the deno_core 0.412 / rusty_v8 source):

| Option | Verdict | Reason |
|---|---|---|
| Create `JsRuntime` directly in the actix handler | **Reject** | It compiles, because actix runs `!Send` futures on single-threaded workers inside a `LocalSet`, so the compiler won't stop you. But `execute_script` is synchronous, so a runaway script freezes every connection on that worker. Two interleaved requests on one worker that hold runtimes across `.await` also break rusty_v8's rule *"OwnedIsolate instances must be dropped in the reverse order of creation"*, which is an assert in `Drop` and panics. |
| `web::block` / `spawn_blocking` per request | Acceptable fallback, not preferred | Concurrency is unbounded (tokio allows 512 blocking threads per runtime, and actix has one runtime per worker), so isolates × heap can exhaust memory. The 2 MiB default thread stack is too small for nested `op_rs3_patch` isolates (see Pattern 4), and it can't be set per call. |
| `std::thread::spawn` per job + `Semaphore` | Acceptable simpler variant | Bounded and the stack is configurable. Thread spawn cost (tens of µs) is negligible next to `JsRuntime::new`. The threads descend from whichever thread spawns them, which complicates the V8 protected-platform rule in tests (see Pitfall 3 below). |
| **Fixed pool of dedicated threads (this pattern)** | **Recommend** | It is bounded by construction, stack size is configurable, and all isolate threads come from one place (which satisfies V8 thread ancestry). Each thread has its own tokio context, which deno_core needs (see below). This also matches Deno's own model of isolates on dedicated threads that never move. About 150 LOC. |
| One long-lived `JsRuntime` per worker, reused across jobs | **Reject** | It changes semantics. Classic scripts share the realm's script scope, so `requests/fac.js` (`let num = ...`) fails the second time with *"Identifier 'num' has already been declared"*. Globals and `Deno.core` state would also leak between requests and keys. |

**Hard requirement found in deno_core 0.412 source (`runtime/setup.rs`):** `JsRuntime::try_new` registers the isolate with `tokio::runtime::Handle::try_current()`. If the runtime was created **outside a tokio runtime context** and V8 later posts a *delayed* foreground task (for example the GC memory reducer under heap pressure), deno_core calls **`std::process::abort()`**. Its own `test_heap_limits` enters a `current_thread` runtime before creating the `JsRuntime` for this reason. Every JS worker thread must therefore create the `JsRuntime` while a tokio runtime is entered, and that runtime should be driven between jobs so delayed tasks actually run.

```rust
// src/js/executor.rs (sketch)
#[derive(Clone)]
pub struct JsExecutor { tx: async_channel::Sender<Job>, limits: Limits }

pub struct ScriptRequest { pub name: String, pub source: String, pub store: Store }

impl JsExecutor {
    /// Spawns the pool. Call ONCE per process, from a thread that is (or descends
    /// from) the thread that called `init_platform()`.
    pub fn start(cfg: &JsSettings) -> Self { /* spawn cfg.workers threads */ }

    pub async fn execute(&self, req: ScriptRequest) -> Result<String, ExecError> {
        let group = TerminationGroup::new();
        let _cancel_on_drop = group.guard();               // client disconnect => terminate
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx.try_send(Job { req, group: group.clone(), reply_tx })
            .map_err(|_| ExecError::Busy)?;                 // queue full => 503
        match tokio::time::timeout(self.limits.timeout, reply_rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(ExecError::WorkerGone),
            Err(_elapsed) => { group.terminate_all(); Err(ExecError::Timeout) }
        }
    }
}

fn worker_loop(rx: async_channel::Receiver<Job>) {
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    while let Ok(job) = rt.block_on(rx.recv()) {           // drives V8 delayed tasks between jobs
        let _enter = rt.enter();                            // tokio context for JsRuntime::new
        if job.group.is_cancelled() { let _ = job.reply_tx.send(Err(ExecError::Timeout)); continue; }
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            runtime::run_script(ExecCtx::root(job.req.store, job.group.clone()), &job.req.name, job.req.source)
        })).unwrap_or(Err(ExecError::Internal("js worker panicked".into())));
        let _ = job.reply_tx.send(result);
    }
}
```

**JsRuntime creation cost per request (LOW, not benchmarked):** Without a startup snapshot, deno_core evaluates its built-in bootstrap JS (`00_primordials.js`, `00_infra.js`, `01_core.js`, ...) on every `JsRuntime::new`. Expect single-digit to low tens of milliseconds in release builds, and much more in debug builds. The only historical data point found is deno_core commits citing ~1–2 ms savings per bootstrap step (2023). For this milestone this is acceptable, because a fresh runtime per PATCH is *required* for correct semantics. Add a timing test or bench in the JS phase. If it is too slow, the fix is a **startup snapshot** built in `build.rs` with `JsRuntimeForSnapshot`. It must be built in `build.rs` and not at server runtime, because deno_core asserts *"V8 may only be initialized once in either snapshotting or non-snapshotting mode"*. Defer this to a later milestone.

### Pattern 3: Inject per-job host context through `extension!` options into `OpState`

**What:** `ExecCtx { store: Store, depth: u32, limits: Limits, group: TerminationGroup }` is passed with `extension!(..., options = { ctx: ExecCtx }, state = |state, options| state.put(options.ctx))` and created with `rs3_ext::init(ctx)`. Ops read it with `state.borrow::<ExecCtx>()`. The store reaches JS **per job**, so the executor itself is store-agnostic. One process-wide pool can then serve any number of independent stores, which is exactly what isolated tests need. It is also what a future LSP "run this snippet against store X" needs.

```rust
// src/js/ops.rs (sketch — names become the JS-visible op names)
use deno_core::{op2, OpState};
use deno_error::JsErrorBox;

#[op2]
#[string]
fn op_rs3_list(state: &mut OpState) -> Result<String, JsErrorBox> {
    let ctx = state.borrow::<ExecCtx>();
    let entries = ctx.store.list().map_err(to_js)?;
    Ok(crate::store::legacy::format_list(&entries))      // SAME function GET /list uses
}

#[op2]
fn op_rs3_get(state: &mut OpState, #[string] key: String) -> Result<Option<String>, JsErrorBox> {
    state.borrow::<ExecCtx>().store.get(&key).map_err(to_js)  // missing key => JS null (no panic)
}

// op_rs3_insert(#[string] body) -> String, op_rs3_delete(#[string] key) -> Option<String>,
// op_rs3_patch(#[string] key) -> String   (see Pattern 4)

fn to_js(e: StoreError) -> JsErrorBox {
    match e {
        StoreError::InvalidInput(m) => JsErrorBox::type_error(m),
        other => JsErrorBox::generic(other.to_string()),
    }
}

deno_core::extension!(
    rs3_ext,
    ops = [op_rs3_list, op_rs3_get, op_rs3_insert, op_rs3_delete, op_rs3_patch],
    options = { ctx: ExecCtx },
    state = |state, options| { state.put(options.ctx); },
);
```

Fallible ops must return an error type implementing `deno_error::JsErrorClass` (op2 README). `JsErrorBox::{generic, type_error, range_error}` become the standard JS `Error`, `TypeError` and `RangeError`. **The store layer stays free of `deno_error`.** The `to_js` mapping lives in `js/ops.rs`.

Reading the result (replaces the old "new Context" hack, which read a value through the wrong context):

```rust
let global = rt.execute_script(format!("rs3:key/{name}"), source)?;   // Result<_, Box<JsError>>
deno_core::scope!(scope, &mut rt);
let local = deno_core::v8::Local::new(scope, global);
Ok(local.to_rust_string_lossy(scope))
```

(Pattern from `deno_core/examples/eval_js_value.rs` 0.412.0. Always use `deno_core::v8` and never a direct `v8` dependency, because deno_core uses the `deno_v8` facade and a direct `v8` dependency risks version skew.)

**Op-name mapping to document (README and requirements):**

| Old (`Deno.core.opSync`) | New (`Deno.core.ops.*`) | Change |
|---|---|---|
| `opSync("rs3_list", "")` | `Deno.core.ops.op_rs3_list()` | dummy argument dropped (extra JS args are ignored anyway) |
| `opSync("rs3_get", key)` | `Deno.core.ops.op_rs3_get(key)` | missing key → `null` (was a panic) |
| `opSync("rs3_insert", json)` | `Deno.core.ops.op_rs3_insert(json)` | bad JSON → throws `TypeError` |
| `opSync("rs3_delete", key)` | `Deno.core.ops.op_rs3_delete(key)` | missing key → `null` |
| `opSync("rs3_patch", key)` | `Deno.core.ops.op_rs3_patch(key)` | nesting depth limited; errors thrown |
| `Deno.core.ops();` (cache init) | *delete the line* | no longer exists |

Optional later improvement: ship a tiny `js = [...]` file in the extension defining `globalThis.rs3 = { get, list, ... }`. That gives stored scripts a stable API that won't break if op names change, and it is the natural place for a `.d.ts` when TypeScript arrives.

### Pattern 4: Re-entrant `op_rs3_patch` via a nested isolate with an explicit depth budget

**What:** JS calls `op_rs3_patch(key)`, which runs another stored script. The faithful and simplest implementation keeps the original semantics (the nested script gets fresh globals and can call ops). **Inside the op, on the same thread**, it creates a *new* `JsRuntime` with `ExecCtx { depth: depth + 1, same store, same limits, same group }`, runs it, converts the result to `String`, and **drops it before the op returns**.

**Why it is safe:** rusty_v8 enters an isolate when it is created and exits it on drop, and asserts reverse-creation drop order. Creating and dropping inside one op is LIFO by construction. The nested runtime has its own `OpState`, so the outer `&mut OpState` borrow does not conflict. A rejected alternative is to compile the nested script in the *same* isolate with a `PinScope` op argument. It would share the outer realm's script scope (`let` collisions as above), and a new bare `v8::Context` would lack the `Deno.core.ops` bindings. Another rejected alternative is to dispatch the nested job to another pool worker. That needs async ops and an event loop, and it **deadlocks the pool** once all workers are waiting on nested jobs.

**Mandatory guards:**
1. **Depth limit** (`js.max_patch_depth`, default 4). If `depth >= max` the op returns `JsErrorBox::range_error("rs3_patch: max nesting depth 4 exceeded")`. Without it, a key that patches itself recurses until the **native stack overflows, which is a SIGSEGV and kills the whole server**, not a catchable error.
2. **Stack sizing (MEDIUM; reasoned from V8 internals, verify in a spike):** V8 sets each isolate's JS stack limit relative to the stack position where the thread enters it, default `--stack-size` ≈ 984 KB. Every nesting level therefore adds about 1 MB of native stack plus deno_core setup frames. Size worker threads explicitly: `thread_stack_bytes ≈ (max_patch_depth + 1) × 2 MiB`, default 16 MiB. This is the main reason `spawn_blocking`'s fixed 2 MiB stack is unsuitable.
3. **Shared termination:** Nested isolates register their `IsolateHandle` in the **same** `TerminationGroup`, so a timeout or disconnect terminates the whole chain and not just the outer isolate.
4. **Cycle visibility:** On depth overflow, include the key chain in the error message (`a → b → a → …`) so users can see the cycle.

### Pattern 5: Runaway-script control with `TerminationGroup` (timeout, disconnect, heap)

**What:** `TerminationGroup = Arc<{ cancelled: AtomicBool, handles: Mutex<Vec<v8::IsolateHandle>>, reason: OnceLock<TerminationReason> }>`.
- The worker registers `rt.v8_isolate().thread_safe_handle()` right after each runtime is created (outer and nested). If `cancelled` is already set, it terminates immediately. Registration returns a guard that unregisters on drop.
- **Timeout:** The async side calls `tokio::time::timeout(limits.timeout, reply_rx)`. On elapsed it calls `group.terminate_all()`, which sets `cancelled` and calls `terminate_execution()` on every registered handle. `IsolateHandle` is `Send + Sync` and callable from any thread, per the rusty_v8 docs.
- **Client disconnect:** actix drops the handler future, and the `_cancel_on_drop` guard calls `terminate_all()`. Without this, a disconnected client leaves a runaway script pinning a worker forever, because nobody is left to fire the timeout.
- **Heap limit:** `RuntimeOptions.create_params = Some(v8::Isolate::create_params().heap_limits(0, js.heap_limit_bytes))` plus `rt.add_near_heap_limit_callback(|cur, _| { mark(Reason::Memory); handle.terminate_execution(); cur * 2 })`. Without the callback, hitting the V8 heap limit **aborts the process**. This is exactly the pattern in deno_core's `test_heap_limits`.
- **Classification:** A terminated script surfaces as a `JsError` with `exception_message == "Uncaught Error: execution terminated"`. Map it using `group.reason()` (Timeout / Memory / Cancelled), **not** by string matching. Runtimes are discarded after each job, so `cancel_terminate_execution()` is never needed.
- **Limitation:** `terminate_execution` interrupts JS only, not Rust code inside an op. That is fine for the in-memory store. A future git backend's slow ops need their own deadline checks via `ExecCtx.limits`.

### Pattern 6: Layered errors with `thiserror` and actix `ResponseError`

```
StoreError ──┐                              (store: no HTTP, no V8)
             ├─► ExecError::Store(StoreError)   (js: no HTTP)
JsError ─────┴─► ExecError::Script{..}/Timeout/Memory/Busy/WorkerGone/Internal
StoreError ─────► ApiError (From)  ─┐
ExecError  ─────► ApiError (From)  ─┴─► impl ResponseError { status_code, error_response }
StoreError ─────► JsErrorBox (js/ops.rs::to_js) ─► thrown inside JS ─► maybe uncaught ─► JsError ─► ExecError::Script
```

```rust
// src/js/mod.rs
#[derive(Debug, thiserror::Error)]
pub enum ExecError {
    #[error("script error: {message}")]
    Script { message: String, stack: Option<String> },   // built from Box<JsError> on the worker
    #[error("script exceeded time limit")]  Timeout,
    #[error("script exceeded memory limit")] Memory,
    #[error("js executor busy")]            Busy,
    #[error("js worker unavailable")]       WorkerGone,
    #[error(transparent)]                   Store(#[from] StoreError),
    #[error("internal js error: {0}")]      Internal(String),
}

// src/http/error.rs
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("{0}")] BadRequest(String),
    #[error("{0}")] NotFound(String),
    #[error("{0}")] Script(String),
    #[error("script timed out")] Timeout,
    #[error("busy, retry later")] Busy,
    #[error("internal error")] Internal,          // details logged, not returned
}

impl actix_web::ResponseError for ApiError {
    fn status_code(&self) -> StatusCode { /* table below */ }
    fn error_response(&self) -> HttpResponse {
        HttpResponse::build(self.status_code()).json(serde_json::json!({ "error": self.to_string() }))
    }
}
```

| Source error | HTTP | Notes |
|---|---|---|
| `StoreError::NotFound` (GET/DELETE/PATCH missing key) | **404** | Replaces the `unwrap()` panic and the "`[]` when map empty" bug |
| `StoreError::InvalidInput` (bad JSON, non-object, non-string value) | **400** | Keep the raw `String` body extractor plus manual `serde_json` parse. Switching to `web::Json` would start requiring `Content-Type: application/json`, which existing clients and `requests/*.http` may not send |
| `ExecError::Script` (syntax error, uncaught throw, op error not caught) | **422** | Return `exception_message` only. Log the stack server-side and don't leak it |
| `ExecError::Memory` | **422** | The script is at fault |
| `ExecError::Timeout` | **504** | Same choice as common function platforms (for example Vercel `FUNCTION_INVOCATION_TIMEOUT`). 408 is wrong because it means the client sent the request too slowly |
| `ExecError::Busy` (queue full) | **503** + `Retry-After: 1` | Backpressure instead of unbounded queueing |
| `StoreError::Backend`, `WorkerGone`, `Internal` | **500** | |

Success bodies stay exactly as before (`"Welcome to rs3"`, the double-encoded insert echo, the list string). Only error responses are new.

### Pattern 7: Injected configuration and a single `configure(state)` entry point

- `Settings` is loaded **once** in `main` and stored in `AppState.settings: Arc<Settings>`. `GET /ip` reads `state.settings.server.host`. The current code calls `Config::from_env()` on **every request** and `unwrap()`s it.
- `JsSettings { workers (default available_parallelism, min 1), queue_capacity (workers × 16), timeout_ms (5000), max_patch_depth (4), heap_limit_mb (64), thread_stack_mb (16) }`, all with `set_default`, so tests and `.env`-less runs work.
- Keep the existing env names `SERVER.HOST` / `SERVER.PORT` working. Verify how config 0.15's `Environment` handles dotted names (STACK/PITFALLS).
- `http::configure(state: AppState) -> impl FnOnce(&mut web::ServiceConfig)` registers `app_data(web::Data::new(state))` **and** the routes together. A test can't forget the state, which in actix is only a runtime 500 ("App data is not configured"), not a compile error. Fields of `AppState` are `Arc`s, so the per-worker `Data` wrapper still points at one shared store and pool. This satisfies the actix docs' rule that shared state must be created outside the `HttpServer::new` closure.

```rust
// src/main.rs (complete shape)
#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let settings = rs3::config::Settings::load().map_err(std::io::Error::other)?;
    rs3::js::init_platform();                       // MAIN thread, before any other thread exists
    let state = rs3::AppState::new(settings);       // Store::in_memory() + JsExecutor::start(&settings.js)
    let addr = (state.settings.server.host.clone(), state.settings.server.port);
    actix_web::HttpServer::new(move || actix_web::App::new().configure(rs3::http::configure(state.clone())))
        .bind(addr)?
        .run()
        .await
}
```

---

## Data Flow

### Request Flow — `PATCH /key/{key}` (the only path that touches V8)

```
client ──PATCH /key/fac──► actix worker (async, never blocks)
   handler(Path<String>, Data<AppState>)
      │ 1. store.require("fac")            ──► 404 if missing (no queueing)
      │ 2. js.execute(ScriptRequest{name, source, store.clone()}).await
      │        try_send(Job) ──► [bounded async_channel] ──► 503 if full
      │        timeout(limits.timeout, oneshot) ; CancelOnDrop guard armed
      ▼
rs3-js-K thread (tokio current_thread entered)
      │ 3. JsRuntime::new(extensions=[rs3_ext::init(ExecCtx{store, depth:0, group})], heap limits)
      │ 4. group.register(isolate_handle) ; add_near_heap_limit_callback
      │ 5. execute_script("rs3:key/fac", source)
      │        JS ── Deno.core.ops.op_rs3_get("num") ──► OpState<ExecCtx>.store.get ──► "5"
      │        JS ── Deno.core.ops.op_rs3_patch("other") ──► [nested JsRuntime depth 1, same group] ──► String
      │ 6. scope!(); Local::new(global).to_rust_string_lossy() ──► String
      │ 7. drop runtime (LIFO) ; oneshot.send(Ok(String) | Err(ExecError))
      ▼
handler ◄── Result<String, ExecError> ──► Ok: HttpResponse::Ok().json(string)  |  Err: ApiError ──► 422/504/503/500
```

### Request Flow — store-only routes

```
GET /key/{k}   : handler → state.store.get(k)        → Some(v): 200 json(v)        | None: 404
DELETE /key/{k}: handler → state.store.remove(k)     → Some(old): 200 json(old)    | None: 404
POST /insert   : handler(String body) → legacy::parse_insert_body(&body)?  (400)
                 → store.insert_many(entries)  → 200 json(legacy::format_insert_echo(..))
GET /list      : handler → store.list() → 200 json(legacy::format_list(&entries))   (== op_rs3_list output)
GET /, GET /ip : static / state.settings.server.host
```

### State Management

```
main: Settings ─┐
                ├─► AppState { Store (Arc<dyn Backend>), JsExecutor (Sender + Limits), Arc<Settings> }
                │        │ clone (Arc bumps)                     │ clone per job
                │        ▼                                       ▼
                │   web::Data<AppState> in every actix worker    ExecCtx in OpState of each JsRuntime
                │   (and later: LSP server, git HTTP scope)      (Store clone + depth + TerminationGroup)
```

The **only** mutable shared state is inside the `Store` backend. `JsRuntime`, `OpState` and `v8::Global` never leave their worker thread. Only owned `String`s and `ExecError`s cross threads.

### Key Data Flows

1. **Store injection:** `main` creates a `Store` → `AppState` → `web::Data` (HTTP) and → `ScriptRequest.store` → `ExecCtx` → `OpState` (JS). Tests create a fresh `Store` per test, which gives isolation without `thread::sleep`.
2. **Error propagation:** `StoreError` → (`ApiError` at the HTTP edge) or (`JsErrorBox` inside JS → possibly caught by user JS → otherwise `JsError` → `ExecError::Script` → `ApiError::Script` → 422).
3. **Cancellation:** timeout or handler drop → `TerminationGroup::terminate_all` → every live isolate in the chain → `execute_script` returns "execution terminated" → classified via `group.reason()`.

---

## Scaling Considerations

| Scale | Architecture adjustments |
|-------|--------------------------|
| Dev / tests / single user (now) | One pool per process. Tests share one `LazyLock<JsExecutor>` with a fresh `Store` per test. Debug-build V8 is slow, so keep JS tests few and focused |
| Small team, tens of PATCH/s | Defaults are fine. Worst-case JS memory ≈ `workers × (max_patch_depth+1) × heap_limit` (e.g. 4 × 5 × 64 MB ≈ 1.3 GB), so lower `heap_limit_mb` or depth if this is tight |
| Hundreds of PATCH/s | First bottleneck is `JsRuntime::new` bootstrap per request, fixed by a `build.rs` startup snapshot. Second is pool size, fixed by raising `workers` (CPU-bound, ≈ cores) |
| Persistent / multi-node | Out of scope. The git-backed `StoreBackend` is the persistence path. Multi-node would need a different store, which the trait boundary allows |

### Scaling Priorities

1. **First bottleneck:** isolate bootstrap cost per PATCH. Measure it in the JS phase, and only add a snapshot if it matters.
2. **Second bottleneck:** concurrent PATCH capacity is `workers`. The queue plus 503 keeps the server responsive rather than slow.

---

## Anti-Patterns

### Anti-Pattern 1: Global store (`lazy_static`/`LazyLock` MAP) used directly by handlers and ops
**What people do:** Replace `lazy_static!` with `static MAP: LazyLock<DashMap<..>>` and call it done.
**Why it's wrong:** It meets the "no lazy_static" checkbox but keeps the shared-map test races, and it blocks multiple stores (a git backend, per-test stores).
**Do this instead:** Inject `Store` through `AppState` and `ExecCtx` (Patterns 1 and 3). `LazyLock` is fine **only** for the process-wide test `JsExecutor`.

### Anti-Pattern 2: Holding a `JsRuntime` (or any `v8::Global`/`Local`) across `.await` or thread boundaries
**What people do:** Create the runtime in an actix handler (it compiles because actix futures can be `!Send`), or try to send a `Global` back to the handler.
**Why it's wrong:** It blocks the worker, risks the reverse-order isolate drop assert panic, and a runaway script freezes every connection on that worker.
**Do this instead:** Everything V8-related runs and finishes inside one synchronous call on a JS worker. Only `String`/`ExecError` cross back.

### Anti-Pattern 3: Reusing one `JsRuntime` for many scripts
**Why it's wrong:** Top-level `let`/`const` collide across scripts (`fac.js` breaks on its second run), and state leaks between keys.
**Do this instead:** Use a fresh runtime per job. Optimize later with a snapshot, not with reuse.

### Anti-Pattern 4: Unbounded `web::block` / `spawn_blocking` for JS
**Why it's wrong:** 512 blocking threads per actix worker runtime × V8 heap is effectively unbounded memory, and the 2 MiB stacks are too small for nested isolates.
**Do this instead:** Use the bounded pool with explicit stack size and 503 backpressure.

### Anti-Pattern 5: Store methods that return guards/references (`dashmap::Ref`, `papaya` guard)
**Why it's wrong:** If a guard is held while JS runs or while another store call happens on the same thread (op → nested patch → insert into the same shard), dashmap deadlocks.
**Do this instead:** The `StoreBackend` API returns owned values only, and guards never escape `memory.rs`.

### Anti-Pattern 6: Mapping errors by string matching / one giant crate `Error`
**Why it's wrong:** Messages are brittle (for example `"Uncaught Error: execution terminated"`), and a global enum couples `store` to HTTP and V8.
**Do this instead:** Use per-layer `thiserror` enums, `From` conversions at the edges, and `TerminationGroup::reason()` for terminations.

### Anti-Pattern 7: Letting V8 initialize lazily on an arbitrary thread
**Why it's wrong:** rusty_v8's default (protected) platform requires every V8-using thread to **descend from the thread that called `V8::initialize`**. On Linux x86 with memory protection keys (PKU), which is likely on CI runners, violating this crashes (rusty_v8 issue #1381). deno_core only uses the unprotected platform under its *own* `cfg(test)` or the `unsafe_use_unprotected_platform` feature.
**Do this instead:** Call `rs3::js::init_platform()` (wrapping `JsRuntime::init_platform(None)`) as the first thing in `main`, before actix spawns workers. Create the pool once per process. In tests, use one `LazyLock<JsExecutor>` whose initializer calls `init_platform()` and then spawns the workers from that same thread. The fallback if CI still crashes is to enable `deno_core/unsafe_use_unprotected_platform` **only** under `[dev-dependencies]`.

---

## Integration Points

### Future milestone seams (how this structure keeps them open)

| Future feature | Plugs in at | What this milestone must already provide |
|---|---|---|
| Git-backed storage | `store/git.rs` implementing `StoreBackend` | Sync, fallible, owned-value trait. `StoreError::Backend`. HTTP calls go through `Store` and never through a concrete map |
| VS Code / LSP access | New adapter module (or `src/bin/rs3-lsp.rs`) using `AppState` | Transport-agnostic `AppState`, `JsExecutor::execute(ScriptRequest)` independent of actix, and lib/bin split |
| TypeScript execution (tsc-rs or similar) | A `Transpiler` step in `js/runtime.rs` before `execute_script`, or a deno_core `ModuleLoader` (see `deno_core/examples/ts_module_loader.rs`) | `ScriptRequest` able to grow a `lang: Js \| Ts` field. Executor owns the source → JS step. Optional `globalThis.rs3` shim gives a `.d.ts` target |
| Git over HTTP | A separate `http::git::configure` scope (`/git/...`) | `http::configure` composed from per-area `configure` functions |

### Internal Boundaries

| Boundary | Communication | Notes |
|---|---|---|
| `http` ↔ `store` | Direct sync calls on `Store` | Wrap in `web::block` only once a slow backend exists |
| `http` ↔ `js` | `JsExecutor::execute(..).await` (queue + oneshot) | Only owned data crosses. Timeout and cancel are owned by the executor, not the handler |
| `js` ↔ `store` | `ExecCtx.store` in `OpState`, read by `#[op2]` sync ops | Ops convert `StoreError` → `JsErrorBox`. Legacy codec shared with `http` |
| `js` op ↔ nested `js` | Nested `JsRuntime` created and dropped inside `op_rs3_patch` | Depth budget, same `TerminationGroup`, LIFO drop |
| `config` → everything | `Settings` values passed at construction | No module reads env vars after `main` |

---

## Suggested Build Order (dependencies for the roadmap)

```
[A] Toolchain + Cargo bump (edition 2024, released deps, deno_core compiles on aarch64)
        │   (JS layer may be temporarily stubbed: PATCH → 501, so the tree stays green)
        ▼
[B] lib.rs/main.rs split + config.rs (Settings, injected) ──────────────┐
        │                                                               │
[C] store/ (Store, StoreBackend, MemoryBackend, StoreError, legacy codec, unit tests) — no deps on B
        │                                                               │
        ▼                                                               ▼
[D] http/ port to actix 4.17: AppState(store, settings), configure(state), ApiError,
    handlers return Result; isolated tests via fresh Store (no sleep) ── depends on B + C
        │
        ▼
[E] js/ core: init_platform, ops (#[op2]) + extension!(rs3_ext) with ExecCtx, run_script
    (scope! result read), JsExecutor pool + queue, PATCH wired, op-name mapping, JS samples
    updated ── depends on C (Store) + D (AppState/ApiError)
        │
        ▼
[F] js/ hardening: TerminationGroup (timeout 504, cancel-on-drop), heap limit + callback,
    op_rs3_patch nesting with depth limit + stack sizing, Busy 503 ── depends on E
        │
        ▼
[G] CI (x86_64 runners!), Dockerfile, Readme ── CI should land right after A/E compile,
    not last: the x86 runner is where the V8 thread-ancestry (PKU) issue would show up.
```

**Ordering rationale:**
- **C before D and E:** both layers depend on `Store`. It is pure Rust with no V8 or actix, so it is the cheapest place to settle API shape (owned values, sorted `list`, `insert_many` validate-then-write).
- **B can run in parallel with C.** Both are small and independent.
- **D before E:** E needs `AppState` and `ApiError` to wire PATCH. Porting HTTP first also turns most of the old test suite green before the riskiest work starts.
- **E split from F:** First get "PATCH works with modern deno_core" (core value). Then add limits. F is the part most likely to surprise (nested isolates, stack sizing, termination classification), so it deserves its own verification.
- **CI early:** the protected-platform crash and slow first V8 build only show up on clean x86 CI. Finding them in the final phase would force rework in E.

**Research flags:**
- **Phase E/F needs a spike or deeper research:** (1) actual `JsRuntime::new` cost in release and debug on aarch64, (2) nested `JsRuntime` inside a sync op on deno_core 0.412 (expected to work, LIFO-safe, but not verified by a compiled test because disk space prevented a local build here), (3) native stack needed per nesting level, (4) whether `Result<Option<String>, JsErrorBox>` compiles as an op2 return without attributes (ToV8 for `Option<T>` exists, so likely yes).
- **Phase A:** whether the `deno_core` / `deno_v8` / `v8` prebuilt static library download works in the sandbox and on CI (STACK.md).
- **Phases B, C, D:** standard patterns, no extra research needed.

---

## Sources

Primary (published crate source of the exact versions, read directly; the research-plan seam scores this unregistered provider as LOW, but it is first-hand code evidence):
- deno_core 0.412.0 (crates.io tarball): `runtime/jsruntime.rs` (`execute_script`, `add_near_heap_limit_callback`, `op_state`, `register_isolate` with `Handle::try_current`), `runtime/setup.rs` (abort when there is no tokio context; protected vs unprotected platform; init-once assert), `extensions.rs` (`extension!` `options`/`state`), `runtime/tests/misc.rs` (`terminate_execution`, `test_heap_limits`), `examples/eval_js_value.rs`, `examples/op2.rs`, `01_core.js`/`00_infra.js` (`Deno.core.ops`, `print`), `convert.rs` (`ToV8 for Option<T>`), `Cargo.toml` (`deno_error =0.7.1`, `deno_v8` facade, feature `unsafe_use_unprotected_platform`)
- deno_ops 0.288.0 `op2/README.md`, `valid_args.md`, `valid_retvals.md` (fallible ops need `JsErrorClass`; `&mut OpState`; `#[string]`)
- deno_error 0.7.1 `src/lib.rs` (`JsErrorBox::{new, generic, type_error, range_error}`)
- rusty_v8 `src/isolate.rs` (reverse-order drop assert; `IsolateHandle::terminate_execution` usable from any thread) and `src/platform.rs` (protected platform thread-ancestry and PKU note): https://github.com/denoland/rusty_v8
- rusty_v8 issue #1381 (PKU crash when V8 is initialized off the main thread), cited via the v8 crate troubleshooting notes: https://docs.rs/crate/v8/150.3.0, https://socket.dev/cargo/package/v8/overview/150.4.0

Official docs (MEDIUM via seam, cross-checked):
- actix-web application state and `configure`: https://actix.rs/docs/application
- actix-web `ResponseError` (4.17.0): https://docs.rs/actix-web/latest/actix_web/error/trait.ResponseError.html

Community / secondary (LOW, context only):
- deno_core embedding in an existing tokio runtime (current-thread expectation): https://questions.deno.com/m/1247553728001609729
- deno_core manual tick/poll discussion: https://questions.deno.com/m/1211327794022191104
- Historical bootstrap timing notes in deno core commits: https://code.fosterhangdaan.com/foster/denoland-deno/commits/commit/6255cf4642f97478fbf05f7b2cff1a1934364983/core?page=3
- deno_core setup.rs source on docs.rs (0.409 cross-check): https://docs.rs/crate/deno_core/0.409.0/source/runtime/setup.rs

Existing code reviewed: `src/main.rs`, `src/bindings/mod.rs`, `src/services/mod.rs`, `src/config/mod.rs`, `src/example.js`, `requests/fac.js`, `.planning/PROJECT.md`, `MODERNIZATION_BRIEF.md`.

---
*Architecture research for: rs3 modernization (actix-web + deno_core K/V store)*
*Researched: 2026-10-20*
