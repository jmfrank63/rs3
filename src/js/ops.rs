//! The store ops scripts call as `Deno.core.ops.op_rs3_*`. They reach the
//! injected `Store` only through `OpState`, never panic, and are free of
//! actix-web.
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use deno_core::{FromV8, OpState, op2, v8};
use deno_error::JsErrorBox;

use crate::store::{Store, StoreError, legacy};

/// Per-job context the ops read from `OpState`.
#[derive(Clone, Debug)]
pub struct ExecCtx {
    /// The store of the caller that submitted the job.
    pub store: Store,
    /// How many `op_rs3_patch` levels deep this script runs (0 at the top).
    pub depth: u32,
    /// Set when a nested runtime hit a host fault. Shared by every level of
    /// one job, so the top level answers a host fault even if a script caught
    /// the error `op_rs3_patch` threw.
    host_fault: Arc<AtomicBool>,
    /// Test hook: make runtime creation fail at this depth.
    #[cfg(test)]
    pub(crate) fail_runtime_at_depth: Option<u32>,
}

impl ExecCtx {
    /// A top-level context for `store`.
    pub fn new(store: Store) -> Self {
        Self {
            store,
            depth: 0,
            host_fault: Arc::new(AtomicBool::new(false)),
            #[cfg(test)]
            fail_runtime_at_depth: None,
        }
    }

    /// The context for a script run by `op_rs3_patch`: same store and host
    /// fault flag, one level deeper. Phase 5 enforces a limit on the depth.
    pub fn nested(&self) -> Self {
        Self {
            store: self.store.clone(),
            depth: self.depth + 1,
            host_fault: Arc::clone(&self.host_fault),
            #[cfg(test)]
            fail_runtime_at_depth: self.fail_runtime_at_depth,
        }
    }

    /// Record that a nested runtime hit a host fault during this job.
    pub(crate) fn note_host_fault(&self) {
        self.host_fault.store(true, Ordering::SeqCst);
    }

