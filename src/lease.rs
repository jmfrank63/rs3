//! Durable leases. A claim bumps a task's fencing token; a commit checks its
//! token inside its own transaction, so a stale holder can never commit.
use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition, WriteTransaction};

use crate::journal::{JournalError, storage};

const LEASES: TableDefinition<u64, u64> = TableDefinition::new("leases");
/// The node a task is assigned to; written only when a claim names an owner.
const OWNERS: TableDefinition<u64, u64> = TableDefinition::new("lease_owner");

/// Proof of a claim on `task`, valid until a later claim takes it over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fence {
    pub task: u64,
    pub token: u64,
}

/// Take over `task`, invalidating every earlier fence. There is no expiry: a
/// claim always succeeds.
pub fn claim(db: &Database, task: u64) -> Result<Fence, JournalError> {
    let txn = db.begin_write().map_err(storage)?;
    let fence = claim_in(&txn, task, None)?;
    txn.commit().map_err(storage)?;
    Ok(fence)
}

/// Take over `task` inside `txn`, so the claim commits with the caller's writes. A given
/// `owner` is recorded as the node the task belongs to.
pub fn claim_in(
    txn: &WriteTransaction,
    task: u64,
    owner: Option<u64>,
) -> Result<Fence, JournalError> {
    let token = token_in(txn, task)? + 1;
    txn.open_table(LEASES)
        .map_err(storage)?
        .insert(task, token)
        .map_err(storage)?;
    if let Some(owner) = owner {
        txn.open_table(OWNERS)
            .map_err(storage)?
            .insert(task, owner)
            .map_err(storage)?;
    }
    Ok(Fence { task, token })
}

/// The current token of `task` in `txn`; 0 when never claimed.
pub fn token_in(txn: &WriteTransaction, task: u64) -> Result<u64, JournalError> {
    let leases = txn.open_table(LEASES).map_err(storage)?;
    let token = leases.get(task).map_err(storage)?.map_or(0, |t| t.value());
    Ok(token)
}

/// The node `task` is assigned to in `txn`.
pub fn owner_in(txn: &WriteTransaction, task: u64) -> Result<Option<u64>, JournalError> {
    let owners = txn.open_table(OWNERS).map_err(storage)?;
    let owner = owners.get(task).map_err(storage)?.map(|o| o.value());
    Ok(owner)
}

/// The node `task` is assigned to, if any.
pub fn owner(db: &Database, task: u64) -> Result<Option<u64>, JournalError> {
    read(db, OWNERS, task)
}

/// The current token of `task`; 0 when never claimed.
pub fn token(db: &Database, task: u64) -> Result<u64, JournalError> {
    Ok(read(db, LEASES, task)?.unwrap_or(0))
}

fn read(
    db: &Database,
    table: TableDefinition<u64, u64>,
    task: u64,
) -> Result<Option<u64>, JournalError> {
    let txn = db.begin_read().map_err(storage)?;
    match txn.open_table(table) {
        Ok(t) => Ok(t.get(task).map_err(storage)?.map(|v| v.value())),
        Err(redb::TableError::TableDoesNotExist(_)) => Ok(None),
        Err(e) => Err(storage(e)),
    }
}

