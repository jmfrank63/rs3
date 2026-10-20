//! The store ops scripts call as `Deno.core.ops.op_rs3_*`. They reach the
//! injected `Store` only through `OpState`, never panic, and are free of
//! actix_web.
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

deno_core::extension!(
    rs3_ext,
    ops = [op_rs3_get],
    options = { ctx: ExecCtx },
    state = |state, options| {
        state.put(options.ctx);
    },
);

#[cfg(test)]
mod tests {
    use crate::js::run_js;
    use crate::store::Store;

    #[test]
    fn run_script_reads_the_store() {
        let store = Store::new();
        store.insert("num".into(), "5".into());
        assert_eq!(
            run_js(&store, "Deno.core.ops.op_rs3_get('num')"),
            Ok("5".to_owned())
        );
    }
}
