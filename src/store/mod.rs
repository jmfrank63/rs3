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
    #[error("invalid input: {0}")]
    InvalidInput(String),
}

/// Shared handle to the key/value map. Clones share the same map.
#[derive(Clone, Default, Debug)]
pub struct Store {
    map: Arc<papaya::HashMap<String, String>>,
}

// RED stubs: signatures only, replaced by the GREEN implementation.
impl Store {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, key: &str) -> Result<String, StoreError> {
        Err(StoreError::NotFound(key.to_owned()))
    }

    pub fn insert(&self, _key: String, _value: String) -> Option<String> {
        None
    }

    pub fn insert_many<I>(&self, _entries: I) -> Result<usize, StoreError>
    where
        I: IntoIterator<Item = Result<(String, String), StoreError>>,
    {
        Ok(0)
    }

    pub fn len(&self) -> usize {
        0
    }

    pub fn is_empty(&self) -> bool {
        true
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
    fn insert_many_writes_all() {
        let store = Store::new();
        let n = store.insert_many([Ok(kv("a", "1")), Ok(kv("b", "2"))]);
        assert_eq!(n, Ok(2));
        assert_eq!(store.get("a"), Ok("1".to_owned()));
        assert_eq!(store.get("b"), Ok("2".to_owned()));
    }

    #[test]
    fn insert_many_all_or_nothing() {
        let store = Store::new();
        store.insert("keep".into(), "v".into());
        let before = store.len();
        let bad = || StoreError::InvalidInput("bad entry".into());
        let result = store.insert_many([Ok(kv("x", "1")), Err(bad()), Ok(kv("y", "2"))]);
        assert_eq!(result, Err(bad()));
        assert_eq!(store.get("x"), Err(StoreError::NotFound("x".into())));
        assert_eq!(store.get("y"), Err(StoreError::NotFound("y".into())));
        assert_eq!(store.len(), before);
        assert!(!store.is_empty());
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
}
