//! Durable, checksummed journal of model inference streams.
//!
//! A stream holds everything needed to restore a model's thinking state on
//! another run: the exact prompt, the model and tokenizer hashes, the sampler
//! configuration, every generated token ID and the random generator state after
//! each batch. Restoring rebuilds that state from the journal, so no committed
//! token has to be generated again.
//!
//! Every record stores a BLAKE3 hash over its stream, sequence number, the
//! previous record's hash and its own body, which chains the records together.
//! `restore` verifies the whole chain. A per-stream head, written in the same
//! transaction as each record, also catches records missing from the end. The
//! chain detects corruption; it is not protection against a deliberate forgery.
//!
//! `append` returns only after the commit has been flushed to disk. `append_in`
//! writes inside a caller's transaction instead, and the caller commits. Both take the
//! sequence number the caller expects to write, so a writer working from a stale
//! view of a stream is refused instead of forking it.
use std::sync::Arc;

use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};

const RECORDS: TableDefinition<(u64, u64), &[u8]> = TableDefinition::new("journal_records");
const HEADS: TableDefinition<u64, &[u8]> = TableDefinition::new("journal_heads");

/// Version byte at the start of every stored record.
const FORMAT: u8 = 1;
const HASH_LEN: usize = 32;
// Stored record layout: version byte, previous hash, own hash, JSON body.
// Stored head layout: last sequence number, last hash, ended flag, checksum.

type Hash = [u8; HASH_LEN];

/// One entry of a stream.
///
/// `sampler` and `rng_state` are opaque to the journal because their contents
/// depend on the inference engine. They are not floats on purpose: a value must
/// come back bit-exact to continue generation from the same state.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Record {
    /// Always the first record of a stream, and only the first.
    InferenceStart {
        engine: String,
        model_hash: String,
        tokenizer_hash: String,
        sampler: String,
        rng_state: Vec<u8>,
        prompt_tokens: Vec<u32>,
    },
    /// A batch of generated token IDs and the generator state after the batch.
    Tokens {
        tokens: Vec<u32>,
        rng_state: Vec<u8>,
    },
    /// The stream is finished. Nothing can be appended after this record.
    InferenceEnd { reason: String },
}

/// A model's thinking state as rebuilt from a verified stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThinkingState {
    pub engine: String,
    pub model_hash: String,
    pub tokenizer_hash: String,
    pub sampler: String,
    pub prompt_tokens: Vec<u32>,
    /// Every committed generated token, in order.
    pub tokens: Vec<u32>,
    /// The generator state after the last committed token.
    pub rng_state: Vec<u8>,
    /// The sequence number the next `append` must use.
    pub next_seq: u64,
    /// The end reason, when the stream has finished.
    pub ended: Option<String>,
}

/// Errors returned by journal operations.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum JournalError {
    #[error("storage failure: {0}")]
    Storage(String),
    /// The caller's view of the stream is stale, or it skipped ahead.
    #[error("stream {stream}: expected to write seq {expected}, but the next seq is {actual}")]
    SeqConflict {
        stream: u64,
        expected: u64,
        actual: u64,
    },
    /// The caller's lease was taken over by a later claim.
    #[error("task {task}: fence {held} is stale, the current token is {current}")]
    Fenced { task: u64, held: u64, current: u64 },
    #[error("stream {stream} has ended")]
    Ended { stream: u64 },
    /// The record is not allowed at this position of the stream.
    #[error("stream {stream}: invalid record at seq {seq}: {reason}")]
    InvalidRecord {
        stream: u64,
        seq: u64,
        reason: &'static str,
    },
    /// Stored data failed verification. Nothing from this stream can be trusted
    /// past `seq` until it is repaired from another copy.
    #[error("stream {stream}: corrupt at seq {seq}: {reason}")]
    Corrupt {
        stream: u64,
        seq: u64,
        reason: String,
    },
}

pub(crate) fn storage<E: Into<redb::Error>>(e: E) -> JournalError {
    JournalError::Storage(e.into().to_string())
}

