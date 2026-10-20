//! Runs one script on a fresh `JsRuntime` and stringifies its completion value.
//! This module is free of actix-web.
use deno_core::{JsRuntime, RuntimeOptions, v8};

use super::ExecError;
use super::ops::{ExecCtx, rs3_ext};

/// Run `source` as a script named `name` with the store ops bound to `ctx`, and
/// return `String(completion value)`.
///
/// Every call creates one fresh runtime and drops it before returning, so no
/// state leaks between scripts. It must be called on a thread with a tokio
/// runtime entered: deno_core aborts when V8 posts a delayed task without one.
/// A thrown exception, a syntax error or an uncaught op error becomes
/// `ExecError::Script` carrying the JS exception message; the stack goes to
/// stderr only.
pub fn run_script(ctx: ExecCtx, name: &str, source: String) -> Result<String, ExecError> {
    let mut rt = JsRuntime::try_new(RuntimeOptions {
        extensions: vec![rs3_ext::init(ctx)],
        ..Default::default()
    })
    .map_err(|e| ExecError::Internal(format!("cannot create JS runtime: {e}")))?;
    let global = rt.execute_script(name.to_owned(), source).map_err(|e| {
        // Debug-format the name so a key with control characters cannot forge log lines.
        eprintln!("js error in {name:?}: {e}");
        ExecError::Script {
            message: e.exception_message.clone(),
        }
    })?;
    // Read the value in the runtime's own main context; never build a new one.
    deno_core::scope!(scope, &mut rt);
    let local = v8::Local::new(scope, global);
    Ok(local.to_rust_string_lossy(scope))
}