/// Refuse with `Fenced` unless `fence` is the latest claim, judged inside
/// `txn`, so the check and the writes that follow commit together.
pub fn check_in(txn: &WriteTransaction, fence: &Fence) -> Result<(), JournalError> {
    let current = token_in(txn, fence.task)?;
    if current != fence.token {
        return Err(JournalError::Fenced {
            task: fence.task,
            held: fence.token,
            current,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::journal::{Journal, Record};
    use crate::storage::{TempDir, open};

    fn start() -> Record {
        Record::InferenceStart {
            engine: "e".into(),
            model_hash: "m".into(),
            tokenizer_hash: "t".into(),
            sampler: "s".into(),
            rng_state: vec![1],
            prompt_tokens: vec![1],
        }
    }

    fn tokens(n: u32) -> Record {
        Record::Tokens {
            tokens: vec![n],
            rng_state: vec![n as u8],
        }
    }

    /// Append `record` at `seq` under `fence` in one transaction.
    fn fenced_append(
        db: &Database,
        journal: &Journal,
        fence: &Fence,
        seq: u64,
        record: &Record,
    ) -> Result<(), JournalError> {
        let txn = db.begin_write().map_err(storage)?;
        check_in(&txn, fence)?;
        journal.append_in(&txn, fence.task, seq, record)?;
        txn.commit().map_err(storage)
    }

    #[test]
    fn stale_holder_is_refused_at_the_current_sequence_number() {
        let dir = TempDir::new();
        let db = open(&dir.path().join("db")).unwrap();
        let journal = Journal::new(db.clone()).unwrap();
        journal.append(7, 0, &start()).unwrap();
        let a = claim(&db, 7).unwrap();
        let b = claim(&db, 7).unwrap();
        let before = journal.restore(7).unwrap().unwrap();

        let err = fenced_append(&db, &journal, &a, before.next_seq, &tokens(5)).unwrap_err();
        assert_eq!(
            err,
            JournalError::Fenced {
                task: 7,
                held: a.token,
                current: b.token
            }
        );
        assert_eq!(journal.restore(7).unwrap().unwrap(), before);

        fenced_append(&db, &journal, &b, before.next_seq, &tokens(5)).unwrap();
        assert_eq!(journal.restore(7).unwrap().unwrap().tokens, vec![5]);
    }

    #[test]
    fn latest_holder_commits_repeatedly() {
        let dir = TempDir::new();
        let db = open(&dir.path().join("db")).unwrap();
        let journal = Journal::new(db.clone()).unwrap();
        journal.append(1, 0, &start()).unwrap();
        let f = claim(&db, 1).unwrap();
        fenced_append(&db, &journal, &f, 1, &tokens(1)).unwrap();
        fenced_append(&db, &journal, &f, 2, &tokens(2)).unwrap();
        assert_eq!(journal.restore(1).unwrap().unwrap().tokens, vec![1, 2]);
    }

    #[test]
    fn tokens_count_up_across_a_reopen() {
        let dir = TempDir::new();
        let path = dir.path().join("db");
        let db = open(&path).unwrap();
        let first = claim(&db, 1).unwrap();
        assert_eq!(first.token, 1);
        let second = claim(&db, 1).unwrap();
        assert_eq!(second.token, 2);
        drop(db);

        let db = open(&path).unwrap();
        let txn = db.begin_write().unwrap();
        assert!(check_in(&txn, &second).is_ok());
        drop(txn);
        let third = claim(&db, 1).unwrap();
        assert_eq!(third.token, 3);
        let txn = db.begin_write().unwrap();
        assert!(matches!(
            check_in(&txn, &second),
            Err(JournalError::Fenced { .. })
        ));
    }

    #[test]
    fn unclaimed_and_forged_fences_are_refused() {
        let dir = TempDir::new();
        let db = open(&dir.path().join("db")).unwrap();
        let txn = db.begin_write().unwrap();
        let none = Fence { task: 3, token: 1 };
        assert_eq!(
            check_in(&txn, &none).unwrap_err(),
            JournalError::Fenced {
                task: 3,
                held: 1,
                current: 0
            }
        );
        drop(txn);
        let real = claim(&db, 3).unwrap();
        let forged = Fence {
            task: 3,
            token: real.token + 1,
        };
        let txn = db.begin_write().unwrap();
        assert_eq!(
            check_in(&txn, &forged).unwrap_err(),
            JournalError::Fenced {
                task: 3,
                held: 2,
                current: 1
            }
        );
    }

    #[test]
    fn an_owner_is_recorded_only_when_a_claim_names_one() {
        let dir = TempDir::new();
        let db = open(&dir.path().join("db")).unwrap();
        assert_eq!(owner(&db, 1).unwrap(), None);
        claim(&db, 1).unwrap();
        assert_eq!(owner(&db, 1).unwrap(), None);
        let txn = db.begin_write().unwrap();
        assert_eq!(claim_in(&txn, 1, Some(4)).unwrap().token, 2);
        assert_eq!(claim_in(&txn, 2, Some(5)).unwrap().token, 1);
        txn.commit().unwrap();
        assert_eq!(owner(&db, 1).unwrap(), Some(4));
        assert_eq!(owner(&db, 2).unwrap(), Some(5));
        claim(&db, 1).unwrap();
        assert_eq!(owner(&db, 1).unwrap(), Some(4));
    }

    #[test]
    fn claims_on_one_task_leave_another_valid() {
        let dir = TempDir::new();
        let db = open(&dir.path().join("db")).unwrap();
        let on2 = claim(&db, 2).unwrap();
        claim(&db, 1).unwrap();
        claim(&db, 1).unwrap();
        let txn = db.begin_write().unwrap();
        assert!(check_in(&txn, &on2).is_ok());
    }
}
