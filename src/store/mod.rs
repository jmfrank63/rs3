//! Key/value store, in memory or backed by a database file.
//!
//! `Store` is a cheap-to-clone handle over one shared concurrent map. Every
//! method is synchronous and returns owned values, so no map guard ever escapes
//! a call. This crate module is free of HTTP, JS and global state.
//!
//! A durable store writes through to disk. Reads only use the map, and a write
//! reaches the map after its commit has been flushed, so a reader never sees a
//! value that a crash could still lose. A failed write leaves the map unchanged.
//! Durable writes block on the disk, so async code must not call them directly.
use std::sync::{Arc, Mutex, PoisonError};

use redb::{Database, ReadableDatabase, ReadableTable, Table, TableDefinition};

pub mod legacy;

const KV: TableDefinition<&str, &str> = TableDefinition::new("kv");

/// Where writes go besides the map.
#[derive(Clone, Default)]
enum Persistence {
    #[default]
    Memory,
    Durable(Arc<Database>),
    /// Every write fails, as with a full or broken disk.
    #[cfg(test)]
    Failing,
}

fn storage<E: Into<redb::Error>>(e: E) -> StoreError {
    StoreError::Storage(e.into().to_string())
}

/// Errors returned by store operations.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum StoreError {
    #[error("key not found: {0}")]
    NotFound(String),
    #[error("malformed JSON: {0}")]
    MalformedJson(String),
    #[error("invalid input: {0}")]
    InvalidInput(String),
    /// The write did not reach the disk, or a failed commit left its outcome
    /// unknown until the next start. The map was not changed.
    #[error("storage failure: {0}")]
    Storage(String),
}

/// Shared handle to the key/value map. Clones share the same map.
#[derive(Clone, Default)]
pub struct Store {
    map: Arc<papaya::HashMap<String, String>>,
    persistence: Persistence,
    /// Held across commit and map update, so the map applies writes in commit order.
    write_order: Arc<Mutex<()>>,
}

// Every method takes its own `pin()` guard and drops it before returning, so a
// guard never escapes and a re-entrant caller (a future JS op) cannot deadlock.
impl Store {
    /// An empty store that keeps its data in memory only.
    pub fn new() -> Self {
        Self::default()
    }

    /// A store backed by `db`, loaded with everything `db` already holds.
    pub fn durable(db: Arc<Database>) -> Result<Self, StoreError> {
        let txn = db.begin_write().map_err(storage)?;
        txn.open_table(KV).map_err(storage)?;
        txn.commit().map_err(storage)?;

        let store = Self {
            persistence: Persistence::Durable(db.clone()),
            ..Self::default()
        };
        let txn = db.begin_read().map_err(storage)?;
        let table = txn.open_table(KV).map_err(storage)?;
        {
            let map = store.map.pin();
            for entry in table.iter().map_err(storage)? {
                let (key, value) = entry.map_err(storage)?;
                map.insert(key.value().to_owned(), value.value().to_owned());
            }
        }
        Ok(store)
    }

    /// A store holding `entries` whose every write fails, for testing how
    /// callers handle that.
    #[cfg(test)]
    pub(crate) fn failing(entries: &[(&str, &str)]) -> Self {
        let store = Self::new();
        for (key, value) in entries {
            store.insert((*key).to_owned(), (*value).to_owned());
        }
        Self {
            persistence: Persistence::Failing,
            ..store
        }
    }

