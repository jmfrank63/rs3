//! Application state.
//!
//! `AppState` is created once in `main` and injected into the HTTP handlers
//! (and, from Phase 3, into the JavaScript ops). It is a cheap-to-clone bundle
//! of shared handles and holds no web or JS types.
use std::sync::Arc;

use crate::config::Settings;
use crate::store::Store;

/// Shared state for every worker: one store and the settings loaded at startup.
#[derive(Clone, Debug)]
pub struct AppState {
    pub store: Store,
    pub settings: Arc<Settings>,
}

impl AppState {
    /// Pair a fresh, empty store with the given settings.
    pub fn new(settings: Settings) -> Self {
        Self {
            store: Store::new(),
            settings: Arc::new(settings),
        }
    }
}

/// A fresh state for one test: an empty store and fixed in-code settings
/// (`SERVER.HOST=test-host`, `SERVER.PORT=8080`), never read from the environment.
#[cfg(test)]
pub(crate) fn test_state() -> AppState {
    let map: ::config::Map<String, String> =
        [("SERVER.HOST", "test-host"), ("SERVER.PORT", "8080")]
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .collect();
    AppState::new(Settings::from_map(map).expect("valid test settings"))
}
