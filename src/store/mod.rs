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
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition};

pub mod command;
pub mod legacy;

pub use command::{Command, Response};

const KV: TableDefinition<&str, &str> = TableDefinition::new("kv");

/// What a committed command changed, for `publish` to apply to the map.
pub(crate) enum Effects {
    Put(Vec<(String, String)>),
    Removed(String, String),
}

impl Effects {
    fn response(&self) -> Response {
        match self {
            Effects::Put(_) => Response::Put,
            Effects::Removed(_, old) => Response::Removed(old.clone()),
        }
    }
}

/// A committed goal command, told to the goals of this node after the commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GoalEvent {
    Assigned {
        task: u64,
        node: u64,
    },
    Cancelled {
        task: u64,
    },
    /// A snapshot replaced every goal.
    Installed,
}

type GoalSink = Box<dyn Fn(GoalEvent) + Send + Sync>;

fn not_kv() -> StoreError {
    StoreError::InvalidInput("not a key/value command".to_owned())
}

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
    /// Who hears of committed goal commands; unset until `on_goal_event`.
    goal_sink: Arc<OnceLock<GoalSink>>,
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

    /// Carry out `cmd` in `txn`, touching the kv table only. Nothing reaches the
    /// map until `publish` runs with the returned effects, after the commit.
    pub(crate) fn apply_in(
        &self,
        txn: &redb::WriteTransaction,
        cmd: &Command,
    ) -> Result<Effects, StoreError> {
        let mut table = txn.open_table(KV).map_err(storage)?;
        match cmd {
            Command::Put { entries } => {
                for (key, value) in entries {
                    table
                        .insert(key.as_str(), value.as_str())
                        .map_err(storage)?;
                }
                Ok(Effects::Put(entries.clone()))
            }
            Command::Remove { key } => {
                let old = table
                    .remove(key.as_str())
                    .map_err(storage)?
                    .map(|old| old.value().to_owned());
                old.map(|old| Effects::Removed(key.clone(), old))
                    .ok_or_else(|| StoreError::NotFound(key.clone()))
            }
            _ => Err(not_kv()),
        }
    }

    /// Tell `sink` of every goal command committed from now on. It runs on the apply thread, so
    /// it must only record and wake, never propose. Only the first sink is kept.
    pub fn on_goal_event(&self, sink: impl Fn(GoalEvent) + Send + Sync + 'static) {
        let _ = self.goal_sink.set(Box::new(sink));
    }

    /// Tell the goal sink what committed command `cmd` answered with `response`.
    pub(crate) fn publish_goal(&self, cmd: &Command, response: &Response) {
        let event = match (cmd, response) {
            (Command::Assign { task, node, .. }, Response::Assigned { .. }) => {
                GoalEvent::Assigned {
                    task: *task,
                    node: *node,
                }
            }
            (Command::CancelGoal { id }, Response::Cancelled) => GoalEvent::Cancelled { task: *id },
            _ => return,
        };
        if let Some(sink) = self.goal_sink.get() {
            sink(event);
        }
    }

    /// Make committed `effects` visible to readers.
    pub(crate) fn publish(&self, effects: &Effects) {
        let map = self.map.pin();
        match effects {
            Effects::Put(entries) => {
                for (key, value) in entries {
                    map.insert(key.clone(), value.clone());
                }
            }
            Effects::Removed(key, _) => {
                map.remove(key);
            }
        }
    }

    /// The value under `key` as of the write transaction `txn`, or `None`.
    pub(crate) fn get_in(
        txn: &redb::WriteTransaction,
        key: &str,
    ) -> Result<Option<String>, StoreError> {
        let table = txn.open_table(KV).map_err(storage)?;
        let value = table.get(key).map_err(storage)?;
        Ok(value.map(|v| v.value().to_owned()))
    }

    /// Every pair the kv table holds as of `txn`.
    pub(crate) fn dump(txn: &redb::ReadTransaction) -> Result<Vec<(String, String)>, StoreError> {
        let table = txn.open_table(KV).map_err(storage)?;
        let mut entries = vec![];
        for entry in table.iter().map_err(storage)? {
            let (key, value) = entry.map_err(storage)?;
            entries.push((key.value().to_owned(), value.value().to_owned()));
        }
        Ok(entries)
    }

    /// Make the kv table in `txn` hold exactly `entries`.
    pub(crate) fn replace_in(
        txn: &redb::WriteTransaction,
        entries: &[(String, String)],
    ) -> Result<(), StoreError> {
        txn.delete_table(KV).map_err(storage)?;
        let mut table = txn.open_table(KV).map_err(storage)?;
        for (key, value) in entries {
            table
                .insert(key.as_str(), value.as_str())
                .map_err(storage)?;
        }
        Ok(())
    }

    /// Make the map hold exactly the committed `entries`.
    pub(crate) fn publish_all(&self, entries: &[(String, String)]) {
        let map = self.map.pin();
        map.clear();
        for (key, value) in entries {
            map.insert(key.clone(), value.clone());
        }
        if let Some(sink) = self.goal_sink.get() {
            sink(GoalEvent::Installed);
        }
    }

    /// Apply `cmd`, then publish it. A durable store runs `hook` in the same
    /// transaction, so what it writes commits or rolls back with `cmd`. A memory
    /// store only publishes and runs no hook. On an error nothing reaches the map.
    fn run(
        &self,
        cmd: &Command,
        hook: impl FnOnce(&redb::WriteTransaction, &Effects) -> Result<(), StoreError>,
    ) -> Result<Effects, StoreError> {
        let _order = self.lock_writes();
        let effects = match &self.persistence {
            Persistence::Memory => match cmd {
                Command::Put { entries } => Effects::Put(entries.clone()),
                Command::Remove { key } => self
                    .map
                    .pin()
                    .get(key)
                    .map(|old| Effects::Removed(key.clone(), old.clone()))
                    .ok_or_else(|| StoreError::NotFound(key.clone()))?,
                _ => return Err(not_kv()),
            },
            Persistence::Durable(db) => {
                let txn = db.begin_write().map_err(storage)?;
                let effects = self.apply_in(&txn, cmd)?;
                hook(&txn, &effects)?;
                txn.commit().map_err(storage)?;
                effects
            }
            #[cfg(test)]
            Persistence::Failing => {
                return Err(StoreError::Storage("injected failure".to_owned()));
            }
        };
        self.publish(&effects);
        Ok(effects)
    }

    /// Carry out `cmd` and answer what it did.
    pub fn apply(&self, cmd: &Command) -> Result<Response, StoreError> {
        Ok(self.run(cmd, |_, _| Ok(()))?.response())
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
        let old = self.get(&key).ok();
        self.insert_all([(key, value)]).unwrap();
        old
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
        self.insert_all_with(entries, |_| Ok(()))
    }

    /// Like `insert_all`, and `hook` runs inside the commit transaction, so what
    /// it writes commits or rolls back together with the entries. A hook error
    /// writes nothing. A memory store runs no hook.
    pub fn insert_all_with<I: IntoIterator<Item = (String, String)>>(
        &self,
        entries: I,
        hook: impl FnOnce(&redb::WriteTransaction) -> Result<(), StoreError>,
    ) -> Result<usize, StoreError> {
        let entries: Vec<(String, String)> = entries.into_iter().collect();
        let count = entries.len();
        self.run(&Command::Put { entries }, |txn, _| hook(txn))?;
        Ok(count)
    }

    /// Remove `key` and return the value it held, or `NotFound`.
    pub fn remove(&self, key: &str) -> Result<String, StoreError> {
        self.remove_with(key, |_, _| Ok(()))
    }

    /// Like `remove`, and `hook` runs inside the commit transaction with the
    /// value being removed. A hook error removes nothing, and a missing key runs
    /// no hook. A memory store runs no hook.
    pub fn remove_with(
        &self,
        key: &str,
        hook: impl FnOnce(&redb::WriteTransaction, &str) -> Result<(), StoreError>,
    ) -> Result<String, StoreError> {
        let cmd = Command::Remove {
            key: key.to_owned(),
        };
        match self.run(&cmd, |txn, effects| match effects {
            Effects::Removed(_, old) => hook(txn, old),
            Effects::Put(_) => Ok(()),
        })? {
            Effects::Removed(_, old) => Ok(old),
            Effects::Put(_) => Ok(String::new()),
        }
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

    fn note(text: &str) -> crate::journal::Record {
        crate::journal::Record::ScriptStart {
            name: text.to_owned(),
            source: String::new(),
        }
    }

    fn op(name: &str) -> crate::journal::Record {
        crate::journal::Record::OpResult {
            op: name.to_owned(),
            args: vec![],
            result: Ok(String::new()),
        }
    }

    fn hook_failure() -> StoreError {
        StoreError::Storage("hook failed".to_owned())
    }

    fn with_journal(path: &std::path::Path) -> (Store, crate::journal::Journal) {
        let db = crate::storage::open(path).unwrap();
        (
            Store::durable(db.clone()).unwrap(),
            crate::journal::Journal::new(db).unwrap(),
        )
    }

    #[test]
    fn a_hook_record_commits_with_the_writes() {
        let dir = crate::storage::TempDir::new();
        let path = dir.path().join("db");
        {
            let (store, journal) = with_journal(&path);
            store
                .insert_all_with([kv("a", "1")], |txn| {
                    journal
                        .append_in(txn, 7, 0, &note("put"))
                        .map_err(|e| StoreError::Storage(e.to_string()))
                })
                .unwrap();
            store.insert_all([kv("b", "2")]).unwrap();
            let got = store
                .remove_with("b", |txn, old| {
                    assert_eq!(old, "2");
                    journal
                        .append_in(txn, 7, 1, &op("del"))
                        .map_err(|e| StoreError::Storage(e.to_string()))
                })
                .unwrap();
            assert_eq!(got, "2");
        }
        let (store, journal) = with_journal(&path);
        assert_eq!(store.list(), vec![kv("a", "1")]);
        assert_eq!(journal.read(7).unwrap(), vec![note("put"), op("del")]);
    }

    #[test]
    fn a_failing_hook_leaves_insert_unchanged() {
        let dir = crate::storage::TempDir::new();
        let path = dir.path().join("db");
        {
            let (store, journal) = with_journal(&path);
            store.insert_all([kv("a", "1")]).unwrap();
            let result = store.insert_all_with([kv("a", "2"), kv("b", "3")], |txn| {
                journal.append_in(txn, 7, 0, &note("x")).unwrap();
                Err(hook_failure())
            });
            assert_eq!(result, Err(hook_failure()));
            assert_eq!(store.list(), vec![kv("a", "1")]);
            assert_eq!(journal.read(7), Ok(vec![]));
        }
        let (store, journal) = with_journal(&path);
        assert_eq!(store.list(), vec![kv("a", "1")]);
        assert_eq!(journal.read(7), Ok(vec![]));
    }

    #[test]
    fn a_failing_hook_leaves_remove_unchanged() {
        let dir = crate::storage::TempDir::new();
        let path = dir.path().join("db");
        {
            let (store, journal) = with_journal(&path);
            store.insert_all([kv("a", "1")]).unwrap();
            let result = store.remove_with("a", |txn, _| {
                journal.append_in(txn, 7, 0, &note("x")).unwrap();
                Err(hook_failure())
            });
            assert_eq!(result, Err(hook_failure()));
            assert_eq!(store.get("a"), Ok("1".to_owned()));
            assert_eq!(journal.read(7), Ok(vec![]));
        }
        let (store, journal) = with_journal(&path);
        assert_eq!(store.get("a"), Ok("1".to_owned()));
        assert_eq!(journal.read(7), Ok(vec![]));
    }

    #[test]
    fn removing_a_missing_key_does_not_run_the_hook() {
        let dir = crate::storage::TempDir::new();
        let (store, _) = with_journal(&dir.path().join("db"));
        let result = store.remove_with("nope", |_, _| panic!("hook must not run"));
        assert_eq!(result, Err(StoreError::NotFound("nope".into())));
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
