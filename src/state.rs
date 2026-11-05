//! Application state.
//!
//! `AppState` is created once in `main` and injected into the HTTP handlers.
//! It is a cheap-to-clone bundle of shared handles and holds no web or V8
//! types. The store reaches handlers through `web::Data` and JS ops through
//! the per-job `ExecCtx`; `js` is the shared handle to the JS thread pool.
use std::sync::Arc;

use crate::config::Settings;
use crate::goals::Goals;
use crate::js::executor::JsExecutor;
use crate::store::Store;

/// Shared state for every worker: one store, the settings loaded at startup
/// and the handle to the JS thread pool.
#[derive(Clone)]
pub struct AppState {
    pub store: Store,
    pub settings: Arc<Settings>,
    pub js: JsExecutor,
    /// Generation goals. `None` when the server has no database.
    pub goals: Option<Goals>,
}

impl AppState {
    /// Bundle the store with the given settings and JS pool handle.
    pub fn new(store: Store, settings: Settings, js: JsExecutor) -> Self {
        Self {
            store,
            settings: Arc::new(settings),
            js,
            goals: None,
        }
    }
}

/// A fresh state for one test: an empty store, fixed in-code settings
/// (`SERVER.HOST=test-host`, `SERVER.PORT=8080`), never read from the
/// environment, and the JS pool shared by every test in the process.
#[cfg(test)]
pub(crate) fn test_state() -> AppState {
    let map: ::config::Map<String, String> =
        [("SERVER.HOST", "test-host"), ("SERVER.PORT", "8080")]
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .collect();
    AppState::new(
        Store::new(),
        Settings::from_map(map).expect("valid test settings"),
        crate::js::test_executor(),
    )
}
