//! JavaScript runtime seam. Phase 3 adds the store ops and the executor here.
use deno_core::JsRuntime;

/// Initialise the V8 platform once per process, on the main thread, before other threads spawn.
pub fn init_platform() {
    JsRuntime::init_platform(None);
}