/// Shared handle to the journal tables of one database.
#[derive(Clone)]
pub struct Journal {
    db: Arc<Database>,
}

impl Journal {
    /// Use `db` for the journal, creating its tables when they do not exist.
    pub fn new(db: Arc<Database>) -> Result<Self, JournalError> {
        let txn = db.begin_write().map_err(storage)?;
        txn.open_table(RECORDS).map_err(storage)?;
        txn.open_table(HEADS).map_err(storage)?;
        txn.commit().map_err(storage)?;
        Ok(Self { db })
    }

    /// Append `record` as sequence number `expected_seq` and flush it to disk.
    ///
    /// A returned error means the record was not committed, except for a failure
    /// reported by the commit itself, whose outcome only a later `restore` can
    /// tell. On any error, call `restore` before writing to the stream again.
    pub fn append(
        &self,
        stream: u64,
        expected_seq: u64,
        record: &Record,
    ) -> Result<(), JournalError> {
        let txn = self.db.begin_write().map_err(storage)?;
        self.append_in(&txn, stream, expected_seq, record)?;
        txn.commit().map_err(storage)
    }

    /// Append `record` inside the caller's transaction, so it commits or rolls
    /// back together with the caller's other writes. Nothing is committed here.
    /// Later calls on the same transaction see the records written by earlier ones.
    pub fn append_in(
        &self,
        txn: &redb::WriteTransaction,
        stream: u64,
        expected_seq: u64,
        record: &Record,
    ) -> Result<(), JournalError> {
        {
            let mut heads = txn.open_table(HEADS).map_err(storage)?;
            let head = match heads.get(stream).map_err(storage)? {
                Some(raw) => Some(Head::decode(stream, raw.value())?),
                None => None,
            };
            let next_seq = head.as_ref().map_or(0, |h| h.seq + 1);
            if expected_seq != next_seq {
                return Err(JournalError::SeqConflict {
                    stream,
                    expected: expected_seq,
                    actual: next_seq,
                });
            }
            if head.as_ref().is_some_and(|h| h.ended) {
                return Err(JournalError::Ended { stream });
            }
            let is_start = matches!(record, Record::InferenceStart { .. });
            if is_start != (next_seq == 0) {
                return Err(JournalError::InvalidRecord {
                    stream,
                    seq: next_seq,
                    reason: "a stream has one inference_start, as its first record",
                });
            }
            let body = serde_json::to_vec(record)
                .map_err(|e| JournalError::Storage(format!("cannot encode record: {e}")))?;
            let prev = head.map_or([0; HASH_LEN], |h| h.hash);
            let hash = record_hash(stream, next_seq, &prev, &body);

            let stored = [&[FORMAT], &prev[..], &hash[..], &body[..]].concat();
            let mut records = txn.open_table(RECORDS).map_err(storage)?;
            records
                .insert((stream, next_seq), stored.as_slice())
                .map_err(storage)?;
            let new_head = Head {
                seq: next_seq,
                hash,
                ended: matches!(record, Record::InferenceEnd { .. }),
            };
            heads
                .insert(stream, new_head.encode(stream).as_slice())
                .map_err(storage)?;
        }
        Ok(())
    }

    /// Verify a whole stream and rebuild its thinking state, or `None` when the
    /// stream does not exist.
    pub fn restore(&self, stream: u64) -> Result<Option<ThinkingState>, JournalError> {
        let txn = self.db.begin_read().map_err(storage)?;
        let heads = txn.open_table(HEADS).map_err(storage)?;
        let records = txn.open_table(RECORDS).map_err(storage)?;
        let head = match heads.get(stream).map_err(storage)? {
            Some(raw) => Some(Head::decode(stream, raw.value())?),
            None => None,
        };

        let mut state: Option<ThinkingState> = None;
        let mut next_seq = 0;
        let mut prev = [0; HASH_LEN];
        for entry in records
            .range((stream, 0)..=(stream, u64::MAX))
            .map_err(storage)?
        {
            let (key, value) = entry.map_err(storage)?;
            let (_, seq) = key.value();
            if seq != next_seq {
                return Err(corrupt(stream, next_seq, "record is missing"));
            }
            let (hash, record) = decode_record(stream, seq, &prev, value.value())?;
            state = Some(apply(stream, seq, state, record)?);
            prev = hash;
            next_seq += 1;
        }

        check_head(stream, head.as_ref(), next_seq, &prev)?;
        Ok(state.map(|state| ThinkingState { next_seq, ..state }))
    }
}

