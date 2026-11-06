//! Durable leases. A claim bumps a task's fencing token; a commit checks its
//! token inside its own transaction, so a stale holder can never commit.
use redb::{Database, ReadableTable, TableDefinition, WriteTransaction};

use crate::journal::{JournalError, storage};

const LEASES: TableDefinition<u64, u64> = TableDefinition::new("leases");

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
    let fence = claim_in(&txn, task)?;
    txn.commit().map_err(storage)?;
    Ok(fence)
}

/// Take over `task` inside `txn`, so the claim commits with the caller's writes.
pub fn claim_in(txn: &WriteTransaction, task: u64) -> Result<Fence, JournalError> {
    let mut leases = txn.open_table(LEASES).map_err(storage)?;
    let token = leases.get(task).map_err(storage)?.map_or(0, |t| t.value()) + 1;
    leases.insert(task, token).map_err(storage)?;
    Ok(Fence { task, token })
}

/// Refuse with `Fenced` unless `fence` is the latest claim, judged inside
/// `txn`, so the check and the writes that follow commit together.
pub fn check_in(txn: &WriteTransaction, fence: &Fence) -> Result<(), JournalError> {
    let leases = txn.open_table(LEASES).map_err(storage)?;
    let current = leases
        .get(fence.task)
        .map_err(storage)?
        .map_or(0, |t| t.value());
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
