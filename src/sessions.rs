//! Sessions: ordered turns, each backed by one goal: a generation goal, or for a `/run <key>`
//! prompt a script goal that runs the script stored under `key`, read when the turn is applied.
//!
//! A session is a row holding the JSON list of its turns. Starting turn `n` is decided in one
//! write transaction, so a resent turn finds its goal and never creates a second one. A turn's
//! seed comes from its session and number only, and its goal sees only its own prompt, so a turn
//! produces the same output wherever and however often it is started.
use redb::{ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};

use crate::goals;
use crate::journal::{Journal, JournalError, Record, dump_table, storage};
use crate::store::Store;

pub(crate) const SESSIONS: TableDefinition<u64, &[u8]> = TableDefinition::new("sessions");

/// Tokens each turn's goal generates.
pub const TURN_BUDGET: u64 = 1000;

/// One turn of a session: the prompt and the goal that answers it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Turn {
    pub prompt: String,
    pub goal: u64,
}

/// What starting a turn did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnOutcome {
    /// A goal was created for the turn.
    New(u64),
    /// The turn was started before with this prompt; this is its goal and nothing was written.
    Existing(u64),
    /// The turn skips ahead, or was started with another prompt.
    Conflict,
    /// There is no such session.
    NotFound,
    /// A `/run` prompt names a key that holds no script; nothing was written.
    UnknownScript,
}

fn decode(bytes: &[u8]) -> Result<Vec<Turn>, JournalError> {
    serde_json::from_slice(bytes).map_err(|e| JournalError::Storage(format!("session row: {e}")))
}

fn encode(turns: &[Turn]) -> Result<Vec<u8>, JournalError> {
    serde_json::to_vec(turns).map_err(|e| JournalError::Storage(format!("session row: {e}")))
}

/// The seed of turn `turn` of `session`.
fn seed(session: u64, turn: u64) -> u64 {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&session.to_le_bytes());
    hasher.update(&turn.to_le_bytes());
    let mut head = [0u8; 8];
    head.copy_from_slice(&hasher.finalize().as_bytes()[..8]);
    u64::from_le_bytes(head)
}

/// Add an empty session in `txn` under the next free id.
pub(crate) fn create_in(txn: &redb::WriteTransaction) -> Result<u64, JournalError> {
    let mut table = txn.open_table(SESSIONS).map_err(storage)?;
    let id = table
        .last()
        .map_err(storage)?
        .map_or(1, |(key, _)| key.value() + 1);
    table.insert(id, encode(&[])?.as_slice()).map_err(storage)?;
    Ok(id)
}

/// Every session row as of `txn`, raw as stored.
pub(crate) fn dump_in(txn: &redb::ReadTransaction) -> Result<Vec<(u64, Vec<u8>)>, JournalError> {
    dump_table(txn, SESSIONS, |k, v| (k, v.to_vec()))
}

/// Make the sessions table in `txn` hold exactly `rows`.
pub(crate) fn replace_in(
    txn: &redb::WriteTransaction,
    rows: &[(u64, Vec<u8>)],
) -> Result<(), JournalError> {
    txn.delete_table(SESSIONS).map_err(storage)?;
    let mut table = txn.open_table(SESSIONS).map_err(storage)?;
    for (id, row) in rows {
        table.insert(*id, row.as_slice()).map_err(storage)?;
    }
    Ok(())
}

/// The turns of `session` as of `txn`, or `None` when there is no such session.
pub(crate) fn read_in(
    txn: &redb::ReadTransaction,
    session: u64,
) -> Result<Option<Vec<Turn>>, JournalError> {
    let table = txn.open_table(SESSIONS).map_err(storage)?;
    let row = table.get(session).map_err(storage)?;
    row.map(|bytes| decode(bytes.value())).transpose()
}

/// The first record of a turn's goal: the stored script for `/run <key>`, else generation of
/// `prompt`. `None` when the key holds no script.
fn start_record(
    txn: &redb::WriteTransaction,
    seed: u64,
    prompt: &str,
) -> Result<Option<Record>, JournalError> {
    let Some(rest) = prompt
        .strip_prefix("/run")
        .filter(|rest| rest.is_empty() || rest.starts_with(' '))
    else {
        return Ok(Some(goals::inference_start(seed, prompt)));
    };
    let name = rest.trim();
    let source = Store::get_in(txn, name).map_err(|e| JournalError::Storage(e.to_string()))?;
    Ok(source.map(|source| Record::ScriptStart {
        name: name.to_owned(),
        source,
    }))
}

/// Start turn `turn` of `session` with `prompt` in `txn`. Only `New` wrote anything.
pub(crate) fn start_turn_in(
    txn: &redb::WriteTransaction,
    journal: &Journal,
    session: u64,
    turn: u64,
    prompt: String,
) -> Result<TurnOutcome, JournalError> {
    let mut table = txn.open_table(SESSIONS).map_err(storage)?;
    let row = table.get(session).map_err(storage)?;
    let Some(mut turns) = row.map(|bytes| decode(bytes.value())).transpose()? else {
        return Ok(TurnOutcome::NotFound);
    };
    let len = turns.len() as u64;
    if turn < len {
        let known = &turns[turn as usize];
        return Ok(if known.prompt == prompt {
            TurnOutcome::Existing(known.goal)
        } else {
            TurnOutcome::Conflict
        });
    }
    if turn > len {
        return Ok(TurnOutcome::Conflict);
    }
    let Some(start) = start_record(txn, seed(session, turn), &prompt)? else {
        return Ok(TurnOutcome::UnknownScript);
    };
    let goal = goals::create_in(txn, journal, 0, TURN_BUDGET, &start)?;
    turns.push(Turn { prompt, goal });
    table
        .insert(session, encode(&turns)?.as_slice())
        .map_err(storage)?;
    Ok(TurnOutcome::New(goal))
}

#[cfg(test)]
mod tests {
    use std::num::{NonZeroU32, NonZeroUsize};
    use std::time::{Duration, Instant};

    use super::*;
    use crate::goals::Goals;
    use crate::storage::{TempDir, open};

    #[test]
    fn a_resent_turn_generates_its_budget_once() {
        let dir = TempDir::new();
        let db = open(&dir.path().join("db")).unwrap();
        let goals = Goals::open(db, NonZeroU32::new(1_000_000).unwrap()).unwrap();
        goals.start_workers(NonZeroUsize::new(2).unwrap());
        let session = goals.create_session().unwrap();

        let first = goals.start_turn(session, 0, "hi".into()).unwrap();
        let second = goals.start_turn(session, 0, "hi".into()).unwrap();
        assert_eq!(first, TurnOutcome::New(1));
        assert_eq!(second, TurnOutcome::Existing(1));

        let deadline = Instant::now() + Duration::from_secs(30);
        while goals.status(1).unwrap().unwrap().status != "done" {
            assert!(Instant::now() < deadline, "the goal never finished");
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(goals.generated_tokens(), TURN_BUDGET);
    }
}