/// Require the stored head to describe exactly the records that were read.
fn check_head(
    stream: u64,
    head: Option<&Head>,
    next_seq: u64,
    last_hash: &Hash,
) -> Result<(), JournalError> {
    let Some(head) = head else {
        return match next_seq {
            0 => Ok(()),
            _ => Err(corrupt(stream, 0, "records exist without a stream head")),
        };
    };
    if head.seq + 1 != next_seq {
        let reason = format!("head is at seq {} but records end before it", head.seq);
        return Err(corrupt(stream, next_seq, reason));
    }
    if head.hash != *last_hash {
        return Err(corrupt(
            stream,
            head.seq,
            "last record does not match the head",
        ));
    }
    Ok(())
}

struct Head {
    seq: u64,
    hash: Hash,
    ended: bool,
}

impl Head {
    fn encode(&self, stream: u64) -> Vec<u8> {
        let ended = [u8::from(self.ended)];
        let checksum = record_hash(stream, self.seq, &self.hash, &ended);
        [&self.seq.to_le_bytes()[..], &self.hash, &ended, &checksum].concat()
    }

    fn decode(stream: u64, raw: &[u8]) -> Result<Self, JournalError> {
        Self::parse(stream, raw).ok_or_else(|| corrupt(stream, 0, "stream head is corrupt"))
    }

    fn parse(stream: u64, raw: &[u8]) -> Option<Self> {
        let (seq, rest) = raw.split_first_chunk::<8>()?;
        let (hash, rest) = rest.split_first_chunk::<HASH_LEN>()?;
        let (ended, checksum) = rest.split_first()?;
        let seq = u64::from_le_bytes(*seq);
        (checksum == record_hash(stream, seq, hash, &[*ended])).then_some(Self {
            seq,
            hash: *hash,
            ended: *ended == 1,
        })
    }
}

fn corrupt(stream: u64, seq: u64, reason: impl Into<String>) -> JournalError {
    JournalError::Corrupt {
        stream,
        seq,
        reason: reason.into(),
    }
}

fn record_hash(stream: u64, seq: u64, prev: &Hash, body: &[u8]) -> Hash {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&[FORMAT]);
    hasher.update(&stream.to_le_bytes());
    hasher.update(&seq.to_le_bytes());
    hasher.update(prev);
    hasher.update(body);
    *hasher.finalize().as_bytes()
}

/// Verify one stored record against its position and return its hash and record.
fn decode_record(
    stream: u64,
    seq: u64,
    expected_prev: &Hash,
    raw: &[u8],
) -> Result<(Hash, Record), JournalError> {
    let short = || corrupt(stream, seq, "record is too short");
    let (format, rest) = raw.split_first().ok_or_else(short)?;
    let (prev, rest) = rest.split_at_checked(HASH_LEN).ok_or_else(short)?;
    let (hash, body) = rest.split_at_checked(HASH_LEN).ok_or_else(short)?;
    if *format != FORMAT {
        return Err(corrupt(stream, seq, format!("unknown format {format}")));
    }
    if prev != expected_prev {
        return Err(corrupt(
            stream,
            seq,
            "previous hash does not match the chain",
        ));
    }
    let computed = record_hash(stream, seq, expected_prev, body);
    if hash != computed {
        return Err(corrupt(stream, seq, "checksum mismatch"));
    }
    let record = serde_json::from_slice(body)
        .map_err(|e| corrupt(stream, seq, format!("cannot decode record: {e}")))?;
    Ok((computed, record))
}

