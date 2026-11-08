//! Every durable change to the store, as data.
use redb::WriteTransaction;
use serde::{Deserialize, Serialize};

use super::{Effects, Store, StoreError};
use crate::goals;
use crate::journal::{Journal, JournalError, Record, SealRecord};
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
    /// Record how the ended stream `stream` is split into fragments, one per member. A second
    /// seal of the same stream changes nothing.
    Seal {
        stream: u64,
        members: Vec<u64>,
        data: usize,
        parity: usize,
        len: usize,
        checksums: Vec<[u8; 32]>,
    },
    /// `node` has stored its fragment of `stream`. The apply of the last member's confirmation
    /// drops the stream's full copy.
    ConfirmFragment {
        stream: u64,
        node: u64,
        /// The fragment belongs to the pending re-encoding, not the current seal.
        #[serde(default)]
        pending: bool,
    },
    /// Re-encode the fully confirmed `stream` over other members. The new seal waits as pending
    /// until all its members confirm, then replaces the current one.
    Reseal {
        stream: u64,
        members: Vec<u64>,
        data: usize,
        parity: usize,
        len: usize,
        checksums: Vec<[u8; 32]>,
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
    Sealed,
    Confirmed,
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
        JournalError::Storage(_)
        | JournalError::Unavailable(_)
        | JournalError::FragmentsUnavailable { .. } => return Err(journal(e)),
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
        Command::Seal {
            stream,
            members,
            data,
            parity,
            len,
            checksums,
        }
        | Command::Reseal {
            stream,
            members,
            data,
            parity,
            len,
            checksums,
        } => {
            let seal = SealRecord {
                members: members.clone(),
                data: *data,
                parity: *parity,
                len: *len,
                checksums: checksums.clone(),
                confirmed: vec![],
                pending: None,
            };
            let done = if matches!(cmd, Command::Seal { .. }) {
                seal_in(journal_of, txn, *stream, &seal)
            } else {
                reseal_in(journal_of, txn, *stream, seal)
            };
            done.or_else(refusal).map(|answer| (answer, None))
        }
        Command::ConfirmFragment {
            stream,
            node,
            pending,
        } => confirm_in(journal_of, txn, *stream, *node, *pending)
            .or_else(refusal)
            .map(|answer| (answer, None)),
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

/// Store `seal` for `stream` unless it is sealed already; only an ended stream can be sealed.
fn seal_in(
    journal_of: &Journal,
    txn: &WriteTransaction,
    stream: u64,
    seal: &SealRecord,
) -> Result<Response, JournalError> {
    if journal_of.ended_in(txn, stream)? != Some(true) {
        return Ok(Response::Refused("not_ended".into()));
    }
    if !well_formed(seal) {
        return Ok(Response::Refused("bad_seal".into()));
    }
    // An unfinished seal may be replaced over other members: its records are still present.
    let replaces = match journal_of.seal_for_write(txn, stream)? {
        None => true,
        Some(old) => {
            old.members != seal.members && !old.members.iter().all(|m| old.confirmed.contains(m))
        }
    };
    if replaces {
        journal_of.put_seal_in(txn, stream, seal)?;
    }
    Ok(Response::Sealed)
}

/// A seal every node can honour: distinct members, at least 3, the scheme for their count, and a
/// checksum per member.
fn well_formed(seal: &SealRecord) -> bool {
    let n = seal.members.len();
    let distinct = seal
        .members
        .iter()
        .collect::<std::collections::HashSet<_>>()
        .len()
        == n;
    n >= 3
        && distinct
        && (seal.data, seal.parity) == crate::seal::scheme(n)
        && seal.checksums.len() == n
}

/// Queue `seal` as the re-encoding of the fully confirmed `stream`, unless the current or the
/// pending seal has these members already.
fn reseal_in(
    journal_of: &Journal,
    txn: &WriteTransaction,
    stream: u64,
    seal: SealRecord,
) -> Result<Response, JournalError> {
    let Some(mut cur) = journal_of.seal_for_write(txn, stream)? else {
        return Ok(Response::Refused("not_sealed".into()));
    };
    if !all_confirmed(&cur) {
        return Ok(Response::Refused("not_confirmed".into()));
    }
    if !well_formed(&seal) {
        return Ok(Response::Refused("bad_seal".into()));
    }
    let same = |s: &SealRecord| s.members == seal.members;
    if !same(&cur) && !cur.pending.as_deref().is_some_and(same) {
        cur.pending = Some(Box::new(seal));
        journal_of.put_seal_in(txn, stream, &cur)?;
    }
    Ok(Response::Sealed)
}

fn all_confirmed(seal: &SealRecord) -> bool {
    seal.members.iter().all(|m| seal.confirmed.contains(m))
}

/// Add `node` to the confirmed members of `seal` if it is one and has not confirmed yet.
fn confirm_member(seal: &mut SealRecord, node: u64) -> bool {
    let new = seal.members.contains(&node) && !seal.confirmed.contains(&node);
    if new {
        seal.confirmed.push(node);
    }
    new
}

/// Note that `node` stored its fragment of `stream`. The last member's confirmation of the
/// current seal drops the full copy; the last of the pending seal promotes it.
fn confirm_in(
    journal_of: &Journal,
    txn: &WriteTransaction,
    stream: u64,
    node: u64,
    pending: bool,
) -> Result<Response, JournalError> {
    let Some(mut seal) = journal_of.seal_for_write(txn, stream)? else {
        return Ok(Response::Refused("not_sealed".into()));
    };
    if pending {
        if let Some(mut next) = seal.pending.take()
            && confirm_member(&mut next, node)
        {
            seal = if all_confirmed(&next) {
                *next
            } else {
                SealRecord {
                    pending: Some(next),
                    ..seal
                }
            };
            journal_of.put_seal_in(txn, stream, &seal)?;
        }
    } else if confirm_member(&mut seal, node) {
        journal_of.put_seal_in(txn, stream, &seal)?;
        if all_confirmed(&seal) {
            journal_of.drop_records_in(txn, stream)?;
        }
    }
    Ok(Response::Confirmed)
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
            seal(3, &[1, 2, 3]),
            confirm(3, 2),
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
            Response::Sealed,
            Response::Confirmed,
        ] {
            let json = serde_json::to_string(&resp).unwrap();
            assert_eq!(serde_json::from_str::<Response>(&json).unwrap(), resp);
        }
    }

    fn started() -> Record {
        Record::InferenceStart {
            engine: "fake".into(),
            model_hash: "m".into(),
            tokenizer_hash: "t".into(),
            sampler: "{}".into(),
            rng_state: vec![1],
            prompt_tokens: vec![7],
        }
    }

    fn seal(stream: u64, members: &[u64]) -> Command {
        Command::Seal {
            stream,
            members: members.to_vec(),
            data: 2,
            parity: 1,
            len: 9,
            checksums: vec![[7; 32]; members.len()],
        }
    }

    fn confirm(stream: u64, node: u64) -> Command {
        Command::ConfirmFragment {
            stream,
            node,
            pending: false,
        }
    }

    fn confirm_pending(stream: u64, node: u64) -> Command {
        Command::ConfirmFragment {
            stream,
            node,
            pending: true,
        }
    }

    fn reseal(stream: u64, members: &[u64]) -> Command {
        Command::Reseal {
            stream,
            members: members.to_vec(),
            data: 2,
            parity: 1,
            len: 9,
            checksums: vec![[8; 32]; members.len()],
        }
    }

    /// Goal 1 sealed over 1,2,3 and confirmed by all three.
    fn confirmed_node() -> Node {
        let node = Node::new();
        node.goal(true);
        node.apply(&seal(1, &[1, 2, 3]));
        for who in [1, 2, 3] {
            node.apply(&confirm(1, who));
        }
        node
    }

    /// A node's state: a durable store and journal, applying commands the way the state machine
    /// does: a refusal leaves nothing behind.
    struct Node {
        _dir: crate::storage::TempDir,
        db: std::sync::Arc<redb::Database>,
        store: Store,
        journal: Journal,
    }

    impl Node {
        fn new() -> Self {
            let dir = crate::storage::TempDir::new();
            let db = crate::storage::open(&dir.path().join("db")).unwrap();
            Self {
                store: Store::durable(db.clone()).unwrap(),
                journal: Journal::new(db.clone()).unwrap(),
                db,
                _dir: dir,
            }
        }

        fn apply(&self, cmd: &Command) -> Response {
            let txn = self.db.begin_write().unwrap();
            let (answer, _) = apply_in(&self.store, &self.journal, &txn, cmd).unwrap();
            if !answer.is_refusal() {
                txn.commit().unwrap();
            }
            answer
        }

        /// Goal 1 with a start and an end record, or only a start when `ended` is false.
        fn goal(&self, ended: bool) {
            self.journal.append(1, 0, &started()).unwrap();
            if ended {
                self.journal.append(1, 1, &Record::Cancelled).unwrap();
            }
        }
    }

    #[test]
    fn a_live_or_unknown_stream_cannot_be_sealed() {
        let node = Node::new();
        node.goal(false);
        for stream in [1, 9] {
            assert_eq!(
                node.apply(&seal(stream, &[1, 2, 3])),
                Response::Refused("not_ended".into())
            );
            assert_eq!(node.journal.seal(stream), Ok(None));
        }
        assert_eq!(node.journal.ended_unsealed(), Ok(vec![]));
        assert_eq!(node.journal.exists(1), Ok(true));
        assert_eq!(node.journal.exists(9), Ok(false));
    }

    #[test]
    fn an_unfinished_seal_is_replaced_by_one_over_other_members() {
        let node = Node::new();
        node.goal(true);
        assert_eq!(node.journal.ended_unsealed(), Ok(vec![1]));
        assert_eq!(node.apply(&seal(1, &[1, 2, 3])), Response::Sealed);
        assert_eq!(node.apply(&confirm(1, 2)), Response::Confirmed);
        assert_eq!(node.apply(&seal(1, &[4, 5, 6])), Response::Sealed);
        let kept = node.journal.seal(1).unwrap().unwrap();
        assert_eq!(kept.members, [4, 5, 6]);
        assert_eq!(kept.confirmed, Vec::<u64>::new());
        assert_eq!(node.journal.ended_unsealed(), Ok(vec![]));
    }

    #[test]
    fn sealing_again_over_the_same_members_changes_nothing() {
        let node = Node::new();
        node.goal(true);
        node.apply(&seal(1, &[1, 2, 3]));
        node.apply(&confirm(1, 2));
        assert_eq!(node.apply(&seal(1, &[1, 2, 3])), Response::Sealed);
        assert_eq!(node.journal.seal(1).unwrap().unwrap().confirmed, [2]);
    }

    #[test]
    fn a_fully_confirmed_seal_is_never_replaced() {
        let node = Node::new();
        node.goal(true);
        node.apply(&seal(1, &[1, 2, 3]));
        for who in [1, 2, 3] {
            node.apply(&confirm(1, who));
        }
        assert_eq!(node.apply(&seal(1, &[4, 5, 6])), Response::Sealed);
        let kept = node.journal.seal(1).unwrap().unwrap();
        assert_eq!(kept.members, [1, 2, 3]);
        assert_eq!(kept.confirmed, [1, 2, 3]);
    }

    #[test]
    fn the_full_copy_is_dropped_only_by_the_last_confirmation() {
        let node = Node::new();
        node.goal(true);
        let records = node.journal.read(1).unwrap();
        node.apply(&seal(1, &[1, 2, 3]));
        assert_eq!(node.apply(&confirm(1, 1)), Response::Confirmed);
        assert_eq!(node.apply(&confirm(1, 3)), Response::Confirmed);
        assert_eq!(node.journal.read(1).unwrap(), records);
        assert_eq!(node.apply(&confirm(1, 2)), Response::Confirmed);
        assert!(node.journal.exists(1).unwrap());
        assert!(!node.journal.live(1).unwrap());
        let txn = node.journal.begin_read().unwrap();
        assert_eq!(crate::journal::rows_in(&txn, 1), Ok(vec![]));
        assert!(matches!(
            node.journal.read(1),
            Err(JournalError::Corrupt { .. })
        ));
    }

    #[test]
    fn duplicate_and_non_member_confirmations_change_nothing() {
        let node = Node::new();
        node.goal(true);
        node.apply(&seal(1, &[1, 2, 3]));
        for who in [1, 1, 9] {
            assert_eq!(node.apply(&confirm(1, who)), Response::Confirmed);
        }
        assert_eq!(node.journal.seal(1).unwrap().unwrap().confirmed, [1]);
        assert!(node.journal.read(1).is_ok());
    }

    #[test]
    fn a_malformed_seal_is_refused_and_the_full_copy_stays() {
        let node = Node::new();
        node.goal(true);
        let records = node.journal.read(1).unwrap();
        let bad = |members: &[u64], data, parity, sums: usize| Command::Seal {
            stream: 1,
            members: members.to_vec(),
            data,
            parity,
            len: 9,
            checksums: vec![[7; 32]; sums],
        };
        for cmd in [
            bad(&[1], 1, 0, 1),
            bad(&[1, 2], 1, 1, 2),
            bad(&[1, 2, 2], 2, 1, 3),
            bad(&[1, 2, 3], 1, 2, 3),
            bad(&[1, 2, 3], 3, 0, 3),
            bad(&[1, 2, 3], 2, 1, 2),
            bad(&[1, 2, 3], 2, 1, 4),
            bad(&[1, 2, 3, 4, 5], 2, 3, 5),
        ] {
            assert_eq!(node.apply(&cmd), Response::Refused("bad_seal".into()));
            assert_eq!(node.journal.seal(1), Ok(None));
            assert_eq!(
                node.apply(&confirm(1, 1)),
                Response::Refused("not_sealed".into())
            );
            assert_eq!(node.journal.read(1).unwrap(), records);
        }
        assert_eq!(node.apply(&seal(1, &[1, 2, 3])), Response::Sealed);
    }

    #[test]
    fn a_reseal_is_refused_unless_the_stream_is_fully_sealed_and_confirmed() {
        let node = Node::new();
        node.goal(true);
        let refused = |why: &str| Response::Refused(why.into());
        assert_eq!(node.apply(&reseal(1, &[1, 2, 3])), refused("not_sealed"));
        node.apply(&seal(1, &[1, 2, 3]));
        node.apply(&confirm(1, 1));
        assert_eq!(node.apply(&reseal(1, &[1, 2, 4])), refused("not_confirmed"));
        assert_eq!(node.journal.seal(1).unwrap().unwrap().pending, None);
        let done = confirmed_node();
        let bad = Command::Reseal {
            stream: 1,
            members: vec![1, 2],
            data: 1,
            parity: 1,
            len: 9,
            checksums: vec![[8; 32]; 2],
        };
        assert_eq!(done.apply(&bad), refused("bad_seal"));
        assert_eq!(done.journal.seal(1).unwrap().unwrap().pending, None);
    }

    #[test]
    fn a_reseal_waits_for_every_confirmation_before_it_replaces_the_seal() {
        let node = confirmed_node();
        let old = node.journal.seal(1).unwrap().unwrap();
        assert_eq!(node.apply(&reseal(1, &[1, 2, 4])), Response::Sealed);
        assert_eq!(node.apply(&reseal(1, &[1, 2, 4])), Response::Sealed);
        let seal_now = node.journal.seal(1).unwrap().unwrap();
        let pending = seal_now.pending.clone().unwrap();
        assert_eq!(
            (pending.members.clone(), pending.confirmed.len()),
            (vec![1, 2, 4], 0)
        );
        assert_eq!(
            SealRecord {
                pending: None,
                ..seal_now
            },
            old
        );
        for who in [1, 2, 1, 9] {
            assert_eq!(node.apply(&confirm_pending(1, who)), Response::Confirmed);
        }
        let mid = node.journal.seal(1).unwrap().unwrap();
        assert_eq!(mid.members, [1, 2, 3]);
        assert_eq!(mid.pending.unwrap().confirmed, [1, 2]);
        assert_eq!(node.apply(&confirm(1, 1)), Response::Confirmed);
        assert_eq!(
            node.journal
                .seal(1)
                .unwrap()
                .unwrap()
                .pending
                .unwrap()
                .confirmed,
            [1, 2]
        );
        assert_eq!(node.apply(&confirm_pending(1, 4)), Response::Confirmed);
        let now = node.journal.seal(1).unwrap().unwrap();
        assert_eq!(
            (now.members, now.confirmed, now.pending),
            (vec![1, 2, 4], vec![1, 2, 4], None)
        );
        assert_eq!(node.apply(&reseal(1, &[1, 2, 4])), Response::Sealed);
        assert_eq!(node.journal.seal(1).unwrap().unwrap().pending, None);
    }

    #[test]
    fn only_a_confirmed_seal_over_other_members_is_a_reseal_candidate() {
        let node = Node::new();
        node.goal(true);
        node.apply(&seal(1, &[1, 2, 3]));
        node.apply(&confirm(1, 1));
        let candidates = |voters: &[u64]| {
            let found = node.journal.reseal_candidates(voters).unwrap();
            found
                .into_iter()
                .map(|(stream, _)| stream)
                .collect::<Vec<_>>()
        };
        assert_eq!(candidates(&[1, 2, 4]), Vec::<u64>::new(), "not confirmed");
        node.apply(&confirm(1, 2));
        node.apply(&confirm(1, 3));
        assert_eq!(candidates(&[1, 2, 3]), Vec::<u64>::new(), "same members");
        assert_eq!(candidates(&[1, 2, 4]), [1]);
        node.apply(&reseal(1, &[1, 2, 4]));
        assert_eq!(candidates(&[1, 2, 4]), Vec::<u64>::new(), "already pending");
        assert_eq!(candidates(&[1, 2, 5]), [1], "pending to other members");
    }

    #[test]
    fn a_pending_confirmation_without_a_pending_seal_changes_nothing() {
        let node = confirmed_node();
        let before = node.journal.seal(1).unwrap();
        assert_eq!(node.apply(&confirm_pending(1, 1)), Response::Confirmed);
        assert_eq!(node.journal.seal(1).unwrap(), before);
    }

    #[test]
    fn a_confirmation_without_a_seal_is_refused() {
        let node = Node::new();
        node.goal(true);
        assert_eq!(
            node.apply(&confirm(1, 1)),
            Response::Refused("not_sealed".into())
        );
        assert_eq!(node.journal.seal(1), Ok(None));
    }

    #[test]
    fn rows_of_a_stream_verify_to_the_same_records_as_a_read() {
        let node = Node::new();
        node.goal(true);
        let txn = node.journal.begin_read().unwrap();
        let rows = crate::journal::rows_in(&txn, 1).unwrap();
        assert_eq!(
            node.journal.read_rows(1, &rows),
            Ok(vec![started(), Record::Cancelled])
        );
        assert!(matches!(
            node.journal.read_rows(1, &rows[..rows.len() - 1]),
            Err(JournalError::Corrupt { .. })
        ));
    }

    #[test]
    fn a_malformed_seal_row_is_refused_as_corrupt() {
        let node = Node::new();
        node.goal(true);
        let txn = node.db.begin_write().unwrap();
        txn.open_table(crate::journal::SEALS)
            .unwrap()
            .insert(1, b"not json".as_slice())
            .unwrap();
        txn.commit().unwrap();
        assert_eq!(
            node.apply(&confirm(1, 1)),
            Response::Refused("corrupt".into())
        );
        assert!(matches!(
            node.journal.seal(1),
            Err(JournalError::Corrupt { .. })
        ));
    }

    #[actix_web::test]
    async fn seal_commands_apply_identically_on_every_node_of_a_cluster() {
        let cluster = crate::raft::testing::Cluster::start(3, 3).await;
        let leader = cluster.nodes[cluster.leader().await].clone();
        let propose = |cmd: Command| {
            let leader = leader.clone();
            async move {
                tokio::task::spawn_blocking(move || {
                    std::thread::spawn(move || leader.propose_blocking(cmd))
                        .join()
                        .unwrap()
                })
                .await
                .unwrap()
                .unwrap()
            }
        };
        let id = match propose(Command::CreateGoal {
            priority: 0,
            budget: 4,
            start: started(),
        })
        .await
        {
            Ok(Response::GoalCreated(id)) => id,
            other => panic!("{other:?}"),
        };
        assert_eq!(
            propose(seal(id, &[1, 2, 3])).await,
            Ok(Response::Refused("not_ended".into()))
        );
        assert_eq!(
            propose(Command::CancelGoal { id }).await,
            Ok(Response::Cancelled)
        );
        assert_eq!(propose(seal(id, &[1, 2, 3])).await, Ok(Response::Sealed));
        for node in 1..=3 {
            assert_eq!(propose(confirm(id, node)).await, Ok(Response::Confirmed));
        }
        for (node, db) in cluster.nodes.iter().zip(&cluster.dbs) {
            node.read_barrier().await.unwrap();
            let journal = Journal::new(db.clone()).unwrap();
            let sealed = journal.seal(id).unwrap().unwrap();
            assert_eq!(sealed.confirmed, [1, 2, 3]);
            let txn = journal.begin_read().unwrap();
            assert_eq!(crate::journal::rows_in(&txn, id), Ok(vec![]));
            assert!(journal.exists(id).unwrap());
        }
        cluster.stop().await;
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
