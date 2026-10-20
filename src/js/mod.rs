//! The JavaScript runtime layer: the store ops, the script runner and the
//! dedicated JS thread pool. This module is free of actix-web.
use deno_core::JsRuntime;

pub mod ops;
pub mod runtime;

/// Initialise the V8 platform once per process, on the main thread, before other threads spawn.
pub fn init_platform() {
    JsRuntime::init_platform(None);
}

/// Errors from running a script.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ExecError {
    /// The script threw, failed to parse, or left an op error uncaught. The
    /// message is the JS exception message. HTTP maps this to 422 js_error.
    #[error("{message}")]
    Script { message: String },
    /// The executor is closed or dropped the reply. HTTP maps this to 500 internal.
    #[error("JS executor unavailable")]
    Unavailable,
    /// A host fault such as a runtime creation failure or a panicking job.
    /// HTTP maps this to 500 internal.
    #[error("JS host failure: {0}")]
    Internal(String),
}

#[cfg(test)]
use crate::store::Store;

/// Run `source` against `store` and return the stringified completion value.
#[cfg(test)]
pub(crate) fn run_js(store: &Store, source: &str) -> Result<String, ExecError> {
    init_platform();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test tokio runtime");
    let _enter = rt.enter();
    runtime::run_script(ops::ExecCtx::new(store.clone()), "test", source.to_owned())
}