    /// Run `write` in one transaction and flush it. A memory store does nothing.
    fn commit(
        &self,
        write: impl FnOnce(&mut Table<'_, &'static str, &'static str>) -> Result<(), redb::StorageError>,
    ) -> Result<(), StoreError> {
        match &self.persistence {
            Persistence::Memory => Ok(()),
            Persistence::Durable(db) => {
                let txn = db.begin_write().map_err(storage)?;
                {
                    let mut table = txn.open_table(KV).map_err(storage)?;
                    write(&mut table).map_err(storage)?;
                }
                txn.commit().map_err(storage)
            }
            #[cfg(test)]
            Persistence::Failing => Err(StoreError::Storage("injected failure".to_owned())),
        }
    }

    /// Serialize writers. A panic while this was held cannot have left the map
    /// ahead of the disk, so a poisoned lock is safe to reuse.
    fn lock_writes(&self) -> std::sync::MutexGuard<'_, ()> {
        self.write_order
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
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
    #[cfg(test)]
    pub fn insert(&self, key: String, value: String) -> Option<String> {
        let _order = self.lock_writes();
        self.commit(|table| table.insert(key.as_str(), value.as_str()).map(|_| ()))
            .unwrap();
        self.map.pin().insert(key, value).cloned()
    }

    /// Write every pair in one transaction and return the number written. On an
    /// error nothing reaches the map.
    ///
    /// Callers validate the whole batch before calling this (the HTTP handler
    /// does so through `legacy::parse_insert_body`), so an invalid body writes
    /// nothing. The batch is atomic on disk but not for concurrent readers: a
    /// simultaneous `list()` or `get()` may observe a partially written batch.
    pub fn insert_all<I: IntoIterator<Item = (String, String)>>(
        &self,
        entries: I,
    ) -> Result<usize, StoreError> {
        let entries: Vec<(String, String)> = entries.into_iter().collect();
        let _order = self.lock_writes();
        self.commit(|table| {
            for (key, value) in &entries {
                table.insert(key.as_str(), value.as_str())?;
            }
            Ok(())
        })?;
        let count = entries.len();
        let map = self.map.pin();
        for (key, value) in entries {
            map.insert(key, value);
        }
        Ok(count)
    }

    /// Remove `key` and return the value it held, or `NotFound`.
    pub fn remove(&self, key: &str) -> Result<String, StoreError> {
        let not_found = || StoreError::NotFound(key.to_owned());
        let _order = self.lock_writes();
        let map = self.map.pin();
        if !map.contains_key(key) {
            return Err(not_found());
        }
        self.commit(|table| table.remove(key).map(|_| ()))?;
        map.remove(key).cloned().ok_or_else(not_found)
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
        assert_eq!(n, Ok(2));
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

    fn durable_at(path: &std::path::Path) -> Store {
        Store::durable(crate::storage::open(path).unwrap()).unwrap()
    }

    #[test]
    fn durable_writes_survive_reopening() {
        let dir = crate::storage::TempDir::new();
        let path = dir.path().join("db");
        {
            let store = durable_at(&path);
            assert_eq!(
                store.insert_all([kv("a", "1"), kv("b", "2"), kv("c", "3")]),
                Ok(3)
            );
            assert_eq!(store.insert_all([kv("a", "changed")]), Ok(1));
            assert_eq!(store.remove("b"), Ok("2".to_owned()));
        }
        assert_eq!(
            durable_at(&path).list(),
            vec![kv("a", "changed"), kv("c", "3")]
        );
    }

    #[test]
    fn durable_remove_of_a_missing_key_is_not_found() {
        let dir = crate::storage::TempDir::new();
        let store = durable_at(&dir.path().join("db"));
        assert_eq!(
            store.remove("nope"),
            Err(StoreError::NotFound("nope".into()))
        );
    }

    #[test]
    fn memory_and_disk_agree_after_concurrent_writes() {
        let dir = crate::storage::TempDir::new();
        let path = dir.path().join("db");
        let store = durable_at(&path);
        std::thread::scope(|scope| {
            for writer in 0..4 {
                let store = store.clone();
                scope.spawn(move || {
                    for i in 0..10 {
                        store
                            .insert_all([kv("shared", &format!("{writer}-{i}"))])
                            .unwrap();
                    }
                });
            }
        });
        let in_memory = store.list();
        drop(store);
        assert_eq!(durable_at(&path).list(), in_memory);
    }

    #[test]
    fn failed_writes_leave_the_map_unchanged() {
        let store = Store::failing(&[("a", "1")]);
        assert!(matches!(
            store.insert_all([kv("a", "2"), kv("b", "3")]),
            Err(StoreError::Storage(_))
        ));
        assert!(matches!(store.remove("a"), Err(StoreError::Storage(_))));
        assert_eq!(store.list(), vec![kv("a", "1")]);
    }
}
