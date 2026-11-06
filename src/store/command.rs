//! Every durable change to the store, as data.
use redb::WriteTransaction;
use serde::{Deserialize, Serialize};

use super::{Effects, Store, StoreError};
use crate::goals;
use crate::journal::{Journal, JournalError, Record};
use crate::lease::{self, Fence};

/// A change to the key/value table. `Store::apply` is the one place it is carried out.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum Command {
    /// Write every pair in one transaction.
    Put { entries: Vec<(String, String)> },
    /// Remove one key; a missing key is `NotFound`.
    Remove { key: String },
    /// Create a goal whose stream starts with `start`; its id is the next free one.
    CreateGoal {
        priority: i64,
        budget: u64,
        start: Record,
    },
    /// End a live goal durably.
    CancelGoal { id: u64 },
    /// Take over `task` as `node`; refused unless the task is assigned to `node`.
    Claim { task: u64, node: u64 },
    /// Assign `task` to `node` if its token is still `expect_token` (0: never claimed).
    Assign {
        task: u64,
        node: u64,
        expect_token: u64,
    },
    /// Append `records` from `seq` on under fence `token`, all or nothing.
    Append {
        task: u64,
        token: u64,
        seq: u64,
        records: Vec<Record>,
    },
    /// One script op under fence `token`: its `record` at `seq` and the `write` it made, a `Put`
    /// or `Remove`, commit together. A removal logs the value it removed, whatever `record` says.
    ScriptOp {
        task: u64,
        token: u64,
        seq: u64,
        record: Record,
        write: Option<Box<Command>>,
    },
}

/// What a `Command` answered.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum Response {
    Put,
    /// The value the removed key held.
    Removed(String),
    GoalCreated(u64),
    Cancelled,
    /// The goal does not exist or has already ended.
    NotLive,
    Claimed {
        token: u64,
    },
    Assigned {
        token: u64,
    },
    Appended,
    /// A script op's `Remove` found no key; only the op's record was written.
    Missing,
    /// The fence was taken over; nothing was written.
    Fenced {
        task: u64,
        held: u64,
        current: u64,
    },
    /// The command was not allowed; nothing was written.
    Refused(String),
}

impl Response {
    /// Whether the command was turned down and must leave no write behind.
    pub fn is_refusal(&self) -> bool {
        matches!(self, Self::Fenced { .. } | Self::Refused(_))
    }
}

/// Turn a journal refusal into an answer; storage failures stay errors.
fn refusal(e: JournalError) -> Result<Response, StoreError> {
    let code = match e {
        JournalError::Fenced {
            task,
            held,
            current,
        } => {
            return Ok(Response::Fenced {
                task,
                held,
                current,
            });
        }
        JournalError::SeqConflict { .. } => "seq_conflict",
        JournalError::Ended { .. } => "ended",
        JournalError::InvalidRecord { .. } => "invalid_record",
        JournalError::Corrupt { .. } => "corrupt",
        JournalError::Storage(_) | JournalError::Unavailable(_) => return Err(journal(e)),
    };
    Ok(Response::Refused(code.into()))
}

fn append_in(
    journal_of: &Journal,
    txn: &WriteTransaction,
    fence: &Fence,
    seq: u64,
    records: &[Record],
) -> Result<Response, JournalError> {
    lease::check_in(txn, fence)?;
    for (i, record) in records.iter().enumerate() {
        journal_of.append_in(txn, fence.task, seq + i as u64, record)?;
    }
    Ok(Response::Appended)
}

fn journal(e: JournalError) -> StoreError {
    StoreError::Storage(e.to_string())
}

/// Carry out `cmd` in `txn`: the one apply function for every durable change. Key/value
/// effects come back for `Store::publish` to run after the commit.
pub(crate) fn apply_in(
    store: &Store,
    journal_of: &Journal,
    txn: &WriteTransaction,
    cmd: &Command,
) -> Result<(Response, Option<Effects>), StoreError> {
    match cmd {
        Command::Put { .. } | Command::Remove { .. } => {
            let effects = store.apply_in(txn, cmd)?;
            Ok((effects.response(), Some(effects)))
        }
        Command::CreateGoal {
            priority,
            budget,
            start,
        } => goals::create_in(txn, journal_of, *priority, *budget, start)
            .map(|id| (Response::GoalCreated(id), None))
            .map_err(journal),
        Command::CancelGoal { id } => goals::cancel_in(txn, journal_of, *id)
            .map(|done| {
                (
                    if done {
                        Response::Cancelled
                    } else {
                        Response::NotLive
                    },
                    None,
                )
            })
            .map_err(journal),
        Command::Claim { .. } | Command::Assign { .. } | Command::Append { .. } => {
            lease_in(journal_of, txn, cmd).map(|answer| (answer, None))
        }
        Command::ScriptOp {
            task,
            token,
            seq,
            record,
            write,
        } => {
            let fence = Fence {
                task: *task,
                token: *token,
            };
            script_op_in(
                store,
                journal_of,
                txn,
                &fence,
                *seq,
                record,
                write.as_deref(),
            )
        }
    }
}

