//! The Raft log, vote and commit marker, stored in the node's redb file.
use std::fmt::Debug;
use std::ops::RangeBounds;
use std::sync::Arc;

use openraft::storage::{LogFlushed, LogState, RaftLogReader, RaftLogStorage};
use openraft::{ErrorSubject, ErrorVerb, LogId, StorageError, Vote};
use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition};
use serde::{Serialize, de::DeserializeOwned};

use super::{TypeConfig, blocking};

type Entry = openraft::Entry<TypeConfig>;

const LOG: TableDefinition<u64, &[u8]> = TableDefinition::new("raft_log");
const META: TableDefinition<&str, &[u8]> = TableDefinition::new("raft_meta");

/// Log storage on a shared database; clones read the same log.
#[derive(Clone)]
pub struct RedbLog {
    db: Arc<Database>,
}

impl RedbLog {
    /// Log storage on `db`, creating its tables when missing.
    pub fn new(db: Arc<Database>) -> Result<Self, redb::Error> {
        let txn = db.begin_write()?;
        txn.open_table(LOG)?;
        txn.open_table(META)?;
        txn.commit()?;
        Ok(Self { db })
    }

    async fn read_meta<T: DeserializeOwned + Send + 'static>(
        &self,
        subject: ErrorSubject<u64>,
        key: &'static str,
    ) -> Result<Option<T>, Box<StorageError<u64>>> {
        let db = self.db.clone();
        blocking(subject, ErrorVerb::Read, move || {
            let txn = db.begin_read()?;
            let Some(bytes) = txn.open_table(META)?.get(key)? else {
                return Ok(None);
            };
            Ok(Some(serde_json::from_slice(bytes.value())?))
        })
        .await
    }

    async fn write_meta<T: Serialize + Send + 'static>(
        &self,
        subject: ErrorSubject<u64>,
        key: &'static str,
        value: T,
    ) -> Result<(), Box<StorageError<u64>>> {
        let db = self.db.clone();
        blocking(subject, ErrorVerb::Write, move || {
            let txn = db.begin_write()?;
            txn.open_table(META)?
                .insert(key, serde_json::to_vec(&value)?.as_slice())?;
            Ok(txn.commit()?)
        })
        .await
    }
}

/// Remove every log entry whose index is in `range`.
fn remove_range(
    table: &mut redb::Table<u64, &[u8]>,
    range: impl RangeBounds<u64>,
) -> Result<(), redb::Error> {
    let indexes = table
        .range(range)?
        .map(|e| e.map(|(index, _)| index.value()))
        .collect::<Result<Vec<_>, _>>()?;
    for index in indexes {
        table.remove(index)?;
    }
    Ok(())
}

impl RaftLogReader<TypeConfig> for RedbLog {
    async fn try_get_log_entries<RB: RangeBounds<u64> + Clone + Debug + Send>(
        &mut self,
        range: RB,
    ) -> Result<Vec<Entry>, StorageError<u64>> {
        let db = self.db.clone();
        let bounds = (range.start_bound().cloned(), range.end_bound().cloned());
        blocking(ErrorSubject::Logs, ErrorVerb::Read, move || {
            let txn = db.begin_read()?;
            let mut entries = vec![];
            for item in txn.open_table(LOG)?.range(bounds)? {
                entries.push(serde_json::from_slice(item?.1.value())?);
            }
            Ok(entries)
        })
        .await
        .map_err(|e| *e)
    }
}

impl RaftLogStorage<TypeConfig> for RedbLog {
    type LogReader = Self;

    async fn get_log_state(&mut self) -> Result<LogState<TypeConfig>, StorageError<u64>> {
        let last_purged_log_id = self
            .read_meta(ErrorSubject::Logs, "last_purged")
            .await
            .map_err(|e| *e)?;
        let last = self.try_get_log_entries(..).await?.pop();
        Ok(LogState {
            last_log_id: last.map(|e| e.log_id).or(last_purged_log_id),
            last_purged_log_id,
        })
    }

    async fn get_log_reader(&mut self) -> Self::LogReader {
        self.clone()
    }

    async fn save_vote(&mut self, vote: &Vote<u64>) -> Result<(), StorageError<u64>> {
        self.write_meta(ErrorSubject::Vote, "vote", *vote)
            .await
            .map_err(|e| *e)
    }

    async fn read_vote(&mut self) -> Result<Option<Vote<u64>>, StorageError<u64>> {
        self.read_meta(ErrorSubject::Vote, "vote")
            .await
            .map_err(|e| *e)
    }

    async fn save_committed(
        &mut self,
        committed: Option<LogId<u64>>,
    ) -> Result<(), StorageError<u64>> {
        self.write_meta(ErrorSubject::Logs, "committed", committed)
            .await
            .map_err(|e| *e)
    }

    async fn read_committed(&mut self) -> Result<Option<LogId<u64>>, StorageError<u64>> {
        Ok(self
            .read_meta::<Option<LogId<u64>>>(ErrorSubject::Logs, "committed")
            .await
            .map_err(|e| *e)?
            .flatten())
    }

    async fn append<I>(
        &mut self,
        entries: I,
        callback: LogFlushed<TypeConfig>,
    ) -> Result<(), StorageError<u64>>
    where
        I: IntoIterator<Item = Entry> + Send,
        I::IntoIter: Send,
    {
        let entries: Vec<Entry> = entries.into_iter().collect();
        let db = self.db.clone();
        let written = blocking(ErrorSubject::Logs, ErrorVerb::Write, move || {
            let txn = db.begin_write()?;
            {
                let mut table = txn.open_table(LOG)?;
                for entry in &entries {
                    table.insert(entry.log_id.index, serde_json::to_vec(entry)?.as_slice())?;
                }
            }
            Ok(txn.commit()?)
        })
        .await;
        callback.log_io_completed(
            written
                .as_ref()
                .map(|_| ())
                .map_err(|e| std::io::Error::other(e.to_string())),
        );
        written.map_err(|e| *e)
    }

    async fn truncate(&mut self, log_id: LogId<u64>) -> Result<(), StorageError<u64>> {
        let db = self.db.clone();
        blocking(ErrorSubject::Logs, ErrorVerb::Delete, move || {
            let txn = db.begin_write()?;
            remove_range(&mut txn.open_table(LOG)?, log_id.index..)?;
            Ok(txn.commit()?)
        })
        .await
        .map_err(|e| *e)
    }

    async fn purge(&mut self, log_id: LogId<u64>) -> Result<(), StorageError<u64>> {
        let db = self.db.clone();
        blocking(ErrorSubject::Logs, ErrorVerb::Delete, move || {
            let txn = db.begin_write()?;
            remove_range(&mut txn.open_table(LOG)?, ..=log_id.index)?;
            txn.open_table(META)?
                .insert("last_purged", serde_json::to_vec(&log_id)?.as_slice())?;
            Ok(txn.commit()?)
        })
        .await
        .map_err(|e| *e)
    }
}
