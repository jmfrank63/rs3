//! The store ops scripts call as `Deno.core.ops.op_rs3_*`. They reach the
//! injected `Store` only through `OpState`, never panic, and are free of
//! actix-web.
use deno_core::{OpState, op2};
use deno_error::JsErrorBox;

use crate::store::{Store, StoreError};

/// Per-job context the ops read from `OpState`.
#[derive(Clone, Debug)]
pub struct ExecCtx {
    /// The store of the caller that submitted the job.
    pub store: Store,
    /// How many `op_rs3_patch` levels deep this script runs (0 at the top).
    pub depth: u32,
}

impl ExecCtx {
    /// A top-level context for `store`.
    pub fn new(store: Store) -> Self {
        Self { store, depth: 0 }
    }

    /// The context for a script run by `op_rs3_patch`: same store, one level
    /// deeper. Phase 5 enforces a limit on the depth.
    pub fn nested(&self) -> Self {
        Self {
            store: self.store.clone(),
            depth: self.depth + 1,
        }
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

/// Returns the value stored under `key`. Throws `Error: key not found: <key>`
/// when the key is missing.
#[op2]
#[string]
fn op_rs3_get(state: &mut OpState, #[string] key: String) -> Result<String, JsErrorBox> {
    state.borrow::<ExecCtx>().store.get(&key).map_err(to_js)
}

/// Runs the script stored under `key` in a fresh nested runtime on the same
/// thread and returns `String(completion value)`. Throws
/// `Error: key not found: <key>` when the key is missing, and an `Error` with
/// the nested exception message (one leading "Uncaught " removed) when the
/// nested script fails.
//
// `reentrant` is mandatory: the nested runtime calls ops while this op is on
// the stack, and debug builds abort without the flag. The depth is carried for
// the Phase 5 limit. Until then a self-patching script is a known Phase 5 DoS
// item and is never tested here.
#[op2(reentrant)]
#[string]
fn op_rs3_patch(state: &mut OpState, #[string] key: String) -> Result<String, JsErrorBox> {
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
        other => JsErrorBox::generic(other.to_string()),
    })
}

deno_core::extension!(
    rs3_ext,
    ops = [op_rs3_get, op_rs3_patch],
    options = { ctx: ExecCtx },
    // op_panic is a deno_core built-in that panics inside an op, which aborts
    // the whole process. Disabled ops throw a catchable "op is disabled".
    middleware = |op| match op.name {
        "op_panic" => op.disable(),
        _ => op,
    },
    state = |state, options| {
        state.put(options.ctx);
    },
);

#[cfg(test)]
mod tests {
    use super::ExecCtx;
    use crate::js::{ExecError, run_js};
    use crate::store::Store;

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
