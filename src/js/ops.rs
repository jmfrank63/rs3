//! The store ops scripts call as `Deno.core.ops.op_rs3_*`. They reach the
//! injected `Store` only through `OpState`, never panic, and are free of
//! actix-web. Once any level of a job has hit a host fault, every op of that
//! job throws `Error: internal error` instead of touching the store.
//! `op_rs3_patch` refuses to nest deeper than the job's `max_patch_depth`; once
//! it has refused, every op of that job throws the same depth error and
//! touches nothing. Once the job timed out, every op returns an empty string
//! and touches nothing, without throwing: a thrown op error while the isolate
//! is terminating would cancel the termination. Writes made before any stop
//! stay (no transactions).
use std::sync::Arc;

use deno_core::{FromV8, OpState, op2, v8};
use deno_error::JsErrorBox;

use super::{DepthStop, ExecError, JobState};
use crate::store::{Store, StoreError, legacy};

/// Per-job context the ops read from `OpState`.
#[derive(Clone, Debug)]
pub struct ExecCtx {
    /// The store of the caller that submitted the job.
    pub store: Store,
    /// How many `op_rs3_patch` levels deep this script runs (0 at the top).
    pub depth: u32,
    /// The keys from the PATCHed key (`chain[0]`) down to the key this script
    /// runs for, in call order. Names the refused chain in a depth stop.
    pub(crate) chain: Vec<String>,
    /// How many PATCH levels the job may run, counting the PATCHed key.
    pub(crate) max_patch_depth: u32,
    /// The job's shared state: host fault and depth stop. Shared by every
    /// level of one job, so the executor sees a stop even if a script caught
    /// the error `op_rs3_patch` threw.
    pub(crate) job: Arc<JobState>,
    /// Test hook: make runtime creation fail at this depth.
    #[cfg(test)]
    pub(crate) fail_runtime_at_depth: Option<u32>,
    /// Test hook: make runtime creation fail for the script with this name
    /// (the key `op_rs3_patch` runs), at any depth.
    #[cfg(test)]
    pub(crate) fail_runtime_for_key: Option<&'static str>,
    /// Test hook: an injected runtime failure first waits (bounded) until the
    /// job timed out, so the fault and the timeout overlap at that level.
    #[cfg(test)]
    pub(crate) fail_runtime_after_timeout: bool,
}

impl ExecCtx {
    /// A top-level context for `store` running the script named `root`, the
    /// first key of every chain, under `max_patch_depth`.
    pub fn new(store: Store, job: Arc<JobState>, root: &str, max_patch_depth: u32) -> Self {
        Self {
            store,
            depth: 0,
            chain: vec![root.to_owned()],
            max_patch_depth,
            job,
            #[cfg(test)]
            fail_runtime_at_depth: None,
            #[cfg(test)]
            fail_runtime_for_key: None,
            #[cfg(test)]
            fail_runtime_after_timeout: false,
        }
    }

    /// The context for the script `op_rs3_patch` runs for `key`: same store,
    /// job and limit, one level deeper, with `key` appended to the chain.
    /// `op_rs3_patch` checks `max_patch_depth` before it builds one.
    pub fn nested(&self, key: &str) -> Self {
        let mut chain = self.chain.clone();
        chain.push(key.to_owned());
        Self {
            store: self.store.clone(),
            depth: self.depth + 1,
            chain,
            max_patch_depth: self.max_patch_depth,
            job: Arc::clone(&self.job),
            #[cfg(test)]
            fail_runtime_at_depth: self.fail_runtime_at_depth,
            #[cfg(test)]
            fail_runtime_for_key: self.fail_runtime_for_key,
            #[cfg(test)]
            fail_runtime_after_timeout: self.fail_runtime_after_timeout,
        }
    }

    /// Record that a nested runtime hit a host fault during this job.
    pub(crate) fn note_host_fault(&self) {
        self.job.note_host_fault();
    }

    /// Whether any level of this job hit a host fault.
    pub(crate) fn host_fault(&self) -> bool {
        self.job.host_fault()
    }

    /// Write one error line for this job to stderr through the job's sink.
    /// The caller escapes any script-controlled text in it.
    pub(crate) fn log_error(&self, line: std::fmt::Arguments<'_>) {
        self.job.log_error(line);
    }
}

/// A string op argument. Strings pass through unchanged. Other primitives are
/// converted with JS `String(..)` semantics, so a missing argument reads
/// `"undefined"` and `5` reads `"5"`. Objects and symbols throw a `TypeError`.
///
/// deno_core's plain string argument turns every non-string into an empty
/// string, which would make `op_rs3_delete(5)` delete the key `""`. Objects are
/// rejected rather than converted because converting one runs user `toString`
/// code inside the op.
struct StrArg(String);

impl<'a> FromV8<'a> for StrArg {
    type Error = JsErrorBox;

    fn from_v8<'i>(
        scope: &mut v8::PinScope<'a, 'i>,
        value: v8::Local<'a, v8::Value>,
    ) -> Result<Self, Self::Error> {
        if value.is_object() || value.is_symbol() {
            return Err(JsErrorBox::type_error(
                "op argument must be a string or another primitive value",
            ));
        }
        // Converting a non-symbol primitive cannot throw or run user code.
        value
            .to_string(scope)
            .map(|s| StrArg(s.to_rust_string_lossy(scope)))
            .ok_or_else(|| JsErrorBox::type_error("op argument cannot be converted to a string"))
    }
}

/// Map a store error to a JS exception of a built-in class. A custom class name
/// would throw `undefined` in bare deno_core, so only `Error` and `TypeError`
/// are used.
fn to_js(e: StoreError) -> JsErrorBox {
    match e {
        StoreError::NotFound(_) => JsErrorBox::generic(e.to_string()),
        StoreError::MalformedJson(_) => JsErrorBox::type_error(e.to_string()),
        StoreError::InvalidInput(_) => JsErrorBox::type_error(e.to_string()),
    }
}

/// The error thrown into a script for a host fault. It carries no host details.
fn internal_error() -> JsErrorBox {
    JsErrorBox::generic("internal error")
}

/// The error thrown into a script for a depth stop. Built from the
/// `ExecError::DepthLimit` message, so the JS message and the 422 message
/// come from one format.
fn depth_error(stop: &DepthStop) -> JsErrorBox {
    JsErrorBox::generic(
        ExecError::DepthLimit {
            limit: stop.limit,
            chain: stop.chain.clone(),
        }
        .to_string(),
    )
}