/// Carry out a script op in `txn`: check the fence, make the write, then log the op. A removal
/// of a missing key writes nothing and logs the record as given, which holds the refusal the
/// script saw. A fence or seq refusal leaves nothing behind, because the caller drops `txn`.
fn script_op_in(
    store: &Store,
    journal_of: &Journal,
    txn: &WriteTransaction,
    fence: &Fence,
    seq: u64,
    record: &Record,
    write: Option<&Command>,
) -> Result<(Response, Option<Effects>), StoreError> {
    if let Err(e) = lease::check_in(txn, fence) {
        return refusal(e).map(|answer| (answer, None));
    }
    let (effects, mut answer) = match write.map(|w| store.apply_in(txn, w)).transpose() {
        Ok(effects) => (effects, Response::Appended),
        Err(StoreError::NotFound(_)) => (None, Response::Missing),
        Err(e) => return Err(e),
    };
    let mut record = record.clone();
    if let (Some(Effects::Removed(_, old)), Record::OpResult { result, .. }) =
        (&effects, &mut record)
    {
        *result = Ok(old.clone());
        answer = Response::Removed(old.clone());
    }
    match journal_of.append_in(txn, fence.task, seq, &record) {
        Ok(()) => Ok((answer, effects)),
        Err(e) => refusal(e).map(|answer| (answer, None)),
    }
}

/// Carry out a lease command in `txn`.
fn lease_in(
    journal_of: &Journal,
    txn: &WriteTransaction,
    cmd: &Command,
) -> Result<Response, StoreError> {
    match cmd {
        Command::Claim { task, node } => {
            let owned = lease::owner_in(txn, *task).map_err(journal)?;
            if owned != Some(*node) {
                return Ok(Response::Refused("not_owner".into()));
            }
            let fence = lease::claim_in(txn, *task, Some(*node)).map_err(journal)?;
            Ok(Response::Claimed { token: fence.token })
        }
        Command::Assign {
            task,
            node,
            expect_token,
        } => {
            if lease::token_in(txn, *task).map_err(journal)? != *expect_token {
                return Ok(Response::Refused("stale_assign".into()));
            }
            let fence = lease::claim_in(txn, *task, Some(*node)).map_err(journal)?;
            Ok(Response::Assigned { token: fence.token })
        }
        Command::Append {
            task,
            token,
            seq,
            records,
        } => {
            let fence = Fence {
                task: *task,
                token: *token,
            };
            append_in(journal_of, txn, &fence, *seq, records).or_else(refusal)
        }
        _ => Err(StoreError::InvalidInput("not a lease command".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{Store, StoreError};

    fn put(k: &str, v: &str) -> Command {
        Command::Put {
            entries: vec![(k.to_owned(), v.to_owned())],
        }
    }

    #[test]
    fn commands_and_responses_round_trip_through_json() {
        for cmd in [
            put("a", "1"),
            Command::Remove { key: "a".into() },
            Command::CreateGoal {
                priority: -1,
                budget: 3,
                start: Record::Cancelled,
            },
            Command::CancelGoal { id: 2 },
        ] {
            let json = serde_json::to_string(&cmd).unwrap();
            assert_eq!(serde_json::from_str::<Command>(&json).unwrap(), cmd);
        }
        for resp in [
            Response::Put,
            Response::Removed("v".into()),
            Response::GoalCreated(4),
            Response::Cancelled,
            Response::NotLive,
        ] {
            let json = serde_json::to_string(&resp).unwrap();
            assert_eq!(serde_json::from_str::<Response>(&json).unwrap(), resp);
        }
    }

    #[test]
    fn applied_commands_survive_reopening() {
        let dir = crate::storage::TempDir::new();
        let path = dir.path().join("db");
        let open = || Store::durable(crate::storage::open(&path).unwrap()).unwrap();
        {
            let store = open();
            assert_eq!(store.apply(&put("a", "1")), Ok(Response::Put));
            assert_eq!(store.apply(&put("b", "2")), Ok(Response::Put));
            assert_eq!(
                store.apply(&Command::Remove { key: "a".into() }),
                Ok(Response::Removed("1".into()))
            );
        }
        assert_eq!(open().list(), vec![("b".to_owned(), "2".to_owned())]);
    }

    #[test]
    fn removing_a_missing_key_is_not_found_and_changes_nothing() {
        let dir = crate::storage::TempDir::new();
        for store in [
            Store::new(),
            Store::durable(crate::storage::open(&dir.path().join("db")).unwrap()).unwrap(),
        ] {
            store.apply(&put("a", "1")).unwrap();
            assert_eq!(
                store.apply(&Command::Remove { key: "x".into() }),
                Err(StoreError::NotFound("x".into()))
            );
            assert_eq!(store.list(), vec![("a".to_owned(), "1".to_owned())]);
        }
    }

    #[test]
    fn a_store_alone_refuses_goal_commands() {
        let dir = crate::storage::TempDir::new();
        for store in [
            Store::new(),
            Store::durable(crate::storage::open(&dir.path().join("db")).unwrap()).unwrap(),
        ] {
            assert!(matches!(
                store.apply(&Command::CancelGoal { id: 1 }),
                Err(StoreError::InvalidInput(_))
            ));
        }
    }
}
