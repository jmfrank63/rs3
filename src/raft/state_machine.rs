//! The replicated state machine: the kv store plus the last applied log id and
//! membership, committed together so a restart resumes exactly where it stopped.
use std::io::Cursor;
use std::sync::{Arc, Mutex, PoisonError};

use openraft::storage::{RaftStateMachine, Snapshot, SnapshotMeta};
use openraft::{
    EntryPayload, ErrorSubject, ErrorVerb, LogId, RaftSnapshotBuilder, StorageError,
    StoredMembership,
};
use redb::{Database, ReadableDatabase, TableDefinition};
use serde::{Deserialize, Serialize};

use super::{Reply, TypeConfig, blocking};
use crate::journal::{self, Journal};
use crate::store::{Response, Store, StoreError, command};
use crate::{goals, lease, sessions};

type Entry = openraft::Entry<TypeConfig>;
type Membership = StoredMembership<u64, openraft::BasicNode>;

const SM: TableDefinition<&str, &[u8]> = TableDefinition::new("raft_sm");

/// Every table the state machine owns, as a snapshot carries it.
#[derive(Serialize, Deserialize)]
struct Image {
    kv: Vec<(String, String)>,
    goals: Vec<goals::Row>,
    journal: journal::Image,
    leases: lease::Image,
    sessions: Vec<(u64, Vec<u8>)>,
}

/// Applies committed entries to `store` and records how far it got.
#[derive(Clone)]
pub struct RedbStateMachine {
    db: Arc<Database>,
    store: Store,
    journal: Journal,
    /// The last snapshot built, which is what followers behind the purged log are sent.
    /// Serving it costs a copy; building one reads and serialises every table.
    built: Arc<Mutex<Built>>,
}

type Meta = SnapshotMeta<u64, openraft::BasicNode>;
type Built = Option<(Meta, Vec<u8>)>;

impl RedbStateMachine {
    /// A state machine over `store`, which must be durable on `db`.
    pub fn new(db: Arc<Database>, store: Store) -> Result<Self, crate::raft::BoxError> {
        let txn = db.begin_write()?;
        txn.open_table(SM)?;
        txn.commit()?;
        let journal = Journal::new(db.clone())?;
        Ok(Self {
            db,
            store,
            journal,
            built: Arc::default(),
        })
    }
}

fn read_sm(
    txn: &redb::ReadTransaction,
) -> Result<(Option<LogId<u64>>, Membership), crate::raft::BoxError> {
    let table = txn.open_table(SM)?;
    let read = |key| -> Result<_, crate::raft::BoxError> {
        Ok(table.get(key)?.map(|v| v.value().to_vec()))
    };
    let applied = read("last_applied")?
        .map(|b| serde_json::from_slice(&b))
        .transpose()?;
    let membership = read("membership")?
        .map(|b| serde_json::from_slice(&b))
        .transpose()?;
    Ok((applied.flatten(), membership.unwrap_or_default()))
}

fn write_sm(
    txn: &redb::WriteTransaction,
    applied: &LogId<u64>,
    membership: Option<&Membership>,
) -> Result<(), crate::raft::BoxError> {
    let mut table = txn.open_table(SM)?;
    table.insert(
        "last_applied",
        serde_json::to_vec(&Some(applied))?.as_slice(),
    )?;
    if let Some(membership) = membership {
        table.insert("membership", serde_json::to_vec(membership)?.as_slice())?;
    }
    Ok(())
}

impl RaftStateMachine<TypeConfig> for RedbStateMachine {
    type SnapshotBuilder = Self;

    async fn applied_state(
        &mut self,
    ) -> Result<(Option<LogId<u64>>, Membership), StorageError<u64>> {
        let db = self.db.clone();
        blocking(ErrorSubject::StateMachine, ErrorVerb::Read, move || {
            read_sm(&db.begin_read()?)
        })
        .await
        .map_err(|e| *e)
    }