/// Fold one verified record into the state rebuilt so far.
fn apply(
    stream: u64,
    seq: u64,
    state: Option<ThinkingState>,
    record: Record,
) -> Result<ThinkingState, JournalError> {
    match (state, record) {
        (
            None,
            Record::InferenceStart {
                engine,
                model_hash,
                tokenizer_hash,
                sampler,
                rng_state,
                prompt_tokens,
            },
        ) => Ok(ThinkingState {
            engine,
            model_hash,
            tokenizer_hash,
            sampler,
            prompt_tokens,
            tokens: Vec::new(),
            rng_state,
            next_seq: 0,
            ended: None,
        }),
        (Some(mut state), Record::Tokens { tokens, rng_state }) => {
            state.tokens.extend(tokens);
            state.rng_state = rng_state;
            Ok(state)
        }
        (Some(mut state), Record::InferenceEnd { reason }) => {
            state.ended = Some(reason);
            Ok(state)
        }
        _ => Err(corrupt(stream, seq, "record is not valid at this position")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{TempDir, open};
    use crate::store::Store;
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Command, Stdio};

    /// Set only in the child process of the crash test.
    const CRASH_DB_ENV: &str = "RS3_CRASH_TEST_DB";

    fn start() -> Record {
        Record::InferenceStart {
            engine: "fake".into(),
            model_hash: "model".into(),
            tokenizer_hash: "tokenizer".into(),
            sampler: r#"{"seed":42,"temperature":"0.7"}"#.into(),
            rng_state: vec![1],
            prompt_tokens: vec![10, 11, 12],
        }
    }

    fn tokens(tokens: &[u32], rng: u8) -> Record {
        Record::Tokens {
            tokens: tokens.to_vec(),
            rng_state: vec![rng],
        }
    }

    fn journal() -> (TempDir, Arc<Database>, Journal) {
        let dir = TempDir::new();
        let db = open(&dir.path().join("db")).unwrap();
        let journal = Journal::new(db.clone()).unwrap();
        (dir, db, journal)
    }

    /// A three-record stream 7: start, tokens, tokens.
    fn three_records(journal: &Journal) {
        journal.append(7, 0, &start()).unwrap();
        journal.append(7, 1, &tokens(&[1, 2], 2)).unwrap();
        journal.append(7, 2, &tokens(&[3], 3)).unwrap();
    }

    /// Change stored bytes directly, behind the journal's back.
    fn edit_record(db: &Database, seq: u64, edit: impl FnOnce(&mut Vec<u8>)) {
        let txn = db.begin_write().unwrap();
        {
            let mut records = txn.open_table(RECORDS).unwrap();
            let mut raw = records.get((7, seq)).unwrap().unwrap().value().to_vec();
            edit(&mut raw);
            records.insert((7, seq), raw.as_slice()).unwrap();
        }
        txn.commit().unwrap();
    }

    fn delete_record(db: &Database, seq: u64) {
        let txn = db.begin_write().unwrap();
        txn.open_table(RECORDS).unwrap().remove((7, seq)).unwrap();
        txn.commit().unwrap();
    }

    fn corrupt_seq(result: Result<Option<ThinkingState>, JournalError>) -> u64 {
        match result {
            Err(JournalError::Corrupt { seq, .. }) => seq,
            other => panic!("expected corruption, got {other:?}"),
        }
    }

    #[test]
    fn records_appended_in_one_transaction_commit_together() {
        let (_dir, db, journal) = journal();
        let txn = db.begin_write().unwrap();
        journal.append_in(&txn, 7, 0, &start()).unwrap();
        journal.append_in(&txn, 7, 1, &tokens(&[1], 1)).unwrap();
        txn.commit().unwrap();
        let state = journal.restore(7).unwrap().unwrap();
        assert_eq!(state.tokens, [1]);
        assert_eq!(state.next_seq, 2);
    }

    #[test]
    fn dropped_transaction_leaves_the_stream_unchanged() {
        let (_dir, db, journal) = journal();
        journal.append(7, 0, &start()).unwrap();
        let txn = db.begin_write().unwrap();
        journal.append_in(&txn, 7, 1, &tokens(&[1], 1)).unwrap();
        drop(txn);
        assert_eq!(journal.restore(7).unwrap().unwrap().next_seq, 1);
        journal.append(7, 1, &tokens(&[2], 2)).unwrap();
        assert_eq!(journal.restore(7).unwrap().unwrap().tokens, [2]);
    }

    #[test]
    fn second_append_in_a_transaction_sees_the_first() {
        let (_dir, db, journal) = journal();
        let txn = db.begin_write().unwrap();
        journal.append_in(&txn, 7, 0, &start()).unwrap();
        assert_eq!(
            journal.append_in(&txn, 7, 0, &start()),
            Err(JournalError::SeqConflict {
                stream: 7,
                expected: 0,
                actual: 1
            })
        );
    }

    #[test]
    fn missing_stream_restores_to_none() {
        let (_dir, _db, journal) = journal();
        assert_eq!(journal.restore(7), Ok(None));
    }

    #[test]
    fn restore_rebuilds_the_thinking_state() {
        let (_dir, _db, journal) = journal();
        three_records(&journal);
        let state = journal.restore(7).unwrap().unwrap();
        assert_eq!(state.engine, "fake");
        assert_eq!(state.model_hash, "model");
        assert_eq!(state.tokenizer_hash, "tokenizer");
        assert_eq!(state.sampler, r#"{"seed":42,"temperature":"0.7"}"#);
        assert_eq!(state.prompt_tokens, [10, 11, 12]);
        assert_eq!(state.tokens, [1, 2, 3]);
        assert_eq!(state.rng_state, [3]);
        assert_eq!(state.next_seq, 3);
        assert_eq!(state.ended, None);
    }

    #[test]
    fn stream_survives_reopening_the_database() {
        let dir = TempDir::new();
        let path = dir.path().join("db");
        {
            let journal = Journal::new(open(&path).unwrap()).unwrap();
            three_records(&journal);
        }
        let journal = Journal::new(open(&path).unwrap()).unwrap();
        let state = journal.restore(7).unwrap().unwrap();
        assert_eq!(state.tokens, [1, 2, 3]);
        journal.append(7, state.next_seq, &tokens(&[4], 4)).unwrap();
        assert_eq!(journal.restore(7).unwrap().unwrap().tokens, [1, 2, 3, 4]);
    }

    #[test]
    fn streams_are_independent() {
        let (_dir, _db, journal) = journal();
        three_records(&journal);
        journal.append(8, 0, &start()).unwrap();
        journal.append(8, 1, &tokens(&[99], 9)).unwrap();
        assert_eq!(journal.restore(7).unwrap().unwrap().tokens, [1, 2, 3]);
        assert_eq!(journal.restore(8).unwrap().unwrap().tokens, [99]);
    }

    #[test]
    fn stale_writer_is_refused_and_changes_nothing() {
        let (_dir, _db, journal) = journal();
        three_records(&journal);
        let conflict = |expected| JournalError::SeqConflict {
            stream: 7,
            expected,
            actual: 3,
        };
        assert_eq!(journal.append(7, 2, &tokens(&[9], 9)), Err(conflict(2)));
        assert_eq!(journal.append(7, 4, &tokens(&[9], 9)), Err(conflict(4)));
        assert_eq!(journal.restore(7).unwrap().unwrap().tokens, [1, 2, 3]);
    }

    #[test]
    fn stream_must_begin_with_exactly_one_start() {
        let (_dir, _db, journal) = journal();
        let bad = |seq| {
            Err(JournalError::InvalidRecord {
                stream: 7,
                seq,
                reason: "a stream has one inference_start, as its first record",
            })
        };
        assert_eq!(journal.append(7, 0, &tokens(&[1], 1)), bad(0));
        assert_eq!(journal.restore(7), Ok(None));
        journal.append(7, 0, &start()).unwrap();
        assert_eq!(journal.append(7, 1, &start()), bad(1));
    }

    #[test]
    fn ended_stream_refuses_more_records() {
        let (_dir, _db, journal) = journal();
        three_records(&journal);
        let end = Record::InferenceEnd {
            reason: "stop".into(),
        };
        journal.append(7, 3, &end).unwrap();
        assert_eq!(
            journal.append(7, 4, &tokens(&[9], 9)),
            Err(JournalError::Ended { stream: 7 })
        );
        let state = journal.restore(7).unwrap().unwrap();
        assert_eq!(state.ended.as_deref(), Some("stop"));
        assert_eq!(state.next_seq, 4);
    }

    #[test]
    fn altered_token_is_detected_even_when_the_record_still_parses() {
        let (_dir, db, journal) = journal();
        three_records(&journal);
        edit_record(&db, 1, |raw| {
            let at = raw.windows(5).position(|w| w == b"[1,2]").unwrap();
            raw[at + 3] = b'9';
        });
        assert_eq!(corrupt_seq(journal.restore(7)), 1);
    }

    #[test]
    fn flipped_stored_hash_is_detected() {
        let (_dir, db, journal) = journal();
        three_records(&journal);
        edit_record(&db, 2, |raw| raw[1 + HASH_LEN] ^= 1);
        assert_eq!(corrupt_seq(journal.restore(7)), 2);
    }

    #[test]
    fn flipped_previous_hash_is_detected() {
        let (_dir, db, journal) = journal();
        three_records(&journal);
        edit_record(&db, 2, |raw| raw[1] ^= 1);
        assert_eq!(corrupt_seq(journal.restore(7)), 2);
    }

    #[test]
    fn unknown_format_is_detected() {
        let (_dir, db, journal) = journal();
        three_records(&journal);
        edit_record(&db, 0, |raw| raw[0] = FORMAT + 1);
        assert_eq!(corrupt_seq(journal.restore(7)), 0);
    }

    #[test]
    fn truncated_records_are_detected_at_any_length() {
        let (_dir, db, journal) = journal();
        three_records(&journal);
        for len in [64, 33, 32, 1, 0] {
            edit_record(&db, 1, |raw| raw.truncate(len));
            assert_eq!(corrupt_seq(journal.restore(7)), 1, "length {len}");
        }
    }

    #[test]
    fn missing_middle_record_is_detected() {
        let (_dir, db, journal) = journal();
        three_records(&journal);
        delete_record(&db, 1);
        assert_eq!(corrupt_seq(journal.restore(7)), 1);
    }

    #[test]
    fn missing_last_record_is_detected() {
        let (_dir, db, journal) = journal();
        three_records(&journal);
        delete_record(&db, 2);
        assert_eq!(corrupt_seq(journal.restore(7)), 2);
    }

    #[test]
    fn any_corrupted_head_byte_stops_reads_and_writes() {
        let (_dir, db, journal) = journal();
        three_records(&journal);
        journal
            .append(
                7,
                3,
                &Record::InferenceEnd {
                    reason: "stop".into(),
                },
            )
            .unwrap();
        let edit_head = |edit: &dyn Fn(&mut Vec<u8>)| {
            let txn = db.begin_write().unwrap();
            {
                let mut heads = txn.open_table(HEADS).unwrap();
                let mut raw = heads.get(7).unwrap().unwrap().value().to_vec();
                edit(&mut raw);
                heads.insert(7, raw.as_slice()).unwrap();
            }
            txn.commit().unwrap();
        };
        let len = db
            .begin_read()
            .unwrap()
            .open_table(HEADS)
            .unwrap()
            .get(7)
            .unwrap()
            .unwrap()
            .value()
            .len();
        for at in 0..len {
            edit_head(&|raw| raw[at] ^= 1);
            let read = journal.restore(7);
            assert!(
                matches!(read, Err(JournalError::Corrupt { .. })),
                "byte {at}: {read:?}"
            );
            let write = journal.append(7, 4, &tokens(&[9], 9));
            assert!(
                matches!(write, Err(JournalError::Corrupt { .. })),
                "byte {at}: {write:?}"
            );
            edit_head(&|raw| raw[at] ^= 1);
        }
    }

    #[test]
    fn missing_head_is_detected() {
        let (_dir, db, journal) = journal();
        three_records(&journal);
        let txn = db.begin_write().unwrap();
        txn.open_table(HEADS).unwrap().remove(7).unwrap();
        txn.commit().unwrap();
        assert_eq!(corrupt_seq(journal.restore(7)), 0);
    }

    /// The writer the crash test kills. It does nothing in a normal test run.
    ///
    /// It continues the stream from what is on disk, and reports each token and
    /// store write only after they are committed.
    #[test]
    fn crash_test_writer() {
        let Ok(path) = std::env::var(CRASH_DB_ENV) else {
            return;
        };
        let db = open(path.as_ref()).unwrap();
        let journal = Journal::new(db.clone()).unwrap();
        let store = Store::durable(db).unwrap();
        let state = match journal.restore(1).unwrap() {
            Some(state) => state,
            None => {
                journal.append(1, 0, &start()).unwrap();
                journal.restore(1).unwrap().unwrap()
            }
        };
        let mut seq = state.next_seq;
        let mut token = u32::try_from(state.tokens.len()).unwrap();
        let mut out = std::io::stdout();
        loop {
            journal.append(1, seq, &tokens(&[token], 0)).unwrap();
            store
                .insert_all([("last".to_owned(), token.to_string())])
                .unwrap();
            writeln!(out, "ACK {token}").unwrap();
            out.flush().unwrap();
            seq += 1;
            token += 1;
        }
    }

    /// `kill -9` a writer three times, and require that every acknowledged token
    /// and store write survives and that the next run continues without a gap.
    ///
    /// This covers a process crash only. The operating system still flushes its
    /// page cache afterwards, so power loss needs the VM tests.
    #[test]
    fn acknowledged_work_survives_kill_9_and_continues() {
        let dir = TempDir::new();
        let path = dir.path().join("db");
        let mut acked_total = None;
        for _ in 0..3 {
            let mut child = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "journal::tests::crash_test_writer",
                    "--nocapture",
                    "--test-threads=1",
                ])
                .env(CRASH_DB_ENV, &path)
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
            let mut seen = 0;
            let mut acked = acked_total;
            while seen < 25 {
                let line = lines.next().expect("writer stopped early").unwrap();
                // libtest may print its own text on the same line first.
                if let Some((_, n)) = line.split_once("ACK ") {
                    acked = Some(n.trim().parse::<u32>().unwrap());
                    seen += 1;
                }
            }
            child.kill().unwrap();
            for line in lines {
                if let Some((_, n)) = line.unwrap().split_once("ACK ") {
                    // A partial last line is an ack the test cannot read in full.
                    if let Ok(n) = n.trim().parse::<u32>() {
                        acked = Some(n);
                    }
                }
            }
            child.wait().unwrap();
            let acked = acked.unwrap();
            acked_total = Some(acked);

            let db = open(&path).unwrap();
            let state = Journal::new(db.clone())
                .unwrap()
                .restore(1)
                .unwrap()
                .unwrap();
            let expected: Vec<u32> = (0..u32::try_from(state.tokens.len()).unwrap()).collect();
            assert_eq!(state.tokens, expected, "tokens have a gap or a duplicate");
            let durable_tokens = u32::try_from(state.tokens.len()).unwrap();
            assert!(
                durable_tokens > acked,
                "acknowledged token {acked} was lost; {durable_tokens} survived"
            );
            let last: u32 = Store::durable(db)
                .unwrap()
                .get("last")
                .unwrap()
                .parse()
                .unwrap();
            assert!(
                last >= acked,
                "acknowledged store write {acked} was lost; found {last}"
            );
        }
    }
}
