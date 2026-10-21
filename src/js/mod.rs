//! The JavaScript runtime layer: the store ops, the script runner and the
//! dedicated JS thread pool. This module is free of actix-web.
use std::sync::OnceLock;
#[cfg(test)]
use std::sync::atomic::AtomicU32;
use std::sync::atomic::{AtomicBool, Ordering};

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
    /// A PATCH chain went deeper than `max_patch_depth`. `chain` lists the keys
    /// in call order, from the PATCHed key to the refused one. HTTP maps this
    /// to 422 js_depth_limit.
    #[error("PATCH nesting depth limit ({limit}) exceeded: {}", .chain.join(" -> "))]
    DepthLimit { limit: u32, chain: Vec<String> },
}

/// The record of a job's depth stop: the limit in force and the refused chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DepthStop {
    /// The `max_patch_depth` that was exceeded.
    pub limit: u32,
    /// The keys in call order, ending with the refused key.
    pub chain: Vec<String>,
}

/// State shared by every level of one job: the outer script and every nested
/// script `op_rs3_patch` runs for it. The executor creates one per job and
/// classifies the job's result from it once, after the run. It is also the
/// job's one error log sink.
#[derive(Debug, Default)]
pub struct JobState {
    /// Set when a nested runtime hit a host fault.
    host_fault: AtomicBool,
    /// The first depth stop of the job; later stops never replace it.
    depth_stop: OnceLock<DepthStop>,
    /// Test hook: counts the error lines the JS layer wrote for this job.
    #[cfg(test)]
    error_logs: AtomicU32,
}

impl JobState {
    /// Record that a nested runtime hit a host fault during this job.
    pub(crate) fn note_host_fault(&self) {
        self.host_fault.store(true, Ordering::SeqCst);
    }

    /// Whether any level of this job hit a host fault.
    pub(crate) fn host_fault(&self) -> bool {
        self.host_fault.load(Ordering::SeqCst)
    }

    /// Record a depth stop. The first stop wins: it returns the stored record
    /// and whether this call set it.
    pub(crate) fn stop_depth(&self, stop: DepthStop) -> (&DepthStop, bool) {
        let mut newly_set = false;
        let record = self.depth_stop.get_or_init(|| {
            newly_set = true;
            stop
        });
        (record, newly_set)
    }

    /// The job's depth stop, if one was recorded.
    pub(crate) fn depth_stop(&self) -> Option<&DepthStop> {
        self.depth_stop.get()
    }

    /// Whether the job is stopped: a host fault or a depth stop was recorded.
    /// The request's answer is then decided, every op refuses, and the stop
    /// was already logged once where it happened.
    pub(crate) fn stopped(&self) -> bool {
        self.host_fault() || self.depth_stop.get().is_some()
    }

    /// Write one error line for this job to stderr. The caller escapes any
    /// script-controlled text in it.
    pub(crate) fn log_error(&self, line: std::fmt::Arguments<'_>) {
        eprintln!("{line}");
        #[cfg(test)]
        self.error_logs.fetch_add(1, Ordering::SeqCst);
    }

    /// How many error lines the JS layer wrote for this job.
    #[cfg(test)]
    pub(crate) fn error_log_count(&self) -> u32 {
        self.error_logs.load(Ordering::SeqCst)
    }
}

/// Sizing and limits of the JS thread pool.
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
    /// How many PATCH levels one job may run, counting the PATCHed key as
    /// level 1: with 4, a script may nest `op_rs3_patch` three times, and the
    /// fourth nested call is refused with `ExecError::DepthLimit`. Built in
    /// code and never read from the environment. The executor clamps it to
    /// 1..=64: 1 allows no nested PATCH, and 64 is the depth verified on the
    /// smallest worker stack (2 MiB); far above that V8's own stack guard would
    /// end the chain first, as a 500.
    pub max_patch_depth: u32,
}

impl Default for JsSettings {
    /// One worker per available core (at least 1), 16 queued jobs per worker,
    /// 16 MiB of stack per worker and a PATCH depth limit of 4.
    fn default() -> Self {
        let workers = std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get);
        Self {
            workers,
            queue_capacity: workers * 16,
            stack_bytes: 16 * 1024 * 1024,
            max_patch_depth: 4,
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