/// The job context, or what an op must answer once the job is stopped.
///
/// `Ok(None)` once the job timed out, checked first: the op then returns an
/// empty string without touching the store, and never throws. A slow op's
/// `Err` while its isolate is terminating makes deno_core cancel the pending
/// V8 termination (the error builder fails and the try-catch is reset), and
/// the script would keep running past its timeout.
///
/// Otherwise the job's stop error in the D-04 order: `Error: internal error`
/// once any level hit a host fault (the request answers 500), then the depth
/// error once `op_rs3_patch` refused a level (the request answers 422
/// js_depth_limit). Either way no op may read or change the store any more:
/// writes made before the stop stay, writes after it are refused.
fn live_ctx(state: &OpState) -> Result<Option<&ExecCtx>, JsErrorBox> {
    let ctx = state.borrow::<ExecCtx>();
    if ctx.job.timed_out() {
        Ok(None)
    } else if ctx.job.host_fault() {
        Err(internal_error())
    } else if let Some(stop) = ctx.job.depth_stop() {
        Err(depth_error(stop))
    } else {
        Ok(Some(ctx))
    }
}

/// Returns the whole store as the `GET /list` text, `[{"k" : "v"},...]` sorted
/// by key, or `[]` when empty. Extra arguments are ignored. Throws only after
/// a host fault or a depth stop in the same job; returns an empty string
/// after a timeout.
#[op2]
#[string]
fn op_rs3_list(state: &mut OpState) -> Result<String, JsErrorBox> {
    let Some(ctx) = live_ctx(state)? else {
        return Ok(String::new());
    };
    Ok(legacy::format_list(&ctx.store.list()))
}

/// Returns the value stored under `key`. Throws `Error: key not found: <key>`
/// when the key is missing. Like every op, it throws the job's stop error
/// and touches nothing once the job has a host fault or a depth stop, and
/// returns an empty string without touching anything once the job timed out.
#[op2]
#[string]
fn op_rs3_get(state: &mut OpState, #[scoped] key: StrArg) -> Result<String, JsErrorBox> {
    let Some(ctx) = live_ctx(state)? else {
        return Ok(String::new());
    };
    ctx.store.get(&key.0).map_err(to_js)
}

/// Stores every pair of a JSON object of strings and returns the compact echo
/// `{"k":"v"}`. Throws `TypeError: malformed JSON: ..` or
/// `TypeError: invalid input: ..` and writes nothing when the body is rejected.
/// After a host fault or a depth stop it throws the job's stop error and
/// writes nothing; after a timeout it returns an empty string and writes
/// nothing.
#[op2]
#[string]
fn op_rs3_insert(state: &mut OpState, #[scoped] body: StrArg) -> Result<String, JsErrorBox> {
    let Some(ctx) = live_ctx(state)? else {
        return Ok(String::new());
    };
    // Validate the whole body before the first write.
    let entries = legacy::parse_insert_body(&body.0).map_err(to_js)?;
    let echo = legacy::format_insert_echo(&entries);
    ctx.store.insert_all(entries);
    Ok(echo)
}

/// Removes `key` and returns the value it held. Throws
/// `Error: key not found: <key>` when the key is missing. After a host fault
/// or a depth stop it throws the job's stop error and removes nothing; after
/// a timeout it returns an empty string and removes nothing.
#[op2]
#[string]
fn op_rs3_delete(state: &mut OpState, #[scoped] key: StrArg) -> Result<String, JsErrorBox> {
    let Some(ctx) = live_ctx(state)? else {
        return Ok(String::new());
    };
    ctx.store.remove(&key.0).map_err(to_js)
}

/// Runs the script stored under `key` in a fresh nested runtime on the same
/// thread and returns `String(completion value)`. Throws
/// `Error: key not found: <key>` when the key is missing, and an `Error` with
/// the nested exception message (one leading "Uncaught " removed) when the
/// nested script fails. When the calling script used up so much stack that the
/// nested runtime cannot boot, it throws
/// `Error: RangeError: not enough stack to start a nested script`, a script
/// error and not a host fault (see `runtime::boot_failed_for_lack_of_stack`);
/// that close to the limit deno_core may throw the bare message string
/// instead, because building the `Error` needs stack too.
///
/// When the job already runs `max_patch_depth` levels, it records a depth stop
/// for the job (logged once), throws
/// `Error: PATCH nesting depth limit (<limit>) exceeded: <chain>` and neither
/// reads `key` nor creates a runtime. The request then answers 422
/// js_depth_limit even if the script caught the error, and every later op of
/// the job throws the same error and touches nothing.
///
/// Any other host fault in the nested runtime (such as a runtime creation
/// failure) is logged once, by the level where it happened, recorded in the
/// job state and thrown as a plain `Error: internal error`; the top-level run
/// then answers
/// `ExecError::Internal` even if the script caught it, and every later op of
/// the job throws the same error.
///
/// Once the job timed out it returns an empty string without reading `key`
/// or creating a runtime, and it also returns an empty string, never an
/// error, when the job timed out while the nested script ran: throwing then
/// would cancel the termination of the calling script. A host fault in that
/// nested run is still logged and recorded first, so the request answers 500,
/// not 504.
//
// `reentrant` is mandatory: the nested runtime calls ops while this op is on
// the stack, and debug builds abort without the flag. The depth check runs
// before the store read and before any runtime is created, so a self-patching
// or cyclic script stops after `max_patch_depth` levels instead of nesting
// runtimes until memory or the native stack runs out.
#[op2(reentrant)]
#[string]
fn op_rs3_patch(state: &mut OpState, #[scoped] key: StrArg) -> Result<String, JsErrorBox> {
    let StrArg(key) = key;
    // Clone the context out so no OpState borrow is held across the nested run.
    let Some(ctx) = live_ctx(state)? else {
        return Ok(String::new());
    };
    let ctx = ctx.clone();
    // The chain holds every level so far, the PATCHed key being level 1, so the
    // next level would exceed the limit once it already holds the limit.
    if ctx.chain.len() >= ctx.max_patch_depth as usize {
        let mut chain = ctx.chain.clone();
        chain.push(key);
        let (stop, newly_set) = ctx.job.stop_depth(DepthStop {
            limit: ctx.max_patch_depth,
            chain,
        });
        if newly_set {
            // Debug-format the script-controlled keys so they cannot forge log lines.
            ctx.log_error(format_args!(
                "error: JS depth limit ({}) exceeded: {:?}",
                stop.limit,
                stop.chain.join(" -> ")
            ));
        }
        return Err(depth_error(stop));
    }
    let source = ctx.store.get(&key).map_err(to_js)?;
    let result = super::runtime::run_script(ctx.nested(&key), &key, source);
    // Record a host fault before anything else, timeout included: a host fault
    // beats a timeout (D-04), and it must be logged even when the job timed
    // out while the nested level ran. `live_ctx` saw the flag clear when this
    // op started, so if it is set now, a deeper level already logged and
    // recorded the fault, and this result only reports that the nested run
    // saw it. Logging again would blame a key whose runtime ran normally, once
    // per ancestor level.
    if let Err(e @ (ExecError::Internal(_) | ExecError::Unavailable)) = &result
        && !ctx.host_fault()
    {
        // Host details go to stderr only, escaped so they cannot forge log
        // lines. `Internal` already displays with a "JS host failure" prefix,
        // so log its bare detail.
        let detail = match e {
            ExecError::Internal(m) => m.clone(),
            e => e.to_string(),
        };
        ctx.log_error(format_args!(
            "error: nested JS host failure in {key:?} at depth {}: {detail:?}",
            ctx.depth + 1
        ));
        ctx.note_host_fault();
    }
    // Never throw into a terminating script: an Err here would cancel its
    // termination (see live_ctx).
    if ctx.job.timed_out() {
        return Ok(String::new());
    }
    // Every variant is listed, so a new one must be classified here.
    result.map_err(|e| match e {
        // Nested runs never classify depth or timeouts (the executor does that
        // once), so these arms are unreachable today.
        e @ ExecError::DepthLimit { .. } => JsErrorBox::generic(e.to_string()),
        e @ ExecError::Timeout { .. } => JsErrorBox::generic(e.to_string()),
        ExecError::Script { message } => JsErrorBox::generic(
            message
                .strip_prefix("Uncaught ")
                .unwrap_or(&message)
                .to_owned(),
        ),
        // Recorded and logged above.
        ExecError::Internal(_) | ExecError::Unavailable => internal_error(),
    })
}

