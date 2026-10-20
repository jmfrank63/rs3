//! In-memory key/value store.
//!
//! `Store` is a cheap-to-clone handle over one shared concurrent map. Every
//! method is synchronous and returns owned values, so no map guard ever escapes
//! a call. This crate module is free of HTTP, JS and global state.
use std::sync::Arc;

pub mod legacy;

/// Errors returned by store operations.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum StoreError {
    #[error("key not found: {0}")]
    NotFound(String),
    #[error("malformed JSON: {0}")]
    MalformedJson(String),
    #[error("invalid input: {0}")]
    InvalidInput(String),
}

/// Shared handle to the key/value map. Clones share the same map.
#[derive(Clone, Default, Debug)]
pub struct Store {
    map: Arc<papaya::HashMap<String, String>>,
}

// Every method takes its own `pin()` guard and drops it before returning, so a
// guard never escapes and a re-entrant caller (a future JS op) cannot deadlock.
impl Store {
    /// An empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// The value stored under `key`, or `NotFound`.
    pub fn get(&self, key: &str) -> Result<String, StoreError> {
        self.map
            .pin()
            .get(key)
            .cloned()
            .ok_or_else(|| StoreError::NotFound(key.to_owned()))
    }

    /// Store `value` under `key`, returning the value it replaced, if any.
    pub fn insert(&self, key: String, value: String) -> Option<String> {
        self.map.pin().insert(key, value).cloned()
    }

    /// Write every pair under one map guard and return the number written.
    ///
    /// This cannot fail: callers validate the whole batch before calling it
    /// (the HTTP handler does so through `legacy::parse_insert_body`), so an
    /// invalid body writes nothing. It is not atomic for concurrent readers: a
    /// simultaneous `list()` or `get()` may observe a partially written batch.
    pub fn insert_all<I: IntoIterator<Item = (String, String)>>(&self, entries: I) -> usize {
        let map = self.map.pin();
        let mut count = 0;
        for (key, value) in entries {
            map.insert(key, value);
            count += 1;
        }
        count
    }

    /// Remove `key` and return the value it held, or `NotFound`.
    pub fn remove(&self, key: &str) -> Result<String, StoreError> {
        self.map
            .pin()
            .remove(key)
            .cloned()
            .ok_or_else(|| StoreError::NotFound(key.to_owned()))
    }

    /// Owned snapshot of every entry, sorted by key.
    pub fn list(&self) -> Vec<(String, String)> {
        let mut entries: Vec<(String, String)> = self
            .map
            .pin()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        // Keys are unique, so the unstable sort is deterministic.
        entries.sort_unstable_by(|a, b| a.0.cmp(&b.0));
        entries
    }

    /// Number of stored entries.
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Whether the store holds no entries.
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kv(k: &str, v: &str) -> (String, String) {
        (k.to_owned(), v.to_owned())
    }

    #[test]
    fn get_missing_is_not_found() {
        let store = Store::new();
        assert_eq!(
            store.get("missing"),
            Err(StoreError::NotFound("missing".into()))
        );
    }

    #[test]
    fn insert_then_get() {
        let store = Store::new();
        store.insert("a".into(), "1".into());
        assert_eq!(store.get("a"), Ok("1".to_owned()));
    }

    #[test]
    fn insert_returns_previous_value() {
        let store = Store::new();
        assert_eq!(store.insert("a".into(), "1".into()), None);
        assert_eq!(store.insert("a".into(), "2".into()), Some("1".to_owned()));
        assert_eq!(store.get("a"), Ok("2".to_owned()));
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn insert_all_writes_all_and_counts() {
        let store = Store::new();
        let n = store.insert_all([kv("a", "1"), kv("b", "2")]);
        assert_eq!(n, 2);
        assert_eq!(store.get("a"), Ok("1".to_owned()));
        assert_eq!(store.get("b"), Ok("2".to_owned()));
        assert_eq!(store.len(), 2);
        assert!(!store.is_empty());
    }

    #[test]
    fn remove_returns_value() {
        let store = Store::new();
        store.insert("a".into(), "1".into());
        store.insert("b".into(), "2".into());
        assert_eq!(store.len(), 2);
        assert_eq!(store.remove("a"), Ok("1".to_owned()));
        assert_eq!(store.get("a"), Err(StoreError::NotFound("a".into())));
        assert_eq!(store.get("b"), Ok("2".to_owned()));
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn remove_missing_is_not_found() {
        let store = Store::new();
        assert_eq!(
            store.remove("missing"),
            Err(StoreError::NotFound("missing".into()))
        );
        assert!(store.is_empty());
    }

    #[test]
    fn list_empty() {
        assert_eq!(Store::new().list(), Vec::<(String, String)>::new());
    }

    #[test]
    fn list_sorted_by_key() {
        let store = Store::new();
        store.insert("b".into(), "2".into());
        store.insert("a".into(), "1".into());
        store.insert("c".into(), "3".into());
        assert_eq!(store.list(), vec![kv("a", "1"), kv("b", "2"), kv("c", "3")]);
    }

    #[test]
    fn clones_share_state() {
        let store = Store::new();
        let clone = store.clone();
        store.insert("shared".into(), "yes".into());
        assert_eq!(clone.get("shared"), Ok("yes".to_owned()));
        assert_eq!(clone.len(), 1);
    }

    #[test]
    fn concurrent_inserts_are_all_visible() {
        let store = Store::new();
        std::thread::scope(|s| {
            for t in 0..4 {
                let store = store.clone();
                s.spawn(move || {
                    for i in 0..100 {
                        store.insert(format!("t{t}-k{i}"), format!("v{i}"));
                    }
                });
            }
        });
        assert_eq!(store.len(), 400);
        assert_eq!(store.get("t3-k99"), Ok("v99".to_owned()));
    }

    #[test]
    fn concurrent_remove_of_one_key_succeeds_once() {
        let store = Store::new();
        store.insert("k".into(), "v".into());
        let results: Vec<Result<String, StoreError>> = std::thread::scope(|s| {
            let handles: Vec<_> = (0..8)
                .map(|_| {
                    let store = store.clone();
                    s.spawn(move || store.remove("k"))
                })
                .collect();
            handles
                .into_iter()
                .map(|h| h.join().expect("remover thread panicked"))
                .collect()
        });
        let ok: Vec<&String> = results.iter().filter_map(|r| r.as_ref().ok()).collect();
        assert_eq!(ok, vec!["v"]);
        let not_found = results
            .iter()
            .filter(|r| **r == Err(StoreError::NotFound("k".into())))
            .count();
        assert_eq!(not_found, 7);
        assert!(store.is_empty());
    }
}