    async fn apply<I>(&mut self, entries: I) -> Result<Vec<Reply>, StorageError<u64>>
    where
        I: IntoIterator<Item = Entry> + Send,
        I::IntoIter: Send,
    {
        let entries: Vec<Entry> = entries.into_iter().collect();
        let (db, store, journal) = (self.db.clone(), self.store.clone(), self.journal.clone());
        blocking(ErrorSubject::StateMachine, ErrorVerb::Write, move || {
            let mut replies = vec![];
            for entry in entries {
                let txn = db.begin_write()?;
                let mut membership = None;
                let mut effects = None;
                let reply = match &entry.payload {
                    EntryPayload::Normal(cmd) => {
                        match command::apply_in(&store, &journal, &txn, cmd) {
                            Ok((reply, done)) => {
                                effects = done;
                                Ok(reply)
                            }
                            Err(StoreError::NotFound(key)) => Err(key),
                            Err(e) => return Err(e.into()),
                        }
                    }
                    EntryPayload::Membership(m) => {
                        membership = Some(StoredMembership::new(Some(entry.log_id), m.clone()));
                        Ok(Response::Put)
                    }
                    EntryPayload::Blank => Ok(Response::Put),
                };
                // A refusal leaves no write behind, even a partial one.
                let txn = if matches!(&reply, Ok(r) if r.is_refusal()) {
                    drop(txn);
                    db.begin_write()?
                } else {
                    txn
                };
                write_sm(&txn, &entry.log_id, membership.as_ref())?;
                txn.commit()?;
                if let Some(effects) = &effects {
                    store.publish(effects);
                }
                if let (EntryPayload::Normal(cmd), Ok(response)) = (&entry.payload, &reply) {
                    store.publish_goal(cmd, response);
                }
                replies.push(reply);
            }
            Ok(replies)
        })
        .await
        .map_err(|e| *e)
    }

    async fn get_snapshot_builder(&mut self) -> Self::SnapshotBuilder {
        self.clone()
    }

    async fn begin_receiving_snapshot(
        &mut self,
    ) -> Result<Box<Cursor<Vec<u8>>>, StorageError<u64>> {
        Ok(Box::new(Cursor::new(vec![])))
    }

    async fn install_snapshot(
        &mut self,
        meta: &SnapshotMeta<u64, openraft::BasicNode>,
        snapshot: Box<Cursor<Vec<u8>>>,
    ) -> Result<(), StorageError<u64>> {
        let (db, store, meta) = (self.db.clone(), self.store.clone(), meta.clone());
        let (built, bytes) = (self.built.clone(), snapshot.get_ref().clone());
        blocking(ErrorSubject::StateMachine, ErrorVerb::Write, move || {
            let image: Image = serde_json::from_slice(snapshot.get_ref())?;
            let txn = db.begin_write()?;
            Store::replace_in(&txn, &image.kv)?;
            goals::replace_in(&txn, &image.goals)?;
            image.journal.replace_in(&txn)?;
            image.leases.replace_in(&txn)?;
            sessions::replace_in(&txn, &image.sessions)?;
            if let Some(applied) = &meta.last_log_id {
                write_sm(&txn, applied, Some(&meta.last_membership))?;
            }
            txn.commit()?;
            // What was installed is newer than anything built before it.
            *built.lock().unwrap_or_else(PoisonError::into_inner) = Some((meta, bytes));
            store.publish_all(&image.kv);
            Ok(())
        })
        .await
        .map_err(|e| *e)
    }

    async fn get_current_snapshot(
        &mut self,
    ) -> Result<Option<Snapshot<TypeConfig>>, StorageError<u64>> {
        // Runs inside the apply loop, so it serves the last built snapshot rather than
        // building one; only a node that has built none since it started builds here.
        let built = self
            .built
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let snapshot = match built {
            Some((meta, data)) => Snapshot {
                meta,
                snapshot: Box::new(Cursor::new(data)),
            },
            None => self.build_snapshot().await?,
        };
        Ok(snapshot.meta.last_log_id.is_some().then_some(snapshot))
    }
}

impl RaftSnapshotBuilder<TypeConfig> for RedbStateMachine {
    async fn build_snapshot(&mut self) -> Result<Snapshot<TypeConfig>, StorageError<u64>> {
        let (db, built) = (self.db.clone(), self.built.clone());
        blocking(ErrorSubject::StateMachine, ErrorVerb::Read, move || {
            let txn = db.begin_read()?;
            let (last_log_id, last_membership) = read_sm(&txn)?;
            let snapshot_id = last_log_id.map_or_else(|| "empty".to_owned(), |id| id.to_string());
            let data = serde_json::to_vec(&Image {
                kv: Store::dump(&txn)?,
                goals: goals::dump_in(&txn)?,
                journal: journal::Image::dump_in(&txn)?,
                leases: lease::Image::dump_in(&txn)?,
                sessions: sessions::dump_in(&txn)?,
            })?;
            let meta = SnapshotMeta {
                last_log_id,
                last_membership,
                snapshot_id,
            };
            let mut built = built.lock().unwrap_or_else(PoisonError::into_inner);
            let index = |m: &Meta| m.last_log_id.map(|id| id.index);
            if built
                .as_ref()
                .is_none_or(|(old, _)| index(old) <= index(&meta))
            {
                *built = Some((meta.clone(), data.clone()));
            }
            drop(built);
            Ok(Snapshot {
                meta,
                snapshot: Box::new(Cursor::new(data)),
            })
        })
        .await
        .map_err(|e| *e)
    }
}