/// Replaces the implementation of deno_core's built-in `op_print`, which backs
/// `Deno.core.print(text, isErr)`. Text for stdout passes through unchanged.
/// Text for stderr is written as one Debug-escaped line per call, prefixed with
/// `js stderr: `, so a script cannot forge server log lines. Never registered
/// under its own name; the signature must match `op_print` exactly.
///
/// Both streams write and flush through `write_all_retrying` and
/// `flush_retrying`, so a non-blocking stdout or stderr is retried like
/// upstream instead of failing the print. A pipe nobody drains therefore stalls
/// the worker: the client still gets the backstop 504, but the worker stays
/// busy until the stall ends.
#[op2(fast)]
fn op_rs3_print(#[string] msg: &str, is_err: bool) -> Result<(), std::io::Error> {
    if is_err {
        let line = format!("js stderr: {msg:?}\n");
        let mut err = std::io::stderr().lock();
        write_all_retrying(&mut err, line.as_bytes())?;
        flush_retrying(&mut err)
    } else {
        let mut out = std::io::stdout().lock();
        write_all_retrying(&mut out, msg.as_bytes())?;
        flush_retrying(&mut out)
    }
}

/// Writes all of `bytes` to `w` like deno_core's `op_print`: `Interrupted`
/// retries, `WouldBlock` yields the thread and retries, any other error is
/// returned. A write that accepts zero bytes is a `WriteZero` error instead of
/// a spin.
fn write_all_retrying(w: &mut impl std::io::Write, mut bytes: &[u8]) -> std::io::Result<()> {
    use std::io::ErrorKind;
    while !bytes.is_empty() {
        match w.write(bytes) {
            Ok(0) => return Err(ErrorKind::WriteZero.into()),
            Ok(n) => bytes = bytes.get(n..).unwrap_or_default(),
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(e) if e.kind() == ErrorKind::WouldBlock => std::thread::yield_now(),
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// Flushes `w` with the same retry rule as `write_all_retrying`; never unwraps.
fn flush_retrying(w: &mut impl std::io::Write) -> std::io::Result<()> {
    use std::io::ErrorKind;
    loop {
        match w.flush() {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(e) if e.kind() == ErrorKind::WouldBlock => std::thread::yield_now(),
            Err(e) => return Err(e),
        }
    }
}

deno_core::extension!(
    rs3_ext,
    ops = [op_rs3_list, op_rs3_get, op_rs3_insert, op_rs3_delete, op_rs3_patch],
    options = { ctx: ExecCtx },
    // op_panic is a deno_core built-in that panics inside an op, which aborts
    // the whole process. Disabled ops throw a catchable "op is disabled".
    // op_print keeps its name but escapes what it writes to stderr.
    middleware = |op| match op.name {
        "op_panic" => op.disable(),
        "op_print" => op.with_implementation_from(&op_rs3_print()),
        _ => op,
    },
    state = |state, options| {
        state.put(options.ctx);
    },
);

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::ExecCtx;
    use crate::js::executor::JsExecutor;
    use crate::js::runtime::run_script;
    use crate::js::{
        DepthStop, ExecError, JsSettings, block_on, dedicated_executor, run_js, test_executor,
    };
    use crate::store::{Store, legacy};

    /// A top-level context named "test" with a fresh job and the default limit.
    fn ctx_for(store: &Store) -> ExecCtx {
        ExecCtx::new(store.clone(), Arc::default(), "test", 4)
    }

    fn ok(s: &str) -> Result<String, ExecError> {
        Ok(s.to_owned())
    }

    #[test]
    fn run_script_reads_the_store() {
        let store = Store::new();
        store.insert("num".into(), "5".into());
        assert_eq!(run_js(&store, "Deno.core.ops.op_rs3_get('num')"), ok("5"));
    }

    #[test]
    fn nested_patch_returns_inner_result() {
        let store = Store::new();
        store.insert("num".into(), "5".into());
        store.insert("inner".into(), "Deno.core.ops.op_rs3_get('num')".into());
        assert_eq!(
            run_js(&store, "Deno.core.ops.op_rs3_patch('inner')"),
            ok("5")
        );
    }

    #[test]
    fn nested_patch_has_own_top_level_scope() {
        let store = Store::new();
        store.insert("inner".into(), "let q = 70; q".into());
        assert_eq!(
            run_js(
                &store,
                "let q = 1; q + Number(Deno.core.ops.op_rs3_patch('inner'))"
            ),
            ok("71")
        );
    }

    #[test]
    fn nested_throw_is_catchable_without_uncaught_prefix() {
        let store = Store::new();
        store.insert("inner".into(), "throw new RangeError('inner boom')".into());
        let src = "try { Deno.core.ops.op_rs3_patch('inner'); 'no throw' } \
                   catch (e) { (e instanceof Error) + '|' + e.message }";
        assert_eq!(run_js(&store, src), ok("true|RangeError: inner boom"));
    }

    #[test]
    fn nested_symbol_result_matches_js_string() {
        let store = Store::new();
        store.insert("inner".into(), "Symbol('x')".into());
        assert_eq!(
            run_js(&store, "'[' + Deno.core.ops.op_rs3_patch('inner') + ']'"),
            ok("[Symbol(x)]")
        );
    }

    #[test]
    fn nested_stringify_exception_throws_into_caller() {
        let store = Store::new();
        store.insert(
            "throws".into(),
            "({ toString() { throw new Error('x') } })".into(),
        );
        store.insert("bare".into(), "Object.create(null)".into());
        let src = "const g = (k) => { try { Deno.core.ops.op_rs3_patch(k); return 'no throw'; } \
                   catch (e) { return (e instanceof Error) + ' ' + e.message; } }; \
                   g('throws') + '|' + g('bare')";
        let got = run_js(&store, src).expect("script result");
        assert!(got.starts_with("true Error: x|true TypeError"), "{got}");
        assert_eq!(
            run_js(&store, "Deno.core.ops.op_rs3_patch('throws')"),
            Err(ExecError::Script {
                message: "Uncaught Error: Error: x".to_owned()
            })
        );
    }

    /// Run `source` against `ctx` on the shared test executor.
    fn run_ctx(ctx: ExecCtx, source: &str) -> Result<String, ExecError> {
        let source = source.to_owned();
        block_on(test_executor().run(move || run_script(ctx, "test", source)))
    }

    /// Run `source` against `ctx` as the job of `ctx` on `exec`: the context's
    /// runtimes are armed by the watchdog and the result is classified. Use it
    /// for every script that can loop.
    fn run_ctx_on(exec: &JsExecutor, ctx: ExecCtx, source: &str) -> Result<String, ExecError> {
        let source = source.to_owned();
        let job = Arc::clone(&ctx.job);
        block_on(exec.run_job(job, "test".to_owned(), move || {
            run_script(ctx, "test", source)
        }))
    }

    /// A dedicated one-worker executor with a 300 ms timeout.
    fn timeout_exec() -> JsExecutor {
        dedicated_executor(JsSettings {
            workers: 1,
            queue_capacity: 4,
            timeout_ms: 300,
            ..JsSettings::default()
        })
    }

    const TIMEOUT_300: Result<String, ExecError> = Err(ExecError::Timeout { limit_ms: 300 });

    #[test]
    fn timeout_refuses_writes_after_stop() {
        let store = Store::new();
        store.insert("loop".into(), "while(true){}".into());
        // Straight-line after the catch, so this cannot hang before the fix.
        let src = r#"try { Deno.core.ops.op_rs3_patch('loop') } catch (e) {}
                     Deno.core.ops.op_rs3_insert('{"after":"1"}'); 'done'"#;
        let r = run_ctx_on(&timeout_exec(), ctx_for(&store), src);
        assert_eq!(r, TIMEOUT_300);
        assert!(
            store.get("after").is_err(),
            "a write after the timeout landed"
        );
    }

    #[test]
    fn timeout_nested_loop_is_timeout() {
        let store = Store::new();
        store.insert("loop".into(), "while(true){}".into());
        let r = run_ctx_on(
            &timeout_exec(),
            ctx_for(&store),
            "Deno.core.ops.op_rs3_patch('loop')",
        );
        assert_eq!(r, TIMEOUT_300);
    }

    #[test]
    fn timeout_survives_caught_nested_termination() {
        let store = Store::new();
        store.insert("loop".into(), "while(true){}".into());
        let src = "try { Deno.core.ops.op_rs3_patch('loop') } catch (e) {} while (true) {}";
        let start = std::time::Instant::now();
        let r = run_ctx_on(&timeout_exec(), ctx_for(&store), src);
        let elapsed = start.elapsed();
        assert_eq!(r, TIMEOUT_300);
        assert!(
            elapsed < std::time::Duration::from_millis(1300),
            "{elapsed:?}"
        );
    }

    #[test]
    fn timeout_throwing_op_loop_is_timeout() {
        let exec = timeout_exec();
        let src = "for (;;) { try { Deno.core.ops.op_rs3_get('missing') } catch (e) {} }";
        for run in 0..3 {
            let start = std::time::Instant::now();
            let r = run_ctx_on(&exec, ctx_for(&Store::new()), src);
            let elapsed = start.elapsed();
            assert_eq!(r, TIMEOUT_300, "run {run}");
            assert!(
                elapsed < std::time::Duration::from_millis(1300),
                "run {run}: {elapsed:?}"
            );
        }
    }

    #[test]
    fn timeout_during_stringify_is_timeout() {
        let r = run_ctx_on(
            &timeout_exec(),
            ctx_for(&Store::new()),
            "({ toString() { while (true) {} } })",
        );
        assert_eq!(r, TIMEOUT_300);
    }

    #[test]
    fn timeout_is_logged_once() {
        let store = Store::new();
        store.insert("loop".into(), "while(true){}".into());
        let exec = timeout_exec();
        for src in [
            "Deno.core.ops.op_rs3_patch('loop')",
            "try { Deno.core.ops.op_rs3_patch('loop') } catch (e) {} while (true) {}",
        ] {
            let ctx = ctx_for(&store);
            let job = Arc::clone(&ctx.job);
            assert_eq!(run_ctx_on(&exec, ctx, src), TIMEOUT_300, "{src}");
            // Only the worker's timeout line; no per-level js error lines.
            assert_eq!(job.error_log_count(), 1, "{src}");
        }
    }

    #[test]
    fn timeout_beats_depth_limit() {
        let store = Store::new();
        store.insert("me".into(), "Deno.core.ops.op_rs3_patch('me')".into());
        let src = "try { Deno.core.ops.op_rs3_patch('me') } catch (e) {} while (true) {}";
        let ctx = ctx_for(&store);
        let job = Arc::clone(&ctx.job);
        assert_eq!(run_ctx_on(&timeout_exec(), ctx, src), TIMEOUT_300);
        // The depth stop happened, and the timeout still wins (D-04).
        assert!(job.depth_stop().is_some());
    }

    #[test]
    fn host_fault_beats_timeout() {
        let store = Store::new();
        store.insert("bad".into(), "1".into());
        let src = "try { Deno.core.ops.op_rs3_patch('bad') } catch (e) {} while (true) {}";
        let ctx = failing_for(&store, "bad");
        let job = Arc::clone(&ctx.job);
        let r = run_ctx_on(&timeout_exec(), ctx, src);
        assert!(is_internal(&r), "{r:?}");
        assert!(job.timed_out());
    }

    #[test]
    fn host_fault_during_the_timeout_beats_it_and_is_logged() {
        let store = Store::new();
        store.insert("bad".into(), "1".into());
        // The injected failure of `bad` returns only once the job timed out,
        // so the fault and the timeout overlap at that level. Straight-line
        // after the call: the armed 300 ms timeout ends the job either way.
        let mut ctx = failing_for(&store, "bad");
        ctx.fail_runtime_after_timeout = true;
        let job = Arc::clone(&ctx.job);
        let r = run_ctx_on(
            &timeout_exec(),
            ctx,
            "Deno.core.ops.op_rs3_patch('bad'); 'done'",
        );
        assert!(job.timed_out());
        assert!(job.host_fault(), "the nested fault was dropped");
        assert!(is_internal(&r), "{r:?}");
        // The nested host failure line and the worker's timeout line.
        assert_eq!(job.error_log_count(), 2);
    }

    fn failing_at(store: &Store, depth: u32) -> ExecCtx {
        let mut ctx = ctx_for(store);
        ctx.fail_runtime_at_depth = Some(depth);
        ctx
    }

    fn failing_for(store: &Store, key: &'static str) -> ExecCtx {
        let mut ctx = ctx_for(store);
        ctx.fail_runtime_for_key = Some(key);
        ctx
    }

    fn is_internal(r: &Result<String, ExecError>) -> bool {
        matches!(r, Err(ExecError::Internal(_)))
    }

    /// Run `source` against `ctx` and also return how many error lines the JS
    /// layer logged for the job.
    fn run_counting(ctx: ExecCtx, source: &str) -> (Result<String, ExecError>, u32) {
        let job = Arc::clone(&ctx.job);
        let r = run_ctx(ctx, source);
        (r, job.error_log_count())
    }

    #[test]
    fn top_level_runtime_failure_is_internal() {
        let r = run_ctx(failing_at(&Store::new(), 0), "1");
        assert!(is_internal(&r), "{r:?}");
    }

    #[test]
    fn nested_runtime_failure_is_internal() {
        let store = Store::new();
        store.insert("inner".into(), "1".into());
        let r = run_ctx(failing_at(&store, 1), "Deno.core.ops.op_rs3_patch('inner')");
        assert!(is_internal(&r), "{r:?}");
    }

    #[test]
    fn caught_nested_runtime_failure_is_still_internal_and_refuses_later_writes() {
        let store = Store::new();
        store.insert("inner".into(), "1".into());
        store.insert("keep".into(), "k".into());
        let src = "Deno.core.ops.op_rs3_insert(JSON.stringify({before: '1'})); \
                   try { Deno.core.ops.op_rs3_patch('inner'); 'no throw' } \
                   catch (e) { \
                     try { Deno.core.ops.op_rs3_insert(JSON.stringify({after: e.message})) } \
                     catch (e2) {} \
                     try { Deno.core.ops.op_rs3_delete('keep') } catch (e2) {} \
                     'caught' }";
        let r = run_ctx(failing_at(&store, 1), src);
        assert!(is_internal(&r), "{r:?}");
        // No transactions: the write before the fault stays.
        assert_eq!(store.get("before"), Ok("1".to_owned()));
        assert!(store.get("after").is_err());
        assert_eq!(store.get("keep"), Ok("k".to_owned()));
    }

    #[test]
    fn every_op_throws_internal_error_once_the_job_faulted() {
        let store = Store::new();
        store.insert("k".into(), "v".into());
        store.insert("inner".into(), "1".into());
        let ctx = ctx_for(&store);
        ctx.note_host_fault();
        // Pre-setting the flag is test-only: in production no run starts on a
        // faulted job. The fault predates this run, so its completion value
        // comes back.
        let src = r#"const g = (f) => { try { f(); return 'no throw'; }
                       catch (e) { return e.name + ': ' + e.message; } };
                     [g(() => Deno.core.ops.op_rs3_list()),
                      g(() => Deno.core.ops.op_rs3_get('k')),
                      g(() => Deno.core.ops.op_rs3_insert('{"w":"1"}')),
                      g(() => Deno.core.ops.op_rs3_delete('k')),
                      g(() => Deno.core.ops.op_rs3_patch('inner'))].join('|')"#;
        let refused = ["Error: internal error"; 5].join("|");
        assert_eq!(run_ctx(ctx, src), Ok(refused));
        assert_eq!(store.get("k"), Ok("v".to_owned()));
        assert!(store.get("w").is_err());
    }

    /// Pins the defence-in-depth branch in `run_script`, which production
    /// cannot reach while every op refuses after a fault.
    #[test]
    fn a_fault_recorded_before_a_run_is_not_blamed_on_it() {
        let ctx = ctx_for(&Store::new());
        ctx.note_host_fault();
        assert_eq!(run_ctx(ctx, "6*7"), ok("42"));
    }

    #[test]
    fn patch_after_a_caught_fault_is_refused_without_running() {
        let store = Store::new();
        store.insert("bad".into(), "1".into());
        store.insert(
            "good".into(),
            r#"Deno.core.ops.op_rs3_insert('{"good_ran":"1"}')"#.into(),
        );
        let src = "try { Deno.core.ops.op_rs3_patch('bad') } catch (e) {} \
                   Deno.core.ops.op_rs3_patch('good')";
        let r = run_ctx(failing_for(&store, "bad"), src);
        assert!(is_internal(&r), "{r:?}");
        assert!(store.get("good_ran").is_err());
        // The same chain without the injected failure runs `good`.
        assert_eq!(run_ctx(ctx_for(&store), src), ok(r#"{"good_ran":"1"}"#));
        assert_eq!(store.get("good_ran"), Ok("1".to_owned()));
    }

    #[test]
    fn deeper_nested_runtime_failure_is_internal() {
        let store = Store::new();
        store.insert("mid".into(), "Deno.core.ops.op_rs3_patch('inner')".into());
        store.insert("inner".into(), "1".into());
        let src = "try { Deno.core.ops.op_rs3_patch('mid') } catch (e) { 'caught' }";
        let r = run_ctx(failing_at(&store, 2), src);
        assert!(is_internal(&r), "{r:?}");
        // Without the injected failure the same chain succeeds.
        assert_eq!(
            run_ctx(ctx_for(&store), "Deno.core.ops.op_rs3_patch('mid')"),
            ok("1")
        );
    }

    #[test]
    fn one_nested_host_fault_is_logged_once_at_any_depth() {
        let store = Store::new();
        store.insert("a".into(), "Deno.core.ops.op_rs3_patch('b')".into());
        store.insert("b".into(), "Deno.core.ops.op_rs3_patch('c')".into());
        store.insert("c".into(), "1".into());
        // Uncaught at every level: each ancestor fails with the internal error.
        let (r, logs) = run_counting(failing_at(&store, 3), "Deno.core.ops.op_rs3_patch('a')");
        assert!(is_internal(&r), "{r:?}");
        assert_eq!(logs, 1);
        // Caught at the top: still one line, and still Internal.
        let src = "try { Deno.core.ops.op_rs3_patch('a') } catch (e) { 'caught' }";
        let (r, logs) = run_counting(failing_at(&store, 3), src);
        assert!(is_internal(&r), "{r:?}");
        assert_eq!(logs, 1);
        // A direct nested failure logs its one line too.
        let (r, logs) = run_counting(failing_at(&store, 1), "Deno.core.ops.op_rs3_patch('c')");
        assert!(is_internal(&r), "{r:?}");
        assert_eq!(logs, 1);
    }

    #[test]
    fn script_errors_without_a_host_fault_are_still_logged() {
        let store = Store::new();
        store.insert("inner".into(), "throw new Error('boom')".into());
        // The inner script error and the uncaught rethrow at the top.
        let (r, logs) = run_counting(ctx_for(&store), "Deno.core.ops.op_rs3_patch('inner')");
        assert!(matches!(r, Err(ExecError::Script { .. })), "{r:?}");
        assert_eq!(logs, 2);
    }

    /// The depth error a job stopped at `chain` (limit 4) answers.
    fn depth_err(chain: &[&str]) -> Result<String, ExecError> {
        Err(ExecError::DepthLimit {
            limit: 4,
            chain: chain.iter().map(|k| (*k).to_owned()).collect(),
        })
    }

    #[test]
    fn depth_stop_refuses_later_ops_and_keeps_earlier_writes() {
        let store = Store::new();
        store.insert("me".into(), "Deno.core.ops.op_rs3_patch('me')".into());
        store.insert("keep".into(), "k".into());
        let src = "Deno.core.ops.op_rs3_insert(JSON.stringify({before: '1'})); \
                   try { Deno.core.ops.op_rs3_patch('me') } \
                   catch (e) { \
                     try { Deno.core.ops.op_rs3_insert(JSON.stringify({after: e.message})) } \
                     catch (e2) {} \
                     try { Deno.core.ops.op_rs3_delete('keep') } catch (e2) {} \
                     try { Deno.core.ops.op_rs3_get('keep') } catch (e2) {} \
                     'caught' }";
        assert_eq!(
            run_js(&store, src),
            depth_err(&["test", "me", "me", "me", "me"])
        );
        // No transactions: the write before the stop stays.
        assert_eq!(store.get("before"), Ok("1".to_owned()));
        assert!(store.get("after").is_err(), "a write after the stop landed");
        assert_eq!(store.get("keep"), Ok("k".to_owned()));
    }

    #[test]
    fn every_op_throws_the_depth_error_after_the_stop() {
        let store = Store::new();
        store.insert("k".into(), "v".into());
        store.insert("inner".into(), "1".into());
        let ctx = ctx_for(&store);
        // Pre-setting the record is test-only. run_ctx is unclassified, so the
        // pre-set stop does not turn the completion value into an error.
        ctx.job.stop_depth(DepthStop {
            limit: 4,
            chain: vec!["a".to_owned(), "b".to_owned()],
        });
        let src = r#"const g = (f) => { try { f(); return 'no throw'; }
                       catch (e) { return e.name + ': ' + e.message; } };
                     [g(() => Deno.core.ops.op_rs3_list()),
                      g(() => Deno.core.ops.op_rs3_get('k')),
                      g(() => Deno.core.ops.op_rs3_insert('{"w":"1"}')),
                      g(() => Deno.core.ops.op_rs3_delete('k')),
                      g(() => Deno.core.ops.op_rs3_patch('inner'))].join('|')"#;
        let refused = ["Error: PATCH nesting depth limit (4) exceeded: a -> b"; 5].join("|");
        assert_eq!(run_ctx(ctx, src), Ok(refused));
        assert_eq!(store.get("k"), Ok("v".to_owned()));
        assert!(store.get("w").is_err());
        assert_eq!(store.len(), 2);
    }

    #[test]
    fn depth_limit_chain_names_a_missing_key() {
        let store = Store::new();
        store.insert("k1".into(), "Deno.core.ops.op_rs3_patch('k2')".into());
        store.insert("k2".into(), "Deno.core.ops.op_rs3_patch('k3')".into());
        store.insert("k3".into(), "Deno.core.ops.op_rs3_patch('ghost')".into());
        // Refused before any store read, so the absent key is named, not 404'd.
        assert_eq!(
            run_js(&store, "Deno.core.ops.op_rs3_patch('k1')"),
            depth_err(&["test", "k1", "k2", "k3", "ghost"])
        );
    }

    #[test]
    fn depth_stop_is_logged_once() {
        let store = Store::new();
        store.insert("me".into(), "Deno.core.ops.op_rs3_patch('me')".into());
        let want = DepthStop {
            limit: 4,
            chain: ["test", "me", "me", "me", "me"].map(str::to_owned).to_vec(),
        };
        for src in [
            "Deno.core.ops.op_rs3_patch('me')",
            "try { Deno.core.ops.op_rs3_patch('me') } catch (e) { 'caught' }",
        ] {
            let ctx = ctx_for(&store);
            let job = Arc::clone(&ctx.job);
            let (_, logs) = run_counting(ctx, src);
            assert_eq!(logs, 1, "{src}");
            assert_eq!(job.depth_stop(), Some(&want), "{src}");
        }
    }

    #[test]
    fn nested_patch_of_missing_key_throws() {
        let store = Store::new();
        let src = "try { Deno.core.ops.op_rs3_patch('ghost'); 'no throw' } \
                   catch (e) { e.message }";
        assert_eq!(run_js(&store, src), ok("key not found: ghost"));
    }

    /// A script that recurses until V8's stack guard throws (so the recursion
    /// is bounded), then calls `op_rs3_patch(next)` from as deep as it can
    /// get into the op: a frame whose call overflows in JS rethrows the
    /// `RangeError` so its parent retries with more room. Its completion
    /// value is `value: <nested result>` or `error: <message the op threw>`.
    /// Near the stack limit deno_core may throw an op error as its bare
    /// message string instead of an `Error` (its error builder overflowed),
    /// so both forms are read.
    fn patch_at_stack_limit(next: &str) -> String {
        format!(
            "let out = null; \
             function f() {{ try {{ f(); }} catch (e) {{ \
               if (out !== null) return; \
               try {{ out = 'value: ' + Deno.core.ops.op_rs3_patch('{next}'); }} \
               catch (e2) {{ if (e2 instanceof RangeError) throw e2; \
                 out = 'error: ' + (typeof e2 === 'string' ? e2 : e2.message); }} \
             }} }} \
             f(); out"
        )
    }

    const NO_STACK: &str = "error: RangeError: not enough stack to start a nested script";

    #[test]
    fn nested_boot_without_stack_is_a_catchable_script_error() {
        let store = Store::new();
        store.insert("leaf".into(), "'leaf'".into());
        store.insert("k".into(), "v".into());
        // One level, the script caught the error: not a host fault, and the
        // job's later ops still work.
        let ctx = ctx_for(&store);
        let job = Arc::clone(&ctx.job);
        let src = format!(
            "{}; out + '|' + Deno.core.ops.op_rs3_get('k')",
            patch_at_stack_limit("leaf")
        );
        assert_eq!(run_ctx(ctx, &src), Ok(format!("{NO_STACK}|v")));
        assert!(!job.host_fault());
        // Two keys: the PATCHed script nests normally, and its nested key
        // recurses to the limit before nesting again.
        store.insert("k1".into(), patch_at_stack_limit("leaf"));
        assert_eq!(
            run_js(&store, "Deno.core.ops.op_rs3_patch('k1')"),
            ok(NO_STACK)
        );
    }

    #[test]
    fn uncaught_nested_boot_without_stack_is_a_script_error() {
        let store = Store::new();
        store.insert("leaf".into(), "'leaf'".into());
        // Only stack RangeErrors are caught; the op's Error propagates.
        let src = "function f() { try { return f(); } catch (e) { \
                     if (!(e instanceof RangeError)) throw e; \
                     return Deno.core.ops.op_rs3_patch('leaf'); } } f()";
        // An `Error` object, or its bare message where deno_core's error
        // builder itself ran out of stack.
        let is_no_stack = |r: &Result<String, ExecError>| {
            matches!(r, Err(ExecError::Script { message }) if [
                "Uncaught Error: RangeError: not enough stack to start a nested script",
                "Uncaught RangeError: not enough stack to start a nested script",
            ]
            .contains(&message.as_str()))
        };
        let r = run_js(&store, src);
        assert!(is_no_stack(&r), "{r:?}");
        // The same on the 2 MiB stack floor.
        let small = dedicated_executor(JsSettings {
            workers: 1,
            queue_capacity: 4,
            stack_bytes: 0,
            ..JsSettings::default()
        });
        let r = block_on(small.execute(store, "test".to_owned(), src.to_owned()));
        assert!(is_no_stack(&r), "{r:?}");
    }

    #[test]
    fn exec_ctx_nested_increments_depth_and_shares_store() {
        let ctx = ExecCtx::new(Store::new(), Arc::default(), "test", 4);
        assert_eq!(ctx.depth, 0);
        assert_eq!(ctx.chain, ["test"]);
        let nested = ctx.nested("k");
        assert_eq!(nested.depth, 1);
        assert_eq!(nested.chain, ["test", "k"]);
        assert_eq!(nested.max_patch_depth, 4);
        assert!(Arc::ptr_eq(&ctx.job, &nested.job));
        nested.store.insert("k".into(), "v".into());
        assert_eq!(ctx.store.get("k"), Ok("v".to_owned()));
    }

    #[test]
    fn op_panic_is_disabled_and_catchable() {
        let store = Store::new();
        let src = "try { Deno.core.ops.op_panic('x'); 'no throw' } \
                   catch (e) { (e instanceof Error) + '|' + e.message }";
        assert_eq!(run_js(&store, src), ok("true|op is disabled"));
    }

    #[test]
    fn list_matches_shared_codec_sorted() {
        let store = Store::new();
        store.insert("b".into(), "2".into());
        store.insert("a".into(), "1".into());
        store.insert("B".into(), "3".into());
        let want = r#"[{"B" : "3"},{"a" : "1"},{"b" : "2"}]"#;
        assert_eq!(legacy::format_list(&store.list()), want);
        assert_eq!(run_js(&store, "Deno.core.ops.op_rs3_list()"), ok(want));
    }

    #[test]
    fn list_on_empty_store_is_brackets() {
        assert_eq!(
            run_js(&Store::new(), "Deno.core.ops.op_rs3_list()"),
            ok("[]")
        );
    }

    #[test]
    fn list_ignores_an_extra_argument() {
        let store = Store::new();
        store.insert("k".into(), "v".into());
        let plain = run_js(&store, "Deno.core.ops.op_rs3_list()");
        assert_eq!(plain, ok(r#"[{"k" : "v"}]"#));
        assert_eq!(run_js(&store, "Deno.core.ops.op_rs3_list('')"), plain);
    }

    #[test]
    fn get_missing_key_throws_catchable_error() {
        let src = "try { Deno.core.ops.op_rs3_get('nope'); 'no throw' } \
                   catch (e) { e.name + '|' + e.message + '|' + (e instanceof Error) }";
        assert_eq!(
            run_js(&Store::new(), src),
            ok("Error|key not found: nope|true")
        );
    }

    #[test]
    fn get_missing_key_uncaught_is_script_error() {
        assert_eq!(
            run_js(&Store::new(), "Deno.core.ops.op_rs3_get('nope')"),
            Err(ExecError::Script {
                message: "Uncaught Error: key not found: nope".to_owned()
            })
        );
    }

    #[test]
    fn get_coerces_its_argument() {
        let store = Store::new();
        store.insert("undefined".into(), "u".into());
        store.insert("5".into(), "five".into());
        assert_eq!(
            run_js(
                &store,
                "Deno.core.ops.op_rs3_get() + '|' + Deno.core.ops.op_rs3_get(5)"
            ),
            ok("u|five")
        );
    }

    #[test]
    fn numeric_key_never_aliases_the_empty_key() {
        let store = Store::new();
        store.insert("".into(), "empty".into());
        store.insert("5".into(), "five".into());
        assert_eq!(run_js(&store, "Deno.core.ops.op_rs3_delete(5)"), ok("five"));
        assert_eq!(store.get(""), Ok("empty".to_owned()));
        assert!(store.get("5").is_err());
    }

    #[test]
    fn object_and_symbol_arguments_throw_type_error() {
        let store = Store::new();
        store.insert("k".into(), "v".into());
        let src = "const g = (f) => { try { f(); return 'no throw'; } \
                   catch (e) { return e.name + ': ' + e.message; } }; \
                   let ran = false; \
                   const o = { toString() { ran = true; return 'k'; } }; \
                   [g(() => Deno.core.ops.op_rs3_get(o)), \
                    g(() => Deno.core.ops.op_rs3_delete(Symbol('k'))), \
                    'toString ran: ' + ran].join('|')";
        let msg = "TypeError: op argument must be a string or another primitive value";
        assert_eq!(
            run_js(&store, src),
            Ok(format!("{msg}|{msg}|toString ran: false"))
        );
        assert_eq!(store.get("k"), Ok("v".to_owned()));
    }

    #[test]
    fn get_twice_returns_the_same_value() {
        let store = Store::new();
        store.insert("k".into(), "v".into());
        let src = "const a = Deno.core.ops.op_rs3_get('k'); \
                   const b = Deno.core.ops.op_rs3_get('k'); a + '|' + b";
        assert_eq!(run_js(&store, src), ok("v|v"));
    }

    #[test]
    fn insert_writes_all_and_echoes() {
        let store = Store::new();
        let src = r#"Deno.core.ops.op_rs3_insert('{"a":"1","b":"2"}')"#;
        assert_eq!(run_js(&store, src), ok(r#"{"a":"1","b":"2"}"#));
        assert_eq!(store.get("a"), Ok("1".to_owned()));
        assert_eq!(store.get("b"), Ok("2".to_owned()));
        assert_eq!(store.len(), 2);
    }

    #[test]
    fn insert_same_body_twice_is_idempotent() {
        let store = Store::new();
        let src = r#"const x = Deno.core.ops.op_rs3_insert('{"a":"1"}');
                     const y = Deno.core.ops.op_rs3_insert('{"a":"1"}');
                     x === y ? x : 'differ: ' + x + ' / ' + y"#;
        assert_eq!(run_js(&store, src), ok(r#"{"a":"1"}"#));
        assert_eq!(store.get("a"), Ok("1".to_owned()));
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn insert_empty_object_is_noop() {
        let store = Store::new();
        assert_eq!(
            run_js(&store, "Deno.core.ops.op_rs3_insert('{}')"),
            ok("{}")
        );
        assert!(store.is_empty());
    }

    #[test]
    fn insert_malformed_throws_type_error_and_writes_nothing() {
        let store = Store::new();
        let src = "try { Deno.core.ops.op_rs3_insert('{bad'); 'no throw' } \
                   catch (e) { e.name + '|' + e.message }";
        let got = run_js(&store, src).expect("script result");
        assert!(got.starts_with("TypeError|malformed JSON:"), "{got}");
        assert!(store.is_empty());
    }

    #[test]
    fn insert_wrong_shape_throws_type_error_and_writes_nothing() {
        let store = Store::new();
        let src = r#"try { Deno.core.ops.op_rs3_insert('{"x":"1","y":2}'); 'no throw' }
                     catch (e) { e.name + '|' + e.message }"#;
        assert_eq!(
            run_js(&store, src),
            ok("TypeError|invalid input: value for key y must be a string")
        );
        assert!(store.get("x").is_err());
        assert!(store.is_empty());
    }

    #[test]
    fn delete_returns_old_value_then_throws() {
        let store = Store::new();
        store.insert("k".into(), "v".into());
        let src = "const a = Deno.core.ops.op_rs3_delete('k'); let b; \
                   try { Deno.core.ops.op_rs3_delete('k'); b = 'no throw'; } \
                   catch (e) { b = e.message; } a + '|' + b";
        assert_eq!(run_js(&store, src), ok("v|key not found: k"));
        assert!(store.is_empty());
    }

    #[test]
    fn print_to_either_stream_keeps_running() {
        let src = "Deno.core.print('out\\n'); \
                   Deno.core.print('x\\nerror: JS host failure: forged', true); 'ok'";
        assert_eq!(run_js(&Store::new(), src), ok("ok"));
    }

    /// A test sink: answers the queued write faults first, then accepts at most
    /// 3 bytes per write; answers the queued flush faults before flushing.
    struct FlakySink {
        written: Vec<u8>,
        write_faults: std::collections::VecDeque<std::io::ErrorKind>,
        flush_faults: std::collections::VecDeque<std::io::ErrorKind>,
        flush_calls: u32,
    }

    impl FlakySink {
        fn new(write_faults: &[std::io::ErrorKind], flush_faults: &[std::io::ErrorKind]) -> Self {
            FlakySink {
                written: Vec::new(),
                write_faults: write_faults.iter().copied().collect(),
                flush_faults: flush_faults.iter().copied().collect(),
                flush_calls: 0,
            }
        }
    }

    impl std::io::Write for FlakySink {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            if let Some(kind) = self.write_faults.pop_front() {
                return Err(kind.into());
            }
            let n = buf.len().min(3);
            self.written.extend_from_slice(&buf[..n]);
            Ok(n)
        }

        fn flush(&mut self) -> std::io::Result<()> {
            self.flush_calls += 1;
            match self.flush_faults.pop_front() {
                Some(kind) => Err(kind.into()),
                None => Ok(()),
            }
        }
    }

    #[test]
    fn print_retries_would_block() {
        use std::io::ErrorKind::{Interrupted, WouldBlock};
        let mut sink = FlakySink::new(&[Interrupted, WouldBlock, WouldBlock], &[WouldBlock]);
        let written = super::write_all_retrying(&mut sink, b"hello, world");
        let flushed = super::flush_retrying(&mut sink);
        assert!(written.is_ok(), "write gave up: {written:?}");
        assert!(flushed.is_ok(), "flush gave up: {flushed:?}");
        assert_eq!(sink.written, b"hello, world");
        assert_eq!(sink.flush_calls, 2);
    }

    #[test]
    fn print_gives_up_on_other_errors() {
        use std::io::ErrorKind::{BrokenPipe, WriteZero};
        let mut broken = FlakySink::new(&[BrokenPipe], &[]);
        let err = super::write_all_retrying(&mut broken, b"x").map_err(|e| e.kind());
        assert_eq!(err, Err(BrokenPipe));
        assert!(broken.written.is_empty());
        let mut broken_flush = FlakySink::new(&[], &[BrokenPipe]);
        let err = super::flush_retrying(&mut broken_flush).map_err(|e| e.kind());
        assert_eq!(err, Err(BrokenPipe));
        assert_eq!(broken_flush.flush_calls, 1);
        let mut full = ZeroSink;
        let err = super::write_all_retrying(&mut full, b"x").map_err(|e| e.kind());
        assert_eq!(err, Err(WriteZero));
    }

    /// A sink that accepts nothing: every write returns Ok(0).
    struct ZeroSink;

    impl std::io::Write for ZeroSink {
        fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
            Ok(0)
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn uncaught_op_panic_is_a_script_error() {
        let store = Store::new();
        assert_eq!(
            run_js(&store, "Deno.core.ops.op_panic('x')"),
            Err(ExecError::Script {
                message: "Uncaught Error: op is disabled".to_owned()
            })
        );
    }
}