    /// Whether any level of this job hit a host fault.
    pub(crate) fn host_fault(&self) -> bool {
        self.host_fault.load(Ordering::SeqCst)
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

/// Returns the whole store as the `GET /list` text, `[{"k" : "v"},...]` sorted
/// by key, or `[]` when empty. Extra arguments are ignored. Never throws.
#[op2]
#[string]
fn op_rs3_list(state: &mut OpState) -> String {
    legacy::format_list(&state.borrow::<ExecCtx>().store.list())
}

/// Returns the value stored under `key`. Throws `Error: key not found: <key>`
/// when the key is missing.
#[op2]
#[string]
fn op_rs3_get(state: &mut OpState, #[scoped] key: StrArg) -> Result<String, JsErrorBox> {
    state.borrow::<ExecCtx>().store.get(&key.0).map_err(to_js)
}

/// Stores every pair of a JSON object of strings and returns the compact echo
/// `{"k":"v"}`. Throws `TypeError: malformed JSON: ..` or
/// `TypeError: invalid input: ..` and writes nothing when the body is rejected.
#[op2]
#[string]
fn op_rs3_insert(state: &mut OpState, #[scoped] body: StrArg) -> Result<String, JsErrorBox> {
    // Validate the whole body before the first write.
    let entries = legacy::parse_insert_body(&body.0).map_err(to_js)?;
    let echo = legacy::format_insert_echo(&entries);
    state.borrow::<ExecCtx>().store.insert_all(entries);
    Ok(echo)
}

/// Removes `key` and returns the value it held. Throws
/// `Error: key not found: <key>` when the key is missing.
#[op2]
#[string]
fn op_rs3_delete(state: &mut OpState, #[scoped] key: StrArg) -> Result<String, JsErrorBox> {
    state
        .borrow::<ExecCtx>()
        .store
        .remove(&key.0)
        .map_err(to_js)
}

/// Runs the script stored under `key` in a fresh nested runtime on the same
/// thread and returns `String(completion value)`. Throws
/// `Error: key not found: <key>` when the key is missing, and an `Error` with
/// the nested exception message (one leading "Uncaught " removed) when the
/// nested script fails. A host fault in the nested runtime (such as a runtime
/// creation failure) is logged, recorded in the job context and thrown as a
/// plain `Error: internal error`; the top-level run then answers
/// `ExecError::Internal` even if the script caught it.
//
// `reentrant` is mandatory: the nested runtime calls ops while this op is on
// the stack, and debug builds abort without the flag. The depth is carried for
// the Phase 5 limit. Until then a self-patching script is a known Phase 5 DoS
// item and is never tested here.
#[op2(reentrant)]
#[string]
fn op_rs3_patch(state: &mut OpState, #[scoped] key: StrArg) -> Result<String, JsErrorBox> {
    let StrArg(key) = key;
    // Clone the context out so no OpState borrow is held across the nested run.
    let ctx = state.borrow::<ExecCtx>().clone();
    let source = ctx.store.get(&key).map_err(to_js)?;
    super::runtime::run_script(ctx.nested(), &key, source).map_err(|e| match e {
        super::ExecError::Script { message } => JsErrorBox::generic(
            message
                .strip_prefix("Uncaught ")
                .unwrap_or(&message)
                .to_owned(),
        ),
        other => {
            // Host details go to stderr only, escaped so they cannot forge log lines.
            eprintln!(
                "error: nested JS host failure in {key:?} at depth {}: {:?}",
                ctx.depth + 1,
                other.to_string()
            );
            ctx.note_host_fault();
            JsErrorBox::generic("internal error")
        }
    })
}

/// Replaces the implementation of deno_core's built-in `op_print`, which backs
/// `Deno.core.print(text, isErr)`. Text for stdout passes through unchanged.
/// Text for stderr is written as one Debug-escaped line per call, prefixed with
/// `js stderr: `, so a script cannot forge server log lines. Never registered
/// under its own name; the signature must match `op_print` exactly.
#[op2(fast)]
fn op_rs3_print(#[string] msg: &str, is_err: bool) -> Result<(), std::io::Error> {
    use std::io::Write;
    if is_err {
        writeln!(std::io::stderr().lock(), "js stderr: {msg:?}")
    } else {
        let mut out = std::io::stdout().lock();
        out.write_all(msg.as_bytes())?;
        out.flush()
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
    use super::ExecCtx;
    use crate::js::runtime::run_script;
    use crate::js::{ExecError, block_on, run_js, test_executor};
    use crate::store::{Store, legacy};

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

    fn failing_at(store: &Store, depth: u32) -> ExecCtx {
        let mut ctx = ExecCtx::new(store.clone());
        ctx.fail_runtime_at_depth = Some(depth);
        ctx
    }

    fn is_internal(r: &Result<String, ExecError>) -> bool {
        matches!(r, Err(ExecError::Internal(_)))
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
    fn caught_nested_runtime_failure_is_still_internal_and_hides_details() {
        let store = Store::new();
        store.insert("inner".into(), "1".into());
        let src = "try { Deno.core.ops.op_rs3_patch('inner'); 'no throw' } \
                   catch (e) { Deno.core.ops.op_rs3_insert(JSON.stringify({seen: e.message})); \
                   'caught' }";
        let r = run_ctx(failing_at(&store, 1), src);
        assert!(is_internal(&r), "{r:?}");
        assert_eq!(store.get("seen"), Ok("internal error".to_owned()));
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
            run_ctx(
                ExecCtx::new(store.clone()),
                "Deno.core.ops.op_rs3_patch('mid')"
            ),
            ok("1")
        );
    }

    #[test]
    fn nested_patch_of_missing_key_throws() {
        let store = Store::new();
        let src = "try { Deno.core.ops.op_rs3_patch('ghost'); 'no throw' } \
                   catch (e) { e.message }";
        assert_eq!(run_js(&store, src), ok("key not found: ghost"));
    }

    #[test]
    fn exec_ctx_nested_increments_depth_and_shares_store() {
        let ctx = ExecCtx::new(Store::new());
        assert_eq!(ctx.depth, 0);
        let nested = ctx.nested();
        assert_eq!(nested.depth, 1);
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
