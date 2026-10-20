//! The JavaScript runtime layer: the store ops, the script runner and the
//! dedicated JS thread pool. This module is free of actix-web.
use deno_core::JsRuntime;

pub mod executor;
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

/// Sizing of the JS thread pool.
///
/// Built in code and never read from the environment: the Phase 2 config
/// allowlist admits only server.host and server.port. Env-configurable pool
/// settings are deferred.
#[derive(Clone, Debug)]
pub struct JsSettings {
    /// Number of `rs3-js-*` worker threads.
    pub workers: usize,
    /// Jobs that can wait in the queue before submitters wait too.
    pub queue_capacity: usize,
    /// Stack size of each worker thread in bytes.
    pub stack_bytes: usize,
}

impl Default for JsSettings {
    /// One worker per available core (at least 1), 16 queued jobs per worker,
    /// and 16 MiB of stack per worker.
    fn default() -> Self {
        let workers = std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get);
        Self {
            workers,
            queue_capacity: workers * 16,
            stack_bytes: 16 * 1024 * 1024,
        }
    }
}

#[cfg(test)]
use crate::store::Store;

/// The process-wide executor every test shares. It holds an executor, not a
/// store; stores stay per test through `ExecCtx`.
#[cfg(test)]
pub(crate) fn test_executor() -> executor::JsExecutor {
    static EXEC: std::sync::LazyLock<executor::JsExecutor> = std::sync::LazyLock::new(|| {
        // Unprotected in test builds through the dev-dependency feature.
        init_platform();
        executor::JsExecutor::start(&JsSettings::default()).expect("test JS executor")
    });
    EXEC.clone()
}

/// Drive `f` to completion on a fresh current_thread runtime.
#[cfg(test)]
pub(crate) fn block_on<F: std::future::Future>(f: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test tokio runtime")
        .block_on(f)
}

/// Run `source` against `store` on the shared test executor and return the
/// stringified completion value.
#[cfg(test)]
pub(crate) fn run_js(store: &Store, source: &str) -> Result<String, ExecError> {
    block_on(test_executor().execute(store.clone(), "test".to_owned(), source.to_owned()))
}
